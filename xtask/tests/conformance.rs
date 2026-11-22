use std::{fs, path::Path, time::Duration};

use xtask::{
    cli::{self, Command},
    conformance::{self, Config, Kind, KnownDivergence, Outcome, Source},
    isolate::{Isolated, run_isolated},
    jtreg::{self, Expectation, JtregCase},
};

/// A scratch directory under the target directory, removed first so reruns start clean.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, relative: &str, text: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn config(sources: Vec<Source>) -> Config {
    Config {
        sources,
        level: syntax::LanguageLevel::latest(),
        timeout: Duration::from_secs(5),
        known_divergences: Vec::new(),
    }
}

fn source(id: &str, dir: &Path, kind: Kind) -> Source {
    Source {
        id: id.to_string(),
        dir: dir.to_path_buf(),
        kind,
    }
}

// ---- isolation ----

#[test]
fn isolation_returns_the_value() {
    let result = run_isolated(Duration::from_secs(5), || 6 * 7);
    assert!(matches!(result, Isolated::Done(42)), "{result:?}");
}

#[test]
fn isolation_contains_a_panic() {
    xtask::isolate::silence_panics();
    let result = run_isolated(Duration::from_secs(5), || -> u32 { panic!("boom {}", 1) });
    match result {
        Isolated::Panicked(message) => assert_eq!(message, "boom 1"),
        other => panic!("expected a contained panic, got {other:?}"),
    }
}

#[test]
fn isolation_gives_up_on_a_hang() {
    let result = run_isolated(Duration::from_millis(100), || {
        std::thread::sleep(Duration::from_secs(3));
    });
    assert!(matches!(result, Isolated::TimedOut), "{result:?}");
}

// ---- running a source ----

#[test]
fn valid_and_invalid_sources_are_judged_by_their_kind() {
    let valid = scratch("judged-valid");
    write(&valid, "A.java", "class A { int x; }");
    write(
        &valid,
        "sub/B.java",
        "class B { void f() { int y = 1 + 2; } }",
    );
    write(&valid, "Broken.java", "class Broken {");
    write(&valid, "notes.txt", "not java, not parsed");

    let invalid = scratch("judged-invalid");
    write(&invalid, "Bad.java", "class Bad {");
    write(&invalid, "Fine.java", "class Fine {}");

    let report = conformance::run(&config(vec![
        source("valid", &valid, Kind::Valid),
        source("invalid", &invalid, Kind::Invalid),
    ]));

    let v = &report.sources[0];
    assert_eq!(
        v.results.len(),
        3,
        "only .java files are parsed: {:?}",
        v.results
    );
    assert_eq!(v.conforming(), 2);
    let gaps: Vec<_> = v.gaps().map(|r| r.path.as_str()).collect();
    assert_eq!(gaps, ["Broken.java"]);

    let i = &report.sources[1];
    assert_eq!(i.conforming(), 1);
    let gaps: Vec<_> = i.gaps().map(|r| r.path.as_str()).collect();
    assert_eq!(
        gaps,
        ["Fine.java"],
        "an invalid file that parses cleanly is a compliance gap"
    );

    assert_eq!(report.critical_count(), 0);
}

#[test]
fn paths_are_relative_with_forward_slashes_and_sorted() {
    let dir = scratch("paths");
    write(&dir, "b/Two.java", "class Two {}");
    write(&dir, "a/One.java", "class One {}");
    write(&dir, "Top.java", "class Top {}");

    let report = conformance::run(&config(vec![source("s", &dir, Kind::Valid)]));
    let paths: Vec<_> = report.sources[0]
        .results
        .iter()
        .map(|r| r.path.as_str())
        .collect();
    assert_eq!(paths, ["Top.java", "a/One.java", "b/Two.java"]);
}

