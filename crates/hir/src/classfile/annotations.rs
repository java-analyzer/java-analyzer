//! RuntimeVisible* / RuntimeInvisible* annotation and JVMS §4.7.20 `TypePath` nullability decoders.

use std::collections::BTreeMap;

use ristretto_classfile::{
    ConstantPool,
    attributes::{Annotation, Attribute, TargetPath, TargetType, TypeAnnotation},
};
use smol_str::{SmolStr, format_smolstr};

use crate::{
    symbols::{
        NullabilitySource, NullnessScope, ParamSummary, classify_nullability_annotation,
        classify_nullness_scope_annotation, is_type_use_nullability_annotation,
    },
    ty::Nullability,
};

use super::signature::{
    DecodedClassSignature, DecodedMethodSignature, DecodedType, DecodedWildcard,
    render_type_param_bounds,
};

pub(super) struct MethodSigCtx<'a> {
    pub(super) attributes: &'a [Attribute],
    pub(super) cp: &'a ConstantPool<'a>,
    pub(super) is_ctor: bool,
    pub(super) has_varargs_flag: bool,
    pub(super) method_decl: Option<(Nullability, NullabilitySource, bool)>,
    pub(super) param_decls: &'a [Option<(Nullability, NullabilitySource, bool)>],
    pub(super) param_names: &'a [Option<SmolStr>],
}

pub(super) struct FinalizedMethodSig {
    pub(super) type_params: Vec<SmolStr>,
    pub(super) type_param_bounds: BTreeMap<SmolStr, Vec<String>>,
    pub(super) params: Vec<ParamSummary>,
    pub(super) return_type: Option<String>,
    pub(super) is_varargs: bool,
    pub(super) decl_nullability: Option<(Nullability, NullabilitySource)>,
}

fn annotation_descriptor_to_fqn(cp: &ConstantPool<'_>, type_index: u16) -> Option<String> {
    let raw = cp.try_get_utf8(type_index).ok()?.to_rust_string();
    let inner = raw.strip_prefix('L')?.strip_suffix(';')?;
    if inner.is_empty() {
        return None;
    }
    Some(inner.replace(['/', '$'], "."))
}

fn collect_annotations(attributes: &[Attribute]) -> Vec<&Annotation> {
    let mut out = Vec::new();
    for attr in attributes {
        match attr {
            Attribute::RuntimeVisibleAnnotations { annotations, .. }
            | Attribute::RuntimeInvisibleAnnotations { annotations, .. } => {
                out.extend(annotations.iter());
            }
            _ => {}
        }
    }
    out
}

fn collect_type_annotations(attributes: &[Attribute]) -> Vec<&TypeAnnotation> {
    let mut out = Vec::new();
    for attr in attributes {
        match attr {
            Attribute::RuntimeVisibleTypeAnnotations {
                type_annotations, ..
            }
            | Attribute::RuntimeInvisibleTypeAnnotations {
                type_annotations, ..
            } => {
                out.extend(type_annotations.iter());
            }
            _ => {}
        }
    }
    out
}

pub(super) fn extract_nullness_scope(
    attributes: &[Attribute],
    cp: &ConstantPool<'_>,
) -> NullnessScope {
    let mut result = NullnessScope::Inherited;
    for ann in collect_annotations(attributes) {
        let Some(fqn) = annotation_descriptor_to_fqn(cp, ann.type_index) else {
            continue;
        };
        if let Some(scope) = classify_nullness_scope_annotation(&fqn) {
            match (result, scope) {
                (NullnessScope::Inherited, _) => result = scope,
                (
                    NullnessScope::NonNullByDefault(NullabilitySource::ThirdPartyAnnotation)
                    | NullnessScope::Unspecified(NullabilitySource::ThirdPartyAnnotation),
                    NullnessScope::NonNullByDefault(NullabilitySource::JSpecify)
                    | NullnessScope::Unspecified(NullabilitySource::JSpecify),
                ) => result = scope,
                _ => {}
            }
        }
    }
    result
}

fn classify_annotations_slice(
    annotations: &[Annotation],
    cp: &ConstantPool<'_>,
) -> Option<(Nullability, NullabilitySource, bool)> {
    let mut best: Option<(Nullability, NullabilitySource, bool)> = None;
    for ann in annotations {
        let Some(fqn) = annotation_descriptor_to_fqn(cp, ann.type_index) else {
            continue;
        };
        if let Some((nullability, source)) = classify_nullability_annotation(&fqn) {
            let is_type_use = is_type_use_nullability_annotation(&fqn);
            match best {
                None => best = Some((nullability, source, is_type_use)),
                Some((_, NullabilitySource::ThirdPartyAnnotation, _))
                    if source == NullabilitySource::JSpecify =>
                {
                    best = Some((nullability, source, is_type_use));
                }
                _ => {}
            }
        }
    }
    best
}

