//! Three-tier workspace configuration probing, manifest introspection, conflict detection,
//! external version reconciliation, JDK discovery, and minimal-overlay TOML generation.
//!
//! See `docs/specs/0004-lightweight-project-and-workspace-model.md`.

mod ant_ivy;
mod gradle;
mod jdk;
mod maven;
mod scan;
mod toml_config;
mod versions;
mod xml;

#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use ide::{
    FileId, LanguageLevel, ModuleDelta, ModuleDependency, Release, RootScope, WorkspaceDelta,
    WorkspaceLayout, infer_workspace_layout_from_sources, is_path_within, normalize_rel_path,
};

pub use jdk::{discover_jdks, is_valid_jdk_home, probe_jdk_release};
pub use toml_config::{find_config_file, parse_config_toml, render_minimal_toml};
pub use versions::{parse_version_string, reconcile_version_conflict};

use ant_ivy::{introspect_ant_build_xml, introspect_ivy_xml};
use gradle::introspect_gradle_build;
use maven::{introspect_maven_pom, parse_maven_pom_metadata};
use scan::scan_workspace_fs;
use versions::detect_version_specs;

/// Supported build tool / project specification kinds (Tier 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuildToolKind {
    None,
    Maven,
    Gradle,
    /// Apache Ant (`build.xml`) optionally paired with Apache Ivy (`ivy.xml` / `ivysettings.xml`).
    Ant,
    Bazel,
    Mill,
    Sbt,
    Elide,
    JBang,
    Custom,
}

/// A build manifest discovered in the workspace (Tier 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedBuildSpec {
    pub kind: BuildToolKind,
    pub manifest_path: PathBuf,
}

/// A Java release/version declaration discovered in a manifest or version pin file (Tier 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedVersionSpec {
    pub source: String,
    pub path: PathBuf,
    pub release: Release,
    pub preview: bool,
}

/// An ambiguity across multiple external Tier 2 project specifications that has not been
/// disambiguated by `.java-analyzer.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    BuildTool(Vec<DetectedBuildSpec>),
    Release(Vec<DetectedVersionSpec>),
}

impl Conflict {
    pub fn diagnostic_message(&self) -> String {
        match self {
            Conflict::BuildTool(specs) => {
                let mut parts: Vec<String> = specs
                    .iter()
                    .map(|s| {
                        let name = s
                            .manifest_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("manifest");
                        format!("{} ({})", name, build_tool_name(s.kind))
                    })
                    .collect();
                parts.sort();
                parts.dedup();
                format!(
                    "Conflicting build tool manifests detected: {}. Run `java-analyzer configure` to disambiguate.",
                    parts.join(", ")
                )
            }
            Conflict::Release(specs) => {
                let parts: Vec<String> = specs
                    .iter()
                    .map(|s| format!("{} (Java {})", s.source, s.release.number()))
                    .collect();
                format!(
                    "Conflicting Java release specifications detected: {}. Run `java-analyzer configure` to reconcile or disambiguate.",
                    parts.join(", ")
                )
            }
        }
    }
}

/// Explicit workspace configuration parsed from `.java-analyzer.toml` / `java-analyzer.toml` (Tier 1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExplicitConfig {
    pub build: Option<BuildToolKind>,
    pub jdk: Option<PathBuf>,
    pub release: Option<Release>,
    pub preview: Option<bool>,
    pub classpath_command: Vec<String>,
    pub delta: WorkspaceDelta,
}

impl ExplicitConfig {
    pub fn is_empty(&self) -> bool {
        self.build.is_none()
            && self.jdk.is_none()
            && self.release.is_none()
            && self.preview.is_none()
            && self.classpath_command.is_empty()
            && self.delta.include.is_empty()
            && self.delta.exclude.is_empty()
            && self.delta.modules.values().all(ModuleDelta::is_empty)
    }
}

/// A discovered local JDK installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JdkCandidate {
    pub home: PathBuf,
    pub release: Option<Release>,
    pub source: String,
}

