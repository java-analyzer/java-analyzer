//! Tier 2 corpus diagnostic conformance evaluator (`cargo xtask diag-conformance`).
//!
//! Evaluates `java-analyzer` diagnostics against `openjdk-langtools` `@compile/fail/ref=*.out`
//! negative fixtures and `xtask/conformance/diag_mapping.toml`, scoring line match rate,
//! category parity, UX lint pass rate, and cascade noise ratio.
//!
//! See `docs/specs/0020-diagnostic-conformance-testing.md`.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use ide::{AnalysisHost, FileId, diag_snapshot::check_ux_invariants};
use line_index::{LineIndex, TextSize};
use serde::{Deserialize, Serialize};

use crate::jtreg;

pub const EMBEDDED_DIAG_MAPPING: &str = include_str!("../conformance/diag_mapping.toml");
pub const SCHEMA_VERSION: u32 = 1;
const TOP_LIMIT: usize = 25;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, usage::ValueEnum,
)]
#[usage(crate = usage)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Syntax,
    Semantic,
    All,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Semantic => "semantic",
            Self::All => "all",
        }
    }

    fn includes(self, target: MappingPhase) -> bool {
        matches!(
            (self, target),
            (Self::Syntax, MappingPhase::Syntax)
                | (Self::Semantic, MappingPhase::Semantic)
                | (Self::All, _)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MappingPhase {
    Syntax,
    Semantic,
}

impl MappingPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Semantic => "semantic",
        }
    }
}

