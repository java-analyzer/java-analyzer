//! Pure, IO-free symbol extraction and workspace indexing for Java source files.
//! See `docs/specs/0005-symbol-indexing-and-name-resolution.md`.

use std::{collections::BTreeMap, sync::Arc};

use smol_str::SmolStr;
use syntax::{
    SyntaxElement, SyntaxNode, TextRange,
    ext::{collapsed_text, has_direct_token, is_type_node, name_child, path_segments, path_text, type_child},
    syntax_kind::SyntaxKind::{self, *},
};

use crate::{
    FileId,
    item_tree::{FileItemTree, TypeId},
    ty::Nullability,
};

/// Where a nullability constraint or defaulting scope originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NullabilitySource {
    /// `org.jspecify.annotations.{Nullable, NonNull, NullMarked, NullUnmarked}`.
    JSpecify,
    /// Recognized third-party annotations (`jakarta.annotation.*`, `javax.annotation.*`,
    /// `org.jetbrains.annotations.*`, `org.checkerframework.checker.nullness.qual.*`,
    /// `edu.umd.cs.findbugs.annotations.*`, `org.springframework.lang.*`).
    ThirdPartyAnnotation,
    /// Built-in Java language semantics (Java 28+ `T!`/`T?` syntax, primitives, literals, `new`, `this`).
    LanguageSyntax,
    /// Derived via expression inference, LUB, or generic substitution.
    Inferred,
}

/// Lexical nullability defaulting scope on a module, package, type, or member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NullnessScope {
    /// Inherit from enclosing scope (`member → inner type → outer type → package → module`).
    #[default]
    Inherited,
    /// Unannotated type uses in scope default to `Nullability::NonNull` (e.g. `@NullMarked`).
    NonNullByDefault(NullabilitySource),
    /// Explicitly resets scope default to `Nullability::Unspecified` (e.g. `@NullUnmarked`).
    Unspecified(NullabilitySource),
}

/// Classifies a fully qualified annotation name as a recognized nullability annotation.
#[must_use]
pub fn classify_nullability_annotation(fqn: &str) -> Option<(Nullability, NullabilitySource)> {
    match fqn {
        "org.jspecify.annotations.Nullable" => {
            Some((Nullability::Nullable, NullabilitySource::JSpecify))
        }
        "jakarta.annotation.Nullable"
        | "javax.annotation.Nullable"
        | "org.jetbrains.annotations.Nullable"
        | "org.checkerframework.checker.nullness.qual.Nullable"
        | "edu.umd.cs.findbugs.annotations.Nullable"
        | "org.springframework.lang.Nullable" => {
            Some((Nullability::Nullable, NullabilitySource::ThirdPartyAnnotation))
        }
        "org.jspecify.annotations.NonNull" => {
            Some((Nullability::NonNull, NullabilitySource::JSpecify))
        }
        "jakarta.annotation.Nonnull"
        | "javax.annotation.Nonnull"
        | "org.jetbrains.annotations.NotNull"
        | "org.checkerframework.checker.nullness.qual.NonNull"
        | "edu.umd.cs.findbugs.annotations.NonNull"
        | "org.springframework.lang.NonNull" => {
            Some((Nullability::NonNull, NullabilitySource::ThirdPartyAnnotation))
        }
        _ => None,
    }
}

/// Classifies a fully qualified annotation name as a recognized lexical nullness scope annotation.
#[must_use]
pub fn classify_nullness_scope_annotation(fqn: &str) -> Option<NullnessScope> {
    match fqn {
        "org.jspecify.annotations.NullMarked" => {
            Some(NullnessScope::NonNullByDefault(NullabilitySource::JSpecify))
        }
        "org.springframework.lang.NonNullApi" => Some(NullnessScope::NonNullByDefault(
            NullabilitySource::ThirdPartyAnnotation,
        )),
        "org.jspecify.annotations.NullUnmarked" => {
            Some(NullnessScope::Unspecified(NullabilitySource::JSpecify))
        }
        _ => None,
    }
}

/// Returns `true` if `fqn` is a recognized `TYPE_USE` nullability annotation (whose placement
/// before an array type like `@Nullable String[]` annotates the element type `String` per
/// JLS §9.7.4 rather than the outer array).
#[must_use]
pub fn is_type_use_nullability_annotation(fqn: &str) -> bool {
    matches!(
        fqn,
        "org.jspecify.annotations.Nullable"
            | "org.jspecify.annotations.NonNull"
            | "org.checkerframework.checker.nullness.qual.Nullable"
            | "org.checkerframework.checker.nullness.qual.NonNull"
            | "org.jetbrains.annotations.Nullable"
            | "org.jetbrains.annotations.NotNull"
    )
}

