//! Scope-aware type reference resolution, JSpecify nullness scope inheritance, and type-parameter lookup.

use std::collections::BTreeMap;

use rustc_hash::FxHashSet;
use smol_str::SmolStr;

use crate::{
    DefDatabase, FileId,
    classpath::{BinaryOrigin, BinaryTypeSymbol, ClasspathIndex},
    hierarchy::linearized_supertypes,
    item_tree::{FileItemTree, TypeItem},
    symbols::{ImportKind, NullnessScope, TypeKind, WorkspaceIndex},
};

use super::{MAX_SUBST_DEPTH, Nullability, Ty, TypeArg, WildcardBound, parse::parse_type_ref};

pub(super) struct TypeResolveEnv<'a> {
    pub(super) db: &'a dyn DefDatabase,
    pub(super) file_tree: &'a FileItemTree,
    pub(super) workspace: &'a WorkspaceIndex,
    pub(super) classpath: &'a ClasspathIndex,
    pub(super) owner_ty: Option<&'a TypeItem>,
    pub(super) nullness_scope: NullnessScope,
    pub(super) type_params_in_scope: &'a BTreeMap<SmolStr, Vec<String>>,
}

// ---- Nullness Scope Resolution & JSpecify Defaulting ----

/// Resolves the effective lexical [`NullnessScope`] at file/package/module level for `file_id`.
#[must_use]
pub fn effective_nullness_scope_for_file(db: &dyn DefDatabase, file_id: FileId) -> NullnessScope {
    let file_tree = db.file_item_tree(file_id);
    if file_tree.nullness_scope != NullnessScope::Inherited {
        return file_tree.nullness_scope;
    }
    let workspace = db.workspace_index();
    let pkg = file_tree.package.as_deref().unwrap_or("");
    let pkg_scope = workspace.package_nullness(pkg);
    if pkg_scope != NullnessScope::Inherited {
        return pkg_scope;
    }
    if let Some(mod_name) = workspace.file_module(file_id) {
        let mod_scope = workspace.module_nullness(mod_name);
        if mod_scope != NullnessScope::Inherited {
            return mod_scope;
        }
    }
    NullnessScope::Inherited
}

pub(super) fn effective_nullness_scope_in_file_tree(
    db: &dyn DefDatabase,
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    start_ty: &TypeItem,
) -> NullnessScope {
    let mut visited = FxHashSet::default();
    let mut cur = start_ty.clone();
    for _ in 0..MAX_SUBST_DEPTH {
        if cur.nullness_scope != NullnessScope::Inherited {
            return cur.nullness_scope;
        }
        let Some(enc_fqn) = cur.enclosing_type.clone() else {
            break;
        };
        if !visited.insert(enc_fqn.clone()) {
            break;
        }
        if let Some((_, enc_ty)) = file_tree.find_type_by_fqn(&enc_fqn) {
            cur = enc_ty.clone();
        } else if let Some((enc_fid, enc_tid)) = workspace.get_type(&enc_fqn) {
            cur = db.file_item_tree(enc_fid).types[enc_tid].clone();
        } else {
            break;
        }
    }
    effective_nullness_scope_for_file(db, file_tree.file_id)
}

