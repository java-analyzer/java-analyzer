//! Bidirectional, cycle-safe Java expression and local variable type inference.
//! See `docs/specs/0008-expression-type-inference-and-generics.md` (Section 3).

mod call;
mod sam;
mod scope;
#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, sync::Arc};

use rustc_hash::{FxHashMap, FxHashSet};
use smol_str::SmolStr;
use syntax::{
    SyntaxNode, TextRange,
    ext::{collapsed_text, has_direct_token, is_type_node},
    syntax_kind::SyntaxKind::*,
};

use crate::{
    DefDatabase, FileId,
    classpath::{BinaryMemberSymbol, ClasspathIndex},
    item_tree::{AstIdMap, FileItemTree, MemberItem},
    resolve::{self, ResolveCtx},
    symbols::{
        ImportResolver, MemberKind, NullnessScope, TypeKind, WorkspaceIndex,
        extract_modifier_nullability, node_ident_text, render_type_with_dims_and_nullability,
    },
    ty::{
        Nullability, PrimitiveType, Substitution, Ty, binary_numeric_promotion,
        least_upper_bound, resolve_binary_type_ref_for_member,
        resolve_type_ref_in_file_for_member, supertype_substitution_for_ty,
        type_param_bounds_of_fqn, type_params_of_fqn, unary_numeric_promotion,
    },
};

pub use sam::{SamSignature, find_functional_interface_sam};

pub(crate) use call::parameter_for_arg;
pub(crate) use sam::erase_type_vars_to_bound;
pub(crate) use scope::{
    effective_nullness_scope_at, format_member_detail, is_raw_class_ty,
    resolve_field_access_target, resolve_method_call_target,
    resolve_method_ref_target_with_context, resolve_params_in_binary_for_member,
    resolve_params_in_file_for_member, resolve_unqualified_call_target,
};

/// Maximum recursion depth for expression and local-binding type inference.
pub(super) const MAX_INFER_DEPTH: usize = 32;

pub(super) const OBJECT_FQN: &str = "java.lang.Object";
const STRING_FQN: &str = "java.lang.String";
const CLASS_FQN: &str = "java.lang.Class";
const ITERABLE_FQN: &str = "java.lang.Iterable";

/// Infers the [`Ty`] of a Java expression CST node, optionally guided by an `expected` target type.
#[must_use]
pub fn infer_expr_ty(
    db: &dyn DefDatabase,
    file_id: FileId,
    expr_node: &SyntaxNode,
    expected: Option<&Ty>,
) -> Ty {
    let mut ctx = InferCtx::new(db, file_id);
    ctx.infer_expr(expr_node, expected)
}

/// Infers the [`Ty`] of a local variable, parameter, for-each binding, resource variable,
/// catch parameter, or pattern variable (handling explicit types as well as `var` and untyped
/// lambda parameters).
#[must_use]
pub fn infer_local_ty(db: &dyn DefDatabase, file_id: FileId, binding_node: &SyntaxNode) -> Ty {
    let mut ctx = InferCtx::new(db, file_id);
    ctx.infer_local(binding_node)
}

// ---- Inference Context ----

pub(super) struct InferCtx<'a> {
    pub(super) db: &'a dyn DefDatabase,
    pub(super) file_id: FileId,
    pub(super) file_tree: Arc<FileItemTree>,
    pub(super) ast_id_map: Arc<AstIdMap>,
    pub(super) index: Arc<WorkspaceIndex>,
    pub(super) classpath: Arc<ClasspathIndex>,
    pub(super) import_resolver: ImportResolver,
    depth: usize,
    visiting_nodes: FxHashSet<TextRange>,
    pub(super) lambda_param_bindings: FxHashMap<TextRange, Ty>,
}

impl<'a> InferCtx<'a> {
    pub(super) fn new(db: &'a dyn DefDatabase, file_id: FileId) -> Self {
        let file_tree = db.file_item_tree(file_id);
        let import_resolver = ImportResolver::from_imports(
            file_tree.package.as_deref(),
            file_tree
                .imports
                .iter()
                .map(|(_, imp)| (imp.kind, imp.path.as_slice())),
        );
        Self {
            db,
            file_id,
            file_tree,
            ast_id_map: db.ast_id_map(file_id),
            index: db.workspace_index(),
            classpath: db.classpath_index(),
            import_resolver,
            depth: 0,
            visiting_nodes: FxHashSet::default(),
            lambda_param_bindings: FxHashMap::default(),
        }
    }