/// Extracts the identifier text of a `NAME` or `NAME_REF` node without heap allocation for short
/// names.
pub(crate) fn node_ident_text(node: &SyntaxNode) -> SmolStr {
    if let Some(tok) = node
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia())
    {
        return SmolStr::new(tok.text());
    }
    smol_str::format_smolstr!("{}", node.text())
}

/// Classification of a Java `import` declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportKind {
    /// `import a.b.Foo;`
    SingleType,
    /// `import a.b.*;`
    TypeOnDemand,
    /// `import static a.b.Foo.BAR;`
    StaticSingle,
    /// `import static a.b.Foo.*;`
    StaticOnDemand,
    /// `import module java.base;`
    Module,
}

/// A parsed `import` declaration in a compilation unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub kind: ImportKind,
    /// Dotted segments of the `PATH` node (e.g. `["a", "b", "Foo"]`).
    pub path: Vec<String>,
    pub range: TextRange,
}

/// Object identity semantics of a declared type (JEP 401: Value Classes and Objects).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TypeIdentity {
    /// Standard identity class, enum, or identity record.
    #[default]
    Identity,
    /// Identity-free value class or value record (`value class` / `value record`).
    Value,
    /// Interfaces, annotations, and abstract classes that do not mandate identity or value
    /// semantics and may be extended/implemented by either.
    Unspecified,
}

/// Kind of a declared Java type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
}

impl TypeKind {
    pub(crate) fn from_syntax(kind: SyntaxKind) -> Option<Self> {
        match kind {
            CLASS_DECL => Some(Self::Class),
            INTERFACE_DECL => Some(Self::Interface),
            ENUM_DECL => Some(Self::Enum),
            RECORD_DECL => Some(Self::Record),
            ANNOTATION_TYPE_DECL => Some(Self::Annotation),
            _ => None,
        }
    }
}

/// A top-level or nested type declaration extracted from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeSymbol {
    pub file_id: FileId,
    /// Simple name (e.g. `"Inner"`).
    pub name: SmolStr,
    /// Canonical dot-separated FQN (e.g. `"com.example.Outer.Inner"`, or `"Outer.Inner"` in unnamed package).
    pub qualified_name: String,
    /// Enclosing type's `qualified_name`, if nested.
    pub enclosing_type: Option<String>,
    pub kind: TypeKind,
    pub identity: TypeIdentity,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub nullness_scope: NullnessScope,
    pub type_params: Vec<SmolStr>,
    /// Explicit `extends` bounds per type parameter (omits parameters with no explicit bounds).
    pub type_param_bounds: BTreeMap<SmolStr, Vec<String>>,
    /// Raw type references from `extends` clause (e.g. `["Base", "com.foo.Iface<T>"]`).
    pub extends: Vec<String>,
    /// Raw type references from `implements` clause.
    pub implements: Vec<String>,
    /// Raw type references from `permits` clause (sealed types).
    pub permits: Vec<String>,
    /// Direct members of this type (fields, methods, constructors, enum constants, record components).
    pub members: Vec<MemberSymbol>,
    /// Simple names of directly nested types (resolved via `"{self.qualified_name}.{nested}"`).
    pub nested_types: Vec<SmolStr>,
    pub full_range: TextRange,
    pub selection_range: TextRange,
}

/// Kind of a member inside a type (or compact source file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberKind {
    Field,
    Method,
    Constructor,
    EnumConstant,
    RecordComponent,
}

/// Summary of a single method or constructor parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSummary {
    pub name: Option<SmolStr>,
    pub type_ref: String,
    pub is_varargs: bool,
    /// Declaration-level nullability annotation attached to the parameter's `MODIFIER_LIST`
    /// (or binary parameter annotations) when applicable to the outer parameter type.
    pub decl_nullability: Option<(Nullability, NullabilitySource)>,
}

impl ParamSummary {
    pub fn display(&self) -> String {
        let ty = if self.is_varargs {
            format!("{}...", self.type_ref)
        } else {
            self.type_ref.clone()
        };
        match &self.name {
            Some(name) => format!("{ty} {name}"),
            None => ty,
        }
    }
}

