use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use xtask::{
    bench::{
        self, BenchArgs, BenchPlotArgs, GenPlainArgs, OutputFormat, ServerSpec,
        charts::render_all_svg_charts,
        html::format_html_report,
        probes::{
            BenchManifest, OpCategory, Shape, Tier, resolve_anchor_utf16,
            resolve_cursor_after_utf16,
        },
        report::{
            BenchReport, EnvironmentInfo, LifecycleStats, OpStats, OpStatus,
            REPORT_SCHEMA_VERSION, RepoServerResult, format_lifecycle_csv, format_markdown,
            format_markdown_with_charts, format_ops_csv, format_stdout_tables, percentile_ms,
        },
        rss::parse_ps_tree_rss_kb,
        synth::generate_plain_small,
    },
    cli::{self, Command as CliCommand},
};

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("bench-tests")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn collect_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ---- 1. Synthetic plain-small generator ----

#[test]
fn synth_generates_valid_plain_small_corpus_with_expected_hierarchies() {
    let dir = scratch("plain-small-synth");
    let summary = generate_plain_small(&dir).unwrap();
    assert_eq!(summary.total_files, 150);

    let files = collect_files(&dir);
    let java_files: Vec<&PathBuf> = files
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("java"))
        .collect();
    assert_eq!(java_files.len(), 150);

    // Zero build files in plain-small.
    for forbidden in ["pom.xml", "build.gradle", "build.gradle.kts", "build.xml", "ivy.xml"] {
        assert!(
            !dir.join(forbidden).exists(),
            "plain-small must not contain build file {forbidden}"
        );
    }

    // Every generated .java file must parse with zero syntax errors.
    for path in &java_files {
        let text = fs::read_to_string(path).unwrap();
        let parse = syntax::parse(&text);
        assert!(
            parse.errors().is_empty(),
            "syntax errors in {}: {:?}",
            path.display(),
            parse.errors()
        );
    }

    // Verify controlled hierarchy shapes and large file LOC.
    for level in 0..=8 {
        assert!(dir.join(format!("src/bench/chain/Chain{level}.java")).is_file());
    }
    for name in ["RootIface", "LeftIface", "RightIface", "DiamondImpl"] {
        assert!(dir.join(format!("src/bench/diamond/{name}.java")).is_file());
    }
    assert!(dir.join("src/bench/wide/Handler.java").is_file());
    for idx in 1..=50 {
        assert!(dir.join(format!("src/bench/wide/Handler{idx:02}.java")).is_file());
    }

    let large_text = fs::read_to_string(dir.join("src/bench/app/LargeFile.java")).unwrap();
    assert!(
        large_text.lines().count() >= 1500,
        "LargeFile.java should be >= 1500 LOC, got {}",
        large_text.lines().count()
    );
}

// ---- 2. Embedded probes manifest & plain-small probe resolution ----

#[test]
fn embedded_manifest_defines_nine_repos_and_plain_small_probes_resolve() {
    let manifest = BenchManifest::embedded().unwrap();
    let ids: BTreeSet<&str> = manifest.repos.iter().map(|r| r.id.as_str()).collect();
    let expected = [
        "plain-small",
        "jackson-core",
        "gson",
        "commons-lang",
        "ant-ivy",
        "gradle-medium",
        "tomcat",
        "junit-framework",
        "error-prone",
    ];
    assert_eq!(manifest.repos.len(), 9);
    for id in expected {
        assert!(ids.contains(id), "missing repo `{id}` in probes.toml");
    }

    let plain = manifest
        .repos
        .iter()
        .find(|r| r.id == "plain-small")
        .unwrap();
    assert_eq!(plain.tier, Tier::Small);
    assert_eq!(plain.shapes, vec![Shape::Plain, Shape::SingleModule]);
    assert!(plain.url.is_empty());

    let dir = scratch("plain-small-probes");
    generate_plain_small(&dir).unwrap();

    for (op_id, spec) in &plain.probes {
        let path = dir.join(&spec.file);
        assert!(path.is_file(), "probe {op_id} missing file {}", spec.file);
        if !spec.anchor.is_empty() {
            let text = fs::read_to_string(&path).unwrap();
            let pos = resolve_anchor_utf16(&text, &spec.anchor, &spec.symbol);
            assert!(
                pos.is_ok(),
                "probe {op_id} failed to resolve anchor `{}` / `{}`: {pos:?}",
                spec.anchor,
                spec.symbol
            );
        }
    }

    for (op_id, edit) in &plain.edits {
        let path = dir.join(&edit.file);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            resolve_anchor_utf16(&text, &edit.anchor, &edit.symbol).is_ok(),
            "edit probe {op_id} anchor not found"
        );
        assert!(
            text.contains(&edit.replace),
            "edit probe {op_id} replace text `{}` missing",
            edit.replace
        );
    }

    for (op_id, comp) in &plain.completions {
        let path = dir.join(&comp.file);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains(&comp.anchor),
            "completion probe {op_id} anchor `{}` missing",
            comp.anchor
        );
        assert!(
            text.contains(&comp.replace),
            "completion probe {op_id} replace `{}` missing",
            comp.replace
        );
        let edited = text.replacen(&comp.replace, &comp.with, 1);
        assert!(
            resolve_cursor_after_utf16(&edited, &comp.with, &comp.cursor_after).is_ok(),
            "completion probe {op_id} cursor_after `{}` failed",
            comp.cursor_after
        );
    }
}

