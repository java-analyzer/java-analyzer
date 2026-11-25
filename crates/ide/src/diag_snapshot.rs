//! Tier 1 `rustc`-style ASCII caret snapshot renderer and automated UX invariant checker.
//!
//! See `docs/specs/0020-diagnostic-conformance-testing.md`.

use std::fmt::Write as _;

use hir::FileId;
use line_index::{LineIndex, TextSize};

use crate::{Diagnostic, Severity};

/// Diagnostic codes that must attach at least one [`crate::RelatedInformation`] entry.
pub const REQUIRES_RELATED_CODES: &[&str] = &[
    "type/incompatible-types",
    "symbol/duplicate-declaration",
    "type/incompatible-override",
    "resolve/ambiguous-call",
];

const LEAKED_SYNTAX_KINDS: &[&str] = &[
    "SEMI",
    "L_PAREN",
    "R_PAREN",
    "L_BRACE",
    "R_BRACE",
    "L_BRACK",
    "R_BRACK",
    "IDENT",
    "EOF",
    "WHITESPACE",
];

const LEAKED_DEBUG_FRAGMENTS: &[&str] = &["Some(", "Ty::", "SmolStr", "SyntaxKind::"];

/// A structural UX quality violation detected on an emitted [`Diagnostic`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UxViolation {
    pub code: &'static str,
    pub reason: String,
}

/// Checks all automated Tier 1 UX invariants on `diags` produced for `text`.
pub fn check_ux_invariants(text: &str, diags: &[Diagnostic]) -> Vec<UxViolation> {
    let mut violations = Vec::new();

    for diag in diags {
        check_text_cleanliness(diag.code, "primary message", &diag.message, &mut violations);
        if let Some(help) = &diag.help {
            check_text_cleanliness(diag.code, "help text", help, &mut violations);
        }
        for rel in &diag.related {
            check_text_cleanliness(
                diag.code,
                "related info message",
                &rel.message,
                &mut violations,
            );
        }

        if !is_valid_diagnostic_code(diag.code) {
            violations.push(UxViolation {
                code: diag.code,
                reason: format!(
                    "diagnostic code `{}` must follow `<category>/<slug>` in lowercase ASCII",
                    diag.code
                ),
            });
        }

        let trimmed_msg = diag.message.trim();
        if trimmed_msg.is_empty() {
            violations.push(UxViolation {
                code: diag.code,
                reason: "primary message must not be empty".to_string(),
            });
        } else if trimmed_msg.ends_with('.') && !trimmed_msg.contains(". ") {
            violations.push(UxViolation {
                code: diag.code,
                reason: format!(
                    "single-sentence primary message must not end with a period: {:?}",
                    diag.message
                ),
            });
        }

        let start = usize::from(diag.range.start());
        let end = usize::from(diag.range.end());
        if start > end || end > text.len() {
            violations.push(UxViolation {
                code: diag.code,
                reason: format!(
                    "span {start}..{end} is out of bounds for source length {}",
                    text.len()
                ),
            });
        } else if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            violations.push(UxViolation {
                code: diag.code,
                reason: format!("span {start}..{end} does not align to UTF-8 char boundaries"),
            });
        } else if start == end && start != text.len() && !allows_empty_caret(diag) {
            violations.push(UxViolation {
                code: diag.code,
                reason: format!(
                    "span {start}..{end} is empty before EOF ({})",
                    text.len()
                ),
            });
        }

        if REQUIRES_RELATED_CODES.contains(&diag.code) && diag.related.is_empty() {
            violations.push(UxViolation {
                code: diag.code,
                reason: format!(
                    "diagnostic `{}` requires at least one RelatedInformation entry",
                    diag.code
                ),
            });
        }
    }

    violations
}

fn allows_empty_caret(diag: &Diagnostic) -> bool {
    diag.code == "syntax/expected-token"
        || (diag.code == "syntax/illegal-start" && diag.message.starts_with("illegal start of "))
}

