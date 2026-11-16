//! Text-fixture test harness, shared by the lexer and (later) parser fixture tests.
//!
//! See "Test strategy" in `docs/specs/0002-java-syntax-subset.md` for the file format:
//!
//! ```text
//! =====
//! Case title
//! =====
//! <source code under test>
//! ---
//! <expected output, one item per line>
//! ```
//!
//! Run with `UPDATE_EXPECT=1` to rewrite the expected sections from the actual output.

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use similar::TextDiff;

#[derive(Debug)]
pub struct Case {
    pub title: String,
    /// 1-based line of the opening `=====` rule, for failure messages.
    pub line: usize,
    /// Source under test: every source line terminated by `\n`, or empty.
    pub source: String,
    /// Whitespace separated words following the dashes of the `---` separator.
    pub options: Vec<String>,
    /// Expected output lines (blank lines dropped, trailing whitespace trimmed).
    pub expected: Vec<String>,
}

impl Case {
    pub fn has_option(&self, name: &str) -> bool {
        self.options.iter().any(|o| o == name)
    }
}

fn is_header_rule(line: &str) -> bool {
    line.len() >= 3 && line.bytes().all(|b| b == b'=')
}

fn is_separator(line: &str) -> bool {
    let rest = line.trim_start_matches('-');
    line.len() - rest.len() >= 3 && (rest.is_empty() || rest.starts_with(char::is_whitespace))
}

pub fn parse_cases(text: &str) -> Result<Vec<Case>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut cases = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].trim().is_empty() {
            i += 1;
            continue;
        }
        if !is_header_rule(lines[i]) {
            return Err(format!("line {}: expected a '=====' case header", i + 1));
        }
        let header_line = i + 1;
        i += 1;

        let title = lines
            .get(i)
            .filter(|l| !is_header_rule(l) && !l.trim().is_empty())
            .ok_or_else(|| format!("line {}: expected a case title", i + 1))?
            .trim()
            .to_string();
        i += 1;

        if !lines.get(i).is_some_and(|l| is_header_rule(l)) {
            return Err(format!(
                "line {}: expected a '=====' rule closing the title",
                i + 1
            ));
        }
        i += 1;

        let mut source = String::new();
        let options = loop {
            match lines.get(i) {
                None => return Err(format!(
                    "case {title:?} (line {header_line}): missing '---' separator after the source"
                )),
                Some(l) if is_separator(l) => {
                    i += 1;
                    break l
                        .trim_start_matches('-')
                        .split_whitespace()
                        .map(str::to_string)
                        .collect();
                }
                Some(l) => {
                    source.push_str(l);
                    source.push('\n');
                    i += 1;
                }
            }
        };

        let mut expected = Vec::new();
        while let Some(l) = lines.get(i) {
            if is_header_rule(l) {
                break;
            }
            if !l.trim().is_empty() {
                expected.push(l.trim_end().to_string());
            }
            i += 1;
        }

        cases.push(Case {
            title,
            line: header_line,
            source,
            options,
            expected,
        });
    }

    Ok(cases)
}

fn render_file(cases: &[Case], actual: &[Vec<String>]) -> String {
    let mut out = String::new();
    for (n, (case, lines)) in cases.iter().zip(actual).enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let _ = writeln!(out, "=====\n{}\n=====", case.title);
        out.push_str(&case.source);
        if case.options.is_empty() {
            out.push_str("---\n");
        } else {
            let _ = writeln!(out, "--- {}", case.options.join(" "));
        }
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn fixture_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read fixture directory {}: {e}", dir.display()))
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "txt"))
        .collect();
    files.sort();
    files
}

/// Runs every fixture under `test_data/<phase>`, rendering each case with `render`.
///
/// All mismatches are collected and reported together, so one failing fixture does not hide
/// the others.
pub fn run_fixtures(phase: &str, render: impl Fn(&Case) -> Vec<String>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(phase);
    let update = std::env::var_os("UPDATE_EXPECT").is_some();
    let files = fixture_files(&dir);
    assert!(!files.is_empty(), "no fixtures found in {}", dir.display());

    let mut failures = Vec::new();
    let mut case_count = 0;

    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
        let cases = match parse_cases(&text) {
            Ok(cases) => cases,
            Err(e) => {
                failures.push(format!("{name}: malformed fixture: {e}"));
                continue;
            }
        };
        if cases.is_empty() {
            failures.push(format!("{name}: fixture contains no cases"));
            continue;
        }

        let actual: Vec<Vec<String>> = cases.iter().map(&render).collect();
        case_count += cases.len();

        if update {
            let new_text = render_file(&cases, &actual);
            if new_text != text {
                fs::write(path, new_text).unwrap_or_else(|e| panic!("cannot write {name}: {e}"));
                eprintln!("updated {name}");
            }
            continue;
        }

        for (case, actual) in cases.iter().zip(&actual) {
            if &case.expected != actual {
                let expected = case.expected.join("\n");
                let actual = actual.join("\n");
                let diff = TextDiff::from_lines(&expected, &actual)
                    .unified_diff()
                    .header("expected", "actual")
                    .to_string();
                failures.push(format!(
                    "{name}:{} case {:?} does not match\n{diff}",
                    case.line, case.title
                ));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} of {case_count} fixture case(s) failed \
             (re-run with UPDATE_EXPECT=1 to regenerate after reviewing):\n\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

/// 1-based `(line, column)` of byte `offset` in `text`; columns count `char`s.
pub fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[line_start..].chars().count() + 1)
}
