//! Pure, cycle-safe type hierarchy and inheritance queries across source and binary types.
//! See `docs/specs/0007-type-hierarchy-and-inheritance.md`.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

use rustc_hash::FxHashSet;

use crate::{
    DefDatabase, FileId,
    classpath::{BinaryTypeSymbol, ClasspathIndex},
    item_tree::{FileItemTree, MemberId, TypeItem},
    symbols::{
        ImportKind, MemberKind, TypeKind, WorkspaceIndex, format_params_types_signature,
    },
};

const OBJECT_FQN: &str = "java.lang.Object";
const ENUM_FQN: &str = "java.lang.Enum";
const RECORD_FQN: &str = "java.lang.Record";
const ANNOTATION_FQN: &str = "java.lang.annotation.Annotation";

/// Resolved direct supertypes for a canonical type FQN.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectSupertypes {
    /// Direct superclass FQN (`None` for `java.lang.Object` and interfaces).
    pub super_class: Option<String>,
    /// Directly implemented/extended interface FQNs (deduplicated, declaration order).
    pub interfaces: Vec<String>,
    /// Directly permitted subtype FQNs (sealed types).
    pub permits: Vec<String>,
}

/// Position-independent reverse index of workspace subtype relationships.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceHierarchyIndex {
    /// Supertype FQN -> sorted list of direct workspace subtype FQNs.
    pub direct_subtypes: BTreeMap<String, Vec<String>>,
    /// Workspace type FQN -> resolved [`DirectSupertypes`].
    pub supertypes: BTreeMap<String, DirectSupertypes>,
    /// Binary supertype FQN -> sorted list of binary subtype FQNs that sit on a path to at least
    /// one workspace subtype (so `all_subtypes` on a binary interface like `Collection` finds
    /// workspace classes implementing `List` without scanning the entire JDK).
    pub(crate) binary_subtypes: BTreeMap<String, Vec<String>>,
}

/// Strips generics (`<...>`), leading `@Annotations(...)`, trailing `!`/`?` nullability markers,
/// and rejects arrays, primitives, and `var`.
pub(crate) fn parse_type_ref_segments(type_ref: &str) -> Option<Vec<String>> {
    let trimmed = type_ref.trim().trim_end_matches(['!', '?']).trim();
    if trimmed.is_empty() || trimmed.ends_with(']') {
        return None;
    }

    let mut cleaned = String::with_capacity(trimmed.len());
    let mut chars = trimmed.chars().peekable();
    let mut angle_depth = 0usize;

    while let Some(c) = chars.next() {
        match c {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '!' | '?' if angle_depth == 0 => {}
            '@' if angle_depth == 0 => {
                while let Some(&next) = chars.peek() {
                    if next.is_alphanumeric() || next == '_' || next == '$' || next == '.' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
                    chars.next();
                }
                if chars.peek() == Some(&'(') {
                    let mut paren_depth = 0usize;
                    for ch in chars.by_ref() {
                        if ch == '(' {
                            paren_depth += 1;
                        } else if ch == ')' {
                            paren_depth = paren_depth.saturating_sub(1);
                            if paren_depth == 0 {
                                break;
                            }
                        }
                    }
                }
            }
            _ if angle_depth == 0 => cleaned.push(c),
            _ => {}
        }
    }

    let segs: Vec<String> = cleaned
        .split('.')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if segs.is_empty() {
        return None;
    }
    if let [only] = segs.as_slice()
        && matches!(
            only.as_str(),
            "boolean"
                | "byte"
                | "short"
                | "int"
                | "long"
                | "char"
                | "float"
                | "double"
                | "void"
                | "var"
        )
    {
        return None;
    }
    Some(segs)
}

fn push_unique(out: &mut Vec<String>, value: String) {
    if !out.contains(&value) {
        out.push(value);
    }
}

fn resolve_known_fqn(
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    fqn: &str,
) -> Option<String> {
    if workspace.contains_type(fqn)
        || file_tree.find_type_by_fqn(fqn).is_some()
        || classpath.type_by_fqn(fqn).is_some()
    {
        Some(fqn.to_string())
    } else {
        None
    }
}

