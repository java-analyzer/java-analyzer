//! Version synchronization and release readiness checks (`cargo xtask release-prep`).
//! See `docs/specs/0022-release-process-and-publishing.md`.

use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, PartialEq, Eq, usage::Args)]
#[usage(crate = usage)]
pub struct ReleasePrepArgs {
    /// Target SemVer version (`X.Y.Z` or `X.Y.Z-rc.N`); defaults to workspace version in `--check` mode
    #[usage(value_name = "VERSION")]
    pub version: Option<String>,
    /// Verify version synchronization and CHANGELOG.md without modifying files
    #[usage(long)]
    pub check: bool,
    /// Allow updating or checking manifests even if CHANGELOG.md lacks `## [<VERSION>]`
    #[usage(long)]
    pub allow_missing_changelog: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReport {
    pub target_version: String,
    pub updated_files: Vec<PathBuf>,
}

/// Locates the workspace root starting from the current directory, falling back to
/// `CARGO_MANIFEST_DIR`'s parent when invoked outside the repository.
pub fn workspace_root() -> Result<PathBuf, String> {
    if let Ok(cwd) = std::env::current_dir()
        && let Some(found) = find_workspace_root_from(&cwd)
    {
        return Ok(found);
    }
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "cannot locate workspace root".to_string())
}

fn find_workspace_root_from(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.is_file()
            && let Ok(text) = fs::read_to_string(&cargo_toml)
            && text.contains("[workspace]")
        {
            return Some(current.to_path_buf());
        }
        dir = current.parent();
    }
    None
}

/// Validates that `version` is strict SemVer `X.Y.Z` or `X.Y.Z-rc.N`.
pub fn validate_semver(version: &str) -> Result<(), String> {
    if version.is_empty() {
        return Err("version must not be empty".to_string());
    }
    if version.starts_with('v') || version.starts_with('V') {
        return Err(format!(
            "invalid version '{version}': omit leading 'v' (expected X.Y.Z or X.Y.Z-rc.N)"
        ));
    }
    let (core, pre) = match version.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (version, None),
    };

    let mut parts = core.split('.');
    let Some(major) = parts.next() else {
        return Err(format!(
            "invalid version '{version}': expected SemVer X.Y.Z or X.Y.Z-rc.N"
        ));
    };
    let Some(minor) = parts.next() else {
        return Err(format!(
            "invalid version '{version}': expected SemVer X.Y.Z or X.Y.Z-rc.N"
        ));
    };
    let Some(patch) = parts.next() else {
        return Err(format!(
            "invalid version '{version}': expected SemVer X.Y.Z or X.Y.Z-rc.N"
        ));
    };
    if parts.next().is_some()
        || !is_semver_num(major)
        || !is_semver_num(minor)
        || !is_semver_num(patch)
    {
        return Err(format!(
            "invalid version '{version}': expected SemVer X.Y.Z or X.Y.Z-rc.N"
        ));
    }

    if let Some(pre) = pre {
        let Some(rc_num) = pre.strip_prefix("rc.") else {
            return Err(format!(
                "invalid version '{version}': pre-release suffix must be '-rc.N'"
            ));
        };
        if !is_semver_num(rc_num) {
            return Err(format!(
                "invalid version '{version}': invalid rc number in '{pre}'"
            ));
        }
    }

    Ok(())
}

fn is_semver_num(part: &str) -> bool {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    part == "0" || !part.starts_with('0')
}

pub fn run(args: &ReleasePrepArgs) -> i32 {
    let root = match workspace_root() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("error: {err}");
            return 1;
        }
    };
    match run_in(&root, args) {
        Ok(report) => {
            if args.check {
                println!(
                    "release-prep: all manifests and CHANGELOG.md match v{}",
                    report.target_version
                );
            } else {
                println!(
                    "release-prep: synchronized {} file(s) to v{}",
                    report.updated_files.len(),
                    report.target_version
                );
            }
            0
        }
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    }
}

