//! Pure, IO-free Java name resolution at a source offset.
//! See `docs/specs/0005-symbol-indexing-and-name-resolution.md`.

mod lexical;
mod member;
mod overload;

use smol_str::SmolStr;
use syntax::{
    SyntaxNode, TextRange, TextSize,
    ext::{
        collapsed_text, has_direct_token, is_type_node, path_segments, type_text_with_dims,
    },
    syntax_kind::SyntaxKind::*,
};

use crate::{
    DefDatabase, FileId,
    classpath::BinaryTarget,
    hierarchy::find_overridden_method_in_scope,
    infer::{self, infer_expr_ty, infer_local_ty},
    item_tree::{AstIdMap, FileItemTree, MemberId, TypeId},
    scope::VisibleScope,
    symbols::{
        ImportKind, MemberKind, MemberSymbol, NullnessScope, ParamSummary, TypeKind, TypeSymbol,
        extract_field_decl, extract_local_type_symbol, extract_method_or_ctor,
        format_params_signature, node_ident_text,
    },
    ty::{
        Ty, resolve_type_ref_in_file_for_owner, ty_has_explicit_nullability,
        type_param_bounds_of_fqn_in_scope, type_params_of_fqn_in_scope,
    },
};

pub(crate) use lexical::{find_local_decl_node, resolve_via_static_imports_pub};
pub(crate) use overload::{
    CallableCandidate, OverloadPhase, collect_constructor_candidates,
    collect_method_candidates_on_ty, collect_static_import_method_candidates,
    collect_top_level_method_candidates, is_candidate_strictly_more_specific_in_scope,
    score_candidate_phase_in_scope, select_best_callable_candidate_in_scope,
};

use lexical::{
    enclosing_type_targets, find_local_binding, lookup_static_member_or_nested,
    lookup_static_member_or_nested_binary, lookup_type_by_fqn, lookup_workspace_or_binary_type,
    resolve_type_segments, resolve_via_static_imports,
};
use member::{
    CallableQuery, TypeRefTarget, lookup_member_on_type, lookup_method_across_hierarchy,
    lookup_method_or_member, lookup_non_method_member_or_nested_with_supers, resolve_expr_type,
    resolve_super_expr_target, select_callable_from_tree, target_to_type_ref, type_ref_from_fqn,
};

/// Kind of a resolved Java symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvedSymbolKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
    Field,
    Method,
    Constructor,
    EnumConstant,
    RecordComponent,
    LocalVar,
    Param,
    LambdaParam,
    ForEachVar,
    CatchParam,
    ResourceVar,
    PatternVar,
    TypeParam,
    Package,
}

impl From<TypeKind> for ResolvedSymbolKind {
    fn from(kind: TypeKind) -> Self {
        match kind {
            TypeKind::Class => Self::Class,
            TypeKind::Interface => Self::Interface,
            TypeKind::Enum => Self::Enum,
            TypeKind::Record => Self::Record,
            TypeKind::Annotation => Self::Annotation,
        }
    }
}

impl From<MemberKind> for ResolvedSymbolKind {
    fn from(kind: MemberKind) -> Self {
        match kind {
            MemberKind::Field => Self::Field,
            MemberKind::Method => Self::Method,
            MemberKind::Constructor => Self::Constructor,
            MemberKind::EnumConstant => Self::EnumConstant,
            MemberKind::RecordComponent => Self::RecordComponent,
        }
    }
}

/// Kind of a local variable or parameter binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalKind {
    LocalVar,
    Param,
    LambdaParam,
    ForEachVar,
    CatchParam,
    ResourceVar,
    PatternVar,
}

impl From<LocalKind> for ResolvedSymbolKind {
    fn from(kind: LocalKind) -> Self {
        match kind {
            LocalKind::LocalVar => Self::LocalVar,
            LocalKind::Param => Self::Param,
            LocalKind::LambdaParam => Self::LambdaParam,
            LocalKind::ForEachVar => Self::ForEachVar,
            LocalKind::CatchParam => Self::CatchParam,
            LocalKind::ResourceVar => Self::ResourceVar,
            LocalKind::PatternVar => Self::PatternVar,
        }
    }
}

/// Structured metadata carried by a [`ResolvedSymbol`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolDetail {
    TypeParams(Vec<SmolStr>),
    TypeRef(String),
    Method {
        return_type: Option<String>,
        params: String,
    },
    Constructor {
        params: String,
    },
    Bounds(Vec<String>),
}

/// A resolved semantic symbol with source location and display metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSymbol {
    pub file_id: Option<FileId>,
    pub full_range: Option<TextRange>,
    pub focus_range: Option<TextRange>,
    pub binary_target: Option<BinaryTarget>,
    pub name: SmolStr,
    pub qualified_name: String,
    pub detail: Option<SymbolDetail>,
    pub kind: ResolvedSymbolKind,
    /// Nearest overridden supertype method signature (`"OwnerFqn.methodName(ParamTypes...)"`),
    /// populated when resolving an overriding method declaration or reference.
    pub overrides: Option<String>,
}

