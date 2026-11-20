use hir::{DefDatabase, FileId, ResolvedSymbol, ResolvedSymbolKind, SourceDatabase};
use syntax::{syntax_kind::SyntaxKind::*, SyntaxNode};

pub use syntax::TextRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverResult {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationTarget {
    pub file_id: FileId,
    /// Range of the entire declaration.
    pub full_range: TextRange,
    /// Range of the declaration's identifier (`NAME`), used as cursor destination.
    pub focus_range: TextRange,
}

pub fn goto_definition(
    db: &dyn DefDatabase,
    file_id: FileId,
    offset: u32,
) -> Option<NavigationTarget> {
    let resolved = hir::resolve_at_offset(db, file_id, offset)?;
    let (file_id, full_range, focus_range) = resolved.location()?;
    Some(NavigationTarget {
        file_id,
        full_range,
        focus_range,
    })
}

pub fn hover(db: &dyn DefDatabase, file_id: FileId, offset: u32) -> Option<HoverResult> {
    if let Some(resolved) = hir::resolve_at_offset(db, file_id, offset) {
        return Some(HoverResult {
            content: format_resolved_hover(&resolved),
        });
    }

    let syntax_node = db.syntax_tree(file_id);

    // Hover positions refer to the character under the pointer, so prefer the token that
    // starts at `offset` over the one that ends there.
    let token = syntax_node.token_at_offset(offset.into()).right_biased()?;

    match token.kind() {
        CLASS_KW => Some(HoverResult {
            content: "Keyword `class` declares a new class".to_string(),
        }),
        IDENT => Some(HoverResult {
            content: format!("Identifier `{}`", token.text()),
        }),
        _ => None,
    }
}

fn format_resolved_hover(sym: &ResolvedSymbol) -> String {
    match sym.kind {
        ResolvedSymbolKind::Class
        | ResolvedSymbolKind::Interface
        | ResolvedSymbolKind::Enum
        | ResolvedSymbolKind::Record
        | ResolvedSymbolKind::Annotation => {
            let kw = match sym.kind {
                ResolvedSymbolKind::Class => "class",
                ResolvedSymbolKind::Interface => "interface",
                ResolvedSymbolKind::Enum => "enum",
                ResolvedSymbolKind::Record => "record",
                ResolvedSymbolKind::Annotation => "@interface",
                _ => unreachable!(),
            };
            let tparams = sym.detail.as_deref().unwrap_or("");
            format!("```java\n{kw} {}{tparams}\n```", sym.qualified_name)
        }
        ResolvedSymbolKind::Field | ResolvedSymbolKind::RecordComponent => match &sym.detail {
            Some(ty) => format!("```java\n{ty} {}\n```", sym.qualified_name),
            None => format!("```java\n{}\n```", sym.qualified_name),
        },
        ResolvedSymbolKind::Method => {
            let sig = format_method_signature(&sym.qualified_name, sym.detail.as_deref());
            format!("```java\n{sig}\n```")
        }
        ResolvedSymbolKind::Constructor => {
            let params = sym.detail.as_deref().unwrap_or("()");
            format!("```java\n{}{params}\n```", sym.qualified_name)
        }
        ResolvedSymbolKind::EnumConstant => format!("```java\n{}\n```", sym.qualified_name),
        ResolvedSymbolKind::LocalVar
        | ResolvedSymbolKind::Param
        | ResolvedSymbolKind::LambdaParam
        | ResolvedSymbolKind::ForEachVar
        | ResolvedSymbolKind::CatchParam
        | ResolvedSymbolKind::ResourceVar
        | ResolvedSymbolKind::PatternVar => {
            let ty = sym.detail.as_deref().unwrap_or("var");
            let label = match sym.kind {
                ResolvedSymbolKind::LocalVar => "local variable",
                ResolvedSymbolKind::Param => "parameter",
                ResolvedSymbolKind::LambdaParam => "lambda parameter",
                ResolvedSymbolKind::ForEachVar => "for-each variable",
                ResolvedSymbolKind::CatchParam => "catch parameter",
                ResolvedSymbolKind::ResourceVar => "resource variable",
                ResolvedSymbolKind::PatternVar => "pattern variable",
                _ => unreachable!(),
            };
            format!("```java\n{ty} {}\n```\n{label}", sym.name)
        }
        ResolvedSymbolKind::TypeParam => match &sym.detail {
            Some(bounds) => format!("```java\ntype parameter {} {bounds}\n```", sym.name),
            None => format!("```java\ntype parameter {}\n```", sym.name),
        },
        ResolvedSymbolKind::Package => format!("```java\npackage {}\n```", sym.qualified_name),
    }
}

fn format_method_signature(qualified_name: &str, detail: Option<&str>) -> String {
    let Some(detail) = detail else {
        return format!("{qualified_name}()");
    };
    if let Some(paren_idx) = trailing_paren_group_start(detail) {
        let ret = detail[..paren_idx].trim();
        let params = detail[paren_idx..].trim_end();
        if ret.is_empty() {
            format!("{qualified_name}{params}")
        } else {
            format!("{ret} {qualified_name}{params}")
        }
    } else {
        format!("{detail} {qualified_name}()")
    }
}

fn trailing_paren_group_start(s: &str) -> Option<usize> {
    let trimmed = s.trim_end();
    if !trimmed.ends_with(')') {
        return None;
    }

    let mut depth = 0usize;
    let mut group_start = None;
    let mut last_group = None;
    let mut in_quote: Option<char> = None;
    let mut escaped = false;

    for (idx, ch) in trimmed.char_indices() {
        if let Some(q) = in_quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                in_quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => in_quote = Some(ch),
            '(' => {
                if depth == 0 {
                    group_start = Some(idx);
                }
                depth += 1;
            }
            ')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    last_group = group_start.take().map(|start| (start, idx));
                }
            }
            _ => {}
        }
    }

    let (start, end) = last_group?;
    (end == trimmed.len() - 1).then_some(start)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
    Field,
    Method,
    Constructor,
    EnumConstant,
}

