//! The preview VFS miss routes and the request gate (preview VFS contract
//! D2, D4, D6, D7, D8).
//!
//! Each test boots a real preview server against a fixture and drives the
//! routes over HTTP:
//!
//! - `GET /api/preview/file?path=` returns an admitted file's bytes and
//!   promotes it into the Automerge index (so it is synced and watched
//!   from then on); 404 for an absent path, 403 for a refused one.
//! - `GET /api/preview/stat?path=` answers exists / is-file / is-dir, and
//!   watches the directory it was asked about.
//! - `GET /api/preview/list?path=` returns only the names the exposure
//!   policy admits, and watches the directory.
//! - A file created later in a watched directory is enrolled into the
//!   index; a file created where nothing asked is not.
//! - The router-wide `Host` gate refuses a DNS-rebinding request on every
//!   route.

use std::io::{Read, Write};
use std::net::TcpListener as StdTcpListener;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use automerge::ReadDoc;
use quarto_hub::HubContext;
use quarto_preview::{PreviewConfig, run_with_on_ready};
use tokio::sync::oneshot;

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

struct Server {
    port: u16,
    ctx: Arc<HubContext>,
    /// The canonical project root (the deck directory in single-file mode).
    root: std::path::PathBuf,
    _handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    _project: tempfile::TempDir,
    _data: tempfile::TempDir,
}

/// Boot a preview server over a fixture built by `setup`. `single_file`
/// selects single-file mode with that deck.
async fn boot(single_file: Option<&str>, setup: impl FnOnce(&Path)) -> Server {
    let project = tempfile::TempDir::with_prefix("q2-preview-miss-").unwrap();
    let root = quarto_system_runtime::canonicalize(project.path()).unwrap();
    setup(&root);
    let data = tempfile::TempDir::with_prefix("q2-preview-miss-data-").unwrap();

    let port = pick_free_port();
    let config = PreviewConfig {
        render: Default::default(),
        host: "127.0.0.1".to_string(),
        port,
        project_root: Some(root.clone()),
        single_file: single_file.map(Into::into),
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: None,
        engine_policy: Default::default(),
        resource_html_files: Vec::new(),
        cache_dir: None,
        allow_edit: false,
        share: false,
        ui: Default::default(),
    };

    let (ready_tx, ready_rx) = oneshot::channel::<Arc<HubContext>>();
    let mut ready_tx = Some(ready_tx);
    let handle = tokio::spawn(async move {
        run_with_on_ready(config, move |ctx| {
            if let Some(tx) = ready_tx.take() {
                let _ = tx.send(ctx);
            }
        })
        .await
    });
    let ctx = tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .expect("server reached on_ready within 10s")
        .expect("on_ready callback fired");
    wait_for_health(port).await;
    Server {
        port,
        ctx,
        root,
        _handle: handle,
        _project: project,
        _data: data,
    }
}

