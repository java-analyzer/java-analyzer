//! Pure, IO-free `.class` bytecode parsing via `ristretto_classfile`.
//!
//! All `ristretto_classfile` types remain strictly encapsulated inside this module.
//! See `docs/specs/0006-jdk-and-jar-classpath-indexing.md`.

use std::{borrow::Cow, collections::BTreeMap, error::Error, fmt};

use ristretto_classfile::{
    ClassAccessFlags, ClassFile, ConstantPool, FieldAccessFlags, MethodAccessFlags,
    attributes::{Attribute, NestedClassAccessFlags, RequiresFlags},
};
use smol_str::{SmolStr, format_smolstr};

use crate::{
    classpath::{BinaryMemberSymbol, BinaryModuleInfo, BinaryOrigin, BinaryTypeSymbol},
    symbols::{MemberKind, ParamSummary, TypeKind},
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
    if class_file.access_flags.contains(ClassAccessFlags::MODULE)
        || class_file
            .access_flags
            .contains(ClassAccessFlags::SYNTHETIC)
    {
        return Ok(None);
    }

    let internal_name = class_file
        .class_name()
        .map_err(ClassfileError::from_ristretto)?
        .to_rust_string();

    if internal_name == "package-info" || internal_name.ends_with("/package-info") {
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
    let is_static = is_static
        || (enclosing_type.is_some()
            && matches!(
                kind,
                TypeKind::Interface | TypeKind::Enum | TypeKind::Record | TypeKind::Annotation
            ));

    let type_params = find_signature(&class_file.attributes, cp)
        .and_then(|sig| parse_class_type_params(&sig))
        .unwrap_or_default();

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
            let type_ref = find_signature(&rec.attributes, cp)
                .and_then(|sig| decode_field_signature(&sig, &inner_map))
                .or_else(|| decode_field_descriptor(&desc, &inner_map));
            members.push(BinaryMemberSymbol {
                owner_type: qualified_name.clone(),
                name: SmolStr::new(name),
                kind: MemberKind::RecordComponent,
                is_public: true,
                is_static: false,
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
        let type_ref = find_signature(&field.attributes, cp)
            .and_then(|sig| decode_field_signature(&sig, &inner_map))
            .or_else(|| decode_field_descriptor(&desc, &inner_map));

        members.push(BinaryMemberSymbol {
            owner_type: qualified_name.clone(),
            name: SmolStr::new(name),
            kind: MemberKind::Field,
            is_public: is_field_public,
            is_static: field.access_flags.contains(FieldAccessFlags::STATIC),
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
        let (param_types, return_type) = find_signature(&method.attributes, cp)
            .and_then(|sig| decode_method_signature(&sig, &inner_map))
            .or_else(|| decode_method_descriptor(&desc, &inner_map))
            .unwrap_or_else(|| (Vec::new(), None));

        let is_varargs = method.access_flags.contains(MethodAccessFlags::VARARGS)
            && param_types.last().is_some_and(|t| t.ends_with("[]"));
        let param_names = extract_method_param_names(&method.attributes, cp, param_types.len())?;

        let last_idx = param_types.len().saturating_sub(1);
        let params = param_types
            .into_iter()
            .enumerate()
            .map(|(i, mut ty)| {
                let param_varargs = is_varargs && i == last_idx;
                if param_varargs && let Some(elem) = ty.strip_suffix("[]") {
                    ty = elem.to_string();
                }
                let pname = param_names
                    .get(i)
                    .and_then(|opt| opt.clone())
                    .unwrap_or_else(|| format_smolstr!("arg{i}"));
                ParamSummary {
                    name: Some(pname),
                    type_ref: ty,
                    is_varargs: param_varargs,
                }
            })
            .collect();

        members.push(BinaryMemberSymbol {
            owner_type: qualified_name.clone(),
            name: member_name,
            kind: member_kind,
            is_public: is_method_public,
            is_static: !is_ctor && method.access_flags.contains(MethodAccessFlags::STATIC),
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
        is_public,
        is_static,
        is_final,
        is_abstract,
        type_params,
        super_class,
        interfaces,
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
        if let Attribute::Module {
            module_name_index,
            exports: mod_exports,
            requires: mod_requires,
            ..
        } = attr
        {
            let name = cp
                .try_get_module(*module_name_index)
                .map_err(ClassfileError::from_ristretto)?
                .to_rust_string();

            let mut exports = Vec::new();
            for exp in mod_exports {
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
            for req in mod_requires {
                if req.flags.contains(RequiresFlags::TRANSITIVE) {
                    let dep_name = cp
                        .try_get_module(req.index)
                        .map_err(ClassfileError::from_ristretto)?
                        .to_rust_string();
                    requires_transitive.push(SmolStr::new(dep_name));
                }
            }

            return Ok(Some(BinaryModuleInfo {
                name: SmolStr::new(name),
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
            .filter(|p| !p.access_flags.contains(MethodAccessFlags::SYNTHETIC))
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

// ---- JVM Descriptor Decoding ----

fn decode_field_descriptor(desc: &str, inner_map: &InnerClassMap) -> Option<String> {
    let mut parser = DescriptorParser::new(desc, inner_map);
    let ty = parser.parse_field_type(0)?;
    parser.is_eof().then_some(ty)
}

fn decode_method_descriptor(
    desc: &str,
    inner_map: &InnerClassMap,
) -> Option<(Vec<String>, Option<String>)> {
    let mut parser = DescriptorParser::new(desc, inner_map);
    parser.expect_byte(b'(')?;
    let mut params = Vec::new();
    while parser.peek_byte() != Some(b')') {
        params.push(parser.parse_field_type(0)?);
    }
    parser.expect_byte(b')')?;
    let ret = if parser.peek_byte() == Some(b'V') {
        parser.advance(1);
        Some("void".to_string())
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

    fn parse_field_type(&mut self, depth: usize) -> Option<String> {
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
        let base = match self.peek_byte()? {
            b'B' => {
                self.pos += 1;
                "byte".to_string()
            }
            b'C' => {
                self.pos += 1;
                "char".to_string()
            }
            b'D' => {
                self.pos += 1;
                "double".to_string()
            }
            b'F' => {
                self.pos += 1;
                "float".to_string()
            }
            b'I' => {
                self.pos += 1;
                "int".to_string()
            }
            b'J' => {
                self.pos += 1;
                "long".to_string()
            }
            b'S' => {
                self.pos += 1;
                "short".to_string()
            }
            b'Z' => {
                self.pos += 1;
                "boolean".to_string()
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
                internal_to_java_type(internal, self.inner_map)
            }
            _ => return None,
        };

        if dims == 0 {
            Some(base)
        } else {
            let mut out = String::with_capacity(base.len() + dims * 2);
            out.push_str(&base);
            for _ in 0..dims {
                out.push_str("[]");
            }
            Some(out)
        }
    }
}

// ---- JVM Generic Signature Decoding (JVMS §4.7.9.1) ----

fn parse_class_type_params(sig: &str) -> Option<Vec<SmolStr>> {
    let empty = InnerClassMap::new();
    let mut parser = SignatureParser::new(sig, &empty);
    if parser.peek_byte() != Some(b'<') {
        return Some(Vec::new());
    }
    let params = parser.parse_type_parameters(0)?;
    // Validate that at least one superclass signature follows `<...>`.
    let _super_sig = parser.parse_class_type_sig(0)?;
    Some(params)
}

fn decode_field_signature(sig: &str, inner_map: &InnerClassMap) -> Option<String> {
    let mut parser = SignatureParser::new(sig, inner_map);
    let ty = parser.parse_ref_type_sig(0)?;
    parser.is_eof().then_some(ty)
}

fn decode_method_signature(
    sig: &str,
    inner_map: &InnerClassMap,
) -> Option<(Vec<String>, Option<String>)> {
    let mut parser = SignatureParser::new(sig, inner_map);
    if parser.peek_byte() == Some(b'<') {
        let _ = parser.parse_type_parameters(0)?;
    }
    parser.expect_byte(b'(')?;
    let mut params = Vec::new();
    while parser.peek_byte() != Some(b')') {
        params.push(parser.parse_java_type_sig(0)?);
    }
    parser.expect_byte(b')')?;
    let ret = if parser.peek_byte() == Some(b'V') {
        parser.advance(1);
        Some("void".to_string())
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
    parser.is_eof().then_some((params, ret))
}

struct SignatureParser<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    inner_map: &'a InnerClassMap,
}

impl<'a> SignatureParser<'a> {
    fn new(src: &'a str, inner_map: &'a InnerClassMap) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
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

    fn parse_type_parameters(&mut self, depth: usize) -> Option<Vec<SmolStr>> {
        if depth >= MAX_SIGNATURE_DEPTH {
            return None;
        }
        self.expect_byte(b'<')?;
        let mut names = Vec::new();
        while self.peek_byte() != Some(b'>') {
            let name = self.read_identifier_until(b":")?;
            names.push(SmolStr::new(name));
            self.expect_byte(b':')?;
            // Optional ClassBound (may be empty if immediately followed by `:`).
            if matches!(self.peek_byte(), Some(b'L' | b'T' | b'[')) {
                let _ = self.parse_ref_type_sig(depth + 1)?;
            }
            // Zero or more InterfaceBounds (`:ReferenceTypeSignature`).
            while self.peek_byte() == Some(b':') {
                self.pos += 1;
                let _ = self.parse_ref_type_sig(depth + 1)?;
            }
        }
        self.expect_byte(b'>')?;
        (!names.is_empty()).then_some(names)
    }

    fn parse_java_type_sig(&mut self, depth: usize) -> Option<String> {
        if depth >= MAX_SIGNATURE_DEPTH {
            return None;
        }
        match self.peek_byte()? {
            b'B' => {
                self.pos += 1;
                Some("byte".to_string())
            }
            b'C' => {
                self.pos += 1;
                Some("char".to_string())
            }
            b'D' => {
                self.pos += 1;
                Some("double".to_string())
            }
            b'F' => {
                self.pos += 1;
                Some("float".to_string())
            }
            b'I' => {
                self.pos += 1;
                Some("int".to_string())
            }
            b'J' => {
                self.pos += 1;
                Some("long".to_string())
            }
            b'S' => {
                self.pos += 1;
                Some("short".to_string())
            }
            b'Z' => {
                self.pos += 1;
                Some("boolean".to_string())
            }
            _ => self.parse_ref_type_sig(depth + 1),
        }
    }

    fn parse_ref_type_sig(&mut self, depth: usize) -> Option<String> {
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
                let elem = self.parse_java_type_sig(depth + 1)?;
                let mut out = String::with_capacity(elem.len() + dims * 2);
                out.push_str(&elem);
                for _ in 0..dims {
                    out.push_str("[]");
                }
                Some(out)
            }
            _ => None,
        }
    }

    fn parse_type_var_sig(&mut self) -> Option<String> {
        self.expect_byte(b'T')?;
        let name = self.read_identifier_until(b";")?;
        self.expect_byte(b';')?;
        Some(name.to_string())
    }

    fn parse_class_type_sig(&mut self, depth: usize) -> Option<String> {
        if depth >= MAX_SIGNATURE_DEPTH {
            return None;
        }
        self.expect_byte(b'L')?;
        let internal_head = self.read_identifier_until(b"<.;")?;
        let mut out = internal_to_java_type(internal_head, self.inner_map);
        if self.peek_byte() == Some(b'<') {
            let args = self.parse_type_args(depth + 1)?;
            out.push_str(&args);
        }
        while self.peek_byte() == Some(b'.') {
            self.pos += 1;
            let suffix = self.read_identifier_until(b"<.;")?;
            out.push('.');
            out.push_str(suffix);
            if self.peek_byte() == Some(b'<') {
                let args = self.parse_type_args(depth + 1)?;
                out.push_str(&args);
            }
        }
        self.expect_byte(b';')?;
        Some(out)
    }

    fn parse_type_args(&mut self, depth: usize) -> Option<String> {
        if depth >= MAX_SIGNATURE_DEPTH {
            return None;
        }
        self.expect_byte(b'<')?;
        let mut args = Vec::new();
        while self.peek_byte() != Some(b'>') {
            let arg = match self.peek_byte()? {
                b'*' => {
                    self.pos += 1;
                    "?".to_string()
                }
                b'+' => {
                    self.pos += 1;
                    let bound = self.parse_ref_type_sig(depth + 1)?;
                    format!("? extends {bound}")
                }
                b'-' => {
                    self.pos += 1;
                    let bound = self.parse_ref_type_sig(depth + 1)?;
                    format!("? super {bound}")
                }
                _ => self.parse_ref_type_sig(depth + 1)?,
            };
            args.push(arg);
        }
        self.expect_byte(b'>')?;
        if args.is_empty() {
            return None;
        }
        Some(format!("<{}>", args.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use ristretto_classfile::{
        BaseType, ClassAccessFlags, ClassFile, ConstantPool, Field, FieldAccessFlags, FieldType,
        JAVA_17, JAVA_21, JAVA_25, Method, MethodAccessFlags, Version,
        attributes::{
            Attribute, Exports, ExportsFlags, InnerClass, MethodParameter, ModuleAccessFlags,
            NestedClassAccessFlags, Record as RecordAttr, Requires, RequiresFlags,
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
                                    access_flags: MethodAccessFlags::FINAL,
                                },
                                MethodParameter {
                                    name_index: p1_name,
                                    access_flags: MethodAccessFlags::empty(),
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
}
