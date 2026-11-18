use std::{collections::HashMap, error::Error, path::Path, sync::Arc};

use hir::{normalize_rel_path, FileId, RootDatabase, SourceDatabase};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
        PublishDiagnostics,
    },
    request::{DocumentSymbolRequest, HoverRequest, Request as _},
    DiagnosticSeverity, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, Hover,
    HoverContents, HoverParams, HoverProviderCapability, InitializeParams, MarkupContent,
    MarkupKind, OneOf, PublishDiagnosticsParams, ServerCapabilities, SymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};

use crate::{
    convert::{position_to_offset, text_range_to_range},
    workspace_config::{parse_shebang_level, probe_workspace, WorkspaceProbe},
};

type BoxError = Box<dyn Error + Sync + Send>;

/// All mutable server state. Lives on the main loop thread only.
pub struct GlobalState {
    db: RootDatabase,
    files: HashMap<Url, FileId>,
    next_file_id: u32,
    workspace_probe: Option<WorkspaceProbe>,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalState {
    pub fn new() -> Self {
        Self {
            db: RootDatabase::default(),
            files: HashMap::new(),
            next_file_id: 0,
            workspace_probe: None,
        }
    }

    pub fn with_workspace(workspace_dir: &Path) -> Self {
        Self {
            db: RootDatabase::default(),
            files: HashMap::new(),
            next_file_id: 0,
            workspace_probe: Some(probe_workspace(workspace_dir)),
        }
    }

    pub fn set_workspace_root(&mut self, workspace_dir: &Path) {
        self.workspace_probe = Some(probe_workspace(workspace_dir));
    }

    pub fn workspace_probe(&self) -> Option<&WorkspaceProbe> {
        self.workspace_probe.as_ref()
    }

    pub fn set_text(&mut self, uri: &Url, text: String) {
        let level = parse_shebang_level(&text)
            .or_else(|| {
                let probe = self.workspace_probe.as_ref()?;
                let path = uri.to_file_path().ok()?;
                Some(probe.language_level_for_path(&path))
            })
            .unwrap_or_else(hir::LanguageLevel::latest);

        let file_id = match self.files.get(uri) {
            Some(&id) => id,
            None => {
                let id = FileId(self.next_file_id);
                self.next_file_id += 1;
                self.files.insert(uri.clone(), id);
                id
            }
        };
        self.db.set_file_language_level(file_id, level);
        self.db.set_file_text(file_id, Arc::new(text));
    }

    pub fn close(&mut self, uri: &Url) {
        if let Some(file_id) = self.files.remove(uri) {
            // Free the text; the FileId is never reused.
            self.db.set_file_text(file_id, Arc::new(String::new()));
        }
    }

    pub fn diagnostics(&self, uri: &Url, version: Option<i32>) -> PublishDiagnosticsParams {
        let mut diagnostics: Vec<lsp_types::Diagnostic> = match self.files.get(uri) {
            Some(&file_id) => {
                let text = self.db.file_text(file_id);
                let mut diags: Vec<lsp_types::Diagnostic> = ide::diagnostics(&self.db, file_id)
                    .into_iter()
                    .map(|d| lsp_types::Diagnostic {
                        range: text_range_to_range(
                            &text,
                            d.range.start().into(),
                            d.range.end().into(),
                        ),
                        severity: Some(match d.severity {
                            ide::Severity::Error => DiagnosticSeverity::ERROR,
                        }),
                        source: Some("java-analyzer".to_string()),
                        message: d.message,
                        ..Default::default()
                    })
                    .collect();

                // Surface workspace-level diagnostics (PackageMismatch and multi-spec Conflicts)
                if let Some(probe) = &self.workspace_probe {
                    for conflict in &probe.conflicts {
                        diags.push(lsp_types::Diagnostic {
                            range: text_range_to_range(&text, 0, 0),
                            severity: Some(DiagnosticSeverity::WARNING),
                            source: Some("java-analyzer".to_string()),
                            message: conflict.diagnostic_message(),
                            ..Default::default()
                        });
                    }

                    if let Ok(file_path) = uri.to_file_path() {
                        if let Ok(rel) = file_path.strip_prefix(&probe.workspace_dir) {
                            let norm = normalize_rel_path(&rel.to_string_lossy());
                            let syntax_tree = self.db.syntax_tree(file_id);
                            if let Some(pkg_diag) =
                                probe.layout.check_file_package(file_id, &norm, &syntax_tree)
                            {
                                diags.push(lsp_types::Diagnostic {
                                    range: text_range_to_range(
                                        &text,
                                        pkg_diag.range.start().into(),
                                        pkg_diag.range.end().into(),
                                    ),
                                    severity: Some(DiagnosticSeverity::ERROR),
                                    source: Some("java-analyzer".to_string()),
                                    message: pkg_diag.message(),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }

                diags
            }
            None => Vec::new(),
        };
        diagnostics.dedup_by(|a, b| a.range == b.range && a.message == b.message);
        PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics,
            version,
        }
    }

    fn document_symbols(&self, params: DocumentSymbolParams) -> Option<DocumentSymbolResponse> {
        let file_id = *self.files.get(&params.text_document.uri)?;
        let text = self.db.file_text(file_id);
        let symbols = ide::document_symbols(&self.db, file_id)
            .iter()
            .map(|node| to_document_symbol(&text, node))
            .collect();
        Some(DocumentSymbolResponse::Nested(symbols))
    }

    fn hover(&self, params: HoverParams) -> Option<Hover> {
        let pos = params.text_document_position_params;
        let file_id = *self.files.get(&pos.text_document.uri)?;
        let text = self.db.file_text(file_id);
        let offset = position_to_offset(&text, pos.position)?;
        let result = ide::hover(&self.db, file_id, offset)?;
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: result.content,
            }),
            range: None,
        })
    }
}

pub fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

#[allow(deprecated)] // `DocumentSymbol::deprecated` is deprecated but has no default.
fn to_document_symbol(text: &str, node: &ide::StructureNode) -> DocumentSymbol {
    let range = |r: ide::TextRange| text_range_to_range(text, r.start().into(), r.end().into());
    DocumentSymbol {
        name: node.name.clone(),
        detail: node.detail.clone(),
        kind: match node.kind {
            ide::SymbolKind::Class => SymbolKind::CLASS,
            ide::SymbolKind::Interface | ide::SymbolKind::Annotation => SymbolKind::INTERFACE,
            ide::SymbolKind::Enum => SymbolKind::ENUM,
            ide::SymbolKind::Record => SymbolKind::STRUCT,
            ide::SymbolKind::Field => SymbolKind::FIELD,
            ide::SymbolKind::Method => SymbolKind::METHOD,
            ide::SymbolKind::Constructor => SymbolKind::CONSTRUCTOR,
            ide::SymbolKind::EnumConstant => SymbolKind::ENUM_MEMBER,
        },
        tags: None,
        deprecated: None,
        range: range(node.range),
        selection_range: range(node.selection_range),
        children: Some(
            node.children
                .iter()
                .map(|c| to_document_symbol(text, c))
                .collect(),
        ),
    }
}

/// Runs the server main loop until the client requests shutdown/exit.
pub fn run(connection: Connection) -> Result<(), BoxError> {
    let init_params_val = connection.initialize(serde_json::to_value(capabilities())?)?;

    let mut state = GlobalState::new();
    if let Ok(init_params) = serde_json::from_value::<InitializeParams>(init_params_val) {
        #[allow(deprecated)]
        let workspace_url = init_params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .map(|f| &f.uri)
            .or(init_params.root_uri.as_ref());
        if let Some(uri) = workspace_url {
            if let Ok(dir) = uri.to_file_path() {
                state.set_workspace_root(&dir);
            }
        }
    }

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                let response = handle_request(&state, req);
                connection.sender.send(Message::Response(response))?;
            }
            Message::Notification(not) => {
                for outgoing in handle_notification(&mut state, not)? {
                    connection.sender.send(Message::Notification(outgoing))?;
                }
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn handle_request(state: &GlobalState, req: Request) -> Response {
    match req.method.as_str() {
        HoverRequest::METHOD => match cast_request::<HoverRequest>(req) {
            Ok((id, params)) => Response::new_ok(id, state.hover(params)),
            Err(response) => response,
        },
        DocumentSymbolRequest::METHOD => match cast_request::<DocumentSymbolRequest>(req) {
            Ok((id, params)) => Response::new_ok(id, state.document_symbols(params)),
            Err(response) => response,
        },
        _ => Response::new_err(
            req.id,
            ErrorCode::MethodNotFound as i32,
            format!("unsupported request: {}", req.method),
        ),
    }
}

fn cast_request<R>(req: Request) -> Result<(RequestId, R::Params), Response>
where
    R: lsp_types::request::Request,
{
    let id = req.id.clone();
    req.extract(R::METHOD)
        .map_err(|e| Response::new_err(id, ErrorCode::InvalidParams as i32, e.to_string()))
}

fn publish(params: PublishDiagnosticsParams) -> Result<Notification, BoxError> {
    Ok(Notification::new(
        PublishDiagnostics::METHOD.to_string(),
        serde_json::to_value(params)?,
    ))
}

/// Applies a notification to `state` and returns the notifications to send in response.
fn handle_notification(
    state: &mut GlobalState,
    not: Notification,
) -> Result<Vec<Notification>, BoxError> {
    let mut outgoing = Vec::new();
    match not.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let params: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
            let doc = params.text_document;
            state.set_text(&doc.uri, doc.text);
            outgoing.push(publish(state.diagnostics(&doc.uri, Some(doc.version)))?);
        }
        DidChangeTextDocument::METHOD => {
            let params: lsp_types::DidChangeTextDocumentParams =
                serde_json::from_value(not.params)?;
            let doc = params.text_document;
            // We advertise FULL sync, so the last change holds the whole document.
            if let Some(change) = params.content_changes.into_iter().last() {
                state.set_text(&doc.uri, change.text);
                outgoing.push(publish(state.diagnostics(&doc.uri, Some(doc.version)))?);
            }
        }
        DidCloseTextDocument::METHOD => {
            let params: lsp_types::DidCloseTextDocumentParams = serde_json::from_value(not.params)?;
            let uri = params.text_document.uri;
            state.close(&uri);
            // A closed file has no diagnostics from us: clear whatever the client shows.
            outgoing.push(publish(state.diagnostics(&uri, None))?);
        }
        _ => {}
    }
    Ok(outgoing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env, fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn surfaces_package_mismatch_and_conflict_diagnostics_in_lsp() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir: PathBuf = env::temp_dir().join(format!(
            "java_analyzer_lsp_diag_test_{}_{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(dir.join("src/com/example")).unwrap();

        fs::write(dir.join(".java-version"), "17\n").unwrap();
        fs::write(dir.join(".sdkmanrc"), "java=21.0.2-tem\n").unwrap();
        fs::write(
            dir.join("src/com/example/package-info.java"),
            "package com.example;\n",
        )
        .unwrap();
        let bad_file = dir.join("src/com/example/Broken.java");
        let bad_text = "package wrong.pkg;\npublic class Broken {}\n";
        fs::write(&bad_file, bad_text).unwrap();

        let mut state = GlobalState::with_workspace(&dir);
        let uri = Url::from_file_path(&bad_file).unwrap();
        state.set_text(&uri, bad_text.to_string());

        let pub_diags = state.diagnostics(&uri, Some(1));
        assert!(
            pub_diags
                .diagnostics
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::WARNING)
                    && d.message.contains("Conflicting Java release specifications")),
            "Expected release conflict warning diagnostic, got: {:?}",
            pub_diags.diagnostics
        );
        assert!(
            pub_diags
                .diagnostics
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.contains("declared package 'wrong.pkg' does not match package-info.java package 'com.example'")),
            "Expected package mismatch error diagnostic, got: {:?}",
            pub_diags.diagnostics
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_mismatch_updates_live_without_disk_writes_or_reprobing() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir: PathBuf = env::temp_dir().join(format!(
            "java_analyzer_lsp_live_pkg_test_{}_{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(dir.join("src/com/example")).unwrap();

        let pkg_info = dir.join("src/com/example/package-info.java");
        fs::write(&pkg_info, "package com.example;\n").unwrap();
        let file_path = dir.join("src/com/example/Live.java");
        let bad_text = "package wrong.pkg;\npublic class Live {}\n";
        fs::write(&file_path, bad_text).unwrap();

        let mut state = GlobalState::with_workspace(&dir);
        let uri = Url::from_file_path(&file_path).unwrap();

        // Remove package-info.java on disk after initial probe: if `set_text` re-probed disk,
        // it would lose the authoritative package-info knowledge.
        fs::remove_file(&pkg_info).unwrap();

        // 1. Open with mismatched package -> error diagnostic reported.
        state.set_text(&uri, bad_text.to_string());
        let diags = state.diagnostics(&uri, Some(1));
        assert!(
            diags.diagnostics.iter().any(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.contains("declared package 'wrong.pkg' does not match package-info.java package 'com.example'")
            }),
            "Expected package-info mismatch without re-probing disk, got: {:?}",
            diags.diagnostics
        );

        // 2. Fix package in memory (disk still has `wrong.pkg`) -> diagnostic clears immediately.
        state.set_text(&uri, "package com.example;\npublic class Live {}\n".to_string());
        let diags = state.diagnostics(&uri, Some(2));
        assert!(
            diags.diagnostics.is_empty(),
            "Expected diagnostics to clear on in-memory fix, got: {:?}",
            diags.diagnostics
        );

        // 3. Introduce a new mismatch in memory -> diagnostic reflects the live buffer.
        state.set_text(&uri, "package other.bad;\npublic class Live {}\n".to_string());
        let diags = state.diagnostics(&uri, Some(3));
        assert!(
            diags.diagnostics.iter().any(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR)
                    && d.message.contains("declared package 'other.bad' does not match package-info.java package 'com.example'")
            }),
            "Expected live mismatch for 'other.bad', got: {:?}",
            diags.diagnostics
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