pub(super) fn extract_decl_nullability(
    attributes: &[Attribute],
    cp: &ConstantPool<'_>,
) -> Option<(Nullability, NullabilitySource, bool)> {
    let mut best: Option<(Nullability, NullabilitySource, bool)> = None;
    for ann in collect_annotations(attributes) {
        let Some(fqn) = annotation_descriptor_to_fqn(cp, ann.type_index) else {
            continue;
        };
        if let Some((nullability, source)) = classify_nullability_annotation(&fqn) {
            let is_type_use = is_type_use_nullability_annotation(&fqn);
            match best {
                None => best = Some((nullability, source, is_type_use)),
                Some((_, NullabilitySource::ThirdPartyAnnotation, _))
                    if source == NullabilitySource::JSpecify =>
                {
                    best = Some((nullability, source, is_type_use));
                }
                _ => {}
            }
        }
    }
    best
}

pub(super) fn extract_param_decl_nullabilities(
    attributes: &[Attribute],
    cp: &ConstantPool<'_>,
    param_count: usize,
) -> Vec<Option<(Nullability, NullabilitySource, bool)>> {
    let mut result = vec![None; param_count];
    for attr in attributes {
        let param_anns = match attr {
            Attribute::RuntimeVisibleParameterAnnotations {
                parameter_annotations,
                ..
            }
            | Attribute::RuntimeInvisibleParameterAnnotations {
                parameter_annotations,
                ..
            } => parameter_annotations,
            _ => continue,
        };
        let offset = param_count.saturating_sub(param_anns.len());
        for (i, pa) in param_anns.iter().enumerate() {
            let target_idx = offset + i;
            let Some(slot) = result.get_mut(target_idx) else {
                continue;
            };
            if let Some(found @ (_, source, _)) = classify_annotations_slice(&pa.annotations, cp) {
                match *slot {
                    None => *slot = Some(found),
                    Some((_, NullabilitySource::ThirdPartyAnnotation, _))
                        if source == NullabilitySource::JSpecify =>
                    {
                        *slot = Some(found);
                    }
                    _ => {}
                }
            }
        }
    }
    result
}

fn sorted_type_annotations<'a>(
    attributes: &'a [Attribute],
    cp: &ConstantPool<'_>,
) -> Vec<(&'a TypeAnnotation, Nullability, NullabilitySource)> {
    let mut collected = Vec::new();
    for ta in collect_type_annotations(attributes) {
        let Some(fqn) = annotation_descriptor_to_fqn(cp, ta.type_index) else {
            continue;
        };
        if let Some((nullability, source)) = classify_nullability_annotation(&fqn) {
            collected.push((ta, nullability, source));
        }
    }
    // Apply ThirdPartyAnnotation first so JSpecify takes precedence when both target the same node.
    collected.sort_by_key(|(_, _, src)| match src {
        NullabilitySource::JSpecify => 1u8,
        _ => 0u8,
    });
    collected
}

fn apply_bound_type_annotation(
    bounds_entries: &mut [(SmolStr, bool, Vec<DecodedType>)],
    type_parameter_index: u8,
    bound_index: u8,
    type_path: &[TargetPath],
    nullability: Nullability,
) {
    let Some((_, has_explicit_class_bound, bounds)) =
        bounds_entries.get_mut(type_parameter_index as usize)
    else {
        return;
    };
    let idx = if *has_explicit_class_bound {
        Some(bound_index as usize)
    } else if bound_index >= 1 {
        Some((bound_index - 1) as usize)
    } else {
        None
    };
    if let Some(bound_ty) = idx.and_then(|i| bounds.get_mut(i)) {
        bound_ty.apply_type_path(type_path, nullability);
    }
}

