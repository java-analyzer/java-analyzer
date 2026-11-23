//! Pure-Rust static SVG chart generation for `BENCHMARKS.md` (GitHub light & dark mode).

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use crate::bench::report::{BenchReport, OpStatus, RepoServerResult};

/// Key comparable interactive operations plotted in `interactive-p50.svg`.
const COMPARABLE_OPS: [&str; 5] = [
    "hover_workspace",
    "def_workspace",
    "impl_wide",
    "doc_symbol_large",
    "edit_body",
];

#[derive(Clone, Copy)]
struct ServerColor {
    light: &'static str,
    dark: &'static str,
}

const KNOWN_SERVER_COLORS: [(&str, ServerColor); 4] = [
    (
        "java-analyzer",
        ServerColor {
            light: "#0072B2",
            dark: "#38BDF8",
        },
    ),
    (
        "jdtls",
        ServerColor {
            light: "#D55E00",
            dark: "#FB923C",
        },
    ),
    (
        "jls",
        ServerColor {
            light: "#009E73",
            dark: "#34D399",
        },
    ),
    (
        "nbls",
        ServerColor {
            light: "#CC79A7",
            dark: "#F472B6",
        },
    ),
];

const FALLBACK_COLORS: [ServerColor; 5] = [
    ServerColor {
        light: "#E69F00",
        dark: "#FBBF24",
    },
    ServerColor {
        light: "#56B4E9",
        dark: "#60A5FA",
    },
    ServerColor {
        light: "#6366F1",
        dark: "#818CF8",
    },
    ServerColor {
        light: "#65A30D",
        dark: "#A3E635",
    },
    ServerColor {
        light: "#0D9488",
        dark: "#2DD4BF",
    },
];

fn color_for_server(name: &str) -> ServerColor {
    for &(known, color) in &KNOWN_SERVER_COLORS {
        if known == name {
            return color;
        }
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let idx = (hash as usize) % FALLBACK_COLORS.len();
    FALLBACK_COLORS
        .get(idx)
        .copied()
        .unwrap_or(ServerColor {
            light: "#E69F00",
            dark: "#FBBF24",
        })
}

/// Renders all 4 static SVG charts for Markdown inclusion.
pub fn render_all_svg_charts(report: &BenchReport) -> BTreeMap<&'static str, String> {
    let mut charts = BTreeMap::new();
    charts.insert("cold-first-response.svg", render_cold_first_response(report));
    charts.insert("ready-time.svg", render_ready_time(report));
    charts.insert("memory-rss.svg", render_memory_rss(report));
    charts.insert("interactive-p50.svg", render_interactive_p50(report));
    charts
}

fn collect_servers_and_repos(report: &BenchReport) -> (Vec<String>, Vec<String>) {
    let mut servers = Vec::new();
    let mut seen_servers = BTreeSet::new();
    for s in &report.servers {
        if seen_servers.insert(s.clone()) {
            servers.push(s.clone());
        }
    }
    let mut extra_servers = BTreeSet::new();
    for res in &report.results {
        if !seen_servers.contains(&res.server) {
            extra_servers.insert(res.server.clone());
        }
    }
    for s in extra_servers {
        servers.push(s);
    }

    let mut repos = Vec::new();
    let mut seen_repos = BTreeSet::new();
    for res in &report.results {
        if seen_repos.insert(res.repo.clone()) {
            repos.push(res.repo.clone());
        }
    }
    (servers, repos)
}

fn index_results(report: &BenchReport) -> BTreeMap<(&str, &str), &RepoServerResult> {
    let mut map = BTreeMap::new();
    for res in &report.results {
        map.insert((res.repo.as_str(), res.server.as_str()), res);
    }
    map
}

