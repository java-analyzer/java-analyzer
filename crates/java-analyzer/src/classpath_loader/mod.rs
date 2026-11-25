//! On-demand JDK (`.jmod` / `lib/modules` / `rt.jar`), local `.jar` (including JEP 238 MR-JARs), and
//! class-directory classpath indexing, source/stub materialization, and Javadoc lookup.
//!
//! See `docs/specs/0006-jdk-and-jar-classpath-indexing.md`.

mod archive;
mod jdk;
mod jimage;

#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use ide::{
    BinaryOrigin, BinaryTarget, ClasspathIndex, ModuleDependency, TextRange,
    extract_javadoc_from_html, extract_javadoc_from_source, locate_binary_symbol_in_source,
    synthesize_binary_stub,
};

use crate::workspace_config::WorkspaceProbe;

pub use jdk::jdk_version_key;
#[cfg(test)]
use jdk::{JMOD_MAGIC, ct_sym_version_char};

/// Paths to optional companion source and Javadoc archives discovered alongside a `.jar`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JarCompanionPaths {
    pub sources_jar: Option<PathBuf>,
    pub javadoc_jar: Option<PathBuf>,
}

/// Mapping from `BinaryOrigin::Jar { label }` to discovered companion archives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JarCompanions {
    by_label: BTreeMap<String, JarCompanionPaths>,
}

impl JarCompanions {
    #[must_use]
    pub fn get(&self, label: &str) -> Option<&JarCompanionPaths> {
        self.by_label.get(label)
    }

    pub fn insert(&mut self, label: impl Into<String>, paths: JarCompanionPaths) {
        self.by_label.insert(label.into(), paths);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_label.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_label.len()
    }
}

/// Loads all binary types and JPMS modules from the probed JDK and workspace local dependencies.
#[must_use]
pub fn load_classpath(probe: &WorkspaceProbe) -> (ClasspathIndex, JarCompanions) {
    let mut index = ClasspathIndex::default();
    let mut companions = JarCompanions::default();

    let target_release = probe.resolved_level.release;

    if let Some(jdk_home) = &probe.resolved_jdk {
        jdk::load_jdk_into(&mut index, jdk_home, target_release);
    }
    let mut seen_paths: BTreeSet<PathBuf> = BTreeSet::new();

    for module in &probe.layout.modules {
        for dep in &module.dependencies {
            let ModuleDependency::Path { path, .. } = dep else {
                continue;
            };
            let raw = Path::new(path);
            let resolved = if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                probe.workspace_dir.join(raw)
            };
            if !seen_paths.insert(resolved.clone()) {
                continue;
            }

            if resolved.is_file() {
                let is_jar = resolved
                    .extension()
                    .and_then(OsStr::to_str)
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"));
                if is_jar {
                    if let Some(found) = archive::discover_jar_companions(&resolved) {
                        companions.insert(path.clone(), found);
                    }
                    if let Some(partial) = archive::load_jar(&resolved, path, target_release) {
                        index.merge(partial);
                    }
                }
            } else if resolved.is_dir() {
                if let Some(partial) = archive::load_class_directory(&resolved, path) {
                    index.merge(partial);
                }
            } else {
                tracing::warn!(
                    path = %resolved.display(),
                    "configured classpath dependency path does not exist"
                );
            }
        }
    }

    index.finalize();
    (index, companions)
}

/// Returns the user-scoped cache root directory for extracted JDK/JAR sources and stubs.
#[must_use]
pub fn cache_root() -> PathBuf {
    if let Some(xdg) = env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(xdg).join("java-analyzer");
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = env::var_os("HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home).join("Library/Caches/java-analyzer");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(home) = env::var_os("HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home).join(".cache/java-analyzer");
    }

    #[cfg(windows)]
    if let Some(local) = env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        return PathBuf::from(local).join("java-analyzer");
    }

    env::temp_dir().join("java-analyzer")
}

/// Materializes the `.java` source (from JDK `src.zip` or companion `*-sources.jar`) or a
/// synthesized `.java` stub on disk under [`cache_root()`], and locates `target` inside it.
#[must_use]
pub fn materialize_binary_location(
    classpath: &ClasspathIndex,
    jdk_home: Option<&Path>,
    companions: &JarCompanions,
    target: &BinaryTarget,
) -> Option<(PathBuf, String, Option<TextRange>)> {
    materialize_binary_location_in_cache_root(
        &cache_root(),
        classpath,
        jdk_home,
        companions,
        target,
    )
}