// ---- 3. UTF-16 anchor resolution ----

#[test]
fn resolve_anchor_utf16_handles_ascii_utf16_surrogates_and_errors() {
    let src = "\
package demo;
class Foo {
    String emoji = \"😀\"; int target = 42;
    String bmp = \"αβ\"; int afterBmp = target;
}
";
    // Line 2: `    String emoji = "😀"; int target = 42;`
    // UTF-16 length of `    String emoji = "😀"; int ` =
    // 4 + 16 (`String emoji = "`) + 2 (`😀`) + 7 (`"; int `) = 29
    let (line, col) = resolve_anchor_utf16(src, "int target = 42;", "target").unwrap();
    assert_eq!(line, 2);
    assert_eq!(col, 29);

    // Line 3: `    String bmp = "αβ"; int afterBmp = target;`
    // Each BMP char is 1 UTF-16 code unit:
    // `    String bmp = "αβ"; int ` = 4 + 14 + 2 + 7 = 27
    let (line_bmp, col_bmp) =
        resolve_anchor_utf16(src, "int afterBmp = target;", "afterBmp").unwrap();
    assert_eq!(line_bmp, 3);
    assert_eq!(col_bmp, 27);

    // Empty symbol resolves to start of anchor.
    let (line_empty, col_empty) = resolve_anchor_utf16(src, "int target = 42;", "").unwrap();
    assert_eq!(line_empty, 2);
    assert_eq!(col_empty, 25);

    // Missing anchor or symbol returns clear Err.
    assert!(resolve_anchor_utf16(src, "nonexistent", "x").is_err());
    assert!(resolve_anchor_utf16(src, "int target = 42;", "missingSym").is_err());
}

// ---- 4. Process-tree RSS parser ----

#[test]
fn parse_ps_tree_rss_kb_sums_multilevel_descendants_and_ignores_unrelated() {
    let ps_output = "\
  100     1   10240
  200   100   20480
  201   100    5120
  300   200    4096
  400   300    1024
  999     1   99999
 1000   999   50000
 bad_line
";
    // Root 100 + child 200 + child 201 + grandchild 300 + great-grandchild 400
    assert_eq!(
        parse_ps_tree_rss_kb(ps_output, 100),
        10240 + 20480 + 5120 + 4096 + 1024
    );
    // Subtree 200
    assert_eq!(parse_ps_tree_rss_kb(ps_output, 200), 20480 + 4096 + 1024);
    // Missing root
    assert_eq!(parse_ps_tree_rss_kb(ps_output, 777), 0);

    // Cycle protection
    let cyclic = "\
   10    20  1000
   20    10  2000
";
    assert_eq!(parse_ps_tree_rss_kb(cyclic, 10), 3000);
}

// ---- 5. Percentiles, OpStats, JSON v1, stdout & Markdown formatting ----

