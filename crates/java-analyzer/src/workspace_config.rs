//! Three-tier workspace configuration probing, manifest introspection, conflict detection,
//! external version reconciliation, JDK discovery, and minimal-overlay TOML generation.
//!
//! See `docs/specs/0004-lightweight-project-and-workspace-model.md`.

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use hir::{
    extract_file_header, infer_workspace_layout, normalize_rel_path, DeltaSet, FileId,
    LanguageLevel, ModuleDelta, ModuleDependency, Release, RootScope, WorkspaceDelta,
    WorkspaceLayout,
};

/// Directories ignored when scanning a workspace for zero-config Java sources and `META-INF/`.
const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".jj",
    ".hg",
    ".svn",
    "target",
    "build",
    "out",
    "dist",
    "node_modules",
    ".gradle",
    ".idea",
    ".vscode",
    ".zed",
    ".bsp",
    ".bloop",
    ".metals",
    ".settings",
    "bin",
];

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

/// Extracts the language level from a `#!` shebang line if present.
pub fn parse_shebang_level(text: &str) -> Option<LanguageLevel> {
    let parse = syntax::parse_with_level(text, LanguageLevel::latest());
    extract_file_header(&parse.syntax_node()).shebang_level
}

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
        workspace_dir,
        false,
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

    // Parse scanned Java files at resolved_level
    let mut parsed_nodes = Vec::with_capacity(raw_java_files.len());
    let mut file_paths = BTreeMap::new();
    for (idx, (rel_path, text)) in raw_java_files.iter().enumerate() {
        let file_id = FileId(idx as u32);
        file_paths.insert(rel_path.clone(), file_id);
        let parse = syntax::parse_with_level(text, resolved_level);
        parsed_nodes.push((file_id, rel_path.as_str(), parse.syntax_node()));
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

    let file_refs: Vec<(FileId, &str, &syntax::SyntaxNode)> = parsed_nodes
        .iter()
        .map(|(id, p, node)| (*id, *p, node))
        .collect();

    let mut layout =
        infer_workspace_layout(&file_refs, &resource_dir_refs, resolved_level, &combined_delta);

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

        if root_mod.rel_path == "." {
            if let Some(test_rel) = manifest_meta.maven_test_release {
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
    }
}

pub fn find_config_file(workspace_dir: &Path) -> Option<PathBuf> {
    let dot_file = workspace_dir.join(".java-analyzer.toml");
    if dot_file.is_file() {
        return Some(dot_file);
    }
    let plain_file = workspace_dir.join("java-analyzer.toml");
    if plain_file.is_file() {
        return Some(plain_file);
    }
    None
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

fn detect_version_specs(workspace_dir: &Path) -> Vec<DetectedVersionSpec> {
    let mut specs = Vec::new();

    // 1. .java-version
    let java_version_path = workspace_dir.join(".java-version");
    if let Ok(content) = fs::read_to_string(&java_version_path) {
        if let Some(release) = parse_version_string(&content) {
            specs.push(DetectedVersionSpec {
                source: ".java-version".to_string(),
                path: java_version_path,
                release,
                preview: false,
            });
        }
    }

    // 2. .sdkmanrc
    let sdkman_path = workspace_dir.join(".sdkmanrc");
    if let Ok(content) = fs::read_to_string(&sdkman_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == "java" {
                    if let Some(release) = parse_version_string(v) {
                        specs.push(DetectedVersionSpec {
                            source: ".sdkmanrc".to_string(),
                            path: sdkman_path.clone(),
                            release,
                            preview: false,
                        });
                    }
                }
            }
        }
    }

    // 3. .tool-versions
    let tool_versions_path = workspace_dir.join(".tool-versions");
    if let Ok(content) = fs::read_to_string(&tool_versions_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            if parts.next() == Some("java") {
                if let Some(ver) = parts.next() {
                    if let Some(release) = parse_version_string(ver) {
                        specs.push(DetectedVersionSpec {
                            source: ".tool-versions".to_string(),
                            path: tool_versions_path.clone(),
                            release,
                            preview: false,
                        });
                    }
                }
            }
        }
    }

    // 4. mise.toml / .mise.toml
    for mise_name in ["mise.toml", ".mise.toml"] {
        let mise_path = workspace_dir.join(mise_name);
        if let Ok(content) = fs::read_to_string(&mise_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    if k.trim() == "java" {
                        if let Some(release) = parse_version_string(v) {
                            specs.push(DetectedVersionSpec {
                                source: mise_name.to_string(),
                                path: mise_path.clone(),
                                release,
                                preview: false,
                            });
                        }
                    }
                }
            }
        }
    }

    // 5. .bazelrc
    let bazelrc_path = workspace_dir.join(".bazelrc");
    if let Ok(content) = fs::read_to_string(&bazelrc_path) {
        for line in content.lines() {
            if let Some(idx) = line.find("--java_language_version=") {
                let rest = &line[idx + "--java_language_version=".len()..];
                let token = rest.split_whitespace().next().unwrap_or("");
                if let Some(release) = parse_version_string(token) {
                    specs.push(DetectedVersionSpec {
                        source: ".bazelrc".to_string(),
                        path: bazelrc_path.clone(),
                        release,
                        preview: false,
                    });
                    break;
                }
            }
        }
    }

    specs
}

fn release_from_u16(n: u16) -> Option<Release> {
    if (8..=27).contains(&n) {
        Some(Release::new(n))
    } else {
        None
    }
}

pub fn parse_version_string(raw: &str) -> Option<Release> {
    let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("1.") {
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let n: u16 = num_str.parse().ok()?;
        return release_from_u16(n);
    }
    // Skip vendor prefix like `temurin-21.0.2` or `openjdk-17`
    let digit_start = trimmed.find(|c: char| c.is_ascii_digit())?;
    let after = &trimmed[digit_start..];
    if let Some(rest) = after.strip_prefix("1.") {
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let n: u16 = num_str.parse().ok()?;
        return release_from_u16(n);
    }
    let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: u16 = num_str.parse().ok()?;
    release_from_u16(n)
}

#[derive(Default)]
struct ManifestMetadata {
    extra_source_dirs: Vec<(String, RootScope)>,
    extra_resource_dirs: Vec<String>,
    dependencies: Vec<ModuleDependency>,
    maven_properties: BTreeMap<String, String>,
    maven_managed_versions: BTreeMap<(String, String), String>,
    maven_test_release: Option<Release>,
    maven_preview: bool,
}

