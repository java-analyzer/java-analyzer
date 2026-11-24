//! Pure, IO-free binary symbol stub synthesis, source location, and Javadoc formatting.
//! See `docs/specs/0006-jdk-and-jar-classpath-indexing.md`.

use std::{collections::BTreeSet, fmt::Write as _};

use hir::{
    BinaryMemberSymbol, BinaryTarget, BinaryTypeSymbol, ClasspathIndex, MemberKind, ParamSummary,
    SmolStr, TypeKind,
};
use syntax::{
    SyntaxNode, TextRange,
    ext::name_child,
    syntax_kind::SyntaxKind::{self, *},
};

/// Synthesizes a valid, readable `.java` stub file for the top-level compilation unit containing
/// `target`.
///
/// Walks `enclosing_type` up to the top-level type in `index` and renders its package declaration,
/// modifiers, type parameters, `extends`/`implements`/`permits` clauses, record components, enum
/// constants, fields, constructors, methods, and nested types recursively.
#[must_use]
pub fn synthesize_binary_stub(index: &ClasspathIndex, target: &BinaryTarget) -> Option<String> {
    let mut current = index.type_by_fqn(&target.qualified_name)?;
    let mut visited = BTreeSet::new();
    visited.insert(current.qualified_name.as_str());

    let mut path_to_target = vec![current.qualified_name.clone()];
    while let Some(enclosing_fqn) = &current.enclosing_type {
        if !visited.insert(enclosing_fqn.as_str()) {
            break;
        }
        let Some(parent) = index.type_by_fqn(enclosing_fqn) else {
            break;
        };
        current = parent;
        path_to_target.push(current.qualified_name.clone());
    }

    let mut out = String::new();
    if !current.package.is_empty() {
        let _ = writeln!(out, "package {};", current.package);
        out.push('\n');
    }

    render_type_stub(index, current, 0, &path_to_target, &mut out);
    Some(out)
}

fn render_type_stub(
    index: &ClasspathIndex,
    ty: &BinaryTypeSymbol,
    indent_level: usize,
    path_to_target: &[String],
    out: &mut String,
) {
    let indent = "    ".repeat(indent_level);
    let is_nested = indent_level > 0 || ty.enclosing_type.is_some();

    out.push_str(&indent);
    if ty.is_public {
        out.push_str("public ");
    } else if is_nested {
        out.push_str("protected ");
    }

    if is_nested && ty.is_static && ty.kind == TypeKind::Class {
        out.push_str("static ");
    }

    if !ty.permitted_subclasses.is_empty()
        && matches!(ty.kind, TypeKind::Class | TypeKind::Interface)
    {
        out.push_str("sealed ");
    } else if ty.is_final && ty.kind == TypeKind::Class {
        out.push_str("final ");
    }
    if ty.is_abstract && ty.kind == TypeKind::Class {
        out.push_str("abstract ");
    }

    let kw = match ty.kind {
        TypeKind::Class => "class",
        TypeKind::Interface => "interface",
        TypeKind::Enum => "enum",
        TypeKind::Record => "record",
        TypeKind::Annotation => "@interface",
    };
    let _ = write!(out, "{kw} {}", ty.name);

    if !ty.type_params.is_empty() && ty.kind != TypeKind::Enum && ty.kind != TypeKind::Annotation {
        out.push('<');
        for (i, tp) in ty.type_params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(tp);
        }
        out.push('>');
    }

    if ty.kind == TypeKind::Record {
        out.push('(');
        let mut first = true;
        for m in &ty.members {
            if m.kind != MemberKind::RecordComponent {
                continue;
            }
            if !first {
                out.push_str(", ");
            }
            first = false;
            let comp_ty = non_empty_type_or(m.type_ref.as_deref(), "Object");
            let _ = write!(out, "{comp_ty} {}", m.name);
        }
        out.push(')');
    }

    match ty.kind {
        TypeKind::Class => {
            if let Some(sup) = &ty.super_class
                && sup != "java.lang.Object"
            {
                let _ = write!(out, " extends {sup}");
            }
            if !ty.interfaces.is_empty() {
                let _ = write!(out, " implements {}", ty.interfaces.join(", "));
            }
            if !ty.permitted_subclasses.is_empty() {
                let _ = write!(out, " permits {}", ty.permitted_subclasses.join(", "));
            }
        }
        TypeKind::Interface => {
            if !ty.interfaces.is_empty() {
                let _ = write!(out, " extends {}", ty.interfaces.join(", "));
            }
            if !ty.permitted_subclasses.is_empty() {
                let _ = write!(out, " permits {}", ty.permitted_subclasses.join(", "));
            }
        }
        TypeKind::Enum | TypeKind::Record => {
            if !ty.interfaces.is_empty() {
                let _ = write!(out, " implements {}", ty.interfaces.join(", "));
            }
        }
        TypeKind::Annotation => {}
    }

    out.push_str(" {\n");

    let member_indent = "    ".repeat(indent_level + 1);
    render_type_members(ty, &member_indent, out);

    let mut rendered_nested = BTreeSet::new();
    for nested_name in &ty.nested_types {
        let nested_fqn = format!("{}.{nested_name}", ty.qualified_name);
        if let Some(nested_ty) = index.type_by_fqn(&nested_fqn) {
            rendered_nested.insert(nested_fqn);
            out.push('\n');
            render_type_stub(index, nested_ty, indent_level + 1, path_to_target, out);
        }
    }

    // Defensive fallback when `index.finalize()` was not called by a caller/test fixture.
    for candidate_fqn in path_to_target {
        if !rendered_nested.contains(candidate_fqn)
            && let Some(nested_ty) = index.type_by_fqn(candidate_fqn)
            && nested_ty.enclosing_type.as_deref() == Some(ty.qualified_name.as_str())
        {
            rendered_nested.insert(candidate_fqn.clone());
            out.push('\n');
            render_type_stub(index, nested_ty, indent_level + 1, path_to_target, out);
        }
    }

    let _ = writeln!(out, "{indent}}}");
}

fn render_type_members(ty: &BinaryTypeSymbol, member_indent: &str, out: &mut String) {
    if ty.kind == TypeKind::Enum {
        let enum_consts: Vec<&str> = ty
            .members
            .iter()
            .filter(|m| m.kind == MemberKind::EnumConstant)
            .map(|m| m.name.as_str())
            .collect();
        if !enum_consts.is_empty() {
            let _ = writeln!(out, "{member_indent}{};", enum_consts.join(", "));
        }
    }

    for m in &ty.members {
        match m.kind {
            MemberKind::EnumConstant | MemberKind::RecordComponent => {}
            MemberKind::Field => render_field_stub(m, member_indent, out),
            MemberKind::Constructor => render_constructor_stub(ty, m, member_indent, out),
            MemberKind::Method => render_method_stub(ty, m, member_indent, out),
        }
    }
}

fn render_field_stub(member: &BinaryMemberSymbol, indent: &str, out: &mut String) {
    let vis = if member.is_public {
        "public"
    } else {
        "protected"
    };
    let static_kw = if member.is_static { " static" } else { "" };
    let field_ty = non_empty_type_or(member.type_ref.as_deref(), "Object");
    let _ = writeln!(out, "{indent}{vis}{static_kw} {field_ty} {};", member.name);
}

fn render_constructor_stub(
    owner: &BinaryTypeSymbol,
    member: &BinaryMemberSymbol,
    indent: &str,
    out: &mut String,
) {
    let vis = if member.is_public {
        "public"
    } else {
        "protected"
    };
    let params = format_stub_params(&member.params);
    let _ = writeln!(
        out,
        "{indent}{vis} {}({params}) {{ throw new UnsupportedOperationException(); }}",
        owner.name
    );
}

