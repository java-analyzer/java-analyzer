use hir::{
    BinaryTarget, DefDatabase, FileId, ResolvedSymbol, ResolvedSymbolKind, SmolStr, SymbolDetail,
    TypeKind,
};
use syntax::TextRange;

use crate::SymbolKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationTarget {
    Source {
        file_id: FileId,
        /// Range of the entire declaration.
        full_range: TextRange,
        /// Range of the declaration's identifier (`NAME`), used as cursor destination.
        focus_range: TextRange,
    },
    Binary(BinaryTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeHierarchyNode {
    pub name: SmolStr,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub target: NavigationTarget,
}

fn navigation_target_from_resolved(resolved: ResolvedSymbol) -> Option<NavigationTarget> {
    if let Some((file_id, full_range, focus_range)) = resolved.location() {
        return Some(NavigationTarget::Source {
            file_id,
            full_range,
            focus_range,
        });
    }
    resolved.binary_target.map(NavigationTarget::Binary)
}

fn source_navigation_target(
    db: &dyn DefDatabase,
    file_id: FileId,
    ast_id: hir::AstId,
) -> NavigationTarget {
    let range = db.ast_id_map(file_id).range(ast_id);
    NavigationTarget::Source {
        file_id,
        full_range: range.full_range,
        focus_range: range.selection_range,
    }
}

fn type_kind_to_symbol_kind(kind: TypeKind) -> SymbolKind {
    match kind {
        TypeKind::Class => SymbolKind::Class,
        TypeKind::Interface => SymbolKind::Interface,
        TypeKind::Enum => SymbolKind::Enum,
        TypeKind::Record => SymbolKind::Record,
        TypeKind::Annotation => SymbolKind::Annotation,
    }
}

fn count_params_in_signature(params: &str) -> usize {
    let trimmed = params.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(trimmed)
        .trim();
    if inner.is_empty() {
        return 0;
    }

    let mut count = 1usize;
    let mut angle_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut in_char = false;
    let mut escaped = false;

    for ch in inner.chars() {
        if in_string || in_char {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
            } else if in_string && ch == '"' {
                in_string = false;
            } else if in_char && ch == '\'' {
                in_char = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '\'' => in_char = true,
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            ',' if angle_depth == 0 && paren_depth == 0 && bracket_depth == 0 => {
                count += 1;
            }
            _ => {}
        }
    }

    count
}

fn resolve_type_hierarchy_node(db: &dyn DefDatabase, fqn: &str) -> Option<TypeHierarchyNode> {
    let workspace = db.workspace_index();
    if let Some((file_id, type_id)) = workspace.get_type(fqn) {
        let tree = db.file_item_tree(file_id);
        let ty = &tree.types[type_id];
        return Some(TypeHierarchyNode {
            name: ty.name.clone(),
            qualified_name: ty.qualified_name.clone(),
            kind: type_kind_to_symbol_kind(ty.kind),
            target: source_navigation_target(db, file_id, ty.ast_id),
        });
    }

    let classpath = db.classpath_index();
    let bin = classpath.type_by_fqn(fqn)?;
    Some(TypeHierarchyNode {
        name: bin.name.clone(),
        qualified_name: bin.qualified_name.clone(),
        kind: type_kind_to_symbol_kind(bin.kind),
        target: NavigationTarget::Binary(bin.to_target(None)),
    })
}

fn resolved_to_type_hierarchy_node(resolved: ResolvedSymbol) -> Option<TypeHierarchyNode> {
    let kind = match resolved.kind {
        ResolvedSymbolKind::Class => SymbolKind::Class,
        ResolvedSymbolKind::Interface => SymbolKind::Interface,
        ResolvedSymbolKind::Enum => SymbolKind::Enum,
        ResolvedSymbolKind::Record => SymbolKind::Record,
        ResolvedSymbolKind::Annotation => SymbolKind::Annotation,
        _ => return None,
    };
    let name = resolved.name.clone();
    let qualified_name = resolved.qualified_name.clone();
    let target = navigation_target_from_resolved(resolved)?;
    Some(TypeHierarchyNode {
        name,
        qualified_name,
        kind,
        target,
    })
}

#[must_use]
pub fn goto_definition(
    db: &dyn DefDatabase,
    file_id: FileId,
    offset: u32,
) -> Option<NavigationTarget> {
    let resolved = hir::resolve_at_offset(db, file_id, offset)?;
    navigation_target_from_resolved(resolved)
}

/// Finds workspace implementations of the type or method at `offset` in `file_id`.
#[must_use]
pub fn goto_implementation(
    db: &dyn DefDatabase,
    file_id: FileId,
    offset: u32,
) -> Vec<NavigationTarget> {
    let Some(resolved) = hir::resolve_at_offset(db, file_id, offset) else {
        return Vec::new();
    };

    match resolved.kind {
        ResolvedSymbolKind::Class
        | ResolvedSymbolKind::Interface
        | ResolvedSymbolKind::Enum
        | ResolvedSymbolKind::Record
        | ResolvedSymbolKind::Annotation => {
            let workspace = db.workspace_index();
            hir::all_subtypes(db, &resolved.qualified_name)
                .into_iter()
                .filter_map(|sub_fqn| {
                    let (sub_file_id, type_id) = workspace.get_type(&sub_fqn)?;
                    let tree = db.file_item_tree(sub_file_id);
                    let ty = &tree.types[type_id];
                    Some(source_navigation_target(db, sub_file_id, ty.ast_id))
                })
                .collect()
        }
        ResolvedSymbolKind::Method => {
            let Some((owner_fqn, _)) = resolved.qualified_name.rsplit_once('.') else {
                return Vec::new();
            };
            let param_count = match &resolved.detail {
                Some(SymbolDetail::Method { params, .. }) => count_params_in_signature(params),
                _ => return Vec::new(),
            };
            hir::find_overriding_methods(db, owner_fqn, &resolved.name, param_count)
                .into_iter()
                .map(|(ov_file_id, member_id)| {
                    let tree = db.file_item_tree(ov_file_id);
                    let member = &tree.members[member_id];
                    source_navigation_target(db, ov_file_id, member.ast_id)
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Navigates to the declared type definition of the symbol or expression at `offset` in `file_id`.
#[must_use]
pub fn goto_type_definition(
    db: &dyn DefDatabase,
    file_id: FileId,
    offset: u32,
) -> Option<NavigationTarget> {
    let resolved = hir::resolve_type_of_symbol_at_offset(db, file_id, offset.into())?;
    navigation_target_from_resolved(resolved)
}

/// Prepares a [`TypeHierarchyNode`] for the type declaration or reference at `offset` in `file_id`.
#[must_use]
pub fn prepare_type_hierarchy(
    db: &dyn DefDatabase,
    file_id: FileId,
    offset: u32,
) -> Option<TypeHierarchyNode> {
    hir::resolve_at_offset(db, file_id, offset).and_then(resolved_to_type_hierarchy_node)
}

/// Resolves direct supertypes (`super_class` followed by `interfaces`) of `fqn` across workspace
/// sources and the binary classpath.
#[must_use]
pub fn type_hierarchy_supertypes(db: &dyn DefDatabase, fqn: &str) -> Vec<TypeHierarchyNode> {
    let direct = hir::direct_supertypes(db, fqn);
    let mut out = Vec::new();
    if let Some(sup) = direct.super_class
        && let Some(node) = resolve_type_hierarchy_node(db, &sup)
    {
        out.push(node);
    }
    for iface in direct.interfaces {
        if !out.iter().any(|n| n.qualified_name == iface)
            && let Some(node) = resolve_type_hierarchy_node(db, &iface)
        {
            out.push(node);
        }
    }
    out
}

/// Resolves direct workspace subtypes of `fqn`.
#[must_use]
pub fn type_hierarchy_subtypes(db: &dyn DefDatabase, fqn: &str) -> Vec<TypeHierarchyNode> {
    hir::direct_subtypes(db, fqn)
        .into_iter()
        .filter_map(|sub| resolve_type_hierarchy_node(db, &sub))
        .collect()
}
