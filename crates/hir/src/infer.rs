//! Bidirectional, cycle-safe Java expression and local variable type inference.
//! See `docs/specs/0008-expression-type-inference-and-generics.md` (Section 3).

use std::{collections::BTreeMap, sync::Arc};

use rustc_hash::{FxHashMap, FxHashSet};
use smol_str::SmolStr;
use syntax::{
    SyntaxNode, TextRange,
    ext::{collapsed_text, has_direct_token, has_modifier, is_type_node, name_child, path_segments},
    syntax_kind::SyntaxKind::*,
};

use crate::{
    DefDatabase, FileId,
    classpath::{BinaryMemberSymbol, BinaryTypeSymbol, ClasspathIndex},
    hierarchy::linearized_supertypes,
    item_tree::{AstIdMap, FileItemTree, MemberItem},
    resolve::{self, ResolveCtx, ResolvedTarget, SymbolDetail},
    symbols::{
        ImportResolver, MemberKind, NullnessScope, ParamSummary, TypeKind, WorkspaceIndex,
        extract_modifier_nullability, extract_modifier_nullness_scope, format_params_signature,
        node_ident_text, render_type_node, render_type_with_dims_and_nullability,
    },
    ty::{
        MAX_SUBST_DEPTH, Nullability, PrimitiveType, Substitution, Ty, TypeArg, WildcardBound,
        apply_nullness_scope, binary_numeric_promotion, effective_nullness_scope_for_binary_member,
        effective_nullness_scope_for_file, effective_nullness_scope_for_member,
        effective_nullness_scope_for_type, is_subtype, least_upper_bound, parse_type_ref,
        resolve_binary_type_ref_for_member, resolve_binary_type_ref_for_owner,
        resolve_type_ref_in_file_for_member, resolve_type_ref_in_file_for_owner,
        resolve_type_ref_in_file_with_scope, supertype_substitution_for_ty,
        ty_has_explicit_nullability, type_param_bounds_of_fqn, type_params_of_fqn,
        unary_numeric_promotion,
    },
};

/// Maximum recursion depth for expression and local-binding type inference.
pub const MAX_INFER_DEPTH: usize = 32;

const OBJECT_FQN: &str = "java.lang.Object";
const STRING_FQN: &str = "java.lang.String";
const CLASS_FQN: &str = "java.lang.Class";
const ITERABLE_FQN: &str = "java.lang.Iterable";

/// Signature of the Single Abstract Method (SAM) of a functional interface,
/// instantiated with the target interface's type arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SamSignature {
    pub method_name: SmolStr,
    pub param_types: Vec<Ty>,
    pub return_type: Ty,
}

/// Details of a resolved call expression, used by both type inference and hover/resolution.
#[derive(Debug, Clone)]
pub(crate) struct CallInference {
    pub target: ResolvedTarget,
    pub subst: Substitution,
    pub return_ty: Ty,
    pub resolved_params: Vec<(Ty, bool)>,
}

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

/// Finds the Single Abstract Method (SAM) of `ty` if `ty` denotes a functional interface
/// (JLS §9.8), returning its parameter and return types substituted with `ty`'s type arguments.
#[must_use]
pub fn find_functional_interface_sam(db: &dyn DefDatabase, ty: &Ty) -> Option<SamSignature> {
    let (fqn, type_args) = match ty {
        Ty::Class { fqn, type_args, .. } => (fqn.as_str(), type_args.as_slice()),
        _ => return None,
    };

    let workspace = db.workspace_index();
    let classpath = db.classpath_index();

    let is_interface = if let Some((fid, tid)) = workspace.get_type(fqn) {
        db.file_item_tree(fid).types[tid].kind == TypeKind::Interface
    } else if let Some(bin) = classpath.type_by_fqn(fqn) {
        bin.kind == TypeKind::Interface
    } else {
        false
    };
    if !is_interface {
        return None;
    }

    let mut visited_ifaces = FxHashSet::default();
    let mut seen_sigs: FxHashSet<(SmolStr, Vec<String>)> = FxHashSet::default();
    let mut candidates: Vec<SamCandidate> = Vec::new();

    let mut ordered_ifaces = Vec::with_capacity(1);
    ordered_ifaces.push(fqn.to_string());
    for sup in linearized_supertypes(db, fqn) {
        if sup != OBJECT_FQN {
            ordered_ifaces.push(sup);
        }
    }

    for iface_fqn in ordered_ifaces {
        if !visited_ifaces.insert(iface_fqn.clone()) {
            continue;
        }
        if let Some((fid, tid)) = workspace.get_type(&iface_fqn) {
            let tree = db.file_item_tree(fid);
            let ty_item = &tree.types[tid];
            if ty_item.kind != TypeKind::Interface {
                continue;
            }
            let owner_scope = type_param_bounds_of_fqn(db, &iface_fqn);
            for &mid in &ty_item.members {
                let m = &tree.members[mid];
                if m.kind != MemberKind::Method || m.is_static {
                    continue;
                }
                let sig_key = sam_signature_key(db, Some(fid), Some(&iface_fqn), None, &m.name, &m.params, &owner_scope);
                if !seen_sigs.insert(sig_key) {
                    continue;
                }
                if !m.is_abstract || m.is_default {
                    continue;
                }
                if is_object_public_method_source(db, fid, &iface_fqn, m, &owner_scope) {
                    continue;
                }
                let mut m_scope = owner_scope.clone();
                for tp in &m.type_params {
                    m_scope.insert(
                        tp.clone(),
                        m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
                    );
                }
                let param_tys = resolve_params_in_file_for_member(
                    db,
                    fid,
                    Some(&iface_fqn),
                    m.nullness_scope,
                    &m.params,
                    &m_scope,
                );
                let ret_ty = m
                    .type_ref
                    .as_deref()
                    .map_or(Ty::Void, |r| {
                        resolve_type_ref_in_file_for_member(
                            db,
                            fid,
                            Some(&iface_fqn),
                            m.nullness_scope,
                            r,
                            &m_scope,
                        )
                    });
                candidates.push(SamCandidate {
                    owner_fqn: iface_fqn.clone(),
                    method_name: m.name.clone(),
                    has_method_type_params: !m.type_params.is_empty(),
                    param_tys,
                    ret_ty,
                });
            }
        } else if let Some(bin) = classpath.type_by_fqn(&iface_fqn) {
            if bin.kind != TypeKind::Interface {
                continue;
            }
            let owner_scope = type_param_bounds_of_fqn(db, &iface_fqn);
            for m in &bin.members {
                if m.kind != MemberKind::Method || m.is_static {
                    continue;
                }
                let sig_key = sam_signature_key(db, None, None, Some(bin), &m.name, &m.params, &owner_scope);
                if !seen_sigs.insert(sig_key) {
                    continue;
                }
                if !m.is_abstract {
                    continue;
                }
                if is_object_public_method_binary(&classpath, bin, m, &owner_scope) {
                    continue;
                }
                let mut m_scope = owner_scope.clone();
                for tp in &m.type_params {
                    m_scope.insert(
                        tp.clone(),
                        m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
                    );
                }
                let param_tys = resolve_params_in_binary_for_member(
                    &classpath,
                    bin,
                    m.nullness_scope,
                    &m.params,
                    &m_scope,
                );
                let ret_ty = m
                    .type_ref
                    .as_deref()
                    .map_or(Ty::Void, |r| {
                        resolve_binary_type_ref_for_member(&classpath, bin, m.nullness_scope, r, &m_scope)
                    });
                candidates.push(SamCandidate {
                    owner_fqn: iface_fqn.clone(),
                    method_name: m.name.clone(),
                    has_method_type_params: !m.type_params.is_empty(),
                    param_tys,
                    ret_ty,
                });
            }
        }
    }

    if candidates.len() != 1 {
        return None;
    }
    let sam = candidates.into_iter().next()?;
    let subst = crate::ty::supertype_substitution(db, fqn, type_args, &sam.owner_fqn)
        .unwrap_or_default();

    // Raw functional interface: erase class type variables to Object/bounds if `type_args` is empty
    // and the interface declares type parameters.
    let raw_iface = !type_params_of_fqn(db, fqn).is_empty() && type_args.is_empty();
    let param_types = sam
        .param_tys
        .into_iter()
        .map(|(p, _)| {
            if raw_iface {
                erase_type_vars_to_bound(&p)
            } else {
                subst.apply_contravariant(&p)
            }
        })
        .collect();
    let return_type = if raw_iface {
        erase_type_vars_to_bound(&sam.ret_ty)
    } else {
        subst.apply_covariant(&sam.ret_ty)
    };
    let _ = sam.has_method_type_params;

    Some(SamSignature {
        method_name: sam.method_name,
        param_types,
        return_type,
    })
}

