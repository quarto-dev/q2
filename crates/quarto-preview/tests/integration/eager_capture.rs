//! End-to-end integration test for Phase C.1's server-side eager
//! engine-capture recording.
//!
//! Drives `quarto_preview::run_with_on_ready` against a tempdir
//! project that contains one `.qmd` file using a registered
//! passthrough engine. After the server reports ready, polls the
//! IndexDocument's capture sidecar (V2 schema landed in C.3) until
//! a `captureDocId` is populated for the file, then verifies the
//! linked binary doc round-trips back to a parseable `EngineCapture`.
//!
//! This is the §C.1 integration item #3 in
//! `claude-notes/plans/2026-05-13-q2-preview-phase-c.md` —
//! "start `q2 preview` against a fixture with one code cell, assert
//! the index doc's sidecar entry gets `captureDocId` populated
//! within 30 s and that the linked binary doc contains a parseable
//! `EngineCapture`."
//!
//! No real R / Python runtime needed: the passthrough engine reports
//! as a non-`markdown` name to bypass `EngineExecutionStage`'s
//! short-circuit and trigger capture emission.

use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError,
};
use quarto_hub::HubContext;
use quarto_preview::{PreviewConfig, capture_driver, run_with_on_ready};
use tokio::sync::oneshot;

struct PassthroughTestEngine;

impl ExecutionEngine for PassthroughTestEngine {
    fn name(&self) -> &str {
        "test-passthrough"
    }
    fn execute(
        &self,
        input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        let mut out = String::from(input);
        out.push_str("\n<!-- test-passthrough -->\n");
        Ok(ExecuteResult::passthrough(&out))
    }
}

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn eager_capture_populates_index_sidecar() {
    let project = tempfile::TempDir::with_prefix("q2-preview-c1-").unwrap();
    let qmd = "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nSENTINEL\n```\n";
    std::fs::write(project.path().join("doc.qmd"), qmd).unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-c1-data-").unwrap();

    let port = pick_free_port();

    // Register the passthrough engine so the eager-capture driver
    // doesn't fall back to the default `markdown` engine (which
    // would short-circuit and emit no capture).
    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(PassthroughTestEngine));

    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port,
        project_root: Some(project.path().to_path_buf()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: Some(registry),
        engine_policy: Default::default(),
        cache_dir: None,
    };

    let (ready_tx, ready_rx) = oneshot::channel::<Arc<HubContext>>();
    // run_with_on_ready takes a FnOnce, but the test future holds the
    // oneshot tx by value — using a one-shot is the simplest way to
    // hand the ctx across the spawn boundary without locking.
    let mut ready_tx = Some(ready_tx);
    let server_handle = tokio::spawn(async move {
        run_with_on_ready(config, move |ctx| {
            if let Some(tx) = ready_tx.take() {
                let _ = tx.send(ctx);
            }
        })
        .await
    });

    // Receive the ctx the moment the driver has been spawned.
    let ctx = tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .expect("server reached on_ready within 10s")
        .expect("on_ready callback fired");

    // The driver runs on a blocking worker after on_ready returns,
    // so poll the sidecar for the capture entry.
    let deadline = Instant::now() + Duration::from_secs(30);
    let entry = loop {
        if let Some(entry) = ctx.index().get_capture("doc.qmd") {
            break entry;
        }
        if Instant::now() >= deadline {
            panic!("capture sidecar not populated within 30s");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    assert!(
        !entry.capture_doc_id.is_empty(),
        "captureDocId should be non-empty"
    );

    // Pull the binary doc back out and verify it's a parseable
    // EngineCapture with the expected fields. This exercises the
    // same path Phase C.4 will use in the browser: read the binary
    // doc, ungzip, parse JSON.
    let captures = capture_driver::read_capture_from_doc(&ctx, &entry.capture_doc_id)
        .await
        .expect("capture doc round-trips");
    assert_eq!(captures.len(), 1);
    let capture = &captures[0];

    assert_eq!(capture.engine_name, "test-passthrough");
    assert!(
        capture.input_qmd.contains("SENTINEL"),
        "captured input_qmd should preserve cell body; got: {}",
        capture.input_qmd
    );
    let markdown = capture
        .result
        .get("markdown")
        .and_then(|v| v.as_str())
        .expect("capture.result.markdown");
    assert!(
        markdown.contains("<!-- test-passthrough -->"),
        "captured result.markdown should include the engine's sentinel comment; got: {}",
        markdown
    );

    server_handle.abort();
    let _ = server_handle.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prose_only_doc_leaves_sidecar_empty() {
    // Negative case (§C.1 integration test #4): the same lifecycle
    // against a prose-only fixture must NOT write a sidecar entry.
    let project = tempfile::TempDir::with_prefix("q2-preview-c1-prose-").unwrap();
    std::fs::write(
        project.path().join("doc.qmd"),
        "---\ntitle: Prose only\n---\n\nNo cells here.\n",
    )
    .unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-c1-prose-data-").unwrap();

    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port: pick_free_port(),
        project_root: Some(project.path().to_path_buf()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: None,
        engine_policy: Default::default(),
        cache_dir: None,
    };

    let (ready_tx, ready_rx) = oneshot::channel::<Arc<HubContext>>();
    let mut ready_tx = Some(ready_tx);
    let server_handle = tokio::spawn(async move {
        run_with_on_ready(config, move |ctx| {
            if let Some(tx) = ready_tx.take() {
                let _ = tx.send(ctx);
            }
        })
        .await
    });

    let ctx = tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .expect("on_ready in 10s")
        .expect("ctx sent");

    // Wait long enough for the driver to have run if it was going to,
    // then assert sidecar is still empty. We don't have a "driver
    // done" signal, so the wait is conservative — the driver itself
    // is sub-second for a 1-file project.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        !ctx.index().has_capture("doc.qmd"),
        "prose-only doc should not produce a sidecar entry"
    );

    server_handle.abort();
    let _ = server_handle.await;
}