fn render_method_stub(
    owner: &BinaryTypeSymbol,
    member: &BinaryMemberSymbol,
    indent: &str,
    out: &mut String,
) {
    let vis = if member.is_public {
        "public"
    } else {
        "protected"
    };
    let static_kw = if member.is_static { " static" } else { "" };
    let ret_ty = non_empty_type_or(member.type_ref.as_deref(), "void");
    let params = format_stub_params(&member.params);
    let body = if owner.kind == TypeKind::Annotation
        || (owner.kind == TypeKind::Interface && !member.is_static)
    {
        ";"
    } else {
        " { throw new UnsupportedOperationException(); }"
    };
    let _ = writeln!(
        out,
        "{indent}{vis}{static_kw} {ret_ty} {}({params}){body}",
        member.name
    );
}

fn format_stub_params(params: &[ParamSummary]) -> String {
    let mut out = String::new();
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let ty = non_empty_type_or(Some(param.type_ref.as_str()), "Object");
        out.push_str(ty);
        if param.is_varargs {
            out.push_str("...");
        }
        out.push(' ');
        match &param.name {
            Some(name) if !name.is_empty() => out.push_str(name),
            _ => {
                let _ = write!(out, "arg{i}");
            }
        }
    }
    out
}

fn non_empty_type_or<'a>(ty: Option<&'a str>, fallback: &'a str) -> &'a str {
    match ty {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => fallback,
    }
}

/// Locates the `NAME` [`TextRange`] of `target` (type or member) inside `source` using
/// `syntax::parse`.
///
/// Walks nested type declarations matching `target`'s type chain, then matches
/// `target.member_name` (fields, methods, constructors, enum constants, record components),
/// falling back to the enclosing target type's `NAME` range when the member is not found.
#[must_use]
pub fn locate_binary_symbol_in_source(source: &str, target: &BinaryTarget) -> Option<TextRange> {
    let root = syntax::parse(source).syntax_node();
    let type_node = find_target_type_node(&root, target)?;
    let type_name_range = name_child(&type_node).map(|n| n.text_range());

    let Some(member_name) = target.member_name.as_deref() else {
        return type_name_range;
    };

    if let Some((_, named_node)) = find_member_in_type(&type_node, member_name) {
        return name_child(&named_node)
            .map(|n| n.text_range())
            .or(type_name_range);
    }

    type_name_range
}

/// Extracts and formats the leading `/** ... */` Javadoc comment on `target` from `source` into
/// clean Markdown.
#[must_use]
pub fn extract_javadoc_from_source(source: &str, target: &BinaryTarget) -> Option<String> {
    let root = syntax::parse(source).syntax_node();
    let type_node = find_target_type_node(&root, target)?;

    let doc_owner = match target.member_name.as_deref() {
        Some(member_name) => find_member_in_type(&type_node, member_name)?.0,
        None => type_node,
    };

    let raw_comment = find_leading_doc_comment(&doc_owner)?;
    format_source_javadoc(&raw_comment)
}

fn is_type_decl_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        CLASS_DECL | INTERFACE_DECL | ENUM_DECL | RECORD_DECL | ANNOTATION_TYPE_DECL
    )
}

fn node_name_matches(node: &SyntaxNode, expected: &str) -> bool {
    name_child(node).is_some_and(|n| ident_text(&n) == expected)
}

fn ident_text(name_node: &SyntaxNode) -> SmolStr {
    if let Some(tok) = name_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| !t.kind().is_trivia())
    {
        return SmolStr::new(tok.text());
    }
    SmolStr::new(name_node.text().to_string())
}

fn target_type_chain(target: &BinaryTarget) -> Vec<&str> {
    let simple_part = target
        .internal_name
        .rsplit('/')
        .next()
        .unwrap_or(&target.internal_name);
    let chain: Vec<&str> = simple_part.split('$').filter(|s| !s.is_empty()).collect();
    if !chain.is_empty() {
        return chain;
    }
    target
        .qualified_name
        .rsplit('.')
        .next()
        .filter(|s| !s.is_empty())
        .into_iter()
        .collect()
}

fn find_target_type_node(root: &SyntaxNode, target: &BinaryTarget) -> Option<SyntaxNode> {
    let chain = target_type_chain(target);
    if let Some(found) = walk_type_chain(root, &chain) {
        return Some(found);
    }

    // Fallback for top-level types whose Java identifier contains literal `$`.
    let raw_simple = target
        .internal_name
        .rsplit('/')
        .next()
        .unwrap_or(&target.internal_name);
    if !raw_simple.is_empty() && chain.len() > 1 {
        return walk_type_chain(root, &[raw_simple]);
    }

    None
}

fn walk_type_chain(root: &SyntaxNode, chain: &[&str]) -> Option<SyntaxNode> {
    let (&first, rest) = chain.split_first()?;
    let mut current = root
        .children()
        .find(|n| is_type_decl_kind(n.kind()) && node_name_matches(n, first))?;

    for &segment in rest {
        current = current
            .children()
            .filter(|n| matches!(n.kind(), CLASS_BODY | ENUM_BODY))
            .flat_map(|body| body.children())
            .find(|n| is_type_decl_kind(n.kind()) && node_name_matches(n, segment))?;
    }

    Some(current)
}

