mod common;

use common::{shutdown, start_server};
use serde_json::{json, Value};

#[test]
fn hover_on_identifier_after_open() {
    let (mut client, handle) = start_server();
    client.open("class Main { }");

    let result = client.hover(0, 7);
    assert_eq!(result["contents"]["value"], json!("Identifier `Main`"));

    shutdown(client, handle);
}

#[test]
fn hover_reflects_document_changes() {
    let (mut client, handle) = start_server();
    client.open("class Main { }");
    client.change(2, "class Renamed { }");

    let result = client.hover(0, 8);
    assert_eq!(result["contents"]["value"], json!("Identifier `Renamed`"));

    shutdown(client, handle);
}

#[test]
fn hover_on_whitespace_or_unknown_file_is_null() {
    let (mut client, handle) = start_server();
    client.open("class Main { }");

    assert_eq!(client.hover(0, 5), Value::Null);

    let unknown = client.request(
        "textDocument/hover",
        json!({
            "textDocument": { "uri": "file:///tmp/Other.java" },
            "position": { "line": 0, "character": 0 }
        }),
    );
    assert_eq!(unknown, Value::Null);

    shutdown(client, handle);
}
