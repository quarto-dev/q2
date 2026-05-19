//! Engine capture driver for the q2 preview server (Phase C.1).
//!
//! Once a `HubContext` is constructed and its initial filesystem sync
//! has settled, this module walks the tracked `.qmd` files and — for
//! any file with code cells but no recorded capture yet — runs the
//! engine via [`quarto_core::engine::preview_record::record_capture`]
//! and stores the result.
//!
//! Storage shape:
//!   - The serialized [`EngineCapture`] is gzipped JSON and stored as a
//!     samod *binary* document (`{ content, mimeType, hash }` schema
//!     from `quarto_hub::resource::create_binary_document`). The MIME
//!     type `application/x-engine-capture+gzip` flags it for the
//!     browser-side reader (Phase C.4).
//!   - The binary doc's document ID is written to the IndexDocument's
//!     V2 capture sidecar via `IndexDocument::set_capture`. The SPA
//!     observes the sidecar through the sync client's
//!     `onCapturesChange` callback (landed in Phase C.3).
//!
//! Cap (per plan §C.1 Risk #2): documents are processed *sequentially*
//! so a project with five engine-bearing docs doesn't burst five
//! concurrent engine invocations on startup.

use std::io::Write;
use std::sync::Arc;

use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::compute_input_qmd;
use quarto_core::project::ProjectContext;
use quarto_hub::HubContext;
use quarto_hub::index::CaptureRef;
use quarto_hub::resource::create_binary_document;
use quarto_system_runtime::SystemRuntime;
use quarto_trace::EngineCapture;

use crate::cache::record_capture_cached;
use crate::config::EnginePolicy;

/// MIME type marker written into the capture binary doc so the
/// browser-side reader can verify it's looking at the right payload
/// shape (gzipped EngineCapture JSON) instead of an unrelated resource.
pub const CAPTURE_MIME_TYPE: &str = "application/x-engine-capture+gzip";

/// Walk the project's `.qmd` files and record engine captures for any
/// file that has code cells but no existing sidecar entry yet.
///
/// Returns the number of captures successfully written. Soft-fails on
/// per-file errors (logs and continues) — one bad document shouldn't
/// keep us from recording captures for the rest. Returns `Ok(0)` for
/// standalone-mode contexts (no project files).
///
/// Tests pass `engine_registry: Some(...)` to substitute a passthrough
/// engine for jupyter/knitr without a real runtime; production callers
/// pass `None` and use the default registry.
pub async fn record_eager_captures(
    ctx: Arc<HubContext>,
    runtime: Arc<dyn SystemRuntime>,
    engine_registry: Option<EngineRegistry>,
    policy: EnginePolicy,
    cache_dir: &std::path::Path,
) -> Result<usize, RecordError> {
    // C.6: `preview.engine: off` disables all engine execution, including
    // the eager run. Code cells render as inert source in the SPA.
    if policy == EnginePolicy::Off {
        tracing::debug!("eager capture driver skipped: engine policy = off");
        return Ok(0);
    }
    let Some(project_root) = ctx.storage().project_root().map(|p| p.to_path_buf()) else {
        // Standalone mode — nothing to record.
        return Ok(0);
    };
    // Touch project_files to bail early on a context that has the
    // project_root but no discovered files (a malformed state, but
    // defensive).
    if ctx.project_files().is_none() {
        return Ok(0);
    }

    // Snapshot the file list. The index map will be mutated (sidecar
    // writes) as we go; iterating a snapshot keeps the loop simple.
    let files = ctx.index().get_all_files();

    let mut recorded = 0_usize;
    for (rel_path, _doc_id) in files {
        // Sidecar is keyed by the same path as `files` — skip files
        // that already have a capture so re-runs are idempotent.
        if ctx.index().has_capture(&rel_path) {
            tracing::debug!(rel_path = %rel_path, "capture already recorded, skipping");
            continue;
        }

        // Resolve to an absolute path inside the project tree. The
        // IndexDocument stores forward-slash relative paths regardless
        // of platform; PathBuf::push handles platform-native joining.
        let abs_path = project_root.join(&rel_path);

        match record_one(
            &abs_path,
            &rel_path,
            &ctx,
            &runtime,
            engine_registry.clone(),
            cache_dir,
        )
        .await
        {
            Ok(true) => recorded += 1,
            Ok(false) => {} // no capture needed; not an error
            Err(e) => {
                tracing::warn!(
                    rel_path = %rel_path,
                    error = %e,
                    "failed to record engine capture; continuing",
                );
            }
        }
    }

    if recorded > 0 {
        tracing::info!(count = recorded, "recorded engine captures");
    }
    Ok(recorded)
}

