//! CLI argument parser and the `java-analyzer configure` interactive TUI / non-interactive
//! workspace configurator.
//!
//! `java-analyzer configure` is the **only** CLI command that writes `.java-analyzer.toml` or
//! reconciles external specification files.

use std::{
    env, fs,
    io::{self, BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
};

use hir::{normalize_rel_path, ModuleDependency, ModuleTarget, Release, RootScope};

use crate::workspace_config::{
    build_tool_name, discover_jdks, parse_build_tool_kind, parse_version_string, probe_workspace,
    reconcile_version_conflict, render_minimal_toml, BuildToolKind, Conflict, ExplicitConfig,
    WorkspaceProbe,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    /// Run the stdio LSP server (default when invoked with no arguments or `lsp`).
    Lsp,
    /// Print CLI help message.
    Help,
    /// Run `java-analyzer configure [OPTIONS] [DIR]`.
    Configure(ConfigureArgs),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleDeltaArgs {
    pub module_key: String,
    pub include_sources: Vec<String>,
    pub exclude_sources: Vec<String>,
    pub include_resources: Vec<String>,
    pub exclude_resources: Vec<String>,
    pub include_deps: Vec<String>,
    pub exclude_deps: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigureArgs {
    pub workspace_dir: Option<PathBuf>,
    pub jdk: Option<PathBuf>,
    pub release: Option<Release>,
    pub preview: Option<bool>,
    pub build: Option<BuildToolKind>,
    pub classpath_cmd: Vec<String>,
    pub include_modules: Vec<String>,
    pub exclude_modules: Vec<String>,
    pub module_deltas: Vec<ModuleDeltaArgs>,
    pub reconcile: bool,
    pub print_only: bool,
    pub yes: bool,
}

impl ConfigureArgs {
    pub fn has_explicit_mutation_flags(&self) -> bool {
        self.jdk.is_some()
            || self.release.is_some()
            || self.preview.is_some()
            || self.build.is_some()
            || !self.classpath_cmd.is_empty()
            || !self.include_modules.is_empty()
            || !self.exclude_modules.is_empty()
            || !self.module_deltas.is_empty()
            || self.reconcile
    }
}

pub fn parse_cli_args<I, S>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut iter = args.into_iter().map(|s| s.as_ref().to_string());
    let Some(first) = iter.next() else {
        return Ok(CliCommand::Lsp);
    };

    match first.as_str() {
        "lsp" | "--stdio" => Ok(CliCommand::Lsp),
        "-h" | "--help" | "help" => Ok(CliCommand::Help),
        "configure" => {
            let rest: Vec<String> = iter.collect();
            parse_configure_args(&rest)
        }
        other => Err(format!(
            "Unknown subcommand or argument '{other}'. Run `java-analyzer --help` for usage."
        )),
    }
}

fn parse_configure_args(args: &[String]) -> Result<CliCommand, String> {
    let mut cfg = ConfigureArgs::default();
    let mut current_module = ".".to_string();
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-h" | "--help" => return Ok(CliCommand::Help),
            "--print" | "--dry-run" => {
                cfg.print_only = true;
                i += 1;
            }
            "-y" | "--yes" => {
                cfg.yes = true;
                i += 1;
            }
            "--reconcile" => {
                cfg.reconcile = true;
                i += 1;
            }
            "--preview" => {
                cfg.preview = Some(true);
                i += 1;
            }
            "--no-preview" => {
                cfg.preview = Some(false);
                i += 1;
            }
            "--jdk" => {
                let val = args.get(i + 1).ok_or("--jdk requires a <PATH> argument")?;
                cfg.jdk = Some(PathBuf::from(val));
                i += 2;
            }
            "--release" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--release requires a Java release number (8..=27)")?;
                let rel = parse_version_string(val)
                    .ok_or_else(|| format!("Unsupported Java release '{val}' (expected 8..=27)"))?;
                cfg.release = Some(rel);
                i += 2;
            }
            "--build" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--build requires a build tool kind (none, maven, gradle, ant, bazel, mill, sbt, elide, jbang, custom)")?;
                let kind = parse_build_tool_kind(val)
                    .ok_or_else(|| format!("Unknown build tool kind '{val}'"))?;
                cfg.build = Some(kind);
                i += 2;
            }
            "--classpath-cmd" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--classpath-cmd requires a command string")?;
                cfg.classpath_cmd = val.split_whitespace().map(ToString::to_string).collect();
                i += 2;
            }
            "--include" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--include requires a module directory path")?;
                cfg.include_modules.push(normalize_rel_path(val));
                i += 2;
            }
            "--exclude" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--exclude requires a module directory path")?;
                cfg.exclude_modules.push(normalize_rel_path(val));
                i += 2;
            }
            "--module" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--module requires a module relative path or JPMS name")?;
                current_module = if val.contains('/') || val == "." {
                    normalize_rel_path(val)
                } else {
                    val.clone()
                };
                i += 2;
            }
            "--include-source" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--include-source requires a directory path")?;
                get_or_insert_mod_delta(&mut cfg.module_deltas, &current_module)
                    .include_sources
                    .push(normalize_rel_path(val));
                i += 2;
            }
            "--exclude-source" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--exclude-source requires a directory path")?;
                get_or_insert_mod_delta(&mut cfg.module_deltas, &current_module)
                    .exclude_sources
                    .push(normalize_rel_path(val));
                i += 2;
            }
            "--include-resource" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--include-resource requires a directory path")?;
                get_or_insert_mod_delta(&mut cfg.module_deltas, &current_module)
                    .include_resources
                    .push(normalize_rel_path(val));
                i += 2;
            }
            "--exclude-resource" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--exclude-resource requires a directory path")?;
                get_or_insert_mod_delta(&mut cfg.module_deltas, &current_module)
                    .exclude_resources
                    .push(normalize_rel_path(val));
                i += 2;
            }
            "--include-dep" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--include-dep requires a dependency spec")?;
                get_or_insert_mod_delta(&mut cfg.module_deltas, &current_module)
                    .include_deps
                    .push(val.clone());
                i += 2;
            }
            "--exclude-dep" => {
                let val = args
                    .get(i + 1)
                    .ok_or("--exclude-dep requires a dependency spec")?;
                get_or_insert_mod_delta(&mut cfg.module_deltas, &current_module)
                    .exclude_deps
                    .push(val.clone());
                i += 2;
            }
            flag if flag.starts_with('-') => {
                return Err(format!("Unknown option '{flag}' for `configure`"));
            }
            dir => {
                cfg.workspace_dir = Some(PathBuf::from(dir));
                i += 1;
            }
        }
    }

    Ok(CliCommand::Configure(cfg))
}

