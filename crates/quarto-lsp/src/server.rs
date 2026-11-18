//! The native LSP host: JSON-RPC over stdio (via `lsp-server`) in front of a
//! [`quarto_lsp_project::Workspace`] (epic bd-3f6ijnw9, C3).
//!
//! The host owns no analysis state. It decodes LSP messages into workspace
//! [`Input`]s, answers requests through the workspace's query API, and
//! carries out its [`Output`]s: publishing diagnostics, watching session
//! roots, and running background [`Job`]s on rayon.
//!
//! **One loop, in order.** `lsp-server` delivers messages on a channel in
//! the order the client sent them. The loop blocks for the next event (a
//! message, a finished job, a file change), then drains everything else
//! that is already queued, handling each in order, and only then calls
//! [`Workspace::flush`]. A burst of keystrokes is therefore analysed once,
//! and a request always sees every edit sent before it.
//!
//! [`Job`]: quarto_lsp_project::Job

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender, select};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use quarto_lsp_project::{
    ClientEvent, ClientId, DocUri, FileChange, Input, JobOutput, Output, Workspace, WorkspaceConfig,
};
use quarto_system_runtime::NativeRuntime;
use serde_json::Value;

use crate::capabilities::server_capabilities;
use crate::convert;
use crate::preview::{
    HostServices, PREVIEW_PROTOCOL_VERSION, PREVIEW_START, PREVIEW_STOP, PreviewEvent,
    PreviewRegistry, PreviewRequest,
};

/// The one client a stdio server talks to.
const CLIENT: ClientId = 0;

/// `initializationOptions` the server understands.
#[derive(Debug, Default)]
struct InitOptions {
    /// Active project profiles: a list, or a comma/space separated string.
    profiles: Option<Vec<String>>,
    /// Profile a project's documents in the background on open (R5).
    sweep: Option<bool>,
}

impl InitOptions {
    fn from_json(value: Option<&Value>) -> Self {
        let Some(Value::Object(map)) = value else {
            return Self::default();
        };
        let profiles = match map.get("profile") {
            Some(Value::String(s)) => Some(vec![s.clone()]),
            Some(Value::Array(items)) => Some(
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
            ),
            _ => None,
        };
        let sweep = map.get("sweep").and_then(Value::as_bool);
        Self { profiles, sweep }
    }
}

/// Run the LSP server over stdio until the client exits.
pub fn run_server() -> Result<()> {
    run_server_with(HostServices::default())
}

/// [`run_server`] with host-provided services (previews; bd-foum4ql3).
///
/// Stdout is reserved for JSON-RPC: the process's own stdout is pointed at
/// stderr first (see [`crate::stdio`]), so renders and their child
/// processes cannot corrupt the protocol.
pub fn run_server_with(services: HostServices) -> Result<()> {
    let (connection, io_threads) = crate::stdio::protocol_stdio()?;
    serve_with(connection, services)?;
    // The reader thread stops on `exit` even when the client keeps stdin
    // open, so this join does not hang (the bug q2 hub-lsp found in
    // tower-lsp; see its plan).
    io_threads.join()?;
    Ok(())
}

/// Run the server over an established connection (stdio, or an in-memory
/// pair in tests) until `exit`.
pub fn serve(connection: Connection) -> Result<()> {
    serve_with(connection, HostServices::default())
}

/// [`serve`] with host-provided services.
pub fn serve_with(connection: Connection, services: HostServices) -> Result<()> {
    let (id, params) = connection.initialize_start()?;
    let options = InitOptions::from_json(params.get("initializationOptions"));
    let client_watches = params
        .pointer("/capabilities/workspace/didChangeWatchedFiles/dynamicRegistration")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut capabilities = serde_json::to_value(server_capabilities())?;
    if services.previews.is_some() {
        capabilities["experimental"]["quarto"]["preview"] = PREVIEW_PROTOCOL_VERSION.into();
    }
    let result = serde_json::json!({
        "capabilities": capabilities,
        "serverInfo": {
            "name": "quarto-lsp",
            // The effective Quarto version (nightly override aware,
            // bd-p4ljdp2e), so an editor shows the same string as
            // `q2 --version`.
            "version": quarto_util::cli_version().to_string(),
        },
    });
    connection.initialize_finish(id, result)?;

    let mut config = WorkspaceConfig::new(Arc::new(NativeRuntime::new()));
    config.profiles = options.profiles;
    if let Some(sweep) = options.sweep {
        config.sweep = sweep;
    }
    let mut host = Host::new(connection, Workspace::new(config), client_watches);
    host.previews = services.previews.map(PreviewRegistry::new);
    let result = host.run();
    if let Some(previews) = host.previews.as_mut() {
        previews.stop_all();
    }
    result
}

