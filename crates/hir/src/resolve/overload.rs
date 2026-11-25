use std::collections::BTreeMap;

use rustc_hash::FxHashSet;
use smol_str::SmolStr;
use syntax::{SyntaxNode, syntax_kind::SyntaxKind::*};

use crate::{
    DefDatabase,
    hierarchy::linearized_supertypes_in_scope,
    infer::{
        erase_type_vars_to_bound, find_functional_interface_sam_in_scope, is_raw_class_ty_in_scope,
        parameter_for_arg, resolve_params_in_binary_for_member, resolve_params_in_file_for_member,
    },
    scope::VisibleScope,
    symbols::{ImportKind, MemberKind, NullnessScope, ParamSummary, TypeKind},
    ty::{
        Substitution, Ty, is_assignable_in_scope, is_subtype_in_scope,
        resolve_binary_type_ref_for_member, resolve_type_ref_in_file_for_member,
        supertype_substitution_for_ty_in_scope, type_param_bounds_of_fqn_in_scope,
    },
};

use super::{ResolveCtx, ResolvedTarget, lexical::lookup_type_by_fqn};

// ---- JLS §15.12.2 Type-Based Overload Resolution ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OverloadPhase {
    Strict = 1,
    Loose = 2,
    Varargs = 3,
    Fallback = 4,
}

#[derive(Debug, Clone)]
pub(crate) struct CallableCandidate {
    pub target: ResolvedTarget,
    pub owner_fqn: String,
    pub member_nullness_scope: NullnessScope,
    pub is_static: bool,
    pub is_raw_receiver: bool,
    pub method_type_params: Vec<SmolStr>,
    pub method_type_param_bounds: BTreeMap<SmolStr, Vec<String>>,
    pub param_tys: Vec<(Ty, bool)>,
    pub return_ty: Ty,
    pub owner_subst: Substitution,
}

struct OwnerMethodLookup<'a> {
    owner_fqn: &'a str,
    name: &'a str,
    arity: usize,
    is_super: bool,
    only_static: bool,
    is_raw_receiver: bool,
    owner_subst: &'a Substitution,
}

fn arity_compatible(params: &[ParamSummary], arity: usize) -> bool {
    let is_varargs = params.last().is_some_and(|p| p.is_varargs);
    if is_varargs {
        arity + 1 >= params.len()
    } else {
        params.len() == arity
    }
}

pub(crate) fn collect_method_candidates_on_ty(
    ctx: ResolveCtx<'_>,
    recv_ty: &Ty,
    name: &str,
    arity: usize,
) -> Vec<CallableCandidate> {
    let mut roots: Vec<String> = Vec::new();
    match recv_ty {
        Ty::Array { .. } => roots.push("java.lang.Object".to_string()),
        Ty::TypeVar { bounds, .. } => {
            if bounds.is_empty() {
                roots.push("java.lang.Object".to_string());
            } else {
                for b in bounds {
                    if let Some(fqn) = b.erased_fqn() {
                        roots.push(fqn.to_string());
                    }
                }
            }
        }
        Ty::Intersection(members) => {
            for m in members {
                if let Some(fqn) = m.erased_fqn() {
                    roots.push(fqn.to_string());
                }
            }
        }
        _ => {
            if let Some(fqn) = recv_ty.erased_fqn() {
                roots.push(fqn.to_string());
            }
        }
    }

    let mut visited = FxHashSet::default();
    let mut ordered_owners: Vec<(String, bool)> = Vec::new();
    for root in roots {
        if visited.insert(root.clone()) {
            ordered_owners.push((root.clone(), false));
        }
        for sup in linearized_supertypes_in_scope(ctx.db, ctx.scope, &root) {
            if visited.insert(sup.clone()) {
                ordered_owners.push((sup, true));
            }
        }
    }

    let is_raw = is_raw_class_ty_in_scope(ctx.db, ctx.scope, recv_ty);
    let mut out = Vec::new();
    for (owner_fqn, is_super) in ordered_owners {
        let owner_subst =
            supertype_substitution_for_ty_in_scope(ctx.db, ctx.scope, recv_ty, &owner_fqn)
                .unwrap_or_default();
        collect_owner_methods(
            ctx,
            &OwnerMethodLookup {
                owner_fqn: &owner_fqn,
                name,
                arity,
                is_super,
                only_static: false,
                is_raw_receiver: is_raw,
                owner_subst: &owner_subst,
            },
            &mut out,
        );
    }
    out
}

