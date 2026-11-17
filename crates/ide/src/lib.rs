use hir::{FileId, SourceDatabase};
use syntax::{syntax_kind::SyntaxKind::*, SyntaxNode};

pub use syntax::TextRange;

pub struct HoverResult {
    pub content: String,
}

pub fn hover(db: &dyn SourceDatabase, file_id: FileId, offset: u32) -> Option<HoverResult> {
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
        (db, file_id)
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
        let (db, file_id) = db_with("class Main { }");

        // Offset 7 is inside 'Main'
        let hover = hover(&db, file_id, 7).expect("Hover should return result");
        assert_eq!(hover.content, "Identifier `Main`");
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
