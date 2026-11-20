//! The LSP surface, driven in process: the `tower-lsp` service on one side,
//! an in-process hub on the other.

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use quarto_hub_lsp::server::build_service;
use quarto_hub_lsp::workspace::HubWorkspace;
use quarto_hub_provider::HeaderDialer;
use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use tower_lsp::jsonrpc::{Request, Response};
use tower_lsp::{ClientSocket, LspService};

use crate::support::{create_project, set_text, spawn_hub};

const WAIT: Duration = Duration::from_secs(10);

struct TestClient {
    service: LspService<quarto_hub_lsp::server::HubLanguageServer>,
    socket: ClientSocket,
    next_id: i64,
}

impl TestClient {
    async fn connect(ws_url: url::Url) -> Self {
        let dialer = HeaderDialer::new(ws_url, Vec::new()).unwrap();
        let (workspace, hints) = HubWorkspace::connect(Arc::new(dialer), WAIT)
            .await
            .expect("connect to the hub");
        let (service, socket) = build_service(workspace, hints);
        Self {
            service,
            socket,
            next_id: 1,
        }
    }

    async fn request(&mut self, method: &'static str, params: Value) -> Result<Value, Value> {
        let id = self.next_id;
        self.next_id += 1;
        let request = Request::build(method).params(params).id(id).finish();
        let response: Response = self
            .service
            .ready()
            .await
            .unwrap()
            .call(request)
            .await
            .unwrap()
            .expect("a request gets a response");
        let (_, body) = response.into_parts();
        body.map_err(|e| serde_json::to_value(e).unwrap())
    }

    async fn notify(&mut self, method: &'static str, params: Value) {
        let notification = Request::build(method).params(params).finish();
        let response = self
            .service
            .ready()
            .await
            .unwrap()
            .call(notification)
            .await
            .unwrap();
        assert!(response.is_none(), "a notification gets no response");
    }

    /// `initialize` with one workspace folder per project id, then
    /// `initialized`.
    async fn start(&mut self, projects: &[&str]) -> Value {
        let folders: Vec<Value> = projects
            .iter()
            .map(|id| json!({"uri": format!("automerge:{id}"), "name": id}))
            .collect();
        let result = self
            .request(
                "initialize",
                json!({"capabilities": {}, "workspaceFolders": folders}),
            )
            .await
            .expect("initialize succeeds");
        self.notify("initialized", json!({})).await;
        result
    }

    async fn project_comments(&mut self, project: &str) -> Result<Value, Value> {
        self.request(
            "quarto/projectComments",
            json!({"project": format!("automerge:{project}")}),
        )
        .await
    }

    /// The params of the next server-to-client message named `method`,
    /// skipping any others (log messages).
    async fn next_notification(&mut self, method: &str) -> Value {
        tokio::time::timeout(WAIT, async {
            loop {
                let message = self.socket.next().await.expect("the socket stays open");
                if message.method() == method {
                    return message.params().cloned().unwrap_or(Value::Null);
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {method}"))
    }
}

fn comment_texts(result: &Value) -> Vec<(String, Vec<String>)> {
    result["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().to_string(),
                f["comments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| c["text"].as_str().unwrap().to_string())
                    .collect(),
            )
        })
        .collect()
}

#[tokio::test]
async fn initialize_advertises_workspace_folders_and_no_text_sync() {
    let (_hub, ws_url) = spawn_hub().await;
    let mut client = TestClient::connect(ws_url).await;

    let result = client.start(&[]).await;

    let folders = &result["capabilities"]["workspace"]["workspaceFolders"];
    assert_eq!(folders["supported"], json!(true));
    assert_eq!(folders["changeNotifications"], json!(true));
    assert_eq!(result["capabilities"].get("textDocumentSync"), None);
    assert_eq!(result["serverInfo"]["name"], json!("quarto-hub-lsp"));
}

#[tokio::test]
async fn project_comments_answers_for_a_folder_given_at_initialize() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, ids) = create_project(&hub, &[("index.qmd", "Intro.\n\n[>> test! ]\n")]).await;
    let mut client = TestClient::connect(ws_url).await;
    client.start(&[&index.document_id()]).await;

    let result = client
        .project_comments(&index.document_id())
        .await
        .expect("projectComments succeeds");

    assert_eq!(
        result["project"],
        json!(format!("automerge:{}", index.document_id()))
    );
    let file = &result["files"][0];
    assert_eq!(file["path"], json!("index.qmd"));
    assert_eq!(file["document"], json!(format!("automerge:{}", ids[0])));
    assert_eq!(file.get("error"), None);
    let comment = &file["comments"][0];
    assert_eq!(comment["text"], json!("test!"));
    assert_eq!(comment["start_line"], json!(3));
    assert_eq!(comment["file"], json!("index.qmd"));
}

#[tokio::test]
async fn a_file_that_does_not_parse_carries_its_error() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, _) = create_project(&hub, &[("a.qmd", "Prose [>> half typed\n")]).await;
    let mut client = TestClient::connect(ws_url).await;
    client.start(&[&index.document_id()]).await;

    let result = client.project_comments(&index.document_id()).await.unwrap();

    let file = &result["files"][0];
    assert_eq!(file["comments"], json!([]));
    let message = file["error"]["messages"][0].as_str().unwrap();
    assert!(message.starts_with("Q-2-21: "), "{message}");
}