pub(super) fn effective_nullness_scope_for_binary_type(
    classpath: &ClasspathIndex,
    bin: &BinaryTypeSymbol,
) -> NullnessScope {
    let mut visited = FxHashSet::default();
    visited.insert(bin.qualified_name.clone());
    let mut cur = bin;
    for _ in 0..MAX_SUBST_DEPTH {
        if cur.nullness_scope != NullnessScope::Inherited {
            return cur.nullness_scope;
        }
        let Some(enc_fqn) = cur.enclosing_type.as_deref() else {
            break;
        };
        if !visited.insert(enc_fqn.to_string()) {
            break;
        }
        let Some(enc_bin) = classpath.type_by_fqn(enc_fqn) else {
            break;
        };
        cur = enc_bin;
    }

    let pkg_scope = classpath.package_nullness(&bin.package);
    if pkg_scope != NullnessScope::Inherited {
        return pkg_scope;
    }

    match &bin.origin {
        BinaryOrigin::Jdk {
            module_name: Some(mod_name),
        } => {
            if let Some(mod_info) = classpath.modules.get(mod_name.as_str())
                && mod_info.nullness_scope != NullnessScope::Inherited
            {
                return mod_info.nullness_scope;
            }
        }
        BinaryOrigin::Jar { .. } | BinaryOrigin::Jdk { module_name: None } => {
            if let Some(mod_info) = classpath.modules.values().find(|m| {
                m.nullness_scope != NullnessScope::Inherited
                    && m.exports.iter().any(|exp| exp == &bin.package)
            }) {
                return mod_info.nullness_scope;
            }
        }
    }

    NullnessScope::Inherited
}

/// Resolves the effective lexical [`NullnessScope`] for `owner_fqn`, walking:
/// inner type -> enclosing outer types (cycle-guarded) -> package -> module.
#[must_use]
pub fn effective_nullness_scope_for_type(db: &dyn DefDatabase, owner_fqn: &str) -> NullnessScope {
    let workspace = db.workspace_index();
    if let Some((fid, tid)) = workspace.get_type(owner_fqn) {
        let mut visited = FxHashSet::default();
        visited.insert(owner_fqn.to_string());
        let mut cur_fid = fid;
        let mut cur_tid = tid;
        for _ in 0..MAX_SUBST_DEPTH {
            let tree = db.file_item_tree(cur_fid);
            let ty = &tree.types[cur_tid];
            if ty.nullness_scope != NullnessScope::Inherited {
                return ty.nullness_scope;
            }
            let Some(enc_fqn) = ty.enclosing_type.clone() else {
                return effective_nullness_scope_for_file(db, cur_fid);
            };
            if !visited.insert(enc_fqn.clone()) {
                break;
            }
            if let Some((enc_tid, _)) = tree.find_type_by_fqn(&enc_fqn) {
                cur_tid = enc_tid;
            } else if let Some((enc_fid, enc_tid)) = workspace.get_type(&enc_fqn) {
                cur_fid = enc_fid;
                cur_tid = enc_tid;
            } else {
                return effective_nullness_scope_for_file(db, cur_fid);
            }
        }
        return effective_nullness_scope_for_file(db, fid);
    }

    let classpath = db.classpath_index();
    if let Some(bin) = classpath.type_by_fqn(owner_fqn) {
        return effective_nullness_scope_for_binary_type(&classpath, bin);
    }

    NullnessScope::Inherited
}

/// Resolves the effective lexical [`NullnessScope`] for a member declared in `owner_fqn`.
#[must_use]
pub fn effective_nullness_scope_for_member(
    db: &dyn DefDatabase,
    owner_fqn: &str,
    member_scope: NullnessScope,
) -> NullnessScope {
    if member_scope != NullnessScope::Inherited {
        return member_scope;
    }
    effective_nullness_scope_for_type(db, owner_fqn)
}

pub(crate) fn effective_nullness_scope_for_binary_member(
    classpath: &ClasspathIndex,
    owner: &BinaryTypeSymbol,
    member_scope: NullnessScope,
) -> NullnessScope {
    if member_scope != NullnessScope::Inherited {
        return member_scope;
    }
    effective_nullness_scope_for_binary_type(classpath, owner)
}

/// Applies JSpecify 1.0 defaulting rules for `scope` to `ty`.
///
/// When `is_local_root` is `true` (explicit local-variable declarations per JSpecify §3.2.5),
/// the outermost class/array root is not forced to [`Nullability::NonNull`], while nested type
/// arguments and array element types still receive defaulting.
#[must_use]
pub fn apply_nullness_scope(ty: Ty, scope: NullnessScope, is_local_root: bool) -> Ty {
    apply_nullness_scope_inner(ty, scope, is_local_root, 0)
}

