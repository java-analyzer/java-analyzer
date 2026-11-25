use std::collections::BTreeMap;

use smol_str::SmolStr;
use syntax::{
    SyntaxNode,
    ext::{collapsed_text, has_direct_token, has_modifier, is_type_node, name_child},
    syntax_kind::SyntaxKind::*,
};

use crate::{
    DefDatabase, FileId,
    classpath::{BinaryTypeSymbol, ClasspathIndex},
    hierarchy::linearized_supertypes_in_scope,
    item_tree::{AstIdMap, FileItemTree},
    resolve::{ResolvedTarget, SymbolDetail},
    scope::VisibleScope,
    symbols::{
        ImportResolver, MemberKind, NullnessScope, ParamSummary, TypeKind,
        extract_modifier_nullability, extract_modifier_nullness_scope, format_params_signature,
        node_ident_text, render_type_node, render_type_with_dims_and_nullability,
    },
    ty::{
        Nullability, Substitution, Ty, TypeArg, apply_nullness_scope,
        effective_nullness_scope_for_binary_member, effective_nullness_scope_for_file,
        effective_nullness_scope_for_member_in_scope, effective_nullness_scope_for_type_in_scope,
        parse_type_ref, resolve_binary_type_ref_for_member, resolve_binary_type_ref_for_owner,
        resolve_type_ref_in_file_for_member, resolve_type_ref_in_file_for_owner,
        resolve_type_ref_in_file_with_scope_and_visible, supertype_substitution_for_ty_in_scope,
        ty_has_explicit_nullability, type_param_bounds_of_fqn_in_scope,
        type_params_of_fqn_in_scope,
    },
};

use super::{
    InferCtx, OBJECT_FQN,
    sam::{erase_type_vars_to_bound, find_functional_interface_sam_in_scope},
};

impl<'a> InferCtx<'a> {
    pub(super) fn lookup_field_ty_on_ty(
        &self,
        recv_ty: &Ty,
        field_name: &str,
        site_scope: NullnessScope,
    ) -> Option<Ty> {
        let recv_fqn = recv_ty.erased_fqn()?;
        let mut chain = Vec::with_capacity(1);
        chain.push(recv_fqn.to_string());
        chain.extend(linearized_supertypes_in_scope(self.db, &self.scope, recv_fqn));

        for owner_fqn in chain {
            if let Some((fid, tid)) = self.scope.get_workspace_type(&owner_fqn) {
                let tree = self.db.file_item_tree(fid);
                let ty_item = &tree.types[tid];
                if let Some(&mid) = ty_item.members.iter().find(|&&mid| {
                    let m = &tree.members[mid];
                    m.kind != MemberKind::Method
                        && m.kind != MemberKind::Constructor
                        && m.name == field_name
                }) {
                    let m = &tree.members[mid];
                    if m.kind == MemberKind::EnumConstant {
                        let member_scope = effective_nullness_scope_for_member_in_scope(
                            self.db,
                            &self.scope,
                            &owner_fqn,
                            m.nullness_scope,
                        );
                        let ty = Ty::raw_class(owner_fqn);
                        return Some(
                            if matches!(member_scope, NullnessScope::NonNullByDefault(_))
                                || matches!(site_scope, NullnessScope::NonNullByDefault(_))
                            {
                                ty.with_nullability(Nullability::NonNull)
                            } else {
                                ty
                            },
                        );
                    }
                    let raw = m.type_ref.as_deref()?;
                    let scope = type_param_bounds_of_fqn_in_scope(self.db, &self.scope, &owner_fqn);
                    let raw_ty = resolve_type_ref_in_file_for_member(
                        self.db,
                        fid,
                        Some(&owner_fqn),
                        m.nullness_scope,
                        raw,
                        &scope,
                    );
                    if m.is_static {
                        return Some(raw_ty);
                    }
                    return Some(self.substitute_field_ty(recv_ty, &owner_fqn, &raw_ty));
                }
                if ty_item.nested_types.iter().any(|n| n == field_name) {
                    return Some(Ty::raw_class(format!("{owner_fqn}.{field_name}")));
                }
            } else if let Some(bin) = self.scope.get_binary_type(&owner_fqn) {
                if let Some(m) = bin.members.iter().find(|m| {
                    m.kind != MemberKind::Method
                        && m.kind != MemberKind::Constructor
                        && m.name == field_name
                }) {
                    if m.kind == MemberKind::EnumConstant {
                        let member_scope = effective_nullness_scope_for_binary_member(
                            self.scope.classpath(),
                            bin,
                            m.nullness_scope,
                        );
                        let ty = Ty::raw_class(owner_fqn);
                        return Some(
                            if matches!(member_scope, NullnessScope::NonNullByDefault(_))
                                || matches!(site_scope, NullnessScope::NonNullByDefault(_))
                            {
                                ty.with_nullability(Nullability::NonNull)
                            } else {
                                ty
                            },
                        );
                    }
                    let raw = m.type_ref.as_deref()?;
                    let scope = type_param_bounds_of_fqn_in_scope(self.db, &self.scope, &owner_fqn);
                    let raw_ty = resolve_binary_type_ref_for_member(
                        self.scope.classpath(),
                        bin,
                        m.nullness_scope,
                        raw,
                        &scope,
                    );
                    if m.is_static {
                        return Some(raw_ty);
                    }
                    return Some(self.substitute_field_ty(recv_ty, &owner_fqn, &raw_ty));
                }
                if bin.nested_types.iter().any(|n| n == field_name) {
                    return Some(Ty::raw_class(format!("{owner_fqn}.{field_name}")));
                }
            }
        }
        None
    }

