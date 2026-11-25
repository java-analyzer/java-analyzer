//! Pure, IO-free Java name resolution at a source offset.
//! See `docs/specs/0005-symbol-indexing-and-name-resolution.md`.

use std::collections::{BTreeMap, VecDeque};

use rustc_hash::FxHashSet;
use smol_str::SmolStr;
use syntax::{
    SyntaxNode, TextRange, TextSize,
    ext::{
        collapsed_text, has_direct_token, has_modifier, is_type_node, path_segments,
        type_text_with_dims,
    },
    syntax_kind::SyntaxKind::*,
};

use crate::{
    DefDatabase, FileId,
    classpath::{BinaryTarget, ClasspathIndex},
    hierarchy::{
        direct_supertypes, find_overridden_method, linearized_supertypes, parse_type_ref_segments,
    },
    infer::{
        self, erase_type_vars_to_bound, find_functional_interface_sam, infer_expr_ty,
        infer_local_ty, is_raw_class_ty, parameter_for_arg, resolve_params_in_binary_for_member,
        resolve_params_in_file_for_member,
    },
    item_tree::{AstIdMap, FileItemTree, MemberId, TypeId},
    symbols::{
        ImportKind, MemberKind, MemberSymbol, NullnessScope, ParamSummary, TypeKind, TypeSymbol,
        WorkspaceIndex, extract_field_decl, extract_local_type_symbol, extract_method_or_ctor,
        format_params_signature, node_ident_text,
    },
    ty::{
        Substitution, Ty, is_assignable, is_subtype, resolve_binary_type_ref_for_member,
        resolve_type_ref_in_file_for_member, resolve_type_ref_in_file_for_owner,
        supertype_substitution_for_ty, ty_has_explicit_nullability, type_param_bounds_of_fqn,
    },
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
    file_id: FileId,
    name: SmolStr,
    kind: LocalKind,
    type_ref: Option<String>,
    full_range: TextRange,
    selection_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeParamSymbol {
    file_id: FileId,
    name: SmolStr,
    bounds: Vec<String>,
    full_range: TextRange,
    selection_range: TextRange,
}

#[derive(Debug, Clone)]
enum TypeRefTarget {
    Indexed(FileId, TypeId),
    Local(Box<TypeSymbol>),
    Binary(String),
}

impl TypeRefTarget {
    fn qualified_name<'a>(&'a self, db: &dyn DefDatabase, own_tree: &'a FileItemTree) -> String {
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

impl From<TypeRefTarget> for ResolvedTarget {
    fn from(target: TypeRefTarget) -> Self {
        match target {
            TypeRefTarget::Indexed(fid, tid) => Self::IndexedType(fid, tid),
            TypeRefTarget::Local(ty) => Self::LocalType(*ty),
            TypeRefTarget::Binary(fqn) => Self::BinaryType(fqn),
        }
    }
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
        classpath: &ClasspathIndex,
    ) -> Option<ResolvedSymbol> {
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
                let detail = infer::format_member_detail(db, &self).or_else(|| {
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
                            find_overridden_method(db, owner_fqn, &m.name, m.params.len())
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
                let detail = infer::format_member_detail(db, &target_ref)
                    .or_else(|| member_detail(m.kind, m.type_ref.as_deref(), &m.params));
                let qualified_name = match &m.owner_type {
                    Some(owner) => format!("{owner}.{}", m.name),
                    None => m.name.to_string(),
                };
                let overrides = (m.kind == MemberKind::Method && !m.is_static)
                    .then(|| {
                        m.owner_type.as_deref().and_then(|owner_fqn| {
                            find_overridden_method(db, owner_fqn, &m.name, m.params.len())
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
                let detail = infer::format_member_detail(db, &self);
                let t = classpath.type_by_fqn(owner_fqn)?;
                let m = t.members.get(member_index)?;
                let detail = detail
                    .or_else(|| member_detail(m.kind, m.type_ref.as_deref(), &m.params));
                let overrides = (m.kind == MemberKind::Method && !m.is_static)
                    .then(|| find_overridden_method(db, owner_fqn, &m.name, m.params.len()))
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
                let mut sym = inner.into_resolved_symbol(db, classpath)?;
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
    pub(crate) index: &'a WorkspaceIndex,
    pub(crate) classpath: &'a ClasspathIndex,
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
    let index = db.workspace_index();
    let classpath = db.classpath_index();
    let ctx = ResolveCtx {
        db,
        file_id,
        file_tree: &file_tree,
        ast_id_map: &ast_id_map,
        index: &index,
        classpath: &classpath,
    };

    let target = resolve_target_at_offset(ctx, &root, offset_size)?;
    target.into_resolved_symbol(db, &classpath)
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
    let index = db.workspace_index();
    let classpath = db.classpath_index();
    let ctx = ResolveCtx {
        db,
        file_id,
        file_tree: &file_tree,
        ast_id_map: &ast_id_map,
        index: &index,
        classpath: &classpath,
    };

    let token = root.token_at_offset(offset).right_biased()?;
    let parent = token.parent()?;

    if token.kind() == THIS_KW && parent.kind() == THIS_EXPR {
        let inferred = infer_expr_ty(db, file_id, &parent, None);
        if !matches!(inferred, Ty::Unknown) {
            return unwrap_ty_to_target(ctx, &inferred)
                .and_then(|t| ResolvedTarget::from(t).into_resolved_symbol(db, &classpath));
        }
        let ty = resolve_expr_type(ctx, &parent, 0)?;
        return ResolvedTarget::from(ty).into_resolved_symbol(db, &classpath);
    }

    if token.kind() == NEW_KW && parent.kind() == NEW_EXPR {
        let inferred = infer_expr_ty(db, file_id, &parent, None);
        if !matches!(inferred, Ty::Unknown) {
            return unwrap_ty_to_target(ctx, &inferred)
                .and_then(|t| ResolvedTarget::from(t).into_resolved_symbol(db, &classpath));
        }
        let ty = resolve_expr_type(ctx, &parent, 0)?;
        return ResolvedTarget::from(ty).into_resolved_symbol(db, &classpath);
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
                .and_then(|t| ResolvedTarget::from(t).into_resolved_symbol(db, &classpath));
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
            } => classpath
                .type_by_fqn(owner_fqn)
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
                        ResolvedTarget::from(t).into_resolved_symbol(db, &classpath)
                    });
                }
            }
        }

        let ty = match unwrapped_inner.clone() {
            ResolvedTarget::IndexedMember(m_file, m_id) => {
                let m_tree = db.file_item_tree(m_file);
                if m_tree.members[m_id].kind == MemberKind::Constructor {
                    let owner_fqn = m_tree.members[m_id].owner_type.as_deref()?;
                    lookup_type_by_fqn(&m_tree, &index, owner_fqn)
                        .map(|(fid, tid)| TypeRefTarget::Indexed(fid, tid))?
                } else {
                    target_to_type_ref(ctx, ResolvedTarget::IndexedMember(m_file, m_id), 0)?
                }
            }
            ResolvedTarget::LocalMember(m) if m.kind == MemberKind::Constructor => {
                let owner_fqn = m.owner_type.as_deref()?;
                let decl_tree = db.file_item_tree(m.file_id);
                lookup_type_by_fqn(&decl_tree, &index, owner_fqn)
                    .map(|(fid, tid)| TypeRefTarget::Indexed(fid, tid))?
            }
            ResolvedTarget::BinaryMember {
                owner_fqn,
                member_index,
            } => {
                let owner_ty = classpath.type_by_fqn(&owner_fqn)?;
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
        return ResolvedTarget::from(ty).into_resolved_symbol(db, &classpath);
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

fn local_symbol_from_node(
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

fn local_symbol_with_type(
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
                    let anno_ty = ctx.classpath.type_by_fqn(&fqn)?;
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
                    lookup_type_by_fqn(ctx.file_tree, ctx.index, &type_fqn)
                {
                    return lookup_static_member_or_nested(
                        ctx,
                        owner_file,
                        owner_id,
                        member_name,
                        None,
                    );
                }
                if ctx.classpath.type_by_fqn(&type_fqn).is_some() {
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
            if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, &prefix_fqn) {
                return Some(ResolvedTarget::IndexedType(fid, tid));
            }
            if ctx.classpath.type_by_fqn(&prefix_fqn).is_some() {
                return Some(ResolvedTarget::BinaryType(prefix_fqn));
            }
            if ctx.index.has_package_prefix(&prefix_fqn) || ctx.classpath.has_package(&prefix_fqn)
            {
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
    if let Some(found) = infer::resolve_method_ref_target_with_context(ctx.db, ctx.file_id, method_ref) {
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

fn resolve_name_ref_expr(
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
            MemberKind::Method,
            &name,
            arg_count,
            false,
            false,
            false,
        )
        .map(CallableMatch::into_inner)
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
            let tp_bounds = type_param_bounds_of_fqn(ctx.db, &enc_fqn);
            let enc_args = crate::ty::type_params_of_fqn(ctx.db, &enc_fqn)
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

// ---- Lexical local scope resolution ----

pub(crate) fn find_local_decl_node(ref_node: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    find_local_binding(FileId(0), ref_node, name).map(|(_, decl_node)| decl_node)
}

fn find_local_binding(
    file_id: FileId,
    ref_node: &SyntaxNode,
    name: &str,
) -> Option<(LocalSymbol, SyntaxNode)> {
    let ref_range = ref_node.text_range();
    let mut child_on_path = ref_node.clone();

    for ancestor in ref_node.ancestors().skip(1) {
        match ancestor.kind() {
            BLOCK => {
                if let Some(found) = find_local_var_in_block(file_id, &ancestor, ref_range, name) {
                    return Some(found);
                }
            }
            FOR_STMT => {
                for decl in ancestor.children().filter(|n| n.kind() == LOCAL_VAR_DECL) {
                    if let Some(found) = find_declarator_before(file_id, &decl, ref_range, name) {
                        return Some(found);
                    }
                }
            }
            FOREACH_STMT => {
                let after_paren = ancestor
                    .children_with_tokens()
                    .find(|e| e.as_token().is_some_and(|t| t.kind() == R_PAREN))
                    .map(|e| e.text_range().end());
                if after_paren.is_some_and(|end| ref_range.start() >= end)
                    && let Some(name_node) = ancestor.children().find(|n| n.kind() == NAME)
                    && name_node.text() == name
                {
                    let sym = local_symbol_from_node(
                        file_id,
                        &ancestor,
                        &name_node,
                        LocalKind::ForEachVar,
                        false,
                    );
                    return Some((sym, ancestor));
                }
            }
            CATCH_CLAUSE if child_on_path.kind() == BLOCK => {
                if let Some(name_node) = ancestor.children().find(|n| n.kind() == NAME)
                    && name_node.text() == name
                {
                    let sym = local_symbol_from_node(
                        file_id,
                        &ancestor,
                        &name_node,
                        LocalKind::CatchParam,
                        false,
                    );
                    return Some((sym, ancestor));
                }
            }
            TRY_STMT => {
                let in_try_scope = child_on_path.kind() == RESOURCE_LIST
                    || ancestor
                        .children()
                        .find(|n| n.kind() == BLOCK)
                        .is_some_and(|b| b.text_range() == child_on_path.text_range());
                if in_try_scope
                    && let Some(res_list) = ancestor.children().find(|n| n.kind() == RESOURCE_LIST)
                {
                    for res in rev_children(&res_list).filter(|n| n.kind() == RESOURCE) {
                        if res.text_range().end() <= ref_range.start()
                            && let Some(name_node) = res.children().find(|n| n.kind() == NAME)
                            && name_node.text() == name
                        {
                            let sym = local_symbol_from_node(
                                file_id,
                                &res,
                                &name_node,
                                LocalKind::ResourceVar,
                                false,
                            );
                            return Some((sym, res));
                        }
                    }
                }
            }
            LAMBDA_EXPR => {
                if let Some(params_node) = ancestor.children().find(|n| n.kind() == LAMBDA_PARAMS)
                    && ref_range.start() >= params_node.text_range().end()
                {
                    for param in rev_children(&params_node).filter(|n| n.kind() == PARAM) {
                        if let Some(name_node) = param.children().find(|n| n.kind() == NAME)
                            && name_node.text() == name
                        {
                            let sym = local_symbol_from_node(
                                file_id,
                                &param,
                                &name_node,
                                LocalKind::LambdaParam,
                                true,
                            );
                            return Some((sym, param));
                        }
                    }
                }
            }
            BINARY_EXPR => {
                if let Some(amp2) = ancestor
                    .children_with_tokens()
                    .find(|e| e.as_token().is_some_and(|t| t.kind() == AMP2))
                    && ref_range.start() >= amp2.text_range().end()
                    && let Some(lhs) = ancestor.children().next()
                {
                    let mut patterns = Vec::new();
                    collect_positive_patterns(file_id, &lhs, &mut patterns);
                    if let Some(found) = patterns.into_iter().rev().find(|(p, _)| p.name == name) {
                        return Some(found);
                    }
                }
            }
            IF_STMT => {
                let r_paren_end = ancestor
                    .children_with_tokens()
                    .find(|e| e.as_token().is_some_and(|t| t.kind() == R_PAREN))
                    .map(|e| e.text_range().end());
                let else_start = ancestor
                    .children_with_tokens()
                    .find(|e| e.as_token().is_some_and(|t| t.kind() == ELSE_KW))
                    .map(|e| e.text_range().start());
                let in_then = r_paren_end.is_some_and(|s| child_on_path.text_range().start() >= s)
                    && else_start.is_none_or(|e| child_on_path.text_range().end() <= e);
                if in_then
                    && let Some(cond) = ancestor
                        .children()
                        .find(|c| r_paren_end.is_none_or(|end| c.text_range().end() <= end))
                {
                    let mut patterns = Vec::new();
                    collect_positive_patterns(file_id, &cond, &mut patterns);
                    if let Some(found) = patterns.into_iter().rev().find(|(p, _)| p.name == name) {
                        return Some(found);
                    }
                }
            }
            SWITCH_CASE => {
                if let Some(label) = ancestor.children().find(|n| n.kind() == SWITCH_LABEL) {
                    let mut patterns = Vec::new();
                    for desc in label.descendants() {
                        if desc.kind() == TYPE_PATTERN
                            && desc.text_range().end() <= ref_range.start()
                        {
                            collect_type_pattern(file_id, &desc, &mut patterns);
                        }
                    }
                    if let Some(found) = patterns.into_iter().rev().find(|(p, _)| p.name == name) {
                        return Some(found);
                    }
                }
            }
            METHOD_DECL | CONSTRUCTOR_DECL => {
                if let Some(param_list) = ancestor.children().find(|n| n.kind() == PARAM_LIST)
                    && ref_range.start() >= param_list.text_range().end()
                {
                    for param in rev_children(&param_list).filter(|n| n.kind() == PARAM) {
                        if let Some(name_node) = param.children().find(|n| n.kind() == NAME)
                            && name_node.text() == name
                        {
                            let sym = local_symbol_from_node(
                                file_id,
                                &param,
                                &name_node,
                                LocalKind::Param,
                                true,
                            );
                            return Some((sym, param));
                        }
                    }
                }
                break;
            }
            INITIALIZER | FIELD_DECL | SOURCE_FILE => break,
            k if TypeKind::from_syntax(k).is_some() => break,
            _ => {}
        }

        child_on_path = ancestor;
    }

    None
}

fn rev_children(node: &SyntaxNode) -> impl Iterator<Item = SyntaxNode> {
    std::iter::successors(node.last_child(), SyntaxNode::prev_sibling)
}

fn find_local_var_in_block(
    file_id: FileId,
    block: &SyntaxNode,
    ref_range: TextRange,
    name: &str,
) -> Option<(LocalSymbol, SyntaxNode)> {
    for decl in rev_children(block).filter(|n| n.kind() == LOCAL_VAR_DECL) {
        if let Some(found) = find_declarator_before(file_id, &decl, ref_range, name) {
            return Some(found);
        }
    }
    None
}

fn find_declarator_before(
    file_id: FileId,
    local_var_decl: &SyntaxNode,
    ref_range: TextRange,
    name: &str,
) -> Option<(LocalSymbol, SyntaxNode)> {
    let base_type = local_var_decl.children().find(|n| is_type_node(n.kind()));
    for declarator in rev_children(local_var_decl).filter(|n| n.kind() == VARIABLE_DECLARATOR) {
        if declarator.text_range().end() > ref_range.start() {
            continue;
        }
        if let Some(name_node) = declarator.children().find(|n| n.kind() == NAME)
            && name_node.text() == name
        {
            let sym = local_symbol_with_type(
                file_id,
                &declarator,
                &name_node,
                LocalKind::LocalVar,
                base_type.as_ref(),
                true,
            );
            return Some((sym, declarator));
        }
    }
    None
}

fn collect_positive_patterns(
    file_id: FileId,
    node: &SyntaxNode,
    out: &mut Vec<(LocalSymbol, SyntaxNode)>,
) {
    match node.kind() {
        PAREN_EXPR => {
            if let Some(inner) = node.children().next() {
                collect_positive_patterns(file_id, &inner, out);
            }
        }
        BINARY_EXPR if has_direct_token(node, AMP2) => {
            let mut children = node.children();
            if let Some(lhs) = children.next() {
                collect_positive_patterns(file_id, &lhs, out);
            }
            if let Some(rhs) = children.next() {
                collect_positive_patterns(file_id, &rhs, out);
            }
        }
        INSTANCEOF_EXPR => {
            for child in node.children() {
                match child.kind() {
                    TYPE_PATTERN => collect_type_pattern(file_id, &child, out),
                    RECORD_PATTERN => collect_record_pattern(file_id, &child, out),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn collect_record_pattern(
    file_id: FileId,
    node: &SyntaxNode,
    out: &mut Vec<(LocalSymbol, SyntaxNode)>,
) {
    for child in node.children() {
        match child.kind() {
            TYPE_PATTERN => collect_type_pattern(file_id, &child, out),
            RECORD_PATTERN => collect_record_pattern(file_id, &child, out),
            _ => {}
        }
    }
}

fn collect_type_pattern(
    file_id: FileId,
    node: &SyntaxNode,
    out: &mut Vec<(LocalSymbol, SyntaxNode)>,
) {
    if let Some(name_node) = node.children().find(|n| n.kind() == NAME) {
        let sym = local_symbol_from_node(file_id, node, &name_node, LocalKind::PatternVar, false);
        out.push((sym, node.clone()));
    }
}

// ---- Type & import resolution ----

fn resolve_type_segments(
    ctx: ResolveCtx<'_>,
    site_node: &SyntaxNode,
    segs: &[&str],
) -> Option<ResolvedTarget> {
    if segs.is_empty() {
        return None;
    }

    if segs.len() == 1 {
        let name = segs[0];

        // 1. Walk enclosing scopes innermost-to-outermost (local types, method/type params,
        // enclosing types and their nested/inherited nested types), stopping type parameter
        // propagation across `static` boundaries.
        if let Some(found) = resolve_enclosing_type_or_param(ctx, site_node, name) {
            return Some(found);
        }

        // 2. Single-type imports (workspace first, then binary classpath).
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::SingleType
                && imp.path.last().map(String::as_str) == Some(name)
            {
                let fqn = imp.path.join(".");
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, &fqn) {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
        }
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::SingleType
                && imp.path.last().map(String::as_str) == Some(name)
            {
                let fqn = imp.path.join(".");
                if ctx.classpath.type_by_fqn(&fqn).is_some() {
                    return Some(ResolvedTarget::BinaryType(fqn));
                }
            }
        }

        // 3. Static single imports resolving to a nested type (workspace first, then binary).
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::StaticSingle
                && imp.path.last().map(String::as_str) == Some(name)
            {
                let fqn = imp.path.join(".");
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, &fqn) {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
        }
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::StaticSingle
                && imp.path.last().map(String::as_str) == Some(name)
            {
                let fqn = imp.path.join(".");
                if ctx.classpath.type_by_fqn(&fqn).is_some() {
                    return Some(ResolvedTarget::BinaryType(fqn));
                }
            }
        }

        // 4. Same-package top-level types (workspace first, then binary).
        let pkg = ctx.file_tree.package.as_deref().unwrap_or("");
        if let Some(found) = lookup_package_type_in_workspace(ctx.file_tree, ctx.index, pkg, name)
        {
            return Some(found);
        }
        if let Some(ty) = ctx.classpath.type_in_package(pkg, name) {
            return Some(ResolvedTarget::BinaryType(ty.qualified_name.clone()));
        }

        // 5. Implicit `java.lang.*` (workspace `java.lang` first if any, then binary `java.lang`).
        if let Some(found) =
            lookup_package_type_in_workspace(ctx.file_tree, ctx.index, "java.lang", name)
        {
            return Some(found);
        }
        if let Some(ty) = ctx.classpath.type_in_package("java.lang", name) {
            return Some(ResolvedTarget::BinaryType(ty.qualified_name.clone()));
        }

        // 6. Type-import-on-demand (`import a.b.*;`) (workspace first, then binary).
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::TypeOnDemand {
                let fqn = format!("{}.{name}", imp.path.join("."));
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, &fqn) {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
        }
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::TypeOnDemand {
                let fqn = format!("{}.{name}", imp.path.join("."));
                if ctx.classpath.type_by_fqn(&fqn).is_some() {
                    return Some(ResolvedTarget::BinaryType(fqn));
                }
            }
        }

        // 7. Static-import-on-demand (`import static a.b.Outer.*;`) (workspace first, then binary).
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::StaticOnDemand {
                let fqn = format!("{}.{name}", imp.path.join("."));
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, &fqn) {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
        }
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::StaticOnDemand {
                let fqn = format!("{}.{name}", imp.path.join("."));
                if ctx.classpath.type_by_fqn(&fqn).is_some() {
                    return Some(ResolvedTarget::BinaryType(fqn));
                }
            }
        }

        // 8. Module imports (`import module M;`, JEP 511, transitive & cycle-safe).
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::Module {
                let module_name = imp.path.join(".");
                for exported_pkg in ctx.classpath.transitive_module_exports(&module_name) {
                    if let Some(found) = lookup_package_type_in_workspace(
                        ctx.file_tree,
                        ctx.index,
                        exported_pkg,
                        name,
                    ) {
                        return Some(found);
                    }
                    if let Some(ty) = ctx.classpath.type_in_package(exported_pkg, name) {
                        return Some(ResolvedTarget::BinaryType(ty.qualified_name.clone()));
                    }
                }
            }
        }

        return None;
    }

    // Multi-segment `[s0, s1, ..., sk]`:
    // 1. Direct FQN match (`com.example.Foo`, `java.util.Map.Entry`).
    let joined = segs.join(".");
    if let Some(found) = lookup_workspace_or_binary_type(ctx, &joined) {
        return Some(found);
    }

    // 2. Resolve `s0` as a single-segment type, then walk nested types `s1..=sk`.
    let first_qname = match resolve_type_segments(ctx, site_node, &segs[..1])? {
        ResolvedTarget::IndexedType(fid, tid) => {
            TypeRefTarget::Indexed(fid, tid).qualified_name(ctx.db, ctx.file_tree)
        }
        ResolvedTarget::LocalType(ty) => ty.qualified_name,
        ResolvedTarget::BinaryType(fqn) => fqn,
        _ => return None,
    };
    let mut current_qname = first_qname;
    let mut resolved = None;
    for seg in &segs[1..] {
        let next_fqn = format!("{current_qname}.{seg}");
        if let Some(found) = lookup_workspace_or_binary_type(ctx, &next_fqn) {
            resolved = Some(found);
            current_qname = next_fqn;
        } else {
            let mut inherited_found = None;
            for sup_fqn in linearized_supertypes(ctx.db, &current_qname) {
                let sup_next = format!("{sup_fqn}.{seg}");
                if let Some(found) = lookup_workspace_or_binary_type(ctx, &sup_next) {
                    inherited_found = Some((found, sup_next));
                    break;
                }
            }
            let (found, next) = inherited_found?;
            resolved = Some(found);
            current_qname = next;
        }
    }
    resolved
}

fn lookup_package_type_in_workspace(
    file_tree: &FileItemTree,
    index: &WorkspaceIndex,
    pkg: &str,
    name: &str,
) -> Option<ResolvedTarget> {
    if let Some(fqn) = index.package_type_map.get(pkg).and_then(|m| m.get(name))
        && let Some(&(fid, tid)) = index.types_by_fqn.get(fqn)
    {
        return Some(ResolvedTarget::IndexedType(fid, tid));
    }
    if file_tree.package.as_deref().unwrap_or("") == pkg
        && let Some((tid, _)) = file_tree
            .types
            .iter()
            .find(|(_, t)| t.enclosing_type.is_none() && t.name == name)
    {
        return Some(ResolvedTarget::IndexedType(file_tree.file_id, tid));
    }
    None
}

fn lookup_workspace_or_binary_type(ctx: ResolveCtx<'_>, fqn: &str) -> Option<ResolvedTarget> {
    if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, fqn) {
        return Some(ResolvedTarget::IndexedType(fid, tid));
    }
    ctx.classpath
        .type_by_fqn(fqn)
        .map(|_| ResolvedTarget::BinaryType(fqn.to_string()))
}

fn resolve_enclosing_type_or_param(
    ctx: ResolveCtx<'_>,
    site_node: &SyntaxNode,
    name: &str,
) -> Option<ResolvedTarget> {
    let file_id = ctx.file_id;
    let file_tree = ctx.file_tree;
    let ast_id_map = ctx.ast_id_map;
    let index = ctx.index;
    let site_start = site_node.text_range().start();
    let mut allow_type_params = true;

    for ancestor in site_node.ancestors() {
        match ancestor.kind() {
            BLOCK => {
                for ty_node in rev_children(&ancestor)
                    .filter(|n| TypeKind::from_syntax(n.kind()).is_some())
                {
                    if let Some(name_node) = ty_node.children().find(|n| n.kind() == NAME)
                        && name_node.text() == name
                        && name_node.text_range().end() <= site_start
                        && let Some(local_ty) = extract_local_type_symbol(file_id, &ty_node)
                    {
                        return Some(ResolvedTarget::LocalType(local_ty));
                    }
                }
            }
            METHOD_DECL | CONSTRUCTOR_DECL => {
                if allow_type_params
                    && let Some(tp) = find_type_param_on_node(file_id, &ancestor, name)
                {
                    return Some(ResolvedTarget::TypeParam(tp));
                }
                if ancestor.kind() == METHOD_DECL && has_modifier(&ancestor, STATIC_KW) {
                    allow_type_params = false;
                }
            }
            FIELD_DECL | INITIALIZER
                if has_modifier(&ancestor, STATIC_KW) || has_direct_token(&ancestor, STATIC_KW) =>
            {
                allow_type_params = false;
            }
            k if TypeKind::from_syntax(k).is_some() => {
                if allow_type_params
                    && let Some(tp) = find_type_param_on_node(file_id, &ancestor, name)
                {
                    return Some(ResolvedTarget::TypeParam(tp));
                }

                let range = ancestor.text_range();
                let indexed = ast_id_map
                    .find_by_full(range)
                    .and_then(|ast_id| file_tree.find_type_by_ast_id(ast_id));

                let mut is_static_boundary = has_modifier(&ancestor, STATIC_KW);
                if let Some((type_id, ty)) = indexed {
                    is_static_boundary |= ty.is_static
                        || matches!(
                            ty.kind,
                            TypeKind::Interface
                                | TypeKind::Enum
                                | TypeKind::Record
                                | TypeKind::Annotation
                        );
                    if ty.name == name {
                        return Some(ResolvedTarget::IndexedType(file_id, type_id));
                    }
                    if ty.nested_types.iter().any(|n| n == name) {
                        let nested_fqn = format!("{}.{}", ty.qualified_name, name);
                        if let Some((fid, tid)) = lookup_type_by_fqn(file_tree, index, &nested_fqn)
                        {
                            return Some(ResolvedTarget::IndexedType(fid, tid));
                        }
                    }
                    for sup_fqn in linearized_supertypes(ctx.db, &ty.qualified_name) {
                        let inherited_fqn = format!("{sup_fqn}.{name}");
                        if let Some(found) = lookup_workspace_or_binary_type(ctx, &inherited_fqn) {
                            return Some(found);
                        }
                    }
                } else if let Some(ty) = extract_local_type_symbol(file_id, &ancestor) {
                    is_static_boundary |= ty.is_static
                        || matches!(
                            ty.kind,
                            TypeKind::Interface
                                | TypeKind::Enum
                                | TypeKind::Record
                                | TypeKind::Annotation
                        );
                    if ty.name == name {
                        return Some(ResolvedTarget::LocalType(ty));
                    }
                    if ty.nested_types.iter().any(|n| n == name) {
                        let nested_fqn = format!("{}.{}", ty.qualified_name, name);
                        if let Some((fid, tid)) = lookup_type_by_fqn(file_tree, index, &nested_fqn)
                        {
                            return Some(ResolvedTarget::IndexedType(fid, tid));
                        }
                    }
                    let local_target = TypeRefTarget::Local(Box::new(ty));
                    for sup_fqn in supertypes_of_target(ctx, &local_target) {
                        let inherited_fqn = format!("{sup_fqn}.{name}");
                        if let Some(found) = lookup_workspace_or_binary_type(ctx, &inherited_fqn) {
                            return Some(found);
                        }
                    }
                }

                if is_static_boundary {
                    allow_type_params = false;
                }
            }
            _ => {}
        }
    }

    None
}

fn find_type_param_on_node(
    file_id: FileId,
    node: &SyntaxNode,
    name: &str,
) -> Option<TypeParamSymbol> {
    let tps = node.children().find(|n| n.kind() == TYPE_PARAMS)?;
    for tp in tps.children().filter(|n| n.kind() == TYPE_PARAM) {
        if let Some(name_node) = tp.children().find(|n| n.kind() == NAME)
            && name_node.text() == name
        {
            let bounds = tp
                .children()
                .filter(|n| is_type_node(n.kind()))
                .map(|t| collapsed_text(&t))
                .collect();
            return Some(TypeParamSymbol {
                file_id,
                name: node_ident_text(&name_node),
                bounds,
                full_range: tp.text_range(),
                selection_range: name_node.text_range(),
            });
        }
    }
    None
}

fn enclosing_type_targets(
    file_id: FileId,
    file_tree: &FileItemTree,
    ast_id_map: &AstIdMap,
    node: &SyntaxNode,
) -> Vec<TypeRefTarget> {
    let mut out = Vec::new();
    for ancestor in node.ancestors() {
        if TypeKind::from_syntax(ancestor.kind()).is_some() {
            let range = ancestor.text_range();
            if let Some((type_id, _)) = ast_id_map
                .find_by_full(range)
                .and_then(|ast_id| file_tree.find_type_by_ast_id(ast_id))
            {
                out.push(TypeRefTarget::Indexed(file_id, type_id));
            } else if let Some(local_ty) = extract_local_type_symbol(file_id, &ancestor) {
                out.push(TypeRefTarget::Local(Box::new(local_ty)));
            }
        }
    }
    out
}

fn lookup_type_by_fqn(
    file_tree: &FileItemTree,
    index: &WorkspaceIndex,
    fqn: &str,
) -> Option<(FileId, TypeId)> {
    index.types_by_fqn.get(fqn).copied().or_else(|| {
        file_tree
            .find_type_by_fqn(fqn)
            .map(|(tid, _)| (file_tree.file_id, tid))
    })
}

// ---- Member lookup & static imports ----

pub(crate) fn resolve_via_static_imports_pub(
    ctx: ResolveCtx<'_>,
    name: &str,
    call_arity: Option<usize>,
    prefer_callable: bool,
) -> Option<ResolvedTarget> {
    resolve_via_static_imports(ctx, name, call_arity, prefer_callable)
}

fn resolve_via_static_imports(
    ctx: ResolveCtx<'_>,
    name: &str,
    call_arity: Option<usize>,
    prefer_callable: bool,
) -> Option<ResolvedTarget> {
    let callable_arity = prefer_callable.then_some(call_arity);

    // 1. Static single imports (`import static a.b.Foo.BAR;`): workspace first, then binary.
    for (_, imp) in ctx.file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticSingle
            && imp.path.len() >= 2
            && imp.path.last().map(String::as_str) == Some(name)
        {
            let owner_fqn = imp.path[..imp.path.len() - 1].join(".");
            if let Some((owner_file, owner_id)) =
                lookup_type_by_fqn(ctx.file_tree, ctx.index, &owner_fqn)
                && let Some(found) = lookup_static_member_or_nested(
                    ctx,
                    owner_file,
                    owner_id,
                    name,
                    callable_arity,
                )
            {
                return Some(found);
            }
        }
    }
    for (_, imp) in ctx.file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticSingle
            && imp.path.len() >= 2
            && imp.path.last().map(String::as_str) == Some(name)
        {
            let owner_fqn = imp.path[..imp.path.len() - 1].join(".");
            if ctx.classpath.type_by_fqn(&owner_fqn).is_some()
                && let Some(found) = lookup_static_member_or_nested_binary(
                    ctx,
                    &owner_fqn,
                    name,
                    callable_arity,
                )
            {
                return Some(found);
            }
        }
    }

    // 2. Static-import-on-demand (`import static a.b.Foo.*;`): workspace first, then binary.
    for (_, imp) in ctx.file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticOnDemand && !imp.path.is_empty() {
            let owner_fqn = imp.path.join(".");
            if let Some((owner_file, owner_id)) =
                lookup_type_by_fqn(ctx.file_tree, ctx.index, &owner_fqn)
                && let Some(found) = lookup_static_member_or_nested(
                    ctx,
                    owner_file,
                    owner_id,
                    name,
                    callable_arity,
                )
            {
                return Some(found);
            }
        }
    }
    for (_, imp) in ctx.file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticOnDemand && !imp.path.is_empty() {
            let owner_fqn = imp.path.join(".");
            if ctx.classpath.type_by_fqn(&owner_fqn).is_some()
                && let Some(found) = lookup_static_member_or_nested_binary(
                    ctx,
                    &owner_fqn,
                    name,
                    callable_arity,
                )
            {
                return Some(found);
            }
        }
    }

    None
}

fn lookup_static_member_or_nested(
    ctx: ResolveCtx<'_>,
    owner_file: FileId,
    owner_id: TypeId,
    name: &str,
    callable_arity: Option<Option<usize>>,
) -> Option<ResolvedTarget> {
    let owner_tree = ctx.db.file_item_tree(owner_file);
    let owner_ty = &owner_tree.types[owner_id];

    if let Some(arity) = callable_arity {
        return select_callable_from_tree(
            &owner_tree,
            &owner_ty.members,
            MemberKind::Method,
            name,
            arity,
            false,
            true,
            false,
        )
        .map(|m| ResolvedTarget::IndexedMember(owner_file, m.into_inner()));
    }

    if let Some(&mid) = owner_ty.members.iter().find(|&&mid| {
        let m = &owner_tree.members[mid];
        m.is_static && m.kind != MemberKind::Method && m.name == name
    }) {
        return Some(ResolvedTarget::IndexedMember(owner_file, mid));
    }

    if owner_ty.nested_types.iter().any(|n| n == name) {
        let nested_fqn = format!("{}.{}", owner_ty.qualified_name, name);
        if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, &nested_fqn) {
            return Some(ResolvedTarget::IndexedType(fid, tid));
        }
    }

    // Fallback: static method (e.g. inside `import static a.b.Foo.bar;`).
    if let Some(&mid) = owner_ty.members.iter().find(|&&mid| {
        let m = &owner_tree.members[mid];
        m.is_static && m.kind == MemberKind::Method && m.name == name
    }) {
        return Some(ResolvedTarget::IndexedMember(owner_file, mid));
    }

    None
}

fn lookup_static_member_or_nested_binary(
    ctx: ResolveCtx<'_>,
    owner_fqn: &str,
    name: &str,
    callable_arity: Option<Option<usize>>,
) -> Option<ResolvedTarget> {
    let owner_ty = ctx.classpath.type_by_fqn(owner_fqn)?;

    if let Some(arity) = callable_arity {
        return select_callable_from_binary(
            &owner_ty.members,
            MemberKind::Method,
            name,
            arity,
            false,
            true,
            false,
        )
        .map(|m| ResolvedTarget::BinaryMember {
            owner_fqn: owner_fqn.to_string(),
            member_index: m.into_inner(),
        });
    }

    if let Some(member_index) = owner_ty.members.iter().position(|m| {
        m.is_static
            && m.kind != MemberKind::Method
            && m.kind != MemberKind::Constructor
            && m.name == name
    }) {
        return Some(ResolvedTarget::BinaryMember {
            owner_fqn: owner_fqn.to_string(),
            member_index,
        });
    }

    if owner_ty.nested_types.iter().any(|n| n == name) {
        let nested_fqn = format!("{}.{}", owner_ty.qualified_name, name);
        if let Some(found) = lookup_workspace_or_binary_type(ctx, &nested_fqn) {
            return Some(found);
        }
    }

    if let Some(member_index) = owner_ty
        .members
        .iter()
        .position(|m| m.is_static && m.kind == MemberKind::Method && m.name == name)
    {
        return Some(ResolvedTarget::BinaryMember {
            owner_fqn: owner_fqn.to_string(),
            member_index,
        });
    }

    None
}

#[derive(Clone, Copy)]
enum CallableMatch<ID> {
    Exact(ID),
    Varargs(ID),
    Fallback(ID),
}

impl<ID> CallableMatch<ID> {
    fn into_inner(self) -> ID {
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

fn target_is_interface(ctx: ResolveCtx<'_>, ty: &TypeRefTarget) -> bool {
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
            .classpath
            .type_by_fqn(fqn)
            .is_some_and(|b| matches!(b.kind, TypeKind::Interface | TypeKind::Annotation)),
    }
}

fn supertypes_of_target(ctx: ResolveCtx<'_>, ty: &TypeRefTarget) -> Vec<String> {
    match ty {
        TypeRefTarget::Indexed(_, _) | TypeRefTarget::Binary(_) => {
            let fqn = ty.qualified_name(ctx.db, ctx.file_tree);
            linearized_supertypes(ctx.db, &fqn)
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
                for s in linearized_supertypes(ctx.db, &sup_fqn) {
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
                for s in linearized_supertypes(ctx.db, &iface) {
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

fn type_ref_from_fqn(ctx: ResolveCtx<'_>, fqn: &str) -> Option<TypeRefTarget> {
    if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, fqn) {
        return Some(TypeRefTarget::Indexed(fid, tid));
    }
    ctx.classpath
        .type_by_fqn(fqn)
        .map(|_| TypeRefTarget::Binary(fqn.to_string()))
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
                MemberKind::Method,
                name,
                call_arity,
                type_item.kind == TypeKind::Record,
                false,
                exclude_static,
            )
            .map(|m| m.map(|mid| ResolvedTarget::IndexedMember(*ty_file, mid)))
        }
        TypeRefTarget::Local(local_ty) => select_callable_from_slice(
            &local_ty.members,
            MemberKind::Method,
            name,
            call_arity,
            local_ty.kind == TypeKind::Record,
            false,
            exclude_static,
        )
        .map(|m| m.map(|mem| ResolvedTarget::LocalMember(mem.clone()))),
        TypeRefTarget::Binary(fqn) => {
            let bin_ty = ctx.classpath.type_by_fqn(fqn)?;
            select_callable_from_binary(
                &bin_ty.members,
                MemberKind::Method,
                name,
                call_arity,
                bin_ty.kind == TypeKind::Record,
                false,
                exclude_static,
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

fn lookup_method_across_hierarchy(
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

fn lookup_non_method_member_or_nested_with_supers(
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

fn lookup_member_on_type(
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
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, &nested_fqn)
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
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, &nested_fqn)
                {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
            None
        }
        TypeRefTarget::Binary(fqn) => {
            let bin_ty = ctx.classpath.type_by_fqn(fqn)?;
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

fn lookup_method_or_member(
    ctx: ResolveCtx<'_>,
    ty: &TypeRefTarget,
    name: &str,
) -> Option<ResolvedTarget> {
    if let Some(found) = lookup_method_across_hierarchy(ctx, ty, name, None) {
        return Some(found);
    }
    lookup_non_method_member_or_nested_with_supers(ctx, ty, name)
}

#[expect(
    clippy::too_many_arguments,
    reason = "forwards callable matching flags into select_callable_by"
)]
fn select_callable_from_tree(
    tree: &FileItemTree,
    member_ids: &[MemberId],
    target_kind: MemberKind,
    name: &str,
    call_arity: Option<usize>,
    include_record_components: bool,
    only_static: bool,
    exclude_static: bool,
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
        target_kind,
        name,
        call_arity,
        include_record_components,
        only_static,
        exclude_static,
    )
}

fn select_callable_from_slice<'a>(
    members: &'a [MemberSymbol],
    target_kind: MemberKind,
    name: &str,
    call_arity: Option<usize>,
    include_record_components: bool,
    only_static: bool,
    exclude_static: bool,
) -> Option<CallableMatch<&'a MemberSymbol>> {
    select_callable_by(
        members
            .iter()
            .enumerate()
            .map(|(i, m)| (i, m.kind, m.name.as_str(), m.is_static, m.params.as_slice())),
        target_kind,
        name,
        call_arity,
        include_record_components,
        only_static,
        exclude_static,
    )
    .map(|m| m.map(|idx| &members[idx]))
}

fn select_callable_from_binary(
    members: &[crate::classpath::BinaryMemberSymbol],
    target_kind: MemberKind,
    name: &str,
    call_arity: Option<usize>,
    include_record_components: bool,
    only_static: bool,
    exclude_static: bool,
) -> Option<CallableMatch<usize>> {
    select_callable_by(
        members
            .iter()
            .enumerate()
            .map(|(i, m)| (i, m.kind, m.name.as_str(), m.is_static, m.params.as_slice())),
        target_kind,
        name,
        call_arity,
        include_record_components,
        only_static,
        exclude_static,
    )
}

fn select_callable_by<'a, I, ID>(
    members: I,
    target_kind: MemberKind,
    name: &str,
    call_arity: Option<usize>,
    include_record_components: bool,
    only_static: bool,
    exclude_static: bool,
) -> Option<CallableMatch<ID>>
where
    ID: Copy,
    I: Iterator<Item = (ID, MemberKind, &'a str, bool, &'a [ParamSummary])>,
{
    let mut first_match: Option<ID> = None;
    let mut varargs_match: Option<ID> = None;
    let mut record_comp_match: Option<ID> = None;

    for (id, kind, m_name, is_static, params) in members {
        if target_kind == MemberKind::Method && m_name != name {
            continue;
        }
        if kind == MemberKind::RecordComponent {
            if target_kind == MemberKind::Method
                && !only_static
                && include_record_components
                && call_arity.is_none_or(|a| a == 0)
                && record_comp_match.is_none()
            {
                record_comp_match = Some(id);
            }
            continue;
        }
        if kind != target_kind || (only_static && !is_static) || (exclude_static && is_static) {
            continue;
        }
        if first_match.is_none() {
            first_match = Some(id);
        }
        if let Some(arity) = call_arity {
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
                MemberKind::Constructor,
                "",
                arg_count,
                false,
                false,
                false,
            )
            .map(|m| ResolvedTarget::IndexedMember(*ty_file, m.into_inner()))
        }
        TypeRefTarget::Local(local_ty) => select_callable_from_slice(
            &local_ty.members,
            MemberKind::Constructor,
            "",
            arg_count,
            false,
            false,
            false,
        )
        .map(|m| ResolvedTarget::LocalMember(m.into_inner().clone())),
        TypeRefTarget::Binary(fqn) => {
            let bin_ty = ctx.classpath.type_by_fqn(fqn)?;
            select_callable_from_binary(
                &bin_ty.members,
                MemberKind::Constructor,
                "",
                arg_count,
                false,
                false,
                false,
            )
            .map(|m| ResolvedTarget::BinaryMember {
                owner_fqn: fqn.clone(),
                member_index: m.into_inner(),
            })
        }
    }
}

fn resolve_super_target_type(ctx: ResolveCtx<'_>, super_expr: &SyntaxNode) -> Option<TypeRefTarget> {
    // Qualified `Interface.super` or `Outer.super`.
    if let Some(qualifier) = super_expr.children().next() {
        let segs = extract_dotted_name_chain(&qualifier)?;
        let seg_refs: Vec<&str> = segs.iter().map(SmolStr::as_str).collect();
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
        let sup_fqn = direct_supertypes(ctx.db, &qual_fqn).super_class?;
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
                let sup_fqn = direct_supertypes(ctx.db, &ty.qualified_name).super_class?;
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

fn resolve_super_expr_target(
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
        if let Some(best) =
            select_best_callable_candidate(ctx.db, ctors, &arg_tys, Some(&arg_nodes))
        {
            return Some(best.target);
        }
        return lookup_constructor_on_type(ctx, &super_ty, Some(arg_nodes.len()))
            .or_else(|| Some(ResolvedTarget::from(super_ty)));
    }

    resolve_super_target_type(ctx, super_expr).map(ResolvedTarget::from)
}

// ---- Receiver type resolution ----

const MAX_TYPE_DEPTH: u8 = 32;

fn resolve_expr_type(
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
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, &fqn) {
                    return Some(TypeRefTarget::Indexed(fid, tid));
                }
                if ctx.classpath.type_by_fqn(&fqn).is_some() {
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

fn target_to_type_ref(
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
                ctx.index,
                ctx.classpath,
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
                        ctx.index,
                        ctx.classpath,
                        m_file,
                        decl_offset,
                        ty_str,
                    )
                }
                MemberKind::EnumConstant => {
                    let owner_fqn = m.owner_type.as_deref()?;
                    lookup_type_by_fqn(&decl_tree, ctx.index, owner_fqn)
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
                    ctx.index,
                    ctx.classpath,
                    m.file_id,
                    m.selection_range.start(),
                    ty_str,
                )
            }
            MemberKind::EnumConstant => {
                let owner_fqn = m.owner_type.as_deref()?;
                let decl_tree = ctx.db.file_item_tree(m.file_id);
                lookup_type_by_fqn(&decl_tree, ctx.index, owner_fqn)
                    .map(|(fid, tid)| TypeRefTarget::Indexed(fid, tid))
            }
            MemberKind::Constructor => None,
        },
        ResolvedTarget::BinaryMember {
            owner_fqn,
            member_index,
        } => {
            let owner_ty = ctx.classpath.type_by_fqn(&owner_fqn)?;
            let m = owner_ty.members.get(member_index)?;
            match m.kind {
                MemberKind::Field | MemberKind::RecordComponent | MemberKind::Method => {
                    let ty_str = m.type_ref.as_deref()?;
                    resolve_binary_type_ref(ctx.index, ctx.classpath, &owner_fqn, ty_str)
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
    index: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    owner_fqn: &str,
    type_ref: &str,
) -> Option<TypeRefTarget> {
    let segs = parse_type_ref_segments(type_ref)?;
    let joined = segs.join(".");
    if let Some(&(fid, tid)) = index.types_by_fqn.get(&joined) {
        return Some(TypeRefTarget::Indexed(fid, tid));
    }
    if classpath.type_by_fqn(&joined).is_some() {
        return Some(TypeRefTarget::Binary(joined));
    }
    if let Some(pkg) = classpath.type_by_fqn(owner_fqn).map(|t| t.package.as_str())
        && !pkg.is_empty()
    {
        let same_pkg_fqn = format!("{pkg}.{joined}");
        if let Some(&(fid, tid)) = index.types_by_fqn.get(&same_pkg_fqn) {
            return Some(TypeRefTarget::Indexed(fid, tid));
        }
        if classpath.type_by_fqn(&same_pkg_fqn).is_some() {
            return Some(TypeRefTarget::Binary(same_pkg_fqn));
        }
    }
    let java_lang_fqn = format!("java.lang.{joined}");
    if let Some(&(fid, tid)) = index.types_by_fqn.get(&java_lang_fqn) {
        return Some(TypeRefTarget::Indexed(fid, tid));
    }
    if classpath.type_by_fqn(&java_lang_fqn).is_some() {
        return Some(TypeRefTarget::Binary(java_lang_fqn));
    }
    None
}

fn resolve_type_ref_in_file(
    db: &dyn DefDatabase,
    index: &WorkspaceIndex,
    classpath: &ClasspathIndex,
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
    let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
    let decl_ctx = ResolveCtx {
        db,
        file_id: decl_file_id,
        file_tree: &decl_tree,
        ast_id_map: &decl_map,
        index,
        classpath,
    };
    match resolve_type_segments(decl_ctx, &site_node, &seg_refs)? {
        ResolvedTarget::IndexedType(fid, tid) => Some(TypeRefTarget::Indexed(fid, tid)),
        ResolvedTarget::LocalType(ty) => Some(TypeRefTarget::Local(Box::new(ty))),
        ResolvedTarget::BinaryType(fqn) => Some(TypeRefTarget::Binary(fqn)),
        _ => None,
    }
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

fn count_args(arg_list: &SyntaxNode) -> usize {
    arg_list.children().count()
}

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
        for sup in linearized_supertypes(ctx.db, &root) {
            if visited.insert(sup.clone()) {
                ordered_owners.push((sup, true));
            }
        }
    }

    let is_raw = is_raw_class_ty(ctx.db, recv_ty);
    let mut out = Vec::new();
    for (owner_fqn, is_super) in ordered_owners {
        let owner_subst = supertype_substitution_for_ty(ctx.db, recv_ty, &owner_fqn)
            .unwrap_or_default();
        collect_owner_methods(
            ctx,
            &owner_fqn,
            name,
            arity,
            is_super,
            false,
            is_raw,
            &owner_subst,
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

    for (_, imp) in ctx.file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticSingle
            && imp.path.len() >= 2
            && imp.path.last().map(String::as_str) == Some(name)
        {
            let owner_fqn = imp.path[..imp.path.len() - 1].join(".");
            collect_owner_methods(
                ctx,
                &owner_fqn,
                name,
                arity,
                false,
                true,
                false,
                &Substitution::default(),
                &mut out,
            );
        }
    }

    for (_, imp) in ctx.file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticOnDemand && !imp.path.is_empty() {
            let owner_fqn = imp.path.join(".");
            collect_owner_methods(
                ctx,
                &owner_fqn,
                name,
                arity,
                false,
                true,
                false,
                &Substitution::default(),
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
    let owner_scope = type_param_bounds_of_fqn(ctx.db, owner_fqn);

    if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, owner_fqn) {
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

    if let Some(bin) = ctx.classpath.type_by_fqn(owner_fqn) {
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
                ctx.classpath,
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

#[expect(
    clippy::too_many_arguments,
    reason = "collects candidate methods across source and binary owners with call-site context"
)]
fn collect_owner_methods(
    ctx: ResolveCtx<'_>,
    owner_fqn: &str,
    name: &str,
    arity: usize,
    is_super: bool,
    only_static: bool,
    is_raw_receiver: bool,
    owner_subst: &Substitution,
    out: &mut Vec<CallableCandidate>,
) {
    let owner_scope = type_param_bounds_of_fqn(ctx.db, owner_fqn);

    if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.index, owner_fqn) {
        let tree = ctx.db.file_item_tree(fid);
        let ty_item = &tree.types[tid];
        let exclude_static =
            is_super && matches!(ty_item.kind, TypeKind::Interface | TypeKind::Annotation);
        let is_record = ty_item.kind == TypeKind::Record;

        for &mid in &ty_item.members {
            let m = &tree.members[mid];
            if m.kind == MemberKind::RecordComponent {
                if is_record && !only_static && arity == 0 && m.name == name {
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
                        is_raw_receiver,
                        method_type_params: Vec::new(),
                        method_type_param_bounds: BTreeMap::new(),
                        param_tys: Vec::new(),
                        return_ty,
                        owner_subst: owner_subst.clone(),
                    });
                }
                continue;
            }
            if m.kind != MemberKind::Method
                || m.name != name
                || (only_static && !m.is_static)
                || (exclude_static && m.is_static)
                || !arity_compatible(&m.params, arity)
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
                is_raw_receiver: !m.is_static && is_raw_receiver,
                method_type_params: m.type_params.clone(),
                method_type_param_bounds: m.type_param_bounds.clone(),
                param_tys,
                return_ty,
                owner_subst: owner_subst.clone(),
            });
        }
        return;
    }

    if let Some(bin) = ctx.classpath.type_by_fqn(owner_fqn) {
        let exclude_static =
            is_super && matches!(bin.kind, TypeKind::Interface | TypeKind::Annotation);
        let is_record = bin.kind == TypeKind::Record;

        for (member_index, m) in bin.members.iter().enumerate() {
            if m.kind == MemberKind::RecordComponent {
                if is_record && !only_static && arity == 0 && m.name == name {
                    let return_ty = m.type_ref.as_deref().map_or(Ty::Unknown, |raw| {
                        resolve_binary_type_ref_for_member(
                            ctx.classpath,
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
                        is_raw_receiver,
                        method_type_params: Vec::new(),
                        method_type_param_bounds: BTreeMap::new(),
                        param_tys: Vec::new(),
                        return_ty,
                        owner_subst: owner_subst.clone(),
                    });
                }
                continue;
            }
            if m.kind != MemberKind::Method
                || m.name != name
                || (only_static && !m.is_static)
                || (exclude_static && m.is_static)
                || !arity_compatible(&m.params, arity)
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
                ctx.classpath,
                bin,
                m.nullness_scope,
                &m.params,
                &scope,
            );
            let return_ty = m.type_ref.as_deref().map_or(Ty::Unknown, |raw| {
                resolve_binary_type_ref_for_member(
                    ctx.classpath,
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
                is_raw_receiver: !m.is_static && is_raw_receiver,
                method_type_params: m.type_params.clone(),
                method_type_param_bounds: m.type_param_bounds.clone(),
                param_tys,
                return_ty,
                owner_subst: owner_subst.clone(),
            });
        }
    }
}

pub(crate) fn select_best_callable_candidate(
    db: &dyn DefDatabase,
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
            let phase = score_candidate_phase(db, &cand, arg_tys, arg_nodes);
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
        if is_candidate_strictly_more_specific(db, &cand, &best, arg_tys.len()) {
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

pub(crate) fn score_candidate_phase(
    db: &dyn DefDatabase,
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
            match classify_arg_compatibility(db, arg_ty, arg_node, &p_ty, &m_tvars) {
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
                classify_arg_compatibility(db, arg_ty, arg_node, &p_ty, &m_tvars),
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
    arg_ty: &Ty,
    arg_node: Option<&SyntaxNode>,
    param_ty: &Ty,
    method_tvars: &FxHashSet<SmolStr>,
) -> ArgCompat {
    if let Some(node) = arg_node {
        match node.kind() {
            LAMBDA_EXPR => {
                let Some(sam) = find_functional_interface_sam(db, param_ty) else {
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
                return if find_functional_interface_sam(db, param_ty).is_some() {
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
        return classify_generic_param_compat(db, arg_ty, param_ty, method_tvars);
    }

    if is_subtype(db, arg_ty, param_ty) {
        ArgCompat::Strict
    } else if is_assignable(db, arg_ty, param_ty) {
        ArgCompat::Loose
    } else {
        ArgCompat::IncompatibleOrUnknown
    }
}

fn classify_generic_param_compat(
    db: &dyn DefDatabase,
    arg_ty: &Ty,
    param_ty: &Ty,
    method_tvars: &FxHashSet<SmolStr>,
) -> ArgCompat {
    match param_ty {
        Ty::TypeVar { name, bounds, .. } if method_tvars.contains(name) => {
            let satisfies_bounds = |ref_ty: &Ty| -> bool {
                bounds.iter().all(|b| {
                    contains_any_tvar(b, method_tvars) || is_subtype(db, ref_ty, b)
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
                classify_generic_param_compat(db, a_elem, p_elem, method_tvars)
            }
            _ => ArgCompat::IncompatibleOrUnknown,
        },
        Ty::Class { fqn, .. } => {
            let raw_target = Ty::raw_class(fqn);
            if is_subtype(db, arg_ty, &raw_target) {
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

pub(crate) fn is_candidate_strictly_more_specific(
    db: &dyn DefDatabase,
    a: &CallableCandidate,
    b: &CallableCandidate,
    arity: usize,
) -> bool {
    more_specific_or_eq(db, a, b, arity) && !more_specific_or_eq(db, b, a, arity)
}

fn more_specific_or_eq(
    db: &dyn DefDatabase,
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
        if pa != pb && !is_subtype(db, &pa, &pb) {
            return false;
        }
    }
    true
}
