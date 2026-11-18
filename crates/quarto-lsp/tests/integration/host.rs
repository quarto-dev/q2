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
        Self::start_with(init_options).0
    }

    /// Start, also returning the `initialize` result.
    fn start_with(init_options: Value) -> (Self, Value) {
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
        (client, init)
    }

    fn notify(&self, method: &str, params: Value) {
        self.conn
            .sender
            .send(Notification::new(method.to_string(), params).into())
            .unwrap();
    }

    /// Send a request and return its result, skipping other messages.
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.request_raw(method, params)
            .expect("a successful response")
    }

    /// Send a request and return its result or error.
    fn request_raw(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, lsp_server::ResponseError> {
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
                    return r.response_result;
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

// ── C4: quarto/documentMetadata and quarto/documentProfile ──────────────

fn layered_project() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write(
        &root,
        "_quarto.yml",
        "project:\n  type: default\ndescription: from project\nformat:\n  html:\n    toc: true\n",
    );
    write(
        &root,
        "sub/_metadata.yml",
        "description: from dir\nkeywords: [dir]\n",
    );
    write(
        &root,
        "sub/_child.qmd",
        "Child text [>> a note from the child ]\n",
    );
    write(
        &root,
        "sub/doc.qmd",
        "---\ntitle: On *disk*\n---\n\n# First\n\n{{< include _child.qmd >}}\n",
    );
    (tmp, root)
}

#[test]
fn the_server_advertises_its_protocol_extensions() {
    let (client, init) = Client::start_with(json!({ "sweep": false }));
    assert_eq!(
        init["capabilities"]["experimental"]["quarto"]["documentProfile"],
        1
    );
    assert_eq!(
        init["capabilities"]["experimental"]["quarto"]["documentMetadata"],
        1
    );
    client.shutdown();
}

#[test]
fn document_metadata_is_the_authored_merge_of_the_buffer() {
    let (_tmp, root) = layered_project();
    let doc = uri(&root.join("sub/doc.qmd"));
    let mut client = Client::start(json!({ "sweep": false }));
    client.open(
        &doc,
        "---\ntitle: In the *buffer*\nauthor: Ada\n---\n\n# First\n",
    );

    let all = client.request(
        "quarto/documentMetadata",
        json!({ "textDocument": { "uri": doc } }),
    );
    assert_eq!(all["version"], 1);
    assert_eq!(all["format"], "html");
    assert_eq!(all["found"], true);
    let value = &all["value"];
    assert_eq!(
        value["title"], "In the *buffer*",
        "markdown, from the unsaved buffer"
    );
    assert_eq!(
        value["description"], "from dir",
        "directory layer beats project"
    );
    assert_eq!(value["keywords"], json!(["dir"]));
    assert_eq!(value["toc"], true, "format.html flattened");
    assert!(value.get("quarto").is_none(), "no derived keys");

    let one = client.request(
        "quarto/documentMetadata",
        json!({ "textDocument": { "uri": doc }, "path": "author" }),
    );
    assert_eq!(one["value"], "Ada");
    let missing = client.request(
        "quarto/documentMetadata",
        json!({ "textDocument": { "uri": doc }, "path": "nope.deeper" }),
    );
    assert_eq!(
        (missing["found"].clone(), missing["value"].clone()),
        (json!(false), Value::Null)
    );
    let pandoc = client.request(
        "quarto/documentMetadata",
        json!({ "textDocument": { "uri": doc }, "path": "title", "output": "pandoc" }),
    );
    assert!(
        !pandoc["value"].is_string(),
        "a Pandoc fragment: {}",
        pandoc["value"]
    );
    client.shutdown();
}

#[test]
fn document_profile_resolves_spans_to_locations_including_included_files() {
    let (_tmp, root) = layered_project();
    let doc = uri(&root.join("sub/doc.qmd"));
    let child = uri(&root.join("sub/_child.qmd"));
    let mut client = Client::start(json!({ "sweep": false }));

    // Not opened: read from the project's files, so no buffer version.
    let answer = client.request(
        "quarto/documentProfile",
        json!({ "textDocument": { "uri": doc } }),
    );
    assert_eq!(answer["version"], Value::Null);
    assert_eq!(answer["schemaVersion"], 1);
    let profile = &answer["profile"];
    assert_eq!(profile["sourcePath"], "sub/doc.qmd");
    assert_eq!(profile["outputHref"], "sub/doc.html");
    assert_eq!(
        profile["title"],
        json!({ "text": "On disk", "markdown": "On *disk*" })
    );
    let first = &profile["outline"][0];
    assert_eq!(first["title"]["text"], "First");
    assert_eq!(first["location"]["uri"], doc.as_str());
    assert_eq!(
        first["location"]["range"]["start"]["line"], 4,
        "the heading's line"
    );
    let comment = &profile["comments"][0];
    assert_eq!(comment["text"], "a note from the child");
    assert_eq!(
        comment["location"]["uri"],
        child.as_str(),
        "points into the include"
    );
    assert_eq!(profile["includes"][0]["path"], "sub/_child.qmd");
    assert_eq!(profile["includes"][0]["uri"], child.as_str());
    assert!(
        profile.get("listingItem").is_none(),
        "listings-only surfaces stay off the wire"
    );

    let filtered = client.request(
        "quarto/documentProfile",
        json!({ "textDocument": { "uri": doc }, "fields": ["title"] }),
    );
    let keys: Vec<&String> = filtered["profile"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["title"]);
    client.shutdown();
}

#[test]
fn a_document_that_fails_pass_1_has_no_profile_and_says_why() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let doc = uri(&root.join("doc.qmd"));
    let mut client = Client::start(json!({ "sweep": false }));
    client.open(&doc, "Text [>> half a comment\n");
    let answer = client.request(
        "quarto/documentProfile",
        json!({ "textDocument": { "uri": doc } }),
    );
    assert_eq!(answer["profile"], Value::Null);
    assert!(!answer["diagnostics"].as_array().unwrap().is_empty());
    client.shutdown();
}

