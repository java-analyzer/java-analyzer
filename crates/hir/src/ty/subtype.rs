//! Numeric promotion, supertype substitution, subtyping, assignability, and least upper bound.

use std::collections::VecDeque;

use rustc_hash::FxHashSet;
use smol_str::SmolStr;

use crate::{
    DefDatabase,
    hierarchy::linearized_supertypes_in_scope,
    scope::VisibleScope,
    symbols::TypeKind,
};

use super::{
    ANNOTATION_FQN, CLONEABLE_FQN, ENUM_FQN, MAX_SUBST_DEPTH, Nullability, OBJECT_FQN,
    PrimitiveType, RECORD_FQN, SERIALIZABLE_FQN, Substitution, Ty, TypeArg, WildcardBound,
    scope::{
        TypeResolveEnv, apply_nullness_scope, effective_nullness_scope_for_binary_type,
        effective_nullness_scope_in_file_tree, resolve_binary_type_ref_in_db,
        resolve_type_ref_in_file_inner, type_param_bounds_for_binary,
        type_param_bounds_for_workspace_item, type_params_of_fqn_in_scope,
    },
};

// ---- Numeric Promotion & Boxing ----

fn unbox_numeric(ty: &Ty) -> Option<PrimitiveType> {
    match ty {
        Ty::Primitive(p) if p.is_numeric() => Some(*p),
        Ty::Class { fqn, type_args, .. } if type_args.is_empty() => {
            PrimitiveType::from_boxed_fqn(fqn).filter(|p| p.is_numeric())
        }
        _ => None,
    }
}

/// Applies JLS §5.6 unary numeric promotion (`byte`/`short`/`char` -> `int`; handles boxed wrappers).
#[must_use]
pub(crate) fn unary_numeric_promotion(ty: &Ty) -> Option<PrimitiveType> {
    let p = unbox_numeric(ty)?;
    match p {
        PrimitiveType::Byte | PrimitiveType::Short | PrimitiveType::Char | PrimitiveType::Int => {
            Some(PrimitiveType::Int)
        }
        PrimitiveType::Long | PrimitiveType::Float | PrimitiveType::Double => Some(p),
        PrimitiveType::Boolean => None,
    }
}

/// Applies JLS §5.6 binary numeric promotion (handles unboxed and boxed numeric types).
#[must_use]
pub(crate) fn binary_numeric_promotion(a: &Ty, b: &Ty) -> Option<PrimitiveType> {
    let pa = unbox_numeric(a)?;
    let pb = unbox_numeric(b)?;
    if pa == PrimitiveType::Double || pb == PrimitiveType::Double {
        Some(PrimitiveType::Double)
    } else if pa == PrimitiveType::Float || pb == PrimitiveType::Float {
        Some(PrimitiveType::Float)
    } else if pa == PrimitiveType::Long || pb == PrimitiveType::Long {
        Some(PrimitiveType::Long)
    } else {
        Some(PrimitiveType::Int)
    }
}

// ---- Supertype Substitution ----

/// Computes the [`Substitution`] from any [`Ty`] (`Class`, `TypeVar`, `Intersection`, `Array`)
/// to `target_fqn`.
#[cfg(test)]
#[expect(dead_code, reason = "kept as convenience wrapper for unit tests")]
pub(crate) fn supertype_substitution_for_ty(
    db: &dyn DefDatabase,
    ty: &Ty,
    target_fqn: &str,
) -> Option<Substitution> {
    let scope = VisibleScope::flat(db);
    supertype_substitution_for_ty_in_scope(db, &scope, ty, target_fqn)
}

pub(crate) fn supertype_substitution_for_ty_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    ty: &Ty,
    target_fqn: &str,
) -> Option<Substitution> {
    supertype_substitution_for_ty_inner(db, scope, ty, target_fqn, 0)
}

