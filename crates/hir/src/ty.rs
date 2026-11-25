//! Structured Java type IR, generic substitution, subtyping, and LUB.
//! See `docs/specs/0008-expression-type-inference-and-generics.md` (Section 2).

use std::collections::{BTreeMap, VecDeque};

use rustc_hash::FxHashSet;
use smol_str::SmolStr;

use crate::{
    DefDatabase, FileId,
    classpath::{BinaryOrigin, BinaryTypeSymbol, ClasspathIndex},
    hierarchy::linearized_supertypes,
    item_tree::{FileItemTree, TypeItem},
    symbols::{ImportKind, NullnessScope, TypeIdentity, TypeKind, WorkspaceIndex},
};

/// Maximum recursion depth for type substitution, subtyping, and type-variable resolution.
pub const MAX_SUBST_DEPTH: usize = 32;

const MAX_PARSE_DEPTH: usize = 64;

const OBJECT_FQN: &str = "java.lang.Object";
const STRING_FQN: &str = "java.lang.String";
const ENUM_FQN: &str = "java.lang.Enum";
const RECORD_FQN: &str = "java.lang.Record";
const CLONEABLE_FQN: &str = "java.lang.Cloneable";
const SERIALIZABLE_FQN: &str = "java.io.Serializable";
const ANNOTATION_FQN: &str = "java.lang.annotation.Annotation";

/// Java primitive types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    Boolean,
    Byte,
    Short,
    Int,
    Long,
    Char,
    Float,
    Double,
}

impl PrimitiveType {
    /// Returns the canonical FQN of the corresponding boxed wrapper class (e.g. `"java.lang.Integer"`).
    #[must_use]
    pub const fn boxed_fqn(&self) -> &'static str {
        match self {
            Self::Boolean => "java.lang.Boolean",
            Self::Byte => "java.lang.Byte",
            Self::Short => "java.lang.Short",
            Self::Int => "java.lang.Integer",
            Self::Long => "java.lang.Long",
            Self::Char => "java.lang.Character",
            Self::Float => "java.lang.Float",
            Self::Double => "java.lang.Double",
        }
    }

    /// Returns the [`PrimitiveType`] corresponding to a boxed wrapper FQN, if any.
    #[must_use]
    pub fn from_boxed_fqn(fqn: &str) -> Option<Self> {
        match fqn {
            "java.lang.Boolean" => Some(Self::Boolean),
            "java.lang.Byte" => Some(Self::Byte),
            "java.lang.Short" => Some(Self::Short),
            "java.lang.Integer" => Some(Self::Int),
            "java.lang.Long" => Some(Self::Long),
            "java.lang.Character" => Some(Self::Char),
            "java.lang.Float" => Some(Self::Float),
            "java.lang.Double" => Some(Self::Double),
            _ => None,
        }
    }

    /// Returns `true` if `self` is an integral primitive type (`byte`, `short`, `int`, `long`, `char`).
    #[must_use]
    pub const fn is_integral(self) -> bool {
        matches!(
            self,
            Self::Byte | Self::Short | Self::Int | Self::Long | Self::Char
        )
    }

    /// Returns `true` if `self` is a numeric primitive type (every primitive except `boolean`).
    #[must_use]
    pub const fn is_numeric(self) -> bool {
        !matches!(self, Self::Boolean)
    }

    /// Returns `true` if `self` strictly widens to `other` per JLS §5.1.2 (`self != other`).
    #[must_use]
    pub const fn widens_to(self, other: Self) -> bool {
        matches!(
            (self, other),
            (
                Self::Byte,
                Self::Short | Self::Int | Self::Long | Self::Float | Self::Double
            ) | (
                Self::Short | Self::Char,
                Self::Int | Self::Long | Self::Float | Self::Double
            ) | (Self::Int, Self::Long | Self::Float | Self::Double)
                | (Self::Long, Self::Float | Self::Double)
                | (Self::Float, Self::Double)
        )
    }

    /// Returns the Java source keyword for this primitive type.
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Byte => "byte",
            Self::Short => "short",
            Self::Int => "int",
            Self::Long => "long",
            Self::Char => "char",
            Self::Float => "float",
            Self::Double => "double",
        }
    }
}

/// Bound on a wildcard type argument (`?`, `? extends T`, `? super T`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WildcardBound {
    Unbounded,
    Extends(Box<Ty>),
    Super(Box<Ty>),
}

/// A type argument inside a parameterized [`Ty::Class`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeArg {
    Type(Ty),
    Wildcard(WildcardBound),
}

impl TypeArg {
    /// Formats this type argument with short simple class names (e.g. `? extends Number`).
    #[must_use]
    pub fn display_short(&self) -> String {
        match self {
            Self::Type(ty) => ty.display_short(),
            Self::Wildcard(WildcardBound::Unbounded) => "?".to_string(),
            Self::Wildcard(WildcardBound::Extends(ty)) => {
                format!("? extends {}", ty.display_short())
            }
            Self::Wildcard(WildcardBound::Super(ty)) => {
                format!("? super {}", ty.display_short())
            }
        }
    }

    /// Formats this type argument with fully qualified class names.
    #[must_use]
    pub fn display_qualified(&self) -> String {
        match self {
            Self::Type(ty) => ty.display_qualified(),
            Self::Wildcard(WildcardBound::Unbounded) => "?".to_string(),
            Self::Wildcard(WildcardBound::Extends(ty)) => {
                format!("? extends {}", ty.display_qualified())
            }
            Self::Wildcard(WildcardBound::Super(ty)) => {
                format!("? super {}", ty.display_qualified())
            }
        }
    }
}

/// Use-site nullability marker (supports Valhalla `T!`/`T?` and JSpecify `@NonNull`/`@Nullable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Nullability {
    /// Legacy / unspecified nullability (nullable at runtime, no explicit `!`/`?`).
    #[default]
    Unspecified,
    /// Null-restricted / non-null type (`Point!`, `@NonNull String`).
    NonNull,
    /// Explicitly nullable type (`Point?`, `@Nullable String`).
    Nullable,
}

impl Nullability {
    #[must_use]
    const fn suffix(self) -> &'static str {
        match self {
            Self::Unspecified => "",
            Self::NonNull => "!",
            Self::Nullable => "?",
        }
    }
}

/// Structured, cycle-safe Java type representation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    Primitive(PrimitiveType),
    Void,
    Null,
    Class {
        fqn: String,
        type_args: Vec<TypeArg>,
        nullability: Nullability,
    },
    Array {
        elem: Box<Ty>,
        nullability: Nullability,
    },
    TypeVar {
        name: SmolStr,
        bounds: Vec<Ty>,
        nullability: Nullability,
    },
    Intersection(Vec<Ty>),
    Unknown,
}

impl Ty {
    /// Constructs a parameterized class type with [`Nullability::Unspecified`].
    #[must_use]
    pub fn class(fqn: impl Into<String>, type_args: Vec<TypeArg>) -> Self {
        Self::Class {
            fqn: fqn.into(),
            type_args,
            nullability: Nullability::Unspecified,
        }
    }

    /// Constructs an unparameterized/raw class type with [`Nullability::Unspecified`].
    #[must_use]
    pub fn raw_class(fqn: impl Into<String>) -> Self {
        Self::class(fqn, Vec::new())
    }

    /// Constructs an array type with [`Nullability::Unspecified`].
    #[must_use]
    pub fn array(elem: Self) -> Self {
        Self::Array {
            elem: Box::new(elem),
            nullability: Nullability::Unspecified,
        }
    }

    /// Constructs a type variable with [`Nullability::Unspecified`].
    #[must_use]
    pub fn type_var(name: impl Into<SmolStr>, bounds: Vec<Self>) -> Self {
        Self::TypeVar {
            name: name.into(),
            bounds,
            nullability: Nullability::Unspecified,
        }
    }

    /// Returns `self` with its use-site nullability updated (no-op on `Primitive`, `Void`,
    /// `Null`, `Intersection`, and `Unknown`).
    #[must_use]
    pub fn with_nullability(self, nullability: Nullability) -> Self {
        match self {
            Self::Class { fqn, type_args, .. } => Self::Class {
                fqn,
                type_args,
                nullability,
            },
            Self::Array { elem, .. } => Self::Array { elem, nullability },
            Self::TypeVar { name, bounds, .. } => Self::TypeVar {
                name,
                bounds,
                nullability,
            },
            other => other,
        }
    }

    /// Returns `java.lang.Object`.
    #[must_use]
    pub fn object() -> Self {
        Self::raw_class(OBJECT_FQN)
    }

    /// Returns `java.lang.String`.
    #[must_use]
    pub fn string() -> Self {
        Self::raw_class(STRING_FQN)
    }

    /// Returns `true` if `self` is one of the 8 built-in Java primitive types.
    #[must_use]
    pub const fn is_primitive(&self) -> bool {
        matches!(self, Self::Primitive(_))
    }

    /// Returns the effective use-site [`Nullability`] of this type.
    #[must_use]
    pub const fn nullability(&self) -> Nullability {
        match self {
            Self::Primitive(_) => Nullability::NonNull,
            Self::Null => Nullability::Nullable,
            Self::Class { nullability, .. }
            | Self::Array { nullability, .. }
            | Self::TypeVar { nullability, .. } => *nullability,
            Self::Void | Self::Intersection(_) | Self::Unknown => Nullability::Unspecified,
        }
    }

    /// Returns `true` if values of this type may be `null` (not `NonNull` and not `void`).
    #[must_use]
    pub const fn is_nullable(&self) -> bool {
        !matches!(self.nullability(), Nullability::NonNull) && !matches!(self, Self::Void)
    }

    /// Returns `true` if `self` is a primitive or a declared Project Valhalla value class/record.
    #[must_use]
    pub fn is_value_type(&self, db: &dyn DefDatabase) -> bool {
        match self {
            Self::Primitive(_) => true,
            Self::Class { fqn, .. } => {
                if let Some((fid, tid)) = db.workspace_index().get_type(fqn) {
                    return db.file_item_tree(fid).types[tid].identity == TypeIdentity::Value;
                }
                if let Some(bin) = db.classpath_index().type_by_fqn(fqn) {
                    return bin.identity == TypeIdentity::Value;
                }
                false
            }
            _ => false,
        }
    }

    /// Boxes a primitive type to its non-null wrapper class (`int` -> `java.lang.Integer!`);
    /// returns other types unchanged.
    #[must_use]
    pub fn boxed(&self) -> Self {
        match self {
            Self::Primitive(p) => Self::raw_class(p.boxed_fqn()).with_nullability(Nullability::NonNull),
            other => other.clone(),
        }
    }

    /// Returns the erased canonical FQN for `Ty::Class`, or the first class bound for
    /// `Ty::TypeVar` / `Ty::Intersection`, or `None` otherwise.
    #[must_use]
    pub fn erased_fqn(&self) -> Option<&str> {
        self.erased_fqn_inner(0)
    }

    fn erased_fqn_inner(&self, depth: usize) -> Option<&str> {
        if depth >= MAX_SUBST_DEPTH {
            return None;
        }
        match self {
            Self::Class { fqn, .. } => Some(fqn.as_str()),
            Self::TypeVar { bounds, .. } => {
                bounds.iter().find_map(|b| b.erased_fqn_inner(depth + 1))
            }
            Self::Intersection(members) => {
                members.iter().find_map(|m| m.erased_fqn_inner(depth + 1))
            }
            _ => None,
        }
    }

    /// Formats this type concisely with short class names (e.g. `List<User>`, `int[]`).
    #[must_use]
    pub fn display_short(&self) -> String {
        self.display_inner(false, 0)
    }

    /// Formats this type with fully qualified class names (e.g. `java.util.List<com.example.User>`).
    #[must_use]
    pub fn display_qualified(&self) -> String {
        self.display_inner(true, 0)
    }

    fn display_inner(&self, qualified: bool, depth: usize) -> String {
        if depth >= MAX_SUBST_DEPTH {
            return "...".to_string();
        }
        match self {
            Self::Primitive(p) => p.keyword().to_string(),
            Self::Void => "void".to_string(),
            Self::Null => "null".to_string(),
            Self::Class {
                fqn,
                type_args,
                nullability,
            } => {
                let base = if qualified {
                    fqn.as_str()
                } else {
                    short_fqn_name(fqn)
                };
                let suffix = nullability.suffix();
                if type_args.is_empty() {
                    if suffix.is_empty() {
                        base.to_string()
                    } else {
                        format!("{base}{suffix}")
                    }
                } else {
                    let args: Vec<String> = type_args
                        .iter()
                        .map(|a| {
                            if qualified {
                                a.display_qualified()
                            } else {
                                a.display_short()
                            }
                        })
                        .collect();
                    format!("{base}<{}>{suffix}", args.join(", "))
                }
            }
            Self::Array { elem, nullability } => format!(
                "{}[]{}",
                elem.display_inner(qualified, depth + 1),
                nullability.suffix()
            ),
            Self::TypeVar {
                name, nullability, ..
            } => format!("{name}{}", nullability.suffix()),
            Self::Intersection(members) => {
                let parts: Vec<String> = members
                    .iter()
                    .map(|m| m.display_inner(qualified, depth + 1))
                    .collect();
                parts.join(" & ")
            }
            Self::Unknown => "<unknown>".to_string(),
        }
    }

    fn is_reference(&self) -> bool {
        matches!(
            self,
            Self::Null
                | Self::Class { .. }
                | Self::Array { .. }
                | Self::TypeVar { .. }
                | Self::Intersection(_)
        )
    }
}

fn short_fqn_name(fqn: &str) -> &str {
    // Strip leading lowercase package segments (so `java.util.Map.Entry` -> `Map.Entry`,
    // `java.util.List` -> `List`, and `com.example.foo` falls back to `foo`).
    let mut last_pkg_dot: Option<usize> = None;
    let mut seg_start = 0usize;
    for (idx, ch) in fqn.char_indices() {
        if ch == '.' {
            if let Some(first) = fqn.get(seg_start..idx).and_then(|s| s.chars().next())
                && first.is_ascii_lowercase()
            {
                last_pkg_dot = Some(idx);
            }
            seg_start = idx + 1;
        }
    }
    if let Some(dot_idx) = last_pkg_dot
        && let Some(rest) = fqn.get(dot_idx + 1..)
        && !rest.is_empty()
    {
        return rest;
    }
    fqn
}

/// Deterministic type-parameter substitution map (`E -> TypeArg`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Substitution {
    pub map: BTreeMap<SmolStr, TypeArg>,
}

impl Substitution {
    /// Inserts a concrete type mapping `name -> TypeArg::Type(ty)`.
    pub fn insert_ty(&mut self, name: SmolStr, ty: Ty) {
        self.map.insert(name, TypeArg::Type(ty));
    }

    /// Builds a [`Substitution`] by zipping `params` with `args`.
    #[must_use]
    pub fn from_params_and_args(params: &[SmolStr], args: &[TypeArg]) -> Self {
        let map = params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        Self { map }
    }

    /// Substitutes type variables in `ty`, bounded by [`MAX_SUBST_DEPTH`].
    ///
    /// Type arguments inside parameterized classes retain wildcards; a top-level
    /// `TypeVar` mapped to a wildcard projects to its upper bound (`Object` for `?` / `? super`).
    #[must_use]
    pub fn apply(&self, ty: &Ty) -> Ty {
        if self.map.is_empty() {
            return ty.clone();
        }
        self.apply_inner(ty, 0, false)
    }

    /// Substitutes type variables in a covariant position (member return/field read),
    /// projecting wildcards: `? extends U` -> `U`, `? super U` / `?` -> `java.lang.Object`.
    #[must_use]
    pub fn apply_covariant(&self, ty: &Ty) -> Ty {
        if self.map.is_empty() {
            return ty.clone();
        }
        self.apply_inner(ty, 0, false)
    }

    /// Substitutes type variables in a contravariant position (functional interface SAM parameters),
    /// projecting wildcards: `? super L` -> `L`, `? extends U` -> `U`, `?` -> `java.lang.Object`.
    #[must_use]
    pub fn apply_contravariant(&self, ty: &Ty) -> Ty {
        if self.map.is_empty() {
            return ty.clone();
        }
        self.apply_inner(ty, 0, true)
    }