async fn wait_for_health(port: u16) {
    let url = format!("http://127.0.0.1:{port}/health");
    for _ in 0..100 {
        if reqwest::get(&url).await.is_ok_and(|r| r.status() == 200) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never became healthy");
}

fn write(root: &Path, rel: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

async fn get(port: u16, route: &str, path: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/api/preview/{route}"))
        .query(&[("path", path)])
        .send()
        .await
        .expect("request succeeds")
}

/// Poll until `cond` holds, for up to five seconds (the preview watcher
/// debounces for 500 ms).
async fn eventually(what: &str, mut cond: impl AsyncFnMut() -> bool) {
    for _ in 0..100 {
        if cond().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

/// The text content of an indexed text document.
async fn doc_text(ctx: &HubContext, rel: &str) -> Option<String> {
    let id = ctx.index().get_file(rel)?;
    let handle = ctx
        .repo()
        .find(samod::DocumentId::from_str(&id).ok()?)
        .await
        .ok()??;
    handle.with_document(|doc| {
        let (_, text) = doc.get(automerge::ROOT, "text").ok()??;
        doc.text(&text).ok()
    })
}

// ── file ──────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_serves_admitted_bytes_and_promotes_them() {
    let s = boot(None, |root| {
        write(root, "index.qmd", "# Hi");
        write(root, "refs.bib", "@book{a, title={A}}");
    })
    .await;
    assert!(
        !s.ctx.index().has_file("refs.bib"),
        "the walk does not sync bibliographies"
    );

    for path in ["refs.bib", "/project/refs.bib"] {
        let resp = get(s.port, "file", path).await;
        assert_eq!(resp.status(), 200, "{path}");
        assert_eq!(resp.text().await.unwrap(), "@book{a, title={A}}");
    }
    assert!(
        s.ctx.index().has_file("refs.bib"),
        "a pulled file is promoted"
    );
    assert_eq!(
        doc_text(&s.ctx, "refs.bib").await.as_deref(),
        Some("@book{a, title={A}}")
    );

    // Promoted means watched: a disk edit reaches the document.
    write(&s.root, "refs.bib", "@book{b, title={B}}");
    eventually("the disk edit to sync", async || {
        doc_text(&s.ctx, "refs.bib").await.as_deref() == Some("@book{b, title={B}}")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_refuses_what_the_policy_refuses() {
    let outside = tempfile::TempDir::new().unwrap();
    write(outside.path(), "secret.qmd", "secret");
    let outside_path = outside.path().join("secret.qmd");
    let s = boot(None, move |root| {
        write(root, "index.qmd", "# Hi");
        write(root, ".env", "TOKEN=x");
        write(root, "index_files/x.css", "a{}");
        write(root, "a.exe", "MZ");
        write(root, "big.png", vec![0u8; 26 * 1024 * 1024]);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside_path, root.join("leak.qmd")).unwrap();
        #[cfg(not(unix))]
        let _ = outside_path;
    })
    .await;

    assert_eq!(get(s.port, "file", "missing.bib").await.status(), 404);
    let mut refused = vec![
        "../outside.qmd",
        "/etc/hosts",
        ".env",
        "index_files/x.css",
        "a.exe",
        "big.png",
    ];
    if cfg!(unix) {
        refused.push("leak.qmd");
    }
    for path in refused {
        let resp = get(s.port, "file", path).await;
        assert_eq!(resp.status(), 403, "{path}");
        assert!(!resp.text().await.unwrap().is_empty(), "{path}: a reason");
    }
    // `big.png` is absent from this list on purpose: project mode's eager
    // walk syncs images whatever their size; the cap bounds pulls only.
    for rel in [".env", "index_files/x.css", "leak.qmd"] {
        assert!(!s.ctx.index().has_file(rel), "{rel} must not be promoted");
    }
}

// ── stat ──────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stat_answers_for_admitted_paths() {
    let s = boot(None, |root| {
        write(root, "index.qmd", "# Hi");
        write(root, "styles/deck.scss", "$x: 1;");
    })
    .await;
    let json = async |path: &str| -> (u16, serde_json::Value) {
        let resp = get(s.port, "stat", path).await;
        let status = resp.status().as_u16();
        (status, resp.json().await.unwrap_or_default())
    };
    let (status, body) = json("styles").await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        serde_json::json!({"exists": true, "isFile": false, "isDir": true})
    );
    let (_, body) = json("styles/deck.scss").await;
    assert_eq!(
        body,
        serde_json::json!({"exists": true, "isFile": true, "isDir": false})
    );
    let (status, body) = json("styles/_missing.scss").await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        serde_json::json!({"exists": false, "isFile": false, "isDir": false})
    );
    assert_eq!(json(".git/config").await.0, 403);
    assert_eq!(json("../x.scss").await.0, 403);
}

// ── list ──────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_returns_only_admitted_names() {
    let s = boot(None, |root| {
        write(root, "index.qmd", "# Hi");
        write(root, "dir/a.scss", "$a: 1;");
        write(root, "dir/b.exe", "MZ");
        write(root, "dir/.hidden.scss", "$h: 1;");
        write(root, "dir/sub/c.css", "c{}");
        write(root, "dir/.git/HEAD", "ref");
        write(root, "dir/node_modules/x.css", "x{}");
        write(root, ".quarto/x.css", "x{}");
    })
    .await;
    let resp = get(s.port, "list", "dir").await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body,
        serde_json::json!({"entries": [
            {"name": "a.scss", "kind": "file"},
            {"name": "sub", "kind": "dir"},
        ]})
    );
    assert_eq!(get(s.port, "list", "nope").await.status(), 404);
    assert_eq!(get(s.port, "list", ".quarto").await.status(), 403);
    assert_eq!(get(s.port, "list", "dir/node_modules").await.status(), 403);
    assert_eq!(get(s.port, "list", "index.qmd").await.status(), 404);
}

