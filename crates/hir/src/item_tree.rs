//! Position-independent per-file item tree and separate source-range map.
//!
//! `FileItemTree` contains zero [`TextRange`] values so Salsa early-cutoff preserves downstream
//! module and workspace indexes across body edits of any length. Source ranges are looked up
//! lazily via [`AstId`] in the companion [`AstIdMap`].

use la_arena::{Arena, Idx};
use smol_str::SmolStr;
use syntax::{
    ext::{has_modifier, name_child, path_segments, type_child, type_text_with_dims},
    syntax_kind::SyntaxKind::*,
    SyntaxNode, TextRange,
};

use crate::{
    symbols::{
        extract_import_parts, extract_params, extract_type_param_names, format_params_signature,
        node_ident_text, FileSymbols, ImportDecl, ImportKind, MemberKind, MemberSymbol,
        ParamSummary, TypeKind, TypeSymbol,
    },
    FileId,
};

/// Source full and selection byte ranges for a single lowered declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AstRange {
    pub full_range: TextRange,
    pub selection_range: TextRange,
}

/// Stable, traversal-order identifier for a declaration within a file's [`AstIdMap`].
pub type AstId = Idx<AstRange>;

/// Per-file mapping from [`AstId`] to concrete source [`AstRange`]s.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AstIdMap {
    ranges: Arena<AstRange>,
}

impl AstIdMap {
    /// Returns the source [`AstRange`] associated with `id`.
    pub fn range(&self, id: AstId) -> AstRange {
        self.ranges[id]
    }

    /// Finds the [`AstId`] whose selection range matches `range`, if any.
    pub fn find_by_selection(&self, range: TextRange) -> Option<AstId> {
        self.ranges
            .iter()
            .find_map(|(id, r)| (r.selection_range == range).then_some(id))
    }

    /// Finds the [`AstId`] whose full range matches `range`, if any.
    pub fn find_by_full(&self, range: TextRange) -> Option<AstId> {
        self.ranges
            .iter()
            .find_map(|(id, r)| (r.full_range == range).then_some(id))
    }

    fn alloc(&mut self, range: AstRange) -> AstId {
        self.ranges.alloc(range)
    }
}

/// Position-independent representation of a Java `import` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportTree {
    pub kind: ImportKind,
    pub path: Vec<String>,
    pub ast_id: AstId,
}

/// Position-independent representation of a top-level or nested type declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeItem {
    pub file_id: FileId,
    pub name: SmolStr,
    pub qualified_name: String,
    pub enclosing_type: Option<String>,
    pub kind: TypeKind,
    pub is_static: bool,
    pub type_params: Vec<SmolStr>,
    pub members: Vec<MemberId>,
    pub nested_types: Vec<SmolStr>,
    pub ast_id: AstId,
}

/// Position-independent representation of a field, method, constructor, enum constant, or record
/// component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberItem {
    pub file_id: FileId,
    pub owner_type: Option<String>,
    pub name: SmolStr,
    pub kind: MemberKind,
    pub is_static: bool,
    pub type_ref: Option<String>,
    pub params: Vec<ParamSummary>,
    pub ast_id: AstId,
}

impl MemberItem {
    /// Formats the parameter list as `"(T a, U... b)"`.
    pub fn params_signature(&self) -> String {
        format_params_signature(&self.params)
    }
}

pub type TypeId = Idx<TypeItem>;
pub type MemberId = Idx<MemberItem>;
pub type ImportId = Idx<ImportTree>;

/// Position-independent structural item tree for a single Java source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileItemTree {
    pub file_id: FileId,
    pub package: Option<String>,
    pub imports: Arena<ImportTree>,
    pub types: Arena<TypeItem>,
    pub members: Arena<MemberItem>,
    pub top_level_members: Vec<MemberId>,
}

impl FileItemTree {
    /// Finds the [`TypeId`] and [`TypeItem`] whose `ast_id` matches `ast_id`.
    pub fn find_type_by_ast_id(&self, ast_id: AstId) -> Option<(TypeId, &TypeItem)> {
        self.types.iter().find(|(_, ty)| ty.ast_id == ast_id)
    }

    /// Finds the [`MemberId`] and [`MemberItem`] whose `ast_id` matches `ast_id`.
    pub fn find_member_by_ast_id(&self, ast_id: AstId) -> Option<(MemberId, &MemberItem)> {
        self.members.iter().find(|(_, m)| m.ast_id == ast_id)
    }

    /// Finds the [`TypeId`] and [`TypeItem`] with canonical `qualified_name == fqn`.
    pub fn find_type_by_fqn(&self, fqn: &str) -> Option<(TypeId, &TypeItem)> {
        self.types.iter().find(|(_, ty)| ty.qualified_name == fqn)
    }
}

