//! Deterministic unified conformance report generator (`cargo xtask report`).
//! See `docs/specs/0017-automated-conformance-reporting.md`.

use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;

pub const EMBEDDED_KNOWN_FINDINGS: &str =
    include_str!("../conformance/workspace-known-findings.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, usage::ValueEnum)]
#[usage(crate = usage)]
pub enum ClippyStatus {
    Pass,
    Fail,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, usage::Args)]
#[usage(crate = usage)]
pub struct ReportArgs {
    /// Path to conformance report.json
    #[usage(long, value_name = "PATH", default = "target/conformance/report.json")]
    pub conformance: PathBuf,
    /// Directory containing timings.tsv and <repo>.json
    #[usage(long, value_name = "DIR", default = "target/workspace-corpus")]
    pub workspace_corpus: PathBuf,
    /// Captured stdout from `cargo test --workspace --all-targets`
    #[usage(long, value_name = "PATH")]
    pub test_output: Option<PathBuf>,
    /// Clippy result: pass | fail | skipped
    #[usage(long, value_enum, value_name = "STATUS", default = "skipped")]
    pub clippy_status: ClippyStatus,
    /// TOML file of known workspace findings [embedded default]
    #[usage(long, value_name = "PATH")]
    pub known_findings: Option<PathBuf>,
    /// Revision string for the report header (e.g. jj change/commit)
    #[usage(long, value_name = "TEXT")]
    pub revision: Option<String>,
    /// Date string for the report header [current UTC date]
    #[usage(long, value_name = "YYYY-MM-DD")]
    pub date: Option<String>,
    /// Output Markdown path
    #[usage(long, value_name = "PATH", default = "target/conformance/CONFORMANCE.md")]
    pub out: PathBuf,
    /// Exit 1 if clippy/tests failed or any unexpected findings exist
    #[usage(long)]
    pub strict: bool,
}

impl Default for ReportArgs {
    fn default() -> Self {
        Self {
            conformance: PathBuf::from("target/conformance/report.json"),
            workspace_corpus: PathBuf::from("target/workspace-corpus"),
            test_output: None,
            clippy_status: ClippyStatus::Skipped,
            known_findings: None,
            revision: None,
            date: None,
            out: PathBuf::from("target/conformance/CONFORMANCE.md"),
            strict: false,
        }
    }
}

/// Returns the current UTC date formatted as `YYYY-MM-DD`.
pub fn current_utc_date() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Howard Hinnant's civil-from-days algorithm (days since 1970-01-01).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + i64::from(m <= 2);
    (y, m as u32, d as u32)
}

// ---- cargo test output parser ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestSuiteResult {
    pub krate: String,
    pub suite: String,
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
}

impl TestSuiteResult {
    fn total(&self) -> usize {
        self.passed + self.failed + self.ignored
    }

