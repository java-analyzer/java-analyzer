mod common;

use common::{URI, shutdown, start_server};
use serde_json::json;

const PUBLISH: &str = "textDocument/publishDiagnostics";

#[test]
fn valid_file_has_no_diagnostics() {
    let (client, handle) = start_server();
    client.open("package a;\nclass Main { int x; }\n");

    let params = client.next_notification(PUBLISH);
    assert_eq!(params["uri"], json!(URI));
    assert_eq!(params["version"], json!(1));
    assert_eq!(params["diagnostics"], json!([]));

    shutdown(client, handle);
}

#[test]
fn syntax_error_is_published_with_lsp_range() {
    let (client, handle) = start_server();
    client.open("class Main {\n    int x\n}\n");

    let params = client.next_notification(PUBLISH);
    let diagnostics = params["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["message"], json!("expected ';'"));
    assert_eq!(diagnostics[0]["severity"], json!(1));
    assert_eq!(diagnostics[0]["source"], json!("java-analyzer"));
    // Just after `x` on the second line.
    assert_eq!(
        diagnostics[0]["range"],
        json!({ "start": { "line": 1, "character": 9 }, "end": { "line": 1, "character": 9 } })
    );

    shutdown(client, handle);
}

#[test]
fn diagnostics_columns_are_utf16() {
    let (client, handle) = start_server();
    // `😀` is one char but two UTF-16 units, so the string's opening quote is at UTF-16 column
    // 28 (it would be 27 counting chars, or 30 counting bytes).
    client.open("class A { /*😀*/ String s = \"oops;\n}\n");

    let params = client.next_notification(PUBLISH);
    let diagnostics = params["diagnostics"].as_array().unwrap();
    let unterminated = diagnostics
        .iter()
        .find(|d| d["message"] == json!("unterminated string literal"))
        .unwrap_or_else(|| panic!("no unterminated string diagnostic in {diagnostics:?}"));
    assert_eq!(
        unterminated["range"]["start"],
        json!({ "line": 0, "character": 28 })
    );

    shutdown(client, handle);
}

#[test]
fn fixing_the_file_clears_diagnostics() {
    let (client, handle) = start_server();
    client.open("class Main {");
    let first = client.next_notification(PUBLISH);
    assert_eq!(first["diagnostics"].as_array().unwrap().len(), 1);

    client.change(2, "class Main {}");
    let second = client.next_notification(PUBLISH);
    assert_eq!(second["version"], json!(2));
    assert_eq!(second["diagnostics"], json!([]));

    shutdown(client, handle);
}

#[test]
fn closing_the_file_clears_diagnostics() {
    let (client, handle) = start_server();
    client.open("class Main {");
    let opened = client.next_notification(PUBLISH);
    assert_eq!(opened["diagnostics"].as_array().unwrap().len(), 1);

    client.close();
    let closed = client.next_notification(PUBLISH);
    assert_eq!(closed["uri"], json!(URI));
    assert_eq!(closed["diagnostics"], json!([]));

    shutdown(client, handle);
}