fn svg_open(out: &mut String, width: f64, height: f64, title: &str, servers: &[String]) {
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width:.0} {height:.0}\" width=\"{width:.0}\" height=\"{height:.0}\" role=\"img\" aria-label=\"{}\">",
        xml_escape(title)
    );
    out.push_str("<style>\n");
    out.push_str("  svg { font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, Helvetica, Arial, sans-serif; }\n");
    out.push_str("  .bg { fill: #ffffff; }\n");
    out.push_str("  .panel-bg { fill: #f8fafc; stroke: #e2e8f0; stroke-width: 1; }\n");
    out.push_str("  .title { fill: #0f172a; font-size: 15px; font-weight: 700; }\n");
    out.push_str("  .subtitle { fill: #475569; font-size: 11px; }\n");
    out.push_str("  .panel-title { fill: #0f172a; font-size: 12px; font-weight: 700; }\n");
    out.push_str("  .axis-label { fill: #334155; font-size: 11px; font-weight: 600; }\n");
    out.push_str("  .tick-label { fill: #64748b; font-size: 10px; }\n");
    out.push_str("  .bar-label { fill: #0f172a; font-size: 9.5px; font-weight: 600; }\n");
    out.push_str("  .na-label { fill: #94a3b8; font-size: 9px; font-style: italic; }\n");
    out.push_str("  .legend-text { fill: #334155; font-size: 11px; }\n");
    out.push_str("  .grid { stroke: #e2e8f0; stroke-width: 1; stroke-dasharray: 2,3; }\n");
    out.push_str("  .axis { stroke: #94a3b8; stroke-width: 1.2; }\n");
    for (idx, srv) in servers.iter().enumerate() {
        let c = color_for_server(srv);
        let _ = writeln!(out, "  .srv-{idx} {{ fill: {}; }}", c.light);
        let _ = writeln!(
            out,
            "  .srv-{idx}-alt {{ fill: {}; fill-opacity: 0.32; stroke: {}; stroke-width: 1.6; }}",
            c.light, c.light
        );
    }
    out.push_str("  @media (prefers-color-scheme: dark) {\n");
    out.push_str("    .bg { fill: #0d1117; }\n");
    out.push_str("    .panel-bg { fill: #161b22; stroke: #30363d; }\n");
    out.push_str("    .title { fill: #f0f6fc; }\n");
    out.push_str("    .subtitle { fill: #8b949e; }\n");
    out.push_str("    .panel-title { fill: #f0f6fc; }\n");
    out.push_str("    .axis-label { fill: #c9d1d9; }\n");
    out.push_str("    .tick-label { fill: #8b949e; }\n");
    out.push_str("    .bar-label { fill: #e6edf3; }\n");
    out.push_str("    .na-label { fill: #6e7681; }\n");
    out.push_str("    .legend-text { fill: #c9d1d9; }\n");
    out.push_str("    .grid { stroke: #21262d; }\n");
    out.push_str("    .axis { stroke: #484f58; }\n");
    for (idx, srv) in servers.iter().enumerate() {
        let c = color_for_server(srv);
        let _ = writeln!(out, "    .srv-{idx} {{ fill: {}; }}", c.dark);
        let _ = writeln!(
            out,
            "    .srv-{idx}-alt {{ fill: {}; fill-opacity: 0.32; stroke: {}; stroke-width: 1.6; }}",
            c.dark, c.dark
        );
    }
    out.push_str("  }\n");
    out.push_str("</style>\n");
    let _ = writeln!(
        out,
        "<rect class=\"bg\" x=\"0\" y=\"0\" width=\"{width:.0}\" height=\"{height:.0}\" rx=\"8\"/>"
    );
}