fn supertype_substitution_for_ty_inner(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    ty: &Ty,
    target_fqn: &str,
    depth: usize,
) -> Option<Substitution> {
    if depth >= MAX_SUBST_DEPTH {
        return None;
    }
    match ty {
        Ty::Class { fqn, type_args, .. } => {
            supertype_substitution_in_scope(db, scope, fqn, type_args, target_fqn)
        }
        Ty::TypeVar { bounds, .. } => {
            if bounds.is_empty() {
                (target_fqn == OBJECT_FQN).then(Substitution::default)
            } else {
                bounds.iter().find_map(|b| {
                    supertype_substitution_for_ty_inner(db, scope, b, target_fqn, depth + 1)
                })
            }
        }
        Ty::Intersection(members) => members.iter().find_map(|m| {
            supertype_substitution_for_ty_inner(db, scope, m, target_fqn, depth + 1)
        }),
        Ty::Array { .. } => matches!(
            target_fqn,
            OBJECT_FQN | CLONEABLE_FQN | SERIALIZABLE_FQN
        )
        .then(Substitution::default),
        _ => None,
    }
}

/// Walks the inheritance graph from `sub_fqn<sub_args>` to `target_fqn` and computes the
/// instantiated [`Substitution`] mapping `target_fqn`'s type parameters to their substituted [`TypeArg`]s.
#[cfg(test)]
#[must_use]
pub(crate) fn supertype_substitution(
    db: &dyn DefDatabase,
    sub_fqn: &str,
    sub_args: &[TypeArg],
    target_fqn: &str,
) -> Option<Substitution> {
    let scope = VisibleScope::flat(db);
    supertype_substitution_in_scope(db, &scope, sub_fqn, sub_args, target_fqn)
}

#[must_use]
pub(crate) fn supertype_substitution_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    sub_fqn: &str,
    sub_args: &[TypeArg],
    target_fqn: &str,
) -> Option<Substitution> {
    if sub_fqn == target_fqn {
        let params = type_params_of_fqn_in_scope(db, scope, sub_fqn);
        if !params.is_empty() && sub_args.is_empty() {
            return Some(Substitution::default());
        }
        return Some(Substitution::from_params_and_args(&params, sub_args));
    }

    let mut visited = FxHashSet::default();
    visited.insert(sub_fqn.to_string());

    let mut queue: VecDeque<(String, Vec<TypeArg>, usize)> = VecDeque::new();
    queue.push_back((sub_fqn.to_string(), sub_args.to_vec(), 0));

    let mut start_exists = false;

    while let Some((cur_fqn, cur_args, depth)) = queue.pop_front() {
        if depth >= MAX_SUBST_DEPTH {
            continue;
        }

        let (type_params, super_tys, exists) = direct_generic_supertypes(db, scope, &cur_fqn);
        if depth == 0 {
            start_exists = exists;
        }
        if !exists {
            continue;
        }

        let is_raw = !type_params.is_empty() && cur_args.is_empty();
        let cur_subst = if is_raw {
            Substitution::default()
        } else {
            Substitution::from_params_and_args(&type_params, &cur_args)
        };

        for sup_ty in super_tys {
            let Ty::Class {
                fqn: sup_fqn,
                type_args: sup_raw_args,
                ..
            } = sup_ty
            else {
                continue;
            };
            let next_args: Vec<TypeArg> = if is_raw {
                Vec::new()
            } else {
                sup_raw_args
                    .iter()
                    .map(|arg| cur_subst.apply_arg(arg))
                    .collect()
            };

            if sup_fqn == target_fqn {
                let target_params = type_params_of_fqn_in_scope(db, scope, target_fqn);
                if !target_params.is_empty() && next_args.is_empty() {
                    return Some(Substitution::default());
                }
                return Some(Substitution::from_params_and_args(
                    &target_params,
                    &next_args,
                ));
            }

            if visited.insert(sup_fqn.clone()) {
                queue.push_back((sup_fqn, next_args, depth + 1));
            }
        }
    }

    if target_fqn == OBJECT_FQN && start_exists {
        return Some(Substitution::default());
    }

    None
}