    /// Substitutes type variables in a [`TypeArg`], preserving wildcards.
    #[must_use]
    pub fn apply_arg(&self, arg: &TypeArg) -> TypeArg {
        if self.map.is_empty() {
            return arg.clone();
        }
        self.apply_arg_inner(arg, 0)
    }

    /// Composes `self` followed by `outer`, such that `(self.compose(outer)).apply(t)`
    /// matches `outer.apply(&self.apply(t))`.
    #[must_use]
    pub fn compose(&self, outer: &Self) -> Self {
        let mut map = BTreeMap::new();
        for (name, arg) in &self.map {
            map.insert(name.clone(), outer.apply_arg(arg));
        }
        for (name, arg) in &outer.map {
            map.entry(name.clone()).or_insert_with(|| arg.clone());
        }
        Self { map }
    }

    fn apply_inner(&self, ty: &Ty, depth: usize, contravariant: bool) -> Ty {
        if depth >= MAX_SUBST_DEPTH {
            return ty.clone();
        }
        match ty {
            Ty::TypeVar {
                name,
                bounds,
                nullability,
            } => {
                if let Some(arg) = self.map.get(name) {
                    let substituted = match arg {
                        TypeArg::Type(replacement) => replacement.clone(),
                        TypeArg::Wildcard(WildcardBound::Extends(upper)) => (**upper).clone(),
                        TypeArg::Wildcard(WildcardBound::Super(lower)) => {
                            if contravariant {
                                (**lower).clone()
                            } else {
                                tvar_upper_bound(bounds)
                            }
                        }
                        TypeArg::Wildcard(WildcardBound::Unbounded) => tvar_upper_bound(bounds),
                    };
                    apply_tvar_nullability(substituted, *nullability)
                } else {
                    let subst_bounds = bounds
                        .iter()
                        .map(|b| self.apply_inner(b, depth + 1, contravariant))
                        .collect();
                    Ty::TypeVar {
                        name: name.clone(),
                        bounds: subst_bounds,
                        nullability: *nullability,
                    }
                }
            }
            Ty::Class {
                fqn,
                type_args,
                nullability,
            } => Ty::Class {
                fqn: fqn.clone(),
                type_args: type_args
                    .iter()
                    .map(|a| self.apply_arg_inner(a, depth + 1))
                    .collect(),
                nullability: *nullability,
            },
            Ty::Array { elem, nullability } => Ty::Array {
                elem: Box::new(self.apply_inner(elem, depth + 1, contravariant)),
                nullability: *nullability,
            },
            Ty::Intersection(members) => Ty::Intersection(
                members
                    .iter()
                    .map(|m| self.apply_inner(m, depth + 1, contravariant))
                    .collect(),
            ),
            Ty::Primitive(_) | Ty::Void | Ty::Null | Ty::Unknown => ty.clone(),
        }
    }

    fn apply_arg_inner(&self, arg: &TypeArg, depth: usize) -> TypeArg {
        if depth >= MAX_SUBST_DEPTH {
            return arg.clone();
        }
        match arg {
            TypeArg::Type(ty @ Ty::TypeVar {
                name, nullability, ..
            }) => {
                if let Some(mapped) = self.map.get(name) {
                    apply_tvar_nullability_to_arg(mapped.clone(), *nullability)
                } else {
                    TypeArg::Type(self.apply_inner(ty, depth + 1, false))
                }
            }
            TypeArg::Type(ty) => TypeArg::Type(self.apply_inner(ty, depth + 1, false)),
            TypeArg::Wildcard(WildcardBound::Unbounded) => {
                TypeArg::Wildcard(WildcardBound::Unbounded)
            }
            TypeArg::Wildcard(WildcardBound::Extends(ty)) => TypeArg::Wildcard(
                WildcardBound::Extends(Box::new(self.apply_inner(ty, depth + 1, false))),
            ),
            TypeArg::Wildcard(WildcardBound::Super(ty)) => TypeArg::Wildcard(
                WildcardBound::Super(Box::new(self.apply_inner(ty, depth + 1, false))),
            ),
        }
    }
}

fn tvar_upper_bound(bounds: &[Ty]) -> Ty {
    match bounds {
        [] => Ty::object(),
        [single] => single.clone(),
        many => Ty::Intersection(many.to_vec()),
    }
}

fn apply_tvar_nullability(ty: Ty, tvar_nullability: Nullability) -> Ty {
    match tvar_nullability {
        Nullability::Unspecified => ty,
        n @ (Nullability::NonNull | Nullability::Nullable) => ty.with_nullability(n),
    }
}

fn apply_tvar_nullability_to_arg(arg: TypeArg, tvar_nullability: Nullability) -> TypeArg {
    match (arg, tvar_nullability) {
        (other, Nullability::Unspecified) => other,
        (TypeArg::Type(ty), n) => TypeArg::Type(ty.with_nullability(n)),
        (TypeArg::Wildcard(WildcardBound::Extends(upper)), n) => {
            TypeArg::Wildcard(WildcardBound::Extends(Box::new(upper.with_nullability(n))))
        }
        (other, _) => other,
    }
}

// ---- Parsing ----

/// Parses a raw Java type reference string into a [`Ty`], resolving non-primitive named types
/// through `resolve_type`.
pub fn parse_type_ref(raw: &str, resolve_type: &mut impl FnMut(&[&str]) -> Option<Ty>) -> Ty {
    let mut parser = TypeParser::new(raw);
    parser.parse_type(resolve_type, 0)
}

