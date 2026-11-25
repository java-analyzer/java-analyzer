use std::collections::VecDeque;

use rustc_hash::FxHashSet;
use syntax::{
    SyntaxNode, TextSize,
    ext::{has_direct_token, path_segments},
    syntax_kind::SyntaxKind::*,
};

use crate::{
    DefDatabase, FileId,
    classpath::BinaryMemberSymbol,
    hierarchy::{direct_supertypes_in_scope, linearized_supertypes_in_scope, parse_type_ref_segments},
    infer::infer_expr_ty,
    item_tree::{FileItemTree, MemberId, TypeId},
    scope::VisibleScope,
    symbols::{
        MemberKind, MemberSymbol, ParamSummary, TypeKind, TypeSymbol,
        extract_local_type_symbol, node_ident_text,
    },
    ty::Ty,
};

use super::{
    ResolveCtx, ResolvedTarget, count_args, extract_dotted_name_chain,
    lexical::{
        enclosing_type_targets, lookup_type_by_fqn, lookup_workspace_or_binary_type,
        resolve_type_segments,
    },
    overload::{collect_constructor_candidates, select_best_callable_candidate_in_scope},
    resolve_name_ref_expr,
};

#[derive(Debug, Clone)]
pub(super) enum TypeRefTarget {
    Indexed(FileId, TypeId),
    Local(Box<TypeSymbol>),
    Binary(String),
}

impl TypeRefTarget {
    pub(super) fn qualified_name<'a>(
        &'a self,
        db: &dyn DefDatabase,
        own_tree: &'a FileItemTree,
    ) -> String {
        match self {
            Self::Indexed(fid, tid) => {
                if *fid == own_tree.file_id {
                    own_tree.types[*tid].qualified_name.clone()
                } else {
                    db.file_item_tree(*fid).types[*tid].qualified_name.clone()
                }
            }
            Self::Local(ty) => ty.qualified_name.clone(),
            Self::Binary(fqn) => fqn.clone(),
        }
    }
}