fn direct_generic_supertypes(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    fqn: &str,
) -> (Vec<SmolStr>, Vec<Ty>, bool) {
    if let Some((fid, tid)) = scope.get_workspace_type(fqn) {
        let tree = db.file_item_tree(fid);
        let ty_item = &tree.types[tid];
        let owned_target_scope;
        let resolve_scope = if !scope.is_scoped() || scope.caller_file() == Some(fid) {
            scope
        } else {
            owned_target_scope = VisibleScope::for_file(db, fid);
            &owned_target_scope
        };
        let tp_scope = type_param_bounds_for_workspace_item(db, resolve_scope, &tree, ty_item);
        let eff_scope = effective_nullness_scope_in_file_tree(db, &tree, resolve_scope, ty_item);
        let mut out = Vec::new();
        let mut visited_tv = FxHashSet::default();
        let env = TypeResolveEnv {
            db,
            file_tree: &tree,
            scope: resolve_scope,
            owner_ty: Some(ty_item),
            nullness_scope: eff_scope,
            type_params_in_scope: &tp_scope,
        };

        let mut resolve_ws_sup = |raw: &str| {
            let ty = resolve_type_ref_in_file_inner(&env, raw, &mut visited_tv, 0);
            apply_nullness_scope(ty, eff_scope, false)
        };

        match ty_item.kind {
            TypeKind::Class => {
                if let Some(first_ext) = ty_item.extends.first() {
                    out.push(resolve_ws_sup(first_ext));
                } else if ty_item.qualified_name != OBJECT_FQN {
                    out.push(Ty::object());
                }
                for raw in &ty_item.implements {
                    out.push(resolve_ws_sup(raw));
                }
            }
            TypeKind::Interface => {
                for raw in &ty_item.extends {
                    out.push(resolve_ws_sup(raw));
                }
            }
            TypeKind::Enum => {
                let self_cls = apply_nullness_scope(
                    Ty::raw_class(&ty_item.qualified_name),
                    eff_scope,
                    false,
                );
                out.push(Ty::class(ENUM_FQN, vec![TypeArg::Type(self_cls)]));
                for raw in &ty_item.implements {
                    out.push(resolve_ws_sup(raw));
                }
            }
            TypeKind::Record => {
                out.push(Ty::raw_class(RECORD_FQN));
                for raw in &ty_item.implements {
                    out.push(resolve_ws_sup(raw));
                }
            }
            TypeKind::Annotation => {
                out.push(Ty::raw_class(ANNOTATION_FQN));
            }
        }
        return (ty_item.type_params.clone(), out, true);
    }

    if let Some(bin) = scope.get_binary_type(fqn) {
        let classpath = scope.classpath();
        let tp_scope = type_param_bounds_for_binary(db, bin);
        let eff_scope = effective_nullness_scope_for_binary_type(classpath, bin);
        let mut out = Vec::new();
        let mut visited_tv = FxHashSet::default();

        let mut resolve_bin_sup = |raw: &str| {
            let ty = resolve_binary_type_ref_in_db(
                classpath,
                bin,
                eff_scope,
                raw,
                &tp_scope,
                &mut visited_tv,
                0,
            );
            apply_nullness_scope(ty, eff_scope, false)
        };

        if bin.qualified_name != OBJECT_FQN {
            match bin.kind {
                TypeKind::Interface => {}
                TypeKind::Annotation => {
                    out.push(Ty::raw_class(ANNOTATION_FQN));
                }
                TypeKind::Enum => {
                    if let Some(raw) = &bin.super_class_generic {
                        out.push(resolve_bin_sup(raw));
                    } else if let Some(sup) = &bin.super_class {
                        out.push(Ty::raw_class(sup));
                    } else {
                        let self_cls = apply_nullness_scope(
                            Ty::raw_class(&bin.qualified_name),
                            eff_scope,
                            false,
                        );
                        out.push(Ty::class(ENUM_FQN, vec![TypeArg::Type(self_cls)]));
                    }
                }
                TypeKind::Record => {
                    if let Some(raw) = &bin.super_class_generic {
                        out.push(resolve_bin_sup(raw));
                    } else {
                        out.push(Ty::raw_class(
                            bin.super_class.as_deref().unwrap_or(RECORD_FQN),
                        ));
                    }
                }
                TypeKind::Class => {
                    if let Some(raw) = &bin.super_class_generic {
                        out.push(resolve_bin_sup(raw));
                    } else {
                        out.push(Ty::raw_class(
                            bin.super_class.as_deref().unwrap_or(OBJECT_FQN),
                        ));
                    }
                }
            }
        }

        if !bin.interfaces_generic.is_empty() {
            for raw in &bin.interfaces_generic {
                out.push(resolve_bin_sup(raw));
            }
        } else {
            for iface in &bin.interfaces {
                out.push(Ty::raw_class(iface));
            }
        }

        return (bin.type_params.clone(), out, true);
    }

    if fqn == ENUM_FQN || fqn == RECORD_FQN {
        return (
            if fqn == ENUM_FQN {
                vec![SmolStr::new("E")]
            } else {
                Vec::new()
            },
            vec![Ty::object()],
            true,
        );
    }

    (Vec::new(), Vec::new(), false)
}