/// Complete result of probing a workspace directory across Tiers 1, 2, and 3.
#[derive(Debug, Clone)]
pub struct WorkspaceProbe {
    pub workspace_dir: PathBuf,
    pub config_file: Option<PathBuf>,
    pub explicit_config: Option<ExplicitConfig>,
    pub build_specs: Vec<DetectedBuildSpec>,
    pub version_specs: Vec<DetectedVersionSpec>,
    /// Uniquely inferred build tool from Tier 2 (if exactly 1 distinct tool was found).
    pub inferred_build: Option<BuildToolKind>,
    /// Uniquely inferred language level from Tier 2 / Tier 3 (if unconflicted).
    pub inferred_level: Option<LanguageLevel>,
    pub conflicts: Vec<Conflict>,
    pub resolved_build: BuildToolKind,
    pub resolved_level: LanguageLevel,
    pub resolved_jdk: Option<PathBuf>,
    pub layout: WorkspaceLayout,
    /// Mapping from normalized workspace-relative file path to its assigned `FileId`.
    pub file_paths: BTreeMap<String, FileId>,
    /// Retained file contents from the workspace scan, keyed by normalized relative path.
    pub file_contents: BTreeMap<String, Arc<String>>,
}

pub fn build_tool_name(kind: BuildToolKind) -> &'static str {
    match kind {
        BuildToolKind::None => "none",
        BuildToolKind::Maven => "maven",
        BuildToolKind::Gradle => "gradle",
        BuildToolKind::Ant => "ant",
        BuildToolKind::Bazel => "bazel",
        BuildToolKind::Mill => "mill",
        BuildToolKind::Sbt => "sbt",
        BuildToolKind::Elide => "elide",
        BuildToolKind::JBang => "jbang",
        BuildToolKind::Custom => "custom",
    }
}

pub fn parse_build_tool_kind(s: &str) -> Option<BuildToolKind> {
    match s.trim().to_ascii_lowercase().as_str() {
        "none" | "unmanaged" | "zero-config" => Some(BuildToolKind::None),
        "maven" | "mvn" => Some(BuildToolKind::Maven),
        "gradle" => Some(BuildToolKind::Gradle),
        "ant" | "ivy" | "ant+ivy" => Some(BuildToolKind::Ant),
        "bazel" => Some(BuildToolKind::Bazel),
        "mill" => Some(BuildToolKind::Mill),
        "sbt" => Some(BuildToolKind::Sbt),
        "elide" => Some(BuildToolKind::Elide),
        "jbang" => Some(BuildToolKind::JBang),
        "custom" => Some(BuildToolKind::Custom),
        _ => None,
    }
}

pub use ide::parse_shebang_level;

