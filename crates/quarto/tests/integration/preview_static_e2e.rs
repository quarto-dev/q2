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
        // `-v` so the driver's own log lines (which render ran, why)
        // land in the captured stderr for a failure's diagnosis. The
        // driver itself logs at debug, so a dropped watcher event
        // ("event without a content change; ignored") is visible too;
        // `RUST_LOG` takes precedence over `-v`.
        let mut command = Command::new(Q2_BIN);
        command
            .env(
                "RUST_LOG",
                "quarto=info,q2=info,q2::commands::preview_static=debug",
            )
            .arg("-v")
            .arg("preview")
            .arg("--static")
            .arg("--no-browser")
            .args(args)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Own process group, so `send_interrupt` can target this child
        // alone with a console Ctrl-Break.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = command.spawn().expect("spawn q2 preview --static");
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
        SseReader {
            reader,
            server_stderr: Arc::clone(&self.stderr),
        }
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
    /// The server's captured stderr, for failure messages: a missing
    /// event is diagnosed by the driver's log, not by the stream.
    server_stderr: Arc<Mutex<String>>,
}

impl SseReader {
    fn server_stderr(&self) -> String {
        self.server_stderr.lock().unwrap().clone()
    }

    /// Read events until one named `name` arrives; returns its `data`
    /// line. Panics after [`EVENT_DEADLINE`].
    fn wait_for(&mut self, name: &str) -> String {
        let deadline = Instant::now() + EVENT_DEADLINE;
        let mut current_event = String::new();
        let mut line = String::new();
        loop {
            assert!(
                Instant::now() < deadline,
                "no `{name}` event within {EVENT_DEADLINE:?}; stderr:\n{}",
                self.server_stderr()
            );
            line.clear();
            let n = match self.reader.read_line(&mut line) {
                Ok(n) => n,
                Err(e) => panic!(
                    "reading SSE stream while waiting for `{name}`: {e}; stderr:\n{}",
                    self.server_stderr()
                ),
            };
            assert!(
                n > 0,
                "SSE stream closed while waiting for `{name}`; stderr:\n{}",
                self.server_stderr()
            );
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

    /// The names of every event that arrives within `window`. Bounded
    /// by wall clock, not by silence: a server that keeps emitting
    /// (PR #712's Linux re-render loop) must end the window, not
    /// extend it.
    fn events_within(&mut self, window: Duration) -> Vec<String> {
        let deadline = Instant::now() + window;
        let mut names = Vec::new();
        let mut line = String::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return names;
            }
            self.reader
                .get_ref()
                .set_read_timeout(Some(remaining))
                .unwrap();
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => return names,
                Ok(_) => {
                    if let Some(name) = line.trim_end().strip_prefix("event: ") {
                        names.push(name.to_string());
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return names;
                }
                Err(e) => panic!("SSE read error: {e}"),
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
    // Q-1-1 "YAML Syntax Error" (was the generic Q-0-99 until bd-x30aq7ae).
    assert!(
        stop.contains("Q-1-1"),
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
    // Only the *reload* must be absent. A second render cycle for the
    // same save can legitimately follow (Linux inotify may spread one
    // save over two debounce windows); it fails the same way and is
    // equally not a reload.
    let later = sse.events_within(Duration::from_secs(3));
    assert!(
        !later.iter().any(|e| e == "reload"),
        "a failed render must not send a reload; saw {later:?}; stderr:\n{}",
        server.stderr()
    );
    assert!(
        later.is_empty(),
        "one save is one render: no further cycles may follow (a render's own \
         reads must not re-trigger it); saw {later:?}; stderr:\n{}",
        server.stderr()
    );
    assert!(
        server.stderr().contains("Q-1-1"),
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
        server.stderr().contains("Q-1-1"),
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

/// Interrupts the child the way a terminal user would: SIGINT on unix.
#[cfg(unix)]
fn send_interrupt(child: &Child) {
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("run kill -INT");
    assert!(status.success());
}

/// Interrupts the child with a console Ctrl-Break. Ctrl-C can't be
/// sent to a single process group on Windows, only to the whole
/// console (which would include the test runner).
#[cfg(windows)]
fn send_interrupt(child: &Child) {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
    // SAFETY: plain FFI call; the child was spawned with
    // CREATE_NEW_PROCESS_GROUP, so its pid is its process-group id.
    let ok = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) };
    assert!(
        ok != 0,
        "GenerateConsoleCtrlEvent failed: {}",
        std::io::Error::last_os_error()
    );
}

/// Console Ctrl events only reach processes sharing the sender's
/// console, so a test runner started without one (headless CI)
/// allocates one before spawning the child. A no-op when a console
/// is already attached.
#[cfg(windows)]
fn ensure_console() {
    // SAFETY: plain FFI call; failure means a console already exists.
    unsafe { windows_sys::Win32::System::Console::AllocConsole() };
}

/// An interrupt shuts the server down cleanly with a message.
#[test]
fn interrupt_exits_cleanly() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    #[cfg(windows)]
    ensure_console();
    let mut server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    send_interrupt(&server.child);
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

// ── F5b: the resolved format, the render flags, the failure page ─────

/// A two-chapter book (`one.qmd`, `two.qmd` after `index.qmd`) whose
/// project config is `_quarto.yml` with `extra` appended after the book.
fn book(temp: &TempDir, extra: &str) -> PathBuf {
    let dir = temp.path().canonicalize().unwrap();
    write(
        &dir.join("_quarto.yml"),
        &format!(
            "project:\n  type: book\nbook:\n  title: Test Book\n  author: Me\n  chapters:\n    \
             - index.qmd\n    - one.qmd\n    - two.qmd\n{extra}"
        ),
    );
    write(&dir.join("index.qmd"), "# Preface\n\nHello.\n");
    write(&dir.join("one.qmd"), "# One\n\nFirst chapter.\n");
    write(&dir.join("two.qmd"), "# Two\n\nSecond chapter.\n");
    dir
}

/// `q2 preview --static ARGS` that is expected to exit on its own.
fn exits(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(Q2_BIN)
        .arg("preview")
        .arg("--static")
        .arg("--no-browser")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run q2 preview --static")
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Whether a whole-book typst render works on this machine (the binary and
/// the `@preview/orange-book` package, which is fetched). A skip is a
/// silent pass, so the reason is printed.
fn typst_book_renders(dir: &Path, extra_args: &[&str]) -> bool {
    if which_typst().is_none() {
        eprintln!("skipping: no typst binary");
        return false;
    }
    let copy = TempDir::new().unwrap();
    let copy_dir = copy.path().canonicalize().unwrap();
    for name in ["_quarto.yml", "index.qmd", "one.qmd", "two.qmd"] {
        std::fs::copy(dir.join(name), copy_dir.join(name)).unwrap();
    }
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("_quarto-") {
            std::fs::copy(entry.path(), copy_dir.join(&name)).unwrap();
        }
    }
    let ok = Command::new(Q2_BIN)
        .arg("render")
        .args(extra_args)
        .current_dir(&copy_dir)
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("skipping: a whole-book typst render does not work here (package fetch?)");
    }
    ok
}

fn which_typst() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("QUARTO_TYPST") {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(if cfg!(windows) { "typst.exe" } else { "typst" }))
            .find(|p| p.is_file())
    })
}

#[test]
fn a_format_that_cannot_be_served_is_refused_once_before_any_render() {
    for fmt in ["epub", "docx", "pptx"] {
        let temp = TempDir::new().unwrap();
        let dir = book(&temp, &format!("format: {fmt}\n"));
        let out = exits(&dir, &[dir.to_str().unwrap()]);
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(1), "{fmt}: {err}");
        assert_eq!(
            err.matches("not supported with --static").count(),
            1,
            "{fmt}: refused once; stderr:\n{err}"
        );
        assert!(
            !err.contains("Rendering project"),
            "{fmt}: no render ran: {err}"
        );
        assert!(!dir.join("_book").exists(), "{fmt}: nothing was written");
    }