fn get_or_insert_mod_delta<'a>(
    deltas: &'a mut Vec<ModuleDeltaArgs>,
    module_key: &str,
) -> &'a mut ModuleDeltaArgs {
    if let Some(pos) = deltas.iter().position(|d| d.module_key == module_key) {
        return &mut deltas[pos];
    }
    deltas.push(ModuleDeltaArgs {
        module_key: module_key.to_string(),
        ..Default::default()
    });
    deltas.last_mut().unwrap()
}

pub fn help_text() -> &'static str {
    "\
java-analyzer - A lightweight Java LSP server and workspace configurator

USAGE:
    java-analyzer [lsp]
    java-analyzer configure [OPTIONS] [DIR]

SUBCOMMANDS:
    lsp          Start the Language Server Protocol server over stdio (default)
    configure    Inspect, disambiguate, and configure a Java workspace

CONFIGURE OPTIONS:
    --jdk <PATH>                 Pin the JDK installation directory
    --release <N>                Pin the Java release version (8..=27)
    --preview / --no-preview     Enable or disable Java preview features
    --build <KIND>               Disambiguate build tool (none, maven, gradle, ant, bazel, mill, sbt, elide, jbang, custom)
    --include <DIR>              Include an additional workspace module directory
    --exclude <DIR>              Exclude an auto-detected workspace module directory
    --module <PATH_OR_NAME>      Target module for subsequent --include-* / --exclude-* flags (default: \".\")
    --include-source <DIR>       Add a Java source root to the target module
    --exclude-source <DIR>       Remove a Java source root from the target module
    --include-resource <DIR>     Add a resource root to the target module
    --exclude-resource <DIR>     Remove a resource root from the target module
    --include-dep <SPEC>         Add a compile-time dependency (Maven coord, github:owner/repo#ref, path, or module)
    --exclude-dep <SPEC>         Remove a compile-time dependency from the target module
    --classpath-cmd <CMD>        Set custom command to emit compile-time classpath
    --reconcile                  Reconcile conflicting external version files (.java-version, .sdkmanrc, pom.xml, etc.)
    --print, --dry-run           Print inferred workspace layout and minimal .java-analyzer.toml without writing
    -y, --yes                    Run non-interactively and write minimal .java-analyzer.toml if overrides are needed
    -h, --help                   Show this help message
"
}

pub fn run_configure(args: ConfigureArgs) -> io::Result<()> {
    let stdin = io::stdin();
    let is_tty = stdin.is_terminal() && io::stdout().is_terminal();
    let mut reader = stdin.lock();
    let mut stdout = io::stdout();
    run_configure_with_io(args, is_tty, &mut reader, &mut stdout)
}

pub fn run_configure_with_io<R: BufRead, W: Write>(
    args: ConfigureArgs,
    interactive: bool,
    reader: &mut R,
    out: &mut W,
) -> io::Result<()> {
    let workspace_dir = match &args.workspace_dir {
        Some(dir) => dir.clone(),
        None => env::current_dir()?,
    };

    let mut probe = probe_workspace(&workspace_dir);
    let mut desired = probe.explicit_config.clone().unwrap_or_default();

    // Apply CLI flag mutations onto `desired`
    apply_cli_args_to_config(&args, &mut desired);

    // Handle `--reconcile` flag non-interactively
    if args.reconcile {
        let target_release = args.release.unwrap_or(probe.resolved_level.release);
        let modified = reconcile_version_conflict(&workspace_dir, target_release);
        for p in &modified {
            writeln!(
                out,
                "Reconciled external specification: {} -> Java {}",
                p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("spec"),
                target_release.number()
            )?;
        }
        // Re-probe after reconciling external files so `render_minimal_toml` sees the unified release!
        probe = probe_workspace(&workspace_dir);
    }

    render_workspace_summary(&probe, out)?;

    if args.print_only {
        let minimal_toml = render_minimal_toml(&probe, &desired);
        writeln!(out, "\n--- Minimal .java-analyzer.toml ---")?;
        if minimal_toml.is_empty() {
            writeln!(
                out,
                "# (empty: all settings are uniquely inferred from external specs / workspace layout)"
            )?;
        } else {
            write!(out, "{minimal_toml}")?;
        }
        return Ok(());
    }

    // Interactive TUI session when connected to a terminal (or interactive test harness)
    // and neither `-y/--yes` nor explicit CLI mutation flags were given.
    if interactive && !args.yes && !args.has_explicit_mutation_flags() {
        run_interactive_tui(&workspace_dir, &mut probe, &mut desired, reader, out)?;
    }

    let minimal_toml = render_minimal_toml(&probe, &desired);
    let config_path = probe
        .config_file
        .clone()
        .unwrap_or_else(|| workspace_dir.join(".java-analyzer.toml"));

    if minimal_toml.is_empty() {
        if config_path.is_file() {
            fs::remove_file(&config_path)?;
            writeln!(
                out,
                "Removed redundant {} (all settings are uniquely inferred).",
                config_path.display()
            )?;
        } else {
            writeln!(
                out,
                "No .java-analyzer.toml needed (all settings are uniquely inferred)."
            )?;
        }
    } else {
        fs::write(&config_path, &minimal_toml)?;
        writeln!(
            out,
            "Wrote minimal configuration overlay to {}",
            config_path.display()
        )?;
    }

    Ok(())
}

fn apply_cli_args_to_config(args: &ConfigureArgs, cfg: &mut ExplicitConfig) {
    if let Some(build) = args.build {
        cfg.build = Some(build);
    }
    if let Some(jdk) = &args.jdk {
        cfg.jdk = Some(jdk.clone());
    }
    if let Some(rel) = args.release {
        cfg.release = Some(rel);
    }
    if let Some(prev) = args.preview {
        cfg.preview = Some(prev);
    }
    if !args.classpath_cmd.is_empty() {
        cfg.classpath_command = args.classpath_cmd.clone();
    }
    for inc in &args.include_modules {
        if !cfg.delta.include.contains(inc) {
            cfg.delta.include.push(inc.clone());
        }
        cfg.delta.exclude.retain(|x| x != inc);
    }
    for exc in &args.exclude_modules {
        if !cfg.delta.exclude.contains(exc) {
            cfg.delta.exclude.push(exc.clone());
        }
        cfg.delta.include.retain(|x| x != exc);
    }
    for md in &args.module_deltas {
        let entry = cfg.delta.modules.entry(md.module_key.clone()).or_default();
        for s in &md.include_sources {
            if !entry.sources.include.contains(s) {
                entry.sources.include.push(s.clone());
            }
            entry.sources.exclude.retain(|x| x != s);
        }
        for s in &md.exclude_sources {
            if !entry.sources.exclude.contains(s) {
                entry.sources.exclude.push(s.clone());
            }
            entry.sources.include.retain(|x| x != s);
        }
        for r in &md.include_resources {
            if !entry.resources.include.contains(r) {
                entry.resources.include.push(r.clone());
            }
            entry.resources.exclude.retain(|x| x != r);
        }
        for r in &md.exclude_resources {
            if !entry.resources.exclude.contains(r) {
                entry.resources.exclude.push(r.clone());
            }
            entry.resources.include.retain(|x| x != r);
        }
        for d in &md.include_deps {
            if !entry.dependencies.include.contains(d) {
                entry.dependencies.include.push(d.clone());
            }
            entry.dependencies.exclude.retain(|x| x != d);
        }
        for d in &md.exclude_deps {
            if !entry.dependencies.exclude.contains(d) {
                entry.dependencies.exclude.push(d.clone());
            }
            entry.dependencies.include.retain(|x| x != d);
        }
    }
}

fn render_workspace_summary<W: Write>(probe: &WorkspaceProbe, out: &mut W) -> io::Result<()> {
    writeln!(
        out,
        "┌─ java-analyzer configure ─────────────────────────────────────────┐"
    )?;
    writeln!(out, "│ Workspace: {}", probe.workspace_dir.display())?;

    if probe.build_specs.is_empty() {
        writeln!(out, "│ Build Specs: none (Zero-Config Tier 3)")?;
    } else {
        let specs: Vec<String> = probe
            .build_specs
            .iter()
            .map(|s| {
                let fname = s
                    .manifest_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("manifest");
                format!("{} ({})", fname, build_tool_name(s.kind))
            })
            .collect();
        writeln!(out, "│ Build Specs: {}", specs.join(", "))?;
    }

    if !probe.version_specs.is_empty() {
        let vspecs: Vec<String> = probe
            .version_specs
            .iter()
            .map(|v| format!("{} (Java {})", v.source, v.release.number()))
            .collect();
        writeln!(out, "│ Version Specs: {}", vspecs.join(", "))?;
    }

    let preview_suffix = if probe.resolved_level.preview {
        " (--enable-preview)"
    } else {
        ""
    };
    writeln!(
        out,
        "│ Active Build Tool: {} | Active Language Level: Java {}{}",
        build_tool_name(probe.resolved_build),
        probe.resolved_level.release.number(),
        preview_suffix
    )?;

    for conflict in &probe.conflicts {
        writeln!(out, "│ [!] CONFLICT: {}", conflict.diagnostic_message())?;
    }

    writeln!(out, "│ Modules ({}):", probe.layout.modules.len())?;
    for m in &probe.layout.modules {
        let name_label = match &m.module_name {
            Some(jpms) => format!("{} (module {})", m.rel_path, jpms),
            None => format!("{} (unnamed module)", m.rel_path),
        };
        writeln!(
            out,
            "│   • {} [Java {}]",
            name_label,
            m.language_level.release.number()
        )?;
        for src in &m.sources {
            let scope_tag = match src.scope {
                RootScope::Main => "main",
                RootScope::Test => "test",
            };
            writeln!(out, "│       source:   {} [{}]", src.rel_path, scope_tag)?;
        }
        for res in &m.resources {
            let scope_tag = match res.scope {
                RootScope::Main => "main",
                RootScope::Test => "test",
            };
            writeln!(out, "│       resource: {} [{}]", res.rel_path, scope_tag)?;
        }
        for dep in &m.dependencies {
            writeln!(out, "│       dep:      {}", format_dependency(dep))?;
        }
    }

    for diag in &probe.layout.package_mismatches {
        writeln!(out, "│ [!] PACKAGE ERROR: {}", diag.message())?;
    }

    writeln!(
        out,
        "└───────────────────────────────────────────────────────────────────┘"
    )?;
    Ok(())
}

fn format_dependency(dep: &ModuleDependency) -> String {
    match dep {
        ModuleDependency::Module { target, scope, .. } => match target {
            ModuleTarget::Workspace(id) => {
                format!("module:#{} [{}]", id.0, scope_label(*scope))
            }
            ModuleTarget::External(name) => {
                format!("module:{name} [{}]", scope_label(*scope))
            }
        },
        ModuleDependency::Path { path, scope } => {
            format!("path:{path} [{}]", scope_label(*scope))
        }
        ModuleDependency::Maven {
            group,
            artifact,
            version,
            classifier,
            scope,
        } => {
            let cls = classifier
                .as_deref()
                .map(|c| format!(":{c}"))
                .unwrap_or_default();
            format!(
                "maven:{group}:{artifact}:{version}{cls} [{}]",
                scope_label(*scope)
            )
        }
        ModuleDependency::Git {
            repo,
            reference,
            scope,
        } => {
            let r = reference
                .as_deref()
                .map(|v| format!("#{v}"))
                .unwrap_or_default();
            format!("git:{repo}{r} [{}]", scope_label(*scope))
        }
    }
}

fn scope_label(scope: RootScope) -> &'static str {
    match scope {
        RootScope::Main => "main",
        RootScope::Test => "test",
    }
}

