//! CLI argument parser declarations for `java-analyzer`.

use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
    str::FromStr,
};

use ide::{Release, normalize_rel_path};

use crate::workspace_config::{BuildToolKind, parse_version_string};

/// A validated Java release argument (`8..=27`) parsed from `--release <N>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReleaseArg(pub Release);

impl From<Release> for ReleaseArg {
    fn from(release: Release) -> Self {
        Self(release)
    }
}

impl From<ReleaseArg> for Release {
    fn from(arg: ReleaseArg) -> Self {
        arg.0
    }
}

impl FromStr for ReleaseArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_version_string(s)
            .map(Self)
            .ok_or_else(|| format!("Unsupported Java release '{s}' (expected 8..=27)"))
    }
}

/// A lightweight Java LSP server and workspace configurator.
#[derive(Debug, Clone, PartialEq, Eq, usage::Cli)]
#[usage(
    crate = usage,
    name = "java-analyzer",
    bin = "java-analyzer",
    version = env!("CARGO_PKG_VERSION"),
    unknown_flags = "error",
    validate_with = validate_cli
)]
pub struct Cli {
    /// Accepted for LSP clients that pass `--stdio` (default transport).
    #[usage(long, hide)]
    pub stdio: bool,

    #[usage(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, usage::Subcommands, PartialEq, Eq)]
#[usage(crate = usage)]
pub enum Command {
    /// Start the Language Server Protocol server over stdio (default).
    ///
    /// Indexes workspace sources, JDK modules, and JAR dependencies for semantic analysis,
    /// hover (with on-demand Javadoc), and jump-to-definition.
    Lsp,
    /// Inspect autodiscovered workspace configuration, source/resource roots, and conflicts.
    ///
    /// Read-only view of how the LSP discovers build specs, JDK/release settings, and module
    /// layouts; intentionally avoids indexing binary JDK/JAR bytecode.
    Inspect(InspectArgs),
    /// Disambiguate workspace conflicts and write a minimal `.java-analyzer.toml` configuration.
    ///
    /// Focused strictly on workspace layout, JDK/release pinning, and dependency overrides;
    /// does not index binary JDK/JAR bytecode.
    Configure(Box<ConfigureArgs>),
    /// Print the canonical KDL usage spec for java-analyzer to stdout
    #[usage(hide)]
    UsageSpec,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, usage::Args)]
#[usage(crate = usage)]
pub struct InspectArgs {
    /// Emit machine-readable JSON summary
    #[usage(long)]
    pub json: bool,

    /// List files with syntax/parse errors
    #[usage(long)]
    pub show_errors: bool,

    /// Workspace directory to inspect (defaults to current directory)
    #[usage(value_name = "WORKSPACE_DIR")]
    pub workspace_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, usage::Args)]
#[usage(crate = usage)]
pub struct ConfigureArgs {
    /// Pin the JDK installation directory
    #[usage(long, value_name = "PATH")]
    pub jdk: Option<PathBuf>,

    /// Pin the Java release version (8..=27)
    #[usage(long, value_name = "N")]
    pub release: Option<ReleaseArg>,

    /// Enable Java preview features
    #[usage(long)]
    pub preview: bool,

    /// Disable Java preview features
    #[usage(long)]
    pub no_preview: bool,

    /// Disambiguate build tool (none, maven, gradle, ant, bazel, mill, sbt, elide, jbang, custom)
    #[usage(long, value_name = "KIND")]
    pub build: Option<BuildToolKind>,

    /// Set custom command to emit compile-time classpath
    #[usage(long = "classpath-cmd", value_name = "CMD")]
    pub classpath_cmd: Option<String>,

    /// Include an additional workspace module directory
    #[usage(long = "include", var, value_name = "DIR")]
    pub include_modules: Vec<String>,

    /// Exclude an auto-detected workspace module directory
    #[usage(long = "exclude", var, value_name = "DIR")]
    pub exclude_modules: Vec<String>,

    /// Target module for `--include-*` / `--exclude-*` flags
    #[usage(long, default = ".", value_name = "PATH_OR_NAME")]
    pub module: String,

    /// Add a Java source root to the target module
    #[usage(long = "include-source", var, value_name = "DIR")]
    pub include_source: Vec<String>,

    /// Remove a Java source root from the target module
    #[usage(long = "exclude-source", var, value_name = "DIR")]
    pub exclude_source: Vec<String>,

    /// Add a resource root to the target module
    #[usage(long = "include-resource", var, value_name = "DIR")]
    pub include_resource: Vec<String>,