fn is_valid_diagnostic_code(code: &str) -> bool {
    let Some((category, slug)) = code.split_once('/') else {
        return false;
    };
    !slug.contains('/') && is_kebab_segment(category) && is_kebab_segment(slug)
}

fn is_kebab_segment(seg: &str) -> bool {
    !seg.is_empty()
        && !seg.starts_with('-')
        && !seg.ends_with('-')
        && seg
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn check_text_cleanliness(
    code: &'static str,
    field: &str,
    value: &str,
    out: &mut Vec<UxViolation>,
) {
    for token in LEAKED_SYNTAX_KINDS {
        if contains_whole_word(value, token) {
            out.push(UxViolation {
                code,
                reason: format!("{field} leaked internal SyntaxKind `{token}`: {value:?}"),
            });
        }
    }
    for fragment in LEAKED_DEBUG_FRAGMENTS {
        if value.contains(fragment) {
            out.push(UxViolation {
                code,
                reason: format!("{field} leaked internal Rust debug fragment `{fragment}`: {value:?}"),
            });
        }
    }
}

fn contains_whole_word(haystack: &str, word: &str) -> bool {
    let mut offset = 0;
    while let Some(rel_idx) = haystack.get(offset..).and_then(|s| s.find(word)) {
        let idx = offset + rel_idx;
        let before_ok = haystack
            .get(..idx)
            .and_then(|s| s.chars().next_back())
            .is_none_or(|c| !is_ident_char(c));
        let after_idx = idx + word.len();
        let after_ok = haystack
            .get(after_idx..)
            .and_then(|s| s.chars().next())
            .is_none_or(|c| !is_ident_char(c));
        if before_ok && after_ok {
            return true;
        }
        offset = idx + word.len();
    }
    false
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Renders `diags` against `text` in a deterministic `rustc`-style ASCII caret format.
pub fn render_diagnostics_snapshot(
    file_name: &str,
    text: &str,
    diags: &[Diagnostic],
    resolve_file: impl Fn(FileId) -> Option<(String, String)>,
) -> String {
    if diags.is_empty() {
        return String::new();
    }

    let mut ordered: Vec<&Diagnostic> = diags.iter().collect();
    ordered.sort_by(|a, b| {
        (
            a.range.start(),
            a.range.end(),
            a.code,
            a.message.as_str(),
        )
            .cmp(&(
                b.range.start(),
                b.range.end(),
                b.code,
                b.message.as_str(),
            ))
    });

    let primary_index = LineIndex::new(text);
    let mut out = String::new();

    for (idx, diag) in ordered.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let sev = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        let _ = writeln!(out, "{sev}[{}]: {}", diag.code, diag.message);
        render_snippet(
            &mut out,
            file_name,
            text,
            &primary_index,
            diag.range,
            diag.help.as_deref(),
        );

        for rel in &diag.related {
            let _ = writeln!(out, "  note: {}", rel.message);
            if let Some((rel_name, rel_text)) = resolve_file(rel.file_id) {
                let rel_index = LineIndex::new(&rel_text);
                render_snippet(&mut out, &rel_name, &rel_text, &rel_index, rel.range, None);
            }
        }
    }

    out
}

fn render_snippet(
    out: &mut String,
    file_name: &str,
    text: &str,
    index: &LineIndex,
    range: crate::TextRange,
    help: Option<&str>,
) {
    let start = clamp_to_char_boundary(text, usize::from(range.start()));
    let end = clamp_to_char_boundary(text, usize::from(range.end())).max(start);

    let start_lc = index.line_col(TextSize::from(start as u32));
    let mut end_lc = index.line_col(TextSize::from(end as u32));

    // If a non-empty multi-line span ends right at column 0 of the next line (trailing `\n`),
    // trim the visual underline back to the end of the previous line.
    if end > start && end_lc.line > start_lc.line && end_lc.col == 0 {
        let prev_line = end_lc.line - 1;
        let prev_text = line_without_newline(text, index, prev_line);
        end_lc.line = prev_line;
        end_lc.col = prev_text.len() as u32;
    }

    let start_line_1 = start_lc.line + 1;
    let end_line_1 = end_lc.line + 1;
    let start_line_str = line_without_newline(text, index, start_lc.line);
    let start_col_1 = safe_char_count(start_line_str, start_lc.col as usize) + 1;

    let gutter_width = end_line_1.to_string().len();
    let margin = " ".repeat(gutter_width);

    let _ = writeln!(
        out,
        " {margin}--> {file_name}:{start_line_1}:{start_col_1}"
    );
    let _ = writeln!(out, " {margin} |");

    if start_lc.line == end_lc.line {
        let start_chars = safe_char_count(start_line_str, start_lc.col as usize);
        let end_chars = safe_char_count(start_line_str, end_lc.col as usize);
        let carets = end_chars.saturating_sub(start_chars).max(1);
        let spaces = " ".repeat(start_chars);
        let markers = "^".repeat(carets);
        let _ = writeln!(out, " {start_line_1:>gutter_width$} | {start_line_str}");
        let _ = writeln!(out, " {margin} | {spaces}{markers}");
    } else {
        let total_lines = end_lc.line - start_lc.line + 1;
        let show_all = total_lines <= 4;

        let first_start = safe_char_count(start_line_str, start_lc.col as usize);
        let first_len = start_line_str.chars().count();
        let first_carets = first_len.saturating_sub(first_start).max(1);
        let first_spaces = " ".repeat(first_start);
        let first_markers = "^".repeat(first_carets);
        let _ = writeln!(out, " {start_line_1:>gutter_width$} | {start_line_str}");
        let _ = writeln!(out, " {margin} | {first_spaces}{first_markers}");

        if show_all {
            for line_idx in (start_lc.line + 1)..end_lc.line {
                let mid = line_without_newline(text, index, line_idx);
                let mid_carets = mid.chars().count().max(1);
                let mid_markers = "^".repeat(mid_carets);
                let _ = writeln!(out, " {:>gutter_width$} | {mid}", line_idx + 1);
                let _ = writeln!(out, " {margin} | {mid_markers}");
            }
        } else {
            let _ = writeln!(out, " {margin} | ...");
        }

        let end_line_str = line_without_newline(text, index, end_lc.line);
        let end_carets = safe_char_count(end_line_str, end_lc.col as usize).max(1);
        let end_markers = "^".repeat(end_carets);
        let _ = writeln!(out, " {end_line_1:>gutter_width$} | {end_line_str}");
        let _ = writeln!(out, " {margin} | {end_markers}");
    }

    if let Some(help) = help {
        let _ = writeln!(out, " {margin} |");
        let _ = writeln!(out, " {margin} = help: {help}");
    }
}

fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
    let mut off = offset.min(text.len());
    while off > 0 && !text.is_char_boundary(off) {
        off -= 1;
    }
    off
}

