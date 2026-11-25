//! Module-scoped, JPMS-aware, and JEP 238 Multi-Release symbol visibility views.
//! See `docs/specs/0009-module-scoping-and-mrjar-source-roots.md`.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

use rustc_hash::FxHashSet;
use smol_str::SmolStr;
use syntax::Release;

use crate::{
    DefDatabase, FileId,
    classpath::{BinaryOrigin, BinaryTypeSymbol, ClasspathIndex},
    item_tree::{FileItemTree, JpmsModuleSummary, TypeId},
    symbols::{
        FileOrigin, ModuleGraphData, ModuleIndex, NullnessScope, SymbolSlice, WorkspaceIndex,
    },
    workspace::{ModuleDependency, ModuleId, ModuleTarget, RootScope},
};

#[derive(Debug, Clone)]
pub(crate) enum ModuleImportTarget {
    Workspace(FileId, TypeId),
    Binary(String),
}

struct DepModuleView {
    index: Arc<ModuleIndex>,
    caller_jpms_name: Option<SmolStr>,
    dep_jpms: Option<JpmsModuleSummary>,
}

impl DepModuleView {
    fn is_package_visible(&self, pkg: &str) -> bool {
        let (Some(caller_name), Some(dep_jpms)) = (&self.caller_jpms_name, &self.dep_jpms) else {
            return true;
        };
        dep_jpms.exports.iter().any(|exp| {
            exp.package == pkg && (exp.targets.is_empty() || exp.targets.contains(caller_name))
        })
    }
}

struct ScopedVisibleView {
    caller_file: FileId,
    origin: FileOrigin,
    caller_view_release: Release,
    own_index: Arc<ModuleIndex>,
    deps: Vec<DepModuleView>,
    readable_jpms_modules: Option<FxHashSet<SmolStr>>,
    classpath: Arc<ClasspathIndex>,
    graph: Arc<ModuleGraphData>,
}

enum VisibleScopeMode {
    Flat {
        workspace: Arc<WorkspaceIndex>,
        classpath: Arc<ClasspathIndex>,
    },
    Scoped(Box<ScopedVisibleView>),
}

/// Caller-scoped view over workspace and binary symbols, enforcing `ModuleId` dependencies,
/// `Main`/`Test` isolation, JPMS `requires`/`exports` readability, and JEP 238 MR-JAR overlays.
pub struct VisibleScope {
    mode: VisibleScopeMode,
}

impl VisibleScope {
    /// Builds an unscoped flat visibility view (used for queries without a `FileId` context).
    #[must_use]
    pub fn flat(db: &dyn DefDatabase) -> Self {
        Self {
            mode: VisibleScopeMode::Flat {
                workspace: db.workspace_index(),
                classpath: db.classpath_index(),
            },
        }
    }