/// Drive a single file's capture record. Returns `Ok(true)` if a
/// capture was written, `Ok(false)` if no capture was needed (prose-only
/// or default markdown engine), and `Err` for failures the caller
/// should log.
async fn record_one(
    abs_path: &std::path::Path,
    rel_path: &str,
    ctx: &Arc<HubContext>,
    runtime: &Arc<dyn SystemRuntime>,
    engine_registry: Option<EngineRegistry>,
    cache_dir: &std::path::Path,
) -> Result<bool, RecordError> {
    // Project discovery walks up for `_quarto.yml`; for single-file
    // projects this produces an is_single_file context.
    let project = ProjectContext::discover(abs_path, runtime.as_ref())
        .map_err(|e| RecordError::DiscoverFailed(format!("{}", e)))?;

    // C.7: route through the cache-aware wrapper so identical content
    // across (re-)opens hits the filesystem cache instead of re-running
    // the engine.
    let capture = record_capture_cached(
        cache_dir,
        abs_path,
        &project,
        runtime.clone(),
        engine_registry,
    )
    .await
    .map_err(|e| RecordError::RecordFailed(format!("{}", e)))?;

    let Some(capture) = capture else {
        return Ok(false);
    };

    let capture_doc_id = write_capture_doc(ctx, &capture).await?;

    let capture_ref = CaptureRef {
        capture_doc_id,
        staleness: Some(false),
        state: None,
        last_error: None,
    };
    ctx.index()
        .set_capture(rel_path, &capture_ref)
        .map_err(|e| RecordError::SidecarFailed(format!("{}", e)))?;

    tracing::info!(
        rel_path = %rel_path,
        engine = %capture.engine_name,
        "recorded engine capture",
    );
    Ok(true)
}