// ── enrolment ─────────────────────────────────────────────────────────

/// Single-file mode syncs nothing beyond the deck until a route asks, and a
/// directory a route answered for is watched: a file created there later
/// is enrolled, a file created where nothing asked is not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_file_enrols_only_where_a_route_asked() {
    let s = boot(Some("deck.qmd"), |root| {
        write(root, "deck.qmd", "# Deck");
        write(root, "notes.qmd", "# Unrelated");
        write(root, "refs.bib", "@book{a,}");
        std::fs::create_dir_all(root.join("partials")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
    })
    .await;
    let mut indexed: Vec<String> = s.ctx.index().get_all_files().into_keys().collect();
    indexed.sort();
    assert_eq!(
        indexed,
        vec!["deck.qmd".to_string()],
        "nothing else at boot"
    );

    // A probe for a missing partial watches its directory.
    let resp = get(s.port, "stat", "partials/_vars.scss").await;
    assert_eq!(resp.status(), 200);
    // A pull in single-file mode promotes, like in project mode.
    assert_eq!(get(s.port, "file", "refs.bib").await.status(), 200);
    assert!(s.ctx.index().has_file("refs.bib"));

    write(&s.root, "other/x.scss", "$o: 1;");
    write(&s.root, "partials/_vars.scss", "$v: 1;");
    eventually("the probed partial to be enrolled", async || {
        s.ctx.index().has_file("partials/_vars.scss")
    })
    .await;
    // Give a stray event for `other/` the same window to arrive.
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(!s.ctx.index().has_file("other/x.scss"));
    assert!(!s.ctx.index().has_file("notes.qmd"));

    // The pulled bibliography is watched too.
    write(&s.root, "refs.bib", "@book{edited,}");
    eventually("the pulled .bib edit to sync", async || {
        doc_text(&s.ctx, "refs.bib").await.as_deref() == Some("@book{edited,}")
    })
    .await;
}

/// Project mode: a file created after boot that the walk would have synced
/// reaches the index without a restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_mode_enrols_new_files() {
    let s = boot(None, |root| {
        write(root, "_quarto.yml", "project:\n  type: default\n");
        write(root, "index.qmd", "# Hi");
    })
    .await;
    write(&s.root, "posts/new.qmd", "# New");
    write(&s.root, "img/new.png", [0x89, 0x50, 0x4e, 0x47]);
    write(&s.root, "index_files/gen.png", [0x89, 0x50]);
    eventually("the new files to be enrolled", async || {
        s.ctx.index().has_file("posts/new.qmd") && s.ctx.index().has_file("img/new.png")
    })
    .await;
    assert!(!s.ctx.index().has_file("index_files/gen.png"));
}

// ── the request gate ──────────────────────────────────────────────────

/// Send a raw HTTP/1.1 request with an explicit `Host` header and return
/// the status code.
fn raw_status(port: u16, target: &str, host: &str, extra: &str) -> u16 {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).unwrap();
    let head = String::from_utf8_lossy(&buf[..n]);
    head.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no status line in {head:?}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_host_gate_refuses_rebinding_requests_on_every_route() {
    let s = boot(None, |root| {
        write(root, "index.qmd", "# Hi");
    })
    .await;
    let port = s.port;
    let ws = "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n";
    let statuses = tokio::task::spawn_blocking(move || {
        let evil = format!("evil.example:{port}");
        let mut out = Vec::new();
        for (target, extra) in [
            ("/health", ""),
            ("/api/files", ""),
            ("/api/preview/config", ""),
            ("/api/preview/file?path=index.qmd", ""),
            ("/ws", ws),
            ("/", ""),
        ] {
            out.push((target, raw_status(port, target, &evil, extra)));
        }
        for host in [
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            format!("[::1]:{port}"),
            "LOCALHOST".to_string(),
            format!("preview.localhost:{port}"),
        ] {
            out.push(("ok:/health", raw_status(port, "/health", &host, "")));
        }
        out
    })
    .await
    .unwrap();
    for (target, status) in statuses {
        if target.starts_with("ok:") {
            assert_eq!(status, 200, "{target}");
        } else {
            assert_eq!(status, 403, "{target} with a foreign Host");
        }
    }
}