pub(crate) fn collect_top_level_method_candidates(
    ctx: ResolveCtx<'_>,
    name: &str,
    arity: usize,
) -> Vec<CallableCandidate> {
    let mut out = Vec::new();
    for &mid in &ctx.file_tree.top_level_members {
        let m = &ctx.file_tree.members[mid];
        if m.kind != MemberKind::Method || m.name != name || !arity_compatible(&m.params, arity) {
            continue;
        }
        let mut scope = BTreeMap::new();
        for tp in &m.type_params {
            scope.insert(
                tp.clone(),
                m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
            );
        }
        let param_tys = resolve_params_in_file_for_member(
            ctx.db,
            ctx.file_id,
            None,
            m.nullness_scope,
            &m.params,
            &scope,
        );
        let return_ty = m.type_ref.as_deref().map_or(Ty::Unknown, |raw| {
            resolve_type_ref_in_file_for_member(
                ctx.db,
                ctx.file_id,
                None,
                m.nullness_scope,
                raw,
                &scope,
            )
        });
        out.push(CallableCandidate {
            target: ResolvedTarget::IndexedMember(ctx.file_id, mid),
            owner_fqn: String::new(),
            member_nullness_scope: m.nullness_scope,
            is_static: m.is_static,
            is_raw_receiver: false,
            method_type_params: m.type_params.clone(),
            method_type_param_bounds: m.type_param_bounds.clone(),
            param_tys,
            return_ty,
            owner_subst: Substitution::default(),
        });
    }
    out
}

pub(crate) fn collect_static_import_method_candidates(
    ctx: ResolveCtx<'_>,
    name: &str,
    arity: usize,
) -> Vec<CallableCandidate> {
    let mut out = Vec::new();
    let default_subst = Substitution::default();

    for (_, imp) in ctx.file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticSingle
            && imp.path.len() >= 2
            && imp.path.last().map(String::as_str) == Some(name)
        {
            let owner_fqn = imp.path[..imp.path.len() - 1].join(".");
            collect_owner_methods(
                ctx,
                &OwnerMethodLookup {
                    owner_fqn: &owner_fqn,
                    name,
                    arity,
                    is_super: false,
                    only_static: true,
                    is_raw_receiver: false,
                    owner_subst: &default_subst,
                },
                &mut out,
            );
        }
    }

    for (_, imp) in ctx.file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticOnDemand && !imp.path.is_empty() {
            let owner_fqn = imp.path.join(".");
            collect_owner_methods(
                ctx,
                &OwnerMethodLookup {
                    owner_fqn: &owner_fqn,
                    name,
                    arity,
                    is_super: false,
                    only_static: true,
                    is_raw_receiver: false,
                    owner_subst: &default_subst,
                },
                &mut out,
            );
        }
    }

    out
}

pub(crate) fn collect_constructor_candidates(
    ctx: ResolveCtx<'_>,
    owner_fqn: &str,
    arity: usize,
) -> Vec<CallableCandidate> {
    let mut out = Vec::new();
    let owner_scope = type_param_bounds_of_fqn_in_scope(ctx.db, ctx.scope, owner_fqn);

    if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, owner_fqn) {
        let tree = ctx.db.file_item_tree(fid);
        let ty_item = &tree.types[tid];
        for &mid in &ty_item.members {
            let m = &tree.members[mid];
            if m.kind != MemberKind::Constructor || !arity_compatible(&m.params, arity) {
                continue;
            }
            let mut scope = owner_scope.clone();
            for tp in &m.type_params {
                scope.insert(
                    tp.clone(),
                    m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
                );
            }
            let param_tys = resolve_params_in_file_for_member(
                ctx.db,
                fid,
                Some(owner_fqn),
                m.nullness_scope,
                &m.params,
                &scope,
            );
            out.push(CallableCandidate {
                target: ResolvedTarget::IndexedMember(fid, mid),
                owner_fqn: owner_fqn.to_string(),
                member_nullness_scope: m.nullness_scope,
                is_static: false,
                is_raw_receiver: false,
                method_type_params: m.type_params.clone(),
                method_type_param_bounds: m.type_param_bounds.clone(),
                param_tys,
                return_ty: Ty::Void,
                owner_subst: Substitution::default(),
            });
        }
        return out;
    }

    if let Some(bin) = ctx.scope.get_binary_type(owner_fqn) {
        for (member_index, m) in bin.members.iter().enumerate() {
            if m.kind != MemberKind::Constructor || !arity_compatible(&m.params, arity) {
                continue;
            }
            let mut scope = owner_scope.clone();
            for tp in &m.type_params {
                scope.insert(
                    tp.clone(),
                    m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
                );
            }
            let param_tys = resolve_params_in_binary_for_member(
                ctx.scope.classpath(),
                bin,
                m.nullness_scope,
                &m.params,
                &scope,
            );
            out.push(CallableCandidate {
                target: ResolvedTarget::BinaryMember {
                    owner_fqn: owner_fqn.to_string(),
                    member_index,
                },
                owner_fqn: owner_fqn.to_string(),
                member_nullness_scope: m.nullness_scope,
                is_static: false,
                is_raw_receiver: false,
                method_type_params: m.type_params.clone(),
                method_type_param_bounds: m.type_param_bounds.clone(),
                param_tys,
                return_ty: Ty::Void,
                owner_subst: Substitution::default(),
            });
        }
    }

    out
}

