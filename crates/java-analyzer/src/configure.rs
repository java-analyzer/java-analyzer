//! `java-analyzer configure` interactive TUI and non-interactive workspace configurator.
//!
//! `java-analyzer configure` is the **only** CLI command that writes `.java-analyzer.toml` or
//! reconciles external specification files.

use std::{
    env, fs,
    io::{self, BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
};

use hir::{normalize_rel_path, ModuleDependency, ModuleTarget, Release, RootScope};

use crate::{
    cli::ConfigureArgs,
    workspace_config::{
        build_tool_name, discover_jdks, parse_build_tool_kind, parse_version_string,
        probe_workspace, reconcile_version_conflict, render_minimal_toml, BuildToolKind, Conflict,
        ExplicitConfig, WorkspaceProbe,
    },
};

pub fn run(args: ConfigureArgs) -> io::Result<()> {
    let stdin = io::stdin();
    let is_tty = stdin.is_terminal() && io::stdout().is_terminal();
    let mut reader = stdin.lock();
    let mut stdout = io::stdout();
    run_with_io(args, is_tty, &mut reader, &mut stdout)
}

pub fn run_with_io<R: BufRead, W: Write>(
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

pub(crate) fn apply_cli_args_to_config(args: &ConfigureArgs, cfg: &mut ExplicitConfig) {
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

pub(crate) fn format_build_specs(probe: &WorkspaceProbe) -> String {
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

pub(crate) fn format_version_specs(probe: &WorkspaceProbe) -> Option<String> {
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

pub(crate) fn render_module_tree<W: Write>(
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

pub(crate) fn format_dependency(dep: &ModuleDependency) -> String {
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

pub(crate) fn scope_label(scope: RootScope) -> &'static str {
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
                "java_analyzer_configure_test_{}_{}_{}",
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
        run_with_io(args, true, &mut input, &mut output).unwrap();

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
        run_with_io(args, false, &mut input, &mut output).unwrap();

        assert!(
            !tmp.0.join(".java-analyzer.toml").exists(),
            "Uniquely inferred build=maven and release=21 must not be hardcoded into .java-analyzer.toml"
        );
    }

    #[test]
    fn apply_cli_args_updates_explicit_config() {
        let args = ConfigureArgs {
            build: Some(BuildToolKind::Gradle),
            release: Some(Release::JAVA_21),
            preview: true,
            classpath_cmd: Some(
                "mvn dependency:build-classpath -Dmdep.outputFile=/dev/stdout".to_string(),
            ),
            include_modules: vec!["./extra-mod/".to_string()],
            exclude_modules: vec!["./legacy-mod/".to_string()],
            module: "./services/core/".to_string(),
            include_source: vec!["./src/gen/java/".to_string()],
            exclude_source: vec!["src/old/java".to_string()],
            include_resource: vec!["src/extra/resources".to_string()],
            exclude_resource: vec!["src/legacy/resources".to_string()],
            include_dep: vec!["github:google/guava#v33.3.1".to_string()],
            exclude_dep: vec!["maven:junit:junit".to_string()],
            ..Default::default()
        };

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
}