/// Lowers `root` into a position-independent [`FileItemTree`] and its companion [`AstIdMap`]
/// without descending into method, constructor, or initializer bodies.
pub fn lower_file_item_tree(file_id: FileId, root: &SyntaxNode) -> (FileItemTree, AstIdMap) {
    let mut package = None;
    let mut imports = Arena::new();
    let mut types = Arena::new();
    let mut members = Arena::new();
    let mut top_level_members = Vec::new();
    let mut map = AstIdMap::default();

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
                let (kind, path) = extract_import_parts(&child);
                let range = child.text_range();
                let ast_id = map.alloc(AstRange {
                    full_range: range,
                    selection_range: range,
                });
                imports.alloc(ImportTree { kind, path, ast_id });
            }
            kind if TypeKind::from_syntax(kind).is_some() => {
                lower_type_decl(
                    file_id,
                    &child,
                    package.as_deref(),
                    None,
                    None,
                    &mut types,
                    &mut members,
                    &mut map,
                );
            }
            FIELD_DECL => {
                lower_field_decl(
                    file_id,
                    &child,
                    None,
                    None,
                    &mut members,
                    &mut map,
                    &mut top_level_members,
                );
            }
            METHOD_DECL => {
                if let Some(member_id) = lower_method_or_ctor(
                    file_id,
                    &child,
                    None,
                    MemberKind::Method,
                    &mut members,
                    &mut map,
                ) {
                    top_level_members.push(member_id);
                }
            }
            _ => {}
        }
    }

    (
        FileItemTree {
            file_id,
            package,
            imports,
            types,
            members,
            top_level_members,
        },
        map,
    )
}

/// Materializes a legacy [`FileSymbols`] snapshot from a [`FileItemTree`] and [`AstIdMap`].
pub fn file_symbols_from_item_tree(tree: &FileItemTree, map: &AstIdMap) -> FileSymbols {
    let imports = tree
        .imports
        .iter()
        .map(|(_, imp)| ImportDecl {
            kind: imp.kind,
            path: imp.path.clone(),
            range: map.range(imp.ast_id).full_range,
        })
        .collect();

    let types = tree
        .types
        .iter()
        .map(|(_, ty)| {
            let range = map.range(ty.ast_id);
            let members = ty
                .members
                .iter()
                .map(|&id| member_symbol_from_item(&tree.members[id], map))
                .collect();
            TypeSymbol {
                file_id: ty.file_id,
                name: ty.name.clone(),
                qualified_name: ty.qualified_name.clone(),
                enclosing_type: ty.enclosing_type.clone(),
                kind: ty.kind,
                is_static: ty.is_static,
                type_params: ty.type_params.clone(),
                members,
                nested_types: ty.nested_types.clone(),
                full_range: range.full_range,
                selection_range: range.selection_range,
            }
        })
        .collect();

    let top_level_members = tree
        .top_level_members
        .iter()
        .map(|&id| member_symbol_from_item(&tree.members[id], map))
        .collect();

    FileSymbols {
        file_id: tree.file_id,
        package: tree.package.clone(),
        imports,
        types,
        top_level_members,
    }
}