// ---- Subtyping, Assignability & LUB ----

/// Returns `true` if `sub` is a subtype of `sup` (JLS §4.10).
#[cfg(test)]
#[must_use]
pub(crate) fn is_subtype(db: &dyn DefDatabase, sub: &Ty, sup: &Ty) -> bool {
    let scope = VisibleScope::flat(db);
    is_subtype_in_scope(db, &scope, sub, sup)
}

#[must_use]
pub(crate) fn is_subtype_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    sub: &Ty,
    sup: &Ty,
) -> bool {
    is_subtype_inner(db, scope, sub, sup, 0)
}

const fn nullability_is_subtype(sub: Nullability, sup: Nullability) -> bool {
    match (sub, sup) {
        (Nullability::Unspecified, Nullability::Unspecified)
        | (Nullability::NonNull, Nullability::NonNull)
        | (Nullability::Nullable, Nullability::Nullable) => true,
        // `NonNull` (`Point!`) is a subtype of both `Nullable` (`Point?`) and legacy `Unspecified` (`Point`).
        (Nullability::NonNull, Nullability::Nullable | Nullability::Unspecified) => true,
        // Legacy compatibility between `Unspecified` and `Nullable`.
        (Nullability::Nullable, Nullability::Unspecified)
        | (Nullability::Unspecified, Nullability::Nullable) => true,
        // `Nullable` / `Unspecified` is NOT strictly `<: NonNull`.
        (Nullability::Nullable | Nullability::Unspecified, Nullability::NonNull) => false,
    }
}

const fn lub_nullability(a: Nullability, b: Nullability) -> Nullability {
    match (a, b) {
        (Nullability::NonNull, Nullability::NonNull) => Nullability::NonNull,
        (Nullability::Nullable, _) | (_, Nullability::Nullable) => Nullability::Nullable,
        (Nullability::Unspecified, _) | (_, Nullability::Unspecified) => Nullability::Unspecified,
    }
}

