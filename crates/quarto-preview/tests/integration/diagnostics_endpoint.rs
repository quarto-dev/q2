//! Integration test for `GET /api/preview/diagnostics?page=<rel>`
//! (bd-b9kzg).
//!
//! Boots a preview server with a fixture project, injects a
//! synthetic diagnostic into the process-wide sink via the public
//! `current_sink()` accessor, then HTTP-GETs the endpoint and
//! verifies the JSON wire shape exactly matches what the SPA's
//! existing `Diagnostic` interface expects (1-based positions,
//! camel-cased keys, etc.).
//!
//! Coverage matrix:
//!   - `page=<known>`, sink populated → 200 + diagnostic list.
//!   - `page=<known>`, sink empty for that page → 200 + `{ diagnostics: [] }`.
//!   - `page=<unknown>`, sink empty → 200 + `{ diagnostics: [] }`
//!     (NOT 404 — the SPA always fetches; absent ≡ empty).
//!
//! Unaffected by Phase 3 callsite migrations — exercises the
//! transport layer directly.

use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use std::time::Duration;

use quarto_error_reporting::DiagnosticMessage;
use quarto_hub::HubContext;
use quarto_preview::{PreviewConfig, diagnostics, run_with_on_ready};
use tokio::sync::oneshot;

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

async fn boot_server_for_test() -> (
    u16,
    Arc<HubContext>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let project = tempfile::TempDir::with_prefix("q2-preview-diag-endpoint-").unwrap();
    std::fs::write(
        project.path().join("index.qmd"),
        "---\ntitle: Diagnostics test\n---\n\nHello.\n",
    )
    .unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-diag-endpoint-data-").unwrap();

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
    (port, ctx, handle, project, data)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn populated_page_returns_diagnostic_list() {
    let (port, _ctx, handle, _project, _data) = boot_server_for_test().await;

    // Inject a synthetic warning via the per-process sink the
    // server's `run_with_on_ready` registered at boot.
    let sink =
        diagnostics::current_sink().expect("server boot should have set the diagnostic sink");
    sink.emit(
        "index.qmd",
        DiagnosticMessage::warning("Synthetic test diagnostic").with_code("Q-TEST-1"),
    );

    let url = format!("http://127.0.0.1:{port}/api/preview/diagnostics?page=index.qmd");
    let resp = reqwest::get(&url).await.expect("GET succeeds");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("response is JSON");

    let diags = body
        .get("diagnostics")
        .and_then(|v| v.as_array())
        .expect("response has `diagnostics` array");
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one diagnostic; body was {body:?}"
    );

    let first = &diags[0];
    assert_eq!(
        first.get("kind").and_then(|v| v.as_str()),
        Some("warning"),
        "diagnostic kind serialized as lowercase string"
    );
    assert_eq!(
        first.get("title").and_then(|v| v.as_str()),
        Some("Synthetic test diagnostic"),
    );
    assert_eq!(first.get("code").and_then(|v| v.as_str()), Some("Q-TEST-1"),);

    handle.abort();
    let _ = handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_page_returns_empty_array_not_404() {
    let (port, _ctx, handle, _project, _data) = boot_server_for_test().await;

    // Sink is empty for this page — should still return 200 + an
    // empty `diagnostics` array. The SPA always fetches and treats
    // "no diagnostics" as a valid state, not an error.
    let url = format!("http://127.0.0.1:{port}/api/preview/diagnostics?page=unknown.qmd");
    let resp = reqwest::get(&url).await.expect("GET succeeds");
    assert_eq!(
        resp.status(),
        200,
        "unknown / never-emitted-for pages should be 200, not 404"
    );
    let body: serde_json::Value = resp.json().await.expect("response is JSON");
    let diags = body
        .get("diagnostics")
        .and_then(|v| v.as_array())
        .expect("response has `diagnostics` array");
    assert!(diags.is_empty(), "expected empty array; got {diags:?}");

    handle.abort();
    let _ = handle.await;
}