/// A field, method, constructor, enum constant, or record component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSymbol {
    pub file_id: FileId,
    /// Enclosing type's `qualified_name` (`None` only for JEP 512 compact-source top-level members).
    pub owner_type: Option<String>,
    pub name: SmolStr,
    pub kind: MemberKind,
    pub is_static: bool,
    pub is_default: bool,
    pub is_abstract: bool,
    pub has_override: bool,
    pub nullness_scope: NullnessScope,
    pub decl_nullability: Option<(Nullability, NullabilitySource)>,
    /// Type parameter names for generic methods and constructors (e.g. `["T", "U"]`).
    pub type_params: Vec<SmolStr>,
    /// Explicit `extends` bounds per type parameter (omits parameters with no explicit bounds).
    pub type_param_bounds: BTreeMap<SmolStr, Vec<String>>,
    /// Collapsed source text of the declared type (`None` for constructors and enum constants).
    pub type_ref: Option<String>,
    /// Parameter list summary for methods and constructors.
    pub params: Vec<ParamSummary>,
    pub full_range: TextRange,
    pub selection_range: TextRange,
}

pub(crate) fn format_params_signature(params: &[ParamSummary]) -> String {
    let inner: Vec<String> = params.iter().map(ParamSummary::display).collect();
    format!("({})", inner.join(", "))
}

pub(crate) fn format_params_types_signature(params: &[ParamSummary]) -> String {
    let inner: Vec<String> = params
        .iter()
        .map(|p| {
            if p.is_varargs {
                format!("{}...", p.type_ref)
            } else {
                p.type_ref.clone()
            }
        })
        .collect();
    format!("({})", inner.join(", "))
}

impl MemberSymbol {
    /// Formats the parameter list as `"(T a, U... b)"`.
    pub fn params_signature(&self) -> String {
        format_params_signature(&self.params)
    }
}

/// Structural symbols extracted from a single file, omitting method/constructor/initializer bodies
/// so Salsa early-cutoff avoids invalidating workspace queries on body-only edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSymbols {
    pub file_id: FileId,
    /// Declared package name (e.g. `Some("com.example")`), or `None` for the unnamed package.
    pub package: Option<String>,
    pub nullness_scope: NullnessScope,
    pub imports: Vec<ImportDecl>,
    /// All top-level and nested type declarations in this file.
    pub types: Vec<TypeSymbol>,
    /// Top-level fields/methods in JEP 512 compact source files (direct children of `SOURCE_FILE`).
    pub top_level_members: Vec<MemberSymbol>,
}

/// Position-independent symbol lookup index for a single module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleIndex {
    /// Canonical FQN (`"com.example.Outer.Inner"`, or `"Foo"` in unnamed package) -> `(FileId, TypeId)`.
    pub(crate) types_by_fqn: BTreeMap<String, (FileId, TypeId)>,
    /// Package name (`""` for unnamed package, `"com.example"` otherwise) -> top-level type FQNs.
    pub(crate) types_by_package: BTreeMap<String, Vec<String>>,
    /// Package name -> simple name -> top-level type FQN for fast same-package & wildcard lookup.
    pub(crate) package_type_map: BTreeMap<String, BTreeMap<SmolStr, String>>,
    /// Package name -> lexical nullness scope from `package-info.java`.
    pub package_nullness: BTreeMap<String, NullnessScope>,
    /// JPMS module name -> lexical nullness scope from `module-info.java`.
    pub module_nullness: BTreeMap<SmolStr, NullnessScope>,
    /// FileId -> JPMS module name declared in the same workspace module, if any.
    pub(crate) file_modules: BTreeMap<FileId, SmolStr>,
}

impl ModuleIndex {
    /// Returns the indexed `(FileId, TypeId)` for `fqn`, if present.
    pub fn get_type(&self, fqn: &str) -> Option<(FileId, TypeId)> {
        self.types_by_fqn.get(fqn).copied()
    }

    /// Returns `true` if `fqn` is present in this module index.
    pub fn contains_type(&self, fqn: &str) -> bool {
        self.types_by_fqn.contains_key(fqn)
    }

    /// Returns the indexed top-level type FQNs belonging to `pkg` (`""` for the unnamed package).
    pub fn package_types(&self, pkg: &str) -> Option<&[String]> {
        self.types_by_package.get(pkg).map(Vec::as_slice)
    }

    /// Returns the number of distinct indexed packages in this module.
    pub fn package_count(&self) -> usize {
        self.types_by_package.len()
    }

    /// Returns the total number of indexed types in this module.
    pub fn type_count(&self) -> usize {
        self.types_by_fqn.len()
    }

    /// Returns `true` if the module index contains no types.
    pub fn is_empty(&self) -> bool {
        self.types_by_fqn.is_empty()
    }

    /// Returns `true` if `prefix` is a known package or ancestor prefix of any indexed package.
    pub fn has_package_prefix(&self, prefix: &str) -> bool {
        has_package_prefix_in(&self.types_by_package, prefix)
    }

