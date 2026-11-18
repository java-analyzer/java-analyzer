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
    let tier1_root_excludes: Vec<String> = explicit_config
        .as_ref()
        .and_then(|c| c.delta.modules.get("."))
        .map(|m| m.dependencies.exclude.clone())
        .unwrap_or_default();

    let all_modules_snapshot = layout.modules.clone();
    let root_idx = layout
        .modules
        .iter()
        .position(|m| m.rel_path == ".")
        .or_else(|| if layout.modules.is_empty() { None } else { Some(0) });
    if let Some(idx) = root_idx {
        let root_mod = &mut layout.modules[idx];
        for dep in manifest_meta.dependencies {
            let excluded = tier1_root_excludes
                .iter()
                .any(|f| dep.matches_filter(f, &all_modules_snapshot));
            if !excluded && !root_mod.dependencies.contains(&dep) {
                root_mod.dependencies.push(dep);
            }
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
}

fn introspect_manifests(
    workspace_dir: &Path,
    build_specs: &[DetectedBuildSpec],
    version_specs: &mut Vec<DetectedVersionSpec>,
) -> ManifestMetadata {
    let mut meta = ManifestMetadata::default();

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
                introspect_ant_build_xml(
                    workspace_dir,
                    &content,
                    &spec.manifest_path,
                    version_specs,
                    &mut meta,
                );
            }
            "ivy.xml" => {
                introspect_ivy_xml(&content, &mut meta);
            }
            _ => {}
        }
    }

    meta
}