#[derive(Default)]
struct MavenPomMetadata {
    release: Option<Release>,
    test_release: Option<Release>,
    preview: bool,
    extra_source_dirs: Vec<(String, RootScope)>,
    dependencies: Vec<ModuleDependency>,
    properties: BTreeMap<String, String>,
    managed_versions: BTreeMap<(String, String), String>,
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

fn introspect_maven_pom(
    content: &str,
    path: &Path,
    version_specs: &mut Vec<DetectedVersionSpec>,
    meta: &mut ManifestMetadata,
) {
    let empty_props = BTreeMap::new();
    let empty_managed = BTreeMap::new();
    let pom = parse_maven_pom_metadata(content, &empty_props, &empty_managed);

    if let Some(release) = pom.release {
        version_specs.push(DetectedVersionSpec {
            source: "pom.xml".to_string(),
            path: path.to_path_buf(),
            release,
            preview: pom.preview,
        });
    }

    meta.extra_source_dirs.extend(pom.extra_source_dirs);
    for dep in pom.dependencies {
        if !meta.dependencies.contains(&dep) {
            meta.dependencies.push(dep);
        }
    }
    meta.maven_properties = pom.properties;
    meta.maven_managed_versions = pom.managed_versions;
    meta.maven_test_release = pom.test_release;
    meta.maven_preview = pom.preview;
}

fn parse_maven_pom_metadata(
    content: &str,
    inherited_props: &BTreeMap<String, String>,
    inherited_managed: &BTreeMap<(String, String), String>,
) -> MavenPomMetadata {
    let cleaned = strip_xml_comments(content);
    let mut props = inherited_props.clone();
    props.extend(parse_maven_properties(&cleaned));

    let (without_profiles, profile_blocks) = remove_xml_blocks(&cleaned, "profiles");
    let preview = without_profiles.contains("<enablePreview>true</enablePreview>")
        || without_profiles.contains("--enable-preview");

    let mut release = None;
    for tag in [
        "maven.compiler.release",
        "maven.compiler.source",
        "java.version",
        "release",
    ] {
        if let Some(raw_val) = extract_xml_tag_text(&without_profiles, tag) {
            let val = expand_props(&raw_val, &props);
            if let Some(rel) = parse_version_string(&val) {
                release = Some(rel);
                break;
            }
        }
    }

    let mut test_release = None;
    for tag in [
        "maven.compiler.testRelease",
        "maven.compiler.testSource",
        "testRelease",
    ] {
        if let Some(raw_val) = extract_xml_tag_text(&without_profiles, tag) {
            let val = expand_props(&raw_val, &props);
            if let Some(rel) = parse_version_string(&val) {
                test_release = Some(rel);
                break;
            }
        }
    }
    if test_release.is_none() {
        test_release = extract_test_release_from_executions(&without_profiles, &props);
    }
    if test_release.is_none() {
        for profile_block in &profile_blocks {
            if let Some(rel) = extract_test_release_from_executions(profile_block, &props) {
                test_release = Some(test_release.map_or(rel, |cur| cur.max(rel)));
            }
        }
    }

    // Extract <sourceDirectory> and <testSourceDirectory> from <build> BEFORE stripping <build>.
    let mut extra_source_dirs = Vec::new();
    if let Some(raw_src_dir) = extract_xml_tag_text(&without_profiles, "sourceDirectory") {
        let src_dir = expand_props(&raw_src_dir, &props);
        if !src_dir.contains("${") {
            extra_source_dirs.push((normalize_rel_path(&src_dir), RootScope::Main));
        }
    }
    if let Some(raw_test_dir) = extract_xml_tag_text(&without_profiles, "testSourceDirectory") {
        let test_dir = expand_props(&raw_test_dir, &props);
        if !test_dir.contains("${") {
            extra_source_dirs.push((normalize_rel_path(&test_dir), RootScope::Test));
        }
    }

    // Strip <build>, <reporting>, <plugins>, and <pluginManagement> before scanning <dependency>
    // blocks so plugin dependencies never leak into project compile/test dependencies.
    let mut deps_xml = without_profiles;
    for container in ["build", "reporting", "plugins", "pluginManagement"] {
        deps_xml = remove_xml_blocks(&deps_xml, container).0;
    }

    // Parse <dependencyManagement> separately so managed-only entries are not emitted as direct
    // dependencies and can provide version fallbacks when direct <dependency> entries omit <version>.
    let (without_dep_mgmt, dep_mgmt_blocks) = remove_xml_blocks(&deps_xml, "dependencyManagement");
    let mut own_managed: BTreeMap<(String, String), String> = BTreeMap::new();
    for mgmt_block in &dep_mgmt_blocks {
        let mut rest = mgmt_block.as_str();
        while let Some(start) = rest.find("<dependency>") {
            let after = &rest[start + "<dependency>".len()..];
            let Some(end) = after.find("</dependency>") else {
                break;
            };
            let block = &after[..end];
            rest = &after[end + "</dependency>".len()..];
            let (block_clean, _) = remove_xml_blocks(block, "exclusions");

            let group = extract_xml_tag_text(&block_clean, "groupId")
                .map(|s| expand_props(&s, &props).trim().to_string())
                .filter(|s| !s.is_empty());
            let artifact = extract_xml_tag_text(&block_clean, "artifactId")
                .map(|s| expand_props(&s, &props).trim().to_string())
                .filter(|s| !s.is_empty());
            let version = extract_xml_tag_text(&block_clean, "version")
                .map(|s| expand_props(&s, &props).trim().to_string())
                .filter(|s| !s.is_empty());

            if let (Some(g), Some(a), Some(v)) = (group, artifact, version) {
                own_managed.entry((g, a)).or_insert(v);
            }
        }
    }
    let mut managed_versions = inherited_managed.clone();
    managed_versions.extend(own_managed);

    // Extract compile-time <dependency> blocks (Main: default/compile/provided; Test: test; skip runtime)
    let mut dependencies = Vec::new();
    let mut rest = without_dep_mgmt.as_str();
    while let Some(start) = rest.find("<dependency>") {
        let after = &rest[start + "<dependency>".len()..];
        let Some(end) = after.find("</dependency>") else {
            break;
        };
        let block = &after[..end];
        rest = &after[end + "</dependency>".len()..];
        let (block_clean, _) = remove_xml_blocks(block, "exclusions");

        let group = extract_xml_tag_text(&block_clean, "groupId")
            .map(|s| expand_props(&s, &props).trim().to_string())
            .filter(|s| !s.is_empty());
        let artifact = extract_xml_tag_text(&block_clean, "artifactId")
            .map(|s| expand_props(&s, &props).trim().to_string())
            .filter(|s| !s.is_empty());
        let explicit_version = extract_xml_tag_text(&block_clean, "version")
            .map(|s| expand_props(&s, &props).trim().to_string())
            .filter(|s| !s.is_empty());
        let classifier = extract_xml_tag_text(&block_clean, "classifier")
            .map(|s| expand_props(&s, &props).trim().to_string())
            .filter(|s| !s.is_empty());
        let scope_str = extract_xml_tag_text(&block_clean, "scope").unwrap_or_default();

        if let (Some(g), Some(a)) = (group, artifact) {
            let version = explicit_version
                .or_else(|| managed_versions.get(&(g.clone(), a.clone())).cloned())
                .unwrap_or_else(|| "*".to_string());
            let scope = match scope_str.trim() {
                "" | "compile" | "provided" => Some(RootScope::Main),
                "test" => Some(RootScope::Test),
                // Ignore runtime/import/system deployment-only scopes
                _ => None,
            };
            if let Some(scope) = scope {
                let dep = ModuleDependency::Maven {
                    group: g,
                    artifact: a,
                    version,
                    classifier,
                    scope,
                };
                if !dependencies.contains(&dep) {
                    dependencies.push(dep);
                }
            }
        }
    }

    MavenPomMetadata {
        release,
        test_release,
        preview,
        extra_source_dirs,
        dependencies,
        properties: props,
        managed_versions,
    }
}

fn extract_test_release_from_executions(
    xml: &str,
    props: &BTreeMap<String, String>,
) -> Option<Release> {
    let mut max_release: Option<Release> = None;
    let (_, exec_blocks) = remove_xml_blocks(xml, "execution");
    for block in exec_blocks {
        let lower = block.to_ascii_lowercase();
        if !lower.contains("testcompile") && !lower.contains("test-compile") {
            continue;
        }
        for tag in ["testRelease", "release"] {
            if let Some(raw_val) = extract_xml_tag_text(&block, tag) {
                let val = expand_props(&raw_val, props);
                if let Some(rel) = parse_version_string(&val) {
                    max_release = Some(max_release.map_or(rel, |cur| cur.max(rel)));
                    break;
                }
            }
        }
    }
    max_release
}

fn strip_xml_comments(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "<!--".len()..];
        if let Some(end) = after.find("-->") {
            rest = &after[end + "-->".len()..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

fn remove_xml_blocks(xml: &str, tag: &str) -> (String, Vec<String>) {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut kept = String::with_capacity(xml.len());
    let mut removed = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        kept.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        if let Some(end) = after_open.find(&close) {
            removed.push(after_open[..end].to_string());
            rest = &after_open[end + close.len()..];
        } else {
            kept.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    kept.push_str(rest);
    (kept, removed)
}

fn extract_maven_project_coords(content: &str) -> (Option<String>, Option<String>, Option<String>) {
    let (without_parent, parent_blocks) = remove_xml_blocks(content, "parent");
    let mut top_only = without_parent;
    for container in [
        "dependencies",
        "dependencyManagement",
        "build",
        "profiles",
        "reporting",
        "properties",
    ] {
        top_only = remove_xml_blocks(&top_only, container).0;
    }
    let group_id = extract_xml_tag_text(&top_only, "groupId")
        .or_else(|| {
            parent_blocks
                .first()
                .and_then(|p| extract_xml_tag_text(p, "groupId"))
        })
        .filter(|v| !v.is_empty());
    let artifact_id = extract_xml_tag_text(&top_only, "artifactId").filter(|v| !v.is_empty());
    let version = extract_xml_tag_text(&top_only, "version")
        .or_else(|| {
            parent_blocks
                .first()
                .and_then(|p| extract_xml_tag_text(p, "version"))
        })
        .filter(|v| !v.is_empty());
    (group_id, artifact_id, version)
}

fn parse_maven_properties(content: &str) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    let (proj_group, proj_artifact, proj_ver) = extract_maven_project_coords(content);
    if let Some(g) = proj_group {
        props.insert("project.groupId".to_string(), g);
    }
    if let Some(a) = proj_artifact {
        props.insert("project.artifactId".to_string(), a);
    }
    if let Some(v) = proj_ver {
        props.insert("project.version".to_string(), v);
    }

    let (without_profiles, _) = remove_xml_blocks(content, "profiles");
    if let Some(parent_block) = extract_xml_tag_text(&without_profiles, "parent") {
        for (xml_tag, prop_key) in [
            ("groupId", "project.parent.groupId"),
            ("artifactId", "project.parent.artifactId"),
            ("version", "project.parent.version"),
        ] {
            if let Some(val) = extract_xml_tag_text(&parent_block, xml_tag).filter(|v| !v.is_empty())
            {
                props.insert(prop_key.to_string(), val);
            }
        }
    }
    let props_source = if extract_xml_tag_text(&without_profiles, "properties").is_some() {
        without_profiles.as_str()
    } else {
        content
    };
    if let Some(props_block) = extract_xml_tag_text(props_source, "properties") {
        let mut rest = props_block.as_str();
        while let Some(open_lt) = rest.find('<') {
            let after_lt = &rest[open_lt + 1..];
            if after_lt.starts_with('/') || after_lt.starts_with('!') || after_lt.starts_with('?')
            {
                if let Some(gt) = after_lt.find('>') {
                    rest = &after_lt[gt + 1..];
                } else {
                    break;
                }
                continue;
            }
            let Some(gt) = after_lt.find('>') else {
                break;
            };
            let tag_header = &after_lt[..gt];
            if tag_header.ends_with('/') {
                let tag_name = tag_header
                    .trim_end_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                if !tag_name.is_empty() {
                    props.insert(tag_name.to_string(), String::new());
                }
                rest = &after_lt[gt + 1..];
                continue;
            }
            let tag_name = tag_header.split_whitespace().next().unwrap_or("");
            if tag_name.is_empty() {
                rest = &after_lt[gt + 1..];
                continue;
            }
            let close_tag = format!("</{tag_name}>");
            let after_open = &after_lt[gt + 1..];
            if let Some(close_idx) = after_open.find(&close_tag) {
                let val = after_open[..close_idx].trim().to_string();
                props.insert(tag_name.to_string(), val);
                rest = &after_open[close_idx + close_tag.len()..];
            } else {
                rest = after_open;
            }
        }
    }

    props
}

fn introspect_gradle_build(
    content: &str,
    source_name: &str,
    path: &Path,
    version_specs: &mut Vec<DetectedVersionSpec>,
    meta: &mut ManifestMetadata,
) {
    let preview = content.contains("--enable-preview");
    let mut found_release = None;

    if let Some(idx) = content.find("JavaLanguageVersion.of(") {
        let after = &content[idx + "JavaLanguageVersion.of(".len()..];
        let arg: String = after.chars().take_while(|c| *c != ')').collect();
        found_release = parse_version_string(&arg);
    }
    if found_release.is_none() {
        for key in ["sourceCompatibility", "targetCompatibility"] {
            if let Some(idx) = content.find(key) {
                let line = content[idx..].lines().next().unwrap_or("");
                if let Some(ver_idx) = line.find("VERSION_") {
                    let after = &line[ver_idx + "VERSION_".len()..];
                    let ver = after.replace('_', ".");
                    found_release = parse_version_string(&ver);
                } else if let Some((_, rhs)) = line.split_once('=') {
                    found_release = parse_version_string(rhs);
                }
                if found_release.is_some() {
                    break;
                }
            }
        }
    }

    if let Some(release) = found_release {
        version_specs.push(DetectedVersionSpec {
            source: source_name.to_string(),
            path: path.to_path_buf(),
            release,
            preview,
        });
    }

    // Extract compile-time dependencies (implementation, api, compileOnly, testImplementation, testCompileOnly)
    for line in content.lines() {
        let trimmed = line.trim();
        let (scope, after_conf) = if let Some(r) =
            strip_gradle_conf(trimmed, &["implementation", "api", "compileOnly", "compile"])
        {
            (RootScope::Main, r)
        } else if let Some(r) = strip_gradle_conf(
            trimmed,
            &["testImplementation", "testCompileOnly", "testCompile"],
        ) {
            (RootScope::Test, r)
        } else {
            continue;
        };

        if let Some(proj_idx) = after_conf.find("project(") {
            let after_proj = &after_conf[proj_idx + "project(".len()..];
            if let Some(quoted) = extract_first_quoted(after_proj) {
                let target = quoted.trim_start_matches(':').replace(':', "/");
                if !target.is_empty() {
                    let dep = ModuleDependency::parse(&target, scope, &[]);
                    if !meta.dependencies.contains(&dep) {
                        meta.dependencies.push(dep);
                    }
                }
            }
        } else if let Some(coords) = extract_first_quoted(after_conf) {
            let parts: Vec<&str> = coords.split(':').collect();
            if parts.len() >= 3 {
                let dep = ModuleDependency::Maven {
                    group: parts[0].to_string(),
                    artifact: parts[1].to_string(),
                    version: parts[2].to_string(),
                    classifier: parts.get(3).map(|s| (*s).to_string()),
                    scope,
                };
                if !meta.dependencies.contains(&dep) {
                    meta.dependencies.push(dep);
                }
            }
        }
    }
}

fn strip_gradle_conf<'a>(line: &'a str, confs: &[&str]) -> Option<&'a str> {
    for conf in confs {
        if let Some(rest) = line.strip_prefix(conf) {
            let first = rest.chars().next()?;
            if first == '(' || first.is_whitespace() || first == '"' || first == '\'' {
                return Some(rest);
            }
        }
    }
    None
}

fn extract_first_quoted(s: &str) -> Option<String> {
    for quote in ['"', '\''] {
        if let Some(start) = s.find(quote) {
            let after = &s[start + 1..];
            if let Some(end) = after.find(quote) {
                return Some(after[..end].to_string());
            }
        }
    }
    None
}

fn load_ant_properties_file(content: &str, props: &mut BTreeMap<String, String>) {
    for raw_line in content.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }
        let Some(sep_idx) = trimmed.find(['=', ':']) else {
            continue;
        };
        let key = trimmed[..sep_idx].trim();
        let val = trimmed[sep_idx + 1..].trim();
        if !key.is_empty() && !props.contains_key(key) {
            props.insert(key.to_string(), val.to_string());
        }
    }
}