    // The same through the flags, on an html book.
    let temp = TempDir::new().unwrap();
    let dir = book(&temp, "format: html\n");
    for args in [&["--to", "epub"][..], &["-M", "format:epub"][..]] {
        let mut full = args.to_vec();
        full.push(dir.to_str().unwrap());
        let out = exits(&dir, &full);
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {err}");
        assert!(
            err.contains("not supported with --static"),
            "{args:?}: {err}"
        );
        assert!(!dir.join("_book").exists(), "{args:?}: nothing was written");
    }
}

#[test]
fn to_q2_preview_still_serves_html() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let server = Server::spawn(&dir, &["--to", "q2-preview", dir.to_str().unwrap()]);
    assert_eq!(server.get("/index.html").status, 200);
}

#[test]
fn a_malformed_metadata_flag_fails_before_anything_renders() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let out = exits(&dir, &["-M", "nocolon", dir.to_str().unwrap()]);
    let err = stderr_of(&out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("invalid metadata argument"), "{err}");
    assert!(!dir.join("_site").exists(), "nothing rendered");
}

#[test]
fn book_scope_chapter_on_a_book_exits_with_the_message() {
    let temp = TempDir::new().unwrap();
    let dir = book(&temp, "");
    // A directory, and a chapter inside the book (which previews the whole
    // project): both are the FR6 error, not a failure page and not a gate
    // error.
    for target in [dir.clone(), dir.join("one.qmd")] {
        let out = exits(
            &dir,
            &["-M", "book-scope:chapter", target.to_str().unwrap()],
        );
        let err = stderr_of(&out);
        assert_eq!(out.status.code(), Some(1), "{target:?}: {err}");
        assert!(err.contains("Q-5-47"), "{target:?}: {err}");
        assert!(!err.contains("not supported with --static"), "{err}");
    }
}