    pub(super) fn effective_scope_at(&self, node: &SyntaxNode) -> NullnessScope {
        scope::effective_nullness_scope_at_inner(
            self.db,
            self.file_id,
            &self.file_tree,
            &self.ast_id_map,
            &self.import_resolver,
            node,
        )
    }

    pub(super) fn resolve_ctx(&self) -> ResolveCtx<'_> {
        ResolveCtx {
            db: self.db,
            file_id: self.file_id,
            file_tree: &self.file_tree,
            ast_id_map: &self.ast_id_map,
            index: &self.index,
            classpath: &self.classpath,
        }
    }

    fn with_guard(&mut self, node: &SyntaxNode, f: impl FnOnce(&mut Self) -> Ty) -> Ty {
        if self.depth >= MAX_INFER_DEPTH {
            return Ty::Unknown;
        }
        let range = node.text_range();
        if !self.visiting_nodes.insert(range) {
            return Ty::Unknown;
        }
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        self.visiting_nodes.remove(&range);
        result
    }

    pub(super) fn infer_expr(&mut self, expr_node: &SyntaxNode, expected: Option<&Ty>) -> Ty {
        self.with_guard(expr_node, |ctx| ctx.infer_expr_inner(expr_node, expected))
    }

    fn infer_local(&mut self, binding_node: &SyntaxNode) -> Ty {
        let target = if binding_node.kind() == NAME {
            match binding_node.parent() {
                Some(p) => p,
                None => return Ty::Unknown,
            }
        } else {
            binding_node.clone()
        };
        self.with_guard(&target, |ctx| ctx.infer_local_inner(&target))
    }

    fn infer_expr_inner(&mut self, expr: &SyntaxNode, expected: Option<&Ty>) -> Ty {
        match expr.kind() {
            LITERAL => infer_literal_ty(expr, self.effective_scope_at(expr)),
            CLASS_LITERAL => self.infer_class_literal(expr),
            THIS_EXPR => self.infer_this_expr(expr),
            SUPER_EXPR => self.infer_super_expr(expr),
            NAME_REF_EXPR => self.infer_name_ref_expr(expr),
            FIELD_ACCESS_EXPR => self.infer_field_access_expr(expr),
            METHOD_CALL_EXPR => self
                .infer_method_call(expr, expected)
                .map_or(Ty::Unknown, |c| c.return_ty),
            CALL_EXPR => self.infer_call_expr(expr, expected),
            NEW_EXPR => self.infer_new_expr(expr, expected),
            ARRAY_ACCESS_EXPR => {
                let Some(arr_expr) = expr.children().next() else {
                    return Ty::Unknown;
                };
                match self.infer_expr(&arr_expr, None) {
                    Ty::Array { elem, .. } => *elem,
                    _ => Ty::Unknown,
                }
            }
            PAREN_EXPR => {
                let Some(inner) = expr.children().next() else {
                    return Ty::Unknown;
                };
                self.infer_expr(&inner, expected)
            }
            CAST_EXPR => self.infer_cast_expr(expr),
            INSTANCEOF_EXPR => Ty::Primitive(PrimitiveType::Boolean),
            ASSIGN_EXPR => {
                let Some(lhs) = expr.children().next() else {
                    return Ty::Unknown;
                };
                self.infer_expr(&lhs, None)
            }
            UNARY_EXPR => self.infer_unary_expr(expr),
            POSTFIX_EXPR => {
                let Some(operand) = expr.children().next() else {
                    return Ty::Unknown;
                };
                self.infer_expr(&operand, None)
            }
            BINARY_EXPR => self.infer_binary_expr(expr),
            TERNARY_EXPR => self.infer_ternary_expr(expr, expected),
            SWITCH_EXPR => self.infer_switch_expr(expr, expected),
            LAMBDA_EXPR | METHOD_REF_EXPR => {
                if let Some(exp) = expected {
                    exp.clone()
                } else {
                    self.expected_target_type(expr).unwrap_or(Ty::Unknown)
                }
            }
            _ => Ty::Unknown,
        }
    }

    fn infer_class_literal(&self, expr: &SyntaxNode) -> Ty {
        let inner_ty = if let Some(type_node) = expr.children().find(|n| is_type_node(n.kind())) {
            if has_direct_token(&type_node, VOID_KW) {
                Ty::Void
            } else {
                self.resolve_type_node(&type_node, expr)
            }
        } else if let Some(chain_node) = expr.children().next() {
            if let Some(segs) = resolve::extract_dotted_name_chain(&chain_node) {
                let raw = segs.iter().map(SmolStr::as_str).collect::<Vec<_>>().join(".");
                self.resolve_type_str(&raw, expr)
            } else {
                Ty::Unknown
            }
        } else {
            Ty::Unknown
        };

        let is_marked = matches!(self.effective_scope_at(expr), NullnessScope::NonNullByDefault(_));
        let boxed = match inner_ty {
            Ty::Primitive(p) => {
                let b = Ty::raw_class(p.boxed_fqn());
                if is_marked {
                    b.with_nullability(Nullability::NonNull)
                } else {
                    b
                }
            }
            Ty::Void => {
                let v = Ty::raw_class("java.lang.Void");
                if is_marked {
                    v.with_nullability(Nullability::NonNull)
                } else {
                    v
                }
            }
            Ty::Unknown => {
                let c = Ty::raw_class(CLASS_FQN);
                return if is_marked {
                    c.with_nullability(Nullability::NonNull)
                } else {
                    c
                };
            }
            other => other,
        };

        let cls = Ty::class(CLASS_FQN, vec![crate::ty::TypeArg::Type(boxed)]);
        if is_marked {
            cls.with_nullability(Nullability::NonNull)
        } else {
            cls
        }
    }

    fn infer_this_expr(&self, expr: &SyntaxNode) -> Ty {
        let is_marked = matches!(self.effective_scope_at(expr), NullnessScope::NonNullByDefault(_));
        let mark = |ty: Ty| {
            if is_marked {
                ty.with_nullability(Nullability::NonNull)
            } else {
                ty
            }
        };
        if let Some(qual) = expr.children().next()
            && let Some(segs) = resolve::extract_dotted_name_chain(&qual)
        {
            let raw = segs.iter().map(SmolStr::as_str).collect::<Vec<_>>().join(".");
            let ty = self.resolve_type_str(&raw, expr);
            if let Ty::Class { fqn, .. } = ty {
                return mark(self.parameterized_self_ty(&fqn));
            }
            return mark(ty);
        }

        for ancestor in expr.ancestors().skip(1) {
            if ancestor.kind() == NEW_EXPR
                && ancestor.children().any(|n| n.kind() == CLASS_BODY)
                && let Some(path_type) = ancestor.children().find(|n| n.kind() == PATH_TYPE)
            {
                return mark(self.resolve_type_node(&path_type, &ancestor));
            }
            if TypeKind::from_syntax(ancestor.kind()).is_some() {
                if let Some(fqn) = self.type_fqn_for_decl(&ancestor) {
                    return mark(self.parameterized_self_ty(&fqn));
                }
                break;
            }
        }

        Ty::Unknown
    }

    fn infer_super_expr(&self, expr: &SyntaxNode) -> Ty {
        let is_marked = matches!(self.effective_scope_at(expr), NullnessScope::NonNullByDefault(_));
        let mark = |ty: Ty| {
            if is_marked {
                ty.with_nullability(Nullability::NonNull)
            } else {
                ty
            }
        };
        if let Some(qual) = expr.children().next()
            && let Some(segs) = resolve::extract_dotted_name_chain(&qual)
        {
            let raw = segs.iter().map(SmolStr::as_str).collect::<Vec<_>>().join(".");
            let qual_ty = self.resolve_type_str(&raw, expr);
            if let Ty::Class { fqn, .. } = &qual_ty {
                let self_ty = self.parameterized_self_ty(fqn);
                if self.is_interface_fqn(fqn) {
                    return mark(
                        supertype_substitution_for_ty(self.db, &self.infer_enclosing_self_ty(expr), fqn)
                            .map_or(self_ty, |subst| subst.apply_covariant(&self.parameterized_self_ty(fqn))),
                    );
                }
                return mark(self.direct_superclass_ty(&self_ty).unwrap_or(Ty::Unknown));
            }
            return Ty::Unknown;
        }

        for ancestor in expr.ancestors().skip(1) {
            if ancestor.kind() == NEW_EXPR
                && ancestor.children().any(|n| n.kind() == CLASS_BODY)
                && let Some(path_type) = ancestor.children().find(|n| n.kind() == PATH_TYPE)
            {
                return mark(self.resolve_type_node(&path_type, &ancestor));
            }
            if TypeKind::from_syntax(ancestor.kind()).is_some() {
                if let Some(fqn) = self.type_fqn_for_decl(&ancestor) {
                    let self_ty = self.parameterized_self_ty(&fqn);
                    return mark(self.direct_superclass_ty(&self_ty).unwrap_or(Ty::Unknown));
                }
                break;
            }
        }

        Ty::Unknown
    }

    fn infer_name_ref_expr(&mut self, expr: &SyntaxNode) -> Ty {
        let Some(ref_node) = expr.children().find(|n| n.kind() == NAME_REF) else {
            return Ty::Unknown;
        };
        let name = node_ident_text(&ref_node);
        let site_scope = self.effective_scope_at(expr);

        // 1. Local variables, parameters, lambda params, foreach, catch, resource, pattern vars.
        if let Some(decl_node) = resolve::find_local_decl_node(&ref_node, &name) {
            return self.infer_local(&decl_node);
        }

        // 2. Enclosing types' fields, record components, enum constants (with supertype substitution).
        for enc_node in expr.ancestors().skip(1) {
            if enc_node.kind() == NEW_EXPR
                && enc_node.children().any(|n| n.kind() == CLASS_BODY)
                && let Some(path_type) = enc_node.children().find(|n| n.kind() == PATH_TYPE)
            {
                let base_ty = self.resolve_type_node(&path_type, &enc_node);
                if let Some(ty) = self.lookup_field_ty_on_ty(&base_ty, &name, site_scope) {
                    return ty;
                }
            }
            if TypeKind::from_syntax(enc_node.kind()).is_some()
                && let Some(fqn) = self.type_fqn_for_decl(&enc_node)
            {
                let self_ty = self.parameterized_self_ty(&fqn);
                if let Some(ty) = self.lookup_field_ty_on_ty(&self_ty, &name, site_scope) {
                    return ty;
                }
            }
        }

        // 3. Top-level compact source fields.
        if let Some(&mid) = self.file_tree.top_level_members.iter().find(|&&mid| {
            let m = &self.file_tree.members[mid];
            m.kind != MemberKind::Method && m.name == name
        }) {
            let m = &self.file_tree.members[mid];
            if let Some(raw) = m.type_ref.as_deref() {
                return resolve_type_ref_in_file_for_member(
                    self.db,
                    self.file_id,
                    None,
                    m.nullness_scope,
                    raw,
                    &BTreeMap::new(),
                );
            }
        }

        // 4. Static imports (fields / enum constants / nested types).
        if let Some(target) = resolve::resolve_via_static_imports_pub(self.resolve_ctx(), &name, None, false) {
            return self.target_ty_with_site_scope(&target, &Substitution::default(), site_scope);
        }

        // 5. Type reference (e.g., `Collections` in `Collections.emptyList()` or `User` in `User.FOO`).
        self.resolve_type_str(&name, expr)
    }

    fn infer_field_access_expr(&mut self, expr: &SyntaxNode) -> Ty {
        if let Some(chain) = resolve::extract_dotted_name_chain(expr) {
            let fqn = chain.iter().map(SmolStr::as_str).collect::<Vec<_>>().join(".");
            if self.is_known_fqn(&fqn) {
                return Ty::raw_class(fqn);
            }
        }

        let Some(ref_node) = expr.children().find(|n| n.kind() == NAME_REF) else {
            return Ty::Unknown;
        };
        let Some(receiver) = expr.children().next() else {
            return Ty::Unknown;
        };
        let member_name = node_ident_text(&ref_node);
        let recv_ty = self.infer_expr(&receiver, None);
        let site_scope = self.effective_scope_at(expr);

        if matches!(recv_ty, Ty::Array { .. }) && member_name == "length" {
            return Ty::Primitive(PrimitiveType::Int);
        }

        self.lookup_field_ty_on_ty(&recv_ty, &member_name, site_scope)
            .unwrap_or(Ty::Unknown)
    }

    fn infer_call_expr(&mut self, expr: &SyntaxNode, expected: Option<&Ty>) -> Ty {
        let Some(callee) = expr.children().next() else {
            return Ty::Unknown;
        };
        if matches!(callee.kind(), THIS_EXPR | SUPER_EXPR) {
            return Ty::Void;
        }
        self.infer_unqualified_call(expr, expected)
            .map_or(Ty::Unknown, |c| c.return_ty)
    }

    pub(super) fn infer_cast_expr(&self, expr: &SyntaxNode) -> Ty {
        let type_nodes: Vec<SyntaxNode> = expr
            .children()
            .filter(|n| is_type_node(n.kind()))
            .collect();
        match type_nodes.as_slice() {
            [] => Ty::Unknown,
            [single] => self.resolve_type_node(single, expr),
            many => Ty::Intersection(
                many.iter()
                    .map(|n| self.resolve_type_node(n, expr))
                    .collect(),
            ),
        }
    }

    fn infer_unary_expr(&mut self, expr: &SyntaxNode) -> Ty {
        if has_direct_token(expr, BANG) {
            return Ty::Primitive(PrimitiveType::Boolean);
        }
        let Some(operand) = expr.children().next() else {
            return Ty::Unknown;
        };
        let op_ty = self.infer_expr(&operand, None);
        if has_direct_token(expr, PLUS2) || has_direct_token(expr, MINUS2) {
            return op_ty;
        }
        unary_numeric_promotion(&op_ty)
            .map(Ty::Primitive)
            .unwrap_or(op_ty)
    }

    fn infer_binary_expr(&mut self, expr: &SyntaxNode) -> Ty {
        let mut children = expr.children();
        let Some(lhs) = children.next() else {
            return Ty::Unknown;
        };
        let Some(rhs) = children.next() else {
            return Ty::Unknown;
        };

        let op_kind = expr
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| {
                matches!(
                    t.kind(),
                    EQ2 | NEQ
                        | L_ANGLE
                        | R_ANGLE
                        | LTEQ
                        | GTEQ
                        | AMP2
                        | PIPE2
                        | PLUS
                        | MINUS
                        | STAR
                        | SLASH
                        | PERCENT
                        | SHL
                        | SHR
                        | USHR
                        | AMP
                        | PIPE
                        | CARET
                )
            })
            .map(|t| t.kind());

        match op_kind {
            Some(EQ2 | NEQ | L_ANGLE | R_ANGLE | LTEQ | GTEQ | AMP2 | PIPE2) => {
                Ty::Primitive(PrimitiveType::Boolean)
            }
            Some(SHL | SHR | USHR) => {
                let lty = self.infer_expr(&lhs, None);
                unary_numeric_promotion(&lty)
                    .map(Ty::Primitive)
                    .unwrap_or(lty)
            }
            Some(PLUS) => {
                let lty = self.infer_expr(&lhs, None);
                let rty = self.infer_expr(&rhs, None);
                if is_string_ty(&lty) || is_string_ty(&rty) {
                    let s = Ty::string();
                    return if matches!(self.effective_scope_at(expr), NullnessScope::NonNullByDefault(_)) {
                        s.with_nullability(Nullability::NonNull)
                    } else {
                        s
                    };
                }
                binary_numeric_promotion(&lty, &rty)
                    .map(Ty::Primitive)
                    .unwrap_or(Ty::Unknown)
            }
            Some(MINUS | STAR | SLASH | PERCENT) => {
                let lty = self.infer_expr(&lhs, None);
                let rty = self.infer_expr(&rhs, None);
                binary_numeric_promotion(&lty, &rty)
                    .map(Ty::Primitive)
                    .unwrap_or(Ty::Unknown)
            }
            Some(AMP | PIPE | CARET) => {
                let lty = self.infer_expr(&lhs, None);
                let rty = self.infer_expr(&rhs, None);
                if is_boolean_ty(&lty) && is_boolean_ty(&rty) {
                    Ty::Primitive(PrimitiveType::Boolean)
                } else {
                    binary_numeric_promotion(&lty, &rty)
                        .map(Ty::Primitive)
                        .unwrap_or(Ty::Unknown)
                }
            }
            _ => Ty::Unknown,
        }
    }

    fn infer_ternary_expr(&mut self, expr: &SyntaxNode, expected: Option<&Ty>) -> Ty {
        let mut children = expr.children();
        let _cond = children.next();
        let Some(then_expr) = children.next() else {
            return Ty::Unknown;
        };
        let Some(else_expr) = children.next() else {
            return self.infer_expr(&then_expr, expected);
        };
        let a = self.infer_expr(&then_expr, expected);
        let b = self.infer_expr(&else_expr, expected);
        match (&a, &b) {
            (Ty::Unknown, _) => b,
            (_, Ty::Unknown) => a,
            _ => least_upper_bound(self.db, &a, &b),
        }
    }

    fn infer_switch_expr(&mut self, expr: &SyntaxNode, expected: Option<&Ty>) -> Ty {
        let Some(body) = expr.children().find(|n| n.kind() == SWITCH_BODY) else {
            return Ty::Unknown;
        };
        let mut branch_exprs = Vec::new();
        for case_node in body.children().filter(|n| n.kind() == SWITCH_CASE) {
            let is_arrow = case_node
                .children()
                .find(|n| n.kind() == SWITCH_LABEL)
                .is_some_and(|l| has_direct_token(&l, ARROW));
            if is_arrow {
                for child in case_node.children() {
                    match child.kind() {
                        EXPR_STMT => {
                            if let Some(e) = child.children().next() {
                                branch_exprs.push(e);
                            }
                        }
                        BLOCK => collect_yield_exprs(&child, &mut branch_exprs),
                        _ => {}
                    }
                }
            } else {
                collect_yield_exprs(&case_node, &mut branch_exprs);
            }
        }

        let mut acc: Option<Ty> = None;
        for b_expr in branch_exprs {
            let ty = self.infer_expr(&b_expr, expected);
            if matches!(ty, Ty::Unknown) {
                continue;
            }
            acc = Some(match acc {
                None => ty,
                Some(prev) => least_upper_bound(self.db, &prev, &ty),
            });
        }
        acc.unwrap_or(Ty::Unknown)
    }

    // ---- Local variable & binding inference ----

    fn infer_local_inner(&mut self, binding: &SyntaxNode) -> Ty {
        match binding.kind() {
            VARIABLE_DECLARATOR => {
                let Some(decl_parent) = binding.parent() else {
                    return Ty::Unknown;
                };
                if !matches!(decl_parent.kind(), LOCAL_VAR_DECL | FIELD_DECL) {
                    return Ty::Unknown;
                }
                let type_node = decl_parent.children().find(|n| is_type_node(n.kind()));
                if let Some(tn) = type_node
                    && !is_var_type_node(&tn)
                {
                    let is_local_root = decl_parent.kind() == LOCAL_VAR_DECL;
                    return self.resolve_explicit_type_node(
                        &tn,
                        binding,
                        &decl_parent,
                        is_local_root,
                    );
                }
                // `var x = init;`
                let init = binding.children().find(|n| n.kind() != NAME);
                if let Some(init_expr) = init {
                    let ty = self.infer_expr(&init_expr, None);
                    if matches!(ty, Ty::Null) {
                        return Ty::Unknown;
                    }
                    return ty;
                }
                Ty::Unknown
            }
            FOREACH_STMT => {
                let type_node = binding.children().find(|n| is_type_node(n.kind()));
                if let Some(tn) = type_node
                    && !is_var_type_node(&tn)
                {
                    return self.resolve_explicit_type_node(&tn, binding, binding, true);
                }
                let colon_end = binding
                    .children_with_tokens()
                    .find(|e| e.as_token().is_some_and(|t| t.kind() == COLON))
                    .map(|e| e.text_range().end());
                let iterable_expr = binding.children().find(|n| {
                    colon_end.is_some_and(|end| n.text_range().start() >= end)
                        && !matches!(n.kind(), BLOCK | EXPR_STMT | LOCAL_VAR_DECL)
                });
                let Some(iter_expr) = iterable_expr else {
                    return Ty::Unknown;
                };
                let iter_ty = self.infer_expr(&iter_expr, None);
                match iter_ty {
                    Ty::Array { elem, .. } => *elem,
                    other => {
                        let Some(subst) =
                            supertype_substitution_for_ty(self.db, &other, ITERABLE_FQN)
                        else {
                            return Ty::Unknown;
                        };
                        let tv_name = type_params_of_fqn(self.db, ITERABLE_FQN)
                            .into_iter()
                            .next()
                            .unwrap_or_else(|| SmolStr::new("T"));
                        if subst.map.is_empty() {
                            Ty::object()
                        } else {
                            subst.apply_covariant(&Ty::type_var(tv_name, Vec::new()))
                        }
                    }
                }
            }
            RESOURCE => {
                let type_node = binding.children().find(|n| is_type_node(n.kind()));
                if let Some(tn) = type_node
                    && !is_var_type_node(&tn)
                {
                    return self.resolve_explicit_type_node(&tn, binding, binding, true);
                }
                let eq_end = binding
                    .children_with_tokens()
                    .find(|e| e.as_token().is_some_and(|t| t.kind() == EQ))
                    .map(|e| e.text_range().end());
                let init_expr = binding
                    .children()
                    .find(|n| eq_end.is_some_and(|end| n.text_range().start() >= end));
                init_expr
                    .map(|e| self.infer_expr(&e, None))
                    .unwrap_or(Ty::Unknown)
            }
            PARAM => {
                if let Some(type_node) = binding.children().find(|n| is_type_node(n.kind()))
                    && !is_var_type_node(&type_node)
                {
                    let is_lambda_param =
                        binding.parent().is_some_and(|p| p.kind() == LAMBDA_PARAMS);
                    let is_varargs = has_direct_token(binding, DOT3);
                    let mod_null = extract_modifier_nullability(binding, &self.import_resolver);
                    let (raw, decl_null) = render_type_with_dims_and_nullability(
                        &type_node,
                        binding,
                        mod_null,
                        is_varargs,
                        &self.import_resolver,
                    );
                    let base =
                        self.resolve_type_str_with_local(&raw, binding, is_lambda_param);
                    if is_varargs && !matches!(base, Ty::Unknown | Ty::Void) {
                        let arr = Ty::array(base);
                        let eff_scope = self.effective_scope_at(binding);
                        return if let Some((n, _)) = decl_null {
                            arr.with_nullability(n)
                        } else if matches!(eff_scope, NullnessScope::NonNullByDefault(_)) {
                            arr.with_nullability(Nullability::NonNull)
                        } else {
                            arr
                        };
                    }
                    return base;
                }
                if let Some(bound) = self.lambda_param_bindings.get(&binding.text_range()) {
                    return bound.clone();
                }
                let Some(params_node) = binding.parent().filter(|p| p.kind() == LAMBDA_PARAMS)
                else {
                    return Ty::Unknown;
                };
                let Some(lambda_expr) = params_node.parent().filter(|p| p.kind() == LAMBDA_EXPR)
                else {
                    return Ty::Unknown;
                };
                let param_idx = params_node
                    .children()
                    .filter(|n| n.kind() == PARAM)
                    .position(|n| n.text_range() == binding.text_range());
                let Some(idx) = param_idx else {
                    return Ty::Unknown;
                };
                let Some(target_ty) = self.expected_target_type(&lambda_expr) else {
                    return Ty::Unknown;
                };
                let Some(sam) = find_functional_interface_sam(self.db, &target_ty) else {
                    return Ty::Unknown;
                };
                sam.param_types.get(idx).cloned().unwrap_or(Ty::Unknown)
            }
            CATCH_CLAUSE => {
                let types: Vec<Ty> = binding
                    .children()
                    .filter(|n| is_type_node(n.kind()))
                    .map(|t| self.resolve_type_node(&t, binding))
                    .filter(|t| !matches!(t, Ty::Unknown))
                    .collect();
                let mut acc: Option<Ty> = None;
                for t in types {
                    acc = Some(match acc {
                        None => t,
                        Some(prev) => least_upper_bound(self.db, &prev, &t),
                    });
                }
                acc.unwrap_or(Ty::Unknown)
            }
            TYPE_PATTERN => {
                if let Some(type_node) = binding.children().find(|n| is_type_node(n.kind()))
                    && !is_var_type_node(&type_node)
                {
                    return self.resolve_explicit_type_node(&type_node, binding, binding, true);
                }
                self.infer_record_pattern_component(binding)
            }
            RECORD_COMPONENT => {
                let Some(type_node) = binding.children().find(|n| is_type_node(n.kind())) else {
                    return Ty::Unknown;
                };
                self.resolve_explicit_type_node(&type_node, binding, binding, false)
            }
            _ => Ty::Unknown,
        }
    }

    fn infer_record_pattern_component(&mut self, pattern_node: &SyntaxNode) -> Ty {
        let Some(rec_pat) = pattern_node.parent().filter(|p| p.kind() == RECORD_PATTERN) else {
            return Ty::Unknown;
        };
        let Some(comp_idx) = rec_pat
            .children()
            .filter(|n| matches!(n.kind(), TYPE_PATTERN | RECORD_PATTERN | UNNAMED_PATTERN))
            .position(|n| n.text_range() == pattern_node.text_range())
        else {
            return Ty::Unknown;
        };
        let Some(rec_type_node) = rec_pat.children().find(|n| is_type_node(n.kind())) else {
            return Ty::Unknown;
        };
        let rec_ty = self.resolve_type_node(&rec_type_node, &rec_pat);
        let Some(rec_fqn) = rec_ty.erased_fqn() else {
            return Ty::Unknown;
        };
        let subst = supertype_substitution_for_ty(self.db, &rec_ty, rec_fqn).unwrap_or_default();

        if let Some((fid, tid)) = self.index.get_type(rec_fqn) {
            let tree = self.db.file_item_tree(fid);
            let comps: Vec<&MemberItem> = tree.types[tid]
                .members
                .iter()
                .map(|&mid| &tree.members[mid])
                .filter(|m| m.kind == MemberKind::RecordComponent)
                .collect();
            if let Some(comp) = comps.get(comp_idx)
                && let Some(raw) = comp.type_ref.as_deref()
            {
                let scope = type_param_bounds_of_fqn(self.db, rec_fqn);
                let ty = resolve_type_ref_in_file_for_member(
                    self.db,
                    fid,
                    Some(rec_fqn),
                    comp.nullness_scope,
                    raw,
                    &scope,
                );
                return subst.apply_covariant(&ty);
            }
        } else if let Some(bin) = self.classpath.type_by_fqn(rec_fqn) {
            let comps: Vec<&BinaryMemberSymbol> = bin
                .members
                .iter()
                .filter(|m| m.kind == MemberKind::RecordComponent)
                .collect();
            if let Some(comp) = comps.get(comp_idx)
                && let Some(raw) = comp.type_ref.as_deref()
            {
                let scope = type_param_bounds_of_fqn(self.db, rec_fqn);
                let ty = resolve_binary_type_ref_for_member(
                    &self.classpath,
                    bin,
                    comp.nullness_scope,
                    raw,
                    &scope,
                );
                return subst.apply_covariant(&ty);
            }
        }
        Ty::Unknown
    }
}