#[test]
fn percentile_stats_json_and_report_formatting_are_deterministic() {
    assert_eq!(percentile_ms(&[], 50.0), 0.0);
    assert_eq!(percentile_ms(&[42.0], 95.0), 42.0);
    let samples = [50.0, 10.0, 30.0, 20.0, 40.0];
    assert!((percentile_ms(&samples, 50.0) - 30.0).abs() < 1e-9);
    assert!((percentile_ms(&samples, 100.0) - 50.0).abs() < 1e-9);
    assert!((percentile_ms(&samples, 0.0) - 10.0).abs() < 1e-9);

    let supported = OpStats::from_samples(Some(12.5), &[4.0, 8.0, 6.0, 10.0], 5, 1, 3);
    assert_eq!(supported.status, OpStatus::Supported);
    assert_eq!(supported.first_ms, Some(12.5));
    assert!((supported.error_rate_pct - 20.0).abs() < 1e-9);
    assert!((supported.hit_rate_pct - 60.0).abs() < 1e-9);
    assert!((supported.p50_ms.unwrap() - 7.0).abs() < 1e-9);
    assert!((supported.max_ms.unwrap() - 10.0).abs() < 1e-9);

    let unsupported = OpStats::unsupported();
    assert_eq!(unsupported.status, OpStatus::Unsupported);
    assert_eq!(unsupported.p50_ms, None);

    let mut caps = BTreeMap::new();
    caps.insert("hover".to_string(), true);
    caps.insert("completion".to_string(), false);

    let mut ops = BTreeMap::new();
    ops.insert("hover_workspace".to_string(), supported);
    ops.insert("completion_dot_workspace".to_string(), unsupported);

    let mut server_versions = BTreeMap::new();
    server_versions.insert("java-analyzer".to_string(), "target/debug/java-analyzer lsp".to_string());

    let report = BenchReport {
        schema_version: REPORT_SCHEMA_VERSION,
        environment: EnvironmentInfo {
            os: "macos".to_string(),
            arch: "aarch64".to_string(),
            cpu_model: "Apple Silicon".to_string(),
            cpu_cores: 8,
            ram_gb: "32 GB".to_string(),
            java_version: "21.0.5".to_string(),
            revision: "abc12345".to_string(),
            date: "2026-11-23".to_string(),
            server_versions,
        },
        servers: vec!["java-analyzer".to_string()],
        results: vec![RepoServerResult {
            repo: "plain-small".to_string(),
            tier: Tier::Small,
            shapes: vec![Shape::Plain, Shape::SingleModule],
            files: 150,
            server: "java-analyzer".to_string(),
            capabilities: caps,
            lifecycle: LifecycleStats {
                cold_init_ms: 15.0,
                cold_first_response_ms: Some(32.0),
                cold_ready_ms: Some(45.0),
                warm_init_ms: Some(12.0),
                warm_first_response_ms: Some(25.0),
                warm_ready_ms: Some(30.0),
                peak_rss_mb: 28.4,
                steady_rss_mb: 26.1,
                clean_file_errors: 0,
            },
            total_requests: 5,
            total_errors: 1,
            ops,
        }],
    };

    let json = serde_json::to_string_pretty(&report).unwrap();
    let roundtrip: BenchReport = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.schema_version, 1);
    assert_eq!(roundtrip, report);

    let stdout = format_stdout_tables(&report);
    assert!(stdout.contains("Matrix 1: Lifecycle, First Response, Memory & Reliability"));
    assert!(stdout.contains("hover_workspace"));
    assert!(stdout.contains("completion_dot_workspace"));
    assert!(stdout.contains("N/A"));

    let md = format_markdown(&report);
    assert!(md.contains("## Environment & Revisions"));
    assert!(md.contains("## Matrix 1 — Lifecycle, First Response, Memory & Reliability"));
    assert!(md.contains("## Matrix 2 — Interactive Latency, Error Rate & Hit Rate"));
    assert!(md.contains("## Capability Coverage Matrix"));
    assert!(md.contains("supported"));
    assert!(md.contains("N/A"));
    assert_eq!(md, format_markdown(&report));
}

// ---- 6. CLI argument parsing ----

