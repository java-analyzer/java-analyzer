//! Pure, IO-free symbol extraction and workspace indexing for Java source files.
//! See `docs/specs/0005-symbol-indexing-and-name-resolution.md`.

use std::{collections::BTreeMap, sync::Arc};

use syntax::{
    syntax_kind::SyntaxKind::{self, *},
    SyntaxNode, TextRange,
};

use crate::FileId;

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

pub type ImportItem = ImportDecl;

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
    pub name: String,
    /// Canonical dot-separated FQN (e.g. `"com.example.Outer.Inner"`, or `"Outer.Inner"` in unnamed package).
    pub qualified_name: String,
    /// Enclosing type's `qualified_name`, if nested.
    pub enclosing_type: Option<String>,
    pub kind: TypeKind,
    pub is_static: bool,
    pub type_params: Vec<String>,
    /// Direct members of this type (fields, methods, constructors, enum constants, record components).
    pub members: Vec<MemberSymbol>,
    /// Simple names of directly nested types (resolved via `"{self.qualified_name}.{nested}"`).
    pub nested_types: Vec<String>,
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
    pub name: Option<String>,
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
    pub name: String,
    pub kind: MemberKind,
    pub is_static: bool,
    /// Collapsed source text of the declared type (`None` for constructors and enum constants).
    pub type_ref: Option<String>,
    /// Parameter list summary for methods and constructors.
    pub params: Vec<ParamSummary>,
    pub full_range: TextRange,
    pub selection_range: TextRange,
}