    /// Builds a visibility view for `caller_file`.
    ///
    /// Falls back to the O(1) flat view when `workspace_module_graph` is unset or does not
    /// contain `caller_file`.
    #[must_use]
    pub fn for_file(db: &dyn DefDatabase, caller_file: FileId) -> Self {
        let graph = db.workspace_module_graph();
        let classpath = db.classpath_index();

        let Some(&origin) = graph.file_origins.get(&caller_file) else {
            return Self {
                mode: VisibleScopeMode::Flat {
                    workspace: db.workspace_index(),
                    classpath,
                },
            };
        };

        let caller_view_release = origin
            .release_override
            .or_else(|| {
                graph.modules.get(&origin.module_id).map(|m| {
                    if origin.scope == RootScope::Test {
                        m.test_release
                    } else {
                        m.base_release
                    }
                })
            })
            .unwrap_or(Release::LATEST);

        let own_index = db.module_index(origin.module_id);
        let caller_jpms = (origin.scope == RootScope::Main)
            .then(|| own_index.effective_main_jpms(caller_view_release).cloned())
            .flatten();

        // 1. BFS reachable workspace dependencies in declaration order.
        let mut visited_mods = FxHashSet::default();
        visited_mods.insert(origin.module_id);
        let mut queue = VecDeque::new();

        if let Some(cfg) = graph.modules.get(&origin.module_id) {
            for dep in &cfg.dependencies {
                if let ModuleDependency::Module {
                    target: ModuleTarget::Workspace(dep_id),
                    scope,
                    ..
                } = dep
                    && (*scope == RootScope::Main || origin.scope == RootScope::Test)
                    && visited_mods.insert(*dep_id)
                {
                    queue.push_back(*dep_id);
                }
            }
        }

        let mut jpms_name_to_id: Option<BTreeMap<SmolStr, ModuleId>> = None;
        let mut raw_deps: Vec<(ModuleId, Arc<ModuleIndex>)> = Vec::new();

        while let Some(dep_id) = queue.pop_front() {
            let dep_index = db.module_index(dep_id);

            if let Some(cfg) = graph.modules.get(&dep_id) {
                for dep in &cfg.dependencies {
                    if let ModuleDependency::Module {
                        target: ModuleTarget::Workspace(next_id),
                        scope: RootScope::Main,
                        exported: true,
                    } = dep
                        && visited_mods.insert(*next_id)
                    {
                        queue.push_back(*next_id);
                    }
                }
            }

            if let Some(dep_jpms) = dep_index.effective_main_jpms(caller_view_release) {
                for req in &dep_jpms.requires {
                    if !req.is_transitive {
                        continue;
                    }
                    let name_map = jpms_name_to_id.get_or_insert_with(|| {
                        let mut map = BTreeMap::new();
                        for &mid in graph.modules.keys() {
                            let idx = if mid == origin.module_id {
                                own_index.clone()
                            } else if mid == dep_id {
                                dep_index.clone()
                            } else {
                                db.module_index(mid)
                            };
                            if let Some(jpms) = idx.effective_main_jpms(caller_view_release) {
                                map.insert(jpms.name.clone(), mid);
                            }
                        }
                        map
                    });
                    if let Some(&next_id) = name_map.get(&req.module_name)
                        && visited_mods.insert(next_id)
                    {
                        queue.push_back(next_id);
                    }
                }
            }

            raw_deps.push((dep_id, dep_index));
        }

        // 2. Compute readable JPMS module names when caller is a named `Main` module.
        let readable_jpms_modules = caller_jpms.as_ref().map(|caller| {
            let mut readable = FxHashSet::default();
            let mut mod_queue = VecDeque::new();

            let java_base = SmolStr::new("java.base");
            if readable.insert(java_base.clone()) {
                mod_queue.push_back(java_base);
            }
            if readable.insert(caller.name.clone()) {
                mod_queue.push_back(caller.name.clone());
            }
            for req in &caller.requires {
                if readable.insert(req.module_name.clone()) {
                    mod_queue.push_back(req.module_name.clone());
                }
            }

            let mut ws_jpms_by_name: BTreeMap<SmolStr, &JpmsModuleSummary> = BTreeMap::new();
            ws_jpms_by_name.insert(caller.name.clone(), caller);
            for (_, dep_idx) in &raw_deps {
                if let Some(dep_jpms) = dep_idx.effective_main_jpms(caller_view_release) {
                    ws_jpms_by_name.insert(dep_jpms.name.clone(), dep_jpms);
                }
            }

            while let Some(mod_name) = mod_queue.pop_front() {
                if let Some(ws_jpms) = ws_jpms_by_name.get(&mod_name) {
                    for req in &ws_jpms.requires {
                        if req.is_transitive && readable.insert(req.module_name.clone()) {
                            mod_queue.push_back(req.module_name.clone());
                        }
                    }
                }
                if let Some(bin_mod) = classpath.modules.get(mod_name.as_str()) {
                    for trans in &bin_mod.requires_transitive {
                        if readable.insert(trans.clone()) {
                            mod_queue.push_back(trans.clone());
                        }
                    }
                }
            }

            readable
        });

        // 3. Filter named dependencies by readability and attach export descriptors.
        let mut deps = Vec::with_capacity(raw_deps.len());
        for (_, dep_index) in raw_deps {
            let dep_jpms = dep_index.effective_main_jpms(caller_view_release).cloned();
            if let (Some(readable), Some(d_jpms)) = (&readable_jpms_modules, &dep_jpms)
                && !readable.contains(&d_jpms.name)
            {
                continue;
            }
            deps.push(DepModuleView {
                index: dep_index,
                caller_jpms_name: caller_jpms.as_ref().map(|c| c.name.clone()),
                dep_jpms,
            });
        }

        Self {
            mode: VisibleScopeMode::Scoped(Box::new(ScopedVisibleView {
                caller_file,
                origin,
                caller_view_release,
                own_index,
                deps,
                readable_jpms_modules,
                classpath,
                graph,
            })),
        }
    }

