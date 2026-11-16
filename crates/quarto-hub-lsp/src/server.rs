//! The LSP surface over a [`HubWorkspace`].
//!
//! The server owns the document text, which it reads from the hub, so it
//! offers no text document synchronization. Its client names projects as
//! workspace folders and asks about them with custom requests:
//!
//! - `quarto/projectComments` (request): the comments of every qmd file in
//!   one project.
//! - `quarto/documentsChanged` (server-to-client notification): which
//!   documents of a project changed, so the client knows when to ask again.
//!
//! Projects and documents are named by Automerge URLs, `automerge:` followed
//! by the document id.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedReceiver;
use tower_lsp::jsonrpc::{Error, ErrorCode, Result};
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::{
    DidChangeWorkspaceFoldersParams, InitializeParams, InitializeResult, InitializedParams,
    MessageType, OneOf, ServerCapabilities, ServerInfo, WorkspaceFoldersServerCapabilities,
    WorkspaceServerCapabilities,
};
use tower_lsp::{Client, ClientSocket, LanguageServer, LspService};

use crate::comments::{JsonComment, ParseFailure};
use crate::workspace::{ChangeHint, HubWorkspace, WorkspaceError};

/// The scheme prefix of an Automerge URL.
pub const AUTOMERGE_URL_PREFIX: &str = "automerge:";

/// How long change hints accumulate before one notification per project
/// goes out, so a burst of edits (typing) is one message, not dozens.
const HINT_BATCH_WINDOW: Duration = Duration::from_millis(200);

/// Params of `quarto/projectComments`.
#[derive(Debug, Deserialize)]
pub struct ProjectCommentsParams {
    /// The project's index document, as an Automerge URL.
    pub project: String,
}

/// Result of `quarto/projectComments`.
#[derive(Debug, Serialize)]
pub struct ProjectCommentsResult {
    pub project: String,
    /// One entry per qmd file, sorted by path.
    pub files: Vec<FileCommentsResult>,
}

#[derive(Debug, Serialize)]
pub struct FileCommentsResult {
    pub path: String,
    /// The file's document, as an Automerge URL.
    pub document: String,
    pub comments: Vec<JsonComment>,
    /// Present when the file's comments could not be read, for example
    /// while it does not parse. The client should keep its previous view of
    /// the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ParseFailure>,
}

/// `quarto/documentsChanged`, sent by the server.
pub enum DocumentsChanged {}

impl Notification for DocumentsChanged {
    type Params = DocumentsChangedParams;
    const METHOD: &'static str = "quarto/documentsChanged";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentsChangedParams {
    /// The project's index document, as an Automerge URL.
    pub project: String,
    /// The changed documents, as Automerge URLs, sorted.
    pub documents: Vec<String>,
}

pub struct HubLanguageServer {
    client: Client,
    workspace: Arc<HubWorkspace>,
    /// Folders `initialize` could not follow, logged once the client is
    /// ready for messages.
    startup_warnings: Mutex<Vec<String>>,
}

/// Build the service and its client socket. Must run inside a tokio
/// runtime: it spawns the task that turns change hints into
/// `quarto/documentsChanged` notifications.
pub fn build_service(
    workspace: HubWorkspace,
    hints: UnboundedReceiver<ChangeHint>,
) -> (LspService<HubLanguageServer>, ClientSocket) {
    let workspace = Arc::new(workspace);
    LspService::build(move |client| {
        tokio::spawn(forward_hints(client.clone(), workspace.clone(), hints));
        HubLanguageServer {
            client,
            workspace,
            startup_warnings: Mutex::new(Vec::new()),
        }
    })
    .custom_method(
        "quarto/projectComments",
        HubLanguageServer::project_comments,
    )
    .finish()
}

/// Serve LSP over this process's stdin and stdout until the client exits.
/// Nothing else may write to stdout while it runs.
pub async fn serve_stdio(workspace: HubWorkspace, hints: UnboundedReceiver<ChangeHint>) {
    serve(workspace, hints, tokio::io::stdin(), tokio::io::stdout()).await;
}

/// Serve LSP over `input` and `output` until the client exits: when `input`
/// ends, or as soon as an `exit` notification has been handled.
///
/// `tower_lsp::Server::serve` on its own returns only when `input` ends,
/// since it notices that the service has exited only when the next message
/// arrives. A client may send `exit` and keep its end of the pipe open, and
/// the server must exit regardless.
pub async fn serve<I, O>(
    workspace: HubWorkspace,
    hints: UnboundedReceiver<ChangeHint>,
    input: I,
    output: O,
) where
    I: tokio::io::AsyncRead + Unpin,
    O: tokio::io::AsyncWrite,
{
    let (service, socket) = build_service(workspace, hints);
    let (exited, on_exit) = tokio::sync::oneshot::channel();
    let service = ExitWatch {
        inner: service,
        exited: Some(exited),
    };
    tokio::select! {
        () = tower_lsp::Server::new(input, output, socket).serve(service) => {}
        _ = on_exit => {}
    }
}

