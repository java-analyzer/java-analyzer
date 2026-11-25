//! Structured Java type IR, generic substitution, subtyping, and LUB.
//! See `docs/specs/0008-expression-type-inference-and-generics.md` (Section 2).

mod parse;
mod scope;
mod subtype;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use smol_str::SmolStr;

use crate::{DefDatabase, symbols::TypeIdentity};

pub use scope::{
    apply_nullness_scope, effective_nullness_scope_for_file, effective_nullness_scope_for_member,
    effective_nullness_scope_for_type,
};

pub(crate) use parse::parse_type_ref;
pub(crate) use scope::{
    effective_nullness_scope_for_binary_member, effective_nullness_scope_for_member_in_scope,
    effective_nullness_scope_for_type_in_scope, resolve_binary_type_ref_for_member,
    resolve_binary_type_ref_for_owner, resolve_type_ref_in_file_for_member,
    resolve_type_ref_in_file_for_owner, resolve_type_ref_in_file_with_scope_and_visible,
    ty_has_explicit_nullability, type_param_bounds_of_fqn_in_scope, type_params_of_fqn_in_scope,
};
#[cfg(test)]
pub(crate) use scope::{type_param_bounds_of_fqn, type_params_of_fqn};
pub(crate) use subtype::{
    binary_numeric_promotion, is_assignable_in_scope, is_subtype_in_scope,
    least_upper_bound_in_scope, supertype_substitution_for_ty_in_scope,
    supertype_substitution_in_scope, unary_numeric_promotion,
};
#[cfg(test)]
pub(crate) use subtype::{is_assignable, is_subtype, least_upper_bound, supertype_substitution};

#[cfg(test)]
use scope::{resolve_binary_type_ref, resolve_type_ref_in_file};

/// Maximum recursion depth for type substitution, subtyping, and type-variable resolution.
pub(crate) const MAX_SUBST_DEPTH: usize = 32;

pub(super) const OBJECT_FQN: &str = "java.lang.Object";
const STRING_FQN: &str = "java.lang.String";
pub(super) const ENUM_FQN: &str = "java.lang.Enum";
pub(super) const RECORD_FQN: &str = "java.lang.Record";
pub(super) const CLONEABLE_FQN: &str = "java.lang.Cloneable";
pub(super) const SERIALIZABLE_FQN: &str = "java.io.Serializable";
pub(super) const ANNOTATION_FQN: &str = "java.lang.annotation.Annotation";

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

    pub(super) fn is_reference(&self) -> bool {
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