fn draw_header_and_legend(
    out: &mut String,
    title: &str,
    subtitle: &str,
    servers: &[String],
    pair_legend: Option<(&str, &str)>,
    left: f64,
    right_edge: f64,
) {
    let _ = writeln!(
        out,
        "<text class=\"title\" x=\"{left:.1}\" y=\"28\">{}</text>",
        xml_escape(title)
    );
    let _ = writeln!(
        out,
        "<text class=\"subtitle\" x=\"{left:.1}\" y=\"45\">{}</text>",
        xml_escape(subtitle)
    );

    let mut lx = left;
    let ly = 64.0;
    for (idx, srv) in servers.iter().enumerate() {
        let est_w = 24.0 + (srv.len() as f64) * 6.8;
        if lx + est_w > right_edge && lx > left {
            lx = left;
        }
        let _ = writeln!(
            out,
            "<rect class=\"srv-{idx}\" x=\"{lx:.1}\" y=\"{:.1}\" width=\"11\" height=\"11\" rx=\"2\"/>",
            ly - 9.5
        );
        let _ = writeln!(
            out,
            "<text class=\"legend-text\" x=\"{:.1}\" y=\"{ly:.1}\">{}</text>",
            lx + 16.0,
            xml_escape(srv)
        );
        lx += est_w + 10.0;
    }

    if let Some((solid_label, alt_label)) = pair_legend {
        let _ = writeln!(
            out,
            "<rect class=\"axis-label\" x=\"{lx:.1}\" y=\"{:.1}\" width=\"11\" height=\"11\" rx=\"2\" fill-opacity=\"0.8\"/>",
            ly - 9.5
        );
        let _ = writeln!(
            out,
            "<text class=\"legend-text\" x=\"{:.1}\" y=\"{ly:.1}\">{}</text>",
            lx + 16.0,
            xml_escape(solid_label)
        );
        lx += 24.0 + (solid_label.len() as f64) * 6.6 + 8.0;
        let _ = writeln!(
            out,
            "<rect x=\"{lx:.1}\" y=\"{:.1}\" width=\"11\" height=\"11\" rx=\"2\" fill=\"none\" stroke=\"currentColor\" class=\"axis\" stroke-width=\"1.5\"/>",
            ly - 9.5
        );
        let _ = writeln!(
            out,
            "<text class=\"legend-text\" x=\"{:.1}\" y=\"{ly:.1}\">{}</text>",
            lx + 16.0,
            xml_escape(alt_label)
        );
    }
}

fn empty_svg(title: &str, subtitle: &str) -> String {
    let mut out = String::new();
    svg_open(&mut out, 760.0, 220.0, title, &[]);
    draw_header_and_legend(&mut out, title, subtitle, &[], None, 28.0, 732.0);
    out.push_str("<text class=\"subtitle\" x=\"380\" y=\"135\" text-anchor=\"middle\">No benchmark results available</text>\n");
    out.push_str("</svg>\n");
    out
}

fn log10_bounds(values: impl Iterator<Item = f64>) -> (i32, i32) {
    let mut min_v = f64::INFINITY;
    let mut max_v = 0.0_f64;
    for v in values {
        if v.is_finite() {
            let clamped = v.max(0.01);
            min_v = min_v.min(clamped);
            max_v = max_v.max(clamped);
        }
    }
    if !min_v.is_finite() || max_v <= 0.0 {
        return (0, 3);
    }
    let min_exp = (min_v.log10().floor() as i32).clamp(-2, 2);
    let mut max_exp = (max_v.log10().ceil() as i32).max(min_exp + 1);
    if max_exp == min_exp {
        max_exp += 1;
    }
    (min_exp, max_exp)
}

fn log10_fraction(value_ms: f64, min_exp: i32, max_exp: i32) -> f64 {
    let floor_ms = 10_f64.powi(min_exp);
    let clamped = value_ms.max(floor_ms);
    let span = (max_exp - min_exp).max(1) as f64;
    ((clamped.log10() - f64::from(min_exp)) / span).clamp(0.0, 1.0)
}

fn draw_log_grid(
    out: &mut String,
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    min_exp: i32,
    max_exp: i32,
) {
    let span = (max_exp - min_exp).max(1) as f64;
    let plot_h = bottom - top;
    for exp in min_exp..=max_exp {
        let frac = f64::from(exp - min_exp) / span;
        let y = bottom - frac * plot_h;
        let _ = writeln!(
            out,
            "<line class=\"grid\" x1=\"{left:.1}\" y1=\"{y:.1}\" x2=\"{right:.1}\" y2=\"{y:.1}\"/>"
        );
        let label = fmt_log_tick(exp);
        let _ = writeln!(
            out,
            "<text class=\"tick-label\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\">{label}</text>",
            left - 8.0,
            y + 3.5
        );
    }
    let _ = writeln!(
        out,
        "<line class=\"axis\" x1=\"{left:.1}\" y1=\"{top:.1}\" x2=\"{left:.1}\" y2=\"{bottom:.1}\"/>"
    );
    let _ = writeln!(
        out,
        "<line class=\"axis\" x1=\"{left:.1}\" y1=\"{bottom:.1}\" x2=\"{right:.1}\" y2=\"{bottom:.1}\"/>"
    );
}