enum Event {
    Lsp(Message),
    Job(JobOutput),
    Files(Vec<FileChange>),
    Preview(String, PreviewEvent),
}

struct Host {
    connection: Connection,
    workspace: Workspace,
    client_watches: bool,
    jobs_tx: Sender<JobOutput>,
    jobs_rx: Receiver<JobOutput>,
    files_tx: Sender<Vec<FileChange>>,
    files_rx: Receiver<Vec<FileChange>>,
    /// Kept alive for as long as their roots are watched.
    watchers: Vec<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>>,
    next_request: i32,
    /// Preview sessions, when the host provides a launcher.
    previews: Option<PreviewRegistry>,
    preview_tx: Sender<(String, PreviewEvent)>,
    preview_rx: Receiver<(String, PreviewEvent)>,
}

impl Host {
    fn new(connection: Connection, workspace: Workspace, client_watches: bool) -> Self {
        let (jobs_tx, jobs_rx) = crossbeam_channel::unbounded();
        let (files_tx, files_rx) = crossbeam_channel::unbounded();
        let (preview_tx, preview_rx) = crossbeam_channel::unbounded();
        Self {
            connection,
            workspace,
            client_watches,
            jobs_tx,
            jobs_rx,
            files_tx,
            files_rx,
            watchers: Vec::new(),
            next_request: 0,
            previews: None,
            preview_tx,
            preview_rx,
        }
    }

    fn run(&mut self) -> Result<()> {
        loop {
            let first = select! {
                recv(self.connection.receiver) -> m => match m {
                    Ok(m) => Event::Lsp(m),
                    Err(_) => return Ok(()), // client went away
                },
                recv(self.jobs_rx) -> j => Event::Job(j.expect("the host holds a sender")),
                recv(self.files_rx) -> f => Event::Files(f.expect("the host holds a sender")),
                recv(self.preview_rx) -> p => {
                    let (id, event) = p.expect("the host holds a sender");
                    Event::Preview(id, event)
                }
            };
            if self.handle(first)? {
                return Ok(());
            }
            // Drain what is already queued before analysing (debounce).
            loop {
                let next = if let Ok(m) = self.connection.receiver.try_recv() {
                    Event::Lsp(m)
                } else if let Ok(j) = self.jobs_rx.try_recv() {
                    Event::Job(j)
                } else if let Ok(f) = self.files_rx.try_recv() {
                    Event::Files(f)
                } else if let Ok((id, event)) = self.preview_rx.try_recv() {
                    Event::Preview(id, event)
                } else {
                    break;
                };
                if self.handle(next)? {
                    return Ok(());
                }
            }
            let outputs = self.workspace.flush();
            self.apply(outputs);
        }
    }

    /// Handle one event. `Ok(true)` means the server should stop.
    fn handle(&mut self, event: Event) -> Result<bool> {
        match event {
            Event::Lsp(Message::Request(req)) => {
                if self.connection.handle_shutdown(&req)? {
                    return Ok(true);
                }
                self.request(req);
            }
            Event::Lsp(Message::Notification(n)) => {
                if n.method == lsp_types::notification::Exit::METHOD {
                    return Ok(true);
                }
                self.notification(n);
            }
            // Responses to our own requests (client/registerCapability).
            Event::Lsp(Message::Response(_)) => {}
            Event::Job(output) => {
                let outputs = self.workspace.handle(Input::JobDone(output));
                self.apply(outputs);
            }
            Event::Preview(id, event) => {
                if let Some(previews) = self.previews.as_mut() {
                    for message in previews.event(&id, event) {
                        self.send(message);
                    }
                }
            }
            Event::Files(changes) => {
                let outputs = self.workspace.handle(Input::Files(changes));
                self.apply(outputs);
            }
        }
        Ok(false)
    }