fn apply_nullness_scope_inner(
    ty: Ty,
    scope: NullnessScope,
    is_local_root: bool,
    depth: usize,
) -> Ty {
    if depth >= MAX_SUBST_DEPTH || !matches!(scope, NullnessScope::NonNullByDefault(_)) {
        return ty;
    }
    match ty {
        Ty::Class {
            fqn,
            type_args,
            nullability,
        } => {
            let nullability = if !is_local_root && nullability == Nullability::Unspecified {
                Nullability::NonNull
            } else {
                nullability
            };
            let type_args = type_args
                .into_iter()
                .map(|arg| apply_nullness_scope_arg(arg, scope, depth + 1))
                .collect();
            Ty::Class {
                fqn,
                type_args,
                nullability,
            }
        }
        Ty::Array { elem, nullability } => {
            let nullability = if !is_local_root && nullability == Nullability::Unspecified {
                Nullability::NonNull
            } else {
                nullability
            };
            let elem = Box::new(apply_nullness_scope_inner(*elem, scope, false, depth + 1));
            Ty::Array { elem, nullability }
        }
        // JSpecify parametric rule: unannotated `TypeVar` usages stay `Unspecified` so substitution
        // inherits the type argument's nullability verbatim.
        Ty::TypeVar { .. } => ty,
        Ty::Intersection(members) => Ty::Intersection(
            members
                .into_iter()
                .map(|m| apply_nullness_scope_inner(m, scope, is_local_root, depth + 1))
                .collect(),
        ),
        Ty::Primitive(_) | Ty::Void | Ty::Null | Ty::Unknown => ty,
    }
}

fn apply_nullness_scope_arg(arg: TypeArg, scope: NullnessScope, depth: usize) -> TypeArg {
    if depth >= MAX_SUBST_DEPTH {
        return arg;
    }
    match arg {
        TypeArg::Type(t) => TypeArg::Type(apply_nullness_scope_inner(t, scope, false, depth)),
        TypeArg::Wildcard(WildcardBound::Unbounded) => TypeArg::Wildcard(WildcardBound::Unbounded),
        TypeArg::Wildcard(WildcardBound::Extends(t)) => TypeArg::Wildcard(WildcardBound::Extends(
            Box::new(apply_nullness_scope_inner(*t, scope, false, depth)),
        )),
        TypeArg::Wildcard(WildcardBound::Super(t)) => TypeArg::Wildcard(WildcardBound::Super(
            Box::new(apply_nullness_scope_inner(*t, scope, false, depth)),
        )),
    }
}

pub(crate) fn ty_has_explicit_nullability(ty: &Ty) -> bool {
    ty_has_explicit_nullability_inner(ty, 0)
}

fn ty_has_explicit_nullability_inner(ty: &Ty, depth: usize) -> bool {
    if depth >= MAX_SUBST_DEPTH {
        return false;
    }
    match ty {
        Ty::Class {
            type_args,
            nullability,
            ..
        } => {
            *nullability != Nullability::Unspecified
                || type_args.iter().any(|arg| match arg {
                    TypeArg::Type(t) => ty_has_explicit_nullability_inner(t, depth + 1),
                    TypeArg::Wildcard(WildcardBound::Extends(t) | WildcardBound::Super(t)) => {
                        ty_has_explicit_nullability_inner(t, depth + 1)
                    }
                    TypeArg::Wildcard(WildcardBound::Unbounded) => false,
                })
        }
        Ty::Array { elem, nullability } => {
            *nullability != Nullability::Unspecified
                || ty_has_explicit_nullability_inner(elem, depth + 1)
        }
        Ty::TypeVar { nullability, .. } => *nullability != Nullability::Unspecified,
        Ty::Intersection(members) => members
            .iter()
            .any(|m| ty_has_explicit_nullability_inner(m, depth + 1)),
        Ty::Primitive(_) | Ty::Void | Ty::Null | Ty::Unknown => false,
    }
}

