#![expect(
    clippy::unwrap_used,
    reason = "integration test helper functions"
)]

mod common;

use std::{fs, io::Write as _, path::Path};

use common::{TempWorkspace, shutdown, start_server_with_root};
use serde_json::json;
use zip::write::SimpleFileOptions;

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

#[derive(Clone, Copy)]
enum AnnRetention {
    Visible,
    Invisible,
}

struct CpBuilder(Vec<Vec<u8>>);

impl CpBuilder {
    fn new() -> Self {
        Self(Vec::new())
    }

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

fn encode_attribute(name_idx: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + payload.len());
    out.extend_from_slice(&name_idx.to_be_bytes());
    out.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn encode_annotations_attr(name_idx: u16, type_idxs: &[u16]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&u16::try_from(type_idxs.len()).unwrap().to_be_bytes());
    for &idx in type_idxs {
        payload.extend_from_slice(&idx.to_be_bytes());
        payload.extend_from_slice(&0u16.to_be_bytes());
    }
    encode_attribute(name_idx, &payload)
}

fn encode_return_type_annotation_attr(
    name_idx: u16,
    type_idx: u16,
    type_path: &[(u8, u8)],
) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1u16.to_be_bytes()); // num_annotations
    payload.push(0x14); // target_type = METHOD_RETURN (Empty)
    payload.push(u8::try_from(type_path.len()).unwrap());
    for &(kind, arg_idx) in type_path {
        payload.push(kind);
        payload.push(arg_idx);
    }
    payload.extend_from_slice(&type_idx.to_be_bytes());
    payload.extend_from_slice(&0u16.to_be_bytes()); // num_element_value_pairs
    encode_attribute(name_idx, &payload)
}

fn encode_param_annotations_attr(name_idx: u16, per_param: &[Vec<u16>]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(u8::try_from(per_param.len()).unwrap());
    for anns in per_param {
        payload.extend_from_slice(&u16::try_from(anns.len()).unwrap().to_be_bytes());
        for &idx in anns {
            payload.extend_from_slice(&idx.to_be_bytes());
            payload.extend_from_slice(&0u16.to_be_bytes());
        }
    }
    encode_attribute(name_idx, &payload)
}

