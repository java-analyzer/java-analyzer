//! The conformance runner: parses every `.java` file of the given source directories, checks the
//! parser's invariants on it, and reports how much of the valid code it accepts and how much of
//! the invalid code it rejects. Report only: nothing here decides whether a run "passes".
//! See `docs/specs/0003-java-parser-conformance-testing.md`.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::isolate::{Isolated, run_isolated};

/// What the files of a source are expected to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Must parse without errors.
    Valid,
    /// Must produce at least one error.
    Invalid,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Valid => "valid",
            Kind::Invalid => "invalid",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Source {
    /// Name shown in the report.
    pub id: String,
    pub dir: PathBuf,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownDivergence {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub sources: Vec<Source>,
    pub level: syntax::LanguageLevel,
    /// Per-file limit for parsing. A file that exceeds it is a critical finding.
    pub timeout: Duration,
    /// Known gaps excluded from unexpected divergence counts.
    pub known_divergences: Vec<KnownDivergence>,
}

/// Parses `known-divergences.toml` (`[[divergence]]` entries with `path` and `reason` strings).
pub fn parse_known_divergences(text: &str) -> Result<Vec<KnownDivergence>, String> {
    let mut list = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_reason: Option<String> = None;
    let mut in_entry = false;

    let flush = |list: &mut Vec<KnownDivergence>,
                 path: &mut Option<String>,
                 reason: &mut Option<String>|
     -> Result<(), String> {
        let Some(p) = path.take() else {
            return Err("[[divergence]] entry is missing `path`".to_string());
        };
        let Some(r) = reason.take() else {
            return Err(format!("[[divergence]] for `{p}` is missing `reason`"));
        };
        list.push(KnownDivergence { path: p, reason: r });
        Ok(())
    };

    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[divergence]]" {
            if in_entry {
                flush(&mut list, &mut current_path, &mut current_reason)?;
            }
            in_entry = true;
            continue;
        }
        if !in_entry {
            return Err(format!("line {}: expected `[[divergence]]`", idx + 1));
        }
        let Some((key, val)) = line.split_once('=') else {
            return Err(format!("line {}: expected `key = \"value\"`", idx + 1));
        };
        let key = key.trim();
        let val = val
            .trim()
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .ok_or_else(|| format!("line {}: expected quoted string value", idx + 1))?;
        match key {
            "path" => current_path = Some(val.to_string()),
            "reason" => current_reason = Some(val.to_string()),
            other => return Err(format!("line {}: unknown key `{other}`", idx + 1)),
        }
    }
    if in_entry {
        flush(&mut list, &mut current_path, &mut current_reason)?;
    }
    Ok(list)
}

/// How a single file went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Parsed without errors.
    Clean,
    /// Parsed with these errors (`line:col message`, in file order, at most `MAX_ERRORS_KEPT`).
    Errors { count: usize, first: Vec<String> },
    /// The parser panicked.
    Panicked(String),
    /// The parser did not finish within the timeout.
    TimedOut,
    /// The tree's text differs from the file's text.
    RoundtripFailed,
}

const MAX_ERRORS_KEPT: usize = 3;

#[derive(Debug, Clone)]
pub struct FileResult {
    /// Path relative to the source directory, with `/` separators.
    pub path: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone)]
pub struct SourceReport {
    pub id: String,
    pub dir: PathBuf,
    pub kind: Kind,
    /// Why the whole source was not run (missing directory, nothing found).
    pub skipped: Option<String>,
    pub results: Vec<FileResult>,
    /// Files that were not parsed, with the reason.
    pub skipped_files: Vec<(String, String)>,
    /// Known divergences configured for this run.
    pub known: Vec<KnownDivergence>,
}

impl SourceReport {
    fn files(&self) -> usize {
        self.results.len()
    }

    fn known_reason(&self, path: &str) -> Option<&str> {
        self.known
            .iter()
            .find(|k| path == k.path || path.ends_with(&format!("/{}", k.path)))
            .map(|k| k.reason.as_str())
    }

    /// Findings that mean a parser bug whatever the file is (panic, hang, lossy tree).
    pub fn critical(&self) -> impl Iterator<Item = &FileResult> {
        self.results.iter().filter(|r| {
            matches!(
                r.outcome,
                Outcome::Panicked(_) | Outcome::TimedOut | Outcome::RoundtripFailed
            )
        })
    }

