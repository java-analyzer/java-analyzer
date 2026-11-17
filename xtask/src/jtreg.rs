//! jtreg header parser and parser-phase diagnostic allowlist classification for
//! `openjdk-langtools` (`test/langtools/tools/javac`).
//! See `docs/specs/0003-java-parser-conformance-testing.md`.

use std::collections::BTreeSet;

use syntax::{LanguageLevel, Release};

/// Derived expectation for a jtreg test file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expectation {
    /// `@compile` without `/fail`: must parse without errors.
    Valid,
    /// `@compile/fail/ref=*.out` whose `.out` file contains only parser-phase `compiler.err.*`
    /// codes: must produce at least one syntax error.
    Invalid,
    /// Not a syntax-only test (helper file, semantic `@compile/fail`, missing `.out`, etc.).
    Skipped(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JtregCase {
    pub expectation: Expectation,
    pub level: LanguageLevel,
}

/// Parses `parser_errors.txt` into a set of `compiler.err.*` diagnostic keys.
pub fn parse_allowlist(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Classifies a Java source file by its jtreg `@compile` tags and (for `@compile/fail/ref=*.out`)
/// the diagnostic codes in its referenced `.out` file.
pub fn classify(
    source: &str,
    mut read_ref: impl FnMut(&str) -> Option<String>,
    allowlist: &BTreeSet<String>,
) -> JtregCase {
    let directives = extract_compile_directives(source);
    let Some(first) = directives.first() else {
        return JtregCase {
            expectation: Expectation::Skipped("no @compile directive".to_string()),
            level: LanguageLevel::latest(),
        };
    };

    let level = parse_compile_level(&first.args);
    if !first.fail {
        if directives.iter().any(|d| d.fail) {
            return JtregCase {
                expectation: Expectation::Skipped(
                    "mixed @compile and @compile/fail directives".to_string(),
                ),
                level,
            };
        }
        return JtregCase {
            expectation: Expectation::Valid,
            level,
        };
    }

    let Some(ref_name) = &first.ref_out else {
        return JtregCase {
            expectation: Expectation::Skipped("@compile/fail without ref=*.out".to_string()),
            level,
        };
    };
    let Some(out_text) = read_ref(ref_name) else {
        return JtregCase {
            expectation: Expectation::Skipped(format!("missing reference file {ref_name}")),
            level,
        };
    };
    let codes = extract_error_codes(&out_text);
    if codes.is_empty() {
        return JtregCase {
            expectation: Expectation::Skipped(format!(
                "no compiler.err.* codes in {ref_name}"
            )),
            level,
        };
    }
    if let Some(non_parser) = codes.iter().find(|c| !allowlist.contains(c.as_str())) {
        return JtregCase {
            expectation: Expectation::Skipped(format!("non-parser error {non_parser}")),
            level,
        };
    }

    JtregCase {
        expectation: Expectation::Invalid,
        level,
    }
}

struct CompileDirective {
    fail: bool,
    ref_out: Option<String>,
    args: Vec<String>,
}

fn extract_compile_directives(source: &str) -> Vec<CompileDirective> {
    let mut out = Vec::new();
    let mut current: Option<CompileDirective> = None;

    for raw_line in source.lines() {
        let trimmed = raw_line
            .trim()
            .trim_start_matches("/*")
            .trim_end_matches("*/")
            .trim()
            .trim_start_matches('*')
            .trim();
        if let Some(rest) = trimmed.strip_prefix('@') {
            if let Some(prev) = current.take() {
                out.push(prev);
            }
            let mut words = rest.split_whitespace();
            let Some(tag) = words.next() else {
                continue;
            };
            if tag == "compile" || tag.starts_with("compile/") {
                let mut fail = false;
                let mut ref_out = None;
                for modifier in tag.split('/').skip(1) {
                    if modifier == "fail" {
                        fail = true;
                    } else if let Some(r) = modifier.strip_prefix("ref=") {
                        ref_out = Some(r.to_string());
                    }
                }
                current = Some(CompileDirective {
                    fail,
                    ref_out,
                    args: words.map(str::to_string).collect(),
                });
            }
        } else if let Some(active) = &mut current {
            if raw_line.contains("*/") {
                active.args.extend(trimmed.split_whitespace().map(str::to_string));
                out.push(current.take().unwrap());
            } else if raw_line.trim_start().starts_with('*') {
                active.args.extend(trimmed.split_whitespace().map(str::to_string));
            } else {
                out.push(current.take().unwrap());
            }
        }
    }
    if let Some(last) = current {
        out.push(last);
    }
    out
}

fn parse_compile_level(args: &[String]) -> LanguageLevel {
    let mut release = Release::LATEST;
    let mut preview = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--enable-preview" => preview = true,
            "--release" | "-source" | "--source" => {
                if let Some(ver) = args.get(i + 1) {
                    if let Ok(parsed) = ver.parse::<Release>() {
                        release = parsed;
                    }
                    i += 1;
                }
            }
            arg => {
                if let Some(ver) = arg.strip_prefix("--release=") {
                    if let Ok(parsed) = ver.parse::<Release>() {
                        release = parsed;
                    }
                }
            }
        }
        i += 1;
    }
    LanguageLevel::new(release, preview)
}

fn extract_error_codes(out_text: &str) -> Vec<String> {
    let mut codes = Vec::new();
    for line in out_text.lines() {
        for token in line.split(|c: char| c.is_whitespace() || c == ':' || c == ',') {
            if token.starts_with("compiler.err.") {
                codes.push(token.to_string());
            }
        }
    }
    codes
}
