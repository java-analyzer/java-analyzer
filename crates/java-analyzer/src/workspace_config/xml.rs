use std::collections::BTreeMap;

pub(super) fn strip_xml_comments(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "<!--".len()..];
        if let Some(end) = after.find("-->") {
            rest = &after[end + "-->".len()..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

pub(super) fn remove_xml_blocks(xml: &str, tag: &str) -> (String, Vec<String>) {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut kept = String::with_capacity(xml.len());
    let mut removed = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        kept.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        if let Some(end) = after_open.find(&close) {
            removed.push(after_open[..end].to_string());
            rest = &after_open[end + close.len()..];
        } else {
            kept.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    kept.push_str(rest);
    (kept, removed)
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

pub(super) fn extract_xml_tag_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    Some(xml[start..start + end].trim().to_string())
}

fn find_xml_tag_end(slice: &str) -> Option<usize> {
    let mut in_quote: Option<char> = None;
    for (offset, ch) in slice.char_indices() {
        match in_quote {
            Some(q) if ch == q => in_quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => in_quote = Some(ch),
            None if ch == '>' => return Some(offset),
            None => {}
        }
    }
    None
}

pub(super) fn find_xml_start_tags(xml: &str, tag: &str) -> Vec<String> {
    let mut results = Vec::new();
    let needle = format!("<{tag}");
    let mut rest = xml;
    while let Some(idx) = rest.find(&needle) {
        let after_name = &rest[idx + needle.len()..];
        let next_ch = after_name.chars().next().unwrap_or(' ');
        if next_ch.is_whitespace() || next_ch == '>' || next_ch == '/' {
            let slice = &rest[idx..];
            if let Some(end) = find_xml_tag_end(slice) {
                results.push(slice[..=end].to_string());
                rest = &slice[end + 1..];
                continue;
            }
        }
        rest = &rest[idx + needle.len()..];
    }
    results
}

pub(super) fn extract_xml_blocks_with_attrs(xml: &str, tag: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let open_prefix = format!("<{tag}");
    let close_tag = format!("</{tag}>");
    let mut rest = xml;

    while let Some(idx) = rest.find(&open_prefix) {
        let after_name = &rest[idx + open_prefix.len()..];
        let next_ch = after_name.chars().next().unwrap_or(' ');
        if !(next_ch.is_whitespace() || next_ch == '>' || next_ch == '/') {
            rest = after_name;
            continue;
        }
        let slice = &rest[idx..];
        let Some(tag_end) = find_xml_tag_end(slice) else {
            break;
        };
        let is_self_closing = slice[..tag_end].trim_end().ends_with('/');
        let after_open = &slice[tag_end + 1..];
        if is_self_closing {
            rest = after_open;
            continue;
        }

        let mut depth = 1usize;
        let mut cursor = after_open;
        let mut block_end = None;
        while depth > 0 {
            let next_open = cursor.find(&open_prefix);
            let next_close = cursor.find(&close_tag);
            match (next_open, next_close) {
                (_, None) => break,
                (Some(o), Some(c)) if o < c => {
                    let after_nested = &cursor[o + open_prefix.len()..];
                    let nested_ch = after_nested.chars().next().unwrap_or(' ');
                    if nested_ch.is_whitespace() || nested_ch == '>' || nested_ch == '/' {
                        let nested_slice = &cursor[o..];
                        let Some(nested_end) = find_xml_tag_end(nested_slice) else {
                            break;
                        };
                        let nested_self_closing =
                            nested_slice[..nested_end].trim_end().ends_with('/');
                        if !nested_self_closing {
                            depth += 1;
                        }
                        cursor = &nested_slice[nested_end + 1..];
                    } else {
                        cursor = after_nested;
                    }
                }
                (_, Some(c)) => {
                    depth -= 1;
                    if depth == 0 {
                        let matched_len = after_open.len() - cursor.len() + c;
                        block_end = Some(matched_len);
                    }
                    cursor = &cursor[c + close_tag.len()..];
                }
            }
        }

        if let Some(end) = block_end {
            blocks.push(after_open[..end].to_string());
            rest = &after_open[end + close_tag.len()..];
        } else {
            rest = after_open;
        }
    }

    blocks
}

pub(super) fn extract_xml_attr(tag_text: &str, attr: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pat = format!("{attr}={quote}");
        if let Some(start) = tag_text.find(&pat) {
            let after = &tag_text[start + pat.len()..];
            if let Some(end) = after.find(quote) {
                return Some(after[..end].to_string());
            }
        }
    }
    None
}
