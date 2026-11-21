#![allow(dead_code)] // shared by several test crates, each using a subset

use std::{
    fs,
    path::{Path, PathBuf},
    thread,
};

use java_analyzer::convert::path_to_uri;
use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{json, Value};

pub struct TempWorkspace(tempfile::TempDir);

impl TempWorkspace {
    pub fn new() -> Self {
        Self(tempfile::TempDir::new().unwrap())
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }

    pub fn write(&self, rel: &str, content: &str) -> PathBuf {
        let full = self.0.path().join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full, content).unwrap();
        full
    }

    pub fn write_uri(&self, rel: &str, content: &str) -> String {
        let path = self.write(rel, content);
        path_to_uri(&path).unwrap().to_string()
    }

    pub fn root_uri(&self) -> String {
        path_to_uri(self.path()).unwrap().to_string()
    }
}

impl Default for TempWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

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
                    return resp.response_result.expect("unexpected error response");
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
        self.open_uri(URI, text);
    }

    pub fn open_uri(&self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "java", "version": 1, "text": text } }),
        );
    }

    pub fn change(&self, version: i32, text: &str) {
        self.change_uri(URI, version, text);
    }

    pub fn change_uri(&self, uri: &str, version: i32, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }]
            }),
        );
    }

    pub fn close(&self) {
        self.close_uri(URI);
    }

    pub fn close_uri(&self, uri: &str) {
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        );
    }

    pub fn hover(&mut self, line: u32, character: u32) -> Value {
        self.hover_uri(URI, line, character)
    }

    pub fn hover_uri(&mut self, uri: &str, line: u32, character: u32) -> Value {
        self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
    }

    pub fn definition_uri(&mut self, uri: &str, line: u32, character: u32) -> Value {
        self.request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
    }
}

pub fn start_server() -> (Client, thread::JoinHandle<()>) {
    start_server_with_init(json!({ "capabilities": {} }))
}

pub fn start_server_with_root(root_uri: &str) -> (Client, thread::JoinHandle<()>) {
    start_server_with_init(json!({
        "rootUri": root_uri,
        "workspaceFolders": [{ "uri": root_uri, "name": "workspace" }],
        "capabilities": {}
    }))
}

pub fn start_server_with_init(init_params: Value) -> (Client, thread::JoinHandle<()>) {
    let (server_conn, client_conn) = Connection::memory();
    let handle = thread::spawn(move || {
        java_analyzer::server::run(server_conn).expect("server failed");
    });
    let mut client = Client {
        conn: client_conn,
        next_id: 0,
    };
    let init = client.request("initialize", init_params);
    assert_eq!(init["capabilities"]["hoverProvider"], json!(true));
    assert_eq!(init["capabilities"]["definitionProvider"], json!(true));
    client.notify("initialized", json!({}));
    (client, handle)
}

pub fn shutdown(mut client: Client, handle: thread::JoinHandle<()>) {
    client.request("shutdown", Value::Null);
    client.notify("exit", Value::Null);
    handle.join().unwrap();
}