fn is_subtype_inner(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    sub: &Ty,
    sup: &Ty,
    depth: usize,
) -> bool {
    if depth >= MAX_SUBST_DEPTH {
        return false;
    }
    if matches!(sub, Ty::Unknown | Ty::Void) || matches!(sup, Ty::Unknown | Ty::Void) {
        return false;
    }
    if sub == sup {
        return true;
    }

    // Intersection on the right: `sub` must be a subtype of every member.
    if let Ty::Intersection(sup_members) = sup {
        return !sup_members.is_empty()
            && sup_members
                .iter()
                .all(|m| is_subtype_inner(db, scope, sub, m, depth + 1));
    }

    // `null` is a subtype of every nullable reference type (NOT `Point!`, `T!`, `String[]!`, or primitives).
    if matches!(sub, Ty::Null) {
        return sup.is_nullable() && sup.is_reference();
    }

    // Intersection on the left: any member of `sub` satisfies `sup`.
    if let Ty::Intersection(sub_members) = sub {
        return sub_members
            .iter()
            .any(|m| is_subtype_inner(db, scope, m, sup, depth + 1));
    }

    match (sub, sup) {
        (Ty::Primitive(a), Ty::Primitive(b)) => a == b || a.widens_to(*b),
        (
            Ty::TypeVar {
                name: sub_name,
                bounds,
                nullability: sub_null,
            },
            _,
        ) => {
            if let Ty::TypeVar {
                name: sup_name,
                nullability: sup_null,
                ..
            } = sup
                && sub_name == sup_name
            {
                return nullability_is_subtype(*sub_null, *sup_null);
            }
            let apply_null = |bound: Ty| -> Ty {
                match sub_null {
                    Nullability::Unspecified => bound,
                    n @ (Nullability::NonNull | Nullability::Nullable) => bound.with_nullability(*n),
                }
            };
            if bounds.is_empty() {
                is_subtype_inner(db, scope, &apply_null(Ty::object()), sup, depth + 1)
            } else {
                bounds
                    .iter()
                    .any(|b| is_subtype_inner(db, scope, &apply_null(b.clone()), sup, depth + 1))
            }
        }
        (
            Ty::Array {
                elem: sub_elem,
                nullability: sub_null,
            },
            Ty::Array {
                elem: sup_elem,
                nullability: sup_null,
            },
        ) => {
            if !nullability_is_subtype(*sub_null, *sup_null) {
                return false;
            }
            if sub_elem.is_primitive() || sup_elem.is_primitive() {
                sub_elem == sup_elem
            } else {
                is_subtype_inner(db, scope, sub_elem, sup_elem, depth + 1)
            }
        }
        (
            Ty::Array {
                nullability: sub_null,
                ..
            },
            Ty::Class {
                fqn,
                type_args,
                nullability: sup_null,
            },
        ) => {
            nullability_is_subtype(*sub_null, *sup_null)
                && type_args.is_empty()
                && matches!(
                    fqn.as_str(),
                    OBJECT_FQN | CLONEABLE_FQN | SERIALIZABLE_FQN
                )
        }
        (
            Ty::Class {
                fqn: sub_fqn,
                type_args: sub_args,
                nullability: sub_null,
            },
            Ty::Class {
                fqn: sup_fqn,
                type_args: sup_args,
                nullability: sup_null,
            },
        ) => {
            nullability_is_subtype(*sub_null, *sup_null)
                && is_class_subtype(db, scope, sub_fqn, sub_args, sup_fqn, sup_args, depth)
        }
        _ => false,
    }
}

fn is_class_subtype(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    sub_fqn: &str,
    sub_args: &[TypeArg],
    sup_fqn: &str,
    sup_args: &[TypeArg],
    depth: usize,
) -> bool {
    if sup_fqn == OBJECT_FQN && sup_args.is_empty() {
        return true;
    }

    if sub_fqn == sup_fqn {
        if sup_args.is_empty() {
            return true;
        }
        // Raw type compatibility: `List <: List<String>`.
        if sub_args.is_empty() {
            return true;
        }
        if sub_args.len() != sup_args.len() {
            return false;
        }
        return sub_args
            .iter()
            .zip(sup_args.iter())
            .all(|(sa, pa)| type_arg_contained_by(db, scope, sa, pa, depth + 1));
    }

    let Some(subst) = supertype_substitution_in_scope(db, scope, sub_fqn, sub_args, sup_fqn) else {
        // Fallback to linearized_supertypes when `sub_fqn` isn't indexed directly for generics
        // or if `sup_args` is empty.
        return sup_args.is_empty()
            && linearized_supertypes_in_scope(db, scope, sub_fqn)
                .iter()
                .any(|s| s == sup_fqn);
    };

    if sup_args.is_empty() {
        return true;
    }

    let sub_params = type_params_of_fqn_in_scope(db, scope, sub_fqn);
    if !sub_params.is_empty() && sub_args.is_empty() {
        // Raw subtype is compatible with any parameterization of `sup_fqn`.
        return true;
    }

    let sup_params = type_params_of_fqn_in_scope(db, scope, sup_fqn);
    if sup_params.len() != sup_args.len() {
        return false;
    }

    for (param, sup_arg) in sup_params.iter().zip(sup_args.iter()) {
        let Some(sub_arg) = subst.map.get(param) else {
            // Raw supertype path in hierarchy.
            return true;
        };
        if !type_arg_contained_by(db, scope, sub_arg, sup_arg, depth + 1) {
            return false;
        }
    }
    true
}