/// Forwards every message to `inner`, and signals `exited` once an `exit`
/// notification has been handled.
struct ExitWatch<S> {
    inner: S,
    exited: Option<tokio::sync::oneshot::Sender<()>>,
}

impl<S> tower::Service<tower_lsp::jsonrpc::Request> for ExitWatch<S>
where
    S: tower::Service<tower_lsp::jsonrpc::Request>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = futures::future::BoxFuture<'static, std::result::Result<S::Response, S::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), S::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: tower_lsp::jsonrpc::Request) -> Self::Future {
        let exited = if request.method() == "exit" {
            self.exited.take()
        } else {
            None
        };
        let handled = self.inner.call(request);
        Box::pin(async move {
            let response = handled.await;
            if let Some(exited) = exited {
                let _ = exited.send(());
            }
            response
        })
    }
}

impl HubLanguageServer {
    async fn project_comments(
        &self,
        params: ProjectCommentsParams,
    ) -> Result<ProjectCommentsResult> {
        let project = document_id(&params.project).ok_or_else(|| {
            Error::invalid_params(format!("'{}' is not an Automerge URL", params.project))
        })?;
        let comments = self
            .workspace
            .project_comments(project)
            .await
            .map_err(workspace_error)?;
        Ok(ProjectCommentsResult {
            project: automerge_url(&comments.project),
            files: comments
                .files
                .into_iter()
                .map(|file| FileCommentsResult {
                    path: file.path,
                    document: automerge_url(&file.document),
                    comments: file.comments,
                    error: file.error,
                })
                .collect(),
        })
    }

    /// Follow the project a workspace folder names, describing a failure.
    async fn follow_folder(&self, uri: &str) -> std::result::Result<(), String> {
        let project = document_id(uri).ok_or_else(|| format!("'{uri}' is not an Automerge URL"))?;
        self.workspace
            .follow(project)
            .await
            .map_err(|e| format!("could not follow {uri}: {e}"))
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for HubLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        for folder in params.workspace_folders.unwrap_or_default() {
            if let Err(warning) = self.follow_folder(folder.uri.as_str()).await {
                self.startup_warnings.lock().unwrap().push(warning);
            }
        }
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "quarto-hub-lsp".to_string(),
                version: Some(quarto_util::cli_version().to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        let warnings = std::mem::take(&mut *self.startup_warnings.lock().unwrap());
        for warning in warnings {
            self.client.log_message(MessageType::WARNING, warning).await;
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        for folder in params.event.removed {
            if let Some(project) = document_id(folder.uri.as_str()) {
                self.workspace.unfollow(project);
            }
        }
        for folder in params.event.added {
            if let Err(warning) = self.follow_folder(folder.uri.as_str()).await {
                self.client.log_message(MessageType::WARNING, warning).await;
            }
        }
    }
}

/// Batch change hints and send one `quarto/documentsChanged` per project
/// per batch. Hints for projects no longer followed are dropped, and so are
/// hints before the client has finished initializing: it asks for what it
/// needs once it starts.
async fn forward_hints(
    client: Client,
    workspace: Arc<HubWorkspace>,
    mut hints: UnboundedReceiver<ChangeHint>,
) {
    while let Some(first) = hints.recv().await {
        let mut batch: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        batch
            .entry(first.project)
            .or_default()
            .insert(first.document);
        let deadline = tokio::time::Instant::now() + HINT_BATCH_WINDOW;
        while let Ok(Some(hint)) = tokio::time::timeout_at(deadline, hints.recv()).await {
            batch.entry(hint.project).or_default().insert(hint.document);
        }
        for (project, documents) in batch {
            if !workspace.is_following(&project) {
                continue;
            }
            client
                .send_notification::<DocumentsChanged>(DocumentsChangedParams {
                    project: automerge_url(&project),
                    documents: documents.iter().map(|d| automerge_url(d)).collect(),
                })
                .await;
        }
    }
}

/// The document id an Automerge URL names.
fn document_id(url: &str) -> Option<&str> {
    url.strip_prefix(AUTOMERGE_URL_PREFIX)
        .filter(|id| !id.is_empty())
}

fn automerge_url(document_id: &str) -> String {
    format!("{AUTOMERGE_URL_PREFIX}{document_id}")
}

fn workspace_error(error: WorkspaceError) -> Error {
    match error {
        WorkspaceError::UnknownProject(_) | WorkspaceError::IndexNotFound(_) => {
            Error::invalid_params(error.to_string())
        }
        WorkspaceError::Connect(_) | WorkspaceError::Hub(_) => Error {
            code: ErrorCode::InternalError,
            message: error.to_string().into(),
            data: None,
        },
    }
}
