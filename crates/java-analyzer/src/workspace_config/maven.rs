use std::{collections::BTreeMap, path::Path};

use hir::{normalize_rel_path, ModuleDependency, Release, RootScope};

use super::{
    parse_version_string,
    xml::{expand_props, extract_xml_tag_text, remove_xml_blocks, strip_xml_comments},
    DetectedVersionSpec, ManifestMetadata,
};

#[derive(Default)]
pub(super) struct MavenPomMetadata {
    pub(super) release: Option<Release>,
    pub(super) test_release: Option<Release>,
    pub(super) preview: bool,
    pub(super) extra_source_dirs: Vec<(String, RootScope)>,
    pub(super) dependencies: Vec<ModuleDependency>,
    pub(super) properties: BTreeMap<String, String>,
    pub(super) managed_versions: BTreeMap<(String, String), String>,
}

pub(super) fn introspect_maven_pom(
    content: &str,
    path: &Path,
    version_specs: &mut Vec<DetectedVersionSpec>,
    meta: &mut ManifestMetadata,
) {
    let empty_props = BTreeMap::new();
    let empty_managed = BTreeMap::new();
    let pom = parse_maven_pom_metadata(content, &empty_props, &empty_managed);

    if let Some(release) = pom.release {
        version_specs.push(DetectedVersionSpec {
            source: "pom.xml".to_string(),
            path: path.to_path_buf(),
            release,
            preview: pom.preview,
        });
    }

    meta.extra_source_dirs.extend(pom.extra_source_dirs);
    for dep in pom.dependencies {
        if !meta.dependencies.contains(&dep) {
            meta.dependencies.push(dep);
        }
    }
    meta.maven_properties = pom.properties;
    meta.maven_managed_versions = pom.managed_versions;
    meta.maven_test_release = pom.test_release;
    meta.maven_preview = pom.preview;
}