#[test]
fn queries_say_why_they_cannot_answer() {
    let (_tmp, root) = layered_project();
    let mut client = Client::start(json!({ "sweep": false }));
    let config = client
        .request_raw(
            "quarto/documentProfile",
            json!({ "textDocument": { "uri": uri(&root.join("_quarto.yml")) } }),
        )
        .unwrap_err();
    assert_eq!(config.data.unwrap()["kind"], "unsupportedInput");
    let untitled = client
        .request_raw(
            "quarto/documentMetadata",
            json!({ "textDocument": { "uri": "untitled:Untitled-9" } }),
        )
        .unwrap_err();
    assert_eq!(untitled.data.unwrap()["kind"], "unknownDocument");
    client.shutdown();
}

/// Decode `semanticTokens/full` data into absolute `(line, start, length,
/// type name)` tuples using the advertised legend.
fn decode_tokens(init: &Value, result: &Value) -> Vec<(u32, u32, u32, String)> {
    let legend: Vec<String> =
        init["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"]
            .as_array()
            .expect("a legend")
            .iter()
            .map(|t| t.as_str().unwrap().to_string())
            .collect();
    let data: Vec<u32> = result["data"]
        .as_array()
        .expect("token data")
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect();
    let (mut line, mut start) = (0, 0);
    data.chunks(5)
        .map(|c| {
            if c[0] > 0 {
                line += c[0];
                start = c[1];
            } else {
                start += c[1];
            }
            (line, start, c[2], legend[c[3] as usize].clone())
        })
        .collect()
}

#[test]
fn semantic_tokens_colour_the_open_buffer_including_unsaved_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write(&root, "doc.qmd", "plain\n");
    let doc = uri(&root.join("doc.qmd"));

    let (mut client, init) = Client::start_with(json!({ "sweep": false }));
    client.open(&doc, "# Title\n\n```{python}\nimport os\n```\n");
    let params = json!({ "textDocument": { "uri": doc } });
    let tokens = decode_tokens(
        &init,
        &client.request("textDocument/semanticTokens/full", params.clone()),
    );
    assert!(
        tokens.iter().any(|t| t.0 == 0 && t.3 == "qmdHeading"),
        "{tokens:?}"
    );
    assert!(tokens.contains(&(3, 0, 6, "keyword".into())), "{tokens:?}");

    // The unsaved buffer, not the disk, is what gets coloured.
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": doc, "version": 2 },
            "contentChanges": [{ "text": "plain *emph*\n" }],
        }),
    );
    let tokens = decode_tokens(
        &init,
        &client.request("textDocument/semanticTokens/full", params),
    );
    assert!(tokens.iter().all(|t| t.3 != "qmdHeading"), "{tokens:?}");
    assert!(
        tokens.iter().any(|t| t.0 == 0 && t.3 == "qmdEmphasis"),
        "{tokens:?}"
    );
    client.shutdown();
}

#[test]
fn semantic_tokens_for_a_document_that_is_not_open_are_null() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write(&root, "doc.qmd", "# Title\n");
    let mut client = Client::start(json!({ "sweep": false }));
    let result = client.request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": uri(&root.join("doc.qmd")) } }),
    );
    assert!(result.is_null(), "{result}");
    client.shutdown();
}
