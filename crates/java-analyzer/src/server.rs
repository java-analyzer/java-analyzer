use std::{collections::HashMap, error::Error, sync::Arc};

use hir::{FileId, RootDatabase, SourceDatabase};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
        PublishDiagnostics,
    },
    request::{DocumentSymbolRequest, HoverRequest, Request as _},
    DiagnosticSeverity, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, Hover,
    HoverContents, HoverParams, HoverProviderCapability, MarkupContent, MarkupKind, OneOf,
    PublishDiagnosticsParams, ServerCapabilities, SymbolKind, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url,
};

use crate::convert::{position_to_offset, text_range_to_range};

type BoxError = Box<dyn Error + Sync + Send>;

/// All mutable server state. Lives on the main loop thread only.
struct GlobalState {
    db: RootDatabase,
    files: HashMap<Url, FileId>,
    next_file_id: u32,
}

impl GlobalState {
    fn new() -> Self {
        Self {
            db: RootDatabase::default(),
            files: HashMap::new(),
            next_file_id: 0,
        }
    }

    fn set_text(&mut self, uri: &Url, text: String) {
        let file_id = match self.files.get(uri) {
            Some(&id) => id,
            None => {
                let id = FileId(self.next_file_id);
                self.next_file_id += 1;
                self.files.insert(uri.clone(), id);
                id
            }
        };
        self.db.set_file_text(file_id, Arc::new(text));
    }

    fn close(&mut self, uri: &Url) {
        if let Some(file_id) = self.files.remove(uri) {
            // Free the text; the FileId is never reused.
            self.db.set_file_text(file_id, Arc::new(String::new()));
        }
    }

    fn diagnostics(&self, uri: &Url, version: Option<i32>) -> PublishDiagnosticsParams {
        let diagnostics = match self.files.get(uri) {
            Some(&file_id) => {
                let text = self.db.file_text(file_id);
                ide::diagnostics(&self.db, file_id)
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
                    .collect()
            }
            None => Vec::new(),
        };
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
    connection.initialize(serde_json::to_value(capabilities())?)?;

    let mut state = GlobalState::new();
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
