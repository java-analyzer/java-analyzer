#![expect(
    clippy::unwrap_used,
    reason = "integration test helper functions"
)]

mod common;

use std::{fs, io::Write as _, path::Path};

use common::{TempWorkspace, shutdown, start_server_with_root};
use java_analyzer::convert::uri_to_path;
use lsp_types::Uri;
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

fn build_class_bytes(
    internal_name: &str,
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
    out.extend_from_slice(&0x0001u16.to_be_bytes()); // ACC_PUBLIC
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
fn lsp_hover_enriches_javadoc_and_goto_definition_materializes_sources_and_stubs() {
    let ws = TempWorkspace::new();

    // JAR 1: with companion -sources.jar
    let widget_cls = build_class_bytes(
        "com/example/ext/Widget",
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
            b"package com.example.ext;\n/** A UI widget. */\npublic class Widget {\n    /** Renders the widget. */\n    public int render() { return 42; }\n}\n",
        )],
    );

    // JAR 2: with companion -javadoc.jar only (no sources -> stub fallback for goto_definition)
    let gadget_cls = build_class_bytes(
        "com/example/ext/Gadget",
        &[("spin", "()V", 0x0001)],
        &[],
    );
    write_jar(
        &ws.path().join("libs/gadget.jar"),
        &[("com/example/ext/Gadget.class", &gadget_cls)],
    );
    write_jar(
        &ws.path().join("libs/gadget-javadoc.jar"),
        &[(
            "com/example/ext/Gadget.html",
            br#"<html><body>
<section class="class-description" id="class-description">
<div class="block">A mechanical gadget.</div>
</section>
<section class="detail" id="spin()">
<h3>spin</h3>
<div class="block">Spins the gadget quickly.</div>
</section>
</body></html>"#,
        )],
    );

    ws.write(
        ".java-analyzer.toml",
        r#"release = 21
[modules."."]
dependencies = { include = ["libs/widget.jar", "libs/gadget.jar"] }
"#,
    );

    let app_text = "\
package com.app;
import com.example.ext.Widget;
import com.example.ext.Gadget;
public class App {
    void demo(Widget w, Gadget g) {
        int r = w.render();
        g.spin();
    }
}
";
    let app_uri = ws.write_uri("src/com/app/App.java", app_text);

    let (mut client, handle) = start_server_with_root(&ws.root_uri());
    client.open_uri(&app_uri, app_text);

    // 1. Hover on `w.render()` (line 5, col 18) -> enriched from widget-sources.jar
    let render_hover = client.hover_uri(&app_uri, 5, 18);
    let render_md = render_hover["contents"]["value"]
        .as_str()
        .expect("hover markdown");
    assert!(
        render_md.contains("int com.example.ext.Widget.render()"),
        "hover: {render_md}"
    );
    assert!(
        render_md.contains("Renders the widget."),
        "expected source javadoc in hover: {render_md}"
    );
    assert!(
        render_md.contains("*from `libs/widget.jar`*"),
        "expected origin in hover: {render_md}"
    );

    // 2. Hover on `g.spin()` (line 6, col 10) -> enriched from gadget-javadoc.jar HTML
    let spin_hover = client.hover_uri(&app_uri, 6, 10);
    let spin_md = spin_hover["contents"]["value"]
        .as_str()
        .expect("hover markdown");
    assert!(
        spin_md.contains("void com.example.ext.Gadget.spin()"),
        "hover: {spin_md}"
    );
    assert!(
        spin_md.contains("Spins the gadget quickly."),
        "expected HTML javadoc in hover: {spin_md}"
    );

    // 3. Go-to-definition on `w.render()` -> extracted Widget.java at `render`
    let widget_def = client.definition_uri(&app_uri, 5, 18);
    assert_ne!(widget_def, Value::Null);
    let widget_uri_str = widget_def["uri"].as_str().expect("uri");
    assert!(widget_uri_str.starts_with("file://"), "{widget_def}");
    let widget_path = uri_to_path(&widget_uri_str.parse::<Uri>().unwrap()).unwrap();
    assert!(widget_path.is_file());
    assert_eq!(widget_def["range"]["start"]["line"], json!(4));
    let _ = fs::remove_file(&widget_path);

    // 4. Go-to-definition on `g.spin()` -> synthesized stub Gadget.java at `spin`
    let gadget_def = client.definition_uri(&app_uri, 6, 10);
    assert_ne!(gadget_def, Value::Null);
    let gadget_uri_str = gadget_def["uri"].as_str().expect("uri");
    assert!(gadget_uri_str.starts_with("file://"), "{gadget_def}");
    let gadget_path = uri_to_path(&gadget_uri_str.parse::<Uri>().unwrap()).unwrap();
    assert!(gadget_path.is_file());
    let stub_text = fs::read_to_string(&gadget_path).unwrap();
    assert!(stub_text.contains("public void spin()"), "{stub_text}");
    let _ = fs::remove_file(&gadget_path);

    shutdown(client, handle);
}
