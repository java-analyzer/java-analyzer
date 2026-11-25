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

#![allow(dead_code)] // shared by several test crates, each using a subset
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration test harness helper module"
)]

pub mod sexp;

use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use sexp::Sexp;
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

    pub fn language_level(&self) -> syntax::LanguageLevel {
        parse_language_level(self.options.iter().map(String::as_str))
    }
}

fn parse_language_level<'a>(options: impl IntoIterator<Item = &'a str>) -> syntax::LanguageLevel {
    let mut release = syntax::Release::LATEST;
    let mut preview = false;
    for opt in options {
        if opt == "preview" {
            preview = true;
        } else if let Some(ver) = opt.strip_prefix("release=") {
            release = ver
                .parse()
                .unwrap_or_else(|e| panic!("invalid fixture option `{opt}`: {e}"));
        }
    }
    syntax::LanguageLevel::new(release, preview)
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
                None => {
                    return Err(format!(
                        "case {title:?} (line {header_line}): missing '---' separator after the source"
                    ));
                }
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

/// Loads every case of every fixture file under `test_data/<phase>`, with the file name.
pub fn load_cases(phase: &str) -> Vec<(String, Case)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(phase);
    let mut all = Vec::new();
    for path in fixture_files(&dir) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
        let cases = parse_cases(&text).unwrap_or_else(|e| panic!("{name}: malformed fixture: {e}"));
        all.extend(cases.into_iter().map(|case| (name.clone(), case)));
    }
    all
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
            // `!!` lines report violated invariants; they must never be baked into a fixture.
            let violated: Vec<&str> = cases
                .iter()
                .zip(&actual)
                .filter(|(_, lines)| lines.iter().any(|l| l.starts_with("!!")))
                .map(|(case, _)| case.title.as_str())
                .collect();
            if !violated.is_empty() {
                failures.push(format!(
                    "{name}: refusing to update, invariants violated in case(s): {violated:?}"
                ));
                continue;
            }
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

// ---- Paired fixtures: `name.java` + `name.scm` ----

/// One paired fixture: a Java source file and the expected tree next to it.
#[derive(Debug)]
pub struct PairCase {
    /// File stem, e.g. `01_package`.
    pub name: String,
    pub source: String,
    /// True when the expected file is `name.trivia.scm`: whitespace and comments are included
    /// in the expected tree.
    pub include_trivia: bool,
}

impl PairCase {
    /// Reads optional `release=<N>` and `preview` directives from a leading `// ...` comment on
    /// line 1 of the `.java` fixture, defaulting to [`syntax::LanguageLevel::latest`].
    pub fn language_level(&self) -> syntax::LanguageLevel {
        let Some(first_line) = self.source.lines().next() else {
            return syntax::LanguageLevel::latest();
        };
        let Some(comment) = first_line.trim_start().strip_prefix("//") else {
            return syntax::LanguageLevel::latest();
        };
        let words: Vec<&str> = comment.trim_start_matches('-').split_whitespace().collect();
        if words
            .iter()
            .all(|w| *w == "preview" || w.starts_with("release="))
        {
            parse_language_level(words)
        } else {
            syntax::LanguageLevel::latest()
        }
    }
}

struct PairFiles {
    stem: String,
    java: PathBuf,
    expected: Option<PathBuf>,
    include_trivia: bool,
}