pub(super) fn parse_maven_pom_metadata(
    content: &str,
    inherited_props: &BTreeMap<String, String>,
    inherited_managed: &BTreeMap<(String, String), String>,
) -> MavenPomMetadata {
    let cleaned = strip_xml_comments(content);
    let mut props = inherited_props.clone();
    props.extend(parse_maven_properties(&cleaned));

    let (without_profiles, profile_blocks) = remove_xml_blocks(&cleaned, "profiles");
    let preview = without_profiles.contains("<enablePreview>true</enablePreview>")
        || without_profiles.contains("--enable-preview");

    let mut release = None;
    for tag in [
        "maven.compiler.release",
        "maven.compiler.source",
        "java.version",
        "release",
    ] {
        if let Some(raw_val) = extract_xml_tag_text(&without_profiles, tag) {
            let val = expand_props(&raw_val, &props);
            if let Some(rel) = parse_version_string(&val) {
                release = Some(rel);
                break;
            }
        }
    }

    let mut test_release = None;
    for tag in [
        "maven.compiler.testRelease",
        "maven.compiler.testSource",
        "testRelease",
    ] {
        if let Some(raw_val) = extract_xml_tag_text(&without_profiles, tag) {
            let val = expand_props(&raw_val, &props);
            if let Some(rel) = parse_version_string(&val) {
                test_release = Some(rel);
                break;
            }
        }
    }
    if test_release.is_none() {
        test_release = extract_test_release_from_executions(&without_profiles, &props);
    }
    if test_release.is_none() {
        for profile_block in &profile_blocks {
            if let Some(rel) = extract_test_release_from_executions(profile_block, &props) {
                test_release = Some(test_release.map_or(rel, |cur| cur.max(rel)));
            }
        }
    }

    // Extract <sourceDirectory> and <testSourceDirectory> from <build> BEFORE stripping <build>.
    let mut extra_source_dirs = Vec::new();
    if let Some(raw_src_dir) = extract_xml_tag_text(&without_profiles, "sourceDirectory") {
        let src_dir = expand_props(&raw_src_dir, &props);
        if !src_dir.contains("${") {
            extra_source_dirs.push((normalize_rel_path(&src_dir), RootScope::Main));
        }
    }
    if let Some(raw_test_dir) = extract_xml_tag_text(&without_profiles, "testSourceDirectory") {
        let test_dir = expand_props(&raw_test_dir, &props);
        if !test_dir.contains("${") {
            extra_source_dirs.push((normalize_rel_path(&test_dir), RootScope::Test));
        }
    }

    // Strip <build>, <reporting>, <plugins>, and <pluginManagement> before scanning <dependency>
    // blocks so plugin dependencies never leak into project compile/test dependencies.
    let mut deps_xml = without_profiles;
    for container in ["build", "reporting", "plugins", "pluginManagement"] {
        deps_xml = remove_xml_blocks(&deps_xml, container).0;
    }

    // Parse <dependencyManagement> separately so managed-only entries are not emitted as direct
    // dependencies and can provide version fallbacks when direct <dependency> entries omit <version>.
    let (without_dep_mgmt, dep_mgmt_blocks) = remove_xml_blocks(&deps_xml, "dependencyManagement");
    let mut own_managed: BTreeMap<(String, String), String> = BTreeMap::new();
    for mgmt_block in &dep_mgmt_blocks {
        let mut rest = mgmt_block.as_str();
        while let Some(start) = rest.find("<dependency>") {
            let after = &rest[start + "<dependency>".len()..];
            let Some(end) = after.find("</dependency>") else {
                break;
            };
            let block = &after[..end];
            rest = &after[end + "</dependency>".len()..];
            let (block_clean, _) = remove_xml_blocks(block, "exclusions");

            let group = extract_xml_tag_text(&block_clean, "groupId")
                .map(|s| expand_props(&s, &props).trim().to_string())
                .filter(|s| !s.is_empty());
            let artifact = extract_xml_tag_text(&block_clean, "artifactId")
                .map(|s| expand_props(&s, &props).trim().to_string())
                .filter(|s| !s.is_empty());
            let version = extract_xml_tag_text(&block_clean, "version")
                .map(|s| expand_props(&s, &props).trim().to_string())
                .filter(|s| !s.is_empty());

            if let (Some(g), Some(a), Some(v)) = (group, artifact, version) {
                own_managed.entry((g, a)).or_insert(v);
            }
        }
    }
    let mut managed_versions = inherited_managed.clone();
    managed_versions.extend(own_managed);

    // Extract compile-time <dependency> blocks (Main: default/compile/provided; Test: test; skip runtime)
    let mut dependencies = Vec::new();
    let mut rest = without_dep_mgmt.as_str();
    while let Some(start) = rest.find("<dependency>") {
        let after = &rest[start + "<dependency>".len()..];
        let Some(end) = after.find("</dependency>") else {
            break;
        };
        let block = &after[..end];
        rest = &after[end + "</dependency>".len()..];
        let (block_clean, _) = remove_xml_blocks(block, "exclusions");

        let group = extract_xml_tag_text(&block_clean, "groupId")
            .map(|s| expand_props(&s, &props).trim().to_string())
            .filter(|s| !s.is_empty());
        let artifact = extract_xml_tag_text(&block_clean, "artifactId")
            .map(|s| expand_props(&s, &props).trim().to_string())
            .filter(|s| !s.is_empty());
        let explicit_version = extract_xml_tag_text(&block_clean, "version")
            .map(|s| expand_props(&s, &props).trim().to_string())
            .filter(|s| !s.is_empty());
        let classifier = extract_xml_tag_text(&block_clean, "classifier")
            .map(|s| expand_props(&s, &props).trim().to_string())
            .filter(|s| !s.is_empty());
        let scope_str = extract_xml_tag_text(&block_clean, "scope").unwrap_or_default();

        if let (Some(g), Some(a)) = (group, artifact) {
            let version = explicit_version
                .or_else(|| managed_versions.get(&(g.clone(), a.clone())).cloned())
                .unwrap_or_else(|| "*".to_string());
            let scope = match scope_str.trim() {
                "" | "compile" | "provided" => Some(RootScope::Main),
                "test" => Some(RootScope::Test),
                // Ignore runtime/import/system deployment-only scopes
                _ => None,
            };
            if let Some(scope) = scope {
                let dep = ModuleDependency::Maven {
                    group: g,
                    artifact: a,
                    version,
                    classifier,
                    scope,
                };
                if !dependencies.contains(&dep) {
                    dependencies.push(dep);
                }
            }
        }
    }

    MavenPomMetadata {
        release,
        test_release,
        preview,
        extra_source_dirs,
        dependencies,
        properties: props,
        managed_versions,
    }
}

