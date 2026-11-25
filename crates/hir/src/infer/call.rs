use std::collections::BTreeMap;

use rustc_hash::FxHashSet;
use smol_str::SmolStr;
use syntax::{
    SyntaxNode,
    ext::{has_direct_token, is_type_node, path_segments},
    syntax_kind::SyntaxKind::*,
};

use crate::{
    resolve::{self, ResolvedTarget},
    symbols::{NullnessScope, TypeKind, node_ident_text},
    ty::{
        Nullability, Substitution, Ty, TypeArg, WildcardBound, effective_nullness_scope_for_file,
        effective_nullness_scope_for_member_in_scope, is_subtype_in_scope,
        least_upper_bound_in_scope, supertype_substitution_for_ty_in_scope,
        supertype_substitution_in_scope, type_params_of_fqn_in_scope,
    },
};

use super::{
    InferCtx, MAX_INFER_DEPTH, is_var_type_node,
    sam::{SamSignature, erase_type_vars_to_bound, find_functional_interface_sam_in_scope},
};

/// Details of a resolved call expression, used by both type inference and hover/resolution.
#[derive(Debug, Clone)]
pub(super) struct CallInference {
    pub(super) target: ResolvedTarget,
    pub(super) subst: Substitution,
    pub(super) return_ty: Ty,
    pub(super) resolved_params: Vec<(Ty, bool)>,
}

impl<'a> InferCtx<'a> {
    pub(super) fn infer_method_call(
        &mut self,
        method_call: &SyntaxNode,
        expected: Option<&Ty>,
    ) -> Option<CallInference> {
        let receiver = method_call.children().next()?;
        let ref_node = method_call.children().find(|n| n.kind() == NAME_REF)?;
        let member_name = node_ident_text(&ref_node);
        let recv_ty = self.infer_expr(&receiver, None);
        if matches!(recv_ty, Ty::Unknown) {
            return None;
        }

        let arg_nodes = collect_arg_nodes(method_call);
        let pre_arg_tys = self.infer_non_functional_args(&arg_nodes);
        let candidates = resolve::collect_method_candidates_on_ty(
            self.resolve_ctx(),
            &recv_ty,
            &member_name,
            arg_nodes.len(),
        );
        if candidates.is_empty() {
            return None;
        }

        let winner = self.select_best_candidate(candidates, &pre_arg_tys, &arg_nodes)?;
        let explicit_targs = self.extract_explicit_type_args(method_call);
        Some(self.finish_call_inference(winner, &arg_nodes, &pre_arg_tys, &explicit_targs, expected))
    }

    pub(super) fn infer_unqualified_call(
        &mut self,
        call_expr: &SyntaxNode,
        expected: Option<&Ty>,
    ) -> Option<CallInference> {
        let callee = call_expr.children().next()?;
        if callee.kind() != NAME_REF_EXPR {
            return None;
        }
        let ref_node = callee.children().find(|n| n.kind() == NAME_REF)?;
        let member_name = node_ident_text(&ref_node);

        let arg_nodes = collect_arg_nodes(call_expr);
        let pre_arg_tys = self.infer_non_functional_args(&arg_nodes);

        let mut candidates = Vec::new();
        for enc_node in call_expr.ancestors().skip(1) {
            if enc_node.kind() == NEW_EXPR
                && enc_node.children().any(|n| n.kind() == CLASS_BODY)
                && let Some(path_type) = enc_node.children().find(|n| n.kind() == PATH_TYPE)
            {
                let base_ty = self.resolve_type_node(&path_type, &enc_node);
                candidates.extend(resolve::collect_method_candidates_on_ty(
                    self.resolve_ctx(),
                    &base_ty,
                    &member_name,
                    arg_nodes.len(),
                ));
            }
            if TypeKind::from_syntax(enc_node.kind()).is_some()
                && let Some(fqn) = self.type_fqn_for_decl(&enc_node)
            {
                let self_ty = self.parameterized_self_ty(&fqn);
                candidates.extend(resolve::collect_method_candidates_on_ty(
                    self.resolve_ctx(),
                    &self_ty,
                    &member_name,
                    arg_nodes.len(),
                ));
            }
        }

        if candidates.is_empty() {
            candidates.extend(resolve::collect_top_level_method_candidates(
                self.resolve_ctx(),
                &member_name,
                arg_nodes.len(),
            ));
        }

        if candidates.is_empty() {
            candidates.extend(resolve::collect_static_import_method_candidates(
                self.resolve_ctx(),
                &member_name,
                arg_nodes.len(),
            ));
        }

        if candidates.is_empty() {
            return None;
        }

        let winner = self.select_best_candidate(candidates, &pre_arg_tys, &arg_nodes)?;
        let explicit_targs = self.extract_explicit_type_args(call_expr);
        Some(self.finish_call_inference(winner, &arg_nodes, &pre_arg_tys, &explicit_targs, expected))
    }

