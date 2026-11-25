#![expect(
    clippy::unwrap_used,
    reason = "integration test helper functions"
)]

use std::{fs, path::Path};

use xtask::{
    cli::{self, Command},
    report::{
        self, ClippyStatus, FindingKind, ReportArgs, Verdict, civil_from_days, current_utc_date,
    },
};

/// A scratch directory under the target directory, removed first so reruns start clean.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("report-tests")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, relative: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    path
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ---- 1. CLI argument parsing ----

#[test]
fn cli_parses_report_defaults_and_overrides() {
    match cli::parse_args(args(&["report"])).unwrap() {
        Command::Report(parsed) => {
            assert_eq!(parsed, ReportArgs::default());
            assert_eq!(
                parsed.conformance,
                Path::new("target/conformance/report.json")
            );
            assert_eq!(parsed.workspace_corpus, Path::new("target/workspace-corpus"));
            assert_eq!(parsed.test_output, None);
            assert_eq!(parsed.clippy_status, ClippyStatus::Skipped);
            assert_eq!(parsed.known_findings, None);
            assert_eq!(parsed.revision, None);
            assert_eq!(parsed.date, None);
            assert_eq!(parsed.out, Path::new("target/conformance/CONFORMANCE.md"));
            assert!(!parsed.strict);
        }
        other => panic!("expected Report, got {other:?}"),
    }

    match cli::parse_args(args(&[
        "report",
        "--conformance",
        "custom/report.json",
        "--workspace-corpus",
        "custom/wc",
        "--test-output",
        "custom/tests.txt",
        "--clippy-status",
        "pass",
        "--known-findings",
        "custom/known.toml",
        "--revision",
        "abcd1234",
        "--date",
        "2026-11-22",
        "--out",
        "CONFORMANCE.md",
        "--strict",
    ]))
    .unwrap()
    {
        Command::Report(parsed) => {
            assert_eq!(parsed.conformance, Path::new("custom/report.json"));
            assert_eq!(parsed.workspace_corpus, Path::new("custom/wc"));
            assert_eq!(parsed.test_output.as_deref(), Some(Path::new("custom/tests.txt")));
            assert_eq!(parsed.clippy_status, ClippyStatus::Pass);
            assert_eq!(
                parsed.known_findings.as_deref(),
                Some(Path::new("custom/known.toml"))
            );
            assert_eq!(parsed.revision.as_deref(), Some("abcd1234"));
            assert_eq!(parsed.date.as_deref(), Some("2026-11-22"));
            assert_eq!(parsed.out, Path::new("CONFORMANCE.md"));
            assert!(parsed.strict);
        }
        other => panic!("expected Report, got {other:?}"),
    }

    assert_eq!(
        cli::parse_args(args(&["report", "-h"])).unwrap(),
        Command::ReportHelp
    );
    assert_eq!(
        cli::parse_args(args(&["report", "--help"])).unwrap(),
        Command::ReportHelp
    );
    assert_eq!(cli::run(args(&["report", "--help"])), 0);

    for bad in [
        &["report", "--bogus"][..],
        &["report", "--out"],
        &["report", "--conformance"],
        &["report", "--clippy-status", "maybe"],
        &["report", "extra-positional"],
    ] {
        assert!(cli::parse_args(args(bad)).is_err(), "{bad:?}");
        assert_eq!(cli::run(args(bad)), 2, "{bad:?}");
    }
}

// ---- 2. cargo test output parser ----

