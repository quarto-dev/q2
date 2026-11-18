//! `q2 lsp` hosting static previews in-process (bd-foum4ql3; plan
//! `claude-notes/plans/2026-11-17-q2-vscode-extension.md`, D-C/D-E).
//!
//! Drives the real binary over stdio with `Content-Length` framing. Every
//! byte the server writes to stdout must be a framed JSON-RPC message, even
//! while a render runs a pre-render script that prints to its (inherited)
//! stdout: `q2 lsp` points the process's stdout at stderr and keeps the
//! protocol on a private descriptor.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");
const DEADLINE: Duration = Duration::from_secs(60);

/// What the reader thread saw on stdout.
enum Frame {
    Message(Value),
    /// Bytes that are not a well-formed frame: the protocol is corrupt.
    Garbage(String),
}

struct Lsp {
    child: Child,
    stdin: ChildStdin,
    frames: mpsc::Receiver<Frame>,
    next_id: i64,
    /// Notifications seen while waiting for responses.
    notifications: Vec<Value>,
}

impl Lsp {
    fn start(cwd: &Path) -> Lsp {
        let mut child = Command::new(Q2_BIN)
            .arg("lsp")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn q2 lsp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, frames) = mpsc::channel();
        std::thread::spawn(move || read_frames(BufReader::new(stdout), tx));
        Lsp {
            child,
            stdin,
            frames,
            next_id: 0,
            notifications: Vec::new(),
        }
    }

    fn send(&mut self, message: Value) {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Send a request; return its `result` or `error`.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let deadline = Instant::now() + DEADLINE;
        loop {
            let message = self.next_message(deadline);
            if message.get("id") == Some(&json!(id)) && message.get("method").is_none() {
                return match message.get("error") {
                    Some(e) => Err(e.clone()),
                    None => Ok(message["result"].clone()),
                };
            }
            if message.get("method").is_some() && message.get("id").is_none() {
                self.notifications.push(message);
            }
        }
    }

    fn next_message(&mut self, deadline: Instant) -> Value {
        let left = deadline.saturating_duration_since(Instant::now());
        match self.frames.recv_timeout(left) {
            Ok(Frame::Message(m)) => m,
            Ok(Frame::Garbage(g)) => panic!("non-JSON-RPC bytes on q2 lsp's stdout: {g:?}"),
            Err(e) => panic!("no message from q2 lsp: {e}"),
        }
    }

    /// Wait for a `quarto/previewStatus` notification matching `pred`.
    fn wait_status(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        if let Some(i) = self.notifications.iter().position(|n| is_status(n, &pred)) {
            return self.notifications.remove(i);
        }
        let deadline = Instant::now() + DEADLINE;
        loop {
            let message = self.next_message(deadline);
            if is_status(&message, &pred) {
                return message;
            }
        }
    }

    fn shutdown(mut self) {
        self.request("shutdown", Value::Null).unwrap();
        self.notify("exit", Value::Null);
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "q2 lsp exited with {status}");
                break;
            }
            assert!(Instant::now() < deadline, "q2 lsp did not exit");
            std::thread::sleep(Duration::from_millis(50));
        }
        // Every byte after the last message must still be framed.
        while let Ok(frame) = self.frames.try_recv() {
            if let Frame::Garbage(g) = frame {
                panic!("non-JSON-RPC bytes on q2 lsp's stdout: {g:?}");
            }
        }
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn is_status(message: &Value, pred: &impl Fn(&Value) -> bool) -> bool {
    message["method"] == "quarto/previewStatus" && pred(&message["params"])
}

fn read_frames(mut out: BufReader<std::process::ChildStdout>, tx: mpsc::Sender<Frame>) {
    loop {
        let mut length = None;
        loop {
            let mut line = String::new();
            match out.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            if line == "\r\n" {
                break;
            }
            match line.strip_prefix("Content-Length: ") {
                Some(n) => length = n.trim().parse::<usize>().ok(),
                None if line.starts_with("Content-Type:") => {}
                None => {
                    let _ = tx.send(Frame::Garbage(line));
                    return;
                }
            }
        }
        let Some(length) = length else {
            let _ = tx.send(Frame::Garbage("a header without Content-Length".into()));
            return;
        };
        let mut body = vec![0; length];
        if out.read_exact(&mut body).is_err() {
            return;
        }
        let frame = match serde_json::from_slice(&body) {
            Ok(v) => Frame::Message(v),
            Err(_) => Frame::Garbage(String::from_utf8_lossy(&body).into_owned()),
        };
        if tx.send(frame).is_err() {
            return;
        }
    }
}

