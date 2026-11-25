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