#[test]
fn parses_cargo_test_output_with_unit_integration_zero_doctests_and_failures() {
    let raw = "\
   Compiling syntax v0.1.0
     Running unittests src/lib.rs (target/debug/deps/hir-089b25eba5c048ce)

running 28 tests
test tests::one ... ok

test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/lib.rs (target/debug/deps/java_analyzer-8a5cf349202fc80e)

running 47 tests
test convert::tests::one ... ok

test result: ok. 46 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.06s

     Running unittests src/main.rs (target/debug/deps/java_analyzer-40280854128001d7)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lsp_diagnostics.rs (target/debug/deps/lsp_diagnostics-bfff2260ff631f97)

running 5 tests
test syntax_error ... ok

test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

   Doc-tests hir

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests syntax

running 2 tests
test src/lib.rs - parse (line 10) ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
";

    let summary = report::parse_cargo_test_output(raw).unwrap();
    assert_eq!(summary.suites.len(), 6);

    assert_eq!(summary.suites[0].krate, "hir");
    assert_eq!(summary.suites[0].suite, "Unit tests (src/lib.rs)");
    assert_eq!(summary.suites[0].passed, 28);
    assert_eq!(summary.suites[0].failed, 0);

    assert_eq!(summary.suites[1].krate, "java-analyzer");
    assert_eq!(summary.suites[1].suite, "Unit tests (src/lib.rs)");
    assert_eq!(summary.suites[1].passed, 46);
    assert_eq!(summary.suites[1].failed, 1);
    assert_eq!(summary.suites[1].ignored, 2);

    assert_eq!(summary.suites[2].krate, "java-analyzer");
    assert_eq!(summary.suites[2].suite, "Unit tests (src/main.rs)");
    assert_eq!(summary.suites[2].passed, 0);

    assert_eq!(summary.suites[3].krate, "java-analyzer");
    assert_eq!(summary.suites[3].suite, "lsp_diagnostics");
    assert_eq!(summary.suites[3].passed, 4);
    assert_eq!(summary.suites[3].failed, 1);

    assert_eq!(summary.suites[4].krate, "hir");
    assert_eq!(summary.suites[4].suite, "Doc-tests");
    assert_eq!(summary.suites[4].passed, 0);

    assert_eq!(summary.suites[5].krate, "syntax");
    assert_eq!(summary.suites[5].suite, "Doc-tests");
    assert_eq!(summary.suites[5].passed, 2);

    assert_eq!(summary.total_passed, 80);
    assert_eq!(summary.total_failed, 2);
    assert_eq!(summary.total_ignored, 2);

    // Zero-test doc-tests and zero-test binaries should be omitted from Markdown.
    let md = report::render_test_section(ClippyStatus::Pass, Some(&summary));
    assert!(md.contains("| `hir` | Unit tests (src/lib.rs) | 28 | 0 | 0 |"));
    assert!(md.contains("| `java-analyzer` | `lsp_diagnostics` | 4 | 1 | 0 |"));
    assert!(md.contains("| `syntax` | Doc-tests | 2 | 0 | 0 |"));
    assert!(!md.contains("Unit tests (src/main.rs)"));
    assert!(!md.contains("| `hir` | Doc-tests |"));
    assert!(md.contains("| **Total** | | **80** | **2** | **2** |"));
}

// ---- 3. report.json ingestion ----