impl From<TypeRefTarget> for ResolvedTarget {
    fn from(target: TypeRefTarget) -> Self {
        match target {
            TypeRefTarget::Indexed(fid, tid) => Self::IndexedType(fid, tid),
            TypeRefTarget::Local(ty) => Self::LocalType(*ty),
            TypeRefTarget::Binary(fqn) => Self::BinaryType(fqn),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum CallableMatch<ID> {
    Exact(ID),
    Varargs(ID),
    Fallback(ID),
}

impl<ID> CallableMatch<ID> {
    pub(super) fn into_inner(self) -> ID {
        match self {
            Self::Exact(id) | Self::Varargs(id) | Self::Fallback(id) => id,
        }
    }

    fn map<U>(self, f: impl FnOnce(ID) -> U) -> CallableMatch<U> {
        match self {
            Self::Exact(id) => CallableMatch::Exact(f(id)),
            Self::Varargs(id) => CallableMatch::Varargs(f(id)),
            Self::Fallback(id) => CallableMatch::Fallback(f(id)),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct CallableQuery<'a> {
    pub(super) target_kind: MemberKind,
    pub(super) name: &'a str,
    pub(super) call_arity: Option<usize>,
    pub(super) include_record_components: bool,
    pub(super) only_static: bool,
    pub(super) exclude_static: bool,
}

pub(super) fn target_is_interface(ctx: ResolveCtx<'_>, ty: &TypeRefTarget) -> bool {
    match ty {
        TypeRefTarget::Indexed(fid, tid) => {
            let owned;
            let tree = if *fid == ctx.file_tree.file_id {
                ctx.file_tree
            } else {
                owned = ctx.db.file_item_tree(*fid);
                &owned
            };
            matches!(
                tree.types[*tid].kind,
                TypeKind::Interface | TypeKind::Annotation
            )
        }
        TypeRefTarget::Local(local_ty) => {
            matches!(local_ty.kind, TypeKind::Interface | TypeKind::Annotation)
        }
        TypeRefTarget::Binary(fqn) => ctx
            .scope
            .get_binary_type(fqn)
            .is_some_and(|b| matches!(b.kind, TypeKind::Interface | TypeKind::Annotation)),
    }
}

pub(super) fn supertypes_of_target(ctx: ResolveCtx<'_>, ty: &TypeRefTarget) -> Vec<String> {
    match ty {
        TypeRefTarget::Indexed(_, _) | TypeRefTarget::Binary(_) => {
            let fqn = ty.qualified_name(ctx.db, ctx.file_tree);
            linearized_supertypes_in_scope(ctx.db, ctx.scope, &fqn)
        }
        TypeRefTarget::Local(local_ty) => {
            let mut visited = FxHashSet::default();
            visited.insert(local_ty.qualified_name.clone());
            let mut out = Vec::new();
            let mut iface_queue = VecDeque::new();

            let resolve_local_ref = |raw: &str| -> Option<String> {
                let segs = parse_type_ref_segments(raw)?;
                let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
                let root = ctx.db.syntax_tree(local_ty.file_id);
                let site_node = root
                    .token_at_offset(local_ty.selection_range.start())
                    .right_biased()
                    .and_then(|t| t.parent())
                    .unwrap_or(root);
                match resolve_type_segments(ctx, &site_node, &seg_refs)? {
                    ResolvedTarget::IndexedType(fid, tid) => {
                        Some(TypeRefTarget::Indexed(fid, tid).qualified_name(ctx.db, ctx.file_tree))
                    }
                    ResolvedTarget::LocalType(t) => Some(t.qualified_name),
                    ResolvedTarget::BinaryType(fqn) => Some(fqn),
                    _ => None,
                }
            };

            if let Some(first_ext) = local_ty.extends.first()
                && let Some(sup_fqn) = resolve_local_ref(first_ext)
                && sup_fqn != "java.lang.Object"
                && visited.insert(sup_fqn.clone())
            {
                out.push(sup_fqn.clone());
                for s in linearized_supertypes_in_scope(ctx.db, ctx.scope, &sup_fqn) {
                    if s != "java.lang.Object" && visited.insert(s.clone()) {
                        out.push(s);
                    }
                }
            }
            for raw_impl in &local_ty.implements {
                if let Some(iface_fqn) = resolve_local_ref(raw_impl) {
                    iface_queue.push_back(iface_fqn);
                }
            }
            while let Some(iface) = iface_queue.pop_front() {
                if iface == "java.lang.Object" || !visited.insert(iface.clone()) {
                    continue;
                }
                out.push(iface.clone());
                for s in linearized_supertypes_in_scope(ctx.db, ctx.scope, &iface) {
                    if s != "java.lang.Object" && visited.insert(s.clone()) {
                        out.push(s);
                    }
                }
            }
            if visited.insert("java.lang.Object".to_string()) {
                out.push("java.lang.Object".to_string());
            }
            out
        }
    }
}

pub(super) fn type_ref_from_fqn(ctx: ResolveCtx<'_>, fqn: &str) -> Option<TypeRefTarget> {
    if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, fqn) {
        return Some(TypeRefTarget::Indexed(fid, tid));
    }
    ctx.scope
        .contains_binary_type(fqn)
        .then(|| TypeRefTarget::Binary(fqn.to_string()))
}

fn lookup_method_on_single_type(
    ctx: ResolveCtx<'_>,
    ty: &TypeRefTarget,
    name: &str,
    call_arity: Option<usize>,
    exclude_interface_static: bool,
) -> Option<CallableMatch<ResolvedTarget>> {
    let exclude_static = exclude_interface_static && target_is_interface(ctx, ty);
    match ty {
        TypeRefTarget::Indexed(ty_file, ty_id) => {
            let owned_tree;
            let ty_tree = if *ty_file == ctx.file_tree.file_id {
                ctx.file_tree
            } else {
                owned_tree = ctx.db.file_item_tree(*ty_file);
                &owned_tree
            };
            let type_item = &ty_tree.types[*ty_id];
            select_callable_from_tree(
                ty_tree,
                &type_item.members,
                CallableQuery {
                    target_kind: MemberKind::Method,
                    name,
                    call_arity,
                    include_record_components: type_item.kind == TypeKind::Record,
                    only_static: false,
                    exclude_static,
                },
            )
            .map(|m| m.map(|mid| ResolvedTarget::IndexedMember(*ty_file, mid)))
        }
        TypeRefTarget::Local(local_ty) => select_callable_from_slice(
            &local_ty.members,
            CallableQuery {
                target_kind: MemberKind::Method,
                name,
                call_arity,
                include_record_components: local_ty.kind == TypeKind::Record,
                only_static: false,
                exclude_static,
            },
        )
        .map(|m| m.map(|mem| ResolvedTarget::LocalMember(mem.clone()))),
        TypeRefTarget::Binary(fqn) => {
            let bin_ty = ctx.scope.get_binary_type(fqn)?;
            select_callable_from_binary(
                &bin_ty.members,
                CallableQuery {
                    target_kind: MemberKind::Method,
                    name,
                    call_arity,
                    include_record_components: bin_ty.kind == TypeKind::Record,
                    only_static: false,
                    exclude_static,
                },
            )
            .map(|m| {
                m.map(|member_index| ResolvedTarget::BinaryMember {
                    owner_fqn: fqn.clone(),
                    member_index,
                })
            })
        }
    }
}

pub(super) fn lookup_method_across_hierarchy(
    ctx: ResolveCtx<'_>,
    ty: &TypeRefTarget,
    name: &str,
    call_arity: Option<usize>,
) -> Option<ResolvedTarget> {
    let mut best_varargs = None;
    let mut best_fallback = None;

    if let Some(m) = lookup_method_on_single_type(ctx, ty, name, call_arity, false) {
        match m {
            CallableMatch::Exact(target) => return Some(target),
            CallableMatch::Varargs(target) => best_varargs = Some(target),
            CallableMatch::Fallback(target) => best_fallback = Some(target),
        }
    }

    for sup_fqn in supertypes_of_target(ctx, ty) {
        let Some(sup_ty) = type_ref_from_fqn(ctx, &sup_fqn) else {
            continue;
        };
        if let Some(m) = lookup_method_on_single_type(ctx, &sup_ty, name, call_arity, true) {
            match m {
                CallableMatch::Exact(target) => return Some(target),
                CallableMatch::Varargs(target) if best_varargs.is_none() => {
                    best_varargs = Some(target);
                }
                CallableMatch::Fallback(target) if best_fallback.is_none() => {
                    best_fallback = Some(target);
                }
                _ => {}
            }
        }
    }

    best_varargs.or(best_fallback)
}

pub(super) fn lookup_non_method_member_or_nested_with_supers(
    ctx: ResolveCtx<'_>,
    ty: &TypeRefTarget,
    name: &str,
) -> Option<ResolvedTarget> {
    if let Some(found) = lookup_non_method_member_or_nested(ctx, ty, name) {
        return Some(found);
    }
    for sup_fqn in supertypes_of_target(ctx, ty) {
        if let Some(sup_ty) = type_ref_from_fqn(ctx, &sup_fqn)
            && let Some(found) = lookup_non_method_member_or_nested(ctx, &sup_ty, name)
        {
            return Some(found);
        }
    }
    None
}

pub(super) fn lookup_member_on_type(
    ctx: ResolveCtx<'_>,
    ty: &TypeRefTarget,
    name: &str,
    call_arity: Option<usize>,
    prefer_callable: bool,
) -> Option<ResolvedTarget> {
    if prefer_callable {
        return lookup_method_across_hierarchy(ctx, ty, name, call_arity);
    }

    if let Some(found) = lookup_non_method_member_or_nested_with_supers(ctx, ty, name) {
        return Some(found);
    }

    lookup_method_across_hierarchy(ctx, ty, name, call_arity)
}

fn lookup_non_method_member_or_nested(
    ctx: ResolveCtx<'_>,
    ty: &TypeRefTarget,
    name: &str,
) -> Option<ResolvedTarget> {
    match ty {
        TypeRefTarget::Indexed(ty_file, ty_id) => {
            let owned_tree;
            let ty_tree = if *ty_file == ctx.file_tree.file_id {
                ctx.file_tree
            } else {
                owned_tree = ctx.db.file_item_tree(*ty_file);
                &owned_tree
            };
            let type_item = &ty_tree.types[*ty_id];
            if let Some(&mid) = type_item.members.iter().find(|&&mid| {
                let m = &ty_tree.members[mid];
                m.kind != MemberKind::Method && m.kind != MemberKind::Constructor && m.name == name
            }) {
                return Some(ResolvedTarget::IndexedMember(*ty_file, mid));
            }
            if type_item.nested_types.iter().any(|n| n == name) {
                let nested_fqn = format!("{}.{}", type_item.qualified_name, name);
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, &nested_fqn)
                {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
            None
        }
        TypeRefTarget::Local(local_ty) => {
            if let Some(m) = local_ty.members.iter().find(|m| {
                m.kind != MemberKind::Method && m.kind != MemberKind::Constructor && m.name == name
            }) {
                return Some(ResolvedTarget::LocalMember(m.clone()));
            }
            if local_ty.nested_types.iter().any(|n| n == name) {
                let nested_fqn = format!("{}.{}", local_ty.qualified_name, name);
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, &nested_fqn)
                {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
            None
        }
        TypeRefTarget::Binary(fqn) => {
            let bin_ty = ctx.scope.get_binary_type(fqn)?;
            if let Some(member_index) = bin_ty.members.iter().position(|m| {
                m.kind != MemberKind::Method && m.kind != MemberKind::Constructor && m.name == name
            }) {
                return Some(ResolvedTarget::BinaryMember {
                    owner_fqn: fqn.clone(),
                    member_index,
                });
            }
            if bin_ty.nested_types.iter().any(|n| n == name) {
                let nested_fqn = format!("{}.{}", bin_ty.qualified_name, name);
                if let Some(found) = lookup_workspace_or_binary_type(ctx, &nested_fqn) {
                    return Some(found);
                }
            }
            None
        }
    }
}

pub(super) fn lookup_method_or_member(
    ctx: ResolveCtx<'_>,
    ty: &TypeRefTarget,
    name: &str,
) -> Option<ResolvedTarget> {
    if let Some(found) = lookup_method_across_hierarchy(ctx, ty, name, None) {
        return Some(found);
    }
    lookup_non_method_member_or_nested_with_supers(ctx, ty, name)
}

pub(super) fn select_callable_from_tree(
    tree: &FileItemTree,
    member_ids: &[MemberId],
    query: CallableQuery<'_>,
) -> Option<CallableMatch<MemberId>> {
    select_callable_by(
        member_ids.iter().map(|&mid| {
            let m = &tree.members[mid];
            (
                mid,
                m.kind,
                m.name.as_str(),
                m.is_static,
                m.params.as_slice(),
            )
        }),
        query,
    )
}

fn select_callable_from_slice<'a>(
    members: &'a [MemberSymbol],
    query: CallableQuery<'_>,
) -> Option<CallableMatch<&'a MemberSymbol>> {
    select_callable_by(
        members
            .iter()
            .enumerate()
            .map(|(i, m)| (i, m.kind, m.name.as_str(), m.is_static, m.params.as_slice())),
        query,
    )
    .map(|m| m.map(|idx| &members[idx]))
}

pub(super) fn select_callable_from_binary(
    members: &[BinaryMemberSymbol],
    query: CallableQuery<'_>,
) -> Option<CallableMatch<usize>> {
    select_callable_by(
        members
            .iter()
            .enumerate()
            .map(|(i, m)| (i, m.kind, m.name.as_str(), m.is_static, m.params.as_slice())),
        query,
    )
}

fn select_callable_by<'a, I, ID>(members: I, query: CallableQuery<'_>) -> Option<CallableMatch<ID>>
where
    ID: Copy,
    I: Iterator<Item = (ID, MemberKind, &'a str, bool, &'a [ParamSummary])>,
{
    let mut first_match: Option<ID> = None;
    let mut varargs_match: Option<ID> = None;
    let mut record_comp_match: Option<ID> = None;

    for (id, kind, m_name, is_static, params) in members {
        if query.target_kind == MemberKind::Method && m_name != query.name {
            continue;
        }
        if kind == MemberKind::RecordComponent {
            if query.target_kind == MemberKind::Method
                && !query.only_static
                && query.include_record_components
                && query.call_arity.is_none_or(|a| a == 0)
                && record_comp_match.is_none()
            {
                record_comp_match = Some(id);
            }
            continue;
        }
        if kind != query.target_kind
            || (query.only_static && !is_static)
            || (query.exclude_static && is_static)
        {
            continue;
        }
        if first_match.is_none() {
            first_match = Some(id);
        }
        if let Some(arity) = query.call_arity {
            let is_varargs = params.last().is_some_and(|p| p.is_varargs);
            // 1. Exact fixed-arity match.
            if !is_varargs && params.len() == arity {
                return Some(CallableMatch::Exact(id));
            }
            // 2. Varargs arity match.
            if is_varargs && varargs_match.is_none() && arity + 1 >= params.len() {
                varargs_match = Some(id);
            }
        } else {
            return Some(CallableMatch::Exact(id));
        }
    }

    // 3. Fallback to first varargs match, record component (0-arg exact), or first matching callable.
    if let Some(id) = varargs_match {
        return Some(CallableMatch::Varargs(id));
    }
    if let Some(id) = record_comp_match {
        return Some(CallableMatch::Exact(id));
    }
    first_match.map(CallableMatch::Fallback)
}

fn lookup_constructor_on_type(
    ctx: ResolveCtx<'_>,
    ty: &TypeRefTarget,
    arg_count: Option<usize>,
) -> Option<ResolvedTarget> {
    let query = CallableQuery {
        target_kind: MemberKind::Constructor,
        name: "",
        call_arity: arg_count,
        include_record_components: false,
        only_static: false,
        exclude_static: false,
    };
    match ty {
        TypeRefTarget::Indexed(ty_file, ty_id) => {
            let owned_tree;
            let ty_tree = if *ty_file == ctx.file_tree.file_id {
                ctx.file_tree
            } else {
                owned_tree = ctx.db.file_item_tree(*ty_file);
                &owned_tree
            };
            let type_item = &ty_tree.types[*ty_id];
            select_callable_from_tree(ty_tree, &type_item.members, query)
                .map(|m| ResolvedTarget::IndexedMember(*ty_file, m.into_inner()))
        }
        TypeRefTarget::Local(local_ty) => select_callable_from_slice(&local_ty.members, query)
            .map(|m| ResolvedTarget::LocalMember(m.into_inner().clone())),
        TypeRefTarget::Binary(fqn) => {
            let bin_ty = ctx.scope.get_binary_type(fqn)?;
            select_callable_from_binary(&bin_ty.members, query).map(|m| {
                ResolvedTarget::BinaryMember {
                    owner_fqn: fqn.clone(),
                    member_index: m.into_inner(),
                }
            })
        }
    }
}

fn resolve_super_target_type(ctx: ResolveCtx<'_>, super_expr: &SyntaxNode) -> Option<TypeRefTarget> {
    // Qualified `Interface.super` or `Outer.super`.
    if let Some(qualifier) = super_expr.children().next() {
        let segs = extract_dotted_name_chain(&qualifier)?;
        let seg_refs: Vec<&str> = segs.iter().map(|s| s.as_str()).collect();
        let qual_target = match resolve_type_segments(ctx, &qualifier, &seg_refs)? {
            ResolvedTarget::IndexedType(fid, tid) => TypeRefTarget::Indexed(fid, tid),
            ResolvedTarget::LocalType(ty) => TypeRefTarget::Local(Box::new(ty)),
            ResolvedTarget::BinaryType(fqn) => TypeRefTarget::Binary(fqn),
            _ => return None,
        };
        if target_is_interface(ctx, &qual_target) {
            return Some(qual_target);
        }
        let qual_fqn = qual_target.qualified_name(ctx.db, ctx.file_tree);
        let sup_fqn = direct_supertypes_in_scope(ctx.db, ctx.scope, &qual_fqn).super_class?;
        return type_ref_from_fqn(ctx, &sup_fqn);
    }

    // Unqualified `super`: walk enclosing type or anonymous class `NEW_EXPR`.
    for ancestor in super_expr.ancestors().skip(1) {
        if ancestor.kind() == NEW_EXPR
            && let Some(path_type) = ancestor.children().find(|n| n.kind() == PATH_TYPE)
        {
            let segs = path_segments(&path_type);
            let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
            return match resolve_type_segments(ctx, &path_type, &seg_refs)? {
                ResolvedTarget::IndexedType(fid, tid) => Some(TypeRefTarget::Indexed(fid, tid)),
                ResolvedTarget::LocalType(ty) => Some(TypeRefTarget::Local(Box::new(ty))),
                ResolvedTarget::BinaryType(fqn) => Some(TypeRefTarget::Binary(fqn)),
                _ => None,
            };
        }
        if TypeKind::from_syntax(ancestor.kind()).is_some() {
            let range = ancestor.text_range();
            if let Some((_, ty)) = ctx
                .ast_id_map
                .find_by_full(range)
                .and_then(|ast_id| ctx.file_tree.find_type_by_ast_id(ast_id))
            {
                let sup_fqn =
                    direct_supertypes_in_scope(ctx.db, ctx.scope, &ty.qualified_name).super_class?;
                return type_ref_from_fqn(ctx, &sup_fqn);
            }
            if let Some(local_ty) = extract_local_type_symbol(ctx.file_id, &ancestor) {
                let sup_fqn = local_ty
                    .extends
                    .first()
                    .and_then(|raw| parse_type_ref_segments(raw))
                    .and_then(|segs| {
                        let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
                        match resolve_type_segments(ctx, &ancestor, &seg_refs)? {
                            ResolvedTarget::IndexedType(fid, tid) => Some(
                                TypeRefTarget::Indexed(fid, tid)
                                    .qualified_name(ctx.db, ctx.file_tree),
                            ),
                            ResolvedTarget::LocalType(t) => Some(t.qualified_name),
                            ResolvedTarget::BinaryType(fqn) => Some(fqn),
                            _ => None,
                        }
                    })
                    .unwrap_or_else(|| "java.lang.Object".to_string());
                return type_ref_from_fqn(ctx, &sup_fqn);
            }
        }
    }

    None
}

pub(super) fn resolve_super_expr_target(
    ctx: ResolveCtx<'_>,
    super_expr: &SyntaxNode,
) -> Option<ResolvedTarget> {
    if let Some(call_expr) = super_expr.parent().filter(|p| {
        p.kind() == CALL_EXPR
            && p.children()
                .next()
                .is_some_and(|c| c.text_range() == super_expr.text_range())
    }) {
        let arg_nodes: Vec<SyntaxNode> = call_expr
            .children()
            .find(|n| n.kind() == ARG_LIST)
            .map(|al| al.children().collect())
            .unwrap_or_default();
        let super_ty = resolve_super_target_type(ctx, super_expr)?;
        let super_fqn = super_ty.qualified_name(ctx.db, ctx.file_tree);
        let arg_tys: Vec<Ty> = arg_nodes
            .iter()
            .map(|n| {
                if matches!(n.kind(), LAMBDA_EXPR | METHOD_REF_EXPR) {
                    Ty::Unknown
                } else {
                    infer_expr_ty(ctx.db, ctx.file_id, n, None)
                }
            })
            .collect();
        let ctors = collect_constructor_candidates(ctx, &super_fqn, arg_nodes.len());
        if let Some(best) = select_best_callable_candidate_in_scope(
            ctx.db,
            ctx.scope,
            ctors,
            &arg_tys,
            Some(&arg_nodes),
        ) {
            return Some(best.target);
        }
        return lookup_constructor_on_type(ctx, &super_ty, Some(arg_nodes.len()))
            .or_else(|| Some(ResolvedTarget::from(super_ty)));
    }

    resolve_super_target_type(ctx, super_expr).map(ResolvedTarget::from)
}

// ---- Receiver type resolution ----

const MAX_TYPE_DEPTH: u8 = 32;

pub(super) fn resolve_expr_type(
    ctx: ResolveCtx<'_>,
    expr: &SyntaxNode,
    depth: u8,
) -> Option<TypeRefTarget> {
    if depth >= MAX_TYPE_DEPTH {
        return None;
    }

    match expr.kind() {
        THIS_EXPR => {
            if let Some(qualifier) = expr.children().next() {
                return resolve_expr_type(ctx, &qualifier, depth + 1);
            }
            enclosing_type_targets(ctx.file_id, ctx.file_tree, ctx.ast_id_map, expr)
                .into_iter()
                .next()
        }
        SUPER_EXPR => resolve_super_target_type(ctx, expr),
        NAME_REF_EXPR => {
            let ref_node = expr.children().find(|n| n.kind() == NAME_REF)?;
            let target = resolve_name_ref_expr(ctx, expr, &ref_node)?;
            target_to_type_ref(ctx, target, depth + 1)
        }
        FIELD_ACCESS_EXPR => {
            if let Some(chain) = extract_dotted_name_chain(expr) {
                let fqn = chain.join(".");
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, &fqn) {
                    return Some(TypeRefTarget::Indexed(fid, tid));
                }
                if ctx.scope.contains_binary_type(&fqn) {
                    return Some(TypeRefTarget::Binary(fqn));
                }
            }
            let ref_node = expr.children().find(|n| n.kind() == NAME_REF)?;
            let receiver = expr.children().next()?;
            let recv_ty = resolve_expr_type(ctx, &receiver, depth + 1)?;
            let member_name = node_ident_text(&ref_node);
            let target = lookup_member_on_type(ctx, &recv_ty, &member_name, None, false)?;
            target_to_type_ref(ctx, target, depth + 1)
        }
        METHOD_CALL_EXPR => {
            let ref_node = expr.children().find(|n| n.kind() == NAME_REF)?;
            let receiver = expr.children().next()?;
            let recv_ty = resolve_expr_type(ctx, &receiver, depth + 1)?;
            let arg_count = expr
                .children()
                .find(|n| n.kind() == ARG_LIST)
                .map(|al| count_args(&al));
            let member_name = node_ident_text(&ref_node);
            let target = lookup_member_on_type(ctx, &recv_ty, &member_name, arg_count, true)?;
            target_to_type_ref(ctx, target, depth + 1)
        }
        CALL_EXPR => {
            let callee = expr.children().next()?;
            if callee.kind() != NAME_REF_EXPR {
                return None;
            }
            let ref_node = callee.children().find(|n| n.kind() == NAME_REF)?;
            let target = resolve_name_ref_expr(ctx, &callee, &ref_node)?;
            target_to_type_ref(ctx, target, depth + 1)
        }
        NEW_EXPR => {
            let path_type = expr.children().find(|n| n.kind() == PATH_TYPE)?;
            let segs = path_segments(&path_type);
            if has_direct_token(expr, DOT) {
                let outer_expr = expr.children().next()?;
                let outer_ty = resolve_expr_type(ctx, &outer_expr, depth + 1)?;
                let mut current_qname = outer_ty.qualified_name(ctx.db, ctx.file_tree);
                let mut resolved = None;
                for seg in &segs {
                    let next_fqn = format!("{current_qname}.{seg}");
                    resolved = match lookup_workspace_or_binary_type(ctx, &next_fqn)? {
                        ResolvedTarget::IndexedType(fid, tid) => {
                            Some(TypeRefTarget::Indexed(fid, tid))
                        }
                        ResolvedTarget::BinaryType(fqn) => Some(TypeRefTarget::Binary(fqn)),
                        _ => return None,
                    };
                    current_qname = next_fqn;
                }
                return resolved;
            }
            let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
            match resolve_type_segments(ctx, &path_type, &seg_refs)? {
                ResolvedTarget::IndexedType(fid, tid) => Some(TypeRefTarget::Indexed(fid, tid)),
                ResolvedTarget::LocalType(ty) => Some(TypeRefTarget::Local(Box::new(ty))),
                ResolvedTarget::BinaryType(fqn) => Some(TypeRefTarget::Binary(fqn)),
                _ => None,
            }
        }
        PAREN_EXPR => {
            let inner = expr.children().next()?;
            resolve_expr_type(ctx, &inner, depth + 1)
        }
        CAST_EXPR => {
            let path_type = expr.children().find(|n| n.kind() == PATH_TYPE)?;
            let segs = path_segments(&path_type);
            let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
            match resolve_type_segments(ctx, &path_type, &seg_refs)? {
                ResolvedTarget::IndexedType(fid, tid) => Some(TypeRefTarget::Indexed(fid, tid)),
                ResolvedTarget::LocalType(ty) => Some(TypeRefTarget::Local(Box::new(ty))),
                ResolvedTarget::BinaryType(fqn) => Some(TypeRefTarget::Binary(fqn)),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn target_to_type_ref(
    ctx: ResolveCtx<'_>,
    target: ResolvedTarget,
    depth: u8,
) -> Option<TypeRefTarget> {
    if depth >= MAX_TYPE_DEPTH {
        return None;
    }
    match target {
        ResolvedTarget::IndexedType(fid, tid) => Some(TypeRefTarget::Indexed(fid, tid)),
        ResolvedTarget::LocalType(ty) => Some(TypeRefTarget::Local(Box::new(ty))),
        ResolvedTarget::BinaryType(fqn) => Some(TypeRefTarget::Binary(fqn)),
        ResolvedTarget::Local(l) => {
            let ty_str = l.type_ref.as_deref()?;
            resolve_type_ref_in_file(
                ctx.db,
                ctx.scope,
                l.file_id,
                l.selection_range.start(),
                ty_str,
            )
        }
        ResolvedTarget::IndexedMember(m_file, m_id) => {
            let decl_tree = ctx.db.file_item_tree(m_file);
            let m = &decl_tree.members[m_id];
            match m.kind {
                MemberKind::Field | MemberKind::RecordComponent | MemberKind::Method => {
                    let ty_str = m.type_ref.as_deref()?;
                    let decl_offset = ctx
                        .db
                        .ast_id_map(m_file)
                        .range(m.ast_id)
                        .selection_range
                        .start();
                    resolve_type_ref_in_file(
                        ctx.db,
                        ctx.scope,
                        m_file,
                        decl_offset,
                        ty_str,
                    )
                }
                MemberKind::EnumConstant => {
                    let owner_fqn = m.owner_type.as_deref()?;
                    lookup_type_by_fqn(&decl_tree, ctx.scope, owner_fqn)
                        .map(|(fid, tid)| TypeRefTarget::Indexed(fid, tid))
                }
                MemberKind::Constructor => None,
            }
        }
        ResolvedTarget::LocalMember(m) => match m.kind {
            MemberKind::Field | MemberKind::RecordComponent | MemberKind::Method => {
                let ty_str = m.type_ref.as_deref()?;
                resolve_type_ref_in_file(
                    ctx.db,
                    ctx.scope,
                    m.file_id,
                    m.selection_range.start(),
                    ty_str,
                )
            }
            MemberKind::EnumConstant => {
                let owner_fqn = m.owner_type.as_deref()?;
                let decl_tree = ctx.db.file_item_tree(m.file_id);
                lookup_type_by_fqn(&decl_tree, ctx.scope, owner_fqn)
                    .map(|(fid, tid)| TypeRefTarget::Indexed(fid, tid))
            }
            MemberKind::Constructor => None,
        },
        ResolvedTarget::BinaryMember {
            owner_fqn,
            member_index,
        } => {
            let owner_ty = ctx.scope.get_binary_type(&owner_fqn)?;
            let m = owner_ty.members.get(member_index)?;
            match m.kind {
                MemberKind::Field | MemberKind::RecordComponent | MemberKind::Method => {
                    let ty_str = m.type_ref.as_deref()?;
                    resolve_binary_type_ref(ctx.scope, &owner_fqn, ty_str)
                }
                MemberKind::EnumConstant => Some(TypeRefTarget::Binary(owner_fqn)),
                MemberKind::Constructor => None,
            }
        }
        ResolvedTarget::TypeParam(_) | ResolvedTarget::Package(_) => None,
        ResolvedTarget::WithDetail { inner, .. } => target_to_type_ref(ctx, *inner, depth + 1),
    }
}

fn resolve_binary_type_ref(
    scope: &VisibleScope,
    owner_fqn: &str,
    type_ref: &str,
) -> Option<TypeRefTarget> {
    let segs = parse_type_ref_segments(type_ref)?;
    let joined = segs.join(".");
    if let Some((fid, tid)) = scope.get_workspace_type(&joined) {
        return Some(TypeRefTarget::Indexed(fid, tid));
    }
    if scope.contains_binary_type(&joined) {
        return Some(TypeRefTarget::Binary(joined));
    }
    if let Some(pkg) = scope.get_binary_type(owner_fqn).map(|t| t.package.as_str())
        && !pkg.is_empty()
    {
        let same_pkg_fqn = format!("{pkg}.{joined}");
        if let Some((fid, tid)) = scope.get_workspace_type(&same_pkg_fqn) {
            return Some(TypeRefTarget::Indexed(fid, tid));
        }
        if scope.contains_binary_type(&same_pkg_fqn) {
            return Some(TypeRefTarget::Binary(same_pkg_fqn));
        }
    }
    let java_lang_fqn = format!("java.lang.{joined}");
    if let Some((fid, tid)) = scope.get_workspace_type(&java_lang_fqn) {
        return Some(TypeRefTarget::Indexed(fid, tid));
    }
    if scope.contains_binary_type(&java_lang_fqn) {
        return Some(TypeRefTarget::Binary(java_lang_fqn));
    }
    None
}

fn resolve_type_ref_in_file(
    db: &dyn DefDatabase,
    caller_scope: &VisibleScope,
    decl_file_id: FileId,
    decl_offset: TextSize,
    type_ref: &str,
) -> Option<TypeRefTarget> {
    let segs = parse_type_ref_segments(type_ref)?;
    let root = db.syntax_tree(decl_file_id);
    if decl_offset > root.text_range().end() {
        return None;
    }
    let site_node = root
        .token_at_offset(decl_offset)
        .right_biased()
        .and_then(|t| t.parent())
        .unwrap_or(root);
    let decl_tree = db.file_item_tree(decl_file_id);
    let decl_map = db.ast_id_map(decl_file_id);
    let owned_scope;
    let decl_scope = if !caller_scope.is_scoped() || caller_scope.caller_file() == Some(decl_file_id) {
        caller_scope
    } else {
        owned_scope = VisibleScope::for_file(db, decl_file_id);
        &owned_scope
    };
    let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
    let decl_ctx = ResolveCtx {
        db,
        file_id: decl_file_id,
        file_tree: &decl_tree,
        ast_id_map: &decl_map,
        scope: decl_scope,
    };
    match resolve_type_segments(decl_ctx, &site_node, &seg_refs)? {
        ResolvedTarget::IndexedType(fid, tid) => Some(TypeRefTarget::Indexed(fid, tid)),
        ResolvedTarget::LocalType(ty) => Some(TypeRefTarget::Local(Box::new(ty))),
        ResolvedTarget::BinaryType(fqn) => Some(TypeRefTarget::Binary(fqn)),
        _ => None,
    }
}
