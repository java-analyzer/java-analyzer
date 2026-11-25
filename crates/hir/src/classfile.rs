//! Pure, IO-free `.class` bytecode parsing via `ristretto_classfile`.
//!
//! All `ristretto_classfile` types remain strictly encapsulated inside this module.
//! See `docs/specs/0006-jdk-and-jar-classpath-indexing.md`.

use std::{borrow::Cow, collections::BTreeMap, error::Error, fmt};

use ristretto_classfile::{
    ClassAccessFlags, ClassFile, ConstantPool, FieldAccessFlags, MethodAccessFlags, Version,
    attributes::{
        Annotation, Attribute, MethodParameterAccessFlags, NestedClassAccessFlags, RequiresFlags,
        TargetPath, TargetType, TypeAnnotation,
    },
};
use smol_str::{SmolStr, format_smolstr};

use crate::{
    classpath::{BinaryMemberSymbol, BinaryModuleInfo, BinaryOrigin, BinaryTypeSymbol},
    symbols::{
        MemberKind, NullabilitySource, NullnessScope, ParamSummary, TypeIdentity, TypeKind,
        classify_nullability_annotation, classify_nullness_scope_annotation,
        is_type_use_nullability_annotation,
    },
    ty::Nullability,
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

type InnerClassMap = BTreeMap<String, (String, SmolStr)>;
type RawTypeParamsWithBounds = (Vec<SmolStr>, Vec<(SmolStr, bool, Vec<DecodedType>)>);

const MAX_SIGNATURE_DEPTH: usize = 64;

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
        Some(decoded) => decoded.into_parts(),
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
        let (method_type_params, method_type_param_bounds, params, return_type, is_varargs, decl_nullability) =
            finalize_method_signature(
                &mut decoded_method,
                &method.attributes,
                cp,
                is_ctor,
                method.access_flags.contains(MethodAccessFlags::VARARGS),
                method_decl,
                &param_decls,
                param_names,
            );

        members.push(BinaryMemberSymbol {
            owner_type: qualified_name.clone(),
            name: member_name,
            kind: member_kind,
            is_public: is_method_public,
            is_static: !is_ctor && method.access_flags.contains(MethodAccessFlags::STATIC),
            is_abstract: !is_ctor && method.access_flags.contains(MethodAccessFlags::ABSTRACT),
            nullness_scope: method_scope,
            decl_nullability,
            type_params: method_type_params,
            type_param_bounds: method_type_param_bounds,
            type_ref: if is_ctor { None } else { return_type },
            params,
            is_varargs,
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
fn internal_to_fqn(internal: &str, inner_map: &InnerClassMap) -> String {
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
fn internal_to_java_type(internal: &str, inner_map: &InnerClassMap) -> String {
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

// ---- Annotation & Type-Annotation Decoding (JVMS §4.7.16 - §4.7.20) ----

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

fn extract_nullness_scope(attributes: &[Attribute], cp: &ConstantPool<'_>) -> NullnessScope {
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

fn extract_decl_nullability(
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

fn extract_param_decl_nullabilities(
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

fn apply_class_type_annotations(
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

fn finalize_field_type_and_nullability(
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

#[expect(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "deferred to Commit 2 classfile split"
)]
fn finalize_method_signature(
    decoded: &mut DecodedMethodSignature,
    attributes: &[Attribute],
    cp: &ConstantPool<'_>,
    is_ctor: bool,
    has_varargs_flag: bool,
    method_decl: Option<(Nullability, NullabilitySource, bool)>,
    param_decls: &[Option<(Nullability, NullabilitySource, bool)>],
    param_names: Vec<Option<SmolStr>>,
) -> (
    Vec<SmolStr>,
    BTreeMap<SmolStr, Vec<String>>,
    Vec<ParamSummary>,
    Option<String>,
    bool,
    Option<(Nullability, NullabilitySource)>,
) {
    let mut return_has_type_ann = false;
    let mut param_has_type_ann = vec![false; decoded.params.len()];

    for (ta, nullability, _) in sorted_type_annotations(attributes, cp) {
        match ta.target_type {
            TargetType::Empty { target_type: 0x14 } if !is_ctor => {
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

    let decl_nullability = if is_ctor {
        None
    } else if let Some(ret) = decoded.return_type.as_mut() {
        apply_decl_fallback_to_decoded(ret, method_decl, return_has_type_ann)
    } else {
        None
    };

    let is_varargs =
        has_varargs_flag && matches!(decoded.params.last(), Some(DecodedType::Array { .. }));
    let last_idx = decoded.params.len().saturating_sub(1);
    let params = std::mem::take(&mut decoded.params)
        .into_iter()
        .enumerate()
        .map(|(i, mut ty)| {
            let param_varargs = is_varargs && i == last_idx;
            let p_decl_raw = param_decls.get(i).copied().flatten();
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

            let pname = param_names
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
    (
        std::mem::take(&mut decoded.type_params),
        type_param_bounds,
        params,
        return_type,
        is_varargs,
        decl_nullability,
    )
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

fn render_type_param_bounds(
    entries: &[(SmolStr, bool, Vec<DecodedType>)],
) -> BTreeMap<SmolStr, Vec<String>> {
    let mut map = BTreeMap::new();
    for (name, _has_explicit, bounds) in entries {
        let rendered: Vec<String> = bounds.iter().map(DecodedType::render).collect();
        let is_implicit_object_only =
            rendered.len() == 1 && rendered[0] == "java.lang.Object";
        if !rendered.is_empty() && !is_implicit_object_only {
            map.insert(name.clone(), rendered);
        }
    }
    map
}

// ---- Structured Type Tree & TypePath Application ----

#[derive(Debug, Clone)]
enum DecodedType {
    Primitive(&'static str),
    TypeVar {
        name: String,
        nullability: Option<Nullability>,
    },
    Class {
        segments: Vec<ClassSegment>,
    },
    Array {
        elem: Box<DecodedType>,
        nullability: Option<Nullability>,
    },
    Wildcard(DecodedWildcard),
}

#[derive(Debug, Clone)]
struct ClassSegment {
    name: String,
    args: Vec<DecodedType>,
    nullability: Option<Nullability>,
}

#[derive(Debug, Clone)]
enum DecodedWildcard {
    Unbounded,
    Extends(Box<DecodedType>),
    Super(Box<DecodedType>),
}

const fn nullability_suffix_opt(n: Option<Nullability>) -> &'static str {
    match n {
        Some(Nullability::NonNull) => "!",
        Some(Nullability::Nullable) => "?",
        Some(Nullability::Unspecified) | None => "",
    }
}

impl DecodedType {
    fn is_array(&self) -> bool {
        matches!(self, Self::Array { .. })
    }

    fn is_void(&self) -> bool {
        matches!(self, Self::Primitive("void"))
    }

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

    fn render(&self) -> String {
        match self {
            Self::Primitive(kw) => (*kw).to_string(),
            Self::TypeVar { name, nullability } => {
                format!("{name}{}", nullability_suffix_opt(*nullability))
            }
            Self::Class { segments } => {
                let last_args_idx = segments.iter().rposition(|seg| !seg.args.is_empty());
                let mut out = String::new();
                for (i, seg) in segments.iter().enumerate() {
                    if i > 0 {
                        out.push('.');
                    }
                    out.push_str(&seg.name);
                    if Some(i) == last_args_idx && !seg.args.is_empty() {
                        let args: Vec<String> = seg.args.iter().map(Self::render).collect();
                        out.push('<');
                        out.push_str(&args.join(", "));
                        out.push('>');
                    }
                    out.push_str(nullability_suffix_opt(seg.nullability));
                }
                out
            }
            Self::Array { elem, nullability } => {
                format!(
                    "{}[]{}",
                    elem.render(),
                    nullability_suffix_opt(*nullability)
                )
            }
            Self::Wildcard(wc) => match wc {
                DecodedWildcard::Unbounded => "?".to_string(),
                DecodedWildcard::Extends(bound) => format!("? extends {}", bound.render()),
                DecodedWildcard::Super(bound) => format!("? super {}", bound.render()),
            },
        }
    }
}

// ---- JVM Descriptor Decoding ----

fn decode_field_descriptor(desc: &str, inner_map: &InnerClassMap) -> Option<DecodedType> {
    let mut parser = DescriptorParser::new(desc, inner_map);
    let ty = parser.parse_field_type(0)?;
    parser.is_eof().then_some(ty)
}

fn decode_method_descriptor(
    desc: &str,
    inner_map: &InnerClassMap,
) -> Option<(Vec<DecodedType>, Option<DecodedType>)> {
    let mut parser = DescriptorParser::new(desc, inner_map);
    parser.expect_byte(b'(')?;
    let mut params = Vec::new();
    while parser.peek_byte() != Some(b')') {
        params.push(parser.parse_field_type(0)?);
    }
    parser.expect_byte(b')')?;
    let ret = if parser.peek_byte() == Some(b'V') {
        parser.advance(1);
        Some(DecodedType::Primitive("void"))
    } else {
        Some(parser.parse_field_type(0)?)
    };
    parser.is_eof().then_some((params, ret))
}

struct DescriptorParser<'a> {
    bytes: &'a [u8],
    pos: usize,
    inner_map: &'a InnerClassMap,
}

impl<'a> DescriptorParser<'a> {
    fn new(input: &'a str, inner_map: &'a InnerClassMap) -> Self {
        Self {
            bytes: input.as_bytes(),
            pos: 0,
            inner_map,
        }
    }

    fn is_eof(&self) -> bool {
        self.pos == self.bytes.len()
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.bytes.len());
    }

    fn expect_byte(&mut self, expected: u8) -> Option<()> {
        if self.peek_byte() == Some(expected) {
            self.pos += 1;
            Some(())
        } else {
            None
        }
    }

    fn parse_field_type(&mut self, depth: usize) -> Option<DecodedType> {
        if depth >= MAX_SIGNATURE_DEPTH {
            return None;
        }
        let mut dims = 0usize;
        while self.peek_byte() == Some(b'[') {
            dims += 1;
            self.pos += 1;
            if dims > MAX_SIGNATURE_DEPTH {
                return None;
            }
        }
        let mut ty = match self.peek_byte()? {
            b'B' => {
                self.pos += 1;
                DecodedType::Primitive("byte")
            }
            b'C' => {
                self.pos += 1;
                DecodedType::Primitive("char")
            }
            b'D' => {
                self.pos += 1;
                DecodedType::Primitive("double")
            }
            b'F' => {
                self.pos += 1;
                DecodedType::Primitive("float")
            }
            b'I' => {
                self.pos += 1;
                DecodedType::Primitive("int")
            }
            b'J' => {
                self.pos += 1;
                DecodedType::Primitive("long")
            }
            b'S' => {
                self.pos += 1;
                DecodedType::Primitive("short")
            }
            b'Z' => {
                self.pos += 1;
                DecodedType::Primitive("boolean")
            }
            b'L' => {
                self.pos += 1;
                let rest = std::str::from_utf8(self.bytes.get(self.pos..)?).ok()?;
                let semi_rel = rest.find(';')?;
                let internal = &rest[..semi_rel];
                if internal.is_empty() {
                    return None;
                }
                self.pos += semi_rel + 1;
                DecodedType::Class {
                    segments: vec![ClassSegment {
                        name: internal_to_java_type(internal, self.inner_map),
                        args: Vec::new(),
                        nullability: None,
                    }],
                }
            }
            _ => return None,
        };

        for _ in 0..dims {
            ty = DecodedType::Array {
                elem: Box::new(ty),
                nullability: None,
            };
        }
        Some(ty)
    }
}

// ---- JVM Generic Signature Decoding (JVMS §4.7.9.1) ----

struct DecodedClassSignature {
    type_params: Vec<SmolStr>,
    type_param_bounds: Vec<(SmolStr, bool, Vec<DecodedType>)>,
    super_class_generic: Option<DecodedType>,
    interfaces_generic: Vec<DecodedType>,
}

impl DecodedClassSignature {
    #[expect(
        clippy::type_complexity,
        reason = "deferred to Commit 2 classfile split"
    )]
    fn into_parts(
        self,
    ) -> (
        Vec<SmolStr>,
        BTreeMap<SmolStr, Vec<String>>,
        Option<String>,
        Vec<String>,
    ) {
        let bounds = render_type_param_bounds(&self.type_param_bounds);
        let sup = self.super_class_generic.as_ref().map(DecodedType::render);
        let ifaces = self
            .interfaces_generic
            .iter()
            .map(DecodedType::render)
            .collect();
        (self.type_params, bounds, sup, ifaces)
    }
}

struct DecodedMethodSignature {
    type_params: Vec<SmolStr>,
    type_param_bounds: Vec<(SmolStr, bool, Vec<DecodedType>)>,
    params: Vec<DecodedType>,
    return_type: Option<DecodedType>,
}

fn decode_class_signature(
    sig: &str,
    inner_map: &InnerClassMap,
) -> Option<DecodedClassSignature> {
    let mut parser = SignatureParser::new(sig, inner_map, false);
    let (type_params, type_param_bounds) = if parser.peek_byte() == Some(b'<') {
        parser.parse_type_parameters(0)?
    } else {
        (Vec::new(), Vec::new())
    };
    let super_class_generic = Some(parser.parse_class_type_sig(0)?);
    let mut interfaces_generic = Vec::new();
    while !parser.is_eof() {
        interfaces_generic.push(parser.parse_class_type_sig(0)?);
    }
    parser.is_eof().then_some(DecodedClassSignature {
        type_params,
        type_param_bounds,
        super_class_generic,
        interfaces_generic,
    })
}

fn decode_field_signature(sig: &str, inner_map: &InnerClassMap) -> Option<DecodedType> {
    let mut parser = SignatureParser::new(sig, inner_map, true);
    let ty = parser.parse_ref_type_sig(0)?;
    parser.is_eof().then_some(ty)
}

fn decode_method_signature(
    sig: &str,
    inner_map: &InnerClassMap,
) -> Option<DecodedMethodSignature> {
    let mut parser = SignatureParser::new(sig, inner_map, false);
    let (type_params, type_param_bounds) = if parser.peek_byte() == Some(b'<') {
        parser.parse_type_parameters(0)?
    } else {
        (Vec::new(), Vec::new())
    };
    parser.shorten_java_lang = true;
    parser.expect_byte(b'(')?;
    let mut params = Vec::new();
    while parser.peek_byte() != Some(b')') {
        params.push(parser.parse_java_type_sig(0)?);
    }
    parser.expect_byte(b')')?;
    let return_type = if parser.peek_byte() == Some(b'V') {
        parser.advance(1);
        Some(DecodedType::Primitive("void"))
    } else {
        Some(parser.parse_java_type_sig(0)?)
    };
    // Optional trailing ThrowsSignature (`^...`).
    while parser.peek_byte() == Some(b'^') {
        parser.advance(1);
        match parser.peek_byte() {
            Some(b'L') => {
                let _ = parser.parse_class_type_sig(0)?;
            }
            Some(b'T') => {
                let _ = parser.parse_type_var_sig()?;
            }
            _ => return None,
        }
    }
    parser.is_eof().then_some(DecodedMethodSignature {
        type_params,
        type_param_bounds,
        params,
        return_type,
    })
}

struct SignatureParser<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    inner_map: &'a InnerClassMap,
    shorten_java_lang: bool,
}

impl<'a> SignatureParser<'a> {
    fn new(src: &'a str, inner_map: &'a InnerClassMap, shorten_java_lang: bool) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            inner_map,
            shorten_java_lang,
        }
    }

    fn format_internal_name(&self, internal: &str) -> String {
        if self.shorten_java_lang {
            internal_to_java_type(internal, self.inner_map)
        } else {
            internal_to_fqn(internal, self.inner_map)
        }
    }

    fn is_eof(&self) -> bool {
        self.pos == self.bytes.len()
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.bytes.len());
    }

    fn expect_byte(&mut self, expected: u8) -> Option<()> {
        if self.peek_byte() == Some(expected) {
            self.pos += 1;
            Some(())
        } else {
            None
        }
    }

    fn read_identifier_until(&mut self, stops: &[u8]) -> Option<&'a str> {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if stops.contains(&b) {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        self.src.get(start..self.pos)
    }

    fn parse_type_parameters(&mut self, depth: usize) -> Option<RawTypeParamsWithBounds> {
        if depth >= MAX_SIGNATURE_DEPTH {
            return None;
        }
        self.expect_byte(b'<')?;
        let mut names = Vec::new();
        let mut bounds_entries = Vec::new();
        while self.peek_byte() != Some(b'>') {
            let name = SmolStr::new(self.read_identifier_until(b":")?);
            self.expect_byte(b':')?;
            let mut param_bounds = Vec::new();
            let has_explicit_class_bound = matches!(self.peek_byte(), Some(b'L' | b'T' | b'['));
            if has_explicit_class_bound {
                param_bounds.push(self.parse_ref_type_sig(depth + 1)?);
            }
            while self.peek_byte() == Some(b':') {
                self.pos += 1;
                param_bounds.push(self.parse_ref_type_sig(depth + 1)?);
            }
            names.push(name.clone());
            bounds_entries.push((name, has_explicit_class_bound, param_bounds));
        }
        self.expect_byte(b'>')?;
        (!names.is_empty()).then_some((names, bounds_entries))
    }

    fn parse_java_type_sig(&mut self, depth: usize) -> Option<DecodedType> {
        if depth >= MAX_SIGNATURE_DEPTH {
            return None;
        }
        match self.peek_byte()? {
            b'B' => {
                self.pos += 1;
                Some(DecodedType::Primitive("byte"))
            }
            b'C' => {
                self.pos += 1;
                Some(DecodedType::Primitive("char"))
            }
            b'D' => {
                self.pos += 1;
                Some(DecodedType::Primitive("double"))
            }
            b'F' => {
                self.pos += 1;
                Some(DecodedType::Primitive("float"))
            }
            b'I' => {
                self.pos += 1;
                Some(DecodedType::Primitive("int"))
            }
            b'J' => {
                self.pos += 1;
                Some(DecodedType::Primitive("long"))
            }
            b'S' => {
                self.pos += 1;
                Some(DecodedType::Primitive("short"))
            }
            b'Z' => {
                self.pos += 1;
                Some(DecodedType::Primitive("boolean"))
            }
            _ => self.parse_ref_type_sig(depth + 1),
        }
    }

    fn parse_ref_type_sig(&mut self, depth: usize) -> Option<DecodedType> {
        if depth >= MAX_SIGNATURE_DEPTH {
            return None;
        }
        match self.peek_byte()? {
            b'L' => self.parse_class_type_sig(depth + 1),
            b'T' => self.parse_type_var_sig(),
            b'[' => {
                let mut dims = 0usize;
                while self.peek_byte() == Some(b'[') {
                    dims += 1;
                    self.pos += 1;
                    if dims > MAX_SIGNATURE_DEPTH {
                        return None;
                    }
                }
                let mut ty = self.parse_java_type_sig(depth + 1)?;
                for _ in 0..dims {
                    ty = DecodedType::Array {
                        elem: Box::new(ty),
                        nullability: None,
                    };
                }
                Some(ty)
            }
            _ => None,
        }
    }

    fn parse_type_var_sig(&mut self) -> Option<DecodedType> {
        self.expect_byte(b'T')?;
        let name = self.read_identifier_until(b";")?;
        self.expect_byte(b';')?;
        Some(DecodedType::TypeVar {
            name: name.to_string(),
            nullability: None,
        })
    }

    fn parse_class_type_sig(&mut self, depth: usize) -> Option<DecodedType> {
        if depth >= MAX_SIGNATURE_DEPTH {
            return None;
        }
        self.expect_byte(b'L')?;
        let internal_head = self.read_identifier_until(b"<.;")?;
        let head_name = self.format_internal_name(internal_head);
        let head_args = if self.peek_byte() == Some(b'<') {
            self.parse_type_args(depth + 1)?
        } else {
            Vec::new()
        };
        let mut segments = vec![ClassSegment {
            name: head_name,
            args: head_args,
            nullability: None,
        }];
        while self.peek_byte() == Some(b'.') {
            self.pos += 1;
            let suffix = self.read_identifier_until(b"<.;")?;
            let suffix_args = if self.peek_byte() == Some(b'<') {
                self.parse_type_args(depth + 1)?
            } else {
                Vec::new()
            };
            segments.push(ClassSegment {
                name: suffix.to_string(),
                args: suffix_args,
                nullability: None,
            });
        }
        self.expect_byte(b';')?;
        Some(DecodedType::Class { segments })
    }

    fn parse_type_args(&mut self, depth: usize) -> Option<Vec<DecodedType>> {
        if depth >= MAX_SIGNATURE_DEPTH {
            return None;
        }
        self.expect_byte(b'<')?;
        let mut args = Vec::new();
        while self.peek_byte() != Some(b'>') {
            let arg = match self.peek_byte()? {
                b'*' => {
                    self.pos += 1;
                    DecodedType::Wildcard(DecodedWildcard::Unbounded)
                }
                b'+' => {
                    self.pos += 1;
                    let bound = self.parse_ref_type_sig(depth + 1)?;
                    DecodedType::Wildcard(DecodedWildcard::Extends(Box::new(bound)))
                }
                b'-' => {
                    self.pos += 1;
                    let bound = self.parse_ref_type_sig(depth + 1)?;
                    DecodedType::Wildcard(DecodedWildcard::Super(Box::new(bound)))
                }
                _ => self.parse_ref_type_sig(depth + 1)?,
            };
            args.push(arg);
        }
        self.expect_byte(b'>')?;
        (!args.is_empty()).then_some(args)
    }
}

#[cfg(test)]
mod tests {
    use ristretto_classfile::{
        BaseType, ClassAccessFlags, ClassFile, ConstantPool, Field, FieldAccessFlags, FieldType,
        JAVA_17, JAVA_21, JAVA_25, Method, MethodAccessFlags, Version,
        attributes::{
            Attribute, Exports, ExportsFlags, InnerClass, MethodParameter,
            MethodParameterAccessFlags, ModuleAccessFlags, ModuleAttribute, NestedClassAccessFlags,
            Record as RecordAttr, Requires, RequiresFlags,
        },
    };

    use super::*;

    fn jdk_base_origin() -> BinaryOrigin {
        BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base")),
        }
    }

    fn serialize(cf: &ClassFile<'_>) -> Vec<u8> {
        let mut bytes = Vec::new();
        cf.to_bytes(&mut bytes).expect("serialize synthetic classfile");
        bytes
    }

    #[test]
    fn parses_java_27_class_with_generics_methods_and_method_parameters() {
        let mut cp = ConstantPool::default();
        let this_class = cp.add_class("com/example/Box").unwrap();
        let super_class = cp.add_class("java/lang/Object").unwrap();
        let iface = cp.add_class("java/io/Serializable").unwrap();

        let sig_name = cp.add_utf8("Signature").unwrap();
        let class_sig = cp.add_utf8("<T:Ljava/lang/Object;>Ljava/lang/Object;Ljava/io/Serializable;").unwrap();

        let field_name = cp.add_utf8("value").unwrap();
        let field_desc = cp.add_utf8("Ljava/lang/Object;").unwrap();
        let field_sig = cp.add_utf8("TT;").unwrap();

        let ctor_name = cp.add_utf8("<init>").unwrap();
        let ctor_desc = cp.add_utf8("(Ljava/lang/Object;)V").unwrap();
        let ctor_sig = cp.add_utf8("(TT;)V").unwrap();

        let method_name = cp.add_utf8("formatAll").unwrap();
        let method_desc = cp.add_utf8("(I[Ljava/lang/String;)Ljava/util/List;").unwrap();
        let method_sig = cp.add_utf8("(I[Ljava/lang/String;)Ljava/util/List<Ljava/lang/String;>;").unwrap();
        let mp_attr_name = cp.add_utf8("MethodParameters").unwrap();
        let p0_name = cp.add_utf8("limit").unwrap();
        let p1_name = cp.add_utf8("items").unwrap();

        let cf = ClassFile {
            version: JAVA_25,
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            this_class,
            super_class,
            interfaces: vec![iface],
            fields: vec![Field {
                access_flags: FieldAccessFlags::PUBLIC,
                name_index: field_name,
                descriptor_index: field_desc,
                field_type: FieldType::Object("java/lang/Object".into()),
                attributes: vec![Attribute::Signature {
                    name_index: sig_name,
                    signature_index: field_sig,
                }],
            }],
            methods: vec![
                Method {
                    access_flags: MethodAccessFlags::PUBLIC,
                    name_index: ctor_name,
                    descriptor_index: ctor_desc,
                    attributes: vec![Attribute::Signature {
                        name_index: sig_name,
                        signature_index: ctor_sig,
                    }],
                },
                Method {
                    access_flags: MethodAccessFlags::PUBLIC
                        | MethodAccessFlags::STATIC
                        | MethodAccessFlags::VARARGS,
                    name_index: method_name,
                    descriptor_index: method_desc,
                    attributes: vec![
                        Attribute::Signature {
                            name_index: sig_name,
                            signature_index: method_sig,
                        },
                        Attribute::MethodParameters {
                            name_index: mp_attr_name,
                            parameters: vec![
                                MethodParameter {
                                    name_index: p0_name,
                                    access_flags: MethodParameterAccessFlags::FINAL,
                                },
                                MethodParameter {
                                    name_index: p1_name,
                                    access_flags: MethodParameterAccessFlags::empty(),
                                },
                            ],
                        },
                    ],
                },
            ],
            attributes: vec![Attribute::Signature {
                name_index: sig_name,
                signature_index: class_sig,
            }],
            constant_pool: cp,
            ..Default::default()
        };

        let mut bytes = serialize(&cf);
        // Stamp Java 27 major version (71) into the classfile header to verify JDK 27 support.
        bytes[6..8].copy_from_slice(&71u16.to_be_bytes());
        assert_eq!(u16::from_be_bytes([bytes[6], bytes[7]]), 71);
        let Some(ParsedClassfile::Type(ty)) = parse_class_bytes(&bytes, jdk_base_origin()).unwrap()
        else {
            panic!("expected ParsedClassfile::Type");
        };

        assert_eq!(ty.name, "Box");
        assert_eq!(ty.qualified_name, "com.example.Box");
        assert_eq!(ty.package, "com.example");
        assert_eq!(ty.kind, TypeKind::Class);
        assert_eq!(ty.type_params, vec![SmolStr::new("T")]);
        assert_eq!(ty.super_class.as_deref(), Some("java.lang.Object"));
        assert_eq!(ty.interfaces, vec!["java.io.Serializable".to_string()]);

        assert_eq!(ty.members.len(), 3);
        let field = &ty.members[0];
        assert_eq!(field.kind, MemberKind::Field);
        assert_eq!(field.name, "value");
        assert_eq!(field.type_ref.as_deref(), Some("T"));

        let ctor = &ty.members[1];
        assert_eq!(ctor.kind, MemberKind::Constructor);
        assert_eq!(ctor.name, "Box");
        assert_eq!(ctor.type_ref, None);
        assert_eq!(ctor.params[0].type_ref, "T");
        assert_eq!(ctor.params[0].name.as_deref(), Some("arg0"));

        let method = &ty.members[2];
        assert_eq!(method.kind, MemberKind::Method);
        assert_eq!(method.name, "formatAll");
        assert!(method.is_static);
        assert!(method.is_varargs);
        assert_eq!(method.type_ref.as_deref(), Some("java.util.List<String>"));
        assert_eq!(method.params.len(), 2);
        assert_eq!(method.params[0].name.as_deref(), Some("limit"));
        assert_eq!(method.params[0].type_ref, "int");
        assert!(!method.params[0].is_varargs);
        assert_eq!(method.params[1].name.as_deref(), Some("items"));
        assert_eq!(method.params[1].type_ref, "String");
        assert!(method.params[1].is_varargs);
    }

    #[test]
    fn parses_interface_enum_record_annotation_and_sealed_permits() {
        // 1. Sealed interface with PermittedSubclasses & InnerClasses
        let mut cp = ConstantPool::default();
        let this_class = cp.add_class("java/util/Map").unwrap();
        let super_class = cp.add_class("java/lang/Object").unwrap();
        let entry_class = cp.add_class("java/util/Map$Entry").unwrap();
        let entry_name = cp.add_utf8("Entry").unwrap();
        let ic_attr = cp.add_utf8("InnerClasses").unwrap();
        let permits_attr = cp.add_utf8("PermittedSubclasses").unwrap();
        let sub_class = cp.add_class("java/util/HashMap").unwrap();

        let iface_cf = ClassFile {
            version: JAVA_17,
            access_flags: ClassAccessFlags::PUBLIC
                | ClassAccessFlags::INTERFACE
                | ClassAccessFlags::ABSTRACT,
            this_class,
            super_class,
            attributes: vec![
                Attribute::InnerClasses {
                    name_index: ic_attr,
                    classes: vec![InnerClass {
                        class_info_index: entry_class,
                        outer_class_info_index: this_class,
                        name_index: entry_name,
                        access_flags: NestedClassAccessFlags::PUBLIC
                            | NestedClassAccessFlags::STATIC
                            | NestedClassAccessFlags::INTERFACE
                            | NestedClassAccessFlags::ABSTRACT,
                    }],
                },
                Attribute::PermittedSubclasses {
                    name_index: permits_attr,
                    class_indexes: vec![sub_class],
                },
            ],
            constant_pool: cp,
            ..Default::default()
        };
        let Some(ParsedClassfile::Type(iface_ty)) =
            parse_class_bytes(&serialize(&iface_cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected interface");
        };
        assert_eq!(iface_ty.kind, TypeKind::Interface);
        assert_eq!(iface_ty.nested_types, vec![SmolStr::new("Entry")]);
        assert_eq!(
            iface_ty.permitted_subclasses,
            vec!["java.util.HashMap".to_string()]
        );

        // 2. Nested Map$Entry classfile
        let mut cp = ConstantPool::default();
        let entry_cls = cp.add_class("java/util/Map$Entry").unwrap();
        let map_cls = cp.add_class("java/util/Map").unwrap();
        let obj_cls = cp.add_class("java/lang/Object").unwrap();
        let entry_utf = cp.add_utf8("Entry").unwrap();
        let ic_attr = cp.add_utf8("InnerClasses").unwrap();
        let entry_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::INTERFACE | ClassAccessFlags::ABSTRACT,
            this_class: entry_cls,
            super_class: obj_cls,
            attributes: vec![Attribute::InnerClasses {
                name_index: ic_attr,
                classes: vec![InnerClass {
                    class_info_index: entry_cls,
                    outer_class_info_index: map_cls,
                    name_index: entry_utf,
                    access_flags: NestedClassAccessFlags::PUBLIC
                        | NestedClassAccessFlags::STATIC
                        | NestedClassAccessFlags::INTERFACE,
                }],
            }],
            constant_pool: cp,
            ..Default::default()
        };
        let Some(ParsedClassfile::Type(entry_ty)) =
            parse_class_bytes(&serialize(&entry_cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected Map.Entry");
        };
        assert_eq!(entry_ty.name, "Entry");
        assert_eq!(entry_ty.qualified_name, "java.util.Map.Entry");
        assert_eq!(entry_ty.internal_name, "java/util/Map$Entry");
        assert_eq!(entry_ty.enclosing_type.as_deref(), Some("java.util.Map"));
        assert!(entry_ty.is_public);
        assert!(entry_ty.is_static);

        // 3. Enum
        let mut cp = ConstantPool::default();
        let enum_cls = cp.add_class("p/Color").unwrap();
        let enum_super = cp.add_class("java/lang/Enum").unwrap();
        let red_name = cp.add_utf8("RED").unwrap();
        let red_desc = cp.add_utf8("Lp/Color;").unwrap();
        let enum_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC
                | ClassAccessFlags::FINAL
                | ClassAccessFlags::ENUM,
            this_class: enum_cls,
            super_class: enum_super,
            fields: vec![Field {
                access_flags: FieldAccessFlags::PUBLIC
                    | FieldAccessFlags::STATIC
                    | FieldAccessFlags::FINAL
                    | FieldAccessFlags::ENUM,
                name_index: red_name,
                descriptor_index: red_desc,
                field_type: FieldType::Object("p/Color".into()),
                attributes: vec![],
            }],
            constant_pool: cp,
            ..Default::default()
        };
        let Some(ParsedClassfile::Type(enum_ty)) =
            parse_class_bytes(&serialize(&enum_cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected enum");
        };
        assert_eq!(enum_ty.kind, TypeKind::Enum);
        assert_eq!(enum_ty.members[0].kind, MemberKind::EnumConstant);
        assert_eq!(enum_ty.members[0].name, "RED");

        // 4. Record
        let mut cp = ConstantPool::default();
        let rec_cls = cp.add_class("p/Point").unwrap();
        let rec_super = cp.add_class("java/lang/Record").unwrap();
        let rec_attr = cp.add_utf8("Record").unwrap();
        let x_name = cp.add_utf8("x").unwrap();
        let i_desc = cp.add_utf8("I").unwrap();
        let rec_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL,
            this_class: rec_cls,
            super_class: rec_super,
            attributes: vec![Attribute::Record {
                name_index: rec_attr,
                records: vec![RecordAttr {
                    name_index: x_name,
                    descriptor_index: i_desc,
                    attributes: vec![],
                }],
            }],
            constant_pool: cp,
            ..Default::default()
        };
        let Some(ParsedClassfile::Type(rec_ty)) =
            parse_class_bytes(&serialize(&rec_cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected record");
        };
        assert_eq!(rec_ty.kind, TypeKind::Record);
        assert_eq!(rec_ty.members[0].kind, MemberKind::RecordComponent);
        assert_eq!(rec_ty.members[0].name, "x");
        assert_eq!(rec_ty.members[0].type_ref.as_deref(), Some("int"));

        // 5. Annotation
        let mut cp = ConstantPool::default();
        let ann_cls = cp.add_class("p/Tag").unwrap();
        let obj_cls = cp.add_class("java/lang/Object").unwrap();
        let ann_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC
                | ClassAccessFlags::INTERFACE
                | ClassAccessFlags::ABSTRACT
                | ClassAccessFlags::ANNOTATION,
            this_class: ann_cls,
            super_class: obj_cls,
            constant_pool: cp,
            ..Default::default()
        };
        let Some(ParsedClassfile::Type(ann_ty)) =
            parse_class_bytes(&serialize(&ann_cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected annotation");
        };
        assert_eq!(ann_ty.kind, TypeKind::Annotation);
    }

    #[test]
    fn parses_module_info_with_unqualified_exports_and_requires_transitive() {
        let mut cp = ConstantPool::default();
        let this_class = cp.add_class("module-info").unwrap();
        let mod_attr_name = cp.add_utf8("Module").unwrap();
        let self_mod = cp.add_module("java.sql").unwrap();
        let dep_trans = cp.add_module("java.xml").unwrap();
        let dep_priv = cp.add_module("java.logging").unwrap();
        let friend_mod = cp.add_module("some.friend").unwrap();
        let pub_pkg = cp.add_package("java/sql").unwrap();
        let qual_pkg = cp.add_package("com/sun/sql/internal").unwrap();

        let mod_cf = ClassFile {
            version: Version::Java21 { minor: 0 },
            access_flags: ClassAccessFlags::MODULE,
            this_class,
            super_class: 0,
            attributes: vec![Attribute::Module {
                name_index: mod_attr_name,
                module: Box::new(ModuleAttribute {
                    module_name_index: self_mod,
                    flags: ModuleAccessFlags::empty(),
                    version_index: 0,
                    requires: vec![
                        Requires {
                            index: dep_trans,
                            flags: RequiresFlags::TRANSITIVE,
                            version_index: 0,
                        },
                        Requires {
                            index: dep_priv,
                            flags: RequiresFlags::empty(),
                            version_index: 0,
                        },
                    ],
                    exports: vec![
                        Exports {
                            index: pub_pkg,
                            flags: ExportsFlags::empty(),
                            to_index: vec![],
                        },
                        Exports {
                            index: qual_pkg,
                            flags: ExportsFlags::empty(),
                            to_index: vec![friend_mod],
                        },
                    ],
                    opens: vec![],
                    uses: vec![],
                    provides: vec![],
                }),
            }],
            constant_pool: cp,
            ..Default::default()
        };

        let Some(ParsedClassfile::Module(m)) =
            parse_class_bytes(&serialize(&mod_cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected ParsedClassfile::Module");
        };
        assert_eq!(m.name, "java.sql");
        assert_eq!(m.exports, vec!["java.sql".to_string()]);
        assert_eq!(m.requires_transitive, vec![SmolStr::new("java.xml")]);
    }

    #[test]
    fn filters_synthetic_bridge_private_and_anonymous_local_entries() {
        // Package-private top-level class -> None
        let mut cp = ConstantPool::default();
        let pkg_priv_cls = cp.add_class("p/Hidden").unwrap();
        let obj_cls = cp.add_class("java/lang/Object").unwrap();
        let hidden_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::SUPER,
            this_class: pkg_priv_cls,
            super_class: obj_cls,
            constant_pool: cp,
            ..Default::default()
        };
        assert_eq!(
            parse_class_bytes(&serialize(&hidden_cf), jdk_base_origin()).unwrap(),
            None
        );

        // Anonymous class ($1) -> None
        let mut cp = ConstantPool::default();
        let anon_cls = cp.add_class("p/Outer$1").unwrap();
        let obj_cls = cp.add_class("java/lang/Object").unwrap();
        let anon_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            this_class: anon_cls,
            super_class: obj_cls,
            constant_pool: cp,
            ..Default::default()
        };
        assert_eq!(
            parse_class_bytes(&serialize(&anon_cf), jdk_base_origin()).unwrap(),
            None
        );

        // Local class ($1Local) -> None
        let mut cp = ConstantPool::default();
        let local_cls = cp.add_class("p/Outer$1Local").unwrap();
        let obj_cls = cp.add_class("java/lang/Object").unwrap();
        let local_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            this_class: local_cls,
            super_class: obj_cls,
            constant_pool: cp,
            ..Default::default()
        };
        assert_eq!(
            parse_class_bytes(&serialize(&local_cf), jdk_base_origin()).unwrap(),
            None
        );

        // InnerClasses without outer_class_info_index -> None
        let mut cp = ConstantPool::default();
        let inner_no_outer = cp.add_class("p/Outer$NamedLocal").unwrap();
        let obj_cls = cp.add_class("java/lang/Object").unwrap();
        let ic_attr = cp.add_utf8("InnerClasses").unwrap();
        let local_name = cp.add_utf8("NamedLocal").unwrap();
        let no_outer_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            this_class: inner_no_outer,
            super_class: obj_cls,
            attributes: vec![Attribute::InnerClasses {
                name_index: ic_attr,
                classes: vec![InnerClass {
                    class_info_index: inner_no_outer,
                    outer_class_info_index: 0,
                    name_index: local_name,
                    access_flags: NestedClassAccessFlags::PUBLIC,
                }],
            }],
            constant_pool: cp,
            ..Default::default()
        };
        assert_eq!(
            parse_class_bytes(&serialize(&no_outer_cf), jdk_base_origin()).unwrap(),
            None
        );

        // Public class with synthetic/bridge/private members + malformed generic signature fallback.
        let mut cp = ConstantPool::default();
        let pub_cls = cp.add_class("p/Api").unwrap();
        let obj_cls = cp.add_class("java/lang/Object").unwrap();
        let f_priv = cp.add_utf8("secret").unwrap();
        let f_synth = cp.add_utf8("synth").unwrap();
        let f_prot = cp.add_utf8("prot").unwrap();
        let desc_i = cp.add_utf8("I").unwrap();
        let desc_obj = cp.add_utf8("Ljava/lang/Object;").unwrap();
        let m_bridge = cp.add_utf8("compareTo").unwrap();
        let m_clinit = cp.add_utf8("<clinit>").unwrap();
        let m_ok = cp.add_utf8("ok").unwrap();
        let desc_m = cp.add_utf8("()Ljava/lang/String;").unwrap();
        let desc_v = cp.add_utf8("()V").unwrap();
        let sig_attr = cp.add_utf8("Signature").unwrap();
        let bad_sig = cp.add_utf8("<<<BROKEN").unwrap();

        let api_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            this_class: pub_cls,
            super_class: obj_cls,
            fields: vec![
                Field {
                    access_flags: FieldAccessFlags::PRIVATE,
                    name_index: f_priv,
                    descriptor_index: desc_i,
                    field_type: FieldType::Base(BaseType::Int),
                    attributes: vec![],
                },
                Field {
                    access_flags: FieldAccessFlags::PUBLIC | FieldAccessFlags::SYNTHETIC,
                    name_index: f_synth,
                    descriptor_index: desc_i,
                    field_type: FieldType::Base(BaseType::Int),
                    attributes: vec![],
                },
                Field {
                    access_flags: FieldAccessFlags::PROTECTED,
                    name_index: f_prot,
                    descriptor_index: desc_obj,
                    field_type: FieldType::Object("java/lang/Object".into()),
                    attributes: vec![Attribute::Signature {
                        name_index: sig_attr,
                        signature_index: bad_sig,
                    }],
                },
            ],
            methods: vec![
                Method {
                    access_flags: MethodAccessFlags::PUBLIC | MethodAccessFlags::BRIDGE,
                    name_index: m_bridge,
                    descriptor_index: desc_v,
                    attributes: vec![],
                },
                Method {
                    access_flags: MethodAccessFlags::PUBLIC | MethodAccessFlags::STATIC,
                    name_index: m_clinit,
                    descriptor_index: desc_v,
                    attributes: vec![],
                },
                Method {
                    access_flags: MethodAccessFlags::PUBLIC,
                    name_index: m_ok,
                    descriptor_index: desc_m,
                    attributes: vec![Attribute::Signature {
                        name_index: sig_attr,
                        signature_index: bad_sig,
                    }],
                },
            ],
            constant_pool: cp,
            ..Default::default()
        };

        let Some(ParsedClassfile::Type(api_ty)) =
            parse_class_bytes(&serialize(&api_cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected Api type");
        };
        assert_eq!(api_ty.members.len(), 2);
        assert_eq!(api_ty.members[0].name, "prot");
        assert!(!api_ty.members[0].is_public);
        assert_eq!(api_ty.members[0].type_ref.as_deref(), Some("Object"));
        assert_eq!(api_ty.members[1].name, "ok");
        assert_eq!(api_ty.members[1].type_ref.as_deref(), Some("String"));
    }

    #[test]
    fn returns_typed_error_on_corrupt_class_bytes() {
        let err = parse_class_bytes(b"not a classfile", jdk_base_origin()).unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn parses_class_and_method_generic_signatures_bounds_and_supertypes() {
        let mut cp = ConstantPool::default();
        let this_class = cp.add_class("com/example/MyList").unwrap();
        let super_class = cp.add_class("java/util/AbstractList").unwrap();
        let iface_list = cp.add_class("java/util/List").unwrap();
        let iface_ra = cp.add_class("java/util/RandomAccess").unwrap();
        let iface_comp = cp.add_class("java/util/Comparator").unwrap();

        let sig_attr = cp.add_utf8("Signature").unwrap();
        let class_sig = cp
            .add_utf8(
                "<T:Ljava/lang/Number;:Ljava/lang/Comparable<TT;>;U:Ljava/lang/Object;V::Ljava/io/Serializable;>\
                Ljava/util/AbstractList<Ljava/util/Map<TK;TV;>.Entry<TK;TV;>;>;\
                Ljava/util/List<+Ljava/lang/Number;>;\
                Ljava/util/RandomAccess;\
                Ljava/util/Comparator<-TT;>;",
            )
            .unwrap();

        let ctor_name = cp.add_utf8("<init>").unwrap();
        let ctor_desc = cp.add_utf8("(Ljava/lang/Exception;)V").unwrap();
        let ctor_sig = cp.add_utf8("<E:Ljava/lang/Exception;>(TE;)V").unwrap();

        let method_name = cp.add_utf8("max").unwrap();
        let method_desc = cp
            .add_utf8("(Ljava/lang/Comparable;Ljava/lang/Object;)Ljava/lang/Comparable;")
            .unwrap();
        let method_sig = cp
            .add_utf8("<T:Ljava/lang/Comparable<TT;>;U:Ljava/lang/Object;>(TT;TU;)TT;")
            .unwrap();

        let plain_name = cp.add_utf8("plain").unwrap();
        let plain_desc = cp.add_utf8("()I").unwrap();

        let cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            this_class,
            super_class,
            interfaces: vec![iface_list, iface_ra, iface_comp],
            methods: vec![
                Method {
                    access_flags: MethodAccessFlags::PUBLIC,
                    name_index: ctor_name,
                    descriptor_index: ctor_desc,
                    attributes: vec![Attribute::Signature {
                        name_index: sig_attr,
                        signature_index: ctor_sig,
                    }],
                },
                Method {
                    access_flags: MethodAccessFlags::PUBLIC | MethodAccessFlags::STATIC,
                    name_index: method_name,
                    descriptor_index: method_desc,
                    attributes: vec![Attribute::Signature {
                        name_index: sig_attr,
                        signature_index: method_sig,
                    }],
                },
                Method {
                    access_flags: MethodAccessFlags::PUBLIC,
                    name_index: plain_name,
                    descriptor_index: plain_desc,
                    attributes: vec![],
                },
            ],
            attributes: vec![Attribute::Signature {
                name_index: sig_attr,
                signature_index: class_sig,
            }],
            constant_pool: cp,
            ..Default::default()
        };

        let Some(ParsedClassfile::Type(ty)) =
            parse_class_bytes(&serialize(&cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected ParsedClassfile::Type");
        };

        assert_eq!(
            ty.type_params,
            vec![SmolStr::new("T"), SmolStr::new("U"), SmolStr::new("V")]
        );
        assert_eq!(
            ty.type_param_bounds,
            BTreeMap::from([
                (
                    SmolStr::new("T"),
                    vec![
                        "java.lang.Number".to_string(),
                        "java.lang.Comparable<T>".to_string()
                    ]
                ),
                (
                    SmolStr::new("V"),
                    vec!["java.io.Serializable".to_string()]
                ),
            ])
        );
        assert_eq!(
            ty.super_class_generic.as_deref(),
            Some("java.util.AbstractList<java.util.Map.Entry<K, V>>")
        );
        assert_eq!(
            ty.interfaces_generic,
            vec![
                "java.util.List<? extends java.lang.Number>".to_string(),
                "java.util.RandomAccess".to_string(),
                "java.util.Comparator<? super T>".to_string(),
            ]
        );

        let ctor = &ty.members[0];
        assert_eq!(ctor.type_params, vec![SmolStr::new("E")]);
        assert_eq!(
            ctor.type_param_bounds,
            BTreeMap::from([(SmolStr::new("E"), vec!["java.lang.Exception".to_string()])])
        );
        assert_eq!(ctor.type_ref, None);

        let max = &ty.members[1];
        assert_eq!(max.type_params, vec![SmolStr::new("T"), SmolStr::new("U")]);
        assert_eq!(
            max.type_param_bounds,
            BTreeMap::from([(
                SmolStr::new("T"),
                vec!["java.lang.Comparable<T>".to_string()]
            )])
        );
        assert_eq!(max.type_ref.as_deref(), Some("T"));
        assert_eq!(max.params[0].type_ref, "T");
        assert_eq!(max.params[1].type_ref, "U");

        let plain = &ty.members[2];
        assert!(plain.type_params.is_empty());
        assert!(plain.type_param_bounds.is_empty());
    }

    #[test]
    fn parses_non_generic_class_extending_generic_superclass_and_wildcard_star() {
        let mut cp = ConstantPool::default();
        let this_class = cp.add_class("com/example/StringList").unwrap();
        let super_class = cp.add_class("java/util/ArrayList").unwrap();
        let iface = cp.add_class("java/util/Collection").unwrap();
        let sig_attr = cp.add_utf8("Signature").unwrap();
        let class_sig = cp
            .add_utf8("Ljava/util/ArrayList<Ljava/lang/String;>;Ljava/util/Collection<*>;")
            .unwrap();

        let cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            this_class,
            super_class,
            interfaces: vec![iface],
            attributes: vec![Attribute::Signature {
                name_index: sig_attr,
                signature_index: class_sig,
            }],
            constant_pool: cp,
            ..Default::default()
        };

        let Some(ParsedClassfile::Type(ty)) =
            parse_class_bytes(&serialize(&cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected ParsedClassfile::Type");
        };

        assert!(ty.type_params.is_empty());
        assert!(ty.type_param_bounds.is_empty());
        assert_eq!(
            ty.super_class_generic.as_deref(),
            Some("java.util.ArrayList<java.lang.String>")
        );
        assert_eq!(
            ty.interfaces_generic,
            vec!["java.util.Collection<?>".to_string()]
        );

        // Class without Signature attribute leaves generic fields empty/None.
        let mut cp2 = ConstantPool::default();
        let plain_cls = cp2.add_class("com/example/Plain").unwrap();
        let obj_cls = cp2.add_class("java/lang/Object").unwrap();
        let plain_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            this_class: plain_cls,
            super_class: obj_cls,
            constant_pool: cp2,
            ..Default::default()
        };
        let Some(ParsedClassfile::Type(plain_ty)) =
            parse_class_bytes(&serialize(&plain_cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected ParsedClassfile::Type");
        };
        assert_eq!(plain_ty.super_class_generic, None);
        assert!(plain_ty.interfaces_generic.is_empty());
    }

    #[test]
    fn parses_package_info_and_module_info_nullness_scopes() {
        // 1. package-info.class (marked ACC_SYNTHETIC by javac!) with @NullMarked
        let mut cp = ConstantPool::default();
        let this_cls = cp.add_class("com/example/package-info").unwrap();
        let obj_cls = cp.add_class("java/lang/Object").unwrap();
        let rva_attr = cp.add_utf8("RuntimeVisibleAnnotations").unwrap();
        let null_marked_desc = cp.add_utf8("Lorg/jspecify/annotations/NullMarked;").unwrap();

        let pkg_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::INTERFACE
                | ClassAccessFlags::ABSTRACT
                | ClassAccessFlags::SYNTHETIC,
            this_class: this_cls,
            super_class: obj_cls,
            attributes: vec![Attribute::RuntimeVisibleAnnotations {
                name_index: rva_attr,
                annotations: vec![Annotation {
                    type_index: null_marked_desc,
                    elements: vec![],
                }],
            }],
            constant_pool: cp,
            ..Default::default()
        };
        let Some(ParsedClassfile::Package {
            package,
            nullness_scope,
        }) = parse_class_bytes(&serialize(&pkg_cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected ParsedClassfile::Package");
        };
        assert_eq!(package, "com.example");
        assert_eq!(
            nullness_scope,
            NullnessScope::NonNullByDefault(NullabilitySource::JSpecify)
        );

        // 2. module-info.class with @NullMarked
        let mut cp = ConstantPool::default();
        let this_cls = cp.add_class("module-info").unwrap();
        let mod_attr = cp.add_utf8("Module").unwrap();
        let self_mod = cp.add_module("com.example.mod").unwrap();
        let rva_attr = cp.add_utf8("RuntimeVisibleAnnotations").unwrap();
        let null_marked_desc = cp.add_utf8("Lorg/jspecify/annotations/NullMarked;").unwrap();

        let mod_cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::MODULE,
            this_class: this_cls,
            super_class: 0,
            attributes: vec![
                Attribute::Module {
                    name_index: mod_attr,
                    module: Box::new(ModuleAttribute {
                        module_name_index: self_mod,
                        flags: ModuleAccessFlags::empty(),
                        version_index: 0,
                        requires: vec![],
                        exports: vec![],
                        opens: vec![],
                        uses: vec![],
                        provides: vec![],
                    }),
                },
                Attribute::RuntimeVisibleAnnotations {
                    name_index: rva_attr,
                    annotations: vec![Annotation {
                        type_index: null_marked_desc,
                        elements: vec![],
                    }],
                },
            ],
            constant_pool: cp,
            ..Default::default()
        };
        let Some(ParsedClassfile::Module(m)) =
            parse_class_bytes(&serialize(&mod_cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected ParsedClassfile::Module");
        };
        assert_eq!(
            m.nullness_scope,
            NullnessScope::NonNullByDefault(NullabilitySource::JSpecify)
        );
    }

    #[test]
    fn decodes_visible_and_invisible_annotations_and_type_paths() {
        let mut cp = ConstantPool::default();
        let this_cls = cp.add_class("com/example/NullApi").unwrap();
        let super_cls = cp.add_class("java/util/ArrayList").unwrap();

        let sig_attr = cp.add_utf8("Signature").unwrap();
        let rva_attr = cp.add_utf8("RuntimeVisibleAnnotations").unwrap();
        let ria_attr = cp.add_utf8("RuntimeInvisibleAnnotations").unwrap();
        let rvpa_attr = cp.add_utf8("RuntimeVisibleParameterAnnotations").unwrap();
        let rvta_attr = cp.add_utf8("RuntimeVisibleTypeAnnotations").unwrap();
        let rita_attr = cp.add_utf8("RuntimeInvisibleTypeAnnotations").unwrap();

        let jspec_marked = cp.add_utf8("Lorg/jspecify/annotations/NullMarked;").unwrap();
        let jspec_unmarked = cp
            .add_utf8("Lorg/jspecify/annotations/NullUnmarked;")
            .unwrap();
        let jspec_nullable = cp.add_utf8("Lorg/jspecify/annotations/Nullable;").unwrap();
        let jspec_nonnull = cp.add_utf8("Lorg/jspecify/annotations/NonNull;").unwrap();
        let jb_nullable = cp
            .add_utf8("Lorg/jetbrains/annotations/Nullable;")
            .unwrap();
        let javax_nullable = cp.add_utf8("Ljavax/annotation/Nullable;").unwrap();

        // Class signature: `<T:Ljava/lang/Object;>Ljava/util/ArrayList<Ljava/lang/String;>;`
        // with `<T extends @Nullable Object>` and `extends ArrayList<@Nullable String>`.
        let class_sig = cp
            .add_utf8("<T:Ljava/lang/Object;>Ljava/util/ArrayList<Ljava/lang/String;>;")
            .unwrap();

        // Field 1: `List<@Nullable String>`
        let f_list = cp.add_utf8("list").unwrap();
        let f_list_desc = cp.add_utf8("Ljava/util/List;").unwrap();
        let f_list_sig = cp.add_utf8("Ljava/util/List<Ljava/lang/String;>;").unwrap();

        // Field 2: `String @Nullable []` (type_path = [])
        let f_arr_outer = cp.add_utf8("arrOuter").unwrap();
        let f_arr_desc = cp.add_utf8("[Ljava/lang/String;").unwrap();

        // Field 3: `@org.jetbrains.annotations.Nullable String[]` (CLASS-retention invisible!)
        let f_arr_elem = cp.add_utf8("arrElem").unwrap();

        // Field 4: `Map<String, @Nullable List<@NonNull User>>`
        let f_map = cp.add_utf8("nestedMap").unwrap();
        let f_map_desc = cp.add_utf8("Ljava/util/Map;").unwrap();
        let f_map_sig = cp
            .add_utf8("Ljava/util/Map<Ljava/lang/String;Ljava/util/List<Lcom/example/User;>;>;")
            .unwrap();

        // Method: `@NullUnmarked List<? extends @Nullable String> transform(@javax.annotation.Nullable String a, @Nullable String[] b)`
        let m_name = cp.add_utf8("transform").unwrap();
        let m_desc = cp
            .add_utf8("(Ljava/lang/String;[Ljava/lang/String;)Ljava/util/List;")
            .unwrap();
        let m_sig = cp
            .add_utf8("(Ljava/lang/String;[Ljava/lang/String;)Ljava/util/List<+Ljava/lang/String;>;")
            .unwrap();

        let cf = ClassFile {
            version: JAVA_21,
            access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
            this_class: this_cls,
            super_class: super_cls,
            fields: vec![
                Field {
                    access_flags: FieldAccessFlags::PUBLIC,
                    name_index: f_list,
                    descriptor_index: f_list_desc,
                    field_type: FieldType::Object("java/util/List".into()),
                    attributes: vec![
                        Attribute::Signature {
                            name_index: sig_attr,
                            signature_index: f_list_sig,
                        },
                        Attribute::RuntimeVisibleTypeAnnotations {
                            name_index: rvta_attr,
                            type_annotations: vec![TypeAnnotation {
                                target_type: TargetType::Empty { target_type: 0x13 },
                                type_path: vec![TargetPath {
                                    type_path_kind: 3,
                                    type_argument_index: 0,
                                }],
                                type_index: jspec_nullable,
                                elements: vec![],
                            }],
                        },
                    ],
                },
                Field {
                    access_flags: FieldAccessFlags::PUBLIC,
                    name_index: f_arr_outer,
                    descriptor_index: f_arr_desc,
                    field_type: FieldType::Array(Box::new(FieldType::Object(
                        "java/lang/String".into(),
                    ))),
                    attributes: vec![Attribute::RuntimeVisibleTypeAnnotations {
                        name_index: rvta_attr,
                        type_annotations: vec![TypeAnnotation {
                            target_type: TargetType::Empty { target_type: 0x13 },
                            type_path: vec![],
                            type_index: jspec_nullable,
                            elements: vec![],
                        }],
                    }],
                },
                Field {
                    access_flags: FieldAccessFlags::PUBLIC,
                    name_index: f_arr_elem,
                    descriptor_index: f_arr_desc,
                    field_type: FieldType::Array(Box::new(FieldType::Object(
                        "java/lang/String".into(),
                    ))),
                    attributes: vec![
                        // JetBrains emits both RuntimeInvisibleAnnotations and RuntimeInvisibleTypeAnnotations.
                        Attribute::RuntimeInvisibleAnnotations {
                            name_index: ria_attr,
                            annotations: vec![Annotation {
                                type_index: jb_nullable,
                                elements: vec![],
                            }],
                        },
                        Attribute::RuntimeInvisibleTypeAnnotations {
                            name_index: rita_attr,
                            type_annotations: vec![TypeAnnotation {
                                target_type: TargetType::Empty { target_type: 0x13 },
                                type_path: vec![TargetPath {
                                    type_path_kind: 0,
                                    type_argument_index: 0,
                                }],
                                type_index: jb_nullable,
                                elements: vec![],
                            }],
                        },
                    ],
                },
                Field {
                    access_flags: FieldAccessFlags::PUBLIC,
                    name_index: f_map,
                    descriptor_index: f_map_desc,
                    field_type: FieldType::Object("java/util/Map".into()),
                    attributes: vec![
                        Attribute::Signature {
                            name_index: sig_attr,
                            signature_index: f_map_sig,
                        },
                        Attribute::RuntimeVisibleTypeAnnotations {
                            name_index: rvta_attr,
                            type_annotations: vec![
                                TypeAnnotation {
                                    target_type: TargetType::Empty { target_type: 0x13 },
                                    type_path: vec![TargetPath {
                                        type_path_kind: 3,
                                        type_argument_index: 1,
                                    }],
                                    type_index: jspec_nullable,
                                    elements: vec![],
                                },
                                TypeAnnotation {
                                    target_type: TargetType::Empty { target_type: 0x13 },
                                    type_path: vec![
                                        TargetPath {
                                            type_path_kind: 3,
                                            type_argument_index: 1,
                                        },
                                        TargetPath {
                                            type_path_kind: 3,
                                            type_argument_index: 0,
                                        },
                                    ],
                                    type_index: jspec_nonnull,
                                    elements: vec![],
                                },
                            ],
                        },
                    ],
                },
            ],
            methods: vec![Method {
                access_flags: MethodAccessFlags::PUBLIC,
                name_index: m_name,
                descriptor_index: m_desc,
                attributes: vec![
                    Attribute::Signature {
                        name_index: sig_attr,
                        signature_index: m_sig,
                    },
                    Attribute::RuntimeVisibleAnnotations {
                        name_index: rva_attr,
                        annotations: vec![Annotation {
                            type_index: jspec_unmarked,
                            elements: vec![],
                        }],
                    },
                    Attribute::RuntimeVisibleParameterAnnotations {
                        name_index: rvpa_attr,
                        parameter_annotations: vec![
                            ristretto_classfile::attributes::ParameterAnnotation {
                                annotations: vec![Annotation {
                                    type_index: javax_nullable,
                                    elements: vec![],
                                }],
                            },
                            ristretto_classfile::attributes::ParameterAnnotation {
                                annotations: vec![],
                            },
                        ],
                    },
                    Attribute::RuntimeVisibleTypeAnnotations {
                        name_index: rvta_attr,
                        type_annotations: vec![
                            TypeAnnotation {
                                target_type: TargetType::Empty { target_type: 0x14 },
                                type_path: vec![
                                    TargetPath {
                                        type_path_kind: 3,
                                        type_argument_index: 0,
                                    },
                                    TargetPath {
                                        type_path_kind: 2,
                                        type_argument_index: 0,
                                    },
                                ],
                                type_index: jspec_nullable,
                                elements: vec![],
                            },
                            TypeAnnotation {
                                target_type: TargetType::FormalParameter {
                                    target_type: 0x16,
                                    formal_parameter_index: 1,
                                },
                                type_path: vec![TargetPath {
                                    type_path_kind: 0,
                                    type_argument_index: 0,
                                }],
                                type_index: jspec_nullable,
                                elements: vec![],
                            },
                        ],
                    },
                ],
            }],
            attributes: vec![
                Attribute::Signature {
                    name_index: sig_attr,
                    signature_index: class_sig,
                },
                Attribute::RuntimeVisibleAnnotations {
                    name_index: rva_attr,
                    annotations: vec![Annotation {
                        type_index: jspec_marked,
                        elements: vec![],
                    }],
                },
                Attribute::RuntimeVisibleTypeAnnotations {
                    name_index: rvta_attr,
                    type_annotations: vec![
                        TypeAnnotation {
                            target_type: TargetType::TypeParameterBound {
                                target_type: 0x11,
                                type_parameter_index: 0,
                                bound_index: 0,
                            },
                            type_path: vec![],
                            type_index: jspec_nullable,
                            elements: vec![],
                        },
                        TypeAnnotation {
                            target_type: TargetType::SuperType {
                                target_type: 0x10,
                                supertype_index: u16::MAX,
                            },
                            type_path: vec![TargetPath {
                                type_path_kind: 3,
                                type_argument_index: 0,
                            }],
                            type_index: jspec_nullable,
                            elements: vec![],
                        },
                    ],
                },
            ],
            constant_pool: cp,
            ..Default::default()
        };

        let Some(ParsedClassfile::Type(ty)) =
            parse_class_bytes(&serialize(&cf), jdk_base_origin()).unwrap()
        else {
            panic!("expected ParsedClassfile::Type");
        };

        assert_eq!(
            ty.nullness_scope,
            NullnessScope::NonNullByDefault(NullabilitySource::JSpecify)
        );
        assert_eq!(
            ty.type_param_bounds,
            BTreeMap::from([(SmolStr::new("T"), vec!["java.lang.Object?".to_string()])])
        );
        assert_eq!(
            ty.super_class_generic.as_deref(),
            Some("java.util.ArrayList<java.lang.String?>")
        );

        assert_eq!(
            ty.members[0].type_ref.as_deref(),
            Some("java.util.List<String?>")
        );
        assert_eq!(ty.members[1].type_ref.as_deref(), Some("String[]?"));
        assert_eq!(ty.members[2].type_ref.as_deref(), Some("String?[]"));
        assert_eq!(ty.members[2].decl_nullability, None);
        assert_eq!(
            ty.members[3].type_ref.as_deref(),
            Some("java.util.Map<String, java.util.List<com.example.User!>?>")
        );

        let m = &ty.members[4];
        assert_eq!(
            m.nullness_scope,
            NullnessScope::Unspecified(NullabilitySource::JSpecify)
        );
        assert_eq!(
            m.type_ref.as_deref(),
            Some("java.util.List<? extends String?>")
        );
        assert_eq!(m.params[0].type_ref, "String?");
        assert_eq!(
            m.params[0].decl_nullability,
            Some((
                Nullability::Nullable,
                NullabilitySource::ThirdPartyAnnotation
            ))
        );
        assert_eq!(m.params[1].type_ref, "String?[]");
        assert_eq!(m.params[1].decl_nullability, None);
    }
}