fn introspect_ant_build_xml(
    workspace_dir: &Path,
    content: &str,
    path: &Path,
    version_specs: &mut Vec<DetectedVersionSpec>,
    meta: &mut ManifestMetadata,
) -> BTreeMap<String, String> {
    let cleaned = strip_xml_comments(content);

    // 1. Collect <property name="..." value="..." / location="..."/> and <property file="..."/>
    let mut props: BTreeMap<String, String> = BTreeMap::new();
    props.insert("basedir".to_string(), ".".to_string());
    for tag in find_xml_start_tags(&cleaned, "property") {
        if let Some(name) = extract_xml_attr(&tag, "name") {
            if let Some(val) =
                extract_xml_attr(&tag, "value").or_else(|| extract_xml_attr(&tag, "location"))
            {
                if !props.contains_key(&name) {
                    let resolved = expand_props(&val, &props);
                    props.insert(name, resolved);
                }
            }
        } else if let Some(file_attr) = extract_xml_attr(&tag, "file") {
            let expanded_path = expand_props(&file_attr, &props);
            if !expanded_path.contains("${") {
                let prop_path = workspace_dir.join(expanded_path.trim());
                if let Ok(prop_text) = fs::read_to_string(&prop_path) {
                    load_ant_properties_file(&prop_text, &mut props);
                }
            }
        }
    }

    // 2. Inspect <javac ...> tags for srcdir, release/source/target, classpath
    let mut found_release = None;
    for javac_tag in find_xml_start_tags(&cleaned, "javac") {
        for attr in ["release", "source", "target"] {
            if let Some(raw_ver) = extract_xml_attr(&javac_tag, attr) {
                let expanded = expand_props(&raw_ver, &props);
                if let Some(rel) = parse_version_string(&expanded) {
                    found_release = Some(rel);
                    break;
                }
            }
        }
        if let Some(srcdir) = extract_xml_attr(&javac_tag, "srcdir") {
            let expanded = expand_props(&srcdir, &props);
            for part in expanded.split([':', ';']) {
                let norm = normalize_rel_path(part.trim());
                if norm != "." && !norm.contains("${") && !norm.contains("@{") {
                    let scope = RootScope::from_path(&norm);
                    if !meta.extra_source_dirs.iter().any(|(p, _)| p == &norm) {
                        meta.extra_source_dirs.push((norm, scope));
                    }
                }
            }
        }
        if let Some(cp) = extract_xml_attr(&javac_tag, "classpath") {
            let expanded = expand_props(&cp, &props);
            for part in expanded.split([':', ';']) {
                let norm = normalize_rel_path(part.trim());
                if norm != "." && !norm.contains("${") && !norm.contains("@{") {
                    let dep = ModuleDependency::Path {
                        path: norm,
                        scope: RootScope::Main,
                    };
                    if !meta.dependencies.contains(&dep) {
                        meta.dependencies.push(dep);
                    }
                }
            }
        }
    }

    // Also inspect <src path="..."/>
    for src_tag in find_xml_start_tags(&cleaned, "src") {
        if let Some(p) =
            extract_xml_attr(&src_tag, "path").or_else(|| extract_xml_attr(&src_tag, "location"))
        {
            let expanded = expand_props(&p, &props);
            let norm = normalize_rel_path(&expanded);
            if norm != "." && !norm.contains("${") && !norm.contains("@{") {
                let scope = RootScope::from_path(&norm);
                if !meta.extra_source_dirs.iter().any(|(d, _)| d == &norm) {
                    meta.extra_source_dirs.push((norm, scope));
                }
            }
        }
    }

    // Inspect <fileset dir="..."/> inside <path> and <classpath> blocks for compile-time JARs
    let mut cp_blocks = extract_xml_blocks_with_attrs(&cleaned, "path");
    cp_blocks.extend(extract_xml_blocks_with_attrs(&cleaned, "classpath"));
    for block in cp_blocks {
        for fs_tag in find_xml_start_tags(&block, "fileset") {
            if let Some(dir) = extract_xml_attr(&fs_tag, "dir") {
                let expanded = expand_props(&dir, &props);
                let norm = normalize_rel_path(&expanded);
                if norm != "." && !norm.contains("${") && !norm.contains("@{") {
                    let full_dir = workspace_dir.join(&norm);
                    let mut added_jars = false;
                    if let Ok(entries) = fs::read_dir(&full_dir) {
                        let mut jar_paths: Vec<String> = entries
                            .flatten()
                            .filter_map(|e| {
                                let p = e.path();
                                if p.extension().and_then(|x| x.to_str()) == Some("jar") {
                                    let name = p.file_name()?.to_str()?;
                                    Some(normalize_rel_path(&format!("{norm}/{name}")))
                                } else {
                                    None
                                }
                            })
                            .collect();
                        jar_paths.sort();
                        for jp in jar_paths {
                            added_jars = true;
                            let dep = ModuleDependency::Path {
                                path: jp,
                                scope: RootScope::Main,
                            };
                            if !meta.dependencies.contains(&dep) {
                                meta.dependencies.push(dep);
                            }
                        }
                    }
                    if !added_jars {
                        let dep = ModuleDependency::Path {
                            path: norm,
                            scope: RootScope::Main,
                        };
                        if !meta.dependencies.contains(&dep) {
                            meta.dependencies.push(dep);
                        }
                    }
                }
            }
        }
    }

    if let Some(release) = found_release {
        version_specs.push(DetectedVersionSpec {
            source: "build.xml".to_string(),
            path: path.to_path_buf(),
            release,
            preview: cleaned.contains("--enable-preview"),
        });
    }

    props
}

fn introspect_ivy_xml(
    content: &str,
    ant_props: &BTreeMap<String, String>,
    meta: &mut ManifestMetadata,
) {
    let cleaned = strip_xml_comments(content);
    for dep_tag in find_xml_start_tags(&cleaned, "dependency") {
        let org = extract_xml_attr(&dep_tag, "org")
            .map(|s| expand_props(&s, ant_props).trim().to_string())
            .filter(|s| !s.is_empty());
        let name = extract_xml_attr(&dep_tag, "name")
            .map(|s| expand_props(&s, ant_props).trim().to_string())
            .filter(|s| !s.is_empty());
        let rev = extract_xml_attr(&dep_tag, "rev")
            .map(|s| expand_props(&s, ant_props).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "*".to_string());
        let conf = extract_xml_attr(&dep_tag, "conf").unwrap_or_default();

        if let (Some(org), Some(name)) = (org, name) {
            if let Some(scope) = classify_ivy_conf(&conf) {
                let dep = ModuleDependency::Maven {
                    group: org,
                    artifact: name,
                    version: rev,
                    classifier: None,
                    scope,
                };
                if !meta.dependencies.contains(&dep) {
                    meta.dependencies.push(dep);
                }
            }
        }
    }
}

/// Classifies an Apache Ivy `conf` attribute into a compile-time `RootScope`, or `None` if it is
/// strictly a runtime/deployment configuration (`runtime`, `deploy`, `dist`, `war`, `ear`).
fn classify_ivy_conf(conf: &str) -> Option<RootScope> {
    let trimmed = conf.trim();
    if trimmed.is_empty() {
        return Some(RootScope::Main);
    }
    let lower = trimmed.to_ascii_lowercase();
    let lhs_parts: Vec<&str> = lower
        .split(';')
        .map(|mapping| mapping.split("->").next().unwrap_or(mapping).trim())
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .collect();

    let has_compile = lhs_parts.iter().any(|c| {
        matches!(
            *c,
            "*" | "compile" | "default" | "provided" | "build" | "main" | "api"
        )
    });
    if has_compile {
        return Some(RootScope::Main);
    }
    let has_test = lhs_parts
        .iter()
        .any(|c| *c == "test" || c.starts_with("test-"));
    if has_test {
        return Some(RootScope::Test);
    }
    // Otherwise it's runtime / deploy / packaging only — out of scope for LSP compile-time resolution
    None
}

fn expand_props(input: &str, props: &BTreeMap<String, String>) -> String {
    let mut result = input.to_string();
    for _ in 0..4 {
        let mut changed = false;
        for (k, v) in props {
            let needle = format!("${{{k}}}");
            if result.contains(&needle) {
                result = result.replace(&needle, v);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    result
}

fn extract_xml_tag_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    Some(xml[start..start + end].trim().to_string())
}

fn find_xml_tag_end(slice: &str) -> Option<usize> {
    let mut in_quote: Option<char> = None;
    for (offset, ch) in slice.char_indices() {
        match in_quote {
            Some(q) if ch == q => in_quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => in_quote = Some(ch),
            None if ch == '>' => return Some(offset),
            None => {}
        }
    }
    None
}

fn find_xml_start_tags(xml: &str, tag: &str) -> Vec<String> {
    let mut results = Vec::new();
    let needle = format!("<{tag}");
    let mut rest = xml;
    while let Some(idx) = rest.find(&needle) {
        let after_name = &rest[idx + needle.len()..];
        let next_ch = after_name.chars().next().unwrap_or(' ');
        if next_ch.is_whitespace() || next_ch == '>' || next_ch == '/' {
            let slice = &rest[idx..];
            if let Some(end) = find_xml_tag_end(slice) {
                results.push(slice[..=end].to_string());
                rest = &slice[end + 1..];
                continue;
            }
        }
        rest = &rest[idx + needle.len()..];
    }
    results
}

fn extract_xml_blocks_with_attrs(xml: &str, tag: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let open_prefix = format!("<{tag}");
    let close_tag = format!("</{tag}>");
    let mut rest = xml;

    while let Some(idx) = rest.find(&open_prefix) {
        let after_name = &rest[idx + open_prefix.len()..];
        let next_ch = after_name.chars().next().unwrap_or(' ');
        if !(next_ch.is_whitespace() || next_ch == '>' || next_ch == '/') {
            rest = after_name;
            continue;
        }
        let slice = &rest[idx..];
        let Some(tag_end) = find_xml_tag_end(slice) else {
            break;
        };
        let is_self_closing = slice[..tag_end].trim_end().ends_with('/');
        let after_open = &slice[tag_end + 1..];
        if is_self_closing {
            rest = after_open;
            continue;
        }

        let mut depth = 1usize;
        let mut cursor = after_open;
        let mut block_end = None;
        while depth > 0 {
            let next_open = cursor.find(&open_prefix);
            let next_close = cursor.find(&close_tag);
            match (next_open, next_close) {
                (_, None) => break,
                (Some(o), Some(c)) if o < c => {
                    let after_nested = &cursor[o + open_prefix.len()..];
                    let nested_ch = after_nested.chars().next().unwrap_or(' ');
                    if nested_ch.is_whitespace() || nested_ch == '>' || nested_ch == '/' {
                        let nested_slice = &cursor[o..];
                        let Some(nested_end) = find_xml_tag_end(nested_slice) else {
                            break;
                        };
                        let nested_self_closing =
                            nested_slice[..nested_end].trim_end().ends_with('/');
                        if !nested_self_closing {
                            depth += 1;
                        }
                        cursor = &nested_slice[nested_end + 1..];
                    } else {
                        cursor = after_nested;
                    }
                }
                (_, Some(c)) => {
                    depth -= 1;
                    if depth == 0 {
                        let matched_len = after_open.len() - cursor.len() + c;
                        block_end = Some(matched_len);
                    }
                    cursor = &cursor[c + close_tag.len()..];
                }
            }
        }

        if let Some(end) = block_end {
            blocks.push(after_open[..end].to_string());
            rest = &after_open[end + close_tag.len()..];
        } else {
            rest = after_open;
        }
    }

    blocks
}

fn extract_xml_attr(tag_text: &str, attr: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pat = format!("{attr}={quote}");
        if let Some(start) = tag_text.find(&pat) {
            let after = &tag_text[start + pat.len()..];
            if let Some(end) = after.find(quote) {
                return Some(after[..end].to_string());
            }
        }
    }
    None
}

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

fn scan_workspace_fs(
    root: &Path,
    current: &Path,
    in_resource_root: bool,
    java_files: &mut Vec<(String, String)>,
    resource_dirs: &mut BTreeSet<String>,
) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    let mut sorted_entries: Vec<_> = entries.flatten().collect();
    sorted_entries.sort_by_key(|e| e.file_name());

    for entry in sorted_entries {
        let path = entry.path();
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };

        if path.is_dir() {
            if IGNORED_DIRS.contains(&name_str) {
                continue;
            }
            let rel_norm = path
                .strip_prefix(root)
                .ok()
                .map(|p| normalize_rel_path(&p.to_string_lossy()));

            // Skip Maven site directory (`src/site` or `.../src/site`)
            if rel_norm
                .as_deref()
                .is_some_and(|r| r == "src/site" || r.ends_with("/src/site"))
            {
                continue;
            }

            // Strong resource root indicator directly under `current` (handles workspace root `META-INF/`)
            if !in_resource_root
                && name_str == "META-INF"
                && (path.join("MANIFEST.MF").is_file() || path.join("services").is_dir())
            {
                if let Ok(rel_parent) = current.strip_prefix(root) {
                    let norm = normalize_rel_path(&rel_parent.to_string_lossy());
                    resource_dirs.insert(norm);
                }
            }

            let is_conv_res = name_str == "resources"
                && rel_norm
                    .as_deref()
                    .is_some_and(|r| is_conventional_resource_dir(r, current));
            let is_noncompilable_res = rel_norm
                .as_deref()
                .is_some_and(is_noncompilable_resource_dir);
            let is_meta_inf_res =
                !in_resource_root && !is_noncompilable_res && is_meta_inf_resource_root(&path);

            if is_conv_res || is_meta_inf_res {
                if let Some(norm) = rel_norm {
                    resource_dirs.insert(norm);
                }
            }

            // Conventional resource dirs and `src/*/resources-*` skip `.java` fixtures; directories
            // that merely contain `META-INF/` (e.g. Tomcat's `test/` or workspace root) still scan `.java`.
            let child_in_resource = in_resource_root || is_conv_res || is_noncompilable_res;
            scan_workspace_fs(root, &path, child_in_resource, java_files, resource_dirs);
        } else if !in_resource_root && path.is_file() && name_str.ends_with(".java") {
            if let Ok(rel) = path.strip_prefix(root) {
                if let Ok(text) = fs::read_to_string(&path) {
                    let norm = normalize_rel_path(&rel.to_string_lossy());
                    java_files.push((norm, text));
                }
            }
        }
    }
}

