//! The LSP host driven in process over `lsp_server::Connection::memory()`
//! (epic bd-3f6ijnw9, C3): project-aware analysis through the real JSON-RPC
//! adapter, and a disk change reaching an open document via the watcher.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{Value, json};

struct Client {
    conn: Connection,
    server: Option<thread::JoinHandle<anyhow::Result<()>>>,
    next_id: i32,
}

impl Client {
    fn start(init_options: Value) -> Self {
        let (server_side, client_side) = Connection::memory();
        let server = thread::spawn(move || quarto_lsp::server::serve(server_side));
        let mut client = Self {
            conn: client_side,
            server: Some(server),
            next_id: 0,
        };
        let init = client.request(
            "initialize",
            json!({ "capabilities": {}, "initializationOptions": init_options }),
        );
        assert!(init.get("capabilities").is_some(), "{init}");
        client.notify("initialized", json!({}));
        client
    }

    fn notify(&self, method: &str, params: Value) {
        self.conn
            .sender
            .send(Notification::new(method.to_string(), params).into())
            .unwrap();
    }

    /// Send a request and return its result, skipping other messages.
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        self.conn
            .sender
            .send(Request::new(id.clone(), method.to_string(), params).into())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.conn.receiver.recv_timeout(left).expect("a response") {
                Message::Response(r) if r.id == id => {
                    return r.response_result.expect("a successful response");
                }
                _ => continue,
            }
        }
    }

    fn open(&self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "quarto", "version": 1, "text": text } }),
        );
    }

    fn symbol_names(&mut self, uri: &str) -> Vec<String> {
        fn walk(symbols: &Value, out: &mut Vec<String>) {
            for s in symbols.as_array().into_iter().flatten() {
                out.push(s["name"].as_str().unwrap_or_default().to_string());
                walk(&s["children"], out);
            }
        }
        let result = self.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
        );
        let mut out = Vec::new();
        walk(&result, &mut out);
        out
    }

    fn shutdown(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        let server = self.server.take().unwrap();
        server.join().unwrap().unwrap();
    }
}

fn uri(path: &Path) -> String {
    quarto_lsp_project::DocUri::from_file_path(path)
        .unwrap()
        .as_str()
        .to_string()
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_outline_sees_includes_and_unsaved_include_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write(&root, "_quarto.yml", "project:\n  type: default\n");
    write(&root, "_child.qmd", "## From disk\n");
    let doc_text = "# Top\n\n{{< include _child.qmd >}}\n";
    write(&root, "doc.qmd", doc_text);
    let doc = uri(&root.join("doc.qmd"));
    let child = uri(&root.join("_child.qmd"));

    let mut client = Client::start(json!({ "sweep": false }));
    client.open(&doc, doc_text);
    assert_eq!(client.symbol_names(&doc), ["Top", "From disk"]);

    client.open(&child, "## From disk\n");
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": child, "version": 2 },
            "contentChanges": [{ "text": "## Unsaved edit\n" }],
        }),
    );
    assert_eq!(client.symbol_names(&doc), ["Top", "Unsaved edit"]);
    client.shutdown();
}

#[test]
fn a_change_on_disk_reaches_an_open_document_through_the_watcher() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write(&root, "_quarto.yml", "project:\n  type: default\n");
    write(&root, "_child.qmd", "## Before\n");
    let doc_text = "# Top\n\n{{< include _child.qmd >}}\n";
    write(&root, "doc.qmd", doc_text);
    let doc = uri(&root.join("doc.qmd"));

    // The client does not offer to watch, so the host runs `notify`.
    let mut client = Client::start(json!({ "sweep": false }));
    client.open(&doc, doc_text);
    assert_eq!(client.symbol_names(&doc), ["Top", "Before"]);

    write(&root, "_child.qmd", "## After\n");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let names = client.symbol_names(&doc);
        if names == ["Top", "After"] {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the watcher never delivered the change: {names:?}"
        );
        thread::sleep(Duration::from_millis(100));
    }
    client.shutdown();
}

#[test]
fn exit_without_shutdown_stops_the_server() {
    let client = Client::start(json!({}));
    client.notify("exit", Value::Null);
    let mut client = client;
    let server = client.server.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !server.is_finished() {
        assert!(Instant::now() < deadline, "server did not stop on exit");
        thread::sleep(Duration::from_millis(20));
    }
    server.join().unwrap().unwrap();
}