    fn request(&mut self, req: Request) {
        if req.method == PREVIEW_START || req.method == PREVIEW_STOP {
            self.preview_request(req);
            return;
        }
        let response = match req.method.as_str() {
            lsp_types::request::DocumentSymbolRequest::METHOD => {
                match text_document_uri(&req.params) {
                    Some(uri) => {
                        let symbols = self
                            .workspace
                            .document_symbols(CLIENT, &uri)
                            .unwrap_or_default();
                        let symbols: Vec<lsp_types::DocumentSymbol> =
                            symbols.iter().map(convert::symbol_to_lsp).collect();
                        Response::new_ok(req.id, symbols)
                    }
                    None => invalid_params(req.id, "missing textDocument.uri"),
                }
            }
            lsp_types::request::SemanticTokensFullRequest::METHOD => {
                match text_document_uri(&req.params) {
                    Some(uri) => self.semantic_tokens(req.id, &uri),
                    None => invalid_params(req.id, "missing textDocument.uri"),
                }
            }
            DOCUMENT_PROFILE => self.document_profile(req.id, &req.params),
            DOCUMENT_METADATA => self.document_metadata(req.id, &req.params),
            _ => Response::new_err(
                req.id,
                ErrorCode::MethodNotFound as i32,
                format!("unknown request: {}", req.method),
            ),
        };
        self.send(response.into());
    }

    /// `quarto/previewStart` / `quarto/previewStop` (bd-foum4ql3). A
    /// start is answered once its session is ready, from the event arm.
    fn preview_request(&mut self, req: Request) {
        let Some(previews) = self.previews.as_mut() else {
            self.send(
                Response::new_err(
                    req.id,
                    ErrorCode::MethodNotFound as i32,
                    format!("{} is not available in this server", req.method),
                )
                .into(),
            );
            return;
        };
        let messages = if req.method == PREVIEW_START {
            let path = text_document_uri(&req.params).and_then(|uri| uri.to_file_path());
            match path {
                Some(path) => {
                    let format = req
                        .params
                        .get("format")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let tx = self.preview_tx.clone();
                    previews.start(req.id, PreviewRequest { path, format }, move |id| {
                        Box::new(move |event| {
                            let _ = tx.send((id.clone(), event));
                        })
                    })
                }
                None => vec![invalid_params(req.id, "missing or non-file textDocument.uri").into()],
            }
        } else {
            match req.params.get("previewId").and_then(Value::as_str) {
                Some(preview_id) => previews.stop(req.id, preview_id),
                None => vec![invalid_params(req.id, "missing previewId").into()],
            }
        };
        for message in messages {
            self.send(message);
        }
    }

    /// `textDocument/semanticTokens/full` (bd-xnpyb2vv P1): highlight
    /// tokens for an open buffer. Only the text is needed, so this never
    /// waits for an analysis. A document that is not open (or is not a
    /// document, like `_quarto.yml`) gets `null`.
    fn semantic_tokens(&mut self, id: RequestId, uri: &DocUri) -> Response {
        match self.workspace.open_document_text(CLIENT, uri) {
            Some((text, _version)) => {
                let text = String::from_utf8_lossy(&text);
                Response::new_ok(
                    id,
                    crate::semantic_tokens::tokens_for_text(uri.as_str(), &text),
                )
            }
            None => Response::new_ok(id, Value::Null),
        }
    }

    /// `quarto/documentProfile` (C4): the wire profile of a document.
    fn document_profile(&mut self, id: RequestId, params: &Value) -> Response {
        let Some(uri) = text_document_uri(params) else {
            return invalid_params(id, "missing textDocument.uri");
        };
        let format = params.get("format").and_then(Value::as_str);
        match self.workspace.document_profile(CLIENT, &uri, format) {
            Ok(answer) => {
                let mut profile = serde_json::to_value(&answer.profile).unwrap_or(Value::Null);
                if let (Some(fields), Value::Object(map)) =
                    (params.get("fields").and_then(Value::as_array), &mut profile)
                {
                    let keep: Vec<&str> = fields.iter().filter_map(Value::as_str).collect();
                    map.retain(|k, _| keep.contains(&k.as_str()));
                }
                let diagnostics: Vec<lsp_types::Diagnostic> = answer
                    .diagnostics
                    .iter()
                    .map(convert::diagnostic_to_lsp)
                    .collect();
                Response::new_ok(
                    id,
                    serde_json::json!({
                        "uri": uri.as_str(),
                        "version": answer.version,
                        "format": answer.format,
                        "schemaVersion": quarto_lsp_project::wire::WIRE_SCHEMA_VERSION,
                        "profile": profile,
                        "diagnostics": diagnostics,
                    }),
                )
            }
            Err(e) => query_error(id, e),
        }
    }

