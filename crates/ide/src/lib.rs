mod binary;

use std::sync::Arc;

use hir::{DefDatabase, ResolvedSymbol, ResolvedSymbolKind, SourceDatabase, SymbolDetail};
use syntax::{
    SyntaxNode,
    ext::{collapsed_text, name_child, type_child},
    syntax_kind::SyntaxKind::*,
};

pub use binary::{
    append_javadoc_to_hover, extract_javadoc_from_html, extract_javadoc_from_source,
    locate_binary_symbol_in_source, synthesize_binary_stub,
};
pub use hir::{
    BinaryMemberSymbol, BinaryModuleInfo, BinaryOrigin, BinaryTarget, BinaryTypeSymbol,
    Cancellable, Cancelled, ClassfileError, ClasspathIndex, DeltaSet, FileId, LanguageLevel,
    MemberKind, ModuleData, ModuleDelta, ModuleDependency, ModuleId, ModuleTarget, ParamSummary,
    ParsedClassfile, Release, ResourceRoot, RootScope, SmolStr, SourceRoot, TypeKind,
    WorkspaceDelta, WorkspaceLayout, infer_workspace_layout_from_sources, is_path_within,
    normalize_rel_path, parse_class_bytes, parse_manifest_multi_release, parse_shebang_level,
    select_mrjar_entries,
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

    pub fn set_workspace_modules(&mut self, modules: Vec<(ModuleId, Arc<Vec<FileId>>)>) {
        self.db.set_workspace_modules(modules);
    }

    pub fn set_classpath_index(&mut self, index: Arc<ClasspathIndex>) {
        self.db.set_classpath_index(index);
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

    pub fn classpath_index_stats(&self) -> Cancellable<(usize, usize)> {
        hir::catch_cancelled(|| {
            let index = self.db.classpath_index();
            (index.type_count(), index.module_count())
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverResult {
    pub content: String,
    pub binary_target: Option<BinaryTarget>,
}

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

pub fn goto_definition(
    db: &dyn DefDatabase,
    file_id: FileId,
    offset: u32,
) -> Option<NavigationTarget> {
    let resolved = hir::resolve_at_offset(db, file_id, offset)?;
    if let Some((file_id, full_range, focus_range)) = resolved.location() {
        return Some(NavigationTarget::Source {
            file_id,
            full_range,
            focus_range,
        });
    }
    resolved.binary_target.map(NavigationTarget::Binary)
}

pub fn hover(db: &dyn DefDatabase, file_id: FileId, offset: u32) -> Option<HoverResult> {
    if let Some(resolved) = hir::resolve_at_offset(db, file_id, offset) {
        return Some(HoverResult {
            content: format_resolved_hover(&resolved),
            binary_target: resolved.binary_target,
        });
    }

    let syntax_node = db.syntax_tree(file_id);

    // Hover positions refer to the character under the pointer, so prefer the token that
    // starts at `offset` over the one that ends there.
    let token = syntax_node.token_at_offset(offset.into()).right_biased()?;

    match token.kind() {
        CLASS_KW => Some(HoverResult {
            content: "Keyword `class` declares a new class".to_string(),
            binary_target: None,
        }),
        IDENT => Some(HoverResult {
            content: format!("Identifier `{}`", token.text()),
            binary_target: None,
        }),
        _ => None,
    }
}

fn format_resolved_hover(sym: &ResolvedSymbol) -> String {
    let base = match sym.kind {
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
    };

    match &sym.binary_target {
        Some(target) => match &target.origin {
            BinaryOrigin::Jdk {
                module_name: Some(module_name),
            } => format!("{base}\n*from JDK module `{module_name}`*"),
            BinaryOrigin::Jdk { module_name: None } => format!("{base}\n*from JDK*"),
            BinaryOrigin::Jar { label } => format!("{base}\n*from `{label}`*"),
        },
        None => base,
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
        assert_eq!(
            own_field_hover.content,
            "```java\nint com.app.App.count\n```"
        );
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
        assert_eq!(h1.content, "```java\n@Anno(\"x\") String C.foo(int a)\n```");

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
            binary_target: None,
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

    fn assert_source_target(
        target: NavigationTarget,
        expected_file_id: FileId,
    ) -> (TextRange, TextRange) {
        match target {
            NavigationTarget::Source {
                file_id,
                full_range,
                focus_range,
            } => {
                assert_eq!(file_id, expected_file_id);
                (full_range, focus_range)
            }
            NavigationTarget::Binary(bin) => {
                panic!("expected NavigationTarget::Source, got Binary({bin:?})");
            }
        }
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

        let (param_full, param_focus) = assert_source_target(
            goto_definition(&db, file_id, cursor(text, "/*use_param*/")).unwrap(),
            file_id,
        );
        assert_eq!(slice(text, param_focus), "param");
        assert_eq!(slice(text, param_full), "int param");

        let (field_full, field_focus) = assert_source_target(
            goto_definition(&db, file_id, cursor(text, "/*use_field*/")).unwrap(),
            file_id,
        );
        assert_eq!(slice(text, field_focus), "base");
        assert_eq!(slice(text, field_full), "base = 10");

        let (method_full, method_focus) = assert_source_target(
            goto_definition(&db, file_id, cursor(text, "/*use_method*/")).unwrap(),
            file_id,
        );
        assert_eq!(slice(text, method_focus), "helper");
        assert!(slice(text, method_full).starts_with("int helper(int v)"));

        let (_, local_focus) = assert_source_target(
            goto_definition(&db, file_id, cursor(text, "/*use_local*/")).unwrap(),
            file_id,
        );
        assert_eq!(slice(text, local_focus), "local");
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

        let (_, imp_focus) = assert_source_target(
            goto_definition(&db, app_id, cursor(app, "/*imp*/")).unwrap(),
            helper_id,
        );
        assert_eq!(slice(helper, imp_focus), "Helper");

        let (_, cls_focus) = assert_source_target(
            goto_definition(&db, app_id, cursor(app, "/*cls*/")).unwrap(),
            helper_id,
        );
        assert_eq!(slice(helper, cls_focus), "Helper");

        let (_, new_focus) = assert_source_target(
            goto_definition(&db, app_id, cursor(app, "/*ctor_cls*/")).unwrap(),
            helper_id,
        );
        assert_eq!(slice(helper, new_focus), "Helper");

        let (_, field_focus) = assert_source_target(
            goto_definition(&db, app_id, cursor(app, "/*field*/")).unwrap(),
            helper_id,
        );
        assert_eq!(slice(helper, field_focus), "value");

        let (_, method_focus) = assert_source_target(
            goto_definition(&db, app_id, cursor(app, "/*method*/")).unwrap(),
            helper_id,
        );
        assert_eq!(slice(helper, method_focus), "work");

        let (_, peer_focus) = assert_source_target(
            goto_definition(&db, app_id, cursor(app, "/*peer*/")).unwrap(),
            peer_id,
        );
        assert_eq!(slice(peer, peer_focus), "Peer");

        let (_, peer_method_focus) = assert_source_target(
            goto_definition(&db, app_id, cursor(app, "/*peer_method*/")).unwrap(),
            peer_id,
        );
        assert_eq!(slice(peer, peer_method_focus), "ping");
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
        assert!(diags[0].message.contains(
            "declared package 'wrong.pkg' does not match package-info.java package 'com.example'"
        ));
    }

    #[test]
    fn analysis_host_snapshot_isolation_and_cancellation() {
        let mut host = AnalysisHost::new();
        let file1 = FileId(1);
        let file2 = FileId(2);
        host.set_file_text(
            file1,
            Arc::new("package p1;\nclass Before { int x; }".to_string()),
        );
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
        host.set_file_text(
            file1,
            Arc::new("package p1;\nclass After { int y }".to_string()),
        );
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

    fn sample_ide_classpath() -> ClasspathIndex {
        let mut cp = ClasspathIndex::default();
        let jdk_base = BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base")),
        };
        let jdk8 = BinaryOrigin::Jdk { module_name: None };
        let jar = BinaryOrigin::Jar {
            label: "lib/demo-1.0.jar".to_string(),
        };

        cp.insert_module(BinaryModuleInfo {
            name: SmolStr::new("java.base"),
            exports: vec!["java.lang".to_string(), "java.util".to_string()],
            requires_transitive: vec![],
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("String"),
            qualified_name: "java.lang.String".to_string(),
            internal_name: "java/lang/String".to_string(),
            package: "java.lang".to_string(),
            enclosing_type: None,
            kind: TypeKind::Class,
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            type_params: vec![],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![BinaryMemberSymbol {
                owner_type: "java.lang.String".to_string(),
                name: SmolStr::new("length"),
                kind: MemberKind::Method,
                is_public: true,
                is_static: false,
                type_ref: Some("int".to_string()),
                params: vec![],
                is_varargs: false,
            }],
            nested_types: vec![],
            origin: jdk_base,
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Legacy"),
            qualified_name: "java.util.Legacy".to_string(),
            internal_name: "java/util/Legacy".to_string(),
            package: "java.util".to_string(),
            enclosing_type: None,
            kind: TypeKind::Class,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: false,
            type_params: vec![],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![],
            nested_types: vec![],
            origin: jdk8,
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Assert"),
            qualified_name: "org.demo.Assert".to_string(),
            internal_name: "org/demo/Assert".to_string(),
            package: "org.demo".to_string(),
            enclosing_type: None,
            kind: TypeKind::Class,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: false,
            type_params: vec![],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![BinaryMemberSymbol {
                owner_type: "org.demo.Assert".to_string(),
                name: SmolStr::new("VALUE"),
                kind: MemberKind::Field,
                is_public: true,
                is_static: true,
                type_ref: Some("int".to_string()),
                params: vec![],
                is_varargs: false,
            }],
            nested_types: vec![],
            origin: jar,
        });

        cp.finalize();
        cp
    }

    #[test]
    fn binary_hover_origin_and_navigation_target() {
        let app = "\
package com.app;
import java.util./*leg*/Legacy;
import org.demo./*jar*/Assert;
class App {
    void run(/*str*/String s) {
        int n = s./*len*/length();
        int v = Assert./*val*/VALUE;
    }
}
";
        let mut host = AnalysisHost::new();
        let file_id = FileId(1);
        host.set_file_text(file_id, Arc::new(app.to_string()));
        host.set_workspace_files(Arc::new(vec![file_id]));

        let snap_empty = host.analysis();
        assert_eq!(snap_empty.classpath_index_stats().unwrap(), (0, 0));
        drop(snap_empty);

        host.set_classpath_index(Arc::new(sample_ide_classpath()));
        let snap = host.analysis();
        assert_eq!(snap.classpath_index_stats().unwrap(), (3, 1));

        let str_hover = snap.hover(file_id, cursor(app, "/*str*/")).unwrap().unwrap();
        assert_eq!(
            str_hover.content,
            "```java\nclass java.lang.String\n```\n*from JDK module `java.base`*"
        );
        assert_eq!(
            str_hover.binary_target.as_ref().map(|t| t.qualified_name.as_str()),
            Some("java.lang.String")
        );

        let len_hover = snap.hover(file_id, cursor(app, "/*len*/")).unwrap().unwrap();
        assert_eq!(
            len_hover.content,
            "```java\nint java.lang.String.length()\n```\n*from JDK module `java.base`*"
        );
        assert_eq!(
            len_hover
                .binary_target
                .as_ref()
                .and_then(|t| t.member_name.as_deref()),
            Some("length")
        );

        let leg_hover = snap.hover(file_id, cursor(app, "/*leg*/")).unwrap().unwrap();
        assert_eq!(
            leg_hover.content,
            "```java\nclass java.util.Legacy\n```\n*from JDK*"
        );

        let val_hover = snap.hover(file_id, cursor(app, "/*val*/")).unwrap().unwrap();
        assert_eq!(
            val_hover.content,
            "```java\nint org.demo.Assert.VALUE\n```\n*from `lib/demo-1.0.jar`*"
        );

        // Binary goto_definition returns NavigationTarget::Binary
        let str_def = snap
            .goto_definition(file_id, cursor(app, "/*str*/"))
            .unwrap()
            .unwrap();
        assert_eq!(
            str_def,
            NavigationTarget::Binary(BinaryTarget {
                origin: BinaryOrigin::Jdk {
                    module_name: Some(SmolStr::new("java.base")),
                },
                internal_name: "java/lang/String".to_string(),
                qualified_name: "java.lang.String".to_string(),
                member_name: None,
            })
        );

        let jar_def = snap
            .goto_definition(file_id, cursor(app, "/*jar*/"))
            .unwrap()
            .unwrap();
        assert_eq!(
            jar_def,
            NavigationTarget::Binary(BinaryTarget {
                origin: BinaryOrigin::Jar {
                    label: "lib/demo-1.0.jar".to_string(),
                },
                internal_name: "org/demo/Assert".to_string(),
                qualified_name: "org.demo.Assert".to_string(),
                member_name: None,
            })
        );
    }

    #[test]
    fn workspace_source_shadows_binary_in_hover_and_goto_definition() {
        let ws_assert = "package org.demo;\npublic class Assert {}\n";
        let app = "\
package com.app;
import org.demo./*jar*/Assert;
class App { Assert a; }
";
        let mut host = AnalysisHost::new();
        host.set_file_text(FileId(1), Arc::new(ws_assert.to_string()));
        host.set_file_text(FileId(2), Arc::new(app.to_string()));
        host.set_workspace_files(Arc::new(vec![FileId(1), FileId(2)]));
        host.set_classpath_index(Arc::new(sample_ide_classpath()));

        let snap = host.analysis();
        let h = snap.hover(FileId(2), cursor(app, "/*jar*/")).unwrap().unwrap();
        assert_eq!(h.content, "```java\nclass org.demo.Assert\n```");
        assert!(h.binary_target.is_none());

        let def = snap
            .goto_definition(FileId(2), cursor(app, "/*jar*/"))
            .unwrap()
            .unwrap();
        let (_, focus) = assert_source_target(def, FileId(1));
        assert_eq!(slice(ws_assert, focus), "Assert");
    }
}
