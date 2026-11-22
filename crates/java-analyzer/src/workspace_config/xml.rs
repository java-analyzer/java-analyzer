//! Shared `roxmltree` DOM helpers and `${prop}` expansion for XML build manifests
//! (`pom.xml`, `build.xml`, `ivy.xml`).

use std::collections::BTreeMap;

use roxmltree::{Document, Node, ParsingOptions};

pub(super) fn parse_xml(content: &str) -> Option<Document<'_>> {
    match Document::parse_with_options(
        content,
        ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    ) {
        Ok(doc) => Some(doc),
        Err(err) => {
            tracing::warn!(%err, "failed to parse XML build manifest");
            None
        }
    }
}

pub(super) fn is_tag(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

pub(super) fn has_any_ancestor(node: Node<'_, '_>, names: &[&str]) -> bool {
    let mut cur = node.parent();
    while let Some(parent) = cur {
        if parent.is_element() && names.contains(&parent.tag_name().name()) {
            return true;
        }
        cur = parent.parent();
    }
    false
}

pub(super) fn child_text(node: Node<'_, '_>) -> String {
    let mut out = String::new();
    for child in node.children() {
        if child.is_text() {
            if let Some(t) = child.text() {
                out.push_str(t);
            }
        }
    }
    out.trim().to_string()
}

pub(super) fn direct_child_text(node: Node<'_, '_>, tag: &str) -> Option<String> {
    node.children()
        .find(|child| is_tag(*child, tag))
        .map(child_text)
}

pub(super) fn expand_props(input: &str, props: &BTreeMap<String, String>) -> String {
    let mut result = input.to_string();
    for _ in 0..4 {
        let mut changed = false;
        for (k, v) in props {
            let needle = format!("${{{k}}}");
            if result.contains(&needle) {
                result = result.replace(&needle, v);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    result
}