    #[must_use]
    pub fn is_scoped(&self) -> bool {
        matches!(self.mode, VisibleScopeMode::Scoped(_))
    }

    #[must_use]
    pub fn caller_file(&self) -> Option<FileId> {
        match &self.mode {
            VisibleScopeMode::Flat { .. } => None,
            VisibleScopeMode::Scoped(view) => Some(view.caller_file),
        }
    }

    #[must_use]
    pub fn classpath(&self) -> &ClasspathIndex {
        match &self.mode {
            VisibleScopeMode::Flat { classpath, .. } => classpath,
            VisibleScopeMode::Scoped(view) => &view.classpath,
        }
    }

    /// Looks up a workspace type by canonical FQN across visible slices.
    #[must_use]
    pub fn get_workspace_type(&self, fqn: &str) -> Option<(FileId, TypeId)> {
        match &self.mode {
            VisibleScopeMode::Flat { workspace, .. } => workspace.get_type(fqn),
            VisibleScopeMode::Scoped(view) => {
                if let Some(found) = find_in_own_slices(view, |s| s.get_type(fqn)) {
                    return Some(found);
                }
                for dep in &view.deps {
                    if let Some(loc) = find_in_dep_slices(dep, view.caller_view_release, |s| {
                        let loc = s.get_type(fqn)?;
                        let pkg = s.package_of_type(fqn)?;
                        dep.is_package_visible(pkg).then_some(loc)
                    }) {
                        return Some(loc);
                    }
                }
                None
            }
        }
    }

    #[must_use]
    pub fn contains_workspace_type(&self, fqn: &str) -> bool {
        self.get_workspace_type(fqn).is_some()
    }

    /// Looks up a workspace type by FQN, falling back to `file_tree` when `caller_file` is not yet
    /// in the workspace index.
    #[must_use]
    pub fn lookup_type_by_fqn(
        &self,
        file_tree: &FileItemTree,
        fqn: &str,
    ) -> Option<(FileId, TypeId)> {
        self.get_workspace_type(fqn).or_else(|| {
            file_tree
                .find_type_by_fqn(fqn)
                .map(|(tid, _)| (file_tree.file_id, tid))
        })
    }

    /// Looks up a top-level workspace type by package and simple name.
    #[must_use]
    pub fn lookup_package_type_in_workspace(
        &self,
        file_tree: &FileItemTree,
        pkg: &str,
        name: &str,
    ) -> Option<(FileId, TypeId)> {
        match &self.mode {
            VisibleScopeMode::Flat { workspace, .. } => {
                if let Some(fqn) = workspace.package_type_map.get(pkg).and_then(|m| m.get(name))
                    && let Some(&loc) = workspace.types_by_fqn.get(fqn)
                {
                    return Some(loc);
                }
            }
            VisibleScopeMode::Scoped(view) => {
                if let Some(found) = find_in_own_slices(view, |s| s.get_package_type(pkg, name)) {
                    return Some(found);
                }
                for dep in &view.deps {
                    if dep.is_package_visible(pkg)
                        && let Some(loc) = find_in_dep_slices(dep, view.caller_view_release, |s| {
                            s.get_package_type(pkg, name)
                        })
                    {
                        return Some(loc);
                    }
                }
            }
        }

        if file_tree.package.as_deref().unwrap_or("") == pkg
            && let Some((tid, _)) = file_tree
                .types
                .iter()
                .find(|(_, t)| t.enclosing_type.is_none() && t.name == name)
        {
            return Some((file_tree.file_id, tid));
        }

        None
    }