#[test]
fn cli_parses_bench_and_bench_gen_plain_args() {
    match cli::parse_args(args(&["bench"])).unwrap() {
        CliCommand::Bench(parsed) => {
            assert_eq!(*parsed, BenchArgs::default());
        }
        other => panic!("expected Bench, got {other:?}"),
    }

    match cli::parse_args(args(&[
        "bench",
        "--server",
        "ja=target/debug/java-analyzer lsp",
        "--repo",
        "plain-small=/tmp/plain",
        "--tier",
        "small",
        "--shape",
        "plain",
        "--ops",
        "nav,hierarchy,edit",
        "--probes",
        "custom/probes.toml",
        "--warmup",
        "2",
        "--iterations",
        "7",
        "--ready-timeout",
        "60",
        "--request-timeout",
        "10",
        "--skip-warm-restart",
        "--revision",
        "abcd1234",
        "--json-out",
        "out/r.json",
        "--md-out",
        "out/B.md",
    ]))
    .unwrap()
    {
        CliCommand::Bench(parsed) => {
            assert_eq!(
                parsed.servers,
                vec![ServerSpec {
                    name: "ja".to_string(),
                    cmd: "target/debug/java-analyzer lsp".to_string()
                }]
            );
            assert_eq!(
                parsed.repos,
                vec![("plain-small".to_string(), PathBuf::from("/tmp/plain"))]
            );
            assert_eq!(parsed.tiers, vec![Tier::Small]);
            assert_eq!(parsed.shapes, vec![Shape::Plain]);
            assert_eq!(
                parsed.ops,
                vec![OpCategory::Nav, OpCategory::Hierarchy, OpCategory::Edit]
            );
            assert_eq!(
                parsed.probes.as_deref(),
                Some(Path::new("custom/probes.toml"))
            );
            assert_eq!(parsed.warmup, 2);
            assert_eq!(parsed.iterations, 7);
            assert_eq!(parsed.ready_timeout_secs, 60);
            assert_eq!(parsed.request_timeout_secs, 10);
            assert!(parsed.skip_warm_restart);
            assert_eq!(parsed.revision.as_deref(), Some("abcd1234"));
            assert_eq!(parsed.json_out, Some(PathBuf::from("out/r.json")));
            assert_eq!(parsed.md_out, Some(PathBuf::from("out/B.md")));
            assert!(parsed.out.is_empty());
            assert_eq!(parsed.out_dir, PathBuf::from("target/bench"));
            assert_eq!(parsed.html_out, None);
        }
        other => panic!("expected Bench, got {other:?}"),
    }

    match cli::parse_args(args(&[
        "bench",
        "--out",
        "json",
        "--out",
        "markdown,csv",
        "--out-dir",
        "custom/out",
        "--html-out",
        "custom/h.html",
    ]))
    .unwrap()
    {
        CliCommand::Bench(parsed) => {
            assert_eq!(
                parsed.out,
                vec![OutputFormat::Json, OutputFormat::Md, OutputFormat::Csv]
            );
            assert_eq!(parsed.out_dir, PathBuf::from("custom/out"));
            assert_eq!(parsed.html_out, Some(PathBuf::from("custom/h.html")));
        }
        other => panic!("expected Bench, got {other:?}"),
    }

    match cli::parse_args(args(&["bench-plot"])).unwrap() {
        CliCommand::BenchPlot(parsed) => {
            assert_eq!(parsed, BenchPlotArgs::default());
        }
        other => panic!("expected BenchPlot, got {other:?}"),
    }

    match cli::parse_args(args(&[
        "bench-plot",
        "--input",
        "custom/in.json",
        "--out",
        "html,md",
        "--out-dir",
        "custom/plot",
        "--json-out",
        "custom/j.json",
        "--md-out",
        "custom/M.md",
        "--html-out",
        "custom/R.html",
    ]))
    .unwrap()
    {
        CliCommand::BenchPlot(parsed) => {
            assert_eq!(parsed.input, PathBuf::from("custom/in.json"));
            assert_eq!(parsed.out, vec![OutputFormat::Html, OutputFormat::Md]);
            assert_eq!(parsed.out_dir, PathBuf::from("custom/plot"));
            assert_eq!(parsed.json_out, Some(PathBuf::from("custom/j.json")));
            assert_eq!(parsed.md_out, Some(PathBuf::from("custom/M.md")));
            assert_eq!(parsed.html_out, Some(PathBuf::from("custom/R.html")));
        }
        other => panic!("expected BenchPlot, got {other:?}"),
    }

    assert_eq!(
        "markdown".parse::<OutputFormat>().unwrap(),
        OutputFormat::Md
    );
    assert_eq!("HTML".parse::<OutputFormat>().unwrap(), OutputFormat::Html);
    assert!("pdf".parse::<OutputFormat>().is_err());

    match cli::parse_args(args(&["bench-gen-plain", "--out", "/tmp/corpus"])).unwrap() {
        CliCommand::BenchGenPlain(parsed) => {
            assert_eq!(
                parsed,
                GenPlainArgs {
                    out: PathBuf::from("/tmp/corpus")
                }
            );
        }
        other => panic!("expected BenchGenPlain, got {other:?}"),
    }

    assert_eq!(
        cli::parse_args(args(&["bench", "--help"])).unwrap(),
        CliCommand::BenchHelp
    );
    assert_eq!(
        cli::parse_args(args(&["bench-plot", "--help"])).unwrap(),
        CliCommand::BenchPlotHelp
    );
    assert_eq!(
        cli::parse_args(args(&["bench-gen-plain", "-h"])).unwrap(),
        CliCommand::BenchGenPlainHelp
    );
    assert!(cli::parse_args(args(&["bench", "--iterations", "0"])).is_err());
    assert!(cli::parse_args(args(&["bench", "--tier", "huge"])).is_err());
    assert!(cli::parse_args(args(&["bench", "--out", "bogus"])).is_err());
    assert!(cli::parse_args(args(&["bench-plot", "--out", "bogus"])).is_err());
}

