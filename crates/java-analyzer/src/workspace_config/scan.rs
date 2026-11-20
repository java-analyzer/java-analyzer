use std::{collections::BTreeSet, fs, path::Path};

use hir::normalize_rel_path;
use ignore::{overrides::OverrideBuilder, WalkBuilder};

/// Default fallback directory/path exclusions when scanning a workspace.
const DEFAULT_IGNORED_GLOBS: &[&str] = &[
    // VCS / IDE directories
    "!.git/",
    "!.jj/",
    "!.hg/",
    "!.svn/",
    "!.gradle/",
    "!.idea/",
    "!.vscode/",
    "!.zed/",
    "!.bsp/",
    "!.bloop/",
    "!.metals/",
    "!.settings/",
    // Build outputs / dependencies
    "!target/",
    "!build/",
    "!out/",
    "!dist/",
    "!bin/",
    "!node_modules/",
    // Maven site docs
    "!src/site/",
    "!**/src/site/",
];

fn is_meta_inf_resource_root(dir: &Path) -> bool {
    let meta_inf = dir.join("META-INF");
    meta_inf.is_dir()
        && (meta_inf.join("MANIFEST.MF").is_file() || meta_inf.join("services").is_dir())
}

fn is_conventional_resource_dir(rel_norm: &str, parent_dir: &Path) -> bool {
    rel_norm == "resources"
        || rel_norm == "src/main/resources"
        || rel_norm == "src/test/resources"
        || rel_norm.ends_with("/src/main/resources")
        || rel_norm.ends_with("/src/test/resources")
        || (rel_norm.ends_with("/resources") && parent_dir.join("src").is_dir())
}

fn is_noncompilable_resource_dir(norm: &str) -> bool {
    let last_seg = norm.rsplit('/').next().unwrap_or(norm);
    last_seg.starts_with("resources-") && (norm.starts_with("src/") || norm.contains("/src/"))
}

fn is_in_skipped_root(norm: &str, skipped_java_roots: &[String]) -> bool {
    skipped_java_roots.iter().any(|r| {
        norm.strip_prefix(r.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

pub(super) fn scan_workspace_fs(
    root: &Path,
    java_files: &mut Vec<(String, String)>,
    resource_dirs: &mut BTreeSet<String>,
) {
    if is_meta_inf_resource_root(root) {
        resource_dirs.insert(".".to_string());
    }

    let mut override_builder = OverrideBuilder::new(root);
    for glob in DEFAULT_IGNORED_GLOBS {
        let _ = override_builder.add(glob);
    }

    let mut walker = WalkBuilder::new(root);
    walker
        .standard_filters(true)
        .require_git(false)
        .hidden(false)
        .sort_by_file_name(|a, b| a.cmp(b));
    if let Ok(overrides) = override_builder.build() {
        walker.overrides(overrides);
    }

    let mut skipped_java_roots: Vec<String> = Vec::new();

    for entry in walker.build().flatten() {
        if entry.depth() == 0 {
            continue;
        }
        let path = entry.path();
        let Some(name_str) = entry.file_name().to_str() else {
            continue;
        };
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel_norm = normalize_rel_path(&rel.to_string_lossy());
        let in_skipped_root = is_in_skipped_root(&rel_norm, &skipped_java_roots);

        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            // Conventional resource dirs and `src/*/resources-*` skip `.java` fixtures; directories
            // that merely contain `META-INF/` (e.g. Tomcat's `test/` or workspace root) still scan `.java`.
            if is_noncompilable_resource_dir(&rel_norm) {
                skipped_java_roots.push(rel_norm);
            } else if name_str == "resources"
                && path
                    .parent()
                    .is_some_and(|p| is_conventional_resource_dir(&rel_norm, p))
            {
                resource_dirs.insert(rel_norm.clone());
                skipped_java_roots.push(rel_norm);
            } else if !in_skipped_root && is_meta_inf_resource_root(path) {
                resource_dirs.insert(rel_norm);
            }
        } else if !in_skipped_root
            && name_str.ends_with(".java")
            && (entry.file_type().is_some_and(|ft| ft.is_file()) || path.is_file())
        {
            if let Ok(text) = fs::read_to_string(path) {
                java_files.push((rel_norm, text));
            }
        }
    }
}
