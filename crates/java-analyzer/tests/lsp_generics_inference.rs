mod common;

use std::{fs, io::Write as _, path::Path};

use common::{TempWorkspace, shutdown, start_server_with_root};
use java_analyzer::{convert::uri_to_path, workspace_config::discover_jdks};
use lsp_types::Uri;
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

fn lsp_range(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Value {
    json!({
        "start": { "line": start_line, "character": start_char },
        "end": { "line": end_line, "character": end_char }
    })
}

fn line_col(text: &str, marker: &str) -> (u32, u32) {
    let idx = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing marker `{marker}`"));
    let offset = idx + marker.len();
    let prefix = &text[..offset];
    let line = u32::try_from(prefix.bytes().filter(|&b| b == b'\n').count()).unwrap();
    let line_start = prefix.rfind('\n').map_or(0, |p| p + 1);
    let col = u32::try_from(text[line_start..offset].encode_utf16().count()).unwrap();
    (line, col)
}

struct MethodSpec<'a> {
    name: &'a str,
    desc: &'a str,
    sig: Option<&'a str>,
    flags: u16,
}

fn build_generic_class_bytes(
    internal_name: &str,
    access_flags: u16,
    class_sig: Option<&str>,
    super_name: &str,
    interfaces: &[&str],
    methods: &[MethodSpec<'_>],
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
    let super_cls = cp.class(super_name);
    let sig_attr_name = cp.utf8("Signature");
    let class_sig_idx = class_sig.map(|s| cp.utf8(s));
    let iface_idxs: Vec<u16> = interfaces.iter().map(|i| cp.class(i)).collect();

    let enc_methods: Vec<(u16, u16, u16, Option<u16>)> = methods
        .iter()
        .map(|m| {
            (
                m.flags,
                cp.utf8(m.name),
                cp.utf8(m.desc),
                m.sig.map(|s| cp.utf8(s)),
            )
        })
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
    out.extend_from_slice(&u16::try_from(iface_idxs.len()).unwrap().to_be_bytes());
    for idx in iface_idxs {
        out.extend_from_slice(&idx.to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes()); // fields
    out.extend_from_slice(&u16::try_from(enc_methods.len()).unwrap().to_be_bytes());
    for (f, n, d, sig) in enc_methods {
        out.extend_from_slice(&f.to_be_bytes());
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&d.to_be_bytes());
        if let Some(sig_idx) = sig {
            out.extend_from_slice(&1u16.to_be_bytes());
            out.extend_from_slice(&sig_attr_name.to_be_bytes());
            out.extend_from_slice(&2u32.to_be_bytes());
            out.extend_from_slice(&sig_idx.to_be_bytes());
        } else {
            out.extend_from_slice(&0u16.to_be_bytes());
        }
    }
    if let Some(sig_idx) = class_sig_idx {
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&sig_attr_name.to_be_bytes());
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&sig_idx.to_be_bytes());
    } else {
        out.extend_from_slice(&0u16.to_be_bytes());
    }
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
fn lsp_generics_var_lambdas_overloads_and_binary_jar() {
    let ws = TempWorkspace::new();

    let obj_cls = build_generic_class_bytes("java/lang/Object", 0x0001, None, "java/lang/Object", &[], &[]);
    let str_cls = build_generic_class_bytes(
        "java/lang/String",
        0x0011,
        None,
        "java/lang/Object",
        &[],
        &[MethodSpec {
            name: "length",
            desc: "()I",
            sig: None,
            flags: 0x0001,
        }],
    );
    let func_cls = build_generic_class_bytes(
        "java/util/function/Function",
        0x0601,
        Some("<T:Ljava/lang/Object;R:Ljava/lang/Object;>Ljava/lang/Object;"),
        "java/lang/Object",
        &[],
        &[MethodSpec {
            name: "apply",
            desc: "(Ljava/lang/Object;)Ljava/lang/Object;",
            sig: Some("(TT;)TR;"),
            flags: 0x0401,
        }],
    );
    let cons_cls = build_generic_class_bytes(
        "java/util/function/Consumer",
        0x0601,
        Some("<T:Ljava/lang/Object;>Ljava/lang/Object;"),
        "java/lang/Object",
        &[],
        &[MethodSpec {
            name: "accept",
            desc: "(Ljava/lang/Object;)V",
            sig: Some("(TT;)V"),
            flags: 0x0401,
        }],
    );
    let stream_cls = build_generic_class_bytes(
        "java/util/stream/Stream",
        0x0601,
        Some("<T:Ljava/lang/Object;>Ljava/lang/Object;"),
        "java/lang/Object",
        &[],
        &[
            MethodSpec {
                name: "map",
                desc: "(Ljava/util/function/Function;)Ljava/util/stream/Stream;",
                sig: Some("<R:Ljava/lang/Object;>(Ljava/util/function/Function<-TT;+TR;>;)Ljava/util/stream/Stream<TR;>;"),
                flags: 0x0401,
            },
            MethodSpec {
                name: "toList",
                desc: "()Ljava/util/List;",
                sig: Some("()Ljava/util/List<TT;>;"),
                flags: 0x0001,
            },
        ],
    );
    let iterable_cls = build_generic_class_bytes(
        "java/lang/Iterable",
        0x0601,
        Some("<T:Ljava/lang/Object;>Ljava/lang/Object;"),
        "java/lang/Object",
        &[],
        &[MethodSpec {
            name: "forEach",
            desc: "(Ljava/util/function/Consumer;)V",
            sig: Some("(Ljava/util/function/Consumer<-TT;>;)V"),
            flags: 0x0001,
        }],
    );
    let collection_cls = build_generic_class_bytes(
        "java/util/Collection",
        0x0601,
        Some("<E:Ljava/lang/Object;>Ljava/lang/Object;Ljava/lang/Iterable<TE;>;"),
        "java/lang/Object",
        &["java/lang/Iterable"],
        &[MethodSpec {
            name: "stream",
            desc: "()Ljava/util/stream/Stream;",
            sig: Some("()Ljava/util/stream/Stream<TE;>;"),
            flags: 0x0001,
        }],
    );
    let list_cls = build_generic_class_bytes(
        "java/util/List",
        0x0601,
        Some("<E:Ljava/lang/Object;>Ljava/lang/Object;Ljava/util/Collection<TE;>;"),
        "java/lang/Object",
        &["java/util/Collection"],
        &[
            MethodSpec {
                name: "get",
                desc: "(I)Ljava/lang/Object;",
                sig: Some("(I)TE;"),
                flags: 0x0401,
            },
            MethodSpec {
                name: "of",
                desc: "(Ljava/lang/Object;)Ljava/util/List;",
                sig: Some("<E:Ljava/lang/Object;>(TE;)Ljava/util/List<TE;>;"),
                flags: 0x0009,
            },
            MethodSpec {
                name: "remove",
                desc: "(I)Ljava/lang/Object;",
                sig: Some("(I)TE;"),
                flags: 0x0401,
            },
            MethodSpec {
                name: "remove",
                desc: "(Ljava/lang/Object;)Z",
                sig: None,
                flags: 0x0401,
            },
        ],
    );

    write_jar(
        &ws.path().join("libs/jdk-mini.jar"),
        &[
            ("java/lang/Object.class", &obj_cls),
            ("java/lang/String.class", &str_cls),
            ("java/util/function/Function.class", &func_cls),
            ("java/util/function/Consumer.class", &cons_cls),
            ("java/util/stream/Stream.class", &stream_cls),
            ("java/lang/Iterable.class", &iterable_cls),
            ("java/util/Collection.class", &collection_cls),
            ("java/util/List.class", &list_cls),
        ],
    );

    ws.write(
        ".java-analyzer.toml",
        r#"release = 21
[modules."."]
dependencies = { include = ["libs/jdk-mini.jar"] }
"#,
    );

    let user_text = "\
package com.example;
public class User {
    public String getName() { return \"\"; }
    public void ping() {}
}
";
    let box_text = "\
package com.example;
public class Box<T> {
    public T get() { return null; }
}
";
    let app_text = "\
package com.example;
import java.util.List;
public class App {
    void foo(int x) {}
    void foo(String s) {}
    void run(List<User> users, Box<User> box) {
        var /*var_x*/x = List.of(new User());
        /*var_kw*/var /*var_u*/u = users./*list_get*/get(0);
        var /*names*/names = users.stream().map(/*lparam*/item -> item./*lambda_get_name*/getName()).toList();
        users.forEach(User::/*mref_ping*/ping);
        /*foo_int*/foo(1);
        /*foo_str*/foo(\"hi\");
        String n = users.get(0)./*chain_get_name*/getName();
        String bn = box./*box_get*/get().getName();
        for (var /*fe_u*/elem : users) {
            elem./*fe_ping*/ping();
        }
        users./*rem_idx*/remove(0);
        users./*rem_obj*/remove(\"item\");
    }
}
";
    let user_uri = ws.write_uri("src/com/example/User.java", user_text);
    let _box_uri = ws.write_uri("src/com/example/Box.java", box_text);
    let app_uri = ws.write_uri("src/com/example/App.java", app_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&app_uri, app_text);

    // 1. Hover on `var` locals, untyped lambda param, and generic-substituted call
    let (l, c) = line_col(app_text, "/*var_x*/");
    let hx = client.hover_uri(&app_uri, l, c);
    assert_eq!(
        hx["contents"]["value"],
        json!("```java\nList<User> x\n```\nlocal variable")
    );

    let (l, c) = line_col(app_text, "/*var_u*/");
    let hu = client.hover_uri(&app_uri, l, c);
    assert_eq!(
        hu["contents"]["value"],
        json!("```java\nUser u\n```\nlocal variable")
    );

    let (l, c) = line_col(app_text, "/*list_get*/");
    let hget = client.hover_uri(&app_uri, l, c);
    let hget_md = hget["contents"]["value"].as_str().unwrap();
    assert!(hget_md.contains("User java.util.List.get(int "), "{hget_md}");

    let (l, c) = line_col(app_text, "/*lparam*/");
    let hlparam = client.hover_uri(&app_uri, l, c);
    assert_eq!(
        hlparam["contents"]["value"],
        json!("```java\nUser item\n```\nlambda parameter")
    );

    let (l, c) = line_col(app_text, "/*names*/");
    let hnames = client.hover_uri(&app_uri, l, c);
    assert_eq!(
        hnames["contents"]["value"],
        json!("```java\nList<String> names\n```\nlocal variable")
    );

    let (l, c) = line_col(app_text, "/*box_get*/");
    let hbox = client.hover_uri(&app_uri, l, c);
    assert_eq!(
        hbox["contents"]["value"],
        json!("```java\nUser com.example.Box.get()\n```")
    );

    let (l, c) = line_col(app_text, "/*fe_u*/");
    let hfe = client.hover_uri(&app_uri, l, c);
    assert_eq!(
        hfe["contents"]["value"],
        json!("```java\nUser elem\n```\nfor-each variable")
    );

    // Binary overload hover on List.remove(int) vs List.remove(Object)
    let (l, c) = line_col(app_text, "/*rem_idx*/");
    let h_rem_idx = client.hover_uri(&app_uri, l, c);
    let h_rem_idx_md = h_rem_idx["contents"]["value"].as_str().unwrap();
    assert!(h_rem_idx_md.contains("User java.util.List.remove(int "), "{h_rem_idx_md}");

    let (l, c) = line_col(app_text, "/*rem_obj*/");
    let h_rem_obj = client.hover_uri(&app_uri, l, c);
    let h_rem_obj_md = h_rem_obj["contents"]["value"].as_str().unwrap();
    assert!(
        h_rem_obj_md.contains("boolean java.util.List.remove(Object "),
        "{h_rem_obj_md}"
    );

    // 2. textDocument/definition on chained generic calls, lambdas, method refs, and overloads
    for marker in ["/*chain_get_name*/", "/*lambda_get_name*/"] {
        let (l, c) = line_col(app_text, marker);
        assert_eq!(
            client.definition_uri(&app_uri, l, c),
            json!({ "uri": user_uri, "range": lsp_range(2, 18, 2, 25) }),
            "at {marker}"
        );
    }

    let (l, c) = line_col(app_text, "/*mref_ping*/");
    assert_eq!(
        client.definition_uri(&app_uri, l, c),
        json!({ "uri": user_uri, "range": lsp_range(3, 16, 3, 20) })
    );

    let (l, c) = line_col(app_text, "/*fe_ping*/");
    assert_eq!(
        client.definition_uri(&app_uri, l, c),
        json!({ "uri": user_uri, "range": lsp_range(3, 16, 3, 20) })
    );

    let (l, c) = line_col(app_text, "/*foo_int*/");
    assert_eq!(
        client.definition_uri(&app_uri, l, c),
        json!({ "uri": app_uri, "range": lsp_range(3, 9, 3, 12) })
    );

    let (l, c) = line_col(app_text, "/*foo_str*/");
    assert_eq!(
        client.definition_uri(&app_uri, l, c),
        json!({ "uri": app_uri, "range": lsp_range(4, 9, 4, 12) })
    );

    // 3. textDocument/typeDefinition on `var`, `var u = users.get(0);`, lambda param, and for-each `elem` -> User
    for marker in ["/*var_kw*/", "/*var_u*/", "/*lparam*/", "/*fe_u*/"] {
        let (l, c) = line_col(app_text, marker);
        assert_eq!(
            client.type_definition_uri(&app_uri, l, c),
            json!({ "uri": user_uri, "range": lsp_range(1, 13, 1, 17) }),
            "at {marker}"
        );
    }

    // typeDefinition on `var x = List.of(new User());` -> materialized binary List stub
    let (l, c) = line_col(app_text, "/*var_x*/");
    let list_td = client.type_definition_uri(&app_uri, l, c);
    assert_ne!(list_td, Value::Null);
    let list_uri = list_td["uri"].as_str().expect("uri");
    let list_path = uri_to_path(&list_uri.parse::<Uri>().unwrap()).unwrap();
    assert!(list_path.is_file());
    let list_stub = fs::read_to_string(&list_path).unwrap();
    assert!(list_stub.contains("interface List"), "{list_stub}");
    let _ = fs::remove_file(&list_path);

    shutdown(client, handle);
}

#[test]
fn lsp_real_jdk_generics_hover_definition_and_type_definition() {
    if discover_jdks(None).is_empty() {
        return;
    }

    let ws = TempWorkspace::new();
    let user_text = "\
package com.example;
public class User {
    public String getName() { return \"\"; }
    public void ping() {}
}
";
    let app_text = "\
package com.example;
import java.util.List;
public class App {
    void run(List<User> users) {
        var /*var_x*/x = List.of(new User());
        var /*var_u*/u = users./*list_get*/get(0);
        var /*names*/names = users.stream().map(/*lparam*/item -> item./*lambda_get_name*/getName()).toList();
        users.forEach(User::/*mref_ping*/ping);
        String n = users.get(0)./*chain_get_name*/getName();
        System.out./*println_int*/println(1);
        System.out./*println_str*/println(\"hi\");
    }
}
";
    let user_uri = ws.write_uri("src/com/example/User.java", user_text);
    let app_uri = ws.write_uri("src/com/example/App.java", app_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&app_uri, app_text);

    let (l, c) = line_col(app_text, "/*var_x*/");
    let hx = client.hover_uri(&app_uri, l, c);
    assert_eq!(
        hx["contents"]["value"],
        json!("```java\nList<User> x\n```\nlocal variable")
    );

    let (l, c) = line_col(app_text, "/*list_get*/");
    let hget = client.hover_uri(&app_uri, l, c);
    let hget_md = hget["contents"]["value"].as_str().unwrap();
    assert!(hget_md.contains("User java.util.List.get(int "), "{hget_md}");

    let (l, c) = line_col(app_text, "/*lparam*/");
    let hlparam = client.hover_uri(&app_uri, l, c);
    assert_eq!(
        hlparam["contents"]["value"],
        json!("```java\nUser item\n```\nlambda parameter")
    );

    let (l, c) = line_col(app_text, "/*names*/");
    let hnames = client.hover_uri(&app_uri, l, c);
    assert_eq!(
        hnames["contents"]["value"],
        json!("```java\nList<String> names\n```\nlocal variable")
    );

    // Real JDK overload resolution on PrintStream.println(int) vs PrintStream.println(String)
    let (l, c) = line_col(app_text, "/*println_int*/");
    let hp_int = client.hover_uri(&app_uri, l, c);
    let hp_int_md = hp_int["contents"]["value"].as_str().unwrap();
    assert!(
        hp_int_md.contains("void java.io.PrintStream.println(int "),
        "{hp_int_md}"
    );

    let (l, c) = line_col(app_text, "/*println_str*/");
    let hp_str = client.hover_uri(&app_uri, l, c);
    let hp_str_md = hp_str["contents"]["value"].as_str().unwrap();
    assert!(
        hp_str_md.contains("void java.io.PrintStream.println(String "),
        "{hp_str_md}"
    );

    for marker in ["/*chain_get_name*/", "/*lambda_get_name*/"] {
        let (l, c) = line_col(app_text, marker);
        assert_eq!(
            client.definition_uri(&app_uri, l, c),
            json!({ "uri": user_uri, "range": lsp_range(2, 18, 2, 25) }),
            "at {marker}"
        );
    }

    let (l, c) = line_col(app_text, "/*var_u*/");
    assert_eq!(
        client.type_definition_uri(&app_uri, l, c),
        json!({ "uri": user_uri, "range": lsp_range(1, 13, 1, 17) })
    );

    let (l, c) = line_col(app_text, "/*mref_ping*/");
    assert_eq!(
        client.definition_uri(&app_uri, l, c),
        json!({ "uri": user_uri, "range": lsp_range(3, 16, 3, 20) })
    );

    let (l, c) = line_col(app_text, "/*list_get*/");
    let list_def = client.definition_uri(&app_uri, l, c);
    assert_ne!(list_def, Value::Null);
    let list_uri = list_def["uri"].as_str().expect("uri");
    let list_path = uri_to_path(&list_uri.parse::<Uri>().unwrap()).unwrap();
    assert!(list_path.is_file());
    let _ = fs::remove_file(&list_path);

    shutdown(client, handle);
}