struct MethodSpec<'a> {
    name: &'a str,
    desc: &'a str,
    sig: Option<&'a str>,
    flags: u16,
    ret_type_ann: Option<(AnnRetention, &'a str)>,
    param_anns: Vec<(usize, AnnRetention, &'a str)>,
}

fn build_class_with_nullability(
    internal_name: &str,
    access_flags: u16,
    class_sig: Option<&str>,
    super_name: &str,
    class_ann: Option<(AnnRetention, &str)>,
    methods: &[MethodSpec<'_>],
) -> Vec<u8> {
    let mut cp = CpBuilder::new();
    let this_cls = cp.class(internal_name);
    let super_cls = cp.class(super_name);
    let sig_name = cp.utf8("Signature");
    let class_sig_idx = class_sig.map(|s| cp.utf8(s));
    let rva_name = cp.utf8("RuntimeVisibleAnnotations");
    let ria_name = cp.utf8("RuntimeInvisibleAnnotations");
    let rvpa_name = cp.utf8("RuntimeVisibleParameterAnnotations");
    let ripa_name = cp.utf8("RuntimeInvisibleParameterAnnotations");
    let rvta_name = cp.utf8("RuntimeVisibleTypeAnnotations");
    let rita_name = cp.utf8("RuntimeInvisibleTypeAnnotations");

    let class_ann_idx = class_ann.map(|(ret, desc)| (ret, cp.utf8(desc)));

    let mut enc_methods = Vec::new();
    for m in methods {
        let name_idx = cp.utf8(m.name);
        let desc_idx = cp.utf8(m.desc);
        let mut attrs: Vec<Vec<u8>> = Vec::new();
        if let Some(sig) = m.sig {
            let sig_idx = cp.utf8(sig);
            attrs.push(encode_attribute(sig_name, &sig_idx.to_be_bytes()));
        }
        if let Some((ret, desc)) = m.ret_type_ann {
            let attr_name = match ret {
                AnnRetention::Visible => rvta_name,
                AnnRetention::Invisible => rita_name,
            };
            let type_idx = cp.utf8(desc);
            attrs.push(encode_return_type_annotation_attr(attr_name, type_idx, &[]));
        }
        if !m.param_anns.is_empty() {
            let max_param = m.param_anns.iter().map(|(i, _, _)| *i + 1).max().unwrap_or(0);
            let mut vis: Vec<Vec<u16>> = vec![Vec::new(); max_param];
            let mut invis: Vec<Vec<u16>> = vec![Vec::new(); max_param];
            let mut has_vis = false;
            let mut has_invis = false;
            for &(idx, ret, desc) in &m.param_anns {
                let type_idx = cp.utf8(desc);
                match ret {
                    AnnRetention::Visible => {
                        vis[idx].push(type_idx);
                        has_vis = true;
                    }
                    AnnRetention::Invisible => {
                        invis[idx].push(type_idx);
                        has_invis = true;
                    }
                }
            }
            if has_vis {
                attrs.push(encode_param_annotations_attr(rvpa_name, &vis));
            }
            if has_invis {
                attrs.push(encode_param_annotations_attr(ripa_name, &invis));
            }
        }
        enc_methods.push((m.flags, name_idx, desc_idx, attrs));
    }

    let mut class_attrs: Vec<Vec<u8>> = Vec::new();
    if let Some(sig_idx) = class_sig_idx {
        class_attrs.push(encode_attribute(sig_name, &sig_idx.to_be_bytes()));
    }
    if let Some((ret, idx)) = class_ann_idx {
        let attr_name = match ret {
            AnnRetention::Visible => rva_name,
            AnnRetention::Invisible => ria_name,
        };
        class_attrs.push(encode_annotations_attr(attr_name, &[idx]));
    }

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
    out.extend_from_slice(&0u16.to_be_bytes()); // fields
    out.extend_from_slice(&u16::try_from(enc_methods.len()).unwrap().to_be_bytes());
    for (flags, n, d, attrs) in enc_methods {
        out.extend_from_slice(&flags.to_be_bytes());
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&d.to_be_bytes());
        out.extend_from_slice(&u16::try_from(attrs.len()).unwrap().to_be_bytes());
        for attr in attrs {
            out.extend_from_slice(&attr);
        }
    }
    out.extend_from_slice(&u16::try_from(class_attrs.len()).unwrap().to_be_bytes());
    for attr in class_attrs {
        out.extend_from_slice(&attr);
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
fn lsp_nullability_hover_workspace_and_binary_jar() {
    let ws = TempWorkspace::new();

    let obj_cls = build_class_with_nullability(
        "java/lang/Object",
        0x0001,
        None,
        "java/lang/Object",
        None,
        &[],
    );
    let str_cls = build_class_with_nullability(
        "java/lang/String",
        0x0011,
        None,
        "java/lang/Object",
        None,
        &[],
    );
    let list_cls = build_class_with_nullability(
        "java/util/List",
        0x0601,
        Some("<E:Ljava/lang/Object;>Ljava/lang/Object;"),
        "java/lang/Object",
        None,
        &[MethodSpec {
            name: "get",
            desc: "(I)Ljava/lang/Object;",
            sig: Some("(I)TE;"),
            flags: 0x0401,
            ret_type_ann: None,
            param_anns: vec![],
        }],
    );

    // `@NullMarked` binary JAR interface using RuntimeVisible* JSpecify annotations.
    let jspec_repo_cls = build_class_with_nullability(
        "com/lib/NullRepo",
        0x0601,
        None,
        "java/lang/Object",
        Some((AnnRetention::Visible, "Lorg/jspecify/annotations/NullMarked;")),
        &[
            MethodSpec {
                name: "findUser",
                desc: "(Ljava/lang/String;)Lcom/example/User;",
                sig: None,
                flags: 0x0401,
                ret_type_ann: Some((AnnRetention::Visible, "Lorg/jspecify/annotations/Nullable;")),
                param_anns: vec![],
            },
            MethodSpec {
                name: "requireUser",
                desc: "(Ljava/lang/String;)Lcom/example/User;",
                sig: None,
                flags: 0x0401,
                ret_type_ann: None,
                param_anns: vec![],
            },
        ],
    );

    // Unmarked binary JAR interface using RuntimeInvisible* JetBrains annotations.
    let jb_repo_cls = build_class_with_nullability(
        "com/lib/JetBrainsRepo",
        0x0601,
        None,
        "java/lang/Object",
        None,
        &[MethodSpec {
            name: "lookup",
            desc: "(Ljava/lang/String;)Lcom/example/User;",
            sig: None,
            flags: 0x0401,
            ret_type_ann: Some((AnnRetention::Invisible, "Lorg/jetbrains/annotations/Nullable;")),
            param_anns: vec![(0, AnnRetention::Invisible, "Lorg/jetbrains/annotations/NotNull;")],
        }],
    );

    write_jar(
        &ws.path().join("libs/null-api.jar"),
        &[
            ("java/lang/Object.class", &obj_cls),
            ("java/lang/String.class", &str_cls),
            ("java/util/List.class", &list_cls),
            ("com/lib/NullRepo.class", &jspec_repo_cls),
            ("com/lib/JetBrainsRepo.class", &jb_repo_cls),
        ],
    );

    ws.write(
        ".java-analyzer.toml",
        r#"release = 21
[modules."."]
dependencies = { include = ["libs/null-api.jar"] }
"#,
    );

    let pkg_info = "\
@org.jspecify.annotations.NullMarked
package com.example;
";
    let user_text = "\
package com.example;
import org.jspecify.annotations.Nullable;
public class User {
    public String name;
    public @Nullable String nickname;
}
";
    let app_text = "\
package com.example;
import com.lib.NullRepo;
import com.lib.JetBrainsRepo;
import java.util.List;

public class App {
    void run(boolean flag, NullRepo repo, JetBrainsRepo jb, List<User> /*p_users*/users) {
        var /*v_new*/u = new User();
        var /*v_lub*/maybe = flag ? u : null;
        var /*v_jspec*/j1 = repo./*call_find*/findUser(\"id\");
        var /*v_req*/j2 = repo./*call_req*/requireUser(\"id\");
        var /*v_jb*/j3 = jb./*call_jb*/lookup(\"id\");
        var /*v_list*/u0 = users.get(0);
    }
}
";
    ws.write_uri("src/com/example/package-info.java", pkg_info);
    ws.write_uri("src/com/example/User.java", user_text);
    let app_uri = ws.write_uri("src/com/example/App.java", app_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&app_uri, app_text);

    let mut check_hover = |marker: &str, expected: &str| {
        let (l, c) = line_col(app_text, marker);
        let h = client.hover_uri(&app_uri, l, c);
        assert_eq!(h["contents"]["value"], json!(expected), "at {marker}");
    };

    check_hover(
        "/*p_users*/",
        "```java\nList<User!>! users\n```\nparameter",
    );
    check_hover("/*v_new*/", "```java\nUser! u\n```\nlocal variable");
    check_hover("/*v_lub*/", "```java\nUser? maybe\n```\nlocal variable");
    check_hover("/*v_jspec*/", "```java\nUser? j1\n```\nlocal variable");
    check_hover("/*v_req*/", "```java\nUser! j2\n```\nlocal variable");
    check_hover("/*v_jb*/", "```java\nUser? j3\n```\nlocal variable");
    check_hover("/*v_list*/", "```java\nUser! u0\n```\nlocal variable");

    check_hover(
        "/*call_find*/",
        "```java\nUser? com.lib.NullRepo.findUser(String! arg0)\n```\n*from `libs/null-api.jar`*",
    );
    check_hover(
        "/*call_req*/",
        "```java\nUser! com.lib.NullRepo.requireUser(String! arg0)\n```\n*from `libs/null-api.jar`*",
    );
    check_hover(
        "/*call_jb*/",
        "```java\nUser? com.lib.JetBrainsRepo.lookup(String! arg0)\n```\n*from `libs/null-api.jar`*",
    );

    shutdown(client, handle);
}
