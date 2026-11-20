//! CLI argument parser and the `java-analyzer configure` interactive TUI / non-interactive
//! workspace configurator.
//!
//! `java-analyzer configure` is the **only** CLI command that writes `.java-analyzer.toml` or
//! reconciles external specification files.

use std::{
    env, fs,
    io::{self, BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::{Args, Parser, Subcommand};
use hir::{
    normalize_rel_path, DefDatabase, FileId, ModuleDependency, ModuleTarget, Release,
    RootDatabase, RootScope, SourceDatabase,
};

use crate::workspace_config::{
    build_tool_name, discover_jdks, parse_build_tool_kind, parse_version_string, probe_workspace,
    reconcile_version_conflict, render_minimal_toml, BuildToolKind, Conflict, ExplicitConfig,
    WorkspaceProbe,
};

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

pub fn run_inspect(args: InspectArgs) -> io::Result<()> {
    let mut stdout = io::stdout();
    run_inspect_with_io(args, &mut stdout)
}

pub fn run_inspect_with_io<W: Write>(args: InspectArgs, out: &mut W) -> io::Result<()> {
    let workspace_dir = match &args.workspace_dir {
        Some(dir) => dir.clone(),
        None => env::current_dir()?,
    };

    let probe = probe_workspace(&workspace_dir);

    let mut db = RootDatabase::default();
    let mut file_ids: Vec<FileId> = Vec::with_capacity(probe.file_paths.len());
    for (rel_path, &file_id) in &probe.file_paths {
        let abs_path = probe.workspace_dir.join(rel_path);
        let text = fs::read_to_string(&abs_path)?;
        let level = probe.language_level_for_path(&abs_path);
        db.set_file_language_level(file_id, level);
        db.set_file_text(file_id, Arc::new(text));
        file_ids.push(file_id);
    }
    db.set_workspace_files(Arc::new(file_ids));

    let mut files_with_errors = 0usize;
    let mut file_errors: Vec<(String, usize, String)> = Vec::new();
    for (rel_path, &file_id) in &probe.file_paths {
        let errors = db.syntax_errors(file_id);
        if !errors.is_empty() {
            files_with_errors += 1;
            file_errors.push((rel_path.clone(), errors.len(), errors[0].message.clone()));
        }
    }

    let index = db.workspace_index();
    let indexed_packages = index.types_by_package.len();
    let indexed_types = index.types_by_fqn.len();

    if args.json {
        render_inspect_json(
            &probe,
            files_with_errors,
            &file_errors,
            indexed_packages,
            indexed_types,
            args.show_errors,
            out,
        )
    } else {
        render_inspect_human(
            &probe,
            files_with_errors,
            &file_errors,
            indexed_packages,
            indexed_types,
            args.show_errors,
            out,
        )
    }
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

fn apply_delta_items(
    items: &[String],
    normalize: bool,
    add_to: &mut Vec<String>,
    remove_from: &mut Vec<String>,
) {
    for raw in items {
        let val = if normalize {
            normalize_rel_path(raw)
        } else {
            raw.clone()
        };
        if !add_to.contains(&val) {
            add_to.push(val.clone());
        }
        remove_from.retain(|x| x != &val);
    }
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
    if let Some(prev) = args.preview_override() {
        cfg.preview = Some(prev);
    }
    if let Some(cmd) = &args.classpath_cmd {
        let parts: Vec<String> = cmd.split_whitespace().map(ToString::to_string).collect();
        if !parts.is_empty() {
            cfg.classpath_command = parts;
        }
    }
    apply_delta_items(
        &args.include_modules,
        true,
        &mut cfg.delta.include,
        &mut cfg.delta.exclude,
    );
    apply_delta_items(
        &args.exclude_modules,
        true,
        &mut cfg.delta.exclude,
        &mut cfg.delta.include,
    );

    let has_module_deltas = !args.include_source.is_empty()
        || !args.exclude_source.is_empty()
        || !args.include_resource.is_empty()
        || !args.exclude_resource.is_empty()
        || !args.include_dep.is_empty()
        || !args.exclude_dep.is_empty();
    if has_module_deltas {
        let entry = cfg
            .delta
            .modules
            .entry(args.normalized_module_key())
            .or_default();
        apply_delta_items(
            &args.include_source,
            true,
            &mut entry.sources.include,
            &mut entry.sources.exclude,
        );
        apply_delta_items(
            &args.exclude_source,
            true,
            &mut entry.sources.exclude,
            &mut entry.sources.include,
        );
        apply_delta_items(
            &args.include_resource,
            true,
            &mut entry.resources.include,
            &mut entry.resources.exclude,
        );
        apply_delta_items(
            &args.exclude_resource,
            true,
            &mut entry.resources.exclude,
            &mut entry.resources.include,
        );
        apply_delta_items(
            &args.include_dep,
            false,
            &mut entry.dependencies.include,
            &mut entry.dependencies.exclude,
        );
        apply_delta_items(
            &args.exclude_dep,
            false,
            &mut entry.dependencies.exclude,
            &mut entry.dependencies.include,
        );
    }
}

fn format_build_specs(probe: &WorkspaceProbe) -> String {
    if probe.build_specs.is_empty() {
        "none (Zero-Config Tier 3)".to_string()
    } else {
        probe
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
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_version_specs(probe: &WorkspaceProbe) -> Option<String> {
    if probe.version_specs.is_empty() {
        None
    } else {
        let vspecs: Vec<String> = probe
            .version_specs
            .iter()
            .map(|v| {
                let prev = if v.preview { " --enable-preview" } else { "" };
                format!("{} (Java {}{})", v.source, v.release.number(), prev)
            })
            .collect();
        Some(vspecs.join(", "))
    }
}

fn render_module_tree<W: Write>(
    probe: &WorkspaceProbe,
    include_file_counts: bool,
    out: &mut W,
) -> io::Result<()> {
    writeln!(out, "│ Modules ({}):", probe.layout.modules.len())?;
    for m in &probe.layout.modules {
        let name_label = match &m.module_name {
            Some(jpms) => format!("{} (module {})", m.rel_path, jpms),
            None => format!("{} (unnamed module)", m.rel_path),
        };
        let preview_suffix = if m.language_level.preview {
            " --enable-preview"
        } else {
            ""
        };
        writeln!(
            out,
            "│   • {} [Java {}{}]",
            name_label,
            m.language_level.release.number(),
            preview_suffix
        )?;
        for src in &m.sources {
            if include_file_counts {
                let count = src.files.len();
                let unit = if count == 1 { "file" } else { "files" };
                writeln!(
                    out,
                    "│       source:   {} [{}] ({} .java {})",
                    src.rel_path,
                    scope_label(src.scope),
                    count,
                    unit
                )?;
            } else {
                writeln!(
                    out,
                    "│       source:   {} [{}]",
                    src.rel_path,
                    scope_label(src.scope)
                )?;
            }
        }
        for res in &m.resources {
            writeln!(
                out,
                "│       resource: {} [{}]",
                res.rel_path,
                scope_label(res.scope)
            )?;
        }
        for dep in &m.dependencies {
            writeln!(out, "│       dep:      {}", format_dependency(dep))?;
        }
    }
    Ok(())
}

fn render_inspect_human<W: Write>(
    probe: &WorkspaceProbe,
    files_with_errors: usize,
    file_errors: &[(String, usize, String)],
    indexed_packages: usize,
    indexed_types: usize,
    show_errors: bool,
    out: &mut W,
) -> io::Result<()> {
    writeln!(
        out,
        "┌─ java-analyzer inspect ───────────────────────────────────────────┐"
    )?;
    writeln!(out, "│ Workspace: {}", probe.workspace_dir.display())?;
    let config_label = probe
        .config_file
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "none".to_string());
    writeln!(out, "│ Config File: {config_label}")?;
    writeln!(out, "│ Build Specs: {}", format_build_specs(probe))?;
    if let Some(vspecs) = format_version_specs(probe) {
        writeln!(out, "│ Version Specs: {vspecs}")?;
    } else {
        writeln!(out, "│ Version Specs: none")?;
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
    let jdk_label = probe
        .resolved_jdk
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "none".to_string());
    writeln!(out, "│ Resolved JDK: {jdk_label}")?;

    if probe.conflicts.is_empty() {
        writeln!(out, "│ Conflicts: none")?;
    } else {
        for conflict in &probe.conflicts {
            writeln!(out, "│ [!] CONFLICT: {}", conflict.diagnostic_message())?;
        }
    }

    render_module_tree(probe, true, out)?;

    writeln!(
        out,
        "│ Package Mismatches: {}",
        probe.layout.package_mismatches.len()
    )?;
    for diag in &probe.layout.package_mismatches {
        writeln!(out, "│   • {}: {}", diag.path, diag.message())?;
    }

    writeln!(out, "│ Health & Index:")?;
    writeln!(
        out,
        "│   • Total .java files: {} | Files with parse errors: {}",
        probe.file_paths.len(),
        files_with_errors
    )?;
    writeln!(
        out,
        "│   • Indexed packages: {} | Indexed types: {}",
        indexed_packages, indexed_types
    )?;

    if show_errors && !file_errors.is_empty() {
        writeln!(out, "│   • Parse errors:")?;
        for (rel_path, count, first_msg) in file_errors {
            let unit = if *count == 1 { "error" } else { "errors" };
            writeln!(out, "│       {rel_path} ({count} {unit}): {first_msg}")?;
        }
    }

    writeln!(
        out,
        "└───────────────────────────────────────────────────────────────────┘"
    )?;
    Ok(())
}

fn render_inspect_json<W: Write>(
    probe: &WorkspaceProbe,
    files_with_errors: usize,
    file_errors: &[(String, usize, String)],
    indexed_packages: usize,
    indexed_types: usize,
    show_errors: bool,
    out: &mut W,
) -> io::Result<()> {
    let parse_errors_json = show_errors.then(|| {
        file_errors
            .iter()
            .map(|(path, count, msg)| {
                serde_json::json!({
                    "path": path,
                    "error_count": count,
                    "first_error": msg,
                })
            })
            .collect::<Vec<_>>()
    });

    let payload = serde_json::json!({
        "workspace_dir": probe.workspace_dir.display().to_string(),
        "config_file": probe.config_file.as_ref().map(|p| p.display().to_string()),
        "build_specs": probe.build_specs.iter().map(|s| serde_json::json!({
            "kind": build_tool_name(s.kind),
            "manifest_path": s.manifest_path.display().to_string(),
        })).collect::<Vec<_>>(),
        "version_specs": probe.version_specs.iter().map(|v| serde_json::json!({
            "source": v.source,
            "path": v.path.display().to_string(),
            "release": v.release.number(),
            "preview": v.preview,
        })).collect::<Vec<_>>(),
        "active_build_tool": build_tool_name(probe.resolved_build),
        "active_language_level": {
            "release": probe.resolved_level.release.number(),
            "preview": probe.resolved_level.preview,
        },
        "resolved_jdk": probe.resolved_jdk.as_ref().map(|p| p.display().to_string()),
        "conflicts": probe.conflicts.iter().map(|c| serde_json::json!({
            "kind": match c {
                Conflict::BuildTool(_) => "build_tool",
                Conflict::Release(_) => "release",
            },
            "message": c.diagnostic_message(),
        })).collect::<Vec<_>>(),
        "modules": probe.layout.modules.iter().map(|m| serde_json::json!({
            "id": m.id.0,
            "rel_path": m.rel_path,
            "module_name": m.module_name,
            "language_level": {
                "release": m.language_level.release.number(),
                "preview": m.language_level.preview,
            },
            "sources": m.sources.iter().map(|s| serde_json::json!({
                "rel_path": s.rel_path,
                "scope": scope_label(s.scope),
                "file_count": s.files.len(),
            })).collect::<Vec<_>>(),
            "resources": m.resources.iter().map(|r| serde_json::json!({
                "rel_path": r.rel_path,
                "scope": scope_label(r.scope),
            })).collect::<Vec<_>>(),
            "dependencies": m.dependencies.iter().map(dependency_to_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "package_mismatches": probe.layout.package_mismatches.iter().map(|pm| serde_json::json!({
            "path": pm.path,
            "declared": pm.declared,
            "expected": pm.expected,
            "authoritative_package_info": pm.authoritative_package_info,
            "message": pm.message(),
        })).collect::<Vec<_>>(),
        "health": {
            "total_java_files": probe.file_paths.len(),
            "files_with_parse_errors": files_with_errors,
            "indexed_packages": indexed_packages,
            "indexed_types": indexed_types,
            "parse_errors": parse_errors_json,
        }
    });

    let rendered = serde_json::to_string_pretty(&payload)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    writeln!(out, "{rendered}")
}

fn dependency_to_json(dep: &ModuleDependency) -> serde_json::Value {
    match dep {
        ModuleDependency::Module {
            target,
            scope,
            exported,
        } => match target {
            ModuleTarget::Workspace(id) => serde_json::json!({
                "kind": "workspace_module",
                "module_id": id.0,
                "scope": scope_label(*scope),
                "exported": exported,
            }),
            ModuleTarget::External(name) => serde_json::json!({
                "kind": "external_module",
                "name": name,
                "scope": scope_label(*scope),
                "exported": exported,
            }),
        },
        ModuleDependency::Path { path, scope } => serde_json::json!({
            "kind": "path",
            "path": path,
            "scope": scope_label(*scope),
        }),
        ModuleDependency::Maven {
            group,
            artifact,
            version,
            classifier,
            scope,
        } => serde_json::json!({
            "kind": "maven",
            "group": group,
            "artifact": artifact,
            "version": version,
            "classifier": classifier,
            "scope": scope_label(*scope),
        }),
        ModuleDependency::Git {
            repo,
            reference,
            scope,
        } => serde_json::json!({
            "kind": "git",
            "repo": repo,
            "reference": reference,
            "scope": scope_label(*scope),
        }),
    }
}

fn render_workspace_summary<W: Write>(probe: &WorkspaceProbe, out: &mut W) -> io::Result<()> {
    writeln!(
        out,
        "┌─ java-analyzer configure ─────────────────────────────────────────┐"
    )?;
    writeln!(out, "│ Workspace: {}", probe.workspace_dir.display())?;
    writeln!(out, "│ Build Specs: {}", format_build_specs(probe))?;

    if let Some(vspecs) = format_version_specs(probe) {
        writeln!(out, "│ Version Specs: {vspecs}")?;
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

    render_module_tree(probe, false, out)?;

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
    fn cli_defaults_to_lsp_and_supports_stdio_and_lsp_subcommand() {
        let default_cli = Cli::try_parse_from(["java-analyzer"]).unwrap();
        assert!(!default_cli.stdio);
        assert_eq!(default_cli.command, None);

        let stdio_cli = Cli::try_parse_from(["java-analyzer", "--stdio"]).unwrap();
        assert!(stdio_cli.stdio);
        assert_eq!(stdio_cli.command, None);

        let lsp_cli = Cli::try_parse_from(["java-analyzer", "lsp"]).unwrap();
        assert!(!lsp_cli.stdio);
        assert_eq!(lsp_cli.command, Some(Command::Lsp));
    }

    #[test]
    fn cli_parses_configure_flags_and_applies_config() {
        let cli = Cli::try_parse_from([
            "java-analyzer",
            "configure",
            "--build",
            "gradle",
            "--release",
            "21",
            "--preview",
            "--classpath-cmd",
            "mvn dependency:build-classpath -Dmdep.outputFile=/dev/stdout",
            "--include",
            "./extra-mod/",
            "--exclude",
            "./legacy-mod/",
            "--module",
            "./services/core/",
            "--include-source",
            "./src/gen/java/",
            "--exclude-source",
            "src/old/java",
            "--include-resource",
            "src/extra/resources",
            "--exclude-resource",
            "src/legacy/resources",
            "--include-dep",
            "github:google/guava#v33.3.1",
            "--exclude-dep",
            "maven:junit:junit",
            "--reconcile",
            "--print",
            "-y",
            "/tmp/ws",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Configure(args)) => {
                assert_eq!(args.build, Some(BuildToolKind::Gradle));
                assert_eq!(args.release, Some(Release::JAVA_21));
                assert!(args.preview);
                assert!(!args.no_preview);
                assert_eq!(args.preview_override(), Some(true));
                assert_eq!(
                    args.classpath_cmd.as_deref(),
                    Some("mvn dependency:build-classpath -Dmdep.outputFile=/dev/stdout")
                );
                assert_eq!(args.module, "./services/core/");
                assert_eq!(args.normalized_module_key(), "services/core");
                assert!(args.reconcile);
                assert!(args.print_only);
                assert!(args.yes);
                assert_eq!(args.workspace_dir, Some(PathBuf::from("/tmp/ws")));
                assert!(args.has_explicit_mutation_flags());

                let mut cfg = ExplicitConfig::default();
                apply_cli_args_to_config(&args, &mut cfg);
                assert_eq!(cfg.build, Some(BuildToolKind::Gradle));
                assert_eq!(cfg.release, Some(Release::JAVA_21));
                assert_eq!(cfg.preview, Some(true));
                assert_eq!(
                    cfg.classpath_command,
                    vec![
                        "mvn",
                        "dependency:build-classpath",
                        "-Dmdep.outputFile=/dev/stdout"
                    ]
                );
                assert_eq!(cfg.delta.include, vec!["extra-mod"]);
                assert_eq!(cfg.delta.exclude, vec!["legacy-mod"]);
                let mod_delta = cfg.delta.modules.get("services/core").unwrap();
                assert_eq!(mod_delta.sources.include, vec!["src/gen/java"]);
                assert_eq!(mod_delta.sources.exclude, vec!["src/old/java"]);
                assert_eq!(mod_delta.resources.include, vec!["src/extra/resources"]);
                assert_eq!(mod_delta.resources.exclude, vec!["src/legacy/resources"]);
                assert_eq!(
                    mod_delta.dependencies.include,
                    vec!["github:google/guava#v33.3.1"]
                );
                assert_eq!(mod_delta.dependencies.exclude, vec!["maven:junit:junit"]);
            }
            other => panic!("expected Configure, got {other:?}"),
        }
    }

    #[test]
    fn cli_supports_dry_run_alias_no_preview_and_module_key_normalization() {
        let cli = Cli::try_parse_from([
            "java-analyzer",
            "configure",
            "--dry-run",
            "--no-preview",
            "--module",
            "com.example.core",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Configure(args)) => {
                assert!(args.print_only);
                assert!(args.no_preview);
                assert_eq!(args.preview_override(), Some(false));
                assert_eq!(args.normalized_module_key(), "com.example.core");
                assert!(args.has_explicit_mutation_flags());
            }
            other => panic!("expected Configure, got {other:?}"),
        }

        let default_args = ConfigureArgs::default();
        assert_eq!(default_args.normalized_module_key(), ".");
        assert_eq!(default_args.preview_override(), None);
        assert!(!default_args.has_explicit_mutation_flags());

        let conflict =
            Cli::try_parse_from(["java-analyzer", "configure", "--preview", "--no-preview"])
                .unwrap_err();
        assert_eq!(conflict.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn cli_rejects_invalid_release_and_build_values() {
        let bad_release =
            Cli::try_parse_from(["java-analyzer", "configure", "--release", "99"]).unwrap_err();
        assert_eq!(bad_release.kind(), clap::error::ErrorKind::ValueValidation);

        let non_num_release =
            Cli::try_parse_from(["java-analyzer", "configure", "--release", "abc"]).unwrap_err();
        assert_eq!(
            non_num_release.kind(),
            clap::error::ErrorKind::ValueValidation
        );

        let bad_build =
            Cli::try_parse_from(["java-analyzer", "configure", "--build", "unknown"]).unwrap_err();
        assert_eq!(bad_build.kind(), clap::error::ErrorKind::ValueValidation);
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

    #[test]
    fn cli_parses_inspect_flags_and_help() {
        let cli =
            Cli::try_parse_from(["java-analyzer", "inspect", "--json", "--show-errors", "/tmp/demo"])
                .unwrap();
        assert_eq!(
            cli.command,
            Some(Command::Inspect(InspectArgs {
                workspace_dir: Some(PathBuf::from("/tmp/demo")),
                json: true,
                show_errors: true,
            }))
        );

        let default_cli = Cli::try_parse_from(["java-analyzer", "inspect"]).unwrap();
        assert_eq!(
            default_cli.command,
            Some(Command::Inspect(InspectArgs {
                workspace_dir: None,
                json: false,
                show_errors: false,
            }))
        );

        let top_help = Cli::try_parse_from(["java-analyzer", "--help"]).unwrap_err();
        assert_eq!(top_help.kind(), clap::error::ErrorKind::DisplayHelp);
        let top_help_text = top_help.to_string();
        assert!(top_help_text.contains("lsp"));
        assert!(top_help_text.contains("inspect"));
        assert!(top_help_text.contains("configure"));
        assert!(!top_help_text.contains("--stdio"));

        let inspect_help =
            Cli::try_parse_from(["java-analyzer", "inspect", "--help"]).unwrap_err();
        assert_eq!(inspect_help.kind(), clap::error::ErrorKind::DisplayHelp);
        let inspect_help_text = inspect_help.to_string();
        assert!(inspect_help_text.contains("--json"));
        assert!(inspect_help_text.contains("--show-errors"));

        let configure_help =
            Cli::try_parse_from(["java-analyzer", "configure", "--help"]).unwrap_err();
        assert_eq!(configure_help.kind(), clap::error::ErrorKind::DisplayHelp);
        let configure_help_text = configure_help.to_string();
        assert!(configure_help_text.contains("--print"));
        assert!(configure_help_text.contains("--dry-run"));
        assert!(configure_help_text.contains("--release"));

        let unknown = Cli::try_parse_from(["java-analyzer", "inspect", "--unknown"]).unwrap_err();
        assert_eq!(unknown.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn inspect_human_output_reports_layout_health_and_errors() {
        let tmp = TempDir::new("inspect_human");
        tmp.write(
            "pom.xml",
            r#"<project>
                <properties>
                    <maven.compiler.release>21</maven.compiler.release>
                </properties>
                <dependencies>
                    <dependency>
                        <groupId>com.google.guava</groupId>
                        <artifactId>guava</artifactId>
                        <version>33.2.1-jre</version>
                    </dependency>
                </dependencies>
            </project>"#,
        );
        tmp.write(
            "src/main/java/com/example/App.java",
            "package com.example;\npublic class App {\n    public static void main(String[] args) {}\n}\n",
        );
        tmp.write(
            "src/test/java/com/example/BrokenTest.java",
            "package com.example;\npublic class BrokenTest {\n",
        );
        tmp.write("src/main/resources/app.properties", "name=demo\n");

        let mut output = Vec::new();
        run_inspect_with_io(
            InspectArgs {
                workspace_dir: Some(tmp.0.clone()),
                json: false,
                show_errors: true,
            },
            &mut output,
        )
        .unwrap();

        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("java-analyzer inspect"), "output:\n{text}");
        assert!(text.contains("Active Build Tool: maven | Active Language Level: Java 21"), "output:\n{text}");
        assert!(text.contains("src/main/java [main] (1 .java file)"), "output:\n{text}");
        assert!(text.contains("src/test/java [test] (1 .java file)"), "output:\n{text}");
        assert!(text.contains("src/main/resources [main]"), "output:\n{text}");
        assert!(text.contains("maven:com.google.guava:guava:33.2.1-jre [main]"), "output:\n{text}");
        assert!(text.contains("Package Mismatches: 0"), "output:\n{text}");
        assert!(text.contains("Total .java files: 2 | Files with parse errors: 1"), "output:\n{text}");
        assert!(text.contains("Indexed packages: 1 | Indexed types: 2"), "output:\n{text}");
        assert!(text.contains("src/test/java/com/example/BrokenTest.java (1 error): expected '}'"), "output:\n{text}");
    }

    #[test]
    fn inspect_json_output_is_structured_and_complete() {
        let tmp = TempDir::new("inspect_json");
        tmp.write(".java-version", "17\n");
        tmp.write(
            "src/com/example/Main.java",
            "package com.example;\npublic class Main { class Inner {} }\n",
        );
        tmp.write(
            "src/com/example/Wrong.java",
            "package com.wrong;\npublic class Wrong {\n",
        );

        let mut output = Vec::new();
        run_inspect_with_io(
            InspectArgs {
                workspace_dir: Some(tmp.0.clone()),
                json: true,
                show_errors: true,
            },
            &mut output,
        )
        .unwrap();

        let val: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(val["active_build_tool"], "none");
        assert_eq!(val["active_language_level"]["release"], 17);
        assert_eq!(val["active_language_level"]["preview"], false);
        assert_eq!(val["modules"].as_array().unwrap().len(), 1);
        assert_eq!(val["modules"][0]["rel_path"], ".");
        assert_eq!(val["modules"][0]["sources"][0]["rel_path"], "src");
        assert_eq!(val["modules"][0]["sources"][0]["scope"], "main");
        assert_eq!(val["modules"][0]["sources"][0]["file_count"], 2);
        assert_eq!(val["package_mismatches"].as_array().unwrap().len(), 1);
        assert_eq!(val["health"]["total_java_files"], 2);
        assert_eq!(val["health"]["files_with_parse_errors"], 1);
        assert_eq!(val["health"]["indexed_packages"], 2);
        assert_eq!(val["health"]["indexed_types"], 3);
        let errs = val["health"]["parse_errors"].as_array().unwrap();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0]["path"], "src/com/example/Wrong.java");
        assert_eq!(errs[0]["error_count"], 1);
        assert_eq!(errs[0]["first_error"], "expected '}'");
    }
}
