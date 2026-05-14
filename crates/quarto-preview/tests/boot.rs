//! End-to-end in-process boot test for `quarto_preview::run`.
//!
//! Phase A.5 (bd-mflk). Spins up the full preview server (hub +
//! embedded SPA) in-process against a fresh tempdir project with one
//! `.qmd` file, hits the HTTP surface, asserts:
//!
//!   1. `GET /` serves the SPA's `index.html` (React mount present).
//!   2. `GET /health` returns the hub's status JSON, including a
//!      non-empty `index_document_id` (the SPA reads this on boot to
//!      know which automerge doc to subscribe to).
//!   3. `GET /api/some-unknown-path` falls back to the SPA (client-
//!      side routing works for unmatched paths).
//!
//! What this *doesn't* cover (intentional Phase-A gaps):
//!   - The websocket handshake — `/ws` is samod's upgrade endpoint
//!     and asserting the full samod handshake requires a samod
//!     client harness. The Playwright smoke (A.7 / bd-vpsy) gets at
//!     this end-to-end via the browser.
//!   - Browser auto-open and signal-driven shutdown — both belong
//!     to `commands/preview.rs` (the binary), not this lib-level
//!     test. The CLI surface tests in
//!     `crates/quarto/tests/preview_cli.rs` pin the args; the
//!     manual smoke (A.5.5) covers the binary boot.

use std::net::TcpListener as StdTcpListener;
use std::time::{Duration, Instant};

use quarto_preview::PreviewConfig;

/// Bind `127.0.0.1:0`, capture the assigned port, release the
/// listener. Small race window before run() rebinds, but acceptable
/// for a foreground test.
fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

/// Poll `GET /health` until it returns 200 or we hit `deadline`.
/// HubContext::new can take a beat (samod init, initial fs sync) so
/// we don't want a brittle fixed sleep.
async fn wait_for_health(port: u16) {
    let url = format!("http://127.0.0.1:{port}/health");
    let deadline = Instant::now() + Duration::from_secs(10);
    let client = reqwest::Client::new();
    loop {
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                return;
            }
        }
        if Instant::now() >= deadline {
            panic!("server didn't come up on port {port} within 10s");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn boots_serves_spa_plus_hub_health() {
    let project = tempfile::TempDir::with_prefix("q2-preview-test-proj-").unwrap();
    std::fs::write(
        project.path().join("foo.qmd"),
        "# Hello\n\nA tiny fixture page.\n",
    )
    .unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-test-data-").unwrap();

    let port = pick_free_port();
    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port,
        project_root: Some(project.path().to_path_buf()),
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: None,
    };

    // Spawn the server. `run()` blocks until shutdown; we abort the
    // handle at the end of the test (the TempDirs clean up via Drop).
    let server = tokio::spawn(async move {
        let _ = quarto_preview::run(config).await;
    });

    wait_for_health(port).await;

    // 1. GET / serves the SPA's index.html.
    let body = reqwest::get(format!("http://127.0.0.1:{port}/"))
        .await
        .expect("GET /")
        .error_for_status()
        .expect("200 OK")
        .text()
        .await
        .expect("body");
    assert!(
        body.contains(r#"id="root""#),
        "GET / should serve the SPA index.html; got:\n{body}",
    );

    // 2. GET /health → hub status JSON with index_document_id.
    let health: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/health"))
        .await
        .expect("GET /health")
        .error_for_status()
        .expect("200 OK")
        .json()
        .await
        .expect("body is JSON");
    assert_eq!(health["status"], "ok");
    let doc_id = health["index_document_id"]
        .as_str()
        .expect("index_document_id should be a string");
    assert!(
        !doc_id.is_empty(),
        "index_document_id should be non-empty; got: {doc_id}",
    );

    // 3. SPA fallback for arbitrary client-side routes.
    let resp = reqwest::get(format!("http://127.0.0.1:{port}/preview/some-route"))
        .await
        .expect("GET /preview/...");
    assert_eq!(resp.status().as_u16(), 200);
    let body = resp.text().await.expect("body");
    assert!(
        body.contains(r#"id="root""#),
        "fallback should serve SPA index.html; got:\n{body}",
    );

    server.abort();
}