#[test]
fn parses_conformance_json_clean_divergences_gaps_and_critical() {
    let json = r#"{
  "schema": 1,
  "sources": [
    {
      "id": "commons-lang",
      "dir": "target/conformance/corpus/commons-lang",
      "kind": "valid",
      "skipped": null,
      "files": 249,
      "conforming": 249,
      "critical": [],
      "gaps": [],
      "knownDivergences": [],
      "skippedFiles": []
    },
    {
      "id": "jdk-27-ga",
      "dir": "target/conformance/jdk/jdk-27-ga",
      "kind": "valid",
      "skipped": null,
      "files": 100,
      "conforming": 93,
      "critical": [
        { "path": "Crash.java", "outcome": "panic", "message": "boom" }
      ],
      "gaps": [
        { "path": "MissingFeature.java", "outcome": "errors", "errors": 1, "first": ["3:5 expected ';'"] }
      ],
      "knownDivergences": [
        { "path": "IBM33722.java", "reason": "Exceeds MAX_CHAIN" },
        { "path": "CollationData_ja.java", "reason": "Exceeds MAX_CHAIN" },
        { "path": "CollationData_ko.java", "reason": "Exceeds MAX_CHAIN" },
        { "path": "CollationData_zh.java", "reason": "Exceeds MAX_CHAIN" },
        { "path": "CollationData_zh_TW.java", "reason": "Exceeds MAX_CHAIN" }
      ],
      "skippedFiles": []
    }
  ]
}"#;

    let summary = report::parse_conformance_json(json).unwrap();
    assert_eq!(summary.sources.len(), 2);
    assert_eq!(summary.total_files, 349);
    assert_eq!(summary.total_conforming, 342);
    assert_eq!(summary.total_critical, 1);
    assert_eq!(summary.total_gaps, 1);
    assert_eq!(summary.total_known_divergences, 5);

    let md = report::render_conformance_section(Path::new("report.json"), Some(&summary));
    assert!(md.contains("| `commons-lang` | valid | 249 | 249 | 100.0% | 0 | Clean |"));
    // Effective conforming rate for jdk-27-ga excludes 5 known divergences: 93 / 95 = 97.9%*
    assert!(md.contains("| `jdk-27-ga` | valid | 100 | 93 | 97.9%* | 1 | 1 critical, 1 unexpected gap(s), 5 expected exclusion(s) |"), "{md}");
    assert!(md.contains("Crash.java"));
    assert!(md.contains("MissingFeature.java"));
    assert!(md.contains("IBM33722.java"));

    let bad_schema = r#"{"schema": 2, "sources": []}"#;
    assert!(report::parse_conformance_json(bad_schema).is_err());
}

// ---- 4. Workspace-corpus ingestion & known-findings matching ----

#[test]
fn embedded_known_findings_toml_is_valid() {
    let rules = report::parse_known_findings(report::EMBEDDED_KNOWN_FINDINGS).unwrap();
    assert_eq!(rules.len(), 8);
    assert_eq!(rules[0].repo, "gson");
    assert_eq!(rules[0].kind, FindingKind::ParseError);
    assert_eq!(rules[3].repo, "error-prone");
    assert_eq!(rules[3].kind, FindingKind::PackageMismatch);
    assert!(rules[3].path_prefix.ends_with('/'));
}