    fn substitute_field_ty(&self, recv_ty: &Ty, owner_fqn: &str, raw_ty: &Ty) -> Ty {
        let is_raw = is_raw_class_ty_in_scope(self.db, &self.scope, recv_ty);
        if is_raw {
            return erase_type_vars_to_bound(raw_ty);
        }
        let subst = supertype_substitution_for_ty_in_scope(self.db, &self.scope, recv_ty, owner_fqn)
            .unwrap_or_default();
        subst.apply_covariant(raw_ty)
    }

    pub(super) fn target_ty(&self, target: &ResolvedTarget, subst: &Substitution) -> Ty {
        self.target_ty_with_site_scope(target, subst, NullnessScope::Inherited)
    }

    pub(super) fn target_ty_with_site_scope(
        &self,
        target: &ResolvedTarget,
        subst: &Substitution,
        site_scope: NullnessScope,
    ) -> Ty {
        let enum_const_ty = |owner_fqn: &str, member_scope: NullnessScope| {
            let eff = effective_nullness_scope_for_member_in_scope(
                self.db,
                &self.scope,
                owner_fqn,
                member_scope,
            );
            let ty = Ty::raw_class(owner_fqn);
            if matches!(eff, NullnessScope::NonNullByDefault(_))
                || matches!(site_scope, NullnessScope::NonNullByDefault(_))
            {
                ty.with_nullability(Nullability::NonNull)
            } else {
                ty
            }
        };
        let raw_ty = match target {
            ResolvedTarget::IndexedType(fid, tid) => {
                let fqn = self.db.file_item_tree(*fid).types[*tid]
                    .qualified_name
                    .clone();
                return Ty::raw_class(fqn);
            }
            ResolvedTarget::BinaryType(fqn) => {
                return Ty::raw_class(fqn);
            }
            ResolvedTarget::LocalType(ty) => {
                return Ty::raw_class(&ty.qualified_name);
            }
            ResolvedTarget::IndexedMember(fid, mid) => {
                let tree = self.db.file_item_tree(*fid);
                let m = &tree.members[*mid];
                if m.kind == MemberKind::EnumConstant {
                    return m
                        .owner_type
                        .as_deref()
                        .map_or(Ty::Unknown, |fqn| enum_const_ty(fqn, m.nullness_scope));
                }
                let Some(raw) = m.type_ref.as_deref() else {
                    return Ty::Unknown;
                };
                let scope = m
                    .owner_type
                    .as_deref()
                    .map(|fqn| type_param_bounds_of_fqn_in_scope(self.db, &self.scope, fqn))
                    .unwrap_or_default();
                resolve_type_ref_in_file_for_member(
                    self.db,
                    *fid,
                    m.owner_type.as_deref(),
                    m.nullness_scope,
                    raw,
                    &scope,
                )
            }
            ResolvedTarget::LocalMember(m) => {
                if m.kind == MemberKind::EnumConstant {
                    return m
                        .owner_type
                        .as_deref()
                        .map_or(Ty::Unknown, |fqn| enum_const_ty(fqn, m.nullness_scope));
                }
                let Some(raw) = m.type_ref.as_deref() else {
                    return Ty::Unknown;
                };
                resolve_type_ref_in_file_for_member(
                    self.db,
                    m.file_id,
                    m.owner_type.as_deref(),
                    m.nullness_scope,
                    raw,
                    &BTreeMap::new(),
                )
            }
            ResolvedTarget::BinaryMember {
                owner_fqn,
                member_index,
            } => {
                let Some(bin) = self.scope.get_binary_type(owner_fqn) else {
                    return Ty::Unknown;
                };
                let Some(m) = bin.members.get(*member_index) else {
                    return Ty::Unknown;
                };
                if m.kind == MemberKind::EnumConstant {
                    return enum_const_ty(owner_fqn, m.nullness_scope);
                }
                let Some(raw) = m.type_ref.as_deref() else {
                    return Ty::Unknown;
                };
                let scope = type_param_bounds_of_fqn_in_scope(self.db, &self.scope, owner_fqn);
                resolve_binary_type_ref_for_member(
                    self.scope.classpath(),
                    bin,
                    m.nullness_scope,
                    raw,
                    &scope,
                )
            }
            _ => return Ty::Unknown,
        };
        subst.apply_covariant(&raw_ty)
    }