// ---- 7. End-to-end 4-phase harness test against real `java-analyzer lsp` ----

#[test]
fn end_to_end_four_phase_benchmark_against_java_analyzer_lsp() {
    // Build `java-analyzer` binary in debug mode so we test the real LSP end-to-end.
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap();
    let status = Command::new("cargo")
        .args(["build", "-p", "java-analyzer", "--bin", "java-analyzer"])
        .current_dir(workspace_root)
        .status()
        .unwrap();
    assert!(status.success(), "failed to build java-analyzer binary");

    let bin_path = workspace_root.join("target/debug/java-analyzer");
    assert!(bin_path.is_file(), "missing binary at {}", bin_path.display());

    let dir = scratch("e2e-java-analyzer");
    let corpus_dir = dir.join("plain-small");
    generate_plain_small(&corpus_dir).unwrap();

    let json_out = dir.join("results.json");
    let md_out = dir.join("BENCHMARKS.md");

    let bench_args = BenchArgs {
        servers: vec![ServerSpec {
            name: "java-analyzer".to_string(),
            cmd: format!("{} lsp", bin_path.display()),
        }],
        repos: vec![("plain-small".to_string(), corpus_dir)],
        tiers: Vec::new(),
        shapes: Vec::new(),
        ops: vec![
            OpCategory::Nav,
            OpCategory::Hierarchy,
            OpCategory::Completion,
            OpCategory::Edit,
        ],
        probes: None,
        warmup: 1,
        iterations: 2,
        ready_timeout_secs: 60,
        request_timeout_secs: 15,
        skip_warm_restart: false,
        revision: None,
        out: Vec::new(),
        out_dir: dir.clone(),
        json_out: Some(json_out.clone()),
        md_out: Some(md_out.clone()),
        html_out: None,
    };

    let code = bench::run(&bench_args);
    assert_eq!(code, 0, "bench::run failed");
    assert!(json_out.is_file());
    assert!(md_out.is_file());
    assert!(dir.join("lifecycle.csv").is_file());
    assert!(dir.join("ops.csv").is_file());
    assert!(dir.join("report.html").is_file());
    for svg_name in [
        "cold-first-response.svg",
        "ready-time.svg",
        "memory-rss.svg",
        "interactive-p50.svg",
    ] {
        assert!(
            dir.join("charts").join(svg_name).is_file(),
            "missing SVG chart {svg_name}"
        );
    }

    let parsed: BenchReport =
        serde_json::from_str(&fs::read_to_string(&json_out).unwrap()).unwrap();
    assert_eq!(parsed.schema_version, 1);
    assert_eq!(parsed.results.len(), 1);

    let res = &parsed.results[0];
    assert_eq!(res.repo, "plain-small");
    assert_eq!(res.files, 150);
    assert!(res.lifecycle.cold_init_ms >= 0.0);
    assert!(res.lifecycle.cold_first_response_ms.is_some());
    assert!(res.lifecycle.cold_ready_ms.is_some());
    assert!(res.lifecycle.warm_init_ms.is_some());
    assert!(res.lifecycle.warm_first_response_ms.is_some());
    assert!(res.lifecycle.warm_ready_ms.is_some());
    assert!(res.lifecycle.peak_rss_mb > 0.0);
    assert_eq!(res.lifecycle.clean_file_errors, 0);

    // Supported operations in `java-analyzer` should have measured stats & hits.
    for op in [
        "hover_workspace",
        "def_workspace",
        "type_def",
        "doc_symbol_small",
        "doc_symbol_large",
        "impl_simple",
        "impl_deep",
        "impl_wide",
        "hierarchy_super",
        "hierarchy_sub",
        "edit_body",
        "edit_signature",
    ] {
        let stats = res.ops.get(op).unwrap_or_else(|| panic!("missing op {op}"));
        assert_eq!(stats.status, OpStatus::Supported, "expected {op} supported");
        assert!(stats.first_ms.is_some(), "expected {op} first_ms");
        assert!(stats.p50_ms.is_some(), "expected {op} p50_ms");
        assert_eq!(stats.errors, 0, "expected 0 errors for {op}");
        assert_eq!(stats.hits, 2, "expected 2 hits for {op}");
    }

    // Unsupported operations in `java-analyzer` must be gated to N/A (`Unsupported`).
    for op in ["workspace_symbol", "completion_dot_workspace", "signature_help"] {
        let stats = res.ops.get(op).unwrap_or_else(|| panic!("missing op {op}"));
        assert_eq!(
            stats.status,
            OpStatus::Unsupported,
            "expected {op} unsupported"
        );
    }

    let md = fs::read_to_string(&md_out).unwrap();
    assert!(md.contains("plain-small"));
    assert!(md.contains("N/A"));
    assert!(md.contains("![Cold Time-to-First-Response](charts/cold-first-response.svg)"));
}