    /// Files that behaved as the source's kind demands.
    pub fn conforming(&self) -> usize {
        self.results
            .iter()
            .filter(|r| {
                matches!(
                    (&r.outcome, self.kind),
                    (Outcome::Clean, Kind::Valid) | (Outcome::Errors { .. }, Kind::Invalid)
                )
            })
            .count()
    }

    /// Unexpected gaps: valid files with errors, invalid files without, excluding entries in
    /// `known-divergences.toml`. Critical findings are counted separately.
    pub fn gaps(&self) -> impl Iterator<Item = &FileResult> {
        self.results.iter().filter(move |r| {
            let is_gap = matches!(
                (&r.outcome, self.kind),
                (Outcome::Errors { .. }, Kind::Valid) | (Outcome::Clean, Kind::Invalid)
            );
            is_gap && self.known_reason(&r.path).is_none()
        })
    }

    /// Gaps that match an entry in `known-divergences.toml`, paired with their justification.
    pub fn known_divergences(&self) -> impl Iterator<Item = (&FileResult, &str)> {
        self.results.iter().filter_map(move |r| {
            let is_gap = matches!(
                (&r.outcome, self.kind),
                (Outcome::Errors { .. }, Kind::Valid) | (Outcome::Clean, Kind::Invalid)
            );
            is_gap
                .then(|| self.known_reason(&r.path).map(|reason| (r, reason)))
                .flatten()
        })
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    pub sources: Vec<SourceReport>,
}

impl Report {
    pub fn critical_count(&self) -> usize {
        self.sources.iter().map(|s| s.critical().count()).sum()
    }
}

pub fn run(config: &Config) -> Report {
    Report {
        sources: config
            .sources
            .iter()
            .map(|s| run_source(s, config.level, config.timeout, &config.known_divergences))
            .collect(),
    }
}

fn run_source(
    source: &Source,
    level: syntax::LanguageLevel,
    timeout: Duration,
    known_divergences: &[KnownDivergence],
) -> SourceReport {
    let mut report = SourceReport {
        id: source.id.clone(),
        dir: source.dir.clone(),
        kind: source.kind,
        skipped: None,
        results: Vec::new(),
        skipped_files: Vec::new(),
        known: known_divergences.to_vec(),
    };
    if !source.dir.is_dir() {
        report.skipped = Some(format!("{} is not a directory", source.dir.display()));
        return report;
    }

    let mut files = Vec::new();
    if let Err(e) = collect_java_files(&source.dir, &mut files) {
        report.skipped = Some(format!("cannot read {}: {e}", source.dir.display()));
        return report;
    }
    files.sort();
    if files.is_empty() {
        report.skipped = Some(format!("no .java files under {}", source.dir.display()));
        return report;
    }

    for path in files {
        let relative = path
            .strip_prefix(&source.dir)
            .unwrap_or(&path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        match fs::read(&path) {
            Err(e) => report
                .skipped_files
                .push((relative, format!("cannot read: {e}"))),
            Ok(bytes) => match String::from_utf8(bytes) {
                Err(_) => report
                    .skipped_files
                    .push((relative, "not valid UTF-8".to_string())),
                Ok(text) => {
                    let outcome = check_file_with_level(text, level, timeout);
                    report.results.push(FileResult {
                        path: relative,
                        outcome,
                    });
                }
            },
        }
    }
    report
}

/// Collects `.java` files below `dir`. Symbolic links are not followed, so a link cycle cannot
/// hang the walk.
fn collect_java_files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_java_files(&path, out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|e| e == "java") {
            out.push(path);
        }
    }
    Ok(())
}

/// Parses `text` at [`syntax::LanguageLevel::latest`] in isolation and checks the parser's
/// invariants.
pub fn check_file(text: String, timeout: Duration) -> Outcome {
    check_file_with_level(text, syntax::LanguageLevel::latest(), timeout)
}