// ---- External Spec Reconciliation ----

/// Updates conflicting external version specification files in `workspace_dir` (`.java-version`,
/// `.sdkmanrc`, `.tool-versions`, `pom.xml`) to `target_release`, returning the paths modified.
pub fn reconcile_version_conflict(workspace_dir: &Path, target_release: Release) -> Vec<PathBuf> {
    let mut modified = Vec::new();
    let num = target_release.number();

    // 1. .java-version
    let java_ver = workspace_dir.join(".java-version");
    if java_ver.is_file() {
        if fs::write(&java_ver, format!("{num}\n")).is_ok() {
            modified.push(java_ver);
        }
    }

    // 2. .sdkmanrc
    let sdkman = workspace_dir.join(".sdkmanrc");
    if let Ok(content) = fs::read_to_string(&sdkman) {
        let mut new_lines = Vec::new();
        let mut changed = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('#') {
                if let Some((k, v)) = trimmed.split_once('=') {
                    if k.trim() == "java" {
                        let suffix = v
                            .trim()
                            .find('-')
                            .map(|idx| &v.trim()[idx..])
                            .unwrap_or("-tem");
                        new_lines.push(format!("java={num}.0.0{suffix}"));
                        changed = true;
                        continue;
                    }
                }
            }
            new_lines.push(line.to_string());
        }
        if changed && fs::write(&sdkman, format!("{}\n", new_lines.join("\n"))).is_ok() {
            modified.push(sdkman);
        }
    }

    // 3. .tool-versions
    let tool_ver = workspace_dir.join(".tool-versions");
    if let Ok(content) = fs::read_to_string(&tool_ver) {
        let mut new_lines = Vec::new();
        let mut changed = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('#') && trimmed.split_whitespace().next() == Some("java") {
                new_lines.push(format!("java {num}"));
                changed = true;
                continue;
            }
            new_lines.push(line.to_string());
        }
        if changed && fs::write(&tool_ver, format!("{}\n", new_lines.join("\n"))).is_ok() {
            modified.push(tool_ver);
        }
    }

    // 4. pom.xml (update <maven.compiler.release>, <maven.compiler.source>, <maven.compiler.target>, <java.version>)
    let pom = workspace_dir.join("pom.xml");
    if let Ok(content) = fs::read_to_string(&pom) {
        let mut updated = content.clone();
        for tag in [
            "maven.compiler.release",
            "maven.compiler.source",
            "maven.compiler.target",
            "java.version",
        ] {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            if let Some(start) = updated.find(&open) {
                let val_start = start + open.len();
                if let Some(end) = updated[val_start..].find(&close) {
                    updated.replace_range(val_start..val_start + end, &num.to_string());
                }
            }
        }
        if updated != content && fs::write(&pom, updated).is_ok() {
            modified.push(pom);
        }
    }

    modified
}

// ---- Minimal-Overlay `.java-analyzer.toml` Parser & Serializer ----

pub fn parse_config_toml(text: &str) -> ExplicitConfig {
    let mut cfg = ExplicitConfig::default();
    let mut current_section = String::new();

    for raw_line in text.lines() {
        let line = strip_toml_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        let Some((raw_key, raw_val)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        let val = raw_val.trim();

        if current_section.is_empty() {
            match key {
                "build" => {
                    if let Some(s) = unquote_toml_str(val) {
                        cfg.build = parse_build_tool_kind(&s);
                    }
                }
                "jdk" => {
                    if let Some(s) = unquote_toml_str(val) {
                        cfg.jdk = Some(PathBuf::from(s));
                    }
                }
                "release" => {
                    if let Some(r) = parse_version_string(val) {
                        cfg.release = Some(r);
                    }
                }
                "preview" => {
                    if let Some(b) = parse_toml_bool(val) {
                        cfg.preview = Some(b);
                    }
                }
                "classpath_command" => {
                    if val.starts_with('[') {
                        cfg.classpath_command = parse_toml_str_array(val);
                    } else if let Some(s) = unquote_toml_str(val) {
                        cfg.classpath_command =
                            s.split_whitespace().map(ToString::to_string).collect();
                    }
                }
                "include" => {
                    cfg.delta.include = parse_toml_str_array(val)
                        .into_iter()
                        .map(|p| normalize_rel_path(&p))
                        .collect();
                }
                "exclude" => {
                    cfg.delta.exclude = parse_toml_str_array(val)
                        .into_iter()
                        .map(|p| normalize_rel_path(&p))
                        .collect();
                }
                _ => {}
            }
        } else if let Some(mod_rest) = current_section.strip_prefix("modules.") {
            let (mod_key, sub_key) = parse_module_section_header(mod_rest);
            let entry = cfg.delta.modules.entry(mod_key).or_default();
            match sub_key.as_deref() {
                None => match key {
                    "release" => {
                        if let Some(r) = parse_version_string(val) {
                            entry.release = Some(r);
                        }
                    }
                    "preview" => {
                        if let Some(b) = parse_toml_bool(val) {
                            entry.preview = Some(b);
                        }
                    }
                    "sources" => {
                        entry.sources = parse_inline_delta_set(val, true);
                    }
                    "resources" => {
                        entry.resources = parse_inline_delta_set(val, true);
                    }
                    "dependencies" => {
                        entry.dependencies = parse_inline_delta_set(val, false);
                    }
                    _ => {}
                },
                Some("sources") => apply_delta_kv(&mut entry.sources, key, val, true),
                Some("resources") => apply_delta_kv(&mut entry.resources, key, val, true),
                Some("dependencies") => apply_delta_kv(&mut entry.dependencies, key, val, false),
                _ => {}
            }
        }
    }

    cfg
}

fn parse_module_section_header(rest: &str) -> (String, Option<String>) {
    let trimmed = rest.trim();
    if let Some(after_quote) = trimmed.strip_prefix('"') {
        if let Some(end_quote) = after_quote.find('"') {
            let mod_name = after_quote[..end_quote].to_string();
            let tail = after_quote[end_quote + 1..].trim();
            let sub = tail.strip_prefix('.').map(|s| s.trim().to_string());
            let norm_mod = if mod_name.contains('/') || mod_name == "." {
                normalize_rel_path(&mod_name)
            } else {
                mod_name
            };
            return (norm_mod, sub);
        }
    }
    if let Some((m, s)) = trimmed.rsplit_once('.') {
        if matches!(s, "sources" | "resources" | "dependencies") {
            let m = m.trim_matches('"');
            return (m.to_string(), Some(s.to_string()));
        }
    }
    (trimmed.trim_matches('"').to_string(), None)
}

fn parse_inline_delta_set(val: &str, normalize_paths: bool) -> DeltaSet<String> {
    let mut delta = DeltaSet::default();
    let inner = val
        .trim()
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(val);

    for field in ["include", "exclude"] {
        if let Some(idx) = inner.find(field) {
            let after = &inner[idx + field.len()..];
            if let Some(eq_idx) = after.find('=') {
                let after_eq = after[eq_idx + 1..].trim();
                if let Some(open_b) = after_eq.find('[') {
                    if let Some(close_b) = after_eq[open_b..].find(']') {
                        let arr_str = &after_eq[open_b..=open_b + close_b];
                        apply_delta_kv(&mut delta, field, arr_str, normalize_paths);
                    }
                }
            }
        }
    }
    delta
}

fn apply_delta_kv(delta: &mut DeltaSet<String>, key: &str, val: &str, normalize_paths: bool) {
    let items: Vec<String> = parse_toml_str_array(val)
        .into_iter()
        .map(|s| {
            if normalize_paths {
                normalize_rel_path(&s)
            } else {
                s
            }
        })
        .collect();
    match key {
        "include" => delta.include = items,
        "exclude" => delta.exclude = items,
        _ => {}
    }
}

fn strip_toml_comment(line: &str) -> &str {
    let mut in_quotes = false;
    let mut quote_ch = '"';
    for (i, ch) in line.char_indices() {
        if !in_quotes && (ch == '"' || ch == '\'') {
            in_quotes = true;
            quote_ch = ch;
        } else if in_quotes && ch == quote_ch {
            in_quotes = false;
        } else if !in_quotes && ch == '#' {
            return &line[..i];
        }
    }
    line
}

fn unquote_toml_str(val: &str) -> Option<String> {
    let t = val.trim();
    if (t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')) {
        if t.len() >= 2 {
            return Some(t[1..t.len() - 1].to_string());
        }
    }
    None
}

fn parse_toml_bool(val: &str) -> Option<bool> {
    match val.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_toml_str_array(val: &str) -> Vec<String> {
    let t = val.trim();
    let inner = t
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or("");
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote_ch = '"';
    for ch in inner.chars() {
        if !in_quote && (ch == '"' || ch == '\'') {
            in_quote = true;
            quote_ch = ch;
        } else if in_quote && ch == quote_ch {
            in_quote = false;
            items.push(current.clone());
            current.clear();
        } else if in_quote {
            current.push(ch);
        }
    }
    items
}

/// Serializes `config` into a minimal-overlay `.java-analyzer.toml` string against `probe`.
/// Any field in `config` that duplicates what `probe` already uniquely inferred from Tier 2 / Tier 3
/// is omitted so `.java-analyzer.toml` never hardcodes externally declared facts.
pub fn render_minimal_toml(probe: &WorkspaceProbe, config: &ExplicitConfig) -> String {
    let mut lines = Vec::new();

    if let Some(build) = config.build {
        if probe.inferred_build != Some(build) {
            lines.push(format!("build = \"{}\"", build_tool_name(build)));
        }
    }

    if let Some(jdk) = &config.jdk {
        lines.push(format!("jdk = \"{}\"", jdk.display()));
    }

    if let Some(release) = config.release {
        let inferred_rel = probe.inferred_level.map(|l| l.release);
        if inferred_rel != Some(release) {
            lines.push(format!("release = {}", release.number()));
        }
    }

    if let Some(preview) = config.preview {
        let inferred_prev = probe.inferred_level.map(|l| l.preview).unwrap_or(false);
        if preview != inferred_prev {
            lines.push(format!("preview = {preview}"));
        }
    }

    if !config.classpath_command.is_empty() {
        lines.push(format!(
            "classpath_command = {}",
            format_toml_str_array(&config.classpath_command)
        ));
    }

    if !config.delta.include.is_empty() {
        lines.push(format!(
            "include = {}",
            format_toml_str_array(&config.delta.include)
        ));
    }
    if !config.delta.exclude.is_empty() {
        lines.push(format!(
            "exclude = {}",
            format_toml_str_array(&config.delta.exclude)
        ));
    }

    for (mod_key, mod_cfg) in &config.delta.modules {
        if mod_cfg.is_empty() {
            continue;
        }
        let mut mod_lines = Vec::new();
        if let Some(rel) = mod_cfg.release {
            mod_lines.push(format!("release = {}", rel.number()));
        }
        if let Some(prev) = mod_cfg.preview {
            if prev {
                mod_lines.push(format!("preview = {prev}"));
            }
        }
        if !mod_cfg.sources.is_empty() {
            mod_lines.push(format!(
                "sources = {}",
                format_inline_delta_set(&mod_cfg.sources)
            ));
        }
        if !mod_cfg.resources.is_empty() {
            mod_lines.push(format!(
                "resources = {}",
                format_inline_delta_set(&mod_cfg.resources)
            ));
        }
        if !mod_cfg.dependencies.is_empty() {
            mod_lines.push(format!(
                "dependencies = {}",
                format_inline_delta_set(&mod_cfg.dependencies)
            ));
        }
        if !mod_lines.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(format!("[modules.\"{mod_key}\"]"));
            lines.extend(mod_lines);
        }
    }

    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn format_toml_str_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

fn format_inline_delta_set(delta: &DeltaSet<String>) -> String {
    let mut parts = Vec::new();
    if !delta.include.is_empty() {
        parts.push(format!(
            "include = {}",
            format_toml_str_array(&delta.include)
        ));
    }
    if !delta.exclude.is_empty() {
        parts.push(format!(
            "exclude = {}",
            format_toml_str_array(&delta.exclude)
        ));
    }
    format!("{{ {} }}", parts.join(", "))
}

// ---- JDK Discovery ----

pub fn discover_jdks(preferred_release: Option<Release>) -> Vec<JdkCandidate> {
    let mut candidates = Vec::new();
    let mut seen_homes = BTreeSet::new();

    // 1. JAVA_HOME
    if let Ok(java_home) = env::var("JAVA_HOME") {
        let path = PathBuf::from(java_home);
        if is_valid_jdk_home(&path) && seen_homes.insert(path.clone()) {
            let release = probe_jdk_release(&path);
            candidates.push(JdkCandidate {
                home: path,
                release,
                source: "JAVA_HOME".to_string(),
            });
        }
    }

    // 2. Standard macOS / Linux JVM install roots
    let system_roots = [
        PathBuf::from("/Library/Java/JavaVirtualMachines"),
        PathBuf::from("/usr/lib/jvm"),
    ];
    for root in &system_roots {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let mac_home = path.join("Contents/Home");
                let candidate_home = if is_valid_jdk_home(&mac_home) {
                    mac_home
                } else {
                    path
                };
                if is_valid_jdk_home(&candidate_home) && seen_homes.insert(candidate_home.clone()) {
                    let release = probe_jdk_release(&candidate_home);
                    candidates.push(JdkCandidate {
                        home: candidate_home,
                        release,
                        source: "system".to_string(),
                    });
                }
            }
        }
    }

    // Sort: matching preferred_release first, then highest release
    candidates.sort_by(|a, b| {
        let a_match = preferred_release.is_some() && a.release == preferred_release;
        let b_match = preferred_release.is_some() && b.release == preferred_release;
        b_match
            .cmp(&a_match)
            .then_with(|| b.release.cmp(&a.release))
    });

    candidates
}