fn member_symbol_from_item(item: &MemberItem, map: &AstIdMap) -> MemberSymbol {
    let range = map.range(item.ast_id);
    MemberSymbol {
        file_id: item.file_id,
        owner_type: item.owner_type.clone(),
        name: item.name.clone(),
        kind: item.kind,
        is_static: item.is_static,
        type_ref: item.type_ref.clone(),
        params: item.params.clone(),
        full_range: range.full_range,
        selection_range: range.selection_range,
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_type_decl(
    file_id: FileId,
    node: &SyntaxNode,
    package: Option<&str>,
    enclosing_fqn: Option<&str>,
    enclosing_kind: Option<TypeKind>,
    types: &mut Arena<TypeItem>,
    members: &mut Arena<MemberItem>,
    map: &mut AstIdMap,
) -> Option<SmolStr> {
    let kind = TypeKind::from_syntax(node.kind())?;
    let name_node = name_child(node)?;
    let name = node_ident_text(&name_node);

    let qualified_name = match enclosing_fqn {
        Some(parent) => format!("{parent}.{name}"),
        None => match package {
            Some(pkg) => format!("{pkg}.{name}"),
            None => name.to_string(),
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
    let ast_id = map.alloc(AstRange {
        full_range: node.text_range(),
        selection_range: name_node.text_range(),
    });
    let type_id = types.alloc(TypeItem {
        file_id,
        name: name.clone(),
        qualified_name: qualified_name.clone(),
        enclosing_type: enclosing_fqn.map(str::to_string),
        kind,
        is_static,
        type_params,
        members: Vec::new(),
        nested_types: Vec::new(),
        ast_id,
    });

    let mut type_members = Vec::new();
    let mut nested_types = Vec::new();

    for part in node.children() {
        match part.kind() {
            RECORD_COMPONENT_LIST => {
                for comp in part.children().filter(|n| n.kind() == RECORD_COMPONENT) {
                    if let Some(comp_name) = name_child(&comp) {
                        let type_ref = type_child(&comp).map(|t| type_text_with_dims(&t, &comp));
                        let comp_ast_id = map.alloc(AstRange {
                            full_range: comp.text_range(),
                            selection_range: comp_name.text_range(),
                        });
                        let member_id = members.alloc(MemberItem {
                            file_id,
                            owner_type: Some(qualified_name.clone()),
                            name: node_ident_text(&comp_name),
                            kind: MemberKind::RecordComponent,
                            is_static: false,
                            type_ref,
                            params: Vec::new(),
                            ast_id: comp_ast_id,
                        });
                        type_members.push(member_id);
                    }
                }
            }
            CLASS_BODY | ENUM_BODY => {
                for member_node in part.children() {
                    match member_node.kind() {
                        ENUM_CONSTANT => {
                            if let Some(const_name) = name_child(&member_node) {
                                let const_ast_id = map.alloc(AstRange {
                                    full_range: member_node.text_range(),
                                    selection_range: const_name.text_range(),
                                });
                                let member_id = members.alloc(MemberItem {
                                    file_id,
                                    owner_type: Some(qualified_name.clone()),
                                    name: node_ident_text(&const_name),
                                    kind: MemberKind::EnumConstant,
                                    is_static: true,
                                    type_ref: None,
                                    params: Vec::new(),
                                    ast_id: const_ast_id,
                                });
                                type_members.push(member_id);
                            }
                        }
                        FIELD_DECL => {
                            lower_field_decl(
                                file_id,
                                &member_node,
                                Some(&qualified_name),
                                Some(kind),
                                members,
                                map,
                                &mut type_members,
                            );
                        }
                        METHOD_DECL => {
                            if let Some(member_id) = lower_method_or_ctor(
                                file_id,
                                &member_node,
                                Some(&qualified_name),
                                MemberKind::Method,
                                members,
                                map,
                            ) {
                                type_members.push(member_id);
                            }
                        }
                        CONSTRUCTOR_DECL => {
                            if let Some(member_id) = lower_method_or_ctor(
                                file_id,
                                &member_node,
                                Some(&qualified_name),
                                MemberKind::Constructor,
                                members,
                                map,
                            ) {
                                type_members.push(member_id);
                            }
                        }
                        k if TypeKind::from_syntax(k).is_some() => {
                            if let Some(nested_name) = lower_type_decl(
                                file_id,
                                &member_node,
                                package,
                                Some(&qualified_name),
                                Some(kind),
                                types,
                                members,
                                map,
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

    types[type_id].members = type_members;
    types[type_id].nested_types = nested_types;

    Some(name)
}

fn lower_field_decl(
    file_id: FileId,
    node: &SyntaxNode,
    owner_fqn: Option<&str>,
    owner_kind: Option<TypeKind>,
    members: &mut Arena<MemberItem>,
    map: &mut AstIdMap,
    out: &mut Vec<MemberId>,
) {
    let base_type = type_child(node);
    let is_static = has_modifier(node, STATIC_KW)
        || matches!(
            owner_kind,
            Some(TypeKind::Interface | TypeKind::Annotation)
        );

    for declarator in node.children().filter(|n| n.kind() == VARIABLE_DECLARATOR) {
        let Some(name_node) = name_child(&declarator) else {
            continue;
        };
        let type_ref = base_type
            .as_ref()
            .map(|t| type_text_with_dims(t, &declarator));
        let ast_id = map.alloc(AstRange {
            full_range: declarator.text_range(),
            selection_range: name_node.text_range(),
        });
        let member_id = members.alloc(MemberItem {
            file_id,
            owner_type: owner_fqn.map(str::to_string),
            name: node_ident_text(&name_node),
            kind: MemberKind::Field,
            is_static,
            type_ref,
            params: Vec::new(),
            ast_id,
        });
        out.push(member_id);
    }
}

fn lower_method_or_ctor(
    file_id: FileId,
    node: &SyntaxNode,
    owner_fqn: Option<&str>,
    kind: MemberKind,
    members: &mut Arena<MemberItem>,
    map: &mut AstIdMap,
) -> Option<MemberId> {
    let name_node = name_child(node)?;
    let is_static = kind == MemberKind::Method && has_modifier(node, STATIC_KW);
    let type_ref = if kind == MemberKind::Method {
        type_child(node).map(|t| type_text_with_dims(&t, node))
    } else {
        None
    };

    let params = node
        .children()
        .find(|n| n.kind() == PARAM_LIST)
        .map(|pl| extract_params(&pl))
        .unwrap_or_default();

    let ast_id = map.alloc(AstRange {
        full_range: node.text_range(),
        selection_range: name_node.text_range(),
    });
    Some(members.alloc(MemberItem {
        file_id,
        owner_type: owner_fqn.map(str::to_string),
        name: node_ident_text(&name_node),
        kind,
        is_static,
        type_ref,
        params,
        ast_id,
    }))
}