#[test]
fn error_details_are_kept_for_triage() {
    let text = "class A {\n    int x\n}\n".to_string();
    match conformance::check_file(text, Duration::from_secs(5)) {
        Outcome::Errors { count, first } => {
            assert_eq!(count, 1);
            assert_eq!(first, ["2:10 expected ';'"]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn files_that_are_not_utf8_are_skipped_not_fatal() {
    let dir = scratch("not-utf8");
    write(&dir, "Good.java", "class Good {}");
    fs::write(dir.join("Latin1.java"), [b'c', b'l', 0xE9, 0xFF]).unwrap();

    let report = conformance::run(&config(vec![source("s", &dir, Kind::Valid)]));
    let s = &report.sources[0];
    assert_eq!(s.results.len(), 1);
    assert_eq!(s.skipped_files.len(), 1);
    assert_eq!(s.skipped_files[0].0, "Latin1.java");
    assert!(s.skipped_files[0].1.contains("UTF-8"));
}

#[test]
fn a_missing_or_empty_source_is_skipped_with_a_reason() {
    let empty = scratch("empty-source");
    let missing = empty.join("does-not-exist");

    let report = conformance::run(&config(vec![
        source("missing", &missing, Kind::Valid),
        source("empty", &empty, Kind::Valid),
    ]));
    assert!(
        report.sources[0]
            .skipped
            .as_deref()
            .unwrap()
            .contains("not a directory")
    );
    assert!(
        report.sources[1]
            .skipped
            .as_deref()
            .unwrap()
            .contains("no .java files")
    );
}

#[cfg(unix)]
#[test]
fn symlink_cycles_do_not_hang_the_walk() {
    let dir = scratch("symlink-cycle");
    write(&dir, "A.java", "class A {}");
    std::os::unix::fs::symlink(&dir, dir.join("loop")).unwrap();

    let report = conformance::run(&config(vec![source("s", &dir, Kind::Valid)]));
    assert_eq!(report.sources[0].results.len(), 1);
}

// ---- report output ----

#[test]
fn report_files_are_written_with_the_documented_schema() {
    let valid = scratch("report-valid");
    write(&valid, "Good.java", "class Good {}");
    write(&valid, "Bad.java", "class Bad {");
    let out = scratch("report-out");

    let report = conformance::run(&config(vec![source("valid", &valid, Kind::Valid)]));
    conformance::write_report(&report, &out).unwrap();

    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(json["schema"], 1);
    let source = &json["sources"][0];
    assert_eq!(source["id"], "valid");
    assert_eq!(source["kind"], "valid");
    assert_eq!(source["files"], 2);
    assert_eq!(source["conforming"], 1);
    assert_eq!(source["gaps"][0]["path"], "Bad.java");
    assert_eq!(source["gaps"][0]["outcome"], "errors");
    assert_eq!(source["critical"], serde_json::json!([]));

    let markdown = fs::read_to_string(out.join("report.md")).unwrap();
    assert!(markdown.contains("Critical findings: none"));
    assert!(
        markdown.contains("| valid | valid | 2 | 1 | 50.0% | 0 |"),
        "{markdown}"
    );
    assert!(markdown.contains("`Bad.java`"));
    assert!(
        markdown.contains("expected '}'"),
        "the first-error histogram is shown: {markdown}"
    );

    let failures = fs::read_to_string(out.join("failures/valid/completeness.txt")).unwrap();
    assert_eq!(failures, "Bad.java\n");
}

#[test]
fn clean_run_writes_no_failure_lists() {
    let valid = scratch("clean-valid");
    write(&valid, "Good.java", "class Good {}");
    let out = scratch("clean-out");

    let report = conformance::run(&config(vec![source("valid", &valid, Kind::Valid)]));
    conformance::write_report(&report, &out).unwrap();
    assert!(out.join("report.json").exists());
    assert!(!out.join("failures").exists());
}

// ---- command line ----

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn command_line_parsing() {
    match cli::parse_args(args(&[
        "conformance",
        "--valid",
        "a",
        "--invalid",
        "b",
        "--timeout",
        "9",
    ]))
    .unwrap()
    {
        Command::Conformance(parsed) => {
            assert_eq!(parsed.config.timeout_secs, 9);
            assert_eq!(parsed.config.level, syntax::LanguageLevel::latest());
            assert_eq!(parsed.out, Path::new("target/conformance"));
            assert_eq!(
                parsed.config.sources,
                [(Kind::Valid, "a".into()), (Kind::Invalid, "b".into())]
            );
        }
        other => panic!("{other:?}"),
    }

    match cli::parse_args(args(&[
        "conformance",
        "--valid",
        "a",
        "--release",
        "14",
        "--preview",
    ]))
    .unwrap()
    {
        Command::Conformance(parsed) => {
            assert_eq!(
                parsed.config.level,
                syntax::LanguageLevel::new(syntax::Release::JAVA_14, true)
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(cli::parse_args(args(&[])).unwrap(), Command::Help);
    assert_eq!(
        cli::parse_args(args(&["conformance", "--help"])).unwrap(),
        Command::Help
    );

    for bad in [
        &["frobnicate"][..],
        &["conformance", "--bogus"],
        &["conformance", "--valid"],
        &["conformance", "--timeout", "0"],
        &["conformance", "--timeout", "soon"],
    ] {
        assert!(cli::parse_args(args(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn source_ids_are_unique() {
    let parsed = match cli::parse_args(args(&[
        "conformance",
        "--valid",
        "x/src",
        "--valid",
        "y/src",
        "--invalid",
        "z/other",
    ]))
    .unwrap()
    {
        Command::Conformance(parsed) => parsed,
        other => panic!("{other:?}"),
    };
    let ids: Vec<_> = cli::to_config(&parsed.config)
        .sources
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, ["src", "src-2", "other"]);
}

#[test]
fn a_run_with_nothing_to_do_succeeds_and_a_usage_error_does_not() {
    let out = scratch("cli-nothing");
    let code = cli::run(args(&["conformance", "--out", out.to_str().unwrap()]));
    assert_eq!(code, 0, "no sources is not a failure");
    assert!(out.join("report.json").exists());

    assert_eq!(cli::run(args(&["conformance", "--bogus"])), 2);
}

#[test]
fn conformance_results_never_change_the_exit_code() {
    let valid = scratch("exit-valid");
    write(&valid, "Broken.java", "class Broken {");
    let out = scratch("exit-out");
    let code = cli::run(args(&[
        "conformance",
        "--valid",
        valid.to_str().unwrap(),
        "--invalid",
        scratch("exit-missing-parent")
            .join("nope")
            .to_str()
            .unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]));
    assert_eq!(code, 0, "gaps and skipped sources are report-only");
}

#[test]
fn an_unwritable_output_directory_is_a_tool_failure() {
    let dir = scratch("unwritable");
    write(&dir, "file", "a file where a directory is needed");
    let code = cli::run(args(&[
        "conformance",
        "--out",
        dir.join("file/sub").to_str().unwrap(),
    ]));
    assert_eq!(code, 1);
}

// ---- jtreg header parser and allowlist classification ----

#[test]
fn jtreg_header_parsing_and_allowlist_classification() {
    let allowlist = jtreg::parse_allowlist(include_str!("../conformance/parser_errors.txt"));
    assert!(allowlist.contains("compiler.err.expected"));

    // Plain `@compile`: valid at latest release.
    let valid_src = "/*\n * @test\n * @compile Foo.java\n */\nclass Foo {}\n";
    assert_eq!(
        jtreg::classify(valid_src, |_| None, &allowlist),
        JtregCase {
            expectation: Expectation::Valid,
            level: syntax::LanguageLevel::latest(),
        }
    );

    // `@compile --release 14 --enable-preview`: valid at Java 14 preview.
    let preview_src =
        "/*\n * @test\n * @compile --release 14 --enable-preview R.java\n */\nrecord R() {}\n";
    assert_eq!(
        jtreg::classify(preview_src, |_| None, &allowlist),
        JtregCase {
            expectation: Expectation::Valid,
            level: syntax::LanguageLevel::new(syntax::Release::JAVA_14, true),
        }
    );

    // `@compile -source 1.8`: valid at Java 8.
    let source8 = "/* @test\n * @compile -source 1.8 Old.java\n */\nclass Old { int _ = 1; }\n";
    assert_eq!(
        jtreg::classify(source8, |_| None, &allowlist),
        JtregCase {
            expectation: Expectation::Valid,
            level: syntax::LanguageLevel::new(syntax::Release::JAVA_8, false),
        }
    );

    // `@compile/fail/ref=Bad.out` with only parser-phase error codes: invalid.
    let fail_syntax =
        "/*\n * @test\n * @compile/fail/ref=Bad.out -XDrawDiagnostics Bad.java\n */\nclass Bad {\n";
    let bad_out = "Bad.java:5:12: compiler.err.premature.eof\n1 error\n";
    assert_eq!(
        jtreg::classify(
            fail_syntax,
            |name| (name == "Bad.out").then(|| bad_out.to_string()),
            &allowlist
        ),
        JtregCase {
            expectation: Expectation::Invalid,
            level: syntax::LanguageLevel::latest(),
        }
    );

    // `@compile/fail/ref=Sem.out` containing a semantic error code: skipped.
    let fail_sem = "/*\n * @test\n * @compile/fail/ref=Sem.out -XDrawDiagnostics Sem.java\n */\nclass Sem {}\n";
    let sem_out = "Sem.java:2:5: compiler.err.expected: ';'\nSem.java:3:5: compiler.err.cant.resolve.location: ...\n2 errors\n";
    assert!(matches!(
        jtreg::classify(
            fail_sem,
            |name| (name == "Sem.out").then(|| sem_out.to_string()),
            &allowlist
        )
        .expectation,
        Expectation::Skipped(_)
    ));

    // `@compile/fail` without a `ref=*.out` file: skipped.
    let no_ref = "/* @test\n * @compile/fail NoRef.java\n */\nclass NoRef {}\n";
    assert!(matches!(
        jtreg::classify(no_ref, |_| None, &allowlist).expectation,
        Expectation::Skipped(_)
    ));

    // Helper file with no `@compile` directive: skipped.
    let helper = "/* helper class */\nclass Helper {}\n";
    assert!(matches!(
        jtreg::classify(helper, |_| None, &allowlist).expectation,
        Expectation::Skipped(_)
    ));
}

// ---- known divergences ----

#[test]
fn known_divergences_are_excluded_from_unexpected_gaps() {
    let toml_text = r#"
[[divergence]]
path = "sub/KnownLongChain.java"
reason = "Exceeds MAX_CHAIN limit"
"#;
    let divergences = conformance::parse_known_divergences(toml_text).unwrap();
    assert_eq!(
        divergences,
        vec![KnownDivergence {
            path: "sub/KnownLongChain.java".to_string(),
            reason: "Exceeds MAX_CHAIN limit".to_string(),
        }]
    );

    let valid = scratch("known-div-valid");
    write(&valid, "Good.java", "class Good {}");
    write(&valid, "sub/KnownLongChain.java", "class KnownLongChain {");
    write(&valid, "Unexpected.java", "class Unexpected {");
    let out = scratch("known-div-out");

    let mut cfg = config(vec![source("jdk", &valid, Kind::Valid)]);
    cfg.known_divergences = divergences;
    let report = conformance::run(&cfg);
    conformance::write_report(&report, &out).unwrap();

    let s = &report.sources[0];
    let unexpected: Vec<_> = s.gaps().map(|r| r.path.as_str()).collect();
    assert_eq!(
        unexpected,
        ["Unexpected.java"],
        "known divergence must be excluded from unexpected gaps"
    );
    let known: Vec<_> = s
        .known_divergences()
        .map(|(r, _)| r.path.as_str())
        .collect();
    assert_eq!(known, ["sub/KnownLongChain.java"]);

    let failures = fs::read_to_string(out.join("failures/jdk/completeness.txt")).unwrap();
    assert_eq!(failures, "Unexpected.java\n");
}

#[test]
fn plaintext_summary_table_is_aligned_with_totals_and_notes() {
    let clean = scratch("table-clean");
    write(&clean, "Clean.java", "class Clean {}");
    let valid = scratch("table-valid");
    write(&valid, "A.java", "class A {}");
    write(&valid, "Known.java", "class Known {");
    let empty = scratch("table-empty");

    let mut cfg = config(vec![
        source("commons-lang", &clean, Kind::Valid),
        source("jdk-27-ga", &valid, Kind::Valid),
        source("empty-src", &empty, Kind::Valid),
    ]);
    cfg.known_divergences = vec![KnownDivergence {
        path: "Known.java".to_string(),
        reason: "known limit".to_string(),
    }];

    let report = conformance::run(&cfg);
    let table = conformance::format_summary_table(&report);

    let skip_note = format!("skipped (no .java files under {})", empty.display());
    let note_sep = "-".repeat(skip_note.len());
    let expected = format!(
        "\
Source        Files  Conforming     Rate  Critical  Notes
------------  -----  ----------  -------  --------  {note_sep}
commons-lang      1           1  100.0%          0  clean
jdk-27-ga         2           1  100.0%*         0  1 known divergence(s)
empty-src         -           -       -          -  {skip_note}
------------  -----  ----------  -------  --------  {note_sep}
Total             3           2  100.0%*         0  1 known divergence(s)
"
    );
    assert_eq!(table, expected);
}