    fn is_unit_or_doctest(&self) -> bool {
        self.suite.starts_with("Unit tests (") || self.suite == "Doc-tests"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TestSummary {
    pub suites: Vec<TestSuiteResult>,
    pub total_passed: usize,
    pub total_failed: usize,
    pub total_ignored: usize,
}

/// Parses standard `cargo test` stdout without requiring unstable JSON output.
pub fn parse_cargo_test_output(output: &str) -> Result<TestSummary, String> {
    let mut summary = TestSummary::default();
    let mut current_crate: Option<String> = None;
    let mut pending_suite: Option<(String, String)> = None;

    for (idx, raw_line) in output.lines().enumerate() {
        let line = raw_line.trim();
        if let Some(rest) = line.strip_prefix("Running unittests ") {
            let (src, bin_path) = rest
                .rsplit_once(" (")
                .and_then(|(s, p)| p.strip_suffix(')').map(|p| (s.trim(), p.trim())))
                .ok_or_else(|| format!("line {}: malformed Running unittests header", idx + 1))?;
            let krate = extract_crate_from_binary(bin_path);
            current_crate = Some(krate.clone());
            pending_suite = Some((krate, format!("Unit tests ({src})")));
        } else if let Some(rest) = line.strip_prefix("Running ") {
            let target = rest
                .rsplit_once(" (")
                .map_or(rest, |(t, _)| t)
                .trim();
            let normalized = target.replace('\\', "/");
            let suite = normalized
                .strip_prefix("tests/")
                .and_then(|s| s.strip_suffix(".rs"))
                .unwrap_or(&normalized)
                .to_string();
            let krate = current_crate
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            pending_suite = Some((krate, suite));
        } else if let Some(rest) = line.strip_prefix("Doc-tests ") {
            let krate = normalize_crate_name(rest.trim());
            current_crate = Some(krate.clone());
            pending_suite = Some((krate, "Doc-tests".to_string()));
        } else if let Some(rest) = line.strip_prefix("test result: ") {
            let (krate, suite) = pending_suite.take().ok_or_else(|| {
                format!(
                    "line {}: found `test result:` without preceding suite header",
                    idx + 1
                )
            })?;
            let (status, counts) = rest
                .split_once('.')
                .ok_or_else(|| format!("line {}: malformed `test result:` line", idx + 1))?;
            let (passed, mut failed, ignored) = parse_test_counts(counts, idx + 1)?;
            if status.trim() == "FAILED" && failed == 0 {
                failed = 1;
            }
            summary.total_passed += passed;
            summary.total_failed += failed;
            summary.total_ignored += ignored;
            summary.suites.push(TestSuiteResult {
                krate,
                suite,
                passed,
                failed,
                ignored,
            });
        }
    }

    Ok(summary)
}

fn extract_crate_from_binary(bin_path: &str) -> String {
    let file_name = bin_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(bin_path);
    let stem = file_name.strip_suffix(".exe").unwrap_or(file_name);
    let raw = match stem.rsplit_once('-') {
        Some((name, hash))
            if !name.is_empty()
                && !hash.is_empty()
                && hash.chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            name
        }
        _ => stem,
    };
    normalize_crate_name(raw)
}

fn normalize_crate_name(raw: &str) -> String {
    raw.replace('_', "-")
}

fn parse_test_counts(counts: &str, line_no: usize) -> Result<(usize, usize, usize), String> {
    let mut passed = None;
    let mut failed = None;
    let mut ignored = None;
    for part in counts.split(';') {
        let mut words = part.split_whitespace();
        let Some(num_str) = words.next() else {
            continue;
        };
        let Some(label) = words.next() else {
            continue;
        };
        if !matches!(label, "passed" | "failed" | "ignored") {
            continue;
        }
        let num = num_str.parse::<usize>().map_err(|_| {
            format!("line {line_no}: invalid count `{num_str}` for `{label}`")
        })?;
        match label {
            "passed" => passed = Some(num),
            "failed" => failed = Some(num),
            "ignored" => ignored = Some(num),
            _ => {}
        }
    }
    Ok((
        passed.ok_or_else(|| format!("line {line_no}: missing `passed` count"))?,
        failed.ok_or_else(|| format!("line {line_no}: missing `failed` count"))?,
        ignored.ok_or_else(|| format!("line {line_no}: missing `ignored` count"))?,
    ))
}

// ---- Known workspace findings TOML ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    ParseError,
    PackageMismatch,
    Conflict,
}

impl fmt::Display for FindingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::ParseError => "parse_error",
            Self::PackageMismatch => "package_mismatch",
            Self::Conflict => "conflict",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KnownFindingRule {
    pub repo: String,
    pub kind: FindingKind,
    pub path_prefix: String,
    pub reason: String,
}

impl KnownFindingRule {
    pub fn matches(&self, repo: &str, kind: FindingKind, normalized_path: &str) -> bool {
        if self.repo != repo || self.kind != kind {
            return false;
        }
        let norm_prefix = normalize_path(&self.path_prefix);
        normalized_path == norm_prefix
            || (norm_prefix.ends_with('/') && normalized_path.starts_with(&norm_prefix))
    }
}

#[derive(Debug, Deserialize)]
struct KnownFindingsFile {
    #[serde(default)]
    finding: Vec<KnownFindingRule>,
}

