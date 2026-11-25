//! Pure, IO-free `.class` bytecode parsing via `ristretto_classfile`.
//!
//! All `ristretto_classfile` types remain strictly encapsulated inside this module.
//! See `docs/specs/0006-jdk-and-jar-classpath-indexing.md`.

mod annotations;
mod signature;
#[cfg(test)]
mod tests;

use std::{borrow::Cow, collections::BTreeMap, error::Error, fmt};

use ristretto_classfile::{
    ClassAccessFlags, ClassFile, ConstantPool, FieldAccessFlags, MethodAccessFlags, Version,
    attributes::{Attribute, MethodParameterAccessFlags, NestedClassAccessFlags, RequiresFlags},
};
use smol_str::SmolStr;

use crate::{
    classpath::{BinaryMemberSymbol, BinaryModuleInfo, BinaryOrigin, BinaryTypeSymbol},
    symbols::{MemberKind, NullnessScope, TypeIdentity, TypeKind},
};

use annotations::{
    MethodSigCtx, apply_class_type_annotations, extract_decl_nullability, extract_nullness_scope,
    extract_param_decl_nullabilities, finalize_field_type_and_nullability,
    finalize_method_signature,
};
use signature::{
    DecodedMethodSignature, decode_class_signature, decode_field_descriptor,
    decode_field_signature, decode_method_descriptor, decode_method_signature,
};

/// Result of parsing a single `.class` file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "Transient parse result matching Spec 0006 API (`Type(BinaryTypeSymbol)`); moved directly into ClasspathIndex."
)]
pub enum ParsedClassfile {
    Type(BinaryTypeSymbol),
    Module(BinaryModuleInfo),
    Package {
        package: String,
        nullness_scope: NullnessScope,
    },
}

/// Error returned when a `.class` byte slice is malformed or unreadable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassfileError {
    message: String,
}

