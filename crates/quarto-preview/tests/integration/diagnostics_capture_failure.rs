//! Integration test: the eager-capture driver's failure path
//! (bd-b9kzg).
//!
//! Boots a preview server against a project containing a `.qmd`
//! whose declared engine is registered but always fails on
//! `execute()`. The capture driver runs the engine, sees an Err,
//! and (post-Phase-3) emits a structured diagnostic to the
//! per-page sink instead of just `tracing::warn!`-ing to stderr.
//!
//! Pins Phase 3's `capture_driver.rs:114-120` migration. The
//! "engine fallback for an unknown name" path (which falls back
//! to `markdown` and short-circuits) wouldn't trigger the Err
//! branch — we need a registered engine that affirmatively fails,
//! which is why we register `FailingTestEngine` here rather than
//! relying on engine resolution to fail.

use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError,
};
use quarto_hub::HubContext;
use quarto_preview::{PreviewConfig, diagnostics, run_with_on_ready};
use tokio::sync::oneshot;

/// Test engine that's "valid enough" to be picked by engine
/// resolution (non-markdown name → engine path runs) but always
/// returns `Err` from `execute()`. Mirrors `PassthroughTestEngine`
/// in `eager_capture.rs` but with the failure semantics this test
/// needs.
struct FailingTestEngine;

impl ExecutionEngine for FailingTestEngine {
    fn name(&self) -> &str {
        "test-failing"
    }
    fn execute(
        &self,
        _input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        Err(ExecutionError::Other(
            "synthetic test-failing engine failure".to_string(),
        ))
    }
}

fn pick_free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capture_failure_lands_in_sink() {
    let project = tempfile::TempDir::with_prefix("q2-preview-diag-cap-").unwrap();
    // `engine: test-failing` matches the FailingTestEngine
    // registered below; the cell triggers EngineExecutionStage
    // (without code cells the pre-engine pipeline returns early).
    let qmd = "---\nengine: test-failing\n---\n\n```{test-failing}\nbody\n```\n";
    std::fs::write(project.path().join("broken.qmd"), qmd).unwrap();
    let data = tempfile::TempDir::with_prefix("q2-preview-diag-cap-data-").unwrap();

    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(FailingTestEngine));

    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port: pick_free_port(),
        project_root: Some(project.path().to_path_buf()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: Some(registry),
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

    let _ctx = tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .expect("server reached on_ready within 10s")
        .expect("on_ready callback fired");

    let sink =
        diagnostics::current_sink().expect("server boot should have registered a diagnostic sink");

    // The capture driver runs on a blocking worker after on_ready;
    // poll the sink for the expected diagnostic. Bounded by the
    // same 30s window the existing eager_capture integration test
    // uses for sidecar updates.
    let deadline = Instant::now() + Duration::from_secs(30);
    let diagnostics_for_page = loop {
        let diags = sink.get_for_page("broken.qmd");
        if !diags.is_empty() {
            break diags;
        }
        if Instant::now() >= deadline {
            panic!(
                "no diagnostic emitted for broken.qmd within 30s; \
                 capture_driver migration (Phase 3) may not be wired up yet, \
                 OR the failure path is being silently swallowed"
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    let diag = &diagnostics_for_page[0];
    assert!(
        diag.title.to_lowercase().contains("capture")
            || diag.title.to_lowercase().contains("engine"),
        "diagnostic title should mention capture or engine failure; got: {:?}",
        diag.title
    );
    // The engine's own error message should reach the diagnostic's
    // `problem` field so the user sees *why* it failed, not just
    // that it failed.
    if let Some(problem) = &diag.problem {
        assert!(
            problem.as_str().contains("synthetic test-failing"),
            "problem field should include the engine's error message; got: {:?}",
            problem.as_str()
        );
    }

    handle.abort();
    let _ = handle.await;
}