    /// Returns `true` if `bin` is visible from this scope under JPMS module readability/exports.
    #[must_use]
    pub fn is_binary_type_visible(&self, bin: &BinaryTypeSymbol) -> bool {
        let VisibleScopeMode::Scoped(view) = &self.mode else {
            return true;
        };
        let Some(readable) = &view.readable_jpms_modules else {
            return true;
        };
        match &bin.origin {
            BinaryOrigin::Jar { .. } | BinaryOrigin::Jdk { module_name: None } => true,
            BinaryOrigin::Jdk {
                module_name: Some(mod_name),
            } => {
                if !readable.contains(mod_name) {
                    return false;
                }
                if let Some(mod_info) = view.classpath.modules.get(mod_name.as_str()) {
                    mod_info.exports.iter().any(|exp| exp == &bin.package)
                } else {
                    true
                }
            }
        }
    }

    /// Looks up a visible binary type by canonical FQN.
    #[must_use]
    pub fn get_binary_type(&self, fqn: &str) -> Option<&BinaryTypeSymbol> {
        let bin = self.classpath().type_by_fqn(fqn)?;
        self.is_binary_type_visible(bin).then_some(bin)
    }

    #[must_use]
    pub fn contains_binary_type(&self, fqn: &str) -> bool {
        self.get_binary_type(fqn).is_some()
    }

    /// Looks up a visible top-level binary type by package and simple name.
    #[must_use]
    pub fn get_binary_type_in_package(&self, pkg: &str, name: &str) -> Option<&BinaryTypeSymbol> {
        let bin = self.classpath().type_in_package(pkg, name)?;
        self.is_binary_type_visible(bin).then_some(bin)
    }

    /// Returns `true` if `fqn` resolves in workspace sources, `file_tree`, or visible binary classpath.
    #[must_use]
    pub fn is_known_type_fqn(&self, file_tree: &FileItemTree, fqn: &str) -> bool {
        self.contains_workspace_type(fqn)
            || file_tree.find_type_by_fqn(fqn).is_some()
            || self.contains_binary_type(fqn)
    }

    /// Returns `true` if `prefix` is a visible package or ancestor prefix in workspace or binary classpath.
    #[must_use]
    pub fn has_package_prefix(&self, prefix: &str) -> bool {
        if prefix.is_empty() {
            return false;
        }
        match &self.mode {
            VisibleScopeMode::Flat {
                workspace,
                classpath,
            } => workspace.has_package_prefix(prefix) || classpath.has_package(prefix),
            VisibleScopeMode::Scoped(view) => {
                if find_in_own_slices(view, |s| s.has_package_prefix(prefix).then_some(())).is_some()
                {
                    return true;
                }
                let prefix_dot = format!("{prefix}.");
                for dep in &view.deps {
                    let has_visible =
                        find_in_dep_slices(dep, view.caller_view_release, |s| {
                            if s.types_by_package.contains_key(prefix)
                                && dep.is_package_visible(prefix)
                            {
                                return Some(());
                            }
                            for (pkg, _) in s.types_by_package.range(prefix_dot.clone()..) {
                                if !pkg.starts_with(&prefix_dot) {
                                    break;
                                }
                                if dep.is_package_visible(pkg) {
                                    return Some(());
                                }
                            }
                            None
                        })
                        .is_some();
                    if has_visible {
                        return true;
                    }
                }
                has_visible_binary_package_prefix(self, &view.classpath, prefix)
            }
        }
    }

