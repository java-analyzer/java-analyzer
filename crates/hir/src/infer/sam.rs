use std::collections::BTreeMap;

use rustc_hash::FxHashSet;
use smol_str::SmolStr;

use crate::{
    DefDatabase, FileId,
    classpath::{BinaryMemberSymbol, BinaryTypeSymbol, ClasspathIndex},
    hierarchy::linearized_supertypes_in_scope,
    item_tree::MemberItem,
    scope::VisibleScope,
    symbols::{MemberKind, ParamSummary, TypeKind},
    ty::{
        MAX_SUBST_DEPTH, Nullability, PrimitiveType, Ty, resolve_binary_type_ref_for_member,
        resolve_binary_type_ref_for_owner, resolve_type_ref_in_file_for_member,
        resolve_type_ref_in_file_for_owner, supertype_substitution_in_scope,
        type_param_bounds_of_fqn_in_scope, type_params_of_fqn_in_scope,
    },
};

use super::{
    OBJECT_FQN,
    scope::{resolve_params_in_binary_for_member, resolve_params_in_file_for_member},
};

/// Signature of the Single Abstract Method (SAM) of a functional interface,
/// instantiated with the target interface's type arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SamSignature {
    pub method_name: SmolStr,
    pub param_types: Vec<Ty>,
    pub return_type: Ty,
}

/// Finds the Single Abstract Method (SAM) of `ty` if `ty` denotes a functional interface
/// (JLS §9.8), returning its parameter and return types substituted with `ty`'s type arguments.
#[must_use]
pub fn find_functional_interface_sam(db: &dyn DefDatabase, ty: &Ty) -> Option<SamSignature> {
    let scope = VisibleScope::flat(db);
    find_functional_interface_sam_in_scope(db, &scope, ty)
}

#[must_use]
pub(crate) fn find_functional_interface_sam_in_scope(
    db: &dyn DefDatabase,
    scope: &VisibleScope,
    ty: &Ty,
) -> Option<SamSignature> {
    let (fqn, type_args) = match ty {
        Ty::Class { fqn, type_args, .. } => (fqn.as_str(), type_args.as_slice()),
        _ => return None,
    };

    let classpath = scope.classpath();

    let is_interface = if let Some((fid, tid)) = scope.get_workspace_type(fqn) {
        db.file_item_tree(fid).types[tid].kind == TypeKind::Interface
    } else if let Some(bin) = scope.get_binary_type(fqn) {
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
    for sup in linearized_supertypes_in_scope(db, scope, fqn) {
        if sup != OBJECT_FQN {
            ordered_ifaces.push(sup);
        }
    }

    for iface_fqn in ordered_ifaces {
        if !visited_ifaces.insert(iface_fqn.clone()) {
            continue;
        }
        if let Some((fid, tid)) = scope.get_workspace_type(&iface_fqn) {
            let tree = db.file_item_tree(fid);
            let ty_item = &tree.types[tid];
            if ty_item.kind != TypeKind::Interface {
                continue;
            }
            let owner_scope = type_param_bounds_of_fqn_in_scope(db, scope, &iface_fqn);
            for &mid in &ty_item.members {
                let m = &tree.members[mid];
                if m.kind != MemberKind::Method || m.is_static {
                    continue;
                }
                let sig_key = sam_signature_key(
                    db,
                    Some(fid),
                    Some(&iface_fqn),
                    None,
                    &m.name,
                    &m.params,
                    &owner_scope,
                );
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
                let ret_ty = m.type_ref.as_deref().map_or(Ty::Void, |r| {
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
        } else if let Some(bin) = scope.get_binary_type(&iface_fqn) {
            if bin.kind != TypeKind::Interface {
                continue;
            }
            let owner_scope = type_param_bounds_of_fqn_in_scope(db, scope, &iface_fqn);
            for m in &bin.members {
                if m.kind != MemberKind::Method || m.is_static {
                    continue;
                }
                let sig_key = sam_signature_key(
                    db,
                    None,
                    None,
                    Some(bin),
                    &m.name,
                    &m.params,
                    &owner_scope,
                );
                if !seen_sigs.insert(sig_key) {
                    continue;
                }
                if !m.is_abstract {
                    continue;
                }
                if is_object_public_method_binary(classpath, bin, m, &owner_scope) {
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
                    classpath,
                    bin,
                    m.nullness_scope,
                    &m.params,
                    &m_scope,
                );
                let ret_ty = m.type_ref.as_deref().map_or(Ty::Void, |r| {
                    resolve_binary_type_ref_for_member(
                        classpath,
                        bin,
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
        }
    }

    if candidates.len() != 1 {
        return None;
    }
    let sam = candidates.into_iter().next()?;
    let subst = supertype_substitution_in_scope(db, scope, fqn, type_args, &sam.owner_fqn)
        .unwrap_or_default();

    // Raw functional interface: erase class type variables to Object/bounds if `type_args` is empty
    // and the interface declares type parameters.
    let raw_iface = !type_params_of_fqn_in_scope(db, scope, fqn).is_empty() && type_args.is_empty();
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
            let p_ty =
                resolve_type_ref_in_file_for_owner(db, fid, Some(owner_fqn), &p.type_ref, scope)
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