#[test]
fn workspace_corpus_preserves_timings_order_and_classifies_findings() {
    let dir = scratch("workspace-corpus");
    // Deliberately non-alphabetical order in timings.tsv.
    write(
        &dir,
        "timings.tsv",
        "gson\t774\nerror-prone\t4723\nmy-repo\t120\n",
    );

    write(
        &dir,
        "gson.json",
        r#"{
  "active_build_tool": "maven",
  "active_language_level": { "release": 8, "preview": false },
  "conflicts": [],
  "modules": [{}, {}],
  "package_mismatches": [],
  "health": {
    "total_java_files": 268,
    "files_with_parse_errors": 2,
    "indexed_packages": 10,
    "indexed_types": 736,
    "parse_errors": [
      {
        "path": "gson/src/test/java/com/google/gson/functional/Java17RecordTest.java",
        "error_count": 1,
        "first_error": "records require language level 16"
      },
      {
        "path": "test-graal-native-image/src/test/java/com/google/gson/native_test/Java17RecordReflectionTest.java",
        "error_count": 1,
        "first_error": "records require language level 16"
      }
    ]
  }
}"#,
    );

    // Prefix match grouping (2 files under error-prone refaster input/) + 1 unexpected mismatch.
    write(
        &dir,
        "error-prone.json",
        r#"{
  "active_build_tool": "maven",
  "active_language_level": { "release": 27, "preview": false },
  "conflicts": [],
  "modules": [{}],
  "package_mismatches": [
    {
      "path": "core/src/test/java/com/google/errorprone/refaster/testdata/input/One.java",
      "declared": "com.google.errorprone.refaster.testdata",
      "expected": "com.google.errorprone.refaster.testdata.input",
      "message": "package declaration mismatch"
    },
    {
      "path": "core\\src\\test\\java\\com\\google\\errorprone\\refaster\\testdata\\input\\Two.java",
      "declared": "com.google.errorprone.refaster.testdata",
      "expected": "com.google.errorprone.refaster.testdata.input",
      "message": "package declaration mismatch"
    },
    {
      "path": "core/src/main/java/UnexpectedMismatch.java",
      "declared": "wrong.pkg",
      "expected": "right.pkg",
      "message": "declared `wrong.pkg`, expected `right.pkg`"
    }
  ],
  "health": {
    "total_java_files": 100,
    "files_with_parse_errors": 0,
    "indexed_packages": 5,
    "indexed_types": 200,
    "parse_errors": []
  }
}"#,
    );

    // Conflict + unexpected parse error.
    write(
        &dir,
        "my-repo.json",
        r#"{
  "active_build_tool": "gradle",
  "active_language_level": { "release": 21, "preview": false },
  "conflicts": [
    { "kind": "build_tool", "message": "both pom.xml and build.gradle found" }
  ],
  "modules": [{}],
  "package_mismatches": [],
  "health": {
    "total_java_files": 50,
    "files_with_parse_errors": 1,
    "indexed_packages": 2,
    "indexed_types": 80,
    "parse_errors": [
      {
        "path": "src/main/java/Broken.java",
        "error_count": 1,
        "first_error": "expected ';'"
      }
    ]
  }
}"#,
    );

    let rules = report::parse_known_findings(report::EMBEDDED_KNOWN_FINDINGS).unwrap();
    let summary = report::load_workspace_corpus(&dir, &rules)
        .unwrap()
        .expect("corpus exists");

    let repo_ids: Vec<_> = summary.repos.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(repo_ids, ["gson", "error-prone", "my-repo"]);

    // 2 exact gson parse errors + 1 grouped error-prone rule (count 2).
    assert_eq!(summary.known_findings.len(), 3);
    let ep_group = summary
        .known_findings
        .iter()
        .find(|g| g.rule.repo == "error-prone")
        .unwrap();
    assert_eq!(ep_group.count, 2);

    // Unexpected: 1 package_mismatch in error-prone, 1 conflict + 1 parse_error in my-repo.
    assert_eq!(summary.unexpected_findings.len(), 3);
    assert!(summary
        .unexpected_findings
        .iter()
        .any(|f| f.repo == "error-prone" && f.kind == FindingKind::PackageMismatch && f.path == "core/src/main/java/UnexpectedMismatch.java"));
    assert!(summary
        .unexpected_findings
        .iter()
        .any(|f| f.repo == "my-repo" && f.kind == FindingKind::ParseError && f.path == "src/main/java/Broken.java"));
    assert!(summary
        .unexpected_findings
        .iter()
        .any(|f| f.repo == "my-repo" && f.kind == FindingKind::Conflict));

    let md = report::render_workspace_section(Some(&summary));
    let gson_pos = md.find("| `gson` |").unwrap();
    let ep_pos = md.find("| `error-prone` |").unwrap();
    let my_pos = md.find("| `my-repo` |").unwrap();
    assert!(gson_pos < ep_pos && ep_pos < my_pos, "timings.tsv order must be preserved");
    assert!(md.contains("2 matches under `core/src/test/java/com/google/errorprone/refaster/testdata/`"));
    assert!(md.contains("core/src/main/java/UnexpectedMismatch.java"));
    assert!(md.contains("both pom.xml and build.gradle found"));
}

// ---- 5. Graceful handling of missing optional inputs ----

