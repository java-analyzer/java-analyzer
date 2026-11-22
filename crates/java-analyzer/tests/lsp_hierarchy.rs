mod common;

use std::{fs, io::Write as _, path::Path};

use common::{TempWorkspace, shutdown, start_server_with_root};
use java_analyzer::convert::uri_to_path;
use lsp_types::Uri;
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

fn lsp_range(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Value {
    json!({
        "start": { "line": start_line, "character": start_char },
        "end": { "line": end_line, "character": end_char }
    })
}

fn build_class_bytes(
    internal_name: &str,
    access_flags: u16,
    methods: &[(&str, &str, u16)],
    fields: &[(&str, &str, u16)],
) -> Vec<u8> {
    struct Cp(Vec<Vec<u8>>);
    impl Cp {
        fn push(&mut self, bytes: Vec<u8>) -> u16 {
            self.0.push(bytes);
            u16::try_from(self.0.len()).unwrap()
        }
        fn utf8(&mut self, s: &str) -> u16 {
            let mut b = vec![1];
            b.extend_from_slice(&u16::try_from(s.len()).unwrap().to_be_bytes());
            b.extend_from_slice(s.as_bytes());
            self.push(b)
        }
        fn class(&mut self, s: &str) -> u16 {
            let idx = self.utf8(s);
            let mut b = vec![7];
            b.extend_from_slice(&idx.to_be_bytes());
            self.push(b)
        }
    }

    let mut cp = Cp(Vec::new());
    let this_cls = cp.class(internal_name);
    let super_cls = cp.class("java/lang/Object");

    let enc_fields: Vec<(u16, u16, u16)> = fields
        .iter()
        .map(|&(n, d, f)| (f, cp.utf8(n), cp.utf8(d)))
        .collect();
    let enc_methods: Vec<(u16, u16, u16)> = methods
        .iter()
        .map(|&(n, d, f)| (f, cp.utf8(n), cp.utf8(d)))
        .collect();

    let mut out = Vec::new();
    out.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&61u16.to_be_bytes());
    out.extend_from_slice(&u16::try_from(cp.0.len() + 1).unwrap().to_be_bytes());
    for entry in cp.0 {
        out.extend_from_slice(&entry);
    }
    out.extend_from_slice(&access_flags.to_be_bytes());
    out.extend_from_slice(&this_cls.to_be_bytes());
    out.extend_from_slice(&super_cls.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // interfaces
    out.extend_from_slice(&u16::try_from(enc_fields.len()).unwrap().to_be_bytes());
    for (f, n, d) in enc_fields {
        out.extend_from_slice(&f.to_be_bytes());
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&d.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
    }
    out.extend_from_slice(&u16::try_from(enc_methods.len()).unwrap().to_be_bytes());
    for (f, n, d) in enc_methods {
        out.extend_from_slice(&f.to_be_bytes());
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&d.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes());
    out
}

fn write_jar(path: &Path, entries: &[(&str, &[u8])]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let file = fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for &(name, bytes) in entries {
        writer.start_file(name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn lsp_implementation_on_interfaces_base_classes_and_methods() {
    let ws = TempWorkspace::new();
    let repo_text = "\
package com.api;
public interface Repo {
    void save(String item);
    void delete(int id);
}
";
    let base_text = "\
package com.impl;
import com.api.Repo;
public abstract class BaseRepo implements Repo {
    @Override
    public void save(String item) {}
}
";
    let sql_text = "\
package com.impl;
import com.api.Repo;
public class SqlRepo extends BaseRepo {
    @Override
    public void save(String item) {}
    @Override
    public void delete(int id) {}
    void use(Repo r) {
        r.save(\"x\");
    }
}
";
    let repo_uri = ws.write_uri("src/com/api/Repo.java", repo_text);
    let base_uri = ws.write_uri("src/com/impl/BaseRepo.java", base_text);
    let sql_uri = ws.write_uri("src/com/impl/SqlRepo.java", sql_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&repo_uri, repo_text);
    client.open_uri(&sql_uri, sql_text);

    // Interface `Repo` -> BaseRepo and SqlRepo (sorted by FQN).
    assert_eq!(
        client.implementation_uri(&repo_uri, 1, 17),
        json!([
            { "uri": base_uri, "range": lsp_range(2, 22, 2, 30) },
            { "uri": sql_uri, "range": lsp_range(2, 13, 2, 20) }
        ])
    );

    // Abstract base class `BaseRepo` -> SqlRepo.
    client.open_uri(&base_uri, base_text);
    assert_eq!(
        client.implementation_uri(&base_uri, 2, 22),
        json!([{ "uri": sql_uri, "range": lsp_range(2, 13, 2, 20) }])
    );

    // Interface method `Repo.save` -> BaseRepo.save and SqlRepo.save.
    let expected_save_impls = json!([
        { "uri": base_uri, "range": lsp_range(4, 16, 4, 20) },
        { "uri": sql_uri, "range": lsp_range(4, 16, 4, 20) }
    ]);
    assert_eq!(
        client.implementation_uri(&repo_uri, 2, 9),
        expected_save_impls
    );

    // Call site `r.save("x")` -> same overriding methods.
    assert_eq!(
        client.implementation_uri(&sql_uri, 8, 10),
        expected_save_impls
    );

    // Interface method `Repo.delete` -> SqlRepo.delete only.
    assert_eq!(
        client.implementation_uri(&repo_uri, 3, 9),
        json!([{ "uri": sql_uri, "range": lsp_range(6, 16, 6, 22) }])
    );

    // Concrete leaf class `SqlRepo` has no implementations -> null.
    assert_eq!(client.implementation_uri(&sql_uri, 2, 13), Value::Null);

    shutdown(client, handle);
}

#[test]
fn lsp_type_definition_workspace_and_binary_jar() {
    let ws = TempWorkspace::new();

    let widget_cls = build_class_bytes(
        "com/example/ext/Widget",
        0x0001,
        &[("render", "()I", 0x0001)],
        &[],
    );
    write_jar(
        &ws.path().join("libs/widget.jar"),
        &[("com/example/ext/Widget.class", &widget_cls)],
    );
    write_jar(
        &ws.path().join("libs/widget-sources.jar"),
        &[(
            "com/example/ext/Widget.java",
            b"package com.example.ext;\npublic class Widget {\n    public int render() { return 42; }\n}\n",
        )],
    );

    let gadget_cls = build_class_bytes(
        "com/example/ext/Gadget",
        0x0001,
        &[("spin", "()V", 0x0001)],
        &[],
    );
    write_jar(
        &ws.path().join("libs/gadget.jar"),
        &[("com/example/ext/Gadget.class", &gadget_cls)],
    );

    ws.write(
        ".java-analyzer.toml",
        r#"release = 21
[modules."."]
dependencies = { include = ["libs/widget.jar", "libs/gadget.jar"] }
"#,
    );

    let user_uri = ws.write_uri(
        "src/com/model/User.java",
        "\
package com.model;
public class User {
    public String name;
}
",
    );
    let app_text = "\
package com.app;
import com.model.User;
import com.example.ext.Widget;
import com.example.ext.Gadget;
public class App {
    private User primary;
    void run(Widget w, Gadget g) {
        User local = primary;
        int r = w.render();
    }
}
";
    let app_uri = ws.write_uri("src/com/app/App.java", app_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&app_uri, app_text);

    // Field `primary` -> workspace User.java
    assert_eq!(
        client.type_definition_uri(&app_uri, 5, 17),
        json!({ "uri": user_uri, "range": lsp_range(1, 13, 1, 17) })
    );

    // Local variable `local` -> workspace User.java
    assert_eq!(
        client.type_definition_uri(&app_uri, 7, 13),
        json!({ "uri": user_uri, "range": lsp_range(1, 13, 1, 17) })
    );

    // Whitespace -> null
    assert_eq!(client.type_definition_uri(&app_uri, 4, 0), Value::Null);

    // Binary JAR type with -sources.jar: `w` at usage (8, 16) -> materialized Widget.java
    let widget_loc = client.type_definition_uri(&app_uri, 8, 16);
    assert_ne!(widget_loc, Value::Null);
    let widget_uri_str = widget_loc["uri"].as_str().expect("widget uri");
    assert!(widget_uri_str.starts_with("file://"), "{widget_loc}");
    let widget_path = uri_to_path(&widget_uri_str.parse::<Uri>().unwrap()).unwrap();
    assert!(widget_path.is_file());
    assert_eq!(widget_loc["range"], lsp_range(1, 13, 1, 19));
    let _ = fs::remove_file(&widget_path);

    // Binary JAR type without sources: `g` param (6, 30) -> synthesized stub Gadget.java
    let gadget_loc = client.type_definition_uri(&app_uri, 6, 30);
    assert_ne!(gadget_loc, Value::Null);
    let gadget_uri_str = gadget_loc["uri"].as_str().expect("gadget uri");
    assert!(gadget_uri_str.starts_with("file://"), "{gadget_loc}");
    let gadget_path = uri_to_path(&gadget_uri_str.parse::<Uri>().unwrap()).unwrap();
    assert!(gadget_path.is_file());
    let stub_text = fs::read_to_string(&gadget_path).unwrap();
    assert!(stub_text.contains("class Gadget"), "{stub_text}");
    let _ = fs::remove_file(&gadget_path);

    shutdown(client, handle);
}

#[test]
fn lsp_type_hierarchy_prepare_supertypes_and_subtypes() {
    let ws = TempWorkspace::new();

    // ACC_PUBLIC (0x0001) | ACC_INTERFACE (0x0200) | ACC_ABSTRACT (0x0400)
    let ext_base_cls = build_class_bytes("com/example/ext/ExtBase", 0x0601, &[], &[]);
    write_jar(
        &ws.path().join("libs/ext.jar"),
        &[("com/example/ext/ExtBase.class", &ext_base_cls)],
    );
    ws.write(
        ".java-analyzer.toml",
        r#"release = 21
[modules."."]
dependencies = { include = ["libs/ext.jar"] }
"#,
    );

    let base_uri = ws.write_uri(
        "src/com/h/Base.java",
        "\
package com.h;
public class Base {}
",
    );
    let hierarchy_text = "\
package com.h;
import com.example.ext.ExtBase;
public class Mid extends Base implements ExtBase {
    void helper() {}
}
public class Leaf extends Mid {}
";
    let hierarchy_uri = ws.write_uri("src/com/h/Hierarchy.java", hierarchy_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&hierarchy_uri, hierarchy_text);

    // Non-type symbol -> null.
    assert_eq!(
        client.prepare_type_hierarchy_uri(&hierarchy_uri, 3, 9),
        Value::Null
    );

    // Prepare on `Mid`.
    let prepared = client.prepare_type_hierarchy_uri(&hierarchy_uri, 2, 13);
    let items = prepared.as_array().expect("prepared array");
    assert_eq!(items.len(), 1, "{prepared}");
    let mid_item = items[0].clone();
    assert_eq!(mid_item["name"], json!("Mid"));
    assert_eq!(mid_item["kind"], json!(5)); // SymbolKind::CLASS
    assert_eq!(mid_item["detail"], json!("com.h.Mid"));
    assert_eq!(mid_item["uri"], json!(hierarchy_uri));
    assert_eq!(mid_item["selectionRange"], lsp_range(2, 13, 2, 16));
    assert_eq!(mid_item["range"], lsp_range(2, 0, 4, 1));
    assert_eq!(mid_item["data"], json!("com.h.Mid"));

    // Supertypes of `Mid`: `Base` (source) and `ExtBase` (binary).
    let supertypes = client.type_hierarchy_supertypes(mid_item.clone());
    let super_arr = supertypes.as_array().expect("supertypes array");
    assert_eq!(super_arr.len(), 2, "{supertypes}");

    let base_item = super_arr[0].clone();
    assert_eq!(base_item["name"], json!("Base"));
    assert_eq!(base_item["kind"], json!(5)); // CLASS
    assert_eq!(base_item["detail"], json!("com.h.Base"));
    assert_eq!(base_item["uri"], json!(base_uri));
    assert_eq!(base_item["selectionRange"], lsp_range(1, 13, 1, 17));
    assert_eq!(base_item["data"], json!("com.h.Base"));

    let ext_item = super_arr[1].clone();
    assert_eq!(ext_item["name"], json!("ExtBase"));
    assert_eq!(ext_item["kind"], json!(11)); // INTERFACE
    assert_eq!(ext_item["detail"], json!("com.example.ext.ExtBase"));
    let ext_uri_str = ext_item["uri"].as_str().expect("ext uri");
    assert!(ext_uri_str.starts_with("file://"), "{ext_item}");
    let ext_path = uri_to_path(&ext_uri_str.parse::<Uri>().unwrap()).unwrap();
    assert!(ext_path.is_file());
    assert_eq!(ext_item["data"], json!("com.example.ext.ExtBase"));
    let _ = fs::remove_file(&ext_path);

    // Direct subtypes of `Base` -> `Mid` only (not `Leaf`).
    let base_subs = client.type_hierarchy_subtypes(base_item);
    assert_eq!(base_subs, json!([mid_item]));

    // Direct subtypes of `Mid` -> `Leaf`.
    let mid_subs = client.type_hierarchy_subtypes(mid_item.clone());
    let mid_sub_arr = mid_subs.as_array().expect("mid subtypes");
    assert_eq!(mid_sub_arr.len(), 1, "{mid_subs}");
    assert_eq!(mid_sub_arr[0]["name"], json!("Leaf"));
    assert_eq!(mid_sub_arr[0]["uri"], json!(hierarchy_uri));
    assert_eq!(mid_sub_arr[0]["selectionRange"], lsp_range(5, 13, 5, 17));
    assert_eq!(mid_sub_arr[0]["data"], json!("com.h.Leaf"));

    // Direct subtypes of binary `ExtBase` -> `Mid`.
    let ext_subs = client.type_hierarchy_subtypes(ext_item);
    assert_eq!(ext_subs, json!([mid_item]));

    // Missing `data` on TypeHierarchyItem -> null (no panic).
    let mut no_data = mid_item;
    no_data.as_object_mut().unwrap().remove("data");
    assert_eq!(client.type_hierarchy_supertypes(no_data.clone()), Value::Null);
    assert_eq!(client.type_hierarchy_subtypes(no_data), Value::Null);

    shutdown(client, handle);
}

#[test]
fn lsp_hover_overrides_and_super_goto_definition() {
    let ws = TempWorkspace::new();
    let parent_uri = ws.write_uri(
        "src/com/sup/Parent.java",
        "\
package com.sup;
public class Parent {
    public int count;
    public Parent(int c) { this.count = c; }
    public String greet(String name) { return name; }
}
",
    );
    let child_text = "\
package com.sup;
public class Child extends Parent {
    public Child(int c) {
        super(c);
    }
    @Override
    public String greet(String name) {
        return super.greet(name) + super.count;
    }
}
";
    let child_uri = ws.write_uri("src/com/sup/Child.java", child_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&child_uri, child_text);

    // Hover on overriding method `Child.greet` shows `*overrides ...*`.
    let hover = client.hover_uri(&child_uri, 6, 18);
    let md = hover["contents"]["value"].as_str().expect("hover markdown");
    assert!(
        md.contains("*overrides `com.sup.Parent.greet(String)`*"),
        "expected overrides note in hover, got: {md}"
    );

    // `super(c)` -> Parent constructor at line 3, cols 11..17.
    assert_eq!(
        client.definition_uri(&child_uri, 3, 8),
        json!({ "uri": parent_uri, "range": lsp_range(3, 11, 3, 17) })
    );

    // `super.greet(name)` -> Parent.greet at line 4, cols 18..23.
    assert_eq!(
        client.definition_uri(&child_uri, 7, 21),
        json!({ "uri": parent_uri, "range": lsp_range(4, 18, 4, 23) })
    );

    // `super.count` -> Parent.count at line 2, cols 15..20.
    assert_eq!(
        client.definition_uri(&child_uri, 7, 41),
        json!({ "uri": parent_uri, "range": lsp_range(2, 15, 2, 20) })
    );

    shutdown(client, handle);
}