/// Returns `(doc_owner_node, named_node)` for the first matching member inside `type_node`.
fn find_member_in_type(
    type_node: &SyntaxNode,
    member_name: &str,
) -> Option<(SyntaxNode, SyntaxNode)> {
    for child in type_node.children() {
        match child.kind() {
            RECORD_COMPONENT_LIST => {
                for comp in child.children().filter(|n| n.kind() == RECORD_COMPONENT) {
                    if node_name_matches(&comp, member_name) {
                        return Some((comp.clone(), comp));
                    }
                }
            }
            CLASS_BODY | ENUM_BODY => {
                for member in child.children() {
                    match member.kind() {
                        ENUM_CONSTANT | METHOD_DECL if node_name_matches(&member, member_name) => {
                            return Some((member.clone(), member));
                        }
                        CONSTRUCTOR_DECL
                            if node_name_matches(&member, member_name)
                                || member_name == "<init>" =>
                        {
                            return Some((member.clone(), member));
                        }
                        FIELD_DECL => {
                            for decl in member
                                .children()
                                .filter(|n| n.kind() == VARIABLE_DECLARATOR)
                            {
                                if node_name_matches(&decl, member_name) {
                                    return Some((member.clone(), decl));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn find_leading_doc_comment(node: &SyntaxNode) -> Option<String> {
    let mut cursor = node.prev_sibling_or_token();
    while let Some(el) = cursor {
        let Some(token) = el.as_token() else {
            break;
        };
        match token.kind() {
            WHITESPACE => {
                cursor = el.prev_sibling_or_token();
            }
            BLOCK_COMMENT => {
                let text = token.text();
                if text.starts_with("/**") && text.ends_with("*/") && text.len() > 4 {
                    return Some(text.to_string());
                }
                break;
            }
            _ => break,
        }
    }
    None
}

fn format_source_javadoc(raw_comment: &str) -> Option<String> {
    let inner = raw_comment.strip_prefix("/**")?.strip_suffix("*/")?;

    let mut lines = Vec::new();
    for raw_line in inner.lines() {
        let trimmed_start = raw_line.trim_start();
        let without_star = if let Some(rest) = trimmed_start.strip_prefix('*') {
            rest.strip_prefix(' ').unwrap_or(rest)
        } else {
            trimmed_start
        };
        lines.push(without_star.trim_end());
    }

    let mut body_lines: Vec<String> = Vec::new();
    let mut block_tags: Vec<(String, String)> = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if let Some((tag, rest)) = split_block_tag(trimmed) {
            block_tags.push((tag.to_string(), rest.to_string()));
        } else if let Some((_, last_body)) = block_tags.last_mut() {
            if trimmed.is_empty() {
                if !last_body.ends_with('\n') {
                    last_body.push('\n');
                }
            } else {
                if !last_body.is_empty() && !last_body.ends_with([' ', '\n']) {
                    last_body.push(' ');
                }
                last_body.push_str(trimmed);
            }
        } else {
            body_lines.push(line.to_string());
        }
    }

    let body_raw = body_lines.join("\n");
    let body_expanded = expand_javadoc_inline_tags(&body_raw);
    let body_md = html_fragment_to_markdown(&body_expanded);

    let mut sections = Vec::new();
    if !body_md.is_empty() {
        sections.push(body_md);
    }

    let mut tag_lines = Vec::new();
    for (tag, content) in block_tags {
        let expanded = expand_javadoc_inline_tags(&content);
        let formatted = format_block_tag(&tag, &expanded);
        if !formatted.is_empty() {
            tag_lines.push(formatted);
        }
    }
    if !tag_lines.is_empty() {
        sections.push(tag_lines.join("\n"));
    }

    let result = sections.join("\n\n");
    if result.trim().is_empty() {
        None
    } else {
        Some(result)
    }
}

fn split_block_tag(line: &str) -> Option<(&str, &str)> {
    if !line.starts_with('@') {
        return None;
    }
    let end = line
        .char_indices()
        .skip(1)
        .find(|&(_, c)| c.is_whitespace())
        .map_or(line.len(), |(idx, _)| idx);
    let tag = line.get(..end)?;
    if tag.len() <= 1 {
        return None;
    }
    let rest = line.get(end..)?.trim_start();
    Some((tag, rest))
}

fn format_block_tag(tag: &str, content: &str) -> String {
    let md = html_fragment_to_markdown(content);
    let single_line = collapse_whitespace_preserving_code(&md);
    match tag {
        "@param" => {
            if let Some((param, desc)) = split_first_word(&single_line) {
                if desc.is_empty() {
                    format!("- **@param** `{param}`")
                } else {
                    format!("- **@param** `{param}` — {desc}")
                }
            } else {
                String::new()
            }
        }
        "@throws" | "@exception" => {
            if let Some((exc, desc)) = split_first_word(&single_line) {
                if desc.is_empty() {
                    format!("- **{tag}** `{exc}`")
                } else {
                    format!("- **{tag}** `{exc}` — {desc}")
                }
            } else {
                String::new()
            }
        }
        "@return" => {
            if single_line.is_empty() {
                String::new()
            } else {
                format!("- **@return** {single_line}")
            }
        }
        _ => {
            if single_line.is_empty() {
                format!("- **{tag}**")
            } else {
                format!("- **{tag}** {single_line}")
            }
        }
    }
}

fn split_first_word(s: &str) -> Option<(&str, &str)> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let end = trimmed
        .char_indices()
        .find(|&(_, c)| c.is_whitespace())
        .map_or(trimmed.len(), |(idx, _)| idx);
    let first = trimmed.get(..end)?;
    let rest = trimmed.get(end..)?.trim_start();
    Some((first, rest))
}

fn collapse_whitespace_preserving_code(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_backticks = false;
    let mut prev_space = false;

    for ch in s.chars() {
        if ch == '`' {
            in_backticks = !in_backticks;
            out.push(ch);
            prev_space = false;
        } else if !in_backticks && ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }

    out.trim().to_string()
}

fn expand_javadoc_inline_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;

    while let Some(rest) = input.get(pos..) {
        let Some(rel_start) = rest.find("{@") else {
            out.push_str(rest);
            break;
        };
        if let Some(before) = rest.get(..rel_start) {
            out.push_str(before);
        }
        let tag_start = pos + rel_start;
        let Some(tag_slice) = input.get(tag_start..) else {
            break;
        };
        if let Some((tag_name, body, consumed)) = parse_inline_tag(tag_slice) {
            match tag_name {
                "code" | "value" => {
                    let trimmed = body.trim();
                    if !trimmed.is_empty() {
                        out.push('`');
                        out.push_str(trimmed);
                        out.push('`');
                    }
                }
                "literal" => {
                    out.push_str(body.trim());
                }
                "link" | "linkplain" => {
                    let rendered = format_javadoc_link(body, tag_name == "link");
                    out.push_str(&rendered);
                }
                _ => {
                    let trimmed = body.trim();
                    if !trimmed.is_empty() {
                        out.push_str(trimmed);
                    }
                }
            }
            pos = tag_start + consumed;
        } else {
            out.push_str("{@");
            pos = tag_start + 2;
        }
    }

    out
}

/// Parses `{@tag body}` starting at `s[0..2] == "{@"`, tracking balanced braces.
fn parse_inline_tag(s: &str) -> Option<(&str, &str, usize)> {
    let after_prefix = s.strip_prefix("{@")?;
    let mut name_end = after_prefix.len();
    for (idx, ch) in after_prefix.char_indices() {
        if ch.is_whitespace() || ch == '}' {
            name_end = idx;
            break;
        }
    }
    let tag_name = after_prefix.get(..name_end)?;
    if tag_name.is_empty() {
        return None;
    }

    let mut depth = 1usize;
    let mut body_start = 2 + name_end;
    if let Some(next_ch) = s.get(body_start..).and_then(|r| r.chars().next())
        && next_ch.is_whitespace()
    {
        body_start += next_ch.len_utf8();
    }

    let mut end_brace = None;
    for (rel_idx, ch) in s.get(2 + name_end..)?.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end_brace = Some(2 + name_end + rel_idx);
                    break;
                }
            }
            _ => {}
        }
    }

    let close_idx = end_brace?;
    let body = if body_start <= close_idx {
        s.get(body_start..close_idx).unwrap_or("")
    } else {
        ""
    };
    Some((tag_name, body, close_idx + 1))
}

fn format_javadoc_link(body: &str, as_code: bool) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Split target reference from optional label, keeping parameter lists like `foo(int, String)`
    // intact before checking for a trailing label.
    let (target_ref, label) = split_link_ref_and_label(trimmed);
    let display = if !label.is_empty() {
        label.to_string()
    } else if let Some(member) = target_ref.strip_prefix('#') {
        member.to_string()
    } else {
        target_ref.replace('#', ".")
    };

    if as_code {
        format!("`{display}`")
    } else {
        display
    }
}

fn split_link_ref_and_label(s: &str) -> (&str, &str) {
    let mut paren_depth = 0usize;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            c if c.is_whitespace() && paren_depth == 0 => {
                let target = s.get(..idx).unwrap_or(s);
                let label = s.get(idx..).map_or("", str::trim);
                return (target, label);
            }
            _ => {}
        }
    }
    (s, "")
}

/// Extracts a type-level or member-level Javadoc description from a `javadoc`-generated HTML page
/// and converts common HTML tags and entities to clean Markdown.
#[must_use]
pub fn extract_javadoc_from_html(html: &str, target: &BinaryTarget) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let fragment = match target.member_name.as_deref() {
        Some(member_name) => extract_member_html_block(html, &lower, target, member_name)?,
        None => extract_type_html_block(html, &lower)?,
    };

    let md = html_fragment_to_markdown(fragment);
    if md.trim().is_empty() {
        None
    } else {
        Some(md)
    }
}