    /// Returns the lexical [`NullnessScope`] declared on `pkg` via `package-info.java`, if any.
    pub fn package_nullness(&self, pkg: &str) -> NullnessScope {
        self.package_nullness
            .get(pkg)
            .copied()
            .unwrap_or(NullnessScope::Inherited)
    }

    /// Returns the lexical [`NullnessScope`] declared on `module` via `module-info.java`, if any.
    pub fn module_nullness(&self, module: &str) -> NullnessScope {
        self.module_nullness
            .get(module)
            .copied()
            .unwrap_or(NullnessScope::Inherited)
    }

    /// Returns the JPMS module name associated with `file_id`, if any.
    pub fn file_module(&self, file_id: FileId) -> Option<&str> {
        self.file_modules.get(&file_id).map(SmolStr::as_str)
    }
}

/// Aggregated deterministic workspace-wide symbol lookup index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceIndex {
    /// Canonical FQN (`"com.example.Outer.Inner"`, or `"Foo"` in unnamed package) -> `(FileId, TypeId)`.
    pub(crate) types_by_fqn: BTreeMap<String, (FileId, TypeId)>,
    /// Package name (`""` for unnamed package, `"com.example"` otherwise) -> top-level type FQNs.
    pub(crate) types_by_package: BTreeMap<String, Vec<String>>,
    /// Package name -> simple name -> top-level type FQN for fast same-package & wildcard lookup.
    pub(crate) package_type_map: BTreeMap<String, BTreeMap<SmolStr, String>>,
    /// Package name -> lexical nullness scope from `package-info.java`.
    pub package_nullness: BTreeMap<String, NullnessScope>,
    /// JPMS module name -> lexical nullness scope from `module-info.java`.
    pub module_nullness: BTreeMap<SmolStr, NullnessScope>,
    /// FileId -> JPMS module name declared in the same workspace module, if any.
    pub(crate) file_modules: BTreeMap<FileId, SmolStr>,
}

impl WorkspaceIndex {
    /// Returns the indexed `(FileId, TypeId)` for `fqn`, if present.
    pub fn get_type(&self, fqn: &str) -> Option<(FileId, TypeId)> {
        self.types_by_fqn.get(fqn).copied()
    }

    /// Returns `true` if `fqn` is present in the workspace index.
    pub fn contains_type(&self, fqn: &str) -> bool {
        self.types_by_fqn.contains_key(fqn)
    }

    /// Returns the indexed top-level type FQNs belonging to `pkg` (`""` for the unnamed package).
    pub fn package_types(&self, pkg: &str) -> Option<&[String]> {
        self.types_by_package.get(pkg).map(Vec::as_slice)
    }

    /// Returns the number of distinct indexed packages.
    pub fn package_count(&self) -> usize {
        self.types_by_package.len()
    }

    /// Returns the total number of indexed types (top-level and nested).
    pub fn type_count(&self) -> usize {
        self.types_by_fqn.len()
    }

    /// Returns `true` if the index contains no types.
    pub fn is_empty(&self) -> bool {
        self.types_by_fqn.is_empty()
    }

    /// Returns `true` if `prefix` is a known package or ancestor prefix of any indexed package.
    pub fn has_package_prefix(&self, prefix: &str) -> bool {
        has_package_prefix_in(&self.types_by_package, prefix)
    }

    /// Returns the lexical [`NullnessScope`] declared on `pkg` via `package-info.java`, if any.
    pub fn package_nullness(&self, pkg: &str) -> NullnessScope {
        self.package_nullness
            .get(pkg)
            .copied()
            .unwrap_or(NullnessScope::Inherited)
    }

    /// Returns the lexical [`NullnessScope`] declared on `module` via `module-info.java`, if any.
    pub fn module_nullness(&self, module: &str) -> NullnessScope {
        self.module_nullness
            .get(module)
            .copied()
            .unwrap_or(NullnessScope::Inherited)
    }

    /// Returns the JPMS module name associated with `file_id`, if any.
    pub fn file_module(&self, file_id: FileId) -> Option<&str> {
        self.file_modules.get(&file_id).map(SmolStr::as_str)
    }
}

fn has_package_prefix_in(types_by_package: &BTreeMap<String, Vec<String>>, prefix: &str) -> bool {
    if prefix.is_empty() {
        return false;
    }
    if types_by_package.contains_key(prefix) {
        return true;
    }
    let prefix_dot = format!("{prefix}.");
    types_by_package
        .range(prefix_dot.clone()..)
        .next()
        .is_some_and(|(pkg, _)| pkg.starts_with(&prefix_dot))
}

