//! Position-independent per-file item tree and separate source-range map.
//!
//! `FileItemTree` contains zero [`TextRange`] values so Salsa early-cutoff preserves downstream
//! module and workspace indexes across body edits of any length. Source ranges are looked up
//! lazily via [`AstId`] in the companion [`AstIdMap`].

use std::collections::BTreeMap;

use la_arena::{Arena, Idx};
use smol_str::SmolStr;
use syntax::{
    SyntaxNode, TextRange,
    ext::{
        collapsed_text, has_modifier, is_type_node, name_child, path_segments, path_text,
        type_child, type_text_with_dims,
    },
    syntax_kind::SyntaxKind::{self, *},
};

use crate::{
    FileId,
    symbols::{
        FileSymbols, ImportDecl, ImportKind, MemberKind, MemberSymbol, ParamSummary, TypeIdentity,
        TypeKind, TypeSymbol, extract_import_parts, extract_params, extract_type_params_and_bounds,
        format_params_signature, node_ident_text,
    },
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
    pub identity: TypeIdentity,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub type_params: Vec<SmolStr>,
    /// Explicit `extends` bounds per type parameter (omits parameters with no explicit bounds).
    pub type_param_bounds: BTreeMap<SmolStr, Vec<String>>,
    /// Raw type references from `extends` clause (e.g. `["Base", "com.foo.Iface<T>"]`).
    pub extends: Vec<String>,
    /// Raw type references from `implements` clause.
    pub implements: Vec<String>,
    /// Raw type references from `permits` clause (sealed types).
    pub permits: Vec<String>,
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
    pub is_default: bool,
    pub is_abstract: bool,
    pub has_override: bool,
    pub type_params: Vec<SmolStr>,
    pub type_param_bounds: BTreeMap<SmolStr, Vec<String>>,
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
pub(crate) fn lower_file_item_tree(file_id: FileId, root: &SyntaxNode) -> (FileItemTree, AstIdMap) {
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
pub(crate) fn file_symbols_from_item_tree(tree: &FileItemTree, map: &AstIdMap) -> FileSymbols {
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
        .map(|(_, ty)| type_symbol_from_item(ty, &tree.members, map))
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

pub(crate) fn extract_local_type_symbol(file_id: FileId, node: &SyntaxNode) -> Option<TypeSymbol> {
    let mut types = Arena::new();
    let mut members = Arena::new();
    let mut map = AstIdMap::default();
    lower_type_decl(
        file_id,
        node,
        None,
        None,
        None,
        &mut types,
        &mut members,
        &mut map,
    )?;
    let (_, root_ty) = types.iter().next()?;
    Some(type_symbol_from_item(root_ty, &members, &map))
}

pub(crate) fn extract_field_decl(
    file_id: FileId,
    node: &SyntaxNode,
    owner_fqn: Option<&str>,
    owner_kind: Option<TypeKind>,
    out: &mut Vec<MemberSymbol>,
) {
    let mut members = Arena::new();
    let mut map = AstIdMap::default();
    let mut ids = Vec::new();
    lower_field_decl(
        file_id,
        node,
        owner_fqn,
        owner_kind,
        &mut members,
        &mut map,
        &mut ids,
    );
    out.extend(
        ids.into_iter()
            .map(|id| member_symbol_from_item(&members[id], &map)),
    );
}

pub(crate) fn extract_method_or_ctor(
    file_id: FileId,
    node: &SyntaxNode,
    owner_fqn: Option<&str>,
    kind: MemberKind,
) -> Option<MemberSymbol> {
    let mut members = Arena::new();
    let mut map = AstIdMap::default();
    let id = lower_method_or_ctor(file_id, node, owner_fqn, None, kind, &mut members, &mut map)?;
    Some(member_symbol_from_item(&members[id], &map))
}

fn type_symbol_from_item(ty: &TypeItem, members: &Arena<MemberItem>, map: &AstIdMap) -> TypeSymbol {
    let range = map.range(ty.ast_id);
    let ty_members = ty
        .members
        .iter()
        .map(|&id| member_symbol_from_item(&members[id], map))
        .collect();
    TypeSymbol {
        file_id: ty.file_id,
        name: ty.name.clone(),
        qualified_name: ty.qualified_name.clone(),
        enclosing_type: ty.enclosing_type.clone(),
        kind: ty.kind,
        identity: ty.identity,
        is_static: ty.is_static,
        is_abstract: ty.is_abstract,
        is_final: ty.is_final,
        type_params: ty.type_params.clone(),
        type_param_bounds: ty.type_param_bounds.clone(),
        extends: ty.extends.clone(),
        implements: ty.implements.clone(),
        permits: ty.permits.clone(),
        members: ty_members,
        nested_types: ty.nested_types.clone(),
        full_range: range.full_range,
        selection_range: range.selection_range,
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
        is_default: item.is_default,
        is_abstract: item.is_abstract,
        has_override: item.has_override,
        type_params: item.type_params.clone(),
        type_param_bounds: item.type_param_bounds.clone(),
        type_ref: item.type_ref.clone(),
        params: item.params.clone(),
        full_range: range.full_range,
        selection_range: range.selection_range,
    }
}

fn extract_clause_types(type_node: &SyntaxNode, clause_kind: SyntaxKind) -> Vec<String> {
    type_node
        .children()
        .find(|n| n.kind() == clause_kind)
        .map(|clause| {
            clause
                .children()
                .filter(|n| is_type_node(n.kind()))
                .map(|t| collapsed_text(&t))
                .collect()
        })
        .unwrap_or_default()
}

fn has_override_annotation(node: &SyntaxNode) -> bool {
    let Some(mods) = node.children().find(|n| n.kind() == MODIFIER_LIST) else {
        return false;
    };
    mods.children()
        .filter(|n| n.kind() == ANNOTATION)
        .any(|anno| {
            anno.children()
                .find(|n| n.kind() == PATH)
                .is_some_and(|p| {
                    let text = path_text(&p);
                    text == "Override" || text == "java.lang.Override"
                })
        })
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

    let is_abstract = has_modifier(node, ABSTRACT_KW)
        || matches!(kind, TypeKind::Interface | TypeKind::Annotation);
    let is_final = has_modifier(node, FINAL_KW) || kind == TypeKind::Record;
    let identity = if matches!(kind, TypeKind::Interface | TypeKind::Annotation) || is_abstract {
        TypeIdentity::Unspecified
    } else {
        TypeIdentity::Identity
    };
    let (type_params, type_param_bounds) = extract_type_params_and_bounds(node);
    let extends = extract_clause_types(node, EXTENDS_CLAUSE);
    let implements = extract_clause_types(node, IMPLEMENTS_CLAUSE);
    let permits = extract_clause_types(node, PERMITS_CLAUSE);
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
        identity,
        is_static,
        is_abstract,
        is_final,
        type_params,
        type_param_bounds,
        extends,
        implements,
        permits,
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
                            is_default: false,
                            is_abstract: false,
                            has_override: false,
                            type_params: Vec::new(),
                            type_param_bounds: BTreeMap::new(),
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
                                    is_default: false,
                                    is_abstract: false,
                                    has_override: false,
                                    type_params: Vec::new(),
                                    type_param_bounds: BTreeMap::new(),
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
                                Some(kind),
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
                                Some(kind),
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
        || matches!(owner_kind, Some(TypeKind::Interface | TypeKind::Annotation));

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
            is_default: false,
            is_abstract: false,
            has_override: false,
            type_params: Vec::new(),
            type_param_bounds: BTreeMap::new(),
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
    owner_kind: Option<TypeKind>,
    kind: MemberKind,
    members: &mut Arena<MemberItem>,
    map: &mut AstIdMap,
) -> Option<MemberId> {
    let name_node = name_child(node)?;
    let is_static = kind == MemberKind::Method && has_modifier(node, STATIC_KW);
    let is_default = kind == MemberKind::Method && has_modifier(node, DEFAULT_KW);
    let is_abstract = kind == MemberKind::Method
        && (has_modifier(node, ABSTRACT_KW)
            || (matches!(owner_kind, Some(TypeKind::Interface | TypeKind::Annotation))
                && !is_default
                && !is_static
                && !has_modifier(node, PRIVATE_KW)));
    let has_override = kind == MemberKind::Method && has_override_annotation(node);
    let (type_params, type_param_bounds) = extract_type_params_and_bounds(node);
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
        is_default,
        is_abstract,
        has_override,
        type_params,
        type_param_bounds,
        type_ref,
        params,
        ast_id,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use smol_str::SmolStr;
    use syntax::parse;

    use super::*;

    #[test]
    fn extracts_type_param_bounds_and_generic_member_type_params() {
        let src = "\
package com.example;

public class Box<T extends Number & Comparable<T>, U> {
    public <E extends Exception> Box(E err) {}
    public <K extends CharSequence & java.io.Serializable, V> V pick(K key, V val) {
        return val;
    }
    public int plain() { return 0; }
}

interface Repo<T extends AutoCloseable> {}
record Pair<A extends Comparable<A>, B>(A first, B second) {}
";
        let root = parse(src).syntax_node();
        let (tree, map) = lower_file_item_tree(FileId(1), &root);
        let syms = file_symbols_from_item_tree(&tree, &map);

        let (_, box_item) = tree.find_type_by_fqn("com.example.Box").unwrap();
        assert_eq!(box_item.type_params, vec![SmolStr::new("T"), SmolStr::new("U")]);
        assert_eq!(
            box_item.type_param_bounds,
            BTreeMap::from([(
                SmolStr::new("T"),
                vec!["Number".to_string(), "Comparable<T>".to_string()]
            )])
        );

        let ctor = &tree.members[box_item.members[0]];
        assert_eq!(ctor.kind, MemberKind::Constructor);
        assert_eq!(ctor.type_params, vec![SmolStr::new("E")]);
        assert_eq!(
            ctor.type_param_bounds,
            BTreeMap::from([(SmolStr::new("E"), vec!["Exception".to_string()])])
        );

        let pick = &tree.members[box_item.members[1]];
        assert_eq!(pick.kind, MemberKind::Method);
        assert_eq!(pick.type_params, vec![SmolStr::new("K"), SmolStr::new("V")]);
        assert_eq!(
            pick.type_param_bounds,
            BTreeMap::from([(
                SmolStr::new("K"),
                vec![
                    "CharSequence".to_string(),
                    "java.io.Serializable".to_string()
                ]
            )])
        );

        let plain = &tree.members[box_item.members[2]];
        assert!(plain.type_params.is_empty());
        assert!(plain.type_param_bounds.is_empty());

        let (_, repo_item) = tree.find_type_by_fqn("com.example.Repo").unwrap();
        assert_eq!(
            repo_item.type_param_bounds,
            BTreeMap::from([(SmolStr::new("T"), vec!["AutoCloseable".to_string()])])
        );

        let (_, pair_item) = tree.find_type_by_fqn("com.example.Pair").unwrap();
        assert_eq!(
            pair_item.type_param_bounds,
            BTreeMap::from([(SmolStr::new("A"), vec!["Comparable<A>".to_string()])])
        );

        // Also propagated to legacy TypeSymbol / MemberSymbol.
        let box_sym = &syms.types[0];
        assert_eq!(box_sym.type_param_bounds, box_item.type_param_bounds);
        assert_eq!(box_sym.members[0].type_params, vec![SmolStr::new("E")]);
        assert_eq!(
            box_sym.members[1].type_param_bounds,
            pick.type_param_bounds
        );
    }
}
