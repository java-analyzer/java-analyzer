//! Benchmark statistics, JSON schema v1, CSV, aligned stdout tables, and `BENCHMARKS.md` formatter.

use std::{collections::BTreeMap, fmt::Write as _};

use serde::{Deserialize, Serialize};

use crate::bench::probes::{Shape, Tier};

/// Current JSON output schema version.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// Operation capability support status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpStatus {
    Supported,
    Unsupported,
}

/// Latency, error-rate, and hit-rate metrics for one benchmark operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpStats {
    pub status: OpStatus,
    pub first_ms: Option<f64>,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub error_rate_pct: f64,
    pub hit_rate_pct: f64,
    pub total_requests: usize,
    pub errors: usize,
    pub hits: usize,
}

impl OpStats {
    /// Constructs an `unsupported` (`N/A`) entry.
    pub fn unsupported() -> Self {
        Self {
            status: OpStatus::Unsupported,
            first_ms: None,
            p50_ms: None,
            p95_ms: None,
            max_ms: None,
            error_rate_pct: 0.0,
            hit_rate_pct: 0.0,
            total_requests: 0,
            errors: 0,
            hits: 0,
        }
    }

    /// Computes summary statistics from measured samples, request counts, errors, and hits.
    pub fn from_samples(
        first_ms: Option<f64>,
        samples_ms: &[f64],
        total_requests: usize,
        errors: usize,
        hits: usize,
    ) -> Self {
        let (p50_ms, p95_ms, max_ms) = if samples_ms.is_empty() {
            (None, None, None)
        } else {
            (
                Some(percentile_ms(samples_ms, 50.0)),
                Some(percentile_ms(samples_ms, 95.0)),
                Some(percentile_ms(samples_ms, 100.0)),
            )
        };
        let error_rate_pct = if total_requests == 0 {
            0.0
        } else {
            (errors as f64 / total_requests as f64) * 100.0
        };
        let hit_rate_pct = if total_requests == 0 {
            0.0
        } else {
            (hits as f64 / total_requests as f64) * 100.0
        };
        Self {
            status: OpStatus::Supported,
            first_ms,
            p50_ms,
            p95_ms,
            max_ms,
            error_rate_pct,
            hit_rate_pct,
            total_requests,
            errors,
            hits,
        }
    }
}

/// Lifecycle, first-response, memory, and clean-diagnostic metrics for a `(repo, server)` run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LifecycleStats {
    pub cold_init_ms: f64,
    pub cold_first_response_ms: Option<f64>,
    pub cold_ready_ms: Option<f64>,
    pub warm_init_ms: Option<f64>,
    pub warm_first_response_ms: Option<f64>,
    pub warm_ready_ms: Option<f64>,
    pub peak_rss_mb: f64,
    pub steady_rss_mb: f64,
    pub clean_file_errors: usize,
}

/// Complete benchmark measurements for one `(repo, server)` cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepoServerResult {
    pub repo: String,
    pub tier: Tier,
    pub shapes: Vec<Shape>,
    pub files: usize,
    pub server: String,
    pub capabilities: BTreeMap<String, bool>,
    pub lifecycle: LifecycleStats,
    pub total_requests: usize,
    pub total_errors: usize,
    pub ops: BTreeMap<String, OpStats>,
}

impl RepoServerResult {
    pub fn overall_error_rate_pct(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.total_errors as f64 / self.total_requests as f64) * 100.0
        }
    }
}

/// Host hardware/runtime metadata recorded in the benchmark report header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    pub os: String,
    pub arch: String,
    pub cpu_model: String,
    pub cpu_cores: usize,
    pub ram_gb: String,
    pub java_version: String,
    pub revision: String,
    pub date: String,
    pub server_versions: BTreeMap<String, String>,
}

/// Top-level JSON v1 benchmark report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchReport {
    pub schema_version: u32,
    pub environment: EnvironmentInfo,
    pub servers: Vec<String>,
    pub results: Vec<RepoServerResult>,
}

/// Computes the `p`-th percentile (0..=100) using linear interpolation on sorted samples.
pub fn percentile_ms(samples: &[f64], p: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    if sorted.len() == 1 {
        return sorted.first().copied().unwrap_or(0.0);
    }
    let clamped = p.clamp(0.0, 100.0);
    let rank = (clamped / 100.0) * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let lo_val = sorted.get(lo).copied().unwrap_or(0.0);
    let hi_val = sorted.get(hi).copied().unwrap_or(lo_val);
    let frac = rank - lo as f64;
    lo_val + (hi_val - lo_val) * frac
}

