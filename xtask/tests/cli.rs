use xtask::cli::{self, Command, XtaskCli};

#[test]
fn usage_kdl_matches_live_cli_spec() {
    let checked_in = include_str!("../xtask.usage.kdl");
    let live = XtaskCli::to_kdl();
    assert_eq!(
        checked_in, live,
        "xtask/xtask.usage.kdl is out of sync with XtaskCli; regenerate with `cargo xtask usage-spec > xtask/xtask.usage.kdl`"
    );
}

#[test]
fn usage_spec_command_parses_and_exits_zero() {
    assert_eq!(
        cli::parse_args(vec!["usage-spec".to_string()]).unwrap(),
        Command::UsageSpec
    );
    assert_eq!(cli::run(vec!["usage-spec".to_string()]), 0);
}

#[test]
fn unknown_flags_return_exit_code_two() {
    for argv in [
        vec!["--bogus".to_string()],
        vec!["diag-conformance".to_string(), "--bogus".to_string()],
        vec!["bench".to_string(), "--bogus".to_string()],
        vec!["bench-plot".to_string(), "--bogus".to_string()],
        vec!["bench-gen-plain".to_string(), "--bogus".to_string()],
        vec!["usage-spec".to_string(), "--bogus".to_string()],
    ] {
        assert!(cli::parse_args(argv.clone()).is_err(), "{argv:?}");
        assert_eq!(cli::run(argv.clone()), 2, "{argv:?}");
    }
}

#[test]
fn xtask_cli_dispatch_and_usage_spec_via_command_macro() {
    let help = usage::test::command!("xtask", "--help").assert_success();
    let help_text = help.stdout_text();
    assert!(help_text.contains("conformance"), "{help_text}");
    assert!(help_text.contains("completion"), "{help_text}");

    let spec = usage::test::command!("xtask", "usage-spec").assert_success();
    assert_eq!(spec.stdout_text(), include_str!("../xtask.usage.kdl"));

    let bogus = usage::test::command!("xtask", "--bogus");
    assert_eq!(bogus.status.code(), Some(2));
}

#[test]
fn xtask_completion_scripts_for_bash_zsh_and_fish() {
    for (shell, marker) in [
        ("bash", "complete -F"),
        ("zsh", "#compdef xtask"),
        ("fish", "complete -c 'xtask'"),
    ] {
        let out = usage::test::command!("xtask", "completion", shell).assert_success();
        let text = out.stdout_text();
        assert!(
            text.contains("__complete_word__"),
            "{shell}: missing __complete_word__ in:\n{text}"
        );
        assert!(
            text.contains(marker),
            "{shell}: missing `{marker}` in:\n{text}"
        );
    }
}

#[test]
fn xtask_dynamic_completion_lists_subcommands() {
    let out = usage::test::command!(
        "xtask",
        "__complete_word__",
        "--shell",
        "zsh",
        "--words",
        "xtask",
        ""
    )
    .assert_success();
    let text = out.stdout_text();
    for cmd in [
        "conformance",
        "diag-conformance",
        "report",
        "bench",
        "completion",
    ] {
        assert!(text.contains(cmd), "missing `{cmd}` in:\n{text}");
    }
}

#[test]
fn xtask_completion_install_is_hermetic_via_run_split_in() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let config_home = tmp.path().join("xdg-config");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&config_home).unwrap();

    let env = usage::install::Env::new(
        usage::install::Platform::Linux,
        [
            ("HOME".to_string(), home.as_os_str().to_os_string()),
            (
                "XDG_CONFIG_HOME".to_string(),
                config_home.as_os_str().to_os_string(),
            ),
        ],
    );

    let Command::Completion(cmd) = cli::parse_args(vec![
        "completion".to_string(),
        "fish".to_string(),
        "--install".to_string(),
    ])
    .unwrap() else {
        panic!("expected Completion command");
    };

    let mut out = Vec::new();
    let mut report = Vec::new();
    cmd.completion
        .run_split_in("xtask", &env, &mut out, &mut report)
        .unwrap();

    assert!(out.is_empty(), "install should leave stdout empty");
    let report_text = String::from_utf8(report).unwrap();
    assert!(
        report_text.contains("installed"),
        "unexpected report: {report_text}"
    );
    let installed_path = config_home.join("fish/completions/xtask.fish");
    assert!(installed_path.is_file());
    assert!(
        std::fs::read_to_string(&installed_path)
            .unwrap()
            .contains("__complete_word__")
    );
}