    /// Resolves the lexical [`NullnessScope`] declared on `pkg` via `package-info.java` in visible layers.
    #[must_use]
    pub fn package_nullness(&self, pkg: &str) -> NullnessScope {
        match &self.mode {
            VisibleScopeMode::Flat { workspace, .. } => workspace.package_nullness(pkg),
            VisibleScopeMode::Scoped(view) => {
                if let Some(scope) = find_in_own_slices(view, |s| {
                    s.package_nullness
                        .get(pkg)
                        .copied()
                        .filter(|sc| *sc != NullnessScope::Inherited)
                }) {
                    return scope;
                }
                for dep in &view.deps {
                    if dep.is_package_visible(pkg)
                        && let Some(scope) =
                            find_in_dep_slices(dep, view.caller_view_release, |s| {
                                s.package_nullness
                                    .get(pkg)
                                    .copied()
                                    .filter(|sc| *sc != NullnessScope::Inherited)
                            })
                    {
                        return scope;
                    }
                }
                NullnessScope::Inherited
            }
        }
    }

    /// Resolves the effective module-level [`NullnessScope`] for the caller's module.
    #[must_use]
    pub fn effective_module_nullness(&self, file_id: FileId) -> NullnessScope {
        match &self.mode {
            VisibleScopeMode::Flat { workspace, .. } => workspace
                .file_module(file_id)
                .map_or(NullnessScope::Inherited, |m| workspace.module_nullness(m)),
            VisibleScopeMode::Scoped(view) => {
                if view.origin.scope == RootScope::Test {
                    for slice in view.own_index.iter_test_slices(view.caller_view_release) {
                        if slice.module_nullness != NullnessScope::Inherited {
                            return slice.module_nullness;
                        }
                    }
                }
                view.own_index
                    .effective_main_module_nullness(view.caller_view_release)
            }
        }
    }

    /// Resolves `simple_name` via a JEP 511 `import module M;` directive across readable
    /// workspace and binary JPMS modules.
    pub(crate) fn resolve_module_import_type(
        &self,
        file_tree: &FileItemTree,
        module_name: &str,
        simple_name: &str,
    ) -> Option<ModuleImportTarget> {
        if let VisibleScopeMode::Scoped(view) = &self.mode {
            if let Some(readable) = &view.readable_jpms_modules
                && !readable.contains(module_name)
            {
                return None;
            }
            for exported_pkg in self.workspace_transitive_module_exports(view, module_name) {
                if let Some((fid, tid)) =
                    self.lookup_package_type_in_workspace(file_tree, &exported_pkg, simple_name)
                {
                    return Some(ModuleImportTarget::Workspace(fid, tid));
                }
                if let Some(bin) = self.get_binary_type_in_package(&exported_pkg, simple_name) {
                    return Some(ModuleImportTarget::Binary(bin.qualified_name.clone()));
                }
            }
        }

        for exported_pkg in self.classpath().transitive_module_exports(module_name) {
            if let Some((fid, tid)) =
                self.lookup_package_type_in_workspace(file_tree, exported_pkg, simple_name)
            {
                return Some(ModuleImportTarget::Workspace(fid, tid));
            }
            if let Some(bin) = self.get_binary_type_in_package(exported_pkg, simple_name) {
                return Some(ModuleImportTarget::Binary(bin.qualified_name.clone()));
            }
        }

        None
    }