fn fmt_log_tick(exp: i32) -> String {
    if exp < 0 {
        let val = 10_f64.powi(exp);
        let decimals = (-exp) as usize;
        format!("{val:.decimals$}ms")
    } else if exp < 3 {
        let val = 10_i64.pow(exp as u32);
        format!("{val}ms")
    } else {
        let secs = 10_i64.pow((exp - 3) as u32);
        format!("{secs}s")
    }
}

fn fmt_ms_short(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.2}s", ms / 1000.0)
    } else if ms >= 100.0 {
        format!("{ms:.0}ms")
    } else if ms >= 1.0 {
        format!("{ms:.1}ms")
    } else {
        format!("{ms:.2}ms")
    }
}

fn render_cold_first_response(report: &BenchReport) -> String {
    let title = "Cold Time-to-First-Response (ms, log10 scale)";
    let subtitle = "Spawn to first non-empty LSP response · lower is better";
    let (servers, repos) = collect_servers_and_repos(report);
    if servers.is_empty() || repos.is_empty() {
        return empty_svg(title, subtitle);
    }
    let lookup = index_results(report);
    let (min_exp, max_exp) = log10_bounds(
        report
            .results
            .iter()
            .filter_map(|r| r.lifecycle.cold_first_response_ms),
    );

    let left = 68.0;
    let right_pad = 24.0;
    let top = 92.0;
    let bottom = 290.0;
    let height = 345.0;
    let bar_w = 34.0;
    let bar_gap = 8.0;
    let n_srv = servers.len().max(1) as f64;
    let group_inner = n_srv * bar_w + (n_srv - 1.0) * bar_gap;
    let group_w = group_inner.max(90.0) + 40.0;
    let width = (left + right_pad + (repos.len() as f64) * group_w).max(760.0);
    let right = width - right_pad;
    let actual_group_w = (right - left) / (repos.len() as f64);

    let mut out = String::new();
    svg_open(&mut out, width, height, title, &servers);
    draw_header_and_legend(&mut out, title, subtitle, &servers, None, left, right);
    draw_log_grid(&mut out, left, right, top, bottom, min_exp, max_exp);

    let plot_h = bottom - top;
    for (r_idx, repo) in repos.iter().enumerate() {
        let gx = left + (r_idx as f64) * actual_group_w;
        let center_x = gx + actual_group_w / 2.0;
        let start_x = center_x - group_inner / 2.0;

        let _ = writeln!(
            out,
            "<text class=\"axis-label\" x=\"{center_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>",
            bottom + 20.0,
            xml_escape(repo)
        );

        for (s_idx, srv) in servers.iter().enumerate() {
            let bx = start_x + (s_idx as f64) * (bar_w + bar_gap);
            let label_x = bx + bar_w / 2.0;
            let val = lookup
                .get(&(repo.as_str(), srv.as_str()))
                .and_then(|res| res.lifecycle.cold_first_response_ms);
            if let Some(ms) = val {
                let frac = log10_fraction(ms, min_exp, max_exp);
                let bh = (frac * plot_h).max(4.0);
                let by = bottom - bh;
                let _ = writeln!(
                    out,
                    "<rect class=\"srv-{s_idx}\" x=\"{bx:.1}\" y=\"{by:.1}\" width=\"{bar_w:.1}\" height=\"{bh:.1}\" rx=\"3\"/>"
                );
                let _ = writeln!(
                    out,
                    "<text class=\"bar-label\" x=\"{label_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>",
                    by - 5.0,
                    fmt_ms_short(ms)
                );
            } else {
                let _ = writeln!(
                    out,
                    "<text class=\"na-label\" x=\"{label_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">N/A</text>",
                    bottom - 6.0
                );
            }
        }
    }

    out.push_str("</svg>\n");
    out
}

