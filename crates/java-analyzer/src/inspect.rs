//! `java-analyzer inspect` command execution and human/JSON workspace reporting.

use std::{
    env, fs,
    io::{self, Write},
    sync::Arc,
};

use ide::{AnalysisHost, FileId, ModuleDependency, ModuleTarget};

use crate::{
    cli::InspectArgs,
    configure::{format_build_specs, format_version_specs, render_module_tree, scope_label},
    workspace_config::{build_tool_name, probe_workspace, Conflict, WorkspaceProbe},
};

pub fn run(args: InspectArgs) -> io::Result<()> {
    let mut stdout = io::stdout();
    run_with_io(args, &mut stdout)
}

pub fn run_with_io<W: Write>(args: InspectArgs, out: &mut W) -> io::Result<()> {
    let workspace_dir = match &args.workspace_dir {
        Some(dir) => dir.clone(),
        None => env::current_dir()?,
    };

    let probe = probe_workspace(&workspace_dir);

    let mut host = AnalysisHost::new();
    let mut file_ids: Vec<FileId> = Vec::with_capacity(probe.file_paths.len());
    for (rel_path, &file_id) in &probe.file_paths {
        let abs_path = probe.workspace_dir.join(rel_path);
        let text = match probe.file_contents.get(rel_path) {
            Some(cached) => Arc::clone(cached),
            None => Arc::new(fs::read_to_string(&abs_path)?),
        };
        let level = probe.language_level_for_path(&abs_path);
        host.set_file_language_level(file_id, level);
        host.set_file_text(file_id, text);
        file_ids.push(file_id);
    }
    host.set_workspace_files(Arc::new(file_ids));

    let analysis = host.analysis();
    let mut files_with_errors = 0usize;
    let mut file_errors: Vec<(String, usize, String)> = Vec::new();
    for (rel_path, &file_id) in &probe.file_paths {
        let errors = analysis.diagnostics(file_id).map_err(io::Error::other)?;
        if !errors.is_empty() {
            files_with_errors += 1;
            file_errors.push((rel_path.clone(), errors.len(), errors[0].message.clone()));
        }
    }

    let (indexed_packages, indexed_types) = analysis
        .workspace_index_stats()
        .map_err(io::Error::other)?;

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

    let rendered = serde_json::to_string_pretty(&payload).map_err(io::Error::other)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TestDir;

    #[test]
    fn inspect_human_output_reports_layout_health_and_errors() {
        let tmp = TestDir::new();
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
        run_with_io(
            InspectArgs {
                workspace_dir: Some(tmp.path().to_path_buf()),
                json: false,
                show_errors: true,
            },
            &mut output,
        )
        .unwrap();

        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("java-analyzer inspect"), "output:\n{text}");
        assert!(
            text.contains("Active Build Tool: maven | Active Language Level: Java 21"),
            "output:\n{text}"
        );
        assert!(
            text.contains("src/main/java [main] (1 .java file)"),
            "output:\n{text}"
        );
        assert!(
            text.contains("src/test/java [test] (1 .java file)"),
            "output:\n{text}"
        );
        assert!(text.contains("src/main/resources [main]"), "output:\n{text}");
        assert!(
            text.contains("maven:com.google.guava:guava:33.2.1-jre [main]"),
            "output:\n{text}"
        );
        assert!(text.contains("Package Mismatches: 0"), "output:\n{text}");
        assert!(
            text.contains("Total .java files: 2 | Files with parse errors: 1"),
            "output:\n{text}"
        );
        assert!(
            text.contains("Indexed packages: 1 | Indexed types: 2"),
            "output:\n{text}"
        );
        assert!(
            text.contains("src/test/java/com/example/BrokenTest.java (1 error): expected '}'"),
            "output:\n{text}"
        );
    }

    #[test]
    fn inspect_json_output_is_structured_and_complete() {
        let tmp = TestDir::new();
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
        run_with_io(
            InspectArgs {
                workspace_dir: Some(tmp.path().to_path_buf()),
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
