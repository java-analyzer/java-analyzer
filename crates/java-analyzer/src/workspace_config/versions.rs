use std::{
    fs,
    path::{Path, PathBuf},
};

use ide::Release;

use super::DetectedVersionSpec;

pub(super) fn detect_version_specs(workspace_dir: &Path) -> Vec<DetectedVersionSpec> {
    let mut specs = Vec::new();

    // 1. .java-version
    let java_version_path = workspace_dir.join(".java-version");
    if let Ok(content) = fs::read_to_string(&java_version_path)
        && let Some(release) = parse_version_string(&content)
    {
        specs.push(DetectedVersionSpec {
            source: ".java-version".to_string(),
            path: java_version_path,
            release,
            preview: false,
        });
    }

    // 2. .sdkmanrc
    let sdkman_path = workspace_dir.join(".sdkmanrc");
    if let Ok(content) = fs::read_to_string(&sdkman_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some((k, v)) = line.split_once('=')
                && k.trim() == "java"
                && let Some(release) = parse_version_string(v)
            {
                specs.push(DetectedVersionSpec {
                    source: ".sdkmanrc".to_string(),
                    path: sdkman_path.clone(),
                    release,
                    preview: false,
                });
            }
        }
    }

    // 3. .tool-versions
    let tool_versions_path = workspace_dir.join(".tool-versions");
    if let Ok(content) = fs::read_to_string(&tool_versions_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            if parts.next() == Some("java")
                && let Some(ver) = parts.next()
                && let Some(release) = parse_version_string(ver)
            {
                specs.push(DetectedVersionSpec {
                    source: ".tool-versions".to_string(),
                    path: tool_versions_path.clone(),
                    release,
                    preview: false,
                });
            }
        }
    }

    // 4. mise.toml / .mise.toml
    for mise_name in ["mise.toml", ".mise.toml"] {
        let mise_path = workspace_dir.join(mise_name);
        if let Ok(content) = fs::read_to_string(&mise_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=')
                    && k.trim() == "java"
                    && let Some(release) = parse_version_string(v)
                {
                    specs.push(DetectedVersionSpec {
                        source: mise_name.to_string(),
                        path: mise_path.clone(),
                        release,
                        preview: false,
                    });
                }
            }
        }
    }

    // 5. .bazelrc
    let bazelrc_path = workspace_dir.join(".bazelrc");
    if let Ok(content) = fs::read_to_string(&bazelrc_path) {
        for line in content.lines() {
            if let Some(idx) = line.find("--java_language_version=") {
                let rest = &line[idx + "--java_language_version=".len()..];
                let token = rest.split_whitespace().next().unwrap_or("");
                if let Some(release) = parse_version_string(token) {
                    specs.push(DetectedVersionSpec {
                        source: ".bazelrc".to_string(),
                        path: bazelrc_path,
                        release,
                        preview: false,
                    });
                    break;
                }
            }
        }
    }

    specs
}

fn release_from_u16(n: u16) -> Option<Release> {
    if (8..=27).contains(&n) {
        Some(Release::new(n))
    } else {
        None
    }
}

pub fn parse_version_string(raw: &str) -> Option<Release> {
    let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("1.") {
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let n: u16 = num_str.parse().ok()?;
        return release_from_u16(n);
    }
    // Skip vendor prefix like `temurin-21.0.2` or `openjdk-17`
    let digit_start = trimmed.find(|c: char| c.is_ascii_digit())?;
    let after = &trimmed[digit_start..];
    if let Some(rest) = after.strip_prefix("1.") {
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let n: u16 = num_str.parse().ok()?;
        return release_from_u16(n);
    }
    let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: u16 = num_str.parse().ok()?;
    release_from_u16(n)
}

// ---- External Spec Reconciliation ----

/// Updates conflicting external version specification files in `workspace_dir` (`.java-version`,
/// `.sdkmanrc`, `.tool-versions`, `pom.xml`) to `target_release`, returning the paths modified.
pub fn reconcile_version_conflict(workspace_dir: &Path, target_release: Release) -> Vec<PathBuf> {
    let mut modified = Vec::new();
    let num = target_release.number();

    // 1. .java-version
    let java_ver = workspace_dir.join(".java-version");
    if java_ver.is_file() && fs::write(&java_ver, format!("{num}\n")).is_ok() {
        modified.push(java_ver);
    }

    // 2. .sdkmanrc
    let sdkman = workspace_dir.join(".sdkmanrc");
    if let Ok(content) = fs::read_to_string(&sdkman) {
        let mut new_lines = Vec::new();
        let mut changed = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('#')
                && let Some((k, v)) = trimmed.split_once('=')
                && k.trim() == "java"
            {
                let suffix = v
                    .trim()
                    .find('-')
                    .map(|idx| &v.trim()[idx..])
                    .unwrap_or("-tem");
                new_lines.push(format!("java={num}.0.0{suffix}"));
                changed = true;
                continue;
            }
            new_lines.push(line.to_string());
        }
        if changed && fs::write(&sdkman, format!("{}\n", new_lines.join("\n"))).is_ok() {
            modified.push(sdkman);
        }
    }

    // 3. .tool-versions
    let tool_ver = workspace_dir.join(".tool-versions");
    if let Ok(content) = fs::read_to_string(&tool_ver) {
        let mut new_lines = Vec::new();
        let mut changed = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('#') && trimmed.split_whitespace().next() == Some("java") {
                new_lines.push(format!("java {num}"));
                changed = true;
                continue;
            }
            new_lines.push(line.to_string());
        }
        if changed && fs::write(&tool_ver, format!("{}\n", new_lines.join("\n"))).is_ok() {
            modified.push(tool_ver);
        }
    }

    // 4. pom.xml (update <maven.compiler.release>, <maven.compiler.source>, <maven.compiler.target>, <java.version>)
    let pom = workspace_dir.join("pom.xml");
    if let Ok(content) = fs::read_to_string(&pom) {
        let mut updated = content.clone();
        for tag in [
            "maven.compiler.release",
            "maven.compiler.source",
            "maven.compiler.target",
            "java.version",
        ] {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            if let Some(start) = updated.find(&open) {
                let val_start = start + open.len();
                if let Some(end) = updated[val_start..].find(&close) {
                    updated.replace_range(val_start..val_start + end, &num.to_string());
                }
            }
        }
        if updated != content && fs::write(&pom, updated).is_ok() {
            modified.push(pom);
        }
    }

    modified
}