/// Probes `workspace_dir` strictly read-only following the 3-tier precedence rules.
pub fn probe_workspace(workspace_dir: &Path) -> WorkspaceProbe {
    let config_file = find_config_file(workspace_dir);
    let explicit_config = config_file
        .as_ref()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|text| parse_config_toml(&text));

    let build_specs = detect_build_specs(workspace_dir);
    let mut version_specs = detect_version_specs(workspace_dir);
    let manifest_meta = introspect_manifests(workspace_dir, &build_specs, &mut version_specs);

    // Distinct build tool kinds in Tier 2 (note: build.xml and ivy.xml both have BuildToolKind::Ant)
    let mut distinct_build_kinds: Vec<BuildToolKind> = Vec::new();
    for spec in &build_specs {
        if !distinct_build_kinds.contains(&spec.kind) {
            distinct_build_kinds.push(spec.kind);
        }
    }

    let inferred_build = if distinct_build_kinds.len() == 1 {
        Some(distinct_build_kinds[0])
    } else if distinct_build_kinds.is_empty() {
        Some(BuildToolKind::None)
    } else {
        None
    };

    // Distinct releases in Tier 2
    let mut distinct_releases: Vec<Release> = Vec::new();
    let mut any_preview = false;
    for spec in &version_specs {
        if !distinct_releases.contains(&spec.release) {
            distinct_releases.push(spec.release);
        }
        if spec.preview {
            any_preview = true;
        }
    }

    let mut conflicts = Vec::new();
    let tier1_build = explicit_config.as_ref().and_then(|c| c.build);
    if distinct_build_kinds.len() > 1 && tier1_build.is_none() {
        conflicts.push(Conflict::BuildTool(build_specs.clone()));
    }

    let tier1_release = explicit_config.as_ref().and_then(|c| c.release);
    if distinct_releases.len() > 1 && tier1_release.is_none() {
        conflicts.push(Conflict::Release(version_specs.clone()));
    }

    let resolved_build = tier1_build
        .or(inferred_build)
        .or_else(|| distinct_build_kinds.first().copied())
        .unwrap_or(BuildToolKind::None);

    // Scan workspace files & resource indicators (Tier 3)
    let mut raw_java_files: Vec<(String, String)> = Vec::new();
    let mut detected_resource_dirs: BTreeSet<String> = BTreeSet::new();
    scan_workspace_fs(
        workspace_dir,
        &mut raw_java_files,
        &mut detected_resource_dirs,
    );
    raw_java_files.sort_by(|a, b| a.0.cmp(&b.0));

    // Check shebang language levels if no Tier 2 version spec
    let mut shebang_level: Option<LanguageLevel> = None;
    for (_, text) in &raw_java_files {
        if let Some(lvl) = parse_shebang_level(text) {
            shebang_level = Some(lvl);
            break;
        }
    }

    let inferred_level = if distinct_releases.len() == 1 {
        Some(LanguageLevel::new(distinct_releases[0], any_preview))
    } else if distinct_releases.is_empty() {
        shebang_level
    } else {
        None
    };

    let resolved_release = tier1_release
        .or_else(|| {
            if distinct_releases.len() == 1 {
                Some(distinct_releases[0])
            } else {
                distinct_releases.iter().copied().max()
            }
        })
        .or_else(|| shebang_level.map(|l| l.release))
        .unwrap_or(LanguageLevel::latest().release);

    let resolved_preview = explicit_config
        .as_ref()
        .and_then(|c| c.preview)
        .unwrap_or_else(|| any_preview || shebang_level.is_some_and(|l| l.preview));

    let resolved_level = LanguageLevel::new(resolved_release, resolved_preview);

    // Assign FileIds and collect source texts for layout inference at resolved_level
    let mut file_paths = BTreeMap::new();
    let mut source_files = Vec::with_capacity(raw_java_files.len());
    for (idx, (rel_path, text)) in raw_java_files.iter().enumerate() {
        let file_id = FileId(idx as u32);
        file_paths.insert(rel_path.clone(), file_id);
        source_files.push((file_id, rel_path.as_str(), text.as_str()));
    }

    for res_dir in &manifest_meta.extra_resource_dirs {
        detected_resource_dirs.insert(normalize_rel_path(res_dir));
    }
    let resource_dirs_owned: Vec<String> = detected_resource_dirs.into_iter().collect();
    let resource_dir_refs: Vec<&str> = resource_dirs_owned.iter().map(String::as_str).collect();

    // Combine Tier 2 manifest metadata with Tier 1 `.java-analyzer.toml` deltas (Tier 1 takes precedence)
    let mut combined_delta = explicit_config
        .as_ref()
        .map(|c| c.delta.clone())
        .unwrap_or_default();

    let root_mod_delta = combined_delta.modules.entry(".".to_string()).or_default();
    for (src_dir, _scope) in &manifest_meta.extra_source_dirs {
        let norm = normalize_rel_path(src_dir);
        if !root_mod_delta.sources.exclude.contains(&norm)
            && !root_mod_delta.sources.include.contains(&norm)
        {
            root_mod_delta.sources.include.push(norm);
        }
    }
    for res_dir in &manifest_meta.extra_resource_dirs {
        let norm = normalize_rel_path(res_dir);
        if !root_mod_delta.resources.exclude.contains(&norm)
            && !root_mod_delta.resources.include.contains(&norm)
        {
            root_mod_delta.resources.include.push(norm);
        }
    }

    let mut layout = infer_workspace_layout_from_sources(
        &source_files,
        &resource_dir_refs,
        resolved_level,
        &combined_delta,
    );

    let file_contents: BTreeMap<String, Arc<String>> = raw_java_files
        .into_iter()
        .map(|(rel_path, text)| (rel_path, Arc::new(text)))
        .collect();

    // Merge Tier 2 compile-time manifest dependencies into the root module (respecting Tier 1 excludes)
    // Only attach root manifest dependencies when a `"."` module exists or there is a single module.
    let all_modules_snapshot = layout.modules.clone();
    let root_idx = layout
        .modules
        .iter()
        .position(|m| m.rel_path == ".")
        .or_else(|| (layout.modules.len() == 1).then_some(0));
    if let Some(idx) = root_idx {
        let root_mod = &mut layout.modules[idx];
        let tier1_root_excludes: Vec<String> = explicit_config
            .as_ref()
            .and_then(|c| {
                c.delta.modules.get(&root_mod.rel_path).or_else(|| {
                    root_mod
                        .module_name
                        .as_deref()
                        .and_then(|n| c.delta.modules.get(n))
                })
            })
            .map(|m| m.dependencies.exclude.clone())
            .unwrap_or_default();

        for dep in manifest_meta.dependencies {
            let excluded = tier1_root_excludes
                .iter()
                .any(|f| dep.matches_filter(f, &all_modules_snapshot));
            if !excluded && !root_mod.dependencies.contains(&dep) {
                root_mod.dependencies.push(dep);
            }
        }

        if root_mod.rel_path == "."
            && let Some(test_rel) = manifest_meta.maven_test_release
        {
            let tier1_preview = explicit_config.as_ref().and_then(|c| {
                c.delta
                    .modules
                    .get(".")
                    .and_then(|m| m.preview)
                    .or(c.preview)
            });
            let preview = tier1_preview
                .unwrap_or(manifest_meta.maven_preview || root_mod.language_level.preview);
            root_mod.test_language_level = Some(LanguageLevel::new(test_rel, preview));
        }
    }

    // Introspect submodule `<mod.rel_path>/pom.xml` for non-root modules
    for module in &mut layout.modules {
        if module.rel_path == "." {
            continue;
        }
        let mod_delta = explicit_config.as_ref().and_then(|c| {
            c.delta.modules.get(&module.rel_path).or_else(|| {
                module
                    .module_name
                    .as_deref()
                    .and_then(|n| c.delta.modules.get(n))
            })
        });
        let tier1_mod_release = mod_delta.and_then(|d| d.release);
        let tier1_preview = mod_delta
            .and_then(|d| d.preview)
            .or_else(|| explicit_config.as_ref().and_then(|c| c.preview));

        let sub_pom_path = workspace_dir.join(&module.rel_path).join("pom.xml");
        if sub_pom_path.is_file() {
            if let Ok(content) = fs::read_to_string(&sub_pom_path) {
                let sub_pom = parse_maven_pom_metadata(
                    &content,
                    &manifest_meta.maven_properties,
                    &manifest_meta.maven_managed_versions,
                );

                if tier1_release.is_none() && tier1_mod_release.is_none() {
                    if let Some(sub_rel) = sub_pom.release {
                        let preview = tier1_preview
                            .unwrap_or(sub_pom.preview || module.language_level.preview);
                        module.language_level = LanguageLevel::new(sub_rel, preview);
                    } else if sub_pom.preview && tier1_preview.is_none() {
                        module.language_level.preview = true;
                    }
                }

                if let Some(test_rel) = sub_pom.test_release.or(manifest_meta.maven_test_release) {
                    let preview =
                        tier1_preview.unwrap_or(sub_pom.preview || module.language_level.preview);
                    module.test_language_level = Some(LanguageLevel::new(test_rel, preview));
                }

                let tier1_mod_excludes: Vec<String> = mod_delta
                    .map(|m| m.dependencies.exclude.clone())
                    .unwrap_or_default();
                for dep in sub_pom.dependencies {
                    let excluded = tier1_mod_excludes
                        .iter()
                        .any(|f| dep.matches_filter(f, &all_modules_snapshot));
                    if !excluded && !module.dependencies.contains(&dep) {
                        module.dependencies.push(dep);
                    }
                }
            }
        } else if let Some(test_rel) = manifest_meta.maven_test_release {
            let preview = tier1_preview
                .unwrap_or(manifest_meta.maven_preview || module.language_level.preview);
            module.test_language_level = Some(LanguageLevel::new(test_rel, preview));
        }
    }

    let resolved_jdk = explicit_config
        .as_ref()
        .and_then(|c| c.jdk.clone())
        .or_else(|| {
            discover_jdks(Some(resolved_release))
                .into_iter()
                .next()
                .map(|c| c.home)
        });

    WorkspaceProbe {
        workspace_dir: workspace_dir.to_path_buf(),
        config_file,
        explicit_config,
        build_specs,
        version_specs,
        inferred_build,
        inferred_level,
        conflicts,
        resolved_build,
        resolved_level,
        resolved_jdk,
        layout,
        file_paths,
        file_contents,
    }
}