fn sample_multi_server_report() -> BenchReport {
    let mut caps_ja = BTreeMap::new();
    caps_ja.insert("hover".to_string(), true);
    caps_ja.insert("definition".to_string(), true);
    caps_ja.insert("implementation".to_string(), true);
    caps_ja.insert("documentSymbol".to_string(), true);
    caps_ja.insert("completion".to_string(), false);

    let mut ops_ja = BTreeMap::new();
    ops_ja.insert(
        "hover_workspace".to_string(),
        OpStats::from_samples(Some(4.2), &[0.25, 0.30, 0.35], 3, 0, 3),
    );
    ops_ja.insert(
        "def_workspace".to_string(),
        OpStats::from_samples(Some(0.3), &[0.05, 0.08, 0.12], 3, 0, 3),
    );
    ops_ja.insert(
        "impl_wide".to_string(),
        OpStats::from_samples(Some(0.6), &[0.15, 0.18, 0.22], 3, 0, 3),
    );
    ops_ja.insert(
        "doc_symbol_large".to_string(),
        OpStats::from_samples(Some(2.1), &[1.0, 1.2, 1.4], 3, 0, 3),
    );
    ops_ja.insert(
        "edit_body".to_string(),
        OpStats::from_samples(Some(0.5), &[0.3, 0.32, 0.4], 3, 0, 3),
    );
    ops_ja.insert("completion_dot_workspace".to_string(), OpStats::unsupported());

    let mut caps_jdt = caps_ja.clone();
    caps_jdt.insert("completion".to_string(), true);

    let mut ops_jdt = BTreeMap::new();
    ops_jdt.insert(
        "hover_workspace".to_string(),
        OpStats::from_samples(Some(120.0), &[15.0, 22.0, 35.0], 3, 0, 3),
    );
    ops_jdt.insert(
        "def_workspace".to_string(),
        OpStats::from_samples(Some(95.0), &[12.0, 18.0, 25.0], 3, 0, 3),
    );
    ops_jdt.insert("impl_wide".to_string(), OpStats::unsupported());

    let mut server_versions = BTreeMap::new();
    server_versions.insert("java-analyzer".to_string(), "0.1.0".to_string());
    server_versions.insert("jdtls".to_string(), "1.40".to_string());

    BenchReport {
        schema_version: REPORT_SCHEMA_VERSION,
        environment: EnvironmentInfo {
            os: "macos".to_string(),
            arch: "aarch64".to_string(),
            cpu_model: "Apple M4 Pro".to_string(),
            cpu_cores: 12,
            ram_gb: "24 GB".to_string(),
            java_version: "21.0.5".to_string(),
            revision: "rev </script> & <tag>".to_string(),
            date: "2026-11-23".to_string(),
            server_versions,
        },
        servers: vec!["java-analyzer".to_string(), "jdtls".to_string()],
        results: vec![
            RepoServerResult {
                repo: "plain, \"quoted\"".to_string(),
                tier: Tier::Small,
                shapes: vec![Shape::Plain, Shape::SingleModule],
                files: 150,
                server: "java-analyzer".to_string(),
                capabilities: caps_ja,
                lifecycle: LifecycleStats {
                    cold_init_ms: 17.5,
                    cold_first_response_ms: Some(245.4),
                    cold_ready_ms: Some(566.3),
                    warm_init_ms: Some(12.1),
                    warm_first_response_ms: Some(95.0),
                    warm_ready_ms: Some(140.0),
                    peak_rss_mb: 219.7,
                    steady_rss_mb: 160.7,
                    clean_file_errors: 0,
                },
                total_requests: 15,
                total_errors: 0,
                ops: ops_ja,
            },
            RepoServerResult {
                repo: "plain, \"quoted\"".to_string(),
                tier: Tier::Small,
                shapes: vec![Shape::Plain, Shape::SingleModule],
                files: 150,
                server: "jdtls".to_string(),
                capabilities: caps_jdt,
                lifecycle: LifecycleStats {
                    cold_init_ms: 420.0,
                    cold_first_response_ms: None,
                    cold_ready_ms: Some(4200.0),
                    warm_init_ms: None,
                    warm_first_response_ms: None,
                    warm_ready_ms: None,
                    peak_rss_mb: 850.2,
                    steady_rss_mb: 640.5,
                    clean_file_errors: 1,
                },
                total_requests: 6,
                total_errors: 0,
                ops: ops_jdt,
            },
        ],
    }
}