/// Interactive readline/choice-widget TUI loop for `java-analyzer configure`.
///
/// Follows the specification rule that when conflicting external specs are detected, the
/// **default (option 1)** action is to reconcile/remove the ambiguity in the external specs
/// so `.java-analyzer.toml` stays empty/minimal, while writing a disambiguation entry to
/// `.java-analyzer.toml` is offered as option 2.
fn run_interactive_tui<R: BufRead, W: Write>(
    workspace_dir: &Path,
    probe: &mut WorkspaceProbe,
    desired: &mut ExplicitConfig,
    reader: &mut R,
    out: &mut W,
) -> io::Result<()> {
    // 1. Resolve any detected conflicts first with choice widgets
    let conflicts = probe.conflicts.clone();
    for conflict in conflicts {
        match conflict {
            Conflict::Release(specs) => {
                writeln!(out, "\n[Conflict] Multiple Java release specifications disagree:")?;
                let mut unique_releases: Vec<Release> = Vec::new();
                for s in &specs {
                    writeln!(out, "  - {}: Java {}", s.source, s.release.number())?;
                    if !unique_releases.contains(&s.release) {
                        unique_releases.push(s.release);
                    }
                }
                unique_releases.sort_by(|a, b| b.cmp(a));
                let default_rel = unique_releases.first().copied().unwrap_or(Release::JAVA_25);

                writeln!(out, "\nHow would you like to resolve this release conflict?")?;
                writeln!(
                    out,
                    "  [1] (Default) Reconcile external version files to a single release (keeps .java-analyzer.toml clean)"
                )?;
                writeln!(
                    out,
                    "  [2] Keep external files unchanged and write `release` override to .java-analyzer.toml"
                )?;
                write!(out, "Select action [1]: ")?;
                out.flush()?;
                let action_choice = read_line_trimmed(reader)?;

                write!(
                    out,
                    "Target Java release (candidates: {}) [{}]: ",
                    unique_releases
                        .iter()
                        .map(|r| r.number().to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    default_rel.number()
                )?;
                out.flush()?;
                let rel_input = read_line_trimmed(reader)?;
                let chosen_rel = if rel_input.is_empty() {
                    default_rel
                } else {
                    parse_version_string(&rel_input).unwrap_or(default_rel)
                };

                if action_choice == "2" {
                    desired.release = Some(chosen_rel);
                } else {
                    let modified = reconcile_version_conflict(workspace_dir, chosen_rel);
                    for p in &modified {
                        writeln!(
                            out,
                            "  -> Updated {} to Java {}",
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("spec"),
                            chosen_rel.number()
                        )?;
                    }
                    *probe = probe_workspace(workspace_dir);
                }
            }
            Conflict::BuildTool(specs) => {
                writeln!(out, "\n[Conflict] Multiple build tool manifests detected:")?;
                let mut kinds: Vec<BuildToolKind> = Vec::new();
                for s in &specs {
                    if !kinds.contains(&s.kind) {
                        kinds.push(s.kind);
                    }
                }
                for (idx, kind) in kinds.iter().enumerate() {
                    writeln!(out, "  [{}] {}", idx + 1, build_tool_name(*kind))?;
                }
                write!(
                    out,
                    "Select authoritative build tool [1 ({})]: ",
                    build_tool_name(kinds[0])
                )?;
                out.flush()?;
                let choice = read_line_trimmed(reader)?;
                let chosen_kind = if choice.is_empty() {
                    kinds[0]
                } else if let Ok(n) = choice.parse::<usize>() {
                    kinds.get(n.saturating_sub(1)).copied().unwrap_or(kinds[0])
                } else {
                    parse_build_tool_kind(&choice).unwrap_or(kinds[0])
                };
                desired.build = Some(chosen_kind);
            }
        }
    }

    // 2. Offer optional interactive customization of JDK or module sources/resources/dependencies
    let jdks = discover_jdks(Some(probe.resolved_level.release));
    if !jdks.is_empty() {
        writeln!(out, "\nDiscovered JDKs:")?;
        writeln!(out, "  [0] Auto-detect (recommended - do not pin in .java-analyzer.toml)")?;
        for (idx, jdk) in jdks.iter().enumerate() {
            let rel_str = jdk
                .release
                .map(|r| format!("Java {}", r.number()))
                .unwrap_or_else(|| "unknown".to_string());
            writeln!(
                out,
                "  [{}] {} ({}, via {})",
                idx + 1,
                jdk.home.display(),
                rel_str,
                jdk.source
            )?;
        }
        write!(out, "Select JDK [0]: ")?;
        out.flush()?;
        let jdk_choice = read_line_trimmed(reader)?;
        if let Ok(n) = jdk_choice.parse::<usize>() {
            if n >= 1 {
                if let Some(chosen) = jdks.get(n - 1) {
                    desired.jdk = Some(chosen.home.clone());
                }
            }
        } else if !jdk_choice.is_empty() && jdk_choice != "0" {
            desired.jdk = Some(PathBuf::from(jdk_choice));
        }
    }

    Ok(())
}

fn read_line_trimmed<R: BufRead>(reader: &mut R) -> io::Result<String> {
    let mut buf = String::new();
    let _ = reader.read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = env::temp_dir().join(format!(
                "java_analyzer_cli_test_{}_{}_{}",
                label,
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, rel: &str, content: &str) {
            let full = self.0.join(rel);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(full, content).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn cli_defaults_to_lsp_and_parses_configure_flags() {
        let cmd = parse_cli_args(Vec::<String>::new()).unwrap();
        assert_eq!(cmd, CliCommand::Lsp);

        let cmd = parse_cli_args([
            "configure",
            "--build",
            "gradle",
            "--release",
            "21",
            "--module",
            "services/core",
            "--include-source",
            "src/gen/java",
            "--include-dep",
            "github:google/guava#v33.3.1",
            "--print",
        ])
        .unwrap();

        match cmd {
            CliCommand::Configure(args) => {
                assert_eq!(args.build, Some(BuildToolKind::Gradle));
                assert_eq!(args.release, Some(Release::JAVA_21));
                assert!(args.print_only);
                assert_eq!(args.module_deltas.len(), 1);
                assert_eq!(args.module_deltas[0].module_key, "services/core");
                assert_eq!(args.module_deltas[0].include_sources, vec!["src/gen/java"]);
                assert_eq!(
                    args.module_deltas[0].include_deps,
                    vec!["github:google/guava#v33.3.1"]
                );
            }
            other => panic!("expected Configure, got {other:?}"),
        }
    }

    #[test]
    fn configure_interactive_default_reconciles_external_specs_instead_of_writing_toml() {
        let tmp = TempDir::new("interactive_reconcile");
        tmp.write(".java-version", "17\n");
        tmp.write(".sdkmanrc", "java=21.0.2-tem\n");
        tmp.write(
            "src/com/example/Main.java",
            "package com.example;\npublic class Main {}\n",
        );

        let args = ConfigureArgs {
            workspace_dir: Some(tmp.0.clone()),
            ..Default::default()
        };

        // Press Enter on action prompt [1 = default reconcile], Enter on release prompt [21], Enter on JDK prompt [0]
        let mut input = Cursor::new("\n\n0\n");
        let mut output = Vec::new();
        run_configure_with_io(args, true, &mut input, &mut output).unwrap();

        let out_str = String::from_utf8(output).unwrap();
        assert!(
            out_str.contains("Updated .java-version to Java 21"),
            "output was:\n{out_str}"
        );
        assert_eq!(
            fs::read_to_string(tmp.0.join(".java-version")).unwrap().trim(),
            "21"
        );
        // And because external files were reconciled, no .java-analyzer.toml was needed!
        assert!(
            !tmp.0.join(".java-analyzer.toml").exists(),
            ".java-analyzer.toml should not be created when external specs were reconciled"
        );
    }

    #[test]
    fn configure_never_hardcodes_uniquely_inferred_values_in_toml() {
        let tmp = TempDir::new("minimal_overlay");
        tmp.write(".java-version", "21\n");
        tmp.write(
            "pom.xml",
            "<project><properties><maven.compiler.release>21</maven.compiler.release></properties></project>",
        );

        // Passing --build maven --release 21 when Maven + Java 21 are already uniquely inferred
        // must produce an empty overlay (no .java-analyzer.toml created).
        let args = ConfigureArgs {
            workspace_dir: Some(tmp.0.clone()),
            build: Some(BuildToolKind::Maven),
            release: Some(Release::JAVA_21),
            yes: true,
            ..Default::default()
        };
        let mut input = Cursor::new("");
        let mut output = Vec::new();
        run_configure_with_io(args, false, &mut input, &mut output).unwrap();

        assert!(
            !tmp.0.join(".java-analyzer.toml").exists(),
            "Uniquely inferred build=maven and release=21 must not be hardcoded into .java-analyzer.toml"
        );
    }
}