/// Recompute whether the active sidecar entry for `rel_path` is
/// stale (Phase C.2). Compares the file's current canonical QMD
/// against the recorded capture's `input_qmd` byte-for-byte (per
/// plan §Q-C3 v1 whole-QMD policy). On mismatch, flips
/// `CaptureRef.staleness` to `Some(true)`; on match, normalizes to
/// `Some(false)`.
///
/// No-op when:
///   - the path isn't in the project file index
///   - no sidecar entry exists for the path
///   - the recorded capture binary doc is missing or unparseable
///     (logged but not propagated — we don't want a corrupt cache
///     to spam the watcher with errors).
///
/// Returns `Ok(true)` when staleness flipped, `Ok(false)` when no
/// change was needed, and `Err` for fatal failures the caller
/// should log.
pub async fn recompute_staleness(
    ctx: Arc<HubContext>,
    runtime: Arc<dyn SystemRuntime>,
    rel_path: &str,
    policy: EnginePolicy,
    engine_registry: Option<EngineRegistry>,
    cache_dir: &std::path::Path,
) -> Result<bool, RecordError> {
    // C.6: `preview.engine: off` skips the watcher staleness hook
    // entirely (no captures exist anyway, but defensive).
    if policy == EnginePolicy::Off {
        return Ok(false);
    }
    let Some(project_root) = ctx.storage().project_root().map(|p| p.to_path_buf()) else {
        return Ok(false);
    };
    let Some(existing) = ctx.index().get_capture(rel_path) else {
        // No capture recorded for this file — nothing to invalidate.
        return Ok(false);
    };

    // Load and decode the existing capture so we can compare its
    // input_qmd to what the file would produce now.
    let recorded_input_qmd = match read_capture_from_doc(&ctx, &existing.capture_doc_id).await {
        Ok(cap) => cap.input_qmd,
        Err(e) => {
            tracing::warn!(
                rel_path = %rel_path,
                error = %e,
                "could not read recorded capture for staleness check",
            );
            return Ok(false);
        }
    };

    let abs_path = project_root.join(rel_path);
    let project = ProjectContext::discover(&abs_path, runtime.as_ref())
        .map_err(|e| RecordError::DiscoverFailed(format!("{}", e)))?;

    let current_input_qmd = compute_input_qmd(&abs_path, &project, runtime)
        .await
        .map_err(|e| RecordError::RecordFailed(format!("{}", e)))?;

    let is_stale = current_input_qmd != recorded_input_qmd.as_bytes();
    let target_value = Some(is_stale);
    if existing.staleness == target_value {
        return Ok(false);
    }

    let updated = CaptureRef {
        capture_doc_id: existing.capture_doc_id.clone(),
        staleness: target_value,
        state: existing.state,
        last_error: existing.last_error.clone(),
    };
    ctx.index()
        .set_capture(rel_path, &updated)
        .map_err(|e| RecordError::SidecarFailed(format!("{}", e)))?;

    tracing::debug!(
        rel_path = %rel_path,
        staleness = is_stale,
        "updated sidecar staleness",
    );

    // C.6 Auto policy: when staleness just flipped to true, kick off a
    // re-execute synchronously so the SPA sees a fresh capture without
    // requiring the user to click the overlay. We reuse the same path
    // the HTTP /api/preview/re-execute handler takes, including its
    // in-flight guard (so a debounced flurry of file changes is
    // collapsed to one engine run at a time per doc).
    if policy == EnginePolicy::Auto && is_stale {
        crate::re_execute::trigger_auto_re_execute(
            ctx.clone(),
            rel_path.to_string(),
            engine_registry,
            cache_dir.to_path_buf(),
        );
    }

    Ok(true)
}

/// Serialize + gzip + store the EngineCapture as a samod binary doc.
/// Returns the new doc's stringified DocumentId for use in the sidecar
/// `captureDocId` field.
async fn write_capture_doc(
    ctx: &Arc<HubContext>,
    capture: &EngineCapture,
) -> Result<String, RecordError> {
    let json = serde_json::to_vec(capture).map_err(|e| RecordError::Serialize(format!("{}", e)))?;
    let gzipped = gzip_bytes(&json).map_err(|e| RecordError::Gzip(format!("{}", e)))?;

    let automerge_doc = create_binary_document(&gzipped, CAPTURE_MIME_TYPE)
        .map_err(|e| RecordError::CreateBinaryDoc(format!("{}", e)))?;

    let handle = ctx
        .repo()
        .create(automerge_doc)
        .await
        .map_err(|_stopped| RecordError::RepoStopped)?;

    Ok(handle.document_id().to_string())
}

fn gzip_bytes(input: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(input)?;
    enc.finish()
}

/// Errors the driver can surface. Each variant is a short message
/// suitable for `tracing::warn!` — the calling loop catches the
/// failure, logs, and moves on to the next file. We deliberately
/// don't propagate `PipelineError` shapes here because the calling
/// surface (file-watcher hook, on-ready callback) treats all
/// per-file failures uniformly.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("project discovery failed: {0}")]
    DiscoverFailed(String),
    #[error("engine capture pipeline failed: {0}")]
    RecordFailed(String),
    #[error("serializing capture to JSON failed: {0}")]
    Serialize(String),
    #[error("gzipping capture failed: {0}")]
    Gzip(String),
    #[error("creating capture binary doc failed: {0}")]
    CreateBinaryDoc(String),
    #[error("samod repo is stopped")]
    RepoStopped,
    #[error("writing sidecar entry failed: {0}")]
    SidecarFailed(String),
}

