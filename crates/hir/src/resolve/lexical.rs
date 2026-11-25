use syntax::{
    SyntaxNode, TextRange,
    ext::{collapsed_text, has_direct_token, has_modifier, is_type_node},
    syntax_kind::SyntaxKind::*,
};

use crate::{
    FileId,
    hierarchy::linearized_supertypes_in_scope,
    item_tree::{AstIdMap, FileItemTree, TypeId},
    scope::{ModuleImportTarget, VisibleScope},
    symbols::{
        ImportKind, MemberKind, TypeKind, extract_local_type_symbol, node_ident_text,
    },
};

use super::{
    LocalKind, LocalSymbol, ResolveCtx, ResolvedTarget, TypeParamSymbol,
    local_symbol_from_node, local_symbol_with_type,
    member::{
        CallableQuery, TypeRefTarget, select_callable_from_binary, select_callable_from_tree,
        supertypes_of_target,
    },
};

// ---- Lexical local scope resolution ----

pub(crate) fn find_local_decl_node(ref_node: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    find_local_binding(FileId(0), ref_node, name).map(|(_, decl_node)| decl_node)
}

pub(super) fn find_local_binding(
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

pub(super) fn resolve_type_segments(
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
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, &fqn) {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
        }
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::SingleType
                && imp.path.last().map(String::as_str) == Some(name)
            {
                let fqn = imp.path.join(".");
                if ctx.scope.contains_binary_type(&fqn) {
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
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, &fqn) {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
        }
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::StaticSingle
                && imp.path.last().map(String::as_str) == Some(name)
            {
                let fqn = imp.path.join(".");
                if ctx.scope.contains_binary_type(&fqn) {
                    return Some(ResolvedTarget::BinaryType(fqn));
                }
            }
        }

        // 4. Same-package top-level types (workspace first, then binary).
        let pkg = ctx.file_tree.package.as_deref().unwrap_or("");
        if let Some(found) = lookup_package_type_in_workspace(ctx.file_tree, ctx.scope, pkg, name)
        {
            return Some(found);
        }
        if let Some(ty) = ctx.scope.get_binary_type_in_package(pkg, name) {
            return Some(ResolvedTarget::BinaryType(ty.qualified_name.clone()));
        }

        // 5. Implicit `java.lang.*` (workspace `java.lang` first if any, then binary `java.lang`).
        if let Some(found) =
            lookup_package_type_in_workspace(ctx.file_tree, ctx.scope, "java.lang", name)
        {
            return Some(found);
        }
        if let Some(ty) = ctx.scope.get_binary_type_in_package("java.lang", name) {
            return Some(ResolvedTarget::BinaryType(ty.qualified_name.clone()));
        }

        // 6. Type-import-on-demand (`import a.b.*;`) (workspace first, then binary).
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::TypeOnDemand {
                let fqn = format!("{}.{name}", imp.path.join("."));
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, &fqn) {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
        }
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::TypeOnDemand {
                let fqn = format!("{}.{name}", imp.path.join("."));
                if ctx.scope.contains_binary_type(&fqn) {
                    return Some(ResolvedTarget::BinaryType(fqn));
                }
            }
        }

        // 7. Static-import-on-demand (`import static a.b.Outer.*;`) (workspace first, then binary).
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::StaticOnDemand {
                let fqn = format!("{}.{name}", imp.path.join("."));
                if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, &fqn) {
                    return Some(ResolvedTarget::IndexedType(fid, tid));
                }
            }
        }
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::StaticOnDemand {
                let fqn = format!("{}.{name}", imp.path.join("."));
                if ctx.scope.contains_binary_type(&fqn) {
                    return Some(ResolvedTarget::BinaryType(fqn));
                }
            }
        }

        // 8. Module imports (`import module M;`, JEP 511, transitive & cycle-safe).
        for (_, imp) in ctx.file_tree.imports.iter() {
            if imp.kind == ImportKind::Module {
                let module_name = imp.path.join(".");
                if let Some(target) =
                    ctx.scope
                        .resolve_module_import_type(ctx.file_tree, &module_name, name)
                {
                    return Some(match target {
                        ModuleImportTarget::Workspace(fid, tid) => {
                            ResolvedTarget::IndexedType(fid, tid)
                        }
                        ModuleImportTarget::Binary(fqn) => ResolvedTarget::BinaryType(fqn),
                    });
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
            for sup_fqn in linearized_supertypes_in_scope(ctx.db, ctx.scope, &current_qname) {
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
    scope: &VisibleScope,
    pkg: &str,
    name: &str,
) -> Option<ResolvedTarget> {
    scope
        .lookup_package_type_in_workspace(file_tree, pkg, name)
        .map(|(fid, tid)| ResolvedTarget::IndexedType(fid, tid))
}

pub(super) fn lookup_workspace_or_binary_type(
    ctx: ResolveCtx<'_>,
    fqn: &str,
) -> Option<ResolvedTarget> {
    if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, fqn) {
        return Some(ResolvedTarget::IndexedType(fid, tid));
    }
    ctx.scope
        .contains_binary_type(fqn)
        .then(|| ResolvedTarget::BinaryType(fqn.to_string()))
}

fn resolve_enclosing_type_or_param(
    ctx: ResolveCtx<'_>,
    site_node: &SyntaxNode,
    name: &str,
) -> Option<ResolvedTarget> {
    let file_id = ctx.file_id;
    let file_tree = ctx.file_tree;
    let ast_id_map = ctx.ast_id_map;
    let scope = ctx.scope;
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
                        if let Some((fid, tid)) = lookup_type_by_fqn(file_tree, scope, &nested_fqn)
                        {
                            return Some(ResolvedTarget::IndexedType(fid, tid));
                        }
                    }
                    for sup_fqn in linearized_supertypes_in_scope(ctx.db, ctx.scope, &ty.qualified_name)
                    {
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
                        if let Some((fid, tid)) = lookup_type_by_fqn(file_tree, scope, &nested_fqn)
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

pub(super) fn enclosing_type_targets(
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

pub(super) fn lookup_type_by_fqn(
    file_tree: &FileItemTree,
    scope: &VisibleScope,
    fqn: &str,
) -> Option<(FileId, TypeId)> {
    scope.lookup_type_by_fqn(file_tree, fqn)
}

// ---- Static imports ----

pub(crate) fn resolve_via_static_imports_pub(
    ctx: ResolveCtx<'_>,
    name: &str,
    call_arity: Option<usize>,
    prefer_callable: bool,
) -> Option<ResolvedTarget> {
    resolve_via_static_imports(ctx, name, call_arity, prefer_callable)
}

pub(super) fn resolve_via_static_imports(
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
                lookup_type_by_fqn(ctx.file_tree, ctx.scope, &owner_fqn)
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
            if ctx.scope.contains_binary_type(&owner_fqn)
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
                lookup_type_by_fqn(ctx.file_tree, ctx.scope, &owner_fqn)
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
            if ctx.scope.contains_binary_type(&owner_fqn)
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

pub(super) fn lookup_static_member_or_nested(
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
            CallableQuery {
                target_kind: MemberKind::Method,
                name,
                call_arity: arity,
                include_record_components: false,
                only_static: true,
                exclude_static: false,
            },
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
        if let Some((fid, tid)) = lookup_type_by_fqn(ctx.file_tree, ctx.scope, &nested_fqn) {
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

pub(super) fn lookup_static_member_or_nested_binary(
    ctx: ResolveCtx<'_>,
    owner_fqn: &str,
    name: &str,
    callable_arity: Option<Option<usize>>,
) -> Option<ResolvedTarget> {
    let owner_ty = ctx.scope.get_binary_type(owner_fqn)?;

    if let Some(arity) = callable_arity {
        return select_callable_from_binary(
            &owner_ty.members,
            CallableQuery {
                target_kind: MemberKind::Method,
                name,
                call_arity: arity,
                include_record_components: false,
                only_static: true,
                exclude_static: false,
            },
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