#[test]
fn book_scope_chapter_on_a_website_is_only_a_notice() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    let server = Server::spawn(&dir, &["-M", "book-scope:chapter", dir.to_str().unwrap()]);
    assert_eq!(server.get("/index.html").status, 200);
    assert!(!server.stderr().contains("Q-5-47"), "{}", server.stderr());
}

#[test]
fn an_html_book_has_no_scope_notice_and_re_renders_only_the_changed_chapter() {
    let temp = TempDir::new().unwrap();
    let dir = book(&temp, "format: html\n");
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    assert!(
        !server.stderr().contains("Q-5-42"),
        "a default scope must not warn; stderr:\n{}",
        server.stderr()
    );
    assert_eq!(server.get("/one.html").status, 200);
    let mut sse = server.sse();
    append(&dir.join("one.qmd"), "\n\nCHAPTER-EDIT-MARKER\n");
    let reload = sse.wait_for("reload");
    assert_eq!(reload, r#"{"type":"reload","target":"/one.html"}"#);
    assert!(
        server
            .get("/one.html")
            .text()
            .contains("CHAPTER-EDIT-MARKER")
    );
    let log = server.stderr();
    assert!(log.contains("re-rendering changed inputs"), "{log}");
    assert!(!log.contains("re-rendering the whole project"), "{log}");
}

#[test]
fn a_typst_book_is_served_and_a_chapter_edit_re_renders_the_whole_book() {
    let temp = TempDir::new().unwrap();
    let dir = book(&temp, "format: typst\n");
    if !typst_book_renders(&dir, &[]) {
        return;
    }
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    let log = server.stderr();
    for code in ["Q-5-42", "Q-21-3"] {
        assert!(!log.contains(code), "{code} in boot output:\n{log}");
    }
    let root = server.get("/");
    assert_eq!(root.status, 302);
    assert!(
        root.header("location")
            .is_some_and(|l| l.contains("Test-Book.pdf")),
        "the book's PDF opens in the viewer: {:?}",
        root.header("location")
    );
    let mut sse = server.sse();
    let before = std::fs::metadata(dir.join("_book/Test-Book.pdf"))
        .unwrap()
        .modified()
        .unwrap();
    append(&dir.join("one.qmd"), "\n\nCHAPTER-EDIT-MARKER\n");
    let reload = sse.wait_for("reload");
    // The merged output has no per-chapter page: a plain reload.
    assert_eq!(reload, r#"{"type":"reload","target":null}"#);
    let log = server.stderr();
    assert!(log.contains("re-rendering the whole project"), "{log}");
    assert!(!log.contains("re-rendering changed inputs"), "{log}");
    let after = std::fs::metadata(dir.join("_book/Test-Book.pdf"))
        .unwrap()
        .modified()
        .unwrap();
    assert!(after > before, "the book PDF was rebuilt");
}

#[test]
fn a_profile_can_make_the_project_a_typst_book() {
    let temp = TempDir::new().unwrap();
    let dir = book(&temp, "format: html\n");
    write(&dir.join("_quarto-pdf.yml"), "format: typst\n");
    if !typst_book_renders(&dir, &["--profile", "pdf"]) {
        return;
    }
    let server = Server::spawn(&dir, &["--profile", "pdf", dir.to_str().unwrap()]);
    let root = server.get("/");
    assert_eq!(root.status, 302);
    assert!(
        root.header("location").is_some_and(|l| l.contains(".pdf")),
        "{:?}",
        root.header("location")
    );
}

#[test]
fn a_render_that_wrote_nothing_shows_the_failure_at_the_root() {
    let temp = TempDir::new().unwrap();
    let dir = minimal_site(&temp);
    // A missing filter fails the page before any output is written.
    for page in ["index.qmd", "about.qmd"] {
        write(
            &dir.join(page),
            "---\ntitle: T\nfilters:\n  - nonexistent.lua\n---\n\nBroken.\n",
        );
    }
    let server = Server::spawn(&dir, &[dir.to_str().unwrap()]);
    let root = server.get("/");
    let page = root.text();
    assert_eq!(root.status, 200, "{page}");
    assert!(page.contains("The render failed"), "{page}");
    assert!(
        page.contains("nonexistent.lua"),
        "the diagnostics are on the page: {page}"
    );
    assert!(!page.contains("has no <code>index.html</code>"), "{page}");

    // Fixing the pages replaces the failure with the site.
    let mut sse = server.sse();
    write(&dir.join("index.qmd"), "# Home\n\nFIXED-MARKER\n");
    write(&dir.join("about.qmd"), "# About\n\nFixed.\n");
    // The two saves may arrive as two renders; wait for the one that has
    // both pages.
    let mut fixed = String::new();
    for _ in 0..5 {
        sse.wait_for("reload");
        fixed = server.get("/").text();
        if fixed.contains("FIXED-MARKER") {
            break;
        }
    }
    assert!(
        fixed.contains("FIXED-MARKER"),
        "{fixed}\n{}",
        server.stderr()
    );
}
