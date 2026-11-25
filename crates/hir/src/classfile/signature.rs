//! JVM descriptor and generic signature decoding (JVMS §4.3, §4.7.9.1).

use std::collections::BTreeMap;

use smol_str::SmolStr;

use crate::ty::Nullability;

use super::{InnerClassMap, MAX_SIGNATURE_DEPTH, internal_to_fqn, internal_to_java_type};

type RawTypeParamsWithBounds = (Vec<SmolStr>, Vec<(SmolStr, bool, Vec<DecodedType>)>);

#[derive(Debug, Clone)]
pub(super) enum DecodedType {
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
pub(super) struct ClassSegment {
    pub(super) name: String,
    pub(super) args: Vec<DecodedType>,
    pub(super) nullability: Option<Nullability>,
}

#[derive(Debug, Clone)]
pub(super) enum DecodedWildcard {
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
    pub(super) fn is_array(&self) -> bool {
        matches!(self, Self::Array { .. })
    }

    pub(super) fn is_void(&self) -> bool {
        matches!(self, Self::Primitive("void"))
    }

    pub(super) fn render(&self) -> String {
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

pub(super) fn render_type_param_bounds(
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

// ---- JVM Descriptor Decoding ----

pub(super) fn decode_field_descriptor(desc: &str, inner_map: &InnerClassMap) -> Option<DecodedType> {
    let mut parser = DescriptorParser::new(desc, inner_map);
    let ty = parser.parse_field_type(0)?;
    parser.is_eof().then_some(ty)
}

pub(super) fn decode_method_descriptor(
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

pub(super) struct DecodedClassParts {
    pub(super) type_params: Vec<SmolStr>,
    pub(super) type_param_bounds: BTreeMap<SmolStr, Vec<String>>,
    pub(super) super_class_generic: Option<String>,
    pub(super) interfaces_generic: Vec<String>,
}

pub(super) struct DecodedClassSignature {
    pub(super) type_params: Vec<SmolStr>,
    pub(super) type_param_bounds: Vec<(SmolStr, bool, Vec<DecodedType>)>,
    pub(super) super_class_generic: Option<DecodedType>,
    pub(super) interfaces_generic: Vec<DecodedType>,
}

impl DecodedClassSignature {
    pub(super) fn into_parts(self) -> DecodedClassParts {
        let type_param_bounds = render_type_param_bounds(&self.type_param_bounds);
        let super_class_generic = self.super_class_generic.as_ref().map(DecodedType::render);
        let interfaces_generic = self
            .interfaces_generic
            .iter()
            .map(DecodedType::render)
            .collect();
        DecodedClassParts {
            type_params: self.type_params,
            type_param_bounds,
            super_class_generic,
            interfaces_generic,
        }
    }
}

pub(super) struct DecodedMethodSignature {
    pub(super) type_params: Vec<SmolStr>,
    pub(super) type_param_bounds: Vec<(SmolStr, bool, Vec<DecodedType>)>,
    pub(super) params: Vec<DecodedType>,
    pub(super) return_type: Option<DecodedType>,
}

pub(super) fn decode_class_signature(
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

pub(super) fn decode_field_signature(sig: &str, inner_map: &InnerClassMap) -> Option<DecodedType> {
    let mut parser = SignatureParser::new(sig, inner_map, true);
    let ty = parser.parse_ref_type_sig(0)?;
    parser.is_eof().then_some(ty)
}

pub(super) fn decode_method_signature(
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