struct SamCandidate {
    owner_fqn: String,
    method_name: SmolStr,
    has_method_type_params: bool,
    param_tys: Vec<(Ty, bool)>,
    ret_ty: Ty,
}

fn sam_signature_key(
    db: &dyn DefDatabase,
    file_id: Option<FileId>,
    owner_fqn: Option<&str>,
    bin_owner: Option<&BinaryTypeSymbol>,
    name: &SmolStr,
    params: &[ParamSummary],
    scope: &BTreeMap<SmolStr, Vec<String>>,
) -> (SmolStr, Vec<String>) {
    let erased_params = params
        .iter()
        .map(|p| {
            let ty = if let (Some(fid), Some(fqn)) = (file_id, owner_fqn) {
                resolve_type_ref_in_file_for_owner(db, fid, Some(fqn), &p.type_ref, scope)
            } else if let Some(bin) = bin_owner {
                resolve_binary_type_ref_for_owner(&db.classpath_index(), bin, &p.type_ref, scope)
            } else {
                Ty::Unknown
            };
            let full = if p.is_varargs {
                Ty::array(ty)
            } else {
                ty
            };
            erase_to_signature_key(&full)
        })
        .collect();
    (name.clone(), erased_params)
}

fn erase_to_signature_key(ty: &Ty) -> String {
    match ty {
        Ty::Primitive(p) => p.keyword().to_string(),
        Ty::Void => "void".to_string(),
        Ty::Null => "null".to_string(),
        Ty::Class { fqn, .. } => fqn.clone(),
        Ty::Array { elem, .. } => format!("{}[]", erase_to_signature_key(elem)),
        Ty::TypeVar { bounds, .. } => bounds
            .first()
            .map_or_else(|| OBJECT_FQN.to_string(), erase_to_signature_key),
        Ty::Intersection(members) => members
            .first()
            .map_or_else(|| OBJECT_FQN.to_string(), erase_to_signature_key),
        Ty::Unknown => "<unknown>".to_string(),
    }
}

fn is_object_public_method_source(
    db: &dyn DefDatabase,
    fid: FileId,
    owner_fqn: &str,
    m: &MemberItem,
    scope: &BTreeMap<SmolStr, Vec<String>>,
) -> bool {
    match (m.name.as_str(), m.params.as_slice()) {
        ("toString", []) => m.type_ref.as_deref().is_some_and(|r| {
            resolve_type_ref_in_file_for_owner(db, fid, Some(owner_fqn), r, scope)
                .with_nullability(Nullability::Unspecified)
                == Ty::string()
        }),
        ("hashCode", []) => m.type_ref.as_deref().is_some_and(|r| {
            resolve_type_ref_in_file_for_owner(db, fid, Some(owner_fqn), r, scope)
                == Ty::Primitive(PrimitiveType::Int)
        }),
        ("equals", [p]) => {
            let p_ty = resolve_type_ref_in_file_for_owner(db, fid, Some(owner_fqn), &p.type_ref, scope)
                .with_nullability(Nullability::Unspecified);
            let ret_ok = m.type_ref.as_deref().is_some_and(|r| {
                resolve_type_ref_in_file_for_owner(db, fid, Some(owner_fqn), r, scope)
                    == Ty::Primitive(PrimitiveType::Boolean)
            });
            ret_ok && p_ty == Ty::object()
        }
        _ => false,
    }
}

fn is_object_public_method_binary(
    classpath: &ClasspathIndex,
    owner: &BinaryTypeSymbol,
    m: &BinaryMemberSymbol,
    scope: &BTreeMap<SmolStr, Vec<String>>,
) -> bool {
    match (m.name.as_str(), m.params.as_slice()) {
        ("toString", []) => m.type_ref.as_deref().is_some_and(|r| {
            resolve_binary_type_ref_for_owner(classpath, owner, r, scope)
                .with_nullability(Nullability::Unspecified)
                == Ty::string()
        }),
        ("hashCode", []) => m.type_ref.as_deref().is_some_and(|r| {
            resolve_binary_type_ref_for_owner(classpath, owner, r, scope)
                == Ty::Primitive(PrimitiveType::Int)
        }),
        ("equals", [p]) => {
            let p_ty = resolve_binary_type_ref_for_owner(classpath, owner, &p.type_ref, scope)
                .with_nullability(Nullability::Unspecified);
            let ret_ok = m.type_ref.as_deref().is_some_and(|r| {
                resolve_binary_type_ref_for_owner(classpath, owner, r, scope)
                    == Ty::Primitive(PrimitiveType::Boolean)
            });
            ret_ok && p_ty == Ty::object()
        }
        _ => false,
    }
}

pub(crate) fn erase_type_vars_to_bound(ty: &Ty) -> Ty {
    erase_type_vars_inner(ty, 0)
}

fn erase_type_vars_inner(ty: &Ty, depth: usize) -> Ty {
    if depth >= MAX_SUBST_DEPTH {
        return Ty::object();
    }
    match ty {
        Ty::TypeVar {
            bounds,
            nullability,
            ..
        } => {
            let base = bounds
                .first()
                .map_or_else(Ty::object, |b| erase_type_vars_inner(b, depth + 1));
            match nullability {
                Nullability::Unspecified => base,
                n @ (Nullability::NonNull | Nullability::Nullable) => base.with_nullability(*n),
            }
        }
        Ty::Class {
            fqn, nullability, ..
        } => Ty::raw_class(fqn).with_nullability(*nullability),
        Ty::Array { elem, nullability } => Ty::Array {
            elem: Box::new(erase_type_vars_inner(elem, depth + 1)),
            nullability: *nullability,
        },
        Ty::Intersection(members) => members
            .first()
            .map_or_else(Ty::object, |m| erase_type_vars_inner(m, depth + 1)),
        _ => ty.clone(),
    }
}

// ---- Inference Context ----

struct InferCtx<'a> {
    db: &'a dyn DefDatabase,
    file_id: FileId,
    file_tree: Arc<FileItemTree>,
    ast_id_map: Arc<AstIdMap>,
    index: Arc<WorkspaceIndex>,
    classpath: Arc<ClasspathIndex>,
    import_resolver: ImportResolver,
    depth: usize,
    visiting_nodes: FxHashSet<TextRange>,
    lambda_param_bindings: FxHashMap<TextRange, Ty>,
}