fn extract_type_html_block<'a>(html: &'a str, lower: &str) -> Option<&'a str> {
    // Modern javadoc (JDK 11+): <section class="class-description" id="class-description">
    if let Some(section_range) = find_element_by_tag_and_attr(
        lower,
        "section",
        &["class=\"class-description\"", "id=\"class-description\""],
        0,
    ) && let Some(block) = first_div_block_in_range(html, lower, section_range)
    {
        return Some(block);
    }

    // JDK 8-15 javadoc: <div class="description">
    if let Some(desc_range) =
        find_element_by_tag_and_attr(lower, "div", &["class=\"description\""], 0)
        && let Some(block) = first_div_block_in_range(html, lower, desc_range)
    {
        return Some(block);
    }

    None
}

fn extract_member_html_block<'a>(
    html: &'a str,
    lower: &str,
    target: &BinaryTarget,
    member_name: &str,
) -> Option<&'a str> {
    let simple_type_name = target
        .qualified_name
        .rsplit('.')
        .next()
        .unwrap_or(&target.qualified_name);
    let is_ctor_candidate = member_name == simple_type_name || member_name == "<init>";

    let mut candidate_names = vec![member_name.to_ascii_lowercase()];
    if is_ctor_candidate {
        let init_lower = "<init>".to_string();
        let lt_init = "&lt;init&gt;".to_string();
        let pct_init = "%3cinit%3e".to_string();
        let type_lower = simple_type_name.to_ascii_lowercase();
        for cand in [init_lower, lt_init, pct_init, type_lower] {
            if !candidate_names.contains(&cand) {
                candidate_names.push(cand);
            }
        }
    }

    // Modern javadoc: <section class="detail" id="...">
    let mut search_from = 0;
    while let Some(rel) = lower.get(search_from..)?.find("<section") {
        let open_start = search_from + rel;
        let Some(tag_end_rel) = lower.get(open_start..)?.find('>') else {
            break;
        };
        let open_tag = lower.get(open_start..open_start + tag_end_rel + 1)?;
        if open_tag.contains("class=\"detail\"")
            && let Some(id_val) = extract_attr_value(open_tag, "id")
            && candidate_names
                .iter()
                .any(|cand| matches_modern_member_id(id_val, cand))
            && let Some(close_end) = find_matching_close_tag(lower, "section", open_start)
        {
            let section_range = (open_start + tag_end_rel + 1, close_end);
            if let Some(block) = first_div_block_in_range(html, lower, section_range) {
                return Some(block);
            }
        }
        search_from = open_start + tag_end_rel + 1;
    }

    // Legacy JDK 8 javadoc: <a name="length--"> / <a id="length--">
    let mut anchor_pos = 0;
    while let Some(rel) = lower.get(anchor_pos..)?.find("<a ") {
        let open_start = anchor_pos + rel;
        let Some(tag_end_rel) = lower.get(open_start..)?.find('>') else {
            break;
        };
        let open_tag = lower.get(open_start..open_start + tag_end_rel + 1)?;
        let anchor_id =
            extract_attr_value(open_tag, "name").or_else(|| extract_attr_value(open_tag, "id"));
        if let Some(id_val) = anchor_id
            && candidate_names
                .iter()
                .any(|cand| matches_legacy_member_anchor(id_val, cand))
        {
            let after_anchor = open_start + tag_end_rel + 1;
            let bound = lower
                .get(after_anchor..)
                .and_then(|rest| rest.find("<a ").or_else(|| rest.find("</ul>")))
                .map_or(lower.len(), |off| after_anchor + off);
            if let Some(block) = first_div_block_in_range(html, lower, (after_anchor, bound)) {
                return Some(block);
            }
        }
        anchor_pos = open_start + tag_end_rel + 1;
    }

    None
}

fn matches_modern_member_id(id_val: &str, candidate: &str) -> bool {
    if id_val == candidate {
        return true;
    }
    id_val
        .strip_prefix(candidate)
        .is_some_and(|rest| rest.starts_with('('))
}

fn matches_legacy_member_anchor(anchor_val: &str, candidate: &str) -> bool {
    if anchor_val == candidate {
        return true;
    }
    anchor_val
        .strip_prefix(candidate)
        .is_some_and(|rest| rest.starts_with('-'))
}

fn extract_attr_value<'a>(open_tag: &'a str, attr: &str) -> Option<&'a str> {
    let pattern_dq = format!("{attr}=\"");
    if let Some(pos) = open_tag.find(&pattern_dq) {
        let val_start = pos + pattern_dq.len();
        let val_end = open_tag.get(val_start..)?.find('"')?;
        return open_tag.get(val_start..val_start + val_end);
    }
    let pattern_sq = format!("{attr}='");
    if let Some(pos) = open_tag.find(&pattern_sq) {
        let val_start = pos + pattern_sq.len();
        let val_end = open_tag.get(val_start..)?.find('\'')?;
        return open_tag.get(val_start..val_start + val_end);
    }
    None
}

fn find_element_by_tag_and_attr(
    lower: &str,
    tag_name: &str,
    attr_snippets: &[&str],
    start_from: usize,
) -> Option<(usize, usize)> {
    let needle = format!("<{tag_name}");
    let mut pos = start_from;
    while let Some(rel) = lower.get(pos..)?.find(&needle) {
        let open_start = pos + rel;
        let after_name = open_start + needle.len();
        let next_ch = lower.get(after_name..)?.chars().next()?;
        if !next_ch.is_whitespace() && next_ch != '>' {
            pos = after_name;
            continue;
        }
        let tag_end_rel = lower.get(open_start..)?.find('>')?;
        let open_tag = lower.get(open_start..open_start + tag_end_rel + 1)?;
        if attr_snippets.iter().any(|snip| open_tag.contains(snip)) {
            let content_start = open_start + tag_end_rel + 1;
            let content_end = find_matching_close_tag(lower, tag_name, open_start)?;
            return Some((content_start, content_end));
        }
        pos = open_start + tag_end_rel + 1;
    }
    None
}

fn first_div_block_in_range<'a>(
    html: &'a str,
    lower: &str,
    range: (usize, usize),
) -> Option<&'a str> {
    let (range_start, range_end) = range;
    let scoped_lower = lower.get(..range_end)?;
    let (block_start, block_end) =
        find_element_by_tag_and_attr(scoped_lower, "div", &["class=\"block\""], range_start)?;
    if block_end <= range_end {
        html.get(block_start..block_end)
    } else {
        None
    }
}

/// Finds the start byte offset of the matching `</tag_name>` for the `<tag_name ...>` opening at
/// `open_start`.
fn find_matching_close_tag(lower: &str, tag_name: &str, open_start: usize) -> Option<usize> {
    let open_prefix = format!("<{tag_name}");
    let close_tag = format!("</{tag_name}>");

    let first_gt = lower.get(open_start..)?.find('>')?;
    let mut pos = open_start + first_gt + 1;
    let mut depth = 1usize;

    while let Some(rest) = lower.get(pos..) {
        let next_open = rest.find(&open_prefix).map(|i| pos + i);
        let next_close = rest.find(&close_tag).map(|i| pos + i);

        match (next_open, next_close) {
            (_, None) => return None,
            (Some(o), Some(c)) if o < c => {
                let after = o + open_prefix.len();
                let is_tag_boundary = lower
                    .get(after..)
                    .and_then(|s| s.chars().next())
                    .is_some_and(|ch| ch.is_whitespace() || ch == '>');
                if is_tag_boundary {
                    depth += 1;
                }
                pos = after;
            }
            (_, Some(c)) => {
                depth -= 1;
                if depth == 0 {
                    return Some(c);
                }
                pos = c + close_tag.len();
            }
        }
    }

    None
}