pub fn is_valid_jdk_home(path: &Path) -> bool {
    path.join("bin/javac").is_file()
        || path.join("bin/javac.exe").is_file()
        || path.join("lib/modules").is_file()
}

pub fn probe_jdk_release(jdk_home: &Path) -> Option<Release> {
    // Fast path: read `<jdk_home>/release` file (`JAVA_VERSION="25.0.1"`)
    let release_file = jdk_home.join("release");
    if let Ok(content) = fs::read_to_string(&release_file) {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("JAVA_VERSION=") {
                if let Some(rel) = parse_version_string(val) {
                    return Some(rel);
                }
            }
        }
    }
    // Fallback: parse directory name
    if let Some(name) = jdk_home.file_name().and_then(|n| n.to_str()) {
        if let Some(rel) = parse_version_string(name) {
            return Some(rel);
        }
    }
    // Fallback: invoke `javac -version`
    let javac = jdk_home.join("bin/javac");
    if javac.is_file() {
        if let Ok(out) = Command::new(javac).arg("-version").output() {
            let text = format!(
                "{} {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            if let Some(rel) = parse_version_string(&text) {
                return Some(rel);
            }
        }
    }
    None
}

impl WorkspaceProbe {
    pub fn language_level_for_path(&self, file_path: &Path) -> LanguageLevel {
        let rel_path = file_path
            .strip_prefix(&self.workspace_dir)
            .ok()
            .or_else(|| (!file_path.is_absolute()).then_some(file_path));

        if let Some(rel) = rel_path {
            let norm = normalize_rel_path(&rel.to_string_lossy());

            let best_root = self
                .layout
                .modules
                .iter()
                .flat_map(|module| module.sources.iter().map(move |src| (module, src)))
                .filter(|(_, src)| {
                    src.rel_path == "."
                        || norm == src.rel_path
                        || norm.starts_with(&format!("{}/", src.rel_path))
                })
                .max_by_key(|(_, src)| {
                    if src.rel_path == "." {
                        0
                    } else {
                        src.rel_path.len()
                    }
                });

            if let Some((module, src)) = best_root {
                if src.scope == RootScope::Test {
                    if let Some(test_lvl) = module.test_language_level {
                        return test_lvl;
                    }
                }
                return module.language_level;
            }

            let best_module = self
                .layout
                .modules
                .iter()
                .filter(|module| {
                    module.rel_path == "."
                        || norm == module.rel_path
                        || norm.starts_with(&format!("{}/", module.rel_path))
                })
                .max_by_key(|module| {
                    if module.rel_path == "." {
                        0
                    } else {
                        module.rel_path.len()
                    }
                });

            if let Some(module) = best_module {
                if RootScope::from_path(&norm) == RootScope::Test {
                    if let Some(test_lvl) = module.test_language_level {
                        return test_lvl;
                    }
                }
                return module.language_level;
            }
        }
        self.resolved_level
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = env::temp_dir().join(format!(
                "java_analyzer_ws_test_{}_{}_{}",
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
    fn ant_and_ivy_pair_without_conflict_and_extract_compile_time_deps() {
        let tmp = TempDir::new("ant_ivy");
        tmp.write(
            "build.xml",
            r#"<project name="demo" default="compile" basedir=".">
                <property name="src.dir" value="src"/>
                <property name="test.dir" value="test"/>
                <property name="lib.dir" value="lib"/>
                <target name="compile">
                    <javac srcdir="${src.dir}" destdir="build/classes" release="17">
                        <classpath>
                            <fileset dir="${lib.dir}" includes="**/*.jar"/>
                        </classpath>
                    </javac>
                </target>
            </project>"#,
        );
        tmp.write(
            "ivy.xml",
            r#"<ivy-module version="2.0">
                <info organisation="com.example" module="demo"/>
                <dependencies>
                    <dependency org="com.google.guava" name="guava" rev="33.0.0-jre" conf="compile->default"/>
                    <dependency org="org.junit.jupiter" name="junit-jupiter" rev="5.10.2" conf="test->default"/>
                    <dependency org="org.postgresql" name="postgresql" rev="42.7.2" conf="runtime->default"/>
                </dependencies>
            </ivy-module>"#,
        );
        tmp.write("lib/helper.jar", "");
        tmp.write(
            "src/com/example/App.java",
            "package com.example;\npublic class App {}\n",
        );

        let probe = probe_workspace(&tmp.0);
        assert!(
            probe.conflicts.is_empty(),
            "build.xml + ivy.xml must not conflict: {:?}",
            probe.conflicts
        );
        assert_eq!(probe.resolved_build, BuildToolKind::Ant);
        assert_eq!(
            probe.resolved_level,
            LanguageLevel::new(Release::JAVA_17, false)
        );

        let root_mod = &probe.layout.modules[0];
        assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "com.google.guava".to_string(),
            artifact: "guava".to_string(),
            version: "33.0.0-jre".to_string(),
            classifier: None,
            scope: RootScope::Main,
        }));
        assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "org.junit.jupiter".to_string(),
            artifact: "junit-jupiter".to_string(),
            version: "5.10.2".to_string(),
            classifier: None,
            scope: RootScope::Test,
        }));
        // Runtime-only dependency must be excluded from compile-time LSP model
        assert!(!root_mod.dependencies.iter().any(|d| match d {
            ModuleDependency::Maven { artifact, .. } => artifact == "postgresql",
            _ => false,
        }));
        // Local JAR from <fileset dir="${lib.dir}"/>
        assert!(root_mod.dependencies.contains(&ModuleDependency::Path {
            path: "lib/helper.jar".to_string(),
            scope: RootScope::Main,
        }));
    }

    #[test]
    fn detects_build_and_release_conflicts_and_disambiguates_via_tier1_or_reconcile() {
        let tmp = TempDir::new("conflicts");
        tmp.write(
            "pom.xml",
            r#"<project>
                <properties>
                    <maven.compiler.release>17</maven.compiler.release>
                </properties>
            </project>"#,
        );
        tmp.write(
            "build.gradle.kts",
            r#"java {
                toolchain {
                    languageVersion.set(JavaLanguageVersion.of(21))
                }
            }"#,
        );
        tmp.write(".java-version", "25\n");

        let probe = probe_workspace(&tmp.0);
        assert_eq!(probe.conflicts.len(), 2);
        assert!(probe
            .conflicts
            .iter()
            .any(|c| matches!(c, Conflict::BuildTool(_))));
        assert!(probe
            .conflicts
            .iter()
            .any(|c| matches!(c, Conflict::Release(_))));

        // Reconciling external version files updates .java-version and pom.xml to 21
        let modified = reconcile_version_conflict(&tmp.0, Release::JAVA_21);
        assert_eq!(modified.len(), 2);

        let probe_after_reconcile = probe_workspace(&tmp.0);
        assert!(
            !probe_after_reconcile
                .conflicts
                .iter()
                .any(|c| matches!(c, Conflict::Release(_))),
            "Release conflict should be resolved after reconciling external files"
        );
        assert_eq!(
            probe_after_reconcile.resolved_level,
            LanguageLevel::new(Release::JAVA_21, false)
        );

        // Now disambiguate the remaining build tool conflict with a minimal .java-analyzer.toml
        let mut cfg = ExplicitConfig::default();
        cfg.build = Some(BuildToolKind::Gradle);
        cfg.release = Some(Release::JAVA_21); // Matches uniquely inferred release -> must be omitted!
        let toml_text = render_minimal_toml(&probe_after_reconcile, &cfg);
        assert_eq!(toml_text, "build = \"gradle\"\n");

        tmp.write(".java-analyzer.toml", &toml_text);
        let final_probe = probe_workspace(&tmp.0);
        assert!(final_probe.conflicts.is_empty());
        assert_eq!(final_probe.resolved_build, BuildToolKind::Gradle);
    }

    #[test]
    fn meta_inf_manifest_signals_resource_root_and_tier1_deltas_override() {
        let tmp = TempDir::new("meta_inf_and_deltas");
        tmp.write(
            "src/com/example/App.java",
            "package com.example;\nclass App {}\n",
        );
        tmp.write("assets/META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n");
        tmp.write(
            ".java-analyzer.toml",
            r#"
[modules."."]
sources = { include = ["generated/src"] }
dependencies = { include = ["com.google.guava:guava:33.3.1-jre", "github:google/guava#v33.3.1"] }
"#,
        );

        let probe = probe_workspace(&tmp.0);
        let root_mod = &probe.layout.modules[0];
        assert_eq!(root_mod.rel_path, ".");
        assert!(root_mod.sources.iter().any(|s| s.rel_path == "src"));
        assert!(root_mod
            .sources
            .iter()
            .any(|s| s.rel_path == "generated/src"));
        assert!(
            root_mod.resources.iter().any(|r| r.rel_path == "assets"),
            "assets/ containing META-INF/MANIFEST.MF should be inferred as a ResourceRoot"
        );
        assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "com.google.guava".to_string(),
            artifact: "guava".to_string(),
            version: "33.3.1-jre".to_string(),
            classifier: None,
            scope: RootScope::Main,
        }));
        assert!(root_mod.dependencies.contains(&ModuleDependency::Git {
            repo: "github:google/guava".to_string(),
            reference: Some("v33.3.1".to_string()),
            scope: RootScope::Main,
        }));
    }

    #[test]
    fn maven_pom_strips_comments_expands_properties_and_handles_dependency_management() {
        let tmp = TempDir::new("maven_props_and_mgmt");
        tmp.write(
            "pom.xml",
            r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <parent>
                    <groupId>com.example</groupId>
                    <artifactId>parent-pom</artifactId>
                    <version>9.9.9</version>
                </parent>
                <artifactId>demo</artifactId>
                <version>2.4.0</version>
                <!--
                <properties>
                    <maven.compiler.release>8</maven.compiler.release>
                </properties>
                <dependency>
                    <groupId>com.commented</groupId>
                    <artifactId>ignored-dep</artifactId>
                    <version>1.0.0</version>
                </dependency>
                -->
                <properties>
                    <java.version>21</java.version>
                    <maven.compiler.release>${java.version}</maven.compiler.release>
                    <guava.group>com.google.guava</guava.group>
                    <guava.version>33.4.0-jre</guava.version>
                    <junit.version>5.10.2</junit.version>
                    <custom.src>custom/src</custom.src>
                    <custom.test>custom/test</custom.test>
                    <native.classifier>linux-x86_64</native.classifier>
                </properties>
                <build>
                    <sourceDirectory>${custom.src}</sourceDirectory>
                    <testSourceDirectory>${custom.test}</testSourceDirectory>
                </build>
                <dependencyManagement>
                    <dependencies>
                        <dependency>
                            <groupId>${guava.group}</groupId>
                            <artifactId>guava</artifactId>
                            <version>${guava.version}</version>
                        </dependency>
                        <dependency>
                            <groupId>org.unused</groupId>
                            <artifactId>managed-only</artifactId>
                            <version>1.2.3</version>
                        </dependency>
                    </dependencies>
                </dependencyManagement>
                <dependencies>
                    <dependency>
                        <groupId>${guava.group}</groupId>
                        <artifactId>guava</artifactId>
                    </dependency>
                    <dependency>
                        <groupId>com.example</groupId>
                        <artifactId>sibling</artifactId>
                        <version>${project.version}</version>
                        <classifier>${native.classifier}</classifier>
                    </dependency>
                    <dependency>
                        <groupId>org.junit.jupiter</groupId>
                        <artifactId>junit-jupiter</artifactId>
                        <version>${junit.version}</version>
                        <scope>test</scope>
                    </dependency>
                    <dependency>
                        <groupId>org.unmanaged</groupId>
                        <artifactId>no-version</artifactId>
                    </dependency>
                </dependencies>
            </project>"#,
        );
        tmp.write(
            "custom/src/com/example/App.java",
            "package com.example;\nclass App {}\n",
        );
        tmp.write(
            "custom/test/com/example/AppTest.java",
            "package com.example;\nclass AppTest {}\n",
        );

        let probe = probe_workspace(&tmp.0);
        assert_eq!(probe.resolved_build, BuildToolKind::Maven);
        assert_eq!(
            probe.resolved_level,
            LanguageLevel::new(Release::JAVA_21, false)
        );

        let root_mod = &probe.layout.modules[0];
        assert!(root_mod.sources.iter().any(|s| s.rel_path == "custom/src"));
        assert!(root_mod
            .sources
            .iter()
            .any(|s| s.rel_path == "custom/test" && s.scope == RootScope::Test));

        // Managed version fallback & property expansion
        assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "com.google.guava".to_string(),
            artifact: "guava".to_string(),
            version: "33.4.0-jre".to_string(),
            classifier: None,
            scope: RootScope::Main,
        }));
        // project.version & classifier expansion
        assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "com.example".to_string(),
            artifact: "sibling".to_string(),
            version: "2.4.0".to_string(),
            classifier: Some("linux-x86_64".to_string()),
            scope: RootScope::Main,
        }));
        // Test dependency expansion
        assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "org.junit.jupiter".to_string(),
            artifact: "junit-jupiter".to_string(),
            version: "5.10.2".to_string(),
            classifier: None,
            scope: RootScope::Test,
        }));
        // Unmanaged dependency omitting version still falls back to "*"
        assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "org.unmanaged".to_string(),
            artifact: "no-version".to_string(),
            version: "*".to_string(),
            classifier: None,
            scope: RootScope::Main,
        }));
        // Managed-only and commented dependencies must NOT appear
        assert!(!root_mod.dependencies.iter().any(|d| match d {
            ModuleDependency::Maven { artifact, .. } => {
                artifact == "managed-only" || artifact == "ignored-dep"
            }
            _ => false,
        }));
    }

    #[test]
    fn scan_ignores_site_resources_and_skips_java_fixtures_inside_resource_roots() {
        let tmp = TempDir::new("scan_resources_filter");
        tmp.write(
            "src/main/java/com/example/resources/ResourceHelper.java",
            "package com.example.resources;\npublic class ResourceHelper {}\n",
        );
        tmp.write("src/main/resources/app.properties", "k=v\n");
        tmp.write(
            "src/test/resources/fixtures/Template.java",
            "package fixtures;\npublic class Template {}\n",
        );
        tmp.write("src/site/resources/images/logo.png", "png");
        tmp.write("src/site/resources/SiteSample.java", "class SiteSample {}\n");
        tmp.write("docs/resources/guide.md", "# guide\n");
        tmp.write(
            "spi/resources/META-INF/services/com.example.Spi",
            "com.example.Impl\n",
        );

        let probe = probe_workspace(&tmp.0);
        // Only real Java sources are collected; fixtures in conventional resource roots and src/site are skipped.
        assert!(probe
            .file_paths
            .contains_key("src/main/java/com/example/resources/ResourceHelper.java"));
        assert!(!probe
            .file_paths
            .contains_key("src/test/resources/fixtures/Template.java"));
        assert!(!probe
            .file_paths
            .contains_key("src/site/resources/SiteSample.java"));

        let root_mod = probe.layout.module_by_path(".").unwrap();
        let res_paths: Vec<(&str, RootScope)> = root_mod
            .resources
            .iter()
            .map(|r| (r.rel_path.as_str(), r.scope))
            .collect();
        assert_eq!(
            res_paths,
            vec![
                ("spi/resources", RootScope::Main),
                ("src/main/resources", RootScope::Main),
                ("src/test/resources", RootScope::Test),
            ]
        );
        // Package `com.example.resources` was NOT misclassified as a resource root
        assert!(root_mod
            .sources
            .iter()
            .any(|s| s.rel_path == "src/main/java" && s.scope == RootScope::Main));
    }

    #[test]
    fn meta_inf_dir_allows_java_sources_while_conventional_resources_skip_them() {
        let tmp = TempDir::new("meta_inf_with_java_sources");
        tmp.write("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n");
        tmp.write(
            "src/main/java/com/example/App.java",
            "package com.example;\npublic class App {}\n",
        );
        tmp.write(
            "test/META-INF/services/com.example.Spi",
            "com.example.SpiImpl\n",
        );
        tmp.write(
            "test/com/example/FooTest.java",
            "package com.example;\npublic class FooTest {}\n",
        );
        tmp.write(
            "src/test/resources/Fixture.java",
            "package fixtures;\npublic class Fixture {}\n",
        );

        let probe = probe_workspace(&tmp.0);

        let foo_test_id = probe
            .file_paths
            .get("test/com/example/FooTest.java")
            .copied()
            .expect("FooTest.java under test/ with META-INF/ must be scanned");
        assert!(
            probe
                .file_paths
                .contains_key("src/main/java/com/example/App.java"),
            "root META-INF/ must not skip workspace .java sources"
        );
        assert!(
            !probe.file_paths.contains_key("src/test/resources/Fixture.java"),
            "conventional src/test/resources/ must continue skipping .java fixtures"
        );

        let root_mod = probe.layout.module_by_path(".").unwrap();
        assert!(
            root_mod
                .resources
                .iter()
                .any(|r| r.rel_path == "test" && r.scope == RootScope::Test),
            "test/ should be inferred as a Test ResourceRoot: {:?}",
            root_mod.resources
        );
        assert!(
            root_mod
                .resources
                .iter()
                .any(|r| r.rel_path == "." && r.scope == RootScope::Main),
            "workspace root with META-INF/ should be inferred as a Main ResourceRoot: {:?}",
            root_mod.resources
        );
        assert!(
            root_mod
                .resources
                .iter()
                .any(|r| r.rel_path == "src/test/resources" && r.scope == RootScope::Test),
            "src/test/resources should be inferred as a Test ResourceRoot: {:?}",
            root_mod.resources
        );

        let test_src = root_mod
            .sources
            .iter()
            .find(|s| s.rel_path == "test" && s.scope == RootScope::Test)
            .expect("test/ should also be inferred as a Test SourceRoot");
        assert!(
            test_src.files.contains(&foo_test_id),
            "test SourceRoot should contain FooTest.java: {:?}",
            test_src.files
        );
    }

    #[test]
    fn maven_pom_ignores_build_reporting_and_profile_plugin_dependencies() {
        let tmp = TempDir::new("maven_ignore_plugin_deps");
        tmp.write(
            "pom.xml",
            r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <groupId>com.example</groupId>
                <artifactId>demo</artifactId>
                <version>1.0.0</version>
                <properties>
                    <maven.compiler.release>8</maven.compiler.release>
                    <maven.compiler.testSource>17</maven.compiler.testSource>
                    <custom.src>custom/src</custom.src>
                </properties>
                <build>
                    <sourceDirectory>${custom.src}</sourceDirectory>
                    <pluginManagement>
                        <plugins>
                            <plugin>
                                <groupId>org.eclipse.tycho</groupId>
                                <artifactId>tycho-compiler-plugin</artifactId>
                                <dependencies>
                                    <dependency>
                                        <groupId>org.eclipse.jdt</groupId>
                                        <artifactId>org.eclipse.jdt.core</artifactId>
                                        <version>3.33.0</version>
                                    </dependency>
                                </dependencies>
                            </plugin>
                        </plugins>
                    </pluginManagement>
                    <plugins>
                        <plugin>
                            <groupId>com.github.siom79.japicmp</groupId>
                            <artifactId>japicmp-maven-plugin</artifactId>
                            <dependencies>
                                <dependency>
                                    <groupId>${project.groupId}</groupId>
                                    <artifactId>${project.artifactId}</artifactId>
                                    <version>0.0.0-OLD</version>
                                </dependency>
                            </dependencies>
                        </plugin>
                    </plugins>
                </build>
                <reporting>
                    <plugins>
                        <plugin>
                            <groupId>org.apache.maven.plugins</groupId>
                            <artifactId>maven-javadoc-plugin</artifactId>
                            <dependencies>
                                <dependency>
                                    <groupId>com.example</groupId>
                                    <artifactId>report-doclet</artifactId>
                                    <version>1.0.0</version>
                                </dependency>
                            </dependencies>
                        </plugin>
                    </plugins>
                </reporting>
                <profiles>
                    <profile>
                        <id>extra</id>
                        <dependencies>
                            <dependency>
                                <groupId>com.example</groupId>
                                <artifactId>profile-only</artifactId>
                                <version>1.0.0</version>
                            </dependency>
                        </dependencies>
                    </profile>
                </profiles>
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
            "custom/src/com/example/App.java",
            "package com.example;\nclass App {}\n",
        );
        tmp.write(
            "src/test/java/com/example/AppTest.java",
            "package com.example;\nclass AppTest {}\n",
        );

        let probe = probe_workspace(&tmp.0);
        let root_mod = probe.layout.module_by_path(".").unwrap();
        assert!(root_mod
            .sources
            .iter()
            .any(|s| s.rel_path == "custom/src" && s.scope == RootScope::Main));
        assert!(root_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "com.google.guava".to_string(),
            artifact: "guava".to_string(),
            version: "33.2.1-jre".to_string(),
            classifier: None,
            scope: RootScope::Main,
        }));
        assert_eq!(
            root_mod.dependencies.len(),
            1,
            "plugin/reporting/profile dependencies must not leak: {:?}",
            root_mod.dependencies
        );
        assert_eq!(
            root_mod.test_language_level,
            Some(LanguageLevel::new(Release::JAVA_17, false))
        );
        assert_eq!(
            probe.language_level_for_path(&tmp.0.join("custom/src/com/example/App.java")),
            LanguageLevel::new(Release::JAVA_8, false)
        );
        assert_eq!(
            probe.language_level_for_path(&tmp.0.join("src/test/java/com/example/AppTest.java")),
            LanguageLevel::new(Release::JAVA_17, false)
        );
    }

    #[test]
    fn maven_multi_module_inherits_props_splits_deps_and_resolves_test_language_level() {
        let tmp = TempDir::new("maven_multi_module");
        tmp.write(
            "pom.xml",
            r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <groupId>com.example</groupId>
                <artifactId>parent</artifactId>
                <version>2.0.0</version>
                <packaging>pom</packaging>
                <properties>
                    <maven.compiler.release>8</maven.compiler.release>
                    <maven.compiler.testRelease>17</maven.compiler.testRelease>
                    <junit.version>5.10.2</junit.version>
                </properties>
                <dependencyManagement>
                    <dependencies>
                        <dependency>
                            <groupId>com.google.errorprone</groupId>
                            <artifactId>error_prone_annotations</artifactId>
                            <version>2.28.0</version>
                        </dependency>
                        <dependency>
                            <groupId>com.example</groupId>
                            <artifactId>overridden-lib</artifactId>
                            <version>1.0.0</version>
                        </dependency>
                    </dependencies>
                </dependencyManagement>
                <dependencies>
                    <dependency>
                        <groupId>com.example</groupId>
                        <artifactId>root-only-dep</artifactId>
                        <version>1.0.0</version>
                    </dependency>
                </dependencies>
            </project>"#,
        );
        tmp.write(
            "core/pom.xml",
            r#"<project>
                <parent>
                    <groupId>com.example</groupId>
                    <artifactId>parent</artifactId>
                    <version>2.0.0</version>
                </parent>
                <artifactId>core</artifactId>
                <dependencies>
                    <dependency>
                        <groupId>com.google.errorprone</groupId>
                        <artifactId>error_prone_annotations</artifactId>
                    </dependency>
                    <dependency>
                        <groupId>org.junit.jupiter</groupId>
                        <artifactId>junit-jupiter</artifactId>
                        <version>${junit.version}</version>
                        <scope>test</scope>
                    </dependency>
                </dependencies>
            </project>"#,
        );
        tmp.write(
            "core/src/main/java/com/example/core/Core.java",
            "package com.example.core;\npublic class Core {}\n",
        );
        tmp.write(
            "core/src/test/java/com/example/core/CoreTest.java",
            "package com.example.core;\npublic class CoreTest {}\n",
        );
        tmp.write(
            "modern/pom.xml",
            r#"<project>
                <parent>
                    <groupId>com.example</groupId>
                    <artifactId>parent</artifactId>
                    <version>2.0.0</version>
                </parent>
                <artifactId>modern</artifactId>
                <properties>
                    <maven.compiler.release>17</maven.compiler.release>
                </properties>
                <dependencyManagement>
                    <dependencies>
                        <dependency>
                            <groupId>com.example</groupId>
                            <artifactId>overridden-lib</artifactId>
                            <version>2.0.0</version>
                        </dependency>
                    </dependencies>
                </dependencyManagement>
                <dependencies>
                    <dependency>
                        <groupId>com.example</groupId>
                        <artifactId>overridden-lib</artifactId>
                    </dependency>
                    <dependency>
                        <groupId>org.slf4j</groupId>
                        <artifactId>slf4j-api</artifactId>
                        <version>2.0.12</version>
                    </dependency>
                    <dependency>
                        <groupId>com.example</groupId>
                        <artifactId>excluded-dep</artifactId>
                        <version>1.0.0</version>
                    </dependency>
                </dependencies>
            </project>"#,
        );
        tmp.write(
            "modern/src/main/java/com/example/modern/Modern.java",
            "package com.example.modern;\npublic class Modern {}\n",
        );
        tmp.write(
            "modern/src/test/java/com/example/modern/ModernTest.java",
            "package com.example.modern;\npublic class ModernTest {}\n",
        );
        tmp.write(
            ".java-analyzer.toml",
            r#"
[modules."modern"]
dependencies = { exclude = ["com.example:excluded-dep"] }
"#,
        );

        let probe = probe_workspace(&tmp.0);
        assert!(probe.conflicts.is_empty(), "conflicts: {:?}", probe.conflicts);
        assert_eq!(
            probe.resolved_level,
            LanguageLevel::new(Release::JAVA_8, false)
        );
        assert_eq!(probe.layout.modules.len(), 2);

        let core_mod = probe.layout.module_by_path("core").unwrap();
        assert_eq!(
            core_mod.language_level,
            LanguageLevel::new(Release::JAVA_8, false)
        );
        assert_eq!(
            core_mod.test_language_level,
            Some(LanguageLevel::new(Release::JAVA_17, false))
        );
        assert!(core_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "com.google.errorprone".to_string(),
            artifact: "error_prone_annotations".to_string(),
            version: "2.28.0".to_string(),
            classifier: None,
            scope: RootScope::Main,
        }));
        assert!(core_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "org.junit.jupiter".to_string(),
            artifact: "junit-jupiter".to_string(),
            version: "5.10.2".to_string(),
            classifier: None,
            scope: RootScope::Test,
        }));
        assert!(
            !core_mod.dependencies.iter().any(|d| match d {
                ModuleDependency::Maven { artifact, .. } => {
                    artifact == "root-only-dep" || artifact == "slf4j-api"
                }
                _ => false,
            }),
            "root-only or sibling deps must not leak into core: {:?}",
            core_mod.dependencies
        );

        let modern_mod = probe.layout.module_by_path("modern").unwrap();
        assert_eq!(
            modern_mod.language_level,
            LanguageLevel::new(Release::JAVA_17, false)
        );
        assert_eq!(
            modern_mod.test_language_level,
            Some(LanguageLevel::new(Release::JAVA_17, false))
        );
        assert!(modern_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "com.example".to_string(),
            artifact: "overridden-lib".to_string(),
            version: "2.0.0".to_string(),
            classifier: None,
            scope: RootScope::Main,
        }));
        assert!(modern_mod.dependencies.contains(&ModuleDependency::Maven {
            group: "org.slf4j".to_string(),
            artifact: "slf4j-api".to_string(),
            version: "2.0.12".to_string(),
            classifier: None,
            scope: RootScope::Main,
        }));
        assert!(
            !modern_mod.dependencies.iter().any(|d| match d {
                ModuleDependency::Maven { artifact, .. } => {
                    artifact == "root-only-dep"
                        || artifact == "error_prone_annotations"
                        || artifact == "excluded-dep"
                }
                _ => false,
            }),
            "excluded or root/sibling deps must not appear in modern: {:?}",
            modern_mod.dependencies
        );

        assert_eq!(
            probe.language_level_for_path(&tmp.0.join("core/src/main/java/com/example/core/Core.java")),
            LanguageLevel::new(Release::JAVA_8, false)
        );
        assert_eq!(
            probe.language_level_for_path(
                &tmp.0.join("core/src/test/java/com/example/core/CoreTest.java")
            ),
            LanguageLevel::new(Release::JAVA_17, false)
        );
        assert_eq!(
            probe.language_level_for_path(
                &tmp.0.join("modern/src/main/java/com/example/modern/Modern.java")
            ),
            LanguageLevel::new(Release::JAVA_17, false)
        );
        assert_eq!(
            probe.language_level_for_path(
                &tmp.0.join("modern/src/test/java/com/example/modern/ModernTest.java")
            ),
            LanguageLevel::new(Release::JAVA_17, false)
        );
    }

    #[test]
    fn maven_submodule_expands_project_parent_properties_in_dependencies() {
        let tmp = TempDir::new("maven_parent_props");
        tmp.write(
            "pom.xml",
            r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <groupId>com.example.corp</groupId>
                <artifactId>corp-parent</artifactId>
                <version>3.4.5</version>
                <packaging>pom</packaging>
                <properties>
                    <maven.compiler.release>17</maven.compiler.release>
                </properties>
            </project>"#,
        );
        let sub_pom = r#"<project>
                <parent>
                    <groupId>com.example.corp</groupId>
                    <artifactId>corp-parent</artifactId>
                    <version>3.4.5</version>
                </parent>
                <artifactId>service</artifactId>
                <version>1.0.0-SNAPSHOT</version>
                <dependencies>
                    <dependency>
                        <groupId>${project.parent.groupId}</groupId>
                        <artifactId>shared-core</artifactId>
                        <version>${project.parent.version}</version>
                    </dependency>
                </dependencies>
            </project>"#;
        tmp.write("service/pom.xml", sub_pom);
        tmp.write(
            "service/src/main/java/com/example/Service.java",
            "package com.example;\npublic class Service {}\n",
        );

        let sub_props = parse_maven_properties(sub_pom);
        assert_eq!(
            sub_props.get("project.parent.groupId").map(String::as_str),
            Some("com.example.corp")
        );
        assert_eq!(
            sub_props.get("project.parent.artifactId").map(String::as_str),
            Some("corp-parent")
        );
        assert_eq!(
            sub_props.get("project.parent.version").map(String::as_str),
            Some("3.4.5")
        );

        let probe = probe_workspace(&tmp.0);
        let service_mod = probe.layout.module_by_path("service").unwrap();
        assert!(
            service_mod.dependencies.contains(&ModuleDependency::Maven {
                group: "com.example.corp".to_string(),
                artifact: "shared-core".to_string(),
                version: "3.4.5".to_string(),
                classifier: None,
                scope: RootScope::Main,
            }),
            "expected expanded parent groupId and version in deps: {:?}",
            service_mod.dependencies
        );
    }

    #[test]
    fn maven_test_release_tag_and_profile_test_compile_execution() {
        // 1. Direct <testRelease>17</testRelease> in maven-compiler-plugin configuration
        let tmp_tag = TempDir::new("maven_test_release_tag");
        tmp_tag.write(
            "pom.xml",
            r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <groupId>com.example</groupId>
                <artifactId>demo</artifactId>
                <version>1.0.0</version>
                <properties>
                    <maven.compiler.release>8</maven.compiler.release>
                </properties>
                <build>
                    <plugins>
                        <plugin>
                            <groupId>org.apache.maven.plugins</groupId>
                            <artifactId>maven-compiler-plugin</artifactId>
                            <configuration>
                                <testRelease>17</testRelease>
                            </configuration>
                        </plugin>
                    </plugins>
                </build>
            </project>"#,
        );
        tmp_tag.write(
            "src/main/java/com/example/App.java",
            "package com.example;\nclass App {}\n",
        );
        tmp_tag.write(
            "src/test/java/com/example/AppTest.java",
            "package com.example;\nclass AppTest {}\n",
        );

        let probe_tag = probe_workspace(&tmp_tag.0);
        let root_tag = probe_tag.layout.module_by_path(".").unwrap();
        assert_eq!(
            root_tag.language_level,
            LanguageLevel::new(Release::JAVA_8, false)
        );
        assert_eq!(
            root_tag.test_language_level,
            Some(LanguageLevel::new(Release::JAVA_17, false))
        );
        assert_eq!(
            probe_tag.language_level_for_path(&tmp_tag.0.join("src/test/java/com/example/AppTest.java")),
            LanguageLevel::new(Release::JAVA_17, false)
        );

        // 2. <profile> containing <execution> with testCompile goal and <configuration><release>17</release></configuration>
        let tmp_prof = TempDir::new("maven_profile_test_compile");
        tmp_prof.write(
            "pom.xml",
            r#"<project>
                <modelVersion>4.0.0</modelVersion>
                <groupId>com.example</groupId>
                <artifactId>demo</artifactId>
                <version>1.0.0</version>
                <properties>
                    <maven.compiler.release>8</maven.compiler.release>
                </properties>
                <profiles>
                    <profile>
                        <id>jdk17-tests</id>
                        <build>
                            <plugins>
                                <plugin>
                                    <groupId>org.apache.maven.plugins</groupId>
                                    <artifactId>maven-compiler-plugin</artifactId>
                                    <executions>
                                        <execution>
                                            <id>test-compile-11</id>
                                            <goals><goal>testCompile</goal></goals>
                                            <configuration>
                                                <release>11</release>
                                            </configuration>
                                        </execution>
                                        <execution>
                                            <id>test-compile-17</id>
                                            <goals><goal>testCompile</goal></goals>
                                            <configuration>
                                                <release>17</release>
                                            </configuration>
                                        </execution>
                                    </executions>
                                </plugin>
                            </plugins>
                        </build>
                    </profile>
                </profiles>
            </project>"#,
        );
        tmp_prof.write(
            "src/main/java/com/example/App.java",
            "package com.example;\nclass App {}\n",
        );
        tmp_prof.write(
            "src/test/java/com/example/AppTest.java",
            "package com.example;\nclass AppTest {}\n",
        );

        let probe_prof = probe_workspace(&tmp_prof.0);
        let root_prof = probe_prof.layout.module_by_path(".").unwrap();
        assert_eq!(
            root_prof.language_level,
            LanguageLevel::new(Release::JAVA_8, false)
        );
        assert_eq!(
            root_prof.test_language_level,
            Some(LanguageLevel::new(Release::JAVA_17, false))
        );
        assert_eq!(
            probe_prof.language_level_for_path(&tmp_prof.0.join("src/test/java/com/example/AppTest.java")),
            LanguageLevel::new(Release::JAVA_17, false)
        );
    }

    #[test]
    fn skips_java_fixtures_inside_noncompilable_resource_dirs() {
        let tmp_only = TempDir::new("noncompilable_resources_only");
        tmp_only.write(
            "src/test/resources-noncompilable/Broken.java",
            "package broken;\nclass Broken { @@@ invalid syntax }\n",
        );
        tmp_only.write(
            "src/it/resources-noncompilable/module-info.java",
            "module bogus.fixture.mod { requires foo.bar; }\n",
        );

        let probe_only = probe_workspace(&tmp_only.0);
        assert!(
            probe_only.file_paths.is_empty(),
            "noncompilable resource fixtures must be skipped completely: {:?}",
            probe_only.file_paths
        );
        assert!(
            probe_only
                .layout
                .modules
                .iter()
                .all(|m| m.module_name.is_none()),
            "skipped module-info.java fixture must not produce a JPMS module name: {:?}",
            probe_only.layout.modules
        );
        assert!(
            probe_only
                .layout
                .modules
                .iter()
                .all(|m| m.resources.is_empty()),
            "resources-noncompilable must not be added as a ResourceRoot: {:?}",
            probe_only.layout.modules
        );

        // Also ensure real sources are still scanned alongside skipped resources-* subtrees
        let tmp_mixed = TempDir::new("noncompilable_resources_mixed");
        tmp_mixed.write(
            "src/main/java/com/example/App.java",
            "package com.example;\npublic class App {}\n",
        );
        tmp_mixed.write(
            "src/test/resources-noncompilable/Broken.java",
            "class Broken { @@@ }\n",
        );
        tmp_mixed.write(
            "src/test/resources-noncompilable/META-INF/MANIFEST.MF",
            "Manifest-Version: 1.0\n",
        );
        tmp_mixed.write(
            "src/it/resources-noncompilable/module-info.java",
            "module bogus.fixture.mod {}\n",
        );
        tmp_mixed.write(
            "sub/src/main/resources-filtered/Filtered.java",
            "class Filtered {}\n",
        );

        let probe_mixed = probe_workspace(&tmp_mixed.0);
        assert_eq!(probe_mixed.file_paths.len(), 1);
        assert!(probe_mixed
            .file_paths
            .contains_key("src/main/java/com/example/App.java"));
        assert!(probe_mixed
            .layout
            .modules
            .iter()
            .all(|m| m.module_name.is_none()));
        assert!(probe_mixed
            .layout
            .modules
            .iter()
            .all(|m| m.resources.is_empty()));
    }

    #[test]
    fn ant_property_file_expands_javac_release_and_ivy_dependencies() {
        let tmp = TempDir::new("ant_prop_file_ivy");
        tmp.write(
            "version.properties",
            r#"
# Line comment with hash
! Line comment with bang

java.ver=17
junit.org: org.junit.jupiter
junit.version = 5.10.2
already.set=99
"#,
        );
        tmp.write(
            "build.xml",
            r#"<project name="demo" default="compile" basedir=".">
                <property name="already.set" value="21"/>
                <property name="props.file" value="version.properties"/>
                <property file="${props.file}"/>
                <property name="java.ver" value="8"/>
                <target name="compile">
                    <javac srcdir="src" destdir="build/classes" release="${java.ver}"/>
                </target>
            </project>"#,
        );
        tmp.write(
            "ivy.xml",
            r#"<ivy-module version="2.0">
                <info organisation="com.example" module="demo"/>
                <dependencies>
                    <dependency org="${junit.org}" name="junit-jupiter" rev="${junit.version}" conf="test->default"/>
                    <dependency org="com.example" name="pinned" rev="${already.set}" conf="compile->default"/>
                </dependencies>
            </ivy-module>"#,
        );
        tmp.write(
            "src/com/example/App.java",
            "package com.example;\npublic class App {}\n",
        );

        let probe = probe_workspace(&tmp.0);
        assert!(probe.conflicts.is_empty(), "conflicts: {:?}", probe.conflicts);
        assert_eq!(probe.resolved_build, BuildToolKind::Ant);
        assert_eq!(
            probe.resolved_level,
            LanguageLevel::new(Release::JAVA_17, false)
        );

        let root_mod = &probe.layout.modules[0];
        assert!(
            root_mod.dependencies.contains(&ModuleDependency::Maven {
                group: "org.junit.jupiter".to_string(),
                artifact: "junit-jupiter".to_string(),
                version: "5.10.2".to_string(),
                classifier: None,
                scope: RootScope::Test,
            }),
            "expected expanded ivy dependency from version.properties: {:?}",
            root_mod.dependencies
        );
        assert!(
            root_mod.dependencies.contains(&ModuleDependency::Maven {
                group: "com.example".to_string(),
                artifact: "pinned".to_string(),
                version: "21".to_string(),
                classifier: None,
                scope: RootScope::Main,
            }),
            "expected existing Ant property not to be overwritten by property file: {:?}",
            root_mod.dependencies
        );
    }

    #[test]
    fn ant_fileset_scoped_to_path_and_classpath_and_ignores_macrodef_placeholders() {
        let tmp = TempDir::new("ant_fileset_scope");
        tmp.write(
            "build.xml",
            r#"<project name="demo" default="compile" basedir=".">
                <path id="cp">
                    <fileset dir="lib" includes="*.jar"/>
                </path>
                <macrodef name="compile-macro">
                    <attribute name="filesDir"/>
                    <attribute name="cpDir"/>
                    <sequential>
                        <javac srcdir="@{filesDir}" classpath="@{cpDir}">
                            <classpath>
                                <fileset dir="@{filesDir}"/>
                            </classpath>
                        </javac>
                    </sequential>
                </macrodef>
                <target name="dist">
                    <fileset dir="manual"/>
                </target>
                <target name="compile">
                    <javac srcdir="src" destdir="build/classes" release="17">
                        <classpath refid="cp"/>
                        <classpath>
                            <fileset dir="ext-lib" includes="*.jar"/>
                        </classpath>
                    </javac>
                </target>
            </project>"#,
        );
        tmp.write("lib/dep.jar", "");
        tmp.write("ext-lib/extra.jar", "");
        tmp.write("manual/doc.jar", "");
        tmp.write(
            "src/com/example/App.java",
            "package com.example;\npublic class App {}\n",
        );

        let probe = probe_workspace(&tmp.0);
        let root_mod = &probe.layout.modules[0];

        assert!(
            root_mod.dependencies.contains(&ModuleDependency::Path {
                path: "lib/dep.jar".to_string(),
                scope: RootScope::Main,
            }),
            "expected <path><fileset dir='lib'/></path> to be extracted: {:?}",
            root_mod.dependencies
        );
        assert!(
            root_mod.dependencies.contains(&ModuleDependency::Path {
                path: "ext-lib/extra.jar".to_string(),
                scope: RootScope::Main,
            }),
            "expected <classpath><fileset dir='ext-lib'/></classpath> to be extracted: {:?}",
            root_mod.dependencies
        );
        assert!(
            !root_mod.dependencies.iter().any(|d| match d {
                ModuleDependency::Path { path, .. } => {
                    path.contains("manual") || path.contains("@{")
                }
                _ => false,
            }),
            "outside-classpath <fileset> and @{{...}} placeholders must be ignored: {:?}",
            root_mod.dependencies
        );
        assert!(
            !root_mod
                .sources
                .iter()
                .any(|s| s.rel_path.contains("@{")),
            "macrodef @{{...}} srcdir must be ignored: {:?}",
            root_mod.sources
        );
    }
}