// ---- Scope-Aware Type Resolution ----

/// Resolves a raw source type reference in `file_id` given `type_params_in_scope`.
#[cfg(test)]
#[must_use]
pub(super) fn resolve_type_ref_in_file(
    db: &dyn DefDatabase,
    file_id: FileId,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    resolve_type_ref_in_file_for_owner(db, file_id, None, raw, type_params_in_scope)
}

/// Resolves a raw source type reference in `file_id` within `owner_fqn`'s scope.
pub(crate) fn resolve_type_ref_in_file_for_owner(
    db: &dyn DefDatabase,
    file_id: FileId,
    owner_fqn: Option<&str>,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    resolve_type_ref_in_file_for_member(
        db,
        file_id,
        owner_fqn,
        NullnessScope::Inherited,
        raw,
        type_params_in_scope,
    )
}

/// Resolves a raw source type reference in `file_id` within a member's effective nullness scope.
pub(crate) fn resolve_type_ref_in_file_for_member(
    db: &dyn DefDatabase,
    file_id: FileId,
    owner_fqn: Option<&str>,
    member_scope: NullnessScope,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    let file_tree = db.file_item_tree(file_id);
    let workspace = db.workspace_index();
    let scope = if member_scope != NullnessScope::Inherited {
        member_scope
    } else if let Some(fqn) = owner_fqn {
        let s = effective_nullness_scope_for_type(db, fqn);
        if s != NullnessScope::Inherited {
            s
        } else if let Some((_, ty)) = file_tree.find_type_by_fqn(fqn) {
            effective_nullness_scope_in_file_tree(db, &file_tree, &workspace, ty)
        } else {
            effective_nullness_scope_for_file(db, file_id)
        }
    } else {
        effective_nullness_scope_for_file(db, file_id)
    };
    resolve_type_ref_in_file_with_scope(
        db,
        file_id,
        owner_fqn,
        scope,
        false,
        raw,
        type_params_in_scope,
    )
}

/// Resolves a raw source type reference in `file_id` with an explicit [`NullnessScope`] and `is_local_root`.
pub(crate) fn resolve_type_ref_in_file_with_scope(
    db: &dyn DefDatabase,
    file_id: FileId,
    owner_fqn: Option<&str>,
    nullness_scope: NullnessScope,
    is_local_root: bool,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    let file_tree = db.file_item_tree(file_id);
    let workspace = db.workspace_index();
    let classpath = db.classpath_index();
    let owner_ty = owner_fqn.and_then(|fqn| file_tree.find_type_by_fqn(fqn).map(|(_, t)| t));
    let mut visited = FxHashSet::default();
    let env = TypeResolveEnv {
        db,
        file_tree: &file_tree,
        workspace: &workspace,
        classpath: &classpath,
        owner_ty,
        nullness_scope,
        type_params_in_scope,
    };
    let ty = resolve_type_ref_in_file_inner(&env, raw, &mut visited, 0);
    apply_nullness_scope(ty, nullness_scope, is_local_root)
}

/// Resolves a raw binary type reference in `owner`'s scope against `classpath`.
pub(crate) fn resolve_binary_type_ref_for_owner(
    classpath: &ClasspathIndex,
    owner: &BinaryTypeSymbol,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    resolve_binary_type_ref_for_member(
        classpath,
        owner,
        NullnessScope::Inherited,
        raw,
        type_params_in_scope,
    )
}

/// Resolves a raw binary type reference in a binary member's effective scope against `classpath`.
pub(crate) fn resolve_binary_type_ref_for_member(
    classpath: &ClasspathIndex,
    owner: &BinaryTypeSymbol,
    member_scope: NullnessScope,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    let scope = effective_nullness_scope_for_binary_member(classpath, owner, member_scope);
    let mut visited = FxHashSet::default();
    let ty = resolve_binary_type_ref_in_db(
        classpath,
        owner,
        scope,
        raw,
        type_params_in_scope,
        &mut visited,
        0,
    );
    apply_nullness_scope(ty, scope, false)
}