/// Parses `text` at `level` in isolation and checks the parser's invariants.
pub fn check_file_with_level(
    text: String,
    level: syntax::LanguageLevel,
    timeout: Duration,
) -> Outcome {
    let isolated = run_isolated(timeout, move || {
        let parse = syntax::parse_with_level(&text, level);
        if parse.syntax_node().text().to_string() != text {
            return Outcome::RoundtripFailed;
        }
        let errors = parse.errors();
        if errors.is_empty() {
            return Outcome::Clean;
        }
        let first = errors
            .iter()
            .take(MAX_ERRORS_KEPT)
            .map(|e| {
                let (line, col) = line_col(&text, usize::from(e.range.start()));
                format!("{line}:{col} {}", e.message)
            })
            .collect();
        Outcome::Errors {
            count: errors.len(),
            first,
        }
    });
    match isolated {
        Isolated::Done(outcome) => outcome,
        Isolated::Panicked(message) => Outcome::Panicked(message),
        Isolated::TimedOut => Outcome::TimedOut,
    }
}

/// 1-based line and column (counted in `char`s) of a byte offset.
fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[line_start..].chars().count() + 1)
}

// ---- Report output ----

pub const REPORT_SCHEMA: u64 = 1;

pub fn to_json(report: &Report) -> Value {
    let sources: Vec<Value> = report
        .sources
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "dir": s.dir.display().to_string(),
                "kind": s.kind.as_str(),
                "skipped": s.skipped,
                "files": s.files(),
                "conforming": s.conforming(),
                "critical": s.critical().map(file_json).collect::<Vec<_>>(),
                "gaps": s.gaps().map(file_json).collect::<Vec<_>>(),
                "knownDivergences": s.known_divergences()
                    .map(|(r, reason)| json!({ "path": r.path, "reason": reason }))
                    .collect::<Vec<_>>(),
                "skippedFiles": s.skipped_files.iter()
                    .map(|(path, reason)| json!({ "path": path, "reason": reason }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "schema": REPORT_SCHEMA, "sources": sources })
}

fn file_json(result: &FileResult) -> Value {
    match &result.outcome {
        Outcome::Clean => json!({ "path": result.path, "outcome": "clean" }),
        Outcome::Errors { count, first } => {
            json!({ "path": result.path, "outcome": "errors", "errors": count, "first": first })
        }
        Outcome::Panicked(message) => {
            json!({ "path": result.path, "outcome": "panic", "message": message })
        }
        Outcome::TimedOut => json!({ "path": result.path, "outcome": "timeout" }),
        Outcome::RoundtripFailed => json!({ "path": result.path, "outcome": "roundtrip" }),
    }
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        "n/a".to_string()
    } else {
        format!("{:.1}%", part as f64 * 100.0 / whole as f64)
    }
}

/// How many entries of a list the Markdown report shows before saying how many more there are.
const MD_LIST_LIMIT: usize = 25;

