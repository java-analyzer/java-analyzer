use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs,
    path::Path,
    sync::Arc,
};

use hir::{normalize_rel_path, FileId, RootDatabase, SourceDatabase};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
        PublishDiagnostics,
    },
    request::{DocumentSymbolRequest, GotoDefinition, HoverRequest, Request as _},
    DiagnosticSeverity, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, Location, MarkupContent, MarkupKind, OneOf,
    PublishDiagnosticsParams, ServerCapabilities, SymbolKind, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri,
};

use crate::{
    convert::{path_to_uri, position_to_offset, text_range_to_range, uri_to_path},
    workspace_config::{parse_shebang_level, probe_workspace, WorkspaceProbe},
};

type BoxError = Box<dyn Error + Sync + Send>;

/// All mutable server state. Lives on the main loop thread only.
pub struct GlobalState {
    db: RootDatabase,
    files: HashMap<Uri, FileId>,
    file_urls: HashMap<FileId, Uri>,
    open_files: HashSet<Uri>,
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
            file_urls: HashMap::new(),
            open_files: HashSet::new(),
            next_file_id: 0,
            workspace_probe: None,
        }
    }

    pub fn with_workspace(workspace_dir: &Path) -> Self {
        let mut state = Self::new();
        state.set_workspace_root(workspace_dir);
        state
    }

    pub fn set_workspace_root(&mut self, workspace_dir: &Path) {
        let probe = probe_workspace(workspace_dir);
        for rel_path in probe.file_paths.keys() {
            let abs_path = probe.workspace_dir.join(rel_path);
            let Some(uri) = path_to_uri(&abs_path) else {
                continue;
            };
            if !self.open_files.contains(&uri) {
                if let Ok(text) = fs::read_to_string(&abs_path) {
                    let (file_id, _) = self.ensure_file_id(&uri);
                    let level = parse_shebang_level(&text)
                        .unwrap_or_else(|| probe.language_level_for_path(&abs_path));
                    self.db.set_file_language_level(file_id, level);
                    self.db.set_file_text(file_id, Arc::new(text));
                }
            }
        }
        self.workspace_probe = Some(probe);
        self.sync_workspace_files();
    }

    pub fn workspace_probe(&self) -> Option<&WorkspaceProbe> {
        self.workspace_probe.as_ref()
    }

    fn ensure_file_id(&mut self, uri: &Uri) -> (FileId, bool) {
        if let Some(&id) = self.files.get(uri) {
            return (id, false);
        }
        let id = FileId(self.next_file_id);
        self.next_file_id += 1;
        self.files.insert(uri.clone(), id);
        self.file_urls.insert(id, uri.clone());
        (id, true)
    }

    fn sync_workspace_files(&mut self) {
        let mut ids: Vec<FileId> = self.file_urls.keys().copied().collect();
        ids.sort();
        self.db.set_workspace_files(Arc::new(ids));
    }

    fn language_level_for(&self, uri: &Uri, text: &str) -> hir::LanguageLevel {
        parse_shebang_level(text)
            .or_else(|| {
                let probe = self.workspace_probe.as_ref()?;
                let path = uri_to_path(uri)?;
                Some(probe.language_level_for_path(&path))
            })
            .unwrap_or_else(hir::LanguageLevel::latest)
    }

    pub fn set_text(&mut self, uri: &Uri, text: String) {
        let level = self.language_level_for(uri, &text);
        let (file_id, is_new) = self.ensure_file_id(uri);
        self.open_files.insert(uri.clone());
        self.db.set_file_language_level(file_id, level);
        self.db.set_file_text(file_id, Arc::new(text));
        if is_new {
            self.sync_workspace_files();
        }
    }

    pub fn close(&mut self, uri: &Uri) {
        self.open_files.remove(uri);
        let Some(&file_id) = self.files.get(uri) else {
            return;
        };

        if let Some(path) = uri_to_path(uri) {
            if let Ok(disk_text) = fs::read_to_string(&path) {
                let level = self.language_level_for(uri, &disk_text);
                self.db.set_file_language_level(file_id, level);
                self.db.set_file_text(file_id, Arc::new(disk_text));
                return;
            }
        }

        // Non-disk / deleted file: drop it from the workspace index and release its text.
        self.files.remove(uri);
        self.file_urls.remove(&file_id);
        self.db.set_file_text(file_id, Arc::new(String::new()));
        self.sync_workspace_files();
    }

    pub fn diagnostics(&self, uri: &Uri, version: Option<i32>) -> PublishDiagnosticsParams {
        let mut diagnostics: Vec<lsp_types::Diagnostic> = if self.open_files.contains(uri) {
            match self.files.get(uri) {
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

                        if let Some(file_path) = uri_to_path(uri) {
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
            }
        } else {
            Vec::new()
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

    fn goto_definition(&self, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let pos = params.text_document_position_params;
        let file_id = *self.files.get(&pos.text_document.uri)?;
        let text = self.db.file_text(file_id);
        let offset = position_to_offset(&text, pos.position)?;
        let target = ide::goto_definition(&self.db, file_id, offset)?;
        let target_uri = self.file_urls.get(&target.file_id)?.clone();
        let target_text = self.db.file_text(target.file_id);
        let range = text_range_to_range(
            &target_text,
            target.focus_range.start().into(),
            target.focus_range.end().into(),
        );
        Some(GotoDefinitionResponse::Scalar(Location {
            uri: target_uri,
            range,
        }))
    }
}

pub fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
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
            if let Some(dir) = uri_to_path(uri) {
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
        GotoDefinition::METHOD => match cast_request::<GotoDefinition>(req) {
            Ok((id, params)) => Response::new_ok(id, state.goto_definition(params)),
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
    use crate::test_util::TestDir;
    use std::fs;

    #[test]
    fn surfaces_package_mismatch_and_conflict_diagnostics_in_lsp() {
        let dir = TestDir::new();
        dir.write(".java-version", "17\n");
        dir.write(".sdkmanrc", "java=21.0.2-tem\n");
        dir.write("src/com/example/package-info.java", "package com.example;\n");
        let bad_text = "package wrong.pkg;\npublic class Broken {}\n";
        let bad_file = dir.write("src/com/example/Broken.java", bad_text);

        let mut state = GlobalState::with_workspace(dir.path());
        let uri = path_to_uri(&bad_file).unwrap();
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
    }

    #[test]
    fn package_mismatch_updates_live_without_disk_writes_or_reprobing() {
        let dir = TestDir::new();
        let pkg_info = dir.write("src/com/example/package-info.java", "package com.example;\n");
        let bad_text = "package wrong.pkg;\npublic class Live {}\n";
        let file_path = dir.write("src/com/example/Live.java", bad_text);

        let mut state = GlobalState::with_workspace(dir.path());
        let uri = path_to_uri(&file_path).unwrap();

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
    }
}