/// Builds a deterministic, position-independent [`ModuleIndex`] across a module's [`FileItemTree`]s.
pub(crate) fn build_module_index(files: &[Arc<FileItemTree>]) -> ModuleIndex {
    let mut index = ModuleIndex::default();
    let module_name = files.iter().find_map(|f| f.module_name.clone());

    for file in files {
        let pkg = file.package.as_deref().unwrap_or("");
        if let Some(mod_name) = &module_name {
            index.file_modules.insert(file.file_id, mod_name.clone());
        }
        if file.nullness_scope != NullnessScope::Inherited {
            if let Some(mod_name) = &file.module_name {
                index
                    .module_nullness
                    .insert(mod_name.clone(), file.nullness_scope);
            } else {
                index
                    .package_nullness
                    .insert(pkg.to_string(), file.nullness_scope);
            }
        }
        for (type_id, ty) in file.types.iter() {
            index
                .types_by_fqn
                .insert(ty.qualified_name.clone(), (file.file_id, type_id));
            if ty.enclosing_type.is_none() {
                index
                    .types_by_package
                    .entry(pkg.to_string())
                    .or_default()
                    .push(ty.qualified_name.clone());
                index
                    .package_type_map
                    .entry(pkg.to_string())
                    .or_default()
                    .insert(ty.name.clone(), ty.qualified_name.clone());
            }
        }
    }

    for fqns in index.types_by_package.values_mut() {
        fqns.sort();
        fqns.dedup();
    }

    index
}

/// Merges per-module [`ModuleIndex`]es into a deterministic [`WorkspaceIndex`].
pub(crate) fn merge_module_indexes(modules: &[Arc<ModuleIndex>]) -> WorkspaceIndex {
    let mut index = WorkspaceIndex::default();

    for module in modules {
        for (fqn, &loc) in &module.types_by_fqn {
            index.types_by_fqn.insert(fqn.clone(), loc);
        }
        for (pkg, fqns) in &module.types_by_package {
            index
                .types_by_package
                .entry(pkg.clone())
                .or_default()
                .extend(fqns.iter().cloned());
        }
        for (pkg, map) in &module.package_type_map {
            let entry = index.package_type_map.entry(pkg.clone()).or_default();
            for (name, fqn) in map {
                entry.insert(name.clone(), fqn.clone());
            }
        }
        for (pkg, &scope) in &module.package_nullness {
            index.package_nullness.insert(pkg.clone(), scope);
        }
        for (mod_name, &scope) in &module.module_nullness {
            index.module_nullness.insert(mod_name.clone(), scope);
        }
        for (&fid, mod_name) in &module.file_modules {
            index.file_modules.insert(fid, mod_name.clone());
        }
    }

    for fqns in index.types_by_package.values_mut() {
        fqns.sort();
        fqns.dedup();
    }

    index
}

pub(crate) use crate::item_tree::{
    extract_field_decl, extract_local_type_symbol, extract_method_or_ctor,
};

