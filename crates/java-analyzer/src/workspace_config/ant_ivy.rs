use std::{collections::BTreeMap, fs, path::Path};

use ide::{ModuleDependency, RootScope, normalize_rel_path};

use super::{
    DetectedVersionSpec, ManifestMetadata, parse_version_string,
    xml::{expand_props, has_any_ancestor, is_tag, parse_xml},
};

fn load_ant_properties_file(content: &str, props: &mut BTreeMap<String, String>) {
    for raw_line in content.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }
        let Some(sep_idx) = trimmed.find(['=', ':']) else {
            continue;
        };
        let key = trimmed[..sep_idx].trim();
        let val = trimmed[sep_idx + 1..].trim();
        if !key.is_empty() && !props.contains_key(key) {
            props.insert(key.to_string(), val.to_string());
        }
    }
}

pub(super) fn introspect_ant_build_xml(
    workspace_dir: &Path,
    content: &str,
    path: &Path,
    version_specs: &mut Vec<DetectedVersionSpec>,
    meta: &mut ManifestMetadata,
) -> BTreeMap<String, String> {
    let Some(doc) = parse_xml(content) else {
        return BTreeMap::new();
    };

    // 1. Collect <property name="..." value="..." / location="..."/> and <property file="..."/>
    let mut props: BTreeMap<String, String> = BTreeMap::new();
    props.insert("basedir".to_string(), ".".to_string());
    for node in doc.descendants().filter(|n| is_tag(*n, "property")) {
        if let Some(name) = node.attribute("name") {
            if let Some(val) = node
                .attribute("value")
                .or_else(|| node.attribute("location"))
                && !props.contains_key(name)
            {
                let resolved = expand_props(val, &props);
                props.insert(name.to_string(), resolved);
            }
        } else if let Some(file_attr) = node.attribute("file") {
            let expanded_path = expand_props(file_attr, &props);
            if !expanded_path.contains("${") {
                let prop_path = workspace_dir.join(expanded_path.trim());
                if let Ok(prop_text) = fs::read_to_string(&prop_path) {
                    load_ant_properties_file(&prop_text, &mut props);
                }
            }
        }
    }

    // 2. Inspect <javac ...> tags for srcdir, release/source/target, classpath
    let mut found_release = None;
    for javac_node in doc.descendants().filter(|n| is_tag(*n, "javac")) {
        for attr in ["release", "source", "target"] {
            if let Some(raw_ver) = javac_node.attribute(attr) {
                let expanded = expand_props(raw_ver, &props);
                if let Some(rel) = parse_version_string(&expanded) {
                    found_release = Some(rel);
                    break;
                }
            }
        }
        if let Some(srcdir) = javac_node.attribute("srcdir") {
            let expanded = expand_props(srcdir, &props);
            for part in expanded.split([':', ';']) {
                let norm = normalize_rel_path(part.trim());
                if norm != "." && !norm.contains("${") && !norm.contains("@{") {
                    let scope = RootScope::from_path(&norm);
                    if !meta.extra_source_dirs.iter().any(|(p, _)| p == &norm) {
                        meta.extra_source_dirs.push((norm, scope));
                    }
                }
            }
        }
        if let Some(cp) = javac_node.attribute("classpath") {
            let expanded = expand_props(cp, &props);
            for part in expanded.split([':', ';']) {
                let norm = normalize_rel_path(part.trim());
                if norm != "." && !norm.contains("${") && !norm.contains("@{") {
                    let dep = ModuleDependency::Path {
                        path: norm,
                        scope: RootScope::Main,
                    };
                    if !meta.dependencies.contains(&dep) {
                        meta.dependencies.push(dep);
                    }
                }
            }
        }
    }

    // Also inspect <src path="..."/>
    for src_node in doc.descendants().filter(|n| is_tag(*n, "src")) {
        if let Some(p) = src_node
            .attribute("path")
            .or_else(|| src_node.attribute("location"))
        {
            let expanded = expand_props(p, &props);
            let norm = normalize_rel_path(&expanded);
            if norm != "." && !norm.contains("${") && !norm.contains("@{") {
                let scope = RootScope::from_path(&norm);
                if !meta.extra_source_dirs.iter().any(|(d, _)| d == &norm) {
                    meta.extra_source_dirs.push((norm, scope));
                }
            }
        }
    }

    // Inspect <fileset dir="..."/> inside <path> and <classpath> blocks for compile-time JARs
    for fs_node in doc
        .descendants()
        .filter(|n| is_tag(*n, "fileset") && has_any_ancestor(*n, &["path", "classpath"]))
    {
        if let Some(dir) = fs_node.attribute("dir") {
            let expanded = expand_props(dir, &props);
            let norm = normalize_rel_path(&expanded);
            if norm != "." && !norm.contains("${") && !norm.contains("@{") {
                let full_dir = workspace_dir.join(&norm);
                let mut added_jars = false;
                if let Ok(entries) = fs::read_dir(&full_dir) {
                    let mut jar_paths: Vec<String> = entries
                        .flatten()
                        .filter_map(|e| {
                            let p = e.path();
                            if p.extension().and_then(|x| x.to_str()) == Some("jar") {
                                let name = p.file_name()?.to_str()?;
                                Some(normalize_rel_path(&format!("{norm}/{name}")))
                            } else {
                                None
                            }
                        })
                        .collect();
                    jar_paths.sort();
                    for jp in jar_paths {
                        added_jars = true;
                        let dep = ModuleDependency::Path {
                            path: jp,
                            scope: RootScope::Main,
                        };
                        if !meta.dependencies.contains(&dep) {
                            meta.dependencies.push(dep);
                        }
                    }
                }
                if !added_jars {
                    let dep = ModuleDependency::Path {
                        path: norm,
                        scope: RootScope::Main,
                    };
                    if !meta.dependencies.contains(&dep) {
                        meta.dependencies.push(dep);
                    }
                }
            }
        }
    }

    let has_preview = doc.descendants().any(|n| {
        (n.is_text() && n.text().is_some_and(|t| t.contains("--enable-preview")))
            || (n.is_element()
                && n.attributes()
                    .any(|a| a.value().contains("--enable-preview")))
    });

    if let Some(release) = found_release {
        version_specs.push(DetectedVersionSpec {
            source: "build.xml".to_string(),
            path: path.to_path_buf(),
            release,
            preview: has_preview,
        });
    }

    props
}

