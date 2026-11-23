//! Panic-free LSP client over `Content-Length` framed stdio JSON-RPC.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt as _;

use serde_json::{Value, json};

use crate::bench::rss::parse_ps_descendants;

/// Parsed LSP `ServerCapabilities` used to gate benchmark operations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerCaps {
    pub hover: bool,
    pub definition: bool,
    pub type_definition: bool,
    pub implementation: bool,
    pub type_hierarchy: bool,
    pub document_symbol: bool,
    pub workspace_symbol: bool,
    pub references: bool,
    pub rename: bool,
    pub code_action: bool,
    pub completion: bool,
    pub completion_resolve: bool,
    pub signature_help: bool,
    pub semantic_tokens: bool,
    pub inlay_hints: bool,
    pub formatting: bool,
    pub incremental_sync: bool,
}

impl ServerCaps {
    /// Parses `ServerCapabilities` from an LSP `initialize` result payload.
    pub fn from_initialize_result(result: &Value) -> Self {
        let caps = result.get("capabilities").unwrap_or(&Value::Null);
        let completion_val = caps.get("completionProvider");
        let completion_resolve = completion_val
            .and_then(|v| v.get("resolveProvider"))
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let sync_kind = match caps.get("textDocumentSync") {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::Object(obj)) => obj.get("change").and_then(Value::as_u64),
            _ => None,
        };

        Self {
            hover: cap_enabled(caps.get("hoverProvider")),
            definition: cap_enabled(caps.get("definitionProvider")),
            type_definition: cap_enabled(caps.get("typeDefinitionProvider")),
            implementation: cap_enabled(caps.get("implementationProvider")),
            type_hierarchy: cap_enabled(caps.get("typeHierarchyProvider")),
            document_symbol: cap_enabled(caps.get("documentSymbolProvider")),
            workspace_symbol: cap_enabled(caps.get("workspaceSymbolProvider")),
            references: cap_enabled(caps.get("referencesProvider")),
            rename: cap_enabled(caps.get("renameProvider")),
            code_action: cap_enabled(caps.get("codeActionProvider")),
            completion: cap_enabled(completion_val),
            completion_resolve,
            signature_help: cap_enabled(caps.get("signatureHelpProvider")),
            semantic_tokens: cap_enabled(caps.get("semanticTokensProvider")),
            inlay_hints: cap_enabled(caps.get("inlayHintProvider")),
            formatting: cap_enabled(caps.get("documentFormattingProvider")),
            incremental_sync: sync_kind == Some(2),
        }
    }

    /// Returns a deterministic capability coverage map keyed by operation family.
    pub fn to_coverage_map(&self) -> BTreeMap<String, bool> {
        let mut map = BTreeMap::new();
        map.insert("hover".to_string(), self.hover);
        map.insert("definition".to_string(), self.definition);
        map.insert("typeDefinition".to_string(), self.type_definition);
        map.insert("implementation".to_string(), self.implementation);
        map.insert("typeHierarchy".to_string(), self.type_hierarchy);
        map.insert("documentSymbol".to_string(), self.document_symbol);
        map.insert("workspaceSymbol".to_string(), self.workspace_symbol);
        map.insert("references".to_string(), self.references);
        map.insert("rename".to_string(), self.rename);
        map.insert("codeAction".to_string(), self.code_action);
        map.insert("completion".to_string(), self.completion);
        map.insert("completionResolve".to_string(), self.completion_resolve);
        map.insert("signatureHelp".to_string(), self.signature_help);
        map.insert("semanticTokens".to_string(), self.semantic_tokens);
        map.insert("inlayHint".to_string(), self.inlay_hints);
        map.insert("formatting".to_string(), self.formatting);
        map
    }
}

fn cap_enabled(val: Option<&Value>) -> bool {
    matches!(val, Some(Value::Bool(true)) | Some(Value::Object(_)))
}

/// Failure category for an individual LSP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    Timeout,
    JsonRpc { code: i64, message: String },
    ServerClosed,
    Transport(String),
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "request timed out"),
            Self::JsonRpc { code, message } => write!(f, "JSON-RPC error {code}: {message}"),
            Self::ServerClosed => write!(f, "language server closed stdout"),
            Self::Transport(msg) => write!(f, "transport error: {msg}"),
        }
    }
}

