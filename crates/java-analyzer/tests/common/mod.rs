#![allow(dead_code)] // shared by several test crates, each using a subset

use std::thread;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{json, Value};

pub const URI: &str = "file:///tmp/Main.java";

/// A test client talking to the server over an in-memory connection.
pub struct Client {
    conn: Connection,
    next_id: i32,
}

impl Client {
    /// Sends a request and returns its result, skipping any notifications that arrive first.
    pub fn request(&mut self, method: &str, params: Value) -> Value {
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

    pub fn notify(&self, method: &str, params: Value) {
        self.conn
            .sender
            .send(Message::Notification(Notification::new(
                method.to_string(),
                params,
            )))
            .unwrap();
    }

    /// Waits for the next notification with this method and returns its params.
    pub fn next_notification(&self, method: &str) -> Value {
        loop {
            match self.conn.receiver.recv().expect("server closed connection") {
                Message::Notification(n) if n.method == method => return n.params,
                _ => continue,
            }
        }
    }

    pub fn open(&self, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": URI, "languageId": "java", "version": 1, "text": text } }),
        );
    }

    pub fn change(&self, version: i32, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": URI, "version": version },
                "contentChanges": [{ "text": text }]
            }),
        );
    }

    pub fn close(&self) {
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": URI } }),
        );
    }

    pub fn hover(&mut self, line: u32, character: u32) -> Value {
        self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": URI },
                "position": { "line": line, "character": character }
            }),
        )
    }
}

pub fn start_server() -> (Client, thread::JoinHandle<()>) {
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

pub fn shutdown(mut client: Client, handle: thread::JoinHandle<()>) {
    client.request("shutdown", Value::Null);
    client.notify("exit", Value::Null);
    handle.join().unwrap();
}