/// Resolves a raw binary type reference (as decoded by `classfile`) given `type_params_in_scope`.
#[cfg(test)]
#[must_use]
pub(super) fn resolve_binary_type_ref(
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    let mut visited = FxHashSet::default();
    resolve_binary_type_ref_inner(raw, type_params_in_scope, &mut visited, 0)
}

#[cfg(test)]
fn resolve_binary_type_ref_inner(
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
    visited: &mut FxHashSet<SmolStr>,
    depth: usize,
) -> Ty {
    parse_type_ref(raw, &mut |segs| {
        if let [name] = segs
            && let Some(raw_bounds) = type_params_in_scope.get(*name)
        {
            let tv_name = SmolStr::new(name);
            let bounds = if depth < MAX_SUBST_DEPTH && visited.insert(tv_name.clone()) {
                let resolved: Vec<Ty> = raw_bounds
                    .iter()
                    .map(|b| {
                        resolve_binary_type_ref_inner(b, type_params_in_scope, visited, depth + 1)
                    })
                    .filter(|t| !matches!(t, Ty::Unknown))
                    .collect();
                visited.remove(&tv_name);
                resolved
            } else {
                Vec::new()
            };
            return Some(Ty::type_var(tv_name, bounds));
        }

        let fqn = match segs {
            [name] => format!("java.lang.{name}"),
            [first, ..] if is_known_java_lang_nested_outer(first) => {
                format!("java.lang.{}", segs.join("."))
            }
            _ => segs.join("."),
        };
        Some(Ty::raw_class(fqn))
    })
}

fn is_known_java_lang_nested_outer(name: &str) -> bool {
    matches!(
        name,
        "Character" | "Thread" | "ProcessBuilder" | "StackWalker" | "ModuleLayer" | "System"
    )
}

pub(super) fn resolve_type_ref_in_file_inner(
    env: &TypeResolveEnv<'_>,
    raw: &str,
    visited: &mut FxHashSet<SmolStr>,
    depth: usize,
) -> Ty {
    parse_type_ref(raw, &mut |segs| {
        if let [name] = segs
            && let Some(raw_bounds) = env.type_params_in_scope.get(*name)
        {
            let tv_name = SmolStr::new(name);
            let bounds = if depth < MAX_SUBST_DEPTH && visited.insert(tv_name.clone()) {
                let resolved = if raw_bounds.is_empty() {
                    if matches!(env.nullness_scope, NullnessScope::NonNullByDefault(_)) {
                        vec![Ty::object().with_nullability(Nullability::NonNull)]
                    } else {
                        Vec::new()
                    }
                } else {
                    raw_bounds
                        .iter()
                        .map(|b| {
                            let ty = resolve_type_ref_in_file_inner(env, b, visited, depth + 1);
                            apply_nullness_scope(ty, env.nullness_scope, false)
                        })
                        .filter(|t| !matches!(t, Ty::Unknown))
                        .collect()
                };
                visited.remove(&tv_name);
                resolved
            } else {
                Vec::new()
            };
            return Some(Ty::type_var(tv_name, bounds));
        }

        let fqn = resolve_class_segments_in_file(
            env.db,
            env.file_tree,
            env.workspace,
            env.classpath,
            env.owner_ty,
            segs,
        )?;
        Some(Ty::raw_class(fqn))
    })
}