fn render_ready_time(report: &BenchReport) -> String {
    let title = "Workspace Ready Time — Cold vs. Warm (ms, log10 scale)";
    let subtitle = "Spawn to readiness probe passing · solid = Cold Ready, outlined = Warm Ready · lower is better";
    let (servers, repos) = collect_servers_and_repos(report);
    if servers.is_empty() || repos.is_empty() {
        return empty_svg(title, subtitle);
    }
    let lookup = index_results(report);
    let (min_exp, max_exp) = log10_bounds(report.results.iter().flat_map(|r| {
        [r.lifecycle.cold_ready_ms, r.lifecycle.warm_ready_ms]
            .into_iter()
            .flatten()
    }));

    let left = 68.0;
    let right_pad = 24.0;
    let top = 92.0;
    let bottom = 295.0;
    let height = 350.0;
    let sub_w = 24.0;
    let sub_gap = 4.0;
    let srv_gap = 12.0;
    let pair_w = sub_w * 2.0 + sub_gap;
    let n_srv = servers.len().max(1) as f64;
    let group_inner = n_srv * pair_w + (n_srv - 1.0) * srv_gap;
    let group_w = group_inner.max(110.0) + 42.0;
    let width = (left + right_pad + (repos.len() as f64) * group_w).max(780.0);
    let right = width - right_pad;
    let actual_group_w = (right - left) / (repos.len() as f64);

    let mut out = String::new();
    svg_open(&mut out, width, height, title, &servers);
    draw_header_and_legend(
        &mut out,
        title,
        subtitle,
        &servers,
        Some(("Cold", "Warm")),
        left,
        right,
    );
    draw_log_grid(&mut out, left, right, top, bottom, min_exp, max_exp);

    let plot_h = bottom - top;
    for (r_idx, repo) in repos.iter().enumerate() {
        let gx = left + (r_idx as f64) * actual_group_w;
        let center_x = gx + actual_group_w / 2.0;
        let start_x = center_x - group_inner / 2.0;

        let _ = writeln!(
            out,
            "<text class=\"axis-label\" x=\"{center_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>",
            bottom + 20.0,
            xml_escape(repo)
        );

        for (s_idx, srv) in servers.iter().enumerate() {
            let px = start_x + (s_idx as f64) * (pair_w + srv_gap);
            let res = lookup.get(&(repo.as_str(), srv.as_str())).copied();
            let cold = res.and_then(|r| r.lifecycle.cold_ready_ms);
            let warm = res.and_then(|r| r.lifecycle.warm_ready_ms);

            for (sub_idx, val) in [cold, warm].into_iter().enumerate() {
                let bx = px + (sub_idx as f64) * (sub_w + sub_gap);
                let label_x = bx + sub_w / 2.0;
                let cls = if sub_idx == 0 {
                    format!("srv-{s_idx}")
                } else {
                    format!("srv-{s_idx}-alt")
                };
                if let Some(ms) = val {
                    let frac = log10_fraction(ms, min_exp, max_exp);
                    let bh = (frac * plot_h).max(4.0);
                    let by = bottom - bh;
                    let _ = writeln!(
                        out,
                        "<rect class=\"{cls}\" x=\"{bx:.1}\" y=\"{by:.1}\" width=\"{sub_w:.1}\" height=\"{bh:.1}\" rx=\"2.5\"/>"
                    );
                    let label_y = if sub_idx == 1 && cold.is_some() {
                        by - 15.0
                    } else {
                        by - 5.0
                    };
                    let _ = writeln!(
                        out,
                        "<text class=\"bar-label\" x=\"{label_x:.1}\" y=\"{label_y:.1}\" text-anchor=\"middle\">{}</text>",
                        fmt_ms_short(ms)
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "<text class=\"na-label\" x=\"{label_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">N/A</text>",
                        bottom - 6.0
                    );
                }
            }
        }
    }

    out.push_str("</svg>\n");
    out
}