fn resolve_package_type(
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    pkg: &str,
    name: &str,
) -> Option<String> {
    if let Some(fqn) = workspace
        .package_type_map
        .get(pkg)
        .and_then(|m| m.get(name))
        && workspace.contains_type(fqn)
    {
        return Some(fqn.clone());
    }
    if file_tree.package.as_deref().unwrap_or("") == pkg
        && let Some((_, ty)) = file_tree
            .types
            .iter()
            .find(|(_, t)| t.enclosing_type.is_none() && t.name == name)
    {
        return Some(ty.qualified_name.clone());
    }
    if let Some(bin) = classpath.type_in_package(pkg, name) {
        return Some(bin.qualified_name.clone());
    }
    None
}

fn resolve_single_segment_known(
    name: &str,
    owner_ty: &TypeItem,
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
) -> Option<String> {
    // 1. Direct nested types of `owner_ty` (needed e.g. for `sealed ... permits Nested`) and
    //    enclosing type scopes innermost-to-outermost.
    if owner_ty.nested_types.iter().any(|n| n == name) {
        let nested_fqn = format!("{}.{name}", owner_ty.qualified_name);
        if let Some(found) = resolve_known_fqn(file_tree, workspace, classpath, &nested_fqn) {
            return Some(found);
        }
    }

    let mut enclosing = owner_ty.enclosing_type.as_deref();
    while let Some(enc_fqn) = enclosing {
        let Some((_, enc_ty)) = file_tree.find_type_by_fqn(enc_fqn) else {
            break;
        };
        if enc_ty.name == name {
            return Some(enc_ty.qualified_name.clone());
        }
        if enc_ty.nested_types.iter().any(|n| n == name) {
            let nested_fqn = format!("{enc_fqn}.{name}");
            if let Some(found) = resolve_known_fqn(file_tree, workspace, classpath, &nested_fqn) {
                return Some(found);
            }
        }
        enclosing = enc_ty.enclosing_type.as_deref();
    }

    // 2. Single-type imports (workspace first, then binary).
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::SingleType && imp.path.last().map(String::as_str) == Some(name) {
            let fqn = imp.path.join(".");
            if workspace.contains_type(&fqn) || file_tree.find_type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::SingleType && imp.path.last().map(String::as_str) == Some(name) {
            let fqn = imp.path.join(".");
            if classpath.type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }

    // 3. Static single imports resolving to a nested type.
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticSingle
            && imp.path.last().map(String::as_str) == Some(name)
        {
            let fqn = imp.path.join(".");
            if workspace.contains_type(&fqn) || file_tree.find_type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticSingle
            && imp.path.last().map(String::as_str) == Some(name)
        {
            let fqn = imp.path.join(".");
            if classpath.type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }

    // 4. Same-package top-level types.
    let pkg = file_tree.package.as_deref().unwrap_or("");
    if let Some(found) = resolve_package_type(file_tree, workspace, classpath, pkg, name) {
        return Some(found);
    }

    // 5. Implicit `java.lang.*`.
    if let Some(found) = resolve_package_type(file_tree, workspace, classpath, "java.lang", name) {
        return Some(found);
    }

    // 6. Type-import-on-demand (`import a.b.*;`).
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::TypeOnDemand {
            let fqn = format!("{}.{name}", imp.path.join("."));
            if workspace.contains_type(&fqn) || file_tree.find_type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::TypeOnDemand {
            let fqn = format!("{}.{name}", imp.path.join("."));
            if classpath.type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }

    // 7. Static-import-on-demand (`import static a.b.Outer.*;`).
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticOnDemand {
            let fqn = format!("{}.{name}", imp.path.join("."));
            if workspace.contains_type(&fqn) || file_tree.find_type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticOnDemand {
            let fqn = format!("{}.{name}", imp.path.join("."));
            if classpath.type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }

    // 8. Module imports (`import module M;`).
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::Module {
            let module_name = imp.path.join(".");
            for exported_pkg in classpath.transitive_module_exports(&module_name) {
                if let Some(found) =
                    resolve_package_type(file_tree, workspace, classpath, exported_pkg, name)
                {
                    return Some(found);
                }
            }
        }
    }

    None
}

/// Resolves a raw type reference (`"Base"`, `"com.foo.Iface<T>"`) in `owner_ty`'s declaring file
/// scope using only position-independent [`FileItemTree`], [`WorkspaceIndex`], and
/// [`ClasspathIndex`].
pub(crate) fn resolve_type_ref_in_item_tree(
    raw_ref: &str,
    owner_ty: &TypeItem,
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
) -> Option<String> {
    let segs = parse_type_ref_segments(raw_ref)?;

    if let [name] = segs.as_slice() {
        if let Some(found) =
            resolve_single_segment_known(name, owner_ty, file_tree, workspace, classpath)
        {
            return Some(found);
        }
        // Best-effort fallback for unindexed external types (e.g. `SingleType` import or `Object`).
        for (_, imp) in file_tree.imports.iter() {
            if imp.kind == ImportKind::SingleType
                && imp.path.last().map(String::as_str) == Some(name)
            {
                return Some(imp.path.join("."));
            }
        }
        if name == "Object" {
            return Some(OBJECT_FQN.to_string());
        }
        return Some(name.clone());
    }

    // Multi-segment path: direct FQN first, then resolve `segs[0]` and walk nested segments.
    let joined = segs.join(".");
    if let Some(found) = resolve_known_fqn(file_tree, workspace, classpath, &joined) {
        return Some(found);
    }

    if let [first, rest @ ..] = segs.as_slice()
        && let Some(first_fqn) =
            resolve_single_segment_known(first, owner_ty, file_tree, workspace, classpath)
    {
        let mut current = first_fqn;
        for seg in rest {
            let next = format!("{current}.{seg}");
            current = resolve_known_fqn(file_tree, workspace, classpath, &next)?;
        }
        return Some(current);
    }

    Some(joined)
}

fn direct_supertypes_for_workspace_type(
    ty: &TypeItem,
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
) -> DirectSupertypes {
    let mut interfaces = Vec::new();
    let mut permits = Vec::new();

    for raw in &ty.permits {
        if let Some(resolved) =
            resolve_type_ref_in_item_tree(raw, ty, file_tree, workspace, classpath)
        {
            push_unique(&mut permits, resolved);
        }
    }

    match ty.kind {
        TypeKind::Class => {
            let super_class = ty
                .extends
                .first()
                .and_then(|raw| {
                    resolve_type_ref_in_item_tree(raw, ty, file_tree, workspace, classpath)
                })
                .or_else(|| (ty.qualified_name != OBJECT_FQN).then(|| OBJECT_FQN.to_string()));
            for raw in &ty.implements {
                if let Some(resolved) =
                    resolve_type_ref_in_item_tree(raw, ty, file_tree, workspace, classpath)
                {
                    push_unique(&mut interfaces, resolved);
                }
            }
            DirectSupertypes {
                super_class,
                interfaces,
                permits,
            }
        }
        TypeKind::Interface => {
            for raw in &ty.extends {
                if let Some(resolved) =
                    resolve_type_ref_in_item_tree(raw, ty, file_tree, workspace, classpath)
                {
                    push_unique(&mut interfaces, resolved);
                }
            }
            DirectSupertypes {
                super_class: None,
                interfaces,
                permits,
            }
        }
        TypeKind::Enum => {
            for raw in &ty.implements {
                if let Some(resolved) =
                    resolve_type_ref_in_item_tree(raw, ty, file_tree, workspace, classpath)
                {
                    push_unique(&mut interfaces, resolved);
                }
            }
            DirectSupertypes {
                super_class: Some(ENUM_FQN.to_string()),
                interfaces,
                permits: Vec::new(),
            }
        }
        TypeKind::Record => {
            for raw in &ty.implements {
                if let Some(resolved) =
                    resolve_type_ref_in_item_tree(raw, ty, file_tree, workspace, classpath)
                {
                    push_unique(&mut interfaces, resolved);
                }
            }
            DirectSupertypes {
                super_class: Some(RECORD_FQN.to_string()),
                interfaces,
                permits: Vec::new(),
            }
        }
        TypeKind::Annotation => {
            push_unique(&mut interfaces, ANNOTATION_FQN.to_string());
            DirectSupertypes {
                super_class: None,
                interfaces,
                permits: Vec::new(),
            }
        }
    }
}

fn direct_supertypes_for_binary_type(bin: &BinaryTypeSymbol) -> DirectSupertypes {
    let mut interfaces = Vec::new();
    for iface in &bin.interfaces {
        push_unique(&mut interfaces, iface.clone());
    }

    let mut permits = Vec::new();
    for sub in &bin.permitted_subclasses {
        push_unique(&mut permits, sub.clone());
    }

    let super_class = if bin.qualified_name == OBJECT_FQN {
        None
    } else {
        match bin.kind {
            TypeKind::Interface => None,
            TypeKind::Annotation => {
                push_unique(&mut interfaces, ANNOTATION_FQN.to_string());
                None
            }
            TypeKind::Enum => Some(
                bin.super_class
                    .clone()
                    .unwrap_or_else(|| ENUM_FQN.to_string()),
            ),
            TypeKind::Record => Some(
                bin.super_class
                    .clone()
                    .unwrap_or_else(|| RECORD_FQN.to_string()),
            ),
            TypeKind::Class => Some(
                bin.super_class
                    .clone()
                    .unwrap_or_else(|| OBJECT_FQN.to_string()),
            ),
        }
    };

    DirectSupertypes {
        super_class,
        interfaces,
        permits,
    }
}

/// Builds a deterministic, position-independent [`WorkspaceHierarchyIndex`] from all workspace
/// [`FileItemTree`]s, [`WorkspaceIndex`], and [`ClasspathIndex`].
pub(crate) fn build_workspace_hierarchy_index(
    per_file: &[Arc<FileItemTree>],
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
) -> WorkspaceHierarchyIndex {
    let mut supertypes = BTreeMap::new();
    let mut direct_subtypes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut binary_subtypes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut visited_binary = FxHashSet::default();
    let mut binary_queue = VecDeque::new();

    for file_tree in per_file {
        for (_, ty) in file_tree.types.iter() {
            let direct = direct_supertypes_for_workspace_type(ty, file_tree, workspace, classpath);
            if let Some(sup) = &direct.super_class {
                direct_subtypes
                    .entry(sup.clone())
                    .or_default()
                    .push(ty.qualified_name.clone());
                if !workspace.contains_type(sup)
                    && classpath.type_by_fqn(sup).is_some()
                    && visited_binary.insert(sup.clone())
                {
                    binary_queue.push_back(sup.clone());
                }
            }
            for iface in &direct.interfaces {
                direct_subtypes
                    .entry(iface.clone())
                    .or_default()
                    .push(ty.qualified_name.clone());
                if !workspace.contains_type(iface)
                    && classpath.type_by_fqn(iface).is_some()
                    && visited_binary.insert(iface.clone())
                {
                    binary_queue.push_back(iface.clone());
                }
            }
            supertypes.insert(ty.qualified_name.clone(), direct);
        }
    }

    // Walk binary ancestors reachable from workspace types so `all_subtypes` on an indirect
    // binary supertype (e.g. `java.util.Collection` -> `java.util.List` -> `User`) works
    // without scanning unreferenced JDK classes.
    while let Some(bin_fqn) = binary_queue.pop_front() {
        let Some(bin) = classpath.type_by_fqn(&bin_fqn) else {
            continue;
        };
        let bin_direct = direct_supertypes_for_binary_type(bin);
        if let Some(sup) = bin_direct.super_class {
            binary_subtypes
                .entry(sup.clone())
                .or_default()
                .push(bin_fqn.clone());
            if !workspace.contains_type(&sup)
                && classpath.type_by_fqn(&sup).is_some()
                && visited_binary.insert(sup.clone())
            {
                binary_queue.push_back(sup);
            }
        }
        for iface in bin_direct.interfaces {
            binary_subtypes
                .entry(iface.clone())
                .or_default()
                .push(bin_fqn.clone());
            if !workspace.contains_type(&iface)
                && classpath.type_by_fqn(&iface).is_some()
                && visited_binary.insert(iface.clone())
            {
                binary_queue.push_back(iface);
            }
        }
    }

    for subs in direct_subtypes.values_mut() {
        subs.sort();
        subs.dedup();
    }
    for subs in binary_subtypes.values_mut() {
        subs.sort();
        subs.dedup();
    }

    WorkspaceHierarchyIndex {
        direct_subtypes,
        supertypes,
        binary_subtypes,
    }
}

/// Returns the direct supertypes of `fqn`, checking workspace sources first and then the binary
/// classpath, applying JLS implicit supertypes.
#[must_use]
pub fn direct_supertypes(db: &dyn DefDatabase, fqn: &str) -> DirectSupertypes {
    let hierarchy = db.workspace_hierarchy_index();
    if let Some(direct) = hierarchy.supertypes.get(fqn) {
        return direct.clone();
    }
    let classpath = db.classpath_index();
    if let Some(bin) = classpath.type_by_fqn(fqn) {
        return direct_supertypes_for_binary_type(bin);
    }
    if fqn == ENUM_FQN || fqn == RECORD_FQN {
        return DirectSupertypes {
            super_class: Some(OBJECT_FQN.to_string()),
            interfaces: Vec::new(),
            permits: Vec::new(),
        };
    }
    DirectSupertypes::default()
}

/// Returns all supertypes of `fqn` in deterministic, cycle-safe lookup order:
/// superclass chain first, then superinterfaces in BFS order, and `java.lang.Object` last
/// (excluding `fqn` itself).
#[must_use]
pub fn linearized_supertypes(db: &dyn DefDatabase, fqn: &str) -> Vec<String> {
    let mut visited = FxHashSet::default();
    visited.insert(fqn.to_string());

    let mut result = Vec::new();
    let mut iface_queue = VecDeque::new();

    let first = direct_supertypes(db, fqn);
    for iface in first.interfaces {
        iface_queue.push_back(iface);
    }

    let mut cur_super = first.super_class;
    while let Some(sup) = cur_super {
        if sup == OBJECT_FQN || !visited.insert(sup.clone()) {
            break;
        }
        result.push(sup.clone());
        let sup_direct = direct_supertypes(db, &sup);
        for iface in sup_direct.interfaces {
            iface_queue.push_back(iface);
        }
        cur_super = sup_direct.super_class;
    }

    while let Some(iface) = iface_queue.pop_front() {
        if iface == OBJECT_FQN || !visited.insert(iface.clone()) {
            continue;
        }
        result.push(iface.clone());
        let iface_direct = direct_supertypes(db, &iface);
        for parent_iface in iface_direct.interfaces {
            iface_queue.push_back(parent_iface);
        }
    }

    if fqn != OBJECT_FQN && visited.insert(OBJECT_FQN.to_string()) {
        result.push(OBJECT_FQN.to_string());
    }

    result
}

/// Returns the sorted list of direct workspace subtypes of `fqn` (including any workspace-present
/// `permitted_subclasses` declared on a binary sealed type).
#[must_use]
pub fn direct_subtypes(db: &dyn DefDatabase, fqn: &str) -> Vec<String> {
    let hierarchy = db.workspace_hierarchy_index();
    let mut subs = hierarchy
        .direct_subtypes
        .get(fqn)
        .cloned()
        .unwrap_or_default();

    let classpath = db.classpath_index();
    if let Some(bin) = classpath.type_by_fqn(fqn)
        && !bin.permitted_subclasses.is_empty()
    {
        let workspace = db.workspace_index();
        for permitted in &bin.permitted_subclasses {
            if workspace.contains_type(permitted) && !subs.contains(permitted) {
                subs.push(permitted.clone());
            }
        }
        subs.sort();
        subs.dedup();
    }

    subs
}

/// Returns the cycle-safe transitive closure of [`direct_subtypes`] in deterministic sorted order,
/// excluding `fqn` itself.
#[must_use]
pub fn all_subtypes(db: &dyn DefDatabase, fqn: &str) -> Vec<String> {
    let hierarchy = db.workspace_hierarchy_index();
    let mut visited = FxHashSet::default();
    visited.insert(fqn.to_string());

    let mut out = BTreeSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(fqn.to_string());

    while let Some(cur) = queue.pop_front() {
        for sub in direct_subtypes(db, &cur) {
            if visited.insert(sub.clone()) {
                out.insert(sub.clone());
                queue.push_back(sub);
            }
        }
        if let Some(bin_subs) = hierarchy.binary_subtypes.get(&cur) {
            for bin_sub in bin_subs {
                if visited.insert(bin_sub.clone()) {
                    queue.push_back(bin_sub.clone());
                }
            }
        }
    }

    out.into_iter().collect()
}

/// Finds the nearest supertype method overridden by `(owner_fqn, method_name, param_count)`,
/// returning `"OwnerFqn.methodName(ParamTypes...)"` (e.g. `"com.example.Base.greet(String)"`).
#[must_use]
pub fn find_overridden_method(
    db: &dyn DefDatabase,
    owner_fqn: &str,
    method_name: &str,
    param_count: usize,
) -> Option<String> {
    let workspace = db.workspace_index();
    let classpath = db.classpath_index();

    for sup_fqn in linearized_supertypes(db, owner_fqn) {
        if let Some((fid, tid)) = workspace.get_type(&sup_fqn) {
            let tree = db.file_item_tree(fid);
            if let Some(m) = tree.types[tid].members.iter().find_map(|&mid| {
                let m = &tree.members[mid];
                (m.kind == MemberKind::Method
                    && !m.is_static
                    && m.name == method_name
                    && m.params.len() == param_count)
                    .then_some(m)
            }) {
                return Some(format!(
                    "{sup_fqn}.{method_name}{}",
                    format_params_types_signature(&m.params)
                ));
            }
        } else if let Some(bin) = classpath.type_by_fqn(&sup_fqn)
            && let Some(m) = bin.members.iter().find(|m| {
                m.kind == MemberKind::Method
                    && !m.is_static
                    && m.name == method_name
                    && m.params.len() == param_count
            })
        {
            return Some(format!(
                "{sup_fqn}.{method_name}{}",
                format_params_types_signature(&m.params)
            ));
        }
    }

    None
}

/// Finds all workspace methods in [`all_subtypes`] that override
/// `(owner_fqn, method_name, param_count)`.
#[must_use]
pub fn find_overriding_methods(
    db: &dyn DefDatabase,
    owner_fqn: &str,
    method_name: &str,
    param_count: usize,
) -> Vec<(FileId, MemberId)> {
    let workspace = db.workspace_index();
    let mut out = Vec::new();

    for sub_fqn in all_subtypes(db, owner_fqn) {
        let Some((fid, tid)) = workspace.get_type(&sub_fqn) else {
            continue;
        };
        let tree = db.file_item_tree(fid);
        for &mid in &tree.types[tid].members {
            let m = &tree.members[mid];
            if m.kind == MemberKind::Method
                && !m.is_static
                && m.name == method_name
                && m.params.len() == param_count
            {
                out.push((fid, mid));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use smol_str::SmolStr;

    use super::*;
    use crate::{
        BinaryMemberSymbol, BinaryOrigin, BinaryTypeSymbol,
        test_fixture::{db_with, db_with_files},
    };

    fn binary_type(
        fqn: &str,
        kind: TypeKind,
        super_class: Option<&str>,
        interfaces: &[&str],
        members: Vec<BinaryMemberSymbol>,
    ) -> BinaryTypeSymbol {
        let (pkg, name) = match fqn.rsplit_once('.') {
            Some((p, n)) => (p.to_string(), SmolStr::new(n)),
            None => (String::new(), SmolStr::new(fqn)),
        };
        BinaryTypeSymbol {
            name,
            qualified_name: fqn.to_string(),
            internal_name: fqn.replace('.', "/"),
            package: pkg,
            enclosing_type: None,
            kind,
            identity: if matches!(kind, TypeKind::Interface | TypeKind::Annotation) {
                crate::TypeIdentity::Unspecified
            } else {
                crate::TypeIdentity::Identity
            },
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: matches!(kind, TypeKind::Interface | TypeKind::Annotation),
            nullness_scope: crate::NullnessScope::Inherited,
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            super_class: super_class.map(str::to_string),
            super_class_generic: None,
            interfaces: interfaces.iter().map(|s| (*s).to_string()).collect(),
            interfaces_generic: Vec::new(),
            permitted_subclasses: Vec::new(),
            members,
            nested_types: Vec::new(),
            origin: BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base")),
            },
        }
    }

    #[test]
    fn implicit_supertypes_and_generic_stripping() {
        let iface_file = "\
package com.api;
public interface Repo<T> {
    default int count() { return 0; }
    void save(T item);
    static void util() {}
}
";
        let types_file = "\
package com.model;
import com.api.Repo;
import java.util.List;

public abstract class BaseEntity {}
public class User extends BaseEntity implements Repo<String>, List<String> {
    @Override
    public void save(String item) {}
}
public enum Role { ADMIN, USER }
public record Point(int x, int y) implements Repo<Point> {
    @java.lang.Override
    public void save(Point item) {}
}
public @interface Tag {}
public sealed interface Shape permits Circle, Rect {}
public final class Circle implements Shape {}
public non-sealed class Rect implements Shape {}
";
        let mut db = db_with_files(&[(1, iface_file), (2, types_file)]);
        let mut cp = ClasspathIndex::default();
        cp.insert_type(binary_type(
            "java.lang.Object",
            TypeKind::Class,
            None,
            &[],
            vec![BinaryMemberSymbol {
                owner_type: "java.lang.Object".to_string(),
                name: SmolStr::new("toString"),
                kind: MemberKind::Method,
                is_public: true,
                is_static: false,
                is_abstract: false,
                nullness_scope: crate::NullnessScope::Inherited,
                decl_nullability: None,
                type_params: Vec::new(),
                type_param_bounds: BTreeMap::new(),
                type_ref: Some("java.lang.String".to_string()),
                params: vec![],
                is_varargs: false,
            }],
        ));
        cp.insert_type(binary_type(
            "java.util.Collection",
            TypeKind::Interface,
            Some("java.lang.Object"),
            &[],
            vec![],
        ));
        cp.insert_type(binary_type(
            "java.util.List",
            TypeKind::Interface,
            Some("java.lang.Object"),
            &["java.util.Collection"],
            vec![],
        ));
        cp.finalize();
        db.set_classpath_index(Arc::new(cp));

        // Verify FileItemTree extraction flags.
        let iface_tree = db.file_item_tree(FileId(1));
        let (_, repo_ty) = iface_tree.find_type_by_fqn("com.api.Repo").unwrap();
        assert!(repo_ty.is_abstract);
        assert!(!repo_ty.is_final);
        let count_m = &iface_tree.members[repo_ty.members[0]];
        assert!(count_m.is_default);
        assert!(!count_m.is_abstract);
        let save_m = &iface_tree.members[repo_ty.members[1]];
        assert!(!save_m.is_default);
        assert!(save_m.is_abstract);

        let types_tree = db.file_item_tree(FileId(2));
        let (_, base_ty) = types_tree.find_type_by_fqn("com.model.BaseEntity").unwrap();
        assert!(base_ty.is_abstract);
        assert!(!base_ty.is_final);
        let (_, user_ty) = types_tree.find_type_by_fqn("com.model.User").unwrap();
        assert_eq!(user_ty.extends, vec!["BaseEntity"]);
        assert_eq!(user_ty.implements, vec!["Repo<String>", "List<String>"]);
        assert!(types_tree.members[user_ty.members[0]].has_override);

        let (_, point_ty) = types_tree.find_type_by_fqn("com.model.Point").unwrap();
        assert!(point_ty.is_final);
        assert!(types_tree.members[point_ty.members[2]].has_override);

        let (_, shape_ty) = types_tree.find_type_by_fqn("com.model.Shape").unwrap();
        assert_eq!(shape_ty.permits, vec!["Circle", "Rect"]);

        // Direct & linearized supertypes.
        assert_eq!(
            direct_supertypes(&db, "com.model.BaseEntity"),
            DirectSupertypes {
                super_class: Some("java.lang.Object".to_string()),
                interfaces: vec![],
                permits: vec![],
            }
        );
        assert_eq!(
            direct_supertypes(&db, "com.model.Role").super_class.as_deref(),
            Some("java.lang.Enum")
        );
        assert_eq!(
            direct_supertypes(&db, "com.model.Point").super_class.as_deref(),
            Some("java.lang.Record")
        );
        assert_eq!(
            direct_supertypes(&db, "com.model.Tag"),
            DirectSupertypes {
                super_class: None,
                interfaces: vec!["java.lang.annotation.Annotation".to_string()],
                permits: vec![],
            }
        );
        assert_eq!(
            direct_supertypes(&db, "java.util.List"),
            DirectSupertypes {
                super_class: None,
                interfaces: vec!["java.util.Collection".to_string()],
                permits: vec![],
            }
        );
        assert_eq!(
            direct_supertypes(&db, "java.lang.Object"),
            DirectSupertypes::default()
        );
        assert!(linearized_supertypes(&db, "java.lang.Object").is_empty());

        // Superclass chain first, then interfaces BFS, java.lang.Object last.
        assert_eq!(
            linearized_supertypes(&db, "com.model.User"),
            vec![
                "com.model.BaseEntity",
                "com.api.Repo",
                "java.util.List",
                "java.util.Collection",
                "java.lang.Object",
            ]
        );

        // Subtype queries.
        assert_eq!(
            direct_subtypes(&db, "com.api.Repo"),
            vec!["com.model.Point", "com.model.User"]
        );
        assert_eq!(
            all_subtypes(&db, "com.model.Shape"),
            vec!["com.model.Circle", "com.model.Rect"]
        );
        assert_eq!(
            all_subtypes(&db, "java.util.Collection"),
            vec!["com.model.User"]
        );

        // Override discovery.
        assert_eq!(
            find_overridden_method(&db, "com.model.User", "save", 1).as_deref(),
            Some("com.api.Repo.save(T)")
        );
        let overrides = find_overriding_methods(&db, "com.api.Repo", "save", 1);
        assert_eq!(overrides.len(), 2);
        let override_owners: Vec<_> = overrides
            .iter()
            .filter_map(|&(fid, mid)| db.file_item_tree(fid).members[mid].owner_type.clone())
            .collect();
        assert_eq!(override_owners, vec!["com.model.Point", "com.model.User"]);
    }

    #[test]
    fn cycle_safe_inheritance_never_loops_or_panics() {
        let src = "\
package cyc;
class A extends B {}
class B extends A {}
class Self extends Self {}
interface I extends J {}
interface J extends I {}
";
        let (db, _) = db_with(src);

        assert_eq!(
            linearized_supertypes(&db, "cyc.A"),
            vec!["cyc.B", "java.lang.Object"]
        );
        assert_eq!(
            linearized_supertypes(&db, "cyc.Self"),
            vec!["java.lang.Object"]
        );
        assert_eq!(
            linearized_supertypes(&db, "cyc.I"),
            vec!["cyc.J", "java.lang.Object"]
        );

        assert_eq!(all_subtypes(&db, "cyc.A"), vec!["cyc.B"]);
        assert_eq!(all_subtypes(&db, "cyc.B"), vec!["cyc.A"]);
        assert!(all_subtypes(&db, "cyc.Self").is_empty());
        assert_eq!(all_subtypes(&db, "cyc.I"), vec!["cyc.J"]);

        assert!(find_overridden_method(&db, "cyc.A", "missing", 0).is_none());
        assert!(find_overriding_methods(&db, "cyc.A", "missing", 0).is_empty());
    }
}