#[derive(Default)]
struct SharedState {
    responses: BTreeMap<i64, Value>,
    active_progress: BTreeSet<String>,
    saw_progress: bool,
    service_ready: bool,
    diagnostics_errors: BTreeMap<String, usize>,
    diagnostics_seq: u64,
    closed: bool,
}

/// Managed LSP child process and JSON-RPC stdio client.
pub struct LspClient {
    child: Option<Child>,
    pid: u32,
    writer: Arc<Mutex<BufWriter<ChildStdin>>>,
    shared: Arc<(Mutex<SharedState>, Condvar)>,
    next_id: i64,
    doc_versions: BTreeMap<String, i32>,
    caps: ServerCaps,
    reader_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
    shutting_down: Arc<AtomicBool>,
}

impl LspClient {
    /// Spawns a language server command template with `{cache_dir}` substitution and `XDG_CACHE_HOME`.
    pub fn spawn(cmd_template: &str, work_dir: &Path, cache_dir: &Path) -> Result<Self, String> {
        let tokens = split_command_tokens(cmd_template, cache_dir)?;
        let (program, args) = tokens
            .split_first()
            .ok_or_else(|| "server command must not be empty".to_string())?;

        let resolved_program: PathBuf = {
            let p = Path::new(program);
            if (program.contains('/') || program.contains('\\')) && p.is_relative() {
                std::env::current_dir()
                    .map(|cwd| cwd.join(p))
                    .unwrap_or_else(|_| p.to_path_buf())
            } else {
                p.to_path_buf()
            }
        };

        let mut cmd = Command::new(&resolved_program);
        cmd.args(args)
            .current_dir(work_dir)
            .env("XDG_CACHE_HOME", cache_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn `{program}`: {e}"))?;
        let pid = child.id();

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "child stdin unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "child stdout unavailable".to_string())?;
        let stderr = child.stderr.take();

        let writer = Arc::new(Mutex::new(BufWriter::new(stdin)));
        let shared = Arc::new((Mutex::new(SharedState::default()), Condvar::new()));
        let shutting_down = Arc::new(AtomicBool::new(false));

        let reader_writer = Arc::clone(&writer);
        let reader_shared = Arc::clone(&shared);
        let reader_thread = thread::spawn(move || {
            read_loop(stdout, reader_writer, reader_shared);
        });

        let stderr_thread = stderr.map(|err| {
            thread::spawn(move || {
                let mut reader = BufReader::new(err);
                let mut sink = String::new();
                while reader.read_line(&mut sink).is_ok_and(|n| n > 0) {
                    sink.clear();
                }
            })
        });

        Ok(Self {
            child: Some(child),
            pid,
            writer,
            shared,
            next_id: 0,
            doc_versions: BTreeMap::new(),
            caps: ServerCaps::default(),
            reader_thread: Some(reader_thread),
            stderr_thread,
            shutting_down,
        })
    }

    /// Returns the OS PID of the spawned server root process.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Returns parsed server capabilities (populated after `initialize`).
    pub fn capabilities(&self) -> &ServerCaps {
        &self.caps
    }

    /// Sends `initialize` + `initialized` and stores the advertised [`ServerCaps`].
    pub fn initialize(&mut self, root_dir: &Path, timeout: Duration) -> Result<ServerCaps, String> {
        let root_uri = path_to_file_uri(root_dir)?;
        let folder_name = root_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("workspace");

        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "workspaceFolders": [
                {
                    "uri": root_uri,
                    "name": folder_name,
                }
            ],
            "capabilities": {
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true,
                    "symbol": { "dynamicRegistration": false }
                },
                "textDocument": {
                    "synchronization": {
                        "dynamicRegistration": false,
                        "didSave": true
                    },
                    "hover": {
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "definition": { "linkSupport": false },
                    "typeDefinition": { "linkSupport": false },
                    "implementation": { "linkSupport": false },
                    "typeHierarchy": { "dynamicRegistration": false },
                    "documentSymbol": {
                        "hierarchicalDocumentSymbolSupport": true
                    },
                    "references": {},
                    "rename": { "prepareSupport": true },
                    "codeAction": {
                        "codeActionLiteralSupport": {
                            "codeActionKind": {
                                "valueSet": ["quickfix", "refactor"]
                            }
                        }
                    },
                    "completion": {
                        "completionItem": {
                            "snippetSupport": false,
                            "resolveSupport": {
                                "properties": ["documentation", "detail", "additionalTextEdits"]
                            }
                        }
                    },
                    "signatureHelp": {},
                    "semanticTokens": {
                        "requests": { "full": true },
                        "tokenTypes": ["namespace", "type", "class", "interface", "enum", "method", "variable", "parameter", "property"],
                        "tokenModifiers": ["declaration", "static", "readonly"],
                        "formats": ["relative"]
                    },
                    "inlayHint": {},
                    "formatting": {}
                },
                "window": {
                    "workDoneProgress": true
                }
            }
        });