pub(crate) fn extract_import_parts(node: &SyntaxNode) -> (ImportKind, Vec<String>) {
    let mut has_static = false;
    let mut is_module = false;
    let mut is_wildcard = false;

    for el in node.children_with_tokens() {
        if let Some(tok) = el.into_token() {
            match tok.kind() {
                STATIC_KW => has_static = true,
                IDENT if tok.text() == "module" => is_module = true,
                STAR => is_wildcard = true,
                _ => {}
            }
        }
    }

    let path = node
        .children()
        .find(|n| n.kind() == PATH)
        .map(|p| path_segments(&p))
        .unwrap_or_default();

    let kind = if is_module {
        ImportKind::Module
    } else if has_static && is_wildcard {
        ImportKind::StaticOnDemand
    } else if has_static {
        ImportKind::StaticSingle
    } else if is_wildcard {
        ImportKind::TypeOnDemand
    } else {
        ImportKind::SingleType
    };

    (kind, path)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ImportResolver {
    pub(crate) package: Option<String>,
    pub(crate) single_type_imports: BTreeMap<SmolStr, String>,
    pub(crate) wildcard_packages: Vec<String>,
}

impl ImportResolver {
    fn push_import(&mut self, kind: ImportKind, path: &[String]) {
        match kind {
            ImportKind::SingleType => {
                if let Some(simple) = path.last() {
                    self.single_type_imports
                        .insert(SmolStr::new(simple), path.join("."));
                }
            }
            ImportKind::TypeOnDemand if !path.is_empty() => {
                self.wildcard_packages.push(path.join("."));
            }
            _ => {}
        }
    }

    pub(crate) fn from_imports<'b>(
        package: Option<&str>,
        imports: impl IntoIterator<Item = (ImportKind, &'b [String])>,
    ) -> Self {
        let mut resolver = Self {
            package: package.map(str::to_string),
            single_type_imports: BTreeMap::new(),
            wildcard_packages: Vec::new(),
        };
        for (kind, path) in imports {
            resolver.push_import(kind, path);
        }
        resolver
    }

    pub(crate) fn from_syntax_root(root: &SyntaxNode) -> (Option<String>, Self) {
        let mut package = None;
        let mut imports = Vec::new();
        for child in root.children() {
            match child.kind() {
                PACKAGE_DECL if package.is_none() => {
                    let segs = child
                        .children()
                        .find(|n| n.kind() == PATH)
                        .map(|p| path_segments(&p))
                        .unwrap_or_default();
                    if !segs.is_empty() {
                        package = Some(segs.join("."));
                    }
                }
                IMPORT_DECL => {
                    imports.push(extract_import_parts(&child));
                }
                _ => {}
            }
        }
        let mut resolver = Self {
            package: package.clone(),
            single_type_imports: BTreeMap::new(),
            wildcard_packages: Vec::new(),
        };
        for (kind, path) in &imports {
            resolver.push_import(*kind, path);
        }
        (package, resolver)
    }

    fn resolve_annotation_fqns(&self, raw_path: &str) -> Vec<String> {
        if raw_path.is_empty() {
            return Vec::new();
        }
        if raw_path.contains('.') {
            if let Some((first, rest)) = raw_path.split_once('.')
                && let Some(imported) = self.single_type_imports.get(first)
            {
                return vec![format!("{imported}.{rest}")];
            }
            return vec![raw_path.to_string()];
        }
        if let Some(fqn) = self.single_type_imports.get(raw_path) {
            return vec![fqn.clone()];
        }
        let mut candidates = Vec::new();
        if let Some(pkg) = &self.package {
            candidates.push(format!("{pkg}.{raw_path}"));
        }
        for pkg in &self.wildcard_packages {
            candidates.push(format!("{pkg}.{raw_path}"));
        }
        candidates
    }

    pub(crate) fn resolve_nullability(
        &self,
        ann: &SyntaxNode,
    ) -> Option<(Nullability, NullabilitySource, bool)> {
        let path_node = ann.children().find(|n| n.kind() == PATH)?;
        let raw_path = path_text(&path_node);
        for fqn in self.resolve_annotation_fqns(&raw_path) {
            if let Some((nullability, source)) = classify_nullability_annotation(&fqn) {
                return Some((
                    nullability,
                    source,
                    is_type_use_nullability_annotation(&fqn),
                ));
            }
        }
        None
    }

    pub(crate) fn resolve_nullness_scope(&self, ann: &SyntaxNode) -> Option<NullnessScope> {
        let path_node = ann.children().find(|n| n.kind() == PATH)?;
        let raw_path = path_text(&path_node);
        for fqn in self.resolve_annotation_fqns(&raw_path) {
            if let Some(scope) = classify_nullness_scope_annotation(&fqn) {
                return Some(scope);
            }
        }
        None
    }
}

pub(crate) const fn nullability_suffix(n: Nullability) -> &'static str {
    match n {
        Nullability::Unspecified => "",
        Nullability::NonNull => "!",
        Nullability::Nullable => "?",
    }
}