fn detect_build_specs(workspace_dir: &Path) -> Vec<DetectedBuildSpec> {
    let candidates: &[(&str, BuildToolKind)] = &[
        ("pom.xml", BuildToolKind::Maven),
        ("build.gradle", BuildToolKind::Gradle),
        ("build.gradle.kts", BuildToolKind::Gradle),
        ("settings.gradle", BuildToolKind::Gradle),
        ("settings.gradle.kts", BuildToolKind::Gradle),
        // Ant and Apache Ivy pair together as BuildToolKind::Ant
        ("build.xml", BuildToolKind::Ant),
        ("ivy.xml", BuildToolKind::Ant),
        ("ivysettings.xml", BuildToolKind::Ant),
        ("MODULE.bazel", BuildToolKind::Bazel),
        ("WORKSPACE", BuildToolKind::Bazel),
        ("WORKSPACE.bazel", BuildToolKind::Bazel),
        ("BUILD.bazel", BuildToolKind::Bazel),
        ("build.mill", BuildToolKind::Mill),
        ("build.sc", BuildToolKind::Mill),
        ("build.sbt", BuildToolKind::Sbt),
        ("elide.pkl", BuildToolKind::Elide),
        ("elide.yaml", BuildToolKind::Elide),
        ("jbang.properties", BuildToolKind::JBang),
    ];

    let mut specs = Vec::new();
    for (file_name, kind) in candidates {
        let path = workspace_dir.join(file_name);
        if path.is_file() {
            specs.push(DetectedBuildSpec {
                kind: *kind,
                manifest_path: path,
            });
        }
    }
    specs
}