    /// Remove a resource root from the target module
    #[usage(long = "exclude-resource", var, value_name = "DIR")]
    pub exclude_resource: Vec<String>,

    /// Add a compile-time dependency (Maven coord, github:owner/repo#ref, path, or module)
    #[usage(long = "include-dep", var, value_name = "SPEC")]
    pub include_dep: Vec<String>,

    /// Remove a compile-time dependency from the target module
    #[usage(long = "exclude-dep", var, value_name = "SPEC")]
    pub exclude_dep: Vec<String>,

    /// Reconcile conflicting external version files (.java-version, .sdkmanrc, pom.xml, etc.)
    #[usage(long)]
    pub reconcile: bool,

    /// Print inferred workspace layout and minimal .java-analyzer.toml without writing
    #[usage(long = "print", aliases = ["dry-run"])]
    pub print_only: bool,

    /// Run non-interactively and write minimal .java-analyzer.toml if overrides are needed
    #[usage(short = 'y', long)]
    pub yes: bool,

    /// Workspace directory to configure (defaults to current directory)
    #[usage(value_name = "WORKSPACE_DIR")]
    pub workspace_dir: Option<PathBuf>,
}

fn validate_cli(cli: &Cli) -> Result<(), usage::ValidationError> {
    if let Some(Command::Configure(args)) = &cli.command
        && args.preview
        && args.no_preview
    {
        return Err(usage::ValidationError::field("preview")
            .value("--preview --no-preview")
            .reason("cannot be used with '--no-preview'"));
    }
    Ok(())
}

impl ConfigureArgs {
    pub fn preview_override(&self) -> Option<bool> {
        if self.preview {
            Some(true)
        } else if self.no_preview {
            Some(false)
        } else {
            None
        }
    }

    pub fn normalized_module_key(&self) -> String {
        if self.module.is_empty() || self.module == "." {
            ".".to_string()
        } else if self.module.contains('/') {
            normalize_rel_path(&self.module)
        } else {
            self.module.clone()
        }
    }

    pub fn has_explicit_mutation_flags(&self) -> bool {
        self.jdk.is_some()
            || self.release.is_some()
            || self.preview_override().is_some()
            || self.build.is_some()
            || self
                .classpath_cmd
                .as_deref()
                .is_some_and(|cmd| !cmd.trim().is_empty())
            || !self.include_modules.is_empty()
            || !self.exclude_modules.is_empty()
            || !self.include_source.is_empty()
            || !self.exclude_source.is_empty()
            || !self.include_resource.is_empty()
            || !self.exclude_resource.is_empty()
            || !self.include_dep.is_empty()
            || !self.exclude_dep.is_empty()
            || self.reconcile
    }
}

/// Parses CLI arguments (excluding `argv[0]`) using `usage-rs`'s embedded outcome handler.
pub fn parse_outcome(args: &[OsString]) -> usage::embedded::Outcome<Cli> {
    Cli::embedded_outcome(args)
}