// ---- 8. CSV export, SVG charts, HTML report, and bench-plot offline regeneration ----

#[test]
fn csv_export_formats_headers_escaping_and_deterministic_order() {
    let report = sample_multi_server_report();
    let lifecycle_csv = format_lifecycle_csv(&report);
    let mut lines = lifecycle_csv.lines();
    assert_eq!(
        lines.next().unwrap(),
        "repo,tier,shapes,files,server,cold_init_ms,cold_first_response_ms,cold_ready_ms,warm_init_ms,warm_first_response_ms,warm_ready_ms,peak_rss_mb,steady_rss_mb,error_rate_pct,clean_file_errors"
    );
    let row1 = lines.next().unwrap();
    assert!(row1.starts_with("\"plain, \"\"quoted\"\"\",small,plain+single-module,150,java-analyzer,"));
    let row2 = lines.next().unwrap();
    assert!(row2.starts_with("\"plain, \"\"quoted\"\"\",small,plain+single-module,150,jdtls,"));
    assert!(lines.next().is_none());

    let ops_csv = format_ops_csv(&report);
    let mut op_lines = ops_csv.lines();
    assert_eq!(
        op_lines.next().unwrap(),
        "repo,server,op,status,first_ms,p50_ms,p95_ms,max_ms,error_rate_pct,hit_rate_pct,total_requests,errors,hits"
    );
    let ops_rows: Vec<&str> = op_lines.collect();
    assert_eq!(ops_rows.len(), 9);
    assert!(ops_rows.iter().any(|r| r.contains(",completion_dot_workspace,unsupported,,,,,")));
    assert_eq!(ops_csv, format_ops_csv(&report));
}

#[test]
fn static_svg_charts_render_all_four_valid_svgs() {
    let report = sample_multi_server_report();
    let charts = render_all_svg_charts(&report);
    assert_eq!(charts.len(), 4);

    for name in [
        "cold-first-response.svg",
        "ready-time.svg",
        "memory-rss.svg",
        "interactive-p50.svg",
    ] {
        let svg = charts.get(name).unwrap_or_else(|| panic!("missing {name}"));
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""), "{name}");
        assert!(svg.trim_end().ends_with("</svg>"), "{name}");
        assert!(svg.contains("@media (prefers-color-scheme: dark)"), "{name}");
        assert!(svg.contains("java-analyzer"), "{name}");
        assert!(svg.contains("jdtls"), "{name}");
        assert!(svg.contains("plain, &quot;quoted&quot;"), "{name} should XML-escape repo");
        assert!(!svg.contains("NaN"), "{name} must not contain NaN");
        assert!(!svg.contains("inf"), "{name} must not contain inf");
    }

    assert!(charts["cold-first-response.svg"].contains("N/A"));
    assert!(charts["ready-time.svg"].contains("N/A"));
    assert!(charts["interactive-p50.svg"].contains("0.08ms"));

    let md_with = format_markdown_with_charts(&report, Some("charts"));
    assert!(md_with.contains("## Charts"));
    assert!(md_with.contains("![Cold Time-to-First-Response](charts/cold-first-response.svg)"));
    assert!(md_with.contains("![Interactive Latency p50](charts/interactive-p50.svg)"));
    assert!(!format_markdown(&report).contains("charts/"));
}