#[derive(Default)]
pub(super) struct ManifestMetadata {
    pub(super) extra_source_dirs: Vec<(String, RootScope)>,
    pub(super) extra_resource_dirs: Vec<String>,
    pub(super) dependencies: Vec<ModuleDependency>,
    pub(super) maven_properties: BTreeMap<String, String>,
    pub(super) maven_managed_versions: BTreeMap<(String, String), String>,
    pub(super) maven_test_release: Option<Release>,
    pub(super) maven_preview: bool,
}

fn introspect_manifests(
    workspace_dir: &Path,
    build_specs: &[DetectedBuildSpec],
    version_specs: &mut Vec<DetectedVersionSpec>,
) -> ManifestMetadata {
    let mut meta = ManifestMetadata::default();
    let mut ant_props: BTreeMap<String, String> = BTreeMap::new();
    let mut ivy_contents: Vec<String> = Vec::new();

    for spec in build_specs {
        let file_name = spec
            .manifest_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let Ok(content) = fs::read_to_string(&spec.manifest_path) else {
            continue;
        };

        match file_name {
            "pom.xml" => {
                introspect_maven_pom(&content, &spec.manifest_path, version_specs, &mut meta);
            }
            "build.gradle" | "build.gradle.kts" => {
                introspect_gradle_build(
                    &content,
                    file_name,
                    &spec.manifest_path,
                    version_specs,
                    &mut meta,
                );
            }
            "build.xml" => {
                let props = introspect_ant_build_xml(
                    workspace_dir,
                    &content,
                    &spec.manifest_path,
                    version_specs,
                    &mut meta,
                );
                for (k, v) in props {
                    ant_props.entry(k).or_insert(v);
                }
            }
            "ivy.xml" => {
                ivy_contents.push(content);
            }
            _ => {}
        }
    }

    // Process ivy.xml after build.xml so Ant properties (including <property file="..."/>) are shared.
    for ivy_content in ivy_contents {
        introspect_ivy_xml(&ivy_content, &ant_props, &mut meta);
    }

    meta
}

impl WorkspaceProbe {
    pub fn language_level_for_path(&self, file_path: &Path) -> LanguageLevel {
        let rel_path = file_path
            .strip_prefix(&self.workspace_dir)
            .ok()
            .or_else(|| (!file_path.is_absolute()).then_some(file_path));

        if let Some(rel) = rel_path {
            let norm = normalize_rel_path(&rel.to_string_lossy());

            if let Some((module, src)) = self.layout.enclosing_source_root(&norm) {
                if src.scope == RootScope::Test
                    && let Some(test_lvl) = module.test_language_level
                {
                    return test_lvl;
                }
                return module.language_level;
            }

            let best_module = self
                .layout
                .modules
                .iter()
                .filter(|module| is_path_within(&norm, &module.rel_path))
                .max_by_key(|module| {
                    if module.rel_path == "." {
                        0
                    } else {
                        module.rel_path.len()
                    }
                });

            if let Some(module) = best_module {
                if RootScope::from_path(&norm) == RootScope::Test
                    && let Some(test_lvl) = module.test_language_level
                {
                    return test_lvl;
                }
                return module.language_level;
            }
        }
        self.resolved_level
    }
}