/// Parses CLI arguments (excluding `argv[0]`), returning `Ok(Cli)` or a rendered error string.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
    let os_args: Vec<OsString> = args.into_iter().map(OsString::from).collect();
    let refs: Vec<&OsStr> = os_args.iter().map(OsString::as_os_str).collect();
    Cli::parse_from(&refs).map_err(|err| usage::render_failure_plain(Cli::spec(), &refs, &err))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os_args(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn string_args(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn usage_kdl_matches_live_cli_spec() {
        let checked_in = include_str!("../java-analyzer.usage.kdl");
        let live = Cli::to_kdl();
        assert_eq!(
            checked_in, live,
            "crates/java-analyzer/java-analyzer.usage.kdl is out of sync with Cli; regenerate with `cargo run -p java-analyzer -- usage-spec > crates/java-analyzer/java-analyzer.usage.kdl`"
        );
    }

    #[test]
    fn default_stdio_and_lsp_subcommand_select_lsp_mode() {
        assert_eq!(
            parse_args(string_args(&[])).unwrap(),
            Cli {
                stdio: false,
                command: None,
            }
        );
        assert_eq!(
            parse_args(string_args(&["--stdio"])).unwrap(),
            Cli {
                stdio: true,
                command: None,
            }
        );
        assert_eq!(
            parse_args(string_args(&["lsp"])).unwrap(),
            Cli {
                stdio: false,
                command: Some(Command::Lsp),
            }
        );
    }

    #[test]
    fn help_version_and_usage_spec_outcomes() {
        for flag in ["--help", "-h"] {
            match parse_outcome(&os_args(&[flag])) {
                usage::embedded::Outcome::Exit(exit) => {
                    assert_eq!(exit.code, 0, "{flag}");
                    assert!(!exit.stderr, "{flag}");
                    assert!(exit.text.contains("java-analyzer"), "{flag}: {}", exit.text);
                }
                other => panic!("expected Exit for {flag}, got {other:?}"),
            }
        }

        for flag in ["--version", "-V"] {
            match parse_outcome(&os_args(&[flag])) {
                usage::embedded::Outcome::Exit(exit) => {
                    assert_eq!(exit.code, 0, "{flag}");
                    assert!(!exit.stderr, "{flag}");
                    assert!(
                        exit.text.contains(env!("CARGO_PKG_VERSION")),
                        "{flag}: {}",
                        exit.text
                    );
                }
                other => panic!("expected Exit for {flag}, got {other:?}"),
            }
        }

        assert_eq!(
            parse_args(string_args(&["usage-spec"])).unwrap().command,
            Some(Command::UsageSpec)
        );
    }

    #[test]
    fn inspect_flags_and_positional_parse() {
        assert_eq!(
            parse_args(string_args(&["inspect"])).unwrap().command,
            Some(Command::Inspect(InspectArgs::default()))
        );

        assert_eq!(
            parse_args(string_args(&[
                "inspect",
                "--json",
                "--show-errors",
                "/tmp/workspace"
            ]))
            .unwrap()
            .command,
            Some(Command::Inspect(InspectArgs {
                json: true,
                show_errors: true,
                workspace_dir: Some(PathBuf::from("/tmp/workspace")),
            }))
        );
    }

    #[test]
    fn configure_flags_aliases_and_repeatable_options_parse() {
        let cli = parse_args(string_args(&[
            "configure",
            "--jdk",
            "/opt/jdk-21",
            "--release",
            "21",
            "--preview",
            "--build",
            "maven",
            "--classpath-cmd",
            "mvn -q dependency:build-classpath",
            "--include",
            "mod-a",
            "--include",
            "mod-b",
            "--exclude",
            "legacy",
            "--module",
            "services/core",
            "--include-source",
            "src/gen/java",
            "--include-source",
            "src/extra/java",
            "--exclude-source",
            "src/old/java",
            "--include-resource",
            "res/main",
            "--exclude-resource",
            "res/old",
            "--include-dep",
            "maven:com.google.guava:guava:33.2.1-jre",
            "--exclude-dep",
            "maven:junit:junit",
            "--reconcile",
            "--dry-run",
            "-y",
            "/tmp/repo",
        ]))
        .unwrap();

        assert_eq!(
            cli.command,
            Some(Command::Configure(Box::new(ConfigureArgs {
                jdk: Some(PathBuf::from("/opt/jdk-21")),
                release: Some(Release::JAVA_21.into()),
                preview: true,
                no_preview: false,
                build: Some(BuildToolKind::Maven),
                classpath_cmd: Some("mvn -q dependency:build-classpath".to_string()),
                include_modules: vec!["mod-a".to_string(), "mod-b".to_string()],
                exclude_modules: vec!["legacy".to_string()],
                module: "services/core".to_string(),
                include_source: vec!["src/gen/java".to_string(), "src/extra/java".to_string()],
                exclude_source: vec!["src/old/java".to_string()],
                include_resource: vec!["res/main".to_string()],
                exclude_resource: vec!["res/old".to_string()],
                include_dep: vec!["maven:com.google.guava:guava:33.2.1-jre".to_string()],
                exclude_dep: vec!["maven:junit:junit".to_string()],
                reconcile: true,
                print_only: true,
                yes: true,
                workspace_dir: Some(PathBuf::from("/tmp/repo")),
            })))
        );
    }

    #[test]
    fn configure_rejects_conflicting_preview_and_invalid_values() {
        for argv in [
            vec!["configure", "--preview", "--no-preview"],
            vec!["configure", "--no-preview", "--preview"],
            vec!["configure", "--release", "7"],
            vec!["configure", "--release", "99"],
            vec!["configure", "--build", "not-a-tool"],
            vec!["--bogus"],
            vec!["lsp", "--bogus"],
            vec!["inspect", "--bogus"],
            vec!["configure", "--bogus"],
        ] {
            assert!(parse_args(string_args(&argv)).is_err(), "{argv:?}");
            match parse_outcome(&os_args(&argv)) {
                usage::embedded::Outcome::Exit(exit) => {
                    assert_eq!(exit.code, 2, "{argv:?}: {}", exit.text);
                    assert!(exit.stderr, "{argv:?}");
                }
                other => panic!("expected Exit(2) for {argv:?}, got {other:?}"),
            }
        }
    }
}