    pub(super) fn resolve_member_type_in_owner(
        &self,
        target: &ResolvedTarget,
        owner_fqn: &str,
        raw: &str,
        extra_scope: &BTreeMap<SmolStr, Vec<String>>,
    ) -> Ty {
        let mut scope = type_param_bounds_of_fqn_in_scope(self.db, &self.scope, owner_fqn);
        for (k, v) in extra_scope {
            scope.insert(k.clone(), v.clone());
        }
        match target {
            ResolvedTarget::IndexedMember(fid, mid) => {
                let m_scope = self.db.file_item_tree(*fid).members[*mid].nullness_scope;
                resolve_type_ref_in_file_for_member(
                    self.db,
                    *fid,
                    Some(owner_fqn),
                    m_scope,
                    raw,
                    &scope,
                )
            }
            ResolvedTarget::BinaryMember { member_index, .. } => {
                if let Some(bin) = self.scope.get_binary_type(owner_fqn) {
                    let m_scope = bin
                        .members
                        .get(*member_index)
                        .map_or(NullnessScope::Inherited, |m| m.nullness_scope);
                    resolve_binary_type_ref_for_member(
                        self.scope.classpath(),
                        bin,
                        m_scope,
                        raw,
                        &scope,
                    )
                } else {
                    Ty::Unknown
                }
            }
            ResolvedTarget::LocalMember(m) => resolve_type_ref_in_file_for_member(
                self.db,
                m.file_id,
                Some(owner_fqn),
                m.nullness_scope,
                raw,
                &scope,
            ),
            _ => Ty::Unknown,
        }
    }

    pub(super) fn resolve_explicit_type_node(
        &self,
        type_node: &SyntaxNode,
        dims_owner: &SyntaxNode,
        mod_holder: &SyntaxNode,
        is_local_root: bool,
    ) -> Ty {
        let mod_null = extract_modifier_nullability(mod_holder, &self.import_resolver);
        let (raw, _) = render_type_with_dims_and_nullability(
            type_node,
            dims_owner,
            mod_null,
            false,
            &self.import_resolver,
        );
        self.resolve_type_str_with_local(&raw, dims_owner, is_local_root)
    }

    pub(super) fn resolve_type_node(&self, type_node: &SyntaxNode, site_node: &SyntaxNode) -> Ty {
        let raw = render_type_node(type_node, &self.import_resolver);
        self.resolve_type_str(&raw, site_node)
    }

    pub(super) fn resolve_type_str(&self, raw: &str, site_node: &SyntaxNode) -> Ty {
        self.resolve_type_str_with_local(raw, site_node, false)
    }

    pub(super) fn resolve_type_str_with_local(
        &self,
        raw: &str,
        site_node: &SyntaxNode,
        is_local_root: bool,
    ) -> Ty {
        let scope = type_params_in_scope_at(
            self.db,
            &self.scope,
            &self.file_tree,
            &self.ast_id_map,
            site_node,
        );
        let owner_fqn = enclosing_type_fqn(&self.file_tree, &self.ast_id_map, site_node);
        let nullness_scope = self.effective_scope_at(site_node);
        resolve_type_ref_in_file_with_scope_and_visible(
            self.db,
            self.file_id,
            &self.scope,
            owner_fqn.as_deref(),
            nullness_scope,
            is_local_root,
            raw,
            &scope,
        )
    }

    pub(super) fn extract_explicit_type_args(&self, call_node: &SyntaxNode) -> Vec<TypeArg> {
        let Some(targs_node) = call_node.children().find(|n| n.kind() == TYPE_ARGS) else {
            return Vec::new();
        };
        let rendered_args: Vec<String> = targs_node
            .children()
            .filter(|n| is_type_node(n.kind()) || n.kind() == WILDCARD_TYPE)
            .map(|arg| render_type_node(&arg, &self.import_resolver))
            .collect();
        if rendered_args.is_empty() {
            return Vec::new();
        }
        // Parse `Dummy<...>` via `parse_type_ref` to reuse wildcard/nested arg parsing cleanly.
        let dummy = format!("__Dummy<{}>", rendered_args.join(", "));
        let scope = type_params_in_scope_at(
            self.db,
            &self.scope,
            &self.file_tree,
            &self.ast_id_map,
            call_node,
        );
        let owner_fqn = enclosing_type_fqn(&self.file_tree, &self.ast_id_map, call_node);
        let nullness_scope = self.effective_scope_at(call_node);
        let ty = parse_type_ref(&dummy, &mut |segs| {
            if segs == ["__Dummy"] {
                Some(Ty::raw_class("__Dummy"))
            } else {
                let joined = segs.join(".");
                Some(resolve_type_ref_in_file_with_scope_and_visible(
                    self.db,
                    self.file_id,
                    &self.scope,
                    owner_fqn.as_deref(),
                    nullness_scope,
                    false,
                    &joined,
                    &scope,
                ))
            }
        });
        let ty = apply_nullness_scope(ty, nullness_scope, false);
        match ty {
            Ty::Class { type_args, .. } => type_args,
            _ => Vec::new(),
        }
    }