impl<'a> InferCtx<'a> {
    fn new(db: &'a dyn DefDatabase, file_id: FileId) -> Self {
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

    fn effective_scope_at(&self, node: &SyntaxNode) -> NullnessScope {
        effective_nullness_scope_at_inner(
            self.db,
            self.file_id,
            &self.file_tree,
            &self.ast_id_map,
            &self.import_resolver,
            node,
        )
    }

    fn resolve_ctx(&self) -> ResolveCtx<'_> {
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

    fn infer_expr(&mut self, expr_node: &SyntaxNode, expected: Option<&Ty>) -> Ty {
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

        let cls = Ty::class(CLASS_FQN, vec![TypeArg::Type(boxed)]);
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

    pub(crate) fn infer_method_call(
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

    pub(crate) fn infer_unqualified_call(
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
                let Some(sam) = find_functional_interface_sam(self.db, &target_iface) else {
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
                effective_nullness_scope_for_member(
                    self.db,
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

    fn infer_new_expr(&mut self, expr: &SyntaxNode, expected: Option<&Ty>) -> Ty {
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
            let tp_names = type_params_of_fqn(self.db, fqn);
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

    fn infer_cast_expr(&self, expr: &SyntaxNode) -> Ty {
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

    // ---- Target type derivation ----

    pub(crate) fn expected_target_type(&mut self, expr: &SyntaxNode) -> Option<Ty> {
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
                            let sam = find_functional_interface_sam(self.db, &lambda_target)?;
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
                        let subst = supertype_substitution_for_ty(self.db, &constructed_ty, &fqn)
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
                        Some(prev) => least_upper_bound(ctx.db, &prev, &t),
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

    pub(crate) fn resolve_method_ref_with_sam(
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
            let cands =
                resolve::collect_constructor_candidates(self.resolve_ctx(), &fqn, sam.param_types.len());
            let best = self.select_best_candidate_from_tys(cands, &sam.param_types)?;
            let subst = supertype_substitution_for_ty(self.db, &qual_ty, &fqn).unwrap_or_default();
            return Some((best.target, subst));
        }

        let ref_node = method_ref.children().find(|n| n.kind() == NAME_REF)?;
        let member_name = node_ident_text(&ref_node);

        if is_type_qual {
            // Try unbound instance method `Type::method` (receiver is `sam.param_types[0]`,
            // remaining args are `sam.param_types[1..]`).
            let mut candidates = Vec::new();
            if let Some(first_param) = sam.param_types.first()
                && (is_subtype(self.db, first_param, &qual_ty)
                    || matches!(qual_ty, Ty::Class { .. }) && is_subtype(self.db, &qual_ty, first_param))
            {
                let rest_args = &sam.param_types[1..];
                let recv_for_lookup = if matches!(qual_ty, Ty::Class { ref type_args, .. } if type_args.is_empty())
                    && !matches!(first_param, Ty::Unknown)
                    && is_subtype(self.db, first_param, &qual_ty)
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

            if let Some((best, is_static_match)) = self.select_best_method_ref_candidate(candidates, &sam.param_types) {
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
        resolve::select_best_callable_candidate(self.db, candidates, arg_tys, Some(arg_nodes))
    }

    fn select_best_candidate_from_tys(
        &self,
        candidates: Vec<resolve::CallableCandidate>,
        arg_tys: &[Ty],
    ) -> Option<resolve::CallableCandidate> {
        resolve::select_best_callable_candidate(self.db, candidates, arg_tys, None)
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
            let phase = resolve::score_candidate_phase(self.db, &cand, args, None);
            match &best {
                None => best = Some((phase, cand, is_static)),
                Some((best_phase, best_cand, _)) => {
                    if phase < *best_phase
                        || (phase == *best_phase
                            && resolve::is_candidate_strictly_more_specific(
                                self.db,
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
                    let combined = least_upper_bound(self.db, prev, &boxed_actual);
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
                if let Some(proj) = crate::ty::supertype_substitution(self.db, a_fqn, a_args, p_fqn)
                {
                    let p_params = type_params_of_fqn(self.db, p_fqn);
                    for (p_arg, p_param) in p_args.iter().zip(p_params.iter()) {
                        let Some(a_arg) = proj.map.get(p_param) else {
                            continue;
                        };
                        self.unify_type_arg(p_arg, a_arg, vars_to_bind, out, depth + 1);
                    }
                } else if !a_args.is_empty()
                    && let Some(proj) =
                        crate::ty::supertype_substitution(self.db, p_fqn, p_args, a_fqn)
                {
                    let a_params = type_params_of_fqn(self.db, a_fqn);
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

    // ---- Member & Type Scope Helpers ----

    fn lookup_field_ty_on_ty(
        &self,
        recv_ty: &Ty,
        field_name: &str,
        site_scope: NullnessScope,
    ) -> Option<Ty> {
        let recv_fqn = recv_ty.erased_fqn()?;
        let mut chain = Vec::with_capacity(1);
        chain.push(recv_fqn.to_string());
        chain.extend(linearized_supertypes(self.db, recv_fqn));

        for owner_fqn in chain {
            if let Some((fid, tid)) = self.index.get_type(&owner_fqn) {
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
                        let member_scope =
                            effective_nullness_scope_for_member(self.db, &owner_fqn, m.nullness_scope);
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
                    let scope = type_param_bounds_of_fqn(self.db, &owner_fqn);
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
            } else if let Some(bin) = self.classpath.type_by_fqn(&owner_fqn) {
                if let Some(m) = bin.members.iter().find(|m| {
                    m.kind != MemberKind::Method
                        && m.kind != MemberKind::Constructor
                        && m.name == field_name
                }) {
                    if m.kind == MemberKind::EnumConstant {
                        let member_scope = effective_nullness_scope_for_binary_member(
                            &self.classpath,
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
                    let scope = type_param_bounds_of_fqn(self.db, &owner_fqn);
                    let raw_ty = resolve_binary_type_ref_for_member(
                        &self.classpath,
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
        let is_raw = is_raw_class_ty(self.db, recv_ty);
        if is_raw {
            return erase_type_vars_to_bound(raw_ty);
        }
        let subst = supertype_substitution_for_ty(self.db, recv_ty, owner_fqn).unwrap_or_default();
        subst.apply_covariant(raw_ty)
    }

    fn target_ty(&self, target: &ResolvedTarget, subst: &Substitution) -> Ty {
        self.target_ty_with_site_scope(target, subst, NullnessScope::Inherited)
    }

    fn target_ty_with_site_scope(
        &self,
        target: &ResolvedTarget,
        subst: &Substitution,
        site_scope: NullnessScope,
    ) -> Ty {
        let enum_const_ty = |owner_fqn: &str, member_scope: NullnessScope| {
            let eff = effective_nullness_scope_for_member(self.db, owner_fqn, member_scope);
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
                    .map(|fqn| type_param_bounds_of_fqn(self.db, fqn))
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
                let Some(bin) = self.classpath.type_by_fqn(owner_fqn) else {
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
                let scope = type_param_bounds_of_fqn(self.db, owner_fqn);
                resolve_binary_type_ref_for_member(
                    &self.classpath,
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

    fn resolve_member_type_in_owner(
        &self,
        target: &ResolvedTarget,
        owner_fqn: &str,
        raw: &str,
        extra_scope: &BTreeMap<SmolStr, Vec<String>>,
    ) -> Ty {
        let mut scope = type_param_bounds_of_fqn(self.db, owner_fqn);
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
                if let Some(bin) = self.classpath.type_by_fqn(owner_fqn) {
                    let m_scope = bin
                        .members
                        .get(*member_index)
                        .map_or(NullnessScope::Inherited, |m| m.nullness_scope);
                    resolve_binary_type_ref_for_member(&self.classpath, bin, m_scope, raw, &scope)
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

    fn resolve_explicit_type_node(
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

    fn resolve_type_node(&self, type_node: &SyntaxNode, site_node: &SyntaxNode) -> Ty {
        let raw = render_type_node(type_node, &self.import_resolver);
        self.resolve_type_str(&raw, site_node)
    }

    fn resolve_type_str(&self, raw: &str, site_node: &SyntaxNode) -> Ty {
        self.resolve_type_str_with_local(raw, site_node, false)
    }

    fn resolve_type_str_with_local(
        &self,
        raw: &str,
        site_node: &SyntaxNode,
        is_local_root: bool,
    ) -> Ty {
        let scope = type_params_in_scope_at(self.db, &self.file_tree, &self.ast_id_map, site_node);
        let owner_fqn = enclosing_type_fqn(&self.file_tree, &self.ast_id_map, site_node);
        let nullness_scope = self.effective_scope_at(site_node);
        resolve_type_ref_in_file_with_scope(
            self.db,
            self.file_id,
            owner_fqn.as_deref(),
            nullness_scope,
            is_local_root,
            raw,
            &scope,
        )
    }

    fn extract_explicit_type_args(&self, call_node: &SyntaxNode) -> Vec<TypeArg> {
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
        let scope = type_params_in_scope_at(self.db, &self.file_tree, &self.ast_id_map, call_node);
        let owner_fqn = enclosing_type_fqn(&self.file_tree, &self.ast_id_map, call_node);
        let nullness_scope = self.effective_scope_at(call_node);
        let ty = parse_type_ref(&dummy, &mut |segs| {
            if segs == ["__Dummy"] {
                Some(Ty::raw_class("__Dummy"))
            } else {
                let joined = segs.join(".");
                Some(resolve_type_ref_in_file_with_scope(
                    self.db,
                    self.file_id,
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

    fn extract_path_type_last_args(
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

    fn type_fqn_for_decl(&self, type_node: &SyntaxNode) -> Option<String> {
        self.ast_id_map
            .find_by_full(type_node.text_range())
            .and_then(|id| self.file_tree.find_type_by_ast_id(id))
            .map(|(_, ty)| ty.qualified_name.clone())
    }

    fn parameterized_self_ty(&self, fqn: &str) -> Ty {
        let tp_bounds = type_param_bounds_of_fqn(self.db, fqn);
        let tp_names = type_params_of_fqn(self.db, fqn);
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

    fn infer_enclosing_self_ty(&self, node: &SyntaxNode) -> Ty {
        for ancestor in node.ancestors().skip(1) {
            if TypeKind::from_syntax(ancestor.kind()).is_some()
                && let Some(fqn) = self.type_fqn_for_decl(&ancestor)
            {
                return self.parameterized_self_ty(&fqn);
            }
        }
        Ty::Unknown
    }

    fn direct_superclass_ty(&self, self_ty: &Ty) -> Option<Ty> {
        let fqn = self_ty.erased_fqn()?;
        if let Some((fid, tid)) = self.index.get_type(fqn) {
            let tree = self.db.file_item_tree(fid);
            let ty_item = &tree.types[tid];
            if ty_item.kind != TypeKind::Class {
                return None;
            }
            let sup = if let Some(raw) = ty_item.extends.first() {
                let scope = type_param_bounds_of_fqn(self.db, fqn);
                resolve_type_ref_in_file_for_owner(self.db, fid, Some(fqn), raw, &scope)
            } else if fqn != OBJECT_FQN {
                Ty::object()
            } else {
                return None;
            };
            let subst = supertype_substitution_for_ty(self.db, self_ty, fqn).unwrap_or_default();
            return Some(subst.apply_covariant(&sup));
        }
        if let Some(bin) = self.classpath.type_by_fqn(fqn) {
            if bin.kind != TypeKind::Class || fqn == OBJECT_FQN {
                return None;
            }
            let sup = if let Some(raw) = bin.super_class_generic.as_deref() {
                let scope = type_param_bounds_of_fqn(self.db, fqn);
                resolve_binary_type_ref_for_owner(&self.classpath, bin, raw, &scope)
            } else {
                Ty::raw_class(bin.super_class.as_deref().unwrap_or(OBJECT_FQN))
            };
            let subst = supertype_substitution_for_ty(self.db, self_ty, fqn).unwrap_or_default();
            return Some(subst.apply_covariant(&sup));
        }
        None
    }

    fn is_interface_fqn(&self, fqn: &str) -> bool {
        if let Some((fid, tid)) = self.index.get_type(fqn) {
            self.db.file_item_tree(fid).types[tid].kind == TypeKind::Interface
        } else if let Some(bin) = self.classpath.type_by_fqn(fqn) {
            bin.kind == TypeKind::Interface
        } else {
            false
        }
    }

    fn is_known_fqn(&self, fqn: &str) -> bool {
        self.index.contains_type(fqn)
            || self.file_tree.find_type_by_fqn(fqn).is_some()
            || self.classpath.type_by_fqn(fqn).is_some()
    }
}

//Wait: Look at `with_lambda_bindings` above! Remember our comment about `visiting_nodes`:
// When `finish_call_inference` calls `infer_lambda_return_ty` for `u -> u.getName()` inside `users.stream().map(u -> u.getName())`,
// `InferCtx::visiting_nodes` STILL holds `users.stream().map(u -> u.getName())`'s `TextRange`!
// If `u` calls `infer_local(u)` -> `expected_target_type(lambda)` -> `infer_method_call(map_call)`,
// `infer_expr` checks `visiting_nodes` (Wait: `infer_method_call` is called directly by `expected_target_type`, NOT via `infer_expr`, so `visiting_nodes` is NOT checked in `infer_method_call`!).
// STILL, re-running `infer_method_call` on the enclosing call for every `u` reference inside the lambda would be redundant O(N^2) work AND could re-enter `infer_lambda_return_ty` infinitely!
// Instead, let's store `lambda_param_bindings: BTreeMap<TextRange, Ty>` (keyed by `PARAM`'s `TextRange`) inside `InferCtx`!
// Let's update `InferCtx` to include `lambda_param_bindings: BTreeMap<TextRange, Ty>` right now — that makes `u` inside `infer_lambda_return_ty` O(1) and 100% cycle-free!

// ---- Pure CST helpers ----

pub(crate) fn effective_nullness_scope_at(
    db: &dyn DefDatabase,
    file_id: FileId,
    node: &SyntaxNode,
) -> NullnessScope {
    let file_tree = db.file_item_tree(file_id);
    let ast_id_map = db.ast_id_map(file_id);
    let resolver = ImportResolver::from_imports(
        file_tree.package.as_deref(),
        file_tree
            .imports
            .iter()
            .map(|(_, imp)| (imp.kind, imp.path.as_slice())),
    );
    effective_nullness_scope_at_inner(db, file_id, &file_tree, &ast_id_map, &resolver, node)
}

fn effective_nullness_scope_at_inner(
    db: &dyn DefDatabase,
    file_id: FileId,
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
                    let s = effective_nullness_scope_for_type(db, &ty.qualified_name);
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

fn is_var_type_node(type_node: &SyntaxNode) -> bool {
    type_node.kind() == PATH_TYPE && collapsed_text(type_node) == "var"
}

fn is_diamond_new_expr(type_node: &SyntaxNode) -> bool {
    type_node.descendants().any(|n| {
        n.kind() == TYPE_ARGS
            && !n
                .children()
                .any(|c| is_type_node(c.kind()) || c.kind() == WILDCARD_TYPE)
    })
}

fn is_string_ty(ty: &Ty) -> bool {
    matches!(ty, Ty::Class { fqn, .. } if fqn == STRING_FQN)
}

fn is_boolean_ty(ty: &Ty) -> bool {
    matches!(ty, Ty::Primitive(PrimitiveType::Boolean))
        || matches!(ty, Ty::Class { fqn, type_args, .. } if type_args.is_empty() && fqn == "java.lang.Boolean")
}

pub(crate) fn is_raw_class_ty(db: &dyn DefDatabase, ty: &Ty) -> bool {
    match ty {
        Ty::Class { fqn, type_args, .. } => type_args.is_empty() && !type_params_of_fqn(db, fqn).is_empty(),
        _ => false,
    }
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
                    let map = type_param_bounds_of_fqn(db, &ty.qualified_name);
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
                if has_modifier(&ancestor, STATIC_KW)
                    || !matches!(k, CLASS_DECL)
                {
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
        effective_nullness_scope_for_type(db, fqn)
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

// ---- Crate-internal entry points for `resolve.rs` ----

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
            .is_some_and(|t| is_raw_class_ty(db, &t));
    Some(attach_substituted_detail(db, info.target, &info.subst, is_raw))
}

pub(crate) fn resolve_unqualified_call_target(
    db: &dyn DefDatabase,
    file_id: FileId,
    call_expr: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let mut ctx = InferCtx::new(db, file_id);
    let expected = ctx.expected_target_type(call_expr);
    let info = ctx.infer_unqualified_call(call_expr, expected.as_ref())?;
    Some(attach_substituted_detail(db, info.target, &info.subst, false))
}

pub(crate) fn resolve_method_ref_target_with_context(
    db: &dyn DefDatabase,
    file_id: FileId,
    method_ref: &SyntaxNode,
) -> Option<ResolvedTarget> {
    let mut ctx = InferCtx::new(db, file_id);
    let expected = ctx.expected_target_type(method_ref)?;
    let sam = find_functional_interface_sam(db, &expected)?;
    let (target, subst) = ctx.resolve_method_ref_with_sam(method_ref, &sam)?;
    Some(attach_substituted_detail(db, target, &subst, false))
}

pub(crate) fn resolve_field_access_target(
    db: &dyn DefDatabase,
    _file_id: FileId,
    recv_ty: &Ty,
    target: ResolvedTarget,
) -> ResolvedTarget {
    let is_raw = is_raw_class_ty(db, recv_ty);
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
    let subst = supertype_substitution_for_ty(db, recv_ty, &owner_fqn).unwrap_or_default();
    attach_substituted_detail(db, target, &subst, is_raw)
}

pub(crate) fn format_member_detail(db: &dyn DefDatabase, target: &ResolvedTarget) -> Option<SymbolDetail> {
    format_substituted_detail(db, target, &Substitution::default(), false)
}

fn attach_substituted_detail(
    db: &dyn DefDatabase,
    target: ResolvedTarget,
    subst: &Substitution,
    is_raw: bool,
) -> ResolvedTarget {
    if let Some(detail) = format_substituted_detail(db, &target, subst, is_raw) {
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
    target: &ResolvedTarget,
    subst: &Substitution,
    is_raw: bool,
) -> Option<SymbolDetail> {
    let classpath = db.classpath_index();
    match target {
        ResolvedTarget::IndexedMember(fid, mid) => {
            let tree = db.file_item_tree(*fid);
            let m = &tree.members[*mid];
            let owner_fqn = m.owner_type.as_deref();
            let mut scope = owner_fqn
                .map(|fqn| type_param_bounds_of_fqn(db, fqn))
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
            build_substituted_member_detail(m.kind, m.type_ref.as_deref(), &m.params, subst, is_raw, &resolve_fn)
        }
        ResolvedTarget::LocalMember(m) => {
            let owner_fqn = m.owner_type.as_deref().unwrap_or("");
            let mut scope = if owner_fqn.is_empty() {
                BTreeMap::new()
            } else {
                type_param_bounds_of_fqn(db, owner_fqn)
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
            build_substituted_member_detail(m.kind, m.type_ref.as_deref(), &m.params, subst, is_raw, &resolve_fn)
        }
        ResolvedTarget::BinaryMember {
            owner_fqn,
            member_index,
        } => {
            let bin = classpath.type_by_fqn(owner_fqn)?;
            let m = bin.members.get(*member_index)?;
            let mut scope = type_param_bounds_of_fqn(db, owner_fqn);
            for tp in &m.type_params {
                scope.insert(
                    tp.clone(),
                    m.type_param_bounds.get(tp).cloned().unwrap_or_default(),
                );
            }
            let resolve_fn = |raw: &str| {
                resolve_binary_type_ref_for_member(&classpath, bin, m.nullness_scope, raw, &scope)
            };
            build_substituted_member_detail(m.kind, m.type_ref.as_deref(), &m.params, subst, is_raw, &resolve_fn)
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        SourceDatabase,
        classpath::{BinaryMemberSymbol, BinaryOrigin, BinaryTypeSymbol, ClasspathIndex},
        resolve::{ResolvedSymbolKind, SymbolDetail, resolve_at_offset, resolve_type_of_symbol_at_offset},
        symbols::{MemberKind, ParamSummary, TypeKind},
        test_fixture::{cursor, db_with, db_with_files},
        ty::TypeArg,
    };
    use syntax::TextSize;

    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn bin_cls(
        fqn: &str,
        kind: TypeKind,
        type_params: &[&str],
        super_class: Option<&str>,
        super_class_generic: Option<&str>,
        interfaces: &[&str],
        interfaces_generic: &[&str],
        members: Vec<BinaryMemberSymbol>,
    ) -> BinaryTypeSymbol {
        let (pkg, name) = match fqn.rsplit_once('.') {
            Some((p, n)) => (p.to_string(), SmolStr::new(n)),
            None => (String::new(), SmolStr::new(fqn)),
        };
        BinaryTypeSymbol {
            name,
            qualified_name: fqn.to_string(),
            internal_name: fqn.replace('.', "/"),
            package: pkg,
            enclosing_type: None,
            kind,
            identity: if matches!(kind, TypeKind::Interface | TypeKind::Annotation) {
                crate::TypeIdentity::Unspecified
            } else {
                crate::TypeIdentity::Identity
            },
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: kind == TypeKind::Interface,
            nullness_scope: crate::NullnessScope::Inherited,
            type_params: type_params.iter().map(|s| SmolStr::new(*s)).collect(),
            type_param_bounds: BTreeMap::new(),
            super_class: super_class.map(str::to_string),
            super_class_generic: super_class_generic.map(str::to_string),
            interfaces: interfaces.iter().map(|s| (*s).to_string()).collect(),
            interfaces_generic: interfaces_generic.iter().map(|s| (*s).to_string()).collect(),
            permitted_subclasses: Vec::new(),
            members,
            nested_types: Vec::new(),
            origin: BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base")),
            },
        }
    }

    fn bin_method(
        owner: &str,
        name: &str,
        is_static: bool,
        is_abstract: bool,
        type_params: &[&str],
        ret: &str,
        params: &[(&str, &str, bool)],
    ) -> BinaryMemberSymbol {
        BinaryMemberSymbol {
            owner_type: owner.to_string(),
            name: SmolStr::new(name),
            kind: MemberKind::Method,
            is_public: true,
            is_static,
            is_abstract,
            nullness_scope: crate::NullnessScope::Inherited,
            decl_nullability: None,
            type_params: type_params.iter().map(|s| SmolStr::new(*s)).collect(),
            type_param_bounds: BTreeMap::new(),
            type_ref: Some(ret.to_string()),
            params: params
                .iter()
                .map(|(n, ty, va)| ParamSummary {
                    name: Some(SmolStr::new(*n)),
                    type_ref: (*ty).to_string(),
                    is_varargs: *va,
                    decl_nullability: None,
                })
                .collect(),
            is_varargs: params.last().is_some_and(|(_, _, va)| *va),
        }
    }

    fn bin_ctor(owner: &str, params: &[(&str, &str, bool)]) -> BinaryMemberSymbol {
        let simple = owner.rsplit('.').next().unwrap_or(owner);
        BinaryMemberSymbol {
            owner_type: owner.to_string(),
            name: SmolStr::new(simple),
            kind: MemberKind::Constructor,
            is_public: true,
            is_static: false,
            is_abstract: false,
            nullness_scope: crate::NullnessScope::Inherited,
            decl_nullability: None,
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            type_ref: None,
            params: params
                .iter()
                .map(|(n, ty, va)| ParamSummary {
                    name: Some(SmolStr::new(*n)),
                    type_ref: (*ty).to_string(),
                    is_varargs: *va,
                    decl_nullability: None,
                })
                .collect(),
            is_varargs: params.last().is_some_and(|(_, _, va)| *va),
        }
    }

    fn rich_jdk_classpath() -> ClasspathIndex {
        let mut cp = ClasspathIndex::default();
        cp.insert_type(bin_cls(
            "java.lang.Object",
            TypeKind::Class,
            &[],
            None,
            None,
            &[],
            &[],
            vec![
                bin_method("java.lang.Object", "toString", false, false, &[], "java.lang.String", &[]),
                bin_method(
                    "java.lang.Object",
                    "equals",
                    false,
                    false,
                    &[],
                    "boolean",
                    &[("obj", "java.lang.Object", false)],
                ),
                bin_method("java.lang.Object", "hashCode", false, false, &[], "int", &[]),
            ],
        ));
        cp.insert_type(bin_cls(
            "java.lang.String",
            TypeKind::Class,
            &[],
            Some("java.lang.Object"),
            None,
            &["java.lang.CharSequence"],
            &[],
            vec![bin_method("java.lang.String", "length", false, false, &[], "int", &[])],
        ));
        cp.insert_type(bin_cls(
            "java.lang.CharSequence",
            TypeKind::Interface,
            &[],
            None,
            None,
            &[],
            &[],
            vec![],
        ));
        cp.insert_type(bin_cls(
            "java.lang.Number",
            TypeKind::Class,
            &[],
            Some("java.lang.Object"),
            None,
            &[],
            &[],
            vec![],
        ));
        cp.insert_type(bin_cls(
            "java.lang.Integer",
            TypeKind::Class,
            &[],
            Some("java.lang.Number"),
            None,
            &[],
            &[],
            vec![bin_method(
                "java.lang.Integer",
                "valueOf",
                true,
                false,
                &[],
                "java.lang.Integer",
                &[("i", "int", false)],
            )],
        ));
        cp.insert_type(bin_cls(
            "java.lang.AutoCloseable",
            TypeKind::Interface,
            &[],
            None,
            None,
            &[],
            &[],
            vec![bin_method("java.lang.AutoCloseable", "close", false, true, &[], "void", &[])],
        ));
        cp.insert_type(bin_cls(
            "java.util.function.Function",
            TypeKind::Interface,
            &["T", "R"],
            None,
            None,
            &[],
            &[],
            vec![
                bin_method("java.util.function.Function", "apply", false, true, &[], "R", &[("t", "T", false)]),
                bin_method(
                    "java.util.function.Function",
                    "compose",
                    false,
                    false,
                    &["V"],
                    "java.util.function.Function<V, R>",
                    &[("before", "java.util.function.Function<? super V, ? extends T>", false)],
                ),
                bin_method(
                    "java.util.function.Function",
                    "identity",
                    true,
                    false,
                    &["T"],
                    "java.util.function.Function<T, T>",
                    &[],
                ),
            ],
        ));
        cp.insert_type(bin_cls(
            "java.util.function.Consumer",
            TypeKind::Interface,
            &["T"],
            None,
            None,
            &[],
            &[],
            vec![
                bin_method("java.util.function.Consumer", "accept", false, true, &[], "void", &[("t", "T", false)]),
                bin_method(
                    "java.util.function.Consumer",
                    "andThen",
                    false,
                    false,
                    &[],
                    "java.util.function.Consumer<T>",
                    &[("after", "java.util.function.Consumer<? super T>", false)],
                ),
            ],
        ));
        cp.insert_type(bin_cls(
            "java.util.stream.Stream",
            TypeKind::Interface,
            &["T"],
            None,
            None,
            &["java.lang.AutoCloseable"],
            &[],
            vec![
                bin_method(
                    "java.util.stream.Stream",
                    "map",
                    false,
                    true,
                    &["R"],
                    "java.util.stream.Stream<R>",
                    &[("mapper", "java.util.function.Function<? super T, ? extends R>", false)],
                ),
                bin_method(
                    "java.util.stream.Stream",
                    "toList",
                    false,
                    false,
                    &[],
                    "java.util.List<T>",
                    &[],
                ),
                bin_method(
                    "java.util.stream.Stream",
                    "forEach",
                    false,
                    true,
                    &[],
                    "void",
                    &[("action", "java.util.function.Consumer<? super T>", false)],
                ),
            ],
        ));
        cp.insert_type(bin_cls(
            "java.lang.Iterable",
            TypeKind::Interface,
            &["T"],
            None,
            None,
            &[],
            &[],
            vec![bin_method(
                "java.lang.Iterable",
                "forEach",
                false,
                false,
                &[],
                "void",
                &[("action", "java.util.function.Consumer<? super T>", false)],
            )],
        ));
        cp.insert_type(bin_cls(
            "java.util.Collection",
            TypeKind::Interface,
            &["E"],
            None,
            None,
            &["java.lang.Iterable"],
            &["java.lang.Iterable<E>"],
            vec![
                bin_method("java.util.Collection", "size", false, true, &[], "int", &[]),
                bin_method(
                    "java.util.Collection",
                    "stream",
                    false,
                    false,
                    &[],
                    "java.util.stream.Stream<E>",
                    &[],
                ),
            ],
        ));
        cp.insert_type(bin_cls(
            "java.util.List",
            TypeKind::Interface,
            &["E"],
            None,
            None,
            &["java.util.Collection"],
            &["java.util.Collection<E>"],
            vec![
                bin_method("java.util.List", "get", false, true, &[], "E", &[("index", "int", false)]),
                bin_method("java.util.List", "remove", false, true, &[], "E", &[("index", "int", false)]),
                bin_method(
                    "java.util.List",
                    "remove",
                    false,
                    true,
                    &[],
                    "boolean",
                    &[("o", "java.lang.Object", false)],
                ),
                bin_method(
                    "java.util.List",
                    "of",
                    true,
                    false,
                    &["E"],
                    "java.util.List<E>",
                    &[("e1", "E", false)],
                ),
            ],
        ));
        cp.insert_type(bin_cls(
            "java.util.ArrayList",
            TypeKind::Class,
            &["E"],
            Some("java.lang.Object"),
            Some("java.lang.Object"),
            &["java.util.List"],
            &["java.util.List<E>"],
            vec![
                bin_ctor("java.util.ArrayList", &[]),
                bin_ctor(
                    "java.util.ArrayList",
                    &[("c", "java.util.Collection<? extends E>", false)],
                ),
            ],
        ));
        cp.insert_type(bin_cls(
            "java.util.Collections",
            TypeKind::Class,
            &[],
            Some("java.lang.Object"),
            None,
            &[],
            &[],
            vec![bin_method(
                "java.util.Collections",
                "emptyList",
                true,
                false,
                &["T"],
                "java.util.List<T>",
                &[],
            )],
        ));
        cp.insert_type(bin_cls(
            "java.util.Optional",
            TypeKind::Class,
            &["T"],
            Some("java.lang.Object"),
            None,
            &[],
            &[],
            vec![
                bin_method(
                    "java.util.Optional",
                    "of",
                    true,
                    false,
                    &["T"],
                    "java.util.Optional<T>",
                    &[("value", "T", false)],
                ),
                bin_method("java.util.Optional", "get", false, false, &[], "T", &[]),
            ],
        ));
        cp.finalize();
        cp
    }

    fn cls(fqn: &str) -> Ty {
        Ty::raw_class(fqn)
    }

    fn cls_with(fqn: &str, args: Vec<Ty>) -> Ty {
        Ty::class(fqn, args.into_iter().map(TypeArg::Type).collect())
    }

    #[test]
    fn early_cutoff_preserves_item_tree_and_hierarchy_on_body_edits() {
        let initial = "\
package p;
import java.util.List;
class User { String getName() { return \"a\"; } }
class App {
    void run(List<User> users) {
        var u = users.get(0);
    }
}
";
        let edited = "\
package p;
import java.util.List;
class User { String getName() { return \"a\"; } }
class App {
    void run(List<User> users) {
        var names = users.stream().map(u -> u.getName()).toList();
        var x = \"hello\" + 42;
    }
}
";
        let (mut db, fid) = db_with(initial);
        let tree_before = db.file_item_tree(fid);
        let ws_before = db.workspace_index();
        let hier_before = db.workspace_hierarchy_index();

        db.set_file_text(fid, Arc::new(edited.to_string()));
        assert_eq!(tree_before, db.file_item_tree(fid));
        assert!(Arc::ptr_eq(&ws_before, &db.workspace_index()));
        assert!(Arc::ptr_eq(&hier_before, &db.workspace_hierarchy_index()));
    }

    #[test]
    fn cycle_and_malformed_code_safety() {
        let src = "\
package cyc;
class A<T extends A<T>> {}
class Demo {
    void broken() {
        var /*self*/x = x;
        var /*mut_a*/a = b;
        var b = a;
        var /*empty*/z = ;
        var /*call*/c = unknownObj.missing(
        A<?> rec = null;
        var r = rec;
    }
}
";
        let (db, fid) = db_with(src);
        let root = db.syntax_tree(fid);

        for marker in ["/*self*/", "/*mut_a*/", "/*empty*/"] {
            let off = TextSize::from(cursor(src, marker));
            let tok = root.token_at_offset(off).right_biased().unwrap();
            let decl = tok.parent().unwrap().parent().unwrap();
            assert_eq!(infer_local_ty(&db, fid, &decl), Ty::Unknown);
            let _ = resolve_at_offset(&db, fid, off.into());
            let _ = resolve_type_of_symbol_at_offset(&db, fid, off);
        }
        let call_off = TextSize::from(cursor(src, "/*call*/"));
        let call_tok = root.token_at_offset(call_off).right_biased().unwrap();
        let call_decl = call_tok.parent().unwrap().parent().unwrap();
        let _ = infer_local_ty(&db, fid, &call_decl);
        let _ = resolve_at_offset(&db, fid, call_off.into());
        let _ = resolve_type_of_symbol_at_offset(&db, fid, call_off);
    }

    #[test]
    fn generics_inheritance_and_wildcards_substitution() {
        let src = "\
package com.example;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Optional;

public class User {
    public String getName() { return \"\"; }
}
class UserList extends ArrayList<User> {}
class Demo {
    void test(
        List<User> users,
        List<? extends User> extUsers,
        List<? super User> supUsers
    ) {
        users./*list_get*/get(0)./*get_name*/getName();
        new UserList().get(0)./*userlist_get_name*/getName();
        for (var /*foreach_ul*/ulItem : new UserList()) {
            ulItem./*ul_item_name*/getName();
        }
        var /*ext_var*/eu = extUsers.get(0);
        var /*sup_var*/su = supUsers.get(0);
        var /*col_empty*/empties = Collections.<String>emptyList();
        var /*list_of*/ofUsers = List.of(new User());
        var /*opt_of*/optUser = Optional.of(new User());
        List<String> /*target_empty*/targetList = Collections.emptyList();
        /*target_empty_use*/targetList.get(0);
    }
}
";
        let mut db = db_with_files(&[(1, src)]);
        db.set_classpath_index(Arc::new(rich_jdk_classpath()));
        let fid = FileId(1);

        let list_get = resolve_at_offset(&db, fid, cursor(src, "/*list_get*/")).unwrap();
        assert_eq!(
            list_get.detail,
            Some(SymbolDetail::Method {
                return_type: Some("User".to_string()),
                params: "(int index)".to_string(),
            })
        );

        let get_name = resolve_at_offset(&db, fid, cursor(src, "/*get_name*/")).unwrap();
        assert_eq!(get_name.qualified_name, "com.example.User.getName");

        let ul_get_name = resolve_at_offset(&db, fid, cursor(src, "/*userlist_get_name*/")).unwrap();
        assert_eq!(ul_get_name.qualified_name, "com.example.User.getName");

        let foreach_ul = resolve_at_offset(&db, fid, cursor(src, "/*foreach_ul*/")).unwrap();
        assert_eq!(foreach_ul.detail, Some(SymbolDetail::TypeRef("User".to_string())));

        let ul_item_name = resolve_at_offset(&db, fid, cursor(src, "/*ul_item_name*/")).unwrap();
        assert_eq!(ul_item_name.qualified_name, "com.example.User.getName");

        let ext_var = resolve_at_offset(&db, fid, cursor(src, "/*ext_var*/")).unwrap();
        assert_eq!(ext_var.detail, Some(SymbolDetail::TypeRef("User".to_string())));

        let sup_var = resolve_at_offset(&db, fid, cursor(src, "/*sup_var*/")).unwrap();
        assert_eq!(sup_var.detail, Some(SymbolDetail::TypeRef("Object".to_string())));

        let col_empty = resolve_at_offset(&db, fid, cursor(src, "/*col_empty*/")).unwrap();
        assert_eq!(col_empty.detail, Some(SymbolDetail::TypeRef("List<String>".to_string())));

        let list_of = resolve_at_offset(&db, fid, cursor(src, "/*list_of*/")).unwrap();
        assert_eq!(list_of.detail, Some(SymbolDetail::TypeRef("List<User>".to_string())));

        let opt_of = resolve_at_offset(&db, fid, cursor(src, "/*opt_of*/")).unwrap();
        assert_eq!(opt_of.detail, Some(SymbolDetail::TypeRef("Optional<User>".to_string())));

        let root = db.syntax_tree(fid);
        let target_decl = root
            .token_at_offset(TextSize::from(cursor(src, "/*target_empty*/")))
            .right_biased()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let init_expr = target_decl.children().last().unwrap();
        let expected_list_str = cls_with("java.util.List", vec![Ty::string()]);
        assert_eq!(
            infer_expr_ty(&db, fid, &init_expr, Some(&expected_list_str)),
            expected_list_str
        );
    }

    #[test]
    fn var_and_all_expression_forms() {
        let src = "\
package com.example;
import java.util.ArrayList;
import java.util.List;

class Animal {}
class Dog extends Animal {}
class Cat extends Animal {}
class Box<T> { Box(T val) {} }
record Pair<A, B>(A first, B second) {}
class Res implements AutoCloseable {
    public void close() {}
    public int read() { return 1; }
}

class Demo {
    void run(boolean cond, int code, Object obj, String[] strArr, Dog dog, Cat cat) {
        var /*v_int*/n = 42;
        var /*v_str*/s = \"x\" + 1;
        var /*v_cls*/c = Demo.class;
        var /*v_pcls*/pc = int.class;
        List<Dog> /*v_diam*/dogs = new ArrayList<>();
        var /*v_diam_arg*/box = new Box<>(dog);
        var /*v_arr*/arr = new Dog[3];
        var /*v_arr_len*/len = arr.length;
        var /*v_arr_elem*/first = arr[0];
        var /*v_tern_num*/num = cond ? 1 : 2.5;
        var /*v_tern_ref*/pet = cond ? dog : cat;
        var /*v_sw*/sw = switch (code) {
            case 1 -> dog;
            default -> { yield cat; }
        };
        for (var /*v_for_arr*/item : strArr) {
            item./*arr_item_len*/length();
        }
        try (var /*v_res*/r = new Res()) {
            r./*res_read*/read();
        }
        if (obj instanceof Pair<String, Dog>(var /*v_pat_a*/pa, var /*v_pat_b*/pb)) {
            pa./*pat_a_len*/length();
        }
    }
}
";
        let mut db = db_with_files(&[(1, src)]);
        db.set_classpath_index(Arc::new(rich_jdk_classpath()));
        let fid = FileId(1);

        let check_var = |marker: &str, expected: &str| {
            let res = resolve_at_offset(&db, fid, cursor(src, marker))
                .unwrap_or_else(|| panic!("missing resolved symbol at {marker}"));
            assert_eq!(
                res.detail,
                Some(SymbolDetail::TypeRef(expected.to_string())),
                "mismatch at {marker}"
            );
        };

        check_var("/*v_int*/", "int");
        check_var("/*v_str*/", "String");
        check_var("/*v_cls*/", "Class<Demo>");
        check_var("/*v_pcls*/", "Class<Integer>");
        check_var("/*v_diam_arg*/", "Box<Dog>");
        check_var("/*v_arr*/", "Dog[]");
        check_var("/*v_arr_len*/", "int");
        check_var("/*v_arr_elem*/", "Dog");
        check_var("/*v_tern_num*/", "double");
        check_var("/*v_tern_ref*/", "Animal");
        check_var("/*v_sw*/", "Animal");
        check_var("/*v_for_arr*/", "String");
        check_var("/*v_res*/", "Res");
        check_var("/*v_pat_a*/", "String");
        check_var("/*v_pat_b*/", "Dog");

        // Verify diamond `new ArrayList<>()` with expected `List<Dog>` resolves to `ArrayList<Dog>`.
        let root = db.syntax_tree(fid);
        let diam_decl = root
            .token_at_offset(TextSize::from(cursor(src, "/*v_diam*/")))
            .right_biased()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let new_expr = diam_decl.children().last().unwrap();
        let exp = cls_with("java.util.List", vec![cls("com.example.Dog")]);
        assert_eq!(
            infer_expr_ty(&db, fid, &new_expr, Some(&exp)),
            cls_with("java.util.ArrayList", vec![cls("com.example.Dog")])
        );

        assert_eq!(
            resolve_at_offset(&db, fid, cursor(src, "/*arr_item_len*/"))
                .unwrap()
                .qualified_name,
            "java.lang.String.length"
        );
        assert_eq!(
            resolve_at_offset(&db, fid, cursor(src, "/*res_read*/"))
                .unwrap()
                .qualified_name,
            "com.example.Res.read"
        );
        assert_eq!(
            resolve_at_offset(&db, fid, cursor(src, "/*pat_a_len*/"))
                .unwrap()
                .qualified_name,
            "java.lang.String.length"
        );
    }

    #[test]
    fn lambdas_method_references_and_sam_resolution() {
        let src = "\
package com.example;
import java.util.List;
import java.util.function.Function;

interface ExtraBase {
    void go();
}
interface NotSam extends ExtraBase {
    void stop();
}
interface ObjectOverrideSam {
    boolean equals(Object other);
    String toString();
    int hashCode();
    void run(String s);
    default void helper() {}
    static void util() {}
}

public class User {
    public String getName() { return \"\"; }
    public void ping() {}
}

class Pipeline {
    void run(List<User> users) {
        var /*res_list*/names = users.stream()./*map_call*/map(/*lparam*/u -> /*luse*/u./*get_name*/getName())./*to_list*/toList();
        users.forEach(User::/*mref_ping*/ping);
        Function<User, String> fn = /*fn_lparam*/u -> u.getName();
    }
}
";
        let mut db = db_with_files(&[(1, src)]);
        db.set_classpath_index(Arc::new(rich_jdk_classpath()));
        let fid = FileId(1);

        // SAM identification tests.
        let func_ty = cls_with(
            "java.util.function.Function",
            vec![cls("com.example.User"), Ty::string()],
        );
        let func_sam = find_functional_interface_sam(&db, &func_ty).unwrap();
        assert_eq!(func_sam.method_name, "apply");
        assert_eq!(func_sam.param_types, vec![cls("com.example.User")]);
        assert_eq!(func_sam.return_type, Ty::string());

        assert!(find_functional_interface_sam(&db, &cls("com.example.NotSam")).is_none());
        let obj_sam = find_functional_interface_sam(&db, &cls("com.example.ObjectOverrideSam")).unwrap();
        assert_eq!(obj_sam.method_name, "run");
        assert_eq!(obj_sam.param_types, vec![Ty::string()]);
        assert_eq!(obj_sam.return_type, Ty::Void);

        // Stream pipeline inference.
        let names = resolve_at_offset(&db, fid, cursor(src, "/*res_list*/")).unwrap();
        assert_eq!(names.detail, Some(SymbolDetail::TypeRef("List<String>".to_string())));

        let map_call = resolve_at_offset(&db, fid, cursor(src, "/*map_call*/")).unwrap();
        assert_eq!(map_call.qualified_name, "java.util.stream.Stream.map");
        assert_eq!(
            map_call.detail,
            Some(SymbolDetail::Method {
                return_type: Some("Stream<String>".to_string()),
                params: "(Function<? super User, ? extends String> mapper)".to_string(),
            })
        );

        let lparam = resolve_at_offset(&db, fid, cursor(src, "/*lparam*/")).unwrap();
        assert_eq!(lparam.kind, ResolvedSymbolKind::LambdaParam);
        assert_eq!(lparam.detail, Some(SymbolDetail::TypeRef("User".to_string())));

        let luse = resolve_at_offset(&db, fid, cursor(src, "/*luse*/")).unwrap();
        assert_eq!(luse.detail, Some(SymbolDetail::TypeRef("User".to_string())));

        let get_name = resolve_at_offset(&db, fid, cursor(src, "/*get_name*/")).unwrap();
        assert_eq!(get_name.qualified_name, "com.example.User.getName");

        let to_list = resolve_at_offset(&db, fid, cursor(src, "/*to_list*/")).unwrap();
        assert_eq!(
            to_list.detail,
            Some(SymbolDetail::Method {
                return_type: Some("List<String>".to_string()),
                params: "()".to_string(),
            })
        );

        let mref_ping = resolve_at_offset(&db, fid, cursor(src, "/*mref_ping*/")).unwrap();
        assert_eq!(mref_ping.qualified_name, "com.example.User.ping");

        let fn_lparam = resolve_at_offset(&db, fid, cursor(src, "/*fn_lparam*/")).unwrap();
        assert_eq!(fn_lparam.detail, Some(SymbolDetail::TypeRef("User".to_string())));
    }

    #[test]
    fn type_based_overload_resolution_jls_phases_and_specificity() {
        let src = "\
package com.example;
import java.util.List;

class Base {
    void cross(String s) {}
}

class Overload extends Base {
    void cross(int x) {}

    void foo(int x) {}
    void foo(String s) {}

    void numOrInt(Number n) {}
    void numOrInt(Integer i) {}

    void objOrStr(Object o) {}
    void objOrStr(String s) {}

    void widenOrBox(long l) {}
    void widenOrBox(Integer i) {}

    void varargsTest(int a) {}
    void varargsTest(int a, int... rest) {}

    void test(List<String> xs, Integer boxedInt) {
        /*foo_int*/foo(1);
        /*foo_str*/foo(\"hi\");
        /*num_int*/numOrInt(boxedInt);
        /*num_prim*/numOrInt(1);
        /*obj_str*/objOrStr(\"hi\");
        /*widen_box*/widenOrBox(1);
        /*va_1*/varargsTest(1);
        /*va_2*/varargsTest(1, 2, 3);
        /*cross_str*/cross(\"hello\");
        /*cross_int*/cross(5);
        xs./*rem_idx*/remove(0);
        xs./*rem_obj*/remove(\"item\");
    }
}
";
        let mut db = db_with_files(&[(1, src)]);
        db.set_classpath_index(Arc::new(rich_jdk_classpath()));
        let fid = FileId(1);

        let check_params = |marker: &str, expected_params: &str| {
            let res = resolve_at_offset(&db, fid, cursor(src, marker)).unwrap();
            assert_eq!(
                res.detail,
                Some(SymbolDetail::Method {
                    return_type: Some("void".to_string()),
                    params: expected_params.to_string(),
                }),
                "at {marker}"
            );
        };

        check_params("/*foo_int*/", "(int x)");
        check_params("/*foo_str*/", "(String s)");
        check_params("/*num_int*/", "(Integer i)");
        check_params("/*num_prim*/", "(Integer i)");
        check_params("/*obj_str*/", "(String s)");
        check_params("/*widen_box*/", "(long l)");
        check_params("/*va_1*/", "(int a)");
        check_params("/*va_2*/", "(int a, int... rest)");
        check_params("/*cross_str*/", "(String s)");
        check_params("/*cross_int*/", "(int x)");

        let rem_idx = resolve_at_offset(&db, fid, cursor(src, "/*rem_idx*/")).unwrap();
        assert_eq!(
            rem_idx.detail,
            Some(SymbolDetail::Method {
                return_type: Some("String".to_string()),
                params: "(int index)".to_string(),
            })
        );

        let rem_obj = resolve_at_offset(&db, fid, cursor(src, "/*rem_obj*/")).unwrap();
        assert_eq!(
            rem_obj.detail,
            Some(SymbolDetail::Method {
                return_type: Some("boolean".to_string()),
                params: "(java.lang.Object o)".to_string(),
            })
        );
    }

    #[test]
    fn type_definition_on_var_lambda_param_and_arrays() {
        let src = "\
package com.example;
import java.util.List;

public class User {
    public String getName() { return \"\"; }
}

class Nav {
    void demo(List<User> users, User[] arr) {
        var /*var_decl*/u = users.get(0);
        /*var_use*/u.getName();
        users.stream().map(/*lparam*/x -> /*lparam_use*/x.getName());
        var /*arr_decl*/copy = arr;
        /*arr_use*/copy[0].getName();
        users./*call_get*/get(0);
    }
}
";
        let mut db = db_with_files(&[(1, src)]);
        db.set_classpath_index(Arc::new(rich_jdk_classpath()));
        let fid = FileId(1);

        for marker in [
            "/*var_decl*/",
            "/*var_use*/",
            "/*lparam*/",
            "/*lparam_use*/",
            "/*arr_decl*/",
            "/*arr_use*/",
            "/*call_get*/",
        ] {
            let td = resolve_type_of_symbol_at_offset(&db, fid, TextSize::from(cursor(src, marker)))
                .unwrap_or_else(|| panic!("expected typeDefinition at {marker}"));
            assert_eq!(td.qualified_name, "com.example.User", "at {marker}");
            assert_eq!(td.file_id, Some(fid));
        }
    }

    #[test]
    fn nullability_var_lub_local_root_and_binary_calls() {
        let user_src = "\
package com.example;
import org.jspecify.annotations.*;

@NullMarked
public class User {
    public String getName() { return \"\"; }
}
";
        let app_src = "\
package com.example;
import org.jspecify.annotations.*;
import java.util.List;

enum Role { ADMIN, GUEST }

@NullMarked
class MarkedApp {
    void run(boolean flag, List<String> /*param_list*/xs, BinaryRepo repo) {
        var /*v_user*/u = new User();
        var /*v_lub*/maybe = flag ? u : null;
        var /*v_str*/s = \"hello\";
        var /*v_cls*/c = User.class;
        var /*v_this*/self = this;
        var /*v_enum*/role = Role.ADMIN;
        String /*loc_plain*/plainLocal = \"x\";
        @Nullable String /*loc_q*/nullableLocal = null;
        List<String> /*loc_list*/listLocal = xs;
        var /*v_bin_find*/found = repo./*call_find*/findUser(\"1\");
        var /*v_bin_req*/req = repo./*call_req*/requireUser(\"1\");
    }

    @NullUnmarked
    void legacyMethod(User /*unmarked_param*/u) {
        var /*unmarked_var*/x = new User();
    }
}

class UnmarkedCaller {
    void call(BinaryRepo repo) {
        var /*ext_find*/f = repo.findUser(\"1\");
        var /*ext_req*/r = repo.requireUser(\"1\");
        var /*ext_user*/u = new User();
    }
}
";
        let mut cp = rich_jdk_classpath();
        let mut bin_repo = bin_cls(
            "com.example.BinaryRepo",
            TypeKind::Interface,
            &[],
            None,
            None,
            &[],
            &[],
            vec![
                bin_method(
                    "com.example.BinaryRepo",
                    "findUser",
                    false,
                    true,
                    &[],
                    "com.example.User?",
                    &[("id", "java.lang.String", false)],
                ),
                bin_method(
                    "com.example.BinaryRepo",
                    "requireUser",
                    false,
                    true,
                    &[],
                    "com.example.User",
                    &[("id", "java.lang.String", false)],
                ),
            ],
        );
        bin_repo.nullness_scope =
            crate::NullnessScope::NonNullByDefault(crate::NullabilitySource::JSpecify);
        cp.insert_type(bin_repo);
        cp.finalize();

        let mut db = db_with_files(&[(1, user_src), (2, app_src)]);
        db.set_classpath_index(Arc::new(cp));
        let fid = FileId(2);

        let check_var = |marker: &str, expected: &str| {
            let res = resolve_at_offset(&db, fid, cursor(app_src, marker))
                .unwrap_or_else(|| panic!("missing symbol at {marker}"));
            assert_eq!(
                res.detail,
                Some(SymbolDetail::TypeRef(expected.to_string())),
                "mismatch at {marker}"
            );
        };

        check_var("/*param_list*/", "List<String!>!");
        check_var("/*v_user*/", "User!");
        check_var("/*v_lub*/", "User?");
        check_var("/*v_str*/", "String!");
        check_var("/*v_cls*/", "Class<User!>!");
        check_var("/*v_this*/", "MarkedApp!");
        check_var("/*v_enum*/", "Role!");
        // JSpecify §3.2.5: local variable root is NOT forced NonNull, nested type args ARE.
        check_var("/*loc_plain*/", "String");
        check_var("/*loc_q*/", "String?");
        check_var("/*loc_list*/", "List<String!>");
        check_var("/*v_bin_find*/", "User?");
        check_var("/*v_bin_req*/", "User!");

        // `@NullUnmarked` method inside `@NullMarked` class resets defaulting.
        check_var("/*unmarked_param*/", "User");
        check_var("/*unmarked_var*/", "User");

        // Unmarked caller still receives `User?` / `User!` from `@NullMarked` binary JAR methods,
        // while its own `new User()` stays `User` (zero noise on unmarked code).
        check_var("/*ext_find*/", "User?");
        check_var("/*ext_req*/", "User!");
        check_var("/*ext_user*/", "User");

        let call_find = resolve_at_offset(&db, fid, cursor(app_src, "/*call_find*/")).unwrap();
        assert_eq!(
            call_find.detail,
            Some(SymbolDetail::Method {
                return_type: Some("User?".to_string()),
                params: "(String! id)".to_string(),
            })
        );
        let call_req = resolve_at_offset(&db, fid, cursor(app_src, "/*call_req*/")).unwrap();
        assert_eq!(
            call_req.detail,
            Some(SymbolDetail::Method {
                return_type: Some("User!".to_string()),
                params: "(String! id)".to_string(),
            })
        );
    }
}