fn html_fragment_to_markdown(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut pos = 0usize;
    let mut in_pre = false;
    let mut pre_body_start: Option<usize> = None;
    let mut in_code = false;
    let mut in_backticks = false;
    let mut list_stack: Vec<Option<usize>> = Vec::new();
    let mut link_href_stack: Vec<Option<String>> = Vec::new();
    let mut link_text_start: Option<usize> = None;

    while let Some(rest) = html.get(pos..) {
        if !in_backticks
            && !in_pre
            && rest.starts_with('`')
        {
            in_backticks = true;
            out.push('`');
            pos += 1;
            continue;
        }
        if in_backticks && rest.starts_with('`') {
            in_backticks = false;
            out.push('`');
            pos += 1;
            continue;
        }

        if !in_backticks
            && rest.starts_with('<')
            && let Some(tag_end_rel) = rest.find('>')
        {
            let raw_tag = rest.get(1..tag_end_rel).unwrap_or("");
            let tag_trim = raw_tag.trim();
            let tag_lower = tag_trim.to_ascii_lowercase();
            let is_closing = tag_lower.starts_with('/');
            let name = tag_lower
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("");

            if is_html_tag_name(name) {
                match (is_closing, name) {
                    (false, "p") | (true, "p") | (false, "blockquote") | (true, "blockquote") => {
                        push_paragraph_break(&mut out);
                    }
                    (false, "br") if !out.ends_with('\n') && !out.is_empty() => {
                        out.push('\n');
                    }
                    (false, "pre") => {
                        in_pre = true;
                        push_paragraph_break(&mut out);
                        out.push_str("```\n");
                        pos += tag_end_rel + 1;
                        if let Some(after_pre) = html.get(pos..) {
                            if after_pre.starts_with("\r\n") {
                                pos += 2;
                            } else if after_pre.starts_with('\n') {
                                pos += 1;
                            }
                        }
                        pre_body_start = Some(out.len());
                        continue;
                    }
                    (true, "pre") => {
                        in_pre = false;
                        if let Some(start) = pre_body_start.take()
                            && let Some(body) = out.get(start..)
                        {
                            let trimmed_len = body.trim_end().len();
                            out.truncate(start + trimmed_len);
                        }
                        if !out.ends_with('\n') {
                            out.push('\n');
                        }
                        out.push_str("```");
                        push_paragraph_break(&mut out);
                    }
                    (false, "code") | (false, "tt") if !in_pre && !in_code => {
                        in_code = true;
                        out.push('`');
                    }
                    (true, "code") | (true, "tt") if !in_pre && in_code => {
                        in_code = false;
                        out.push('`');
                    }
                    (false, "b") | (false, "strong") | (true, "b") | (true, "strong")
                        if !in_pre && !in_code =>
                    {
                        out.push_str("**");
                    }
                    (false, "i") | (false, "em") | (true, "i") | (true, "em")
                        if !in_pre && !in_code =>
                    {
                        out.push('*');
                    }
                    (false, "ul") => {
                        list_stack.push(None);
                        push_paragraph_break(&mut out);
                    }
                    (false, "ol") => {
                        list_stack.push(Some(1));
                        push_paragraph_break(&mut out);
                    }
                    (true, "ul") | (true, "ol") => {
                        list_stack.pop();
                        push_paragraph_break(&mut out);
                    }
                    (false, "li") => {
                        if !out.is_empty() && !out.ends_with('\n') {
                            out.push('\n');
                        }
                        let indent = "  ".repeat(list_stack.len().saturating_sub(1));
                        out.push_str(&indent);
                        match list_stack.last_mut() {
                            Some(Some(n)) => {
                                let _ = write!(out, "{n}. ");
                                *n += 1;
                            }
                            _ => out.push_str("- "),
                        }
                    }
                    (true, "li") if !out.ends_with('\n') => {
                        out.push('\n');
                    }
                    (false, "a") => {
                        let href = extract_attr_value(raw_tag, "href")
                            .map(str::trim)
                            .filter(|h| !h.is_empty() && !h.starts_with('#'))
                            .map(str::to_string);
                        if href.is_some() && !in_pre && !in_code {
                            link_text_start = Some(out.len());
                            out.push('[');
                        }
                        link_href_stack.push(href);
                    }
                    (true, "a") => {
                        if let Some(Some(href)) = link_href_stack.pop()
                            && !in_pre
                        {
                            let has_label =
                                link_text_start.is_some_and(|start| out.len() > start + 1);
                            if has_label {
                                let _ = write!(out, "]({href})");
                            } else if let Some(start) = link_text_start {
                                out.truncate(start);
                            }
                        }
                        link_text_start = None;
                    }
                    _ => {}
                }
                pos += tag_end_rel + 1;
                continue;
            }
        }

        if rest.starts_with('&')
            && let Some((decoded, len)) = decode_html_entity(rest)
        {
            out.push_str(&decoded);
            pos += len;
            continue;
        }

        if let Some(ch) = rest.chars().next() {
            if in_pre || in_backticks {
                out.push(ch);
            } else if ch == '\r' {
                // Skip carriage returns outside `<pre>`.
            } else if ch.is_whitespace() {
                if !out.is_empty() && !out.ends_with(['\n', ' ']) {
                    out.push(' ');
                }
            } else {
                out.push(ch);
            }
            pos += ch.len_utf8();
        } else {
            break;
        }
    }

    normalize_markdown_paragraphs(&out)
}

fn is_html_tag_name(name: &str) -> bool {
    matches!(
        name,
        "p" | "br"
            | "pre"
            | "code"
            | "tt"
            | "b"
            | "strong"
            | "i"
            | "em"
            | "ul"
            | "ol"
            | "li"
            | "a"
            | "div"
            | "span"
            | "dl"
            | "dt"
            | "dd"
            | "blockquote"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "table"
            | "thead"
            | "tbody"
            | "tr"
            | "th"
            | "td"
            | "sub"
            | "sup"
            | "hr"
    )
}

fn push_paragraph_break(out: &mut String) {
    while out.ends_with(' ') {
        out.pop();
    }
    if out.is_empty() {
        return;
    }
    if out.ends_with("\n\n") {
        return;
    }
    if out.ends_with('\n') {
        out.push('\n');
    } else {
        out.push_str("\n\n");
    }
}

fn normalize_markdown_paragraphs(s: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut in_fence = false;

    for raw in s.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            lines.push(trimmed);
            continue;
        }
        if in_fence {
            lines.push(raw.trim_end());
        } else {
            lines.push(raw.trim());
        }
    }

    // Collapse multiple consecutive blank lines to a single blank line.
    let mut collapsed = String::with_capacity(s.len());
    let mut blank_run = 0usize;
    for line in lines {
        if line.is_empty() {
            blank_run += 1;
            if blank_run == 1 && !collapsed.is_empty() {
                collapsed.push('\n');
            }
        } else {
            blank_run = 0;
            if !collapsed.is_empty() {
                collapsed.push('\n');
            }
            collapsed.push_str(line);
        }
    }

    collapsed.trim().to_string()
}

fn decode_html_entity(s: &str) -> Option<(String, usize)> {
    let semi = s.find(';')?;
    if semi == 0 || semi > 12 {
        return None;
    }
    let entity = s.get(1..semi)?;
    let decoded = match entity {
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "amp" => "&".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        "nbsp" => " ".to_string(),
        _ if entity.starts_with('#') => {
            let code_str = entity.get(1..)?;
            let code = if let Some(hex) = code_str
                .strip_prefix('x')
                .or_else(|| code_str.strip_prefix('X'))
            {
                u32::from_str_radix(hex, 16).ok()?
            } else {
                code_str.parse::<u32>().ok()?
            };
            let ch = char::from_u32(code)?;
            ch.to_string()
        }
        _ => return None,
    };
    Some((decoded, semi + 1))
}