/// Evaluate diagnostic quality and javac parity against jtreg negative corpora.
///
/// Report only: exits 0 whenever the report is written (even when the corpus is missing), and
/// non-zero only on tool errors (invalid arguments, malformed mapping TOML, or I/O write errors).
#[derive(Debug, Clone, PartialEq, Eq, usage::Args)]
#[usage(crate = usage)]
pub struct DiagConformanceArgs {
    /// Diagnostic phase to evaluate: syntax | semantic | all
    #[usage(long, value_enum, value_name = "PHASE", default = "syntax")]
    pub phase: Phase,
    /// Root directory containing jtreg negative corpus files
    #[usage(long, value_name = "PATH", default = "target/conformance/corpus")]
    pub corpus_dir: PathBuf,
    /// TOML mapping registry overriding embedded xtask/conformance/diag_mapping.toml
    #[usage(long, value_name = "PATH")]
    pub mapping: Option<PathBuf>,
    /// Output JSON report path
    #[usage(long, value_name = "PATH", default = "target/conformance/diagnostics.json")]
    pub out: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DiagMappingEntry {
    pub javac_key: String,
    pub phase: MappingPhase,
    pub ja_code: String,
    pub message_pattern: String,
    #[serde(default)]
    pub requires_related: bool,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiagMappingFile {
    #[serde(default)]
    mapping: Vec<DiagMappingEntry>,
}

pub fn parse_diag_mapping(toml_text: &str) -> Result<Vec<DiagMappingEntry>, String> {
    let parsed: DiagMappingFile =
        toml::from_str(toml_text).map_err(|e| format!("invalid diag_mapping TOML: {e}"))?;

    let mut seen = BTreeSet::new();
    for entry in &parsed.mapping {
        if entry.javac_key.trim().is_empty() {
            return Err("diag_mapping entry has empty `javac_key`".to_string());
        }
        if entry.ja_code.trim().is_empty() {
            return Err(format!(
                "diag_mapping entry `{}` has empty `ja_code`",
                entry.javac_key
            ));
        }
        if entry.message_pattern.trim().is_empty() {
            return Err(format!(
                "diag_mapping entry `{}` has empty `message_pattern`",
                entry.javac_key
            ));
        }
        if !seen.insert(entry.javac_key.clone()) {
            return Err(format!(
                "duplicate `javac_key` `{}` in diag_mapping TOML",
                entry.javac_key
            ));
        }
    }

    Ok(parsed.mapping)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedDiagnostic {
    pub file: String,
    pub line: usize,
    pub column: Option<usize>,
    pub javac_key: String,
}

/// Extracts `<file>:<line>[:<col>]: compiler.(err|warn).<key>` records from `javac -XDrawDiagnostics`
/// `.out` files.
pub fn parse_javac_out(out_text: &str) -> Vec<ExpectedDiagnostic> {
    let mut out = Vec::new();
    for line in out_text.lines() {
        let Some(parsed) = parse_javac_out_line(line) else {
            continue;
        };
        out.push(parsed);
    }
    out
}

fn parse_javac_out_line(line: &str) -> Option<ExpectedDiagnostic> {
    let err_idx = line.find("compiler.err.");
    let warn_idx = line.find("compiler.warn.");
    let key_start = match (err_idx, warn_idx) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };

    let prefix = line.get(..key_start)?.trim_end();
    let prefix = prefix.strip_suffix(':')?.trim_end();
    if prefix.is_empty() || prefix == "-" {
        return None;
    }

    let (before_last, last_str) = prefix.rsplit_once(':')?;
    let last_num = last_str.trim().parse::<usize>().ok()?;

    let (file, line_num, column) = if let Some((file_part, line_str)) = before_last.rsplit_once(':')
        && let Ok(line_num) = line_str.trim().parse::<usize>()
    {
        (file_part.trim(), line_num, Some(last_num))
    } else {
        (before_last.trim(), last_num, None)
    };

    if file.is_empty() || file == "-" {
        return None;
    }

    let key_tail = line.get(key_start..)?;
    let key_len = key_tail
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
        .unwrap_or(key_tail.len());
    let javac_key = key_tail.get(..key_len)?;
    if javac_key.is_empty() {
        return None;
    }

    Some(ExpectedDiagnostic {
        file: file.to_string(),
        line: line_num,
        column,
        javac_key: javac_key.to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportStatus {
    Ok,
    Skipped,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PhaseMetrics {
    pub cases: usize,
    pub skipped_files: usize,
    pub expected_diagnostics: usize,
    pub emitted_diagnostics: usize,
    pub line_matched_exact: usize,
    pub line_matched_plus_minus_1: usize,
    pub line_match_exact_pct: f64,
    pub line_match_plus_minus_1_pct: f64,
    pub category_matched: usize,
    pub category_evaluated: usize,
    pub category_parity_pct: f64,
    pub ux_lint_passed: usize,
    pub ux_lint_violations: usize,
    pub ux_lint_pass_pct: f64,
    pub cascade_noise_ratio: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct JavacKeyMetrics {
    pub expected: usize,
    pub mapped: bool,
    pub expected_ja_code: Option<String>,
    pub line_matched_exact: usize,
    pub line_matched_plus_minus_1: usize,
    pub category_matched: usize,
    pub category_mismatched: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyCount {
    pub javac_key: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MismatchedKeySummary {
    pub javac_key: String,
    pub expected_ja_code: String,
    pub count: usize,
    pub observed_ja_codes: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CascadeFileSummary {
    pub path: String,
    pub phase: String,
    pub expected: usize,
    pub emitted: usize,
    pub ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticConformanceReport {
    pub schema_version: u32,
    pub status: ReportStatus,
    pub skip_reason: Option<String>,
    pub phase: String,
    pub corpus_dir: String,
    pub totals: PhaseMetrics,
    pub phases: BTreeMap<String, PhaseMetrics>,
    pub per_javac_key: BTreeMap<String, JavacKeyMetrics>,
    pub top_unmapped_keys: Vec<KeyCount>,
    pub top_mismatched_keys: Vec<MismatchedKeySummary>,
    pub worst_cascade_files: Vec<CascadeFileSummary>,
    pub ux_lint_violation_counts: BTreeMap<String, usize>,
}

#[derive(Default)]
struct PhaseAccumulator {
    cases: usize,
    skipped_files: usize,
    expected_diagnostics: usize,
    emitted_diagnostics: usize,
    line_matched_exact: usize,
    line_matched_plus_minus_1: usize,
    category_matched: usize,
    category_evaluated: usize,
    ux_lint_passed: usize,
    ux_lint_violations: usize,
    cascade_ratio_sum: f64,
}

impl PhaseAccumulator {
    fn finish(&self) -> PhaseMetrics {
        PhaseMetrics {
            cases: self.cases,
            skipped_files: self.skipped_files,
            expected_diagnostics: self.expected_diagnostics,
            emitted_diagnostics: self.emitted_diagnostics,
            line_matched_exact: self.line_matched_exact,
            line_matched_plus_minus_1: self.line_matched_plus_minus_1,
            line_match_exact_pct: pct(self.line_matched_exact, self.expected_diagnostics),
            line_match_plus_minus_1_pct: pct(
                self.line_matched_plus_minus_1,
                self.expected_diagnostics,
            ),
            category_matched: self.category_matched,
            category_evaluated: self.category_evaluated,
            category_parity_pct: pct(self.category_matched, self.category_evaluated),
            ux_lint_passed: self.ux_lint_passed,
            ux_lint_violations: self.ux_lint_violations,
            ux_lint_pass_pct: pct(self.ux_lint_passed, self.emitted_diagnostics),
            cascade_noise_ratio: if self.cases == 0 {
                0.0
            } else {
                round2(self.cascade_ratio_sum / self.cases as f64)
            },
        }
    }
}

fn pct(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        round2((part as f64 / whole as f64) * 100.0)
    }
}

fn round2(val: f64) -> f64 {
    (val * 100.0).round() / 100.0
}

pub fn evaluate(args: &DiagConformanceArgs) -> Result<DiagnosticConformanceReport, String> {
    let mapping_text = match &args.mapping {
        Some(path) => fs::read_to_string(path)
            .map_err(|e| format!("cannot read mapping {}: {e}", path.display()))?,
        None => EMBEDDED_DIAG_MAPPING.to_string(),
    };
    let mappings = parse_diag_mapping(&mapping_text)?;
    let mapping_by_key: BTreeMap<String, DiagMappingEntry> = mappings
        .into_iter()
        .map(|entry| (entry.javac_key.clone(), entry))
        .collect();

    if !args.corpus_dir.is_dir() {
        return Ok(skipped_report(
            args,
            format!("corpus directory {} not found", args.corpus_dir.display()),
        ));
    }

    let mut java_files = Vec::new();
    collect_java_files(&args.corpus_dir, &mut java_files)
        .map_err(|e| format!("cannot walk {}: {e}", args.corpus_dir.display()))?;
    java_files.sort();

    if java_files.is_empty() {
        return Ok(skipped_report(
            args,
            format!("no .java files under {}", args.corpus_dir.display()),
        ));
    }

    let allowlist = jtreg::parse_allowlist(include_str!("../conformance/parser_errors.txt"));

    let mut phase_accums: BTreeMap<MappingPhase, PhaseAccumulator> = BTreeMap::new();
    let mut total_accum = PhaseAccumulator::default();
    let mut per_javac_key: BTreeMap<String, JavacKeyMetrics> = BTreeMap::new();
    let mut unmapped_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut mismatch_details: BTreeMap<String, (String, usize, BTreeMap<String, usize>)> =
        BTreeMap::new();
    let mut cascade_files: Vec<CascadeFileSummary> = Vec::new();
    let mut ux_lint_violation_counts: BTreeMap<String, usize> = BTreeMap::new();

    for path in java_files {
        let rel_path = path
            .strip_prefix(&args.corpus_dir)
            .unwrap_or(&path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");

        let Ok(text) = fs::read_to_string(&path) else {
            total_accum.skipped_files += 1;
            continue;
        };

        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut captured_ref: Option<String> = None;
        let jtreg_case = jtreg::classify(
            &text,
            |ref_name| {
                let ref_path = parent.join(ref_name);
                let out_text = fs::read_to_string(&ref_path).ok()?;
                captured_ref = Some(out_text.clone());
                Some(out_text)
            },
            &allowlist,
        );

        let Some(out_text) = captured_ref else {
            continue;
        };

        let case_phase = match &jtreg_case.expectation {
            jtreg::Expectation::Invalid => MappingPhase::Syntax,
            jtreg::Expectation::Skipped(reason) if reason.starts_with("non-parser error ") => {
                MappingPhase::Semantic
            }
            _ => continue,
        };

        if !args.phase.includes(case_phase) {
            continue;
        }

        let all_expected = parse_javac_out(&out_text);
        let file_filtered = filter_expected_to_file(&all_expected, &path);
        let phase_expected: Vec<&ExpectedDiagnostic> = file_filtered
            .into_iter()
            .filter(|exp| match case_phase {
                MappingPhase::Syntax => allowlist.contains(exp.javac_key.as_str()),
                MappingPhase::Semantic => !allowlist.contains(exp.javac_key.as_str()),
            })
            .collect();

        if phase_expected.is_empty() {
            continue;
        }

        let diags = run_ide_diagnostics(&text, jtreg_case.level);
        let line_index = LineIndex::new(&text);
        let emitted_lines: Vec<usize> = diags
            .iter()
            .map(|d| {
                let clamped = usize::from(d.range.start()).min(text.len());
                let lc = line_index.line_col(TextSize::from(clamped as u32));
                lc.line as usize + 1
            })
            .collect();

        let mut file_ux_passed = 0usize;
        let mut file_ux_violations = 0usize;
        for diag in &diags {
            let violations = check_ux_invariants(&text, std::slice::from_ref(diag));
            if violations.is_empty() {
                file_ux_passed += 1;
            } else {
                file_ux_violations += 1;
                for v in violations {
                    let key = format!("{}: {}", v.code, v.reason);
                    *ux_lint_violation_counts.entry(key).or_default() += 1;
                }
            }
        }

        let phase_accum = phase_accums.entry(case_phase).or_default();
        phase_accum.cases += 1;
        phase_accum.expected_diagnostics += phase_expected.len();
        phase_accum.emitted_diagnostics += diags.len();
        phase_accum.ux_lint_passed += file_ux_passed;
        phase_accum.ux_lint_violations += file_ux_violations;

        total_accum.cases += 1;
        total_accum.expected_diagnostics += phase_expected.len();
        total_accum.emitted_diagnostics += diags.len();
        total_accum.ux_lint_passed += file_ux_passed;
        total_accum.ux_lint_violations += file_ux_violations;

        let file_ratio = diags.len() as f64 / phase_expected.len() as f64;
        phase_accum.cascade_ratio_sum += file_ratio;
        total_accum.cascade_ratio_sum += file_ratio;
        cascade_files.push(CascadeFileSummary {
            path: rel_path,
            phase: case_phase.as_str().to_string(),
            expected: phase_expected.len(),
            emitted: diags.len(),
            ratio: round2(file_ratio),
        });

        for exp in phase_expected {
            let mapping_entry = mapping_by_key.get(&exp.javac_key);
            let key_metrics =
                per_javac_key
                    .entry(exp.javac_key.clone())
                    .or_insert_with(|| JavacKeyMetrics {
                        expected: 0,
                        mapped: mapping_entry.is_some(),
                        expected_ja_code: mapping_entry.map(|m| m.ja_code.clone()),
                        line_matched_exact: 0,
                        line_matched_plus_minus_1: 0,
                        category_matched: 0,
                        category_mismatched: 0,
                    });
            key_metrics.expected += 1;

            let exact_diags: Vec<&ide::Diagnostic> = diags
                .iter()
                .zip(emitted_lines.iter())
                .filter(|(_, line)| **line == exp.line)
                .map(|(d, _)| d)
                .collect();

            let near_diags: Vec<&ide::Diagnostic> = diags
                .iter()
                .zip(emitted_lines.iter())
                .filter(|(_, line)| line.abs_diff(exp.line) <= 1)
                .map(|(d, _)| d)
                .collect();

            if !exact_diags.is_empty() {
                phase_accum.line_matched_exact += 1;
                total_accum.line_matched_exact += 1;
                key_metrics.line_matched_exact += 1;
            }

            if !near_diags.is_empty() {
                phase_accum.line_matched_plus_minus_1 += 1;
                total_accum.line_matched_plus_minus_1 += 1;
                key_metrics.line_matched_plus_minus_1 += 1;

                phase_accum.category_evaluated += 1;
                total_accum.category_evaluated += 1;

                let matched_candidates = if !exact_diags.is_empty() {
                    &exact_diags
                } else {
                    &near_diags
                };

                if let Some(entry) = mapping_entry {
                    if matched_candidates.iter().any(|d| d.code == entry.ja_code) {
                        phase_accum.category_matched += 1;
                        total_accum.category_matched += 1;
                        key_metrics.category_matched += 1;
                    } else {
                        key_metrics.category_mismatched += 1;
                        let detail = mismatch_details
                            .entry(exp.javac_key.clone())
                            .or_insert_with(|| (entry.ja_code.clone(), 0, BTreeMap::new()));
                        detail.1 += 1;
                        for d in matched_candidates {
                            *detail.2.entry(d.code.to_string()).or_default() += 1;
                        }
                    }
                } else {
                    key_metrics.category_mismatched += 1;
                    *unmapped_counts.entry(exp.javac_key.clone()).or_default() += 1;
                }
            } else if mapping_entry.is_none() {
                *unmapped_counts.entry(exp.javac_key.clone()).or_default() += 1;
            }
        }
    }

    if total_accum.cases == 0 {
        return Ok(skipped_report(
            args,
            format!(
                "no matching jtreg @compile/fail/ref=*.out cases found for phase `{}` under {}",
                args.phase.as_str(),
                args.corpus_dir.display()
            ),
        ));
    }

    let phases: BTreeMap<String, PhaseMetrics> = phase_accums
        .into_iter()
        .map(|(phase, accum)| (phase.as_str().to_string(), accum.finish()))
        .collect();

    let mut top_unmapped_keys: Vec<KeyCount> = unmapped_counts
        .into_iter()
        .map(|(javac_key, count)| KeyCount { javac_key, count })
        .collect();
    top_unmapped_keys.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.javac_key.cmp(&b.javac_key))
    });
    top_unmapped_keys.truncate(TOP_LIMIT);

    let mut top_mismatched_keys: Vec<MismatchedKeySummary> = mismatch_details
        .into_iter()
        .map(
            |(javac_key, (expected_ja_code, count, observed_ja_codes))| MismatchedKeySummary {
                javac_key,
                expected_ja_code,
                count,
                observed_ja_codes,
            },
        )
        .collect();
    top_mismatched_keys.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.javac_key.cmp(&b.javac_key))
    });
    top_mismatched_keys.truncate(TOP_LIMIT);

    cascade_files.sort_by(|a, b| {
        b.ratio
            .partial_cmp(&a.ratio)
            .unwrap_or(Ordering::Equal)
            .then_with(|| b.emitted.cmp(&a.emitted))
            .then_with(|| a.path.cmp(&b.path))
    });
    cascade_files.truncate(TOP_LIMIT);

    Ok(DiagnosticConformanceReport {
        schema_version: SCHEMA_VERSION,
        status: ReportStatus::Ok,
        skip_reason: None,
        phase: args.phase.as_str().to_string(),
        corpus_dir: args.corpus_dir.display().to_string(),
        totals: total_accum.finish(),
        phases,
        per_javac_key,
        top_unmapped_keys,
        top_mismatched_keys,
        worst_cascade_files: cascade_files,
        ux_lint_violation_counts,
    })
}

fn skipped_report(args: &DiagConformanceArgs, reason: String) -> DiagnosticConformanceReport {
    DiagnosticConformanceReport {
        schema_version: SCHEMA_VERSION,
        status: ReportStatus::Skipped,
        skip_reason: Some(reason),
        phase: args.phase.as_str().to_string(),
        corpus_dir: args.corpus_dir.display().to_string(),
        totals: PhaseMetrics::default(),
        phases: BTreeMap::new(),
        per_javac_key: BTreeMap::new(),
        top_unmapped_keys: Vec::new(),
        top_mismatched_keys: Vec::new(),
        worst_cascade_files: Vec::new(),
        ux_lint_violation_counts: BTreeMap::new(),
    }
}

fn filter_expected_to_file<'a>(
    expected: &'a [ExpectedDiagnostic],
    java_path: &Path,
) -> Vec<&'a ExpectedDiagnostic> {
    let Some(target_name) = java_path.file_name() else {
        return expected.iter().collect();
    };
    let matching: Vec<&ExpectedDiagnostic> = expected
        .iter()
        .filter(|exp| Path::new(&exp.file).file_name() == Some(target_name))
        .collect();
    if matching.is_empty() {
        expected.iter().collect()
    } else {
        matching
    }
}

fn run_ide_diagnostics(text: &str, level: syntax::LanguageLevel) -> Vec<ide::Diagnostic> {
    let mut host = AnalysisHost::new();
    let file_id = FileId(0);
    host.set_file_language_level(file_id, level);
    host.set_file_text(file_id, Arc::new(text.to_string()));
    host.set_workspace_files(Arc::new(vec![file_id]));
    let analysis = host.analysis();
    analysis.diagnostics(file_id).unwrap_or_default()
}

/// Collects `.java` files below `dir` without following symbolic links.
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

pub fn write_report(report: &DiagnosticConformanceReport, out_path: &Path) -> Result<(), String> {
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create directory {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(report)
        .map_err(|e| format!("cannot serialize diagnostic conformance JSON: {e}"))?;
    fs::write(out_path, json + "\n")
        .map_err(|e| format!("cannot write {}: {e}", out_path.display()))
}

pub fn parse_report_json(json_text: &str) -> Result<DiagnosticConformanceReport, String> {
    let report: DiagnosticConformanceReport = serde_json::from_str(json_text)
        .map_err(|e| format!("invalid diagnostic conformance JSON: {e}"))?;
    if report.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported diagnostic conformance schema {}, expected {SCHEMA_VERSION}",
            report.schema_version
        ));
    }
    Ok(report)
}

pub fn run(args: &DiagConformanceArgs) -> i32 {
    let report = match evaluate(args) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("error: {err}");
            return 1;
        }
    };

    if let Err(err) = write_report(&report, &args.out) {
        eprintln!("error: {err}");
        return 1;
    }

    match report.status {
        ReportStatus::Skipped => {
            let reason = report.skip_reason.as_deref().unwrap_or("skipped");
            println!(
                "diag-conformance skipped ({reason}); report written to {}",
                args.out.display()
            );
        }
        ReportStatus::Ok => {
            println!(
                "diag-conformance ({}) wrote {} ({} case(s), line exact {:.1}% / ±1 {:.1}%, parity {:.1}%, ux {:.1}%, cascade {:.2})",
                report.phase,
                args.out.display(),
                report.totals.cases,
                report.totals.line_match_exact_pct,
                report.totals.line_match_plus_minus_1_pct,
                report.totals.category_parity_pct,
                report.totals.ux_lint_pass_pct,
                report.totals.cascade_noise_ratio,
            );
        }
    }

    0
}
