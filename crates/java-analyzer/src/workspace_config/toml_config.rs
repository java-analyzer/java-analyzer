use std::path::{Path, PathBuf};

use hir::{normalize_rel_path, DeltaSet};

use super::{
    build_tool_name, parse_build_tool_kind, parse_version_string, ExplicitConfig, WorkspaceProbe,
};

pub const DOT_JAVA_ANALYZER_CONFIG_FILE: &str = ".java-analyzer.toml";
pub const JAVA_ANALYZER_CONFIG_FILE: &str = "java-analyzer.toml";

pub fn find_config_file(workspace_dir: &Path) -> Option<PathBuf> {
    let dot_file = workspace_dir.join(DOT_JAVA_ANALYZER_CONFIG_FILE);
    if dot_file.is_file() {
        return Some(dot_file);
    }
    let plain_file = workspace_dir.join(JAVA_ANALYZER_CONFIG_FILE);
    if plain_file.is_file() {
        return Some(plain_file);
    }
    None
}

// ---- Minimal-Overlay `.java-analyzer.toml` Parser & Serializer ----

pub fn parse_config_toml(text: &str) -> ExplicitConfig {
    let mut cfg = ExplicitConfig::default();
    let mut current_section = String::new();

    for raw_line in text.lines() {
        let line = strip_toml_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        let Some((raw_key, raw_val)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        let val = raw_val.trim();

        if current_section.is_empty() {
            match key {
                "build" => {
                    if let Some(s) = unquote_toml_str(val) {
                        cfg.build = parse_build_tool_kind(&s);
                    }
                }
                "jdk" => {
                    if let Some(s) = unquote_toml_str(val) {
                        cfg.jdk = Some(PathBuf::from(s));
                    }
                }
                "release" => {
                    if let Some(r) = parse_version_string(val) {
                        cfg.release = Some(r);
                    }
                }
                "preview" => {
                    if let Some(b) = parse_toml_bool(val) {
                        cfg.preview = Some(b);
                    }
                }
                "classpath_command" => {
                    if val.starts_with('[') {
                        cfg.classpath_command = parse_toml_str_array(val);
                    } else if let Some(s) = unquote_toml_str(val) {
                        cfg.classpath_command =
                            s.split_whitespace().map(ToString::to_string).collect();
                    }
                }
                "include" => {
                    cfg.delta.include = parse_toml_str_array(val)
                        .into_iter()
                        .map(|p| normalize_rel_path(&p))
                        .collect();
                }
                "exclude" => {
                    cfg.delta.exclude = parse_toml_str_array(val)
                        .into_iter()
                        .map(|p| normalize_rel_path(&p))
                        .collect();
                }
                _ => {}
            }
        } else if let Some(mod_rest) = current_section.strip_prefix("modules.") {
            let (mod_key, sub_key) = parse_module_section_header(mod_rest);
            let entry = cfg.delta.modules.entry(mod_key).or_default();
            match sub_key.as_deref() {
                None => match key {
                    "release" => {
                        if let Some(r) = parse_version_string(val) {
                            entry.release = Some(r);
                        }
                    }
                    "preview" => {
                        if let Some(b) = parse_toml_bool(val) {
                            entry.preview = Some(b);
                        }
                    }
                    "sources" => {
                        entry.sources = parse_inline_delta_set(val, true);
                    }
                    "resources" => {
                        entry.resources = parse_inline_delta_set(val, true);
                    }
                    "dependencies" => {
                        entry.dependencies = parse_inline_delta_set(val, false);
                    }
                    _ => {}
                },
                Some("sources") => apply_delta_kv(&mut entry.sources, key, val, true),
                Some("resources") => apply_delta_kv(&mut entry.resources, key, val, true),
                Some("dependencies") => apply_delta_kv(&mut entry.dependencies, key, val, false),
                _ => {}
            }
        }
    }

    cfg
}

fn parse_module_section_header(rest: &str) -> (String, Option<String>) {
    let trimmed = rest.trim();
    if let Some(after_quote) = trimmed.strip_prefix('"') {
        if let Some(end_quote) = after_quote.find('"') {
            let mod_name = after_quote[..end_quote].to_string();
            let tail = after_quote[end_quote + 1..].trim();
            let sub = tail.strip_prefix('.').map(|s| s.trim().to_string());
            let norm_mod = if mod_name.contains('/') || mod_name == "." {
                normalize_rel_path(&mod_name)
            } else {
                mod_name
            };
            return (norm_mod, sub);
        }
    }
    if let Some((m, s)) = trimmed.rsplit_once('.') {
        if matches!(s, "sources" | "resources" | "dependencies") {
            let m = m.trim_matches('"');
            return (m.to_string(), Some(s.to_string()));
        }
    }
    (trimmed.trim_matches('"').to_string(), None)
}

fn parse_inline_delta_set(val: &str, normalize_paths: bool) -> DeltaSet<String> {
    let mut delta = DeltaSet::default();
    let inner = val
        .trim()
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(val);

    for field in ["include", "exclude"] {
        if let Some(idx) = inner.find(field) {
            let after = &inner[idx + field.len()..];
            if let Some(eq_idx) = after.find('=') {
                let after_eq = after[eq_idx + 1..].trim();
                if let Some(open_b) = after_eq.find('[') {
                    if let Some(close_b) = after_eq[open_b..].find(']') {
                        let arr_str = &after_eq[open_b..=open_b + close_b];
                        apply_delta_kv(&mut delta, field, arr_str, normalize_paths);
                    }
                }
            }
        }
    }
    delta
}

fn apply_delta_kv(delta: &mut DeltaSet<String>, key: &str, val: &str, normalize_paths: bool) {
    let items: Vec<String> = parse_toml_str_array(val)
        .into_iter()
        .map(|s| {
            if normalize_paths {
                normalize_rel_path(&s)
            } else {
                s
            }
        })
        .collect();
    match key {
        "include" => delta.include = items,
        "exclude" => delta.exclude = items,
        _ => {}
    }
}

fn strip_toml_comment(line: &str) -> &str {
    let mut in_quotes = false;
    let mut quote_ch = '"';
    for (i, ch) in line.char_indices() {
        if !in_quotes && (ch == '"' || ch == '\'') {
            in_quotes = true;
            quote_ch = ch;
        } else if in_quotes && ch == quote_ch {
            in_quotes = false;
        } else if !in_quotes && ch == '#' {
            return &line[..i];
        }
    }
    line
}

fn unquote_toml_str(val: &str) -> Option<String> {
    let t = val.trim();
    if (t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')) {
        if t.len() >= 2 {
            return Some(t[1..t.len() - 1].to_string());
        }
    }
    None
}

fn parse_toml_bool(val: &str) -> Option<bool> {
    match val.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_toml_str_array(val: &str) -> Vec<String> {
    let t = val.trim();
    let inner = t
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or("");
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote_ch = '"';
    for ch in inner.chars() {
        if !in_quote && (ch == '"' || ch == '\'') {
            in_quote = true;
            quote_ch = ch;
        } else if in_quote && ch == quote_ch {
            in_quote = false;
            items.push(current.clone());
            current.clear();
        } else if in_quote {
            current.push(ch);
        }
    }
    items
}

/// Serializes `config` into a minimal-overlay `.java-analyzer.toml` string against `probe`.
/// Any field in `config` that duplicates what `probe` already uniquely inferred from Tier 2 / Tier 3
/// is omitted so `.java-analyzer.toml` never hardcodes externally declared facts.
pub fn render_minimal_toml(probe: &WorkspaceProbe, config: &ExplicitConfig) -> String {
    let mut lines = Vec::new();

    if let Some(build) = config.build {
        if probe.inferred_build != Some(build) {
            lines.push(format!("build = \"{}\"", build_tool_name(build)));
        }
    }

    if let Some(jdk) = &config.jdk {
        lines.push(format!("jdk = \"{}\"", jdk.display()));
    }

    if let Some(release) = config.release {
        let inferred_rel = probe.inferred_level.map(|l| l.release);
        if inferred_rel != Some(release) {
            lines.push(format!("release = {}", release.number()));
        }
    }

    if let Some(preview) = config.preview {
        let inferred_prev = probe.inferred_level.map(|l| l.preview).unwrap_or(false);
        if preview != inferred_prev {
            lines.push(format!("preview = {preview}"));
        }
    }

    if !config.classpath_command.is_empty() {
        lines.push(format!(
            "classpath_command = {}",
            format_toml_str_array(&config.classpath_command)
        ));
    }

    if !config.delta.include.is_empty() {
        lines.push(format!(
            "include = {}",
            format_toml_str_array(&config.delta.include)
        ));
    }
    if !config.delta.exclude.is_empty() {
        lines.push(format!(
            "exclude = {}",
            format_toml_str_array(&config.delta.exclude)
        ));
    }

    for (mod_key, mod_cfg) in &config.delta.modules {
        if mod_cfg.is_empty() {
            continue;
        }
        let mut mod_lines = Vec::new();
        if let Some(rel) = mod_cfg.release {
            mod_lines.push(format!("release = {}", rel.number()));
        }
        if let Some(prev) = mod_cfg.preview {
            if prev {
                mod_lines.push(format!("preview = {prev}"));
            }
        }
        if !mod_cfg.sources.is_empty() {
            mod_lines.push(format!(
                "sources = {}",
                format_inline_delta_set(&mod_cfg.sources)
            ));
        }
        if !mod_cfg.resources.is_empty() {
            mod_lines.push(format!(
                "resources = {}",
                format_inline_delta_set(&mod_cfg.resources)
            ));
        }
        if !mod_cfg.dependencies.is_empty() {
            mod_lines.push(format!(
                "dependencies = {}",
                format_inline_delta_set(&mod_cfg.dependencies)
            ));
        }
        if !mod_lines.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(format!("[modules.\"{mod_key}\"]"));
            lines.extend(mod_lines);
        }
    }

    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn format_toml_str_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

fn format_inline_delta_set(delta: &DeltaSet<String>) -> String {
    let mut parts = Vec::new();
    if !delta.include.is_empty() {
        parts.push(format!(
            "include = {}",
            format_toml_str_array(&delta.include)
        ));
    }
    if !delta.exclude.is_empty() {
        parts.push(format!(
            "exclude = {}",
            format_toml_str_array(&delta.exclude)
        ));
    }
    format!("{{ {} }}", parts.join(", "))
}