#[test]
fn missing_optional_inputs_render_not_run_and_exit_zero() {
    let dir = scratch("missing-optional");
    let out = dir.join("CONFORMANCE.md");
    let missing_conf = dir.join("nope/report.json");
    let missing_wc = dir.join("nope/workspace-corpus");

    let code = cli::run(args(&[
        "report",
        "--conformance",
        missing_conf.to_str().unwrap(),
        "--workspace-corpus",
        missing_wc.to_str().unwrap(),
        "--date",
        "2026-11-22",
        "--out",
        out.to_str().unwrap(),
    ]));
    assert_eq!(code, 0);

    let md = fs::read_to_string(&out).unwrap();
    assert!(md.contains("*Not run*"), "{md}");
    assert!(
        md.contains(&format!("*Not run (`{}` not found)*", missing_conf.display())),
        "{md}"
    );

    // Workspace corpus directory exists but has no timings.tsv -> Not run.
    let wc_no_tsv = dir.join("empty-wc");
    fs::create_dir_all(&wc_no_tsv).unwrap();
    let code = cli::run(args(&[
        "report",
        "--conformance",
        missing_conf.to_str().unwrap(),
        "--workspace-corpus",
        wc_no_tsv.to_str().unwrap(),
        "--clippy-status",
        "pass",
        "--date",
        "2026-11-22",
        "--out",
        out.to_str().unwrap(),
    ]));
    assert_eq!(code, 0);
    let md2 = fs::read_to_string(&out).unwrap();
    assert!(md2.contains("`cargo clippy --workspace --all-targets -- -D warnings` (pass"));
}

// ---- 6. Verdict calculation & --strict exit codes ----

fn write_clean_conformance(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        r#"{
  "schema": 1,
  "sources": [
    {
      "id": "commons-lang",
      "kind": "valid",
      "skipped": null,
      "files": 10,
      "conforming": 10,
      "critical": [],
      "gaps": [],
      "knownDivergences": [],
      "skippedFiles": []
    }
  ]
}"#,
    )
    .unwrap();
}

fn write_clean_tests(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        "\
     Running unittests src/lib.rs (target/debug/deps/syntax-11111111)

running 5 tests

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
",
    )
    .unwrap();
}

fn write_single_repo_corpus(dir: &Path, json_body: &str) {
    write(dir, "timings.tsv", "demo\t50\n");
    write(dir, "demo.json", json_body);
}

