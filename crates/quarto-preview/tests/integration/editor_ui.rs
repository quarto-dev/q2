//! `--ui editor` server-level test (live-share plan Phase 4,
//! bd-jt1etjbn): booting the real preview server with
//! `PreviewUi::Editor` must serve the *editor* embed at `/` (the full
//! hub-client build, or its placeholder on an unbuilt tree) while the
//! hub's own routes keep answering. This pins the config → OnceLock →
//! handler plumbing that the lib-level `lookup_embedded` unit tests
//! cannot see.
//!
//! The browser-tier verification (editor boots, sidebar, Monaco edit
//! write-back) is the phase's mandatory recorded e2e run, not a cargo
//! test.

use std::net::TcpListener as StdTcpListener;
use std::time::{Duration, Instant};

use quarto_preview::{PreviewConfig, PreviewUi};

/// Bind `127.0.0.1:0`, capture the assigned port, release the listener.
fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

/// Poll `GET /health` until 200 or a 10 s deadline.
async fn wait_for_health(port: u16) {
    let url = format!("http://127.0.0.1:{port}/health");
    let deadline = Instant::now() + Duration::from_secs(10);
    let client = reqwest::Client::new();
    loop {
        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            return;
        }
        if Instant::now() >= deadline {
            panic!("server didn't come up on port {port} within 10s");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn editor_ui_serves_editor_index_and_keeps_hub_routes() {
    let project = tempfile::TempDir::with_prefix("q2-preview-editor-proj-").unwrap();
    std::fs::write(
        project.path().join("foo.qmd"),
        "# Hello\n\nEditor-mode fixture page.\n",
    )
    .unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-editor-data-").unwrap();

    let port = pick_free_port();
    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port,
        project_root: Some(project.path().to_path_buf()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: None,
        engine_policy: Default::default(),
        resource_html_files: Vec::new(),
        cache_dir: None,
        allow_edit: false,
        share: false,
        ui: PreviewUi::Editor,
    };

    let server = tokio::spawn(async move {
        let _ = quarto_preview::run(config).await;
    });

    wait_for_health(port).await;

    let expected_index = quarto_preview::embedded_editor_index_html()
        .expect("the editor embed always has an index.html (real dist or placeholder)");

    // 1. GET / serves the *editor* embed's index.html.
    let body = reqwest::get(format!("http://127.0.0.1:{port}/"))
        .await
        .expect("GET /")
        .error_for_status()
        .expect("200 OK")
        .text()
        .await
        .expect("body");
    assert_eq!(
        body, expected_index,
        "`--ui editor` must serve the editor embed's index.html at /"
    );

    // 2. Unknown paths still fall back to the (editor) index for
    //    client-side routing.
    let fallback = reqwest::get(format!("http://127.0.0.1:{port}/no/such/path"))
        .await
        .expect("GET /no/such/path")
        .error_for_status()
        .expect("200 OK")
        .text()
        .await
        .expect("body");
    assert_eq!(
        fallback, expected_index,
        "SPA fallback in editor mode must serve the editor index"
    );

    // 3. Hub routes keep winning over the SPA fallback: /health already
    //    answered above; the preview-config route must too (the editor
    //    reads nothing from it today, but the composition shape is the
    //    same one the viewer relies on).
    let cfg: serde_json::Value =
        reqwest::get(format!("http://127.0.0.1:{port}/api/preview/config"))
            .await
            .expect("GET /api/preview/config")
            .error_for_status()
            .expect("200 OK")
            .json()
            .await
            .expect("json body");
    assert_eq!(cfg["allowEdit"], serde_json::Value::Bool(false));

    server.abort();
}
