use std::{collections::BTreeMap, path::Path};

use ide::{normalize_rel_path, ModuleDependency, Release, RootScope};
use roxmltree::{Document, Node};

use super::{
    parse_version_string,
    xml::{child_text, direct_child_text, expand_props, has_any_ancestor, is_tag, parse_xml},
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
    let Some(doc) = parse_xml(content) else {
        return MavenPomMetadata::default();
    };

    let mut props = inherited_props.clone();
    props.extend(parse_maven_properties_doc(&doc));

    let preview = doc.descendants().any(|n| {
        if has_any_ancestor(n, &["profiles"]) {
            return false;
        }
        (is_tag(n, "enablePreview") && child_text(n) == "true")
            || (n.is_text() && n.text().is_some_and(|t| t.contains("--enable-preview")))
    });

    let release = find_release_by_tags(
        &doc,
        &[
            "maven.compiler.release",
            "maven.compiler.source",
            "java.version",
            "release",
        ],
        &props,
    );

    let test_release = find_release_by_tags(
        &doc,
        &[
            "maven.compiler.testRelease",
            "maven.compiler.testSource",
            "testRelease",
        ],
        &props,
    )
    .or_else(|| extract_test_release_from_executions(&doc, false, &props))
    .or_else(|| extract_test_release_from_executions(&doc, true, &props));

    let mut extra_source_dirs = Vec::new();
    for (tag, scope) in [
        ("sourceDirectory", RootScope::Main),
        ("testSourceDirectory", RootScope::Test),
    ] {
        if let Some(node) = doc
            .descendants()
            .find(|n| is_tag(*n, tag) && !has_any_ancestor(*n, &["profiles"]))
        {
            let dir = expand_props(&child_text(node), &props);
            if !dir.is_empty() && !dir.contains("${") {
                extra_source_dirs.push((normalize_rel_path(&dir), scope));
            }
        }
    }

    // Parse <dependencyManagement> separately so managed-only entries are not emitted as direct
    // dependencies and can provide version fallbacks when direct <dependency> entries omit <version>.
    let mut own_managed: BTreeMap<(String, String), String> = BTreeMap::new();
    for dep_node in doc.descendants().filter(|n| {
        is_tag(*n, "dependency")
            && has_any_ancestor(*n, &["dependencyManagement"])
            && !has_any_ancestor(
                *n,
                &[
                    "profiles",
                    "build",
                    "reporting",
                    "plugins",
                    "pluginManagement",
                    "exclusions",
                ],
            )
    }) {
        let group = dependency_child_prop(dep_node, "groupId", &props);
        let artifact = dependency_child_prop(dep_node, "artifactId", &props);
        let version = dependency_child_prop(dep_node, "version", &props);

        if let (Some(g), Some(a), Some(v)) = (group, artifact, version) {
            own_managed.entry((g, a)).or_insert(v);
        }
    }
    let mut managed_versions = inherited_managed.clone();
    managed_versions.extend(own_managed);

    // Extract compile-time <dependency> blocks (Main: default/compile/provided; Test: test; skip runtime)
    let mut dependencies = Vec::new();
    for dep_node in doc.descendants().filter(|n| {
        is_tag(*n, "dependency")
            && !has_any_ancestor(
                *n,
                &[
                    "profiles",
                    "build",
                    "reporting",
                    "plugins",
                    "pluginManagement",
                    "dependencyManagement",
                    "exclusions",
                ],
            )
    }) {
        let group = dependency_child_prop(dep_node, "groupId", &props);
        let artifact = dependency_child_prop(dep_node, "artifactId", &props);
        let explicit_version = dependency_child_prop(dep_node, "version", &props);
        let classifier = dependency_child_prop(dep_node, "classifier", &props);
        let scope_str = direct_child_text(dep_node, "scope").unwrap_or_default();

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

fn dependency_child_prop(
    dep_node: Node<'_, '_>,
    tag: &str,
    props: &BTreeMap<String, String>,
) -> Option<String> {
    direct_child_text(dep_node, tag)
        .map(|s| expand_props(&s, props).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn find_release_by_tags(
    doc: &Document<'_>,
    tags: &[&str],
    props: &BTreeMap<String, String>,
) -> Option<Release> {
    for &tag in tags {
        for node in doc
            .descendants()
            .filter(|n| is_tag(*n, tag) && !has_any_ancestor(*n, &["profiles"]))
        {
            let raw_val = child_text(node);
            let val = expand_props(&raw_val, props);
            if let Some(rel) = parse_version_string(&val) {
                return Some(rel);
            }
        }
    }
    None
}

fn execution_mentions_test_compile(exec: Node<'_, '_>) -> bool {
    exec.descendants().any(|n| {
        (n.is_text()
            && n.text().is_some_and(|t| {
                let lower = t.to_ascii_lowercase();
                lower.contains("testcompile") || lower.contains("test-compile")
            }))
            || (n.is_element()
                && n.attributes().any(|a| {
                    let lower = a.value().to_ascii_lowercase();
                    lower.contains("testcompile") || lower.contains("test-compile")
                }))
    })
}

fn extract_test_release_from_executions(
    doc: &Document<'_>,
    in_profiles: bool,
    props: &BTreeMap<String, String>,
) -> Option<Release> {
    let mut max_release: Option<Release> = None;
    for exec in doc
        .descendants()
        .filter(|n| is_tag(*n, "execution") && has_any_ancestor(*n, &["profiles"]) == in_profiles)
    {
        if !execution_mentions_test_compile(exec) {
            continue;
        }
        for tag in ["testRelease", "release"] {
            if let Some(node) = exec.descendants().find(|n| is_tag(*n, tag)) {
                let raw_val = child_text(node);
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

#[cfg(test)]
pub(super) fn parse_maven_properties(content: &str) -> BTreeMap<String, String> {
    let Some(doc) = parse_xml(content) else {
        return BTreeMap::new();
    };
    parse_maven_properties_doc(&doc)
}

fn parse_maven_properties_doc(doc: &Document<'_>) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    let root = doc.root_element();
    let parent_node = root.children().find(|n| is_tag(*n, "parent"));

    let parent_group = parent_node
        .and_then(|p| direct_child_text(p, "groupId"))
        .filter(|v| !v.is_empty());
    let parent_artifact = parent_node
        .and_then(|p| direct_child_text(p, "artifactId"))
        .filter(|v| !v.is_empty());
    let parent_version = parent_node
        .and_then(|p| direct_child_text(p, "version"))
        .filter(|v| !v.is_empty());

    let proj_group = direct_child_text(root, "groupId")
        .filter(|v| !v.is_empty())
        .or_else(|| parent_group.clone());
    let proj_artifact = direct_child_text(root, "artifactId").filter(|v| !v.is_empty());
    let proj_ver = direct_child_text(root, "version")
        .filter(|v| !v.is_empty())
        .or_else(|| parent_version.clone());

    if let Some(g) = proj_group {
        props.insert("project.groupId".to_string(), g);
    }
    if let Some(a) = proj_artifact {
        props.insert("project.artifactId".to_string(), a);
    }
    if let Some(v) = proj_ver {
        props.insert("project.version".to_string(), v);
    }

    if let Some(g) = parent_group {
        props.insert("project.parent.groupId".to_string(), g);
    }
    if let Some(a) = parent_artifact {
        props.insert("project.parent.artifactId".to_string(), a);
    }
    if let Some(v) = parent_version {
        props.insert("project.parent.version".to_string(), v);
    }

    let props_node = root
        .children()
        .find(|n| is_tag(*n, "properties"))
        .or_else(|| {
            doc.descendants()
                .find(|n| is_tag(*n, "properties") && !has_any_ancestor(*n, &["profiles"]))
        })
        .or_else(|| doc.descendants().find(|n| is_tag(*n, "properties")));

    if let Some(props_el) = props_node {
        for child in props_el.children().filter(|n| n.is_element()) {
            let key = child.tag_name().name();
            if !key.is_empty() {
                props.insert(key.to_string(), child_text(child));
            }
        }
    }

    props
}
