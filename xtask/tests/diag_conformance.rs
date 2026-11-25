use std::{collections::BTreeSet, fs, path::Path};

use xtask::{
    cli::{self, Command},
    diag_conformance::{self, ExpectedDiagnostic, MappingPhase, Phase, ReportStatus},
    jtreg,
};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("diag-conformance-tests")
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

#[test]
fn embedded_diag_mapping_parses_and_covers_all_parser_errors() {
    let entries =
        diag_conformance::parse_diag_mapping(diag_conformance::EMBEDDED_DIAG_MAPPING).unwrap();
    let mapped_keys: BTreeSet<&str> = entries
        .iter()
        .filter(|e| e.phase == MappingPhase::Syntax)
        .map(|e| e.javac_key.as_str())
        .collect();

    let parser_errors = jtreg::parse_allowlist(include_str!("../conformance/parser_errors.txt"));
    assert!(
        !parser_errors.is_empty(),
        "parser_errors.txt should not be empty"
    );
    for key in &parser_errors {
        assert!(
            mapped_keys.contains(key.as_str()),
            "missing syntax mapping in diag_mapping.toml for `{key}`"
        );
    }

    // Duplicate keys must be rejected cleanly.
    let dup_toml = r#"
[[mapping]]
javac_key = "compiler.err.expected"
phase = "syntax"
ja_code = "syntax/expected-token"
message_pattern = "expected"

[[mapping]]
javac_key = "compiler.err.expected"
phase = "syntax"
ja_code = "syntax/expected-token"
message_pattern = "expected"
"#;
    assert!(diag_conformance::parse_diag_mapping(dup_toml).is_err());
}

#[test]
fn parses_javac_xdraw_diagnostics_out_lines() {
    let out_text = "\
Foo.java:12:5: compiler.err.expected: ';'
sub/Bar.java:3:10: compiler.err.illegal.start.of.expr
Legacy.java:7: compiler.warn.raw.type: java.util.List
- compiler.note.unchecked.filename: Legacy.java
1 error
";

    let parsed = diag_conformance::parse_javac_out(out_text);
    assert_eq!(
        parsed,
        vec![
            ExpectedDiagnostic {
                file: "Foo.java".to_string(),
                line: 12,
                column: Some(5),
                javac_key: "compiler.err.expected".to_string(),
            },
            ExpectedDiagnostic {
                file: "sub/Bar.java".to_string(),
                line: 3,
                column: Some(10),
                javac_key: "compiler.err.illegal.start.of.expr".to_string(),
            },
            ExpectedDiagnostic {
                file: "Legacy.java".to_string(),
                line: 7,
                column: None,
                javac_key: "compiler.warn.raw.type".to_string(),
            },
        ]
    );
}

#[test]
fn cli_parses_diag_conformance_flags() {
    match cli::parse_args(args(&["diag-conformance"])).unwrap() {
        Command::DiagConformance(parsed) => {
            assert_eq!(parsed.phase, Phase::Syntax);
            assert_eq!(parsed.corpus_dir, Path::new("target/conformance/corpus"));
            assert_eq!(parsed.mapping, None);
            assert_eq!(
                parsed.out,
                Path::new("target/conformance/diagnostics.json")
            );
        }
        other => panic!("expected DiagConformance, got {other:?}"),
    }

    match cli::parse_args(args(&[
        "diag-conformance",
        "--phase",
        "all",
        "--corpus-dir",
        "custom/corpus",
        "--mapping",
        "custom/mapping.toml",
        "--out",
        "custom/diagnostics.json",
    ]))
    .unwrap()
    {
        Command::DiagConformance(parsed) => {
            assert_eq!(parsed.phase, Phase::All);
            assert_eq!(parsed.corpus_dir, Path::new("custom/corpus"));
            assert_eq!(
                parsed.mapping.as_deref(),
                Some(Path::new("custom/mapping.toml"))
            );
            assert_eq!(parsed.out, Path::new("custom/diagnostics.json"));
        }
        other => panic!("expected DiagConformance, got {other:?}"),
    }

    assert_eq!(
        cli::parse_args(args(&["diag-conformance", "--help"])).unwrap(),
        Command::DiagConformanceHelp
    );
    assert_eq!(cli::run(args(&["diag-conformance", "--bogus"])), 2);
}

