//! End-to-end tests for `q2 preview --static` (bd-sl79jjiq; plan
//! `claude-notes/plans/2026-09-22-q2-preview-static.md` Phase 3).
//!
//! Each test spawns the real binary on a copy of
//! `examples/websites/01-minimal` (or a lone `.qmd`), reads the boot
//! URL off stdout, and talks HTTP over a raw `TcpStream` — no HTTP
//! client dependency. The SSE stream is read as HTTP/1.0 so the server
//! streams raw `event:`/`data:` lines without chunked framing.
//!
//! Timing: the watcher debounces for 500 ms and a re-render of this
//! fixture takes well under a second, so the generous deadlines below
//! only matter on a starved CI box.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");
const EVENT_DEADLINE: Duration = Duration::from_secs(30);

fn fixture_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/websites/01-minimal")
        .canonicalize()
        .expect("examples/websites/01-minimal exists")
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// A copy of the minimal website in a fresh tempdir. Returns the
/// canonical project root.
fn minimal_site(temp: &TempDir) -> PathBuf {
    let dir = temp.path().canonicalize().unwrap();
    for name in ["_quarto.yml", "index.qmd", "about.qmd"] {
        std::fs::copy(fixture_source().join(name), dir.join(name)).unwrap();
    }
    dir
}

/// A running `q2 preview --static`. Killed on drop.
struct Server {
    child: Child,
    base: String,
    /// The full boot URL as printed (`http://127.0.0.1:PORT/<page>`).
    url: String,
    stdout: BufReader<std::process::ChildStdout>,
    stderr: Arc<Mutex<String>>,
}

impl Server {
    fn spawn(cwd: &Path, args: &[&str]) -> Server {
        let mut child = Command::new(Q2_BIN)
            .arg("preview")
            .arg("--static")
            .arg("--no-browser")
            .args(args)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn q2 preview --static");
        let stderr = Arc::new(Mutex::new(String::new()));
        {
            let sink = Arc::clone(&stderr);
            let pipe = child.stderr.take().expect("stderr piped");
            std::thread::spawn(move || {
                // Line by line, so assertions can read stderr while the
                // server is still running.
                let mut reader = BufReader::new(pipe);
                let mut line = String::new();
                while let Ok(n) = reader.read_line(&mut line) {
                    if n == 0 {
                        break;
                    }
                    sink.lock().unwrap().push_str(&line);
                    line.clear();
                }
            });
        }
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout piped"));
        let mut line = String::new();
        let boot_line = loop {
            line.clear();
            let n = stdout.read_line(&mut line).expect("read stdout");
            assert!(
                n > 0,
                "preview exited before printing its boot URL; stderr:\n{}",
                stderr.lock().unwrap()
            );
            if line.contains("→ http") {
                break line.trim().to_string();
            }
        };
        let url = boot_line
            .split("→ ")
            .nth(1)
            .expect("boot line has a URL")
            .trim()
            .to_string();
        // `http://127.0.0.1:PORT/...` → `http://127.0.0.1:PORT`
        let after_scheme = &url["http://".len()..];
        let host_port = after_scheme.split('/').next().unwrap();
        let base = format!("http://{host_port}");
        let server = Server {
            child,
            base,
            url,
            stdout,
            stderr,
        };
        server.wait_until_accepting();
        server
    }

    fn host_port(&self) -> &str {
        &self.base["http://".len()..]
    }