pub fn parse_known_findings(toml_text: &str) -> Result<Vec<KnownFindingRule>, String> {
    let parsed: KnownFindingsFile =
        toml::from_str(toml_text).map_err(|e| format!("invalid known-findings TOML: {e}"))?;
    Ok(parsed.finding)
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

// ---- Conformance report.json ingestion ----

#[derive(Debug, Deserialize)]
struct RawConformanceReport {
    schema: u64,
    #[serde(default)]
    sources: Vec<RawConformanceSource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConformanceSource {
    id: String,
    kind: String,
    #[serde(default)]
    skipped: Option<String>,
    #[serde(default)]
    files: usize,
    #[serde(default)]
    conforming: usize,
    #[serde(default)]
    critical: Vec<ConformanceFinding>,
    #[serde(default)]
    gaps: Vec<ConformanceFinding>,
    #[serde(default)]
    known_divergences: Vec<ConformanceKnownDivergence>,
    #[serde(default)]
    skipped_files: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConformanceFinding {
    pub path: String,
    pub outcome: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub errors: Option<usize>,
    #[serde(default)]
    pub first: Vec<String>,
}

impl ConformanceFinding {
    fn describe(&self) -> String {
        match self.outcome.as_str() {
            "clean" => "parsed without errors".to_string(),
            "errors" => {
                let count = self.errors.unwrap_or(self.first.len());
                if self.first.is_empty() {
                    format!("{count} error(s)")
                } else {
                    format!("{count} error(s), first: {}", self.first.join("; "))
                }
            }
            "panic" => match &self.message {
                Some(msg) => format!("panic: {msg}"),
                None => "panic".to_string(),
            },
            "timeout" => "timed out".to_string(),
            "roundtrip" => "the tree text differs from the file".to_string(),
            other => other.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConformanceKnownDivergence {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceSourceSummary {
    pub id: String,
    pub kind: String,
    pub skipped: Option<String>,
    pub files: usize,
    pub conforming: usize,
    pub critical: Vec<ConformanceFinding>,
    pub gaps: Vec<ConformanceFinding>,
    pub known_divergences: Vec<ConformanceKnownDivergence>,
    pub skipped_files_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceSummary {
    pub sources: Vec<ConformanceSourceSummary>,
    pub total_files: usize,
    pub total_conforming: usize,
    pub total_critical: usize,
    pub total_gaps: usize,
    pub total_known_divergences: usize,
}

pub fn parse_conformance_json(json_text: &str) -> Result<ConformanceSummary, String> {
    let raw: RawConformanceReport =
        serde_json::from_str(json_text).map_err(|e| format!("invalid conformance JSON: {e}"))?;
    if raw.schema != 1 {
        return Err(format!(
            "unsupported conformance report schema {}, expected 1",
            raw.schema
        ));
    }

    let mut total_files = 0;
    let mut total_conforming = 0;
    let mut total_critical = 0;
    let mut total_gaps = 0;
    let mut total_known_divergences = 0;
    let mut sources = Vec::with_capacity(raw.sources.len());

    for s in raw.sources {
        if s.skipped.is_none() {
            total_files += s.files;
            total_conforming += s.conforming;
            total_critical += s.critical.len();
            total_gaps += s.gaps.len();
            total_known_divergences += s.known_divergences.len();
        }
        sources.push(ConformanceSourceSummary {
            id: s.id,
            kind: s.kind,
            skipped: s.skipped,
            files: s.files,
            conforming: s.conforming,
            critical: s.critical,
            gaps: s.gaps,
            known_divergences: s.known_divergences,
            skipped_files_count: s.skipped_files.len(),
        });
    }

    Ok(ConformanceSummary {
        sources,
        total_files,
        total_conforming,
        total_critical,
        total_gaps,
        total_known_divergences,
    })
}

// ---- Workspace-corpus ingestion ----

#[derive(Debug, Deserialize)]
struct RawInspectReport {
    active_build_tool: String,
    active_language_level: RawLanguageLevel,
    #[serde(default)]
    conflicts: Vec<RawInspectConflict>,
    #[serde(default)]
    modules: Vec<serde_json::Value>,
    #[serde(default)]
    package_mismatches: Vec<RawPackageMismatch>,
    health: RawInspectHealth,
}

#[derive(Debug, Deserialize)]
struct RawLanguageLevel {
    release: u32,
    #[serde(default)]
    preview: bool,
}

#[derive(Debug, Deserialize)]
struct RawInspectConflict {
    kind: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct RawPackageMismatch {
    path: String,
    #[serde(default)]
    declared: Option<String>,
    #[serde(default)]
    expected: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl RawPackageMismatch {
    fn diagnostic_message(&self) -> String {
        if let Some(msg) = &self.message {
            return msg.clone();
        }
        match (&self.declared, &self.expected) {
            (Some(declared), Some(expected)) => {
                format!("declared package `{declared}`, expected `{expected}`")
            }
            (None, Some(expected)) => format!("missing package declaration, expected `{expected}`"),
            _ => "package mismatch".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawInspectHealth {
    total_java_files: usize,
    files_with_parse_errors: usize,
    indexed_types: usize,
    #[serde(default)]
    parse_errors: Option<Vec<RawParseError>>,
}

#[derive(Debug, Deserialize)]
struct RawParseError {
    path: String,
    #[serde(default)]
    error_count: Option<usize>,
    first_error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFinding {
    pub repo: String,
    pub kind: FindingKind,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedKnownFinding {
    pub rule: KnownFindingRule,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRepoSummary {
    pub id: String,
    pub build_tool: String,
    pub release: u32,
    pub preview: bool,
    pub modules: usize,
    pub files: usize,
    pub types: usize,
    pub parse_errors: usize,
    pub package_mismatches: usize,
    pub conflicts: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCorpusSummary {
    pub repos: Vec<WorkspaceRepoSummary>,
    pub unexpected_findings: Vec<WorkspaceFinding>,
    pub known_findings: Vec<GroupedKnownFinding>,
    pub total_modules: usize,
    pub total_files: usize,
    pub total_types: usize,
    pub total_parse_errors: usize,
    pub total_package_mismatches: usize,
    pub total_conflicts: usize,
    pub total_elapsed_ms: u64,
}

pub fn load_workspace_corpus(
    dir: &Path,
    rules: &[KnownFindingRule],
) -> Result<Option<WorkspaceCorpusSummary>, String> {
    let timings_path = dir.join("timings.tsv");
    if !dir.is_dir() || !timings_path.is_file() {
        return Ok(None);
    }

    let timings_text = fs::read_to_string(&timings_path)
        .map_err(|e| format!("cannot read {}: {e}", timings_path.display()))?;
    let timings = parse_timings_tsv(&timings_text, &timings_path)?;

    let mut repos = Vec::with_capacity(timings.len());
    let mut unexpected_findings = Vec::new();
    let mut known_counts: BTreeMap<usize, usize> = BTreeMap::new();

    let mut total_modules = 0;
    let mut total_files = 0;
    let mut total_types = 0;
    let mut total_parse_errors = 0;
    let mut total_package_mismatches = 0;
    let mut total_conflicts = 0;
    let mut total_elapsed_ms = 0;

    for (repo_id, elapsed_ms) in timings {
        let json_path = dir.join(format!("{repo_id}.json"));
        let json_text = fs::read_to_string(&json_path)
            .map_err(|e| format!("cannot read {}: {e}", json_path.display()))?;
        let raw: RawInspectReport = serde_json::from_str(&json_text)
            .map_err(|e| format!("invalid inspect JSON in {}: {e}", json_path.display()))?;

        let modules = raw.modules.len();
        let files = raw.health.total_java_files;
        let types = raw.health.indexed_types;
        let parse_errors = raw.health.files_with_parse_errors;
        let package_mismatches = raw.package_mismatches.len();
        let conflicts = raw.conflicts.len();

        total_modules += modules;
        total_files += files;
        total_types += types;
        total_parse_errors += parse_errors;
        total_package_mismatches += package_mismatches;
        total_conflicts += conflicts;
        total_elapsed_ms += elapsed_ms;

        repos.push(WorkspaceRepoSummary {
            id: repo_id.clone(),
            build_tool: raw.active_build_tool,
            release: raw.active_language_level.release,
            preview: raw.active_language_level.preview,
            modules,
            files,
            types,
            parse_errors,
            package_mismatches,
            conflicts,
            elapsed_ms,
        });

        let mut repo_findings = Vec::new();

        match &raw.health.parse_errors {
            Some(errors) => {
                for err in errors {
                    let message = match err.error_count {
                        Some(n) if n > 1 => format!("{} ({n} errors)", err.first_error),
                        _ => err.first_error.clone(),
                    };
                    repo_findings.push(WorkspaceFinding {
                        repo: repo_id.clone(),
                        kind: FindingKind::ParseError,
                        path: normalize_path(&err.path),
                        message,
                    });
                }
            }
            None if parse_errors > 0 => {
                repo_findings.push(WorkspaceFinding {
                    repo: repo_id.clone(),
                    kind: FindingKind::ParseError,
                    path: "<unknown>".to_string(),
                    message: format!(
                        "{parse_errors} file(s) had parse errors (details omitted; run inspect with --show-errors)"
                    ),
                });
            }
            None => {}
        }

        for pm in &raw.package_mismatches {
            repo_findings.push(WorkspaceFinding {
                repo: repo_id.clone(),
                kind: FindingKind::PackageMismatch,
                path: normalize_path(&pm.path),
                message: pm.diagnostic_message(),
            });
        }

        for conflict in &raw.conflicts {
            repo_findings.push(WorkspaceFinding {
                repo: repo_id.clone(),
                kind: FindingKind::Conflict,
                path: normalize_path(&conflict.kind),
                message: conflict.message.clone(),
            });
        }

        repo_findings.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.message.cmp(&b.message))
        });

        for finding in repo_findings {
            if let Some(rule_idx) = rules
                .iter()
                .position(|r| r.matches(&finding.repo, finding.kind, &finding.path))
            {
                *known_counts.entry(rule_idx).or_default() += 1;
            } else {
                unexpected_findings.push(finding);
            }
        }
    }

    let known_findings = known_counts
        .into_iter()
        .filter_map(|(idx, count)| {
            rules.get(idx).map(|rule| GroupedKnownFinding {
                rule: rule.clone(),
                count,
            })
        })
        .collect();

    Ok(Some(WorkspaceCorpusSummary {
        repos,
        unexpected_findings,
        known_findings,
        total_modules,
        total_files,
        total_types,
        total_parse_errors,
        total_package_mismatches,
        total_conflicts,
        total_elapsed_ms,
    }))
}

fn parse_timings_tsv(text: &str, path: &Path) -> Result<Vec<(String, u64)>, String> {
    let mut entries = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (repo_id, ms_str) = line.split_once('\t').ok_or_else(|| {
            format!(
                "{}:{}: expected `<repo>\\t<elapsed_ms>`",
                path.display(),
                idx + 1
            )
        })?;
        let ms = ms_str.trim().parse::<u64>().map_err(|_| {
            format!(
                "{}:{}: invalid elapsed_ms `{ms_str}`",
                path.display(),
                idx + 1
            )
        })?;
        entries.push((repo_id.trim().to_string(), ms));
    }
    Ok(entries)
}

// ---- Unified Report, Verdict, and Rendering ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedReport {
    pub date: String,
    pub revision: Option<String>,
    pub clippy_status: ClippyStatus,
    pub tests: Option<TestSummary>,
    pub conformance_path: PathBuf,
    pub conformance: Option<ConformanceSummary>,
    pub workspace_corpus: Option<WorkspaceCorpusSummary>,
}

impl UnifiedReport {
    pub fn verdict(&self) -> Verdict {
        let clippy_failed = self.clippy_status == ClippyStatus::Fail;
        let tests_failed = self.tests.as_ref().is_some_and(|t| t.total_failed > 0);
        let conformance_failed = self
            .conformance
            .as_ref()
            .is_some_and(|c| c.total_critical > 0 || c.total_gaps > 0);
        let workspace_failed = self
            .workspace_corpus
            .as_ref()
            .is_some_and(|w| !w.unexpected_findings.is_empty());

        if clippy_failed || tests_failed || conformance_failed || workspace_failed {
            Verdict::Fail
        } else {
            Verdict::Pass
        }
    }

    pub fn strict_failed(&self) -> bool {
        self.verdict() == Verdict::Fail
    }

    pub fn render_markdown(&self) -> String {
        let mut out = String::from("# Java Analyzer Conformance & Verification Report\n\n");
        out.push_str(&format!("**Date:** {}  \n", self.date));
        let rev = self
            .revision
            .as_deref()
            .filter(|r| !r.is_empty())
            .unwrap_or("unknown");
        out.push_str(&format!("**Workspace Revision:** `{rev}`  \n"));
        out.push_str(&format!("**Overall Verdict:** {}\n\n", self.verdict_line()));

        out.push_str("---\n\n");
        out.push_str(&render_test_section(
            self.clippy_status,
            self.tests.as_ref(),
        ));
        out.push_str("\n---\n\n");
        out.push_str(&render_conformance_section(
            &self.conformance_path,
            self.conformance.as_ref(),
        ));
        out.push_str("\n---\n\n");
        out.push_str(&render_workspace_section(self.workspace_corpus.as_ref()));
        out
    }

    fn verdict_line(&self) -> String {
        let total_java_files = self.conformance.as_ref().map_or(0, |c| c.total_files)
            + self.workspace_corpus.as_ref().map_or(0, |w| w.total_files);

        if self.verdict() == Verdict::Pass {
            let test_part = match &self.tests {
                Some(t) => format!("{} test failures", format_count(t.total_failed)),
                None => "tests not run".to_string(),
            };
            let clippy_part = match self.clippy_status {
                ClippyStatus::Pass => "0 Clippy warnings",
                ClippyStatus::Skipped => "Clippy skipped",
                ClippyStatus::Fail => "Clippy failed",
            };
            let critical = self.conformance.as_ref().map_or(0, |c| c.total_critical);
            return format!(
                "**PASS** — {test_part}, {clippy_part}, {critical} critical findings, and **0 unexpected findings** across **{} Java files**.",
                format_count(total_java_files)
            );
        }

        let mut reasons = Vec::new();
        if self.clippy_status == ClippyStatus::Fail {
            reasons.push("Clippy failed".to_string());
        }
        if let Some(t) = &self.tests
            && t.total_failed > 0
        {
            reasons.push(format!("{} test failure(s)", format_count(t.total_failed)));
        }
        if let Some(c) = &self.conformance {
            if c.total_critical > 0 {
                reasons.push(format!(
                    "{} critical finding(s)",
                    format_count(c.total_critical)
                ));
            }
            if c.total_gaps > 0 {
                reasons.push(format!(
                    "{} unexpected conformance gap(s)",
                    format_count(c.total_gaps)
                ));
            }
        }
        if let Some(w) = &self.workspace_corpus
            && !w.unexpected_findings.is_empty()
        {
            reasons.push(format!(
                "{} unexpected workspace finding(s)",
                format_count(w.unexpected_findings.len())
            ));
        }

        format!(
            "**FAIL** — {} (across **{} Java files**).",
            reasons.join(", "),
            format_count(total_java_files)
        )
    }
}

pub fn render_test_section(clippy: ClippyStatus, tests: Option<&TestSummary>) -> String {
    let mut out = String::from("## 1. Static Analysis & Workspace Test Suite\n\n");

    let clippy_desc = match clippy {
        ClippyStatus::Pass => {
            "`cargo clippy --workspace --all-targets -- -D warnings` (pass — 0 warnings)"
        }
        ClippyStatus::Fail => {
            "`cargo clippy --workspace --all-targets -- -D warnings` (**fail**)"
        }
        ClippyStatus::Skipped => "`cargo clippy` (skipped)",
    };

    let Some(summary) = tests else {
        if clippy == ClippyStatus::Skipped {
            out.push_str("*Not run*\n");
        } else {
            out.push_str(&format!("Verified with {clippy_desc}.  \nTest suite: *Not run*\n"));
        }
        return out;
    };

    out.push_str(&format!(
        "Verified with {clippy_desc} and `cargo test --workspace --all-targets`.\n\n"
    ));
    out.push_str("| Crate | Test Suite | Passed | Failed | Ignored |\n");
    out.push_str("|---|---|---:|---:|---:|\n");

    for s in &summary.suites {
        if s.total() == 0 {
            continue;
        }
        let suite_cell = if s.is_unit_or_doctest() {
            s.suite.clone()
        } else {
            format!("`{}`", s.suite)
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            s.krate,
            suite_cell,
            format_count(s.passed),
            format_count(s.failed),
            format_count(s.ignored),
        ));
    }

    out.push_str(&format!(
        "| **Total** | | **{}** | **{}** | **{}** |\n",
        format_count(summary.total_passed),
        format_count(summary.total_failed),
        format_count(summary.total_ignored),
    ));
    out
}

pub fn render_conformance_section(path: &Path, summary: Option<&ConformanceSummary>) -> String {
    let mut out = String::from("## 2. Parser Conformance Corpus (`mise run conformance-jdk`)\n\n");
    let Some(c) = summary else {
        out.push_str(&format!("*Not run (`{}` not found)*\n", path.display()));
        return out;
    };

    let corpus_word = if c.sources.len() == 1 {
        "corpus"
    } else {
        "corpora"
    };
    out.push_str(&format!(
        "Executed across {} {corpus_word} (**{} Java files**, **{} conforming**).\n\n",
        c.sources.len(),
        format_count(c.total_files),
        format_count(c.total_conforming),
    ));

    out.push_str("| Corpus | Kind | Files | Conforming | Rate | Critical | Notes |\n");
    out.push_str("|---|---|---:|---:|---:|---:|---|\n");

    for s in &c.sources {
        let (rate, notes) = if let Some(reason) = &s.skipped {
            ("n/a".to_string(), format!("Skipped: {reason}"))
        } else {
            let rate = format_effective_rate(s.conforming, s.files, s.known_divergences.len());
            let mut parts = Vec::new();
            if !s.critical.is_empty() {
                parts.push(format!("{} critical", s.critical.len()));
            }
            if !s.gaps.is_empty() {
                parts.push(format!("{} unexpected gap(s)", s.gaps.len()));
            }
            if !s.known_divergences.is_empty() {
                parts.push(format!(
                    "{} expected exclusion(s)",
                    s.known_divergences.len()
                ));
            }
            let notes = if parts.is_empty() {
                "Clean".to_string()
            } else {
                parts.join(", ")
            };
            (rate, notes)
        };

        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} |\n",
            s.id,
            s.kind,
            format_count(s.files),
            format_count(s.conforming),
            rate,
            format_count(s.critical.len()),
            notes,
        ));
    }

    let total_rate = format_effective_rate(
        c.total_conforming,
        c.total_files,
        c.total_known_divergences,
    );
    let total_notes = if c.total_critical == 0 && c.total_gaps == 0 {
        "**0 unexpected gaps**".to_string()
    } else {
        format!(
            "**{} critical, {} unexpected gap(s)**",
            format_count(c.total_critical),
            format_count(c.total_gaps),
        )
    };
    out.push_str(&format!(
        "| **Total** | | **{}** | **{}** | **{}** | **{}** | {} |\n",
        format_count(c.total_files),
        format_count(c.total_conforming),
        total_rate,
        format_count(c.total_critical),
        total_notes,
    ));

    if c.total_known_divergences > 0 {
        out.push_str("\n\\* Excluding documented entries in `xtask/conformance/known-divergences.toml`:\n");
        for s in &c.sources {
            for kd in &s.known_divergences {
                out.push_str(&format!("- `{}` / `{}`: {}\n", s.id, kd.path, kd.reason));
            }
        }
    }

    if c.total_critical > 0 {
        out.push_str("\n### Critical Findings\n\n");
        for s in &c.sources {
            for finding in &s.critical {
                out.push_str(&format!(
                    "- `{}` / `{}`: {}\n",
                    s.id,
                    finding.path,
                    finding.describe()
                ));
            }
        }
    }

    if c.total_gaps > 0 {
        out.push_str("\n### Unexpected Conformance Gaps\n\n");
        for s in &c.sources {
            for finding in &s.gaps {
                out.push_str(&format!(
                    "- `{}` / `{}`: {}\n",
                    s.id,
                    finding.path,
                    finding.describe()
                ));
            }
        }
    }

    out
}

pub fn render_workspace_section(summary: Option<&WorkspaceCorpusSummary>) -> String {
    let mut out =
        String::from("## 3. Workspace Structure & Indexing Corpus (`mise run workspace-corpus`)\n\n");
    let Some(w) = summary else {
        out.push_str("*Not run*\n");
        return out;
    };

    let repo_word = if w.repos.len() == 1 {
        "repository"
    } else {
        "repositories"
    };
    let conflict_word = if w.total_conflicts == 1 {
        "conflict"
    } else {
        "conflicts"
    };
    out.push_str(&format!(
        "Executed `java-analyzer inspect --json --show-errors` across {} {repo_word} (**{} Java files**, **{} indexed types**, **{} {conflict_word}**, **{} ms** total).\n\n",
        w.repos.len(),
        format_count(w.total_files),
        format_count(w.total_types),
        format_count(w.total_conflicts),
        format_count(w.total_elapsed_ms as usize),
    ));

    out.push_str("| Repository | Build | Release | Modules | Files | Types | Parse Errors | Pkg Mismatches | Conflicts | Time (ms) |\n");
    out.push_str("|---|---|---|---:|---:|---:|---:|---:|---:|---:|\n");

    for r in &w.repos {
        let release = if r.preview {
            format!("Java {} (preview)", r.release)
        } else {
            format!("Java {}", r.release)
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.id,
            r.build_tool,
            release,
            format_count(r.modules),
            format_count(r.files),
            format_count(r.types),
            format_count(r.parse_errors),
            format_count(r.package_mismatches),
            format_count(r.conflicts),
            format_count(r.elapsed_ms as usize),
        ));
    }

    out.push_str(&format!(
        "| **Total** | | | **{}** | **{}** | **{}** | **{}** | **{}** | **{}** | **{}** |\n",
        format_count(w.total_modules),
        format_count(w.total_files),
        format_count(w.total_types),
        format_count(w.total_parse_errors),
        format_count(w.total_package_mismatches),
        format_count(w.total_conflicts),
        format_count(w.total_elapsed_ms as usize),
    ));

    out.push_str("\n### Unexpected Findings\n\n");
    if w.unexpected_findings.is_empty() {
        out.push_str("None.\n");
    } else {
        for f in &w.unexpected_findings {
            out.push_str(&format!(
                "- **{}** (`{}`, `{}`): {}\n",
                f.repo, f.kind, f.path, f.message
            ));
        }
    }

    if !w.known_findings.is_empty() {
        out.push_str("\n### Known / Expected Findings\n\n");
        for g in &w.known_findings {
            if g.count == 1 && !g.rule.path_prefix.ends_with('/') {
                out.push_str(&format!(
                    "- **{}** (`{}`, `{}`): {}\n",
                    g.rule.repo, g.rule.kind, g.rule.path_prefix, g.rule.reason
                ));
            } else {
                let match_word = if g.count == 1 { "match" } else { "matches" };
                out.push_str(&format!(
                    "- **{}** (`{}`, {} {match_word} under `{}`): {}\n",
                    g.rule.repo,
                    g.rule.kind,
                    format_count(g.count),
                    g.rule.path_prefix,
                    g.rule.reason
                ));
            }
        }
    }

    out
}

fn format_effective_rate(conforming: usize, files: usize, known_divergences: usize) -> String {
    let effective = files.saturating_sub(known_divergences);
    let base = if effective == 0 {
        "n/a".to_string()
    } else {
        format!("{:.1}%", conforming.min(effective) as f64 * 100.0 / effective as f64)
    };
    if known_divergences > 0 {
        format!("{base}*")
    } else {
        base
    }
}

fn format_count(n: usize) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (idx, &b) in bytes.iter().enumerate() {
        if idx > 0 && (bytes.len() - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

// ---- Orchestration ----

pub fn generate(args: &ReportArgs) -> Result<UnifiedReport, String> {
    let known_rules = match &args.known_findings {
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            parse_known_findings(&text)?
        }
        None => parse_known_findings(EMBEDDED_KNOWN_FINDINGS)?,
    };

    let tests = match &args.test_output {
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            Some(parse_cargo_test_output(&text)?)
        }
        None => None,
    };

    let conformance = if args.conformance.is_file() {
        let text = fs::read_to_string(&args.conformance)
            .map_err(|e| format!("cannot read {}: {e}", args.conformance.display()))?;
        Some(parse_conformance_json(&text)?)
    } else {
        None
    };

    let workspace_corpus = load_workspace_corpus(&args.workspace_corpus, &known_rules)?;

    let date = args.date.clone().unwrap_or_else(current_utc_date);

    Ok(UnifiedReport {
        date,
        revision: args.revision.clone(),
        clippy_status: args.clippy_status,
        tests,
        conformance_path: args.conformance.clone(),
        conformance,
        workspace_corpus,
    })
}

pub fn run(args: &ReportArgs) -> i32 {
    let report = match generate(args) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("error: {err}");
            return 1;
        }
    };

    if let Some(parent) = args.out.parent()
        && !parent.as_os_str().is_empty()
        && let Err(err) = fs::create_dir_all(parent)
    {
        eprintln!(
            "error: cannot create directory {}: {err}",
            parent.display()
        );
        return 1;
    }

    let markdown = report.render_markdown();
    if let Err(err) = fs::write(&args.out, markdown) {
        eprintln!("error: cannot write {}: {err}", args.out.display());
        return 1;
    }

    println!("report written to {}", args.out.display());
    if args.strict && report.strict_failed() {
        eprintln!("error: strict verification failed");
        return 1;
    }
    0
}