fn type_arg_contained_by(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    sub_arg: &TypeArg,
    sup_arg: &TypeArg,
    depth: usize,
) -> bool {
    if depth >= MAX_SUBST_DEPTH {
        return false;
    }
    match (sub_arg, sup_arg) {
        (_, TypeArg::Wildcard(WildcardBound::Unbounded)) => true,
        (TypeArg::Type(s), TypeArg::Type(t)) => s == t,
        (TypeArg::Type(s), TypeArg::Wildcard(WildcardBound::Extends(u))) => {
            is_subtype_inner(db, scope, s, u, depth + 1)
        }
        (TypeArg::Type(s), TypeArg::Wildcard(WildcardBound::Super(l))) => {
            is_subtype_inner(db, scope, l, s, depth + 1)
        }
        (
            TypeArg::Wildcard(WildcardBound::Extends(u1)),
            TypeArg::Wildcard(WildcardBound::Extends(u2)),
        ) => is_subtype_inner(db, scope, u1, u2, depth + 1),
        (
            TypeArg::Wildcard(WildcardBound::Super(l1)),
            TypeArg::Wildcard(WildcardBound::Super(l2)),
        ) => is_subtype_inner(db, scope, l2, l1, depth + 1),
        (
            TypeArg::Wildcard(WildcardBound::Unbounded),
            TypeArg::Wildcard(WildcardBound::Extends(u)),
        ) => is_subtype_inner(db, scope, &Ty::object(), u, depth + 1),
        _ => false,
    }
}

/// Returns `true` if `from` is assignment-compatible with `to` (JLS §5.2: subtyping plus
/// primitive boxing/unboxing and widening, and resilient migration from `Unspecified`
/// nullability to `NonNull` while rejecting `Null`/`Nullable` -> `NonNull`).
#[cfg(test)]
#[must_use]
pub(crate) fn is_assignable(db: &dyn DefDatabase, from: &Ty, to: &Ty) -> bool {
    let scope = VisibleScope::flat(db);
    is_assignable_in_scope(db, &scope, from, to)
}

#[must_use]
pub(crate) fn is_assignable_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    from: &Ty,
    to: &Ty,
) -> bool {
    if is_subtype_in_scope(db, scope, from, to) {
        return true;
    }
    // Resilient migration: unannotated (`Unspecified`) reference types may be assigned to
    // `NonNull` (`T!`) targets, whereas `Null` and explicitly `Nullable` (`T?`) are rejected.
    if from.nullability() == Nullability::Unspecified
        && to.nullability() == Nullability::NonNull
        && !matches!(from, Ty::Null | Ty::Unknown | Ty::Void)
    {
        let from_non_null = from.clone().with_nullability(Nullability::NonNull);
        if is_subtype_in_scope(db, scope, &from_non_null, to) {
            return true;
        }
    }
    // Boxing conversion followed by optional widening reference conversion.
    if from.is_primitive() {
        return is_subtype_in_scope(db, scope, &from.boxed(), to);
    }
    // Unboxing conversion followed by optional widening primitive conversion.
    if let Ty::Class {
        fqn,
        type_args,
        nullability,
    } = from
        && type_args.is_empty()
        && *nullability != Nullability::Nullable
        && let Some(p_from) = PrimitiveType::from_boxed_fqn(fqn)
        && let Ty::Primitive(p_to) = to
    {
        return p_from == *p_to || p_from.widens_to(*p_to);
    }
    false
}