fn infer_literal_ty(literal: &SyntaxNode, site_scope: NullnessScope) -> Ty {
    let Some(tok) = literal
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia())
    else {
        return Ty::Unknown;
    };
    match tok.kind() {
        INT_NUMBER => Ty::Primitive(PrimitiveType::Int),
        LONG_NUMBER => Ty::Primitive(PrimitiveType::Long),
        FLOAT_NUMBER => Ty::Primitive(PrimitiveType::Float),
        DOUBLE_NUMBER => Ty::Primitive(PrimitiveType::Double),
        CHAR_LITERAL => Ty::Primitive(PrimitiveType::Char),
        STRING_LITERAL | TEXT_BLOCK => {
            let s = Ty::string();
            if matches!(site_scope, NullnessScope::NonNullByDefault(_)) {
                s.with_nullability(Nullability::NonNull)
            } else {
                s
            }
        }
        TRUE_KW | FALSE_KW => Ty::Primitive(PrimitiveType::Boolean),
        NULL_KW => Ty::Null,
        _ => Ty::Unknown,
    }
}

pub(super) fn is_var_type_node(type_node: &SyntaxNode) -> bool {
    type_node.kind() == PATH_TYPE && collapsed_text(type_node) == "var"
}

fn is_string_ty(ty: &Ty) -> bool {
    matches!(ty, Ty::Class { fqn, .. } if fqn == STRING_FQN)
}

fn is_boolean_ty(ty: &Ty) -> bool {
    matches!(ty, Ty::Primitive(PrimitiveType::Boolean))
        || matches!(ty, Ty::Class { fqn, type_args, .. } if type_args.is_empty() && fqn == "java.lang.Boolean")
}

fn collect_yield_exprs(node: &SyntaxNode, out: &mut Vec<SyntaxNode>) {
    for child in node.children() {
        match child.kind() {
            YIELD_STMT => {
                if let Some(e) = child.children().next() {
                    out.push(e);
                }
            }
            SWITCH_EXPR | SWITCH_STMT | LAMBDA_EXPR => {}
            k if TypeKind::from_syntax(k).is_some() => {}
            _ => collect_yield_exprs(&child, out),
        }
    }
}