#[test]
fn strict_mode_exit_codes_and_verdict_calculation() {
    let dir = scratch("strict");
    let conf = dir.join("report.json");
    let tests = dir.join("test-output.txt");
    let wc = dir.join("wc");
    let out = dir.join("CONFORMANCE.md");

    write_clean_conformance(&conf);
    write_clean_tests(&tests);
    // Only a known finding (gson Java17RecordTest) -> should PASS strict.
    write(&wc, "timings.tsv", "gson\t100\n");
    write(
        &wc,
        "gson.json",
        r#"{
  "active_build_tool": "maven",
  "active_language_level": { "release": 8, "preview": false },
  "conflicts": [],
  "modules": [{}],
  "package_mismatches": [],
  "health": {
    "total_java_files": 10,
    "files_with_parse_errors": 1,
    "indexed_packages": 1,
    "indexed_types": 10,
    "parse_errors": [
      {
        "path": "gson/src/test/java/com/google/gson/functional/Java17RecordTest.java",
        "error_count": 1,
        "first_error": "records require language level 16"
      }
    ]
  }
}"#,
    );

    // 1) Clean run with --strict -> exit 0, PASS.
    let code = cli::run(args(&[
        "report",
        "--conformance",
        conf.to_str().unwrap(),
        "--workspace-corpus",
        wc.to_str().unwrap(),
        "--test-output",
        tests.to_str().unwrap(),
        "--clippy-status",
        "pass",
        "--date",
        "2026-11-22",
        "--out",
        out.to_str().unwrap(),
        "--strict",
    ]));
    assert_eq!(code, 0);
    assert!(fs::read_to_string(&out).unwrap().contains("**PASS**"));

    // 2) Clippy fail -> default exits 0, --strict exits 1, verdict FAIL.
    let code_default = cli::run(args(&[
        "report",
        "--conformance",
        conf.to_str().unwrap(),
        "--workspace-corpus",
        wc.to_str().unwrap(),
        "--test-output",
        tests.to_str().unwrap(),
        "--clippy-status",
        "fail",
        "--out",
        out.to_str().unwrap(),
    ]));
    assert_eq!(code_default, 0);
    assert!(fs::read_to_string(&out).unwrap().contains("**FAIL**"));

    let code_strict = cli::run(args(&[
        "report",
        "--conformance",
        conf.to_str().unwrap(),
        "--workspace-corpus",
        wc.to_str().unwrap(),
        "--test-output",
        tests.to_str().unwrap(),
        "--clippy-status",
        "fail",
        "--out",
        out.to_str().unwrap(),
        "--strict",
    ]));
    assert_eq!(code_strict, 1);

    // 3) Unexpected package_mismatch in workspace corpus -> --strict exits 1!
    let wc_mismatch = dir.join("wc-mismatch");
    write_single_repo_corpus(
        &wc_mismatch,
        r#"{
  "active_build_tool": "maven",
  "active_language_level": { "release": 21, "preview": false },
  "conflicts": [],
  "modules": [{}],
  "package_mismatches": [
    {
      "path": "src/main/java/BadPkg.java",
      "declared": "a",
      "expected": "b",
      "message": "declared `a`, expected `b`"
    }
  ],
  "health": {
    "total_java_files": 10,
    "files_with_parse_errors": 0,
    "indexed_packages": 1,
    "indexed_types": 10,
    "parse_errors": []
  }
}"#,
    );
    let code_mismatch = cli::run(args(&[
        "report",
        "--conformance",
        conf.to_str().unwrap(),
        "--workspace-corpus",
        wc_mismatch.to_str().unwrap(),
        "--test-output",
        tests.to_str().unwrap(),
        "--clippy-status",
        "pass",
        "--out",
        out.to_str().unwrap(),
        "--strict",
    ]));
    assert_eq!(code_mismatch, 1);
    assert!(fs::read_to_string(&out).unwrap().contains("**FAIL**"));

    // 4) Failed test -> --strict exits 1.
    let failing_tests = dir.join("failing-tests.txt");
    fs::write(
        &failing_tests,
        "\
     Running unittests src/lib.rs (target/debug/deps/syntax-11111111)

running 5 tests

test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
",
    )
    .unwrap();
    let code_test_fail = cli::run(args(&[
        "report",
        "--conformance",
        conf.to_str().unwrap(),
        "--workspace-corpus",
        wc.to_str().unwrap(),
        "--test-output",
        failing_tests.to_str().unwrap(),
        "--clippy-status",
        "pass",
        "--out",
        out.to_str().unwrap(),
        "--strict",
    ]));
    assert_eq!(code_test_fail, 1);

    // 5) Unexpected conformance gap -> --strict exits 1.
    let gap_conf = dir.join("gap-report.json");
    fs::write(
        &gap_conf,
        r#"{
  "schema": 1,
  "sources": [
    {
      "id": "jdk",
      "kind": "valid",
      "skipped": null,
      "files": 10,
      "conforming": 9,
      "critical": [],
      "gaps": [{ "path": "Gap.java", "outcome": "errors", "errors": 1, "first": ["1:1 err"] }],
      "knownDivergences": [],
      "skippedFiles": []
    }
  ]
}"#,
    )
    .unwrap();
    let code_gap = cli::run(args(&[
        "report",
        "--conformance",
        gap_conf.to_str().unwrap(),
        "--workspace-corpus",
        wc.to_str().unwrap(),
        "--test-output",
        tests.to_str().unwrap(),
        "--clippy-status",
        "pass",
        "--out",
        out.to_str().unwrap(),
        "--strict",
    ]));
    assert_eq!(code_gap, 1);
}

// ---- 7. Deterministic Markdown output & UTC date helper ----

#[test]
fn utc_civil_date_formula_is_accurate() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(19782), (2024, 2, 29));
    assert_eq!(civil_from_days(20779), (2026, 11, 22));
    let today = current_utc_date();
    assert_eq!(today.len(), 10);
    assert_eq!(&today[4..5], "-");
    assert_eq!(&today[7..8], "-");
}