        let result = self
            .request("initialize", params, timeout)
            .map_err(|e| format!("initialize failed: {e}"))?;
        let caps = ServerCaps::from_initialize_result(&result);
        self.caps = caps.clone();
        self.notify("initialized", json!({}))?;
        Ok(caps)
    }

    /// Sends a JSON-RPC request and waits up to `timeout` for its response.
    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RequestError> {
        self.next_id += 1;
        let id = self.next_id;
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        send_json_message(&self.writer, &payload).map_err(RequestError::Transport)?;

        let deadline = Instant::now() + timeout;
        let (lock, cvar) = &*self.shared;
        let mut guard = match lock.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };

        loop {
            if let Some(msg) = guard.responses.remove(&id) {
                if let Some(err) = msg.get("error") {
                    let code = err.get("code").and_then(Value::as_i64).unwrap_or(-1);
                    let message = err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown JSON-RPC error")
                        .to_string();
                    return Err(RequestError::JsonRpc { code, message });
                }
                return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
            }

            if guard.closed {
                return Err(RequestError::ServerClosed);
            }

            let now = Instant::now();
            if now >= deadline {
                return Err(RequestError::Timeout);
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next_guard, wait_res) = match cvar.wait_timeout(guard, remaining) {
                Ok(pair) => pair,
                Err(p) => p.into_inner(),
            };
            guard = next_guard;
            if wait_res.timed_out()
                && !guard.responses.contains_key(&id)
                && Instant::now() >= deadline
            {
                return Err(RequestError::Timeout);
            }
        }
    }

    /// Sends a JSON-RPC notification without waiting for a response.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        send_json_message(&self.writer, &payload)
    }

    /// Opens a file via `textDocument/didOpen` (no-op if already open) and returns its URI.
    pub fn ensure_open(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let uri = path_to_file_uri(path)?;
        if self.doc_versions.contains_key(&uri) {
            return Ok(uri);
        }
        self.doc_versions.insert(uri.clone(), 1);
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "java",
                    "version": 1,
                    "text": text,
                }
            }),
        )?;
        Ok(uri)
    }

    /// Sends `textDocument/didChange` replacing the document buffer from `old_text` to `new_text`.
    pub fn change_file(&mut self, uri: &str, old_text: &str, new_text: &str) -> Result<(), String> {
        let next_ver = self.doc_versions.get(uri).copied().unwrap_or(1) + 1;
        self.doc_versions.insert(uri.to_string(), next_ver);

        let changes = if self.caps.incremental_sync {
            let (end_line, end_char) = end_position_utf16(old_text);
            json!([{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": end_line, "character": end_char },
                },
                "text": new_text,
            }])
        } else {
            json!([{ "text": new_text }])
        };

        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": uri,
                    "version": next_ver,
                },
                "contentChanges": changes,
            }),
        )
    }

    /// Waits up to `timeout` for active `$/progress` tokens or `language/status` to settle.
    pub fn wait_for_progress_settle(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let settle_grace = Instant::now() + Duration::from_millis(300);
        let (lock, cvar) = &*self.shared;
        let mut guard = match lock.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };

        loop {
            if guard.closed || guard.service_ready {
                return;
            }
            if guard.saw_progress && guard.active_progress.is_empty() {
                return;
            }
            let now = Instant::now();
            if !guard.saw_progress && now >= settle_grace {
                // Server did not emit `$/progress` within grace window; proceed to probe readiness.
                return;
            }
            if now >= deadline {
                return;
            }
            let wait_until = if !guard.saw_progress {
                settle_grace.min(deadline)
            } else {
                deadline
            };
            let remaining = wait_until.saturating_duration_since(now);
            let (next_guard, _) = match cvar.wait_timeout(guard, remaining) {
                Ok(pair) => pair,
                Err(p) => p.into_inner(),
            };
            guard = next_guard;
        }
    }

    /// Returns the latest diagnostic `ERROR` count recorded for `uri`.
    pub fn diagnostics_error_count(&self, uri: &str) -> Option<usize> {
        let (lock, _) = &*self.shared;
        let guard = match lock.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.diagnostics_errors.get(uri).copied()
    }

    /// Current monotonic diagnostics notification counter.
    pub fn diagnostics_seq(&self) -> u64 {
        let (lock, _) = &*self.shared;
        let guard = match lock.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.diagnostics_seq
    }

    /// Waits briefly for a `publishDiagnostics` notification newer than `prev_seq`.
    pub fn wait_diagnostics_after(&self, uri: &str, prev_seq: u64, timeout: Duration) -> usize {
        let deadline = Instant::now() + timeout;
        let (lock, cvar) = &*self.shared;
        let mut guard = match lock.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };

        while !guard.closed && guard.diagnostics_seq <= prev_seq {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next_guard, _) = match cvar.wait_timeout(guard, remaining) {
                Ok(pair) => pair,
                Err(p) => p.into_inner(),
            };
            guard = next_guard;
        }

        guard.diagnostics_errors.get(uri).copied().unwrap_or(0)
    }

    /// Gracefully shuts down the LSP server (`shutdown` + `exit`), then cleans up its process tree.
    pub fn shutdown(mut self, timeout: Duration) {
        self.teardown(timeout);
    }

    fn teardown(&mut self, timeout: Duration) {
        self.shutting_down.store(true, Ordering::Relaxed);
        let wait_limit = timeout.min(Duration::from_secs(5));

        if self.child.is_some() {
            let _ = self.request("shutdown", Value::Null, wait_limit);
            let _ = self.notify("exit", Value::Null);
        }

        if let Some(mut child) = self.child.take() {
            let exit_deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < exit_deadline => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    _ => {
                        kill_process_tree(self.pid);
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
            // Ensure no descendant helpers remain even if root exited cleanly.
            kill_process_tree(self.pid);
        }

        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if self.child.is_some() {
            self.teardown(Duration::from_secs(2));
        }
    }
}