impl ClassfileError {
    fn from_ristretto(err: ristretto_classfile::Error) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

impl fmt::Display for ClassfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ClassfileError {}

pub(super) type InnerClassMap = BTreeMap<String, (String, SmolStr)>;

pub(super) const MAX_SIGNATURE_DEPTH: usize = 64;

/// Parses a single `.class` file's raw bytes into a [`ParsedClassfile`].
///
/// Returns `Ok(None)` for entries that should be excluded from the external binary API index:
/// - `ACC_SYNTHETIC` classes and `package-info` markers;
/// - anonymous/local classes (`$1`, `$1Local`, or `InnerClasses` entries without an outer class);
/// - non-public / non-protected top-level or nested declarations;
/// - synthetic/bridge fields and methods, and `<clinit>` class initializers.
pub fn parse_class_bytes(
    bytes: &[u8],
    origin: BinaryOrigin,
) -> Result<Option<ParsedClassfile>, ClassfileError> {
    // `ristretto_classfile` 0.31 validates major version <= 69 (Java 25). Clamp future JDK
    // major versions (e.g. Java 26 = 70, Java 27 = 71) to 69 on a copy so API signatures parse
    // seamlessly without requiring a nightly rustc MSRV.
    let normalized: Cow<'_, [u8]> = if bytes.len() >= 8
        && bytes[..4] == [0xCA, 0xFE, 0xBA, 0xBE]
        && u16::from_be_bytes([bytes[6], bytes[7]]) > 69
    {
        let mut copy = bytes.to_vec();
        copy[6..8].copy_from_slice(&69u16.to_be_bytes());
        Cow::Owned(copy)
    } else {
        Cow::Borrowed(bytes)
    };
    let class_file =
        ClassFile::from_slice(&normalized).map_err(ClassfileError::from_ristretto)?;
    let cp = &class_file.constant_pool;

    if let Some(module_info) = parse_module_attribute(&class_file, cp)? {
        return Ok(Some(ParsedClassfile::Module(module_info)));
    }
    if class_file.access_flags.contains(ClassAccessFlags::MODULE) {
        return Ok(None);
    }

    let internal_name = class_file
        .class_name()
        .map_err(ClassfileError::from_ristretto)?
        .to_rust_string();

    // `javac` marks `package-info.class` with `ACC_SYNTHETIC`, so check it before filtering synthetic classes.
    if internal_name == "package-info" || internal_name.ends_with("/package-info") {
        let nullness_scope = extract_nullness_scope(&class_file.attributes, cp);
        if nullness_scope != NullnessScope::Inherited {
            let package = internal_name
                .rsplit_once('/')
                .map_or_else(String::new, |(pkg, _)| pkg.replace('/', "."));
            return Ok(Some(ParsedClassfile::Package {
                package,
                nullness_scope,
            }));
        }
        return Ok(None);
    }

    if class_file
        .access_flags
        .contains(ClassAccessFlags::SYNTHETIC)
    {
        return Ok(None);
    }

    let (inner_map, direct_nested, self_inner) =
        collect_inner_classes(&class_file, cp, &internal_name)?;

    let (simple_name, enclosing_type, is_public, is_static, is_final, is_abstract) =
        if let Some(inner) = self_inner {
            if inner.outer_internal.is_none() || inner.simple_name.is_none() {
                return Ok(None);
            }
            if inner.is_synthetic || (!inner.is_public && !inner.is_protected) {
                return Ok(None);
            }
            let Some(outer_internal) = inner.outer_internal else {
                return Ok(None);
            };
            let Some(simple_name) = inner.simple_name else {
                return Ok(None);
            };
            if is_anonymous_or_local_internal(&internal_name) {
                return Ok(None);
            }
            let enclosing_fqn = internal_to_fqn(&outer_internal, &inner_map);
            (
                simple_name,
                Some(enclosing_fqn),
                inner.is_public,
                inner.is_static,
                inner.is_final || class_file.access_flags.contains(ClassAccessFlags::FINAL),
                inner.is_abstract || class_file.access_flags.contains(ClassAccessFlags::ABSTRACT),
            )
        } else {
            if is_anonymous_or_local_internal(&internal_name) {
                return Ok(None);
            }
            if !class_file.access_flags.contains(ClassAccessFlags::PUBLIC) {
                return Ok(None);
            }
            let simple = internal_name
                .rsplit('/')
                .next()
                .unwrap_or(&internal_name);
            (
                SmolStr::new(simple),
                None,
                true,
                false,
                class_file.access_flags.contains(ClassAccessFlags::FINAL),
                class_file.access_flags.contains(ClassAccessFlags::ABSTRACT),
            )
        };

    let qualified_name = match &enclosing_type {
        Some(enclosing) => format!("{enclosing}.{simple_name}"),
        None => internal_name.replace('/', "."),
    };

    let package = match internal_name.rsplit_once('/') {
        Some((pkg, _)) => pkg.replace('/', "."),
        None => String::new(),
    };

    let record_attr = find_record_attribute(&class_file.attributes);
    let kind = determine_type_kind(class_file.access_flags, record_attr.is_some());
    let identity = determine_type_identity(class_file.version, class_file.access_flags, kind);
    let is_static = is_static
        || (enclosing_type.is_some()
            && matches!(
                kind,
                TypeKind::Interface | TypeKind::Enum | TypeKind::Record | TypeKind::Annotation
            ));

    let nullness_scope = extract_nullness_scope(&class_file.attributes, cp);

    let mut class_sig = find_signature(&class_file.attributes, cp)
        .and_then(|sig| decode_class_signature(&sig, &inner_map));
    if let Some(ref mut decoded) = class_sig {
        apply_class_type_annotations(&class_file.attributes, cp, decoded);
    }
    let (type_params, type_param_bounds, super_class_generic, interfaces_generic) = match class_sig
    {
        Some(decoded) => {
            let parts = decoded.into_parts();
            (
                parts.type_params,
                parts.type_param_bounds,
                parts.super_class_generic,
                parts.interfaces_generic,
            )
        }
        None => (Vec::new(), BTreeMap::new(), None, Vec::new()),
    };

    let super_class = if class_file.super_class == 0 {
        None
    } else {
        let super_internal = cp
            .try_get_class(class_file.super_class)
            .map_err(ClassfileError::from_ristretto)?
            .to_rust_string();
        Some(internal_to_fqn(&super_internal, &inner_map))
    };

    let mut interfaces = Vec::with_capacity(class_file.interfaces.len());
    for &iface_idx in &class_file.interfaces {
        let iface_internal = cp
            .try_get_class(iface_idx)
            .map_err(ClassfileError::from_ristretto)?
            .to_rust_string();
        interfaces.push(internal_to_fqn(&iface_internal, &inner_map));
    }

    let permitted_subclasses = collect_permitted_subclasses(&class_file.attributes, cp, &inner_map)?;

    let mut members = Vec::new();
    if let Some(records) = record_attr {
        for rec in records {
            let name = cp
                .try_get_utf8(rec.name_index)
                .map_err(ClassfileError::from_ristretto)?
                .to_rust_string();
            let desc = cp
                .try_get_utf8(rec.descriptor_index)
                .map_err(ClassfileError::from_ristretto)?
                .to_rust_string();
            let decoded_ty = find_signature(&rec.attributes, cp)
                .and_then(|sig| decode_field_signature(&sig, &inner_map))
                .or_else(|| decode_field_descriptor(&desc, &inner_map));
            let rec_scope = extract_nullness_scope(&rec.attributes, cp);
            let rec_decl = extract_decl_nullability(&rec.attributes, cp);
            let (type_ref, decl_nullability) =
                finalize_field_type_and_nullability(decoded_ty, &rec.attributes, cp, rec_decl);
            members.push(BinaryMemberSymbol {
                owner_type: qualified_name.clone(),
                name: SmolStr::new(name),
                kind: MemberKind::RecordComponent,
                is_public: true,
                is_static: false,
                is_abstract: false,
                nullness_scope: rec_scope,
                decl_nullability,
                type_params: Vec::new(),
                type_param_bounds: BTreeMap::new(),
                type_ref,
                params: Vec::new(),
                is_varargs: false,
            });
        }
    }

    for field in &class_file.fields {
        if field.access_flags.contains(FieldAccessFlags::SYNTHETIC)
            || has_synthetic_attribute(&field.attributes)
        {
            continue;
        }
        let is_field_public = field.access_flags.contains(FieldAccessFlags::PUBLIC);
        let is_field_protected = field.access_flags.contains(FieldAccessFlags::PROTECTED);
        if !is_field_public && !is_field_protected {
            continue;
        }

        let name = cp
            .try_get_utf8(field.name_index)
            .map_err(ClassfileError::from_ristretto)?
            .to_rust_string();
        let is_enum_const =
            kind == TypeKind::Enum && field.access_flags.contains(FieldAccessFlags::ENUM);

        if is_enum_const {
            members.push(BinaryMemberSymbol {
                owner_type: qualified_name.clone(),
                name: SmolStr::new(name),
                kind: MemberKind::EnumConstant,
                is_public: true,
                is_static: true,
                is_abstract: false,
                nullness_scope: NullnessScope::Inherited,
                decl_nullability: None,
                type_params: Vec::new(),
                type_param_bounds: BTreeMap::new(),
                type_ref: None,
                params: Vec::new(),
                is_varargs: false,
            });
            continue;
        }

        let desc = cp
            .try_get_utf8(field.descriptor_index)
            .map_err(ClassfileError::from_ristretto)?
            .to_rust_string();
        let decoded_ty = find_signature(&field.attributes, cp)
            .and_then(|sig| decode_field_signature(&sig, &inner_map))
            .or_else(|| decode_field_descriptor(&desc, &inner_map));
        let field_scope = extract_nullness_scope(&field.attributes, cp);
        let field_decl = extract_decl_nullability(&field.attributes, cp);
        let (type_ref, decl_nullability) =
            finalize_field_type_and_nullability(decoded_ty, &field.attributes, cp, field_decl);

        members.push(BinaryMemberSymbol {
            owner_type: qualified_name.clone(),
            name: SmolStr::new(name),
            kind: MemberKind::Field,
            is_public: is_field_public,
            is_static: field.access_flags.contains(FieldAccessFlags::STATIC),
            is_abstract: false,
            nullness_scope: field_scope,
            decl_nullability,
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
            type_ref,
            params: Vec::new(),
            is_varargs: false,
        });
    }

    for method in &class_file.methods {
        if method.access_flags.contains(MethodAccessFlags::SYNTHETIC)
            || method.access_flags.contains(MethodAccessFlags::BRIDGE)
            || has_synthetic_attribute(&method.attributes)
        {
            continue;
        }
        let is_method_public = method.access_flags.contains(MethodAccessFlags::PUBLIC);
        let is_method_protected = method.access_flags.contains(MethodAccessFlags::PROTECTED);
        if !is_method_public && !is_method_protected {
            continue;
        }

        let raw_name = cp
            .try_get_utf8(method.name_index)
            .map_err(ClassfileError::from_ristretto)?
            .to_rust_string();
        if raw_name == "<clinit>" {
            continue;
        }

        let is_ctor = raw_name == "<init>";
        let member_kind = if is_ctor {
            MemberKind::Constructor
        } else {
            MemberKind::Method
        };
        let member_name = if is_ctor {
            simple_name.clone()
        } else {
            SmolStr::new(raw_name)
        };

        let desc = cp
            .try_get_utf8(method.descriptor_index)
            .map_err(ClassfileError::from_ristretto)?
            .to_rust_string();
        let mut decoded_method = find_signature(&method.attributes, cp)
            .and_then(|sig| decode_method_signature(&sig, &inner_map))
            .unwrap_or_else(|| {
                let (params, return_type) =
                    decode_method_descriptor(&desc, &inner_map).unwrap_or_else(|| (Vec::new(), None));
                DecodedMethodSignature {
                    type_params: Vec::new(),
                    type_param_bounds: Vec::new(),
                    params,
                    return_type,
                }
            });

        let method_scope = extract_nullness_scope(&method.attributes, cp);
        let method_decl = if is_ctor {
            None
        } else {
            extract_decl_nullability(&method.attributes, cp)
        };
        let param_decls =
            extract_param_decl_nullabilities(&method.attributes, cp, decoded_method.params.len());
        let param_names =
            extract_method_param_names(&method.attributes, cp, decoded_method.params.len())?;
        let finalized = finalize_method_signature(
            &mut decoded_method,
            &MethodSigCtx {
                attributes: &method.attributes,
                cp,
                is_ctor,
                has_varargs_flag: method.access_flags.contains(MethodAccessFlags::VARARGS),
                method_decl,
                param_decls: &param_decls,
                param_names: &param_names,
            },
        );

        members.push(BinaryMemberSymbol {
            owner_type: qualified_name.clone(),
            name: member_name,
            kind: member_kind,
            is_public: is_method_public,
            is_static: !is_ctor && method.access_flags.contains(MethodAccessFlags::STATIC),
            is_abstract: !is_ctor && method.access_flags.contains(MethodAccessFlags::ABSTRACT),
            nullness_scope: method_scope,
            decl_nullability: finalized.decl_nullability,
            type_params: finalized.type_params,
            type_param_bounds: finalized.type_param_bounds,
            type_ref: if is_ctor { None } else { finalized.return_type },
            params: finalized.params,
            is_varargs: finalized.is_varargs,
        });
    }

    Ok(Some(ParsedClassfile::Type(BinaryTypeSymbol {
        name: simple_name,
        qualified_name,
        internal_name,
        package,
        enclosing_type,
        kind,
        identity,
        is_public,
        is_static,
        is_final,
        is_abstract,
        nullness_scope,
        type_params,
        type_param_bounds,
        super_class,
        super_class_generic,
        interfaces,
        interfaces_generic,
        permitted_subclasses,
        members,
        nested_types: direct_nested,
        origin,
    })))
}

fn parse_module_attribute(
    class_file: &ClassFile<'_>,
    cp: &ConstantPool<'_>,
) -> Result<Option<BinaryModuleInfo>, ClassfileError> {
    for attr in &class_file.attributes {
        if let Attribute::Module { module, .. } = attr {
            let name = cp
                .try_get_module(module.module_name_index)
                .map_err(ClassfileError::from_ristretto)?
                .to_rust_string();

            let mut exports = Vec::new();
            for exp in &module.exports {
                // Only unqualified exports are visible to `import module M;`.
                if exp.to_index.is_empty() {
                    let pkg_internal = cp
                        .try_get_package(exp.index)
                        .map_err(ClassfileError::from_ristretto)?
                        .to_rust_string();
                    exports.push(pkg_internal.replace('/', "."));
                }
            }

            let mut requires_transitive = Vec::new();
            for req in &module.requires {
                if req.flags.contains(RequiresFlags::TRANSITIVE) {
                    let dep_name = cp
                        .try_get_module(req.index)
                        .map_err(ClassfileError::from_ristretto)?
                        .to_rust_string();
                    requires_transitive.push(SmolStr::new(dep_name));
                }
            }

            let nullness_scope = extract_nullness_scope(&class_file.attributes, cp);
            return Ok(Some(BinaryModuleInfo {
                name: SmolStr::new(name),
                nullness_scope,
                exports,
                requires_transitive,
            }));
        }
    }
    Ok(None)
}

struct SelfInnerInfo {
    outer_internal: Option<String>,
    simple_name: Option<SmolStr>,
    is_public: bool,
    is_protected: bool,
    is_static: bool,
    is_final: bool,
    is_abstract: bool,
    is_synthetic: bool,
}

fn collect_inner_classes(
    class_file: &ClassFile<'_>,
    cp: &ConstantPool<'_>,
    this_internal: &str,
) -> Result<(InnerClassMap, Vec<SmolStr>, Option<SelfInnerInfo>), ClassfileError> {
    let mut inner_map = InnerClassMap::new();
    let mut direct_nested = Vec::new();
    let mut self_info = None;

    for attr in &class_file.attributes {
        let Attribute::InnerClasses { classes, .. } = attr else {
            continue;
        };
        for ic in classes {
            let inner_internal = cp
                .try_get_class(ic.class_info_index)
                .map_err(ClassfileError::from_ristretto)?
                .to_rust_string();
            let outer_internal = if ic.outer_class_info_index == 0 {
                None
            } else {
                Some(
                    cp.try_get_class(ic.outer_class_info_index)
                        .map_err(ClassfileError::from_ristretto)?
                        .to_rust_string(),
                )
            };
            let simple_name = if ic.name_index == 0 {
                None
            } else {
                Some(SmolStr::new(
                    cp.try_get_utf8(ic.name_index)
                        .map_err(ClassfileError::from_ristretto)?
                        .to_rust_string(),
                ))
            };

            if let (Some(outer), Some(simple)) = (&outer_internal, &simple_name) {
                inner_map.insert(inner_internal.clone(), (outer.clone(), simple.clone()));

                if outer == this_internal
                    && !ic.access_flags.contains(NestedClassAccessFlags::SYNTHETIC)
                    && (ic.access_flags.contains(NestedClassAccessFlags::PUBLIC)
                        || ic.access_flags.contains(NestedClassAccessFlags::PROTECTED))
                    && !is_anonymous_or_local_internal(&inner_internal)
                    && !direct_nested.contains(simple)
                {
                    direct_nested.push(simple.clone());
                }
            }

            if inner_internal == this_internal {
                self_info = Some(SelfInnerInfo {
                    outer_internal,
                    simple_name,
                    is_public: ic.access_flags.contains(NestedClassAccessFlags::PUBLIC),
                    is_protected: ic.access_flags.contains(NestedClassAccessFlags::PROTECTED),
                    is_static: ic.access_flags.contains(NestedClassAccessFlags::STATIC),
                    is_final: ic.access_flags.contains(NestedClassAccessFlags::FINAL),
                    is_abstract: ic.access_flags.contains(NestedClassAccessFlags::ABSTRACT),
                    is_synthetic: ic.access_flags.contains(NestedClassAccessFlags::SYNTHETIC),
                });
            }
        }
    }

    Ok((inner_map, direct_nested, self_info))
}

fn is_anonymous_or_local_internal(internal_name: &str) -> bool {
    let class_part = internal_name.rsplit('/').next().unwrap_or(internal_name);
    let mut parts = class_part.split('$');
    let _outer = parts.next();
    parts.any(|seg| seg.is_empty() || seg.as_bytes().first().is_some_and(u8::is_ascii_digit))
}

fn determine_type_kind(flags: ClassAccessFlags, has_record_attr: bool) -> TypeKind {
    if flags.contains(ClassAccessFlags::ANNOTATION) {
        TypeKind::Annotation
    } else if flags.contains(ClassAccessFlags::INTERFACE) {
        TypeKind::Interface
    } else if flags.contains(ClassAccessFlags::ENUM) {
        TypeKind::Enum
    } else if has_record_attr {
        TypeKind::Record
    } else {
        TypeKind::Class
    }
}

/// Determines the JEP 401 object-identity semantics of a compiled `.class` file.
///
/// Hook for future Java 28+ Valhalla preview classfiles: once preview classfile support
/// (major version >= 69 with preview minor `65535`) is enabled, concrete non-abstract classes
/// lacking `ACC_IDENTITY` (`0x0020`, formerly `ACC_SUPER`) will map to [`TypeIdentity::Value`].
fn determine_type_identity(
    _version: Version,
    access_flags: ClassAccessFlags,
    kind: TypeKind,
) -> TypeIdentity {
    if matches!(kind, TypeKind::Interface | TypeKind::Annotation)
        || access_flags.contains(ClassAccessFlags::ABSTRACT)
    {
        TypeIdentity::Unspecified
    } else {
        TypeIdentity::Identity
    }
}

fn find_record_attribute(
    attributes: &[Attribute],
) -> Option<&[ristretto_classfile::attributes::Record]> {
    attributes.iter().find_map(|attr| match attr {
        Attribute::Record { records, .. } => Some(records.as_slice()),
        _ => None,
    })
}

fn has_synthetic_attribute(attributes: &[Attribute]) -> bool {
    attributes
        .iter()
        .any(|attr| matches!(attr, Attribute::Synthetic { .. }))
}

fn find_signature(attributes: &[Attribute], cp: &ConstantPool<'_>) -> Option<String> {
    let sig_idx = attributes.iter().find_map(|attr| match attr {
        Attribute::Signature {
            signature_index, ..
        } => Some(*signature_index),
        _ => None,
    })?;
    cp.try_get_utf8(sig_idx)
        .ok()
        .map(|s| s.to_rust_string())
}

fn collect_permitted_subclasses(
    attributes: &[Attribute],
    cp: &ConstantPool<'_>,
    inner_map: &InnerClassMap,
) -> Result<Vec<String>, ClassfileError> {
    let mut out = Vec::new();
    for attr in attributes {
        if let Attribute::PermittedSubclasses { class_indexes, .. } = attr {
            out.reserve(class_indexes.len());
            for &idx in class_indexes {
                let internal = cp
                    .try_get_class(idx)
                    .map_err(ClassfileError::from_ristretto)?
                    .to_rust_string();
                out.push(internal_to_fqn(&internal, inner_map));
            }
        }
    }
    Ok(out)
}

fn extract_method_param_names(
    attributes: &[Attribute],
    cp: &ConstantPool<'_>,
    param_count: usize,
) -> Result<Vec<Option<SmolStr>>, ClassfileError> {
    for attr in attributes {
        let Attribute::MethodParameters { parameters, .. } = attr else {
            continue;
        };
        let filtered: Vec<_> = parameters
            .iter()
            .filter(|p| !p.access_flags.contains(MethodParameterAccessFlags::SYNTHETIC))
            .collect();
        let source = if filtered.len() == param_count {
            filtered
        } else {
            parameters.iter().collect()
        };
        let mut names = Vec::with_capacity(param_count);
        let offset = source.len().saturating_sub(param_count);
        for i in 0..param_count {
            let name = match source.get(offset + i) {
                Some(p) if p.name_index != 0 => {
                    let s = cp
                        .try_get_utf8(p.name_index)
                        .map_err(ClassfileError::from_ristretto)?
                        .to_rust_string();
                    if s.is_empty() {
                        None
                    } else {
                        Some(SmolStr::new(s))
                    }
                }
                _ => None,
            };
            names.push(name);
        }
        return Ok(names);
    }
    Ok(Vec::new())
}

/// Converts a JVM internal binary name (e.g. `"java/util/Map$Entry"`) to a canonical Java FQN
/// (`"java.util.Map.Entry"`), consulting `InnerClasses` metadata first.
pub(super) fn internal_to_fqn(internal: &str, inner_map: &InnerClassMap) -> String {
    internal_to_fqn_depth(internal, inner_map, 0)
}

fn internal_to_fqn_depth(internal: &str, inner_map: &InnerClassMap, depth: usize) -> String {
    if depth < MAX_SIGNATURE_DEPTH
        && let Some((outer, simple)) = inner_map.get(internal)
    {
        let outer_fqn = internal_to_fqn_depth(outer, inner_map, depth + 1);
        return format!("{outer_fqn}.{simple}");
    }
    internal.replace('/', ".")
}

/// Formats an internal class name as a Java type string for member signatures/descriptors.
///
/// Top-level `java.lang.*` classes shorten to their simple name (e.g. `"java/lang/String"` ->
/// `"String"`), while types in other packages keep their package FQN (`"java.io.PrintStream"`).
pub(super) fn internal_to_java_type(internal: &str, inner_map: &InnerClassMap) -> String {
    if let Some(rest) = internal.strip_prefix("java/lang/")
        && !rest.contains('/')
        && !rest.contains('$')
    {
        return rest.to_string();
    }
    if let Some((outer, simple)) = inner_map.get(internal) {
        let outer_ty = internal_to_java_type(outer, inner_map);
        return format!("{outer_ty}.{simple}");
    }
    internal.replace('/', ".")
}
