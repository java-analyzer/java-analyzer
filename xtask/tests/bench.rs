use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use xtask::{
    bench::{
        self, BenchArgs, GenPlainArgs, ServerSpec,
        probes::{
            BenchManifest, OpCategory, Shape, Tier, resolve_anchor_utf16,
            resolve_cursor_after_utf16,
        },
        report::{
            BenchReport, EnvironmentInfo, LifecycleStats, OpStats, OpStatus,
            REPORT_SCHEMA_VERSION, RepoServerResult, format_markdown, format_stdout_tables,
            percentile_ms,
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
            assert_eq!(parsed.json_out, PathBuf::from("out/r.json"));
            assert_eq!(parsed.md_out, PathBuf::from("out/B.md"));
        }
        other => panic!("expected Bench, got {other:?}"),
    }

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
        cli::parse_args(args(&["bench-gen-plain", "-h"])).unwrap(),
        CliCommand::BenchGenPlainHelp
    );
    assert!(cli::parse_args(args(&["bench", "--iterations", "0"])).is_err());
    assert!(cli::parse_args(args(&["bench", "--tier", "huge"])).is_err());
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
        json_out: json_out.clone(),
        md_out: md_out.clone(),
    };

    let code = bench::run(&bench_args);
    assert_eq!(code, 0, "bench::run failed");
    assert!(json_out.is_file());
    assert!(md_out.is_file());

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
}