pub fn to_markdown(report: &Report) -> String {
    let mut out = String::from("# Java parser conformance report\n\n");
    out.push_str("Report only: this run never fails on conformance results.\n\n");

    let critical = report.critical_count();
    if critical > 0 {
        out.push_str(&format!("## Critical findings: {critical}\n\n"));
        out.push_str(
            "Panics, timeouts and lossy trees are parser bugs whatever the file contains.\n\n",
        );
        for source in &report.sources {
            for r in source.critical() {
                out.push_str(&format!(
                    "- `{}` / `{}`: {}\n",
                    source.id,
                    r.path,
                    describe(&r.outcome)
                ));
            }
        }
        out.push('\n');
    } else {
        out.push_str("## Critical findings: none\n\n");
    }

    out.push_str("## Summary\n\n");
    out.push_str("| Source | Kind | Files | Conforming | Rate | Critical | Skipped |\n");
    out.push_str("|---|---|---:|---:|---:|---:|---|\n");
    for s in &report.sources {
        let skipped = match (&s.skipped, s.skipped_files.len()) {
            (Some(reason), _) => format!("source: {reason}"),
            (None, 0) => String::new(),
            (None, n) => format!("{n} file(s)"),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            s.id,
            s.kind.as_str(),
            s.files(),
            s.conforming(),
            percent(s.conforming(), s.files()),
            s.critical().count(),
            skipped
        ));
    }
    out.push('\n');

    for s in &report.sources {
        if s.skipped.is_some() || s.files() == 0 {
            continue;
        }
        out.push_str(&format!("## {} ({})\n\n", s.id, s.kind.as_str()));
        let gaps: Vec<&FileResult> = s.gaps().collect();
        let (heading, none) = match s.kind {
            Kind::Valid => (
                "Valid files that produced errors (completeness gaps)",
                "Every valid file parsed cleanly.",
            ),
            Kind::Invalid => (
                "Invalid files that parsed cleanly (compliance gaps)",
                "Every invalid file was rejected.",
            ),
        };
        if gaps.is_empty() {
            out.push_str(&format!("{none}\n\n"));
        } else {
            out.push_str(&format!("{heading}: {}\n\n", gaps.len()));
            for r in gaps.iter().take(MD_LIST_LIMIT) {
                out.push_str(&format!("- `{}`: {}\n", r.path, describe(&r.outcome)));
            }
            if gaps.len() > MD_LIST_LIMIT {
                out.push_str(&format!("- ... and {} more\n", gaps.len() - MD_LIST_LIMIT));
            }
            out.push('\n');
            if s.kind == Kind::Valid {
                push_message_histogram(&mut out, &gaps);
            }
        }
        let known: Vec<_> = s.known_divergences().collect();
        if !known.is_empty() {
            out.push_str(&format!(
                "Known divergences excluded (`known-divergences.toml`): {}\n\n",
                known.len()
            ));
            for (r, reason) in known.iter().take(MD_LIST_LIMIT) {
                out.push_str(&format!("- `{}`: {reason}\n", r.path));
            }
            out.push('\n');
        }
        for (path, reason) in s.skipped_files.iter().take(MD_LIST_LIMIT) {
            out.push_str(&format!("- skipped `{path}`: {reason}\n"));
        }
        if !s.skipped_files.is_empty() {
            out.push('\n');
        }
    }
    out
}

/// The most common first-error messages across the failing valid files: the cheapest way to see
/// which construct the parser lacks.
fn push_message_histogram(out: &mut String, gaps: &[&FileResult]) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in gaps {
        if let Outcome::Errors { first, .. } = &r.outcome
            && let Some(first) = first.first()
        {
            let message = first.split_once(' ').map_or(first.as_str(), |(_, m)| m);
            *counts.entry(message.to_string()).or_default() += 1;
        }
    }
    let mut counts: Vec<(String, usize)> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out.push_str("First error of each failing file, most common first:\n\n");
    for (message, count) in counts.iter().take(10) {
        out.push_str(&format!("- {count} x {message}\n"));
    }
    out.push('\n');
}

fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Clean => "parsed without errors".to_string(),
        Outcome::Errors { count, first } => {
            format!("{count} error(s), first: {}", first.join("; "))
        }
        Outcome::Panicked(message) => format!("panic: {message}"),
        Outcome::TimedOut => "timed out".to_string(),
        Outcome::RoundtripFailed => "the tree text differs from the file".to_string(),
    }
}

/// Writes `failures/<source>/<category>.txt` file lists for triage.
pub fn write_failure_lists(report: &Report, out_dir: &Path) -> io::Result<()> {
    let _ = fs::remove_dir_all(out_dir.join("failures"));
    for s in &report.sources {
        let dir = out_dir.join("failures").join(sanitize(&s.id));
        let mut lists: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for r in s.critical() {
            lists.entry("critical").or_default().push(&r.path);
        }
        let gap_name = match s.kind {
            Kind::Valid => "completeness",
            Kind::Invalid => "compliance",
        };
        for r in s.gaps() {
            lists.entry(gap_name).or_default().push(&r.path);
        }
        if lists.is_empty() {
            continue;
        }
        fs::create_dir_all(&dir)?;
        for (name, paths) in lists {
            fs::write(dir.join(format!("{name}.txt")), paths.join("\n") + "\n")?;
        }
    }
    Ok(())
}

/// A source id as a single, safe path component.
fn sanitize(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.') {
        "source".to_string()
    } else {
        cleaned
    }
}

/// Writes `report.json`, `report.md` and the failure lists to `out_dir`.
pub fn write_report(report: &Report, out_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(out_dir)?;
    let json = serde_json::to_string_pretty(&to_json(report)).expect("a Value always serializes");
    fs::write(out_dir.join("report.json"), json + "\n")?;
    fs::write(out_dir.join("report.md"), to_markdown(report))?;
    write_failure_lists(report, out_dir)
}