/// Operation category display groups for Matrix 2.
pub(crate) const MATRIX2_GROUPS: [(&str, &[&str]); 6] = [
    (
        "Navigation & Resolution",
        &[
            "hover_workspace",
            "hover_jdk",
            "def_workspace",
            "def_jdk",
            "type_def",
            "doc_symbol_small",
            "doc_symbol_large",
            "workspace_symbol",
        ],
    ),
    (
        "Type Hierarchies",
        &[
            "impl_simple",
            "impl_deep",
            "impl_wide",
            "hierarchy_super",
            "hierarchy_sub",
        ],
    ),
    (
        "Cross-References & Refactoring",
        &[
            "refs_local",
            "refs_cross_file",
            "rename_local",
            "rename_private",
            "rename_public",
            "code_action_fix",
            "code_action_refactor",
        ],
    ),
    (
        "Autocompletion",
        &[
            "completion_dot_workspace",
            "completion_dot_jdk",
            "completion_dot_generic",
            "completion_scope",
            "completion_auto_import",
            "completion_override",
            "completion_resolve",
        ],
    ),
    (
        "Incremental Editing & Diagnostics",
        &["edit_body", "edit_signature", "signature_help"],
    ),
    (
        "Presentation & Formatting",
        &["semantic_tokens", "inlay_hints", "formatting"],
    ),
];

/// Capability coverage columns in deterministic order.
pub(crate) const CAPABILITY_COLUMNS: [&str; 16] = [
    "hover",
    "definition",
    "typeDefinition",
    "implementation",
    "typeHierarchy",
    "documentSymbol",
    "workspaceSymbol",
    "references",
    "rename",
    "codeAction",
    "completion",
    "completionResolve",
    "signatureHelp",
    "semanticTokens",
    "inlayHint",
    "formatting",
];

/// Formats aligned plaintext summary tables for stdout.
pub fn format_stdout_tables(report: &BenchReport) -> String {
    let mut out = String::new();

    let _ = writeln!(
        out,
        "Java LSP Benchmark ({} / {}, {} cores, RAM {}, Java {}, rev {}, {})",
        report.environment.os,
        report.environment.arch,
        report.environment.cpu_cores,
        report.environment.ram_gb,
        report.environment.java_version,
        report.environment.revision,
        report.environment.date,
    );
    out.push('\n');

    out.push_str("Matrix 1: Lifecycle, First Response, Memory & Reliability\n");
    let m1_headers = [
        "Repo",
        "Tier",
        "Shape",
        "Files",
        "Server",
        "ColdInit",
        "Cold1st",
        "ColdReady",
        "WarmInit",
        "Warm1st",
        "WarmReady",
        "PeakRSS",
        "SteadyRSS",
        "Err%",
        "CleanErr",
    ];
    let mut m1_rows = Vec::with_capacity(report.results.len());
    for res in &report.results {
        let shape_str = join_shapes(&res.shapes);
        m1_rows.push(vec![
            res.repo.clone(),
            res.tier.as_str().to_string(),
            shape_str,
            res.files.to_string(),
            res.server.clone(),
            fmt_ms_val(Some(res.lifecycle.cold_init_ms)),
            fmt_ms_val(res.lifecycle.cold_first_response_ms),
            fmt_ms_val(res.lifecycle.cold_ready_ms),
            fmt_ms_val(res.lifecycle.warm_init_ms),
            fmt_ms_val(res.lifecycle.warm_first_response_ms),
            fmt_ms_val(res.lifecycle.warm_ready_ms),
            format!("{:.1}MB", res.lifecycle.peak_rss_mb),
            format!("{:.1}MB", res.lifecycle.steady_rss_mb),
            format!("{:.1}%", res.overall_error_rate_pct()),
            res.lifecycle.clean_file_errors.to_string(),
        ]);
    }
    let m1_right = [
        false, false, false, true, false, true, true, true, true, true, true, true, true, true,
        true,
    ];
    write_aligned_table(&mut out, &m1_headers, &m1_rows, &m1_right);

    for (group_title, op_ids) in MATRIX2_GROUPS {
        let mut group_rows = Vec::new();
        for res in &report.results {
            for &op_id in op_ids {
                let Some(stats) = res.ops.get(op_id) else {
                    continue;
                };
                group_rows.push(op_row(res, op_id, stats));
            }
        }
        if group_rows.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\nMatrix 2 ({group_title}):");
        let headers = [
            "Repo", "Server", "Op", "1st", "p50", "p95", "max", "Err%", "Hit%",
        ];
        let right = [false, false, false, true, true, true, true, true, true];
        write_aligned_table(&mut out, &headers, &group_rows, &right);
    }

    if !report.results.is_empty() {
        out.push_str("\nCapability Coverage Matrix:\n");
        let mut seen_servers = BTreeMap::new();
        for res in &report.results {
            seen_servers
                .entry(res.server.clone())
                .or_insert_with(|| res.capabilities.clone());
        }
        let mut headers = vec!["Server".to_string()];
        for cap in CAPABILITY_COLUMNS {
            headers.push(cap.to_string());
        }
        let header_refs: Vec<&str> = headers.iter().map(String::as_str).collect();
        let mut rows = Vec::new();
        for (server, caps) in seen_servers {
            let mut row = vec![server];
            for cap in CAPABILITY_COLUMNS {
                let label = if caps.get(cap).copied().unwrap_or(false) {
                    "yes"
                } else {
                    "N/A"
                };
                row.push(label.to_string());
            }
            rows.push(row);
        }
        let right = vec![false; header_refs.len()];
        write_aligned_table(&mut out, &header_refs, &rows, &right);
    }

    out
}

