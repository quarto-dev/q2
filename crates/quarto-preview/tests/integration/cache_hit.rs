//! Phase C.7 integration test: cache hit avoids engine re-invocation.
//!
//! Drives the full `capture_driver::record_eager_captures` path twice
//! against the same fixture, sharing a single `cache_dir`. Between
//! runs we clear the sidecar (`remove_capture`) so the driver's
//! idempotence check doesn't short-circuit before reaching the cache.
//! The counting passthrough engine asserts the engine was invoked
//! exactly once across both runs — the second run hits the cache.
//!
//! Plan §C.7 acceptance: "edit a code cell back to its original
//! content; assert the second run uses the cache (no engine
//! invocation — verified by registry inspection)."

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError,
};
use quarto_hub::HubContext;
use quarto_hub::context::HubConfig;
use quarto_hub::storage::StorageManager;
use quarto_preview::capture_driver;
use quarto_preview::config::EnginePolicy;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Engine that increments a shared counter on every `execute` call.
/// Counter is held externally so the test can inspect it across two
/// driver invocations.
struct CountingPassthroughEngine {
    calls: Arc<AtomicUsize>,
}

impl ExecutionEngine for CountingPassthroughEngine {
    fn name(&self) -> &str {
        "test-passthrough"
    }
    fn execute(
        &self,
        input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut out = String::from(input);
        out.push_str("\n<!-- counted -->\n");
        Ok(ExecuteResult::passthrough(&out))
    }
}

fn registry_with_counter(calls: Arc<AtomicUsize>) -> EngineRegistry {
    let mut reg = EngineRegistry::new();
    reg.register(Arc::new(CountingPassthroughEngine { calls }));
    reg
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_eager_run_with_same_cache_dir_hits_cache_skips_engine() {
    // Set up a project with one engine-bearing doc.
    let project = tempfile::TempDir::with_prefix("q2-c7-int-").unwrap();
    let qmd = "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nIDENT\n```\n";
    std::fs::write(project.path().join("doc.qmd"), qmd).unwrap();

    let cache_dir = project.path().join("captures");

    let storage = StorageManager::new(project.path()).unwrap();
    let ctx = Arc::new(
        HubContext::new(storage, HubConfig::default())
            .await
            .unwrap(),
    );
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    // First run: cache miss → engine fires once.
    let calls = Arc::new(AtomicUsize::new(0));
    let count = capture_driver::record_eager_captures(
        ctx.clone(),
        runtime.clone(),
        Some(registry_with_counter(calls.clone())),
        EnginePolicy::Manual,
        &cache_dir,
    )
    .await
    .expect("first run succeeds");
    assert_eq!(count, 1, "first run records one capture");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "first run invokes the engine exactly once"
    );

    // Cache file should be present on disk now.
    let entries: Vec<_> = std::fs::read_dir(&cache_dir).unwrap().collect();
    assert_eq!(
        entries.len(),
        1,
        "cache dir should contain one entry after first run"
    );

    // Defeat the driver's idempotence-by-sidecar check so the second
    // call actually walks the file again. This models the "fresh
    // session against the same project content" scenario.
    ctx.index().remove_capture("doc.qmd").unwrap();
    assert!(!ctx.index().has_capture("doc.qmd"));

    // Second run: same content, same cache_dir → cache HIT, NO engine.
    let calls2 = Arc::new(AtomicUsize::new(0));
    let count2 = capture_driver::record_eager_captures(
        ctx.clone(),
        runtime,
        Some(registry_with_counter(calls2.clone())),
        EnginePolicy::Manual,
        &cache_dir,
    )
    .await
    .expect("second run succeeds");
    assert_eq!(
        count2, 1,
        "second run still emits a capture into the sidecar"
    );
    assert_eq!(
        calls2.load(Ordering::SeqCst),
        0,
        "second run must NOT invoke the engine — cache hit"
    );

    // Sidecar is populated again, capture content matches.
    let sidecar = ctx
        .index()
        .get_capture("doc.qmd")
        .expect("sidecar repopulated");
    assert!(!sidecar.capture_doc_id.is_empty());
    let capture = capture_driver::read_capture_from_doc(&ctx, &sidecar.capture_doc_id)
        .await
        .expect("binary doc round-trips");
    assert_eq!(capture.engine_name, "test-passthrough");
    assert!(capture.input_qmd.contains("IDENT"));
    let markdown = capture
        .result
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(
        markdown.contains("<!-- counted -->"),
        "the cached capture (originally produced by the counting engine) carries the sentinel"
    );
}
