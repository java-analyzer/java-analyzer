use std::{
    any::Any,
    borrow::Cow,
    collections::BTreeMap,
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::Arc,
    thread::{self, JoinHandle},
};

use ide::{
    Analysis, AnalysisHost, Cancellable, Cancelled, FileId, LanguageLevel, ModuleId,
    is_path_within, normalize_rel_path,
};
use line_index::LineIndex;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    DiagnosticSeverity, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, Location, MarkupContent, MarkupKind, OneOf,
    PublishDiagnosticsParams, ServerCapabilities, SymbolKind, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri,
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
        PublishDiagnostics,
    },
    request::{DocumentSymbolRequest, GotoDefinition, HoverRequest, Request as _},
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    convert::{
        path_to_uri, position_to_offset_with_index, text_range_to_range_with_index, uri_to_path,
    },
    workspace_config::{WorkspaceProbe, parse_shebang_level, probe_workspace},
};

/// Maximum number of concurrent read-only LSP request worker threads.
const MAX_INFLIGHT_WORKERS: usize = 16;

/// All mutable server state. Lives on the main loop thread only.
pub struct GlobalState {
    host: AnalysisHost,
    files: Arc<FxHashMap<Uri, FileId>>,
    file_urls: Arc<FxHashMap<FileId, Uri>>,
    line_indexes: Arc<FxHashMap<FileId, Arc<LineIndex>>>,
    open_files: FxHashSet<Uri>,
    next_file_id: u32,
    workspace_probe: Option<WorkspaceProbe>,
}