pub(super) fn resolve_binary_type_ref_in_db(
    classpath: &ClasspathIndex,
    owner: &BinaryTypeSymbol,
    nullness_scope: NullnessScope,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
    visited: &mut FxHashSet<SmolStr>,
    depth: usize,
) -> Ty {
    parse_type_ref(raw, &mut |segs| {
        if let [name] = segs
            && let Some(raw_bounds) = type_params_in_scope.get(*name)
        {
            let tv_name = SmolStr::new(name);
            let bounds = if depth < MAX_SUBST_DEPTH && visited.insert(tv_name.clone()) {
                let resolved = if raw_bounds.is_empty() {
                    if matches!(nullness_scope, NullnessScope::NonNullByDefault(_)) {
                        vec![Ty::object().with_nullability(Nullability::NonNull)]
                    } else {
                        Vec::new()
                    }
                } else {
                    raw_bounds
                        .iter()
                        .map(|b| {
                            let ty = resolve_binary_type_ref_in_db(
                                classpath,
                                owner,
                                nullness_scope,
                                b,
                                type_params_in_scope,
                                visited,
                                depth + 1,
                            );
                            apply_nullness_scope(ty, nullness_scope, false)
                        })
                        .filter(|t| !matches!(t, Ty::Unknown))
                        .collect()
                };
                visited.remove(&tv_name);
                resolved
            } else {
                Vec::new()
            };
            return Some(Ty::type_var(tv_name, bounds));
        }

        let joined = segs.join(".");
        if classpath.type_by_fqn(&joined).is_some() {
            return Some(Ty::raw_class(joined));
        }
        if !owner.package.is_empty() {
            let same_pkg = format!("{}.{joined}", owner.package);
            if classpath.type_by_fqn(&same_pkg).is_some() {
                return Some(Ty::raw_class(same_pkg));
            }
        }
        let java_lang = format!("java.lang.{joined}");
        if classpath.type_by_fqn(&java_lang).is_some() {
            return Some(Ty::raw_class(java_lang));
        }
        let fqn = match segs {
            [name] => format!("java.lang.{name}"),
            [first, ..] if is_known_java_lang_nested_outer(first) => java_lang,
            _ => joined,
        };
        Some(Ty::raw_class(fqn))
    })
}

fn is_known_type_fqn(
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    fqn: &str,
) -> bool {
    workspace.contains_type(fqn)
        || file_tree.find_type_by_fqn(fqn).is_some()
        || classpath.type_by_fqn(fqn).is_some()
}

fn resolve_pkg_type(
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

fn core_java_lang_fallback(name: &str) -> Option<&'static str> {
    match name {
        "Object" => Some("java.lang.Object"),
        "String" => Some("java.lang.String"),
        "Number" => Some("java.lang.Number"),
        "Boolean" => Some("java.lang.Boolean"),
        "Byte" => Some("java.lang.Byte"),
        "Short" => Some("java.lang.Short"),
        "Integer" => Some("java.lang.Integer"),
        "Long" => Some("java.lang.Long"),
        "Character" => Some("java.lang.Character"),
        "Float" => Some("java.lang.Float"),
        "Double" => Some("java.lang.Double"),
        "Void" => Some("java.lang.Void"),
        "Enum" => Some("java.lang.Enum"),
        "Record" => Some("java.lang.Record"),
        "Class" => Some("java.lang.Class"),
        "Comparable" => Some("java.lang.Comparable"),
        "Iterable" => Some("java.lang.Iterable"),
        "CharSequence" => Some("java.lang.CharSequence"),
        "Cloneable" => Some("java.lang.Cloneable"),
        "AutoCloseable" => Some("java.lang.AutoCloseable"),
        "Throwable" => Some("java.lang.Throwable"),
        "Exception" => Some("java.lang.Exception"),
        "RuntimeException" => Some("java.lang.RuntimeException"),
        "Error" => Some("java.lang.Error"),
        "Runnable" => Some("java.lang.Runnable"),
        "FunctionalInterface" => Some("java.lang.FunctionalInterface"),
        _ => None,
    }
}