/// Computes the least upper bound (LUB) of `a` and `b` for ternary and switch expression branches.
#[cfg(test)]
#[must_use]
pub(crate) fn least_upper_bound(db: &dyn DefDatabase, a: &Ty, b: &Ty) -> Ty {
    let scope = VisibleScope::flat(db);
    least_upper_bound_in_scope(db, &scope, a, b)
}

#[must_use]
pub(crate) fn least_upper_bound_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    a: &Ty,
    b: &Ty,
) -> Ty {
    least_upper_bound_inner(db, scope, a, b, 0)
}

fn least_upper_bound_inner(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    a: &Ty,
    b: &Ty,
    depth: usize,
) -> Ty {
    if depth >= MAX_SUBST_DEPTH {
        return Ty::object();
    }
    if matches!(a, Ty::Unknown | Ty::Void) || matches!(b, Ty::Unknown | Ty::Void) {
        return Ty::Unknown;
    }
    if a == b {
        return a.clone();
    }

    if matches!(a, Ty::Null) {
        return match b {
            Ty::Primitive(p) => Ty::raw_class(p.boxed_fqn()),
            other if other.nullability() == Nullability::NonNull => {
                other.clone().with_nullability(Nullability::Nullable)
            }
            other => other.clone(),
        };
    }
    if matches!(b, Ty::Null) {
        return match a {
            Ty::Primitive(p) => Ty::raw_class(p.boxed_fqn()),
            other if other.nullability() == Nullability::NonNull => {
                other.clone().with_nullability(Nullability::Nullable)
            }
            other => other.clone(),
        };
    }

    // Primitive & boxed-primitive ternary promotion (JLS §15.25).
    match (a, b) {
        (Ty::Primitive(pa), Ty::Primitive(pb)) => {
            if pa == pb {
                return Ty::Primitive(*pa);
            }
            if let Some(promoted) = binary_numeric_promotion(a, b) {
                return Ty::Primitive(promoted);
            }
            // Mixed boolean & numeric: box both and find reference LUB.
            let ba = pa.boxed_fqn();
            let bb = pb.boxed_fqn();
            return least_upper_bound_inner(
                db,
                scope,
                &Ty::raw_class(ba),
                &Ty::raw_class(bb),
                depth + 1,
            );
        }
        (Ty::Primitive(p), Ty::Class { fqn, type_args, .. })
        | (Ty::Class { fqn, type_args, .. }, Ty::Primitive(p)) => {
            if type_args.is_empty()
                && let Some(pb) = PrimitiveType::from_boxed_fqn(fqn)
            {
                if *p == pb {
                    return Ty::Primitive(*p);
                }
                if let Some(promoted) = binary_numeric_promotion(a, b) {
                    return Ty::Primitive(promoted);
                }
            }
            let boxed = Ty::raw_class(p.boxed_fqn());
            let other = if a.is_primitive() { b } else { a };
            return least_upper_bound_inner(db, scope, &boxed, other, depth + 1);
        }
        _ => {}
    }

    if is_subtype_in_scope(db, scope, a, b) {
        return b.clone();
    }
    if is_subtype_in_scope(db, scope, b, a) {
        return a.clone();
    }

    let merged_null = lub_nullability(a.nullability(), b.nullability());

    match (a, b) {
        (Ty::Array { elem: ea, .. }, Ty::Array { elem: eb, .. }) => {
            if ea.is_primitive() || eb.is_primitive() {
                Ty::object().with_nullability(merged_null)
            } else {
                Ty::array(least_upper_bound_inner(db, scope, ea, eb, depth + 1))
                    .with_nullability(merged_null)
            }
        }
        (
            Ty::Class {
                fqn: fa,
                type_args: args_a,
                ..
            },
            Ty::Class {
                fqn: fb,
                type_args: args_b,
                ..
            },
        ) => {
            let mut b_supers: FxHashSet<String> = FxHashSet::default();
            b_supers.insert(fb.clone());
            for s in linearized_supertypes_in_scope(db, scope, fb) {
                b_supers.insert(s);
            }

            let mut a_chain = Vec::with_capacity(1);
            a_chain.push(fa.clone());
            a_chain.extend(linearized_supertypes_in_scope(db, scope, fa));

            for common_fqn in a_chain {
                if !b_supers.contains(&common_fqn) {
                    continue;
                }
                let params = type_params_of_fqn_in_scope(db, scope, &common_fqn);
                if params.is_empty() {
                    return Ty::raw_class(common_fqn).with_nullability(merged_null);
                }
                let Some(subst_a) =
                    supertype_substitution_in_scope(db, scope, fa, args_a, &common_fqn)
                else {
                    return Ty::raw_class(common_fqn).with_nullability(merged_null);
                };
                let Some(subst_b) =
                    supertype_substitution_in_scope(db, scope, fb, args_b, &common_fqn)
                else {
                    return Ty::raw_class(common_fqn).with_nullability(merged_null);
                };
                if subst_a.map.is_empty() || subst_b.map.is_empty() {
                    return Ty::raw_class(common_fqn).with_nullability(merged_null);
                }
                let mut merged_args = Vec::with_capacity(params.len());
                for p in &params {
                    let (Some(arg_a), Some(arg_b)) = (subst_a.map.get(p), subst_b.map.get(p))
                    else {
                        return Ty::raw_class(common_fqn).with_nullability(merged_null);
                    };
                    merged_args.push(lub_type_arg(db, scope, arg_a, arg_b, depth + 1));
                }
                return Ty::class(common_fqn, merged_args).with_nullability(merged_null);
            }

            Ty::object().with_nullability(merged_null)
        }
        (
            Ty::TypeVar {
                bounds,
                nullability,
                ..
            },
            other,
        )
        | (
            other,
            Ty::TypeVar {
                bounds,
                nullability,
                ..
            },
        ) => {
            let upper = bounds
                .first()
                .cloned()
                .unwrap_or_else(Ty::object)
                .with_nullability(*nullability);
            least_upper_bound_inner(db, scope, &upper, other, depth + 1)
        }
        _ => Ty::object().with_nullability(merged_null),
    }
}