/// Immutable snapshot of [`GlobalState`] sent to background request workers.
struct GlobalStateSnapshot {
    analysis: Analysis,
    files: Arc<FxHashMap<Uri, FileId>>,
    file_urls: Arc<FxHashMap<FileId, Uri>>,
    line_indexes: Arc<FxHashMap<FileId, Arc<LineIndex>>>,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalState {
    pub fn new() -> Self {
        Self {
            host: AnalysisHost::new(),
            files: Arc::new(FxHashMap::default()),
            file_urls: Arc::new(FxHashMap::default()),
            line_indexes: Arc::new(FxHashMap::default()),
            open_files: FxHashSet::default(),
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
        self.apply_workspace_probe(probe);
    }

    pub fn apply_workspace_probe(&mut self, mut probe: WorkspaceProbe) {
        for rel_path in probe.file_paths.keys() {
            let abs_path = probe.workspace_dir.join(rel_path);
            let Some(uri) = path_to_uri(&abs_path) else {
                continue;
            };
            if !self.open_files.contains(&uri) {
                let text = probe
                    .file_contents
                    .get(rel_path)
                    .map(Arc::clone)
                    .or_else(|| fs::read_to_string(&abs_path).ok().map(Arc::new));
                if let Some(text) = text {
                    let (file_id, _) = self.ensure_file_id(&uri);
                    let level = parse_shebang_level(&text)
                        .unwrap_or_else(|| probe.language_level_for_path(&abs_path));
                    self.host.set_file_language_level(file_id, level);
                    self.set_file_text(file_id, text);
                }
            }
        }
        probe.file_contents.clear();
        self.workspace_probe = Some(probe);
        self.sync_workspace_files();
    }

    pub fn workspace_probe(&self) -> Option<&WorkspaceProbe> {
        self.workspace_probe.as_ref()
    }

    fn snapshot(&self) -> GlobalStateSnapshot {
        GlobalStateSnapshot {
            analysis: self.host.analysis(),
            files: Arc::clone(&self.files),
            file_urls: Arc::clone(&self.file_urls),
            line_indexes: Arc::clone(&self.line_indexes),
        }
    }

    fn set_file_text(&mut self, file_id: FileId, text: Arc<String>) {
        Arc::make_mut(&mut self.line_indexes).insert(file_id, Arc::new(LineIndex::new(&text)));
        self.host.set_file_text(file_id, text);
    }

    fn line_index(&self, file_id: FileId, text: &str) -> Cow<'_, LineIndex> {
        match self.line_indexes.get(&file_id) {
            Some(index) => Cow::Borrowed(index.as_ref()),
            None => Cow::Owned(LineIndex::new(text)),
        }
    }

    fn ensure_file_id(&mut self, uri: &Uri) -> (FileId, bool) {
        if let Some(&id) = self.files.get(uri) {
            return (id, false);
        }
        let id = FileId(self.next_file_id);
        self.next_file_id += 1;
        Arc::make_mut(&mut self.files).insert(uri.clone(), id);
        Arc::make_mut(&mut self.file_urls).insert(id, uri.clone());
        (id, true)
    }

    fn sync_workspace_files(&mut self) {
        let mut ids: Vec<FileId> = self.file_urls.keys().copied().collect();
        ids.sort();

        let Some(probe) = &self.workspace_probe else {
            self.host.set_workspace_files(Arc::new(ids));
            return;
        };

        if probe.layout.modules.is_empty() {
            self.host.set_workspace_files(Arc::new(ids));
            return;
        }

        let fallback_module = probe
            .layout
            .modules
            .iter()
            .find(|m| m.rel_path == ".")
            .unwrap_or(&probe.layout.modules[0])
            .id;

        let mut by_module: BTreeMap<ModuleId, Vec<FileId>> = probe
            .layout
            .modules
            .iter()
            .map(|m| (m.id, Vec::new()))
            .collect();

        for file_id in ids {
            let mod_id = self
                .file_urls
                .get(&file_id)
                .and_then(uri_to_path)
                .and_then(|file_path| {
                    let rel = file_path.strip_prefix(&probe.workspace_dir).ok()?;
                    let norm = normalize_rel_path(&rel.to_string_lossy());
                    if let Some((module, _)) = probe.layout.enclosing_source_root(&norm) {
                        return Some(module.id);
                    }
                    probe
                        .layout
                        .modules
                        .iter()
                        .filter(|module| is_path_within(&norm, &module.rel_path))
                        .max_by_key(|module| {
                            if module.rel_path == "." {
                                0
                            } else {
                                module.rel_path.len()
                            }
                        })
                        .map(|module| module.id)
                })
                .unwrap_or(fallback_module);

            by_module.entry(mod_id).or_default().push(file_id);
        }

        let modules = by_module
            .into_iter()
            .map(|(id, file_ids)| (id, Arc::new(file_ids)))
            .collect();
        self.host.set_workspace_modules(modules);
    }

    fn language_level_for(&self, uri: &Uri, text: &str) -> LanguageLevel {
        parse_shebang_level(text)
            .or_else(|| {
                let probe = self.workspace_probe.as_ref()?;
                let path = uri_to_path(uri)?;
                Some(probe.language_level_for_path(&path))
            })
            .unwrap_or_else(LanguageLevel::latest)
    }

    pub fn set_text(&mut self, uri: &Uri, text: String) {
        let level = self.language_level_for(uri, &text);
        let (file_id, is_new) = self.ensure_file_id(uri);
        self.open_files.insert(uri.clone());
        self.host.set_file_language_level(file_id, level);
        self.set_file_text(file_id, Arc::new(text));
        if is_new {
            self.sync_workspace_files();
        }
    }

    pub fn close(&mut self, uri: &Uri) {
        self.open_files.remove(uri);
        let Some(&file_id) = self.files.get(uri) else {
            return;
        };

        if let Some(path) = uri_to_path(uri)
            && let Ok(disk_text) = fs::read_to_string(&path)
        {
            let level = self.language_level_for(uri, &disk_text);
            self.host.set_file_language_level(file_id, level);
            self.set_file_text(file_id, Arc::new(disk_text));
            return;
        }

        // Non-disk / deleted file: drop it from the workspace index and clean up its database entry.
        Arc::make_mut(&mut self.files).remove(uri);
        Arc::make_mut(&mut self.file_urls).remove(&file_id);
        Arc::make_mut(&mut self.line_indexes).remove(&file_id);
        self.sync_workspace_files();
        self.host.remove_file(file_id);
    }

    pub fn diagnostics(&self, uri: &Uri, version: Option<i32>) -> PublishDiagnosticsParams {
        let mut diagnostics: Vec<lsp_types::Diagnostic> = if self.open_files.contains(uri) {
            match self.files.get(uri) {
                Some(&file_id) => {
                    let analysis = self.host.analysis();
                    match analysis.file_text(file_id) {
                        Ok(text) => {
                            let index = self.line_index(file_id, &text);
                            let rel_path = self.workspace_probe.as_ref().and_then(|probe| {
                                let file_path = uri_to_path(uri)?;
                                let rel = file_path.strip_prefix(&probe.workspace_dir).ok()?;
                                Some(normalize_rel_path(&rel.to_string_lossy()))
                            });
                            let layout_ctx = match (&self.workspace_probe, &rel_path) {
                                (Some(probe), Some(norm)) => Some((&probe.layout, norm.as_str())),
                                _ => None,
                            };

                            let mut diags: Vec<lsp_types::Diagnostic> = analysis
                                .diagnostics_with_layout(file_id, layout_ctx)
                                .unwrap_or_default()
                                .into_iter()
                                .map(|d| lsp_types::Diagnostic {
                                    range: text_range_to_range_with_index(
                                        &index,
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

                            // Surface workspace-level conflicts
                            if let Some(probe) = &self.workspace_probe {
                                for conflict in &probe.conflicts {
                                    diags.push(lsp_types::Diagnostic {
                                        range: text_range_to_range_with_index(&index, &text, 0, 0),
                                        severity: Some(DiagnosticSeverity::WARNING),
                                        source: Some("java-analyzer".to_string()),
                                        message: conflict.diagnostic_message(),
                                        ..Default::default()
                                    });
                                }
                            }

                            diags
                        }
                        Err(Cancelled) => Vec::new(),
                    }
                }
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };
        diagnostics.sort_by(|a, b| {
            (
                a.range.start.line,
                a.range.start.character,
                a.range.end.line,
                a.range.end.character,
                a.message.as_str(),
            )
                .cmp(&(
                    b.range.start.line,
                    b.range.start.character,
                    b.range.end.line,
                    b.range.end.character,
                    b.message.as_str(),
                ))
        });
        diagnostics.dedup_by(|a, b| a.range == b.range && a.message == b.message);
        PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics,
            version,
        }
    }

    #[cfg(test)]
    fn document_symbols(&self, params: DocumentSymbolParams) -> Option<DocumentSymbolResponse> {
        self.snapshot().document_symbols(params).unwrap_or(None)
    }

    #[cfg(test)]
    fn hover(&self, params: HoverParams) -> Option<Hover> {
        self.snapshot().hover(params).unwrap_or(None)
    }

    #[cfg(test)]
    fn goto_definition(&self, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        self.snapshot().goto_definition(params).unwrap_or(None)
    }
}

impl GlobalStateSnapshot {
    fn line_index(&self, file_id: FileId, text: &str) -> Cow<'_, LineIndex> {
        match self.line_indexes.get(&file_id) {
            Some(index) => Cow::Borrowed(index.as_ref()),
            None => Cow::Owned(LineIndex::new(text)),
        }
    }

    fn document_symbols(
        &self,
        params: DocumentSymbolParams,
    ) -> Cancellable<Option<DocumentSymbolResponse>> {
        let Some(&file_id) = self.files.get(&params.text_document.uri) else {
            return Ok(None);
        };
        let text = self.analysis.file_text(file_id)?;
        let index = self.line_index(file_id, &text);
        let symbols = self
            .analysis
            .document_symbols(file_id)?
            .iter()
            .map(|node| to_document_symbol(&index, &text, node))
            .collect();
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    fn hover(&self, params: HoverParams) -> Cancellable<Option<Hover>> {
        let pos = params.text_document_position_params;
        let Some(&file_id) = self.files.get(&pos.text_document.uri) else {
            return Ok(None);
        };
        let text = self.analysis.file_text(file_id)?;
        let index = self.line_index(file_id, &text);
        let Some(offset) = position_to_offset_with_index(&index, &text, pos.position) else {
            return Ok(None);
        };
        let Some(result) = self.analysis.hover(file_id, offset)? else {
            return Ok(None);
        };
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: result.content,
            }),
            range: None,
        }))
    }

    fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Cancellable<Option<GotoDefinitionResponse>> {
        let pos = params.text_document_position_params;
        let Some(&file_id) = self.files.get(&pos.text_document.uri) else {
            return Ok(None);
        };
        let text = self.analysis.file_text(file_id)?;
        let index = self.line_index(file_id, &text);
        let Some(offset) = position_to_offset_with_index(&index, &text, pos.position) else {
            return Ok(None);
        };
        let Some(target) = self.analysis.goto_definition(file_id, offset)? else {
            return Ok(None);
        };
        let Some(target_uri) = self.file_urls.get(&target.file_id).cloned() else {
            return Ok(None);
        };
        let target_text = self.analysis.file_text(target.file_id)?;
        let target_index = self.line_index(target.file_id, &target_text);
        let range = text_range_to_range_with_index(
            &target_index,
            &target_text,
            target.focus_range.start().into(),
            target.focus_range.end().into(),
        );
        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri: target_uri,
            range,
        })))
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
fn to_document_symbol(index: &LineIndex, text: &str, node: &ide::StructureNode) -> DocumentSymbol {
    let range = |r: ide::TextRange| {
        text_range_to_range_with_index(index, text, r.start().into(), r.end().into())
    };
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
                .map(|c| to_document_symbol(index, text, c))
                .collect(),
        ),
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic")
}