fn collect_owner_methods(
    ctx: ResolveCtx<'_>,
    lookup: &OwnerMethodLookup<'_>,
    out: &mut Vec<CallableCandidate>,
) {
    let owner_fqn = lookup.owner_fqn;
    let owner_scope = type_param_bounds_of_fqn_in_scope(ctx.db, ctx.scope, owner_fqn);

    if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, owner_fqn) {
        let tree = ctx.db.file_item_tree(fid);
        let ty_item = &tree.types[tid];
        let exclude_static =
            lookup.is_super && matches!(ty_item.kind, TypeKind::Interface | TypeKind::Annotation);
        let is_record = ty_item.kind == TypeKind::Record;

        for &mid in &ty_item.members {
            let m = &tree.members[mid];
            if m.kind == MemberKind::RecordComponent {
                if is_record && !lookup.only_static && lookup.arity == 0 && m.name == lookup.name
                {
                    let return_ty = m.type_ref.as_deref().map_or(Ty::Unknown, |raw| {
                        resolve_type_ref_in_file_for_member(
                            ctx.db,
                            fid,
                            Some(owner_fqn),
                            m.nullness_scope,
                            raw,
                            &owner_scope,
                        )
                    });
                    out.push(CallableCandidate {
                        target: ResolvedTarget::IndexedMember(fid, mid),
                        owner_fqn: owner_fqn.to_string(),
                        member_nullness_scope: m.nullness_scope,
                        is_static: false,
                        is_raw_receiver: lookup.is_raw_receiver,
                        method_type_params: Vec::new(),
                        method_type_param_bounds: BTreeMap::new(),
                        param_tys: Vec::new(),
                        return_ty,
                        owner_subst: lookup.owner_subst.clone(),
                    });
                }
                continue;
            }
            if m.kind != MemberKind::Method
                || m.name != lookup.name
                || (lookup.only_static && !m.is_static)
                || (exclude_static && m.is_static)
                || !arity_compatible(&m.params, lookup.arity)
            {
                continue;
            }
            let mut scope = owner_scope.clone();
            for tp in &m.type_params {
                scope.insert(
                    tp.clone(),
                    m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
                );
            }
            let param_tys = resolve_params_in_file_for_member(
                ctx.db,
                fid,
                Some(owner_fqn),
                m.nullness_scope,
                &m.params,
                &scope,
            );
            let return_ty = m.type_ref.as_deref().map_or(Ty::Unknown, |raw| {
                resolve_type_ref_in_file_for_member(
                    ctx.db,
                    fid,
                    Some(owner_fqn),
                    m.nullness_scope,
                    raw,
                    &scope,
                )
            });
            out.push(CallableCandidate {
                target: ResolvedTarget::IndexedMember(fid, mid),
                owner_fqn: owner_fqn.to_string(),
                member_nullness_scope: m.nullness_scope,
                is_static: m.is_static,
                is_raw_receiver: !m.is_static && lookup.is_raw_receiver,
                method_type_params: m.type_params.clone(),
                method_type_param_bounds: m.type_param_bounds.clone(),
                param_tys,
                return_ty,
                owner_subst: lookup.owner_subst.clone(),
            });
        }
        return;
    }

    if let Some(bin) = ctx.scope.get_binary_type(owner_fqn) {
        let exclude_static =
            lookup.is_super && matches!(bin.kind, TypeKind::Interface | TypeKind::Annotation);
        let is_record = bin.kind == TypeKind::Record;

        for (member_index, m) in bin.members.iter().enumerate() {
            if m.kind == MemberKind::RecordComponent {
                if is_record && !lookup.only_static && lookup.arity == 0 && m.name == lookup.name
                {
                    let return_ty = m.type_ref.as_deref().map_or(Ty::Unknown, |raw| {
                        resolve_binary_type_ref_for_member(
                            ctx.scope.classpath(),
                            bin,
                            m.nullness_scope,
                            raw,
                            &owner_scope,
                        )
                    });
                    out.push(CallableCandidate {
                        target: ResolvedTarget::BinaryMember {
                            owner_fqn: owner_fqn.to_string(),
                            member_index,
                        },
                        owner_fqn: owner_fqn.to_string(),
                        member_nullness_scope: m.nullness_scope,
                        is_static: false,
                        is_raw_receiver: lookup.is_raw_receiver,
                        method_type_params: Vec::new(),
                        method_type_param_bounds: BTreeMap::new(),
                        param_tys: Vec::new(),
                        return_ty,
                        owner_subst: lookup.owner_subst.clone(),
                    });
                }
                continue;
            }
            if m.kind != MemberKind::Method
                || m.name != lookup.name
                || (lookup.only_static && !m.is_static)
                || (exclude_static && m.is_static)
                || !arity_compatible(&m.params, lookup.arity)
            {
                continue;
            }
            let mut scope = owner_scope.clone();
            for tp in &m.type_params {
                scope.insert(
                    tp.clone(),
                    m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
                );
            }
            let param_tys = resolve_params_in_binary_for_member(
                ctx.scope.classpath(),
                bin,
                m.nullness_scope,
                &m.params,
                &scope,
            );
            let return_ty = m.type_ref.as_deref().map_or(Ty::Unknown, |raw| {
                resolve_binary_type_ref_for_member(
                    ctx.scope.classpath(),
                    bin,
                    m.nullness_scope,
                    raw,
                    &scope,
                )
            });
            out.push(CallableCandidate {
                target: ResolvedTarget::BinaryMember {
                    owner_fqn: owner_fqn.to_string(),
                    member_index,
                },
                owner_fqn: owner_fqn.to_string(),
                member_nullness_scope: m.nullness_scope,
                is_static: m.is_static,
                is_raw_receiver: !m.is_static && lookup.is_raw_receiver,
                method_type_params: m.type_params.clone(),
                method_type_param_bounds: m.type_param_bounds.clone(),
                param_tys,
                return_ty,
                owner_subst: lookup.owner_subst.clone(),
            });
        }
    }
}

