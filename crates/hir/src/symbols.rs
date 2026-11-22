//! Pure, IO-free symbol extraction and workspace indexing for Java source files.
//! See `docs/specs/0005-symbol-indexing-and-name-resolution.md`.

use std::{collections::BTreeMap, sync::Arc};

use smol_str::SmolStr;
use syntax::{
    SyntaxNode, TextRange,
    ext::{has_direct_token, name_child, path_segments, type_child, type_text_with_dims},
    syntax_kind::SyntaxKind::{self, *},
};

use crate::{
    FileId,
    item_tree::{FileItemTree, TypeId},
};

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
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub type_params: Vec<SmolStr>,
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

    for file in files {
        let pkg = file.package.as_deref().unwrap_or("");
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

pub(crate) fn extract_params(param_list: &SyntaxNode) -> Vec<ParamSummary> {
    param_list
        .children()
        .filter(|n| n.kind() == PARAM)
        .filter_map(|param| {
            let type_node = type_child(&param)?;
            let type_ref = type_text_with_dims(&type_node, &param);
            let is_varargs = has_direct_token(&param, DOT3);
            let name = name_child(&param).map(|n| node_ident_text(&n));
            Some(ParamSummary {
                name,
                type_ref,
                is_varargs,
            })
        })
        .collect()
}

pub(crate) fn extract_type_param_names(node: &SyntaxNode) -> Vec<SmolStr> {
    node.children()
        .find(|n| n.kind() == TYPE_PARAMS)
        .map(|tps| {
            tps.children()
                .filter(|n| n.kind() == TYPE_PARAM)
                .filter_map(|tp| name_child(&tp).map(|n| node_ident_text(&n)))
                .collect()
        })
        .unwrap_or_default()
}
