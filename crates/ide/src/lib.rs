use std::sync::Arc;

use hir::{
    DefDatabase, ResolvedSymbol, ResolvedSymbolKind, SourceDatabase, SymbolDetail,
};
use syntax::{
    ext::{collapsed_text, name_child, type_child},
    syntax_kind::SyntaxKind::*,
    SyntaxNode,
};

pub use hir::{
    infer_workspace_layout_from_sources, is_path_within, normalize_rel_path, parse_shebang_level,
    Cancellable, Cancelled, DeltaSet, FileId, LanguageLevel, ModuleData, ModuleDelta,
    ModuleDependency, ModuleId, ModuleTarget, Release, ResourceRoot, RootScope, SourceRoot,
    WorkspaceDelta, WorkspaceLayout,
};
pub use syntax::TextRange;

#[derive(Default)]
pub struct AnalysisHost {
    db: hir::RootDatabase,
}

impl AnalysisHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn analysis(&self) -> Analysis {
        Analysis {
            db: self.db.clone(),
        }
    }

    pub fn set_file_text(&mut self, file_id: FileId, text: Arc<String>) {
        self.db.set_file_text(file_id, text);
    }

    pub fn set_file_language_level(&mut self, file_id: FileId, level: LanguageLevel) {
        self.db.set_file_language_level(file_id, level);
    }

    pub fn set_workspace_files(&mut self, file_ids: Arc<Vec<FileId>>) {
        self.db.set_workspace_files(file_ids);
    }

    pub fn remove_file(&mut self, file_id: FileId) {
        self.db.remove_file(file_id);
    }
}

pub struct Analysis {
    db: hir::RootDatabase,
}

impl Analysis {
    pub fn file_text(&self, file_id: FileId) -> Cancellable<Arc<String>> {
        hir::catch_cancelled(|| self.db.file_text(file_id))
    }

    pub fn hover(&self, file_id: FileId, offset: u32) -> Cancellable<Option<HoverResult>> {
        hir::catch_cancelled(|| hover(&self.db, file_id, offset))
    }

    pub fn goto_definition(
        &self,
        file_id: FileId,
        offset: u32,
    ) -> Cancellable<Option<NavigationTarget>> {
        hir::catch_cancelled(|| goto_definition(&self.db, file_id, offset))
    }

    pub fn document_symbols(&self, file_id: FileId) -> Cancellable<Vec<StructureNode>> {
        hir::catch_cancelled(|| document_symbols(&self.db, file_id))
    }

    pub fn diagnostics(&self, file_id: FileId) -> Cancellable<Vec<Diagnostic>> {
        hir::catch_cancelled(|| diagnostics(&self.db, file_id))
    }

    pub fn diagnostics_with_layout(
        &self,
        file_id: FileId,
        layout: Option<(&WorkspaceLayout, &str)>,
    ) -> Cancellable<Vec<Diagnostic>> {
        hir::catch_cancelled(|| diagnostics_with_layout(&self.db, file_id, layout))
    }