    fn workspace_transitive_module_exports(
        &self,
        view: &ScopedVisibleView,
        root_module_name: &str,
    ) -> Vec<String> {
        let mut by_name: BTreeMap<SmolStr, (&ModuleIndex, Option<&JpmsModuleSummary>)> =
            BTreeMap::new();
        if let Some(jpms) = view.own_index.effective_main_jpms(view.caller_view_release) {
            by_name.insert(jpms.name.clone(), (&view.own_index, Some(jpms)));
        }
        for dep in &view.deps {
            if let Some(jpms) = &dep.dep_jpms {
                by_name.insert(jpms.name.clone(), (&dep.index, Some(jpms)));
            }
        }
        if !by_name.contains_key(root_module_name) {
            return Vec::new();
        }

        let caller_name = view
            .own_index
            .effective_main_jpms(view.caller_view_release)
            .map(|j| j.name.as_str());

        let mut visited = BTreeSet::new();
        let mut seen_pkgs = BTreeSet::new();
        let mut out = Vec::new();
        let mut queue = VecDeque::new();
        visited.insert(root_module_name.to_string());
        queue.push_back(root_module_name.to_string());

        while let Some(mod_name) = queue.pop_front() {
            if let Some((_, Some(jpms))) = by_name.get(mod_name.as_str()) {
                for exp in &jpms.exports {
                    let allowed = exp.targets.is_empty()
                        || caller_name.is_some_and(|c| exp.targets.iter().any(|t| t == c));
                    if allowed && seen_pkgs.insert(exp.package.clone()) {
                        out.push(exp.package.clone());
                    }
                }
                for req in &jpms.requires {
                    if req.is_transitive && visited.insert(req.module_name.to_string()) {
                        queue.push_back(req.module_name.to_string());
                    }
                }
            } else if let Some(bin_mod) = view.classpath.modules.get(mod_name.as_str()) {
                for pkg in &bin_mod.exports {
                    if seen_pkgs.insert(pkg.clone()) {
                        out.push(pkg.clone());
                    }
                }
                for trans in &bin_mod.requires_transitive {
                    if visited.insert(trans.to_string()) {
                        queue.push_back(trans.to_string());
                    }
                }
            }
        }
        out
    }

    /// Returns `true` when in scoped mode and `fqn` exists somewhere in the workspace or binary
    /// classpath but is NOT visible to this caller (preventing unindexed-import fallbacks from
    /// leaking inaccessible workspace or JDK types).
    #[must_use]
    pub fn is_blocked_known_type(&self, db: &dyn DefDatabase, fqn: &str) -> bool {
        let VisibleScopeMode::Scoped(view) = &self.mode else {
            return false;
        };
        if self.contains_workspace_type(fqn) || self.contains_binary_type(fqn) {
            return false;
        }
        if view.classpath.type_by_fqn(fqn).is_some() {
            return true;
        }
        let _ = &view.graph;
        db.workspace_index().contains_type(fqn)
    }
}

fn find_in_own_slices<T>(
    view: &ScopedVisibleView,
    mut f: impl FnMut(&SymbolSlice) -> Option<T>,
) -> Option<T> {
    if view.origin.scope == RootScope::Test {
        for slice in view.own_index.iter_test_slices(view.caller_view_release) {
            if let Some(found) = f(slice) {
                return Some(found);
            }
        }
    }
    for slice in view.own_index.iter_main_slices(view.caller_view_release) {
        if let Some(found) = f(slice) {
            return Some(found);
        }
    }
    None
}

fn find_in_dep_slices<T>(
    dep: &DepModuleView,
    view_release: Release,
    mut f: impl FnMut(&SymbolSlice) -> Option<T>,
) -> Option<T> {
    for slice in dep.index.iter_main_slices(view_release) {
        if let Some(found) = f(slice) {
            return Some(found);
        }
    }
    None
}

fn has_visible_binary_package_prefix(
    scope: &VisibleScope,
    classpath: &ClasspathIndex,
    prefix: &str,
) -> bool {
    let prefix_dot = format!("{prefix}.");
    for (pkg, fqns) in classpath.types_by_package.range(prefix.to_string()..) {
        if pkg != prefix && !pkg.starts_with(&prefix_dot) {
            break;
        }
        if fqns.iter().any(|fqn| {
            classpath
                .type_by_fqn(fqn)
                .is_some_and(|bin| scope.is_binary_type_visible(bin))
        }) {
            return true;
        }
    }
    false
}