/// Formats a column-aligned plaintext summary table for terminal output.
pub fn format_summary_table(report: &Report) -> String {
    if report.sources.is_empty() {
        return String::new();
    }

    let headers = ["Source", "Files", "Conforming", "Rate", "Critical", "Notes"];
    let mut rows: Vec<[String; 6]> = Vec::with_capacity(report.sources.len() + 1);

    let mut total_files = 0usize;
    let mut total_conforming = 0usize;
    let mut total_critical = 0usize;
    let mut total_gaps = 0usize;
    let mut total_known = 0usize;

    let any_known = report
        .sources
        .iter()
        .any(|s| s.skipped.is_none() && s.known_divergences().next().is_some());

    for s in &report.sources {
        if let Some(reason) = &s.skipped {
            rows.push([
                s.id.clone(),
                "-".to_string(),
                "-".to_string(),
                if any_known { "- ".to_string() } else { "-".to_string() },
                "-".to_string(),
                format!("skipped ({reason})"),
            ]);
            continue;
        }
        let files = s.results.len();
        let conforming = s.conforming();
        let critical = s.critical().count();
        let gaps = s.gaps().count();
        let known = s.known_divergences().count();

        total_files += files;
        total_conforming += conforming;
        total_critical += critical;
        total_gaps += gaps;
        total_known += known;

        let rate = format_rate(conforming, files, known, any_known);
        let notes = format_notes(gaps, known, critical);
        rows.push([
            s.id.clone(),
            files.to_string(),
            conforming.to_string(),
            rate,
            critical.to_string(),
            notes,
        ]);
    }

    let total_row = (report.sources.len() > 1).then(|| {
        [
            "Total".to_string(),
            total_files.to_string(),
            total_conforming.to_string(),
            format_rate(total_conforming, total_files, total_known, any_known),
            total_critical.to_string(),
            format_notes(total_gaps, total_known, total_critical),
        ]
    });

    let mut widths = headers.map(str::len);
    for row in rows.iter().chain(total_row.iter()) {
        for (w, cell) in widths.iter_mut().zip(row.iter()) {
            *w = (*w).max(cell.len());
        }
    }

    let mut out = String::new();
    write_table_row(&mut out, &headers, &widths);
    let sep: [String; 6] = widths.map(|w| "-".repeat(w));
    write_table_row(&mut out, &sep.each_ref().map(String::as_str), &widths);
    for row in &rows {
        write_table_row(&mut out, &row.each_ref().map(String::as_str), &widths);
    }
    if let Some(total) = &total_row {
        write_table_row(&mut out, &sep.each_ref().map(String::as_str), &widths);
        write_table_row(&mut out, &total.each_ref().map(String::as_str), &widths);
    }
    out
}

fn format_rate(conforming: usize, files: usize, known: usize, reserve_star: bool) -> String {
    let effective = files.saturating_sub(known);
    if effective == 0 {
        return if reserve_star {
            "- ".to_string()
        } else {
            "-".to_string()
        };
    }
    let pct = (conforming as f64 / effective as f64) * 100.0;
    if known > 0 {
        format!("{pct:.1}%*")
    } else if reserve_star {
        format!("{pct:.1}% ")
    } else {
        format!("{pct:.1}%")
    }
}

fn format_notes(gaps: usize, known: usize, critical: usize) -> String {
    let mut parts = Vec::new();
    if gaps > 0 {
        parts.push(format!("{gaps} gap(s)"));
    }
    if known > 0 {
        parts.push(format!("{known} known divergence(s)"));
    }
    if parts.is_empty() && critical == 0 {
        "clean".to_string()
    } else if parts.is_empty() {
        format!("{critical} critical")
    } else {
        parts.join(", ")
    }
}

fn write_table_row(out: &mut String, cols: &[&str; 6], widths: &[usize; 6]) {
    use std::fmt::Write as _;
    let _ = writeln!(
        out,
        "{:<w0$}  {:>w1$}  {:>w2$}  {:>w3$}  {:>w4$}  {}",
        cols[0],
        cols[1],
        cols[2],
        cols[3],
        cols[4],
        cols[5],
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2],
        w3 = widths[3],
        w4 = widths[4],
    );
}

/// Used by the CLI to show how long a run took.
pub fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let value = f();
    (value, started.elapsed())
}