/// Resolve the captured EngineCapture out of a binary doc by its
/// document ID. The on-the-wire format (gzipped JSON of
/// [`EngineCapture`] inside a `quarto_hub::resource::create_binary_document`
/// envelope) is shared with Phase C.4's WASM/SPA reader, so keeping
/// the Rust reader public lets test code and any future server-side
/// inspector use the same path.
pub async fn read_capture_from_doc(
    ctx: &Arc<HubContext>,
    capture_doc_id: &str,
) -> Result<EngineCapture, String> {
    use std::io::Read;
    use std::str::FromStr;

    use automerge::ROOT;
    use automerge::ReadDoc;
    use samod::DocumentId;

    let id = DocumentId::from_str(capture_doc_id).map_err(|e| format!("invalid doc id: {}", e))?;
    let handle = ctx
        .repo()
        .find(id)
        .await
        .map_err(|_stopped| "repo stopped".to_string())?
        .ok_or_else(|| "capture doc not found".to_string())?;

    let gzipped = handle.with_document(|doc| {
        doc.get(ROOT, "content")
            .ok()
            .flatten()
            .and_then(|(v, _)| v.into_bytes().ok().map(|b| b.to_vec()))
            .ok_or_else(|| "binary doc missing 'content' field".to_string())
    })?;

    let mut decoder = flate2::read::GzDecoder::new(&gzipped[..]);
    let mut json_bytes = Vec::new();
    decoder
        .read_to_end(&mut json_bytes)
        .map_err(|e| format!("gunzip failed: {}", e))?;
    serde_json::from_slice(&json_bytes).map_err(|e| format!("JSON parse failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    use quarto_core::engine::{ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError};
    use quarto_hub::context::HubConfig;
    use quarto_hub::storage::StorageManager;
    use quarto_system_runtime::NativeRuntime;
    use tempfile::TempDir;

    /// Passthrough engine reporting as a non-markdown name so it
    /// bypasses EngineExecutionStage's `name() == "markdown"`
    /// short-circuit and triggers capture emission.
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

    fn make_registry() -> EngineRegistry {
        let mut reg = EngineRegistry::new();
        reg.register(Arc::new(PassthroughTestEngine));
        reg
    }

    /// Each test invocation gets a process-unique cache directory so
    /// cross-test pollution can't accidentally turn a cache miss into
    /// a hit (or vice versa). The OS cleans up on process exit; we
    /// don't bother with TempDir RAII because the driver tests don't
    /// depend on the dir being gone before they finish.
    fn cache_dir_for_test() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let mut dir = std::env::temp_dir();
        dir.push(format!("q2-c7-driver-test-{pid}-{id}"));
        dir
    }

    async fn build_ctx_with_files(
        files: &[(&str, &str)],
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        let project = TempDir::with_prefix("c1-driver-test-").unwrap();
        for (rel, content) in files {
            let path = project.path().join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
        let storage = StorageManager::new(project.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        (project, ctx, runtime)
    }

    #[tokio::test]
    async fn standalone_mode_records_nothing() {
        let temp = TempDir::with_prefix("c1-standalone-").unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        let count = record_eager_captures(
            ctx,
            runtime,
            None,
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn prose_only_doc_records_no_capture() {
        let (_tmp, ctx, runtime) =
            build_ctx_with_files(&[("doc.qmd", "---\ntitle: Prose\n---\n\nNo cells.\n")]).await;

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            None,
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(count, 0);
        assert!(!ctx.index().has_capture("doc.qmd"));
    }

    #[tokio::test]
    async fn doc_with_passthrough_engine_records_capture() {
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n42\n```\n",
        )])
        .await;

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(count, 1);
        assert!(ctx.index().has_capture("doc.qmd"));

        let entry = ctx.index().get_capture("doc.qmd").expect("sidecar entry");
        assert!(
            !entry.capture_doc_id.is_empty(),
            "captureDocId should be set"
        );
        assert_eq!(entry.staleness, Some(false));
    }

    #[tokio::test]
    async fn capture_binary_doc_round_trips_through_samod() {
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nSENTINEL\n```\n",
        )])
        .await;

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(count, 1);

        let entry = ctx.index().get_capture("doc.qmd").unwrap();
        let capture = read_capture_from_doc(&ctx, &entry.capture_doc_id)
            .await
            .expect("capture doc round-trips");

        assert_eq!(capture.engine_name, "test-passthrough");
        assert!(
            capture.input_qmd.contains("SENTINEL"),
            "input_qmd should preserve the cell body; got: {}",
            capture.input_qmd
        );
        let markdown = capture
            .result
            .get("markdown")
            .and_then(|v| v.as_str())
            .expect("result.markdown");
        assert!(markdown.contains("<!-- test-passthrough -->"));
    }

    #[tokio::test]
    async fn re_running_driver_is_idempotent() {
        // The first run records a capture; the second run sees the
        // sidecar entry and skips the file, returning 0.
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n",
        )])
        .await;

        let first = record_eager_captures(
            ctx.clone(),
            runtime.clone(),
            Some(make_registry()),
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        let second = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 0, "second run should be a no-op");
    }

    // ──────────────────────────────────────────────────────────────
    // Phase C.2: recompute_staleness
    // ──────────────────────────────────────────────────────────────

    /// Helper: write fixture content to the file backing rel_path,
    /// then return paths + ctx. The path argument is relative to the
    /// project root managed by the HubContext.
    async fn build_ctx_record_then_return_root(
        rel: &str,
        content: &str,
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        let (tmp, ctx, runtime) = build_ctx_with_files(&[(rel, content)]).await;
        let count = record_eager_captures(
            ctx.clone(),
            runtime.clone(),
            Some(make_registry()),
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(count, 1, "fixture must produce a capture");
        (tmp, ctx, runtime)
    }

    #[tokio::test]
    async fn recompute_staleness_marks_stale_when_cell_body_changes() {
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .await;
        // Sanity: initial recompute against unchanged content is a no-op.
        assert!(
            !recompute_staleness(
                ctx.clone(),
                runtime.clone(),
                "doc.qmd",
                EnginePolicy::Manual,
                None,
                &cache_dir_for_test()
            )
            .await
            .unwrap(),
            "unchanged content should not flip staleness"
        );
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false)
        );

        // Edit the cell body on disk — same engine, different cell.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();

        let flipped = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            EnginePolicy::Manual,
            None,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert!(flipped, "recompute should report a flip");
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(true),
            "sidecar should now say staleness: true"
        );
    }

    #[tokio::test]
    async fn recompute_staleness_clears_when_content_reverts() {
        // After staleness is set, restoring the original content
        // must flip staleness back to false (so users can recover by
        // undoing). The recompute is idempotent.
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .await;

        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();
        let _ = recompute_staleness(
            ctx.clone(),
            runtime.clone(),
            "doc.qmd",
            EnginePolicy::Manual,
            None,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(true)
        );

        // Revert.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .unwrap();
        let _ = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            EnginePolicy::Manual,
            None,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false),
            "reverting content should clear staleness"
        );
    }

    #[tokio::test]
    async fn recompute_staleness_also_flips_for_prose_only_edits_v1_limitation() {
        // Plan §Q-C3: v1 whole-QMD byte-equality means prose-only
        // edits also flip the staleness flag. Documented limitation;
        // test pins the v1 behaviour.
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\nA paragraph.\n\n```{test-passthrough}\nx\n```\n",
        )
        .await;

        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\nA different paragraph.\n\n```{test-passthrough}\nx\n```\n",
        )
        .unwrap();
        recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            EnginePolicy::Manual,
            None,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(true),
            "prose-only edit ALSO flips staleness in v1 (known limitation per §Q-C3)"
        );
    }

    #[tokio::test]
    async fn recompute_staleness_noop_when_no_capture() {
        // A path without a sidecar entry (e.g. a fresh doc that
        // never had a capture recorded) is a no-op — recompute
        // should return Ok(false) and not error.
        let (_tmp, ctx, runtime) =
            build_ctx_with_files(&[("doc.qmd", "---\ntitle: Prose\n---\n\nhi\n")]).await;

        let result = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            EnginePolicy::Manual,
            None,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert!(!result);
        assert!(ctx.index().get_capture("doc.qmd").is_none());
    }

    #[tokio::test]
    async fn recompute_staleness_noop_in_standalone_mode() {
        // Standalone hub (no project root) → nothing to recompute.
        let temp = TempDir::with_prefix("c2-staleness-standalone-").unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

        let result = recompute_staleness(
            ctx,
            runtime,
            "anything.qmd",
            EnginePolicy::Manual,
            None,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert!(!result);
    }

    // ──────────────────────────────────────────────────────────────
    // Phase C.6: preview.engine policy (Off / Auto)
    // ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn off_policy_skips_eager_capture_for_doc_with_code_cells() {
        // §C.6 acceptance: "off skips eager + suppresses code-cell exec."
        // A doc with a code cell that would otherwise produce a capture
        // (covered by `doc_with_passthrough_engine_records_capture`)
        // must produce zero captures under `Off`.
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n42\n```\n",
        )])
        .await;

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            EnginePolicy::Off,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(count, 0, "Off policy must skip eager capture");
        assert!(
            !ctx.index().has_capture("doc.qmd"),
            "Off policy must leave the sidecar untouched"
        );
    }

    #[tokio::test]
    async fn off_policy_makes_recompute_staleness_a_noop_even_with_capture() {
        // §C.6 acceptance: under Off, the file-watcher staleness hook
        // does nothing — even if a stale-looking capture exists from a
        // prior session.
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .await;
        // Edit the cell body — under Manual this would flip staleness.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();

        let flipped = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            EnginePolicy::Off,
            None,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert!(!flipped, "Off policy must not flip staleness");
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false),
            "Off policy must leave the existing staleness flag alone"
        );
    }

    #[tokio::test]
    async fn auto_policy_marks_stale_and_triggers_re_execute() {
        // §C.6 acceptance: "auto re-executes on every code-cell change
        // without the overlay." We can't easily black-box assert "the
        // SPA never showed the overlay" in a unit test, but we can
        // assert that recompute_staleness under Auto:
        //   (a) DOES detect the staleness flip (the SPA's invariant);
        //   (b) The capture is eventually replaced (state: idle, new
        //       captureDocId) without an HTTP call.
        crate::re_execute::reset_in_flight_for_tests();
        let (tmp, ctx, runtime) = build_ctx_record_then_return_root(
            "doc.qmd",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n",
        )
        .await;
        let initial_doc_id = ctx
            .index()
            .get_capture("doc.qmd")
            .unwrap()
            .capture_doc_id
            .clone();

        // Edit the cell body so the next recompute sees a mismatch.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();

        let flipped = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            EnginePolicy::Auto,
            Some(make_registry()),
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert!(flipped, "Auto must still report the staleness flip");

        // Auto kicked off a spawn_blocking worker; poll for the new
        // capture (state: idle, fresh captureDocId, staleness: false).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let entry = ctx.index().get_capture("doc.qmd").unwrap();
            if entry.state == Some(quarto_hub::index::CaptureState::Idle)
                && entry.capture_doc_id != initial_doc_id
            {
                assert_eq!(
                    entry.staleness,
                    Some(false),
                    "Auto re-execute must clear the staleness flag"
                );
                break;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "Auto re-execute did not produce a new capture within 10s; entry = {:?}",
                    entry
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}