/// Formats `BENCHMARKS.md` with Environment header, Matrix 1, Matrix 2, and Capability Coverage Matrix.
pub fn format_markdown(report: &BenchReport) -> String {
    format_markdown_with_charts(report, None)
}

/// Formats `BENCHMARKS.md` with optional relative chart links under `chart_rel_prefix`.
pub fn format_markdown_with_charts(
    report: &BenchReport,
    chart_rel_prefix: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("# Java LSP Benchmarks\n\n");
    out.push_str("> [!NOTE]\n");
    out.push_str("> **Incomplete Benchmark:** This report reflects a preliminary, incomplete benchmark run. It skips parts of full benchmarking setup (such as OS disk-cache clearing, isolated per-server workspace resets, and build-tool dependency prefetching) as well as some long-running scenarios and unimplemented LSP capabilities; treat numbers as directional rather than a strict apples-to-apples comparison.\n\n");

    out.push_str("## Environment & Revisions\n\n");
    let _ = writeln!(
        out,
        "- **OS / Arch**: `{}` / `{}`",
        report.environment.os, report.environment.arch
    );
    let _ = writeln!(
        out,
        "- **CPU**: {} ({} cores)",
        report.environment.cpu_model, report.environment.cpu_cores
    );
    let _ = writeln!(out, "- **RAM**: {}", report.environment.ram_gb);
    let _ = writeln!(out, "- **Java**: `{}`", report.environment.java_version);
    let _ = writeln!(out, "- **Revision**: `{}`", report.environment.revision);
    let _ = writeln!(out, "- **Date**: {}", report.environment.date);
    if !report.environment.server_versions.is_empty() {
        out.push_str("- **Servers**:\n");
        for (name, ver) in &report.environment.server_versions {
            let _ = writeln!(out, "  - `{name}`: {ver}");
        }
    }
    out.push('\n');

    if let Some(raw_prefix) = chart_rel_prefix {
        let prefix = raw_prefix.trim_end_matches('/');
        let chart_path = |file: &str| {
            if prefix.is_empty() {
                file.to_string()
            } else {
                format!("{prefix}/{file}")
            }
        };
        out.push_str("## Charts\n\n");
        let _ = writeln!(
            out,
            "![Cold Time-to-First-Response]({})\n",
            chart_path("cold-first-response.svg")
        );
        let _ = writeln!(
            out,
            "![Cold Ready and Warm Ready]({})\n",
            chart_path("ready-time.svg")
        );
        let _ = writeln!(
            out,
            "![Steady-State and Peak Memory RSS]({})\n",
            chart_path("memory-rss.svg")
        );
        let _ = writeln!(
            out,
            "![Interactive Latency p50]({})\n",
            chart_path("interactive-p50.svg")
        );
    }

    out.push_str("## Matrix 1 — Lifecycle, First Response, Memory & Reliability\n\n");
    out.push_str("| Repo | Tier | Shape | Files | Server | Cold Init | Cold 1st Resp | Cold Ready | Warm Init | Warm 1st Resp | Warm Ready | Peak RSS | Steady RSS | Error Rate | Clean-File Errors |\n");
    out.push_str("| :--- | :--- | :--- | ---: | :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for res in &report.results {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | `{}` | {} | {} | {} | {} | {} | {} | {:.1} MB | {:.1} MB | {:.1}% | {} |",
            res.repo,
            res.tier.as_str(),
            join_shapes(&res.shapes),
            res.files,
            res.server,
            fmt_ms_val(Some(res.lifecycle.cold_init_ms)),
            fmt_ms_val(res.lifecycle.cold_first_response_ms),
            fmt_ms_val(res.lifecycle.cold_ready_ms),
            fmt_ms_val(res.lifecycle.warm_init_ms),
            fmt_ms_val(res.lifecycle.warm_first_response_ms),
            fmt_ms_val(res.lifecycle.warm_ready_ms),
            res.lifecycle.peak_rss_mb,
            res.lifecycle.steady_rss_mb,
            res.overall_error_rate_pct(),
            res.lifecycle.clean_file_errors,
        );
    }
    out.push('\n');

    out.push_str("## Matrix 2 — Interactive Latency, Error Rate & Hit Rate\n\n");
    for (group_title, op_ids) in MATRIX2_GROUPS {
        let mut rows = Vec::new();
        for res in &report.results {
            for &op_id in op_ids {
                let Some(stats) = res.ops.get(op_id) else {
                    continue;
                };
                rows.push(op_row(res, op_id, stats));
            }
        }
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(out, "### {group_title}\n");
        out.push_str(
            "| Repo | Server | Operation | 1st Call | p50 | p95 | max | Error Rate | Hit Rate |\n",
        );
        out.push_str(
            "| :--- | :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |\n",
        );
        for row in rows {
            if let [repo, server, op, first, p50, p95, max, err, hit] = row.as_slice() {
                let _ = writeln!(
                    out,
                    "| `{repo}` | `{server}` | `{op}` | {first} | {p50} | {p95} | {max} | {err} | {hit} |"
                );
            }
        }
        out.push('\n');
    }

    out.push_str("## Capability Coverage Matrix\n\n");
    let mut seen_servers = BTreeMap::new();
    for res in &report.results {
        seen_servers
            .entry(res.server.clone())
            .or_insert_with(|| res.capabilities.clone());
    }
    out.push_str("| Server |");
    for cap in CAPABILITY_COLUMNS {
        let _ = write!(out, " `{cap}` |");
    }
    out.push('\n');
    out.push_str("| :--- |");
    for _ in CAPABILITY_COLUMNS {
        out.push_str(" :---: |");
    }
    out.push('\n');
    for (server, caps) in seen_servers {
        let _ = write!(out, "| `{server}` |");
        for cap in CAPABILITY_COLUMNS {
            let mark = if caps.get(cap).copied().unwrap_or(false) {
                "supported"
            } else {
                "N/A"
            };
            let _ = write!(out, " {mark} |");
        }
        out.push('\n');
    }

    out
}