#[test]
fn markdown_output_is_deterministic_and_matches_snapshot() {
    let dir = scratch("snapshot");
    let conf = dir.join("report.json");
    let tests = dir.join("test-output.txt");
    let wc = dir.join("wc");
    let out1 = dir.join("CONFORMANCE_1.md");
    let out2 = dir.join("CONFORMANCE_2.md");

    write_clean_conformance(&conf);
    write_clean_tests(&tests);
    write_single_repo_corpus(
        &wc,
        r#"{
  "active_build_tool": "none",
  "active_language_level": { "release": 27, "preview": false },
  "conflicts": [],
  "modules": [{}],
  "package_mismatches": [],
  "health": {
    "total_java_files": 14,
    "files_with_parse_errors": 0,
    "indexed_packages": 1,
    "indexed_types": 24,
    "parse_errors": []
  }
}"#,
    );

    let report1 = report::generate(&ReportArgs {
        conformance: conf.clone(),
        workspace_corpus: wc.clone(),
        diag_conformance: None,
        test_output: Some(tests.clone()),
        clippy_status: ClippyStatus::Pass,
        known_findings: None,
        revision: Some("qtypxoky (eb01ba88, Spec 0017)".to_string()),
        date: Some("2026-11-22".to_string()),
        out: out1.clone(),
        strict: true,
    })
    .unwrap();
    assert_eq!(report1.verdict(), Verdict::Pass);

    cli::run(args(&[
        "report",
        "--conformance",
        conf.to_str().unwrap(),
        "--workspace-corpus",
        wc.to_str().unwrap(),
        "--test-output",
        tests.to_str().unwrap(),
        "--clippy-status",
        "pass",
        "--revision",
        "qtypxoky (eb01ba88, Spec 0017)",
        "--date",
        "2026-11-22",
        "--out",
        out1.to_str().unwrap(),
    ]));
    cli::run(args(&[
        "report",
        "--conformance",
        conf.to_str().unwrap(),
        "--workspace-corpus",
        wc.to_str().unwrap(),
        "--test-output",
        tests.to_str().unwrap(),
        "--clippy-status",
        "pass",
        "--revision",
        "qtypxoky (eb01ba88, Spec 0017)",
        "--date",
        "2026-11-22",
        "--out",
        out2.to_str().unwrap(),
    ]));

    let md1 = fs::read_to_string(&out1).unwrap();
    let md2 = fs::read_to_string(&out2).unwrap();
    assert_eq!(md1, md2);

    let expected = "\
# Java Analyzer Conformance & Verification Report

**Date:** 2026-11-22  
**Workspace Revision:** `qtypxoky (eb01ba88, Spec 0017)`  
**Overall Verdict:** **PASS** — 0 test failures, 0 Clippy warnings, 0 critical findings, and **0 unexpected findings** across **24 Java files**.

---

## 1. Static Analysis & Workspace Test Suite

Verified with `cargo clippy --workspace --all-targets -- -D warnings` (pass — 0 warnings) and `cargo test --workspace --all-targets`.

| Crate | Test Suite | Passed | Failed | Ignored |
|---|---|---:|---:|---:|
| `syntax` | Unit tests (src/lib.rs) | 5 | 0 | 0 |
| **Total** | | **5** | **0** | **0** |

---

## 2. Parser Conformance Corpus (`mise run conformance-jdk`)

Executed across 1 corpus (**10 Java files**, **10 conforming**).

| Corpus | Kind | Files | Conforming | Rate | Critical | Notes |
|---|---|---:|---:|---:|---:|---|
| `commons-lang` | valid | 10 | 10 | 100.0% | 0 | Clean |
| **Total** | | **10** | **10** | **100.0%** | **0** | **0 unexpected gaps** |

---

## 3. Workspace Structure & Indexing Corpus (`mise run workspace-corpus`)

Executed `java-analyzer inspect --json --show-errors` across 1 repository (**14 Java files**, **24 indexed types**, **0 conflicts**, **50 ms** total).

| Repository | Build | Release | Modules | Files | Types | Parse Errors | Pkg Mismatches | Conflicts | Time (ms) |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| `demo` | none | Java 27 | 1 | 14 | 24 | 0 | 0 | 0 | 50 |
| **Total** | | | **1** | **14** | **24** | **0** | **0** | **0** | **50** |