    fn finish_call_inference(
        &mut self,
        cand: resolve::CallableCandidate,
        arg_nodes: &[SyntaxNode],
        pre_arg_tys: &[Ty],
        explicit_targs: &[TypeArg],
        expected: Option<&Ty>,
    ) -> CallInference {
        let mut subst = cand.owner_subst.clone();
        // Method type parameters shadow any class type parameters with the same name.
        for mp in &cand.method_type_params {
            subst.map.remove(mp);
        }

        // Bind explicit type witnesses if present (e.g. `Collections.<String>emptyList()`).
        for (mp, arg) in cand.method_type_params.iter().zip(explicit_targs.iter()) {
            subst.map.insert(mp.clone(), arg.clone());
        }

        let method_tvar_set: FxHashSet<SmolStr> = cand
            .method_type_params
            .iter()
            .filter(|mp| !subst.map.contains_key(*mp))
            .cloned()
            .collect();

        if !method_tvar_set.is_empty() {
            let mut inferred = Substitution::default();

            // 1. Unify non-lambda, non-method-ref arguments against formal parameter types.
            for (idx, arg_node) in arg_nodes.iter().enumerate() {
                if matches!(arg_node.kind(), LAMBDA_EXPR | METHOD_REF_EXPR) {
                    continue;
                }
                let Some(arg_ty) = pre_arg_tys.get(idx) else {
                    continue;
                };
                if matches!(arg_ty, Ty::Unknown | Ty::Null) {
                    continue;
                }
                if let Some((pattern_ty, is_varargs)) = parameter_for_arg(&cand.param_tys, idx) {
                    let partial_pattern = subst.apply(&pattern_ty);
                    let target_pattern = if is_varargs
                        && idx + 1 == cand.param_tys.len()
                        && arg_nodes.len() == cand.param_tys.len()
                        && matches!(arg_ty, Ty::Array { .. })
                    {
                        partial_pattern
                    } else if is_varargs {
                        match partial_pattern {
                            Ty::Array { elem, .. } => *elem,
                            other => other,
                        }
                    } else {
                        partial_pattern
                    };
                    self.unify(&target_pattern, arg_ty, &method_tvar_set, &mut inferred, 0);
                }
            }

            // Merge non-lambda bindings into `subst` so lambda parameter SAM types see them!
            for (k, v) in &inferred.map {
                subst.map.entry(k.clone()).or_insert_with(|| v.clone());
            }

            // 2. Unify lambda & method-reference arguments via their SAM return types.
            for (idx, arg_node) in arg_nodes.iter().enumerate() {
                if !matches!(arg_node.kind(), LAMBDA_EXPR | METHOD_REF_EXPR) {
                    continue;
                }
                let Some((param_ty, is_varargs)) = parameter_for_arg(&cand.param_tys, idx) else {
                    continue;
                };
                let target_iface = if is_varargs {
                    match subst.apply(&param_ty) {
                        Ty::Array { elem, .. } => *elem,
                        other => other,
                    }
                } else {
                    subst.apply(&param_ty)
                };
                let Some(sam) =
                    find_functional_interface_sam_in_scope(self.db, &self.scope, &target_iface)
                else {
                    continue;
                };
                if matches!(sam.return_type, Ty::Void | Ty::Unknown) {
                    continue;
                }
                let body_ret_ty = match arg_node.kind() {
                    LAMBDA_EXPR => self.infer_lambda_return_ty(arg_node, &sam),
                    METHOD_REF_EXPR => self.infer_method_ref_return_ty(arg_node, &sam),
                    _ => Ty::Unknown,
                };
                if !matches!(body_ret_ty, Ty::Unknown | Ty::Void | Ty::Null) {
                    self.unify(
                        &sam.return_type,
                        &body_ret_ty,
                        &method_tvar_set,
                        &mut inferred,
                        0,
                    );
                }
            }

            for (k, v) in &inferred.map {
                subst.map.entry(k.clone()).or_insert_with(|| v.clone());
            }

            // 3. Target-type unification against `expected` (e.g., `List<String> x = Collections.emptyList()`).
            if let Some(exp) = expected {
                let partial_ret = subst.apply(&cand.return_ty);
                let mut exp_inferred = Substitution::default();
                self.unify(&partial_ret, exp, &method_tvar_set, &mut exp_inferred, 0);
                for (k, v) in exp_inferred.map {
                    subst.map.entry(k).or_insert(v);
                }
            }

            // 4. Default any remaining unbound method type parameters to their upper bound or Object.
            let eff_member_scope = if cand.owner_fqn.is_empty() {
                if cand.member_nullness_scope != NullnessScope::Inherited {
                    cand.member_nullness_scope
                } else {
                    effective_nullness_scope_for_file(self.db, self.file_id)
                }
            } else {
                effective_nullness_scope_for_member_in_scope(
                    self.db,
                    &self.scope,
                    &cand.owner_fqn,
                    cand.member_nullness_scope,
                )
            };
            let implicit_obj = if matches!(eff_member_scope, NullnessScope::NonNullByDefault(_)) {
                Ty::object().with_nullability(Nullability::NonNull)
            } else {
                Ty::object()
            };
            for mp in &cand.method_type_params {
                if !subst.map.contains_key(mp) {
                    let fallback = cand
                        .method_type_param_bounds
                        .get(mp)
                        .and_then(|b| b.first())
                        .map_or_else(|| implicit_obj.clone(), |raw| {
                            self.resolve_member_type_in_owner(
                                &cand.target,
                                &cand.owner_fqn,
                                raw,
                                &BTreeMap::new(),
                            )
                        });
                    let fallback = if matches!(fallback, Ty::Unknown) {
                        implicit_obj.clone()
                    } else {
                        fallback
                    };
                    subst.insert_ty(mp.clone(), fallback);
                }
            }
        }

        let return_ty = if cand.is_raw_receiver && matches!(cand.return_ty, Ty::TypeVar { .. }) {
            erase_type_vars_to_bound(&cand.return_ty)
        } else if cand.is_raw_receiver {
            erase_type_vars_to_bound(&subst.apply_covariant(&cand.return_ty))
        } else {
            subst.apply_covariant(&cand.return_ty)
        };

        let resolved_params = cand
            .param_tys
            .iter()
            .map(|(p, is_varargs)| {
                let p_ty = if cand.is_raw_receiver {
                    erase_type_vars_to_bound(p)
                } else {
                    subst.apply(p)
                };
                (p_ty, *is_varargs)
            })
            .collect();

        CallInference {
            target: cand.target,
            subst,
            return_ty,
            resolved_params,
        }
    }