    pub(super) fn extract_path_type_last_args(
        &self,
        path_type: &SyntaxNode,
        site_node: &SyntaxNode,
    ) -> Option<Vec<TypeArg>> {
        let raw = render_type_node(path_type, &self.import_resolver);
        let ty = self.resolve_type_str(&raw, site_node);
        match ty {
            Ty::Class { type_args, .. } => Some(type_args),
            _ => None,
        }
    }

    pub(super) fn type_fqn_for_decl(&self, type_node: &SyntaxNode) -> Option<String> {
        self.ast_id_map
            .find_by_full(type_node.text_range())
            .and_then(|id| self.file_tree.find_type_by_ast_id(id))
            .map(|(_, ty)| ty.qualified_name.clone())
    }

    pub(super) fn parameterized_self_ty(&self, fqn: &str) -> Ty {
        let tp_bounds = type_param_bounds_of_fqn_in_scope(self.db, &self.scope, fqn);
        let tp_names = type_params_of_fqn_in_scope(self.db, &self.scope, fqn);
        let type_args = tp_names
            .into_iter()
            .map(|tp| {
                let tv = resolve_type_ref_in_file_for_owner(
                    self.db,
                    self.file_id,
                    Some(fqn),
                    tp.as_str(),
                    &tp_bounds,
                );
                match tv {
                    t @ Ty::TypeVar { .. } => TypeArg::Type(t),
                    _ => TypeArg::Type(Ty::type_var(tp, Vec::new())),
                }
            })
            .collect();
        Ty::class(fqn, type_args)
    }

    pub(super) fn infer_enclosing_self_ty(&self, node: &SyntaxNode) -> Ty {
        for ancestor in node.ancestors().skip(1) {
            if TypeKind::from_syntax(ancestor.kind()).is_some()
                && let Some(fqn) = self.type_fqn_for_decl(&ancestor)
            {
                return self.parameterized_self_ty(&fqn);
            }
        }
        Ty::Unknown
    }

    pub(super) fn direct_superclass_ty(&self, self_ty: &Ty) -> Option<Ty> {
        let fqn = self_ty.erased_fqn()?;
        if let Some((fid, tid)) = self.scope.get_workspace_type(fqn) {
            let tree = self.db.file_item_tree(fid);
            let ty_item = &tree.types[tid];
            if ty_item.kind != TypeKind::Class {
                return None;
            }
            let sup = if let Some(raw) = ty_item.extends.first() {
                let scope = type_param_bounds_of_fqn_in_scope(self.db, &self.scope, fqn);
                resolve_type_ref_in_file_for_owner(self.db, fid, Some(fqn), raw, &scope)
            } else if fqn != OBJECT_FQN {
                Ty::object()
            } else {
                return None;
            };
            let subst = supertype_substitution_for_ty_in_scope(self.db, &self.scope, self_ty, fqn)
                .unwrap_or_default();
            return Some(subst.apply_covariant(&sup));
        }
        if let Some(bin) = self.scope.get_binary_type(fqn) {
            if bin.kind != TypeKind::Class || fqn == OBJECT_FQN {
                return None;
            }
            let sup = if let Some(raw) = bin.super_class_generic.as_deref() {
                let scope = type_param_bounds_of_fqn_in_scope(self.db, &self.scope, fqn);
                resolve_binary_type_ref_for_owner(self.scope.classpath(), bin, raw, &scope)
            } else {
                Ty::raw_class(bin.super_class.as_deref().unwrap_or(OBJECT_FQN))
            };
            let subst = supertype_substitution_for_ty_in_scope(self.db, &self.scope, self_ty, fqn)
                .unwrap_or_default();
            return Some(subst.apply_covariant(&sup));
        }
        None
    }

    pub(super) fn is_interface_fqn(&self, fqn: &str) -> bool {
        if let Some((fid, tid)) = self.scope.get_workspace_type(fqn) {
            self.db.file_item_tree(fid).types[tid].kind == TypeKind::Interface
        } else if let Some(bin) = self.scope.get_binary_type(fqn) {
            bin.kind == TypeKind::Interface
        } else {
            false
        }
    }

