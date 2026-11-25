#[test]
fn cli_help_version_and_usage_spec() {
    let help = usage::test::command!("java-analyzer", "--help").assert_success();
    let help_text = help.stdout_text();
    assert!(help_text.contains("java-analyzer"), "{help_text}");
    assert!(help_text.contains("completion"), "{help_text}");
    assert!(help_text.contains("inspect"), "{help_text}");
    assert!(help_text.contains("configure"), "{help_text}");

    let version = usage::test::command!("java-analyzer", "--version").assert_success();
    assert!(
        version.stdout_text().contains(env!("CARGO_PKG_VERSION")),
        "{}",
        version.stdout_text()
    );

    let spec = usage::test::command!("java-analyzer", "usage-spec").assert_success();
    assert_eq!(
        spec.stdout_text(),
        include_str!("../java-analyzer.usage.kdl")
    );
}

#[test]
fn cli_completion_scripts_for_bash_zsh_and_fish() {
    for (shell, marker) in [
        ("bash", "complete -F"),
        ("zsh", "#compdef java-analyzer"),
        ("fish", "complete -c 'java-analyzer'"),
    ] {
        let out = usage::test::command!("java-analyzer", "completion", shell).assert_success();
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
fn cli_dynamic_completion_callbacks() {
    let out = usage::test::command!(
        "java-analyzer",
        "__complete_word__",
        "--shell",
        "zsh",
        "--words",
        "java-analyzer",
        ""
    )
    .assert_success();
    let text = out.stdout_text();
    for cmd in ["lsp", "inspect", "configure", "completion"] {
        assert!(text.contains(cmd), "missing `{cmd}` in:\n{text}");
    }
    assert!(
        !text.contains("usage-spec"),
        "hidden subcommand `usage-spec` leaked into:\n{text}"
    );
}

#[test]
fn cli_unknown_flag_exits_with_code_two() {
    let out = usage::test::command!("java-analyzer", "--bogus");
    assert_eq!(out.status.code(), Some(2));
}
