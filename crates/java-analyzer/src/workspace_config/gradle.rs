use std::path::Path;

use ide::{ModuleDependency, RootScope};

use super::{parse_version_string, DetectedVersionSpec, ManifestMetadata};

pub(super) fn introspect_gradle_build(
    content: &str,
    source_name: &str,
    path: &Path,
    version_specs: &mut Vec<DetectedVersionSpec>,
    meta: &mut ManifestMetadata,
) {
    let preview = content.contains("--enable-preview");
    let mut found_release = None;

    if let Some(idx) = content.find("JavaLanguageVersion.of(") {
        let after = &content[idx + "JavaLanguageVersion.of(".len()..];
        let arg: String = after.chars().take_while(|c| *c != ')').collect();
        found_release = parse_version_string(&arg);
    }
    if found_release.is_none() {
        for key in ["sourceCompatibility", "targetCompatibility"] {
            if let Some(idx) = content.find(key) {
                let line = content[idx..].lines().next().unwrap_or("");
                if let Some(ver_idx) = line.find("VERSION_") {
                    let after = &line[ver_idx + "VERSION_".len()..];
                    let ver = after.replace('_', ".");
                    found_release = parse_version_string(&ver);
                } else if let Some((_, rhs)) = line.split_once('=') {
                    found_release = parse_version_string(rhs);
                }
                if found_release.is_some() {
                    break;
                }
            }
        }
    }

    if let Some(release) = found_release {
        version_specs.push(DetectedVersionSpec {
            source: source_name.to_string(),
            path: path.to_path_buf(),
            release,
            preview,
        });
    }

    // Extract compile-time dependencies (implementation, api, compileOnly, testImplementation, testCompileOnly)
    for line in content.lines() {
        let trimmed = line.trim();
        let (scope, after_conf) = if let Some(r) =
            strip_gradle_conf(trimmed, &["implementation", "api", "compileOnly", "compile"])
        {
            (RootScope::Main, r)
        } else if let Some(r) = strip_gradle_conf(
            trimmed,
            &["testImplementation", "testCompileOnly", "testCompile"],
        ) {
            (RootScope::Test, r)
        } else {
            continue;
        };

        if let Some(proj_idx) = after_conf.find("project(") {
            let after_proj = &after_conf[proj_idx + "project(".len()..];
            if let Some(quoted) = extract_first_quoted(after_proj) {
                let target = quoted.trim_start_matches(':').replace(':', "/");
                if !target.is_empty() {
                    let dep = ModuleDependency::parse(&target, scope, &[]);
                    if !meta.dependencies.contains(&dep) {
                        meta.dependencies.push(dep);
                    }
                }
            }
        } else if let Some(coords) = extract_first_quoted(after_conf) {
            let parts: Vec<&str> = coords.split(':').collect();
            if parts.len() >= 3 {
                let dep = ModuleDependency::Maven {
                    group: parts[0].to_string(),
                    artifact: parts[1].to_string(),
                    version: parts[2].to_string(),
                    classifier: parts.get(3).map(|s| (*s).to_string()),
                    scope,
                };
                if !meta.dependencies.contains(&dep) {
                    meta.dependencies.push(dep);
                }
            }
        }
    }
}

fn strip_gradle_conf<'a>(line: &'a str, confs: &[&str]) -> Option<&'a str> {
    for conf in confs {
        if let Some(rest) = line.strip_prefix(conf) {
            let first = rest.chars().next()?;
            if first == '(' || first.is_whitespace() || first == '"' || first == '\'' {
                return Some(rest);
            }
        }
    }
    None
}

fn extract_first_quoted(s: &str) -> Option<String> {
    for quote in ['"', '\''] {
        if let Some(start) = s.find(quote) {
            let after = &s[start + 1..];
            if let Some(end) = after.find(quote) {
                return Some(after[..end].to_string());
            }
        }
    }
    None
}