fn introspect_maven_pom(
    content: &str,
    path: &Path,
    version_specs: &mut Vec<DetectedVersionSpec>,
    meta: &mut ManifestMetadata,
) {
    let preview = content.contains("<enablePreview>true</enablePreview>")
        || content.contains("--enable-preview");

    for tag in [
        "maven.compiler.release",
        "maven.compiler.source",
        "java.version",
        "release",
    ] {
        if let Some(val) = extract_xml_tag_text(content, tag) {
            if let Some(release) = parse_version_string(&val) {
                version_specs.push(DetectedVersionSpec {
                    source: "pom.xml".to_string(),
                    path: path.to_path_buf(),
                    release,
                    preview,
                });
                break;
            }
        }
    }

    if let Some(src_dir) = extract_xml_tag_text(content, "sourceDirectory") {
        if !src_dir.contains("${") {
            meta.extra_source_dirs
                .push((normalize_rel_path(&src_dir), RootScope::Main));
        }
    }
    if let Some(test_dir) = extract_xml_tag_text(content, "testSourceDirectory") {
        if !test_dir.contains("${") {
            meta.extra_source_dirs
                .push((normalize_rel_path(&test_dir), RootScope::Test));
        }
    }

    // Extract compile-time <dependency> blocks (Main: default/compile/provided; Test: test; skip runtime)
    let mut rest = content;
    while let Some(start) = rest.find("<dependency>") {
        let after = &rest[start + "<dependency>".len()..];
        let Some(end) = after.find("</dependency>") else {
            break;
        };
        let block = &after[..end];
        rest = &after[end + "</dependency>".len()..];

        let group = extract_xml_tag_text(block, "groupId");
        let artifact = extract_xml_tag_text(block, "artifactId");
        let version = extract_xml_tag_text(block, "version").unwrap_or_else(|| "*".to_string());
        let classifier = extract_xml_tag_text(block, "classifier");
        let scope_str = extract_xml_tag_text(block, "scope").unwrap_or_default();

        if let (Some(g), Some(a)) = (group, artifact) {
            let scope = match scope_str.trim() {
                "" | "compile" | "provided" => Some(RootScope::Main),
                "test" => Some(RootScope::Test),
                // Ignore runtime/import/system deployment-only scopes
                _ => None,
            };
            if let Some(scope) = scope {
                let dep = ModuleDependency::Maven {
                    group: g.trim().to_string(),
                    artifact: a.trim().to_string(),
                    version: version.trim().to_string(),
                    classifier,
                    scope,
                };
                if !meta.dependencies.contains(&dep) {
                    meta.dependencies.push(dep);
                }
            }
        }
    }
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

fn introspect_ant_build_xml(
    workspace_dir: &Path,
    content: &str,
    path: &Path,
    version_specs: &mut Vec<DetectedVersionSpec>,
    meta: &mut ManifestMetadata,
) {
    // 1. Collect <property name="..." value="..." / location="..."/>
    let mut props: BTreeMap<String, String> = BTreeMap::new();
    props.insert("basedir".to_string(), ".".to_string());
    for tag in find_xml_start_tags(content, "property") {
        if let Some(name) = extract_xml_attr(&tag, "name") {
            if let Some(val) =
                extract_xml_attr(&tag, "value").or_else(|| extract_xml_attr(&tag, "location"))
            {
                let resolved = expand_ant_props(&val, &props);
                props.insert(name, resolved);
            }
        }
    }

    // 2. Inspect <javac ...> tags for srcdir, release/source/target, classpath
    let mut found_release = None;
    for javac_tag in find_xml_start_tags(content, "javac") {
        for attr in ["release", "source", "target"] {
            if let Some(raw_ver) = extract_xml_attr(&javac_tag, attr) {
                let expanded = expand_ant_props(&raw_ver, &props);
                if let Some(rel) = parse_version_string(&expanded) {
                    found_release = Some(rel);
                    break;
                }
            }
        }
        if let Some(srcdir) = extract_xml_attr(&javac_tag, "srcdir") {
            let expanded = expand_ant_props(&srcdir, &props);
            for part in expanded.split([':', ';']) {
                let norm = normalize_rel_path(part.trim());
                if norm != "." && !norm.contains("${") {
                    let scope = RootScope::from_path(&norm);
                    if !meta.extra_source_dirs.iter().any(|(p, _)| p == &norm) {
                        meta.extra_source_dirs.push((norm, scope));
                    }
                }
            }
        }
        if let Some(cp) = extract_xml_attr(&javac_tag, "classpath") {
            let expanded = expand_ant_props(&cp, &props);
            for part in expanded.split([':', ';']) {
                let norm = normalize_rel_path(part.trim());
                if norm != "." && !norm.contains("${") {
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
    for src_tag in find_xml_start_tags(content, "src") {
        if let Some(p) =
            extract_xml_attr(&src_tag, "path").or_else(|| extract_xml_attr(&src_tag, "location"))
        {
            let expanded = expand_ant_props(&p, &props);
            let norm = normalize_rel_path(&expanded);
            if norm != "." && !norm.contains("${") {
                let scope = RootScope::from_path(&norm);
                if !meta.extra_source_dirs.iter().any(|(d, _)| d == &norm) {
                    meta.extra_source_dirs.push((norm, scope));
                }
            }
        }
    }

    // Inspect <fileset dir="..."/> for compile-time JAR directories
    for fs_tag in find_xml_start_tags(content, "fileset") {
        if let Some(dir) = extract_xml_attr(&fs_tag, "dir") {
            let expanded = expand_ant_props(&dir, &props);
            let norm = normalize_rel_path(&expanded);
            if norm != "." && !norm.contains("${") {
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

    if let Some(release) = found_release {
        version_specs.push(DetectedVersionSpec {
            source: "build.xml".to_string(),
            path: path.to_path_buf(),
            release,
            preview: content.contains("--enable-preview"),
        });
    }
}

fn introspect_ivy_xml(content: &str, meta: &mut ManifestMetadata) {
    for dep_tag in find_xml_start_tags(content, "dependency") {
        let org = extract_xml_attr(&dep_tag, "org");
        let name = extract_xml_attr(&dep_tag, "name");
        let rev = extract_xml_attr(&dep_tag, "rev").unwrap_or_else(|| "*".to_string());
        let conf = extract_xml_attr(&dep_tag, "conf").unwrap_or_default();

        if let (Some(org), Some(name)) = (org, name) {
            if let Some(scope) = classify_ivy_conf(&conf) {
                let dep = ModuleDependency::Maven {
                    group: org.trim().to_string(),
                    artifact: name.trim().to_string(),
                    version: rev.trim().to_string(),
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

fn expand_ant_props(input: &str, props: &BTreeMap<String, String>) -> String {
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

fn find_xml_start_tags(xml: &str, tag: &str) -> Vec<String> {
    let mut results = Vec::new();
    let needle = format!("<{tag}");
    let mut rest = xml;
    while let Some(idx) = rest.find(&needle) {
        let after_name = &rest[idx + needle.len()..];
        let next_ch = after_name.chars().next().unwrap_or(' ');
        if next_ch.is_whitespace() || next_ch == '>' || next_ch == '/' {
            let slice = &rest[idx..];
            let mut in_quote: Option<char> = None;
            let mut end_pos = None;
            for (offset, ch) in slice.char_indices() {
                match in_quote {
                    Some(q) if ch == q => in_quote = None,
                    Some(_) => {}
                    None if ch == '"' || ch == '\'' => in_quote = Some(ch),
                    None if ch == '>' => {
                        end_pos = Some(offset);
                        break;
                    }
                    None => {}
                }
            }
            if let Some(end) = end_pos {
                results.push(slice[..=end].to_string());
                rest = &slice[end + 1..];
                continue;
            }
        }
        rest = &rest[idx + needle.len()..];
    }
    results
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

fn scan_workspace_fs(
    root: &Path,
    current: &Path,
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
            // Strong resource root indicator: META-INF/ directory containing MANIFEST.MF or services/
            if name_str == "META-INF"
                && (path.join("MANIFEST.MF").is_file() || path.join("services").is_dir())
            {
                if let Ok(rel_parent) = current.strip_prefix(root) {
                    let norm = normalize_rel_path(&rel_parent.to_string_lossy());
                    resource_dirs.insert(norm);
                }
            }
            // Conventional resource directory names
            if name_str == "resources" {
                if let Ok(rel_res) = path.strip_prefix(root) {
                    let norm = normalize_rel_path(&rel_res.to_string_lossy());
                    resource_dirs.insert(norm);
                }
            }
            scan_workspace_fs(root, &path, java_files, resource_dirs);
        } else if path.is_file() && name_str.ends_with(".java") {
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
        if let Ok(rel) = file_path.strip_prefix(&self.workspace_dir) {
            let norm = normalize_rel_path(&rel.to_string_lossy());
            for module in &self.layout.modules {
                let in_mod = if module.rel_path == "." {
                    true
                } else {
                    norm.starts_with(&format!("{}/", module.rel_path))
                };
                if in_mod {
                    return module.language_level;
                }
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
}
