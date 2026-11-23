//! Self-contained interactive HTML benchmark report using Vega-Lite v5 and modern HTML/CSS.

use std::collections::BTreeMap;

use askama::Template;

use crate::bench::report::{
    BenchReport, CAPABILITY_COLUMNS, EnvironmentInfo, MATRIX2_GROUPS, OpStatus, join_shapes,
};

const BENCH_CSS: &str = include_str!("../../templates/bench_report.css");
const BENCH_JS: &str = include_str!("../../templates/bench_report.js");

const M1_COLUMNS: [ColHeader; 15] = [
    ColHeader::text("Repo"),
    ColHeader::text("Tier"),
    ColHeader::text("Shape"),
    ColHeader::number("Files"),
    ColHeader::text("Server"),
    ColHeader::number("Cold Init"),
    ColHeader::number("Cold 1st"),
    ColHeader::number("Cold Ready"),
    ColHeader::number("Warm Init"),
    ColHeader::number("Warm 1st"),
    ColHeader::number("Warm Ready"),
    ColHeader::number("Peak RSS"),
    ColHeader::number("Steady RSS"),
    ColHeader::number("Err%"),
    ColHeader::number("Clean Err"),
];

const M2_COLUMNS: [ColHeader; 11] = [
    ColHeader::text("Category"),
    ColHeader::text("Repo"),
    ColHeader::text("Server"),
    ColHeader::text("Operation"),
    ColHeader::text("Status"),
    ColHeader::number("1st Call"),
    ColHeader::number("p50"),
    ColHeader::number("p95"),
    ColHeader::number("max"),
    ColHeader::number("Err%"),
    ColHeader::number("Hit%"),
];

#[derive(Clone, Copy)]
struct ColHeader {
    label: &'static str,
    col_type: &'static str,
}

impl ColHeader {
    const fn text(label: &'static str) -> Self {
        Self {
            label,
            col_type: "text",
        }
    }

    const fn number(label: &'static str) -> Self {
        Self {
            label,
            col_type: "number",
        }
    }
}

struct MsCell {
    metric: &'static str,
    data_value: String,
    display: String,
}

impl MsCell {
    fn new(metric: &'static str, val: Option<f64>) -> Self {
        match val {
            Some(ms) => Self {
                metric,
                data_value: format!("{ms:.4}"),
                display: format!("{ms:.1} ms"),
            },
            None => Self {
                metric,
                data_value: String::new(),
                display: String::from("N/A"),
            },
        }
    }

    fn empty() -> Self {
        Self {
            metric: "",
            data_value: String::new(),
            display: String::from("N/A"),
        }
    }
}

struct LifecycleRow<'a> {
    repo: &'a str,
    tier: &'static str,
    shapes: String,
    files: usize,
    server: &'a str,
    ms_cells: [MsCell; 6],
    peak_rss_data_value: String,
    peak_rss_display: String,
    steady_rss_data_value: String,
    steady_rss_display: String,
    err_pct_data_value: String,
    err_pct_display: String,
    clean_file_errors: usize,
}

struct OpRow<'a> {
    category: &'static str,
    repo: &'a str,
    tier: &'static str,
    shapes: String,
    server: &'a str,
    op_id: &'static str,
    supported: bool,
    ms_cells: [MsCell; 4],
    err_pct_data_value: String,
    err_pct_display: String,
    hit_pct_data_value: String,
    hit_pct_display: String,
}

struct CapRow<'a> {
    server: &'a str,
    values: [bool; 16],
}

#[derive(Template)]
#[template(path = "bench_report.html")]
struct BenchReportTemplate<'a> {
    css: &'static str,
    js: &'static str,
    safe_json: String,
    env: &'a EnvironmentInfo,
    has_warm_data: bool,
    present_tiers: Vec<&'static str>,
    present_shapes: Vec<&'static str>,
    present_op_cats: Vec<&'static str>,
    m1_cols: [ColHeader; 15],
    lifecycle_rows: Vec<LifecycleRow<'a>>,
    m2_cols: [ColHeader; 11],
    op_rows: Vec<OpRow<'a>>,
    cap_cols: [&'static str; 16],
    cap_rows: Vec<CapRow<'a>>,
}