#[test]
fn synthetic_jtreg_corpus_computes_all_four_metrics_and_schema_v1_json() {
    let dir = scratch("synthetic-corpus");
    let corpus = dir.join("corpus");
    let out_json = dir.join("diagnostics.json");

    // Case 1: Missing semicolon on line 3 -> syntax/expected-token on line 3.
    write(
        &corpus,
        "MissingSemi.java",
        "\
/*
 * @test
 * @compile/fail/ref=MissingSemi.out -XDrawDiagnostics MissingSemi.java
 */
class MissingSemi {
    void f() {
        int x = 1
    }
}
",
    );
    write(
        &corpus,
        "MissingSemi.out",
        "MissingSemi.java:7:18: compiler.err.expected: ';'\n1 error\n",
    );

    // Case 2: Preview feature on Java 14 without --enable-preview -> syntax/preview-required.
    write(
        &corpus,
        "RecordPreview.java",
        "\
/*
 * @test
 * @compile/fail/ref=RecordPreview.out -XDrawDiagnostics --release 14 RecordPreview.java
 */
record RecordPreview(int x) {}
",
    );
    write(
        &corpus,
        "RecordPreview.out",
        "RecordPreview.java:5:1: compiler.err.preview.feature.disabled.plural: (compiler.misc.feature.records)\n1 error\n",
    );

    // Case 3: Semantic error only -> must be filtered out under `--phase syntax`.
    write(
        &corpus,
        "SemanticOnly.java",
        "\
/*
 * @test
 * @compile/fail/ref=SemanticOnly.out -XDrawDiagnostics SemanticOnly.java
 */
class SemanticOnly {
    int x = \"nope\";
}
",
    );
    write(
        &corpus,
        "SemanticOnly.out",
        "SemanticOnly.java:6:13: compiler.err.prob.found.req: String, int\n1 error\n",
    );

    let code = cli::run(args(&[
        "diag-conformance",
        "--phase",
        "syntax",
        "--corpus-dir",
        corpus.to_str().unwrap(),
        "--out",
        out_json.to_str().unwrap(),
    ]));
    assert_eq!(code, 0);

    let json_text = fs::read_to_string(&out_json).unwrap();
    let parsed = diag_conformance::parse_report_json(&json_text).unwrap();
    assert_eq!(parsed.schema_version, 1);
    assert_eq!(parsed.status, ReportStatus::Ok);
    assert_eq!(parsed.phase, "syntax");

    let syntax = parsed.phases.get("syntax").expect("syntax phase present");
    assert_eq!(syntax.cases, 2);
    assert_eq!(syntax.expected_diagnostics, 2);
    assert_eq!(syntax.line_matched_exact, 2);
    assert_eq!(syntax.line_matched_plus_minus_1, 2);
    assert!((syntax.line_match_exact_pct - 100.0).abs() < 0.01);
    assert!((syntax.line_match_plus_minus_1_pct - 100.0).abs() < 0.01);
    assert_eq!(syntax.category_matched, 2);
    assert!((syntax.category_parity_pct - 100.0).abs() < 0.01);
    assert!((syntax.ux_lint_pass_pct - 100.0).abs() < 0.01);
    assert_eq!(syntax.ux_lint_violations, 0);
    assert!((syntax.cascade_noise_ratio - 1.0).abs() < 0.01);

    assert_eq!(parsed.totals, *syntax);
    assert!(parsed.per_javac_key.contains_key("compiler.err.expected"));
    assert!(
        parsed
            .per_javac_key
            .contains_key("compiler.err.preview.feature.disabled.plural")
    );
    assert!(parsed.top_unmapped_keys.is_empty());
}

