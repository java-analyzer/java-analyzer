//! Convenience helpers for inspecting and rendering `rowan` Java syntax nodes.

use crate::{
    SyntaxNode,
    syntax_kind::SyntaxKind::{self, *},
};

/// Returns `true` if `kind` is a type syntax node (`PATH_TYPE`, `PRIMITIVE_TYPE`, or `ARRAY_TYPE`).
pub fn is_type_node(kind: SyntaxKind) -> bool {
    matches!(kind, PATH_TYPE | PRIMITIVE_TYPE | ARRAY_TYPE)
}

/// Renders a node's text with every run of whitespace and comments collapsed into a single space.
pub fn collapsed_text(node: &SyntaxNode) -> String {
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

/// Returns the `NAME_REF` segment strings of a `PATH` (or path-like) node.
pub fn path_segments(path_node: &SyntaxNode) -> Vec<String> {
    path_node
        .children()
        .filter(|n| n.kind() == NAME_REF)
        .map(|n| n.text().to_string())
        .collect()
}

/// Concatenates all non-trivia descendant tokens of a `PATH` node (e.g. `"com.example.foo"`).
pub fn path_text(path_node: &SyntaxNode) -> String {
    let mut out = String::new();
    for token in path_node
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        if !token.kind().is_trivia() {
            out.push_str(token.text());
        }
    }
    out
}

/// Returns `true` if `node` has a direct child token of `kind`.
pub fn has_direct_token(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children_with_tokens()
        .any(|el| el.into_token().is_some_and(|t| t.kind() == kind))
}

/// Returns `true` if `node`'s `MODIFIER_LIST` child contains a direct `kw` token.
pub fn has_modifier(node: &SyntaxNode, kw: SyntaxKind) -> bool {
    node.children()
        .find(|n| n.kind() == MODIFIER_LIST)
        .is_some_and(|mods| has_direct_token(&mods, kw))
}

/// Formats `base_type` with any trailing `[]` dimensions declared directly on `dims_owner`.
pub fn type_text_with_dims(base_type: &SyntaxNode, dims_owner: &SyntaxNode) -> String {
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

/// Returns the first `NAME` child of `node`, if any.
pub fn name_child(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.children().find(|n| n.kind() == NAME)
}

/// Returns the first child of `node` whose kind satisfies [`is_type_node`].
pub fn type_child(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.children().find(|n| is_type_node(n.kind()))
}