    pub(super) fn infer_new_expr(&mut self, expr: &SyntaxNode, expected: Option<&Ty>) -> Ty {
        let Some(type_node) = expr.children().find(|n| is_type_node(n.kind())) else {
            return Ty::Unknown;
        };
        let is_marked = matches!(self.effective_scope_at(expr), NullnessScope::NonNullByDefault(_));

        // Count direct `[` tokens on `NEW_EXPR` (`new int[3][]`, `new String[n]`).
        let new_array_dims = expr
            .children_with_tokens()
            .filter(|el| el.as_token().is_some_and(|t| t.kind() == L_BRACK))
            .count();

        let mut base_ty = if has_direct_token(expr, DOT) && type_node.kind() == PATH_TYPE {
            // Qualified `outer.new Inner<...>(...)`.
            if let Some(outer_expr) = expr.children().next() {
                let outer_ty = self.infer_expr(&outer_expr, None);
                if let Some(outer_fqn) = outer_ty.erased_fqn() {
                    let segs = path_segments(&type_node);
                    let mut fqn = outer_fqn.to_string();
                    for seg in &segs {
                        fqn = format!("{fqn}.{seg}");
                    }
                    let explicit_args = self.extract_path_type_last_args(&type_node, expr);
                    Ty::class(fqn, explicit_args.unwrap_or_default())
                } else {
                    Ty::Unknown
                }
            } else {
                Ty::Unknown
            }
        } else {
            self.resolve_type_node(&type_node, expr)
        };

        if new_array_dims > 0 {
            for _ in 0..new_array_dims {
                if !matches!(base_ty, Ty::Unknown | Ty::Void) {
                    base_ty = Ty::array(base_ty);
                    if is_marked {
                        base_ty = base_ty.with_nullability(Nullability::NonNull);
                    }
                }
            }
            return base_ty;
        }

        // Handle diamond `new ArrayList<>(...)` inference.
        if let Ty::Class { fqn, type_args, .. } = &base_ty
            && type_args.is_empty()
            && is_diamond_new_expr(&type_node)
        {
            let tp_names = type_params_of_fqn_in_scope(self.db, &self.scope, fqn);
            if !tp_names.is_empty() {
                let tv_set: FxHashSet<SmolStr> = tp_names.iter().cloned().collect();
                let pattern_args: Vec<TypeArg> = tp_names
                    .iter()
                    .map(|tp| TypeArg::Type(Ty::type_var(tp.clone(), Vec::new())))
                    .collect();
                let pattern_cls = Ty::class(fqn.clone(), pattern_args);
                let mut inferred = Substitution::default();

                if let Some(exp) = expected {
                    self.unify(&pattern_cls, exp, &tv_set, &mut inferred, 0);
                }

                // Also try constructor argument unification if any type vars remain unbound.
                if inferred.map.len() < tp_names.len() {
                    let arg_nodes = collect_arg_nodes(expr);
                    if !arg_nodes.is_empty() {
                        let pre_arg_tys = self.infer_non_functional_args(&arg_nodes);
                        let ctors = resolve::collect_constructor_candidates(
                            self.resolve_ctx(),
                            fqn,
                            arg_nodes.len(),
                        );
                        if let Some(ctor) = self.select_best_candidate(ctors, &pre_arg_tys, &arg_nodes)
                        {
                            for (idx, arg_ty) in pre_arg_tys.iter().enumerate() {
                                if matches!(arg_ty, Ty::Unknown | Ty::Null) {
                                    continue;
                                }
                                if let Some((p_ty, is_varargs)) =
                                    parameter_for_arg(&ctor.param_tys, idx)
                                {
                                    let target_pat = if is_varargs {
                                        match p_ty {
                                            Ty::Array { elem, .. } => *elem,
                                            other => other,
                                        }
                                    } else {
                                        p_ty
                                    };
                                    self.unify(&target_pat, arg_ty, &tv_set, &mut inferred, 0);
                                }
                            }
                        }
                    }
                }

                if !inferred.map.is_empty() {
                    let final_args = tp_names
                        .iter()
                        .map(|tp| {
                            inferred
                                .map
                                .get(tp)
                                .cloned()
                                .unwrap_or_else(|| TypeArg::Type(Ty::object()))
                        })
                        .collect();
                    let cls = Ty::class(fqn.clone(), final_args);
                    return if is_marked {
                        cls.with_nullability(Nullability::NonNull)
                    } else {
                        cls
                    };
                }
            }
        }

        if is_marked {
            base_ty.with_nullability(Nullability::NonNull)
        } else {
            base_ty
        }
    }