/// Inserts Markdown `javadoc` cleanly between the fenced Java signature and the trailing
/// `*from ...*` origin line (if present).
#[must_use]
pub fn append_javadoc_to_hover(content: &str, javadoc: &str) -> String {
    let doc = javadoc.trim();
    if doc.is_empty() {
        return content.to_string();
    }

    let trimmed = content.trim_end();
    if let Some((prefix, last_line)) = trimmed.rsplit_once('\n')
        && last_line.starts_with("*from ")
        && last_line.ends_with('*')
    {
        return format!("{}\n\n{doc}\n\n{last_line}", prefix.trim_end());
    }

    format!("{trimmed}\n\n{doc}")
}

#[cfg(test)]
mod tests {
    use hir::BinaryOrigin;

    use super::*;

    fn slice(text: &str, range: TextRange) -> &str {
        &text[usize::from(range.start())..usize::from(range.end())]
    }

    fn jdk_origin() -> BinaryOrigin {
        BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base")),
        }
    }

    fn build_rich_index() -> ClasspathIndex {
        let mut idx = ClasspathIndex::default();
        let origin = jdk_origin();

        idx.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Map"),
            qualified_name: "java.util.Map".to_string(),
            internal_name: "java/util/Map".to_string(),
            package: "java.util".to_string(),
            enclosing_type: None,
            kind: TypeKind::Interface,
            identity: hir::TypeIdentity::Unspecified,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: true,
            nullness_scope: hir::NullnessScope::Inherited,
            type_params: vec![SmolStr::new("K"), SmolStr::new("V")],
            type_param_bounds: Default::default(),
            super_class: Some("java.lang.Object".to_string()),
            super_class_generic: None,
            interfaces: vec!["java.io.Serializable".to_string()],
            interfaces_generic: vec![],
            permitted_subclasses: vec![],
            members: vec![
                BinaryMemberSymbol {
                    owner_type: "java.util.Map".to_string(),
                    name: SmolStr::new("size"),
                    kind: MemberKind::Method,
                    is_public: true,
                    is_static: false,
                    is_abstract: false,
                    nullness_scope: hir::NullnessScope::Inherited,
                    decl_nullability: None,
                    type_params: vec![],
                    type_param_bounds: Default::default(),
                    type_ref: Some("int".to_string()),
                    params: vec![],
                    is_varargs: false,
                },
                BinaryMemberSymbol {
                    owner_type: "java.util.Map".to_string(),
                    name: SmolStr::new("of"),
                    kind: MemberKind::Method,
                    is_public: true,
                    is_static: true,
                    is_abstract: false,
                    nullness_scope: hir::NullnessScope::Inherited,
                    decl_nullability: None,
                    type_params: vec![],
                    type_param_bounds: Default::default(),
                    type_ref: Some("java.util.Map<K, V>".to_string()),
                    params: vec![ParamSummary {
                        name: Some(SmolStr::new("entries")),
                        type_ref: "java.util.Map.Entry<K, V>".to_string(),
                        is_varargs: true,
                        decl_nullability: None,
                    }],
                    is_varargs: true,
                },
            ],
            nested_types: vec![],
            origin: origin.clone(),
        });

        idx.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Entry"),
            qualified_name: "java.util.Map.Entry".to_string(),
            internal_name: "java/util/Map$Entry".to_string(),
            package: "java.util".to_string(),
            enclosing_type: Some("java.util.Map".to_string()),
            kind: TypeKind::Interface,
            identity: hir::TypeIdentity::Unspecified,
            is_public: true,
            is_static: true,
            is_final: false,
            is_abstract: true,
            nullness_scope: hir::NullnessScope::Inherited,
            type_params: vec![SmolStr::new("K"), SmolStr::new("V")],
            type_param_bounds: Default::default(),
            super_class: Some("java.lang.Object".to_string()),
            super_class_generic: None,
            interfaces: vec![],
            interfaces_generic: vec![],
            permitted_subclasses: vec![],
            members: vec![BinaryMemberSymbol {
                owner_type: "java.util.Map.Entry".to_string(),
                name: SmolStr::new("getKey"),
                kind: MemberKind::Method,
                is_public: true,
                is_static: false,
                is_abstract: false,
                nullness_scope: hir::NullnessScope::Inherited,
                decl_nullability: None,
                type_params: vec![],
                type_param_bounds: Default::default(),
                type_ref: Some("K".to_string()),
                params: vec![],
                is_varargs: false,
            }],
            nested_types: vec![],
            origin: origin.clone(),
        });

        idx.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Shape"),
            qualified_name: "com.example.Shape".to_string(),
            internal_name: "com/example/Shape".to_string(),
            package: "com.example".to_string(),
            enclosing_type: None,
            kind: TypeKind::Class,
            identity: hir::TypeIdentity::Unspecified,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: true,
            nullness_scope: hir::NullnessScope::Inherited,
            type_params: vec![],
            type_param_bounds: Default::default(),
            super_class: Some("com.example.BaseShape".to_string()),
            super_class_generic: None,
            interfaces: vec!["java.io.Serializable".to_string()],
            interfaces_generic: vec![],
            permitted_subclasses: vec!["com.example.Circle".to_string()],
            members: vec![
                BinaryMemberSymbol {
                    owner_type: "com.example.Shape".to_string(),
                    name: SmolStr::new("TAG"),
                    kind: MemberKind::Field,
                    is_public: true,
                    is_static: true,
                    is_abstract: false,
                    nullness_scope: hir::NullnessScope::Inherited,
                    decl_nullability: None,
                    type_params: vec![],
                    type_param_bounds: Default::default(),
                    type_ref: Some("String".to_string()),
                    params: vec![],
                    is_varargs: false,
                },
                BinaryMemberSymbol {
                    owner_type: "com.example.Shape".to_string(),
                    name: SmolStr::new("Shape"),
                    kind: MemberKind::Constructor,
                    is_public: false,
                    is_static: false,
                    is_abstract: false,
                    nullness_scope: hir::NullnessScope::Inherited,
                    decl_nullability: None,
                    type_params: vec![],
                    type_param_bounds: Default::default(),
                    type_ref: None,
                    params: vec![ParamSummary {
                        name: None,
                        type_ref: "int".to_string(),
                        is_varargs: false,
                        decl_nullability: None,
                    }],
                    is_varargs: false,
                },
                BinaryMemberSymbol {
                    owner_type: "com.example.Shape".to_string(),
                    name: SmolStr::new("area"),
                    kind: MemberKind::Method,
                    is_public: true,
                    is_static: false,
                    is_abstract: false,
                    nullness_scope: hir::NullnessScope::Inherited,
                    decl_nullability: None,
                    type_params: vec![],
                    type_param_bounds: Default::default(),
                    type_ref: Some("double".to_string()),
                    params: vec![],
                    is_varargs: false,
                },
            ],
            nested_types: vec![],
            origin: origin.clone(),
        });

        idx.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Color"),
            qualified_name: "com.example.Color".to_string(),
            internal_name: "com/example/Color".to_string(),
            package: "com.example".to_string(),
            enclosing_type: None,
            kind: TypeKind::Enum,
            identity: hir::TypeIdentity::Identity,
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            nullness_scope: hir::NullnessScope::Inherited,
            type_params: vec![],
            type_param_bounds: Default::default(),
            super_class: Some("java.lang.Enum".to_string()),
            super_class_generic: None,
            interfaces: vec![],
            interfaces_generic: vec![],
            permitted_subclasses: vec![],
            members: vec![
                BinaryMemberSymbol {
                    owner_type: "com.example.Color".to_string(),
                    name: SmolStr::new("RED"),
                    kind: MemberKind::EnumConstant,
                    is_public: true,
                    is_static: true,
                    is_abstract: false,
                    nullness_scope: hir::NullnessScope::Inherited,
                    decl_nullability: None,
                    type_params: vec![],
                    type_param_bounds: Default::default(),
                    type_ref: None,
                    params: vec![],
                    is_varargs: false,
                },
                BinaryMemberSymbol {
                    owner_type: "com.example.Color".to_string(),
                    name: SmolStr::new("BLUE"),
                    kind: MemberKind::EnumConstant,
                    is_public: true,
                    is_static: true,
                    is_abstract: false,
                    nullness_scope: hir::NullnessScope::Inherited,
                    decl_nullability: None,
                    type_params: vec![],
                    type_param_bounds: Default::default(),
                    type_ref: None,
                    params: vec![],
                    is_varargs: false,
                },
            ],
            nested_types: vec![],
            origin: origin.clone(),
        });

        idx.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Point"),
            qualified_name: "com.example.Point".to_string(),
            internal_name: "com/example/Point".to_string(),
            package: "com.example".to_string(),
            enclosing_type: None,
            kind: TypeKind::Record,
            identity: hir::TypeIdentity::Identity,
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            nullness_scope: hir::NullnessScope::Inherited,
            type_params: vec![],
            type_param_bounds: Default::default(),
            super_class: Some("java.lang.Record".to_string()),
            super_class_generic: None,
            interfaces: vec!["java.io.Serializable".to_string()],
            interfaces_generic: vec![],
            permitted_subclasses: vec![],
            members: vec![
                BinaryMemberSymbol {
                    owner_type: "com.example.Point".to_string(),
                    name: SmolStr::new("x"),
                    kind: MemberKind::RecordComponent,
                    is_public: true,
                    is_static: false,
                    is_abstract: false,
                    nullness_scope: hir::NullnessScope::Inherited,
                    decl_nullability: None,
                    type_params: vec![],
                    type_param_bounds: Default::default(),
                    type_ref: Some("int".to_string()),
                    params: vec![],
                    is_varargs: false,
                },
                BinaryMemberSymbol {
                    owner_type: "com.example.Point".to_string(),
                    name: SmolStr::new("y"),
                    kind: MemberKind::RecordComponent,
                    is_public: true,
                    is_static: false,
                    is_abstract: false,
                    nullness_scope: hir::NullnessScope::Inherited,
                    decl_nullability: None,
                    type_params: vec![],
                    type_param_bounds: Default::default(),
                    type_ref: Some("int".to_string()),
                    params: vec![],
                    is_varargs: false,
                },
            ],
            nested_types: vec![],
            origin: origin.clone(),
        });

        idx.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Tag"),
            qualified_name: "com.example.Tag".to_string(),
            internal_name: "com/example/Tag".to_string(),
            package: "com.example".to_string(),
            enclosing_type: None,
            kind: TypeKind::Annotation,
            identity: hir::TypeIdentity::Unspecified,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: true,
            nullness_scope: hir::NullnessScope::Inherited,
            type_params: vec![],
            type_param_bounds: Default::default(),
            super_class: Some("java.lang.Object".to_string()),
            super_class_generic: None,
            interfaces: vec!["java.lang.annotation.Annotation".to_string()],
            interfaces_generic: vec![],
            permitted_subclasses: vec![],
            members: vec![BinaryMemberSymbol {
                owner_type: "com.example.Tag".to_string(),
                name: SmolStr::new("value"),
                kind: MemberKind::Method,
                is_public: true,
                is_static: false,
                is_abstract: false,
                nullness_scope: hir::NullnessScope::Inherited,
                decl_nullability: None,
                type_params: vec![],
                type_param_bounds: Default::default(),
                type_ref: Some("String".to_string()),
                params: vec![],
                is_varargs: false,
            }],
            nested_types: vec![],
            origin,
        });

        idx.finalize();
        idx
    }

    #[test]
    fn synthesize_stub_and_locate_roundtrip_for_all_kinds() {
        let idx = build_rich_index();

        // Nested interface member (`Map.Entry.getKey`) walks up to `Map` and parses cleanly.
        let entry_get_key = BinaryTarget {
            origin: jdk_origin(),
            internal_name: "java/util/Map$Entry".to_string(),
            qualified_name: "java.util.Map.Entry".to_string(),
            member_name: Some(SmolStr::new("getKey")),
        };
        let map_stub = synthesize_binary_stub(&idx, &entry_get_key).unwrap();
        let parse_map = syntax::parse(&map_stub);
        assert!(
            parse_map.errors().is_empty(),
            "stub had syntax errors: {:?}\n{map_stub}",
            parse_map.errors()
        );
        assert!(map_stub.contains("package java.util;"));
        assert!(map_stub.contains("public interface Map<K, V> extends java.io.Serializable {"));
        assert!(map_stub.contains("public int size();"));
        assert!(map_stub.contains("public static java.util.Map<K, V> of(java.util.Map.Entry<K, V>... entries) { throw new UnsupportedOperationException(); }"));
        assert!(map_stub.contains("public interface Entry<K, V> {"));

        let located_get_key = locate_binary_symbol_in_source(&map_stub, &entry_get_key).unwrap();
        assert_eq!(slice(&map_stub, located_get_key), "getKey");

        let entry_target = BinaryTarget {
            member_name: None,
            ..entry_get_key.clone()
        };
        assert_eq!(
            slice(&map_stub, locate_binary_symbol_in_source(&map_stub, &entry_target).unwrap()),
            "Entry"
        );

        // Missing member falls back to enclosing target type's NAME range.
        let missing_member = BinaryTarget {
            member_name: Some(SmolStr::new("nonExistent")),
            ..entry_get_key
        };
        assert_eq!(
            slice(&map_stub, locate_binary_symbol_in_source(&map_stub, &missing_member).unwrap()),
            "Entry"
        );

        // Sealed abstract class with extends, implements, permits, field, protected ctor, method.
        let shape_ctor = BinaryTarget {
            origin: jdk_origin(),
            internal_name: "com/example/Shape".to_string(),
            qualified_name: "com.example.Shape".to_string(),
            member_name: Some(SmolStr::new("Shape")),
        };
        let shape_stub = synthesize_binary_stub(&idx, &shape_ctor).unwrap();
        assert!(syntax::parse(&shape_stub).errors().is_empty(), "{shape_stub}");
        assert!(shape_stub.contains(
            "public sealed abstract class Shape extends com.example.BaseShape implements java.io.Serializable permits com.example.Circle"
        ));
        assert!(shape_stub.contains("protected Shape(int arg0) { throw new UnsupportedOperationException(); }"));
        assert_eq!(
            slice(&shape_stub, locate_binary_symbol_in_source(&shape_stub, &shape_ctor).unwrap()),
            "Shape"
        );
        let shape_tag = BinaryTarget {
            member_name: Some(SmolStr::new("TAG")),
            ..shape_ctor
        };
        assert_eq!(
            slice(&shape_stub, locate_binary_symbol_in_source(&shape_stub, &shape_tag).unwrap()),
            "TAG"
        );

        // Enum constants
        let color_red = BinaryTarget {
            origin: jdk_origin(),
            internal_name: "com/example/Color".to_string(),
            qualified_name: "com.example.Color".to_string(),
            member_name: Some(SmolStr::new("RED")),
        };
        let color_stub = synthesize_binary_stub(&idx, &color_red).unwrap();
        assert!(syntax::parse(&color_stub).errors().is_empty(), "{color_stub}");
        assert!(color_stub.contains("RED, BLUE;"));
        assert_eq!(
            slice(&color_stub, locate_binary_symbol_in_source(&color_stub, &color_red).unwrap()),
            "RED"
        );

        // Record components
        let point_y = BinaryTarget {
            origin: jdk_origin(),
            internal_name: "com/example/Point".to_string(),
            qualified_name: "com.example.Point".to_string(),
            member_name: Some(SmolStr::new("y")),
        };
        let point_stub = synthesize_binary_stub(&idx, &point_y).unwrap();
        assert!(syntax::parse(&point_stub).errors().is_empty(), "{point_stub}");
        assert!(point_stub.contains("public record Point(int x, int y) implements java.io.Serializable"));
        assert_eq!(
            slice(&point_stub, locate_binary_symbol_in_source(&point_stub, &point_y).unwrap()),
            "y"
        );

        // Annotation
        let tag_val = BinaryTarget {
            origin: jdk_origin(),
            internal_name: "com/example/Tag".to_string(),
            qualified_name: "com.example.Tag".to_string(),
            member_name: Some(SmolStr::new("value")),
        };
        let tag_stub = synthesize_binary_stub(&idx, &tag_val).unwrap();
        assert!(syntax::parse(&tag_stub).errors().is_empty(), "{tag_stub}");
        assert!(tag_stub.contains("public @interface Tag {"));
        assert!(tag_stub.contains("public String value();"));
        assert_eq!(
            slice(&tag_stub, locate_binary_symbol_in_source(&tag_stub, &tag_val).unwrap()),
            "value"
        );

        // Unknown type returns None
        let missing_type = BinaryTarget {
            origin: jdk_origin(),
            internal_name: "com/example/Missing".to_string(),
            qualified_name: "com.example.Missing".to_string(),
            member_name: None,
        };
        assert!(synthesize_binary_stub(&idx, &missing_type).is_none());
    }

    #[test]
    fn extract_javadoc_from_source_formats_tags_and_html() {
        let src = r#"
package java.util;

/**
 * A map entry (key-value pair). See {@link Map#entrySet()}.
 */
public interface Map<K, V> {
    /**
     * Nested entry docs with {@code Map<K, V>} and <code>null</code>.
     * <p>
     * Example:
     * <pre>
     * var x = entry.compute(1);
     * </pre>
     *
     * @param key the lookup key
     * @return the computed value
     * @throws IllegalArgumentException if invalid
     */
    interface Entry<K, V> {
        /**
         * Returns the key for {@code this} entry.
         * @return the key
         */
        K getKey();
    }
}
"#;

        let map_target = BinaryTarget {
            origin: jdk_origin(),
            internal_name: "java/util/Map".to_string(),
            qualified_name: "java.util.Map".to_string(),
            member_name: None,
        };
        let map_doc = extract_javadoc_from_source(src, &map_target).unwrap();
        assert_eq!(
            map_doc,
            "A map entry (key-value pair). See `Map.entrySet()`."
        );

        let entry_target = BinaryTarget {
            internal_name: "java/util/Map$Entry".to_string(),
            qualified_name: "java.util.Map.Entry".to_string(),
            member_name: None,
            ..map_target.clone()
        };
        let entry_doc = extract_javadoc_from_source(src, &entry_target).unwrap();
        assert!(entry_doc.contains("Nested entry docs with `Map<K, V>` and `null`."), "{entry_doc}");
        assert!(entry_doc.contains("```\nvar x = entry.compute(1);\n```"), "{entry_doc}");
        assert!(entry_doc.contains("- **@param** `key` — the lookup key"), "{entry_doc}");
        assert!(entry_doc.contains("- **@return** the computed value"), "{entry_doc}");
        assert!(
            entry_doc.contains("- **@throws** `IllegalArgumentException` — if invalid"),
            "{entry_doc}"
        );

        let get_key_target = BinaryTarget {
            member_name: Some(SmolStr::new("getKey")),
            ..entry_target
        };
        let get_key_doc = extract_javadoc_from_source(src, &get_key_target).unwrap();
        assert!(get_key_doc.contains("Returns the key for `this` entry."));
        assert!(get_key_doc.contains("- **@return** the key"));

        let missing_member = BinaryTarget {
            member_name: Some(SmolStr::new("missing")),
            ..map_target
        };
        assert!(extract_javadoc_from_source(src, &missing_member).is_none());
    }

    #[test]
    fn extract_javadoc_from_html_supports_modern_and_legacy_javadoc() {
        let modern_html = r#"
<!DOCTYPE HTML>
<html>
<body>
<section class="class-description" id="class-description">
<div class="horizontal-scroll">
<div class="block">Represents character strings like <code>"abc"</code>.
<p>Supports &lt;T&gt; &amp; &quot;quoted&quot;&nbsp;text.</p>
<ul><li>Fast</li><li>Immutable</li></ul>
<a href="CharSequence.html"><code>CharSequence</code></a>
<pre>String s = "hi";</pre>
</div>
</div>
</section>
<section class="detail" id="&lt;init&gt;(char[])">
<h3>String</h3>
<div class="block">Allocates a new <code>String</code> from characters.</div>
</section>
<section class="detail" id="length()">
<h3>length</h3>
<div class="block">Returns the length of this string.</div>
</section>
</body>
</html>
"#;

        let str_type = BinaryTarget {
            origin: jdk_origin(),
            internal_name: "java/lang/String".to_string(),
            qualified_name: "java.lang.String".to_string(),
            member_name: None,
        };
        let type_doc = extract_javadoc_from_html(modern_html, &str_type).unwrap();
        assert!(type_doc.contains("Represents character strings like `\"abc\"`."), "{type_doc}");
        assert!(type_doc.contains("Supports <T> & \"quoted\" text."), "{type_doc}");
        assert!(type_doc.contains("- Fast\n- Immutable"), "{type_doc}");
        assert!(type_doc.contains("[`CharSequence`](CharSequence.html)"), "{type_doc}");
        assert!(type_doc.contains("```\nString s = \"hi\";\n```"), "{type_doc}");

        let len_target = BinaryTarget {
            member_name: Some(SmolStr::new("length")),
            ..str_type.clone()
        };
        assert_eq!(
            extract_javadoc_from_html(modern_html, &len_target).as_deref(),
            Some("Returns the length of this string.")
        );

        let ctor_target = BinaryTarget {
            member_name: Some(SmolStr::new("String")),
            ..str_type.clone()
        };
        assert_eq!(
            extract_javadoc_from_html(modern_html, &ctor_target).as_deref(),
            Some("Allocates a new `String` from characters.")
        );

        // Legacy JDK 8 HTML format
        let legacy_html = r#"
<div class="description">
<ul class="blockList"><li class="blockList">
<div class="block">Legacy class doc.</div>
</li></ul>
</div>
<ul class="blockList"><li class="blockList">
<a name="out"></a>
<h4>out</h4>
<div class="block">The &quot;standard&quot; output stream.</div>
</li></ul>
"#;
        assert_eq!(
            extract_javadoc_from_html(legacy_html, &str_type).as_deref(),
            Some("Legacy class doc.")
        );
        let out_target = BinaryTarget {
            member_name: Some(SmolStr::new("out")),
            ..str_type
        };
        assert_eq!(
            extract_javadoc_from_html(legacy_html, &out_target).as_deref(),
            Some("The \"standard\" output stream.")
        );
    }

    #[test]
    fn append_javadoc_to_hover_inserts_before_origin_line() {
        let with_origin = "```java\nint java.lang.String.length()\n```\n*from JDK module `java.base`*";
        assert_eq!(
            append_javadoc_to_hover(with_origin, "Returns the length."),
            "```java\nint java.lang.String.length()\n```\n\nReturns the length.\n\n*from JDK module `java.base`*"
        );
        assert_eq!(append_javadoc_to_hover(with_origin, "   "), with_origin);

        let without_origin = "```java\nint Foo.bar()\n```";
        assert_eq!(
            append_javadoc_to_hover(without_origin, "Does bar."),
            "```java\nint Foo.bar()\n```\n\nDoes bar."
        );
    }
}