fn read_loop(
    stdout: std::process::ChildStdout,
    writer: Arc<Mutex<BufWriter<ChildStdin>>>,
    shared: Arc<(Mutex<SharedState>, Condvar)>,
) {
    let mut reader = BufReader::new(stdout);
    while let Some(msg) = read_json_message(&mut reader) {

        let has_method = msg.get("method").and_then(Value::as_str);
        let id_val = msg.get("id");

        match (has_method, id_val) {
            (Some(method), Some(req_id)) => {
                let reply_result = match method {
                    "workspace/configuration" => {
                        let count = msg
                            .get("params")
                            .and_then(|p| p.get("items"))
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len);
                        Value::Array(vec![Value::Null; count])
                    }
                    _ => Value::Null,
                };
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": reply_result,
                });
                let _ = send_json_message(&writer, &resp);
            }
            (None, Some(resp_id)) => {
                if let Some(id) = resp_id.as_i64() {
                    let (lock, cvar) = &*shared;
                    let mut guard = match lock.lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                    guard.responses.insert(id, msg);
                    cvar.notify_all();
                }
            }
            (Some(method), None) => {
                let (lock, cvar) = &*shared;
                let mut guard = match lock.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                handle_server_notification(&mut guard, method, &msg);
                cvar.notify_all();
            }
            (None, None) => {}
        }
    }

    let (lock, cvar) = &*shared;
    let mut guard = match lock.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.closed = true;
    cvar.notify_all();
}