pub(super) fn introspect_ivy_xml(
    content: &str,
    ant_props: &BTreeMap<String, String>,
    meta: &mut ManifestMetadata,
) {
    let Some(doc) = parse_xml(content) else {
        return;
    };
    for dep_node in doc.descendants().filter(|n| is_tag(*n, "dependency")) {
        let org = dep_node
            .attribute("org")
            .map(|s| expand_props(s, ant_props).trim().to_string())
            .filter(|s| !s.is_empty());
        let name = dep_node
            .attribute("name")
            .map(|s| expand_props(s, ant_props).trim().to_string())
            .filter(|s| !s.is_empty());
        let rev = dep_node
            .attribute("rev")
            .map(|s| expand_props(s, ant_props).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "*".to_string());
        let conf = dep_node.attribute("conf").unwrap_or_default();

        if let (Some(org), Some(name)) = (org, name)
            && let Some(scope) = classify_ivy_conf(conf)
        {
            let dep = ModuleDependency::Maven {
                group: org,
                artifact: name,
                version: rev,
                classifier: None,
                scope,
            };
            if !meta.dependencies.contains(&dep) {
                meta.dependencies.push(dep);
            }
        }
    }
}

/// Classifies an Apache Ivy `conf` attribute into a compile-time `RootScope`, or `None` if it is
/// strictly a runtime/deployment configuration (`runtime`, `deploy`, `dist`, `war`, `ear`).
fn classify_ivy_conf(conf: &str) -> Option<RootScope> {
    let trimmed = conf.trim();
    if trimmed.is_empty() {
        return Some(RootScope::Main);
    }
    let lower = trimmed.to_ascii_lowercase();
    let lhs_parts: Vec<&str> = lower
        .split(';')
        .map(|mapping| mapping.split("->").next().unwrap_or(mapping).trim())
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .collect();

    let has_compile = lhs_parts.iter().any(|c| {
        matches!(
            *c,
            "*" | "compile" | "default" | "provided" | "build" | "main" | "api"
        )
    });
    if has_compile {
        return Some(RootScope::Main);
    }
    let has_test = lhs_parts
        .iter()
        .any(|c| *c == "test" || c.starts_with("test-"));
    if has_test {
        return Some(RootScope::Test);
    }
    // Otherwise it's runtime / deploy / packaging only — out of scope for LSP compile-time resolution
    None
}