pub(crate) fn select_best_callable_candidate_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    candidates: Vec<CallableCandidate>,
    arg_tys: &[Ty],
    arg_nodes: Option<&[SyntaxNode]>,
) -> Option<CallableCandidate> {
    if candidates.len() <= 1 {
        return candidates.into_iter().next();
    }

    let scored: Vec<(OverloadPhase, CallableCandidate)> = candidates
        .into_iter()
        .map(|cand| {
            let phase = score_candidate_phase_in_scope(db, scope, &cand, arg_tys, arg_nodes);
            (phase, cand)
        })
        .collect();

    let best_phase = scored.iter().map(|(p, _)| *p).min()?;
    let mut in_phase: Vec<CallableCandidate> = scored
        .into_iter()
        .filter(|(p, _)| *p == best_phase)
        .map(|(_, c)| c)
        .collect();

    let mut best = in_phase.drain(..1).next()?;
    for cand in in_phase {
        if is_candidate_strictly_more_specific_in_scope(db, scope, &cand, &best, arg_tys.len()) {
            best = cand;
        }
    }
    Some(best)
}

fn candidate_effective_subst(cand: &CallableCandidate) -> Substitution {
    let mut subst = cand.owner_subst.clone();
    for mp in &cand.method_type_params {
        subst.map.remove(mp);
    }
    subst
}

fn candidate_param_ty(
    cand: &CallableCandidate,
    subst: &Substitution,
    idx: usize,
    for_varargs_elem: bool,
) -> Option<Ty> {
    let (raw_p, is_varargs) = parameter_for_arg(&cand.param_tys, idx)?;
    let substituted = if cand.is_raw_receiver {
        erase_type_vars_to_bound(&raw_p)
    } else {
        subst.apply(&raw_p)
    };
    if for_varargs_elem && is_varargs {
        Some(match substituted {
            Ty::Array { elem, .. } => *elem,
            other => other,
        })
    } else {
        Some(substituted)
    }
}

