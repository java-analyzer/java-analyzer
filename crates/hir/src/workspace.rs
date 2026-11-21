//! Build-tool-agnostic workspace, module, source/resource root, and compile-time dependency model.
//! See `docs/specs/0004-lightweight-project-and-workspace-model.md`.
//!
//! Everything in this module is pure and IO-free: it operates on normalized relative paths and
//! parsed [`SyntaxNode`] trees.

use std::collections::{BTreeMap, BTreeSet};

use syntax::{
    ext::path_text,
    syntax_kind::SyntaxKind::*,
    LanguageLevel, Release, SyntaxNode, TextRange,
};

use crate::FileId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceRootId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceRootId(pub u32);

/// Compile-time scope of a source root, resource root, or module dependency inside a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RootScope {
    Main,
    Test,
}

impl RootScope {
    /// Classifies a normalized workspace-relative directory path as [`RootScope::Main`] or
    /// [`RootScope::Test`].
    pub fn from_path(rel_path: &str) -> Self {
        let norm = normalize_rel_path(rel_path);
        let mut prev_was_src = false;
        for seg in norm.split('/') {
            if seg == "test" || seg == "tests" {
                return Self::Test;
            }
            if prev_was_src && seg.to_ascii_lowercase().contains("test") {
                return Self::Test;
            }
            prev_was_src = seg == "src";
        }
        Self::Main
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRoot {
    pub id: SourceRootId,
    /// Normalized workspace-relative directory path (`"."`, `"src/main/java"`, `"services/core/src"`).
    pub rel_path: String,
    pub scope: RootScope,
    pub files: Vec<FileId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRoot {
    pub id: ResourceRootId,
    /// Normalized workspace-relative directory path (`"src/main/resources"`, `"resources"`, etc.).
    pub rel_path: String,
    pub scope: RootScope,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModuleTarget {
    Workspace(ModuleId),
    External(String),
}

/// Compile-time dependency of a module. Runtime/deployment/packaging dependencies are out of scope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModuleDependency {
    /// Another module in the workspace (by `ModuleId`) or an external/JDK JPMS module by name.
    Module {
        target: ModuleTarget,
        scope: RootScope,
        /// True for `requires transitive` or Gradle `api` dependencies re-exported to dependents.
        exported: bool,
    },
    /// Workspace-relative or filesystem path to a `.jar` or class directory (`"lib/foo.jar"`).
    Path {
        path: String,
        scope: RootScope,
    },
    /// Maven or Ivy coordinate (`group:artifact:version[:classifier]`).
    Maven {
        group: String,
        artifact: String,
        version: String,
        classifier: Option<String>,
        scope: RootScope,
    },
    /// GitHub, GitLab, or Git repository reference (`github:owner/repo#ref`, `gitlab:owner/repo#ref`).
    Git {
        repo: String,
        reference: Option<String>,
        scope: RootScope,
    },
}

impl ModuleDependency {
    /// Parses a dependency specifier string into a [`ModuleDependency`], resolving workspace
    /// module references (by `rel_path` or JPMS `module_name`) against `workspace_modules`.
    pub fn parse(
        raw: &str,
        scope: RootScope,
        workspace_modules: &[(ModuleId, &str, Option<&str>)],
    ) -> Self {
        let spec = raw.trim();

        // 1. Git repository references: `github:owner/repo#ref`, `gitlab:owner/repo#ref`, or `https://...git`.
        if spec.starts_with("github:")
            || spec.starts_with("gitlab:")
            || spec.starts_with("git+")
            || (spec.starts_with("https://") && spec.contains(".git"))
        {
            let (repo, reference) = match spec.split_once('#') {
                Some((r, rev)) => (r.to_string(), Some(rev.to_string())),
                None => (spec.to_string(), None),
            };
            return Self::Git {
                repo,
                reference,
                scope,
            };
        }

        // 2. Maven coordinates: `mvn:g:a:v[:c]` or `g:a:v[:c]`.
        let maven_candidate = spec.strip_prefix("mvn:").unwrap_or(spec);
        if !maven_candidate.contains('/') && !maven_candidate.contains('\\') {
            let parts: Vec<&str> = maven_candidate.split(':').collect();
            if (parts.len() == 3 || parts.len() == 4) && parts.iter().all(|p| !p.is_empty()) {
                return Self::Maven {
                    group: parts[0].to_string(),
                    artifact: parts[1].to_string(),
                    version: parts[2].to_string(),
                    classifier: parts.get(3).map(|c| (*c).to_string()),
                    scope,
                };
            }
        }

        // 3. Explicit path or relative path matching a workspace module or local JAR/directory.
        let path_candidate = spec.strip_prefix("path:").unwrap_or(spec);
        let norm_candidate = normalize_rel_path(path_candidate);
        for &(id, rel_path, jpms_name) in workspace_modules {
            if norm_candidate == rel_path || jpms_name == Some(spec) {
                return Self::Module {
                    target: ModuleTarget::Workspace(id),
                    scope,
                    exported: false,
                };
            }
        }

        if spec.starts_with("path:")
            || spec.starts_with("./")
            || spec.starts_with("../")
            || spec.contains('/')
            || spec.ends_with(".jar")
        {
            return Self::Path {
                path: norm_candidate,
                scope,
            };
        }

        // 4. Named JPMS / JDK module.
        Self::Module {
            target: ModuleTarget::External(spec.to_string()),
            scope,
            exported: false,
        }
    }

    /// Canonical string representation used for matching `exclude` filters in `.java-analyzer.toml`.
    pub fn matches_filter(&self, filter: &str, modules: &[ModuleData]) -> bool {
        let filter = filter.trim();
        let norm_filter = normalize_rel_path(filter.strip_prefix("path:").unwrap_or(filter));
        match self {
            Self::Module { target, .. } => match target {
                ModuleTarget::Workspace(id) => modules
                    .iter()
                    .find(|m| m.id == *id)
                    .is_some_and(|m| {
                        m.rel_path == norm_filter
                            || m.module_name.as_deref() == Some(filter)
                    }),
                ModuleTarget::External(name) => name == filter,
            },
            Self::Path { path, .. } => path == &norm_filter || path == filter,
            Self::Maven {
                group,
                artifact,
                version,
                ..
            } => {
                let gav = format!("{group}:{artifact}:{version}");
                let ga = format!("{group}:{artifact}");
                let stripped = filter.strip_prefix("mvn:").unwrap_or(filter);
                stripped == gav || stripped == ga
            }
            Self::Git {
                repo, reference, ..
            } => {
                if repo == filter {
                    return true;
                }
                if let Some(r) = reference {
                    format!("{repo}#{r}") == filter
                } else {
                    false
                }
            }
        }
    }
}

/// Additive and subtractive `{ include, exclude }` delta applied on top of an inferred list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeltaSet<T> {
    pub include: Vec<T>,
    pub exclude: Vec<T>,
}

impl<T: PartialEq + Clone> DeltaSet<T> {
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleDelta {
    pub release: Option<Release>,
    pub preview: Option<bool>,
    pub sources: DeltaSet<String>,
    pub resources: DeltaSet<String>,
    pub dependencies: DeltaSet<String>,
}

impl ModuleDelta {
    pub fn is_empty(&self) -> bool {
        self.release.is_none()
            && self.preview.is_none()
            && self.sources.is_empty()
            && self.resources.is_empty()
            && self.dependencies.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceDelta {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    /// Keyed by module `rel_path` (`"."`, `"services/core"`) or JPMS `module_name`.
    pub modules: BTreeMap<String, ModuleDelta>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleData {
    pub id: ModuleId,
    /// Normalized workspace-relative directory path (`"."` for the root module, `"services/core"`).
    pub rel_path: String,
    /// Declared JPMS module name from `module-info.java`, or `None` for an unnamed module.
    pub module_name: Option<String>,
    pub language_level: LanguageLevel,
    pub test_language_level: Option<LanguageLevel>,
    pub sources: Vec<SourceRoot>,
    pub resources: Vec<ResourceRoot>,
    pub dependencies: Vec<ModuleDependency>,
}

/// A compile error diagnostic produced when a `.java` file's `package` declaration disagrees with
/// its directory's authoritative `package-info.java` or with its relative path from its
/// enclosing [`SourceRoot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageMismatch {
    pub file_id: FileId,
    pub path: String,
    pub declared: Option<String>,
    pub expected: String,
    pub authoritative_package_info: bool,
    pub range: TextRange,
}

impl PackageMismatch {
    pub fn message(&self) -> String {
        let actual = self.declared.as_deref().unwrap_or("<unnamed>");
        if self.authoritative_package_info {
            format!(
                "declared package '{}' does not match package-info.java package '{}'",
                actual, self.expected
            )
        } else {
            format!(
                "declared package '{}' does not match expected package '{}'",
                actual, self.expected
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLayout {
    pub modules: Vec<ModuleData>,
    pub file_levels: BTreeMap<FileId, LanguageLevel>,
    pub package_mismatches: Vec<PackageMismatch>,
    /// Authoritative package name per normalized directory path, established by `package-info.java`.
    pub authoritative_packages: BTreeMap<String, String>,
}

impl WorkspaceLayout {
    pub fn module_by_path(&self, rel_path: &str) -> Option<&ModuleData> {
        let norm = normalize_rel_path(rel_path);
        self.modules.iter().find(|m| m.rel_path == norm)
    }

    pub fn module_by_name(&self, name: &str) -> Option<&ModuleData> {
        let norm = normalize_rel_path(name);
        self.modules
            .iter()
            .find(|m| m.module_name.as_deref() == Some(name) || m.rel_path == norm)
    }

    /// Checks a live parsed file's `package` declaration against this layout's authoritative
    /// `package-info.java` entries and enclosing [`SourceRoot`]s without re-scanning the workspace.
    pub fn check_file_package(
        &self,
        file_id: FileId,
        rel_path: &str,
        root: &SyntaxNode,
    ) -> Option<PackageMismatch> {
        let path = normalize_rel_path(rel_path);
        let name = file_name(&path);
        if name == "module-info.java" || name == "package-info.java" {
            return None;
        }

        let dir = parent_dir(&path);
        let header = extract_file_header(root);

        if let Some(auth_pkg) = self.authoritative_packages.get(&dir) {
            if header.package.as_deref() != Some(auth_pkg.as_str()) {
                return Some(PackageMismatch {
                    file_id,
                    path,
                    declared: header.package,
                    expected: auth_pkg.clone(),
                    authoritative_package_info: true,
                    range: header.package_range.unwrap_or_default(),
                });
            }
            return None;
        }

        let enclosing_root = self
            .modules
            .iter()
            .flat_map(|m| m.sources.iter())
            .map(|s| s.rel_path.as_str())
            .filter(|r| *r == "." || dir == *r || dir.starts_with(&format!("{r}/")))
            .max_by_key(|r| if *r == "." { 0 } else { r.len() });

        if let Some(src_root) = enclosing_root {
            let expected_pkg = if dir == src_root {
                String::new()
            } else if src_root == "." {
                dir.replace('/', ".")
            } else {
                dir.strip_prefix(&format!("{src_root}/"))
                    .unwrap_or(&dir)
                    .replace('/', ".")
            };
            if let Some(declared) = header.package {
                if declared != expected_pkg {
                    return Some(PackageMismatch {
                        file_id,
                        path,
                        declared: Some(declared),
                        expected: expected_pkg,
                        authoritative_package_info: false,
                        range: header.package_range.unwrap_or_default(),
                    });
                }
            }
            return None;
        }

        if let Some(declared) = header.package {
            if strip_package_suffix(&dir, &declared).is_none() {
                return Some(PackageMismatch {
                    file_id,
                    path,
                    declared: Some(declared),
                    expected: String::new(),
                    authoritative_package_info: false,
                    range: header.package_range.unwrap_or_default(),
                });
            }
        }

        None
    }
}

// ---- Header extraction from a parsed CST ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiresDirective {
    pub name: String,
    pub transitive: bool,
    pub is_static: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleHeader {
    pub name: String,
    pub open: bool,
    pub requires: Vec<RequiresDirective>,
    pub exports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeader {
    pub package: Option<String>,
    pub package_range: Option<TextRange>,
    pub module: Option<ModuleHeader>,
    pub shebang_level: Option<LanguageLevel>,
    pub jbang_deps: Vec<String>,
}

/// Extracts package, module, shebang `--source`/`--release`, and `//DEPS` metadata from a
/// file's [`SyntaxNode`] root.
pub fn extract_file_header(root: &SyntaxNode) -> FileHeader {
    let mut package = None;
    let mut package_range = None;
    let mut module = None;
    let mut shebang_level = None;
    let mut jbang_deps = Vec::new();

    for token in root
        .children_with_tokens()
        .filter_map(|e| e.into_token())
    {
        match token.kind() {
            SHEBANG => {
                shebang_level = syntax::parse_shebang_level(token.text());
            }
            LINE_COMMENT => {
                let body = token.text().trim_start_matches('/').trim();
                if let Some(rest) = body.strip_prefix("DEPS ") {
                    for coord in rest.split_whitespace() {
                        if !coord.is_empty() {
                            jbang_deps.push(coord.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    for child in root.children() {
        match child.kind() {
            PACKAGE_DECL => {
                if let Some(path_node) = child.children().find(|n| n.kind() == PATH) {
                    package = Some(path_text(&path_node));
                    package_range = Some(path_node.text_range());
                }
            }
            MODULE_DECL => {
                module = parse_module_decl(&child);
            }
            _ => {}
        }
    }

    FileHeader {
        package,
        package_range,
        module,
        shebang_level,
        jbang_deps,
    }
}

fn parse_module_decl(node: &SyntaxNode) -> Option<ModuleHeader> {
    let name = node
        .children()
        .find(|n| n.kind() == PATH)
        .map(|p| path_text(&p))?;
    let open = node
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == IDENT && t.text() == "open");
    let mut requires = Vec::new();
    let mut exports = Vec::new();

    if let Some(body) = node.children().find(|n| n.kind() == MODULE_BODY) {
        for directive in body.children().filter(|n| n.kind() == MODULE_DIRECTIVE) {
            let mut kw = None;
            let mut transitive = false;
            let mut is_static = false;
            for tok in directive
                .children_with_tokens()
                .filter_map(|e| e.into_token())
            {
                match tok.kind() {
                    STATIC_KW => is_static = true,
                    IDENT if kw.is_none() => kw = Some(tok.text().to_string()),
                    IDENT if tok.text() == "transitive" => transitive = true,
                    _ => {}
                }
            }
            let first_path = directive
                .children()
                .find(|n| n.kind() == PATH)
                .map(|p| path_text(&p));
            match (kw.as_deref(), first_path) {
                (Some("requires"), Some(target)) => requires.push(RequiresDirective {
                    name: target,
                    transitive,
                    is_static,
                }),
                (Some("exports"), Some(pkg)) => exports.push(pkg),
                _ => {}
            }
        }
    }

    Some(ModuleHeader {
        name,
        open,
        requires,
        exports,
    })
}

// ---- Path normalization & zero-config layout inference ----

pub fn normalize_rel_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut parts = Vec::new();
    for seg in normalized.split('/') {
        match seg {
            "" | "." => {}
            _ => parts.push(seg),
        }
    }
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

fn parent_dir(rel_file_path: &str) -> String {
    let norm = normalize_rel_path(rel_file_path);
    match norm.rsplit_once('/') {
        Some((dir, _)) => normalize_rel_path(dir),
        None => ".".to_string(),
    }
}

fn file_name(rel_file_path: &str) -> &str {
    match rel_file_path.rsplit_once(['/', '\\']) {
        Some((_, name)) => name,
        None => rel_file_path,
    }
}

/// Strips a dot-separated package name (`a.b.c`) from the end of a normalized directory path
/// (`src/main/java/a/b/c`), returning `Some("src/main/java")` (or `Some(".")` when the package
/// spans the entire directory path) if the suffix matches.
fn strip_package_suffix(dir: &str, package: &str) -> Option<String> {
    if package.is_empty() {
        return Some(normalize_rel_path(dir));
    }
    let pkg_path = package.replace('.', "/");
    let norm_dir = normalize_rel_path(dir);
    if norm_dir == pkg_path {
        Some(".".to_string())
    } else {
        norm_dir
            .strip_suffix(&format!("/{pkg_path}"))
            .map(normalize_rel_path)
    }
}

/// Derives the enclosing module directory `rel_path` from a Java source or resource root directory.
/// For standard layouts (`<mod>/src/main/java`, `<mod>/src/test/java`, `<mod>/src/<sourceSet>/java`,
/// `<mod>/src/java`, `<mod>/test/java`, `<mod>/src`, `<mod>/test`), returns `<mod>` (or `"."` at
/// the workspace root); otherwise returns the root directory itself.
pub fn module_dir_for_root(root_dir: &str) -> String {
    let norm = normalize_rel_path(root_dir);
    if norm.starts_with("src/main/")
        || norm.starts_with("src/test/")
        || matches!(
            norm.as_str(),
            "src/main"
                | "src/test"
                | "src/java"
                | "test/java"
                | "src"
                | "test"
                | "tests"
                | "resources"
        )
    {
        return ".".to_string();
    }
    if let Some((prefix, _)) = norm
        .split_once("/src/main/")
        .or_else(|| norm.split_once("/src/test/"))
    {
        return normalize_rel_path(prefix);
    }
    for suffix in ["/src/main", "/src/test", "/src/java", "/test/java"] {
        if let Some(stripped) = norm.strip_suffix(suffix) {
            return normalize_rel_path(stripped);
        }
    }
    let segments: Vec<&str> = norm.split('/').collect();
    if segments.len() >= 3
        && segments[segments.len() - 3] == "src"
        && matches!(segments[segments.len() - 1], "java" | "resources")
    {
        if segments.len() == 3 {
            return ".".to_string();
        }
        return normalize_rel_path(&segments[..segments.len() - 3].join("/"));
    }
    for suffix in ["/src", "/test", "/tests", "/resources"] {
        if let Some(stripped) = norm.strip_suffix(suffix) {
            return normalize_rel_path(stripped);
        }
    }
    norm
}

fn is_excluded(rel_path: &str, excludes: &[String]) -> bool {
    let norm = normalize_rel_path(rel_path);
    excludes.iter().any(|ex| {
        let ex_norm = normalize_rel_path(ex);
        norm == ex_norm || norm.starts_with(&format!("{ex_norm}/"))
    })
}

/// Parses raw `(FileId, rel_path, source_text)` entries at `default_level` and delegates to
/// [`infer_workspace_layout`].
pub fn infer_workspace_layout_from_sources(
    files: &[(FileId, &str, &str)],
    candidate_resources: &[&str],
    default_level: LanguageLevel,
    delta: &WorkspaceDelta,
) -> WorkspaceLayout {
    let parsed: Vec<(FileId, &str, SyntaxNode)> = files
        .iter()
        .map(|&(id, rel_path, text)| {
            let parse = syntax::parse_with_level(text, default_level);
            (id, rel_path, parse.syntax_node())
        })
        .collect();
    let refs: Vec<(FileId, &str, &SyntaxNode)> = parsed
        .iter()
        .map(|(id, rel_path, node)| (*id, *rel_path, node))
        .collect();
    infer_workspace_layout(&refs, candidate_resources, default_level, delta)
}

/// Pure zero-config workspace layout inference from `.java` files, `package-info.java`,
/// `module-info.java`, and candidate resource directories (including `META-INF/` indicators),
/// followed by applying `delta` (`{ include, exclude }`).
pub fn infer_workspace_layout(
    files: &[(FileId, &str, &SyntaxNode)],
    candidate_resources: &[&str],
    default_level: LanguageLevel,
    delta: &WorkspaceDelta,
) -> WorkspaceLayout {
    struct ParsedFile {
        id: FileId,
        path: String,
        dir: String,
        name: String,
        header: FileHeader,
    }

    let mut parsed_files = Vec::new();
    let mut file_levels = BTreeMap::new();

    for &(id, raw_path, root) in files {
        let path = normalize_rel_path(raw_path);
        if is_excluded(&path, &delta.exclude) {
            continue;
        }
        let dir = parent_dir(&path);
        let name = file_name(&path).to_string();
        let header = extract_file_header(root);
        let level = header.shebang_level.unwrap_or(default_level);
        file_levels.insert(id, level);
        parsed_files.push(ParsedFile {
            id,
            path,
            dir,
            name,
            header,
        });
    }

    // 1. Collect `module-info.java` roots and `package-info.java` authoritative directory packages.
    let mut module_headers_by_mod_dir: BTreeMap<String, (RootScope, ModuleHeader)> =
        BTreeMap::new();
    let mut known_source_roots: BTreeSet<String> = BTreeSet::new();
    let mut authoritative_dir_pkg: BTreeMap<String, String> = BTreeMap::new();

    for pf in &parsed_files {
        if pf.name == "module-info.java" {
            known_source_roots.insert(pf.dir.clone());
            let mod_dir = module_dir_for_root(&pf.dir);
            let scope = RootScope::from_path(&pf.dir);
            if let Some(mh) = &pf.header.module {
                let should_insert = match module_headers_by_mod_dir.get(&mod_dir) {
                    None => true,
                    Some((existing_scope, _)) => {
                        *existing_scope == RootScope::Test && scope == RootScope::Main
                    }
                };
                if should_insert {
                    module_headers_by_mod_dir.insert(mod_dir, (scope, mh.clone()));
                }
            }
        } else if pf.name == "package-info.java" {
            if let Some(pkg) = &pf.header.package {
                authoritative_dir_pkg.insert(pf.dir.clone(), pkg.clone());
                if let Some(src_root) = strip_package_suffix(&pf.dir, pkg) {
                    known_source_roots.insert(src_root);
                }
            }
        }
    }

    // 2. Consensus source-root inference per directory from regular `.java` files.
    let mut dir_root_votes: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for pf in &parsed_files {
        if pf.name == "module-info.java" || pf.name == "package-info.java" {
            continue;
        }
        if authoritative_dir_pkg.contains_key(&pf.dir) {
            continue;
        }
        if let Some(pkg) = &pf.header.package {
            if let Some(root) = strip_package_suffix(&pf.dir, pkg) {
                *dir_root_votes
                    .entry(pf.dir.clone())
                    .or_default()
                    .entry(root)
                    .or_default() += 1;
            }
        }
    }
    for votes in dir_root_votes.values() {
        if let Some((best_root, _)) = votes
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.len().cmp(&a.0.len())))
        {
            known_source_roots.insert(best_root.clone());
        }
    }

    // Helper: find the most specific enclosing known source root for `dir`.
    let find_enclosing_root = |dir: &str, roots: &BTreeSet<String>| -> Option<String> {
        roots
            .iter()
            .filter(|r| {
                *r == "." || dir == r.as_str() || dir.starts_with(&format!("{r}/"))
            })
            .max_by_key(|r| if *r == "." { 0 } else { r.len() })
            .cloned()
    };

    // 3. Assign each file to a source root and check package consistency.
    let mut files_by_source_root: BTreeMap<String, Vec<FileId>> = BTreeMap::new();
    let mut jbang_deps_by_mod_dir: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut package_mismatches = Vec::new();

    for pf in &parsed_files {
        let src_root = if let Some(auth_pkg) = authoritative_dir_pkg.get(&pf.dir) {
            let root = strip_package_suffix(&pf.dir, auth_pkg)
                .or_else(|| find_enclosing_root(&pf.dir, &known_source_roots))
                .unwrap_or_else(|| pf.dir.clone());
            if pf.name != "package-info.java"
                && pf.name != "module-info.java"
                && pf.header.package.as_deref() != Some(auth_pkg.as_str())
            {
                package_mismatches.push(PackageMismatch {
                    file_id: pf.id,
                    path: pf.path.clone(),
                    declared: pf.header.package.clone(),
                    expected: auth_pkg.clone(),
                    authoritative_package_info: true,
                    range: pf.header.package_range.unwrap_or_default(),
                });
            }
            root
        } else if let Some(root) = find_enclosing_root(&pf.dir, &known_source_roots) {
            if pf.name != "module-info.java" {
                let expected_pkg = if pf.dir == root {
                    String::new()
                } else if root == "." {
                    pf.dir.replace('/', ".")
                } else {
                    pf.dir
                        .strip_prefix(&format!("{root}/"))
                        .unwrap_or(&pf.dir)
                        .replace('/', ".")
                };
                if let Some(declared) = &pf.header.package {
                    if *declared != expected_pkg {
                        package_mismatches.push(PackageMismatch {
                            file_id: pf.id,
                            path: pf.path.clone(),
                            declared: Some(declared.clone()),
                            expected: expected_pkg,
                            authoritative_package_info: false,
                            range: pf.header.package_range.unwrap_or_default(),
                        });
                    }
                }
            }
            root
        } else if let Some(declared) = &pf.header.package {
            // File declared a package that does not match its directory path and has no enclosing
            // consensus root: use its directory as the fallback source root and flag the mismatch.
            package_mismatches.push(PackageMismatch {
                file_id: pf.id,
                path: pf.path.clone(),
                declared: Some(declared.clone()),
                expected: String::new(),
                authoritative_package_info: false,
                range: pf.header.package_range.unwrap_or_default(),
            });
            pf.dir.clone()
        } else {
            pf.dir.clone()
        };

        files_by_source_root
            .entry(src_root.clone())
            .or_default()
            .push(pf.id);

        let mod_dir = module_dir_for_root(&src_root);
        for dep in &pf.header.jbang_deps {
            let entry = jbang_deps_by_mod_dir.entry(mod_dir.clone()).or_default();
            if !entry.contains(dep) {
                entry.push(dep.clone());
            }
        }
    }

    // 4. Group source roots and resource roots into modules by `mod_dir`.
    let mut mod_source_roots: BTreeMap<String, BTreeMap<String, Vec<FileId>>> = BTreeMap::new();
    for (src_root, file_ids) in files_by_source_root {
        let mod_dir = module_dir_for_root(&src_root);
        mod_source_roots
            .entry(mod_dir)
            .or_default()
            .insert(src_root, file_ids);
    }

    for inc in &delta.include {
        let norm = normalize_rel_path(inc);
        if !is_excluded(&norm, &delta.exclude) {
            let mod_dir = module_dir_for_root(&norm);
            mod_source_roots
                .entry(mod_dir)
                .or_default()
                .entry(norm)
                .or_default();
        }
    }

    let mut mod_resource_roots: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for &raw_res in candidate_resources {
        let res_dir = normalize_rel_path(raw_res);
        if is_excluded(&res_dir, &delta.exclude) {
            continue;
        }
        // Attach resource root to the most specific existing module, or derive its module dir.
        let derived = module_dir_for_root(&res_dir);
        let target_mod = if mod_source_roots.contains_key(&derived) {
            derived
        } else {
            mod_source_roots
                .keys()
                .filter(|m| {
                    *m == "." || res_dir == m.as_str() || res_dir.starts_with(&format!("{m}/"))
                })
                .max_by_key(|m| if *m == "." { 0 } else { m.len() })
                .cloned()
                .unwrap_or(derived)
        };
        mod_source_roots.entry(target_mod.clone()).or_default();
        mod_resource_roots
            .entry(target_mod)
            .or_default()
            .insert(res_dir);
    }

    // 5. Build initial `ModuleData` records and apply per-module `sources` and `resources` deltas.
    let mut modules = Vec::new();
    let mut next_src_id = 0u32;
    let mut next_res_id = 0u32;

    for (idx, (mod_dir, mut src_map)) in mod_source_roots.into_iter().enumerate() {
        let id = ModuleId(idx as u32);
        let mh = module_headers_by_mod_dir.get(&mod_dir).map(|(_, h)| h);
        let module_name = mh.map(|h| h.name.clone());

        let mod_delta = delta
            .modules
            .get(&mod_dir)
            .or_else(|| module_name.as_deref().and_then(|n| delta.modules.get(n)));

        if let Some(md) = mod_delta {
            src_map.retain(|path, _| !is_excluded(path, &md.sources.exclude));
            for inc in &md.sources.include {
                let norm = normalize_rel_path(inc);
                src_map.entry(norm).or_default();
            }
        }

        let mut sources = Vec::new();
        for (src_path, file_ids) in src_map {
            sources.push(SourceRoot {
                id: SourceRootId(next_src_id),
                scope: RootScope::from_path(&src_path),
                rel_path: src_path,
                files: file_ids,
            });
            next_src_id += 1;
        }

        let mut res_set = mod_resource_roots.remove(&mod_dir).unwrap_or_default();
        if let Some(md) = mod_delta {
            res_set.retain(|path| !is_excluded(path, &md.resources.exclude));
            for inc in &md.resources.include {
                res_set.insert(normalize_rel_path(inc));
            }
        }

        let mut resources = Vec::new();
        for res_path in res_set {
            resources.push(ResourceRoot {
                id: ResourceRootId(next_res_id),
                scope: RootScope::from_path(&res_path),
                rel_path: res_path,
            });
            next_res_id += 1;
        }

        let mut language_level = default_level;
        if let Some(md) = mod_delta {
            if let Some(r) = md.release {
                language_level.release = r;
            }
            if let Some(p) = md.preview {
                language_level.preview = p;
            }
        }

        modules.push(ModuleData {
            id,
            rel_path: mod_dir,
            module_name,
            language_level,
            test_language_level: None,
            sources,
            resources,
            dependencies: Vec::new(),
        });
    }

    // 6. Wire compile-time inter-module dependencies (`module-info.java` `requires`, `//DEPS`, and
    // per-module `dependencies` `{ include, exclude }` deltas).
    let module_index: Vec<(ModuleId, String, Option<String>)> = modules
        .iter()
        .map(|m| (m.id, m.rel_path.clone(), m.module_name.clone()))
        .collect();
    let lookup_slice: Vec<(ModuleId, &str, Option<&str>)> = module_index
        .iter()
        .map(|(id, rel, name)| (*id, rel.as_str(), name.as_deref()))
        .collect();

    for i in 0..modules.len() {
        let mod_dir = modules[i].rel_path.clone();
        let mod_name = modules[i].module_name.clone();
        let mut deps = Vec::new();

        if let Some((_, mh)) = module_headers_by_mod_dir.get(&mod_dir) {
            for req in &mh.requires {
                let target = lookup_slice
                    .iter()
                    .find(|(_, _, jpms)| *jpms == Some(req.name.as_str()))
                    .map(|(id, _, _)| ModuleTarget::Workspace(*id))
                    .unwrap_or_else(|| ModuleTarget::External(req.name.clone()));
                deps.push(ModuleDependency::Module {
                    target,
                    scope: RootScope::Main,
                    exported: req.transitive,
                });
            }
        }

        if let Some(jbang) = jbang_deps_by_mod_dir.get(&mod_dir) {
            for raw in jbang {
                let parsed = ModuleDependency::parse(raw, RootScope::Main, &lookup_slice);
                if !deps.contains(&parsed) {
                    deps.push(parsed);
                }
            }
        }

        let mod_delta = delta
            .modules
            .get(&mod_dir)
            .or_else(|| mod_name.as_deref().and_then(|n| delta.modules.get(n)));
        if let Some(md) = mod_delta {
            if !md.dependencies.exclude.is_empty() {
                deps.retain(|d| {
                    !md.dependencies
                        .exclude
                        .iter()
                        .any(|ex| d.matches_filter(ex, &modules))
                });
            }
            for inc in &md.dependencies.include {
                let parsed = ModuleDependency::parse(inc, RootScope::Main, &lookup_slice);
                if !deps.contains(&parsed) {
                    deps.push(parsed);
                }
            }
        }

        modules[i].dependencies = deps;
    }

    WorkspaceLayout {
        modules,
        file_levels,
        package_mismatches,
        authoritative_packages: authoritative_dir_pkg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_tree(src: &str) -> SyntaxNode {
        syntax::parse(src).syntax_node()
    }

    #[test]
    fn infers_source_roots_and_modules_from_packages_and_module_info() {
        let mod_a = parse_tree("module com.example.core { exports com.example.core; }");
        let core_cls = parse_tree("package com.example.core;\npublic class Core {}");
        let mod_b = parse_tree("module com.example.app { requires transitive com.example.core; requires java.sql; }");
        let app_cls = parse_tree("package com.example.app;\npublic class App {}");
        let app_test = parse_tree("package com.example.app;\npublic class AppTest {}");

        let files = [
            (FileId(1), "core/src/main/java/module-info.java", &mod_a),
            (
                FileId(2),
                "core/src/main/java/com/example/core/Core.java",
                &core_cls,
            ),
            (FileId(3), "app/src/main/java/module-info.java", &mod_b),
            (
                FileId(4),
                "app/src/main/java/com/example/app/App.java",
                &app_cls,
            ),
            (
                FileId(5),
                "app/src/test/java/com/example/app/AppTest.java",
                &app_test,
            ),
        ];
        let resources = ["core/src/main/resources", "app/assets"];

        let layout = infer_workspace_layout(
            &files,
            &resources,
            LanguageLevel::latest(),
            &WorkspaceDelta::default(),
        );

        assert_eq!(layout.modules.len(), 2);
        assert!(layout.package_mismatches.is_empty());

        let app = layout.module_by_name("com.example.app").unwrap();
        let core = layout.module_by_name("com.example.core").unwrap();
        assert_eq!(app.rel_path, "app");
        assert_eq!(core.rel_path, "core");

        // `app` has both Main and Test source roots inside the same module.
        let app_roots: Vec<(&str, RootScope)> = app
            .sources
            .iter()
            .map(|r| (r.rel_path.as_str(), r.scope))
            .collect();
        assert_eq!(
            app_roots,
            [
                ("app/src/main/java", RootScope::Main),
                ("app/src/test/java", RootScope::Test)
            ]
        );

        // `app` auto-wired `requires transitive com.example.core` to workspace `core.id`.
        assert_eq!(
            app.dependencies,
            vec![
                ModuleDependency::Module {
                    target: ModuleTarget::Workspace(core.id),
                    scope: RootScope::Main,
                    exported: true,
                },
                ModuleDependency::Module {
                    target: ModuleTarget::External("java.sql".to_string()),
                    scope: RootScope::Main,
                    exported: false,
                },
            ]
        );

        // Resources were attached to their enclosing modules.
        assert_eq!(core.resources[0].rel_path, "core/src/main/resources");
        assert_eq!(app.resources[0].rel_path, "app/assets");
    }

    #[test]
    fn package_info_is_authoritative_and_flags_disagreeing_classes() {
        let pkg_info = parse_tree("@Deprecated\npackage com.example.auth;");
        let good = parse_tree("package com.example.auth;\nclass Good {}");
        let bad = parse_tree("package com.example.wrong;\nclass Bad {}");

        let files = [
            (
                FileId(1),
                "src/com/example/auth/package-info.java",
                &pkg_info,
            ),
            (FileId(2), "src/com/example/auth/Good.java", &good),
            (FileId(3), "src/com/example/auth/Bad.java", &bad),
        ];

        let layout = infer_workspace_layout(
            &files,
            &[],
            LanguageLevel::latest(),
            &WorkspaceDelta::default(),
        );
        assert_eq!(layout.modules.len(), 1);
        assert_eq!(layout.modules[0].rel_path, ".");
        assert_eq!(layout.modules[0].sources[0].rel_path, "src");

        assert_eq!(layout.package_mismatches.len(), 1);
        let mismatch = &layout.package_mismatches[0];
        assert_eq!(mismatch.file_id, FileId(3));
        assert!(mismatch.authoritative_package_info);
        assert_eq!(
            mismatch.message(),
            "declared package 'com.example.wrong' does not match package-info.java package 'com.example.auth'"
        );
    }

    #[test]
    fn without_package_info_relative_path_from_source_root_flags_mismatch() {
        let a = parse_tree("package com.example.svc;\nclass A {}");
        let b = parse_tree("package com.example.svc;\nclass B {}");
        let typo = parse_tree("package com.example.typo;\nclass C {}");

        let files = [
            (FileId(1), "src/main/java/com/example/svc/A.java", &a),
            (FileId(2), "src/main/java/com/example/svc/B.java", &b),
            (FileId(3), "src/main/java/com/example/svc/C.java", &typo),
        ];

        let layout = infer_workspace_layout(
            &files,
            &[],
            LanguageLevel::latest(),
            &WorkspaceDelta::default(),
        );
        assert_eq!(layout.package_mismatches.len(), 1);
        let mismatch = &layout.package_mismatches[0];
        assert_eq!(mismatch.file_id, FileId(3));
        assert!(!mismatch.authoritative_package_info);
        assert_eq!(
            mismatch.message(),
            "declared package 'com.example.typo' does not match expected package 'com.example.svc'"
        );
    }

    #[test]
    fn unnamed_modules_use_relative_paths_and_support_include_exclude_deltas() {
        let script = parse_tree(
            "#!/usr/bin/env java --source 21 --enable-preview\n//DEPS org.slf4j:slf4j-api:2.0.12\nvoid main() {}\n",
        );
        let util = parse_tree("package demo;\nclass Util {}");
        let ignored = parse_tree("package demo;\nclass Ignored {}");

        let files = [
            (FileId(1), "scripts/hello.java", &script),
            (FileId(2), "services/worker/src/demo/Util.java", &util),
            (FileId(3), "test_data/demo/Ignored.java", &ignored),
        ];

        let mut delta = WorkspaceDelta {
            include: Vec::new(),
            exclude: vec!["test_data".to_string()],
            modules: BTreeMap::new(),
        };
        delta.modules.insert(
            "services/worker".to_string(),
            ModuleDelta {
                release: Some(Release::JAVA_21),
                preview: None,
                sources: DeltaSet {
                    include: vec!["services/worker/gen".to_string()],
                    exclude: Vec::new(),
                },
                resources: DeltaSet {
                    include: vec!["services/worker/res".to_string()],
                    exclude: Vec::new(),
                },
                dependencies: DeltaSet {
                    include: vec![
                        "scripts".to_string(),
                        "com.google.guava:guava:33.4.0-jre".to_string(),
                        "github:owner/repo#v1.0".to_string(),
                        "lib/helper.jar".to_string(),
                    ],
                    exclude: Vec::new(),
                },
            },
        );

        let layout = infer_workspace_layout(&files, &[], LanguageLevel::latest(), &delta);
        assert_eq!(layout.modules.len(), 2);
        assert_eq!(
            layout.file_levels.get(&FileId(1)),
            Some(&LanguageLevel::new(Release::JAVA_21, true))
        );

        let scripts_mod = layout.module_by_path("scripts").unwrap();
        assert_eq!(scripts_mod.module_name, None);
        assert_eq!(
            scripts_mod.dependencies,
            vec![ModuleDependency::Maven {
                group: "org.slf4j".to_string(),
                artifact: "slf4j-api".to_string(),
                version: "2.0.12".to_string(),
                classifier: None,
                scope: RootScope::Main,
            }]
        );

        let worker_mod = layout.module_by_path("services/worker").unwrap();
        assert_eq!(worker_mod.language_level.release, Release::JAVA_21);
        let worker_srcs: Vec<&str> = worker_mod
            .sources
            .iter()
            .map(|s| s.rel_path.as_str())
            .collect();
        assert_eq!(
            worker_srcs,
            ["services/worker/gen", "services/worker/src"]
        );
        assert_eq!(worker_mod.resources[0].rel_path, "services/worker/res");
        assert_eq!(
            worker_mod.dependencies,
            vec![
                ModuleDependency::Module {
                    target: ModuleTarget::Workspace(scripts_mod.id),
                    scope: RootScope::Main,
                    exported: false,
                },
                ModuleDependency::Maven {
                    group: "com.google.guava".to_string(),
                    artifact: "guava".to_string(),
                    version: "33.4.0-jre".to_string(),
                    classifier: None,
                    scope: RootScope::Main,
                },
                ModuleDependency::Git {
                    repo: "github:owner/repo".to_string(),
                    reference: Some("v1.0".to_string()),
                    scope: RootScope::Main,
                },
                ModuleDependency::Path {
                    path: "lib/helper.jar".to_string(),
                    scope: RootScope::Main,
                },
            ]
        );
    }

    #[test]
    fn root_scope_and_module_dir_classify_root_and_nested_standard_paths() {
        // Root-level main paths
        assert_eq!(RootScope::from_path("src/main/java"), RootScope::Main);
        assert_eq!(RootScope::from_path("src/main/resources"), RootScope::Main);
        assert_eq!(RootScope::from_path("src/main"), RootScope::Main);
        assert_eq!(RootScope::from_path("src"), RootScope::Main);
        assert_eq!(RootScope::from_path("resources"), RootScope::Main);

        // Root-level test paths
        assert_eq!(RootScope::from_path("src/test"), RootScope::Test);
        assert_eq!(RootScope::from_path("src/test/java"), RootScope::Test);
        assert_eq!(RootScope::from_path("src/test/resources"), RootScope::Test);
        assert_eq!(RootScope::from_path("test"), RootScope::Test);
        assert_eq!(RootScope::from_path("tests"), RootScope::Test);
        assert_eq!(RootScope::from_path("test/java"), RootScope::Test);
        assert_eq!(RootScope::from_path("tests/integration"), RootScope::Test);

        // Nested module main & test paths
        assert_eq!(RootScope::from_path("core/src/main/java"), RootScope::Main);
        assert_eq!(
            RootScope::from_path("core/src/main/resources"),
            RootScope::Main
        );
        assert_eq!(RootScope::from_path("core/src/test"), RootScope::Test);
        assert_eq!(RootScope::from_path("core/src/test/java"), RootScope::Test);
        assert_eq!(
            RootScope::from_path("core/src/test/resources"),
            RootScope::Test
        );
        assert_eq!(
            RootScope::from_path("services/core/test"),
            RootScope::Test
        );
        assert_eq!(
            RootScope::from_path("services/core/tests"),
            RootScope::Test
        );
        assert_eq!(
            RootScope::from_path("src/test.bach/test/java"),
            RootScope::Test
        );
        assert_eq!(RootScope::from_path("src/tests/junit"), RootScope::Test);
        assert_eq!(
            RootScope::from_path("test-jpms/src/main/java"),
            RootScope::Main
        );

        // Root-level module_dir_for_root must return "." before checking "/resources" etc.
        assert_eq!(module_dir_for_root("src/main/java"), ".");
        assert_eq!(module_dir_for_root("src/test/java"), ".");
        assert_eq!(module_dir_for_root("src/main/resources"), ".");
        assert_eq!(module_dir_for_root("src/test/resources"), ".");
        assert_eq!(module_dir_for_root("src/main"), ".");
        assert_eq!(module_dir_for_root("src/test"), ".");
        assert_eq!(module_dir_for_root("src"), ".");
        assert_eq!(module_dir_for_root("test"), ".");
        assert_eq!(module_dir_for_root("tests"), ".");
        assert_eq!(module_dir_for_root("resources"), ".");

        // Nested module_dir_for_root
        assert_eq!(module_dir_for_root("core/src/main/java"), "core");
        assert_eq!(module_dir_for_root("core/src/test/java"), "core");
        assert_eq!(module_dir_for_root("core/src/main/resources"), "core");
        assert_eq!(module_dir_for_root("core/src/test/resources"), "core");
        assert_eq!(module_dir_for_root("services/core/src/test"), "services/core");
        assert_eq!(module_dir_for_root("services/core/resources"), "services/core");
    }

    #[test]
    fn module_dir_for_root_maps_nonstandard_src_main_and_test_subdirs() {
        assert_eq!(module_dir_for_root("gson/src/main/java-templates"), "gson");
        assert_eq!(module_dir_for_root("src/main/java-templates"), ".");
        assert_eq!(module_dir_for_root("gson/src/test/java-templates"), "gson");
        assert_eq!(module_dir_for_root("src/test/java-templates"), ".");
    }

    #[test]
    fn main_module_info_takes_precedence_over_test_module_info() {
        let main_mod = parse_tree("module tools.jackson.core { requires java.logging; }");
        let test_mod =
            parse_tree("module tools.jackson.core.unittest { requires org.junit.jupiter.api; }");
        let main_cls = parse_tree("package tools.jackson.core;\npublic class JsonFactory {}");
        let test_cls = parse_tree("package tools.jackson.core;\npublic class JsonFactoryTest {}");

        // Main before Test
        let files_main_first = [
            (FileId(1), "src/main/java/module-info.java", &main_mod),
            (
                FileId(2),
                "src/main/java/tools/jackson/core/JsonFactory.java",
                &main_cls,
            ),
            (FileId(3), "src/test/java/module-info.java", &test_mod),
            (
                FileId(4),
                "src/test/java/tools/jackson/core/JsonFactoryTest.java",
                &test_cls,
            ),
        ];
        let layout_main_first = infer_workspace_layout(
            &files_main_first,
            &[],
            LanguageLevel::latest(),
            &WorkspaceDelta::default(),
        );
        assert_eq!(layout_main_first.modules.len(), 1);
        let m1 = layout_main_first.module_by_path(".").unwrap();
        assert_eq!(m1.module_name.as_deref(), Some("tools.jackson.core"));
        assert_eq!(
            m1.dependencies,
            vec![ModuleDependency::Module {
                target: ModuleTarget::External("java.logging".to_string()),
                scope: RootScope::Main,
                exported: false,
            }]
        );

        // Test before Main
        let files_test_first = [
            (FileId(1), "src/test/java/module-info.java", &test_mod),
            (FileId(2), "src/main/java/module-info.java", &main_mod),
        ];
        let layout_test_first = infer_workspace_layout(
            &files_test_first,
            &[],
            LanguageLevel::latest(),
            &WorkspaceDelta::default(),
        );
        assert_eq!(layout_test_first.modules.len(), 1);
        let m2 = layout_test_first.module_by_path(".").unwrap();
        assert_eq!(m2.module_name.as_deref(), Some("tools.jackson.core"));
    }

    #[test]
    fn module_dir_and_root_scope_handle_ant_and_gradle_custom_source_sets() {
        // Classic Ant roots
        assert_eq!(module_dir_for_root("src/java"), ".");
        assert_eq!(module_dir_for_root("test/java"), ".");
        assert_eq!(module_dir_for_root("core/src/java"), "core");
        assert_eq!(module_dir_for_root("core/test/java"), "core");

        // Gradle custom source sets
        assert_eq!(
            module_dir_for_root("junit-jupiter-api/src/testFixtures/java"),
            "junit-jupiter-api"
        );
        assert_eq!(
            module_dir_for_root("junit-jupiter-api/src/testFixtures/resources"),
            "junit-jupiter-api"
        );
        assert_eq!(
            module_dir_for_root("junit-jupiter-params/src/jmh/java"),
            "junit-jupiter-params"
        );
        assert_eq!(
            module_dir_for_root("platform-tests/src/graalVmTest/java"),
            "platform-tests"
        );
        assert_eq!(module_dir_for_root("src/testFixtures/java"), ".");
        assert_eq!(module_dir_for_root("src/jmh/resources"), ".");

        // RootScope classification for custom source sets vs module names
        assert_eq!(
            RootScope::from_path("junit-jupiter-api/src/testFixtures/java"),
            RootScope::Test
        );
        assert_eq!(
            RootScope::from_path("platform-tests/src/graalVmTest/java"),
            RootScope::Test
        );
        assert_eq!(
            RootScope::from_path("junit-jupiter-params/src/jmh/java"),
            RootScope::Main
        );
        assert_eq!(
            RootScope::from_path("test-jpms/src/main/java"),
            RootScope::Main
        );
        assert_eq!(
            RootScope::from_path("platform-tests/src/main/java"),
            RootScope::Main
        );
        assert_eq!(
            RootScope::from_path("src/integrationTest/java"),
            RootScope::Test
        );
        assert_eq!(
            RootScope::from_path("src/functionalTest/resources"),
            RootScope::Test
        );
    }
}