/// An entry of a file's outline (a node of the symbol tree).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureNode {
    pub name: String,
    pub kind: SymbolKind,
    /// Extra information shown next to the name: the type of a field or the parameters of a
    /// method.
    pub detail: Option<String>,
    /// The whole declaration, without leading doc comments.
    pub range: TextRange,
    /// The name of the declaration. Always inside `range`.
    pub selection_range: TextRange,
    pub children: Vec<StructureNode>,
}

/// The outline of a file: its type declarations with their members, nested to match the source.
/// Declarations without a name (half-typed code) are left out and their members hoisted.
pub fn document_symbols(db: &dyn SourceDatabase, file_id: FileId) -> Vec<StructureNode> {
    let mut symbols = Vec::new();
    collect_symbols(&db.syntax_tree(file_id), &mut symbols);
    symbols
}

fn collect_symbols(parent: &SyntaxNode, out: &mut Vec<StructureNode>) {
    for node in parent.children() {
        match node.kind() {
            CLASS_DECL | INTERFACE_DECL | ENUM_DECL | RECORD_DECL | ANNOTATION_TYPE_DECL => {
                let kind = match node.kind() {
                    CLASS_DECL => SymbolKind::Class,
                    INTERFACE_DECL => SymbolKind::Interface,
                    ENUM_DECL => SymbolKind::Enum,
                    RECORD_DECL => SymbolKind::Record,
                    _ => SymbolKind::Annotation,
                };
                let mut children = Vec::new();
                for part in node.children() {
                    match part.kind() {
                        RECORD_COMPONENT_LIST => {
                            for component in
                                part.children().filter(|n| n.kind() == RECORD_COMPONENT)
                            {
                                let detail = component
                                    .children()
                                    .find(is_type)
                                    .map(|t| collapsed_text(&t));
                                push_symbol(
                                    &mut children,
                                    &component,
                                    &component,
                                    SymbolKind::Field,
                                    detail,
                                    Vec::new(),
                                );
                            }
                        }
                        CLASS_BODY | ENUM_BODY => collect_symbols(&part, &mut children),
                        _ => {}
                    }
                }
                push_symbol(out, &node, &node, kind, None, children);
            }
            FIELD_DECL => {
                let detail = node.children().find(is_type).map(|t| collapsed_text(&t));
                for declarator in node.children().filter(|n| n.kind() == VARIABLE_DECLARATOR) {
                    push_symbol(
                        out,
                        &declarator,
                        &declarator,
                        SymbolKind::Field,
                        detail.clone(),
                        Vec::new(),
                    );
                }
            }
            METHOD_DECL | CONSTRUCTOR_DECL => {
                let kind = if node.kind() == METHOD_DECL {
                    SymbolKind::Method
                } else {
                    SymbolKind::Constructor
                };
                let detail = Some(
                    node.children()
                        .find(|n| n.kind() == PARAM_LIST)
                        .map_or_else(|| "()".to_string(), |params| collapsed_text(&params)),
                );
                push_symbol(out, &node, &node, kind, detail, Vec::new());
            }
            ENUM_CONSTANT => push_symbol(
                out,
                &node,
                &node,
                SymbolKind::EnumConstant,
                None,
                Vec::new(),
            ),
            _ => {}
        }
    }
}