#[test]
fn html_report_embeds_escaped_json_vega_lite_v5_and_accessible_tables() {
    let report = sample_multi_server_report();
    let html = format_html_report(&report);

    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<meta name=\"color-scheme\" content=\"light dark\">"));
    assert!(html.contains("light-dark("));
    assert!(html.contains(":root:has(> head > meta[name=\"color-scheme\"][content=\"dark\"])"));
    assert!(html.contains("https://cdn.jsdelivr.net/npm/vega@5"));
    assert!(html.contains("https://cdn.jsdelivr.net/npm/vega-lite@5"));
    assert!(html.contains("https://cdn.jsdelivr.net/npm/vega-embed@6"));
    assert!(html.contains("https://vega.github.io/schema/vega-lite/v5.json"));
    assert!(html.contains("role=\"region\""));
    assert!(html.contains("command=\"--sort\""));
    assert!(html.contains("aria-sort"));
    assert!(html.contains("role=\"status\""));
    assert!(html.contains("id=\"baseline-input\""));
    // Only executed tiers and shapes appear in filter dropdowns (unexecuted ones are omitted).
    assert!(html.contains("<option value=\"small\">small</option>"));
    assert!(!html.contains("<option value=\"medium\">medium</option>"));
    assert!(!html.contains("<option value=\"large\">large</option>"));
    assert!(html.contains("<option value=\"plain\">plain</option>"));
    assert!(html.contains("<option value=\"single-module\">single-module</option>"));
    assert!(!html.contains("<option value=\"maven\">maven</option>"));
    assert!(!html.contains("<option value=\"gradle\">gradle</option>"));
    assert!(!html.contains("<option value=\"ant\">ant</option>"));
    assert!(html.contains("activeScopeResults()"));
    assert!(html.contains("syncFilterOptionsToScope()"));
    assert!(html.contains("y2: { datum: floor }"));
    assert!(html.contains("resolve: { scale: { x: 'independent', y: 'independent' } }"));
    assert!(html.contains("class=\"cap-icon-yes\""));

    let marker = "<script id=\"bench-data\" type=\"application/json\">";
    let start = html.find(marker).expect("missing bench-data script") + marker.len();
    let end = html[start..].find("</script>").expect("missing closing script");
    let embedded = &html[start..start + end];
    assert!(embedded.contains("\\u003c/script>"));
    let parsed: BenchReport = serde_json::from_str(embedded).expect("valid embedded JSON");
    assert_eq!(parsed.schema_version, report.schema_version);
    assert_eq!(parsed.environment, report.environment);
    assert_eq!(parsed.servers, report.servers);
    assert_eq!(parsed.results.len(), report.results.len());
    assert_eq!(parsed.results[0].repo, report.results[0].repo);
    assert!((parsed.results[0].ops["def_workspace"].p95_ms.unwrap() - 0.116).abs() < 1e-12);
}

#[test]
fn bench_plot_regenerates_only_selected_or_all_formats_offline() {
    let dir = scratch("bench-plot-offline");
    let input_path = dir.join("saved-results.json");
    let report = sample_multi_server_report();
    fs::write(&input_path, serde_json::to_string_pretty(&report).unwrap()).unwrap();

    // 1. Only HTML selected
    let html_only_dir = dir.join("html-only");
    let code = bench::run_bench_plot(&BenchPlotArgs {
        input: input_path.clone(),
        out: vec![OutputFormat::Html],
        out_dir: html_only_dir.clone(),
        json_out: None,
        md_out: None,
        html_out: None,
    });
    assert_eq!(code, 0);
    assert!(html_only_dir.join("report.html").is_file());
    assert!(!html_only_dir.join("results.json").exists());
    assert!(!html_only_dir.join("lifecycle.csv").exists());
    assert!(!html_only_dir.join("BENCHMARKS.md").exists());

    // 2. Only MD + CSV selected
    let md_csv_dir = dir.join("md-csv");
    let code = bench::run_bench_plot(&BenchPlotArgs {
        input: input_path.clone(),
        out: vec![OutputFormat::Md, OutputFormat::Csv],
        out_dir: md_csv_dir.clone(),
        json_out: None,
        md_out: None,
        html_out: None,
    });
    assert_eq!(code, 0);
    assert!(md_csv_dir.join("lifecycle.csv").is_file());
    assert!(md_csv_dir.join("ops.csv").is_file());
    assert!(md_csv_dir.join("BENCHMARKS.md").is_file());
    assert!(md_csv_dir.join("charts/cold-first-response.svg").is_file());
    assert!(!md_csv_dir.join("results.json").exists());
    assert!(!md_csv_dir.join("report.html").exists());

    // 3. Omitted --out emits all 4 formats
    let all_dir = dir.join("all");
    let code = bench::run_bench_plot(&BenchPlotArgs {
        input: input_path,
        out: Vec::new(),
        out_dir: all_dir.clone(),
        json_out: None,
        md_out: None,
        html_out: None,
    });
    assert_eq!(code, 0);
    assert!(all_dir.join("results.json").is_file());
    assert!(all_dir.join("lifecycle.csv").is_file());
    assert!(all_dir.join("ops.csv").is_file());
    assert!(all_dir.join("BENCHMARKS.md").is_file());
    assert!(all_dir.join("charts/interactive-p50.svg").is_file());
    assert!(all_dir.join("report.html").is_file());
}