fn prune_finished_workers(workers: &mut Vec<JoinHandle<()>>) {
    let mut i = 0;
    while i < workers.len() {
        if workers[i].is_finished() {
            let handle = workers.swap_remove(i);
            let _ = handle.join();
        } else {
            i += 1;
        }
    }
}

fn join_all_workers(workers: &mut Vec<JoinHandle<()>>) {
    for handle in workers.drain(..) {
        let _ = handle.join();
    }
}

fn finish_pending_probe(
    pending_probe: &mut Option<JoinHandle<WorkspaceProbe>>,
    state: &mut GlobalState,
) {
    if let Some(handle) = pending_probe.take() {
        match handle.join() {
            Ok(probe) => state.apply_workspace_probe(probe),
            Err(payload) => {
                let msg = panic_message(&*payload);
                tracing::error!(panic = %msg, "panic while probing workspace");
            }
        }
    }
}

fn notification_needs_workspace(method: &str) -> bool {
    matches!(
        method,
        DidOpenTextDocument::METHOD | DidChangeTextDocument::METHOD | DidCloseTextDocument::METHOD
    )
}

/// Runs the server main loop until the client requests shutdown/exit.
pub fn run(connection: Connection) -> anyhow::Result<()> {
    let init_params_val = connection.initialize(serde_json::to_value(capabilities())?)?;

    let mut state = GlobalState::new();
    let mut pending_probe: Option<JoinHandle<WorkspaceProbe>> = None;
    match serde_json::from_value::<InitializeParams>(init_params_val) {
        Ok(init_params) => {
            #[allow(deprecated)]
            let workspace_url = init_params
                .workspace_folders
                .as_ref()
                .and_then(|folders| folders.first())
                .map(|f| &f.uri)
                .or(init_params.root_uri.as_ref());
            if let Some(uri) = workspace_url
                && let Some(dir) = uri_to_path(uri)
            {
                pending_probe = Some(thread::spawn(move || probe_workspace(&dir)));
            }
        }
        Err(err) => {
            tracing::warn!(%err, "failed to parse InitializeParams");
        }
    }

    let mut workers: Vec<JoinHandle<()>> = Vec::new();

    for msg in &connection.receiver {
        prune_finished_workers(&mut workers);
        if pending_probe.as_ref().is_some_and(JoinHandle::is_finished) {
            finish_pending_probe(&mut pending_probe, &mut state);
        }

        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    join_all_workers(&mut workers);
                    if let Some(handle) = pending_probe.take() {
                        let _ = handle.join();
                    }
                    return Ok(());
                }
                finish_pending_probe(&mut pending_probe, &mut state);
                dispatch_request(&state, &connection, &mut workers, req)?;
            }
            Message::Notification(not) => {
                if notification_needs_workspace(&not.method) {
                    finish_pending_probe(&mut pending_probe, &mut state);
                }
                let method = not.method.clone();
                match catch_unwind(AssertUnwindSafe(|| handle_notification(&mut state, not))) {
                    Ok(Ok(outgoing_list)) => {
                        for outgoing in outgoing_list {
                            connection.sender.send(Message::Notification(outgoing))?;
                        }
                    }
                    Ok(Err(err)) => {
                        tracing::error!(method = %method, %err, "error while handling LSP notification");
                    }
                    Err(payload) => {
                        let msg = panic_message(&*payload);
                        tracing::error!(method = %method, panic = %msg, "panic while handling LSP notification");
                    }
                }
            }
            Message::Response(_) => {}
        }
    }

    join_all_workers(&mut workers);
    if let Some(handle) = pending_probe.take() {
        let _ = handle.join();
    }
    Ok(())
}

