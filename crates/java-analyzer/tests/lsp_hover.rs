use std::thread;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{json, Value};

struct Client {
    conn: Connection,
    next_id: i32,
}

impl Client {
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        self.conn
            .sender
            .send(Message::Request(Request::new(
                id.clone(),
                method.to_string(),
                params,
            )))
            .unwrap();
        loop {
            match self.conn.receiver.recv().expect("server closed connection") {
                Message::Response(resp) if resp.id == id => {
                    assert!(resp.error.is_none(), "unexpected error: {:?}", resp.error);
                    return resp.result.unwrap_or(Value::Null);
                }
                _ => continue,
            }
        }
    }

    fn notify(&self, method: &str, params: Value) {
        self.conn
            .sender
            .send(Message::Notification(Notification::new(
                method.to_string(),
                params,
            )))
            .unwrap();
    }
}

fn start_server() -> (Client, thread::JoinHandle<()>) {
    let (server_conn, client_conn) = Connection::memory();
    let handle = thread::spawn(move || {
        java_analyzer::server::run(server_conn).expect("server failed");
    });
    let mut client = Client {
        conn: client_conn,
        next_id: 0,
    };
    let init = client.request("initialize", json!({ "capabilities": {} }));
    assert_eq!(init["capabilities"]["hoverProvider"], json!(true));
    client.notify("initialized", json!({}));
    (client, handle)
}

fn shutdown(mut client: Client, handle: thread::JoinHandle<()>) {
    client.request("shutdown", Value::Null);
    client.notify("exit", Value::Null);
    handle.join().unwrap();
}

const URI: &str = "file:///tmp/Main.java";

fn open(client: &Client, text: &str) {
    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": URI, "languageId": "java", "version": 1, "text": text } }),
    );
}

fn hover(client: &mut Client, line: u32, character: u32) -> Value {
    client.request(
        "textDocument/hover",
        json!({
            "textDocument": { "uri": URI },
            "position": { "line": line, "character": character }
        }),
    )
}

#[test]
fn hover_on_identifier_after_open() {
    let (mut client, handle) = start_server();
    open(&client, "class Main { }");

    let result = hover(&mut client, 0, 7);
    assert_eq!(result["contents"]["value"], json!("Identifier `Main`"));

    shutdown(client, handle);
}

#[test]
fn hover_reflects_document_changes() {
    let (mut client, handle) = start_server();
    open(&client, "class Main { }");
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": URI, "version": 2 },
            "contentChanges": [{ "text": "class Renamed { }" }]
        }),
    );

    let result = hover(&mut client, 0, 8);
    assert_eq!(result["contents"]["value"], json!("Identifier `Renamed`"));

    shutdown(client, handle);
}

#[test]
fn hover_on_whitespace_or_unknown_file_is_null() {
    let (mut client, handle) = start_server();
    open(&client, "class Main { }");

    assert_eq!(hover(&mut client, 0, 5), Value::Null);

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