    fn wait_until_accepting(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while TcpStream::connect(self.host_port()).is_err() {
            assert!(
                Instant::now() < deadline,
                "server never accepted a connection; stderr:\n{}",
                self.stderr.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// One request, `Connection: close`, whole response read to EOF.
    fn get(&self, path: &str) -> Response {
        self.request("GET", path, &[])
    }

    fn request(&self, method: &str, path: &str, extra_headers: &[&str]) -> Response {
        let mut stream = TcpStream::connect(self.host_port()).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let mut req =
            format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
        for h in extra_headers {
            req.push_str(h);
            req.push_str("\r\n");
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        Response::parse(raw)
    }

    /// Subscribe to the reload channel. HTTP/1.0 keeps the body unframed.
    fn sse(&self) -> SseReader {
        let mut stream = TcpStream::connect(self.host_port()).expect("connect");
        stream.set_read_timeout(Some(EVENT_DEADLINE)).unwrap();
        stream
            .write_all(
                b"GET /__q2-preview/events HTTP/1.0\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n\r\n",
            )
            .unwrap();
        let mut reader = BufReader::new(stream);
        // Consume the status line and headers.
        let mut line = String::new();
        reader.read_line(&mut line).expect("status line");
        assert!(
            line.starts_with("HTTP/1.0 200") || line.starts_with("HTTP/1.1 200"),
            "SSE subscribe failed: {line}"
        );
        loop {
            line.clear();
            reader.read_line(&mut line).expect("header line");
            if line == "\r\n" || line == "\n" {
                break;
            }
        }
        SseReader { reader }
    }

    fn stderr(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn parse(raw: Vec<u8>) -> Response {
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("response has a header terminator");
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let body = raw[split + 4..].to_vec();
        let mut lines = head.lines();
        let status_line = lines.next().expect("status line");
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .expect("status code")
            .parse()
            .expect("numeric status");
        let headers = lines
            .filter_map(|l| {
                let (k, v) = l.split_once(':')?;
                Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
            })
            .collect();
        Response {
            status,
            headers,
            body,
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

struct SseReader {
    reader: BufReader<TcpStream>,
}

impl SseReader {
    /// Read events until one named `name` arrives; returns its `data`
    /// line. Panics after [`EVENT_DEADLINE`].
    fn wait_for(&mut self, name: &str) -> String {
        let deadline = Instant::now() + EVENT_DEADLINE;
        let mut current_event = String::new();
        let mut line = String::new();
        loop {
            assert!(
                Instant::now() < deadline,
                "no `{name}` event within {EVENT_DEADLINE:?}"
            );
            line.clear();
            let n = self
                .reader
                .read_line(&mut line)
                .unwrap_or_else(|e| panic!("reading SSE stream while waiting for `{name}`: {e}"));
            assert!(n > 0, "SSE stream closed while waiting for `{name}`");
            let l = line.trim_end_matches(['\r', '\n']);
            if let Some(ev) = l.strip_prefix("event: ") {
                current_event = ev.to_string();
            } else if let Some(data) = l.strip_prefix("data: ")
                && current_event == name
            {
                return data.to_string();
            }
        }
    }

    /// True when no `event:` line at all arrives within `window`.
    fn is_quiet_for(&mut self, window: Duration) -> bool {
        self.reader
            .get_ref()
            .set_read_timeout(Some(window))
            .unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => return true,
                Ok(_) => {
                    if line.starts_with("event: ") {
                        return false;
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return true;
                }
                Err(e) => panic!("SSE read error: {e}"),
            }
        }
    }
}

fn append(path: &Path, extra: &str) {
    let mut s = std::fs::read_to_string(path).unwrap();
    s.push_str(extra);
    std::fs::write(path, s).unwrap();
}

#[test]
fn boot_renders_the_site_and_serves_it_with_the_client() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);

    assert!(
        dir.join("_site/index.html").exists(),
        "boot rendered to disk; stderr:\n{}",
        server.stderr()
    );
    let index = server.get("/");
    assert_eq!(index.status, 200, "{}", index.text());
    assert_eq!(
        index.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(index.header("cache-control"), Some("no-store, max-age=0"));
    let html = index.text();
    assert!(
        html.contains("Minimal Website"),
        "site title present: {html}"
    );
    assert!(
        html.contains("EventSource(\"/__q2-preview/events\")"),
        "client injected: {html}"
    );
    let about = server.get("/about.html");
    assert_eq!(about.status, 200);
    let missing = server.get("/nope.html");
    assert_eq!(missing.status, 404);
}

#[test]
fn editing_an_input_rerenders_it_and_reloads_to_that_page() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    let mut sse = server.sse();

    append(&dir.join("about.qmd"), "\n\nEDIT-MARKER-ONE\n");
    let start = sse.wait_for("render-start");
    assert_eq!(start, r#"{"type":"render-start"}"#);
    let stop = sse.wait_for("render-stop");
    assert!(stop.contains("\"ok\":true"), "{stop}");
    let reload = sse.wait_for("reload");
    assert_eq!(reload, r#"{"type":"reload","target":"/about.html"}"#);

    let about = server.get("/about.html");
    assert!(about.text().contains("EDIT-MARKER-ONE"), "{}", about.text());
    let index = server.get("/index.html");
    assert!(
        !index.text().contains("EDIT-MARKER-ONE"),
        "only the edited page changed"
    );
}

#[test]
fn no_navigate_reloads_in_place() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let server = Server::spawn(&dir, &["--no-navigate", dir.to_str().unwrap()]);
    let mut sse = server.sse();
    append(&dir.join("about.qmd"), "\n\nEDIT-MARKER-TWO\n");
    let reload = sse.wait_for("reload");
    assert_eq!(reload, r#"{"type":"reload","target":null}"#);
}

#[test]
fn editing_the_project_config_rerenders_every_page() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    let mut sse = server.sse();

    let config = dir.join("_quarto.yml");
    let renamed = std::fs::read_to_string(&config)
        .unwrap()
        .replace("Minimal Website", "Renamed Site");
    assert_ne!(renamed, std::fs::read_to_string(&config).unwrap());
    std::fs::write(&config, renamed).unwrap();

    let reload = sse.wait_for("reload");
    assert_eq!(
        reload, r#"{"type":"reload","target":null}"#,
        "a full re-render reloads in place"
    );
    for page in ["/index.html", "/about.html"] {
        let html = server.get(page).text();
        assert!(html.contains("Renamed Site"), "{page} re-rendered: {html}");
        assert!(!html.contains("Minimal Website"), "{page} stale: {html}");
    }
}

#[test]
fn a_failing_rerender_reports_diagnostics_and_does_not_reload() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    let mut sse = server.sse();

    let about = dir.join("about.qmd");
    let good = std::fs::read_to_string(&about).unwrap();
    write(&about, "---\ntitle: [unclosed\n---\n\nBroken.\n");
    let stop = sse.wait_for("render-stop");
    assert!(stop.contains("\"ok\":false"), "{stop}");
    assert!(stop.contains("\"errors\":1"), "{stop}");
    assert!(
        stop.contains("Q-0-99"),
        "diagnostics text carries the code: {stop}"
    );
    assert!(
        stop.contains("about.qmd"),
        "diagnostics text names the page: {stop}"
    );
    assert!(
        !stop.contains("\\u001b"),
        "no ANSI escapes in the browser text: {stop}"
    );
    assert!(
        sse.is_quiet_for(Duration::from_secs(2)),
        "a failed render must not send a reload"
    );
    assert!(
        server.stderr().contains("Q-0-99"),
        "the terminal sees the diagnostics too; stderr:\n{}",
        server.stderr()
    );

    // Fixing the page brings the reload.
    let mut sse = server.sse();
    write(&about, &format!("{good}\n\nFIXED-MARKER\n"));
    let reload = sse.wait_for("reload");
    assert_eq!(reload, r#"{"type":"reload","target":"/about.html"}"#);
    assert!(server.get("/about.html").text().contains("FIXED-MARKER"));
}

#[test]
fn boot_with_a_failing_page_still_serves_the_rest() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    write(
        &dir.join("bad.qmd"),
        "---\ntitle: [unclosed\n---\n\nBroken.\n",
    );
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    assert_eq!(server.get("/index.html").status, 200);
    assert!(
        server.stderr().contains("Q-0-99"),
        "boot diagnostics printed; stderr:\n{}",
        server.stderr()
    );
}

#[test]
fn no_watch_never_rerenders() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let server = Server::spawn(&dir, &["--no-watch", dir.to_str().unwrap()]);
    let mut sse = server.sse();
    append(&dir.join("about.qmd"), "\n\nEDIT-MARKER-THREE\n");
    assert!(
        sse.is_quiet_for(Duration::from_secs(3)),
        "--no-watch must not react to edits"
    );
    assert!(
        !server
            .get("/about.html")
            .text()
            .contains("EDIT-MARKER-THREE")
    );
}

#[test]
fn a_page_inside_the_project_opens_on_that_page() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let about = dir.join("about.qmd");
    let server = Server::spawn(&dir, &[about.to_str().unwrap()]);
    // The boot URL names the page's output.
    assert!(
        server.url.ends_with("/about.html"),
        "boot URL opens on the requested page: {}",
        server.url
    );
    assert_eq!(server.get("/about.html").status, 200);
    let stderr = server.stderr();
    assert!(
        dir.join("_site/index.html").exists() && dir.join("_site/about.html").exists(),
        "whole project rendered, not just the page; stderr:\n{stderr}"
    );
}

#[test]
fn single_file_outside_a_project_redirects_root_to_the_document() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let doc = dir.join("doc.qmd");
    write(&doc, "---\ntitle: Lone Document\n---\n\nStandalone body.\n");
    let server = Server::spawn(&dir, &["doc.qmd"]);

    let root = server.get("/");
    assert_eq!(root.status, 302, "{}", root.text());
    assert_eq!(root.header("location"), Some("/doc.html"));
    let page = server.get("/doc.html");
    assert_eq!(page.status, 200);
    assert!(page.text().contains("Standalone body."));

    let mut sse = server.sse();
    append(&doc, "\nSINGLE-MARKER\n");
    let reload = sse.wait_for("reload");
    assert_eq!(reload, r#"{"type":"reload","target":"/doc.html"}"#);
    assert!(server.get("/doc.html").text().contains("SINGLE-MARKER"));
}

/// Ctrl-C (SIGINT) shuts the server down cleanly with a message.
#[cfg(unix)]
#[test]
fn sigint_exits_cleanly() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let mut server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    let status = Command::new("kill")
        .args(["-INT", &server.child.id().to_string()])
        .status()
        .expect("run kill -INT");
    assert!(status.success());
    let mut rest = String::new();
    server
        .stdout
        .read_to_string(&mut rest)
        .expect("drain stdout");
    let status = server.child.wait().expect("wait");
    assert!(status.success(), "clean exit on Ctrl-C, got {status:?}");
    assert!(
        rest.contains("shutting down"),
        "shutdown line printed; got:\n{rest}"
    );
}

// ── Phase 3b: lazy code execution ────────────────────────────────────

/// Same gate as the quarto-core engine tests: skip when no Jupyter
/// kernel can run.
fn jupyter_available() -> bool {
    quarto_core::engine::EngineRegistry::default()
        .get("jupyter")
        .is_some_and(|e| e.is_available())
}

const JUPYTER_PAGE: &str = "---\ntitle: Notebook page\nengine: jupyter\n---\n\nText before.\n\n```{python}\nprint(\"LAZY-EXEC-OUTPUT\")\n```\n";

/// A page with no code never costs a render when viewed.
#[test]
fn viewing_a_page_without_code_triggers_no_render() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    let mut sse = server.sse();
    assert_eq!(server.get("/about.html").status, 200);
    assert!(
        sse.is_quiet_for(Duration::from_secs(2)),
        "a markdown-only page view must not start a render"
    );
}

#[test]
fn lazy_execution_boots_inert_and_executes_on_first_view() {
    if !jupyter_available() {
        eprintln!("skipping: jupyter engine not available");
        return;
    }
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    write(&dir.join("nb.qmd"), JUPYTER_PAGE);
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);

