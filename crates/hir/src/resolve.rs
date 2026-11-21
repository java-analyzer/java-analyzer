//! Pure, IO-free Java name resolution at a source offset.
//! See `docs/specs/0005-symbol-indexing-and-name-resolution.md`.

use smol_str::SmolStr;
use syntax::{
    ext::{
        collapsed_text, has_direct_token, has_modifier, is_type_node, path_segments,
        type_text_with_dims,
    },
    syntax_kind::SyntaxKind::*,
    SyntaxNode, TextRange, TextSize,
};

use crate::{
    symbols::{
        extract_field_decl, extract_local_type_symbol, extract_method_or_ctor, node_ident_text,
        FileSymbols, ImportKind, MemberKind, MemberSymbol, TypeKind, TypeSymbol, WorkspaceIndex,
    },
    DefDatabase, FileId,
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
    pub name: SmolStr,
    pub qualified_name: String,
    pub detail: Option<SymbolDetail>,
    pub kind: ResolvedSymbolKind,
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
struct LocalSymbol {
    file_id: FileId,
    name: SmolStr,
    kind: LocalKind,
    type_ref: Option<String>,
    full_range: TextRange,
    selection_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeParamSymbol {
    file_id: FileId,
    name: SmolStr,
    bounds: Vec<String>,
    full_range: TextRange,
    selection_range: TextRange,
}

#[derive(Debug, Clone)]
enum ResolvedTarget {
    Type(TypeSymbol),
    Member(MemberSymbol),
    Local(LocalSymbol),
    TypeParam(TypeParamSymbol),
    Package(String),
}

impl ResolvedTarget {
    fn into_resolved_symbol(self) -> ResolvedSymbol {
        match self {
            Self::Type(t) => {
                let detail =
                    (!t.type_params.is_empty()).then_some(SymbolDetail::TypeParams(t.type_params));
                ResolvedSymbol {
                    file_id: Some(t.file_id),
                    full_range: Some(t.full_range),
                    focus_range: Some(t.selection_range),
                    name: t.name,
                    qualified_name: t.qualified_name,
                    detail,
                    kind: t.kind.into(),
                }
            }
            Self::Member(m) => {
                let qualified_name = match &m.owner_type {
                    Some(owner) => format!("{owner}.{}", m.name),
                    None => m.name.to_string(),
                };
                let detail = match m.kind {
                    MemberKind::Field | MemberKind::RecordComponent => {
                        m.type_ref.clone().map(SymbolDetail::TypeRef)
                    }
                    MemberKind::Method => Some(SymbolDetail::Method {
                        return_type: m.type_ref.clone(),
                        params: m.params_signature(),
                    }),
                    MemberKind::Constructor => Some(SymbolDetail::Constructor {
                        params: m.params_signature(),
                    }),
                    MemberKind::EnumConstant => None,
                };
                ResolvedSymbol {
                    file_id: Some(m.file_id),
                    full_range: Some(m.full_range),
                    focus_range: Some(m.selection_range),
                    name: m.name,
                    qualified_name,
                    detail,
                    kind: m.kind.into(),
                }
            }
            Self::Local(l) => {
                let qualified_name = l.name.to_string();
                ResolvedSymbol {
                    file_id: Some(l.file_id),
                    full_range: Some(l.full_range),
                    focus_range: Some(l.selection_range),
                    name: l.name,
                    qualified_name,
                    detail: l.type_ref.map(SymbolDetail::TypeRef),
                    kind: l.kind.into(),
                }
            }
            Self::TypeParam(tp) => {
                let detail = (!tp.bounds.is_empty()).then_some(SymbolDetail::Bounds(tp.bounds));
                let qualified_name = tp.name.to_string();
                ResolvedSymbol {
                    file_id: Some(tp.file_id),
                    full_range: Some(tp.full_range),
                    focus_range: Some(tp.selection_range),
                    name: tp.name,
                    qualified_name,
                    detail,
                    kind: ResolvedSymbolKind::TypeParam,
                }
            }
            Self::Package(name) => {
                let short = SmolStr::new(name.rsplit('.').next().unwrap_or(&name));
                ResolvedSymbol {
                    file_id: None,
                    full_range: None,
                    focus_range: None,
                    name: short,
                    qualified_name: name,
                    detail: None,
                    kind: ResolvedSymbolKind::Package,
                }
            }
        }
    }
}

/// Resolves the Java identifier at byte `offset` in `file_id`, if any.
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

    let token = root.token_at_offset(offset_size).right_biased()?;
    if token.kind() != IDENT {
        return None;
    }

    let parent = token.parent()?;
    let file_syms = db.file_symbols(file_id);
    let index = db.workspace_index();

    let target = match parent.kind() {
        NAME => resolve_declaration_site(db, file_id, &file_syms, &index, &parent)?,
        NAME_REF => resolve_reference_site(db, file_id, &file_syms, &index, &parent)?,
        _ => return None,
    };

    Some(target.into_resolved_symbol())
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

fn resolve_declaration_site(
    _db: &dyn DefDatabase,
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    name_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let decl = name_node.parent()?;
    let range = name_node.text_range();

    match decl.kind() {
        k if TypeKind::from_syntax(k).is_some() => {
            if let Some(ty) = file_syms.types.iter().find(|t| t.selection_range == range) {
                return Some(ResolvedTarget::Type(ty.clone()));
            }
            extract_local_type_symbol(file_id, &decl).map(ResolvedTarget::Type)
        }
        VARIABLE_DECLARATOR => {
            let decl_parent = decl.parent()?;
            match decl_parent.kind() {
                FIELD_DECL => {
                    resolve_member_declaration(file_id, file_syms, &decl, name_node, range)
                        .map(ResolvedTarget::Member)
                }
                LOCAL_VAR_DECL => {
                    let base_type = decl_parent.children().find(|n| is_type_node(n.kind()));
                    Some(ResolvedTarget::Local(local_symbol_with_type(
                        file_id,
                        &decl,
                        name_node,
                        LocalKind::LocalVar,
                        base_type.as_ref(),
                        true,
                    )))
                }
                _ => None,
            }
        }
        METHOD_DECL | CONSTRUCTOR_DECL | ENUM_CONSTANT | RECORD_COMPONENT => {
            resolve_member_declaration(file_id, file_syms, &decl, name_node, range)
                .map(ResolvedTarget::Member)
        }
        PARAM => {
            let kind = if decl
                .parent()
                .is_some_and(|p| p.kind() == LAMBDA_PARAMS)
            {
                LocalKind::LambdaParam
            } else {
                LocalKind::Param
            };
            Some(ResolvedTarget::Local(local_symbol_from_node(
                file_id,
                &decl,
                name_node,
                kind,
                true,
            )))
        }
        FOREACH_STMT => Some(ResolvedTarget::Local(local_symbol_from_node(
            file_id,
            &decl,
            name_node,
            LocalKind::ForEachVar,
            false,
        ))),
        CATCH_CLAUSE => Some(ResolvedTarget::Local(local_symbol_from_node(
            file_id,
            &decl,
            name_node,
            LocalKind::CatchParam,
            false,
        ))),
        RESOURCE => Some(ResolvedTarget::Local(local_symbol_from_node(
            file_id,
            &decl,
            name_node,
            LocalKind::ResourceVar,
            false,
        ))),
        TYPE_PATTERN => Some(ResolvedTarget::Local(local_symbol_from_node(
            file_id,
            &decl,
            name_node,
            LocalKind::PatternVar,
            false,
        ))),
        TYPE_PARAM => {
            let bounds = decl
                .children()
                .filter(|n| is_type_node(n.kind()))
                .map(|t| collapsed_text(&t))
                .collect();
            Some(ResolvedTarget::TypeParam(TypeParamSymbol {
                file_id,
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
            let ResolvedTarget::Type(anno_ty) =
                resolve_type_segments(file_id, file_syms, index, &anno, &seg_refs)?
            else {
                return None;
            };
            let target_name = node_ident_text(name_node);
            anno_ty
                .members
                .into_iter()
                .find(|m| m.kind == MemberKind::Method && m.name == target_name)
                .map(ResolvedTarget::Member)
        }
        _ => None,
    }
}

fn resolve_member_declaration(
    file_id: FileId,
    file_syms: &FileSymbols,
    decl: &SyntaxNode,
    name_node: &SyntaxNode,
    range: TextRange,
) -> Option<MemberSymbol> {
    if let Some(m) = find_member_by_selection(file_syms, range) {
        return Some(m);
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
            .find(|m| m.selection_range == range);
    }

    // Anonymous class body (`NEW_EXPR` or `ENUM_CONSTANT` -> `CLASS_BODY`).
    match member_node.kind() {
        FIELD_DECL => {
            let mut out = Vec::new();
            extract_field_decl(file_id, &member_node, None, None, &mut out);
            out.into_iter().find(|m| m.selection_range == range)
        }
        METHOD_DECL => extract_method_or_ctor(file_id, &member_node, None, MemberKind::Method),
        CONSTRUCTOR_DECL => {
            extract_method_or_ctor(file_id, &member_node, None, MemberKind::Constructor)
        }
        ENUM_CONSTANT => Some(MemberSymbol {
            file_id,
            owner_type: None,
            name: node_ident_text(name_node),
            kind: MemberKind::EnumConstant,
            is_static: true,
            type_ref: None,
            params: Vec::new(),
            full_range: member_node.text_range(),
            selection_range: range,
        }),
        _ => None,
    }
}

fn find_member_by_selection(file_syms: &FileSymbols, range: TextRange) -> Option<MemberSymbol> {
    for ty in &file_syms.types {
        if let Some(m) = ty.members.iter().find(|m| m.selection_range == range) {
            return Some(m.clone());
        }
    }
    file_syms
        .top_level_members
        .iter()
        .find(|m| m.selection_range == range)
        .cloned()
}

fn resolve_reference_site(
    db: &dyn DefDatabase,
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let ref_parent = ref_node.parent()?;

    match ref_parent.kind() {
        PATH => resolve_path_ref(file_id, file_syms, index, &ref_parent, ref_node),
        PATH_TYPE => resolve_path_type_ref(db, file_id, file_syms, index, &ref_parent, ref_node),
        FIELD_ACCESS_EXPR => {
            resolve_field_access_ref(db, file_id, file_syms, index, &ref_parent, ref_node)
        }
        METHOD_CALL_EXPR => {
            resolve_method_call_ref(db, file_id, file_syms, index, &ref_parent, ref_node)
        }
        METHOD_REF_EXPR => {
            resolve_method_ref(db, file_id, file_syms, index, &ref_parent, ref_node)
        }
        NAME_REF_EXPR => resolve_name_ref_expr(file_id, file_syms, index, &ref_parent, ref_node),
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
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
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
            let import_decl = file_syms
                .imports
                .iter()
                .find(|imp| imp.range == owner.text_range())?;
            if import_decl.kind == ImportKind::Module {
                return None;
            }

            let all_segs = path_segments(path_node);
            let is_last = prefix.len() == all_segs.len();

            if import_decl.kind == ImportKind::StaticSingle && is_last && all_segs.len() >= 2 {
                let type_fqn = all_segs[..all_segs.len() - 1].join(".");
                let member_name = all_segs.last()?;
                let owner_ty = lookup_type_by_fqn(file_syms, index, &type_fqn)?;
                return lookup_static_member_or_nested(
                    file_syms,
                    index,
                    owner_ty,
                    member_name,
                    None,
                );
            }

            let prefix_fqn = prefix.join(".");
            if let Some(ty) = lookup_type_by_fqn(file_syms, index, &prefix_fqn) {
                return Some(ResolvedTarget::Type(ty.clone()));
            }
            if index.has_package_prefix(&prefix_fqn) {
                return Some(ResolvedTarget::Package(prefix_fqn));
            }
            None
        }
        ANNOTATION => {
            let seg_refs: Vec<&str> = prefix.iter().map(SmolStr::as_str).collect();
            resolve_type_segments(file_id, file_syms, index, path_node, &seg_refs)
        }
        _ => None,
    }
}

fn resolve_path_type_ref(
    db: &dyn DefDatabase,
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    path_type: &SyntaxNode,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let prefix = prefix_segments_up_to(path_type, ref_node);
    if prefix.is_empty() {
        return None;
    }

    // Qualified `outer.new Inner()` resolves `Inner` relative to `outer`'s type.
    if let Some(new_expr) = path_type.parent().filter(|p| p.kind() == NEW_EXPR) {
        if has_direct_token(&new_expr, DOT) {
            if let Some(outer_expr) = new_expr.children().next() {
                let outer_ty = resolve_expr_type(db, file_id, file_syms, index, &outer_expr, 0)?;
                let mut current: &TypeSymbol = &outer_ty;
                for seg in &prefix {
                    let next_fqn = format!("{}.{}", current.qualified_name, seg);
                    current = lookup_type_by_fqn(file_syms, index, &next_fqn)?;
                }
                return Some(ResolvedTarget::Type(current.clone()));
            }
        }
    }

    let seg_refs: Vec<&str> = prefix.iter().map(SmolStr::as_str).collect();
    resolve_type_segments(file_id, file_syms, index, path_type, &seg_refs)
}

fn resolve_field_access_ref(
    db: &dyn DefDatabase,
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    field_access: &SyntaxNode,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    // 1. Qualified type path check (e.g. `com.example.Foo` or `com.example.Outer.Inner`).
    if let Some(chain) = extract_dotted_name_chain(field_access) {
        let fqn = chain.join(".");
        if let Some(ty) = lookup_type_by_fqn(file_syms, index, &fqn) {
            return Some(ResolvedTarget::Type(ty.clone()));
        }
    }

    // 2. Resolve receiver expression's type and look up member/nested type on it.
    let receiver = field_access.children().next()?;
    let recv_ty = resolve_expr_type(db, file_id, file_syms, index, &receiver, 0)?;
    let member_name = node_ident_text(ref_node);
    lookup_member_on_type(file_syms, index, &recv_ty, &member_name, None, false)
}

fn resolve_method_call_ref(
    db: &dyn DefDatabase,
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    method_call: &SyntaxNode,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let receiver = method_call.children().next()?;
    let recv_ty = resolve_expr_type(db, file_id, file_syms, index, &receiver, 0)?;
    let member_name = node_ident_text(ref_node);
    let arg_count = method_call
        .children()
        .find(|n| n.kind() == ARG_LIST)
        .map(|al| count_args(&al));
    lookup_member_on_type(file_syms, index, &recv_ty, &member_name, arg_count, true)
}

fn resolve_method_ref(
    db: &dyn DefDatabase,
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    method_ref: &SyntaxNode,
    ref_node: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let qualifier = method_ref.children().next()?;
    let target_ty = if qualifier.kind() == PATH_TYPE {
        let segs = path_segments(&qualifier);
        let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
        match resolve_type_segments(file_id, file_syms, index, &qualifier, &seg_refs)? {
            ResolvedTarget::Type(ty) => ty,
            _ => return None,
        }
    } else {
        resolve_expr_type(db, file_id, file_syms, index, &qualifier, 0)?
    };

    let member_name = node_ident_text(ref_node);
    lookup_method_or_member(&target_ty, &member_name, file_syms, index)
}

fn resolve_name_ref_expr(
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
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
        let arg_count = call_expr
            .children()
            .find(|n| n.kind() == ARG_LIST)
            .map(|al| count_args(&al));

        // 1. Enclosing types' methods (innermost to outermost).
        for ty in enclosing_type_symbols(file_id, file_syms, expr_node) {
            if let Some(m) = select_method(
                &ty.members,
                &name,
                arg_count,
                ty.kind == TypeKind::Record,
                false,
            ) {
                return Some(ResolvedTarget::Member(m.clone()));
            }
        }
        // Compact-source top-level methods.
        if let Some(m) = select_method(&file_syms.top_level_members, &name, arg_count, false, false)
        {
            return Some(ResolvedTarget::Member(m.clone()));
        }

        // 2. Static imports (Single first, then OnDemand).
        if let Some(m) = resolve_via_static_imports(file_syms, index, &name, arg_count, true) {
            return Some(m);
        }
        return None;
    }

    // Non-call unqualified identifier:
    // 1. Lexical local scopes (innermost to outermost).
    if let Some(local) = resolve_local_scope(file_id, ref_node, &name) {
        return Some(ResolvedTarget::Local(local));
    }

    // 2. Enclosing type(s) members (fields, record components, enum constants, nested types)
    //    plus compact-source top-level fields.
    for ty in enclosing_type_symbols(file_id, file_syms, expr_node) {
        if let Some(found) = lookup_non_method_member_or_nested(file_syms, index, &ty, &name) {
            return Some(found);
        }
    }
    if let Some(m) = file_syms
        .top_level_members
        .iter()
        .find(|m| m.kind != MemberKind::Method && m.name == name)
    {
        return Some(ResolvedTarget::Member(m.clone()));
    }

    // 3. Static imports (fields, enum constants, nested types).
    if let Some(found) = resolve_via_static_imports(file_syms, index, &name, None, false) {
        return Some(found);
    }

    // 4. Type & import resolution (allows `Foo` in `Foo.BAR` or `Foo::bar` to resolve to `Foo`).
    resolve_type_segments(file_id, file_syms, index, expr_node, &[&name])
}

// ---- Lexical local scope resolution ----

fn resolve_local_scope(
    file_id: FileId,
    ref_node: &SyntaxNode,
    name: &str,
) -> Option<LocalSymbol> {
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
                if after_paren.is_some_and(|end| ref_range.start() >= end) {
                    if let Some(name_node) = ancestor.children().find(|n| n.kind() == NAME) {
                        if name_node.text() == name {
                            return Some(local_symbol_from_node(
                                file_id,
                                &ancestor,
                                &name_node,
                                LocalKind::ForEachVar,
                                false,
                            ));
                        }
                    }
                }
            }
            CATCH_CLAUSE if child_on_path.kind() == BLOCK => {
                if let Some(name_node) = ancestor.children().find(|n| n.kind() == NAME) {
                    if name_node.text() == name {
                        return Some(local_symbol_from_node(
                            file_id,
                            &ancestor,
                            &name_node,
                            LocalKind::CatchParam,
                            false,
                        ));
                    }
                }
            }
            TRY_STMT => {
                let in_try_scope = child_on_path.kind() == RESOURCE_LIST
                    || ancestor
                        .children()
                        .find(|n| n.kind() == BLOCK)
                        .is_some_and(|b| b.text_range() == child_on_path.text_range());
                if in_try_scope {
                    if let Some(res_list) = ancestor.children().find(|n| n.kind() == RESOURCE_LIST)
                    {
                        let resources: Vec<SyntaxNode> = res_list
                            .children()
                            .filter(|n| n.kind() == RESOURCE)
                            .collect();
                        for res in resources.into_iter().rev() {
                            if res.text_range().end() <= ref_range.start() {
                                if let Some(name_node) = res.children().find(|n| n.kind() == NAME) {
                                    if name_node.text() == name {
                                        return Some(local_symbol_from_node(
                                            file_id,
                                            &res,
                                            &name_node,
                                            LocalKind::ResourceVar,
                                            false,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            LAMBDA_EXPR => {
                if let Some(params_node) = ancestor.children().find(|n| n.kind() == LAMBDA_PARAMS)
                {
                    if ref_range.start() >= params_node.text_range().end() {
                        let params: Vec<SyntaxNode> = params_node
                            .children()
                            .filter(|n| n.kind() == PARAM)
                            .collect();
                        for param in params.into_iter().rev() {
                            if let Some(name_node) = param.children().find(|n| n.kind() == NAME) {
                                if name_node.text() == name {
                                    return Some(local_symbol_from_node(
                                        file_id,
                                        &param,
                                        &name_node,
                                        LocalKind::LambdaParam,
                                        true,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            BINARY_EXPR => {
                if let Some(amp2) = ancestor
                    .children_with_tokens()
                    .find(|e| e.as_token().is_some_and(|t| t.kind() == AMP2))
                {
                    if ref_range.start() >= amp2.text_range().end() {
                        if let Some(lhs) = ancestor.children().next() {
                            let mut patterns = Vec::new();
                            collect_positive_patterns(file_id, &lhs, &mut patterns);
                            if let Some(found) = patterns.into_iter().rev().find(|p| p.name == name)
                            {
                                return Some(found);
                            }
                        }
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
                if in_then {
                    if let Some(cond) = ancestor
                        .children()
                        .find(|c| r_paren_end.is_none_or(|end| c.text_range().end() <= end))
                    {
                        let mut patterns = Vec::new();
                        collect_positive_patterns(file_id, &cond, &mut patterns);
                        if let Some(found) = patterns.into_iter().rev().find(|p| p.name == name) {
                            return Some(found);
                        }
                    }
                }
            }
            SWITCH_CASE => {
                if let Some(label) = ancestor.children().find(|n| n.kind() == SWITCH_LABEL) {
                    let mut patterns = Vec::new();
                    for desc in label.descendants() {
                        if desc.kind() == TYPE_PATTERN && desc.text_range().end() <= ref_range.start()
                        {
                            collect_type_pattern(file_id, &desc, &mut patterns);
                        }
                    }
                    if let Some(found) = patterns.into_iter().rev().find(|p| p.name == name) {
                        return Some(found);
                    }
                }
            }
            METHOD_DECL | CONSTRUCTOR_DECL => {
                if let Some(param_list) = ancestor.children().find(|n| n.kind() == PARAM_LIST) {
                    if ref_range.start() >= param_list.text_range().end() {
                        let params: Vec<SyntaxNode> = param_list
                            .children()
                            .filter(|n| n.kind() == PARAM)
                            .collect();
                        for param in params.into_iter().rev() {
                            if let Some(name_node) = param.children().find(|n| n.kind() == NAME) {
                                if name_node.text() == name {
                                    return Some(local_symbol_from_node(
                                        file_id,
                                        &param,
                                        &name_node,
                                        LocalKind::Param,
                                        true,
                                    ));
                                }
                            }
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

fn find_local_var_in_block(
    file_id: FileId,
    block: &SyntaxNode,
    ref_range: TextRange,
    name: &str,
) -> Option<LocalSymbol> {
    let decls: Vec<SyntaxNode> = block
        .children()
        .filter(|n| n.kind() == LOCAL_VAR_DECL)
        .collect();
    for decl in decls.into_iter().rev() {
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
) -> Option<LocalSymbol> {
    let base_type = local_var_decl.children().find(|n| is_type_node(n.kind()));
    let declarators: Vec<SyntaxNode> = local_var_decl
        .children()
        .filter(|n| n.kind() == VARIABLE_DECLARATOR)
        .collect();
    for declarator in declarators.into_iter().rev() {
        if declarator.text_range().end() > ref_range.start() {
            continue;
        }
        if let Some(name_node) = declarator.children().find(|n| n.kind() == NAME) {
            if name_node.text() == name {
                return Some(local_symbol_with_type(
                    file_id,
                    &declarator,
                    &name_node,
                    LocalKind::LocalVar,
                    base_type.as_ref(),
                    true,
                ));
            }
        }
    }
    None
}

fn collect_positive_patterns(file_id: FileId, node: &SyntaxNode, out: &mut Vec<LocalSymbol>) {
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

fn collect_record_pattern(file_id: FileId, node: &SyntaxNode, out: &mut Vec<LocalSymbol>) {
    for child in node.children() {
        match child.kind() {
            TYPE_PATTERN => collect_type_pattern(file_id, &child, out),
            RECORD_PATTERN => collect_record_pattern(file_id, &child, out),
            _ => {}
        }
    }
}

fn collect_type_pattern(file_id: FileId, node: &SyntaxNode, out: &mut Vec<LocalSymbol>) {
    if let Some(name_node) = node.children().find(|n| n.kind() == NAME) {
        out.push(local_symbol_from_node(
            file_id,
            node,
            &name_node,
            LocalKind::PatternVar,
            false,
        ));
    }
}

// ---- Type & import resolution ----

fn resolve_type_segments(
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    site_node: &SyntaxNode,
    segs: &[&str],
) -> Option<ResolvedTarget> {
    if segs.is_empty() {
        return None;
    }

    if segs.len() == 1 {
        let name = segs[0];

        // 1-3. Walk enclosing scopes innermost-to-outermost (local types, method/type params,
        // enclosing types and their nested types), stopping type parameter propagation across
        // `static` boundaries.
        if let Some(found) =
            resolve_enclosing_type_or_param(file_id, file_syms, index, site_node, name)
        {
            return Some(found);
        }

        // 4. Single-type imports.
        for imp in &file_syms.imports {
            if imp.kind == ImportKind::SingleType
                && imp.path.last().map(String::as_str) == Some(name)
            {
                let fqn = imp.path.join(".");
                if let Some(ty) = lookup_type_by_fqn(file_syms, index, &fqn) {
                    return Some(ResolvedTarget::Type(ty.clone()));
                }
            }
        }

        // 5. Static single imports resolving to a nested type.
        for imp in &file_syms.imports {
            if imp.kind == ImportKind::StaticSingle
                && imp.path.last().map(String::as_str) == Some(name)
            {
                let fqn = imp.path.join(".");
                if let Some(ty) = lookup_type_by_fqn(file_syms, index, &fqn) {
                    return Some(ResolvedTarget::Type(ty.clone()));
                }
            }
        }

        // 6. Same-package top-level types.
        let pkg = file_syms.package.as_deref().unwrap_or("");
        if let Some(fqn) = index.package_type_map.get(pkg).and_then(|m| m.get(name)) {
            if let Some(ty) = index.types_by_fqn.get(fqn) {
                return Some(ResolvedTarget::Type(ty.clone()));
            }
        }
        if let Some(ty) = file_syms
            .types
            .iter()
            .find(|t| t.enclosing_type.is_none() && t.name == name)
        {
            return Some(ResolvedTarget::Type(ty.clone()));
        }

        // 7. Type-import-on-demand (`import a.b.*;`).
        for imp in &file_syms.imports {
            if imp.kind == ImportKind::TypeOnDemand {
                let fqn = format!("{}.{name}", imp.path.join("."));
                if let Some(ty) = lookup_type_by_fqn(file_syms, index, &fqn) {
                    return Some(ResolvedTarget::Type(ty.clone()));
                }
            }
        }

        // 8. Static-import-on-demand (`import static a.b.Outer.*;`).
        for imp in &file_syms.imports {
            if imp.kind == ImportKind::StaticOnDemand {
                let fqn = format!("{}.{name}", imp.path.join("."));
                if let Some(ty) = lookup_type_by_fqn(file_syms, index, &fqn) {
                    return Some(ResolvedTarget::Type(ty.clone()));
                }
            }
        }

        return None;
    }

    // Multi-segment `[s0, s1, ..., sk]`:
    // 1. Direct FQN match (`com.example.Foo`, `com.example.Outer.Inner`).
    let joined = segs.join(".");
    if let Some(ty) = lookup_type_by_fqn(file_syms, index, &joined) {
        return Some(ResolvedTarget::Type(ty.clone()));
    }

    // 2. Resolve `s0` as a single-segment type, then walk nested types `s1..=sk`.
    let ResolvedTarget::Type(first) =
        resolve_type_segments(file_id, file_syms, index, site_node, &segs[..1])?
    else {
        return None;
    };
    let mut current: &TypeSymbol = &first;
    for seg in &segs[1..] {
        let next_fqn = format!("{}.{}", current.qualified_name, seg);
        current = lookup_type_by_fqn(file_syms, index, &next_fqn)?;
    }
    Some(ResolvedTarget::Type(current.clone()))
}

fn resolve_enclosing_type_or_param(
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    site_node: &SyntaxNode,
    name: &str,
) -> Option<ResolvedTarget> {
    let site_start = site_node.text_range().start();
    let mut allow_type_params = true;

    for ancestor in site_node.ancestors() {
        match ancestor.kind() {
            BLOCK => {
                let locals: Vec<SyntaxNode> = ancestor
                    .children()
                    .filter(|n| TypeKind::from_syntax(n.kind()).is_some())
                    .collect();
                for ty_node in locals.into_iter().rev() {
                    if let Some(name_node) = ty_node.children().find(|n| n.kind() == NAME) {
                        if name_node.text() == name && name_node.text_range().end() <= site_start {
                            if let Some(local_ty) = extract_local_type_symbol(file_id, &ty_node) {
                                return Some(ResolvedTarget::Type(local_ty));
                            }
                        }
                    }
                }
            }
            METHOD_DECL | CONSTRUCTOR_DECL => {
                if allow_type_params {
                    if let Some(tp) = find_type_param_on_node(file_id, &ancestor, name) {
                        return Some(ResolvedTarget::TypeParam(tp));
                    }
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
                if allow_type_params {
                    if let Some(tp) = find_type_param_on_node(file_id, &ancestor, name) {
                        return Some(ResolvedTarget::TypeParam(tp));
                    }
                }

                let range = ancestor.text_range();
                let local_ty;
                let ty = if let Some(ty) = file_syms.types.iter().find(|t| t.full_range == range) {
                    Some(ty)
                } else {
                    local_ty = extract_local_type_symbol(file_id, &ancestor);
                    local_ty.as_ref()
                };

                let mut is_static_boundary = has_modifier(&ancestor, STATIC_KW);
                if let Some(ty) = ty {
                    is_static_boundary |= ty.is_static
                        || matches!(
                            ty.kind,
                            TypeKind::Interface
                                | TypeKind::Enum
                                | TypeKind::Record
                                | TypeKind::Annotation
                        );
                    if ty.name == name {
                        return Some(ResolvedTarget::Type(ty.clone()));
                    }
                    if ty.nested_types.iter().any(|n| n == name) {
                        let nested_fqn = format!("{}.{}", ty.qualified_name, name);
                        if let Some(nested) = lookup_type_by_fqn(file_syms, index, &nested_fqn) {
                            return Some(ResolvedTarget::Type(nested.clone()));
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
        if let Some(name_node) = tp.children().find(|n| n.kind() == NAME) {
            if name_node.text() == name {
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
    }
    None
}

fn enclosing_type_symbols(
    file_id: FileId,
    file_syms: &FileSymbols,
    node: &SyntaxNode,
) -> Vec<TypeSymbol> {
    let mut out = Vec::new();
    for ancestor in node.ancestors() {
        if TypeKind::from_syntax(ancestor.kind()).is_some() {
            let range = ancestor.text_range();
            if let Some(ty) = file_syms.types.iter().find(|t| t.full_range == range) {
                out.push(ty.clone());
            } else if let Some(local_ty) = extract_local_type_symbol(file_id, &ancestor) {
                out.push(local_ty);
            }
        }
    }
    out
}

fn lookup_type_by_fqn<'a>(
    file_syms: &'a FileSymbols,
    index: &'a WorkspaceIndex,
    fqn: &str,
) -> Option<&'a TypeSymbol> {
    index
        .types_by_fqn
        .get(fqn)
        .or_else(|| file_syms.types.iter().find(|t| t.qualified_name == fqn))
}

// ---- Member lookup & static imports ----

fn resolve_via_static_imports(
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    name: &str,
    call_arity: Option<usize>,
    prefer_callable: bool,
) -> Option<ResolvedTarget> {
    // 1. Static single imports (`import static a.b.Foo.BAR;`).
    for imp in &file_syms.imports {
        if imp.kind == ImportKind::StaticSingle
            && imp.path.len() >= 2
            && imp.path.last().map(String::as_str) == Some(name)
        {
            let owner_fqn = imp.path[..imp.path.len() - 1].join(".");
            if let Some(owner_ty) = lookup_type_by_fqn(file_syms, index, &owner_fqn) {
                if let Some(found) = lookup_static_member_or_nested(
                    file_syms,
                    index,
                    owner_ty,
                    name,
                    if prefer_callable { Some(call_arity) } else { None },
                ) {
                    return Some(found);
                }
            }
        }
    }

    // 2. Static-import-on-demand (`import static a.b.Foo.*;`).
    for imp in &file_syms.imports {
        if imp.kind == ImportKind::StaticOnDemand && !imp.path.is_empty() {
            let owner_fqn = imp.path.join(".");
            if let Some(owner_ty) = lookup_type_by_fqn(file_syms, index, &owner_fqn) {
                if let Some(found) = lookup_static_member_or_nested(
                    file_syms,
                    index,
                    owner_ty,
                    name,
                    if prefer_callable { Some(call_arity) } else { None },
                ) {
                    return Some(found);
                }
            }
        }
    }

    None
}

fn lookup_static_member_or_nested(
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    owner_ty: &TypeSymbol,
    name: &str,
    callable_arity: Option<Option<usize>>,
) -> Option<ResolvedTarget> {
    if let Some(arity) = callable_arity {
        return select_method(&owner_ty.members, name, arity, false, true)
            .cloned()
            .map(ResolvedTarget::Member);
    }

    if let Some(m) = owner_ty
        .members
        .iter()
        .find(|m| m.is_static && m.kind != MemberKind::Method && m.name == name)
    {
        return Some(ResolvedTarget::Member(m.clone()));
    }

    if owner_ty.nested_types.iter().any(|n| n == name) {
        let nested_fqn = format!("{}.{}", owner_ty.qualified_name, name);
        if let Some(nested) = lookup_type_by_fqn(file_syms, index, &nested_fqn) {
            return Some(ResolvedTarget::Type(nested.clone()));
        }
    }

    // Fallback: static method (e.g. inside `import static a.b.Foo.bar;`).
    if let Some(m) = owner_ty
        .members
        .iter()
        .find(|m| m.is_static && m.kind == MemberKind::Method && m.name == name)
    {
        return Some(ResolvedTarget::Member(m.clone()));
    }

    None
}

fn lookup_member_on_type(
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    ty: &TypeSymbol,
    name: &str,
    call_arity: Option<usize>,
    prefer_callable: bool,
) -> Option<ResolvedTarget> {
    if prefer_callable {
        return select_method(
            &ty.members,
            name,
            call_arity,
            ty.kind == TypeKind::Record,
            false,
        )
        .cloned()
        .map(ResolvedTarget::Member);
    }

    if let Some(found) = lookup_non_method_member_or_nested(file_syms, index, ty, name) {
        return Some(found);
    }

    select_method(&ty.members, name, call_arity, false, false)
        .cloned()
        .map(ResolvedTarget::Member)
}

fn lookup_non_method_member_or_nested(
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    ty: &TypeSymbol,
    name: &str,
) -> Option<ResolvedTarget> {
    if let Some(m) = ty
        .members
        .iter()
        .find(|m| m.kind != MemberKind::Method && m.kind != MemberKind::Constructor && m.name == name)
    {
        return Some(ResolvedTarget::Member(m.clone()));
    }

    if ty.nested_types.iter().any(|n| n == name) {
        let nested_fqn = format!("{}.{}", ty.qualified_name, name);
        if let Some(nested) = lookup_type_by_fqn(file_syms, index, &nested_fqn) {
            return Some(ResolvedTarget::Type(nested.clone()));
        }
    }

    None
}

fn lookup_method_or_member(
    ty: &TypeSymbol,
    name: &str,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
) -> Option<ResolvedTarget> {
    if let Some(m) = select_method(&ty.members, name, None, ty.kind == TypeKind::Record, false) {
        return Some(ResolvedTarget::Member(m.clone()));
    }
    lookup_non_method_member_or_nested(file_syms, index, ty, name)
}

fn select_method<'a>(
    members: &'a [MemberSymbol],
    name: &str,
    call_arity: Option<usize>,
    include_record_components: bool,
    only_static: bool,
) -> Option<&'a MemberSymbol> {
    let mut first_match: Option<&'a MemberSymbol> = None;
    let mut varargs_match: Option<&'a MemberSymbol> = None;

    for m in members {
        if m.kind != MemberKind::Method || m.name != name || (only_static && !m.is_static) {
            continue;
        }
        if first_match.is_none() {
            first_match = Some(m);
        }
        if let Some(arity) = call_arity {
            let is_varargs = m.params.last().is_some_and(|p| p.is_varargs);
            // 1. Exact fixed-arity match.
            if !is_varargs && m.params.len() == arity {
                return Some(m);
            }
            // 2. Varargs arity match.
            if is_varargs && varargs_match.is_none() && arity + 1 >= m.params.len() {
                varargs_match = Some(m);
            }
        } else {
            return Some(m);
        }
    }

    // 3. Fallback to first varargs match or first matching method.
    if let Some(m) = varargs_match.or(first_match) {
        return Some(m);
    }

    if !only_static && include_record_components && call_arity.is_none_or(|a| a == 0) {
        return members
            .iter()
            .find(|m| m.kind == MemberKind::RecordComponent && m.name == name);
    }

    None
}

// ---- Receiver type resolution ----

const MAX_TYPE_DEPTH: u8 = 32;

fn resolve_expr_type(
    db: &dyn DefDatabase,
    file_id: FileId,
    file_syms: &FileSymbols,
    index: &WorkspaceIndex,
    expr: &SyntaxNode,
    depth: u8,
) -> Option<TypeSymbol> {
    if depth >= MAX_TYPE_DEPTH {
        return None;
    }

    match expr.kind() {
        THIS_EXPR => {
            if let Some(qualifier) = expr.children().next() {
                return resolve_expr_type(db, file_id, file_syms, index, &qualifier, depth + 1);
            }
            enclosing_type_symbols(file_id, file_syms, expr)
                .into_iter()
                .next()
        }
        NAME_REF_EXPR => {
            let ref_node = expr.children().find(|n| n.kind() == NAME_REF)?;
            let target = resolve_name_ref_expr(file_id, file_syms, index, expr, &ref_node)?;
            target_to_type_symbol(db, index, target, depth + 1)
        }
        FIELD_ACCESS_EXPR => {
            if let Some(chain) = extract_dotted_name_chain(expr) {
                if let Some(ty) = lookup_type_by_fqn(file_syms, index, &chain.join(".")) {
                    return Some(ty.clone());
                }
            }
            let ref_node = expr.children().find(|n| n.kind() == NAME_REF)?;
            let receiver = expr.children().next()?;
            let recv_ty = resolve_expr_type(db, file_id, file_syms, index, &receiver, depth + 1)?;
            let member_name = node_ident_text(&ref_node);
            let target = lookup_member_on_type(
                file_syms,
                index,
                &recv_ty,
                &member_name,
                None,
                false,
            )?;
            target_to_type_symbol(db, index, target, depth + 1)
        }
        METHOD_CALL_EXPR => {
            let ref_node = expr.children().find(|n| n.kind() == NAME_REF)?;
            let receiver = expr.children().next()?;
            let recv_ty = resolve_expr_type(db, file_id, file_syms, index, &receiver, depth + 1)?;
            let arg_count = expr
                .children()
                .find(|n| n.kind() == ARG_LIST)
                .map(|al| count_args(&al));
            let member_name = node_ident_text(&ref_node);
            let target = lookup_member_on_type(
                file_syms,
                index,
                &recv_ty,
                &member_name,
                arg_count,
                true,
            )?;
            target_to_type_symbol(db, index, target, depth + 1)
        }
        CALL_EXPR => {
            let callee = expr.children().next()?;
            if callee.kind() != NAME_REF_EXPR {
                return None;
            }
            let ref_node = callee.children().find(|n| n.kind() == NAME_REF)?;
            let target = resolve_name_ref_expr(file_id, file_syms, index, &callee, &ref_node)?;
            target_to_type_symbol(db, index, target, depth + 1)
        }
        NEW_EXPR => {
            let path_type = expr.children().find(|n| n.kind() == PATH_TYPE)?;
            let segs = path_segments(&path_type);
            if has_direct_token(expr, DOT) {
                let outer_expr = expr.children().next()?;
                let outer_ty =
                    resolve_expr_type(db, file_id, file_syms, index, &outer_expr, depth + 1)?;
                let mut current: &TypeSymbol = &outer_ty;
                for seg in &segs {
                    let next_fqn = format!("{}.{}", current.qualified_name, seg);
                    current = lookup_type_by_fqn(file_syms, index, &next_fqn)?;
                }
                return Some(current.clone());
            }
            let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
            match resolve_type_segments(file_id, file_syms, index, &path_type, &seg_refs)? {
                ResolvedTarget::Type(ty) => Some(ty),
                _ => None,
            }
        }
        PAREN_EXPR => {
            let inner = expr.children().next()?;
            resolve_expr_type(db, file_id, file_syms, index, &inner, depth + 1)
        }
        CAST_EXPR => {
            let path_type = expr.children().find(|n| n.kind() == PATH_TYPE)?;
            let segs = path_segments(&path_type);
            let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
            match resolve_type_segments(file_id, file_syms, index, &path_type, &seg_refs)? {
                ResolvedTarget::Type(ty) => Some(ty),
                _ => None,
            }
        }
        _ => None,
    }
}

fn target_to_type_symbol(
    db: &dyn DefDatabase,
    index: &WorkspaceIndex,
    target: ResolvedTarget,
    depth: u8,
) -> Option<TypeSymbol> {
    if depth >= MAX_TYPE_DEPTH {
        return None;
    }
    match target {
        ResolvedTarget::Type(ty) => Some(ty),
        ResolvedTarget::Local(l) => {
            let ty_str = l.type_ref.as_deref()?;
            resolve_type_ref_in_file(db, index, l.file_id, l.selection_range.start(), ty_str)
        }
        ResolvedTarget::Member(m) => match m.kind {
            MemberKind::Field | MemberKind::RecordComponent | MemberKind::Method => {
                let ty_str = m.type_ref.as_deref()?;
                resolve_type_ref_in_file(db, index, m.file_id, m.selection_range.start(), ty_str)
            }
            MemberKind::EnumConstant => {
                let owner_fqn = m.owner_type.as_deref()?;
                let decl_syms = db.file_symbols(m.file_id);
                lookup_type_by_fqn(&decl_syms, index, owner_fqn).cloned()
            }
            MemberKind::Constructor => None,
        },
        ResolvedTarget::TypeParam(_) | ResolvedTarget::Package(_) => None,
    }
}

fn resolve_type_ref_in_file(
    db: &dyn DefDatabase,
    index: &WorkspaceIndex,
    decl_file_id: FileId,
    decl_offset: TextSize,
    type_ref: &str,
) -> Option<TypeSymbol> {
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
    let decl_syms = db.file_symbols(decl_file_id);
    let seg_refs: Vec<&str> = segs.iter().map(String::as_str).collect();
    match resolve_type_segments(decl_file_id, &decl_syms, index, &site_node, &seg_refs)? {
        ResolvedTarget::Type(ty) => Some(ty),
        _ => None,
    }
}

/// Strips generics (`<...>`), leading `@Annotations(...)`, and rejects arrays / primitives / `var`.
fn parse_type_ref_segments(type_ref: &str) -> Option<Vec<String>> {
    let trimmed = type_ref.trim();
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
            '@' if angle_depth == 0 => {
                // Skip annotation name (possibly dotted) and optional `(...)`.
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
    if segs.len() == 1
        && matches!(
            segs[0].as_str(),
            "boolean" | "byte" | "short" | "int" | "long" | "char" | "float" | "double" | "void"
                | "var"
        )
    {
        return None;
    }
    Some(segs)
}

fn extract_dotted_name_chain(node: &SyntaxNode) -> Option<Vec<SmolStr>> {
    let mut out = Vec::new();
    collect_dotted_chain(node, &mut out).then_some(out)
}

fn collect_dotted_chain(node: &SyntaxNode, out: &mut Vec<SmolStr>) -> bool {
    match node.kind() {
        NAME_REF_EXPR => {
            if let Some(nr) = node.children().find(|n| n.kind() == NAME_REF) {
                out.push(node_ident_text(&nr));
                true
            } else {
                false
            }
        }
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