    pub(super) fn is_known_fqn(&self, fqn: &str) -> bool {
        self.scope.is_known_type_fqn(&self.file_tree, fqn)
    }
}

// ---- Pure CST helpers ----

pub(crate) fn effective_nullness_scope_at(
    db: &dyn DefDatabase,
    file_id: FileId,
    node: &SyntaxNode,
) -> NullnessScope {
    let file_tree = db.file_item_tree(file_id);
    let ast_id_map = db.ast_id_map(file_id);
    let scope = VisibleScope::for_file(db, file_id);
    let resolver = ImportResolver::from_imports(
        file_tree.package.as_deref(),
        file_tree
            .imports
            .iter()
            .map(|(_, imp)| (imp.kind, imp.path.as_slice())),
    );
    effective_nullness_scope_at_inner(db, file_id, &scope, &file_tree, &ast_id_map, &resolver, node)
}

pub(super) fn effective_nullness_scope_at_inner(
    db: &dyn DefDatabase,
    file_id: FileId,
    scope: &VisibleScope,
    file_tree: &FileItemTree,
    ast_id_map: &AstIdMap,
    import_resolver: &ImportResolver,
    node: &SyntaxNode,
) -> NullnessScope {
    for ancestor in node.ancestors() {
        match ancestor.kind() {
            METHOD_DECL | CONSTRUCTOR_DECL => {
                if let Some(id) = ast_id_map.find_by_full(ancestor.text_range())
                    && let Some((_, m)) = file_tree.find_member_by_ast_id(id)
                {
                    if m.nullness_scope != NullnessScope::Inherited {
                        return m.nullness_scope;
                    }
                } else {
                    let s = extract_modifier_nullness_scope(&ancestor, import_resolver);
                    if s != NullnessScope::Inherited {
                        return s;
                    }
                }
            }
            VARIABLE_DECLARATOR if ancestor.parent().is_some_and(|p| p.kind() == FIELD_DECL) => {
                if let Some(id) = ast_id_map.find_by_full(ancestor.text_range())
                    && let Some((_, m)) = file_tree.find_member_by_ast_id(id)
                {
                    if m.nullness_scope != NullnessScope::Inherited {
                        return m.nullness_scope;
                    }
                } else if let Some(field_node) = ancestor.parent() {
                    let s = extract_modifier_nullness_scope(&field_node, import_resolver);
                    if s != NullnessScope::Inherited {
                        return s;
                    }
                }
            }
            FIELD_DECL => {
                if let Some(decl) = ancestor.children().find(|n| n.kind() == VARIABLE_DECLARATOR)
                    && let Some(id) = ast_id_map.find_by_full(decl.text_range())
                    && let Some((_, m)) = file_tree.find_member_by_ast_id(id)
                {
                    if m.nullness_scope != NullnessScope::Inherited {
                        return m.nullness_scope;
                    }
                } else {
                    let s = extract_modifier_nullness_scope(&ancestor, import_resolver);
                    if s != NullnessScope::Inherited {
                        return s;
                    }
                }
            }
            k if TypeKind::from_syntax(k).is_some() => {
                if let Some((_, ty)) = ast_id_map
                    .find_by_full(ancestor.text_range())
                    .and_then(|id| file_tree.find_type_by_ast_id(id))
                {
                    let s = effective_nullness_scope_for_type_in_scope(db, scope, &ty.qualified_name);
                    if s != NullnessScope::Inherited {
                        return s;
                    }
                    if ty.nullness_scope != NullnessScope::Inherited {
                        return ty.nullness_scope;
                    }
                    return effective_nullness_scope_for_file(db, file_id);
                }
                let s = extract_modifier_nullness_scope(&ancestor, import_resolver);
                if s != NullnessScope::Inherited {
                    return s;
                }
            }
            _ => {}
        }
    }
    effective_nullness_scope_for_file(db, file_id)
}

pub(crate) fn is_raw_class_ty_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    ty: &Ty,
) -> bool {
    match ty {
        Ty::Class { fqn, type_args, .. } => {
            type_args.is_empty() && !type_params_of_fqn_in_scope(db, scope, fqn).is_empty()
        }
        _ => false,
    }
}

pub(crate) fn enclosing_type_fqn(
    file_tree: &FileItemTree,
    ast_id_map: &AstIdMap,
    node: &SyntaxNode,
) -> Option<String> {
    for ancestor in node.ancestors() {
        if TypeKind::from_syntax(ancestor.kind()).is_some()
            && let Some((_, ty)) = ast_id_map
                .find_by_full(ancestor.text_range())
                .and_then(|id| file_tree.find_type_by_ast_id(id))
        {
            return Some(ty.qualified_name.clone());
        }
    }
    None
}