pub(super) fn apply_class_type_annotations(
    attributes: &[Attribute],
    cp: &ConstantPool<'_>,
    decoded: &mut DecodedClassSignature,
) {
    for (ta, nullability, _) in sorted_type_annotations(attributes, cp) {
        match ta.target_type {
            TargetType::TypeParameterBound {
                target_type: 0x11,
                type_parameter_index,
                bound_index,
            } => {
                apply_bound_type_annotation(
                    &mut decoded.type_param_bounds,
                    type_parameter_index,
                    bound_index,
                    &ta.type_path,
                    nullability,
                );
            }
            TargetType::SuperType {
                target_type: 0x10,
                supertype_index,
            } => {
                if supertype_index == u16::MAX {
                    if let Some(sup) = decoded.super_class_generic.as_mut() {
                        sup.apply_type_path(&ta.type_path, nullability);
                    }
                } else if let Some(iface) = decoded
                    .interfaces_generic
                    .get_mut(supertype_index as usize)
                {
                    iface.apply_type_path(&ta.type_path, nullability);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn finalize_field_type_and_nullability(
    decoded_ty: Option<DecodedType>,
    attributes: &[Attribute],
    cp: &ConstantPool<'_>,
    decl_ann: Option<(Nullability, NullabilitySource, bool)>,
) -> (Option<String>, Option<(Nullability, NullabilitySource)>) {
    let Some(mut ty) = decoded_ty else {
        return (None, decl_ann.map(|(n, s, _)| (n, s)));
    };
    let mut has_type_ann = false;
    for (ta, nullability, _) in sorted_type_annotations(attributes, cp) {
        if matches!(ta.target_type, TargetType::Empty { target_type: 0x13 }) {
            ty.apply_type_path(&ta.type_path, nullability);
            has_type_ann = true;
        }
    }
    let decl_nullability =
        apply_decl_fallback_to_decoded(&mut ty, decl_ann, has_type_ann);
    (Some(ty.render()), decl_nullability)
}

pub(super) fn finalize_method_signature(
    decoded: &mut DecodedMethodSignature,
    ctx: &MethodSigCtx<'_>,
) -> FinalizedMethodSig {
    let mut return_has_type_ann = false;
    let mut param_has_type_ann = vec![false; decoded.params.len()];

    for (ta, nullability, _) in sorted_type_annotations(ctx.attributes, ctx.cp) {
        match ta.target_type {
            TargetType::Empty { target_type: 0x14 } if !ctx.is_ctor => {
                if let Some(ret) = decoded.return_type.as_mut()
                    && !ret.is_void()
                {
                    ret.apply_type_path(&ta.type_path, nullability);
                    return_has_type_ann = true;
                }
            }
            TargetType::FormalParameter {
                target_type: 0x16,
                formal_parameter_index,
            } => {
                let idx = formal_parameter_index as usize;
                if let Some(param_ty) = decoded.params.get_mut(idx) {
                    param_ty.apply_type_path(&ta.type_path, nullability);
                    if let Some(flag) = param_has_type_ann.get_mut(idx) {
                        *flag = true;
                    }
                }
            }
            TargetType::TypeParameterBound {
                target_type: 0x12,
                type_parameter_index,
                bound_index,
            } => {
                apply_bound_type_annotation(
                    &mut decoded.type_param_bounds,
                    type_parameter_index,
                    bound_index,
                    &ta.type_path,
                    nullability,
                );
            }
            _ => {}
        }
    }

    let decl_nullability = if ctx.is_ctor {
        None
    } else if let Some(ret) = decoded.return_type.as_mut() {
        apply_decl_fallback_to_decoded(ret, ctx.method_decl, return_has_type_ann)
    } else {
        None
    };

    let is_varargs =
        ctx.has_varargs_flag && matches!(decoded.params.last(), Some(DecodedType::Array { .. }));
    let last_idx = decoded.params.len().saturating_sub(1);
    let params = std::mem::take(&mut decoded.params)
        .into_iter()
        .enumerate()
        .map(|(i, mut ty)| {
            let param_varargs = is_varargs && i == last_idx;
            let p_decl_raw = ctx.param_decls.get(i).copied().flatten();
            let p_has_type_ann = param_has_type_ann.get(i).copied().unwrap_or(false);
            let p_decl = if param_varargs {
                if let DecodedType::Array { elem, nullability } = ty {
                    let mut elem_ty = *elem;
                    let outer_decl = match p_decl_raw {
                        Some((n, src, true)) => {
                            if !p_has_type_ann {
                                elem_ty.apply_decl_fallback(n, true);
                            }
                            nullability.map(|an| (an, src))
                        }
                        Some((n, src, false)) => Some((n, src)),
                        None => nullability.map(|an| (an, NullabilitySource::JSpecify)),
                    };
                    ty = elem_ty;
                    outer_decl
                } else {
                    apply_decl_fallback_to_decoded(&mut ty, p_decl_raw, p_has_type_ann)
                }
            } else {
                apply_decl_fallback_to_decoded(&mut ty, p_decl_raw, p_has_type_ann)
            };

            let pname = ctx
                .param_names
                .get(i)
                .and_then(|opt| opt.clone())
                .unwrap_or_else(|| format_smolstr!("arg{i}"));
            ParamSummary {
                name: Some(pname),
                type_ref: ty.render(),
                is_varargs: param_varargs,
                decl_nullability: p_decl,
            }
        })
        .collect();

    let return_type = decoded.return_type.as_ref().map(DecodedType::render);
    let type_param_bounds = render_type_param_bounds(&decoded.type_param_bounds);
    FinalizedMethodSig {
        type_params: std::mem::take(&mut decoded.type_params),
        type_param_bounds,
        params,
        return_type,
        is_varargs,
        decl_nullability,
    }
}

fn apply_decl_fallback_to_decoded(
    ty: &mut DecodedType,
    decl_ann: Option<(Nullability, NullabilitySource, bool)>,
    has_type_ann: bool,
) -> Option<(Nullability, NullabilitySource)> {
    let (n, src, is_type_use) = decl_ann?;
    if is_type_use && ty.is_array() {
        if !has_type_ann {
            ty.apply_decl_fallback(n, true);
        }
        None
    } else {
        if !has_type_ann || !is_type_use {
            ty.apply_decl_fallback(n, is_type_use);
        }
        Some((n, src))
    }
}

impl DecodedType {
    fn apply_type_path(&mut self, path: &[TargetPath], nullability: Nullability) {
        self.apply_type_path_at(path, 0, nullability);
    }

    fn apply_type_path_at(&mut self, path: &[TargetPath], seg_idx: usize, nullability: Nullability) {
        let Some(step) = path.first() else {
            match self {
                Self::TypeVar {
                    nullability: slot, ..
                }
                | Self::Array {
                    nullability: slot, ..
                } => {
                    *slot = Some(nullability);
                }
                Self::Class { segments } => {
                    let target_idx = seg_idx.min(segments.len().saturating_sub(1));
                    if let Some(seg) = segments.get_mut(target_idx) {
                        seg.nullability = Some(nullability);
                    }
                }
                Self::Primitive(_) | Self::Wildcard(_) => {}
            }
            return;
        };

        let rest = &path[1..];
        match step.type_path_kind {
            // 0 = deeper in array type
            0 => {
                if let Self::Array { elem, .. } = self {
                    elem.apply_type_path_at(rest, 0, nullability);
                }
            }
            // 1 = deeper in nested type
            1 => {
                if let Self::Class { segments } = self {
                    let next_seg = if seg_idx + 1 < segments.len() {
                        seg_idx + 1
                    } else {
                        seg_idx
                    };
                    self.apply_type_path_at(rest, next_seg, nullability);
                }
            }
            // 2 = on the bound of a wildcard type argument
            2 => {
                if let Self::Wildcard(DecodedWildcard::Extends(bound) | DecodedWildcard::Super(bound)) =
                    self
                {
                    bound.apply_type_path_at(rest, 0, nullability);
                }
            }
            // 3 = on a type argument of a parameterized type
            3 => {
                if let Self::Class { segments } = self {
                    let arg_seg = if seg_idx < segments.len() && !segments[seg_idx].args.is_empty()
                    {
                        seg_idx
                    } else if let Some(last) = segments.iter().rposition(|s| !s.args.is_empty()) {
                        last
                    } else {
                        return;
                    };
                    if let Some(arg) = segments
                        .get_mut(arg_seg)
                        .and_then(|seg| seg.args.get_mut(step.type_argument_index as usize))
                    {
                        arg.apply_type_path_at(rest, 0, nullability);
                    }
                }
            }
            _ => {}
        }
    }

    fn apply_decl_fallback(&mut self, nullability: Nullability, is_type_use: bool) {
        match self {
            Self::Array {
                elem,
                nullability: outer,
            } => {
                if is_type_use {
                    elem.apply_decl_fallback(nullability, true);
                } else if outer.is_none() {
                    *outer = Some(nullability);
                }
            }
            Self::TypeVar {
                nullability: slot, ..
            } => {
                if slot.is_none() {
                    *slot = Some(nullability);
                }
            }
            Self::Class { segments } => {
                if let Some(last) = segments.last_mut()
                    && last.nullability.is_none()
                {
                    last.nullability = Some(nullability);
                }
            }
            Self::Primitive(_) | Self::Wildcard(_) => {}
        }
    }
}