impl MemberSymbol {
    /// Formats the parameter list as `"(T a, U... b)"`.
    pub fn params_signature(&self) -> String {
        let inner: Vec<String> = self.params.iter().map(ParamSummary::display).collect();
        format!("({})", inner.join(", "))
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

/// Aggregated deterministic workspace-wide symbol lookup index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceIndex {
    /// Canonical FQN (`"com.example.Outer.Inner"`, or `"Foo"` in unnamed package) -> `TypeSymbol`.
    pub types_by_fqn: BTreeMap<String, TypeSymbol>,
    /// Package name (`""` for unnamed package, `"com.example"` otherwise) -> top-level type FQNs.
    pub types_by_package: BTreeMap<String, Vec<String>>,
    /// Package name -> simple name -> top-level type FQN for fast same-package & wildcard lookup.
    pub package_type_map: BTreeMap<String, BTreeMap<String, String>>,
}

impl WorkspaceIndex {
    /// Returns `true` if `prefix` is a known package or ancestor prefix of any indexed package.
    pub fn has_package_prefix(&self, prefix: &str) -> bool {
        if prefix.is_empty() {
            return false;
        }
        if self.types_by_package.contains_key(prefix) {
            return true;
        }
        let prefix_dot = format!("{prefix}.");
        self.types_by_package
            .range(prefix_dot.clone()..)
            .next()
            .is_some_and(|(pkg, _)| pkg.starts_with(&prefix_dot))
    }
}

/// Extracts all package, import, type, and member declarations from `root`, without descending into
/// method, constructor, or initializer bodies.
pub fn extract_file_symbols(file_id: FileId, root: &SyntaxNode) -> FileSymbols {
    let mut package = None;
    let mut imports = Vec::new();
    let mut types = Vec::new();
    let mut top_level_members = Vec::new();

    for child in root.children() {
        match child.kind() {
            PACKAGE_DECL => {
                if package.is_none() {
                    let segs = child
                        .children()
                        .find(|n| n.kind() == PATH)
                        .map(|p| path_segments(&p))
                        .unwrap_or_default();
                    if !segs.is_empty() {
                        package = Some(segs.join("."));
                    }
                }
            }
            IMPORT_DECL => {
                imports.push(extract_import(&child));
            }
            kind if TypeKind::from_syntax(kind).is_some() => {
                extract_type_decl(
                    file_id,
                    &child,
                    package.as_deref(),
                    None,
                    None,
                    &mut types,
                );
            }
            FIELD_DECL => {
                extract_field_decl(file_id, &child, None, None, &mut top_level_members);
            }
            METHOD_DECL => {
                if let Some(member) = extract_method_or_ctor(file_id, &child, None, MemberKind::Method)
                {
                    top_level_members.push(member);
                }
            }
            _ => {}
        }
    }

    FileSymbols {
        file_id,
        package,
        imports,
        types,
        top_level_members,
    }
}

/// Builds a deterministic [`WorkspaceIndex`] across all provided [`FileSymbols`].
pub fn build_workspace_index(files: &[Arc<FileSymbols>]) -> WorkspaceIndex {
    let mut index = WorkspaceIndex::default();

    for file in files {
        let pkg = file.package.as_deref().unwrap_or("");
        for ty in &file.types {
            index
                .types_by_fqn
                .insert(ty.qualified_name.clone(), ty.clone());
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

/// Extracts a standalone [`TypeSymbol`] from a single type declaration node (used for local types
/// inside method/block bodies as well as workspace types).
pub(crate) fn extract_local_type_symbol(file_id: FileId, node: &SyntaxNode) -> Option<TypeSymbol> {
    let mut out = Vec::new();
    extract_type_decl(file_id, node, None, None, None, &mut out);
    out.into_iter().next()
}

fn extract_import(node: &SyntaxNode) -> ImportDecl {
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

    ImportDecl {
        kind,
        path,
        range: node.text_range(),
    }
}

fn extract_type_decl(
    file_id: FileId,
    node: &SyntaxNode,
    package: Option<&str>,
    enclosing_fqn: Option<&str>,
    enclosing_kind: Option<TypeKind>,
    out: &mut Vec<TypeSymbol>,
) -> Option<String> {
    let kind = TypeKind::from_syntax(node.kind())?;
    let name_node = node.children().find(|n| n.kind() == NAME)?;
    let name = name_node.text().to_string();

    let qualified_name = match enclosing_fqn {
        Some(parent) => format!("{parent}.{name}"),
        None => match package {
            Some(pkg) => format!("{pkg}.{name}"),
            None => name.clone(),
        },
    };

    let is_static = has_modifier(node, STATIC_KW)
        || (enclosing_fqn.is_some()
            && (matches!(
                kind,
                TypeKind::Interface | TypeKind::Enum | TypeKind::Record | TypeKind::Annotation
            ) || matches!(
                enclosing_kind,
                Some(TypeKind::Interface | TypeKind::Annotation)
            )));

    let type_params = extract_type_param_names(node);
    let mut members = Vec::new();
    let mut nested_types = Vec::new();
    let mut nested_symbols = Vec::new();

    for part in node.children() {
        match part.kind() {
            RECORD_COMPONENT_LIST => {
                for comp in part.children().filter(|n| n.kind() == RECORD_COMPONENT) {
                    if let Some(comp_name) = comp.children().find(|n| n.kind() == NAME) {
                        let type_ref = comp
                            .children()
                            .find(|n| is_type_node(n.kind()))
                            .map(|t| type_text_with_dims(&t, &comp));
                        members.push(MemberSymbol {
                            file_id,
                            owner_type: Some(qualified_name.clone()),
                            name: comp_name.text().to_string(),
                            kind: MemberKind::RecordComponent,
                            is_static: false,
                            type_ref,
                            params: Vec::new(),
                            full_range: comp.text_range(),
                            selection_range: comp_name.text_range(),
                        });
                    }
                }
            }
            CLASS_BODY | ENUM_BODY => {
                for member_node in part.children() {
                    match member_node.kind() {
                        ENUM_CONSTANT => {
                            if let Some(const_name) =
                                member_node.children().find(|n| n.kind() == NAME)
                            {
                                members.push(MemberSymbol {
                                    file_id,
                                    owner_type: Some(qualified_name.clone()),
                                    name: const_name.text().to_string(),
                                    kind: MemberKind::EnumConstant,
                                    is_static: true,
                                    type_ref: None,
                                    params: Vec::new(),
                                    full_range: member_node.text_range(),
                                    selection_range: const_name.text_range(),
                                });
                            }
                        }
                        FIELD_DECL => {
                            extract_field_decl(
                                file_id,
                                &member_node,
                                Some(&qualified_name),
                                Some(kind),
                                &mut members,
                            );
                        }
                        METHOD_DECL => {
                            if let Some(m) = extract_method_or_ctor(
                                file_id,
                                &member_node,
                                Some(&qualified_name),
                                MemberKind::Method,
                            ) {
                                members.push(m);
                            }
                        }
                        CONSTRUCTOR_DECL => {
                            if let Some(c) = extract_method_or_ctor(
                                file_id,
                                &member_node,
                                Some(&qualified_name),
                                MemberKind::Constructor,
                            ) {
                                members.push(c);
                            }
                        }
                        k if TypeKind::from_syntax(k).is_some() => {
                            if let Some(nested_name) = extract_type_decl(
                                file_id,
                                &member_node,
                                package,
                                Some(&qualified_name),
                                Some(kind),
                                &mut nested_symbols,
                            ) {
                                nested_types.push(nested_name);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    out.push(TypeSymbol {
        file_id,
        name: name.clone(),
        qualified_name,
        enclosing_type: enclosing_fqn.map(str::to_string),
        kind,
        is_static,
        type_params,
        members,
        nested_types,
        full_range: node.text_range(),
        selection_range: name_node.text_range(),
    });
    out.extend(nested_symbols);

    Some(name)
}

pub(crate) fn extract_field_decl(
    file_id: FileId,
    node: &SyntaxNode,
    owner_fqn: Option<&str>,
    owner_kind: Option<TypeKind>,
    out: &mut Vec<MemberSymbol>,
) {
    let base_type = node.children().find(|n| is_type_node(n.kind()));
    let is_static = has_modifier(node, STATIC_KW)
        || matches!(
            owner_kind,
            Some(TypeKind::Interface | TypeKind::Annotation)
        );

    for declarator in node.children().filter(|n| n.kind() == VARIABLE_DECLARATOR) {
        let Some(name_node) = declarator.children().find(|n| n.kind() == NAME) else {
            continue;
        };
        let type_ref = base_type
            .as_ref()
            .map(|t| type_text_with_dims(t, &declarator));
        out.push(MemberSymbol {
            file_id,
            owner_type: owner_fqn.map(str::to_string),
            name: name_node.text().to_string(),
            kind: MemberKind::Field,
            is_static,
            type_ref,
            params: Vec::new(),
            full_range: declarator.text_range(),
            selection_range: name_node.text_range(),
        });
    }
}

pub(crate) fn extract_method_or_ctor(
    file_id: FileId,
    node: &SyntaxNode,
    owner_fqn: Option<&str>,
    kind: MemberKind,
) -> Option<MemberSymbol> {
    let name_node = node.children().find(|n| n.kind() == NAME)?;
    let is_static = kind == MemberKind::Method && has_modifier(node, STATIC_KW);
    let type_ref = if kind == MemberKind::Method {
        node.children()
            .find(|n| is_type_node(n.kind()))
            .map(|t| type_text_with_dims(&t, node))
    } else {
        None
    };

    let params = node
        .children()
        .find(|n| n.kind() == PARAM_LIST)
        .map(|pl| extract_params(&pl))
        .unwrap_or_default();

    Some(MemberSymbol {
        file_id,
        owner_type: owner_fqn.map(str::to_string),
        name: name_node.text().to_string(),
        kind,
        is_static,
        type_ref,
        params,
        full_range: node.text_range(),
        selection_range: name_node.text_range(),
    })
}

fn extract_params(param_list: &SyntaxNode) -> Vec<ParamSummary> {
    param_list
        .children()
        .filter(|n| n.kind() == PARAM)
        .filter_map(|param| {
            let type_node = param.children().find(|n| is_type_node(n.kind()))?;
            let type_ref = type_text_with_dims(&type_node, &param);
            let is_varargs = param
                .children_with_tokens()
                .any(|el| el.into_token().is_some_and(|t| t.kind() == DOT3));
            let name = param
                .children()
                .find(|n| n.kind() == NAME)
                .map(|n| n.text().to_string());
            Some(ParamSummary {
                name,
                type_ref,
                is_varargs,
            })
        })
        .collect()
}

fn extract_type_param_names(node: &SyntaxNode) -> Vec<String> {
    node.children()
        .find(|n| n.kind() == TYPE_PARAMS)
        .map(|tps| {
            tps.children()
                .filter(|n| n.kind() == TYPE_PARAM)
                .filter_map(|tp| {
                    tp.children()
                        .find(|n| n.kind() == NAME)
                        .map(|n| n.text().to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn path_segments(path_node: &SyntaxNode) -> Vec<String> {
    path_node
        .children()
        .filter(|n| n.kind() == NAME_REF)
        .map(|n| n.text().to_string())
        .collect()
}

pub(crate) fn has_modifier(node: &SyntaxNode, kw: SyntaxKind) -> bool {
    node.children()
        .find(|n| n.kind() == MODIFIER_LIST)
        .is_some_and(|mods| {
            mods.children_with_tokens()
                .any(|el| el.into_token().is_some_and(|t| t.kind() == kw))
        })
}

pub(crate) fn is_type_node(kind: SyntaxKind) -> bool {
    matches!(kind, PATH_TYPE | PRIMITIVE_TYPE | ARRAY_TYPE)
}

pub(crate) fn type_text_with_dims(base_type: &SyntaxNode, dims_owner: &SyntaxNode) -> String {
    let mut text = collapsed_text(base_type);
    let extra_dims = dims_owner
        .children_with_tokens()
        .filter(|el| el.as_token().is_some_and(|t| t.kind() == L_BRACK))
        .count();
    for _ in 0..extra_dims {
        text.push_str("[]");
    }
    text
}

/// Renders a node's text with every run of whitespace and comments collapsed into a single space.
pub(crate) fn collapsed_text(node: &SyntaxNode) -> String {
    let mut out = String::new();
    for token in node
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        if token.kind().is_trivia() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push_str(token.text());
        }
    }
    out.trim().to_string()
}