fn http_get(url: &str) -> String {
    let rest = url.strip_prefix("http://").expect("an http URL");
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let mut stream = TcpStream::connect(authority).unwrap();
    write!(stream, "GET {path} HTTP/1.0\r\nHost: {authority}\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn uri(path: &Path) -> String {
    // A minimal file URI; the paths here have no characters to escape.
    let s = path.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

/// A pre-render script that writes to its stdout, which a render lets it
/// inherit. Returns the script's name in the project root.
#[cfg(unix)]
fn noisy_script(root: &Path) -> &'static str {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join("noisy.sh");
    write(&path, "#!/bin/sh\necho 'pre-render chatter on stdout'\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    "noisy.sh"
}

#[cfg(not(unix))]
fn noisy_script(root: &Path) -> &'static str {
    write(
        &root.join("noisy.cmd"),
        "@echo pre-render chatter on stdout\r\n",
    );
    "noisy.cmd"
}

fn project(temp: &tempfile::TempDir) -> PathBuf {
    let root = temp.path().canonicalize().unwrap();
    let script = noisy_script(&root);
    write(
        &root.join("_quarto.yml"),
        &format!("project:\n  type: website\n  pre-render: {script}\n"),
    );
    write(&root.join("index.qmd"), "# Home page\n");
    write(&root.join("about.qmd"), "# About page\n");
    root
}

#[test]
fn preview_start_serves_the_project_and_reuses_the_session() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = project(&temp);
    let mut lsp = Lsp::start(&root);
    let init = lsp
        .request(
            "initialize",
            json!({ "capabilities": {}, "initializationOptions": { "sweep": false } }),
        )
        .unwrap();
    assert_eq!(
        init["capabilities"]["experimental"]["quarto"]["preview"], 1,
        "{init}"
    );
    lsp.notify("initialized", json!({}));

    let started = lsp
        .request(
            "quarto/previewStart",
            json!({ "textDocument": { "uri": uri(&root.join("index.qmd")) } }),
        )
        .expect("the preview starts");
    let preview_id = started["previewId"].as_str().unwrap().to_string();
    let url = started["url"].as_str().unwrap().to_string();
    assert!(http_get(&url).contains("Home page"), "{url}");

    // Another page of the same project: same session, its own page.
    let about = lsp
        .request(
            "quarto/previewStart",
            json!({ "textDocument": { "uri": uri(&root.join("about.qmd")) } }),
        )
        .unwrap();
    assert_eq!(about["previewId"], preview_id.as_str());
    let about_url = about["url"].as_str().unwrap();
    assert!(about_url.ends_with("/about.html"), "{about_url}");
    assert!(http_get(about_url).contains("About page"));

    // An edit on disk re-renders and is reported.
    write(&root.join("about.qmd"), "# About again\n");
    lsp.wait_status(|p| p["state"] == "rendering");
    let rendered = lsp.wait_status(|p| p["state"] == "rendered");
    assert_eq!(rendered["params"]["ok"], true, "{rendered}");
    assert!(http_get(about_url).contains("About again"));

    lsp.request("quarto/previewStop", json!({ "previewId": preview_id }))
        .unwrap();
    lsp.wait_status(|p| p["state"] == "stopped");
    // The server closes shortly after the stop.
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(url.trim_start_matches("http://").split('/').next().unwrap()).is_ok() {
        assert!(
            Instant::now() < deadline,
            "the preview server is still listening"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    lsp.shutdown();
}

#[test]
fn an_unservable_format_fails_the_start_request() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    write(&root.join("doc.qmd"), "---\nformat: epub\n---\n\n# Book\n");
    let mut lsp = Lsp::start(&root);
    lsp.request("initialize", json!({ "capabilities": {} }))
        .unwrap();
    lsp.notify("initialized", json!({}));
    let err = lsp
        .request(
            "quarto/previewStart",
            json!({ "textDocument": { "uri": uri(&root.join("doc.qmd")) } }),
        )
        .expect_err("epub cannot be previewed");
    assert!(err["message"].as_str().unwrap().contains("epub"), "{err}");
    lsp.shutdown();
}

#[test]
fn exit_stops_a_running_preview() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = project(&temp);
    let mut lsp = Lsp::start(&root);
    lsp.request("initialize", json!({ "capabilities": {} }))
        .unwrap();
    lsp.notify("initialized", json!({}));
    let started = lsp
        .request(
            "quarto/previewStart",
            json!({ "textDocument": { "uri": uri(&root.join("index.qmd")) } }),
        )
        .unwrap();
    let authority = started["url"]
        .as_str()
        .unwrap()
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap()
        .to_string();
    lsp.shutdown();
    assert!(
        TcpStream::connect(&authority).is_err(),
        "the preview outlived q2 lsp"
    );
}

#[test]
fn two_projects_preview_at_once() {
    let temp = tempfile::TempDir::new().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let roots: Vec<PathBuf> = ["one", "two"]
        .iter()
        .map(|name| {
            let root = base.join(name);
            write(&root.join("_quarto.yml"), "project:\n  type: default\n");
            write(&root.join("index.qmd"), &format!("# Project {name}\n"));
            root
        })
        .collect();
    let mut lsp = Lsp::start(&base);
    lsp.request("initialize", json!({ "capabilities": {} }))
        .unwrap();
    lsp.notify("initialized", json!({}));
    let started: Vec<Value> = roots
        .iter()
        .map(|root| {
            lsp.request(
                "quarto/previewStart",
                json!({ "textDocument": { "uri": uri(&root.join("index.qmd")) } }),
            )
            .unwrap()
        })
        .collect();
    assert_ne!(started[0]["previewId"], started[1]["previewId"]);
    assert!(http_get(started[0]["url"].as_str().unwrap()).contains("Project one"));
    assert!(http_get(started[1]["url"].as_str().unwrap()).contains("Project two"));
    lsp.shutdown();
}