    let mut sse = server.sse();
    let first = server.get("/nb.html");
    assert_eq!(first.status, 200);
    let html = first.text();
    // The inert page still shows the cell *source* (which contains the
    // marker inside `print(...)`); execution is visible as a cell
    // output block.
    assert!(
        !html.contains("cell-output"),
        "boot must not execute an unviewed page: {html}"
    );
    assert!(html.contains("print("), "cell source kept: {html}");
    assert!(html.contains("Text before."), "{html}");
    assert!(
        !server.stderr().contains("not available"),
        "an inert page is silent, not a missing-engine warning; stderr:\n{}",
        server.stderr()
    );

    // The view itself triggers the execution.
    let reload = sse.wait_for("reload");
    assert_eq!(reload, r#"{"type":"reload","target":"/nb.html"}"#);
    let html = server.get("/nb.html").text();
    assert!(html.contains("cell-output"), "executed after view: {html}");
    assert!(html.contains("LAZY-EXEC-OUTPUT"), "{html}");

    // A later full re-render keeps the viewed page executed.
    let config = dir.join("_quarto.yml");
    let renamed = std::fs::read_to_string(&config)
        .unwrap()
        .replace("Minimal Website", "Renamed Site");
    std::fs::write(&config, renamed).unwrap();
    let reload = sse.wait_for("reload");
    assert_eq!(reload, r#"{"type":"reload","target":null}"#);
    let html = server.get("/nb.html").text();
    assert!(html.contains("Renamed Site"), "{html}");
    assert!(
        html.contains("cell-output"),
        "viewed pages stay executed across a full re-render: {html}"
    );
}

#[test]
fn preview_engine_off_never_executes() {
    if !jupyter_available() {
        eprintln!("skipping: jupyter engine not available");
        return;
    }
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    write(&dir.join("nb.qmd"), JUPYTER_PAGE);
    append(&dir.join("_quarto.yml"), "\npreview:\n  engine: off\n");
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    let mut sse = server.sse();
    let html = server.get("/nb.html").text();
    assert!(!html.contains("cell-output"), "{html}");
    assert!(
        sse.is_quiet_for(Duration::from_secs(3)),
        "preview.engine: off must not execute on view"
    );
    assert!(!server.get("/nb.html").text().contains("cell-output"));
}

// ── Phase 4: `project: preview:` defaults from _quarto.yml ───────────

/// A free port, chosen the same way the OS would.
fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

#[test]
fn project_preview_keys_set_the_defaults_and_unsupported_keys_warn() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let port = free_port();
    // `preview:` sits under `project:`, the fixture's first block.
    let config = std::fs::read_to_string(dir.join("_quarto.yml")).unwrap();
    let config = config.replacen(
        "project:\n  type: website\n",
        &format!("project:\n  type: website\n  preview:\n    port: {port}\n    navigate: false\n    timeout: 300\n"),
        1,
    );
    std::fs::write(dir.join("_quarto.yml"), config).unwrap();

    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    assert!(
        server.base.ends_with(&format!(":{port}")),
        "project.preview.port is the default port: {}",
        server.base
    );
    assert!(
        server.stderr().contains("timeout") && server.stderr().contains("not supported"),
        "unsupported key warned about once; stderr:\n{}",
        server.stderr()
    );
    let mut sse = server.sse();
    append(&dir.join("about.qmd"), "\n\nEDIT-MARKER-FOUR\n");
    let reload = sse.wait_for("reload");
    assert_eq!(
        reload, r#"{"type":"reload","target":null}"#,
        "project.preview.navigate: false reloads in place"
    );
}

#[test]
fn cli_flags_override_project_preview_keys() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let port = free_port();
    let config = std::fs::read_to_string(dir.join("_quarto.yml"))
        .unwrap()
        .replacen(
            "project:\n  type: website\n",
            &format!(
                "project:\n  type: website\n  preview:\n    port: {port}\n    watch-inputs: false\n"
            ),
            1,
        );
    std::fs::write(dir.join("_quarto.yml"), config).unwrap();

    // `--port 0` asks for an OS-assigned port even though the config
    // names one.
    let server = Server::spawn(&dir, &["--port", "0", dir.to_str().unwrap()]);
    assert!(
        !server.base.ends_with(&format!(":{port}")),
        "--port 0 overrides project.preview.port: {}",
        server.base
    );
    // `watch-inputs: false` from the config is honoured (no CLI flag
    // turns watching back on).
    let mut sse = server.sse();
    append(&dir.join("about.qmd"), "\n\nEDIT-MARKER-FIVE\n");
    assert!(
        sse.is_quiet_for(Duration::from_secs(3)),
        "project.preview.watch-inputs: false disables the watcher"
    );
}