/// Formats a standalone HTML5 benchmark report backed by embedded JSON and Vega-Lite v5 specs.
pub fn format_html_report(report: &BenchReport) -> String {
    let raw_json = serde_json::to_string(report).unwrap_or_else(|_| "{}".to_string());
    let safe_json = raw_json.replace('<', "\\u003c");

    let present_tiers = ["small", "medium", "large"]
        .into_iter()
        .filter(|t| report.results.iter().any(|r| r.tier.as_str() == *t))
        .collect();
    let present_shapes = [
        "plain",
        "maven",
        "gradle",
        "ant",
        "single-module",
        "multi-module",
    ]
    .into_iter()
    .filter(|s| {
        report
            .results
            .iter()
            .any(|r| r.shapes.iter().any(|rs| rs.as_str() == *s))
    })
    .collect();
    let present_op_cats = MATRIX2_GROUPS
        .iter()
        .filter(|(_, op_ids)| {
            report.results.iter().any(|r| {
                op_ids
                    .iter()
                    .any(|id| r.ops.get(*id).is_some_and(|st| st.status == OpStatus::Supported))
            })
        })
        .map(|(cat, _)| *cat)
        .collect();
    let has_warm_data = report.results.iter().any(|r| {
        r.lifecycle.warm_init_ms.is_some()
            || r.lifecycle.warm_first_response_ms.is_some()
            || r.lifecycle.warm_ready_ms.is_some()
    });

    let lifecycle_rows = report
        .results
        .iter()
        .map(|res| {
            let err_pct = res.overall_error_rate_pct();
            LifecycleRow {
                repo: &res.repo,
                tier: res.tier.as_str(),
                shapes: join_shapes(&res.shapes),
                files: res.files,
                server: &res.server,
                ms_cells: [
                    MsCell::new("cold_init_ms", Some(res.lifecycle.cold_init_ms)),
                    MsCell::new("cold_first_response_ms", res.lifecycle.cold_first_response_ms),
                    MsCell::new("cold_ready_ms", res.lifecycle.cold_ready_ms),
                    MsCell::new("warm_init_ms", res.lifecycle.warm_init_ms),
                    MsCell::new("warm_first_response_ms", res.lifecycle.warm_first_response_ms),
                    MsCell::new("warm_ready_ms", res.lifecycle.warm_ready_ms),
                ],
                peak_rss_data_value: format!("{:.3}", res.lifecycle.peak_rss_mb),
                peak_rss_display: format!("{:.1} MB", res.lifecycle.peak_rss_mb),
                steady_rss_data_value: format!("{:.3}", res.lifecycle.steady_rss_mb),
                steady_rss_display: format!("{:.1} MB", res.lifecycle.steady_rss_mb),
                err_pct_data_value: format!("{err_pct:.3}"),
                err_pct_display: format!("{err_pct:.1}%"),
                clean_file_errors: res.lifecycle.clean_file_errors,
            }
        })
        .collect();

    let mut op_rows = Vec::new();
    for (group_title, op_ids) in MATRIX2_GROUPS {
        for res in &report.results {
            let shapes_str = join_shapes(&res.shapes);
            let tier_str = res.tier.as_str();
            for &op_id in op_ids {
                let Some(stats) = res.ops.get(op_id) else {
                    continue;
                };
                let supported = stats.status == OpStatus::Supported;
                let ms_cells = if supported {
                    [
                        MsCell::new("first_ms", stats.first_ms),
                        MsCell::new("p50_ms", stats.p50_ms),
                        MsCell::new("p95_ms", stats.p95_ms),
                        MsCell::new("max_ms", stats.max_ms),
                    ]
                } else {
                    [
                        MsCell::empty(),
                        MsCell::empty(),
                        MsCell::empty(),
                        MsCell::empty(),
                    ]
                };
                op_rows.push(OpRow {
                    category: group_title,
                    repo: &res.repo,
                    tier: tier_str,
                    shapes: shapes_str.clone(),
                    server: &res.server,
                    op_id,
                    supported,
                    ms_cells,
                    err_pct_data_value: format!("{:.3}", stats.error_rate_pct),
                    err_pct_display: format!("{:.1}%", stats.error_rate_pct),
                    hit_pct_data_value: format!("{:.3}", stats.hit_rate_pct),
                    hit_pct_display: format!("{:.1}%", stats.hit_rate_pct),
                });
            }
        }
    }

    let mut seen_servers = BTreeMap::new();
    for res in &report.results {
        seen_servers
            .entry(res.server.as_str())
            .or_insert(&res.capabilities);
    }
    let cap_rows = seen_servers
        .into_iter()
        .map(|(server, caps)| CapRow {
            server,
            values: CAPABILITY_COLUMNS.map(|cap| caps.get(cap).copied().unwrap_or(false)),
        })
        .collect();

    let template = BenchReportTemplate {
        css: BENCH_CSS,
        js: BENCH_JS,
        safe_json,
        env: &report.environment,
        has_warm_data,
        present_tiers,
        present_shapes,
        present_op_cats,
        m1_cols: M1_COLUMNS,
        lifecycle_rows,
        m2_cols: M2_COLUMNS,
        op_rows,
        cap_cols: CAPABILITY_COLUMNS,
        cap_rows,
    };

    template.render().unwrap_or_else(|err| {
        eprintln!("warning: failed to render HTML benchmark report template: {err}");
        String::new()
    })
}
