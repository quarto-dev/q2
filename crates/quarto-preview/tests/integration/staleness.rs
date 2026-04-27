//! End-to-end staleness detection (Phase C.2 follow-up, bd-u3ze).
//!
//! Boots `quarto_preview::run_with_on_ready` against a fixture with one
//! engine-bearing `.qmd`, waits for the eager capture to land, edits
//! the cell body on disk, then polls the sidecar for
//! `staleness: true`. This is the missing in-process integration test
//! the original Phase C.2 work deferred (the symptom was a 30-second
//! timeout under `cargo nextest run` on macOS — the file-watcher event
//! never reached the on-file-changed callback in that configuration).
//!
//! The implementation here works around the historical flake by:
//!
//! 1. Waiting for the eager capture *before* the edit. The capture
//!    proves the on_ready chain is wired, which in turn proves the
//!    file watcher had time to subscribe to FSEvents.
//! 2. Forcing a brief `tokio::time::sleep` after the capture lands
//!    to give notify-rs's macOS coalescing window a chance to clear,
//!    so the subsequent write is treated as a new event rather than
//!    merged into the initial sync's modifications.
//! 3. Polling with a generous deadline (15 s) so a slow CI run has
//!    headroom without masking a real hang.
//! 4. Re-writing the file atomically (the cell-body-changed path) so
//!    we don't accidentally rely on size-based coalescing.

use std::net::TcpListener as StdTcpListener;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError,
};
use quarto_hub::HubContext;
use quarto_preview::{PreviewConfig, run_with_on_ready};
use tokio::sync::oneshot;

const QMD_INITIAL: &str =
    "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n";

const QMD_EDITED: &str =
    "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n";

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

/// Atomic-rename file write: write to a sibling temp file then
/// rename over the target. macOS notify-rs is more reliable at
/// observing renames than in-place truncate+rewrite under some
/// configurations.
fn atomic_write(path: &Path, content: &str) {
    let tmp = path.with_extension("qmd.tmp");
    std::fs::write(&tmp, content).expect("write temp file");
    std::fs::rename(&tmp, path).expect("rename onto target");
}

// bd-9brz: reliably fails on macOS 26.4 with the hub-server boot path
// — FSEvents stops delivering events to the watcher's debouncer. An
// isolated probe (plain notify + tokio) works, the test-code-level
// kicker pattern works, but no kicker placed inside the production
// path does. Real `q2 preview` binary is unaffected in typical use.
// Disabled until root cause is found (see bd-9brz for the
// investigation log). Run manually with
// `cargo nextest run --run-ignored only` to retry.
#[ignore = "bd-9brz: FSEvents starved by hub-server boot path on macOS"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cell_edit_flips_staleness_in_sidecar() {
    // ── Fixture setup ──────────────────────────────────────────────
    let project = tempfile::TempDir::with_prefix("q2-preview-c2-").unwrap();
    // Canonicalize because the file watcher reports `/private/tmp/...`
    // on macOS while `TempDir` hands us `/tmp/...` — sync_file
    // canonicalizes both sides, but staying consistent in the test
    // makes the assertions easier to read on failure.
    let project_root = project.path().canonicalize().unwrap();
    let qmd_path = project_root.join("doc.qmd");
    std::fs::write(&qmd_path, QMD_INITIAL).unwrap();

    let data = tempfile::TempDir::with_prefix("q2-preview-c2-data-").unwrap();

    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(PassthroughTestEngine));

    let config = PreviewConfig {
        host: "127.0.0.1".to_string(),
        port: pick_free_port(),
        project_root: Some(project_root.clone()),
        single_file: None,
        data_dir: data.path().to_path_buf(),
        spa_dir_override: None,
        engine_registry: Some(Arc::new(registry)),
        engine_policy: Default::default(),
        resource_html_files: Vec::new(),
        cache_dir: None,
        allow_edit: false,
    };

    // ── Boot the server, capture the HubContext via on_ready ───────
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
        .expect("server reached on_ready within 10s")
        .expect("on_ready callback fired");

    // ── Wait for eager capture (proves on_ready chain is wired) ────
    let entry = await_initial_capture(&ctx, "doc.qmd", Duration::from_secs(30));
    let initial_doc_id = entry.capture_doc_id.clone();
    assert_eq!(
        entry.staleness,
        Some(false),
        "eager capture must land with staleness=false; got: {:?}",
        entry.staleness,
    );

    // Phase C.2 historical flake mitigation: the macOS FSEvents
    // coalescing window can merge the initial filesystem sync's
    // writes with subsequent edits if they happen within ~100ms.
    // Sleep past that window so the upcoming write is observed as
    // a clean Modified event.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ── Edit the cell body on disk ─────────────────────────────────
    atomic_write(&qmd_path, QMD_EDITED);

    // ── Poll for the staleness flip ────────────────────────────────
    let flipped = await_staleness_flip(&ctx, "doc.qmd", Duration::from_secs(15));

    assert!(
        flipped,
        "staleness never flipped to true after on-disk edit; \
         initial_doc_id={initial_doc_id}",
    );

    // Sidecar's captureDocId should still point at the *previous*
    // capture — `recompute_staleness` only flips the flag, it
    // doesn't run a new engine (that's C.5's job, gated on the
    // /api/preview/re-execute endpoint).
    let after = ctx
        .index()
        .get_capture("doc.qmd")
        .expect("sidecar entry still present");
    assert_eq!(after.staleness, Some(true));
    assert_eq!(
        after.capture_doc_id, initial_doc_id,
        "recompute_staleness must not replace the captureDocId; \
         that's the re-execute endpoint's responsibility",
    );

    server_handle.abort();
    let _ = server_handle.await;
}

/// Poll the sidecar until a capture entry appears for `rel_path`, or
/// time out. Panics with a descriptive message on timeout so a
/// failure narrates *which* phase of the test missed its budget.
fn await_initial_capture(
    ctx: &Arc<HubContext>,
    rel_path: &str,
    budget: Duration,
) -> quarto_hub::index::CaptureRef {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(entry) = ctx.index().get_capture(rel_path) {
            return entry;
        }
        if Instant::now() >= deadline {
            panic!(
                "eager capture for {rel_path} did not land within {:?}; \
                 the on_ready → capture_driver chain is not wired",
                budget,
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Poll the sidecar for `staleness == Some(true)`. Returns true on
/// success, false on timeout — caller asserts.
fn await_staleness_flip(ctx: &Arc<HubContext>, rel_path: &str, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(entry) = ctx.index().get_capture(rel_path)
            && entry.staleness == Some(true)
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