fn op_row(res: &RepoServerResult, op_id: &str, stats: &OpStats) -> Vec<String> {
    if stats.status == OpStatus::Unsupported {
        vec![
            res.repo.clone(),
            res.server.clone(),
            op_id.to_string(),
            "N/A".to_string(),
            "N/A".to_string(),
            "N/A".to_string(),
            "N/A".to_string(),
            "N/A".to_string(),
            "N/A".to_string(),
        ]
    } else {
        vec![
            res.repo.clone(),
            res.server.clone(),
            op_id.to_string(),
            fmt_ms_val(stats.first_ms),
            fmt_ms_val(stats.p50_ms),
            fmt_ms_val(stats.p95_ms),
            fmt_ms_val(stats.max_ms),
            format!("{:.1}%", stats.error_rate_pct),
            format!("{:.1}%", stats.hit_rate_pct),
        ]
    }
}

fn fmt_ms_val(val: Option<f64>) -> String {
    match val {
        Some(ms) => format!("{ms:.1}ms"),
        None => "N/A".to_string(),
    }
}

pub(crate) fn join_shapes(shapes: &[Shape]) -> String {
    shapes
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join("+")
}

/// Formats lifecycle, first-response, memory, and reliability metrics as RFC 4180 CSV.
pub fn format_lifecycle_csv(report: &BenchReport) -> String {
    let mut out = String::from(
        "repo,tier,shapes,files,server,cold_init_ms,cold_first_response_ms,cold_ready_ms,warm_init_ms,warm_first_response_ms,warm_ready_ms,peak_rss_mb,steady_rss_mb,error_rate_pct,clean_file_errors\n",
    );
    for res in &report.results {
        let shapes = join_shapes(&res.shapes);
        let row = [
            csv_escape(&res.repo),
            csv_escape(res.tier.as_str()),
            csv_escape(&shapes),
            res.files.to_string(),
            csv_escape(&res.server),
            format!("{:.3}", res.lifecycle.cold_init_ms),
            fmt_csv_opt_f64(res.lifecycle.cold_first_response_ms),
            fmt_csv_opt_f64(res.lifecycle.cold_ready_ms),
            fmt_csv_opt_f64(res.lifecycle.warm_init_ms),
            fmt_csv_opt_f64(res.lifecycle.warm_first_response_ms),
            fmt_csv_opt_f64(res.lifecycle.warm_ready_ms),
            format!("{:.3}", res.lifecycle.peak_rss_mb),
            format!("{:.3}", res.lifecycle.steady_rss_mb),
            format!("{:.3}", res.overall_error_rate_pct()),
            res.lifecycle.clean_file_errors.to_string(),
        ];
        out.push_str(&row.join(","));
        out.push('\n');
    }
    out
}