fn spawn_request_worker<F, S>(
    workers: &mut Vec<JoinHandle<()>>,
    id: RequestId,
    method: &'static str,
    work: F,
    send: S,
) where
    F: FnOnce() -> Cancellable<serde_json::Result<serde_json::Value>> + Send + 'static,
    S: FnOnce(Response) + Send + 'static,
{
    prune_finished_workers(workers);
    if workers.len() >= MAX_INFLIGHT_WORKERS {
        let oldest = workers.remove(0);
        let _ = oldest.join();
    }
    let handle = thread::spawn(move || {
        let response = match catch_unwind(AssertUnwindSafe(work)) {
            Ok(Ok(Ok(value))) => Response::new_ok(id, value),
            Ok(Ok(Err(err))) => {
                tracing::error!(method = %method, %err, "failed to serialize LSP response");
                Response::new_err(
                    id,
                    ErrorCode::InternalError as i32,
                    format!("failed to serialize {method} response: {err}"),
                )
            }
            Ok(Err(Cancelled)) => Response::new_err(
                id,
                ErrorCode::ContentModified as i32,
                "content modified".to_string(),
            ),
            Err(payload) => {
                let msg = panic_message(&*payload);
                tracing::error!(method = %method, panic = %msg, "panic while handling LSP request");
                Response::new_err(
                    id,
                    ErrorCode::InternalError as i32,
                    format!("internal error handling {method}: {msg}"),
                )
            }
        };
        send(response);
    });
    workers.push(handle);
}

