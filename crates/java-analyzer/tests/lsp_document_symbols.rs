mod common;

use common::{shutdown, start_server, URI};
use serde_json::{json, Value};

fn symbols(client: &mut common::Client) -> Value {
    client.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": URI } }),
    )
}

#[test]
fn document_symbols_are_hierarchical_with_lsp_ranges() {
    let (mut client, handle) = start_server();
    client.open("class A {\n    int x;\n    void run(int n) {}\n}\n");

    let result = symbols(&mut client);
    let top = result.as_array().expect("an array of symbols");
    assert_eq!(top.len(), 1, "{result}");

    let class = &top[0];
    assert_eq!(class["name"], json!("A"));
    assert_eq!(class["kind"], json!(5)); // SymbolKind::CLASS
    assert_eq!(
        class["selectionRange"],
        json!({ "start": { "line": 0, "character": 6 }, "end": { "line": 0, "character": 7 } })
    );
    assert_eq!(
        class["range"],
        json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 3, "character": 1 } })
    );

    let children = class["children"].as_array().expect("children");
    assert_eq!(children.len(), 2);
    assert_eq!(children[0]["name"], json!("x"));
    assert_eq!(children[0]["kind"], json!(8)); // FIELD
    assert_eq!(children[0]["detail"], json!("int"));
    assert_eq!(children[1]["name"], json!("run"));
    assert_eq!(children[1]["kind"], json!(6)); // METHOD
    assert_eq!(children[1]["detail"], json!("(int n)"));

    shutdown(client, handle);
}

#[test]
fn document_symbols_follow_edits() {
    let (mut client, handle) = start_server();
    client.open("class A {}");
    assert_eq!(symbols(&mut client)[0]["name"], json!("A"));

    client.change(2, "class Renamed { int f; }");
    let result = symbols(&mut client);
    assert_eq!(result[0]["name"], json!("Renamed"));
    assert_eq!(result[0]["children"][0]["name"], json!("f"));

    shutdown(client, handle);
}

#[test]
fn document_symbols_of_an_unknown_file_are_null() {
    let (mut client, handle) = start_server();
    let result = client.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": "file:///tmp/Other.java" } }),
    );
    assert_eq!(result, Value::Null);

    shutdown(client, handle);
}