fn resolve_single_segment_class(
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    owner_ty: Option<&TypeItem>,
    name: &str,
) -> Option<String> {
    if let Some(owner) = owner_ty {
        if owner.name == name {
            return Some(owner.qualified_name.clone());
        }
        if owner.nested_types.iter().any(|n| n == name) {
            let nested_fqn = format!("{}.{name}", owner.qualified_name);
            if is_known_type_fqn(file_tree, workspace, classpath, &nested_fqn) {
                return Some(nested_fqn);
            }
        }
        let mut enclosing = owner.enclosing_type.as_deref();
        let mut steps = 0usize;
        while let Some(enc_fqn) = enclosing {
            if steps >= MAX_SUBST_DEPTH {
                break;
            }
            steps += 1;
            let Some((_, enc_ty)) = file_tree.find_type_by_fqn(enc_fqn) else {
                break;
            };
            if enc_ty.name == name {
                return Some(enc_ty.qualified_name.clone());
            }
            if enc_ty.nested_types.iter().any(|n| n == name) {
                let nested_fqn = format!("{enc_fqn}.{name}");
                if is_known_type_fqn(file_tree, workspace, classpath, &nested_fqn) {
                    return Some(nested_fqn);
                }
            }
            enclosing = enc_ty.enclosing_type.as_deref();
        }
    }

    // Single-type imports (workspace first, then binary).
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

    // Static single imports resolving to a nested type.
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

    // Same-package top-level types.
    let pkg = file_tree.package.as_deref().unwrap_or("");
    if let Some(found) = resolve_pkg_type(file_tree, workspace, classpath, pkg, name) {
        return Some(found);
    }

    // Implicit `java.lang.*`.
    if let Some(found) = resolve_pkg_type(file_tree, workspace, classpath, "java.lang", name) {
        return Some(found);
    }

    // Type-import-on-demand (`import a.b.*;`).
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

    // Static-import-on-demand (`import static a.b.Outer.*;`).
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

    // Module imports (`import module M;`).
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::Module {
            let module_name = imp.path.join(".");
            for exported_pkg in classpath.transitive_module_exports(&module_name) {
                if let Some(found) =
                    resolve_pkg_type(file_tree, workspace, classpath, exported_pkg, name)
                {
                    return Some(found);
                }
            }
        }
    }

    // Nested types declared in the same file when no explicit owner was supplied.
    if owner_ty.is_none()
        && let Some((_, ty)) = file_tree
            .types
            .iter()
            .find(|(_, t)| t.enclosing_type.is_some() && t.name == name)
    {
        return Some(ty.qualified_name.clone());
    }

    // Best-effort fallback for unindexed single-type imports and core `java.lang.*` classes.
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::SingleType && imp.path.last().map(String::as_str) == Some(name) {
            return Some(imp.path.join("."));
        }
    }
    if let Some(lang_fqn) = core_java_lang_fallback(name) {
        return Some(lang_fqn.to_string());
    }

    None
}

fn resolve_class_segments_in_file(
    db: &dyn DefDatabase,
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    owner_ty: Option<&TypeItem>,
    segs: &[&str],
) -> Option<String> {
    if let [name] = segs {
        return resolve_single_segment_class(file_tree, workspace, classpath, owner_ty, name);
    }

    let joined = segs.join(".");
    if is_known_type_fqn(file_tree, workspace, classpath, &joined) {
        return Some(joined);
    }

    if let Some((first, rest)) = segs.split_first()
        && let Some(first_fqn) =
            resolve_single_segment_class(file_tree, workspace, classpath, owner_ty, first)
    {
        let mut current = first_fqn;
        let mut all_resolved = true;
        for seg in rest {
            let direct_next = format!("{current}.{seg}");
            if is_known_type_fqn(file_tree, workspace, classpath, &direct_next) {
                current = direct_next;
                continue;
            }
            let inherited = linearized_supertypes(db, &current).into_iter().find_map(|sup| {
                let candidate = format!("{sup}.{seg}");
                is_known_type_fqn(file_tree, workspace, classpath, &candidate)
                    .then_some(candidate)
            });
            if let Some(next) = inherited {
                current = next;
            } else {
                all_resolved = false;
                break;
            }
        }
        if all_resolved {
            return Some(current);
        }
    }

    Some(joined)
}