fn dispatch_request(
    state: &GlobalState,
    connection: &Connection,
    workers: &mut Vec<JoinHandle<()>>,
    req: Request,
) -> anyhow::Result<()> {
    match req.method.as_str() {
        HoverRequest::METHOD => match cast_request::<HoverRequest>(req) {
            Ok((id, params)) => {
                let snap = state.snapshot();
                let sender = connection.sender.clone();
                spawn_request_worker(
                    workers,
                    id,
                    HoverRequest::METHOD,
                    move || Ok(serde_json::to_value(snap.hover(params)?)),
                    move |resp| {
                        let _ = sender.send(Message::Response(resp));
                    },
                );
            }
            Err(response) => connection.sender.send(Message::Response(response))?,
        },
        GotoDefinition::METHOD => match cast_request::<GotoDefinition>(req) {
            Ok((id, params)) => {
                let snap = state.snapshot();
                let sender = connection.sender.clone();
                spawn_request_worker(
                    workers,
                    id,
                    GotoDefinition::METHOD,
                    move || Ok(serde_json::to_value(snap.goto_definition(params)?)),
                    move |resp| {
                        let _ = sender.send(Message::Response(resp));
                    },
                );
            }
            Err(response) => connection.sender.send(Message::Response(response))?,
        },
        DocumentSymbolRequest::METHOD => match cast_request::<DocumentSymbolRequest>(req) {
            Ok((id, params)) => {
                let snap = state.snapshot();
                let sender = connection.sender.clone();
                spawn_request_worker(
                    workers,
                    id,
                    DocumentSymbolRequest::METHOD,
                    move || Ok(serde_json::to_value(snap.document_symbols(params)?)),
                    move |resp| {
                        let _ = sender.send(Message::Response(resp));
                    },
                );
            }
            Err(response) => connection.sender.send(Message::Response(response))?,
        },
        _ => {
            let response = Response::new_err(
                req.id,
                ErrorCode::MethodNotFound as i32,
                format!("unsupported request: {}", req.method),
            );
            connection.sender.send(Message::Response(response))?;
        }
    }
    Ok(())
}

fn cast_request<R>(req: Request) -> Result<(RequestId, R::Params), Response>
where
    R: lsp_types::request::Request,
{
    let id = req.id.clone();
    req.extract(R::METHOD)
        .map_err(|e| Response::new_err(id, ErrorCode::InvalidParams as i32, e.to_string()))
}

fn publish(params: PublishDiagnosticsParams) -> anyhow::Result<Notification> {
    Ok(Notification::new(
        PublishDiagnostics::METHOD.to_string(),
        serde_json::to_value(params)?,
    ))
}