/// Formats per-operation latency, error-rate, and hit-rate metrics as RFC 4180 CSV.
pub fn format_ops_csv(report: &BenchReport) -> String {
    let mut out = String::from(
        "repo,server,op,status,first_ms,p50_ms,p95_ms,max_ms,error_rate_pct,hit_rate_pct,total_requests,errors,hits\n",
    );
    for res in &report.results {
        for (op_id, stats) in &res.ops {
            let status = match stats.status {
                OpStatus::Supported => "supported",
                OpStatus::Unsupported => "unsupported",
            };
            let row = [
                csv_escape(&res.repo),
                csv_escape(&res.server),
                csv_escape(op_id),
                status.to_string(),
                fmt_csv_opt_f64(stats.first_ms),
                fmt_csv_opt_f64(stats.p50_ms),
                fmt_csv_opt_f64(stats.p95_ms),
                fmt_csv_opt_f64(stats.max_ms),
                format!("{:.3}", stats.error_rate_pct),
                format!("{:.3}", stats.hit_rate_pct),
                stats.total_requests.to_string(),
                stats.errors.to_string(),
                stats.hits.to_string(),
            ];
            out.push_str(&row.join(","));
            out.push('\n');
        }
    }
    out
}

fn fmt_csv_opt_f64(val: Option<f64>) -> String {
    match val {
        Some(v) => format!("{v:.3}"),
        None => String::new(),
    }
}

/// Escapes a CSV field according to RFC 4180.
pub fn csv_escape(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        let mut out = String::with_capacity(field.len() + 2);
        out.push('"');
        for ch in field.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
        out
    } else {
        field.to_string()
    }
}

fn write_aligned_table(out: &mut String, headers: &[&str], rows: &[Vec<String>], right: &[bool]) {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(idx)
                && cell.len() > *w
            {
                *w = cell.len();
            }
        }
    }

    write_aligned_row(out, headers.iter().copied(), &widths, right);
    for (idx, width) in widths.iter().enumerate() {
        if idx > 0 {
            out.push_str("  ");
        }
        for _ in 0..*width {
            out.push('-');
        }
    }
    out.push('\n');

    for row in rows {
        write_aligned_row(out, row.iter().map(String::as_str), &widths, right);
    }
}

fn write_aligned_row<'a>(
    out: &mut String,
    cells: impl Iterator<Item = &'a str>,
    widths: &[usize],
    right: &[bool],
) {
    for (idx, cell) in cells.enumerate() {
        if idx > 0 {
            out.push_str("  ");
        }
        let width = widths.get(idx).copied().unwrap_or(cell.len());
        let is_right = right.get(idx).copied().unwrap_or(false);
        if is_right {
            let _ = write!(out, "{cell:>width$}");
        } else {
            let _ = write!(out, "{cell:<width$}");
        }
    }
    out.push('\n');
}