fn lub_type_arg(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    a: &TypeArg,
    b: &TypeArg,
    depth: usize,
) -> TypeArg {
    if a == b {
        return a.clone();
    }
    match (a, b) {
        (TypeArg::Type(ta), TypeArg::Type(tb)) => {
            let l = least_upper_bound_inner(db, scope, ta, tb, depth + 1);
            if l == Ty::object() || matches!(l, Ty::Unknown) {
                TypeArg::Wildcard(WildcardBound::Unbounded)
            } else {
                TypeArg::Wildcard(WildcardBound::Extends(Box::new(l)))
            }
        }
        (
            TypeArg::Wildcard(WildcardBound::Super(la)),
            TypeArg::Wildcard(WildcardBound::Super(lb)),
        ) if la == lb => TypeArg::Wildcard(WildcardBound::Super(la.clone())),
        _ => {
            let ua = type_arg_upper(a);
            let ub = type_arg_upper(b);
            match (ua, ub) {
                (Some(ta), Some(tb)) => {
                    let l = least_upper_bound_inner(db, scope, ta, tb, depth + 1);
                    if l == Ty::object() || matches!(l, Ty::Unknown) {
                        TypeArg::Wildcard(WildcardBound::Unbounded)
                    } else {
                        TypeArg::Wildcard(WildcardBound::Extends(Box::new(l)))
                    }
                }
                _ => TypeArg::Wildcard(WildcardBound::Unbounded),
            }
        }
    }
}

fn type_arg_upper(arg: &TypeArg) -> Option<&Ty> {
    match arg {
        TypeArg::Type(t) => Some(t),
        TypeArg::Wildcard(WildcardBound::Extends(u)) => Some(u),
        TypeArg::Wildcard(WildcardBound::Super(_) | WildcardBound::Unbounded) => None,
    }
}
