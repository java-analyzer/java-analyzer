//! CLI argument parser declarations for `java-analyzer`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use ide::{normalize_rel_path, Release};

use crate::workspace_config::{parse_build_tool_kind, parse_version_string, BuildToolKind};

#[derive(Debug, Parser)]
#[command(
    name = "java-analyzer",
    version,
    about = "A lightweight Java LSP server and workspace configurator"
)]
pub struct Cli {
    /// Accepted for LSP clients that pass `--stdio` (default transport).
    #[arg(long, hide = true)]
    pub stdio: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Start the Language Server Protocol server over stdio (default)
    Lsp,
    /// Inspect inferred workspace structure, parse health, and symbol index
    Inspect(InspectArgs),
    /// Inspect, disambiguate, and configure a Java workspace
    Configure(ConfigureArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct InspectArgs {
    /// Emit machine-readable JSON summary
    #[arg(long)]
    pub json: bool,

    /// List files with syntax/parse errors
    #[arg(long)]
    pub show_errors: bool,

    /// Workspace directory to inspect (defaults to current directory)
    pub workspace_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct ConfigureArgs {
    /// Pin the JDK installation directory
    #[arg(long, value_name = "PATH")]
    pub jdk: Option<PathBuf>,

    /// Pin the Java release version (8..=27)
    #[arg(long, value_name = "N", value_parser = parse_release_arg)]
    pub release: Option<Release>,

    /// Enable Java preview features
    #[arg(long, conflicts_with = "no_preview")]
    pub preview: bool,

    /// Disable Java preview features
    #[arg(long, conflicts_with = "preview")]
    pub no_preview: bool,

    /// Disambiguate build tool (none, maven, gradle, ant, bazel, mill, sbt, elide, jbang, custom)
    #[arg(long, value_name = "KIND", value_parser = parse_build_tool_arg)]
    pub build: Option<BuildToolKind>,

    /// Set custom command to emit compile-time classpath
    #[arg(long, value_name = "CMD")]
    pub classpath_cmd: Option<String>,

    /// Include an additional workspace module directory
    #[arg(long = "include", value_name = "DIR")]
    pub include_modules: Vec<String>,

    /// Exclude an auto-detected workspace module directory
    #[arg(long = "exclude", value_name = "DIR")]
    pub exclude_modules: Vec<String>,

    /// Target module for `--include-*` / `--exclude-*` flags
    #[arg(long, default_value = ".", value_name = "PATH_OR_NAME")]
    pub module: String,

    /// Add a Java source root to the target module
    #[arg(long, value_name = "DIR")]
    pub include_source: Vec<String>,

    /// Remove a Java source root from the target module
    #[arg(long, value_name = "DIR")]
    pub exclude_source: Vec<String>,

    /// Add a resource root to the target module
    #[arg(long, value_name = "DIR")]
    pub include_resource: Vec<String>,

    /// Remove a resource root from the target module
    #[arg(long, value_name = "DIR")]
    pub exclude_resource: Vec<String>,

    /// Add a compile-time dependency (Maven coord, github:owner/repo#ref, path, or module)
    #[arg(long, value_name = "SPEC")]
    pub include_dep: Vec<String>,

    /// Remove a compile-time dependency from the target module
    #[arg(long, value_name = "SPEC")]
    pub exclude_dep: Vec<String>,

    /// Reconcile conflicting external version files (.java-version, .sdkmanrc, pom.xml, etc.)
    #[arg(long)]
    pub reconcile: bool,

    /// Print inferred workspace layout and minimal .java-analyzer.toml without writing
    #[arg(long = "print", visible_alias = "dry-run")]
    pub print_only: bool,

    /// Run non-interactively and write minimal .java-analyzer.toml if overrides are needed
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Workspace directory to configure (defaults to current directory)
    pub workspace_dir: Option<PathBuf>,
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

fn parse_release_arg(s: &str) -> Result<Release, String> {
    parse_version_string(s)
        .ok_or_else(|| format!("Unsupported Java release '{s}' (expected 8..=27)"))
}

fn parse_build_tool_arg(s: &str) -> Result<BuildToolKind, String> {
    parse_build_tool_kind(s).ok_or_else(|| {
        format!(
            "Unknown build tool kind '{s}' (expected: none, maven, gradle, ant, bazel, mill, sbt, elide, jbang, custom)"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }
}