#[test]
fn missing_corpus_directory_skips_gracefully_and_exits_zero() {
    let dir = scratch("missing-corpus");
    let missing = dir.join("does-not-exist");
    let out_json = dir.join("diagnostics.json");

    let code = cli::run(args(&[
        "diag-conformance",
        "--phase",
        "syntax",
        "--corpus-dir",
        missing.to_str().unwrap(),
        "--out",
        out_json.to_str().unwrap(),
    ]));
    assert_eq!(code, 0);

    let json_text = fs::read_to_string(&out_json).unwrap();
    let parsed = diag_conformance::parse_report_json(&json_text).unwrap();
    assert_eq!(parsed.schema_version, 1);
    assert_eq!(parsed.status, ReportStatus::Skipped);
    assert!(parsed.skip_reason.is_some());
    assert_eq!(parsed.totals.cases, 0);
}

#[test]
fn phase_all_tracks_unmapped_mismatched_cascade_and_tool_errors() {
    let dir = scratch("phase-all-and-errors");
    let corpus = dir.join("corpus");
    let out_json = dir.join("diagnostics.json");

    // Syntax error where javac expects `;` on line 6, plus extra cascade/unclosed brace.
    write(
        &corpus,
        "Cascade.java",
        "\
/*
 * @test
 * @compile/fail/ref=Cascade.out -XDrawDiagnostics Cascade.java
 */
class Cascade {
    void f() {
        int a =
        int b = 1;
    }
}
",
    );
    write(
        &corpus,
        "Cascade.out",
        "Cascade.java:7:16: compiler.err.illegal.start.of.expr\n1 error\n",
    );

    // Semantic error (unmapped key in embedded diag_mapping.toml).
    write(
        &corpus,
        "Sem.java",
        "\
/*
 * @test
 * @compile/fail/ref=Sem.out -XDrawDiagnostics Sem.java
 */
class Sem {
    int x = \"bad\";
}
",
    );
    write(
        &corpus,
        "Sem.out",
        "Sem.java:6:13: compiler.err.prob.found.req: String, int\n1 error\n",
    );

    let code = cli::run(args(&[
        "diag-conformance",
        "--phase",
        "all",
        "--corpus-dir",
        corpus.to_str().unwrap(),
        "--out",
        out_json.to_str().unwrap(),
    ]));
    assert_eq!(code, 0);

    let parsed = diag_conformance::parse_report_json(&fs::read_to_string(&out_json).unwrap()).unwrap();
    assert_eq!(parsed.status, ReportStatus::Ok);
    assert_eq!(parsed.phases.len(), 2);
    assert!(parsed.phases.contains_key("syntax"));
    assert!(parsed.phases.contains_key("semantic"));
    assert_eq!(parsed.totals.cases, 2);
    assert!(!parsed.worst_cascade_files.is_empty());
    assert_eq!(parsed.worst_cascade_files[0].path, "Cascade.java");
    assert!(
        parsed
            .top_unmapped_keys
            .iter()
            .any(|k| k.javac_key == "compiler.err.prob.found.req")
    );

    // Malformed mapping TOML returns exit code 1.
    let bad_mapping = dir.join("bad_mapping.toml");
    fs::write(&bad_mapping, "not valid toml = [").unwrap();
    let bad_map_code = cli::run(args(&[
        "diag-conformance",
        "--mapping",
        bad_mapping.to_str().unwrap(),
        "--corpus-dir",
        corpus.to_str().unwrap(),
        "--out",
        out_json.to_str().unwrap(),
    ]));
    assert_eq!(bad_map_code, 1);

    // Unwritable output path returns exit code 1.
    let blocker = dir.join("blocker_file");
    fs::write(&blocker, "file").unwrap();
    let bad_out_code = cli::run(args(&[
        "diag-conformance",
        "--corpus-dir",
        dir.join("missing").to_str().unwrap(),
        "--out",
        blocker.join("diagnostics.json").to_str().unwrap(),
    ]));
    assert_eq!(bad_out_code, 1);
}