/// Adds a symbol named by the `NAME` child of `named`. A nameless declaration contributes only
/// its children.
fn push_symbol(
    out: &mut Vec<StructureNode>,
    node: &SyntaxNode,
    named: &SyntaxNode,
    kind: SymbolKind,
    detail: Option<String>,
    children: Vec<StructureNode>,
) {
    match named.children().find(|n| n.kind() == NAME) {
        Some(name) => out.push(StructureNode {
            name: name.text().to_string(),
            kind,
            detail,
            range: node.text_range(),
            selection_range: name.text_range(),
            children,
        }),
        None => out.extend(children),
    }
}

fn is_type(node: &SyntaxNode) -> bool {
    matches!(node.kind(), PATH_TYPE | PRIMITIVE_TYPE | ARRAY_TYPE)
}

/// The text of a node with every run of whitespace and comments squeezed into one space.
fn collapsed_text(node: &SyntaxNode) -> String {
    let mut out = String::new();
    for token in node
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        if matches!(token.kind(), WHITESPACE | LINE_COMMENT | BLOCK_COMMENT) {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push_str(token.text());
        }
    }
    out.trim().to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: TextRange,
    pub severity: Severity,
    pub message: String,
}

/// Diagnostics for a file: currently its lexical and syntax errors, ordered by position.
pub fn diagnostics(db: &dyn SourceDatabase, file_id: FileId) -> Vec<Diagnostic> {
    db.syntax_errors(file_id)
        .iter()
        .map(|error| Diagnostic {
            range: error.range,
            severity: Severity::Error,
            message: error.message.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hir::RootDatabase;
    use std::sync::Arc;

    fn db_with(text: &str) -> (RootDatabase, FileId) {
        let mut db = RootDatabase::default();
        let file_id = FileId(1);
        db.set_file_text(file_id, Arc::new(text.to_string()));
        db.set_file_language_level(file_id, hir::LanguageLevel::latest());
        db.set_workspace_files(Arc::new(vec![file_id]));
        (db, file_id)
    }

    fn db_with_files(files: &[(u32, &str)]) -> RootDatabase {
        let mut db = RootDatabase::default();
        let mut ids = Vec::with_capacity(files.len());
        for &(id, text) in files {
            let file_id = FileId(id);
            db.set_file_text(file_id, Arc::new(text.to_string()));
            db.set_file_language_level(file_id, hir::LanguageLevel::latest());
            ids.push(file_id);
        }
        db.set_workspace_files(Arc::new(ids));
        db
    }

    fn cursor(text: &str, marker: &str) -> u32 {
        let idx = text
            .find(marker)
            .unwrap_or_else(|| panic!("marker not found: {marker:?}"));
        (idx + marker.len()) as u32
    }

    fn slice(text: &str, range: TextRange) -> &str {
        &text[usize::from(range.start())..usize::from(range.end())]
    }

    #[test]
    fn test_hover_class_keyword() {
        let (db, file_id) = db_with("class Main { }");

        // Offset 1 is inside 'class'
        let hover = hover(&db, file_id, 1).expect("Hover should return result");
        assert_eq!(hover.content, "Keyword `class` declares a new class");
    }

    #[test]
    fn test_hover_token_boundary_prefers_following_token() {
        let (db, file_id) = db_with("class Main { }");

        // Offset 5 is the whitespace right after 'class'.
        assert!(hover(&db, file_id, 5).is_none());
    }

    #[test]
    fn test_hover_ident() {
        let (db, file_id) = db_with("class Main { String s; }");

        // Offset 7 is inside declared class 'Main' -> semantic hover.
        let main_hover = hover(&db, file_id, 7).expect("Hover should return result");
        assert_eq!(main_hover.content, "```java\nclass Main\n```");

        // Offset 13 is inside external unresolved 'String' -> fallback identifier hover.
        let string_hover = hover(&db, file_id, 13).expect("Hover should return fallback");
        assert_eq!(string_hover.content, "Identifier `String`");
    }

    #[test]
    fn test_hover_semantic_symbols() {
        let model = "\
package com.example;
public class Box<T> {
    public int size;
    public int compute(int x, String y) { return x; }
}
";
        let app = "\
package com.app;
import /*imp_pkg*/com.example./*imp_cls*/Box;
public class App<E extends Number> {
    private int count;
    void run(Box<String> /*param*/param) {
        int /*local*/local = /*field*/count;
        int s = /*use_param*/param./*mem_field*/size;
        int c = param./*mem_method*/compute(/*use_local*/local, \"\");
        String unknown = null;
    }
}
";
        let db = db_with_files(&[(1, model), (2, app)]);
        let app_id = FileId(2);

        let cls_hover = hover(&db, app_id, cursor(app, "/*imp_cls*/")).unwrap();
        assert_eq!(cls_hover.content, "```java\nclass com.example.Box<T>\n```");

        let pkg_hover = hover(&db, app_id, cursor(app, "/*imp_pkg*/")).unwrap();
        assert_eq!(pkg_hover.content, "```java\npackage com\n```");

        let field_hover = hover(&db, app_id, cursor(app, "/*mem_field*/")).unwrap();
        assert_eq!(
            field_hover.content,
            "```java\nint com.example.Box.size\n```"
        );

        let method_hover = hover(&db, app_id, cursor(app, "/*mem_method*/")).unwrap();
        assert_eq!(
            method_hover.content,
            "```java\nint com.example.Box.compute(int x, String y)\n```"
        );

        let local_hover = hover(&db, app_id, cursor(app, "/*use_local*/")).unwrap();
        assert_eq!(
            local_hover.content,
            "```java\nint local\n```\nlocal variable"
        );

        let param_hover = hover(&db, app_id, cursor(app, "/*use_param*/")).unwrap();
        assert_eq!(
            param_hover.content,
            "```java\nBox<String> param\n```\nparameter"
        );

        let own_field_hover = hover(&db, app_id, cursor(app, "/*field*/")).unwrap();
        assert_eq!(own_field_hover.content, "```java\nint com.app.App.count\n```");
    }

    #[test]
    fn test_hover_method_with_return_type_annotation_parens() {
        let text = "\
class C {
    <T> @Anno(\"x\") String /*m1*/foo(int a) { return \"\"; }
    java.util.List<@Anno(\"x\") String> /*m2*/bar(int a) { return null; }
}
";
        let (db, file_id) = db_with(text);

        let h1 = hover(&db, file_id, cursor(text, "/*m1*/")).unwrap();
        assert_eq!(
            h1.content,
            "```java\n@Anno(\"x\") String C.foo(int a)\n```"
        );

        let h2 = hover(&db, file_id, cursor(text, "/*m2*/")).unwrap();
        assert_eq!(
            h2.content,
            "```java\njava.util.List<@Anno(\"x\") String> C.bar(int a)\n```"
        );

        assert_eq!(
            format_method_signature("C.foo", Some("@Anno(\"x\") String (int a)")),
            "@Anno(\"x\") String C.foo(int a)"
        );
        assert_eq!(
            format_method_signature("C.foo", Some("@Anno(\")\") String (@Param(\"(\") int a)")),
            "@Anno(\")\") String C.foo(@Param(\"(\") int a)"
        );
    }

    #[test]
    fn goto_definition_within_single_file() {
        let text = "\
class Calc {
    int base = 10;
    int helper(int v) { return v + 1; }
    int compute(int param) {
        int local = /*use_param*/param + /*use_field*/base;
        return /*use_method*/helper(/*use_local*/local);
    }
}
";
        let (db, file_id) = db_with(text);

        let param_target = goto_definition(&db, file_id, cursor(text, "/*use_param*/")).unwrap();
        assert_eq!(param_target.file_id, file_id);
        assert_eq!(slice(text, param_target.focus_range), "param");
        assert_eq!(slice(text, param_target.full_range), "int param");

        let field_target = goto_definition(&db, file_id, cursor(text, "/*use_field*/")).unwrap();
        assert_eq!(field_target.file_id, file_id);
        assert_eq!(slice(text, field_target.focus_range), "base");
        assert_eq!(slice(text, field_target.full_range), "base = 10");

        let method_target = goto_definition(&db, file_id, cursor(text, "/*use_method*/")).unwrap();
        assert_eq!(method_target.file_id, file_id);
        assert_eq!(slice(text, method_target.focus_range), "helper");
        assert!(slice(text, method_target.full_range).starts_with("int helper(int v)"));

        let local_target = goto_definition(&db, file_id, cursor(text, "/*use_local*/")).unwrap();
        assert_eq!(local_target.file_id, file_id);
        assert_eq!(slice(text, local_target.focus_range), "local");
    }

    #[test]
    fn goto_definition_across_files_import_and_same_package() {
        let helper = "\
package com.util;
public class Helper {
    public int value;
    public int work() { return value; }
}
";
        let peer = "\
package com.app;
public class Peer {
    public void ping() {}
}
";
        let app = "\
package com.app;
import com.util./*imp*/Helper;
public class App {
    void run(/*peer*/Peer p) {
        /*cls*/Helper h = new /*ctor_cls*/Helper();
        int v = h./*field*/value;
        int w = h./*method*/work();
        p./*peer_method*/ping();
    }
}
";
        let helper_id = FileId(1);
        let peer_id = FileId(2);
        let app_id = FileId(3);
        let db = db_with_files(&[(1, helper), (2, peer), (3, app)]);

        let imp_target = goto_definition(&db, app_id, cursor(app, "/*imp*/")).unwrap();
        assert_eq!(imp_target.file_id, helper_id);
        assert_eq!(slice(helper, imp_target.focus_range), "Helper");

        let cls_target = goto_definition(&db, app_id, cursor(app, "/*cls*/")).unwrap();
        assert_eq!(cls_target.file_id, helper_id);
        assert_eq!(slice(helper, cls_target.focus_range), "Helper");

        let new_target = goto_definition(&db, app_id, cursor(app, "/*ctor_cls*/")).unwrap();
        assert_eq!(new_target.file_id, helper_id);
        assert_eq!(slice(helper, new_target.focus_range), "Helper");

        let field_target = goto_definition(&db, app_id, cursor(app, "/*field*/")).unwrap();
        assert_eq!(field_target.file_id, helper_id);
        assert_eq!(slice(helper, field_target.focus_range), "value");

        let method_target = goto_definition(&db, app_id, cursor(app, "/*method*/")).unwrap();
        assert_eq!(method_target.file_id, helper_id);
        assert_eq!(slice(helper, method_target.focus_range), "work");

        let peer_target = goto_definition(&db, app_id, cursor(app, "/*peer*/")).unwrap();
        assert_eq!(peer_target.file_id, peer_id);
        assert_eq!(slice(peer, peer_target.focus_range), "Peer");

        let peer_method_target =
            goto_definition(&db, app_id, cursor(app, "/*peer_method*/")).unwrap();
        assert_eq!(peer_method_target.file_id, peer_id);
        assert_eq!(slice(peer, peer_method_target.focus_range), "ping");
    }

    /// Renders the outline as indented `Kind name detail` lines.
    fn outline(text: &str) -> String {
        fn render(nodes: &[StructureNode], depth: usize, out: &mut String) {
            for node in nodes {
                let detail = node
                    .detail
                    .as_deref()
                    .map(|d| format!(" {d}"))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "{}{:?} {}{detail}\n",
                    "  ".repeat(depth),
                    node.kind,
                    node.name
                ));
                render(&node.children, depth + 1, out);
            }
        }
        let (db, file_id) = db_with(text);
        let mut out = String::new();
        render(&document_symbols(&db, file_id), 0, &mut out);
        out
    }

    #[test]
    fn outline_of_nested_declarations() {
        let text = "\
package a;
import b.C;
public class Outer<T> extends Base {
    private int count, limit = 5;
    static final String NAME = \"x\";
    Outer() {}
    public <U> U map(Function<T, U> f, int... rest) throws Exception { return null; }
    interface Callback { void call(); }
    enum Kind { A, B(1) { }, C }
    record Pair(int a, String b) { Pair { } }
    @interface Marker { int value() default 1; }
    { init(); }
}
";
        assert_eq!(
            outline(text),
            "\
Class Outer
  Field count int
  Field limit int
  Field NAME String
  Constructor Outer ()
  Method map (Function<T, U> f, int... rest)
  Interface Callback
    Method call ()
  Enum Kind
    EnumConstant A
    EnumConstant B
    EnumConstant C
  Record Pair
    Field a int
    Field b String
    Constructor Pair ()
  Annotation Marker
    Method value ()
"
        );
    }

    #[test]
    fn outline_ranges_are_precise() {
        let text = "/** Docs */\nclass A {\n    int x = 1, yy;\n    void run() {}\n}\n";
        let (db, file_id) = db_with(text);
        let symbols = document_symbols(&db, file_id);

        let slice = |range: TextRange| &text[usize::from(range.start())..usize::from(range.end())];
        let class = &symbols[0];
        assert_eq!(slice(class.selection_range), "A");
        assert!(
            slice(class.range).starts_with("class A {"),
            "doc comments are excluded"
        );
        assert!(slice(class.range).ends_with('}'));

        let fields: Vec<_> = class
            .children
            .iter()
            .filter(|c| c.kind == SymbolKind::Field)
            .collect();
        assert_eq!(slice(fields[0].range), "x = 1");
        assert_eq!(slice(fields[0].selection_range), "x");
        assert_eq!(slice(fields[1].range), "yy");

        for node in &class.children {
            assert!(node.range.contains_range(node.selection_range));
            assert!(class.range.contains_range(node.range));
        }
    }

    #[test]
    fn outline_of_broken_code_keeps_what_has_names() {
        // The first class has no name and the field has none either; everything else survives.
        let text = "class { void lost() {} }\nclass B {\n    int ;\n    void kept() {}\n}\n";
        assert_eq!(
            outline(text),
            "\
Method lost ()
Class B
  Method kept ()
"
        );
    }

    #[test]
    fn outline_ignores_local_declarations_inside_bodies() {
        let text = "class A {\n    void f() {\n        class Local { int hidden; }\n        int local = 1;\n    }\n    int g;\n}\n";
        assert_eq!(
            outline(text),
            "\
Class A
  Method f ()
  Field g int
"
        );
    }

    #[test]
    fn outline_of_empty_and_declaration_free_files() {
        assert_eq!(outline(""), "");
        assert_eq!(outline("package a;\nimport b.C;\n"), "");
    }

    #[test]
    fn outline_of_compact_source_file() {
        let text = "#!/usr/bin/env java --source 25\nimport module java.base;\nint count = 1;\nvoid main(String[] args) {}\n";
        assert_eq!(
            outline(text),
            "\
Field count int
Method main (String[] args)
"
        );
    }

    #[test]
    fn no_diagnostics_for_valid_code() {
        let (db, file_id) = db_with("package a;\nimport b.C;\nclass Main { int x; }\n");
        assert_eq!(diagnostics(&db, file_id), vec![]);
    }

    #[test]
    fn syntax_errors_become_diagnostics() {
        let (db, file_id) = db_with("class Main {\n    int x\n}\n");

        // `int x` is missing its semicolon, reported just after `x` (which is at offset 21).
        let diagnostics = diagnostics(&db, file_id);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "expected ';'");
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].range, TextRange::empty(22.into()));
    }

    #[test]
    fn lexical_errors_become_diagnostics() {
        let (db, file_id) = db_with("class A { String s = \"oops; }");
        let messages: Vec<_> = diagnostics(&db, file_id)
            .into_iter()
            .map(|d| d.message)
            .collect();
        assert!(
            messages.contains(&"unterminated string literal".to_string()),
            "{messages:?}"
        );
    }
}