    pub fn workspace_index_stats(&self) -> Cancellable<(usize, usize)> {
        hir::catch_cancelled(|| {
            let index = self.db.workspace_index();
            (index.package_count(), index.type_count())
        })
    }
}

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
            let tparams = match &sym.detail {
                Some(SymbolDetail::TypeParams(params)) if !params.is_empty() => {
                    format!("<{}>", params.join(", "))
                }
                _ => String::new(),
            };
            format!("```java\n{kw} {}{tparams}\n```", sym.qualified_name)
        }
        ResolvedSymbolKind::Field | ResolvedSymbolKind::RecordComponent => match &sym.detail {
            Some(SymbolDetail::TypeRef(ty)) => format!("```java\n{ty} {}\n```", sym.qualified_name),
            _ => format!("```java\n{}\n```", sym.qualified_name),
        },
        ResolvedSymbolKind::Method => {
            let sig = match &sym.detail {
                Some(SymbolDetail::Method {
                    return_type: Some(ret),
                    params,
                }) => format!("{ret} {}{params}", sym.qualified_name),
                Some(SymbolDetail::Method {
                    return_type: None,
                    params,
                }) => format!("{}{params}", sym.qualified_name),
                _ => format!("{}()", sym.qualified_name),
            };
            format!("```java\n{sig}\n```")
        }
        ResolvedSymbolKind::Constructor => {
            let params = match &sym.detail {
                Some(SymbolDetail::Constructor { params }) => params.as_str(),
                _ => "()",
            };
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
            let ty = match &sym.detail {
                Some(SymbolDetail::TypeRef(ty)) => ty.as_str(),
                _ => "var",
            };
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
            Some(SymbolDetail::Bounds(bounds)) if !bounds.is_empty() => format!(
                "```java\ntype parameter {} extends {}\n```",
                sym.name,
                bounds.join(" & ")
            ),
            _ => format!("```java\ntype parameter {}\n```", sym.name),
        },
        ResolvedSymbolKind::Package => format!("```java\npackage {}\n```", sym.qualified_name),
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
                                let detail = type_child(&component).map(|t| collapsed_text(&t));
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
                let detail = type_child(&node).map(|t| collapsed_text(&t));
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
    match name_child(named) {
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

/// Diagnostics for a file: lexical/syntax errors plus optional package-vs-layout mismatch errors.
pub fn diagnostics_with_layout(
    db: &dyn SourceDatabase,
    file_id: FileId,
    layout: Option<(&WorkspaceLayout, &str)>,
) -> Vec<Diagnostic> {
    let mut diags: Vec<Diagnostic> = db
        .syntax_errors(file_id)
        .iter()
        .map(|error| Diagnostic {
            range: error.range,
            severity: Severity::Error,
            message: error.message.clone(),
        })
        .collect();

    if let Some((layout, rel_path)) = layout {
        let syntax_tree = db.syntax_tree(file_id);
        if let Some(pkg_diag) = layout.check_file_package(file_id, rel_path, &syntax_tree) {
            diags.push(Diagnostic {
                range: pkg_diag.range,
                severity: Severity::Error,
                message: pkg_diag.message(),
            });
        }
    }

    diags
}

/// Diagnostics for a file: currently its lexical and syntax errors, ordered by position.
pub fn diagnostics(db: &dyn SourceDatabase, file_id: FileId) -> Vec<Diagnostic> {
    diagnostics_with_layout(db, file_id, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hir::test_fixture::{cursor, db_with, db_with_files, slice};

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
    <T> @Anno(\")\") String /*m3*/baz(java.util.List<@Param(\"(\") String> a) { return \"\"; }
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

        let h3 = hover(&db, file_id, cursor(text, "/*m3*/")).unwrap();
        assert_eq!(
            h3.content,
            "```java\n@Anno(\")\") String C.baz(java.util.List<@Param(\"(\") String> a)\n```"
        );

        let structured = ResolvedSymbol {
            file_id: None,
            full_range: None,
            focus_range: None,
            name: "baz".into(),
            qualified_name: "C.baz".to_string(),
            detail: Some(SymbolDetail::Method {
                return_type: Some("@Anno(\")\") String".to_string()),
                params: "(@Param(\"(\") int a)".to_string(),
            }),
            kind: ResolvedSymbolKind::Method,
        };
        assert_eq!(
            format_resolved_hover(&structured),
            "```java\n@Anno(\")\") String C.baz(@Param(\"(\") int a)\n```"
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

    #[test]
    fn diagnostics_with_layout_reports_package_mismatch() {
        let pkg_info = "package com.example;\n";
        let bad_file = "package wrong.pkg;\nclass Broken {}\n";
        let db = db_with_files(&[(1, pkg_info), (2, bad_file)]);
        let layout = hir::infer_workspace_layout_from_sources(
            &[
                (FileId(1), "src/com/example/package-info.java", pkg_info),
                (FileId(2), "src/com/example/Broken.java", bad_file),
            ],
            &[],
            hir::LanguageLevel::latest(),
            &hir::WorkspaceDelta::default(),
        );

        assert!(diagnostics(&db, FileId(2)).is_empty());
        let diags = diagnostics_with_layout(
            &db,
            FileId(2),
            Some((&layout, "src/com/example/Broken.java")),
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0]
            .message
            .contains("declared package 'wrong.pkg' does not match package-info.java package 'com.example'"));
    }

    #[test]
    fn analysis_host_snapshot_isolation_and_cancellation() {
        let mut host = AnalysisHost::new();
        let file1 = FileId(1);
        let file2 = FileId(2);
        host.set_file_text(file1, Arc::new("package p1;\nclass Before { int x; }".to_string()));
        host.set_workspace_files(Arc::new(vec![file1]));

        let snap1 = host.analysis();
        let syms1 = snap1.document_symbols(file1).unwrap();
        assert_eq!(syms1[0].name, "Before");
        assert_eq!(snap1.diagnostics(file1).unwrap(), vec![]);
        assert_eq!(snap1.workspace_index_stats().unwrap(), (1, 1));
        // Drop `snap1` before mutating `host` on the same thread (Salsa waits for live
        // snapshots to drop before acquiring the write lock).
        drop(snap1);

        // Mutating the host updates subsequent snapshots without invalidating results already
        // obtained from `snap1`.
        host.set_file_text(file1, Arc::new("package p1;\nclass After { int y }".to_string()));
        host.set_file_text(file2, Arc::new("package p2;\nclass Helper {}".to_string()));
        host.set_workspace_files(Arc::new(vec![file1, file2]));

        let snap2 = host.analysis();
        assert_eq!(snap2.document_symbols(file1).unwrap()[0].name, "After");
        assert_eq!(snap2.diagnostics(file1).unwrap().len(), 1);
        assert_eq!(snap2.workspace_index_stats().unwrap(), (2, 2));

        // Earlier extracted results remain intact.
        assert_eq!(syms1[0].name, "Before");
        drop(snap2);

        // Concurrent snapshot queries during host mutations never panic: each query
        // returns either `Ok(...)` or `Err(Cancelled)`.
        let snap_concurrent = host.analysis();
        let handle = std::thread::spawn(move || {
            for _ in 0..32 {
                match snap_concurrent.document_symbols(file1) {
                    Ok(nodes) => assert!(!nodes.is_empty()),
                    Err(Cancelled) => break,
                }
            }
        });
        for i in 0..16 {
            host.set_file_text(file1, Arc::new(format!("class Iter{i} {{ int v; }}")));
        }
        handle.join().unwrap();
    }
}