/// Returns the declared type-parameter names of `fqn` from workspace sources or the classpath.
#[must_use]
pub(crate) fn type_params_of_fqn(db: &dyn DefDatabase, fqn: &str) -> Vec<SmolStr> {
    let workspace = db.workspace_index();
    if let Some((fid, tid)) = workspace.get_type(fqn) {
        return db.file_item_tree(fid).types[tid].type_params.clone();
    }
    let classpath = db.classpath_index();
    if let Some(bin) = classpath.type_by_fqn(fqn) {
        return bin.type_params.clone();
    }
    Vec::new()
}

/// Returns the type-parameter bounds map for `fqn` (including unbounded type parameters with an
/// empty `Vec`, and non-static enclosing type parameters).
#[must_use]
pub(crate) fn type_param_bounds_of_fqn(
    db: &dyn DefDatabase,
    fqn: &str,
) -> BTreeMap<SmolStr, Vec<String>> {
    let workspace = db.workspace_index();
    if let Some((fid, tid)) = workspace.get_type(fqn) {
        let tree = db.file_item_tree(fid);
        return type_param_bounds_for_workspace_item(db, &tree, &tree.types[tid]);
    }
    let classpath = db.classpath_index();
    if let Some(bin) = classpath.type_by_fqn(fqn) {
        return type_param_bounds_for_binary(db, bin);
    }
    BTreeMap::new()
}

pub(super) fn type_param_bounds_for_workspace_item(
    db: &dyn DefDatabase,
    tree: &FileItemTree,
    ty: &TypeItem,
) -> BTreeMap<SmolStr, Vec<String>> {
    let mut out = BTreeMap::new();
    // Collect enclosing non-static type parameters outermost-to-innermost so inner shadows outer.
    let mut chain: Vec<TypeItem> = Vec::new();
    let mut cur = ty.clone();
    for _ in 0..MAX_SUBST_DEPTH {
        let is_static = cur.is_static
            || matches!(
                cur.kind,
                TypeKind::Interface | TypeKind::Enum | TypeKind::Record | TypeKind::Annotation
            );
        let enc = cur.enclosing_type.clone();
        chain.push(cur);
        if is_static {
            break;
        }
        let Some(enc_fqn) = enc else {
            break;
        };
        if let Some((_, enc_item)) = tree.find_type_by_fqn(&enc_fqn) {
            cur = enc_item.clone();
        } else if let Some((fid, tid)) = db.workspace_index().get_type(&enc_fqn) {
            cur = db.file_item_tree(fid).types[tid].clone();
        } else {
            break;
        }
    }
    for item in chain.into_iter().rev() {
        for param in &item.type_params {
            let bounds = item
                .type_param_bounds
                .get(param)
                .cloned()
                .unwrap_or_default();
            out.insert(param.clone(), bounds);
        }
    }
    out
}

pub(super) fn type_param_bounds_for_binary(
    db: &dyn DefDatabase,
    bin: &BinaryTypeSymbol,
) -> BTreeMap<SmolStr, Vec<String>> {
    let mut out = BTreeMap::new();
    let classpath = db.classpath_index();
    let mut chain: Vec<&BinaryTypeSymbol> = Vec::new();
    let mut cur = bin;
    for _ in 0..MAX_SUBST_DEPTH {
        let is_static = cur.is_static
            || matches!(
                cur.kind,
                TypeKind::Interface | TypeKind::Enum | TypeKind::Record | TypeKind::Annotation
            );
        chain.push(cur);
        if is_static {
            break;
        }
        let Some(enc_fqn) = cur.enclosing_type.as_deref() else {
            break;
        };
        let Some(enc_bin) = classpath.type_by_fqn(enc_fqn) else {
            break;
        };
        cur = enc_bin;
    }
    for item in chain.into_iter().rev() {
        for param in &item.type_params {
            let bounds = item
                .type_param_bounds
                .get(param)
                .cloned()
                .unwrap_or_default();
            out.insert(param.clone(), bounds);
        }
    }
    out
}