fn extract_test_release_from_executions(
    xml: &str,
    props: &BTreeMap<String, String>,
) -> Option<Release> {
    let mut max_release: Option<Release> = None;
    let (_, exec_blocks) = remove_xml_blocks(xml, "execution");
    for block in exec_blocks {
        let lower = block.to_ascii_lowercase();
        if !lower.contains("testcompile") && !lower.contains("test-compile") {
            continue;
        }
        for tag in ["testRelease", "release"] {
            if let Some(raw_val) = extract_xml_tag_text(&block, tag) {
                let val = expand_props(&raw_val, props);
                if let Some(rel) = parse_version_string(&val) {
                    max_release = Some(max_release.map_or(rel, |cur| cur.max(rel)));
                    break;
                }
            }
        }
    }
    max_release
}

fn extract_maven_project_coords(content: &str) -> (Option<String>, Option<String>, Option<String>) {
    let (without_parent, parent_blocks) = remove_xml_blocks(content, "parent");
    let mut top_only = without_parent;
    for container in [
        "dependencies",
        "dependencyManagement",
        "build",
        "profiles",
        "reporting",
        "properties",
    ] {
        top_only = remove_xml_blocks(&top_only, container).0;
    }
    let group_id = extract_xml_tag_text(&top_only, "groupId")
        .or_else(|| {
            parent_blocks
                .first()
                .and_then(|p| extract_xml_tag_text(p, "groupId"))
        })
        .filter(|v| !v.is_empty());
    let artifact_id = extract_xml_tag_text(&top_only, "artifactId").filter(|v| !v.is_empty());
    let version = extract_xml_tag_text(&top_only, "version")
        .or_else(|| {
            parent_blocks
                .first()
                .and_then(|p| extract_xml_tag_text(p, "version"))
        })
        .filter(|v| !v.is_empty());
    (group_id, artifact_id, version)
}

pub(super) fn parse_maven_properties(content: &str) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    let (proj_group, proj_artifact, proj_ver) = extract_maven_project_coords(content);
    if let Some(g) = proj_group {
        props.insert("project.groupId".to_string(), g);
    }
    if let Some(a) = proj_artifact {
        props.insert("project.artifactId".to_string(), a);
    }
    if let Some(v) = proj_ver {
        props.insert("project.version".to_string(), v);
    }

    let (without_profiles, _) = remove_xml_blocks(content, "profiles");
    if let Some(parent_block) = extract_xml_tag_text(&without_profiles, "parent") {
        for (xml_tag, prop_key) in [
            ("groupId", "project.parent.groupId"),
            ("artifactId", "project.parent.artifactId"),
            ("version", "project.parent.version"),
        ] {
            if let Some(val) = extract_xml_tag_text(&parent_block, xml_tag).filter(|v| !v.is_empty())
            {
                props.insert(prop_key.to_string(), val);
            }
        }
    }
    let props_source = if extract_xml_tag_text(&without_profiles, "properties").is_some() {
        without_profiles.as_str()
    } else {
        content
    };
    if let Some(props_block) = extract_xml_tag_text(props_source, "properties") {
        let mut rest = props_block.as_str();
        while let Some(open_lt) = rest.find('<') {
            let after_lt = &rest[open_lt + 1..];
            if after_lt.starts_with('/') || after_lt.starts_with('!') || after_lt.starts_with('?')
            {
                if let Some(gt) = after_lt.find('>') {
                    rest = &after_lt[gt + 1..];
                } else {
                    break;
                }
                continue;
            }
            let Some(gt) = after_lt.find('>') else {
                break;
            };
            let tag_header = &after_lt[..gt];
            if tag_header.ends_with('/') {
                let tag_name = tag_header
                    .trim_end_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                if !tag_name.is_empty() {
                    props.insert(tag_name.to_string(), String::new());
                }
                rest = &after_lt[gt + 1..];
                continue;
            }
            let tag_name = tag_header.split_whitespace().next().unwrap_or("");
            if tag_name.is_empty() {
                rest = &after_lt[gt + 1..];
                continue;
            }
            let close_tag = format!("</{tag_name}>");
            let after_open = &after_lt[gt + 1..];
            if let Some(close_idx) = after_open.find(&close_tag) {
                let val = after_open[..close_idx].trim().to_string();
                props.insert(tag_name.to_string(), val);
                rest = &after_open[close_idx + close_tag.len()..];
            } else {
                rest = after_open;
            }
        }
    }

    props
}