pub(crate) fn score_candidate_phase_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    cand: &CallableCandidate,
    arg_tys: &[Ty],
    arg_nodes: Option<&[SyntaxNode]>,
) -> OverloadPhase {
    let is_varargs_method = cand.param_tys.last().is_some_and(|(_, va)| *va);
    let subst = candidate_effective_subst(cand);
    let m_tvars: FxHashSet<SmolStr> = cand.method_type_params.iter().cloned().collect();

    // Fixed-arity phases (1: Strict, 2: Loose).
    if !is_varargs_method || cand.param_tys.len() == arg_tys.len() {
        let mut all_strict = true;
        let mut all_loose = true;

        for (idx, arg_ty) in arg_tys.iter().enumerate() {
            let Some(p_ty) = candidate_param_ty(cand, &subst, idx, false) else {
                all_strict = false;
                all_loose = false;
                break;
            };
            let arg_node = arg_nodes.and_then(|ns| ns.get(idx));
            match classify_arg_compatibility(db, scope, arg_ty, arg_node, &p_ty, &m_tvars) {
                ArgCompat::Strict => {}
                ArgCompat::Loose => all_strict = false,
                ArgCompat::IncompatibleOrUnknown => {
                    all_strict = false;
                    all_loose = false;
                    break;
                }
            }
        }

        if all_strict && !is_varargs_method {
            return OverloadPhase::Strict;
        }
        if all_loose && !is_varargs_method {
            return OverloadPhase::Loose;
        }
        // If varargs method was passed an exact matching array argument, JLS treats it as Phase 3
        // only if Phase 1/2 fixed-arity methods didn't match; still, Phase 3 covers varargs!
        if all_loose && is_varargs_method {
            return OverloadPhase::Varargs;
        }
    }

    if is_varargs_method {
        let fixed_len = cand.param_tys.len().saturating_sub(1);
        let mut all_varargs_ok = true;
        for (idx, arg_ty) in arg_tys.iter().enumerate() {
            let is_trailing = idx >= fixed_len;
            let Some(p_ty) = candidate_param_ty(cand, &subst, idx, is_trailing) else {
                all_varargs_ok = false;
                break;
            };
            let arg_node = arg_nodes.and_then(|ns| ns.get(idx));
            if matches!(
                classify_arg_compatibility(db, scope, arg_ty, arg_node, &p_ty, &m_tvars),
                ArgCompat::IncompatibleOrUnknown
            ) {
                all_varargs_ok = false;
                break;
            }
        }
        if all_varargs_ok {
            return OverloadPhase::Varargs;
        }
    }

    OverloadPhase::Fallback
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgCompat {
    Strict,
    Loose,
    IncompatibleOrUnknown,
}

fn classify_arg_compatibility(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    arg_ty: &Ty,
    arg_node: Option<&SyntaxNode>,
    param_ty: &Ty,
    method_tvars: &FxHashSet<SmolStr>,
) -> ArgCompat {
    if let Some(node) = arg_node {
        match node.kind() {
            LAMBDA_EXPR => {
                let Some(sam) = find_functional_interface_sam_in_scope(db, scope, param_ty) else {
                    return ArgCompat::IncompatibleOrUnknown;
                };
                let lambda_arity = node
                    .children()
                    .find(|n| n.kind() == LAMBDA_PARAMS)
                    .map_or(0, |lp| lp.children().filter(|n| n.kind() == PARAM).count());
                return if sam.param_types.len() == lambda_arity {
                    ArgCompat::Strict
                } else {
                    ArgCompat::IncompatibleOrUnknown
                };
            }
            METHOD_REF_EXPR => {
                return if find_functional_interface_sam_in_scope(db, scope, param_ty).is_some() {
                    ArgCompat::Strict
                } else {
                    ArgCompat::IncompatibleOrUnknown
                };
            }
            _ => {}
        }
    }

    if matches!(arg_ty, Ty::Unknown) {
        return ArgCompat::IncompatibleOrUnknown;
    }

    if contains_any_tvar(param_ty, method_tvars) {
        return classify_generic_param_compat(db, scope, arg_ty, param_ty, method_tvars);
    }

    if is_subtype_in_scope(db, scope, arg_ty, param_ty) {
        ArgCompat::Strict
    } else if is_assignable_in_scope(db, scope, arg_ty, param_ty) {
        ArgCompat::Loose
    } else {
        ArgCompat::IncompatibleOrUnknown
    }
}

fn classify_generic_param_compat(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    arg_ty: &Ty,
    param_ty: &Ty,
    method_tvars: &FxHashSet<SmolStr>,
) -> ArgCompat {
    match param_ty {
        Ty::TypeVar { name, bounds, .. } if method_tvars.contains(name) => {
            let satisfies_bounds = |ref_ty: &Ty| -> bool {
                bounds.iter().all(|b| {
                    contains_any_tvar(b, method_tvars)
                        || is_subtype_in_scope(db, scope, ref_ty, b)
                })
            };
            match arg_ty {
                Ty::Primitive(_) => {
                    let boxed = arg_ty.boxed();
                    if satisfies_bounds(&boxed) {
                        ArgCompat::Loose
                    } else {
                        ArgCompat::IncompatibleOrUnknown
                    }
                }
                Ty::Null => ArgCompat::Strict,
                other => {
                    if satisfies_bounds(other) {
                        ArgCompat::Strict
                    } else {
                        ArgCompat::IncompatibleOrUnknown
                    }
                }
            }
        }
        Ty::Array { elem: p_elem, .. } => match arg_ty {
            Ty::Null => ArgCompat::Strict,
            Ty::Array { elem: a_elem, .. } => {
                classify_generic_param_compat(db, scope, a_elem, p_elem, method_tvars)
            }
            _ => ArgCompat::IncompatibleOrUnknown,
        },
        Ty::Class { fqn, .. } => {
            let raw_target = Ty::raw_class(fqn);
            if is_subtype_in_scope(db, scope, arg_ty, &raw_target) {
                ArgCompat::Strict
            } else {
                ArgCompat::IncompatibleOrUnknown
            }
        }
        _ => ArgCompat::IncompatibleOrUnknown,
    }
}

fn contains_any_tvar(ty: &Ty, tvars: &FxHashSet<SmolStr>) -> bool {
    if tvars.is_empty() {
        return false;
    }
    contains_any_tvar_inner(ty, tvars, 0)
}

fn contains_any_tvar_inner(ty: &Ty, tvars: &FxHashSet<SmolStr>, depth: usize) -> bool {
    if depth >= crate::ty::MAX_SUBST_DEPTH {
        return false;
    }
    match ty {
        Ty::TypeVar { name, .. } => tvars.contains(name),
        Ty::Array { elem, .. } => contains_any_tvar_inner(elem, tvars, depth + 1),
        Ty::Class { type_args, .. } => type_args.iter().any(|arg| match arg {
            crate::ty::TypeArg::Type(t) => contains_any_tvar_inner(t, tvars, depth + 1),
            crate::ty::TypeArg::Wildcard(
                crate::ty::WildcardBound::Extends(t) | crate::ty::WildcardBound::Super(t),
            ) => contains_any_tvar_inner(t, tvars, depth + 1),
            crate::ty::TypeArg::Wildcard(crate::ty::WildcardBound::Unbounded) => false,
        }),
        Ty::Intersection(members) => members
            .iter()
            .any(|m| contains_any_tvar_inner(m, tvars, depth + 1)),
        _ => false,
    }
}

pub(crate) fn is_candidate_strictly_more_specific_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    a: &CallableCandidate,
    b: &CallableCandidate,
    arity: usize,
) -> bool {
    more_specific_or_eq(db, scope, a, b, arity) && !more_specific_or_eq(db, scope, b, a, arity)
}

fn more_specific_or_eq(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    a: &CallableCandidate,
    b: &CallableCandidate,
    arity: usize,
) -> bool {
    let subst_a = candidate_effective_subst(a);
    let subst_b = candidate_effective_subst(b);
    let va_a = a.param_tys.last().is_some_and(|(_, va)| *va);
    let va_b = b.param_tys.last().is_some_and(|(_, va)| *va);
    let for_elem = va_a && va_b;

    for idx in 0..arity {
        let Some(pa) = candidate_param_ty(a, &subst_a, idx, for_elem) else {
            return false;
        };
        let Some(pb) = candidate_param_ty(b, &subst_b, idx, for_elem) else {
            return false;
        };
        if pa != pb && !is_subtype_in_scope(db, scope, &pa, &pb) {
            return false;
        }
    }
    true
}