pub(crate) fn type_params_in_scope_at(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    file_tree: &FileItemTree,
    ast_id_map: &AstIdMap,
    node: &SyntaxNode,
) -> BTreeMap<SmolStr, Vec<String>> {
    let mut scopes: Vec<BTreeMap<SmolStr, Vec<String>>> = Vec::new();

    for ancestor in node.ancestors() {
        match ancestor.kind() {
            METHOD_DECL | CONSTRUCTOR_DECL => {
                let mut m_map = BTreeMap::new();
                if let Some(tps) = ancestor.children().find(|n| n.kind() == TYPE_PARAMS) {
                    collect_type_param_bounds_from_node(&tps, &mut m_map);
                }
                if !m_map.is_empty() {
                    scopes.push(m_map);
                }
                if ancestor.kind() == METHOD_DECL && has_modifier(&ancestor, STATIC_KW) {
                    break;
                }
            }
            FIELD_DECL | INITIALIZER
                if has_modifier(&ancestor, STATIC_KW) || has_direct_token(&ancestor, STATIC_KW) =>
            {
                break;
            }
            k if TypeKind::from_syntax(k).is_some() => {
                if let Some((_, ty)) = ast_id_map
                    .find_by_full(ancestor.text_range())
                    .and_then(|id| file_tree.find_type_by_ast_id(id))
                {
                    let map = type_param_bounds_of_fqn_in_scope(db, scope, &ty.qualified_name);
                    if !map.is_empty() {
                        scopes.push(map);
                    }
                    break;
                }
                let mut local_map = BTreeMap::new();
                if let Some(tps) = ancestor.children().find(|n| n.kind() == TYPE_PARAMS) {
                    collect_type_param_bounds_from_node(&tps, &mut local_map);
                }
                if !local_map.is_empty() {
                    scopes.push(local_map);
                }
                if has_modifier(&ancestor, STATIC_KW) || !matches!(k, CLASS_DECL) {
                    break;
                }
            }
            _ => {}
        }
    }

    let mut combined = BTreeMap::new();
    for map in scopes.into_iter().rev() {
        for (k, v) in map {
            combined.insert(k, v);
        }
    }
    combined
}

fn collect_type_param_bounds_from_node(
    tps_node: &SyntaxNode,
    out: &mut BTreeMap<SmolStr, Vec<String>>,
) {
    for tp in tps_node.children().filter(|n| n.kind() == TYPE_PARAM) {
        let Some(name_node) = name_child(&tp) else {
            continue;
        };
        let bounds = tp
            .children()
            .filter(|n| is_type_node(n.kind()))
            .map(|t| collapsed_text(&t))
            .collect();
        out.insert(node_ident_text(&name_node), bounds);
    }
}