pub(crate) fn extract_modifier_nullness_scope(
    node: &SyntaxNode,
    resolver: &ImportResolver,
) -> NullnessScope {
    let Some(mods) = node.children().find(|n| n.kind() == MODIFIER_LIST) else {
        return NullnessScope::Inherited;
    };
    let mut result = NullnessScope::Inherited;
    for ann in mods.children().filter(|n| n.kind() == ANNOTATION) {
        if let Some(scope) = resolver.resolve_nullness_scope(&ann) {
            // Prefer JSpecify over third-party if multiple scope annotations appear.
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

pub(crate) fn extract_modifier_nullability(
    node: &SyntaxNode,
    resolver: &ImportResolver,
) -> Option<(Nullability, NullabilitySource, bool)> {
    let mods = node.children().find(|n| n.kind() == MODIFIER_LIST)?;
    let mut best: Option<(Nullability, NullabilitySource, bool)> = None;
    for ann in mods.children().filter(|n| n.kind() == ANNOTATION) {
        if let Some(found @ (_, source, _)) = resolver.resolve_nullability(&ann) {
            match best {
                None => best = Some(found),
                Some((_, NullabilitySource::ThirdPartyAnnotation, _))
                    if source == NullabilitySource::JSpecify =>
                {
                    best = Some(found);
                }
                _ => {}
            }
        }
    }
    best
}

pub(crate) fn render_type_node(node: &SyntaxNode, resolver: &ImportResolver) -> String {
    match node.kind() {
        PRIMITIVE_TYPE => {
            let mut out = String::new();
            for ann in node.children().filter(|n| n.kind() == ANNOTATION) {
                if resolver.resolve_nullability(&ann).is_none() {
                    out.push_str(&collapsed_text(&ann));
                    out.push(' ');
                }
            }
            if let Some(tok) = node
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|t| !t.kind().is_trivia())
            {
                out.push_str(tok.text());
                out
            } else {
                collapsed_text(node)
            }
        }
        PATH_TYPE => render_path_type(node, None, resolver),
        ARRAY_TYPE => render_array_type(node, None, resolver),
        WILDCARD_TYPE => {
            let mut out = String::new();
            for ann in node.children().filter(|n| n.kind() == ANNOTATION) {
                if resolver.resolve_nullability(&ann).is_none() {
                    out.push_str(&collapsed_text(&ann));
                    out.push(' ');
                }
            }
            out.push('?');
            let has_extends = has_direct_token(node, EXTENDS_KW);
            let has_super = has_direct_token(node, SUPER_KW);
            if has_extends || has_super {
                let kw = if has_extends { "extends" } else { "super" };
                if let Some(bound) = type_child(node) {
                    out.push(' ');
                    out.push_str(kw);
                    out.push(' ');
                    out.push_str(&render_type_node(&bound, resolver));
                }
            }
            out
        }
        _ => collapsed_text(node),
    }
}

fn render_path_type(
    node: &SyntaxNode,
    element_fallback_nullability: Option<Nullability>,
    resolver: &ImportResolver,
) -> String {
    let mut out = String::new();
    let mut pending_nullability: Option<Nullability> = None;
    for el in node.children_with_tokens() {
        match el {
            SyntaxElement::Token(tok) => {
                if tok.kind() == DOT {
                    out.push('.');
                    pending_nullability = None;
                }
            }
            SyntaxElement::Node(child) => match child.kind() {
                ANNOTATION => {
                    if let Some((n, _, _)) = resolver.resolve_nullability(&child) {
                        pending_nullability = Some(n);
                    } else {
                        out.push_str(&collapsed_text(&child));
                        out.push(' ');
                    }
                }
                NAME_REF => {
                    out.push_str(&node_ident_text(&child));
                }
                TYPE_ARGS => {
                    let args: Vec<String> = child
                        .children()
                        .filter(|n| is_type_node(n.kind()) || n.kind() == WILDCARD_TYPE)
                        .map(|arg| render_type_node(&arg, resolver))
                        .collect();
                    if !args.is_empty() {
                        out.push('<');
                        out.push_str(&args.join(", "));
                        out.push('>');
                    }
                }
                _ => {}
            },
        }
    }
    if let Some(n) = pending_nullability.or(element_fallback_nullability) {
        out.push_str(nullability_suffix(n));
    }
    out
}

fn render_array_type(
    node: &SyntaxNode,
    element_fallback_nullability: Option<Nullability>,
    resolver: &ImportResolver,
) -> String {
    let Some(elem) = type_child(node) else {
        return collapsed_text(node);
    };
    let mut out = match elem.kind() {
        ARRAY_TYPE => render_array_type(&elem, element_fallback_nullability, resolver),
        PATH_TYPE => render_path_type(&elem, element_fallback_nullability, resolver),
        _ => render_type_node(&elem, resolver),
    };
    let mut dim_nullability: Option<Nullability> = None;
    for ann in node.children().filter(|n| n.kind() == ANNOTATION) {
        if let Some((n, _, _)) = resolver.resolve_nullability(&ann) {
            dim_nullability = Some(n);
        } else {
            out.push(' ');
            out.push_str(&collapsed_text(&ann));
            out.push(' ');
        }
    }
    out.push_str("[]");
    if let Some(n) = dim_nullability {
        out.push_str(nullability_suffix(n));
    }
    out
}

fn innermost_type_is_primitive(mut node: SyntaxNode) -> bool {
    while node.kind() == ARRAY_TYPE {
        let Some(inner) = type_child(&node) else {
            return false;
        };
        node = inner;
    }
    node.kind() == PRIMITIVE_TYPE
}

pub(crate) fn render_type_with_dims_and_nullability(
    base_type: &SyntaxNode,
    dims_owner: &SyntaxNode,
    mod_nullability: Option<(Nullability, NullabilitySource, bool)>,
    is_varargs: bool,
    resolver: &ImportResolver,
) -> (String, Option<(Nullability, NullabilitySource)>) {
    let mut trailing_dims: Vec<Option<Nullability>> = Vec::new();
    let mut varargs_ann: Option<(Nullability, NullabilitySource)> = None;
    let mut pending_ann: Option<(Nullability, NullabilitySource)> = None;
    let mut seen_dot3 = false;

    for el in dims_owner.children_with_tokens() {
        match el {
            SyntaxElement::Node(n) if n.kind() == ANNOTATION => {
                if let Some((nullability, source, _)) = resolver.resolve_nullability(&n) {
                    pending_ann = Some((nullability, source));
                }
            }
            SyntaxElement::Token(tok) if tok.kind() == DOT3 => {
                seen_dot3 = true;
                varargs_ann = pending_ann.take();
            }
            SyntaxElement::Token(tok) if tok.kind() == L_BRACK => {
                trailing_dims.push(pending_ann.take().map(|(n, _)| n));
            }
            SyntaxElement::Token(tok)
                if !tok.kind().is_trivia() && tok.kind() != R_BRACK && !seen_dot3 =>
            {
                pending_ann = None;
            }
            _ => {}
        }
    }

    let has_array_dims = base_type.kind() == ARRAY_TYPE || !trailing_dims.is_empty();
    let elem_is_primitive = innermost_type_is_primitive(base_type.clone());

    let (elem_fallback, outer_fallback, decl_nullability) = if is_varargs {
        match mod_nullability {
            Some((n, _src, true)) => {
                // JLS §9.7.4: `@Nullable String...` annotates the element type `String`.
                let elem = (!elem_is_primitive).then_some(n);
                (elem, None, varargs_ann)
            }
            Some((n, src, false)) => (None, None, Some((n, src))),
            None => (None, None, varargs_ann),
        }
    } else if has_array_dims {
        match mod_nullability {
            Some((n, _src, true)) => {
                // JLS §9.7.4: TYPE_USE `@Nullable String[]` annotates the element `String`, not `[]`.
                let elem = (!elem_is_primitive).then_some(n);
                (elem, None, None)
            }
            Some((n, src, false)) => {
                // Declaration-only `@Nullable String[]` annotates the outer declared array type.
                (None, Some(n), Some((n, src)))
            }
            None => (None, None, None),
        }
    } else {
        match mod_nullability {
            Some((n, src, _is_type_use)) => {
                let elem = (!elem_is_primitive).then_some(n);
                (elem, None, Some((n, src)))
            }
            None => (None, None, None),
        }
    };

    let mut out = match base_type.kind() {
        ARRAY_TYPE => render_array_type(base_type, elem_fallback, resolver),
        PATH_TYPE => render_path_type(base_type, elem_fallback, resolver),
        _ => render_type_node(base_type, resolver),
    };

    for dim_ann in trailing_dims {
        out.push_str("[]");
        if let Some(n) = dim_ann {
            out.push_str(nullability_suffix(n));
        }
    }

    if let Some(outer_n) = outer_fallback
        && !out.ends_with(['!', '?'])
    {
        out.push_str(nullability_suffix(outer_n));
    }

    (out, decl_nullability)
}

pub(crate) fn extract_params(
    param_list: &SyntaxNode,
    resolver: &ImportResolver,
) -> Vec<ParamSummary> {
    param_list
        .children()
        .filter(|n| n.kind() == PARAM)
        .filter_map(|param| {
            let type_node = type_child(&param)?;
            let is_varargs = has_direct_token(&param, DOT3);
            let mod_nullability = extract_modifier_nullability(&param, resolver);
            let (type_ref, decl_nullability) = render_type_with_dims_and_nullability(
                &type_node,
                &param,
                mod_nullability,
                is_varargs,
                resolver,
            );
            let name = name_child(&param).map(|n| node_ident_text(&n));
            Some(ParamSummary {
                name,
                type_ref,
                is_varargs,
                decl_nullability,
            })
        })
        .collect()
}

pub(crate) fn extract_type_params_and_bounds(
    node: &SyntaxNode,
    resolver: &ImportResolver,
) -> (Vec<SmolStr>, BTreeMap<SmolStr, Vec<String>>) {
    let mut names = Vec::new();
    let mut bounds = BTreeMap::new();
    let Some(tps) = node.children().find(|n| n.kind() == TYPE_PARAMS) else {
        return (names, bounds);
    };
    for tp in tps.children().filter(|n| n.kind() == TYPE_PARAM) {
        let Some(name_node) = name_child(&tp) else {
            continue;
        };
        let name = node_ident_text(&name_node);
        let tp_bounds: Vec<String> = tp
            .children()
            .filter(|n| is_type_node(n.kind()))
            .map(|t| render_type_node(&t, resolver))
            .collect();
        if !tp_bounds.is_empty() {
            bounds.insert(name.clone(), tp_bounds);
        }
        names.push(name);
    }
    (names, bounds)
}