fn nice_linear_ceil(max_val: f64) -> (f64, usize) {
    if !max_val.is_finite() || max_val <= 0.0 {
        return (100.0, 5);
    }
    let exp = max_val.log10().floor();
    let mag = 10_f64.powf(exp);
    let norm = max_val / mag;
    let step_mult = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 2.5 {
        2.5
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    let top = step_mult * mag;
    (top, 5)
}

fn render_memory_rss(report: &BenchReport) -> String {
    let title = "Memory Footprint — Steady-State & Peak RSS (MB, linear scale)";
    let subtitle = "Process-tree RSS · solid = Steady RSS, outlined = Peak RSS · lower is better";
    let (servers, repos) = collect_servers_and_repos(report);
    if servers.is_empty() || repos.is_empty() {
        return empty_svg(title, subtitle);
    }
    let lookup = index_results(report);
    let max_mb = report
        .results
        .iter()
        .flat_map(|r| [r.lifecycle.steady_rss_mb, r.lifecycle.peak_rss_mb])
        .filter(|v| v.is_finite() && *v >= 0.0)
        .fold(0.0_f64, f64::max);
    let (y_max, ticks) = nice_linear_ceil(max_mb);

    let left = 68.0;
    let right_pad = 24.0;
    let top = 92.0;
    let bottom = 295.0;
    let height = 350.0;
    let sub_w = 24.0;
    let sub_gap = 4.0;
    let srv_gap = 12.0;
    let pair_w = sub_w * 2.0 + sub_gap;
    let n_srv = servers.len().max(1) as f64;
    let group_inner = n_srv * pair_w + (n_srv - 1.0) * srv_gap;
    let group_w = group_inner.max(110.0) + 42.0;
    let width = (left + right_pad + (repos.len() as f64) * group_w).max(780.0);
    let right = width - right_pad;
    let actual_group_w = (right - left) / (repos.len() as f64);

    let mut out = String::new();
    svg_open(&mut out, width, height, title, &servers);
    draw_header_and_legend(
        &mut out,
        title,
        subtitle,
        &servers,
        Some(("Steady", "Peak")),
        left,
        right,
    );

    let plot_h = bottom - top;
    for i in 0..=ticks {
        let frac = (i as f64) / (ticks as f64);
        let y = bottom - frac * plot_h;
        let val = frac * y_max;
        let _ = writeln!(
            out,
            "<line class=\"grid\" x1=\"{left:.1}\" y1=\"{y:.1}\" x2=\"{right:.1}\" y2=\"{y:.1}\"/>"
        );
        let _ = writeln!(
            out,
            "<text class=\"tick-label\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\">{val:.0}MB</text>",
            left - 8.0,
            y + 3.5
        );
    }
    let _ = writeln!(
        out,
        "<line class=\"axis\" x1=\"{left:.1}\" y1=\"{top:.1}\" x2=\"{left:.1}\" y2=\"{bottom:.1}\"/>"
    );
    let _ = writeln!(
        out,
        "<line class=\"axis\" x1=\"{left:.1}\" y1=\"{bottom:.1}\" x2=\"{right:.1}\" y2=\"{bottom:.1}\"/>"
    );

    for (r_idx, repo) in repos.iter().enumerate() {
        let gx = left + (r_idx as f64) * actual_group_w;
        let center_x = gx + actual_group_w / 2.0;
        let start_x = center_x - group_inner / 2.0;

        let _ = writeln!(
            out,
            "<text class=\"axis-label\" x=\"{center_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>",
            bottom + 20.0,
            xml_escape(repo)
        );

        for (s_idx, srv) in servers.iter().enumerate() {
            let px = start_x + (s_idx as f64) * (pair_w + srv_gap);
            let res = lookup.get(&(repo.as_str(), srv.as_str())).copied();
            if let Some(res) = res {
                let vals = [res.lifecycle.steady_rss_mb, res.lifecycle.peak_rss_mb];
                for (sub_idx, mb) in vals.into_iter().enumerate() {
                    let bx = px + (sub_idx as f64) * (sub_w + sub_gap);
                    let label_x = bx + sub_w / 2.0;
                    let cls = if sub_idx == 0 {
                        format!("srv-{s_idx}")
                    } else {
                        format!("srv-{s_idx}-alt")
                    };
                    let frac = (mb / y_max).clamp(0.0, 1.0);
                    let bh = (frac * plot_h).max(2.0);
                    let by = bottom - bh;
                    let _ = writeln!(
                        out,
                        "<rect class=\"{cls}\" x=\"{bx:.1}\" y=\"{by:.1}\" width=\"{sub_w:.1}\" height=\"{bh:.1}\" rx=\"2.5\"/>"
                    );
                    let _ = writeln!(
                        out,
                        "<text class=\"bar-label\" x=\"{label_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{mb:.0}</text>",
                        by - 5.0
                    );
                }
            } else {
                let label_x = px + pair_w / 2.0;
                let _ = writeln!(
                    out,
                    "<text class=\"na-label\" x=\"{label_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">N/A</text>",
                    bottom - 6.0
                );
            }
        }
    }

    out.push_str("</svg>\n");
    out
}

fn render_interactive_p50(report: &BenchReport) -> String {
    let title = "Interactive Latency (p50 ms, log10 scale)";
    let subtitle = "Post-warmup median latency on comparable operations · lower is better";
    let (servers, repos) = collect_servers_and_repos(report);
    if servers.is_empty() || repos.is_empty() {
        return empty_svg(title, subtitle);
    }
    let lookup = index_results(report);
    let (min_exp, max_exp) = log10_bounds(report.results.iter().flat_map(|res| {
        COMPARABLE_OPS.iter().filter_map(|&op| {
            res.ops
                .get(op)
                .filter(|st| st.status == OpStatus::Supported)
                .and_then(|st| st.p50_ms)
        })
    }));

    let left = 68.0;
    let right_pad = 24.0;
    let header_h = 84.0;
    let panel_h = 185.0;
    let panel_gap = 22.0;
    let bottom_pad = 18.0;
    let bar_w = 24.0;
    let bar_gap = 6.0;
    let n_srv = servers.len().max(1) as f64;
    let op_inner = n_srv * bar_w + (n_srv - 1.0) * bar_gap;
    let op_group_w = op_inner.max(106.0) + 32.0;
    let width = (left + right_pad + (COMPARABLE_OPS.len() as f64) * op_group_w).max(820.0);
    let right = width - right_pad;
    let actual_op_w = (right - left) / (COMPARABLE_OPS.len() as f64);
    let height =
        header_h + (repos.len() as f64) * panel_h + ((repos.len().saturating_sub(1)) as f64) * panel_gap + bottom_pad;

    let mut out = String::new();
    svg_open(&mut out, width, height, title, &servers);
    draw_header_and_legend(&mut out, title, subtitle, &servers, None, left, right);

    for (r_idx, repo) in repos.iter().enumerate() {
        let p_top = header_h + (r_idx as f64) * (panel_h + panel_gap);
        let plot_top = p_top + 28.0;
        let plot_bottom = p_top + panel_h - 26.0;
        let plot_h = plot_bottom - plot_top;

        let _ = writeln!(
            out,
            "<rect class=\"panel-bg\" x=\"{:.1}\" y=\"{p_top:.1}\" width=\"{:.1}\" height=\"{panel_h:.1}\" rx=\"6\"/>",
            left - 44.0,
            right - (left - 44.0) + 10.0
        );
        let _ = writeln!(
            out,
            "<text class=\"panel-title\" x=\"{:.1}\" y=\"{:.1}\">{}</text>",
            left,
            p_top + 18.0,
            xml_escape(repo)
        );

        draw_log_grid(&mut out, left, right, plot_top, plot_bottom, min_exp, max_exp);

        for (o_idx, &op_id) in COMPARABLE_OPS.iter().enumerate() {
            let gx = left + (o_idx as f64) * actual_op_w;
            let center_x = gx + actual_op_w / 2.0;
            let start_x = center_x - op_inner / 2.0;

            let _ = writeln!(
                out,
                "<text class=\"tick-label\" x=\"{center_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{op_id}</text>",
                plot_bottom + 16.0
            );

            for (s_idx, srv) in servers.iter().enumerate() {
                let bx = start_x + (s_idx as f64) * (bar_w + bar_gap);
                let label_x = bx + bar_w / 2.0;
                let val = lookup
                    .get(&(repo.as_str(), srv.as_str()))
                    .and_then(|res| res.ops.get(op_id))
                    .filter(|st| st.status == OpStatus::Supported)
                    .and_then(|st| st.p50_ms);
                if let Some(ms) = val {
                    let frac = log10_fraction(ms, min_exp, max_exp);
                    let bh = (frac * plot_h).max(4.0);
                    let by = plot_bottom - bh;
                    let _ = writeln!(
                        out,
                        "<rect class=\"srv-{s_idx}\" x=\"{bx:.1}\" y=\"{by:.1}\" width=\"{bar_w:.1}\" height=\"{bh:.1}\" rx=\"2.5\"/>"
                    );
                    let _ = writeln!(
                        out,
                        "<text class=\"bar-label\" x=\"{label_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>",
                        by - 4.0,
                        fmt_ms_short(ms)
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "<text class=\"na-label\" x=\"{label_x:.1}\" y=\"{:.1}\" text-anchor=\"middle\">N/A</text>",
                        plot_bottom - 5.0
                    );
                }
            }
        }
    }

    out.push_str("</svg>\n");
    out
}

fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}