    /// `quarto/documentMetadata` (C4, D7): the authored merged metadata, as
    /// `q2 get-config` reports it.
    fn document_metadata(&mut self, id: RequestId, params: &Value) -> Response {
        let Some(uri) = text_document_uri(params) else {
            return invalid_params(id, "missing textDocument.uri");
        };
        let format = params.get("format").and_then(Value::as_str);
        let path = params.get("path").and_then(Value::as_str).unwrap_or("");
        let output = match params.get("output").and_then(Value::as_str) {
            None | Some("value") => pampa::config_json::ProseMode::Value,
            Some("pandoc") => pampa::config_json::ProseMode::Pandoc,
            Some(other) => {
                return invalid_params(
                    id,
                    &format!("output must be \"value\" or \"pandoc\", not {other:?}"),
                );
            }
        };
        match self
            .workspace
            .document_metadata(CLIENT, &uri, path, output, format)
        {
            Ok(answer) => {
                let mut result = serde_json::json!({
                    "uri": uri.as_str(),
                    "version": answer.version,
                    "format": answer.format,
                    "found": answer.found,
                    "value": answer.value,
                });
                if !answer.diagnostics.is_empty() {
                    let diagnostics: Vec<lsp_types::Diagnostic> = answer
                        .diagnostics
                        .iter()
                        .map(convert::diagnostic_to_lsp)
                        .collect();
                    result["diagnostics"] = serde_json::to_value(diagnostics).unwrap_or_default();
                }
                Response::new_ok(id, result)
            }
            Err(e) => query_error(id, e),
        }
    }

    fn notification(&mut self, n: Notification) {
        use lsp_types::notification as note;
        let event = match n.method.as_str() {
            note::DidOpenTextDocument::METHOD => (|| {
                let doc = n.params.get("textDocument")?;
                Some(ClientEvent::Opened {
                    uri: DocUri::new(doc.get("uri")?.as_str()?),
                    text: doc.get("text")?.as_str()?.to_string(),
                    version: doc.get("version")?.as_i64()? as i32,
                })
            })(),
            note::DidChangeTextDocument::METHOD => (|| {
                let doc = n.params.get("textDocument")?;
                // Full sync: the last change carries the whole text.
                let text = n.params.get("contentChanges")?.as_array()?.last()?;
                Some(ClientEvent::Changed {
                    uri: DocUri::new(doc.get("uri")?.as_str()?),
                    text: text.get("text")?.as_str()?.to_string(),
                    version: doc.get("version")?.as_i64()? as i32,
                })
            })(),
            note::DidCloseTextDocument::METHOD => {
                text_document_uri(&n.params).map(|uri| ClientEvent::Closed { uri })
            }
            note::DidChangeWatchedFiles::METHOD => {
                let changes = watched_file_changes(&n.params);
                let outputs = self.workspace.handle(Input::Files(changes));
                self.apply(outputs);
                None
            }
            _ => None,
        };
        if let Some(event) = event {
            let outputs = self.workspace.handle(Input::Client(CLIENT, event));
            self.apply(outputs);
        }
    }

    fn apply(&mut self, outputs: Vec<Output>) {
        for output in outputs {
            match output {
                Output::Diagnostics {
                    uri,
                    version,
                    diagnostics,
                    ..
                } => {
                    let diagnostics: Vec<lsp_types::Diagnostic> =
                        diagnostics.iter().map(convert::diagnostic_to_lsp).collect();
                    let params = serde_json::json!({
                        "uri": uri.as_str(),
                        "version": version,
                        "diagnostics": diagnostics,
                    });
                    self.send(
                        Notification::new(
                            lsp_types::notification::PublishDiagnostics::METHOD.to_string(),
                            params,
                        )
                        .into(),
                    );
                }
                Output::Watch { root, .. } => self.watch(root),
                Output::Spawn(job) => {
                    let tx = self.jobs_tx.clone();
                    rayon::spawn(move || {
                        let _ = tx.send(job.run());
                    });
                }
            }
        }
    }