### Unexpected Findings

None.
";

    assert_eq!(md1, expected);
}

#[test]
fn diag_conformance_section_renders_when_present_and_omits_gracefully_when_absent() {
    let dir = scratch("diag-conformance-report");
    let conf = dir.join("report.json");
    let diag_json = dir.join("diagnostics.json");
    let missing_diag_json = dir.join("missing-diagnostics.json");
    let out_with = dir.join("WITH_DIAG.md");
    let out_absent = dir.join("WITHOUT_DIAG.md");

    write_clean_conformance(&conf);
    write(
        &dir,
        "diagnostics.json",
        r#"{
  "schema_version": 1,
  "status": "ok",
  "skip_reason": null,
  "phase": "syntax",
  "corpus_dir": "target/conformance/corpus",
  "totals": {
    "cases": 42,
    "skipped_files": 0,
    "expected_diagnostics": 50,
    "emitted_diagnostics": 52,
    "line_matched_exact": 45,
    "line_matched_plus_minus_1": 48,
    "line_match_exact_pct": 90.0,
    "line_match_plus_minus_1_pct": 96.0,
    "category_matched": 44,
    "category_evaluated": 48,
    "category_parity_pct": 91.67,
    "ux_lint_passed": 52,
    "ux_lint_violations": 0,
    "ux_lint_pass_pct": 100.0,
    "cascade_noise_ratio": 1.04
  },
  "phases": {
    "syntax": {
      "cases": 42,
      "skipped_files": 0,
      "expected_diagnostics": 50,
      "emitted_diagnostics": 52,
      "line_matched_exact": 45,
      "line_matched_plus_minus_1": 48,
      "line_match_exact_pct": 90.0,
      "line_match_plus_minus_1_pct": 96.0,
      "category_matched": 44,
      "category_evaluated": 48,
      "category_parity_pct": 91.67,
      "ux_lint_passed": 52,
      "ux_lint_violations": 0,
      "ux_lint_pass_pct": 100.0,
      "cascade_noise_ratio": 1.04
    }
  },
  "per_javac_key": {},
  "top_unmapped_keys": [],
  "top_mismatched_keys": [],
  "worst_cascade_files": [],
  "ux_lint_violation_counts": {}
}"#,
    );

    let code = cli::run(args(&[
        "report",
        "--conformance",
        conf.to_str().unwrap(),
        "--workspace-corpus",
        dir.join("missing-wc").to_str().unwrap(),
        "--diag-conformance",
        diag_json.to_str().unwrap(),
        "--date",
        "2026-11-25",
        "--out",
        out_with.to_str().unwrap(),
        "--strict",
    ]));
    assert_eq!(code, 0);

    let md = fs::read_to_string(&out_with).unwrap();
    assert!(md.contains("## 4. Diagnostic Conformance"), "{md}");
    assert!(
        md.contains("| Phase | Cases | Line Match Exact / ±1 | Category Parity | UX Lint Pass | Cascade Noise Ratio |"),
        "{md}"
    );
    assert!(
        md.contains("| `syntax` | 42 | 90.0% / 96.0% | 91.7% | 100.0% | 1.04 |"),
        "{md}"
    );

    // Absent --diag-conformance file omits Section 4 gracefully and exits 0.
    let code_absent = cli::run(args(&[
        "report",
        "--conformance",
        conf.to_str().unwrap(),
        "--workspace-corpus",
        dir.join("missing-wc").to_str().unwrap(),
        "--diag-conformance",
        missing_diag_json.to_str().unwrap(),
        "--date",
        "2026-11-25",
        "--out",
        out_absent.to_str().unwrap(),
    ]));
    assert_eq!(code_absent, 0);
    let md_absent = fs::read_to_string(&out_absent).unwrap();
    assert!(!md_absent.contains("## 4. Diagnostic Conformance"), "{md_absent}");
}