/// Matches `.java` files with their `.scm` / `.trivia.scm` partners. The second result lists
/// structural problems (orphans, ambiguous partners).
fn discover_pairs(dir: &Path) -> (Vec<PairFiles>, Vec<String>) {
    let names: BTreeSet<String> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read fixture directory {}: {e}", dir.display()))
        .map(|entry| {
            entry
                .expect("readable directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    let mut pairs = Vec::new();
    let mut problems = Vec::new();
    for name in &names {
        if let Some(stem) = name.strip_suffix(".java") {
            let plain = format!("{stem}.scm");
            let trivia = format!("{stem}.trivia.scm");
            let (expected, include_trivia) = match (names.contains(&plain), names.contains(&trivia))
            {
                (true, true) => {
                    problems.push(format!("{stem}: both {plain} and {trivia} exist"));
                    continue;
                }
                (true, false) => (Some(dir.join(&plain)), false),
                (false, true) => (Some(dir.join(&trivia)), true),
                (false, false) => (None, false),
            };
            pairs.push(PairFiles {
                stem: stem.to_string(),
                java: dir.join(name),
                expected,
                include_trivia,
            });
        } else if let Some(stem) = name
            .strip_suffix(".trivia.scm")
            .or_else(|| name.strip_suffix(".scm"))
        {
            if !names.contains(&format!("{stem}.java")) {
                problems.push(format!("{name}: there is no matching {stem}.java"));
            }
        } else {
            problems.push(format!(
                "{name}: unexpected file (fixtures are .java and .scm pairs)"
            ));
        }
    }
    (pairs, problems)
}

fn read_pair_source(pair: &PairFiles) -> PairCase {
    let source = fs::read_to_string(&pair.java)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", pair.java.display()));
    PairCase {
        name: pair.stem.clone(),
        source,
        include_trivia: pair.include_trivia,
    }
}

/// Loads the sources of every pair under `test_data/<phase>`.
pub fn load_pairs(phase: &str) -> Vec<PairCase> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(phase);
    let (pairs, problems) = discover_pairs(&dir);
    assert!(
        problems.is_empty(),
        "malformed fixture directory: {problems:?}"
    );
    pairs.iter().map(read_pair_source).collect()
}

fn is_violation(form: &Sexp) -> bool {
    matches!(form, Sexp::List(items) if matches!(items.first(), Some(Sexp::Atom(a)) if a == "!!"))
}

/// Runs every `name.java` / `name.scm` pair under `test_data/<phase>`.
///
/// `render` turns a case into the S-expression forms that are compared with the expected file.
/// Both sides are compared in canonical printed form, so hand-written expectations may be laid
/// out freely. A form of the shape `(!! "message")` reports a violated invariant: it never
/// matches and is never written by `UPDATE_EXPECT=1`.
pub fn run_pair_fixtures(phase: &str, render: impl Fn(&PairCase) -> Vec<Sexp>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(phase);
    let update = std::env::var_os("UPDATE_EXPECT").is_some();
    let (pairs, mut failures) = discover_pairs(&dir);
    assert!(!pairs.is_empty(), "no fixtures found in {}", dir.display());
    let count = pairs.len();

    for pair in &pairs {
        let case = read_pair_source(pair);
        let forms = render(&case);
        let actual = format!("{}\n", sexp::print_all(&forms));
        let stem = &pair.stem;

        if update {
            if forms.iter().any(is_violation) {
                failures.push(format!(
                    "{stem}: refusing to update, an invariant is violated"
                ));
                continue;
            }
            let path = pair
                .expected
                .clone()
                .unwrap_or_else(|| dir.join(format!("{stem}.scm")));
            if fs::read_to_string(&path).ok().as_deref() != Some(actual.as_str()) {
                fs::write(&path, &actual)
                    .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
                eprintln!("updated {}", path.display());
            }
            continue;
        }

        let Some(path) = &pair.expected else {
            failures.push(format!(
                "{stem}.java has no expected tree: create {stem}.scm (or run with UPDATE_EXPECT=1 and review it)"
            ));
            continue;
        };
        let text = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let expected = match sexp::parse_all(&text) {
            Ok(forms) => format!("{}\n", sexp::print_all(&forms)),
            Err(e) => {
                failures.push(format!("{}: malformed S-expression: {e}", path.display()));
                continue;
            }
        };
        if expected != actual {
            let diff = TextDiff::from_lines(&expected, &actual)
                .unified_diff()
                .header("expected", "actual")
                .to_string();
            failures.push(format!(
                "{stem}.java does not match {}\n{diff}",
                path.display()
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} problem(s) in {count} fixture(s) of `{phase}` \
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