    // ---- Target type derivation ----

    pub(super) fn expected_target_type(&mut self, expr: &SyntaxNode) -> Option<Ty> {
        let mut child = expr.clone();
        let mut parent = expr.parent()?;

        while parent.kind() == PAREN_EXPR {
            child = parent;
            parent = child.parent()?;
        }

        match parent.kind() {
            VARIABLE_DECLARATOR => {
                let decl_parent = parent.parent()?;
                if !matches!(decl_parent.kind(), LOCAL_VAR_DECL | FIELD_DECL) {
                    return None;
                }
                let type_node = decl_parent.children().find(|n| is_type_node(n.kind()))?;
                if is_var_type_node(&type_node) {
                    return None;
                }
                let is_local_root = decl_parent.kind() == LOCAL_VAR_DECL;
                let ty = self.resolve_explicit_type_node(
                    &type_node,
                    &parent,
                    &decl_parent,
                    is_local_root,
                );
                (!matches!(ty, Ty::Unknown)).then_some(ty)
            }
            ASSIGN_EXPR => {
                let lhs = parent.children().next()?;
                if lhs.text_range() == child.text_range() {
                    return None;
                }
                let ty = self.infer_expr(&lhs, None);
                (!matches!(ty, Ty::Unknown)).then_some(ty)
            }
            RETURN_STMT => {
                for ancestor in parent.ancestors().skip(1) {
                    match ancestor.kind() {
                        LAMBDA_EXPR => {
                            let lambda_target = self.expected_target_type(&ancestor)?;
                            let sam = find_functional_interface_sam_in_scope(
                                self.db,
                                &self.scope,
                                &lambda_target,
                            )?;
                            return (!matches!(sam.return_type, Ty::Unknown | Ty::Void))
                                .then_some(sam.return_type);
                        }
                        METHOD_DECL => {
                            let type_node = ancestor.children().find(|n| is_type_node(n.kind()))?;
                            if has_direct_token(&type_node, VOID_KW) {
                                return None;
                            }
                            let ty = self.resolve_explicit_type_node(
                                &type_node,
                                &ancestor,
                                &ancestor,
                                false,
                            );
                            return (!matches!(ty, Ty::Unknown | Ty::Void)).then_some(ty);
                        }
                        CONSTRUCTOR_DECL => return None,
                        k if TypeKind::from_syntax(k).is_some() => return None,
                        _ => {}
                    }
                }
                None
            }
            YIELD_STMT => {
                let switch_expr = parent.ancestors().find(|n| n.kind() == SWITCH_EXPR)?;
                self.expected_target_type(&switch_expr)
            }
            EXPR_STMT => {
                let case_node = parent.parent().filter(|n| n.kind() == SWITCH_CASE)?;
                let switch_expr = case_node.ancestors().find(|n| n.kind() == SWITCH_EXPR)?;
                self.expected_target_type(&switch_expr)
            }
            CAST_EXPR => {
                let ty = self.infer_cast_expr(&parent);
                (!matches!(ty, Ty::Unknown)).then_some(ty)
            }
            TERNARY_EXPR => {
                let cond = parent.children().next()?;
                if cond.text_range() == child.text_range() {
                    return None;
                }
                self.expected_target_type(&parent)
            }
            ARG_LIST => {
                let arg_idx = parent
                    .children()
                    .position(|n| n.text_range() == child.text_range())?;
                let call_node = parent.parent()?;
                match call_node.kind() {
                    METHOD_CALL_EXPR => {
                        let expected_ret = self.expected_target_type(&call_node);
                        let info = self.infer_method_call(&call_node, expected_ret.as_ref())?;
                        parameter_for_arg(&info.resolved_params, arg_idx).map(|(p, _)| p)
                    }
                    CALL_EXPR => {
                        let expected_ret = self.expected_target_type(&call_node);
                        let info =
                            self.infer_unqualified_call(&call_node, expected_ret.as_ref())?;
                        parameter_for_arg(&info.resolved_params, arg_idx).map(|(p, _)| p)
                    }
                    NEW_EXPR => {
                        let type_node = call_node.children().find(|n| is_type_node(n.kind()))?;
                        let constructed_ty = if is_diamond_new_expr(&type_node) {
                            let exp = self.expected_target_type(&call_node);
                            self.infer_new_expr(&call_node, exp.as_ref())
                        } else {
                            self.resolve_type_node(&type_node, &call_node)
                        };
                        let fqn = constructed_ty.erased_fqn()?.to_string();
                        let subst = supertype_substitution_for_ty_in_scope(
                            self.db,
                            &self.scope,
                            &constructed_ty,
                            &fqn,
                        )
                        .unwrap_or_default();
                        let arg_nodes = collect_arg_nodes(&call_node);
                        let pre_arg_tys = self.infer_non_functional_args(&arg_nodes);
                        let ctors = resolve::collect_constructor_candidates(
                            self.resolve_ctx(),
                            &fqn,
                            arg_nodes.len(),
                        );
                        let best = self.select_best_candidate(ctors, &pre_arg_tys, &arg_nodes)?;
                        parameter_for_arg(&best.param_tys, arg_idx)
                            .map(|(p, _)| subst.apply(&p))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    // ---- Lambdas & Method References ----

    fn infer_lambda_return_ty(&mut self, lambda: &SyntaxNode, sam: &SamSignature) -> Ty {
        self.with_lambda_bindings(lambda, sam, |ctx| {
            let Some(body) = lambda.children().last() else {
                return Ty::Unknown;
            };
            if body.kind() == LAMBDA_PARAMS {
                return Ty::Unknown;
            }
            if body.kind() == BLOCK {
                let mut ret_exprs = Vec::new();
                collect_return_exprs(&body, &mut ret_exprs);
                let mut acc: Option<Ty> = None;
                for re in ret_exprs {
                    let t = ctx.infer_expr(&re, None);
                    if matches!(t, Ty::Unknown) {
                        continue;
                    }
                    acc = Some(match acc {
                        None => t,
                        Some(prev) => least_upper_bound_in_scope(ctx.db, &ctx.scope, &prev, &t),
                    });
                }
                acc.unwrap_or(Ty::Unknown)
            } else {
                ctx.infer_expr(&body, None)
            }
        })
    }

    fn with_lambda_bindings<T>(
        &mut self,
        lambda: &SyntaxNode,
        sam: &SamSignature,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let mut inserted = Vec::new();
        if let Some(params_node) = lambda.children().find(|n| n.kind() == LAMBDA_PARAMS) {
            for (idx, param) in params_node
                .children()
                .filter(|n| n.kind() == PARAM)
                .enumerate()
            {
                if let Some(ty) = sam.param_types.get(idx) {
                    let range = param.text_range();
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        self.lambda_param_bindings.entry(range)
                    {
                        e.insert(ty.clone());
                        inserted.push(range);
                    }
                }
            }
        }
        let res = f(self);
        for range in inserted {
            self.lambda_param_bindings.remove(&range);
        }
        res
    }

    fn infer_method_ref_return_ty(&mut self, method_ref: &SyntaxNode, sam: &SamSignature) -> Ty {
        let Some((target, subst)) = self.resolve_method_ref_with_sam(method_ref, sam) else {
            return Ty::Unknown;
        };
        if let ResolvedTarget::IndexedType(fid, tid) = target {
            let fqn = self.db.file_item_tree(fid).types[tid].qualified_name.clone();
            return Ty::raw_class(fqn);
        }
        if let ResolvedTarget::BinaryType(fqn) = target {
            return Ty::raw_class(fqn);
        }
        self.target_ty(&target, &subst)
    }

    pub(super) fn resolve_method_ref_with_sam(
        &mut self,
        method_ref: &SyntaxNode,
        sam: &SamSignature,
    ) -> Option<(ResolvedTarget, Substitution)> {
        let qualifier = method_ref.children().next()?;
        let is_ctor = has_direct_token(method_ref, NEW_KW);

        let (is_type_qual, qual_ty) = self.classify_method_ref_qualifier(&qualifier);
        if matches!(qual_ty, Ty::Unknown) {
            return None;
        }

        if is_ctor {
            if let Ty::Array { .. } = qual_ty {
                return None;
            }
            let fqn = qual_ty.erased_fqn()?.to_string();
            let cands = resolve::collect_constructor_candidates(
                self.resolve_ctx(),
                &fqn,
                sam.param_types.len(),
            );
            let best = self.select_best_candidate_from_tys(cands, &sam.param_types)?;
            let subst =
                supertype_substitution_for_ty_in_scope(self.db, &self.scope, &qual_ty, &fqn)
                    .unwrap_or_default();
            return Some((best.target, subst));
        }

        let ref_node = method_ref.children().find(|n| n.kind() == NAME_REF)?;
        let member_name = node_ident_text(&ref_node);

        if is_type_qual {
            // Try unbound instance method `Type::method` (receiver is `sam.param_types[0]`,
            // remaining args are `sam.param_types[1..]`).
            let mut candidates = Vec::new();
            if let Some(first_param) = sam.param_types.first()
                && (is_subtype_in_scope(self.db, &self.scope, first_param, &qual_ty)
                    || matches!(qual_ty, Ty::Class { .. })
                        && is_subtype_in_scope(self.db, &self.scope, &qual_ty, first_param))
            {
                let rest_args = &sam.param_types[1..];
                let recv_for_lookup = if matches!(qual_ty, Ty::Class { ref type_args, .. } if type_args.is_empty())
                    && !matches!(first_param, Ty::Unknown)
                    && is_subtype_in_scope(self.db, &self.scope, first_param, &qual_ty)
                {
                    first_param
                } else {
                    &qual_ty
                };
                for c in resolve::collect_method_candidates_on_ty(
                    self.resolve_ctx(),
                    recv_for_lookup,
                    &member_name,
                    rest_args.len(),
                ) {
                    if !c.is_static {
                        candidates.push((c, false));
                    }
                }
            }
            // Also try static method `Type::staticMethod` with all `sam.param_types`.
            for c in resolve::collect_method_candidates_on_ty(
                self.resolve_ctx(),
                &qual_ty,
                &member_name,
                sam.param_types.len(),
            ) {
                if c.is_static {
                    candidates.push((c, true));
                }
            }

            if let Some((best, is_static_match)) =
                self.select_best_method_ref_candidate(candidates, &sam.param_types)
            {
                let args = if is_static_match {
                    sam.param_types.as_slice()
                } else {
                    sam.param_types.get(1..).unwrap_or(&[])
                };
                let info = self.finish_call_inference(best, &[], args, &[], Some(&sam.return_type));
                return Some((info.target, info.subst));
            }
            return None;
        }

        // Bound instance method reference `expr::method`.
        let cands = resolve::collect_method_candidates_on_ty(
            self.resolve_ctx(),
            &qual_ty,
            &member_name,
            sam.param_types.len(),
        );
        let best = self.select_best_candidate_from_tys(cands, &sam.param_types)?;
        let info = self.finish_call_inference(
            best,
            &[],
            &sam.param_types,
            &[],
            Some(&sam.return_type),
        );
        Some((info.target, info.subst))
    }

    fn classify_method_ref_qualifier(&mut self, qualifier: &SyntaxNode) -> (bool, Ty) {
        if qualifier.kind() == PATH_TYPE || qualifier.kind() == ARRAY_TYPE {
            return (true, self.resolve_type_node(qualifier, qualifier));
        }
        if let Some(chain) = resolve::extract_dotted_name_chain(qualifier) {
            if chain.len() == 1
                && let Some(name) = chain.first()
                && let Some(ref_node) = qualifier.children().find(|n| n.kind() == NAME_REF)
                && resolve::find_local_decl_node(&ref_node, name).is_some()
            {
                return (false, self.infer_expr(qualifier, None));
            }
            let raw = chain.iter().map(SmolStr::as_str).collect::<Vec<_>>().join(".");
            let as_type = self.resolve_type_str(&raw, qualifier);
            if !matches!(as_type, Ty::Unknown) {
                // Verify it didn't resolve as a field name (if single ident matches field in scope, prefer field).
                if chain.len() == 1
                    && let Some(name) = chain.first()
                    && self.has_field_in_enclosing_scope(qualifier, name)
                {
                    return (false, self.infer_expr(qualifier, None));
                }
                return (true, as_type);
            }
        }
        (false, self.infer_expr(qualifier, None))
    }

    fn has_field_in_enclosing_scope(&self, node: &SyntaxNode, name: &str) -> bool {
        for enc_node in node.ancestors().skip(1) {
            if TypeKind::from_syntax(enc_node.kind()).is_some()
                && let Some(fqn) = self.type_fqn_for_decl(&enc_node)
            {
                let self_ty = self.parameterized_self_ty(&fqn);
                if self
                    .lookup_field_ty_on_ty(&self_ty, name, NullnessScope::Inherited)
                    .is_some()
                {
                    return true;
                }
            }
        }
        false
    }

    // ---- Unification & Overload Selection Helpers ----

    fn infer_non_functional_args(&mut self, arg_nodes: &[SyntaxNode]) -> Vec<Ty> {
        arg_nodes
            .iter()
            .map(|n| {
                if matches!(n.kind(), LAMBDA_EXPR | METHOD_REF_EXPR) {
                    Ty::Unknown
                } else {
                    self.infer_expr(n, None)
                }
            })
            .collect()
    }

    fn select_best_candidate(
        &self,
        candidates: Vec<resolve::CallableCandidate>,
        arg_tys: &[Ty],
        arg_nodes: &[SyntaxNode],
    ) -> Option<resolve::CallableCandidate> {
        resolve::select_best_callable_candidate_in_scope(
            self.db,
            &self.scope,
            candidates,
            arg_tys,
            Some(arg_nodes),
        )
    }

    fn select_best_candidate_from_tys(
        &self,
        candidates: Vec<resolve::CallableCandidate>,
        arg_tys: &[Ty],
    ) -> Option<resolve::CallableCandidate> {
        resolve::select_best_callable_candidate_in_scope(
            self.db,
            &self.scope,
            candidates,
            arg_tys,
            None,
        )
    }

    fn select_best_method_ref_candidate(
        &self,
        candidates: Vec<(resolve::CallableCandidate, bool)>,
        sam_params: &[Ty],
    ) -> Option<(resolve::CallableCandidate, bool)> {
        if candidates.len() <= 1 {
            return candidates.into_iter().next();
        }
        let mut best: Option<(resolve::OverloadPhase, resolve::CallableCandidate, bool)> = None;
        for (cand, is_static) in candidates {
            let args = if is_static {
                sam_params
            } else {
                sam_params.get(1..).unwrap_or(&[])
            };
            let phase =
                resolve::score_candidate_phase_in_scope(self.db, &self.scope, &cand, args, None);
            match &best {
                None => best = Some((phase, cand, is_static)),
                Some((best_phase, best_cand, _)) => {
                    if phase < *best_phase
                        || (phase == *best_phase
                            && resolve::is_candidate_strictly_more_specific_in_scope(
                                self.db,
                                &self.scope,
                                &cand,
                                best_cand,
                                args.len(),
                            ))
                    {
                        best = Some((phase, cand, is_static));
                    }
                }
            }
        }
        best.map(|(_, c, s)| (c, s))
    }

    fn unify(
        &self,
        pattern: &Ty,
        actual: &Ty,
        vars_to_bind: &FxHashSet<SmolStr>,
        out: &mut Substitution,
        depth: usize,
    ) {
        if depth >= MAX_INFER_DEPTH {
            return;
        }
        if matches!(actual, Ty::Unknown | Ty::Null | Ty::Void) {
            return;
        }

        match pattern {
            Ty::TypeVar { name, .. } => {
                if !vars_to_bind.contains(name) {
                    return;
                }
                let boxed_actual = match actual {
                    Ty::Primitive(p) => Ty::raw_class(p.boxed_fqn()),
                    other => other.clone(),
                };
                if let Some(TypeArg::Type(prev)) = out.map.get(name) {
                    let combined =
                        least_upper_bound_in_scope(self.db, &self.scope, prev, &boxed_actual);
                    if !matches!(combined, Ty::Unknown) {
                        out.insert_ty(name.clone(), combined);
                    }
                } else {
                    out.insert_ty(name.clone(), boxed_actual);
                }
            }
            Ty::Array { elem: p_elem, .. } => {
                if let Ty::Array { elem: a_elem, .. } = actual {
                    self.unify(p_elem, a_elem, vars_to_bind, out, depth + 1);
                }
            }
            Ty::Class {
                fqn: p_fqn,
                type_args: p_args,
                ..
            } => {
                if p_args.is_empty() {
                    return;
                }
                let Ty::Class {
                    fqn: a_fqn,
                    type_args: a_args,
                    ..
                } = actual
                else {
                    return;
                };
                if let Some(proj) = supertype_substitution_in_scope(
                    self.db,
                    &self.scope,
                    a_fqn,
                    a_args,
                    p_fqn,
                ) {
                    let p_params = type_params_of_fqn_in_scope(self.db, &self.scope, p_fqn);
                    for (p_arg, p_param) in p_args.iter().zip(p_params.iter()) {
                        let Some(a_arg) = proj.map.get(p_param) else {
                            continue;
                        };
                        self.unify_type_arg(p_arg, a_arg, vars_to_bind, out, depth + 1);
                    }
                } else if !a_args.is_empty()
                    && let Some(proj) = supertype_substitution_in_scope(
                        self.db,
                        &self.scope,
                        p_fqn,
                        p_args,
                        a_fqn,
                    )
                {
                    let a_params = type_params_of_fqn_in_scope(self.db, &self.scope, a_fqn);
                    for (a_arg, a_param) in a_args.iter().zip(a_params.iter()) {
                        let Some(p_arg) = proj.map.get(a_param) else {
                            continue;
                        };
                        self.unify_type_arg(p_arg, a_arg, vars_to_bind, out, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }

    fn unify_type_arg(
        &self,
        p_arg: &TypeArg,
        a_arg: &TypeArg,
        vars_to_bind: &FxHashSet<SmolStr>,
        out: &mut Substitution,
        depth: usize,
    ) {
        if depth >= MAX_INFER_DEPTH {
            return;
        }
        match (p_arg, a_arg) {
            (TypeArg::Type(pt), TypeArg::Type(at)) => {
                self.unify(pt, at, vars_to_bind, out, depth + 1);
            }
            (TypeArg::Wildcard(WildcardBound::Extends(pt)), TypeArg::Type(at))
            | (TypeArg::Wildcard(WildcardBound::Super(pt)), TypeArg::Type(at)) => {
                self.unify(pt, at, vars_to_bind, out, depth + 1);
            }
            (
                TypeArg::Wildcard(WildcardBound::Extends(pt)),
                TypeArg::Wildcard(WildcardBound::Extends(at)),
            )
            | (
                TypeArg::Wildcard(WildcardBound::Super(pt)),
                TypeArg::Wildcard(WildcardBound::Super(at)),
            ) => {
                self.unify(pt, at, vars_to_bind, out, depth + 1);
            }
            _ => {}
        }
    }
}

fn is_diamond_new_expr(type_node: &SyntaxNode) -> bool {
    type_node.descendants().any(|n| {
        n.kind() == TYPE_ARGS
            && !n
                .children()
                .any(|c| is_type_node(c.kind()) || c.kind() == WILDCARD_TYPE)
    })
}

fn collect_arg_nodes(call_node: &SyntaxNode) -> Vec<SyntaxNode> {
    call_node
        .children()
        .find(|n| n.kind() == ARG_LIST)
        .map(|al| al.children().collect())
        .unwrap_or_default()
}

pub(crate) fn parameter_for_arg(param_tys: &[(Ty, bool)], idx: usize) -> Option<(Ty, bool)> {
    if let Some(pair) = param_tys.get(idx) {
        return Some(pair.clone());
    }
    param_tys.last().filter(|(_, is_varargs)| *is_varargs).cloned()
}

fn collect_return_exprs(node: &SyntaxNode, out: &mut Vec<SyntaxNode>) {
    for child in node.children() {
        match child.kind() {
            RETURN_STMT => {
                if let Some(e) = child.children().next() {
                    out.push(e);
                }
            }
            LAMBDA_EXPR => {}
            k if TypeKind::from_syntax(k).is_some() => {}
            _ => collect_return_exprs(&child, out),
        }
    }
}