pub(crate) fn resolve_params_in_file_for_member(
    db: &dyn DefDatabase,
    file_id: FileId,
    owner_fqn: Option<&str>,
    member_scope: NullnessScope,
    params: &[ParamSummary],
    scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Vec<(Ty, bool)> {
    let eff_scope = if member_scope != NullnessScope::Inherited {
        member_scope
    } else if let Some(fqn) = owner_fqn {
        let visible = VisibleScope::for_file(db, file_id);
        effective_nullness_scope_for_type_in_scope(db, &visible, fqn)
    } else {
        effective_nullness_scope_for_file(db, file_id)
    };
    params
        .iter()
        .map(|p| {
            let base = resolve_type_ref_in_file_for_member(
                db,
                file_id,
                owner_fqn,
                member_scope,
                &p.type_ref,
                scope,
            );
            let ty = if p.is_varargs {
                let arr = Ty::array(base);
                if let Some((n, _)) = p.decl_nullability {
                    arr.with_nullability(n)
                } else if matches!(eff_scope, NullnessScope::NonNullByDefault(_)) {
                    arr.with_nullability(Nullability::NonNull)
                } else {
                    arr
                }
            } else {
                base
            };
            (ty, p.is_varargs)
        })
        .collect()
}

pub(crate) fn resolve_params_in_binary_for_member(
    classpath: &ClasspathIndex,
    owner: &BinaryTypeSymbol,
    member_scope: NullnessScope,
    params: &[ParamSummary],
    scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Vec<(Ty, bool)> {
    let eff_scope = effective_nullness_scope_for_binary_member(classpath, owner, member_scope);
    params
        .iter()
        .map(|p| {
            let base = resolve_binary_type_ref_for_member(
                classpath,
                owner,
                member_scope,
                &p.type_ref,
                scope,
            );
            let ty = if p.is_varargs {
                let arr = Ty::array(base);
                if let Some((n, _)) = p.decl_nullability {
                    arr.with_nullability(n)
                } else if matches!(eff_scope, NullnessScope::NonNullByDefault(_)) {
                    arr.with_nullability(Nullability::NonNull)
                } else {
                    arr
                }
            } else {
                base
            };
            (ty, p.is_varargs)
        })
        .collect()
}

// ---- Crate-internal entry points for `resolve` ----

pub(crate) fn resolve_method_call_target(
    db: &dyn DefDatabase,
    file_id: FileId,
    method_call: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let mut ctx = InferCtx::new(db, file_id);
    let expected = ctx.expected_target_type(method_call);
    let info = ctx.infer_method_call(method_call, expected.as_ref())?;
    let is_raw = !info.target.is_static_member(db)
        && method_call
            .children()
            .next()
            .map(|r| ctx.infer_expr(&r, None))
            .is_some_and(|t| is_raw_class_ty_in_scope(db, &ctx.scope, &t));
    Some(attach_substituted_detail(db, &ctx.scope, info.target, &info.subst, is_raw))
}

pub(crate) fn resolve_unqualified_call_target(
    db: &dyn DefDatabase,
    file_id: FileId,
    call_expr: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let mut ctx = InferCtx::new(db, file_id);
    let expected = ctx.expected_target_type(call_expr);
    let info = ctx.infer_unqualified_call(call_expr, expected.as_ref())?;
    Some(attach_substituted_detail(db, &ctx.scope, info.target, &info.subst, false))
}

pub(crate) fn resolve_method_ref_target_with_context(
    db: &dyn DefDatabase,
    file_id: FileId,
    method_ref: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let mut ctx = InferCtx::new(db, file_id);
    let expected = ctx.expected_target_type(method_ref)?;
    let sam = find_functional_interface_sam_in_scope(db, &ctx.scope, &expected)?;
    let (target, subst) = ctx.resolve_method_ref_with_sam(method_ref, &sam)?;
    Some(attach_substituted_detail(db, &ctx.scope, target, &subst, false))
}

pub(crate) fn resolve_field_access_target(
    db: &dyn DefDatabase,
    file_id: FileId,
    recv_ty: &Ty,
    target: ResolvedTarget,
) -> ResolvedTarget {
    let scope = VisibleScope::for_file(db, file_id);
    let is_raw = is_raw_class_ty_in_scope(db, &scope, recv_ty);
    let owner_fqn = match &target {
        ResolvedTarget::IndexedMember(fid, mid) => db.file_item_tree(*fid).members[*mid]
            .owner_type
            .clone(),
        ResolvedTarget::LocalMember(m) => m.owner_type.clone(),
        ResolvedTarget::BinaryMember { owner_fqn, .. } => Some(owner_fqn.clone()),
        _ => None,
    };
    let Some(owner_fqn) = owner_fqn else {
        return target;
    };
    let subst =
        supertype_substitution_for_ty_in_scope(db, &scope, recv_ty, &owner_fqn).unwrap_or_default();
    attach_substituted_detail(db, &scope, target, &subst, is_raw)
}

#[cfg(test)]
#[expect(dead_code, reason = "kept as convenience wrapper for unit tests")]
pub(crate) fn format_member_detail(db: &dyn DefDatabase, target: &ResolvedTarget) -> Option<SymbolDetail> {
    let scope = VisibleScope::flat(db);
    format_member_detail_in_scope(db, &scope, target)
}

pub(crate) fn format_member_detail_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    target: &ResolvedTarget,
) -> Option<SymbolDetail> {
    format_substituted_detail(db, scope, target, &Substitution::default(), false)
}

fn attach_substituted_detail(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    target: ResolvedTarget,
    subst: &Substitution,
    is_raw: bool,
) -> ResolvedTarget {
    if let Some(detail) = format_substituted_detail(db, scope, &target, subst, is_raw) {
        ResolvedTarget::WithDetail {
            inner: Box::new(target),
            detail,
        }
    } else {
        target
    }
}

fn format_substituted_detail(
    db: &dyn DefDatabase,
    visible: &VisibleScope,
    target: &ResolvedTarget,
    subst: &Substitution,
    is_raw: bool,
) -> Option<SymbolDetail> {
    let classpath = visible.classpath();
    match target {
        ResolvedTarget::IndexedMember(fid, mid) => {
            let tree = db.file_item_tree(*fid);
            let m = &tree.members[*mid];
            let owner_fqn = m.owner_type.as_deref();
            let mut scope = owner_fqn
                .map(|fqn| type_param_bounds_of_fqn_in_scope(db, visible, fqn))
                .unwrap_or_default();
            for tp in &m.type_params {
                scope.insert(
                    tp.clone(),
                    m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
                );
            }
            let resolve_fn = |raw: &str| {
                resolve_type_ref_in_file_for_member(
                    db,
                    *fid,
                    owner_fqn,
                    m.nullness_scope,
                    raw,
                    &scope,
                )
            };
            build_substituted_member_detail(
                m.kind,
                m.type_ref.as_deref(),
                &m.params,
                subst,
                is_raw,
                &resolve_fn,
            )
        }
        ResolvedTarget::LocalMember(m) => {
            let owner_fqn = m.owner_type.as_deref().unwrap_or("");
            let mut scope = if owner_fqn.is_empty() {
                BTreeMap::new()
            } else {
                type_param_bounds_of_fqn_in_scope(db, visible, owner_fqn)
            };
            for tp in &m.type_params {
                scope.insert(
                    tp.clone(),
                    m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
                );
            }
            let resolve_fn = |raw: &str| {
                resolve_type_ref_in_file_for_member(
                    db,
                    m.file_id,
                    (!owner_fqn.is_empty()).then_some(owner_fqn),
                    m.nullness_scope,
                    raw,
                    &scope,
                )
            };
            build_substituted_member_detail(
                m.kind,
                m.type_ref.as_deref(),
                &m.params,
                subst,
                is_raw,
                &resolve_fn,
            )
        }
        ResolvedTarget::BinaryMember {
            owner_fqn,
            member_index,
        } => {
            let bin = classpath.type_by_fqn(owner_fqn)?;
            let m = bin.members.get(*member_index)?;
            let mut scope = type_param_bounds_of_fqn_in_scope(db, visible, owner_fqn);
            for tp in &m.type_params {
                scope.insert(
                    tp.clone(),
                    m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
                );
            }
            let resolve_fn = |raw: &str| {
                resolve_binary_type_ref_for_member(classpath, bin, m.nullness_scope, raw, &scope)
            };
            build_substituted_member_detail(
                m.kind,
                m.type_ref.as_deref(),
                &m.params,
                subst,
                is_raw,
                &resolve_fn,
            )
        }
        _ => None,
    }
}

fn build_substituted_member_detail(
    kind: MemberKind,
    type_ref: Option<&str>,
    params: &[ParamSummary],
    subst: &Substitution,
    is_raw: bool,
    resolve_fn: &dyn Fn(&str) -> Ty,
) -> Option<SymbolDetail> {
    let apply_subst = |raw_ty: &Ty| -> Ty {
        if is_raw {
            erase_type_vars_to_bound(raw_ty)
        } else {
            subst.apply_covariant(raw_ty)
        }
    };

    match kind {
        MemberKind::Field | MemberKind::RecordComponent => {
            let raw = type_ref?;
            let orig = resolve_fn(raw);
            let sub = apply_subst(&orig);
            if (sub != orig || ty_has_explicit_nullability(&sub)) && !matches!(sub, Ty::Unknown) {
                Some(SymbolDetail::TypeRef(sub.display_short()))
            } else {
                None
            }
        }
        MemberKind::Method => {
            let mut changed = false;
            let return_type = type_ref.map(|raw| {
                let orig = resolve_fn(raw);
                let sub = apply_subst(&orig);
                if (sub != orig || ty_has_explicit_nullability(&sub)) && !matches!(sub, Ty::Unknown)
                {
                    changed = true;
                    sub.display_short()
                } else {
                    raw.to_string()
                }
            });
            let mut sub_params = Vec::with_capacity(params.len());
            for p in params {
                let orig = resolve_fn(&p.type_ref);
                let sub = apply_subst(&orig);
                let ty_str = if (sub != orig || ty_has_explicit_nullability(&sub))
                    && !matches!(sub, Ty::Unknown)
                {
                    changed = true;
                    sub.display_short()
                } else {
                    p.type_ref.clone()
                };
                sub_params.push(ParamSummary {
                    name: p.name.clone(),
                    type_ref: ty_str,
                    is_varargs: p.is_varargs,
                    decl_nullability: p.decl_nullability,
                });
            }
            changed.then(|| SymbolDetail::Method {
                return_type,
                params: format_params_signature(&sub_params),
            })
        }
        MemberKind::Constructor => {
            let mut changed = false;
            let mut sub_params = Vec::with_capacity(params.len());
            for p in params {
                let orig = resolve_fn(&p.type_ref);
                let sub = apply_subst(&orig);
                let ty_str = if (sub != orig || ty_has_explicit_nullability(&sub))
                    && !matches!(sub, Ty::Unknown)
                {
                    changed = true;
                    sub.display_short()
                } else {
                    p.type_ref.clone()
                };
                sub_params.push(ParamSummary {
                    name: p.name.clone(),
                    type_ref: ty_str,
                    is_varargs: p.is_varargs,
                    decl_nullability: p.decl_nullability,
                });
            }
            changed.then(|| SymbolDetail::Constructor {
                params: format_params_signature(&sub_params),
            })
        }
        MemberKind::EnumConstant => None,
    }
}