    /// Report file changes under `root`: through the client when it can
    /// watch for us, otherwise with `notify`.
    fn watch(&mut self, root: PathBuf) {
        if self.client_watches {
            let Some(base_uri) = DocUri::from_file_path(&root) else {
                return;
            };
            self.next_request += 1;
            let params = serde_json::json!({
                "registrations": [{
                    "id": format!("quarto-watch-{}", self.next_request),
                    "method": lsp_types::notification::DidChangeWatchedFiles::METHOD,
                    "registerOptions": {
                        "watchers": [{
                            "globPattern": { "baseUri": base_uri.as_str(), "pattern": "**/*" },
                        }],
                    },
                }],
            });
            let request = Request::new(
                RequestId::from(format!("quarto-register-{}", self.next_request)),
                lsp_types::request::RegisterCapability::METHOD.to_string(),
                params,
            );
            self.send(request.into());
            return;
        }
        let tx = self.files_tx.clone();
        let debouncer = notify_debouncer_mini::new_debouncer(
            Duration::from_millis(200),
            move |result: notify_debouncer_mini::DebounceEventResult| {
                if let Ok(events) = result {
                    let changes = events
                        .into_iter()
                        .map(|e| {
                            if e.path.exists() {
                                FileChange::Changed(e.path)
                            } else {
                                FileChange::Removed(e.path)
                            }
                        })
                        .collect();
                    let _ = tx.send(changes);
                }
            },
        );
        match debouncer {
            Ok(mut debouncer) => {
                if let Err(e) = debouncer
                    .watcher()
                    .watch(&root, notify::RecursiveMode::Recursive)
                {
                    tracing::warn!(root = %root.display(), error = %e, "cannot watch project");
                    return;
                }
                self.watchers.push(debouncer);
            }
            Err(e) => tracing::warn!(error = %e, "cannot create a file watcher"),
        }
    }

    fn send(&self, message: Message) {
        let _ = self.connection.sender.send(message);
    }
}

fn text_document_uri(params: &Value) -> Option<DocUri> {
    params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .map(DocUri::new)
}

fn watched_file_changes(params: &Value) -> Vec<FileChange> {
    let Some(changes) = params.get("changes").and_then(Value::as_array) else {
        return Vec::new();
    };
    changes
        .iter()
        .filter_map(|c| {
            let path = DocUri::new(c.get("uri")?.as_str()?).to_file_path()?;
            // FileChangeType: 1 created, 2 changed, 3 deleted.
            Some(match c.get("type")?.as_u64()? {
                3 => FileChange::Removed(path),
                _ => FileChange::Changed(path),
            })
        })
        .collect()
}

/// The `quarto/*` protocol extensions (C4).
const DOCUMENT_PROFILE: &str = "quarto/documentProfile";
const DOCUMENT_METADATA: &str = "quarto/documentMetadata";

/// A query that could not be answered: `InvalidParams`, with
/// `data.kind` naming why (`unknownDocument`, `unsupportedInput`).
fn query_error(id: RequestId, error: quarto_lsp_project::QueryError) -> Response {
    use quarto_lsp_project::QueryError;
    let (kind, message) = match error {
        QueryError::UnknownDocument => ("unknownDocument", "no such document"),
        QueryError::UnsupportedInput => (
            "unsupportedInput",
            "not a document this server analyses (only .qmd and .md)",
        ),
    };
    Response {
        id,
        response_result: Err(lsp_server::ResponseError {
            code: ErrorCode::InvalidParams as i32,
            message: message.to_string(),
            data: Some(serde_json::json!({ "kind": kind })),
        }),
    }
}

fn invalid_params(id: RequestId, message: &str) -> Response {
    Response::new_err(id, ErrorCode::InvalidParams as i32, message.to_string())
}