pub fn run_in(root: &Path, args: &ReleasePrepArgs) -> Result<VersionReport, String> {
    let cargo_path = root.join("Cargo.toml");
    let vscode_path = root.join("editors/vscode/package.json");
    let zed_path = root.join("editors/zed/extension.toml");
    let kdl_path = root.join("crates/java-analyzer/java-analyzer.usage.kdl");
    let changelog_path = root.join("CHANGELOG.md");

    let cargo_text = read_file(&cargo_path)?;
    let cargo_ver = read_workspace_cargo_version(&cargo_text, &cargo_path)?;

    let target_version = match &args.version {
        Some(v) => {
            validate_semver(v)?;
            v.clone()
        }
        None if args.check => {
            validate_semver(&cargo_ver)?;
            cargo_ver.clone()
        }
        None => {
            return Err(
                "missing <VERSION> argument (pass `<VERSION>` to update or `--check` to verify)"
                    .to_string(),
            );
        }
    };

    let vscode_text = read_file(&vscode_path)?;
    let vscode_ver = read_vscode_version(&vscode_text, &vscode_path)?;

    let zed_text = read_file(&zed_path)?;
    let zed_ver = read_zed_version(&zed_text, &zed_path)?;

    let kdl_text = read_file(&kdl_path)?;
    let kdl_ver = read_kdl_version(&kdl_text, &kdl_path)?;

    if args.check {
        let mut errors = Vec::new();
        for (label, found) in [
            ("Cargo.toml", cargo_ver.as_str()),
            ("editors/vscode/package.json", vscode_ver.as_str()),
            ("editors/zed/extension.toml", zed_ver.as_str()),
            ("crates/java-analyzer/java-analyzer.usage.kdl", kdl_ver.as_str()),
        ] {
            if found != target_version {
                errors.push(format!(
                    "{label}: expected version '{target_version}', found '{found}'"
                ));
            }
        }
        if !args.allow_missing_changelog {
            check_changelog(&changelog_path, &target_version)
                .unwrap_or_else(|e| errors.push(e));
        }
        if !errors.is_empty() {
            return Err(format!(
                "version synchronization check failed:\n  - {}",
                errors.join("\n  - ")
            ));
        }
        return Ok(VersionReport {
            target_version,
            updated_files: Vec::new(),
        });
    }

    let mut updated_files = Vec::new();

    if cargo_ver != target_version {
        let next = replace_toml_section_version(&cargo_text, Some("workspace.package"), &target_version)
            .map_err(|e| format!("{}: {e}", cargo_path.display()))?;
        let verified = read_workspace_cargo_version(&next, &cargo_path)?;
        if verified != target_version {
            return Err(format!(
                "{}: post-edit verification failed (got '{verified}')",
                cargo_path.display()
            ));
        }
        fs::write(&cargo_path, next)
            .map_err(|e| format!("failed to write {}: {e}", cargo_path.display()))?;
        updated_files.push(cargo_path);
    }

    if vscode_ver != target_version {
        let next = replace_json_top_level_version(&vscode_text, &target_version)
            .map_err(|e| format!("{}: {e}", vscode_path.display()))?;
        let verified = read_vscode_version(&next, &vscode_path)?;
        if verified != target_version {
            return Err(format!(
                "{}: post-edit verification failed (got '{verified}')",
                vscode_path.display()
            ));
        }
        fs::write(&vscode_path, next)
            .map_err(|e| format!("failed to write {}: {e}", vscode_path.display()))?;
        updated_files.push(vscode_path);
    }

    if zed_ver != target_version {
        let next = replace_toml_section_version(&zed_text, None, &target_version)
            .map_err(|e| format!("{}: {e}", zed_path.display()))?;
        let verified = read_zed_version(&next, &zed_path)?;
        if verified != target_version {
            return Err(format!(
                "{}: post-edit verification failed (got '{verified}')",
                zed_path.display()
            ));
        }
        fs::write(&zed_path, next)
            .map_err(|e| format!("failed to write {}: {e}", zed_path.display()))?;
        updated_files.push(zed_path);
    }

    if kdl_ver != target_version {
        let next = replace_kdl_version(&kdl_text, &target_version)
            .map_err(|e| format!("{}: {e}", kdl_path.display()))?;
        let verified = read_kdl_version(&next, &kdl_path)?;
        if verified != target_version {
            return Err(format!(
                "{}: post-edit verification failed (got '{verified}')",
                kdl_path.display()
            ));
        }
        fs::write(&kdl_path, next)
            .map_err(|e| format!("failed to write {}: {e}", kdl_path.display()))?;
        updated_files.push(kdl_path);
    }

    if !args.allow_missing_changelog {
        check_changelog(&changelog_path, &target_version)?;
    }

    Ok(VersionReport {
        target_version,
        updated_files,
    })
}

fn read_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))
}

pub fn read_workspace_cargo_version(text: &str, path: &Path) -> Result<String, String> {
    let val: toml::Value =
        toml::from_str(text).map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    val.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "{}: missing [workspace.package].version string",
                path.display()
            )
        })
}

fn read_vscode_version(text: &str, path: &Path) -> Result<String, String> {
    let val: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    val.get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{}: missing top-level \"version\" string", path.display()))
}

fn read_zed_version(text: &str, path: &Path) -> Result<String, String> {
    let val: toml::Value =
        toml::from_str(text).map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    val.get("version")
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{}: missing top-level `version` string", path.display()))
}

fn read_kdl_version(text: &str, path: &Path) -> Result<String, String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version")
            && rest.starts_with([' ', '\t'])
        {
            let rest = rest.trim_start();
            if let Some(quoted) = rest.strip_prefix('"')
                && let Some(end) = quoted.find('"')
            {
                return Ok(quoted[..end].to_string());
            }
        }
    }
    Err(format!("{}: missing top-level `version \"...\"` line", path.display()))
}