impl ResolvedSymbol {
    /// Returns `(file_id, full_range, focus_range)` if this symbol has a concrete declaration site.
    pub fn location(&self) -> Option<(FileId, TextRange, TextRange)> {
        match (self.file_id, self.full_range, self.focus_range) {
            (Some(file_id), Some(full_range), Some(focus_range)) => {
                Some((file_id, full_range, focus_range))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalSymbol {
    pub(super) file_id: FileId,
    pub(super) name: SmolStr,
    pub(super) kind: LocalKind,
    pub(super) type_ref: Option<String>,
    pub(super) full_range: TextRange,
    pub(super) selection_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeParamSymbol {
    pub(super) file_id: FileId,
    pub(super) name: SmolStr,
    pub(super) bounds: Vec<String>,
    pub(super) full_range: TextRange,
    pub(super) selection_range: TextRange,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedTarget {
    IndexedType(FileId, TypeId),
    LocalType(TypeSymbol),
    BinaryType(String),
    IndexedMember(FileId, MemberId),
    LocalMember(MemberSymbol),
    BinaryMember {
        owner_fqn: String,
        member_index: usize,
    },
    Local(LocalSymbol),
    TypeParam(TypeParamSymbol),
    Package(String),
    WithDetail {
        inner: Box<ResolvedTarget>,
        detail: SymbolDetail,
    },
}

fn member_detail(
    kind: MemberKind,
    type_ref: Option<&str>,
    params: &[ParamSummary],
) -> Option<SymbolDetail> {
    match kind {
        MemberKind::Field | MemberKind::RecordComponent => {
            type_ref.map(|t| SymbolDetail::TypeRef(t.to_string()))
        }
        MemberKind::Method => Some(SymbolDetail::Method {
            return_type: type_ref.map(str::to_string),
            params: format_params_signature(params),
        }),
        MemberKind::Constructor => Some(SymbolDetail::Constructor {
            params: format_params_signature(params),
        }),
        MemberKind::EnumConstant => None,
    }
}

impl ResolvedTarget {
    pub(crate) fn is_static_member(&self, db: &dyn DefDatabase) -> bool {
        match self {
            Self::IndexedMember(fid, mid) => db.file_item_tree(*fid).members[*mid].is_static,
            Self::LocalMember(m) => m.is_static,
            Self::BinaryMember {
                owner_fqn,
                member_index,
            } => db
                .classpath_index()
                .type_by_fqn(owner_fqn)
                .and_then(|t| t.members.get(*member_index))
                .is_some_and(|m| m.is_static),
            Self::WithDetail { inner, .. } => inner.is_static_member(db),
            _ => false,
        }
    }

    fn into_resolved_symbol(
        self,
        db: &dyn DefDatabase,
        scope: &VisibleScope,
    ) -> Option<ResolvedSymbol> {
        let classpath = scope.classpath();
        match self {
            Self::IndexedType(file_id, type_id) => {
                let tree = db.file_item_tree(file_id);
                let t = &tree.types[type_id];
                let range = db.ast_id_map(file_id).range(t.ast_id);
                let detail = (!t.type_params.is_empty())
                    .then(|| SymbolDetail::TypeParams(t.type_params.clone()));
                Some(ResolvedSymbol {
                    file_id: Some(file_id),
                    full_range: Some(range.full_range),
                    focus_range: Some(range.selection_range),
                    binary_target: None,
                    name: t.name.clone(),
                    qualified_name: t.qualified_name.clone(),
                    detail,
                    kind: t.kind.into(),
                    overrides: None,
                })
            }
            Self::LocalType(t) => {
                let detail =
                    (!t.type_params.is_empty()).then_some(SymbolDetail::TypeParams(t.type_params));
                Some(ResolvedSymbol {
                    file_id: Some(t.file_id),
                    full_range: Some(t.full_range),
                    focus_range: Some(t.selection_range),
                    binary_target: None,
                    name: t.name,
                    qualified_name: t.qualified_name,
                    detail,
                    kind: t.kind.into(),
                    overrides: None,
                })
            }
            Self::BinaryType(fqn) => {
                let t = classpath.type_by_fqn(&fqn)?;
                let detail = (!t.type_params.is_empty())
                    .then(|| SymbolDetail::TypeParams(t.type_params.clone()));
                Some(ResolvedSymbol {
                    file_id: None,
                    full_range: None,
                    focus_range: None,
                    binary_target: Some(t.to_target(None)),
                    name: t.name.clone(),
                    qualified_name: t.qualified_name.clone(),
                    detail,
                    kind: t.kind.into(),
                    overrides: None,
                })
            }
            Self::IndexedMember(file_id, member_id) => {
                let detail = infer::format_member_detail_in_scope(db, scope, &self).or_else(|| {
                    let tree = db.file_item_tree(file_id);
                    let m = &tree.members[member_id];
                    member_detail(m.kind, m.type_ref.as_deref(), &m.params)
                });
                let tree = db.file_item_tree(file_id);
                let m = &tree.members[member_id];
                let range = db.ast_id_map(file_id).range(m.ast_id);
                let qualified_name = match &m.owner_type {
                    Some(owner) => format!("{owner}.{}", m.name),
                    None => m.name.to_string(),
                };
                let overrides = (m.kind == MemberKind::Method && !m.is_static)
                    .then(|| {
                        m.owner_type.as_deref().and_then(|owner_fqn| {
                            find_overridden_method_in_scope(
                                db,
                                scope,
                                owner_fqn,
                                &m.name,
                                m.params.len(),
                            )
                        })
                    })
                    .flatten();
                Some(ResolvedSymbol {
                    file_id: Some(file_id),
                    full_range: Some(range.full_range),
                    focus_range: Some(range.selection_range),
                    binary_target: None,
                    name: m.name.clone(),
                    qualified_name,
                    detail,
                    kind: m.kind.into(),
                    overrides,
                })
            }
            Self::LocalMember(m) => {
                let target_ref = Self::LocalMember(m.clone());
                let detail = infer::format_member_detail_in_scope(db, scope, &target_ref)
                    .or_else(|| member_detail(m.kind, m.type_ref.as_deref(), &m.params));
                let qualified_name = match &m.owner_type {
                    Some(owner) => format!("{owner}.{}", m.name),
                    None => m.name.to_string(),
                };
                let overrides = (m.kind == MemberKind::Method && !m.is_static)
                    .then(|| {
                        m.owner_type.as_deref().and_then(|owner_fqn| {
                            find_overridden_method_in_scope(
                                db,
                                scope,
                                owner_fqn,
                                &m.name,
                                m.params.len(),
                            )
                        })
                    })
                    .flatten();
                Some(ResolvedSymbol {
                    file_id: Some(m.file_id),
                    full_range: Some(m.full_range),
                    focus_range: Some(m.selection_range),
                    binary_target: None,
                    name: m.name,
                    qualified_name,
                    detail,
                    kind: m.kind.into(),
                    overrides,
                })
            }
            Self::BinaryMember {
                ref owner_fqn,
                member_index,
            } => {
                let detail = infer::format_member_detail_in_scope(db, scope, &self);
                let t = classpath.type_by_fqn(owner_fqn)?;
                let m = t.members.get(member_index)?;
                let detail = detail
                    .or_else(|| member_detail(m.kind, m.type_ref.as_deref(), &m.params));
                let overrides = (m.kind == MemberKind::Method && !m.is_static)
                    .then(|| {
                        find_overridden_method_in_scope(db, scope, owner_fqn, &m.name, m.params.len())
                    })
                    .flatten();
                Some(ResolvedSymbol {
                    file_id: None,
                    full_range: None,
                    focus_range: None,
                    binary_target: Some(t.to_target(Some(m.name.clone()))),
                    name: m.name.clone(),
                    qualified_name: format!("{}.{}", t.qualified_name, m.name),
                    detail,
                    kind: m.kind.into(),
                    overrides,
                })
            }
            Self::Local(l) => {
                let qualified_name = l.name.to_string();
                Some(ResolvedSymbol {
                    file_id: Some(l.file_id),
                    full_range: Some(l.full_range),
                    focus_range: Some(l.selection_range),
                    binary_target: None,
                    name: l.name,
                    qualified_name,
                    detail: l.type_ref.map(SymbolDetail::TypeRef),
                    kind: l.kind.into(),
                    overrides: None,
                })
            }
            Self::TypeParam(tp) => {
                let detail = (!tp.bounds.is_empty()).then_some(SymbolDetail::Bounds(tp.bounds));
                let qualified_name = tp.name.to_string();
                Some(ResolvedSymbol {
                    file_id: Some(tp.file_id),
                    full_range: Some(tp.full_range),
                    focus_range: Some(tp.selection_range),
                    binary_target: None,
                    name: tp.name,
                    qualified_name,
                    detail,
                    kind: ResolvedSymbolKind::TypeParam,
                    overrides: None,
                })
            }
            Self::Package(name) => {
                let short = SmolStr::new(name.rsplit('.').next().unwrap_or(&name));
                Some(ResolvedSymbol {
                    file_id: None,
                    full_range: None,
                    focus_range: None,
                    binary_target: None,
                    name: short,
                    qualified_name: name,
                    detail: None,
                    kind: ResolvedSymbolKind::Package,
                    overrides: None,
                })
            }
            Self::WithDetail { inner, detail } => {
                let mut sym = inner.into_resolved_symbol(db, scope)?;
                sym.detail = Some(detail);
                Some(sym)
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ResolveCtx<'a> {
    pub(crate) db: &'a dyn DefDatabase,
    pub(crate) file_id: FileId,
    pub(crate) file_tree: &'a FileItemTree,
    pub(crate) ast_id_map: &'a AstIdMap,
    pub(crate) scope: &'a VisibleScope,
}

/// Resolves the Java identifier or `super` keyword at byte `offset` in `file_id`, if any.
pub fn resolve_at_offset(
    db: &dyn DefDatabase,
    file_id: FileId,
    offset: u32,
) -> Option<ResolvedSymbol> {
    let root = db.syntax_tree(file_id);
    let offset_size = TextSize::from(offset);
    if offset_size > root.text_range().end() {
        return None;
    }

    let file_tree = db.file_item_tree(file_id);
    let ast_id_map = db.ast_id_map(file_id);
    let scope = VisibleScope::for_file(db, file_id);
    let ctx = ResolveCtx {
        db,
        file_id,
        file_tree: &file_tree,
        ast_id_map: &ast_id_map,
        scope: &scope,
    };

    let target = resolve_target_at_offset(ctx, &root, offset_size)?;
    target.into_resolved_symbol(db, &scope)
}

/// Resolves the declared type of the variable, parameter, field, record component, method return,
/// or expression at `offset` in `file_id` to its workspace or binary [`ResolvedSymbol`].
#[must_use]
pub fn resolve_type_of_symbol_at_offset(
    db: &dyn DefDatabase,
    file_id: FileId,
    offset: TextSize,
) -> Option<ResolvedSymbol> {
    let root = db.syntax_tree(file_id);
    if offset > root.text_range().end() {
        return None;
    }

    let file_tree = db.file_item_tree(file_id);
    let ast_id_map = db.ast_id_map(file_id);
    let scope = VisibleScope::for_file(db, file_id);
    let ctx = ResolveCtx {
        db,
        file_id,
        file_tree: &file_tree,
        ast_id_map: &ast_id_map,
        scope: &scope,
    };

    let token = root.token_at_offset(offset).right_biased()?;
    let parent = token.parent()?;

    if token.kind() == THIS_KW && parent.kind() == THIS_EXPR {
        let inferred = infer_expr_ty(db, file_id, &parent, None);
        if !matches!(inferred, Ty::Unknown) {
            return unwrap_ty_to_target(ctx, &inferred)
                .and_then(|t| ResolvedTarget::from(t).into_resolved_symbol(db, &scope));
        }
        let ty = resolve_expr_type(ctx, &parent, 0)?;
        return ResolvedTarget::from(ty).into_resolved_symbol(db, &scope);
    }

    if token.kind() == NEW_KW && parent.kind() == NEW_EXPR {
        let inferred = infer_expr_ty(db, file_id, &parent, None);
        if !matches!(inferred, Ty::Unknown) {
            return unwrap_ty_to_target(ctx, &inferred)
                .and_then(|t| ResolvedTarget::from(t).into_resolved_symbol(db, &scope));
        }
        let ty = resolve_expr_type(ctx, &parent, 0)?;
        return ResolvedTarget::from(ty).into_resolved_symbol(db, &scope);
    }

    if token.kind() == IDENT
        && parent.kind() == NAME
        && let Some(decl) = parent.parent()
        && matches!(
            decl.kind(),
            VARIABLE_DECLARATOR | PARAM | FOREACH_STMT | RESOURCE | TYPE_PATTERN | CATCH_CLAUSE
        )
        && decl.parent().is_none_or(|p| p.kind() != FIELD_DECL)
    {
        let inferred = infer_local_ty(db, file_id, &decl);
        if !matches!(inferred, Ty::Unknown) {
            return unwrap_ty_to_target(ctx, &inferred)
                .and_then(|t| ResolvedTarget::from(t).into_resolved_symbol(db, &scope));
        }
    }

    if let Some(target) = resolve_target_at_offset(ctx, &root, offset) {
        let unwrapped_inner = match &target {
            ResolvedTarget::WithDetail { inner, .. } => inner.as_ref(),
            other => other,
        };
        let is_type_or_ctor = matches!(
            unwrapped_inner,
            ResolvedTarget::IndexedType(_, _)
                | ResolvedTarget::LocalType(_)
                | ResolvedTarget::BinaryType(_)
        ) || match unwrapped_inner {
            ResolvedTarget::IndexedMember(m_file, m_id) => {
                db.file_item_tree(*m_file).members[*m_id].kind == MemberKind::Constructor
            }
            ResolvedTarget::LocalMember(m) => m.kind == MemberKind::Constructor,
            ResolvedTarget::BinaryMember {
                owner_fqn,
                member_index,
            } => scope
                .get_binary_type(owner_fqn)
                .and_then(|t| t.members.get(*member_index))
                .is_some_and(|m| m.kind == MemberKind::Constructor),
            _ => false,
        };

        if !is_type_or_ctor
            && token.kind() == IDENT
            && parent.kind() == NAME_REF
            && let Some(gp) = parent.parent()
        {
            let expr_node = if gp.kind() == NAME_REF_EXPR {
                if let Some(call) = gp.parent().filter(|p| {
                    p.kind() == CALL_EXPR
                        && p.children()
                            .next()
                            .is_some_and(|c| c.text_range() == gp.text_range())
                }) {
                    Some(call)
                } else {
                    Some(gp)
                }
            } else if matches!(gp.kind(), FIELD_ACCESS_EXPR | METHOD_CALL_EXPR) {
                Some(gp)
            } else {
                None
            };

            if let Some(expr) = expr_node {
                let inferred = infer_expr_ty(db, file_id, &expr, None);
                if !matches!(inferred, Ty::Unknown) {
                    return unwrap_ty_to_target(ctx, &inferred).and_then(|t| {
                        ResolvedTarget::from(t).into_resolved_symbol(db, &scope)
                    });
                }
            }
        }

        let ty = match unwrapped_inner.clone() {
            ResolvedTarget::IndexedMember(m_file, m_id) => {
                let m_tree = db.file_item_tree(m_file);
                if m_tree.members[m_id].kind == MemberKind::Constructor {
                    let owner_fqn = m_tree.members[m_id].owner_type.as_deref()?;
                    lookup_type_by_fqn(&m_tree, &scope, owner_fqn)
                        .map(|(fid, tid)| TypeRefTarget::Indexed(fid, tid))?
                } else {
                    target_to_type_ref(ctx, ResolvedTarget::IndexedMember(m_file, m_id), 0)?
                }
            }
            ResolvedTarget::LocalMember(m) if m.kind == MemberKind::Constructor => {
                let owner_fqn = m.owner_type.as_deref()?;
                let decl_tree = db.file_item_tree(m.file_id);
                lookup_type_by_fqn(&decl_tree, &scope, owner_fqn)
                    .map(|(fid, tid)| TypeRefTarget::Indexed(fid, tid))?
            }
            ResolvedTarget::BinaryMember {
                owner_fqn,
                member_index,
            } => {
                let owner_ty = scope.get_binary_type(&owner_fqn)?;
                let m = owner_ty.members.get(member_index)?;
                if m.kind == MemberKind::Constructor {
                    TypeRefTarget::Binary(owner_fqn)
                } else {
                    target_to_type_ref(
                        ctx,
                        ResolvedTarget::BinaryMember {
                            owner_fqn,
                            member_index,
                        },
                        0,
                    )?
                }
            }
            other => target_to_type_ref(ctx, other, 0)?,
        };
        return ResolvedTarget::from(ty).into_resolved_symbol(db, &scope);
    }

    None
}

fn unwrap_ty_to_target(ctx: ResolveCtx<'_>, ty: &Ty) -> Option<TypeRefTarget> {
    let mut cur = ty;
    for _ in 0..crate::ty::MAX_SUBST_DEPTH {
        match cur {
            Ty::Array { elem, .. } => cur = elem,
            Ty::Class { fqn, .. } => return type_ref_from_fqn(ctx, fqn),
            Ty::TypeVar { bounds, .. } => {
                cur = bounds.first()?;
            }
            Ty::Intersection(members) => {
                cur = members.first()?;
            }
            Ty::Primitive(_) | Ty::Void | Ty::Null | Ty::Unknown => return None,
        }
    }
    None
}

fn resolve_target_at_offset(
    ctx: ResolveCtx<'_>,
    root: &SyntaxNode,
    offset_size: TextSize,
) -> Option<ResolvedTarget> {
    let token = root.token_at_offset(offset_size).right_biased()?;
    let parent = token.parent()?;
    match token.kind() {
        IDENT => match parent.kind() {
            NAME => resolve_declaration_site(ctx, &parent),
            NAME_REF => resolve_reference_site(ctx, &parent),
            _ => None,
        },
        SUPER_KW if parent.kind() == SUPER_EXPR => resolve_super_expr_target(ctx, &parent),
        _ => None,
    }
}

pub(super) fn local_symbol_from_node(
    file_id: FileId,
    decl_node: &SyntaxNode,
    name_node: &SyntaxNode,
    kind: LocalKind,
    include_dims: bool,
) -> LocalSymbol {
    let type_node = decl_node.children().find(|n| is_type_node(n.kind()));
    local_symbol_with_type(
        file_id,
        decl_node,
        name_node,
        kind,
        type_node.as_ref(),
        include_dims,
    )
}

pub(super) fn local_symbol_with_type(
    file_id: FileId,
    decl_node: &SyntaxNode,
    name_node: &SyntaxNode,
    kind: LocalKind,
    type_node: Option<&SyntaxNode>,
    include_dims: bool,
) -> LocalSymbol {
    let type_ref = type_node.map(|t| {
        if include_dims {
            type_text_with_dims(t, decl_node)
        } else {
            collapsed_text(t)
        }
    });
    LocalSymbol {
        file_id,
        name: node_ident_text(name_node),
        kind,
        type_ref,
        full_range: decl_node.text_range(),
        selection_range: name_node.text_range(),
    }
}

fn enrich_local_inferred_type(
    ctx: ResolveCtx<'_>,
    decl_node: &SyntaxNode,
    mut local: LocalSymbol,
) -> LocalSymbol {
    let missing_or_var = local.type_ref.as_deref().is_none_or(|t| t == "var");
    let in_marked = matches!(
        infer::effective_nullness_scope_at(ctx.db, ctx.file_id, decl_node),
        NullnessScope::NonNullByDefault(_)
    );
    let has_at = local.type_ref.as_deref().is_some_and(|t| t.contains('@'));
    if missing_or_var || in_marked || has_at {
        let inferred = infer_local_ty(ctx.db, ctx.file_id, decl_node);
        if !matches!(inferred, Ty::Unknown)
            && (missing_or_var || in_marked || ty_has_explicit_nullability(&inferred))
        {
            local.type_ref = Some(inferred.display_short());
        }
    }
    local
}

fn resolve_declaration_site(ctx: ResolveCtx<'_>, name_node: &SyntaxNode) -> Option<ResolvedTarget> {
    let decl = name_node.parent()?;
    let range = name_node.text_range();

    match decl.kind() {
        k if TypeKind::from_syntax(k).is_some() => {
            if let Some(ast_id) = ctx.ast_id_map.find_by_selection(range)
                && let Some((type_id, _)) = ctx.file_tree.find_type_by_ast_id(ast_id)
            {
                return Some(ResolvedTarget::IndexedType(ctx.file_id, type_id));
            }
            extract_local_type_symbol(ctx.file_id, &decl).map(ResolvedTarget::LocalType)
        }
        VARIABLE_DECLARATOR => {
            let decl_parent = decl.parent()?;
            match decl_parent.kind() {
                FIELD_DECL => resolve_member_declaration(
                    ctx.file_id,
                    ctx.file_tree,
                    ctx.ast_id_map,
                    &decl,
                    name_node,
                    range,
                ),
                LOCAL_VAR_DECL => {
                    let base_type = decl_parent.children().find(|n| is_type_node(n.kind()));
                    let local = local_symbol_with_type(
                        ctx.file_id,
                        &decl,
                        name_node,
                        LocalKind::LocalVar,
                        base_type.as_ref(),
                        true,
                    );
                    Some(ResolvedTarget::Local(enrich_local_inferred_type(
                        ctx, &decl, local,
                    )))
                }
                _ => None,
            }
        }
        METHOD_DECL | CONSTRUCTOR_DECL | ENUM_CONSTANT | RECORD_COMPONENT => {
            resolve_member_declaration(
                ctx.file_id,
                ctx.file_tree,
                ctx.ast_id_map,
                &decl,
                name_node,
                range,
            )
        }
        PARAM => {
            let kind = if decl.parent().is_some_and(|p| p.kind() == LAMBDA_PARAMS) {
                LocalKind::LambdaParam
            } else {
                LocalKind::Param
            };
            let local = local_symbol_from_node(ctx.file_id, &decl, name_node, kind, true);
            Some(ResolvedTarget::Local(enrich_local_inferred_type(
                ctx, &decl, local,
            )))
        }
        FOREACH_STMT => {
            let local = local_symbol_from_node(
                ctx.file_id,
                &decl,
                name_node,
                LocalKind::ForEachVar,
                false,
            );
            Some(ResolvedTarget::Local(enrich_local_inferred_type(
                ctx, &decl, local,
            )))
        }
        CATCH_CLAUSE => {
            let local = local_symbol_from_node(
                ctx.file_id,
                &decl,
                name_node,
                LocalKind::CatchParam,
                false,
            );
            Some(ResolvedTarget::Local(enrich_local_inferred_type(
                ctx, &decl, local,
            )))
        }
        RESOURCE => {
            let local = local_symbol_from_node(
                ctx.file_id,
                &decl,
                name_node,
                LocalKind::ResourceVar,
                false,
            );
            Some(ResolvedTarget::Local(enrich_local_inferred_type(
                ctx, &decl, local,
            )))
        }
        TYPE_PATTERN => {
            let local = local_symbol_from_node(
                ctx.file_id,
                &decl,
                name_node,
                LocalKind::PatternVar,
                false,
            );
            Some(ResolvedTarget::Local(enrich_local_inferred_type(
                ctx, &decl, local,
            )))
        }
        TYPE_PARAM => {
            let bounds = decl
                .children()
                .filter(|n| is_type_node(n.kind()))
                .map(|t| collapsed_text(&t))
                .collect();
            Some(ResolvedTarget::TypeParam(TypeParamSymbol {
                file_id: ctx.file_id,
                name: node_ident_text(name_node),
                bounds,
                full_range: decl.text_range(),
                selection_range: range,
            }))
        }
        ANNOTATION_ARG => {
            let anno = decl.ancestors().find(|n| n.kind() == ANNOTATION)?;
            let path_node = anno.children().find(|n| n.kind() == PATH)?;
            let segs = path_segments(&path_node);
            let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
            let target_name = node_ident_text(name_node);
            match resolve_type_segments(ctx, &anno, &seg_refs)? {
                ResolvedTarget::IndexedType(ty_file, ty_id) => {
                    let ty_tree = ctx.db.file_item_tree(ty_file);
                    ty_tree.types[ty_id]
                        .members
                        .iter()
                        .copied()
                        .find(|&mid| {
                            let m = &ty_tree.members[mid];
                            m.kind == MemberKind::Method && m.name == target_name
                        })
                        .map(|mid| ResolvedTarget::IndexedMember(ty_file, mid))
                }
                ResolvedTarget::LocalType(anno_ty) => anno_ty
                    .members
                    .into_iter()
                    .find(|m| m.kind == MemberKind::Method && m.name == target_name)
                    .map(ResolvedTarget::LocalMember),
                ResolvedTarget::BinaryType(fqn) => {
                    let anno_ty = ctx.scope.get_binary_type(&fqn)?;
                    anno_ty
                        .members
                        .iter()
                        .position(|m| m.kind == MemberKind::Method && m.name == target_name)
                        .map(|member_index| ResolvedTarget::BinaryMember {
                            owner_fqn: fqn,
                            member_index,
                        })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn resolve_member_declaration(
    file_id: FileId,
    file_tree: &FileItemTree,
    ast_id_map: &AstIdMap,
    decl: &SyntaxNode,
    name_node: &SyntaxNode,
    range: TextRange,
) -> Option<ResolvedTarget> {
    if let Some(ast_id) = ast_id_map.find_by_selection(range)
        && let Some((member_id, _)) = file_tree.find_member_by_ast_id(ast_id)
    {
        return Some(ResolvedTarget::IndexedMember(file_id, member_id));
    }

    let member_node = if decl.kind() == VARIABLE_DECLARATOR {
        decl.parent()?
    } else {
        decl.clone()
    };

    // Walk up past `CLASS_BODY` / `ENUM_BODY` / `RECORD_COMPONENT_LIST` to the enclosing owner node.
    let body_or_list = member_node.parent()?;
    let owner = body_or_list.parent()?;

    if TypeKind::from_syntax(owner.kind()).is_some() {
        let local_ty = extract_local_type_symbol(file_id, &owner)?;
        return local_ty
            .members
            .into_iter()
            .find(|m| m.selection_range == range)
            .map(ResolvedTarget::LocalMember);
    }

    // Anonymous class body (`NEW_EXPR` or `ENUM_CONSTANT` -> `CLASS_BODY`).
    let local_member = match member_node.kind() {
        FIELD_DECL => {
            let mut out = Vec::new();
            extract_field_decl(file_id, &member_node, None, None, &mut out);
            out.into_iter().find(|m| m.selection_range == range)?
        }
        METHOD_DECL => extract_method_or_ctor(file_id, &member_node, None, MemberKind::Method)?,
        CONSTRUCTOR_DECL => {
            extract_method_or_ctor(file_id, &member_node, None, MemberKind::Constructor)?
        }
        ENUM_CONSTANT => MemberSymbol {
            file_id,
            owner_type: None,
            name: node_ident_text(name_node),
            kind: MemberKind::EnumConstant,
            is_static: true,
            is_default: false,
            is_abstract: false,
            has_override: false,
            nullness_scope: crate::NullnessScope::Inherited,
            decl_nullability: None,
            type_params: Vec::new(),
            type_param_bounds: std::collections::BTreeMap::new(),
            type_ref: None,
            params: Vec::new(),
            full_range: member_node.text_range(),
            selection_range: range,
        },
        _ => return None,
    };
    Some(ResolvedTarget::LocalMember(local_member))
}

fn resolve_reference_site(ctx: ResolveCtx<'_>, ref_node: &SyntaxNode) -> Option<ResolvedTarget> {
    let ref_parent = ref_node.parent()?;

    match ref_parent.kind() {
        PATH => resolve_path_ref(ctx, &ref_parent, ref_node),
        PATH_TYPE => resolve_path_type_ref(ctx, &ref_parent, ref_node),
        FIELD_ACCESS_EXPR => resolve_field_access_ref(ctx, &ref_parent, ref_node),
        METHOD_CALL_EXPR => resolve_method_call_ref(ctx, &ref_parent, ref_node),
        METHOD_REF_EXPR => resolve_method_ref(ctx, &ref_parent, ref_node),
        NAME_REF_EXPR => resolve_name_ref_expr(ctx, &ref_parent, ref_node),
        _ => None,
    }
}

fn prefix_segments_up_to(container: &SyntaxNode, target_ref: &SyntaxNode) -> Vec<SmolStr> {
    let target_end = target_ref.text_range().end();
    container
        .children()
        .filter(|n| n.kind() == NAME_REF && n.text_range().end() <= target_end)
        .map(|n| node_ident_text(&n))
        .collect()
}

fn resolve_path_ref(
    ctx: ResolveCtx<'_>,
    path_node: &SyntaxNode,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let owner = path_node.parent()?;
    let prefix = prefix_segments_up_to(path_node, ref_node);
    if prefix.is_empty() {
        return None;
    }

    match owner.kind() {
        PACKAGE_DECL => Some(ResolvedTarget::Package(prefix.join("."))),
        IMPORT_DECL => {
            let import_ast_id = ctx.ast_id_map.find_by_full(owner.text_range())?;
            let (_, import_tree) = ctx
                .file_tree
                .imports
                .iter()
                .find(|(_, imp)| imp.ast_id == import_ast_id)?;
            if import_tree.kind == ImportKind::Module {
                return None;
            }

            let all_segs = path_segments(path_node);
            let is_last = prefix.len() == all_segs.len();

            if import_tree.kind == ImportKind::StaticSingle && is_last && all_segs.len() >= 2 {
                let type_fqn = all_segs[..all_segs.len() - 1].join(".");
                let member_name = all_segs.last()?;
                if let Some((owner_file, owner_id)) =
                    lookup_type_by_fqn(ctx.file_tree, ctx.scope, &type_fqn)
                {
                    return lookup_static_member_or_nested(
                        ctx,
                        owner_file,
                        owner_id,
                        member_name,
                        None,
                    );
                }
                if ctx.scope.contains_binary_type(&type_fqn) {
                    return lookup_static_member_or_nested_binary(
                        ctx,
                        &type_fqn,
                        member_name,
                        None,
                    );
                }
                return None;
            }

            let prefix_fqn = prefix.join(".");
            if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, &prefix_fqn) {
                return Some(ResolvedTarget::IndexedType(fid, tid));
            }
            if ctx.scope.contains_binary_type(&prefix_fqn) {
                return Some(ResolvedTarget::BinaryType(prefix_fqn));
            }
            if ctx.scope.has_package_prefix(&prefix_fqn) {
                return Some(ResolvedTarget::Package(prefix_fqn));
            }
            None
        }
        ANNOTATION => {
            let seg_refs: Vec<&str> = prefix.iter().map(SmolStr::as_str).collect();
            resolve_type_segments(ctx, path_node, &seg_refs)
        }
        _ => None,
    }
}

fn resolve_path_type_ref(
    ctx: ResolveCtx<'_>,
    path_type: &SyntaxNode,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let prefix = prefix_segments_up_to(path_type, ref_node);
    if prefix.is_empty() {
        return None;
    }

    // Qualified `outer.new Inner()` resolves `Inner` relative to `outer`'s type.
    if let Some(new_expr) = path_type.parent().filter(|p| p.kind() == NEW_EXPR)
        && has_direct_token(&new_expr, DOT)
        && let Some(outer_expr) = new_expr.children().next()
    {
        let outer_ty = resolve_expr_type(ctx, &outer_expr, 0)?;
        let mut current_qname = outer_ty.qualified_name(ctx.db, ctx.file_tree);
        let mut resolved = None;
        for seg in &prefix {
            let next_fqn = format!("{current_qname}.{seg}");
            resolved = Some(lookup_workspace_or_binary_type(ctx, &next_fqn)?);
            current_qname = next_fqn;
        }
        return resolved;
    }

    let seg_refs: Vec<&str> = prefix.iter().map(SmolStr::as_str).collect();
    if let Some(found) = resolve_type_segments(ctx, path_type, &seg_refs) {
        return Some(found);
    }

    if seg_refs == ["var"]
        && let Some(parent) = path_type.parent()
    {
        let decl_node = match parent.kind() {
            LOCAL_VAR_DECL => parent
                .children()
                .find(|n| n.kind() == VARIABLE_DECLARATOR),
            FOREACH_STMT | RESOURCE | PARAM | TYPE_PATTERN => Some(parent),
            _ => None,
        };
        if let Some(decl) = decl_node {
            let inferred = infer_local_ty(ctx.db, ctx.file_id, &decl);
            if !matches!(inferred, Ty::Unknown) {
                return unwrap_ty_to_target(ctx, &inferred).map(ResolvedTarget::from);
            }
        }
    }

    None
}

fn resolve_field_access_ref(
    ctx: ResolveCtx<'_>,
    field_access: &SyntaxNode,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    // 1. Qualified type path check (e.g. `com.example.Foo` or `com.example.Outer.Inner`).
    if let Some(chain) = extract_dotted_name_chain(field_access) {
        let fqn = chain.join(".");
        if let Some(found) = lookup_workspace_or_binary_type(ctx, &fqn) {
            return Some(found);
        }
    }

    // 2. Resolve receiver expression's type and look up member/nested type on it.
    let receiver = field_access.children().next()?;
    let member_name = node_ident_text(ref_node);

    let inferred_recv = infer_expr_ty(ctx.db, ctx.file_id, &receiver, None);
    if !matches!(inferred_recv, Ty::Unknown) {
        if let Some(fqn) = inferred_recv.erased_fqn()
            && let Some(recv_target) = type_ref_from_fqn(ctx, fqn)
            && let Some(found) = lookup_member_on_type(ctx, &recv_target, &member_name, None, false)
        {
            return Some(infer::resolve_field_access_target(
                ctx.db,
                ctx.file_id,
                &inferred_recv,
                found,
            ));
        }
        if matches!(inferred_recv, Ty::Array { .. })
            && let Some(obj_target) = type_ref_from_fqn(ctx, "java.lang.Object")
            && let Some(found) = lookup_member_on_type(ctx, &obj_target, &member_name, None, false)
        {
            return Some(found);
        }
    }

    let recv_ty = resolve_expr_type(ctx, &receiver, 0)?;
    lookup_member_on_type(ctx, &recv_ty, &member_name, None, false)
}

fn resolve_method_call_ref(
    ctx: ResolveCtx<'_>,
    method_call: &SyntaxNode,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    if let Some(found) = infer::resolve_method_call_target(ctx.db, ctx.file_id, method_call) {
        return Some(found);
    }

    let receiver = method_call.children().next()?;
    let member_name = node_ident_text(ref_node);
    let arg_count = method_call
        .children()
        .find(|n| n.kind() == ARG_LIST)
        .map(|al| count_args(&al));

    let inferred_recv = infer_expr_ty(ctx.db, ctx.file_id, &receiver, None);
    if let Some(fqn) = inferred_recv.erased_fqn()
        && let Some(recv_target) = type_ref_from_fqn(ctx, fqn)
        && let Some(found) = lookup_member_on_type(ctx, &recv_target, &member_name, arg_count, true)
    {
        return Some(found);
    }

    let recv_ty = resolve_expr_type(ctx, &receiver, 0)?;
    lookup_member_on_type(ctx, &recv_ty, &member_name, arg_count, true)
}

fn resolve_method_ref(
    ctx: ResolveCtx<'_>,
    method_ref: &SyntaxNode,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    if let Some(found) =
        infer::resolve_method_ref_target_with_context(ctx.db, ctx.file_id, method_ref)
    {
        return Some(found);
    }

    let qualifier = method_ref.children().next()?;
    let target_ty = if qualifier.kind() == PATH_TYPE {
        let segs = path_segments(&qualifier);
        let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
        match resolve_type_segments(ctx, &qualifier, &seg_refs)? {
            ResolvedTarget::IndexedType(fid, tid) => TypeRefTarget::Indexed(fid, tid),
            ResolvedTarget::LocalType(ty) => TypeRefTarget::Local(Box::new(ty)),
            ResolvedTarget::BinaryType(fqn) => TypeRefTarget::Binary(fqn),
            _ => return None,
        }
    } else {
        resolve_expr_type(ctx, &qualifier, 0)?
    };

    let member_name = node_ident_text(ref_node);
    lookup_method_or_member(ctx, &target_ty, &member_name)
}

pub(super) fn resolve_name_ref_expr(
    ctx: ResolveCtx<'_>,
    expr_node: &SyntaxNode,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let name = node_ident_text(ref_node);

    // Check if `expr_node` is the callee of a `CALL_EXPR` (`foo(args)`).
    if let Some(call_expr) = expr_node.parent().filter(|p| {
        p.kind() == CALL_EXPR
            && p.children()
                .next()
                .is_some_and(|c| c.text_range() == expr_node.text_range())
    }) {
        if let Some(found) = infer::resolve_unqualified_call_target(ctx.db, ctx.file_id, &call_expr)
        {
            return Some(found);
        }

        let arg_count = call_expr
            .children()
            .find(|n| n.kind() == ARG_LIST)
            .map(|al| count_args(&al));

        // 1. Enclosing types' methods (innermost to outermost, walking supertypes).
        for ty in enclosing_type_targets(ctx.file_id, ctx.file_tree, ctx.ast_id_map, expr_node) {
            if let Some(found) = lookup_method_across_hierarchy(ctx, &ty, &name, arg_count) {
                return Some(found);
            }
        }
        // Compact-source top-level methods.
        if let Some(mid) = select_callable_from_tree(
            ctx.file_tree,
            &ctx.file_tree.top_level_members,
            CallableQuery {
                target_kind: MemberKind::Method,
                name: &name,
                call_arity: arg_count,
                include_record_components: false,
                only_static: false,
                exclude_static: false,
            },
        )
        .map(member::CallableMatch::into_inner)
        {
            return Some(ResolvedTarget::IndexedMember(ctx.file_id, mid));
        }

        // 2. Static imports (Single first, then OnDemand).
        if let Some(m) = resolve_via_static_imports(ctx, &name, arg_count, true) {
            return Some(m);
        }
        return None;
    }

    // Non-call unqualified identifier:
    // 1. Lexical local scopes (innermost to outermost).
    if let Some((local, decl_node)) = find_local_binding(ctx.file_id, ref_node, &name) {
        return Some(ResolvedTarget::Local(enrich_local_inferred_type(
            ctx, &decl_node, local,
        )));
    }

    // 2. Enclosing type(s) members (direct + inherited fields, record components, enum constants,
    //    nested types) plus compact-source top-level fields.
    for ty in enclosing_type_targets(ctx.file_id, ctx.file_tree, ctx.ast_id_map, expr_node) {
        if let Some(found) = lookup_non_method_member_or_nested_with_supers(ctx, &ty, &name) {
            let enc_fqn = ty.qualified_name(ctx.db, ctx.file_tree);
            let tp_bounds = type_param_bounds_of_fqn_in_scope(ctx.db, ctx.scope, &enc_fqn);
            let enc_args = type_params_of_fqn_in_scope(ctx.db, ctx.scope, &enc_fqn)
                .into_iter()
                .map(|tp| {
                    let tv = resolve_type_ref_in_file_for_owner(
                        ctx.db,
                        ctx.file_id,
                        Some(&enc_fqn),
                        tp.as_str(),
                        &tp_bounds,
                    );
                    match tv {
                        t @ Ty::TypeVar { .. } => crate::ty::TypeArg::Type(t),
                        _ => crate::ty::TypeArg::Type(Ty::type_var(tp, Vec::new())),
                    }
                })
                .collect();
            let recv_ty = Ty::class(enc_fqn, enc_args);
            return Some(infer::resolve_field_access_target(
                ctx.db,
                ctx.file_id,
                &recv_ty,
                found,
            ));
        }
    }
    if let Some(&mid) = ctx.file_tree.top_level_members.iter().find(|&&mid| {
        let m = &ctx.file_tree.members[mid];
        m.kind != MemberKind::Method && m.name == name
    }) {
        return Some(ResolvedTarget::IndexedMember(ctx.file_id, mid));
    }

    // 3. Static imports (fields, enum constants, nested types).
    if let Some(found) = resolve_via_static_imports(ctx, &name, None, false) {
        return Some(found);
    }

    // 4. Type & import resolution (allows `Foo` in `Foo.BAR` or `Foo::bar` to resolve to `Foo`).
    resolve_type_segments(ctx, expr_node, &[&name])
}

pub(crate) fn extract_dotted_name_chain(node: &SyntaxNode) -> Option<Vec<SmolStr>> {
    let mut out = Vec::new();
    collect_dotted_chain(node, &mut out).then_some(out)
}

fn collect_dotted_chain(node: &SyntaxNode, out: &mut Vec<SmolStr>) -> bool {
    match node.kind() {
        NAME_REF_EXPR => match node.children().find(|n| n.kind() == NAME_REF) {
            Some(nr) => {
                out.push(node_ident_text(&nr));
                true
            }
            _ => false,
        },
        FIELD_ACCESS_EXPR => {
            let Some(recv) = node.children().next() else {
                return false;
            };
            let Some(nr) = node.children().find(|n| n.kind() == NAME_REF) else {
                return false;
            };
            if collect_dotted_chain(&recv, out) {
                out.push(node_ident_text(&nr));
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

pub(super) fn count_args(arg_list: &SyntaxNode) -> usize {
    arg_list.children().count()
}