fn handle_server_notification(state: &mut SharedState, method: &str, msg: &Value) {
    let params = msg.get("params").unwrap_or(&Value::Null);
    match method {
        "$/progress" => {
            let token = match params.get("token") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => String::new(),
            };
            let kind = params
                .get("value")
                .and_then(|v| v.get("kind"))
                .and_then(Value::as_str);
            match kind {
                Some("begin") => {
                    state.saw_progress = true;
                    state.active_progress.insert(token);
                }
                Some("end") => {
                    state.saw_progress = true;
                    state.active_progress.remove(&token);
                }
                _ => {}
            }
        }
        "language/status"
            if params.get("type").and_then(Value::as_str) == Some("ServiceReady") =>
        {
            state.service_ready = true;
        }
        "textDocument/publishDiagnostics" => {
            if let Some(uri) = params.get("uri").and_then(Value::as_str) {
                let errors = params
                    .get("diagnostics")
                    .and_then(Value::as_array)
                    .map_or(0, |diags| {
                        diags
                            .iter()
                            .filter(|d| d.get("severity").and_then(Value::as_u64) == Some(1))
                            .count()
                    });
                state.diagnostics_errors.insert(uri.to_string(), errors);
                state.diagnostics_seq = state.diagnostics_seq.saturating_add(1);
            }
        }
        _ => {}
    }
}

fn read_json_message<R: BufRead>(reader: &mut R) -> Option<Value> {
    let mut content_length: Option<usize> = None;
    let mut header_line = String::new();

    loop {
        header_line.clear();
        let read = reader.read_line(&mut header_line).ok()?;
        if read == 0 {
            return None;
        }
        let trimmed = header_line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
            && let Ok(len) = value.trim().parse::<usize>()
        {
            content_length = Some(len);
        }
    }

    let len = content_length?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn send_json_message(
    writer: &Arc<Mutex<BufWriter<ChildStdin>>>,
    value: &Value,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| format!("JSON serialize failed: {e}"))?;
    let mut guard = match writer.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    write!(guard, "Content-Length: {}\r\n\r\n", bytes.len())
        .map_err(|e| format!("write header failed: {e}"))?;
    guard
        .write_all(&bytes)
        .map_err(|e| format!("write payload failed: {e}"))?;
    guard
        .flush()
        .map_err(|e| format!("flush payload failed: {e}"))?;
    Ok(())
}

fn end_position_utf16(text: &str) -> (u32, u32) {
    let mut line_idx = 0u32;
    let mut last_line = "";
    for (idx, line) in text.split('\n').enumerate() {
        line_idx = u32::try_from(idx).unwrap_or(u32::MAX);
        last_line = line.strip_suffix('\r').unwrap_or(line);
    }
    let col = u32::try_from(last_line.encode_utf16().count()).unwrap_or(u32::MAX);
    (line_idx, col)
}

/// Converts a filesystem path into a `file://` URI string.
pub fn path_to_file_uri(path: &Path) -> Result<String, String> {
    let abs: PathBuf = std::fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else if let Ok(cwd) = std::env::current_dir() {
            cwd.join(path)
        } else {
            path.to_path_buf()
        }
    });
    let raw = abs.to_string_lossy();
    let normalized = raw.replace('\\', "/");
    let mut out = String::from("file://");
    if !normalized.starts_with('/') {
        out.push('/');
    }
    for byte in normalized.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.' | '~' | ':') {
            out.push(ch);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    Ok(out)
}

/// Splits a server command template into argv tokens, expanding `{cache_dir}`.
pub fn split_command_tokens(cmd: &str, cache_dir: &Path) -> Result<Vec<String>, String> {
    let cache_str = cache_dir.to_string_lossy();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    let mut chars = cmd.chars().peekable();

    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else if ch == '\\' && q == '"' {
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            } else {
                current.push(ch);
            }
            in_token = true;
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
            in_token = true;
        } else if ch.is_whitespace() {
            if in_token {
                tokens.push(current.replace("{cache_dir}", &cache_str));
                current.clear();
                in_token = false;
            }
        } else if ch == '\\' {
            if let Some(escaped) = chars.next() {
                current.push(escaped);
                in_token = true;
            }
        } else {
            current.push(ch);
            in_token = true;
        }
    }

    if quote.is_some() {
        return Err(format!("unterminated quote in server command `{cmd}`"));
    }
    if in_token {
        tokens.push(current.replace("{cache_dir}", &cache_str));
    }
    Ok(tokens)
}

fn kill_process_tree(root_pid: u32) {
    if let Ok(output) = Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .stderr(Stdio::null())
        .output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        let descendants = parse_ps_descendants(&text, root_pid);
        for desc_pid in descendants.into_iter().rev() {
            let _ = Command::new("kill")
                .args(["-KILL", &desc_pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{root_pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