/// Applies a notification to `state` and returns the notifications to send in response.
fn handle_notification(
    state: &mut GlobalState,
    not: Notification,
) -> anyhow::Result<Vec<Notification>> {
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
        dir.write(
            "src/com/example/package-info.java",
            "package com.example;\n",
        );
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
                    && d.message
                        .contains("Conflicting Java release specifications")),
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
        let pkg_info = dir.write(
            "src/com/example/package-info.java",
            "package com.example;\n",
        );
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
        state.set_text(
            &uri,
            "package com.example;\npublic class Live {}\n".to_string(),
        );
        let diags = state.diagnostics(&uri, Some(2));
        assert!(
            diags.diagnostics.is_empty(),
            "Expected diagnostics to clear on in-memory fix, got: {:?}",
            diags.diagnostics
        );

        // 3. Introduce a new mismatch in memory -> diagnostic reflects the live buffer.
        state.set_text(
            &uri,
            "package other.bad;\npublic class Live {}\n".to_string(),
        );
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

    #[test]
    fn line_index_cache_tracks_workspace_edits_closes_and_fallback() {
        let dir = TestDir::new();
        let disk_path = dir.write("src/Disk.java", "class Disk {\n    int x;\n}\n");
        let temp_path = dir.write("src/Temp.java", "class Temp {}\n");
        let disk_uri = path_to_uri(&disk_path).unwrap();
        let temp_uri = path_to_uri(&temp_path).unwrap();

        let mut state = GlobalState::with_workspace(dir.path());
        let disk_id = state.files[&disk_uri];
        let temp_id = state.files[&temp_uri];
        assert!(state.line_indexes.contains_key(&disk_id));
        assert!(state.line_indexes.contains_key(&temp_id));

        // Editing updates cached LineIndex.
        state.set_text(
            &disk_uri,
            "class Disk {\n    int x;\n    int y;\n}\n".to_string(),
        );
        assert_eq!(
            *state.line_indexes[&disk_id],
            LineIndex::new("class Disk {\n    int x;\n    int y;\n}\n")
        );

        // Closing an on-disk file reloads disk text and updates cached LineIndex.
        state.close(&disk_uri);
        assert_eq!(
            *state.line_indexes[&disk_id],
            LineIndex::new("class Disk {\n    int x;\n}\n")
        );

        // Closing a deleted file removes its cached LineIndex.
        fs::remove_file(&temp_path).unwrap();
        state.close(&temp_uri);
        assert!(!state.line_indexes.contains_key(&temp_id));

        // Even if a LineIndex entry is absent, queries fall back cleanly without panicking.
        state.set_text(&disk_uri, "class Disk { int x = ; }\n".to_string());
        Arc::make_mut(&mut state.line_indexes).remove(&disk_id);
        let diags = state.diagnostics(&disk_uri, Some(1));
        assert!(!diags.diagnostics.is_empty());
        let syms = state.document_symbols(DocumentSymbolParams {
            text_document: lsp_types::TextDocumentIdentifier {
                uri: disk_uri.clone(),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        });
        assert!(syms.is_some());
        let _ = state.hover(HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier {
                    uri: disk_uri.clone(),
                },
                position: lsp_types::Position::new(0, 6),
            },
            work_done_progress_params: Default::default(),
        });
        let _ = state.goto_definition(GotoDefinitionParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: disk_uri },
                position: lsp_types::Position::new(0, 6),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        });
    }

    #[test]
    fn diagnostics_deduplicates_non_adjacent_duplicates() {
        let dir = TestDir::new();
        dir.write(".java-version", "17\n");
        dir.write(".sdkmanrc", "java=21.0.2-tem\n");
        dir.write("pom.xml", "<project/>\n");
        dir.write("build.gradle", "");
        let java_path = dir.write("src/Main.java", "class Main {}\n");

        let mut state = GlobalState::with_workspace(dir.path());
        let uri = path_to_uri(&java_path).unwrap();
        state.set_text(&uri, "class Main {}\n".to_string());

        let probe = state.workspace_probe.as_mut().unwrap();
        assert!(probe.conflicts.len() >= 2);
        let first_conflict = probe.conflicts[0].clone();
        let dup_msg = first_conflict.diagnostic_message();
        // Push a duplicate after a different conflict so duplicates are non-adjacent.
        probe.conflicts.push(first_conflict);

        let pub_diags = state.diagnostics(&uri, Some(1));
        assert_eq!(
            pub_diags
                .diagnostics
                .iter()
                .filter(|d| d.message == dup_msg)
                .count(),
            1,
            "Expected non-adjacent duplicate diagnostic to be deduplicated, got: {:?}",
            pub_diags.diagnostics
        );
        assert_eq!(pub_diags.diagnostics.len(), 2);
    }

    #[test]
    fn apply_workspace_probe_clears_file_contents_and_snapshots_share_maps() {
        let dir = TestDir::new();
        dir.write("src/Main.java", "class Main {}\n");

        let state = GlobalState::with_workspace(dir.path());
        assert!(
            state
                .workspace_probe()
                .expect("probe stored")
                .file_contents
                .is_empty(),
            "probe.file_contents should be cleared after loading into AnalysisHost"
        );

        let snap = state.snapshot();
        assert!(Arc::ptr_eq(&state.files, &snap.files));
        assert!(Arc::ptr_eq(&state.file_urls, &snap.file_urls));
        assert!(Arc::ptr_eq(&state.line_indexes, &snap.line_indexes));
    }
}
