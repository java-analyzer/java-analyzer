use std::sync::Arc;

use hir::{
    ResolvedSymbol, ResolvedSymbolKind, SymbolDetail,
    test_fixture::{cursor, db_with, db_with_files, slice},
};

use super::*;
use crate::hover::format_resolved_hover;

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
        overrides: None,
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
    assert_eq!(diagnostics[0].code, "syntax/expected-token");
    assert_eq!(diagnostics[0].message, "';' expected");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(diagnostics[0].range, TextRange::empty(22.into()));
    assert!(diagnostics[0].help.is_some());
}

#[test]
fn lexical_errors_become_diagnostics() {
    let (db, file_id) = db_with("class A { String s = \"oops; }");
    let diags = diagnostics(&db, file_id);
    assert!(
        diags
            .iter()
            .any(|d| d.code == "syntax/unclosed-literal"
                && d.message == "unclosed string literal"),
        "{diags:?}"
    );
}

#[test]
fn classify_syntax_error_covers_all_categories() {
    assert_eq!(
        classify_syntax_error("expected ';'"),
        (
            "syntax/expected-token",
            "';' expected".to_string(),
            Some("add a semicolon at the end of the statement".to_string()),
        )
    );
    assert_eq!(
        classify_syntax_error("unterminated string literal").0,
        "syntax/unclosed-literal"
    );
    assert_eq!(
        classify_syntax_error("unterminated block comment").0,
        "syntax/unclosed-comment"
    );
    assert_eq!(
        classify_syntax_error("expected an expression"),
        (
            "syntax/illegal-start",
            "illegal start of expression".to_string(),
            Some("provide a valid expression here".to_string()),
        )
    );
    assert_eq!(
        classify_syntax_error("records require language level 16"),
        (
            "syntax/feature-not-supported",
            "records require language level 16".to_string(),
            Some("set the project language level to Java 16 or newer".to_string()),
        )
    );
    assert_eq!(
        classify_syntax_error("records are a preview feature in java 14; enable preview"),
        (
            "syntax/preview-required",
            "records are a preview feature in Java 14".to_string(),
            Some("pass `--enable-preview` to enable preview language features".to_string()),
        )
    );
    assert_eq!(
        classify_syntax_error("expression is too long").0,
        "syntax/parse-error"
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
    assert_eq!(diags[0].code, "workspace/package-mismatch");
    assert_eq!(diags[0].severity, Severity::Error);
    assert!(diags[0].message.contains(
        "declared package 'wrong.pkg' does not match package-info.java package 'com.example'"
    ));
    assert!(diags[0].help.is_some());
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
        nullness_scope: NullnessScope::Inherited,
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
        identity: hir::TypeIdentity::Identity,
        is_public: true,
        is_static: false,
        is_final: true,
        is_abstract: false,
        nullness_scope: NullnessScope::Inherited,
        type_params: vec![],
        type_param_bounds: Default::default(),
        super_class: Some("java.lang.Object".to_string()),
        super_class_generic: None,
        interfaces: vec![],
        interfaces_generic: vec![],
        permitted_subclasses: vec![],
        members: vec![BinaryMemberSymbol {
            owner_type: "java.lang.String".to_string(),
            name: SmolStr::new("length"),
            kind: MemberKind::Method,
            is_public: true,
            is_static: false,
            is_abstract: false,
            nullness_scope: NullnessScope::Inherited,
            decl_nullability: None,
            type_params: vec![],
            type_param_bounds: Default::default(),
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
        identity: hir::TypeIdentity::Identity,
        is_public: true,
        is_static: false,
        is_final: false,
        is_abstract: false,
        nullness_scope: NullnessScope::Inherited,
        type_params: vec![],
        type_param_bounds: Default::default(),
        super_class: Some("java.lang.Object".to_string()),
        super_class_generic: None,
        interfaces: vec![],
        interfaces_generic: vec![],
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
        identity: hir::TypeIdentity::Identity,
        is_public: true,
        is_static: false,
        is_final: false,
        is_abstract: false,
        nullness_scope: NullnessScope::Inherited,
        type_params: vec![],
        type_param_bounds: Default::default(),
        super_class: Some("java.lang.Object".to_string()),
        super_class_generic: None,
        interfaces: vec![],
        interfaces_generic: vec![],
        permitted_subclasses: vec![],
        members: vec![BinaryMemberSymbol {
            owner_type: "org.demo.Assert".to_string(),
            name: SmolStr::new("VALUE"),
            kind: MemberKind::Field,
            is_public: true,
            is_static: true,
            is_abstract: false,
            nullness_scope: NullnessScope::Inherited,
            decl_nullability: None,
            type_params: vec![],
            type_param_bounds: Default::default(),
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

fn hierarchy_classpath() -> ClasspathIndex {
    let mut cp = sample_ide_classpath();
    let jdk_base = BinaryOrigin::Jdk {
        module_name: Some(SmolStr::new("java.base")),
    };

    cp.insert_type(BinaryTypeSymbol {
        name: SmolStr::new("Object"),
        qualified_name: "java.lang.Object".to_string(),
        internal_name: "java/lang/Object".to_string(),
        package: "java.lang".to_string(),
        enclosing_type: None,
        kind: TypeKind::Class,
        identity: hir::TypeIdentity::Identity,
        is_public: true,
        is_static: false,
        is_final: false,
        is_abstract: false,
        nullness_scope: NullnessScope::Inherited,
        type_params: vec![],
        type_param_bounds: Default::default(),
        super_class: None,
        super_class_generic: None,
        interfaces: vec![],
        interfaces_generic: vec![],
        permitted_subclasses: vec![],
        members: vec![BinaryMemberSymbol {
            owner_type: "java.lang.Object".to_string(),
            name: SmolStr::new("toString"),
            kind: MemberKind::Method,
            is_public: true,
            is_static: false,
            is_abstract: false,
            nullness_scope: NullnessScope::Inherited,
            decl_nullability: None,
            type_params: vec![],
            type_param_bounds: Default::default(),
            type_ref: Some("java.lang.String".to_string()),
            params: vec![],
            is_varargs: false,
        }],
        nested_types: vec![],
        origin: jdk_base.clone(),
    });

    cp.insert_type(BinaryTypeSymbol {
        name: SmolStr::new("Collection"),
        qualified_name: "java.util.Collection".to_string(),
        internal_name: "java/util/Collection".to_string(),
        package: "java.util".to_string(),
        enclosing_type: None,
        kind: TypeKind::Interface,
        identity: hir::TypeIdentity::Unspecified,
        is_public: true,
        is_static: false,
        is_final: false,
        is_abstract: true,
        nullness_scope: NullnessScope::Inherited,
        type_params: vec![SmolStr::new("E")],
        type_param_bounds: Default::default(),
        super_class: None,
        super_class_generic: None,
        interfaces: vec![],
        interfaces_generic: vec![],
        permitted_subclasses: vec![],
        members: vec![],
        nested_types: vec![],
        origin: jdk_base.clone(),
    });

    cp.insert_type(BinaryTypeSymbol {
        name: SmolStr::new("List"),
        qualified_name: "java.util.List".to_string(),
        internal_name: "java/util/List".to_string(),
        package: "java.util".to_string(),
        enclosing_type: None,
        kind: TypeKind::Interface,
        identity: hir::TypeIdentity::Unspecified,
        is_public: true,
        is_static: false,
        is_final: false,
        is_abstract: true,
        nullness_scope: NullnessScope::Inherited,
        type_params: vec![SmolStr::new("E")],
        type_param_bounds: Default::default(),
        super_class: None,
        super_class_generic: None,
        interfaces: vec!["java.util.Collection".to_string()],
        interfaces_generic: vec![],
        permitted_subclasses: vec![],
        members: vec![BinaryMemberSymbol {
            owner_type: "java.util.List".to_string(),
            name: SmolStr::new("size"),
            kind: MemberKind::Method,
            is_public: true,
            is_static: false,
            is_abstract: false,
            nullness_scope: NullnessScope::Inherited,
            decl_nullability: None,
            type_params: vec![],
            type_param_bounds: Default::default(),
            type_ref: Some("int".to_string()),
            params: vec![],
            is_varargs: false,
        }],
        nested_types: vec![],
        origin: jdk_base,
    });

    cp.finalize();
    cp
}

#[test]
fn hover_displays_overrides_note_before_binary_origin() {
    let base_src = "\
package com.example;
public interface Greeter {
    String greet(String name);
}
";
    let impl_src = "\
package com.example;
public class FriendlyGreeter implements Greeter {
    @Override
    public String /*greet_impl*/greet(String name) {
        return \"hi \" + name;
    }
    public String /*non_override*/extra() {
        return \"ok\";
    }
}
";
    let db = db_with_files(&[(1, base_src), (2, impl_src)]);

    let h = hover(&db, FileId(2), cursor(impl_src, "/*greet_impl*/")).unwrap();
    assert_eq!(
        h.content,
        "```java\nString com.example.FriendlyGreeter.greet(String name)\n```\n*overrides `com.example.Greeter.greet(String)`*"
    );

    let non_ov = hover(&db, FileId(2), cursor(impl_src, "/*non_override*/")).unwrap();
    assert_eq!(
        non_ov.content,
        "```java\nString com.example.FriendlyGreeter.extra()\n```"
    );

    let synthetic = ResolvedSymbol {
        file_id: None,
        full_range: None,
        focus_range: None,
        binary_target: Some(BinaryTarget {
            origin: BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base")),
            },
            internal_name: "java/util/ArrayList".to_string(),
            qualified_name: "java.util.ArrayList".to_string(),
            member_name: Some(SmolStr::new("size")),
        }),
        name: SmolStr::new("size"),
        qualified_name: "java.util.ArrayList.size".to_string(),
        detail: Some(SymbolDetail::Method {
            return_type: Some("int".to_string()),
            params: "()".to_string(),
        }),
        kind: ResolvedSymbolKind::Method,
        overrides: Some("java.util.List.size()".to_string()),
    };
    assert_eq!(
        format_resolved_hover(&synthetic),
        "```java\nint java.util.ArrayList.size()\n```\n*overrides `java.util.List.size()`*\n*from JDK module `java.base`*"
    );
}

#[test]
fn goto_implementation_on_types_and_methods() {
    let api = "\
package com.api;
import java.util.Map;
public interface /*iface*/Repo {
    void /*save_decl*/save(Map<String, Integer> items, int count);
    void save(String single);
}
";
    let base = "\
package com.impl;
import com.api.Repo;
import java.util.Map;
public abstract class /*base*/AbstractRepo implements Repo {
    @Override
    public void save(Map<String, Integer> items, int count) {}
    public void save(String single) {}
}
";
    let sub = "\
package com.impl;
import com.api./*iface_ref*/Repo;
import java.util.List;
import java.util.Map;
public class ConcreteRepo extends AbstractRepo {
    @Override
    public void save(Map<String, Integer> items, int count) {}
    void use(Repo r) {
        r./*save_call*/save(null, 1);
    }
}
class MyList implements /*bin_list*/List<String> {
    @Override
    public int size() { return 0; }
}
";
    let mut host = AnalysisHost::new();
    host.set_file_text(FileId(1), Arc::new(api.to_string()));
    host.set_file_text(FileId(2), Arc::new(base.to_string()));
    host.set_file_text(FileId(3), Arc::new(sub.to_string()));
    host.set_workspace_files(Arc::new(vec![FileId(1), FileId(2), FileId(3)]));
    host.set_classpath_index(Arc::new(hierarchy_classpath()));

    let snap = host.analysis();

    // Interface declaration -> all workspace subtypes (sorted by FQN: AbstractRepo, ConcreteRepo).
    let iface_impls = snap
        .goto_implementation(FileId(1), cursor(api, "/*iface*/"))
        .unwrap();
    assert_eq!(iface_impls.len(), 2);
    let (_, f0) = assert_source_target(iface_impls[0].clone(), FileId(2));
    assert_eq!(slice(base, f0), "AbstractRepo");
    let (_, f1) = assert_source_target(iface_impls[1].clone(), FileId(3));
    assert_eq!(slice(sub, f1), "ConcreteRepo");

    // Type reference to interface -> same workspace subtypes.
    let ref_impls = snap
        .goto_implementation(FileId(3), cursor(sub, "/*iface_ref*/"))
        .unwrap();
    assert_eq!(ref_impls, iface_impls);

    // Abstract base class -> ConcreteRepo only.
    let base_impls = snap
        .goto_implementation(FileId(2), cursor(base, "/*base*/"))
        .unwrap();
    assert_eq!(base_impls.len(), 1);
    let (_, bf) = assert_source_target(base_impls[0].clone(), FileId(3));
    assert_eq!(slice(sub, bf), "ConcreteRepo");

    // Binary interface reference -> workspace implementer MyList.
    let bin_impls = snap
        .goto_implementation(FileId(3), cursor(sub, "/*bin_list*/"))
        .unwrap();
    assert_eq!(bin_impls.len(), 1);
    let (_, mf) = assert_source_target(bin_impls[0].clone(), FileId(3));
    assert_eq!(slice(sub, mf), "MyList");

    // Method declaration (with generic comma inside param type -> still arity 2) -> both overrides.
    let method_impls = snap
        .goto_implementation(FileId(1), cursor(api, "/*save_decl*/"))
        .unwrap();
    assert_eq!(method_impls.len(), 2);
    let (m0_full, m0_focus) = assert_source_target(method_impls[0].clone(), FileId(2));
    assert_eq!(slice(base, m0_focus), "save");
    assert!(slice(base, m0_full).contains("int count"));
    let (m1_full, m1_focus) = assert_source_target(method_impls[1].clone(), FileId(3));
    assert_eq!(slice(sub, m1_focus), "save");
    assert!(slice(sub, m1_full).contains("int count"));

    // Method call site -> same overriding methods.
    let call_impls = snap
        .goto_implementation(FileId(3), cursor(sub, "/*save_call*/"))
        .unwrap();
    assert_eq!(call_impls, method_impls);
}

#[test]
fn goto_type_definition_workspace_and_binary() {
    let model = "\
package com.model;
public class User {
    public String name;
}
";
    let app = "\
package com.app;
import com.model.User;
import java.util.List;
public record Wrapper(User /*rec_comp*/owner) {
    private User /*field*/primary;
    void run(User /*param*/arg, List<String> /*bin_param*/items) {
        User /*local*/u = arg;
        String /*bin_local*/s = u.name;
        int n = /*bin_call*/s.length();
        User x = /*src_use*/u;
    }
}
";
    let mut host = AnalysisHost::new();
    host.set_file_text(FileId(1), Arc::new(model.to_string()));
    host.set_file_text(FileId(2), Arc::new(app.to_string()));
    host.set_workspace_files(Arc::new(vec![FileId(1), FileId(2)]));
    host.set_classpath_index(Arc::new(hierarchy_classpath()));

    let snap = host.analysis();

    for marker in ["/*rec_comp*/", "/*field*/", "/*param*/", "/*local*/", "/*src_use*/"] {
        let target = snap
            .goto_type_definition(FileId(2), cursor(app, marker))
            .unwrap()
            .unwrap();
        let (_, focus) = assert_source_target(target, FileId(1));
        assert_eq!(slice(model, focus), "User");
    }

    let bin_local = snap
        .goto_type_definition(FileId(2), cursor(app, "/*bin_local*/"))
        .unwrap()
        .unwrap();
    assert_eq!(
        bin_local,
        NavigationTarget::Binary(BinaryTarget {
            origin: BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base")),
            },
            internal_name: "java/lang/String".to_string(),
            qualified_name: "java.lang.String".to_string(),
            member_name: None,
        })
    );

    let bin_param = snap
        .goto_type_definition(FileId(2), cursor(app, "/*bin_param*/"))
        .unwrap()
        .unwrap();
    assert_eq!(
        bin_param,
        NavigationTarget::Binary(BinaryTarget {
            origin: BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base")),
            },
            internal_name: "java/util/List".to_string(),
            qualified_name: "java.util.List".to_string(),
            member_name: None,
        })
    );
}

#[test]
fn type_hierarchy_prepare_supertypes_and_subtypes() {
    let base_src = "\
package com.h;
public abstract class Base {}
";
    let types_src = "\
package com.h;
import java.util.List;
public interface Extra {}
public abstract class /*mid_decl*/Mid extends Base implements Extra, List<String> {
    void /*non_type*/helper() {}
}
public class Leaf extends Mid {}
";
    let mut host = AnalysisHost::new();
    host.set_file_text(FileId(1), Arc::new(base_src.to_string()));
    host.set_file_text(FileId(2), Arc::new(types_src.to_string()));
    host.set_workspace_files(Arc::new(vec![FileId(1), FileId(2)]));
    host.set_classpath_index(Arc::new(hierarchy_classpath()));

    let snap = host.analysis();

    // Prepare on non-type returns None.
    assert!(
        snap.prepare_type_hierarchy(FileId(2), cursor(types_src, "/*non_type*/"))
            .unwrap()
            .is_none()
    );

    // Prepare on workspace class declaration.
    let mid_node = snap
        .prepare_type_hierarchy(FileId(2), cursor(types_src, "/*mid_decl*/"))
        .unwrap()
        .unwrap();
    assert_eq!(mid_node.name, "Mid");
    assert_eq!(mid_node.qualified_name, "com.h.Mid");
    assert_eq!(mid_node.kind, SymbolKind::Class);
    let (_, mid_focus) = assert_source_target(mid_node.target.clone(), FileId(2));
    assert_eq!(slice(types_src, mid_focus), "Mid");

    // Supertypes of Mid: Base (source), Extra (source), java.util.List (binary).
    let supers = snap.type_hierarchy_supertypes(&mid_node.qualified_name).unwrap();
    assert_eq!(supers.len(), 3);
    assert_eq!(supers[0].qualified_name, "com.h.Base");
    assert_eq!(supers[0].kind, SymbolKind::Class);
    assert_source_target(supers[0].target.clone(), FileId(1));

    assert_eq!(supers[1].qualified_name, "com.h.Extra");
    assert_eq!(supers[1].kind, SymbolKind::Interface);
    assert_source_target(supers[1].target.clone(), FileId(2));

    assert_eq!(supers[2].qualified_name, "java.util.List");
    assert_eq!(supers[2].kind, SymbolKind::Interface);
    assert_eq!(
        supers[2].target,
        NavigationTarget::Binary(BinaryTarget {
            origin: BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base")),
            },
            internal_name: "java/util/List".to_string(),
            qualified_name: "java.util.List".to_string(),
            member_name: None,
        })
    );

    // Binary supertype query: java.util.List -> java.util.Collection.
    let list_supers = snap.type_hierarchy_supertypes("java.util.List").unwrap();
    assert_eq!(list_supers.len(), 1);
    assert_eq!(list_supers[0].qualified_name, "java.util.Collection");
    assert_eq!(list_supers[0].kind, SymbolKind::Interface);

    // Direct subtypes of Base: Mid (not Leaf, which is transitive).
    let base_subs = snap.type_hierarchy_subtypes("com.h.Base").unwrap();
    assert_eq!(base_subs, vec![mid_node.clone()]);

    // Direct subtypes of Mid: Leaf.
    let mid_subs = snap.type_hierarchy_subtypes("com.h.Mid").unwrap();
    assert_eq!(mid_subs.len(), 1);
    assert_eq!(mid_subs[0].qualified_name, "com.h.Leaf");
    let (_, leaf_focus) = assert_source_target(mid_subs[0].target.clone(), FileId(2));
    assert_eq!(slice(types_src, leaf_focus), "Leaf");

    // Direct subtypes of binary java.util.List: Mid.
    let list_subs = snap.type_hierarchy_subtypes("java.util.List").unwrap();
    assert_eq!(list_subs, vec![mid_node]);
}

fn bin_cls(
    fqn: &str,
    kind: TypeKind,
    type_params: &[&str],
    super_class_generic: Option<&str>,
    interfaces_generic: &[&str],
    members: Vec<BinaryMemberSymbol>,
) -> BinaryTypeSymbol {
    let (pkg, name) = match fqn.rsplit_once('.') {
        Some((p, n)) => (p.to_string(), SmolStr::new(n)),
        None => (String::new(), SmolStr::new(fqn)),
    };
    let strip_generics = |raw: &str| raw.split('<').next().unwrap_or(raw).to_string();
    BinaryTypeSymbol {
        name,
        qualified_name: fqn.to_string(),
        internal_name: fqn.replace('.', "/"),
        package: pkg,
        enclosing_type: None,
        kind,
        identity: if kind == TypeKind::Interface {
            hir::TypeIdentity::Unspecified
        } else {
            hir::TypeIdentity::Identity
        },
        is_public: true,
        is_static: false,
        is_final: false,
        is_abstract: kind == TypeKind::Interface,
        nullness_scope: NullnessScope::Inherited,
        type_params: type_params.iter().map(|s| SmolStr::new(*s)).collect(),
        type_param_bounds: Default::default(),
        super_class: super_class_generic.map(strip_generics),
        super_class_generic: super_class_generic.map(str::to_string),
        interfaces: interfaces_generic.iter().map(|s| strip_generics(s)).collect(),
        interfaces_generic: interfaces_generic.iter().map(|s| (*s).to_string()).collect(),
        permitted_subclasses: Vec::new(),
        members,
        nested_types: Vec::new(),
        origin: BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base")),
        },
    }
}

fn bin_method(
    owner: &str,
    name: &str,
    is_static: bool,
    is_abstract: bool,
    type_params: &[&str],
    ret: &str,
    params: &[(&str, &str, bool)],
) -> BinaryMemberSymbol {
    BinaryMemberSymbol {
        owner_type: owner.to_string(),
        name: SmolStr::new(name),
        kind: MemberKind::Method,
        is_public: true,
        is_static,
        is_abstract,
        nullness_scope: NullnessScope::Inherited,
        decl_nullability: None,
        type_params: type_params.iter().map(|s| SmolStr::new(*s)).collect(),
        type_param_bounds: Default::default(),
        type_ref: Some(ret.to_string()),
        params: params
            .iter()
            .map(|(n, ty, va)| ParamSummary {
                name: Some(SmolStr::new(*n)),
                type_ref: (*ty).to_string(),
                is_varargs: *va,
                decl_nullability: None,
            })
            .collect(),
        is_varargs: params.last().is_some_and(|(_, _, va)| *va),
    }
}

fn generics_classpath() -> ClasspathIndex {
    let mut cp = ClasspathIndex::default();
    cp.insert_type(bin_cls(
        "java.lang.Object",
        TypeKind::Class,
        &[],
        None,
        &[],
        vec![
            bin_method("java.lang.Object", "toString", false, false, &[], "java.lang.String", &[]),
            bin_method(
                "java.lang.Object",
                "equals",
                false,
                false,
                &[],
                "boolean",
                &[("obj", "java.lang.Object", false)],
            ),
            bin_method("java.lang.Object", "hashCode", false, false, &[], "int", &[]),
        ],
    ));
    cp.insert_type(bin_cls(
        "java.lang.String",
        TypeKind::Class,
        &[],
        Some("java.lang.Object"),
        &[],
        vec![bin_method("java.lang.String", "length", false, false, &[], "int", &[])],
    ));
    cp.insert_type(bin_cls(
        "java.lang.Number",
        TypeKind::Class,
        &[],
        Some("java.lang.Object"),
        &[],
        vec![],
    ));
    cp.insert_type(bin_cls(
        "java.lang.Integer",
        TypeKind::Class,
        &[],
        Some("java.lang.Number"),
        &[],
        vec![],
    ));
    cp.insert_type(bin_cls(
        "java.util.function.Function",
        TypeKind::Interface,
        &["T", "R"],
        None,
        &[],
        vec![bin_method(
            "java.util.function.Function",
            "apply",
            false,
            true,
            &[],
            "R",
            &[("t", "T", false)],
        )],
    ));
    cp.insert_type(bin_cls(
        "java.util.function.Consumer",
        TypeKind::Interface,
        &["T"],
        None,
        &[],
        vec![bin_method(
            "java.util.function.Consumer",
            "accept",
            false,
            true,
            &[],
            "void",
            &[("t", "T", false)],
        )],
    ));
    cp.insert_type(bin_cls(
        "java.util.stream.Stream",
        TypeKind::Interface,
        &["T"],
        None,
        &[],
        vec![
            bin_method(
                "java.util.stream.Stream",
                "map",
                false,
                true,
                &["R"],
                "java.util.stream.Stream<R>",
                &[("mapper", "java.util.function.Function<? super T, ? extends R>", false)],
            ),
            bin_method(
                "java.util.stream.Stream",
                "toList",
                false,
                false,
                &[],
                "java.util.List<T>",
                &[],
            ),
        ],
    ));
    cp.insert_type(bin_cls(
        "java.lang.Iterable",
        TypeKind::Interface,
        &["T"],
        None,
        &[],
        vec![bin_method(
            "java.lang.Iterable",
            "forEach",
            false,
            false,
            &[],
            "void",
            &[("action", "java.util.function.Consumer<? super T>", false)],
        )],
    ));
    cp.insert_type(bin_cls(
        "java.util.Collection",
        TypeKind::Interface,
        &["E"],
        None,
        &["java.lang.Iterable<E>"],
        vec![bin_method(
            "java.util.Collection",
            "stream",
            false,
            false,
            &[],
            "java.util.stream.Stream<E>",
            &[],
        )],
    ));
    cp.insert_type(bin_cls(
        "java.util.List",
        TypeKind::Interface,
        &["E"],
        None,
        &["java.util.Collection<E>"],
        vec![
            bin_method("java.util.List", "get", false, true, &[], "E", &[("index", "int", false)]),
            bin_method(
                "java.util.List",
                "of",
                true,
                false,
                &["E"],
                "java.util.List<E>",
                &[("e1", "E", false)],
            ),
            bin_method(
                "java.util.List",
                "remove",
                false,
                true,
                &[],
                "E",
                &[("index", "int", false)],
            ),
            bin_method(
                "java.util.List",
                "remove",
                false,
                true,
                &[],
                "boolean",
                &[("o", "java.lang.Object", false)],
            ),
        ],
    ));
    cp.insert_type(bin_cls(
        "java.util.ArrayList",
        TypeKind::Class,
        &["E"],
        Some("java.lang.Object"),
        &["java.util.List<E>"],
        vec![],
    ));
    cp.finalize();
    cp
}

#[test]
fn hover_generics_var_lambdas_and_substituted_calls() {
    let user_src = "\
package com.example;
public class User {
    public String getName() { return \"\"; }
    public void ping() {}
}
";
    let box_src = "\
package com.example;
public class Box<T> {
    public T get() { return null; }
}
";
    let app_src = "\
package com.example;
import java.util.List;
public class App {
    void demo(List<User> users, Box<User> box) {
        var /*var_x*/x = List.of(new User());
        /*use_x*/x.get(0);
        var /*names*/names = users.stream()./*map_call*/map(/*lparam*/u -> /*luse*/u./*get_name*/getName())./*to_list*/toList();
        users./*list_get*/get(0);
        box./*box_get*/get();
        for (var /*fe_u*/u : users) {
            /*fe_use*/u.ping();
        }
    }
}
";
    let mut host = AnalysisHost::new();
    host.set_file_text(FileId(1), Arc::new(user_src.to_string()));
    host.set_file_text(FileId(2), Arc::new(box_src.to_string()));
    host.set_file_text(FileId(3), Arc::new(app_src.to_string()));
    host.set_workspace_files(Arc::new(vec![FileId(1), FileId(2), FileId(3)]));
    host.set_classpath_index(Arc::new(generics_classpath()));

    let snap = host.analysis();
    let fid = FileId(3);

    for marker in ["/*var_x*/", "/*use_x*/"] {
        let h = snap.hover(fid, cursor(app_src, marker)).unwrap().unwrap();
        assert_eq!(h.content, "```java\nList<User> x\n```\nlocal variable");
    }

    let names_h = snap.hover(fid, cursor(app_src, "/*names*/")).unwrap().unwrap();
    assert_eq!(names_h.content, "```java\nList<String> names\n```\nlocal variable");

    for marker in ["/*lparam*/", "/*luse*/"] {
        let h = snap.hover(fid, cursor(app_src, marker)).unwrap().unwrap();
        assert_eq!(h.content, "```java\nUser u\n```\nlambda parameter");
    }

    let get_name_h = snap.hover(fid, cursor(app_src, "/*get_name*/")).unwrap().unwrap();
    assert_eq!(
        get_name_h.content,
        "```java\nString com.example.User.getName()\n```"
    );

    let map_h = snap.hover(fid, cursor(app_src, "/*map_call*/")).unwrap().unwrap();
    assert_eq!(
        map_h.content,
        "```java\nStream<String> java.util.stream.Stream.map(Function<? super User, ? extends String> mapper)\n```\n*from JDK module `java.base`*"
    );

    let to_list_h = snap.hover(fid, cursor(app_src, "/*to_list*/")).unwrap().unwrap();
    assert_eq!(
        to_list_h.content,
        "```java\nList<String> java.util.stream.Stream.toList()\n```\n*from JDK module `java.base`*"
    );

    let list_get_h = snap.hover(fid, cursor(app_src, "/*list_get*/")).unwrap().unwrap();
    assert_eq!(
        list_get_h.content,
        "```java\nUser java.util.List.get(int index)\n```\n*from JDK module `java.base`*"
    );

    let box_get_h = snap.hover(fid, cursor(app_src, "/*box_get*/")).unwrap().unwrap();
    assert_eq!(
        box_get_h.content,
        "```java\nUser com.example.Box.get()\n```"
    );

    for marker in ["/*fe_u*/", "/*fe_use*/"] {
        let h = snap.hover(fid, cursor(app_src, marker)).unwrap().unwrap();
        assert_eq!(h.content, "```java\nUser u\n```\nfor-each variable");
    }
}

#[test]
fn goto_definition_generics_lambdas_mrefs_and_overloads() {
    let user_src = "\
package com.example;
public class User {
    public String getName() { return \"\"; }
    public void ping() {}
}
";
    let app_src = "\
package com.example;
import java.util.ArrayList;
import java.util.List;
class UserList extends ArrayList<User> {}
public class App {
    void foo(int x) {}
    void foo(String s) {}
    void pick(Number n) {}
    void pick(Integer i) {}
    void run(List<User> users) {
        users./*list_get*/get(0)./*chain_get_name*/getName();
        new UserList().get(0)./*sub_get_name*/getName();
        users.stream().map(u -> /*lambda_use*/u./*lambda_get_name*/getName()).toList();
        users.forEach(User::/*mref_ping*/ping);
        /*foo_int*/foo(1);
        /*foo_str*/foo(\"hi\");
        /*pick_int*/pick(1);
        users./*rem_idx*/remove(0);
        users./*rem_obj*/remove(\"item\");
    }
}
";
    let mut host = AnalysisHost::new();
    host.set_file_text(FileId(1), Arc::new(user_src.to_string()));
    host.set_file_text(FileId(2), Arc::new(app_src.to_string()));
    host.set_workspace_files(Arc::new(vec![FileId(1), FileId(2)]));
    host.set_classpath_index(Arc::new(generics_classpath()));

    let snap = host.analysis();
    let user_fid = FileId(1);
    let app_fid = FileId(2);

    for marker in ["/*chain_get_name*/", "/*sub_get_name*/", "/*lambda_get_name*/"] {
        let target = snap
            .goto_definition(app_fid, cursor(app_src, marker))
            .unwrap()
            .unwrap();
        let (_, focus) = assert_source_target(target, user_fid);
        assert_eq!(slice(user_src, focus), "getName", "at {marker}");
    }

    let list_get_def = snap
        .goto_definition(app_fid, cursor(app_src, "/*list_get*/"))
        .unwrap()
        .unwrap();
    assert_eq!(
        list_get_def,
        NavigationTarget::Binary(BinaryTarget {
            origin: BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base")),
            },
            internal_name: "java/util/List".to_string(),
            qualified_name: "java.util.List".to_string(),
            member_name: Some(SmolStr::new("get")),
        })
    );

    let mref_def = snap
        .goto_definition(app_fid, cursor(app_src, "/*mref_ping*/"))
        .unwrap()
        .unwrap();
    let (_, ping_focus) = assert_source_target(mref_def, user_fid);
    assert_eq!(slice(user_src, ping_focus), "ping");

    let (foo_int_full, foo_int_focus) = assert_source_target(
        snap.goto_definition(app_fid, cursor(app_src, "/*foo_int*/"))
            .unwrap()
            .unwrap(),
        app_fid,
    );
    assert_eq!(slice(app_src, foo_int_focus), "foo");
    assert!(slice(app_src, foo_int_full).contains("int x"));

    let (foo_str_full, foo_str_focus) = assert_source_target(
        snap.goto_definition(app_fid, cursor(app_src, "/*foo_str*/"))
            .unwrap()
            .unwrap(),
        app_fid,
    );
    assert_eq!(slice(app_src, foo_str_focus), "foo");
    assert!(slice(app_src, foo_str_full).contains("String s"));

    let (pick_full, pick_focus) = assert_source_target(
        snap.goto_definition(app_fid, cursor(app_src, "/*pick_int*/"))
            .unwrap()
            .unwrap(),
        app_fid,
    );
    assert_eq!(slice(app_src, pick_focus), "pick");
    assert!(slice(app_src, pick_full).contains("Integer i"));

    // Overload resolution against binary List.remove(int) vs List.remove(Object)
    let rem_idx_h = snap.hover(app_fid, cursor(app_src, "/*rem_idx*/")).unwrap().unwrap();
    assert_eq!(
        rem_idx_h.content,
        "```java\nUser java.util.List.remove(int index)\n```\n*from JDK module `java.base`*"
    );
    let rem_obj_h = snap.hover(app_fid, cursor(app_src, "/*rem_obj*/")).unwrap().unwrap();
    assert_eq!(
        rem_obj_h.content,
        "```java\nboolean java.util.List.remove(java.lang.Object o)\n```\n*from JDK module `java.base`*"
    );
}

#[test]
fn goto_type_definition_generics_var_and_lambdas() {
    let user_src = "\
package com.example;
public class User {
    public String getName() { return \"\"; }
}
";
    let app_src = "\
package com.example;
import java.util.List;
public class App {
    void demo(List<User> users) {
        /*var_kw*/var /*var_u*/u = users./*call_get*/get(0);
        var /*var_name*/name = /*use_u*/u./*call_name*/getName();
        var /*var_list*/list = List.of(new User());
        users.stream().map(/*lparam*/item -> /*luse*/item.getName()).toList();
        for (var /*fe_u*/elem : users) {
            /*fe_use*/elem.getName();
        }
    }
}
";
    let mut host = AnalysisHost::new();
    host.set_file_text(FileId(1), Arc::new(user_src.to_string()));
    host.set_file_text(FileId(2), Arc::new(app_src.to_string()));
    host.set_workspace_files(Arc::new(vec![FileId(1), FileId(2)]));
    host.set_classpath_index(Arc::new(generics_classpath()));

    let snap = host.analysis();
    let user_fid = FileId(1);
    let app_fid = FileId(2);

    for marker in [
        "/*var_kw*/",
        "/*var_u*/",
        "/*use_u*/",
        "/*call_get*/",
        "/*lparam*/",
        "/*luse*/",
        "/*fe_u*/",
        "/*fe_use*/",
    ] {
        let td = snap
            .goto_type_definition(app_fid, cursor(app_src, marker))
            .unwrap()
            .unwrap_or_else(|| panic!("expected type definition at {marker}"));
        let (_, focus) = assert_source_target(td, user_fid);
        assert_eq!(slice(user_src, focus), "User", "at {marker}");
    }

    let list_td = snap
        .goto_type_definition(app_fid, cursor(app_src, "/*var_list*/"))
        .unwrap()
        .unwrap();
    assert_eq!(
        list_td,
        NavigationTarget::Binary(BinaryTarget {
            origin: BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base")),
            },
            internal_name: "java/util/List".to_string(),
            qualified_name: "java.util.List".to_string(),
            member_name: None,
        })
    );

    for marker in ["/*var_name*/", "/*call_name*/"] {
        let str_td = snap
            .goto_type_definition(app_fid, cursor(app_src, marker))
            .unwrap()
            .unwrap();
        assert_eq!(
            str_td,
            NavigationTarget::Binary(BinaryTarget {
                origin: BinaryOrigin::Jdk {
                    module_name: Some(SmolStr::new("java.base")),
                },
                internal_name: "java/lang/String".to_string(),
                qualified_name: "java.lang.String".to_string(),
                member_name: None,
            }),
            "at {marker}"
        );
    }
}

#[test]
fn hover_displays_concise_nullability_suffixes_for_marked_and_binary_symbols() {
    let user_src = "\
package com.example;
import org.jspecify.annotations.*;

@NullMarked
public class User {
    public String name;
    public @Nullable String nickname;
    public @Nullable User findPartner(String id) { return null; }
}
";
    let app_src = "\
package com.example;
import org.jspecify.annotations.*;
import java.util.List;

@NullMarked
public class App {
    void demo(List<@Nullable User> /*p_users*/users, List<User> /*p_nn*/nnUsers, BinaryApi api) {
        var /*u_nn*/u = nnUsers./*nn_get*/get(0);
        var /*u_q*/maybe = users./*q_get*/get(0);
        var /*bin_q*/b1 = api./*bin_find*/findUser(\"a\");
        var /*bin_nn*/b2 = api./*bin_req*/requireUser(\"a\");
        var /*part*/p = u./*call_part*/findPartner(\"x\");
        var /*lub*/m = true ? u : null;
    }
}
";
    let mut cp = generics_classpath();
    let mut bin_api = bin_cls(
        "com.example.BinaryApi",
        TypeKind::Interface,
        &[],
        None,
        &[],
        vec![
            bin_method(
                "com.example.BinaryApi",
                "findUser",
                false,
                true,
                &[],
                "com.example.User?",
                &[("id", "java.lang.String", false)],
            ),
            bin_method(
                "com.example.BinaryApi",
                "requireUser",
                false,
                true,
                &[],
                "com.example.User",
                &[("id", "java.lang.String", false)],
            ),
        ],
    );
    bin_api.nullness_scope = NullnessScope::NonNullByDefault(NullabilitySource::JSpecify);
    bin_api.origin = BinaryOrigin::Jar {
        label: "lib/null-api.jar".to_string(),
    };
    cp.insert_type(bin_api);
    cp.finalize();

    let mut host = AnalysisHost::new();
    host.set_file_text(FileId(1), Arc::new(user_src.to_string()));
    host.set_file_text(FileId(2), Arc::new(app_src.to_string()));
    host.set_workspace_files(Arc::new(vec![FileId(1), FileId(2)]));
    host.set_classpath_index(Arc::new(cp));

    let snap = host.analysis();
    let fid = FileId(2);

    let check = |marker: &str, expected: &str| {
        let h = snap.hover(fid, cursor(app_src, marker)).unwrap().unwrap();
        assert_eq!(h.content, expected, "at {marker}");
    };

    check(
        "/*p_users*/",
        "```java\nList<User?>! users\n```\nparameter",
    );
    check("/*p_nn*/", "```java\nList<User!>! nnUsers\n```\nparameter");
    check("/*u_nn*/", "```java\nUser! u\n```\nlocal variable");
    check("/*u_q*/", "```java\nUser? maybe\n```\nlocal variable");
    check("/*bin_q*/", "```java\nUser? b1\n```\nlocal variable");
    check("/*bin_nn*/", "```java\nUser! b2\n```\nlocal variable");
    check("/*part*/", "```java\nUser? p\n```\nlocal variable");
    check("/*lub*/", "```java\nUser? m\n```\nlocal variable");
    check(
        "/*call_part*/",
        "```java\nUser? com.example.User.findPartner(String! id)\n```",
    );
    check(
        "/*bin_find*/",
        "```java\nUser? com.example.BinaryApi.findUser(String! id)\n```\n*from `lib/null-api.jar`*",
    );
    check(
        "/*bin_req*/",
        "```java\nUser! com.example.BinaryApi.requireUser(String! id)\n```\n*from `lib/null-api.jar`*",
    );
}