fn line_without_newline<'a>(text: &'a str, index: &LineIndex, line: u32) -> &'a str {
    let Some(range) = index.line(line) else {
        return "";
    };
    let start = usize::from(range.start()).min(text.len());
    let end = usize::from(range.end()).min(text.len());
    let raw = text.get(start..end).unwrap_or("");
    let stripped = raw.strip_suffix('\n').unwrap_or(raw);
    stripped.strip_suffix('\r').unwrap_or(stripped)
}

fn safe_char_count(line: &str, byte_col: usize) -> usize {
    let mut col = byte_col.min(line.len());
    while col > 0 && !line.is_char_boundary(col) {
        col -= 1;
    }
    line.get(..col).map_or(0, |s| s.chars().count())
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use hir::{LanguageLevel, Release};
    use syntax::TextRange;

    use super::*;
    use crate::{AnalysisHost, RelatedInformation};

    fn parse_fixture_language_level(text: &str) -> LanguageLevel {
        let Some(first_line) = text.lines().next() else {
            return LanguageLevel::latest();
        };
        let Some(comment) = first_line.trim().strip_prefix("//") else {
            return LanguageLevel::latest();
        };
        let mut release = Release::LATEST;
        let mut preview = false;
        let mut saw_option = false;
        for opt in comment.split_whitespace() {
            if opt == "preview" {
                preview = true;
                saw_option = true;
            } else if let Some(ver) = opt.strip_prefix("release=") {
                release = ver
                    .parse::<Release>()
                    .unwrap_or_else(|e| panic!("invalid fixture release `{opt}`: {e}"));
                saw_option = true;
            }
        }
        if saw_option {
            LanguageLevel::new(release, preview)
        } else {
            LanguageLevel::latest()
        }
    }

    #[test]
    fn ux_invariants_catch_all_violation_classes() {
        let src = "class Main { String s = \"hi\"; }";

        // 1. Leaked SyntaxKind and Rust debug tokens.
        let leaked_kind = Diagnostic {
            code: "syntax/expected-token",
            range: TextRange::new(0.into(), 5.into()),
            severity: Severity::Error,
            message: "expected SEMI after IDENT".to_string(),
            related: vec![],
            help: Some("Some(Ty::Int) SmolStr SyntaxKind::SEMI".to_string()),
        };
        let v1 = check_ux_invariants(src, &[leaked_kind]);
        assert!(v1.iter().any(|v| v.reason.contains("`SEMI`")), "{v1:?}");
        assert!(v1.iter().any(|v| v.reason.contains("`IDENT`")), "{v1:?}");
        assert!(v1.iter().any(|v| v.reason.contains("`Some(`")), "{v1:?}");
        assert!(v1.iter().any(|v| v.reason.contains("`Ty::`")), "{v1:?}");
        assert!(v1.iter().any(|v| v.reason.contains("`SmolStr`")), "{v1:?}");
        assert!(
            v1.iter().any(|v| v.reason.contains("`SyntaxKind::`")),
            "{v1:?}"
        );

        // 2. Trailing period, empty message, invalid code.
        let bad_msg = Diagnostic {
            code: "BadCode",
            range: TextRange::new(0.into(), 5.into()),
            severity: Severity::Error,
            message: "semicolon expected.".to_string(),
            related: vec![],
            help: None,
        };
        let v2 = check_ux_invariants(src, &[bad_msg]);
        assert!(
            v2.iter().any(|v| v.reason.contains("<category>/<slug>")),
            "{v2:?}"
        );
        assert!(
            v2.iter().any(|v| v.reason.contains("period")),
            "{v2:?}"
        );

        // 3. Empty non-EOF span on non-insertion diagnostic.
        let empty_non_eof = Diagnostic {
            code: "syntax/unclosed-literal",
            range: TextRange::empty(10.into()),
            severity: Severity::Error,
            message: "unclosed string literal".to_string(),
            related: vec![],
            help: None,
        };
        let v3 = check_ux_invariants(src, &[empty_non_eof]);
        assert!(
            v3.iter().any(|v| v.reason.contains("empty before EOF")),
            "{v3:?}"
        );

        // Empty span at true EOF is allowed.
        let empty_eof = Diagnostic {
            code: "syntax/unclosed-literal",
            range: TextRange::empty((src.len() as u32).into()),
            severity: Severity::Error,
            message: "unclosed string literal".to_string(),
            related: vec![],
            help: None,
        };
        assert!(check_ux_invariants(src, &[empty_eof]).is_empty());

        // 4. Multi-site completeness check.
        let missing_related = Diagnostic {
            code: "type/incompatible-types",
            range: TextRange::new(13.into(), 19.into()),
            severity: Severity::Error,
            message: "incompatible types: String cannot be converted to int".to_string(),
            related: vec![],
            help: None,
        };
        let v4 = check_ux_invariants(src, &[missing_related]);
        assert!(
            v4.iter()
                .any(|v| v.reason.contains("RelatedInformation")),
            "{v4:?}"
        );
    }

    #[test]
    fn render_snapshot_formats_related_and_utf8_carets() {
        let main_src = "class Main {\n    /*😀*/ int x = getName();\n}\n";
        let helper_src = "class Helper {\n    String getName() { return \"hi\"; }\n}\n";
        let start = main_src.find("getName()").unwrap() as u32;
        let end = start + "getName()".len() as u32;
        let rel_start = helper_src.find("getName").unwrap() as u32;
        let rel_end = rel_start + "getName".len() as u32;

        let diag = Diagnostic {
            code: "type/incompatible-types",
            range: TextRange::new(start.into(), end.into()),
            severity: Severity::Error,
            message: "incompatible types: String cannot be converted to int".to_string(),
            related: vec![RelatedInformation {
                file_id: FileId(2),
                range: TextRange::new(rel_start.into(), rel_end.into()),
                message: "`getName()` returns `String` here".to_string(),
            }],
            help: Some("change the type of `x` to `String`".to_string()),
        };

        assert!(check_ux_invariants(main_src, std::slice::from_ref(&diag)).is_empty());

        let rendered = render_diagnostics_snapshot("Main.java", main_src, &[diag], |fid| {
            (fid == FileId(2)).then(|| ("Helper.java".to_string(), helper_src.to_string()))
        });
        assert!(rendered.contains("error[type/incompatible-types]: incompatible types: String cannot be converted to int"));
        assert!(rendered.contains("Main.java:2:19"), "rendered:\n{rendered}");
        assert!(rendered.contains("^^^^^^^^^"));
        assert!(rendered.contains("note: `getName()` returns `String` here"));
        assert!(rendered.contains("Helper.java:2:12"));
        assert!(rendered.contains("= help: change the type of `x` to `String`"));
    }

    #[test]
    fn syntax_diagnostic_snapshots() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data/diagnostics/syntax");
        let mut java_files: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension().and_then(|e| e.to_str()) == Some("java")).then_some(path)
            })
            .collect();
        java_files.sort();
        assert!(
            !java_files.is_empty(),
            "no .java fixtures found in {}",
            dir.display()
        );

        let update = env::var_os("UPDATE_EXPECT").is_some();

        for java_path in java_files {
            let text = fs::read_to_string(&java_path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", java_path.display()));
            let level = parse_fixture_language_level(&text);

            let mut host = AnalysisHost::new();
            let file_id = FileId(0);
            host.set_file_language_level(file_id, level);
            host.set_file_text(file_id, Arc::new(text.clone()));
            host.set_workspace_files(Arc::new(vec![file_id]));

            let analysis = host.analysis();
            let diags = analysis.diagnostics(file_id).unwrap();

            let violations = check_ux_invariants(&text, &diags);
            assert!(
                violations.is_empty(),
                "UX invariant violations in {}:\n{violations:#?}",
                java_path.display()
            );

            let file_name = java_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Main.java");
            let actual = render_diagnostics_snapshot(file_name, &text, &diags, |fid| {
                (fid == file_id).then(|| (file_name.to_string(), text.clone()))
            });

            let snap_path = java_path.with_extension("snap");
            if update {
                fs::write(&snap_path, &actual)
                    .unwrap_or_else(|e| panic!("failed to write {}: {e}", snap_path.display()));
            } else {
                let expected = fs::read_to_string(&snap_path).unwrap_or_else(|e| {
                    panic!(
                        "missing snapshot {} (run with UPDATE_EXPECT=1): {e}",
                        snap_path.display()
                    )
                });
                assert_eq!(
                    actual, expected,
                    "snapshot mismatch for {} (run with UPDATE_EXPECT=1 to update)",
                    snap_path.display()
                );
            }
        }
    }
}