fn check_changelog(path: &Path, version: &str) -> Result<(), String> {
    let text = read_file(path)?;
    let exact = format!("## [{version}]");
    let with_meta = format!("## [{version}] ");
    let has_section = text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == exact || trimmed.starts_with(&with_meta)
    });
    if !has_section {
        return Err(format!(
            "{}: missing release heading '## [{version}]' (pass --allow-missing-changelog to skip)",
            path.display()
        ));
    }
    Ok(())
}

fn replace_toml_section_version(
    text: &str,
    target_section: Option<&str>,
    new_version: &str,
) -> Result<String, String> {
    let mut current_section: Option<String> = None;
    let mut replaced = false;
    let mut out = String::with_capacity(text.len() + 16);

    for segment in text.split_inclusive('\n') {
        let (line, ending) = split_line_ending(segment);
        let trimmed = line.trim();

        if trimmed.starts_with('[') && !trimmed.starts_with("[[") && trimmed.ends_with(']') {
            let section_name = trimmed[1..trimmed.len() - 1].trim();
            current_section = Some(section_name.to_string());
            out.push_str(line);
            out.push_str(ending);
            continue;
        }

        let in_target = match target_section {
            Some(expected) => current_section.as_deref() == Some(expected),
            None => current_section.is_none(),
        };

        if !replaced
            && in_target
            && let Some(after_key) = trimmed.strip_prefix("version")
            && after_key.trim_start().starts_with('=')
        {
            let updated_line = replace_first_double_quoted(line, new_version)?;
            out.push_str(&updated_line);
            out.push_str(ending);
            replaced = true;
            continue;
        }

        out.push_str(line);
        out.push_str(ending);
    }

    if !replaced {
        let scope = target_section.unwrap_or("<top-level>");
        return Err(format!("could not locate `version = \"...\"` in {scope}"));
    }
    Ok(out)
}

fn replace_json_top_level_version(text: &str, new_version: &str) -> Result<String, String> {
    let mut depth: i32 = 0;
    let mut replaced = false;
    let mut out = String::with_capacity(text.len() + 16);

    for segment in text.split_inclusive('\n') {
        let (line, ending) = split_line_ending(segment);
        let trimmed = line.trim_start();

        if !replaced
            && depth == 1
            && let Some(after_key) = trimmed.strip_prefix("\"version\"")
            && after_key.trim_start().starts_with(':')
        {
            let key_idx = line.find("\"version\"").ok_or_else(|| {
                "internal error locating \"version\" key".to_string()
            })?;
            let after_key_offset = key_idx + "\"version\"".len();
            let prefix = &line[..after_key_offset];
            let suffix = replace_first_double_quoted(&line[after_key_offset..], new_version)?;
            out.push_str(prefix);
            out.push_str(&suffix);
            out.push_str(ending);
            replaced = true;
        } else {
            out.push_str(line);
            out.push_str(ending);
        }

        depth = update_json_brace_depth(depth, line);
    }

    if !replaced {
        return Err("could not locate top-level `\"version\": \"...\"`".to_string());
    }
    Ok(out)
}

fn update_json_brace_depth(mut depth: i32, line: &str) -> i32 {
    let mut in_str = false;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_str {
            match ch {
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn replace_kdl_version(text: &str, new_version: &str) -> Result<String, String> {
    let mut replaced = false;
    let mut out = String::with_capacity(text.len() + 16);

    for segment in text.split_inclusive('\n') {
        let (line, ending) = split_line_ending(segment);
        let trimmed = line.trim_start();

        if !replaced
            && let Some(after_key) = trimmed.strip_prefix("version")
            && after_key.starts_with([' ', '\t'])
        {
            let updated_line = replace_first_double_quoted(line, new_version)?;
            out.push_str(&updated_line);
            out.push_str(ending);
            replaced = true;
            continue;
        }

        out.push_str(line);
        out.push_str(ending);
    }

    if !replaced {
        return Err("could not locate top-level `version \"...\"`".to_string());
    }
    Ok(out)
}

fn split_line_ending(segment: &str) -> (&str, &str) {
    if let Some(stripped) = segment.strip_suffix("\r\n") {
        (stripped, "\r\n")
    } else if let Some(stripped) = segment.strip_suffix('\n') {
        (stripped, "\n")
    } else {
        (segment, "")
    }
}

fn replace_first_double_quoted(line: &str, replacement: &str) -> Result<String, String> {
    let Some(open) = line.find('"') else {
        return Err(format!("expected quoted string in line: {line}"));
    };
    let after_open = &line[open + 1..];
    let Some(close_rel) = after_open.find('"') else {
        return Err(format!("unterminated quoted string in line: {line}"));
    };
    let close = open + 1 + close_rel;
    let mut out = String::with_capacity(line.len() + replacement.len());
    out.push_str(&line[..=open]);
    out.push_str(replacement);
    out.push_str(&line[close..]);
    Ok(out)
}
