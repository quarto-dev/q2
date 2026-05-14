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
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_hub::HubContext;
use quarto_hub::index::CaptureRef;
use quarto_hub::resource::create_binary_document;
use quarto_system_runtime::SystemRuntime;
use quarto_trace::EngineCapture;

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
) -> Result<usize, RecordError> {
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
) -> Result<bool, RecordError> {
    // Project discovery walks up for `_quarto.yml`; for single-file
    // projects this produces an is_single_file context.
    let project = ProjectContext::discover(abs_path, runtime.as_ref())
        .map_err(|e| RecordError::DiscoverFailed(format!("{}", e)))?;

    let capture = record_capture(abs_path, &project, runtime.clone(), engine_registry)
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

        let count = record_eager_captures(ctx, runtime, None).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn prose_only_doc_records_no_capture() {
        let (_tmp, ctx, runtime) =
            build_ctx_with_files(&[("doc.qmd", "---\ntitle: Prose\n---\n\nNo cells.\n")]).await;

        let count = record_eager_captures(ctx.clone(), runtime, None)
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

        let count = record_eager_captures(ctx.clone(), runtime, Some(make_registry()))
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

        let count = record_eager_captures(ctx.clone(), runtime, Some(make_registry()))
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

        let first = record_eager_captures(ctx.clone(), runtime.clone(), Some(make_registry()))
            .await
            .unwrap();
        let second = record_eager_captures(ctx.clone(), runtime, Some(make_registry()))
            .await
            .unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 0, "second run should be a no-op");
    }
}