#[tokio::test]
async fn an_added_workspace_folder_is_followed_and_a_removed_one_is_dropped() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, _) = create_project(&hub, &[("a.qmd", "[>> later ]\n")]).await;
    let project = index.document_id();
    let mut client = TestClient::connect(ws_url).await;
    client.start(&[]).await;

    let folder = json!({"uri": format!("automerge:{project}"), "name": "p"});
    client
        .notify(
            "workspace/didChangeWorkspaceFolders",
            json!({"event": {"added": [folder.clone()], "removed": []}}),
        )
        .await;
    let result = client.project_comments(&project).await.unwrap();
    assert_eq!(
        comment_texts(&result),
        vec![("a.qmd".to_string(), vec!["later".to_string()])]
    );

    client
        .notify(
            "workspace/didChangeWorkspaceFolders",
            json!({"event": {"added": [], "removed": [folder]}}),
        )
        .await;
    let error = client.project_comments(&project).await.unwrap_err();
    assert_eq!(error["code"], json!(-32602), "{error}");
}

#[tokio::test]
async fn project_comments_sent_right_after_adding_its_folder_waits_for_the_follow() {
    // A client adds a folder and asks about it at once, without waiting:
    // the notification has no response to wait for. tower-lsp handles the
    // two concurrently, so the request must wait for the follow in flight
    // instead of answering that the project is unknown.
    let (hub, ws_url) = spawn_hub().await;
    let (index, _) = create_project(&hub, &[("a.qmd", "[>> asked at once ]\n")]).await;
    let project = format!("automerge:{}", index.document_id());
    let mut client = TestClient::connect(ws_url).await;
    client.start(&[]).await;

    let added = Request::build("workspace/didChangeWorkspaceFolders")
        .params(json!({"event": {"added": [{"uri": project, "name": "p"}], "removed": []}}))
        .finish();
    let asked = Request::build("quarto/projectComments")
        .params(json!({"project": project}))
        .id(100)
        .finish();
    let added = client.service.ready().await.unwrap().call(added);
    let asked = client.service.ready().await.unwrap().call(asked);
    let (added, asked) = tokio::join!(added, asked);

    assert!(added.unwrap().is_none());
    let (_, body) = asked.unwrap().expect("a response").into_parts();
    let result = body.expect("projectComments succeeds once the follow completes");
    assert_eq!(
        comment_texts(&result),
        vec![("a.qmd".to_string(), vec!["asked at once".to_string()])]
    );
}

#[tokio::test]
async fn a_hub_side_edit_produces_a_documents_changed_notification() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, ids) = create_project(&hub, &[("a.qmd", "Text.\n")]).await;
    let mut client = TestClient::connect(ws_url).await;
    client.start(&[&index.document_id()]).await;

    set_text(&hub, &ids[0], "Text [>> new ].\n").await;

    let file_url = format!("automerge:{}", ids[0]);
    let params = loop {
        let params = client.next_notification("quarto/documentsChanged").await;
        let documents = params["documents"].as_array().unwrap();
        if documents.contains(&json!(file_url)) {
            break params;
        }
    };
    assert_eq!(
        params["project"],
        json!(format!("automerge:{}", index.document_id()))
    );

    // The hint means the change has reached this peer, so the next query
    // sees it.
    let result = client.project_comments(&index.document_id()).await.unwrap();
    assert_eq!(
        comment_texts(&result),
        vec![("a.qmd".to_string(), vec!["new".to_string()])]
    );
}

#[tokio::test]
async fn serve_returns_after_exit_while_the_input_stays_open() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (_hub, ws_url) = spawn_hub().await;
    let dialer = HeaderDialer::new(ws_url, Vec::new()).unwrap();
    let (workspace, hints) = HubWorkspace::connect(Arc::new(dialer), WAIT).await.unwrap();
    let (mut client_in, server_in) = tokio::io::duplex(64 * 1024);
    let (server_out, mut client_out) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(quarto_hub_lsp::server::serve(
        workspace, hints, server_in, server_out,
    ));
    // Drain the server's output so its writes never block.
    tokio::spawn(async move {
        let mut sink = Vec::new();
        let _ = client_out.read_to_end(&mut sink).await;
    });

    for message in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}}),
        json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown"}),
        json!({"jsonrpc": "2.0", "method": "exit"}),
    ] {
        let body = message.to_string();
        let frame = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        client_in.write_all(frame.as_bytes()).await.unwrap();
    }

    // `client_in` is still open, as a client's end of a pipe may be.
    tokio::time::timeout(WAIT, serving)
        .await
        .expect("serve must return once `exit` is handled")
        .unwrap();
    drop(client_in);
}

#[tokio::test]
async fn project_comments_rejects_an_unknown_project_and_a_non_automerge_uri() {
    let (hub, ws_url) = spawn_hub().await;
    let (index, _) = create_project(&hub, &[("a.qmd", "x\n")]).await;
    let mut client = TestClient::connect(ws_url).await;
    client.start(&[]).await;

    let unknown = client
        .project_comments(&index.document_id())
        .await
        .unwrap_err();
    assert_eq!(unknown["code"], json!(-32602), "{unknown}");

    let not_automerge = client
        .request(
            "quarto/projectComments",
            json!({"project": "file:///tmp/project"}),
        )
        .await
        .unwrap_err();
    assert_eq!(not_automerge["code"], json!(-32602), "{not_automerge}");
}