struct TypeParser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> TypeParser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn starts_with(&self, pat: &str) -> bool {
        self.rest().starts_with(pat)
    }

    fn advance(&mut self, bytes: usize) {
        self.pos = (self.pos + bytes).min(self.src.len());
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.rest().chars().next() {
            if ch.is_whitespace() {
                self.advance(ch.len_utf8());
            } else {
                break;
            }
        }
    }

    fn skip_ws_and_annotations(&mut self) {
        loop {
            self.skip_ws();
            if self.peek_byte() != Some(b'@') {
                break;
            }
            self.advance(1);
            self.skip_ws();
            // Skip qualified annotation name (`@Nullable`, `@com.example.Ann`).
            loop {
                let seg = self.read_ident();
                if seg.is_empty() {
                    break;
                }
                self.skip_ws();
                if self.peek_byte() == Some(b'.') {
                    self.advance(1);
                    self.skip_ws();
                } else {
                    break;
                }
            }
            self.skip_ws();
            if self.peek_byte() == Some(b'(') {
                self.skip_balanced_parens();
            }
        }
    }

    fn skip_balanced_parens(&mut self) {
        if self.peek_byte() != Some(b'(') {
            return;
        }
        let mut depth = 0usize;
        let mut in_string = false;
        let mut in_char = false;
        let mut escaped = false;
        while let Some(ch) = self.rest().chars().next() {
            self.advance(ch.len_utf8());
            if escaped {
                escaped = false;
                continue;
            }
            if in_string {
                if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            if in_char {
                if ch == '\\' {
                    escaped = true;
                } else if ch == '\'' {
                    in_char = false;
                }
                continue;
            }
            match ch {
                '"' => in_string = true,
                '\'' => in_char = true,
                '(' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    fn read_ident(&mut self) -> &'a str {
        let start = self.pos;
        for ch in self.rest().chars() {
            if ch.is_alphanumeric() || ch == '_' || ch == '$' {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        self.src.get(start..self.pos).unwrap_or("")
    }

    fn consume_nullability(&mut self) -> Option<Nullability> {
        match self.peek_byte() {
            Some(b'!') => {
                self.advance(1);
                Some(Nullability::NonNull)
            }
            Some(b'?') => {
                self.advance(1);
                Some(Nullability::Nullable)
            }
            _ => None,
        }
    }

    fn consume_keyword(&mut self, kw: &str) -> bool {
        let rest = self.rest();
        if let Some(after) = rest.strip_prefix(kw) {
            let boundary_ok = after
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '$');
            if boundary_ok {
                self.advance(kw.len());
                return true;
            }
        }
        false
    }

    fn parse_type(
        &mut self,
        resolve_type: &mut impl FnMut(&[&str]) -> Option<Ty>,
        depth: usize,
    ) -> Ty {
        if depth >= MAX_PARSE_DEPTH {
            return Ty::Unknown;
        }
        let first = self.parse_single_type(resolve_type, depth);
        self.skip_ws_and_annotations();
        if self.peek_byte() != Some(b'&') {
            return first;
        }
        let mut members = vec![first];
        while self.peek_byte() == Some(b'&') {
            self.advance(1);
            members.push(self.parse_single_type(resolve_type, depth + 1));
            self.skip_ws_and_annotations();
        }
        Ty::Intersection(members)
    }

    fn parse_single_type(
        &mut self,
        resolve_type: &mut impl FnMut(&[&str]) -> Option<Ty>,
        depth: usize,
    ) -> Ty {
        if depth >= MAX_PARSE_DEPTH {
            return Ty::Unknown;
        }
        self.skip_ws_and_annotations();
        if self.is_eof() || self.peek_byte() == Some(b'?') {
            return Ty::Unknown;
        }

        let mut segments: Vec<&'a str> = Vec::new();
        let mut final_args: Option<Vec<TypeArg>> = None;

        loop {
            self.skip_ws_and_annotations();
            let ident = self.read_ident();
            if ident.is_empty() {
                break;
            }
            segments.push(ident);

            self.skip_ws_and_annotations();
            if self.peek_byte() == Some(b'<') {
                final_args = Some(self.parse_type_args(resolve_type, depth + 1));
            } else {
                final_args = None;
            }

            self.skip_ws_and_annotations();
            if self.starts_with(".") && !self.starts_with("...") {
                self.advance(1);
            } else {
                break;
            }
        }

        if segments.is_empty() {
            return Ty::Unknown;
        }

        let mut base = if segments.len() == 1 && final_args.is_none() {
            match segments.first().copied() {
                Some("boolean") => Ty::Primitive(PrimitiveType::Boolean),
                Some("byte") => Ty::Primitive(PrimitiveType::Byte),
                Some("short") => Ty::Primitive(PrimitiveType::Short),
                Some("int") => Ty::Primitive(PrimitiveType::Int),
                Some("long") => Ty::Primitive(PrimitiveType::Long),
                Some("char") => Ty::Primitive(PrimitiveType::Char),
                Some("float") => Ty::Primitive(PrimitiveType::Float),
                Some("double") => Ty::Primitive(PrimitiveType::Double),
                Some("void") => Ty::Void,
                Some("var") => Ty::Unknown,
                _ => resolve_type(&segments).unwrap_or(Ty::Unknown),
            }
        } else {
            match resolve_type(&segments) {
                Some(Ty::Class {
                    fqn,
                    type_args,
                    nullability,
                }) => Ty::Class {
                    fqn,
                    type_args: final_args.unwrap_or(type_args),
                    nullability,
                },
                Some(other) => other,
                None => Ty::Unknown,
            }
        };

        self.skip_ws_and_annotations();
        if let Some(n) = self.consume_nullability() {
            base = base.with_nullability(n);
        }

        let mut dims = 0usize;
        loop {
            if dims >= MAX_PARSE_DEPTH {
                break;
            }
            self.skip_ws_and_annotations();
            if self.starts_with("...") {
                self.advance(3);
                if !matches!(base, Ty::Void | Ty::Unknown) {
                    base = Ty::array(base);
                }
                self.skip_ws_and_annotations();
                if let Some(n) = self.consume_nullability() {
                    base = base.with_nullability(n);
                }
                break;
            }
            if self.peek_byte() == Some(b'[') {
                self.advance(1);
                self.skip_ws_and_annotations();
                if self.peek_byte() == Some(b']') {
                    self.advance(1);
                    dims += 1;
                    if !matches!(base, Ty::Void | Ty::Unknown) {
                        base = Ty::array(base);
                    }
                    self.skip_ws_and_annotations();
                    if let Some(n) = self.consume_nullability() {
                        base = base.with_nullability(n);
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        base
    }

    fn parse_type_args(
        &mut self,
        resolve_type: &mut impl FnMut(&[&str]) -> Option<Ty>,
        depth: usize,
    ) -> Vec<TypeArg> {
        if self.peek_byte() != Some(b'<') || depth >= MAX_PARSE_DEPTH {
            return Vec::new();
        }
        self.advance(1);
        let mut args = Vec::new();

        loop {
            self.skip_ws_and_annotations();
            if self.is_eof() || self.peek_byte() == Some(b'>') {
                break;
            }

            let arg = if self.peek_byte() == Some(b'?') {
                self.advance(1);
                self.skip_ws_and_annotations();
                if self.consume_keyword("extends") {
                    let bound = self.parse_type(resolve_type, depth + 1);
                    TypeArg::Wildcard(WildcardBound::Extends(Box::new(bound)))
                } else if self.consume_keyword("super") {
                    let bound = self.parse_type(resolve_type, depth + 1);
                    TypeArg::Wildcard(WildcardBound::Super(Box::new(bound)))
                } else {
                    TypeArg::Wildcard(WildcardBound::Unbounded)
                }
            } else {
                TypeArg::Type(self.parse_type(resolve_type, depth + 1))
            };
            args.push(arg);

            self.skip_ws_and_annotations();
            if self.peek_byte() == Some(b',') {
                self.advance(1);
            } else {
                break;
            }
        }

        if self.peek_byte() == Some(b'>') {
            self.advance(1);
        }
        args
    }
}

// ---- Nullness Scope Resolution & JSpecify Defaulting ----

/// Resolves the effective lexical [`NullnessScope`] at file/package/module level for `file_id`.
#[must_use]
pub fn effective_nullness_scope_for_file(db: &dyn DefDatabase, file_id: FileId) -> NullnessScope {
    let file_tree = db.file_item_tree(file_id);
    if file_tree.nullness_scope != NullnessScope::Inherited {
        return file_tree.nullness_scope;
    }
    let workspace = db.workspace_index();
    let pkg = file_tree.package.as_deref().unwrap_or("");
    let pkg_scope = workspace.package_nullness(pkg);
    if pkg_scope != NullnessScope::Inherited {
        return pkg_scope;
    }
    if let Some(mod_name) = workspace.file_module(file_id) {
        let mod_scope = workspace.module_nullness(mod_name);
        if mod_scope != NullnessScope::Inherited {
            return mod_scope;
        }
    }
    NullnessScope::Inherited
}

fn effective_nullness_scope_in_file_tree(
    db: &dyn DefDatabase,
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    start_ty: &TypeItem,
) -> NullnessScope {
    let mut visited = FxHashSet::default();
    let mut cur = start_ty.clone();
    for _ in 0..MAX_SUBST_DEPTH {
        if cur.nullness_scope != NullnessScope::Inherited {
            return cur.nullness_scope;
        }
        let Some(enc_fqn) = cur.enclosing_type.clone() else {
            break;
        };
        if !visited.insert(enc_fqn.clone()) {
            break;
        }
        if let Some((_, enc_ty)) = file_tree.find_type_by_fqn(&enc_fqn) {
            cur = enc_ty.clone();
        } else if let Some((enc_fid, enc_tid)) = workspace.get_type(&enc_fqn) {
            cur = db.file_item_tree(enc_fid).types[enc_tid].clone();
        } else {
            break;
        }
    }
    effective_nullness_scope_for_file(db, file_tree.file_id)
}

pub(crate) fn effective_nullness_scope_for_binary_type(
    classpath: &ClasspathIndex,
    bin: &BinaryTypeSymbol,
) -> NullnessScope {
    let mut visited = FxHashSet::default();
    visited.insert(bin.qualified_name.clone());
    let mut cur = bin;
    for _ in 0..MAX_SUBST_DEPTH {
        if cur.nullness_scope != NullnessScope::Inherited {
            return cur.nullness_scope;
        }
        let Some(enc_fqn) = cur.enclosing_type.as_deref() else {
            break;
        };
        if !visited.insert(enc_fqn.to_string()) {
            break;
        }
        let Some(enc_bin) = classpath.type_by_fqn(enc_fqn) else {
            break;
        };
        cur = enc_bin;
    }

    let pkg_scope = classpath.package_nullness(&bin.package);
    if pkg_scope != NullnessScope::Inherited {
        return pkg_scope;
    }

    match &bin.origin {
        BinaryOrigin::Jdk {
            module_name: Some(mod_name),
        } => {
            if let Some(mod_info) = classpath.modules.get(mod_name.as_str())
                && mod_info.nullness_scope != NullnessScope::Inherited
            {
                return mod_info.nullness_scope;
            }
        }
        BinaryOrigin::Jar { .. } | BinaryOrigin::Jdk { module_name: None } => {
            if let Some(mod_info) = classpath.modules.values().find(|m| {
                m.nullness_scope != NullnessScope::Inherited
                    && m.exports.iter().any(|exp| exp == &bin.package)
            }) {
                return mod_info.nullness_scope;
            }
        }
    }

    NullnessScope::Inherited
}

/// Resolves the effective lexical [`NullnessScope`] for `owner_fqn`, walking:
/// inner type -> enclosing outer types (cycle-guarded) -> package -> module.
#[must_use]
pub fn effective_nullness_scope_for_type(db: &dyn DefDatabase, owner_fqn: &str) -> NullnessScope {
    let workspace = db.workspace_index();
    if let Some((fid, tid)) = workspace.get_type(owner_fqn) {
        let mut visited = FxHashSet::default();
        visited.insert(owner_fqn.to_string());
        let mut cur_fid = fid;
        let mut cur_tid = tid;
        for _ in 0..MAX_SUBST_DEPTH {
            let tree = db.file_item_tree(cur_fid);
            let ty = &tree.types[cur_tid];
            if ty.nullness_scope != NullnessScope::Inherited {
                return ty.nullness_scope;
            }
            let Some(enc_fqn) = ty.enclosing_type.clone() else {
                return effective_nullness_scope_for_file(db, cur_fid);
            };
            if !visited.insert(enc_fqn.clone()) {
                break;
            }
            if let Some((enc_tid, _)) = tree.find_type_by_fqn(&enc_fqn) {
                cur_tid = enc_tid;
            } else if let Some((enc_fid, enc_tid)) = workspace.get_type(&enc_fqn) {
                cur_fid = enc_fid;
                cur_tid = enc_tid;
            } else {
                return effective_nullness_scope_for_file(db, cur_fid);
            }
        }
        return effective_nullness_scope_for_file(db, fid);
    }

    let classpath = db.classpath_index();
    if let Some(bin) = classpath.type_by_fqn(owner_fqn) {
        return effective_nullness_scope_for_binary_type(&classpath, bin);
    }

    NullnessScope::Inherited
}

/// Resolves the effective lexical [`NullnessScope`] for a member declared in `owner_fqn`.
#[must_use]
pub fn effective_nullness_scope_for_member(
    db: &dyn DefDatabase,
    owner_fqn: &str,
    member_scope: NullnessScope,
) -> NullnessScope {
    if member_scope != NullnessScope::Inherited {
        return member_scope;
    }
    effective_nullness_scope_for_type(db, owner_fqn)
}

pub(crate) fn effective_nullness_scope_for_binary_member(
    classpath: &ClasspathIndex,
    owner: &BinaryTypeSymbol,
    member_scope: NullnessScope,
) -> NullnessScope {
    if member_scope != NullnessScope::Inherited {
        return member_scope;
    }
    effective_nullness_scope_for_binary_type(classpath, owner)
}

/// Applies JSpecify 1.0 defaulting rules for `scope` to `ty`.
///
/// When `is_local_root` is `true` (explicit local-variable declarations per JSpecify §3.2.5),
/// the outermost class/array root is not forced to [`Nullability::NonNull`], while nested type
/// arguments and array element types still receive defaulting.
#[must_use]
pub fn apply_nullness_scope(ty: Ty, scope: NullnessScope, is_local_root: bool) -> Ty {
    apply_nullness_scope_inner(ty, scope, is_local_root, 0)
}

fn apply_nullness_scope_inner(
    ty: Ty,
    scope: NullnessScope,
    is_local_root: bool,
    depth: usize,
) -> Ty {
    if depth >= MAX_SUBST_DEPTH || !matches!(scope, NullnessScope::NonNullByDefault(_)) {
        return ty;
    }
    match ty {
        Ty::Class {
            fqn,
            type_args,
            nullability,
        } => {
            let nullability = if !is_local_root && nullability == Nullability::Unspecified {
                Nullability::NonNull
            } else {
                nullability
            };
            let type_args = type_args
                .into_iter()
                .map(|arg| apply_nullness_scope_arg(arg, scope, depth + 1))
                .collect();
            Ty::Class {
                fqn,
                type_args,
                nullability,
            }
        }
        Ty::Array { elem, nullability } => {
            let nullability = if !is_local_root && nullability == Nullability::Unspecified {
                Nullability::NonNull
            } else {
                nullability
            };
            let elem = Box::new(apply_nullness_scope_inner(*elem, scope, false, depth + 1));
            Ty::Array { elem, nullability }
        }
        // JSpecify parametric rule: unannotated `TypeVar` usages stay `Unspecified` so substitution
        // inherits the type argument's nullability verbatim.
        Ty::TypeVar { .. } => ty,
        Ty::Intersection(members) => Ty::Intersection(
            members
                .into_iter()
                .map(|m| apply_nullness_scope_inner(m, scope, is_local_root, depth + 1))
                .collect(),
        ),
        Ty::Primitive(_) | Ty::Void | Ty::Null | Ty::Unknown => ty,
    }
}

fn apply_nullness_scope_arg(arg: TypeArg, scope: NullnessScope, depth: usize) -> TypeArg {
    if depth >= MAX_SUBST_DEPTH {
        return arg;
    }
    match arg {
        TypeArg::Type(t) => TypeArg::Type(apply_nullness_scope_inner(t, scope, false, depth)),
        TypeArg::Wildcard(WildcardBound::Unbounded) => TypeArg::Wildcard(WildcardBound::Unbounded),
        TypeArg::Wildcard(WildcardBound::Extends(t)) => TypeArg::Wildcard(WildcardBound::Extends(
            Box::new(apply_nullness_scope_inner(*t, scope, false, depth)),
        )),
        TypeArg::Wildcard(WildcardBound::Super(t)) => TypeArg::Wildcard(WildcardBound::Super(
            Box::new(apply_nullness_scope_inner(*t, scope, false, depth)),
        )),
    }
}

pub(crate) fn ty_has_explicit_nullability(ty: &Ty) -> bool {
    ty_has_explicit_nullability_inner(ty, 0)
}

fn ty_has_explicit_nullability_inner(ty: &Ty, depth: usize) -> bool {
    if depth >= MAX_SUBST_DEPTH {
        return false;
    }
    match ty {
        Ty::Class {
            type_args,
            nullability,
            ..
        } => {
            *nullability != Nullability::Unspecified
                || type_args.iter().any(|arg| match arg {
                    TypeArg::Type(t) => ty_has_explicit_nullability_inner(t, depth + 1),
                    TypeArg::Wildcard(WildcardBound::Extends(t) | WildcardBound::Super(t)) => {
                        ty_has_explicit_nullability_inner(t, depth + 1)
                    }
                    TypeArg::Wildcard(WildcardBound::Unbounded) => false,
                })
        }
        Ty::Array { elem, nullability } => {
            *nullability != Nullability::Unspecified
                || ty_has_explicit_nullability_inner(elem, depth + 1)
        }
        Ty::TypeVar { nullability, .. } => *nullability != Nullability::Unspecified,
        Ty::Intersection(members) => members
            .iter()
            .any(|m| ty_has_explicit_nullability_inner(m, depth + 1)),
        Ty::Primitive(_) | Ty::Void | Ty::Null | Ty::Unknown => false,
    }
}

// ---- Scope-Aware Type Resolution ----

/// Resolves a raw source type reference in `file_id` given `type_params_in_scope`.
#[must_use]
pub fn resolve_type_ref_in_file(
    db: &dyn DefDatabase,
    file_id: FileId,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    resolve_type_ref_in_file_for_owner(db, file_id, None, raw, type_params_in_scope)
}

/// Resolves a raw source type reference in `file_id` within `owner_fqn`'s scope.
pub(crate) fn resolve_type_ref_in_file_for_owner(
    db: &dyn DefDatabase,
    file_id: FileId,
    owner_fqn: Option<&str>,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    resolve_type_ref_in_file_for_member(
        db,
        file_id,
        owner_fqn,
        NullnessScope::Inherited,
        raw,
        type_params_in_scope,
    )
}

/// Resolves a raw source type reference in `file_id` within a member's effective nullness scope.
pub(crate) fn resolve_type_ref_in_file_for_member(
    db: &dyn DefDatabase,
    file_id: FileId,
    owner_fqn: Option<&str>,
    member_scope: NullnessScope,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    let file_tree = db.file_item_tree(file_id);
    let workspace = db.workspace_index();
    let scope = if member_scope != NullnessScope::Inherited {
        member_scope
    } else if let Some(fqn) = owner_fqn {
        let s = effective_nullness_scope_for_type(db, fqn);
        if s != NullnessScope::Inherited {
            s
        } else if let Some((_, ty)) = file_tree.find_type_by_fqn(fqn) {
            effective_nullness_scope_in_file_tree(db, &file_tree, &workspace, ty)
        } else {
            effective_nullness_scope_for_file(db, file_id)
        }
    } else {
        effective_nullness_scope_for_file(db, file_id)
    };
    resolve_type_ref_in_file_with_scope(
        db,
        file_id,
        owner_fqn,
        scope,
        false,
        raw,
        type_params_in_scope,
    )
}

/// Resolves a raw source type reference in `file_id` with an explicit [`NullnessScope`] and `is_local_root`.
pub(crate) fn resolve_type_ref_in_file_with_scope(
    db: &dyn DefDatabase,
    file_id: FileId,
    owner_fqn: Option<&str>,
    nullness_scope: NullnessScope,
    is_local_root: bool,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    let file_tree = db.file_item_tree(file_id);
    let workspace = db.workspace_index();
    let classpath = db.classpath_index();
    let owner_ty = owner_fqn.and_then(|fqn| file_tree.find_type_by_fqn(fqn).map(|(_, t)| t));
    let mut visited = FxHashSet::default();
    let ty = resolve_type_ref_in_file_inner(
        db,
        &file_tree,
        &workspace,
        &classpath,
        owner_ty,
        nullness_scope,
        raw,
        type_params_in_scope,
        &mut visited,
        0,
    );
    apply_nullness_scope(ty, nullness_scope, is_local_root)
}

/// Resolves a raw binary type reference in `owner`'s scope against `classpath`.
pub(crate) fn resolve_binary_type_ref_for_owner(
    classpath: &ClasspathIndex,
    owner: &BinaryTypeSymbol,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    resolve_binary_type_ref_for_member(
        classpath,
        owner,
        NullnessScope::Inherited,
        raw,
        type_params_in_scope,
    )
}

/// Resolves a raw binary type reference in a binary member's effective scope against `classpath`.
pub(crate) fn resolve_binary_type_ref_for_member(
    classpath: &ClasspathIndex,
    owner: &BinaryTypeSymbol,
    member_scope: NullnessScope,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    let scope = effective_nullness_scope_for_binary_member(classpath, owner, member_scope);
    let mut visited = FxHashSet::default();
    let ty = resolve_binary_type_ref_in_db(
        classpath,
        owner,
        scope,
        raw,
        type_params_in_scope,
        &mut visited,
        0,
    );
    apply_nullness_scope(ty, scope, false)
}

/// Resolves a raw binary type reference (as decoded by `classfile.rs`) given `type_params_in_scope`.
#[must_use]
pub fn resolve_binary_type_ref(
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
) -> Ty {
    let mut visited = FxHashSet::default();
    resolve_binary_type_ref_inner(raw, type_params_in_scope, &mut visited, 0)
}

fn resolve_binary_type_ref_inner(
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
    visited: &mut FxHashSet<SmolStr>,
    depth: usize,
) -> Ty {
    parse_type_ref(raw, &mut |segs| {
        if let [name] = segs
            && let Some(raw_bounds) = type_params_in_scope.get(*name)
        {
            let tv_name = SmolStr::new(name);
            let bounds = if depth < MAX_SUBST_DEPTH && visited.insert(tv_name.clone()) {
                let resolved: Vec<Ty> = raw_bounds
                    .iter()
                    .map(|b| {
                        resolve_binary_type_ref_inner(b, type_params_in_scope, visited, depth + 1)
                    })
                    .filter(|t| !matches!(t, Ty::Unknown))
                    .collect();
                visited.remove(&tv_name);
                resolved
            } else {
                Vec::new()
            };
            return Some(Ty::type_var(tv_name, bounds));
        }

        let fqn = match segs {
            [name] => format!("java.lang.{name}"),
            [first, ..] if is_known_java_lang_nested_outer(first) => {
                format!("java.lang.{}", segs.join("."))
            }
            _ => segs.join("."),
        };
        Some(Ty::raw_class(fqn))
    })
}

fn is_known_java_lang_nested_outer(name: &str) -> bool {
    matches!(
        name,
        "Character" | "Thread" | "ProcessBuilder" | "StackWalker" | "ModuleLayer" | "System"
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "deferred to Commit 2 ty split (TypeResolveEnv)"
)]
fn resolve_type_ref_in_file_inner(
    db: &dyn DefDatabase,
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    owner_ty: Option<&TypeItem>,
    nullness_scope: NullnessScope,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
    visited: &mut FxHashSet<SmolStr>,
    depth: usize,
) -> Ty {
    parse_type_ref(raw, &mut |segs| {
        if let [name] = segs
            && let Some(raw_bounds) = type_params_in_scope.get(*name)
        {
            let tv_name = SmolStr::new(name);
            let bounds = if depth < MAX_SUBST_DEPTH && visited.insert(tv_name.clone()) {
                let resolved = if raw_bounds.is_empty() {
                    if matches!(nullness_scope, NullnessScope::NonNullByDefault(_)) {
                        vec![Ty::object().with_nullability(Nullability::NonNull)]
                    } else {
                        Vec::new()
                    }
                } else {
                    raw_bounds
                        .iter()
                        .map(|b| {
                            let ty = resolve_type_ref_in_file_inner(
                                db,
                                file_tree,
                                workspace,
                                classpath,
                                owner_ty,
                                nullness_scope,
                                b,
                                type_params_in_scope,
                                visited,
                                depth + 1,
                            );
                            apply_nullness_scope(ty, nullness_scope, false)
                        })
                        .filter(|t| !matches!(t, Ty::Unknown))
                        .collect()
                };
                visited.remove(&tv_name);
                resolved
            } else {
                Vec::new()
            };
            return Some(Ty::type_var(tv_name, bounds));
        }

        let fqn = resolve_class_segments_in_file(
            db,
            file_tree,
            workspace,
            classpath,
            owner_ty,
            segs,
        )?;
        Some(Ty::raw_class(fqn))
    })
}

fn resolve_binary_type_ref_in_db(
    classpath: &ClasspathIndex,
    owner: &BinaryTypeSymbol,
    nullness_scope: NullnessScope,
    raw: &str,
    type_params_in_scope: &BTreeMap<SmolStr, Vec<String>>,
    visited: &mut FxHashSet<SmolStr>,
    depth: usize,
) -> Ty {
    parse_type_ref(raw, &mut |segs| {
        if let [name] = segs
            && let Some(raw_bounds) = type_params_in_scope.get(*name)
        {
            let tv_name = SmolStr::new(name);
            let bounds = if depth < MAX_SUBST_DEPTH && visited.insert(tv_name.clone()) {
                let resolved = if raw_bounds.is_empty() {
                    if matches!(nullness_scope, NullnessScope::NonNullByDefault(_)) {
                        vec![Ty::object().with_nullability(Nullability::NonNull)]
                    } else {
                        Vec::new()
                    }
                } else {
                    raw_bounds
                        .iter()
                        .map(|b| {
                            let ty = resolve_binary_type_ref_in_db(
                                classpath,
                                owner,
                                nullness_scope,
                                b,
                                type_params_in_scope,
                                visited,
                                depth + 1,
                            );
                            apply_nullness_scope(ty, nullness_scope, false)
                        })
                        .filter(|t| !matches!(t, Ty::Unknown))
                        .collect()
                };
                visited.remove(&tv_name);
                resolved
            } else {
                Vec::new()
            };
            return Some(Ty::type_var(tv_name, bounds));
        }

        let joined = segs.join(".");
        if classpath.type_by_fqn(&joined).is_some() {
            return Some(Ty::raw_class(joined));
        }
        if !owner.package.is_empty() {
            let same_pkg = format!("{}.{joined}", owner.package);
            if classpath.type_by_fqn(&same_pkg).is_some() {
                return Some(Ty::raw_class(same_pkg));
            }
        }
        let java_lang = format!("java.lang.{joined}");
        if classpath.type_by_fqn(&java_lang).is_some() {
            return Some(Ty::raw_class(java_lang));
        }
        let fqn = match segs {
            [name] => format!("java.lang.{name}"),
            [first, ..] if is_known_java_lang_nested_outer(first) => java_lang,
            _ => joined,
        };
        Some(Ty::raw_class(fqn))
    })
}

fn is_known_type_fqn(
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    fqn: &str,
) -> bool {
    workspace.contains_type(fqn)
        || file_tree.find_type_by_fqn(fqn).is_some()
        || classpath.type_by_fqn(fqn).is_some()
}

fn resolve_pkg_type(
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    pkg: &str,
    name: &str,
) -> Option<String> {
    if let Some(fqn) = workspace
        .package_type_map
        .get(pkg)
        .and_then(|m| m.get(name))
        && workspace.contains_type(fqn)
    {
        return Some(fqn.clone());
    }
    if file_tree.package.as_deref().unwrap_or("") == pkg
        && let Some((_, ty)) = file_tree
            .types
            .iter()
            .find(|(_, t)| t.enclosing_type.is_none() && t.name == name)
    {
        return Some(ty.qualified_name.clone());
    }
    if let Some(bin) = classpath.type_in_package(pkg, name) {
        return Some(bin.qualified_name.clone());
    }
    None
}

fn core_java_lang_fallback(name: &str) -> Option<&'static str> {
    match name {
        "Object" => Some("java.lang.Object"),
        "String" => Some("java.lang.String"),
        "Number" => Some("java.lang.Number"),
        "Boolean" => Some("java.lang.Boolean"),
        "Byte" => Some("java.lang.Byte"),
        "Short" => Some("java.lang.Short"),
        "Integer" => Some("java.lang.Integer"),
        "Long" => Some("java.lang.Long"),
        "Character" => Some("java.lang.Character"),
        "Float" => Some("java.lang.Float"),
        "Double" => Some("java.lang.Double"),
        "Void" => Some("java.lang.Void"),
        "Enum" => Some("java.lang.Enum"),
        "Record" => Some("java.lang.Record"),
        "Class" => Some("java.lang.Class"),
        "Comparable" => Some("java.lang.Comparable"),
        "Iterable" => Some("java.lang.Iterable"),
        "CharSequence" => Some("java.lang.CharSequence"),
        "Cloneable" => Some("java.lang.Cloneable"),
        "AutoCloseable" => Some("java.lang.AutoCloseable"),
        "Throwable" => Some("java.lang.Throwable"),
        "Exception" => Some("java.lang.Exception"),
        "RuntimeException" => Some("java.lang.RuntimeException"),
        "Error" => Some("java.lang.Error"),
        "Runnable" => Some("java.lang.Runnable"),
        "FunctionalInterface" => Some("java.lang.FunctionalInterface"),
        _ => None,
    }
}

fn resolve_single_segment_class(
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    owner_ty: Option<&TypeItem>,
    name: &str,
) -> Option<String> {
    if let Some(owner) = owner_ty {
        if owner.name == name {
            return Some(owner.qualified_name.clone());
        }
        if owner.nested_types.iter().any(|n| n == name) {
            let nested_fqn = format!("{}.{name}", owner.qualified_name);
            if is_known_type_fqn(file_tree, workspace, classpath, &nested_fqn) {
                return Some(nested_fqn);
            }
        }
        let mut enclosing = owner.enclosing_type.as_deref();
        let mut steps = 0usize;
        while let Some(enc_fqn) = enclosing {
            if steps >= MAX_SUBST_DEPTH {
                break;
            }
            steps += 1;
            let Some((_, enc_ty)) = file_tree.find_type_by_fqn(enc_fqn) else {
                break;
            };
            if enc_ty.name == name {
                return Some(enc_ty.qualified_name.clone());
            }
            if enc_ty.nested_types.iter().any(|n| n == name) {
                let nested_fqn = format!("{enc_fqn}.{name}");
                if is_known_type_fqn(file_tree, workspace, classpath, &nested_fqn) {
                    return Some(nested_fqn);
                }
            }
            enclosing = enc_ty.enclosing_type.as_deref();
        }
    }

    // Single-type imports (workspace first, then binary).
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::SingleType && imp.path.last().map(String::as_str) == Some(name) {
            let fqn = imp.path.join(".");
            if workspace.contains_type(&fqn) || file_tree.find_type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::SingleType && imp.path.last().map(String::as_str) == Some(name) {
            let fqn = imp.path.join(".");
            if classpath.type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }

    // Static single imports resolving to a nested type.
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticSingle
            && imp.path.last().map(String::as_str) == Some(name)
        {
            let fqn = imp.path.join(".");
            if workspace.contains_type(&fqn) || file_tree.find_type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticSingle
            && imp.path.last().map(String::as_str) == Some(name)
        {
            let fqn = imp.path.join(".");
            if classpath.type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }

    // Same-package top-level types.
    let pkg = file_tree.package.as_deref().unwrap_or("");
    if let Some(found) = resolve_pkg_type(file_tree, workspace, classpath, pkg, name) {
        return Some(found);
    }

    // Implicit `java.lang.*`.
    if let Some(found) = resolve_pkg_type(file_tree, workspace, classpath, "java.lang", name) {
        return Some(found);
    }

    // Type-import-on-demand (`import a.b.*;`).
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::TypeOnDemand {
            let fqn = format!("{}.{name}", imp.path.join("."));
            if workspace.contains_type(&fqn) || file_tree.find_type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::TypeOnDemand {
            let fqn = format!("{}.{name}", imp.path.join("."));
            if classpath.type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }

    // Static-import-on-demand (`import static a.b.Outer.*;`).
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticOnDemand {
            let fqn = format!("{}.{name}", imp.path.join("."));
            if workspace.contains_type(&fqn) || file_tree.find_type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::StaticOnDemand {
            let fqn = format!("{}.{name}", imp.path.join("."));
            if classpath.type_by_fqn(&fqn).is_some() {
                return Some(fqn);
            }
        }
    }

    // Module imports (`import module M;`).
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::Module {
            let module_name = imp.path.join(".");
            for exported_pkg in classpath.transitive_module_exports(&module_name) {
                if let Some(found) =
                    resolve_pkg_type(file_tree, workspace, classpath, exported_pkg, name)
                {
                    return Some(found);
                }
            }
        }
    }

    // Nested types declared in the same file when no explicit owner was supplied.
    if owner_ty.is_none()
        && let Some((_, ty)) = file_tree
            .types
            .iter()
            .find(|(_, t)| t.enclosing_type.is_some() && t.name == name)
    {
        return Some(ty.qualified_name.clone());
    }

    // Best-effort fallback for unindexed single-type imports and core `java.lang.*` classes.
    for (_, imp) in file_tree.imports.iter() {
        if imp.kind == ImportKind::SingleType && imp.path.last().map(String::as_str) == Some(name) {
            return Some(imp.path.join("."));
        }
    }
    if let Some(lang_fqn) = core_java_lang_fallback(name) {
        return Some(lang_fqn.to_string());
    }

    None
}

fn resolve_class_segments_in_file(
    db: &dyn DefDatabase,
    file_tree: &FileItemTree,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    owner_ty: Option<&TypeItem>,
    segs: &[&str],
) -> Option<String> {
    if let [name] = segs {
        return resolve_single_segment_class(file_tree, workspace, classpath, owner_ty, name);
    }

    let joined = segs.join(".");
    if is_known_type_fqn(file_tree, workspace, classpath, &joined) {
        return Some(joined);
    }

    if let Some((first, rest)) = segs.split_first()
        && let Some(first_fqn) =
            resolve_single_segment_class(file_tree, workspace, classpath, owner_ty, first)
    {
        let mut current = first_fqn;
        let mut all_resolved = true;
        for seg in rest {
            let direct_next = format!("{current}.{seg}");
            if is_known_type_fqn(file_tree, workspace, classpath, &direct_next) {
                current = direct_next;
                continue;
            }
            let inherited = linearized_supertypes(db, &current).into_iter().find_map(|sup| {
                let candidate = format!("{sup}.{seg}");
                is_known_type_fqn(file_tree, workspace, classpath, &candidate)
                    .then_some(candidate)
            });
            if let Some(next) = inherited {
                current = next;
            } else {
                all_resolved = false;
                break;
            }
        }
        if all_resolved {
            return Some(current);
        }
    }

    Some(joined)
}

/// Returns the declared type-parameter names of `fqn` from workspace sources or the classpath.
#[must_use]
pub fn type_params_of_fqn(db: &dyn DefDatabase, fqn: &str) -> Vec<SmolStr> {
    let workspace = db.workspace_index();
    if let Some((fid, tid)) = workspace.get_type(fqn) {
        return db.file_item_tree(fid).types[tid].type_params.clone();
    }
    let classpath = db.classpath_index();
    if let Some(bin) = classpath.type_by_fqn(fqn) {
        return bin.type_params.clone();
    }
    Vec::new()
}

/// Returns the type-parameter bounds map for `fqn` (including unbounded type parameters with an
/// empty `Vec`, and non-static enclosing type parameters).
#[must_use]
pub fn type_param_bounds_of_fqn(db: &dyn DefDatabase, fqn: &str) -> BTreeMap<SmolStr, Vec<String>> {
    let workspace = db.workspace_index();
    if let Some((fid, tid)) = workspace.get_type(fqn) {
        let tree = db.file_item_tree(fid);
        return type_param_bounds_for_workspace_item(db, &tree, &tree.types[tid]);
    }
    let classpath = db.classpath_index();
    if let Some(bin) = classpath.type_by_fqn(fqn) {
        return type_param_bounds_for_binary(db, bin);
    }
    BTreeMap::new()
}

fn type_param_bounds_for_workspace_item(
    db: &dyn DefDatabase,
    tree: &FileItemTree,
    ty: &TypeItem,
) -> BTreeMap<SmolStr, Vec<String>> {
    let mut out = BTreeMap::new();
    // Collect enclosing non-static type parameters outermost-to-innermost so inner shadows outer.
    let mut chain: Vec<TypeItem> = Vec::new();
    let mut cur = ty.clone();
    for _ in 0..MAX_SUBST_DEPTH {
        let is_static = cur.is_static
            || matches!(
                cur.kind,
                TypeKind::Interface | TypeKind::Enum | TypeKind::Record | TypeKind::Annotation
            );
        let enc = cur.enclosing_type.clone();
        chain.push(cur);
        if is_static {
            break;
        }
        let Some(enc_fqn) = enc else {
            break;
        };
        if let Some((_, enc_item)) = tree.find_type_by_fqn(&enc_fqn) {
            cur = enc_item.clone();
        } else if let Some((fid, tid)) = db.workspace_index().get_type(&enc_fqn) {
            cur = db.file_item_tree(fid).types[tid].clone();
        } else {
            break;
        }
    }
    for item in chain.into_iter().rev() {
        for param in &item.type_params {
            let bounds = item
                .type_param_bounds
                .get(param)
                .cloned()
                .unwrap_or_default();
            out.insert(param.clone(), bounds);
        }
    }
    out
}

fn type_param_bounds_for_binary(
    db: &dyn DefDatabase,
    bin: &BinaryTypeSymbol,
) -> BTreeMap<SmolStr, Vec<String>> {
    let mut out = BTreeMap::new();
    let classpath = db.classpath_index();
    let mut chain: Vec<&BinaryTypeSymbol> = Vec::new();
    let mut cur = bin;
    for _ in 0..MAX_SUBST_DEPTH {
        let is_static = cur.is_static
            || matches!(
                cur.kind,
                TypeKind::Interface | TypeKind::Enum | TypeKind::Record | TypeKind::Annotation
            );
        chain.push(cur);
        if is_static {
            break;
        }
        let Some(enc_fqn) = cur.enclosing_type.as_deref() else {
            break;
        };
        let Some(enc_bin) = classpath.type_by_fqn(enc_fqn) else {
            break;
        };
        cur = enc_bin;
    }
    for item in chain.into_iter().rev() {
        for param in &item.type_params {
            let bounds = item
                .type_param_bounds
                .get(param)
                .cloned()
                .unwrap_or_default();
            out.insert(param.clone(), bounds);
        }
    }
    out
}

// ---- Numeric Promotion & Boxing ----

fn unbox_numeric(ty: &Ty) -> Option<PrimitiveType> {
    match ty {
        Ty::Primitive(p) if p.is_numeric() => Some(*p),
        Ty::Class { fqn, type_args, .. } if type_args.is_empty() => {
            PrimitiveType::from_boxed_fqn(fqn).filter(|p| p.is_numeric())
        }
        _ => None,
    }
}

/// Applies JLS §5.6 unary numeric promotion (`byte`/`short`/`char` -> `int`; handles boxed wrappers).
#[must_use]
pub fn unary_numeric_promotion(ty: &Ty) -> Option<PrimitiveType> {
    let p = unbox_numeric(ty)?;
    match p {
        PrimitiveType::Byte | PrimitiveType::Short | PrimitiveType::Char | PrimitiveType::Int => {
            Some(PrimitiveType::Int)
        }
        PrimitiveType::Long | PrimitiveType::Float | PrimitiveType::Double => Some(p),
        PrimitiveType::Boolean => None,
    }
}

/// Applies JLS §5.6 binary numeric promotion (handles unboxed and boxed numeric types).
#[must_use]
pub fn binary_numeric_promotion(a: &Ty, b: &Ty) -> Option<PrimitiveType> {
    let pa = unbox_numeric(a)?;
    let pb = unbox_numeric(b)?;
    if pa == PrimitiveType::Double || pb == PrimitiveType::Double {
        Some(PrimitiveType::Double)
    } else if pa == PrimitiveType::Float || pb == PrimitiveType::Float {
        Some(PrimitiveType::Float)
    } else if pa == PrimitiveType::Long || pb == PrimitiveType::Long {
        Some(PrimitiveType::Long)
    } else {
        Some(PrimitiveType::Int)
    }
}

// ---- Supertype Substitution ----

/// Computes the [`Substitution`] from any [`Ty`] (`Class`, `TypeVar`, `Intersection`, `Array`)
/// to `target_fqn`.
pub(crate) fn supertype_substitution_for_ty(
    db: &dyn DefDatabase,
    ty: &Ty,
    target_fqn: &str,
) -> Option<Substitution> {
    supertype_substitution_for_ty_inner(db, ty, target_fqn, 0)
}

fn supertype_substitution_for_ty_inner(
    db: &dyn DefDatabase,
    ty: &Ty,
    target_fqn: &str,
    depth: usize,
) -> Option<Substitution> {
    if depth >= MAX_SUBST_DEPTH {
        return None;
    }
    match ty {
        Ty::Class { fqn, type_args, .. } => supertype_substitution(db, fqn, type_args, target_fqn),
        Ty::TypeVar { bounds, .. } => {
            if bounds.is_empty() {
                (target_fqn == OBJECT_FQN).then(Substitution::default)
            } else {
                bounds
                    .iter()
                    .find_map(|b| supertype_substitution_for_ty_inner(db, b, target_fqn, depth + 1))
            }
        }
        Ty::Intersection(members) => members
            .iter()
            .find_map(|m| supertype_substitution_for_ty_inner(db, m, target_fqn, depth + 1)),
        Ty::Array { .. } => matches!(
            target_fqn,
            OBJECT_FQN | CLONEABLE_FQN | SERIALIZABLE_FQN
        )
        .then(Substitution::default),
        _ => None,
    }
}

/// Walks the inheritance graph from `sub_fqn<sub_args>` to `target_fqn` and computes the
/// instantiated [`Substitution`] mapping `target_fqn`'s type parameters to their substituted [`TypeArg`]s.
#[must_use]
pub fn supertype_substitution(
    db: &dyn DefDatabase,
    sub_fqn: &str,
    sub_args: &[TypeArg],
    target_fqn: &str,
) -> Option<Substitution> {
    let workspace = db.workspace_index();
    let classpath = db.classpath_index();

    if sub_fqn == target_fqn {
        let params = type_params_of_fqn(db, sub_fqn);
        if !params.is_empty() && sub_args.is_empty() {
            return Some(Substitution::default());
        }
        return Some(Substitution::from_params_and_args(&params, sub_args));
    }

    let mut visited = FxHashSet::default();
    visited.insert(sub_fqn.to_string());

    let mut queue: VecDeque<(String, Vec<TypeArg>, usize)> = VecDeque::new();
    queue.push_back((sub_fqn.to_string(), sub_args.to_vec(), 0));

    let mut start_exists = false;

    while let Some((cur_fqn, cur_args, depth)) = queue.pop_front() {
        if depth >= MAX_SUBST_DEPTH {
            continue;
        }

        let (type_params, super_tys, exists) =
            direct_generic_supertypes(db, &workspace, &classpath, &cur_fqn);
        if depth == 0 {
            start_exists = exists;
        }
        if !exists {
            continue;
        }

        let is_raw = !type_params.is_empty() && cur_args.is_empty();
        let cur_subst = if is_raw {
            Substitution::default()
        } else {
            Substitution::from_params_and_args(&type_params, &cur_args)
        };

        for sup_ty in super_tys {
            let Ty::Class {
                fqn: sup_fqn,
                type_args: sup_raw_args,
                ..
            } = sup_ty
            else {
                continue;
            };
            let next_args: Vec<TypeArg> = if is_raw {
                Vec::new()
            } else {
                sup_raw_args
                    .iter()
                    .map(|arg| cur_subst.apply_arg(arg))
                    .collect()
            };

            if sup_fqn == target_fqn {
                let target_params = type_params_of_fqn(db, target_fqn);
                if !target_params.is_empty() && next_args.is_empty() {
                    return Some(Substitution::default());
                }
                return Some(Substitution::from_params_and_args(
                    &target_params,
                    &next_args,
                ));
            }

            if visited.insert(sup_fqn.clone()) {
                queue.push_back((sup_fqn, next_args, depth + 1));
            }
        }
    }

    if target_fqn == OBJECT_FQN && start_exists {
        return Some(Substitution::default());
    }

    None
}

fn direct_generic_supertypes(
    db: &dyn DefDatabase,
    workspace: &WorkspaceIndex,
    classpath: &ClasspathIndex,
    fqn: &str,
) -> (Vec<SmolStr>, Vec<Ty>, bool) {
    if let Some((fid, tid)) = workspace.get_type(fqn) {
        let tree = db.file_item_tree(fid);
        let ty_item = &tree.types[tid];
        let scope = type_param_bounds_for_workspace_item(db, &tree, ty_item);
        let eff_scope = effective_nullness_scope_in_file_tree(db, &tree, workspace, ty_item);
        let mut out = Vec::new();
        let mut visited_tv = FxHashSet::default();

        let mut resolve_ws_sup = |raw: &str| {
            let ty = resolve_type_ref_in_file_inner(
                db,
                &tree,
                workspace,
                classpath,
                Some(ty_item),
                eff_scope,
                raw,
                &scope,
                &mut visited_tv,
                0,
            );
            apply_nullness_scope(ty, eff_scope, false)
        };

        match ty_item.kind {
            TypeKind::Class => {
                if let Some(first_ext) = ty_item.extends.first() {
                    out.push(resolve_ws_sup(first_ext));
                } else if ty_item.qualified_name != OBJECT_FQN {
                    out.push(Ty::object());
                }
                for raw in &ty_item.implements {
                    out.push(resolve_ws_sup(raw));
                }
            }
            TypeKind::Interface => {
                for raw in &ty_item.extends {
                    out.push(resolve_ws_sup(raw));
                }
            }
            TypeKind::Enum => {
                let self_cls = apply_nullness_scope(
                    Ty::raw_class(&ty_item.qualified_name),
                    eff_scope,
                    false,
                );
                out.push(Ty::class(ENUM_FQN, vec![TypeArg::Type(self_cls)]));
                for raw in &ty_item.implements {
                    out.push(resolve_ws_sup(raw));
                }
            }
            TypeKind::Record => {
                out.push(Ty::raw_class(RECORD_FQN));
                for raw in &ty_item.implements {
                    out.push(resolve_ws_sup(raw));
                }
            }
            TypeKind::Annotation => {
                out.push(Ty::raw_class(ANNOTATION_FQN));
            }
        }
        return (ty_item.type_params.clone(), out, true);
    }

    if let Some(bin) = classpath.type_by_fqn(fqn) {
        let scope = type_param_bounds_for_binary(db, bin);
        let eff_scope = effective_nullness_scope_for_binary_type(classpath, bin);
        let mut out = Vec::new();
        let mut visited_tv = FxHashSet::default();

        let mut resolve_bin_sup = |raw: &str| {
            let ty = resolve_binary_type_ref_in_db(
                classpath,
                bin,
                eff_scope,
                raw,
                &scope,
                &mut visited_tv,
                0,
            );
            apply_nullness_scope(ty, eff_scope, false)
        };

        if bin.qualified_name != OBJECT_FQN {
            match bin.kind {
                TypeKind::Interface => {}
                TypeKind::Annotation => {
                    out.push(Ty::raw_class(ANNOTATION_FQN));
                }
                TypeKind::Enum => {
                    if let Some(raw) = &bin.super_class_generic {
                        out.push(resolve_bin_sup(raw));
                    } else if let Some(sup) = &bin.super_class {
                        out.push(Ty::raw_class(sup));
                    } else {
                        let self_cls = apply_nullness_scope(
                            Ty::raw_class(&bin.qualified_name),
                            eff_scope,
                            false,
                        );
                        out.push(Ty::class(ENUM_FQN, vec![TypeArg::Type(self_cls)]));
                    }
                }
                TypeKind::Record => {
                    if let Some(raw) = &bin.super_class_generic {
                        out.push(resolve_bin_sup(raw));
                    } else {
                        out.push(Ty::raw_class(
                            bin.super_class.as_deref().unwrap_or(RECORD_FQN),
                        ));
                    }
                }
                TypeKind::Class => {
                    if let Some(raw) = &bin.super_class_generic {
                        out.push(resolve_bin_sup(raw));
                    } else {
                        out.push(Ty::raw_class(
                            bin.super_class.as_deref().unwrap_or(OBJECT_FQN),
                        ));
                    }
                }
            }
        }

        if !bin.interfaces_generic.is_empty() {
            for raw in &bin.interfaces_generic {
                out.push(resolve_bin_sup(raw));
            }
        } else {
            for iface in &bin.interfaces {
                out.push(Ty::raw_class(iface));
            }
        }

        return (bin.type_params.clone(), out, true);
    }

    if fqn == ENUM_FQN || fqn == RECORD_FQN {
        return (
            if fqn == ENUM_FQN {
                vec![SmolStr::new("E")]
            } else {
                Vec::new()
            },
            vec![Ty::object()],
            true,
        );
    }

    (Vec::new(), Vec::new(), false)
}

// ---- Subtyping, Assignability & LUB ----

/// Returns `true` if `sub` is a subtype of `sup` (JLS §4.10).
#[must_use]
pub fn is_subtype(db: &dyn DefDatabase, sub: &Ty, sup: &Ty) -> bool {
    is_subtype_inner(db, sub, sup, 0)
}

const fn nullability_is_subtype(sub: Nullability, sup: Nullability) -> bool {
    match (sub, sup) {
        (Nullability::Unspecified, Nullability::Unspecified)
        | (Nullability::NonNull, Nullability::NonNull)
        | (Nullability::Nullable, Nullability::Nullable) => true,
        // `NonNull` (`Point!`) is a subtype of both `Nullable` (`Point?`) and legacy `Unspecified` (`Point`).
        (Nullability::NonNull, Nullability::Nullable | Nullability::Unspecified) => true,
        // Legacy compatibility between `Unspecified` and `Nullable`.
        (Nullability::Nullable, Nullability::Unspecified)
        | (Nullability::Unspecified, Nullability::Nullable) => true,
        // `Nullable` / `Unspecified` is NOT strictly `<: NonNull`.
        (Nullability::Nullable | Nullability::Unspecified, Nullability::NonNull) => false,
    }
}

const fn lub_nullability(a: Nullability, b: Nullability) -> Nullability {
    match (a, b) {
        (Nullability::NonNull, Nullability::NonNull) => Nullability::NonNull,
        (Nullability::Nullable, _) | (_, Nullability::Nullable) => Nullability::Nullable,
        (Nullability::Unspecified, _) | (_, Nullability::Unspecified) => Nullability::Unspecified,
    }
}

fn is_subtype_inner(db: &dyn DefDatabase, sub: &Ty, sup: &Ty, depth: usize) -> bool {
    if depth >= MAX_SUBST_DEPTH {
        return false;
    }
    if matches!(sub, Ty::Unknown | Ty::Void) || matches!(sup, Ty::Unknown | Ty::Void) {
        return false;
    }
    if sub == sup {
        return true;
    }

    // Intersection on the right: `sub` must be a subtype of every member.
    if let Ty::Intersection(sup_members) = sup {
        return !sup_members.is_empty()
            && sup_members
                .iter()
                .all(|m| is_subtype_inner(db, sub, m, depth + 1));
    }

    // `null` is a subtype of every nullable reference type (NOT `Point!`, `T!`, `String[]!`, or primitives).
    if matches!(sub, Ty::Null) {
        return sup.is_nullable() && sup.is_reference();
    }

    // Intersection on the left: any member of `sub` satisfies `sup`.
    if let Ty::Intersection(sub_members) = sub {
        return sub_members
            .iter()
            .any(|m| is_subtype_inner(db, m, sup, depth + 1));
    }

    match (sub, sup) {
        (Ty::Primitive(a), Ty::Primitive(b)) => a == b || a.widens_to(*b),
        (
            Ty::TypeVar {
                name: sub_name,
                bounds,
                nullability: sub_null,
            },
            _,
        ) => {
            if let Ty::TypeVar {
                name: sup_name,
                nullability: sup_null,
                ..
            } = sup
                && sub_name == sup_name
            {
                return nullability_is_subtype(*sub_null, *sup_null);
            }
            let apply_null = |bound: Ty| -> Ty {
                match sub_null {
                    Nullability::Unspecified => bound,
                    n @ (Nullability::NonNull | Nullability::Nullable) => bound.with_nullability(*n),
                }
            };
            if bounds.is_empty() {
                is_subtype_inner(db, &apply_null(Ty::object()), sup, depth + 1)
            } else {
                bounds
                    .iter()
                    .any(|b| is_subtype_inner(db, &apply_null(b.clone()), sup, depth + 1))
            }
        }
        (
            Ty::Array {
                elem: sub_elem,
                nullability: sub_null,
            },
            Ty::Array {
                elem: sup_elem,
                nullability: sup_null,
            },
        ) => {
            if !nullability_is_subtype(*sub_null, *sup_null) {
                return false;
            }
            if sub_elem.is_primitive() || sup_elem.is_primitive() {
                sub_elem == sup_elem
            } else {
                is_subtype_inner(db, sub_elem, sup_elem, depth + 1)
            }
        }
        (
            Ty::Array {
                nullability: sub_null,
                ..
            },
            Ty::Class {
                fqn,
                type_args,
                nullability: sup_null,
            },
        ) => {
            nullability_is_subtype(*sub_null, *sup_null)
                && type_args.is_empty()
                && matches!(
                    fqn.as_str(),
                    OBJECT_FQN | CLONEABLE_FQN | SERIALIZABLE_FQN
                )
        }
        (
            Ty::Class {
                fqn: sub_fqn,
                type_args: sub_args,
                nullability: sub_null,
            },
            Ty::Class {
                fqn: sup_fqn,
                type_args: sup_args,
                nullability: sup_null,
            },
        ) => {
            nullability_is_subtype(*sub_null, *sup_null)
                && is_class_subtype(db, sub_fqn, sub_args, sup_fqn, sup_args, depth)
        }
        _ => false,
    }
}

fn is_class_subtype(
    db: &dyn DefDatabase,
    sub_fqn: &str,
    sub_args: &[TypeArg],
    sup_fqn: &str,
    sup_args: &[TypeArg],
    depth: usize,
) -> bool {
    if sup_fqn == OBJECT_FQN && sup_args.is_empty() {
        return true;
    }

    if sub_fqn == sup_fqn {
        if sup_args.is_empty() {
            return true;
        }
        // Raw type compatibility: `List <: List<String>`.
        if sub_args.is_empty() {
            return true;
        }
        if sub_args.len() != sup_args.len() {
            return false;
        }
        return sub_args
            .iter()
            .zip(sup_args.iter())
            .all(|(sa, pa)| type_arg_contained_by(db, sa, pa, depth + 1));
    }

    let Some(subst) = supertype_substitution(db, sub_fqn, sub_args, sup_fqn) else {
        // Fallback to linearized_supertypes when `sub_fqn` isn't indexed directly for generics
        // or if `sup_args` is empty.
        return sup_args.is_empty()
            && linearized_supertypes(db, sub_fqn)
                .iter()
                .any(|s| s == sup_fqn);
    };

    if sup_args.is_empty() {
        return true;
    }

    let sub_params = type_params_of_fqn(db, sub_fqn);
    if !sub_params.is_empty() && sub_args.is_empty() {
        // Raw subtype is compatible with any parameterization of `sup_fqn`.
        return true;
    }

    let sup_params = type_params_of_fqn(db, sup_fqn);
    if sup_params.len() != sup_args.len() {
        return false;
    }

    for (param, sup_arg) in sup_params.iter().zip(sup_args.iter()) {
        let Some(sub_arg) = subst.map.get(param) else {
            // Raw supertype path in hierarchy.
            return true;
        };
        if !type_arg_contained_by(db, sub_arg, sup_arg, depth + 1) {
            return false;
        }
    }
    true
}

fn type_arg_contained_by(
    db: &dyn DefDatabase,
    sub_arg: &TypeArg,
    sup_arg: &TypeArg,
    depth: usize,
) -> bool {
    if depth >= MAX_SUBST_DEPTH {
        return false;
    }
    match (sub_arg, sup_arg) {
        (_, TypeArg::Wildcard(WildcardBound::Unbounded)) => true,
        (TypeArg::Type(s), TypeArg::Type(t)) => s == t,
        (TypeArg::Type(s), TypeArg::Wildcard(WildcardBound::Extends(u))) => {
            is_subtype_inner(db, s, u, depth + 1)
        }
        (TypeArg::Type(s), TypeArg::Wildcard(WildcardBound::Super(l))) => {
            is_subtype_inner(db, l, s, depth + 1)
        }
        (
            TypeArg::Wildcard(WildcardBound::Extends(u1)),
            TypeArg::Wildcard(WildcardBound::Extends(u2)),
        ) => is_subtype_inner(db, u1, u2, depth + 1),
        (
            TypeArg::Wildcard(WildcardBound::Super(l1)),
            TypeArg::Wildcard(WildcardBound::Super(l2)),
        ) => is_subtype_inner(db, l2, l1, depth + 1),
        (
            TypeArg::Wildcard(WildcardBound::Unbounded),
            TypeArg::Wildcard(WildcardBound::Extends(u)),
        ) => is_subtype_inner(db, &Ty::object(), u, depth + 1),
        _ => false,
    }
}

/// Returns `true` if `from` is assignment-compatible with `to` (JLS §5.2: subtyping plus
/// primitive boxing/unboxing and widening, and resilient migration from `Unspecified`
/// nullability to `NonNull` while rejecting `Null`/`Nullable` -> `NonNull`).
#[must_use]
pub fn is_assignable(db: &dyn DefDatabase, from: &Ty, to: &Ty) -> bool {
    if is_subtype(db, from, to) {
        return true;
    }
    // Resilient migration: unannotated (`Unspecified`) reference types may be assigned to
    // `NonNull` (`T!`) targets, whereas `Null` and explicitly `Nullable` (`T?`) are rejected.
    if from.nullability() == Nullability::Unspecified
        && to.nullability() == Nullability::NonNull
        && !matches!(from, Ty::Null | Ty::Unknown | Ty::Void)
    {
        let from_non_null = from.clone().with_nullability(Nullability::NonNull);
        if is_subtype(db, &from_non_null, to) {
            return true;
        }
    }
    // Boxing conversion followed by optional widening reference conversion.
    if from.is_primitive() {
        return is_subtype(db, &from.boxed(), to);
    }
    // Unboxing conversion followed by optional widening primitive conversion.
    if let Ty::Class {
        fqn,
        type_args,
        nullability,
    } = from
        && type_args.is_empty()
        && *nullability != Nullability::Nullable
        && let Some(p_from) = PrimitiveType::from_boxed_fqn(fqn)
        && let Ty::Primitive(p_to) = to
    {
        return p_from == *p_to || p_from.widens_to(*p_to);
    }
    false
}

/// Computes the least upper bound (LUB) of `a` and `b` for ternary and switch expression branches.
#[must_use]
pub fn least_upper_bound(db: &dyn DefDatabase, a: &Ty, b: &Ty) -> Ty {
    least_upper_bound_inner(db, a, b, 0)
}

fn least_upper_bound_inner(db: &dyn DefDatabase, a: &Ty, b: &Ty, depth: usize) -> Ty {
    if depth >= MAX_SUBST_DEPTH {
        return Ty::object();
    }
    if matches!(a, Ty::Unknown | Ty::Void) || matches!(b, Ty::Unknown | Ty::Void) {
        return Ty::Unknown;
    }
    if a == b {
        return a.clone();
    }

    if matches!(a, Ty::Null) {
        return match b {
            Ty::Primitive(p) => Ty::raw_class(p.boxed_fqn()),
            other if other.nullability() == Nullability::NonNull => {
                other.clone().with_nullability(Nullability::Nullable)
            }
            other => other.clone(),
        };
    }
    if matches!(b, Ty::Null) {
        return match a {
            Ty::Primitive(p) => Ty::raw_class(p.boxed_fqn()),
            other if other.nullability() == Nullability::NonNull => {
                other.clone().with_nullability(Nullability::Nullable)
            }
            other => other.clone(),
        };
    }

    // Primitive & boxed-primitive ternary promotion (JLS §15.25).
    match (a, b) {
        (Ty::Primitive(pa), Ty::Primitive(pb)) => {
            if pa == pb {
                return Ty::Primitive(*pa);
            }
            if let Some(promoted) = binary_numeric_promotion(a, b) {
                return Ty::Primitive(promoted);
            }
            // Mixed boolean & numeric: box both and find reference LUB.
            let ba = pa.boxed_fqn();
            let bb = pb.boxed_fqn();
            return least_upper_bound_inner(db, &Ty::raw_class(ba), &Ty::raw_class(bb), depth + 1);
        }
        (Ty::Primitive(p), Ty::Class { fqn, type_args, .. })
        | (Ty::Class { fqn, type_args, .. }, Ty::Primitive(p)) => {
            if type_args.is_empty()
                && let Some(pb) = PrimitiveType::from_boxed_fqn(fqn)
            {
                if *p == pb {
                    return Ty::Primitive(*p);
                }
                if let Some(promoted) = binary_numeric_promotion(a, b) {
                    return Ty::Primitive(promoted);
                }
            }
            let boxed = Ty::raw_class(p.boxed_fqn());
            let other = if a.is_primitive() { b } else { a };
            return least_upper_bound_inner(db, &boxed, other, depth + 1);
        }
        _ => {}
    }

    if is_subtype(db, a, b) {
        return b.clone();
    }
    if is_subtype(db, b, a) {
        return a.clone();
    }

    let merged_null = lub_nullability(a.nullability(), b.nullability());

    match (a, b) {
        (Ty::Array { elem: ea, .. }, Ty::Array { elem: eb, .. }) => {
            if ea.is_primitive() || eb.is_primitive() {
                Ty::object().with_nullability(merged_null)
            } else {
                Ty::array(least_upper_bound_inner(db, ea, eb, depth + 1))
                    .with_nullability(merged_null)
            }
        }
        (
            Ty::Class {
                fqn: fa,
                type_args: args_a,
                ..
            },
            Ty::Class {
                fqn: fb,
                type_args: args_b,
                ..
            },
        ) => {
            let mut b_supers: FxHashSet<String> = FxHashSet::default();
            b_supers.insert(fb.clone());
            for s in linearized_supertypes(db, fb) {
                b_supers.insert(s);
            }

            let mut a_chain = Vec::with_capacity(1);
            a_chain.push(fa.clone());
            a_chain.extend(linearized_supertypes(db, fa));

            for common_fqn in a_chain {
                if !b_supers.contains(&common_fqn) {
                    continue;
                }
                let params = type_params_of_fqn(db, &common_fqn);
                if params.is_empty() {
                    return Ty::raw_class(common_fqn).with_nullability(merged_null);
                }
                let Some(subst_a) = supertype_substitution(db, fa, args_a, &common_fqn) else {
                    return Ty::raw_class(common_fqn).with_nullability(merged_null);
                };
                let Some(subst_b) = supertype_substitution(db, fb, args_b, &common_fqn) else {
                    return Ty::raw_class(common_fqn).with_nullability(merged_null);
                };
                if subst_a.map.is_empty() || subst_b.map.is_empty() {
                    return Ty::raw_class(common_fqn).with_nullability(merged_null);
                }
                let mut merged_args = Vec::with_capacity(params.len());
                for p in &params {
                    let (Some(arg_a), Some(arg_b)) = (subst_a.map.get(p), subst_b.map.get(p))
                    else {
                        return Ty::raw_class(common_fqn).with_nullability(merged_null);
                    };
                    merged_args.push(lub_type_arg(db, arg_a, arg_b, depth + 1));
                }
                return Ty::class(common_fqn, merged_args).with_nullability(merged_null);
            }

            Ty::object().with_nullability(merged_null)
        }
        (
            Ty::TypeVar {
                bounds,
                nullability,
                ..
            },
            other,
        )
        | (
            other,
            Ty::TypeVar {
                bounds,
                nullability,
                ..
            },
        ) => {
            let upper = bounds
                .first()
                .cloned()
                .unwrap_or_else(Ty::object)
                .with_nullability(*nullability);
            least_upper_bound_inner(db, &upper, other, depth + 1)
        }
        _ => Ty::object().with_nullability(merged_null),
    }
}

fn lub_type_arg(db: &dyn DefDatabase, a: &TypeArg, b: &TypeArg, depth: usize) -> TypeArg {
    if a == b {
        return a.clone();
    }
    match (a, b) {
        (TypeArg::Type(ta), TypeArg::Type(tb)) => {
            let l = least_upper_bound_inner(db, ta, tb, depth + 1);
            if l == Ty::object() || matches!(l, Ty::Unknown) {
                TypeArg::Wildcard(WildcardBound::Unbounded)
            } else {
                TypeArg::Wildcard(WildcardBound::Extends(Box::new(l)))
            }
        }
        (
            TypeArg::Wildcard(WildcardBound::Super(la)),
            TypeArg::Wildcard(WildcardBound::Super(lb)),
        ) if la == lb => TypeArg::Wildcard(WildcardBound::Super(la.clone())),
        _ => {
            let ua = type_arg_upper(a);
            let ub = type_arg_upper(b);
            match (ua, ub) {
                (Some(ta), Some(tb)) => {
                    let l = least_upper_bound_inner(db, ta, tb, depth + 1);
                    if l == Ty::object() || matches!(l, Ty::Unknown) {
                        TypeArg::Wildcard(WildcardBound::Unbounded)
                    } else {
                        TypeArg::Wildcard(WildcardBound::Extends(Box::new(l)))
                    }
                }
                _ => TypeArg::Wildcard(WildcardBound::Unbounded),
            }
        }
    }
}

fn type_arg_upper(arg: &TypeArg) -> Option<&Ty> {
    match arg {
        TypeArg::Type(t) => Some(t),
        TypeArg::Wildcard(WildcardBound::Extends(u)) => Some(u),
        TypeArg::Wildcard(WildcardBound::Super(_) | WildcardBound::Unbounded) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        RootDatabase,
        classpath::{BinaryOrigin, BinaryTypeSymbol, ClasspathIndex},
        symbols::TypeKind,
        test_fixture::{db_with, db_with_files},
    };

    use super::*;

    fn cls(fqn: &str) -> Ty {
        Ty::raw_class(fqn)
    }

    fn cls_with(fqn: &str, type_args: Vec<TypeArg>) -> Ty {
        Ty::class(fqn, type_args)
    }

    fn bin_type(
        fqn: &str,
        kind: TypeKind,
        type_params: &[&str],
        super_class: Option<&str>,
        super_class_generic: Option<&str>,
        interfaces: &[&str],
        interfaces_generic: &[&str],
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
                TypeIdentity::Unspecified
            } else {
                TypeIdentity::Identity
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
            interfaces_generic: interfaces_generic
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            permitted_subclasses: Vec::new(),
            members: Vec::new(),
            nested_types: Vec::new(),
            origin: BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base")),
            },
        }
    }

    fn collection_classpath() -> ClasspathIndex {
        let mut cp = ClasspathIndex::default();
        cp.insert_type(bin_type(
            "java.lang.Object",
            TypeKind::Class,
            &[],
            None,
            None,
            &[],
            &[],
        ));
        cp.insert_type(bin_type(
            "java.lang.Number",
            TypeKind::Class,
            &[],
            Some("java.lang.Object"),
            None,
            &["java.io.Serializable"],
            &[],
        ));
        cp.insert_type(bin_type(
            "java.lang.Integer",
            TypeKind::Class,
            &[],
            Some("java.lang.Number"),
            None,
            &["java.lang.Comparable"],
            &["java.lang.Comparable<java.lang.Integer>"],
        ));
        cp.insert_type(bin_type(
            "java.lang.Double",
            TypeKind::Class,
            &[],
            Some("java.lang.Number"),
            None,
            &["java.lang.Comparable"],
            &["java.lang.Comparable<java.lang.Double>"],
        ));
        cp.insert_type(bin_type(
            "java.lang.String",
            TypeKind::Class,
            &[],
            Some("java.lang.Object"),
            None,
            &["java.io.Serializable", "java.lang.Comparable", "java.lang.CharSequence"],
            &[
                "java.io.Serializable",
                "java.lang.Comparable<java.lang.String>",
                "java.lang.CharSequence",
            ],
        ));
        cp.insert_type(bin_type(
            "java.lang.CharSequence",
            TypeKind::Interface,
            &[],
            None,
            None,
            &[],
            &[],
        ));
        cp.insert_type(bin_type(
            "java.lang.Comparable",
            TypeKind::Interface,
            &["T"],
            None,
            None,
            &[],
            &[],
        ));
        cp.insert_type(bin_type(
            "java.io.Serializable",
            TypeKind::Interface,
            &[],
            None,
            None,
            &[],
            &[],
        ));
        cp.insert_type(bin_type(
            "java.lang.Iterable",
            TypeKind::Interface,
            &["T"],
            None,
            None,
            &[],
            &[],
        ));
        cp.insert_type(bin_type(
            "java.util.Collection",
            TypeKind::Interface,
            &["E"],
            None,
            None,
            &["java.lang.Iterable"],
            &["java.lang.Iterable<E>"],
        ));
        cp.insert_type(bin_type(
            "java.util.List",
            TypeKind::Interface,
            &["E"],
            None,
            None,
            &["java.util.Collection"],
            &["java.util.Collection<E>"],
        ));
        cp.insert_type(bin_type(
            "java.util.AbstractList",
            TypeKind::Class,
            &["E"],
            Some("java.lang.Object"),
            Some("java.lang.Object"),
            &["java.util.List"],
            &["java.util.List<E>"],
        ));
        cp.insert_type(bin_type(
            "java.util.ArrayList",
            TypeKind::Class,
            &["E"],
            Some("java.util.AbstractList"),
            Some("java.util.AbstractList<E>"),
            &["java.util.List"],
            &["java.util.List<E>"],
        ));
        cp.insert_type(bin_type(
            "java.util.LinkedList",
            TypeKind::Class,
            &["E"],
            Some("java.util.AbstractList"),
            Some("java.util.AbstractList<E>"),
            &["java.util.List"],
            &["java.util.List<E>"],
        ));
        cp.finalize();
        cp
    }

    #[test]
    fn parse_type_ref_handles_primitives_arrays_varargs_nested_wildcards_and_annotations() {
        let mut resolver = |segs: &[&str]| -> Option<Ty> {
            match segs {
                ["String"] => Some(Ty::string()),
                ["User"] => Some(cls("com.example.User")),
                ["Number"] => Some(cls("java.lang.Number")),
                ["List"] => Some(cls("java.util.List")),
                ["Map", "Entry"] => Some(cls("java.util.Map.Entry")),
                ["java", "util", "List"] => Some(cls("java.util.List")),
                ["com", "example", "User"] => Some(cls("com.example.User")),
                ["K"] => Some(Ty::type_var("K", Vec::new())),
                ["V"] => Some(Ty::type_var("V", Vec::new())),
                _ => None,
            }
        };

        assert_eq!(
            parse_type_ref("int", &mut resolver),
            Ty::Primitive(PrimitiveType::Int)
        );
        assert_eq!(
            parse_type_ref("boolean", &mut resolver),
            Ty::Primitive(PrimitiveType::Boolean)
        );
        assert_eq!(parse_type_ref("void", &mut resolver), Ty::Void);
        assert_eq!(parse_type_ref("var", &mut resolver), Ty::Unknown);
        assert_eq!(
            parse_type_ref("int[][]", &mut resolver),
            Ty::array(Ty::array(Ty::Primitive(PrimitiveType::Int)))
        );
        assert_eq!(
            parse_type_ref("@Nullable String...", &mut resolver),
            Ty::array(Ty::string())
        );
        assert_eq!(
            parse_type_ref("@com.ann.Foo(bar = \"a(b)\") int @NotNull []", &mut resolver),
            Ty::array(Ty::Primitive(PrimitiveType::Int))
        );
        assert_eq!(
            parse_type_ref("java.util.List<com.example.User>", &mut resolver),
            cls_with("java.util.List", vec![TypeArg::Type(cls("com.example.User"))])
        );
        assert_eq!(
            parse_type_ref("Map.Entry<K, ? extends V>", &mut resolver),
            cls_with(
                "java.util.Map.Entry",
                vec![
                    TypeArg::Type(Ty::type_var("K", Vec::new())),
                    TypeArg::Wildcard(WildcardBound::Extends(Box::new(Ty::type_var(
                        "V",
                        Vec::new(),
                    )))),
                ]
            )
        );
        assert_eq!(
            parse_type_ref("List<?>", &mut resolver),
            cls_with(
                "java.util.List",
                vec![TypeArg::Wildcard(WildcardBound::Unbounded)]
            )
        );
        assert_eq!(
            parse_type_ref("List<? super User>", &mut resolver),
            cls_with(
                "java.util.List",
                vec![TypeArg::Wildcard(WildcardBound::Super(Box::new(cls(
                    "com.example.User"
                ))))]
            )
        );
        assert_eq!(parse_type_ref("MissingType", &mut resolver), Ty::Unknown);
    }

    #[test]
    fn boxing_widening_and_numeric_promotion() {
        assert_eq!(PrimitiveType::Int.boxed_fqn(), "java.lang.Integer");
        assert_eq!(
            PrimitiveType::from_boxed_fqn("java.lang.Character"),
            Some(PrimitiveType::Char)
        );
        assert!(PrimitiveType::Byte.widens_to(PrimitiveType::Short));
        assert!(PrimitiveType::Byte.widens_to(PrimitiveType::Double));
        assert!(PrimitiveType::Char.widens_to(PrimitiveType::Int));
        assert!(!PrimitiveType::Char.widens_to(PrimitiveType::Short));
        assert!(!PrimitiveType::Int.widens_to(PrimitiveType::Int));
        assert!(!PrimitiveType::Boolean.widens_to(PrimitiveType::Int));
        assert!(PrimitiveType::Char.is_integral());
        assert!(!PrimitiveType::Double.is_integral());
        assert!(!PrimitiveType::Boolean.is_numeric());

        assert_eq!(
            unary_numeric_promotion(&Ty::Primitive(PrimitiveType::Byte)),
            Some(PrimitiveType::Int)
        );
        assert_eq!(
            unary_numeric_promotion(&cls("java.lang.Character")),
            Some(PrimitiveType::Int)
        );
        assert_eq!(
            unary_numeric_promotion(&cls("java.lang.Double")),
            Some(PrimitiveType::Double)
        );
        assert_eq!(
            unary_numeric_promotion(&Ty::Primitive(PrimitiveType::Boolean)),
            None
        );

        assert_eq!(
            binary_numeric_promotion(
                &Ty::Primitive(PrimitiveType::Int),
                &cls("java.lang.Long")
            ),
            Some(PrimitiveType::Long)
        );
        assert_eq!(
            binary_numeric_promotion(
                &cls("java.lang.Float"),
                &Ty::Primitive(PrimitiveType::Double)
            ),
            Some(PrimitiveType::Double)
        );
        assert_eq!(
            binary_numeric_promotion(
                &Ty::Primitive(PrimitiveType::Short),
                &Ty::Primitive(PrimitiveType::Byte)
            ),
            Some(PrimitiveType::Int)
        );
    }

    #[test]
    fn substitution_apply_covariant_and_cycle_depth_safety() {
        let user = cls("com.example.User");
        let e = Ty::type_var("E", Vec::new());

        let mut subst_ext = Substitution::default();
        subst_ext.map.insert(
            SmolStr::new("E"),
            TypeArg::Wildcard(WildcardBound::Extends(Box::new(user.clone()))),
        );
        assert_eq!(subst_ext.apply_covariant(&e), user);
        assert_eq!(
            subst_ext.apply_covariant(&cls_with("java.util.List", vec![TypeArg::Type(e.clone())])),
            cls_with(
                "java.util.List",
                vec![TypeArg::Wildcard(WildcardBound::Extends(Box::new(
                    user.clone()
                )))]
            )
        );

        let mut subst_super = Substitution::default();
        subst_super.map.insert(
            SmolStr::new("E"),
            TypeArg::Wildcard(WildcardBound::Super(Box::new(user.clone()))),
        );
        assert_eq!(subst_super.apply_covariant(&e), Ty::object());

        let mut subst_unbounded = Substitution::default();
        subst_unbounded.map.insert(
            SmolStr::new("E"),
            TypeArg::Wildcard(WildcardBound::Unbounded),
        );
        assert_eq!(subst_unbounded.apply_covariant(&e), Ty::object());

        // Composition: {E -> T} composed with {T -> User} yields {E -> User, T -> User}.
        let mut inner = Substitution::default();
        inner.insert_ty(SmolStr::new("E"), Ty::type_var("T", Vec::new()));
        let mut outer = Substitution::default();
        outer.insert_ty(SmolStr::new("T"), user.clone());
        let composed = inner.compose(&outer);
        assert_eq!(composed.apply(&e), user);

        // Deeply nested type stops cleanly at MAX_SUBST_DEPTH without panicking.
        let mut deep = e;
        for _ in 0..(MAX_SUBST_DEPTH + 10) {
            deep = Ty::array(deep);
        }
        let _ = outer.apply(&deep);
    }

    #[test]
    fn supertype_substitution_across_workspace_source_hierarchy() {
        let src = "\
package com.example;

class User {}
class BaseList<E> {}
class MyList<T> extends BaseList<T> {}
class UserList extends MyList<User> {}
";
        let (db, _) = db_with(src);
        let subst =
            supertype_substitution(&db, "com.example.UserList", &[], "com.example.BaseList")
                .expect("should resolve BaseList substitution");
        assert_eq!(
            subst.map.get("E"),
            Some(&TypeArg::Type(cls("com.example.User")))
        );
    }

    #[test]
    fn supertype_substitution_across_mixed_source_and_binary_hierarchy() {
        let user_src = "\
package com.example;
import java.util.ArrayList;

public class User {}
public class UserList extends ArrayList<User> {}
";
        let mut db = RootDatabase::default();
        let fid = FileId(1);
        db.set_file_text(fid, Arc::new(user_src.to_string()));
        db.set_workspace_files(Arc::new(vec![fid]));
        db.set_classpath_index(Arc::new(collection_classpath()));

        let list_subst =
            supertype_substitution(&db, "com.example.UserList", &[], "java.util.List")
                .expect("should resolve java.util.List");
        assert_eq!(
            list_subst.map.get("E"),
            Some(&TypeArg::Type(cls("com.example.User")))
        );

        let iter_subst =
            supertype_substitution(&db, "com.example.UserList", &[], "java.lang.Iterable")
                .expect("should resolve java.lang.Iterable");
        assert_eq!(
            iter_subst.map.get("T"),
            Some(&TypeArg::Type(cls("com.example.User")))
        );
    }

    #[test]
    fn subtyping_assignability_and_least_upper_bound() {
        let dog_src = "\
package p;
class Animal {}
class Dog extends Animal {}
class Cat extends Animal {}
";
        let mut db = db_with_files(&[(1, dog_src)]);
        db.set_classpath_index(Arc::new(collection_classpath()));

        let animal = cls("p.Animal");
        let dog = cls("p.Dog");
        let cat = cls("p.Cat");
        let num = cls("java.lang.Number");
        let int_box = cls("java.lang.Integer");
        let dbl_box = cls("java.lang.Double");

        assert!(is_subtype(&db, &dog, &animal));
        assert!(!is_subtype(&db, &animal, &dog));
        assert!(is_subtype(&db, &Ty::Null, &dog));
        assert!(!is_subtype(&db, &Ty::Null, &Ty::Primitive(PrimitiveType::Int)));
        assert!(!is_subtype(&db, &Ty::Unknown, &Ty::object()));

        // Array covariance and array superinterfaces.
        let dog_arr = Ty::array(dog.clone());
        let animal_arr = Ty::array(animal.clone());
        assert!(is_subtype(&db, &dog_arr, &animal_arr));
        assert!(is_subtype(&db, &dog_arr, &cls("java.lang.Cloneable")));
        assert!(is_subtype(&db, &dog_arr, &cls("java.io.Serializable")));
        assert!(!is_subtype(
            &db,
            &Ty::array(Ty::Primitive(PrimitiveType::Int)),
            &Ty::array(Ty::Primitive(PrimitiveType::Long))
        ));

        // Generics invariance, wildcards, and raw compatibility.
        let list_dog = cls_with("java.util.List", vec![TypeArg::Type(dog.clone())]);
        let arraylist_dog = cls_with("java.util.ArrayList", vec![TypeArg::Type(dog.clone())]);
        let list_animal = cls_with("java.util.List", vec![TypeArg::Type(animal.clone())]);
        let list_ext_animal = cls_with(
            "java.util.List",
            vec![TypeArg::Wildcard(WildcardBound::Extends(Box::new(
                animal.clone(),
            )))],
        );
        let list_sup_dog = cls_with(
            "java.util.List",
            vec![TypeArg::Wildcard(WildcardBound::Super(Box::new(
                dog.clone(),
            )))],
        );
        let list_q = cls_with(
            "java.util.List",
            vec![TypeArg::Wildcard(WildcardBound::Unbounded)],
        );
        assert!(is_subtype(&db, &arraylist_dog, &list_dog));
        assert!(!is_subtype(&db, &list_dog, &list_animal));
        assert!(is_subtype(&db, &arraylist_dog, &list_ext_animal));
        assert!(is_subtype(&db, &list_animal, &list_sup_dog));
        assert!(is_subtype(&db, &list_dog, &list_q));
        assert!(is_subtype(&db, &cls("java.util.ArrayList"), &list_dog));

        // Boxing / unboxing assignability.
        assert!(!is_subtype(&db, &Ty::Primitive(PrimitiveType::Int), &num));
        assert!(is_assignable(&db, &Ty::Primitive(PrimitiveType::Int), &int_box));
        assert!(is_assignable(&db, &Ty::Primitive(PrimitiveType::Int), &num));
        assert!(is_assignable(&db, &int_box, &Ty::Primitive(PrimitiveType::Int)));
        assert!(is_assignable(&db, &int_box, &Ty::Primitive(PrimitiveType::Double)));

        // TypeVar and Intersection subtyping.
        let tv = Ty::type_var("T", vec![num.clone()]);
        assert!(is_subtype(&db, &tv, &num));
        assert!(is_subtype(&db, &tv, &Ty::object()));
        let inter = Ty::Intersection(vec![animal.clone(), cls("java.io.Serializable")]);
        assert!(is_subtype(&db, &inter, &animal));
        assert!(!is_subtype(&db, &dog, &inter));

        // LUB.
        assert_eq!(least_upper_bound(&db, &dog, &cat), animal);
        assert_eq!(least_upper_bound(&db, &Ty::Null, &dog), dog);
        assert_eq!(
            least_upper_bound(
                &db,
                &Ty::Primitive(PrimitiveType::Int),
                &Ty::Primitive(PrimitiveType::Double)
            ),
            Ty::Primitive(PrimitiveType::Double)
        );
        assert_eq!(
            least_upper_bound(
                &db,
                &cls_with("java.util.ArrayList", vec![TypeArg::Type(int_box)]),
                &cls_with("java.util.LinkedList", vec![TypeArg::Type(dbl_box)]),
            ),
            cls_with(
                "java.util.AbstractList",
                vec![TypeArg::Wildcard(WildcardBound::Extends(Box::new(num)))]
            )
        );
    }

    #[test]
    fn recursive_bounds_and_cyclic_inheritance_do_not_loop() {
        let src = "\
package cyc;
class A<T extends A<T>> {}
class Cyc1 extends Cyc2 {}
class Cyc2 extends Cyc1 {}
";
        let (db, fid) = db_with(src);
        let bounds = type_param_bounds_of_fqn(&db, "cyc.A");
        let ty = resolve_type_ref_in_file(&db, fid, "T", &bounds);
        assert_eq!(
            ty,
            Ty::type_var(
                "T",
                vec![cls_with(
                    "cyc.A",
                    vec![TypeArg::Type(Ty::type_var("T", Vec::new()))]
                )],
            )
        );

        assert_eq!(
            supertype_substitution(&db, "cyc.Cyc1", &[], "java.lang.String"),
            None
        );
        assert!(!is_subtype(&db, &cls("cyc.Cyc1"), &cls("java.lang.String")));
    }

    #[test]
    fn formatting_short_and_qualified() {
        let map_entry = cls_with(
            "java.util.Map.Entry",
            vec![
                TypeArg::Type(Ty::string()),
                TypeArg::Wildcard(WildcardBound::Extends(Box::new(cls("java.lang.Number")))),
            ],
        );
        assert_eq!(map_entry.display_short(), "Map.Entry<String, ? extends Number>");
        assert_eq!(
            map_entry.display_qualified(),
            "java.util.Map.Entry<java.lang.String, ? extends java.lang.Number>"
        );

        let arr = Ty::array(Ty::Primitive(PrimitiveType::Int));
        assert_eq!(arr.display_short(), "int[]");
        assert_eq!(arr.display_qualified(), "int[]");
        assert_eq!(Ty::object().display_short(), "Object");
        assert_eq!(Ty::object().display_qualified(), "java.lang.Object");
        assert_eq!(
            TypeArg::Wildcard(WildcardBound::Unbounded).display_short(),
            "?"
        );

        assert_eq!(map_entry.erased_fqn(), Some("java.util.Map.Entry"));
        assert_eq!(arr.erased_fqn(), None);
        assert_eq!(
            Ty::type_var("T", vec![cls("java.lang.Number")]).erased_fqn(),
            Some("java.lang.Number")
        );
    }

    #[test]
    fn resolve_binary_and_file_type_refs_and_inner_type_params() {
        let mut scope = BTreeMap::new();
        scope.insert(SmolStr::new("T"), vec!["Comparable<T>".to_string()]);
        let bin_ty = resolve_binary_type_ref("java.util.Map.Entry<String, T>", &scope);
        assert_eq!(
            bin_ty,
            cls_with(
                "java.util.Map.Entry",
                vec![
                    TypeArg::Type(Ty::string()),
                    TypeArg::Type(Ty::type_var(
                        "T",
                        vec![cls_with(
                            "java.lang.Comparable",
                            vec![TypeArg::Type(Ty::type_var("T", Vec::new()))]
                        )],
                    )),
                ]
            )
        );
        assert_eq!(
            resolve_binary_type_ref("Character.Subset", &BTreeMap::new()),
            cls("java.lang.Character.Subset")
        );

        let src = "\
package app;
import java.util.List;
class Outer<K> {
    class Inner<V> {}
    static class StaticNested<S> {}
}
class Child extends java.util.ArrayList<String> {}
";
        let (db, fid) = db_with(src);
        assert_eq!(
            type_params_of_fqn(&db, "app.Outer"),
            vec![SmolStr::new("K")]
        );
        let inner_bounds = type_param_bounds_of_fqn(&db, "app.Outer.Inner");
        assert!(inner_bounds.contains_key("K"));
        assert!(inner_bounds.contains_key("V"));
        let static_bounds = type_param_bounds_of_fqn(&db, "app.Outer.StaticNested");
        assert!(!static_bounds.contains_key("K"));
        assert!(static_bounds.contains_key("S"));

        let resolved = resolve_type_ref_in_file(&db, fid, "List<? extends String>", &BTreeMap::new());
        assert_eq!(
            resolved,
            cls_with(
                "java.util.List",
                vec![TypeArg::Wildcard(WildcardBound::Extends(Box::new(
                    Ty::string()
                )))]
            )
        );
    }

    #[test]
    fn valhalla_nullability_parsing_and_formatting() {
        let mut resolver = |segs: &[&str]| -> Option<Ty> {
            match segs {
                ["Point"] => Some(cls("com.example.Point")),
                ["String"] => Some(Ty::string()),
                ["Number"] => Some(cls("java.lang.Number")),
                ["List"] => Some(cls("java.util.List")),
                ["Map"] => Some(cls("java.util.Map")),
                ["T"] => Some(Ty::type_var("T", Vec::new())),
                _ => None,
            }
        };

        let point_nn = parse_type_ref("Point!", &mut resolver);
        assert_eq!(point_nn.nullability(), Nullability::NonNull);
        assert!(!point_nn.is_nullable());
        assert_eq!(point_nn.display_short(), "Point!");
        assert_eq!(point_nn.display_qualified(), "com.example.Point!");

        let point_q = parse_type_ref("Point?", &mut resolver);
        assert_eq!(point_q.nullability(), Nullability::Nullable);
        assert!(point_q.is_nullable());
        assert_eq!(point_q.display_short(), "Point?");
        assert_eq!(point_q.display_qualified(), "com.example.Point?");

        let list_point_nn = parse_type_ref("List<Point!>", &mut resolver);
        assert_eq!(list_point_nn.display_short(), "List<Point!>");
        assert_eq!(
            list_point_nn.display_qualified(),
            "java.util.List<com.example.Point!>"
        );

        let map_ty = parse_type_ref("Map<String!, ? extends Number>", &mut resolver);
        assert_eq!(map_ty.display_short(), "Map<String!, ? extends Number>");
        assert_eq!(
            map_ty.display_qualified(),
            "java.util.Map<java.lang.String!, ? extends java.lang.Number>"
        );

        let str_arr_nn = parse_type_ref("String![]!", &mut resolver);
        assert_eq!(str_arr_nn.nullability(), Nullability::NonNull);
        assert!(!str_arr_nn.is_nullable());
        assert_eq!(str_arr_nn.display_short(), "String![]!");
        assert_eq!(str_arr_nn.display_qualified(), "java.lang.String![]!");

        let tvar_nn = parse_type_ref("T!", &mut resolver);
        assert_eq!(tvar_nn.nullability(), Nullability::NonNull);
        assert!(!tvar_nn.is_nullable());
        assert_eq!(tvar_nn.display_short(), "T!");

        // Primitives are always non-null and unaffected by `with_nullability`.
        let prim = Ty::Primitive(PrimitiveType::Int);
        assert_eq!(prim.nullability(), Nullability::NonNull);
        assert!(!prim.is_nullable());
        assert!(prim.is_primitive());
        assert_eq!(prim.clone().with_nullability(Nullability::Nullable), prim);
        assert_eq!(
            prim.boxed(),
            Ty::raw_class("java.lang.Integer").with_nullability(Nullability::NonNull)
        );
    }

    #[test]
    fn valhalla_null_restricted_subtyping_assignability_and_lub() {
        let src = "\
package p;
class Point {}
";
        let mut db = db_with_files(&[(1, src)]);
        db.set_classpath_index(Arc::new(collection_classpath()));

        let point = cls("p.Point");
        let point_nn = point.clone().with_nullability(Nullability::NonNull);
        let point_q = point.clone().with_nullability(Nullability::Nullable);
        let tvar_nn = Ty::type_var("T", Vec::new()).with_nullability(Nullability::NonNull);
        let str_arr_nn = Ty::array(Ty::string()).with_nullability(Nullability::NonNull);

        // `null` is NOT a subtype of non-null types or primitives.
        assert!(!is_subtype(&db, &Ty::Null, &point_nn));
        assert!(!is_subtype(&db, &Ty::Null, &tvar_nn));
        assert!(!is_subtype(&db, &Ty::Null, &str_arr_nn));
        assert!(!is_subtype(&db, &Ty::Null, &Ty::Primitive(PrimitiveType::Int)));
        assert!(is_subtype(&db, &Ty::Null, &point_q));
        assert!(is_subtype(&db, &Ty::Null, &point));

        // Nullability subtyping lattice.
        assert!(is_subtype(&db, &point_nn, &point_q));
        assert!(is_subtype(&db, &point_nn, &point));
        assert!(is_subtype(&db, &point_q, &point));
        assert!(is_subtype(&db, &point, &point_q));
        assert!(!is_subtype(&db, &point_q, &point_nn));
        assert!(!is_subtype(&db, &point, &point_nn));

        // Resilient assignability: `Unspecified -> NonNull` allowed; `Null`/`Nullable -> NonNull` rejected.
        assert!(is_assignable(&db, &point, &point_nn));
        assert!(!is_assignable(&db, &point_q, &point_nn));
        assert!(!is_assignable(&db, &Ty::Null, &point_nn));

        // LUB with Null and mixed nullability.
        assert_eq!(least_upper_bound(&db, &point_nn, &Ty::Null), point_q);
        assert_eq!(least_upper_bound(&db, &Ty::Null, &point_nn), point_q);
        assert_eq!(least_upper_bound(&db, &point_nn, &point_q), point_q);
        assert_eq!(least_upper_bound(&db, &point_nn, &point_nn), point_nn);

        // Array covariance: `Point![] <: Object[]`, while `int[]` stays invariant.
        let point_nn_arr = Ty::array(point_nn);
        let obj_arr = Ty::array(Ty::object());
        let int_arr = Ty::array(Ty::Primitive(PrimitiveType::Int));
        assert!(is_subtype(&db, &point_nn_arr, &obj_arr));
        assert!(!is_subtype(&db, &int_arr, &obj_arr));
    }

    #[test]
    fn valhalla_is_value_type_for_primitives_workspace_and_binary_types() {
        #[salsa::db]
        #[derive(Clone, Default)]
        struct ValueOverrideDb {
            storage: salsa::Storage<Self>,
            inner: RootDatabase,
            value_fqn: String,
        }

        #[salsa::db]
        impl salsa::Database for ValueOverrideDb {}

        impl crate::SourceDatabase for ValueOverrideDb {
            fn file_text(&self, file_id: FileId) -> Arc<String> {
                self.inner.file_text(file_id)
            }
            fn file_language_level(&self, file_id: FileId) -> syntax::LanguageLevel {
                self.inner.file_language_level(file_id)
            }
            fn parse(&self, file_id: FileId) -> syntax::Parse {
                self.inner.parse(file_id)
            }
            fn syntax_tree(&self, file_id: FileId) -> syntax::SyntaxNode {
                self.inner.syntax_tree(file_id)
            }
            fn syntax_errors(&self, file_id: FileId) -> Arc<Vec<syntax::SyntaxError>> {
                self.inner.syntax_errors(file_id)
            }
        }

        impl DefDatabase for ValueOverrideDb {
            fn workspace_files(&self) -> Arc<Vec<FileId>> {
                self.inner.workspace_files()
            }
            fn file_item_tree(&self, file_id: FileId) -> Arc<FileItemTree> {
                let mut tree = (*self.inner.file_item_tree(file_id)).clone();
                for (_, ty) in tree.types.iter_mut() {
                    if ty.qualified_name == self.value_fqn {
                        ty.identity = TypeIdentity::Value;
                    }
                }
                Arc::new(tree)
            }
            fn ast_id_map(&self, file_id: FileId) -> Arc<crate::AstIdMap> {
                self.inner.ast_id_map(file_id)
            }
            fn file_symbols(&self, file_id: FileId) -> Arc<crate::FileSymbols> {
                self.inner.file_symbols(file_id)
            }
            fn module_index(&self, module_id: crate::ModuleId) -> Arc<crate::ModuleIndex> {
                self.inner.module_index(module_id)
            }
            fn workspace_index(&self) -> Arc<WorkspaceIndex> {
                self.inner.workspace_index()
            }
            fn workspace_hierarchy_index(&self) -> Arc<crate::WorkspaceHierarchyIndex> {
                self.inner.workspace_hierarchy_index()
            }
            fn classpath_index(&self) -> Arc<ClasspathIndex> {
                self.inner.classpath_index()
            }
        }

        let src = "\
package p;
class WsValue {}
class WsIdentity {}
abstract class WsAbstract {}
interface WsInterface {}
";
        let mut inner = db_with_files(&[(1, src)]);
        let mut cp = collection_classpath();
        let mut bin_val = bin_type(
            "bin.BinValue",
            TypeKind::Class,
            &[],
            Some("java.lang.Object"),
            None,
            &[],
            &[],
        );
        bin_val.identity = TypeIdentity::Value;
        cp.insert_type(bin_val);
        cp.finalize();
        inner.set_classpath_index(Arc::new(cp));

        let db = ValueOverrideDb {
            storage: salsa::Storage::default(),
            inner,
            value_fqn: "p.WsValue".to_string(),
        };

        assert!(Ty::Primitive(PrimitiveType::Int).is_value_type(&db));
        assert!(cls("p.WsValue").is_value_type(&db));
        assert!(cls("bin.BinValue").is_value_type(&db));
        assert!(!cls("p.WsIdentity").is_value_type(&db));
        assert!(!cls("p.WsAbstract").is_value_type(&db));
        assert!(!cls("p.WsInterface").is_value_type(&db));
        assert!(!cls("java.lang.String").is_value_type(&db));
        assert!(!cls("java.util.List").is_value_type(&db));
        assert!(!Ty::array(cls("p.WsValue")).is_value_type(&db));
    }

    #[test]
    fn valhalla_universal_generic_substitution_preserves_primitive_and_nullability() {
        let tvar_t = Ty::type_var("T", Vec::new());
        let tvar_t_nn = tvar_t.clone().with_nullability(Nullability::NonNull);
        let tvar_t_q = tvar_t.clone().with_nullability(Nullability::Nullable);
        let box_t = cls_with("p.Box", vec![TypeArg::Type(tvar_t.clone())]);

        // Box<int>
        let mut subst_int = Substitution::default();
        subst_int.insert_ty(SmolStr::new("T"), Ty::Primitive(PrimitiveType::Int));
        assert_eq!(subst_int.apply(&tvar_t), Ty::Primitive(PrimitiveType::Int));
        assert_eq!(
            subst_int.apply(&tvar_t_nn),
            Ty::Primitive(PrimitiveType::Int)
        );
        assert_eq!(
            subst_int.apply(&box_t),
            cls_with("p.Box", vec![TypeArg::Type(Ty::Primitive(PrimitiveType::Int))])
        );

        // Box<Point!>
        let point_nn = cls("p.Point").with_nullability(Nullability::NonNull);
        let mut subst_point_nn = Substitution::default();
        subst_point_nn.insert_ty(SmolStr::new("T"), point_nn.clone());
        assert_eq!(subst_point_nn.apply(&tvar_t), point_nn);
        assert_eq!(
            subst_point_nn.apply(&tvar_t_q),
            cls("p.Point").with_nullability(Nullability::Nullable)
        );
        assert_eq!(
            subst_point_nn.apply(&box_t),
            cls_with("p.Box", vec![TypeArg::Type(point_nn.clone())])
        );

        // T! substituted with unspecified `Point` produces `Point!`.
        let mut subst_point = Substitution::default();
        subst_point.insert_ty(SmolStr::new("T"), cls("p.Point"));
        assert_eq!(subst_point.apply(&tvar_t_nn), point_nn);
        assert_eq!(
            subst_point.apply_covariant(&cls_with("p.Box", vec![TypeArg::Type(tvar_t_nn)])),
            cls_with("p.Box", vec![TypeArg::Type(point_nn)])
        );
    }

    #[test]
    fn effective_nullness_scope_nesting_across_module_package_types_and_members() {
        use crate::symbols::{NullabilitySource, NullnessScope};

        let mod_info = "\
import org.jspecify.annotations.NullMarked;
@NullMarked
module com.example.app {}
";
        let unmarked_pkg = "\
@NullUnmarked
package com.example.legacy;
import org.jspecify.annotations.NullUnmarked;
";
        let marked_cls = "\
package com.example.core;
import org.jspecify.annotations.*;

public class InheritedTop {
    public String fromMod;
    @NullUnmarked
    public static class UnmarkedInner {
        public String plain;
        @NullMarked
        public String reMarked() { return \"\"; }
    }
}
";
        let legacy_cls = "\
package com.example.legacy;
import org.jspecify.annotations.*;

public class Legacy {
    public String unmarkedFromPkg;
    @NullMarked
    public static class OptedIn {
        public String nonNull;
    }
}
";
        let db = db_with_files(&[
            (1, mod_info),
            (2, unmarked_pkg),
            (3, marked_cls),
            (4, legacy_cls),
        ]);

        let marked_scope = NullnessScope::NonNullByDefault(NullabilitySource::JSpecify);
        let unmarked_scope = NullnessScope::Unspecified(NullabilitySource::JSpecify);

        assert_eq!(
            effective_nullness_scope_for_file(&db, FileId(3)),
            marked_scope
        );
        assert_eq!(
            effective_nullness_scope_for_file(&db, FileId(4)),
            unmarked_scope
        );

        assert_eq!(
            effective_nullness_scope_for_type(&db, "com.example.core.InheritedTop"),
            marked_scope
        );
        assert_eq!(
            effective_nullness_scope_for_type(&db, "com.example.core.InheritedTop.UnmarkedInner"),
            unmarked_scope
        );
        assert_eq!(
            effective_nullness_scope_for_member(
                &db,
                "com.example.core.InheritedTop.UnmarkedInner",
                marked_scope,
            ),
            marked_scope
        );
        assert_eq!(
            effective_nullness_scope_for_type(&db, "com.example.legacy.Legacy"),
            unmarked_scope
        );
        assert_eq!(
            effective_nullness_scope_for_type(&db, "com.example.legacy.Legacy.OptedIn"),
            marked_scope
        );
    }

    #[test]
    fn jspecify_defaulting_jls_9_7_4_and_typevar_projection() {
        use crate::symbols::{NullabilitySource, NullnessScope};

        let marked = NullnessScope::NonNullByDefault(NullabilitySource::JSpecify);

        // `apply_nullness_scope` with `is_local_root = false` vs `true`.
        let list_str = cls_with("java.util.List", vec![TypeArg::Type(Ty::string())]);
        assert_eq!(
            apply_nullness_scope(list_str.clone(), marked, false).display_short(),
            "List<String!>!"
        );
        assert_eq!(
            apply_nullness_scope(list_str, marked, true).display_short(),
            "List<String!>"
        );

        // JLS §9.7.4 array & nested type placement inside `@NullMarked`.
        let src = "\
package com.example;
import org.jspecify.annotations.*;

@NullMarked
class Outer {
    static class Inner {}
}

@NullMarked
class ArraysAndNested<T, U extends @Nullable Object> {
    @Nullable String[] elemNullableArr;
    String @Nullable [] outerNullableArr;
    Outer.@Nullable Inner nestedNullable;
    Outer.Inner nestedNonNull;
    T parametricT;
    @Nullable T nullableT;
    @NonNull T nonNullT;
    U unboundedNullableBound;
}
";
        let (db, fid) = db_with(src);
        let tree = db.file_item_tree(fid);
        let (_, cls_item) = tree.find_type_by_fqn("com.example.ArraysAndNested").unwrap();
        let tp_bounds = type_param_bounds_of_fqn(&db, "com.example.ArraysAndNested");

        let resolve_member = |idx: usize| {
            let m = &tree.members[cls_item.members[idx]];
            resolve_type_ref_in_file_for_member(
                &db,
                fid,
                Some("com.example.ArraysAndNested"),
                m.nullness_scope,
                m.type_ref.as_deref().unwrap(),
                &tp_bounds,
            )
        };

        assert_eq!(resolve_member(0).display_short(), "String?[]!");
        assert_eq!(resolve_member(1).display_short(), "String![]?");
        assert_eq!(resolve_member(2).display_short(), "Outer.Inner?");
        assert_eq!(resolve_member(3).display_short(), "Outer.Inner!");

        // Parametric TypeVar T: unannotated `T` stays `Unspecified` on the TypeVar itself,
        // with implicit upper bound `Object!`.
        let t_ty = resolve_member(4);
        assert_eq!(t_ty.nullability(), Nullability::Unspecified);
        assert_eq!(t_ty.display_short(), "T");
        assert_eq!(
            t_ty,
            Ty::type_var("T", vec![Ty::object().with_nullability(Nullability::NonNull)])
        );

        let t_nullable = resolve_member(5);
        assert_eq!(t_nullable.nullability(), Nullability::Nullable);
        assert_eq!(t_nullable.display_short(), "T?");

        let t_nonnull = resolve_member(6);
        assert_eq!(t_nonnull.nullability(), Nullability::NonNull);
        assert_eq!(t_nonnull.display_short(), "T!");

        let u_ty = resolve_member(7);
        assert_eq!(
            u_ty,
            Ty::type_var("U", vec![Ty::object().with_nullability(Nullability::Nullable)])
        );

        // Substitution & TypeVar projection.
        let str_q = Ty::string().with_nullability(Nullability::Nullable);
        let str_nn = Ty::string().with_nullability(Nullability::NonNull);
        let mut subst_q = Substitution::default();
        subst_q.insert_ty(SmolStr::new("T"), str_q.clone());
        assert_eq!(subst_q.apply(&t_ty), str_q);
        assert_eq!(subst_q.apply(&t_nonnull), str_nn);

        let mut subst_nn = Substitution::default();
        subst_nn.insert_ty(SmolStr::new("T"), str_nn.clone());
        assert_eq!(subst_nn.apply(&t_ty), str_nn);
        assert_eq!(subst_nn.apply(&t_nullable), str_q);

        // Wildcard `?` covariant projection respects upper bound (`Object!` vs `Object?` vs `Object`).
        let mut subst_wc = Substitution::default();
        subst_wc.map.insert(
            SmolStr::new("T"),
            TypeArg::Wildcard(WildcardBound::Unbounded),
        );
        subst_wc.map.insert(
            SmolStr::new("U"),
            TypeArg::Wildcard(WildcardBound::Unbounded),
        );
        assert_eq!(
            subst_wc.apply_covariant(&t_ty),
            Ty::object().with_nullability(Nullability::NonNull)
        );
        assert_eq!(
            subst_wc.apply_covariant(&u_ty),
            Ty::object().with_nullability(Nullability::Nullable)
        );
        assert_eq!(
            subst_wc.apply_covariant(&t_nullable),
            Ty::object().with_nullability(Nullability::Nullable)
        );
        assert_eq!(
            subst_wc.apply_covariant(&Ty::type_var("T", Vec::new())),
            Ty::object()
        );
    }
}
