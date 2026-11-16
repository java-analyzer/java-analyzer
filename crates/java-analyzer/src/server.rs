use std::{collections::HashMap, error::Error, sync::Arc};

use hir::{FileId, RootDatabase, SourceDatabase};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    },
    request::{HoverRequest, Request as _},
    Hover, HoverContents, HoverParams, HoverProviderCapability, MarkupContent, MarkupKind,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};

use crate::convert::position_to_offset;

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

    fn set_text(&mut self, uri: Url, text: String) {
        let file_id = *self.files.entry(uri).or_insert_with(|| {
            let id = FileId(self.next_file_id);
            self.next_file_id += 1;
            id
        });
        self.db.set_file_text(file_id, Arc::new(text));
    }

    fn close(&mut self, uri: &Url) {
        if let Some(file_id) = self.files.remove(uri) {
            // Free the text; the FileId is never reused.
            self.db.set_file_text(file_id, Arc::new(String::new()));
        }
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
        ..Default::default()
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
            Message::Notification(not) => handle_notification(&mut state, not)?,
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

fn handle_notification(state: &mut GlobalState, not: Notification) -> Result<(), BoxError> {
    match not.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let params: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
            state.set_text(params.text_document.uri, params.text_document.text);
        }
        DidChangeTextDocument::METHOD => {
            let params: lsp_types::DidChangeTextDocumentParams =
                serde_json::from_value(not.params)?;
            // We advertise FULL sync, so the last change holds the whole document.
            if let Some(change) = params.content_changes.into_iter().last() {
                state.set_text(params.text_document.uri, change.text);
            }
        }
        DidCloseTextDocument::METHOD => {
            let params: lsp_types::DidCloseTextDocumentParams = serde_json::from_value(not.params)?;
            state.close(&params.text_document.uri);
        }
        _ => {}
    }
    Ok(())
}
