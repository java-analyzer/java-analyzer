mod common;

use common::{shutdown, start_server};
use serde_json::{json, Value};

#[test]
fn hover_on_identifier_after_open() {
    let (mut client, handle) = start_server();
    client.open("class Main { }");

    let result = client.hover(0, 7);
    assert_eq!(result["contents"]["value"], json!("```java\nclass Main\n```"));

    shutdown(client, handle);
}

#[test]
fn hover_reflects_document_changes() {
    let (mut client, handle) = start_server();
    client.open("class Main { }");
    client.change(2, "class Renamed { }");

    let result = client.hover(0, 8);
    assert_eq!(
        result["contents"]["value"],
        json!("```java\nclass Renamed\n```")
    );

    shutdown(client, handle);
}

#[test]
fn hover_falls_back_for_keyword_and_unresolved_identifier() {
    let (mut client, handle) = start_server();
    client.open("class Main { String s; }");

    let kw = client.hover(0, 1);
    assert_eq!(
        kw["contents"]["value"],
        json!("Keyword `class` declares a new class")
    );

    let unresolved = client.hover(0, 13);
    assert_eq!(unresolved["contents"]["value"], json!("Identifier `String`"));

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