/// Same as [`materialize_binary_location`], using an explicit cache root (for isolated tests).
#[must_use]
pub fn materialize_binary_location_in_cache_root(
    cache_root: &Path,
    classpath: &ClasspathIndex,
    jdk_home: Option<&Path>,
    companions: &JarCompanions,
    target: &BinaryTarget,
) -> Option<(PathBuf, String, Option<TextRange>)> {
    let top_level_internal = archive::resolve_top_level_internal_name(Some(classpath), target);
    let rel_java = format!("{top_level_internal}.java");
    let raw_rel_java = format!("{}.java", target.internal_name);

    let (stub_key, extracted) = match &target.origin {
        BinaryOrigin::Jdk { module_name } => {
            let jdk_key = jdk_home.map_or_else(|| "jdk-unknown".to_string(), jdk_version_key);
            let mut candidates = Vec::new();
            if let Some(m) = module_name {
                candidates.push(format!("{m}/{rel_java}"));
                if raw_rel_java != rel_java {
                    candidates.push(format!("{m}/{raw_rel_java}"));
                }
            }
            candidates.push(rel_java.clone());
            if raw_rel_java != rel_java {
                candidates.push(raw_rel_java);
            }

            let src_dir = cache_root.join("jdk-src").join(&jdk_key);
            let materialized = archive::find_cached_source(&src_dir, &candidates).or_else(|| {
                let src_zip = jdk::find_jdk_src_zip(jdk_home?)?;
                archive::extract_and_cache_from_zip(&src_zip, &src_dir, &candidates)
            });
            (jdk_key, materialized)
        }
        BinaryOrigin::Jar { label } => {
            let jar_key = archive::jar_cache_key(label);
            let mut candidates = vec![rel_java.clone()];
            if raw_rel_java != rel_java {
                candidates.push(raw_rel_java);
            }

            let src_dir = cache_root.join("jar-src").join(&jar_key);
            let materialized = archive::find_cached_source(&src_dir, &candidates).or_else(|| {
                let sources_jar = companions.get(label)?.sources_jar.as_deref()?;
                archive::extract_and_cache_from_zip(sources_jar, &src_dir, &candidates)
            });
            (jar_key, materialized)
        }
    };

    let (path, source) = match extracted {
        Some(pair) => pair,
        None => {
            let stub = synthesize_binary_stub(classpath, target)?;
            let rel_subpath = archive::safe_relative_subpath(&rel_java)?;
            let stub_path = cache_root.join("stubs").join(&stub_key).join(rel_subpath);
            if !archive::write_cached_text_if_changed(&stub_path, &stub) {
                return None;
            }
            (stub_path, stub)
        }
    };

    let range = locate_binary_symbol_in_source(&source, target);
    Some((path, source, range))
}

/// Looks up Javadoc Markdown for `target` from JDK `src.zip`, companion `*-sources.jar`,
/// or companion `*-javadoc.jar` HTML.
#[must_use]
pub fn lookup_binary_javadoc(
    jdk_home: Option<&Path>,
    companions: &JarCompanions,
    target: &BinaryTarget,
) -> Option<String> {
    let top_level_internal = archive::split_top_level_internal_name(&target.internal_name);
    let rel_java = format!("{top_level_internal}.java");
    let raw_rel_java = format!("{}.java", target.internal_name);

    match &target.origin {
        BinaryOrigin::Jdk { module_name } => {
            let src_zip = jdk::find_jdk_src_zip(jdk_home?)?;
            let mut candidates = Vec::new();
            if let Some(m) = module_name {
                candidates.push(format!("{m}/{rel_java}"));
                if raw_rel_java != rel_java {
                    candidates.push(format!("{m}/{raw_rel_java}"));
                }
            }
            candidates.push(rel_java);
            if let Some((_, source)) = archive::read_first_matching_zip_text(&src_zip, &candidates) {
                return extract_javadoc_from_source(&source, target);
            }
            None
        }
        BinaryOrigin::Jar { label } => {
            let companion = companions.get(label)?;
            if let Some(sources_jar) = &companion.sources_jar {
                let mut candidates = Vec::with_capacity(2);
                let push_raw = raw_rel_java != rel_java;
                candidates.push(rel_java);
                if push_raw {
                    candidates.push(raw_rel_java);
                }
                if let Some((_, source)) =
                    archive::read_first_matching_zip_text(sources_jar, &candidates)
                    && let Some(doc) = extract_javadoc_from_source(&source, target)
                {
                    return Some(doc);
                }
            }
            if let Some(javadoc_jar) = &companion.javadoc_jar {
                let html_rel = archive::html_entry_path_for_target(target);
                if let Some((_, html)) =
                    archive::read_first_matching_zip_text(javadoc_jar, &[html_rel])
                {
                    return extract_javadoc_from_html(&html, target);
                }
            }
            None
        }
    }
}
