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

use std::sync::Arc;

use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::{CaptureRun, compute_input_qmd, compute_source_inputs};
use quarto_core::pandoc_request::captures::{CapturePayload, parse_capture_payload};
use quarto_core::project::ProjectContext;
use quarto_core::record::{Record, record_is_stale};
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_hub::HubContext;
use quarto_hub::index::CaptureRef;
use quarto_system_runtime::SystemRuntime;

use crate::cache::record_capture_run_cached;
use crate::config::EnginePolicy;
use crate::diagnostics;

/// MIME type marker written into the capture binary doc so the
/// browser-side reader can verify it's looking at the right payload
/// shape (gzipped EngineCapture JSON) instead of an unrelated resource.
/// Re-exported from quarto-hub, the single source of truth
/// (bd-eiku4ymo).
pub use quarto_hub::resource::CAPTURE_MIME_TYPE;

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
    engine_registry: Option<Arc<EngineRegistry>>,
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
    // Walk the project's `.qmd` files only — engines run against qmd
    // input. Iterating the index map (which also covers `_quarto.yml`,
    // images, etc.) would feed non-qmd bytes into parse-document; that
    // panics on the first binary file (bd-i6jy4 / bd-6qbto).
    let Some(project_files) = ctx.project_files() else {
        return Ok(0);
    };

    // Snapshot the list. The index map will be mutated (sidecar
    // writes) as we go; iterating a snapshot keeps the loop simple.
    let qmd_paths: Vec<String> = project_files
        .qmd_files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    let mut recorded = 0_usize;
    for rel_path in qmd_paths {
        // Sidecar is keyed by the same path as the index — skip files
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
                // bd-b9kzg: also surface the failure to the SPA via
                // the per-page diagnostic sink. `emit` calls
                // `tracing::warn!` itself, so this is a clean
                // replacement (no double-logging). The sink is only
                // populated when the OnceLock is set — i.e. when
                // run_with_on_ready bootstrapped it. Unit tests
                // that call `record_eager_captures` directly without
                // booting a server get a silent no-op.
                if let Some(sink) = diagnostics::current_sink() {
                    sink.emit(
                        &rel_path,
                        DiagnosticMessageBuilder::warning("Engine capture failed")
                            .with_code("Q-PREVIEW-CAP-1")
                            .problem(format!("{e}"))
                            .build(),
                    );
                } else {
                    tracing::warn!(
                        rel_path = %rel_path,
                        error = %e,
                        "failed to record engine capture; continuing",
                    );
                }
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
    engine_registry: Option<Arc<EngineRegistry>>,
    cache_dir: &std::path::Path,
) -> Result<bool, RecordError> {
    // Project discovery walks up for `_quarto.yml`; for single-file
    // projects this produces an is_single_file context.
    let project = ProjectContext::discover(abs_path, runtime.as_ref())
        .map_err(|e| RecordError::DiscoverFailed(format!("{}", e)))?;

    // C.7: route through the cache-aware wrapper so identical content
    // across (re-)opens hits the filesystem cache instead of re-running
    // the engine.
    let run = record_capture_run_cached(
        cache_dir,
        abs_path,
        &project,
        runtime.clone(),
        engine_registry,
    )
    .await
    .map_err(|e| RecordError::RecordFailed(format!("{}", e)))?;

    if run.captures.is_empty() {
        return Ok(false);
    }

    let capture_doc_id = write_capture_doc(ctx, rel_path, &run).await?;

    let capture_ref = CaptureRef {
        capture_doc_id,
        staleness: Some(false),
        state: None,
        last_error: None,
    };
    ctx.index()
        .set_capture(rel_path, &capture_ref)
        .map_err(|e| RecordError::SidecarFailed(format!("{}", e)))?;

    let engines = run
        .captures
        .iter()
        .map(|c| c.engine_name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    tracing::info!(
        rel_path = %rel_path,
        engines = %engines,
        "recorded engine capture(s)",
    );
    Ok(true)
}

/// What the sidecar's capture doc held, as far as staleness is concerned.
enum Recorded {
    /// The first engine's `input_qmd`, the legacy byte-compare key (bd-5yff4).
    Legacy(String),
    Record(Arc<Record>),
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
    engine_registry: Option<Arc<EngineRegistry>>,
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

    // Load and decode the existing capture sequence so we can compare
    // its input_qmd to what the file would produce now. bd-5yff4: the
    // staleness key is the *first* engine's input_qmd — the document's
    // canonical pre-engine QMD. Each later engine's input is a
    // deterministic function of the prior engine's output, so an
    // unchanged first input means the whole recorded sequence is still
    // valid.
    let recorded = match read_capture_from_doc(&ctx, &existing.capture_doc_id).await {
        Ok(CapturePayload::Legacy(captures)) => match captures.into_iter().next() {
            Some(first) => Recorded::Legacy(first.input_qmd),
            None => {
                tracing::warn!(
                    rel_path = %rel_path,
                    "recorded capture doc was empty; skipping staleness check",
                );
                return Ok(false);
            }
        },
        Ok(CapturePayload::Empty) => {
            tracing::warn!(
                rel_path = %rel_path,
                "recorded capture doc was empty; skipping staleness check",
            );
            return Ok(false);
        }
        Ok(CapturePayload::Record(record)) => Recorded::Record(record),
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

    let is_stale = match recorded {
        // Legacy payloads keep the byte compare: any prose edit flips it.
        Recorded::Legacy(recorded_input_qmd) => {
            let current_input_qmd = compute_input_qmd(&abs_path, &project, runtime)
                .await
                .map_err(|e| RecordError::RecordFailed(format!("{}", e)))?;
            current_input_qmd != recorded_input_qmd.as_bytes()
        }
        // Records compare the per-engine document hashes, recomputed from the record's
        // stored declarations on the masked, overlay-free pre-engine document (EXR-15,
        // decision 5). No engine is loaded.
        Recorded::Record(record) => {
            let source = compute_source_inputs(&abs_path, &project, runtime)
                .await
                .map_err(|e| RecordError::RecordFailed(format!("{}", e)))?;
            let input_qmd = String::from_utf8_lossy(&source.input_qmd);
            match record_is_stale(
                &record,
                &input_qmd,
                &source.sugared_blocks,
                &source.metadata,
            ) {
                Ok(stale) => stale,
                Err(e) => {
                    tracing::warn!(
                        rel_path = %rel_path,
                        error = %e,
                        "could not apply the record's hash declarations; skipping staleness check",
                    );
                    return Ok(false);
                }
            }
        }
    };
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

/// Encode and store a capture run as a samod binary doc: a record doc when the
/// writer gate (`record_writing_enabled`) is on and the run has a record, else
/// the legacy `EngineCapture` array (bd-5yff4: one per engine). Returns the new
/// doc's stringified DocumentId for the sidecar `captureDocId` field. Shared by
/// the eager driver and the re-execute handler (EXR-13b).
///
/// `rel_path` is the project-relative source path, stamped (with the
/// engine names) into the doc's uncompressed `meta` audit map
/// (bd-eiku4ymo) so `hub admin scan` can read provenance without
/// gunzipping.
pub async fn write_capture_doc(
    ctx: &Arc<HubContext>,
    rel_path: &str,
    run: &CaptureRun,
) -> Result<String, RecordError> {
    write_capture_doc_gated(
        ctx,
        rel_path,
        run,
        quarto_core::engine::capture_files::record_writing_enabled(),
    )
    .await
}

/// [`write_capture_doc`] with the gate passed explicitly (test seam).
pub async fn write_capture_doc_gated(
    ctx: &Arc<HubContext>,
    rel_path: &str,
    run: &CaptureRun,
    record_gate: bool,
) -> Result<String, RecordError> {
    let encoded = quarto_core::engine::capture_files::encode_capture_payload(
        &run.captures,
        run.record.as_ref(),
        record_gate,
    )
    .map_err(|e| RecordError::Gzip(format!("{}", e)))?;

    let meta = quarto_hub::resource::CaptureDocMeta {
        source_path: rel_path.to_string(),
        engines: encoded.engines,
    };
    let automerge_doc = quarto_hub::resource::create_engine_payload_document(
        encoded.is_record,
        &encoded.gzipped,
        &meta,
    )
    .map_err(|e| RecordError::CreateBinaryDoc(format!("{}", e)))?;

    let handle = ctx
        .repo()
        .create(automerge_doc)
        .await
        .map_err(|_stopped| RecordError::RepoStopped)?;

    Ok(handle.document_id().to_string())
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
    #[error("serializing/gzipping capture failed: {0}")]
    Gzip(String),
    #[error("creating capture binary doc failed: {0}")]
    CreateBinaryDoc(String),
    #[error("samod repo is stopped")]
    RepoStopped,
    #[error("writing sidecar entry failed: {0}")]
    SidecarFailed(String),
}

/// Resolve the capture payload out of a binary doc by its document ID: a
/// legacy `EngineCapture[]` (bd-5yff4: one per engine) or an engine execution
/// record (EXR-13). The on-the-wire format (gzipped JSON inside a
/// `quarto_hub::resource::create_binary_document` envelope) is shared with
/// Phase C.4's WASM/SPA reader, so keeping the Rust reader public lets test
/// code and any future server-side inspector use the same path.
///
/// This fetches `content` and calls
/// [`parse_capture_payload`](quarto_core::pandoc_request::captures::parse_capture_payload),
/// the one reader that decides the shape (it never looks at the MIME).
pub async fn read_capture_from_doc(
    ctx: &Arc<HubContext>,
    capture_doc_id: &str,
) -> Result<CapturePayload, String> {
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
            .and_then(|(v, _)| v.into_bytes().ok())
            .ok_or_else(|| "binary doc missing 'content' field".to_string())
    })?;

    parse_capture_payload(Some(&gzipped), capture_doc_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EXR-13b writer gate (shared by the eager driver and the re-execute
    /// handler): gate off writes a legacy capture doc even when the run carries a
    /// record; gate on writes a record doc; gate on with no record (a cache hit)
    /// falls back to legacy.
    async fn written_mime_and_kind(gate: bool, with_record: bool) -> (String, String) {
        use automerge::ReadDoc;
        let temp = TempDir::new().unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let record = with_record.then(|| {
            quarto_core::record::read_record(include_str!(
                "../../quarto-core/tests/fixtures/records/basic.json"
            ))
            .unwrap()
        });
        let run = CaptureRun {
            captures: Vec::new(),
            record,
        };
        let id = write_capture_doc_gated(&ctx, "doc.qmd", &run, gate)
            .await
            .unwrap();
        let handle = ctx
            .repo()
            .find(id.parse().unwrap())
            .await
            .unwrap()
            .expect("doc exists");
        handle.with_document(|doc| {
            let mime = doc
                .get(automerge::ROOT, "mimeType")
                .unwrap()
                .and_then(|(v, _)| v.into_string().ok())
                .unwrap_or_default();
            let kind = quarto_hub::resource::read_capture_meta(doc)
                .and_then(|m| m.kind)
                .unwrap_or_default();
            (mime, kind)
        })
    }

    #[tokio::test]
    async fn writer_gate_off_writes_a_legacy_capture_doc() {
        let (mime, kind) = written_mime_and_kind(false, true).await;
        assert_eq!(mime, quarto_hub::resource::CAPTURE_MIME_TYPE);
        assert_eq!(kind, "engine-capture");
    }

    #[tokio::test]
    async fn writer_gate_on_writes_a_record_doc() {
        let (mime, kind) = written_mime_and_kind(true, true).await;
        assert_eq!(mime, quarto_hub::resource::RECORD_MIME_TYPE);
        assert_eq!(kind, "engine-record");
    }

    /// EXR-15: the record doc's `meta.engines` is the record's engines list, in order, and the
    /// payload parses back to an equal record.
    #[tokio::test]
    async fn record_doc_meta_and_payload_match_the_record() {
        let temp = TempDir::new().unwrap();
        let storage = StorageManager::new_standalone(temp.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let record = quarto_core::record::read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap();
        let expected: Vec<String> = record
            .metadata
            .quarto_engines
            .iter()
            .map(|e| e.name.clone())
            .collect();
        let run = CaptureRun {
            captures: Vec::new(),
            record: Some(record.clone()),
        };
        let id = write_capture_doc_gated(&ctx, "doc.qmd", &run, true)
            .await
            .unwrap();
        let handle = ctx
            .repo()
            .find(id.parse().unwrap())
            .await
            .unwrap()
            .expect("doc exists");
        let engines = handle.with_document(|doc| {
            quarto_hub::resource::read_capture_meta(doc)
                .map(|m| m.engines)
                .unwrap_or_default()
        });
        assert_eq!(engines, expected);
        let CapturePayload::Record(back) = read_capture_from_doc(&ctx, &id).await.unwrap() else {
            panic!("expected a record payload");
        };
        assert_eq!(*back, record);
    }

    #[tokio::test]
    async fn writer_gate_on_without_a_record_writes_legacy() {
        let (mime, _) = written_mime_and_kind(true, false).await;
        assert_eq!(mime, quarto_hub::resource::CAPTURE_MIME_TYPE);
    }

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
        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> quarto_core::engine::LanguageClaim {
            if language == "test-passthrough" {
                quarto_core::engine::LanguageClaim::Primary(1)
            } else {
                quarto_core::engine::LanguageClaim::None
            }
        }
    }

    fn make_registry() -> Arc<EngineRegistry> {
        let mut reg = EngineRegistry::new();
        reg.register(Arc::new(PassthroughTestEngine));
        Arc::new(reg)
    }

    // ──────────────────────────────────────────────────────────────
    // R5 seam tests (P2-14 / P2-15): the preview capture path runs a
    // real *extension* engine (the Task-13 echo TS engine) because
    // record_capture uses the DISCOVERED project.registry (Task 8),
    // not built-ins. Node-gated — the echo engine spawns a Node
    // engine-host subprocess (see crates/quarto-core tests/integration
    // /echo_engine_e2e.rs).
    // ──────────────────────────────────────────────────────────────

    use quarto_core::engine::ts_process::node_available_for_tests;

    /// Recursively copy `src` into `dst` (dst is created).
    fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let from = entry.path();
            let to = dst.join(entry.file_name());
            if from.is_dir() {
                copy_dir(&from, &to);
            } else {
                std::fs::copy(&from, &to).unwrap();
            }
        }
    }

    /// Absolute path to the committed echo-engine fixture extension
    /// (lives under the sibling `quarto-core` crate's test fixtures).
    fn echo_engine_fixture_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../quarto-core/tests/fixtures/extensions/echo-engine")
    }

    /// Build a HubContext over a temp project that has the echo
    /// extension installed under `_extensions/echo-engine/` and a single
    /// `.qmd` at `rel` with `content`. `ProjectContext::discover` (run
    /// inside `record_one` / `recompute_staleness`) walks up to
    /// `_extensions/` and builds the echo engine into `project.registry`.
    async fn build_ctx_with_echo_extension(
        rel: &str,
        content: &str,
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        let project = TempDir::with_prefix("r5-echo-preview-").unwrap();
        let echo_dst = project.path().join("_extensions").join("echo-engine");
        copy_dir(&echo_engine_fixture_dir(), &echo_dst);
        // The committed echo-engine bundle is deleted (plan1c3: hermetic fixtures
        // are regenerated at test time). These tests execute the engine, so
        // regenerate the real bundle in-place via the same public build lib the
        // quarto-core suites use (the embedded rollup driver, run under Node). Mirrors
        // `engine_fixture_build::build_bundle` — the `../../resources/...` depth is
        // identical because quarto-preview and quarto-core are both one level under
        // `crates/`.
        quarto_core::extension::build::build_ts_extension(
            quarto_core::extension::build::BuildOptions {
                ext_dir: Some(echo_dst.clone()),
                config: None,
                workspace: false,
                workspace_root: Some(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../..")
                        .canonicalize()
                        .expect("workspace root"),
                ),
            },
        )
        .expect("regenerate echo-engine bundle for preview capture test");
        let doc_path = project.path().join(rel);
        if let Some(parent) = doc_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&doc_path, content).unwrap();

        let storage = StorageManager::new(project.path()).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        (project, ctx, runtime)
    }

    /// P2-14 — the EAGER preview capture path runs the echo *extension*
    /// engine server-side. Production passes `engine_registry: None`, so
    /// `record_capture` uses the discovered `project.registry` (which,
    /// post-Task-7b, contains the echo engine built from the installed
    /// `_extensions/echo-engine/`). The recorded capture's result
    /// markdown must contain the engine's `ECHO_EXECUTED` sentinel.
    ///
    /// Fail-on-revert (recorded in the Task-15 report): overwrite
    /// `ctx.registry` with a built-ins `EngineRegistry::new()` right
    /// after `StageContext::new()` in `record_capture` ⇒ echo absent ⇒
    /// `ECHO_EXECUTED` missing ⇒ RED. This binds "the eager preview
    /// capture uses the project registry (with extension engines)".
    #[tokio::test]
    async fn p2_14_eager_capture_runs_extension_engine() {
        if !node_available_for_tests() {
            eprintln!("SKIP: node not found — p2_14_eager_capture_runs_extension_engine");
            return;
        }
        let (_tmp, ctx, runtime) = build_ctx_with_echo_extension(
            "doc.qmd",
            "---\ntitle: Echo Preview\n---\n\n```{echo}\nHello from preview!\n```\n",
        )
        .await;

        // engine_registry = None ⇒ the production path: the discovered
        // project.registry (with the echo extension engine) is used.
        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            None,
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(count, 1, "echo doc should produce exactly one capture");

        let entry = ctx.index().get_capture("doc.qmd").expect("sidecar entry");
        let captures = read_capture_from_doc(&ctx, &entry.capture_doc_id)
            .await
            .expect("capture doc round-trips")
            .into_captures();
        assert_eq!(captures.len(), 1);
        assert_eq!(
            captures[0].engine_name, "echo",
            "the extension engine (echo), not built-in markdown, must own the capture"
        );
        let markdown = captures[0]
            .result
            .get("markdown")
            .and_then(|v| v.as_str())
            .expect("capture result.markdown");
        assert!(
            markdown.contains("ECHO_EXECUTED"),
            "eager preview capture must run the echo extension engine server-side \
             (ECHO_EXECUTED sentinel); got: {markdown}"
        );
    }

    /// P2-15 — the echo extension engine survives an ON-EDIT
    /// re-execution (capture sites 2/3: `recompute_staleness` Auto →
    /// `trigger_auto_re_execute` → `perform_re_execute`). After the
    /// eager capture, a changed cell body forces a cache miss and a
    /// genuinely fresh capture; the new capture must STILL contain
    /// `ECHO_EXECUTED`, and — the vacuity guard — must reflect the
    /// edited body (`SECOND`) with a new `captureDocId`, proving a REAL
    /// re-execute happened rather than a re-assertion of the P2-14
    /// eager capture.
    ///
    /// RED (fail-on-revert): if sites 2/3 fell back to a built-ins
    /// registry, the echo engine would vanish on edit ⇒ the
    /// post-re-execute `ECHO_EXECUTED` assertion goes RED.
    #[tokio::test]
    async fn p2_15_on_edit_re_execution_keeps_extension_engine() {
        if !node_available_for_tests() {
            eprintln!("SKIP: node not found — p2_15_on_edit_re_execution_keeps_extension_engine");
            return;
        }
        crate::re_execute::reset_in_flight_for_tests();

        // One shared cache dir across eager + re-execute so the cache is
        // real: the edited body must MISS it (proving a genuine re-run),
        // not silently reuse the eager capture.
        let cache_dir = cache_dir_for_test();

        let (tmp, ctx, runtime) = build_ctx_with_echo_extension(
            "doc.qmd",
            "---\ntitle: Echo Preview\n---\n\n```{echo}\nFIRST\n```\n",
        )
        .await;

        // Eager capture (production path, None registry) seeds the sidecar.
        let count = record_eager_captures(
            ctx.clone(),
            runtime.clone(),
            None,
            EnginePolicy::Manual,
            &cache_dir,
        )
        .await
        .unwrap();
        assert_eq!(count, 1, "eager echo capture must seed the sidecar");
        let initial_doc_id = ctx
            .index()
            .get_capture("doc.qmd")
            .unwrap()
            .capture_doc_id
            .clone();

        // Edit the cell body ⇒ different canonical QMD ⇒ cache miss ⇒
        // real re-execute.
        std::fs::write(
            tmp.path().join("doc.qmd"),
            "---\ntitle: Echo Preview\n---\n\n```{echo}\nSECOND\n```\n",
        )
        .unwrap();

        let flipped = recompute_staleness(
            ctx.clone(),
            runtime,
            "doc.qmd",
            EnginePolicy::Auto,
            None,
            &cache_dir,
        )
        .await
        .unwrap();
        assert!(flipped, "editing the cell must flip staleness");

        // Auto spawned a blocking re-execute worker; poll for the fresh
        // capture (state: idle, new captureDocId).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let new_entry = loop {
            let entry = ctx.index().get_capture("doc.qmd").unwrap();
            if entry.state == Some(quarto_hub::index::CaptureState::Idle)
                && entry.capture_doc_id != initial_doc_id
            {
                break entry;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "Auto re-execute did not produce a new capture within 30s; entry = {entry:?}"
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        assert_eq!(
            new_entry.staleness,
            Some(false),
            "Auto re-execute must clear staleness"
        );

        let captures = read_capture_from_doc(&ctx, &new_entry.capture_doc_id)
            .await
            .expect("re-executed capture doc round-trips")
            .into_captures();
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].engine_name, "echo");
        // Vacuity guard: the re-executed capture reflects the EDITED body
        // — a genuine cache-miss re-run, not the eager capture replayed.
        assert!(
            captures[0].input_qmd.contains("SECOND"),
            "re-executed capture must reflect the edited cell body (SECOND); got: {}",
            captures[0].input_qmd
        );
        let markdown = captures[0]
            .result
            .get("markdown")
            .and_then(|v| v.as_str())
            .expect("capture result.markdown");
        assert!(
            markdown.contains("ECHO_EXECUTED"),
            "the echo extension engine must STILL run after an on-edit re-execute \
             (sites 2/3 use project.registry); got: {markdown}"
        );
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
        build_ctx_with_text_and_binary_files(files, &[]).await
    }

    async fn build_ctx_with_text_and_binary_files(
        text_files: &[(&str, &str)],
        binary_files: &[(&str, &[u8])],
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        let project = TempDir::with_prefix("c1-driver-test-").unwrap();
        for (rel, content) in text_files {
            let path = project.path().join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
        for (rel, bytes) in binary_files {
            let path = project.path().join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, bytes).unwrap();
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

    /// Regression test for bd-i6jy4: the eager-capture driver must
    /// only iterate `.qmd` files. Before the fix it iterated every
    /// entry in the index — including binaries — and fed their bytes
    /// to the parse-document stage, which panicked on the first
    /// non-UTF-8 byte (sibling bd-6qbto).
    ///
    /// The test puts *valid-qmd-with-engine-cells* content into a
    /// `.png` file so that, on the unfixed driver, the passthrough
    /// engine would actually run and a capture would be recorded for
    /// `logo.png`. The fix makes discovery's binary classification
    /// authoritative: `.png` lives in `binary_files`, never reaches
    /// `record_one`, and `has_capture("logo.png")` stays false.
    #[tokio::test]
    async fn binary_files_are_not_fed_to_parse_pipeline() {
        let engine_cell_bytes =
            b"---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n";

        let (_tmp, ctx, runtime) = build_ctx_with_text_and_binary_files(
            &[("doc.qmd", "---\ntitle: Prose\n---\n\nNo cells.\n")],
            &[("logo.png", engine_cell_bytes.as_slice())],
        )
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

        assert_eq!(
            count, 0,
            "no captures expected: doc.qmd is prose-only, logo.png is a binary that must be skipped",
        );
        assert!(
            !ctx.index().has_capture("logo.png"),
            "binary files must never have engine captures recorded — bd-i6jy4",
        );
        assert!(
            !ctx.index().has_capture("doc.qmd"),
            "prose-only qmd records no capture (sanity)",
        );
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

    /// bd-6d2wj4zp Phase 5 (S5): `.md` inputs never execute engines —
    /// even with an explicit engine spec and code cells. Now that the
    /// hub syncs `.md` into `ProjectFiles::qmd_files` (D10), the eager
    /// capture walk must skip them, or preview would execute engines
    /// the render path refuses to run (Q-2-40 warns and skips there).
    #[tokio::test]
    async fn md_doc_with_engine_spec_records_no_capture() {
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[(
            "notes.md",
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n42\n```\n",
        )])
        .await;

        // Non-vacuity: the .md must actually be in the synced source set
        // (D10) — otherwise this test would pass for the wrong reason.
        assert!(
            ctx.project_files()
                .expect("project mode")
                .qmd_files
                .contains(&std::path::PathBuf::from("notes.md")),
            "notes.md should be discovered into the hub source set"
        );

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry()),
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(count, 0, ".md must be skipped by the capture walk (S5)");
        assert!(!ctx.index().has_capture("notes.md"));
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
        let captures = read_capture_from_doc(&ctx, &entry.capture_doc_id)
            .await
            .expect("capture doc round-trips")
            .into_captures();
        assert_eq!(captures.len(), 1);
        let capture = &captures[0];

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

    /// PC3 — a failing engine on one doc must not stop the eager loop
    /// from recording the NEXT doc's capture, and must emit
    /// `Q-PREVIEW-CAP-1` to the diagnostic sink for the failing doc.
    ///
    /// Mock boundary: only the failing engine is a test double (mirrors
    /// `FailingTestEngine` in `diagnostics_capture_failure.rs`); the
    /// driver and samod context are real, pattern-matched off
    /// `capture_binary_doc_round_trips_through_samod` above. `qmd_files`
    /// is sorted (`discovery.rs:154`), so the `a-`/`b-` filename prefixes
    /// guarantee the failing doc is processed BEFORE the working one —
    /// otherwise a loop-return-on-Err regression could slip through
    /// undetected if the working doc happened to run first.
    ///
    /// Fail-on-revert binding (see task-p3-report.md for the transcript):
    /// (a) commenting out the `sink.emit(...)` call turns the
    /// diagnostics-count assertion RED (diagnostic absent);
    /// (b) replacing the loop's `Err(e) => { ... }` arm with
    /// `Err(e) => return Err(RecordError::RecordFailed(format!("{e}")))`
    /// turns the doc-B assertions RED (loop stops at the first failure,
    /// B's capture is never recorded).
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
                "synthetic PC3 failing engine failure".to_string(),
            ))
        }
        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> quarto_core::engine::LanguageClaim {
            if language == "test-failing" {
                quarto_core::engine::LanguageClaim::Primary(1)
            } else {
                quarto_core::engine::LanguageClaim::None
            }
        }
    }

    fn make_registry_with_failing_and_passthrough() -> Arc<EngineRegistry> {
        let mut reg = EngineRegistry::new();
        reg.register(Arc::new(FailingTestEngine));
        reg.register(Arc::new(PassthroughTestEngine));
        Arc::new(reg)
    }

    #[tokio::test]
    async fn pc3_failing_engine_does_not_block_next_doc_capture() {
        let (_tmp, ctx, runtime) = build_ctx_with_files(&[
            (
                "a-failing.qmd",
                "---\nengine: test-failing\n---\n\n```{test-failing}\nboom\n```\n",
            ),
            (
                "b-echo.qmd",
                "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nSENTINEL\n```\n",
            ),
        ])
        .await;

        // Fresh sink for this test process (nextest runs each test in
        // its own process, so this is the first writer — see
        // diagnostics.rs's `set_sink` doc comment).
        let sink = Arc::new(diagnostics::DiagnosticSink::new());
        diagnostics::set_sink(sink.clone());

        let count = record_eager_captures(
            ctx.clone(),
            runtime,
            Some(make_registry_with_failing_and_passthrough()),
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(
            count, 1,
            "only b-echo.qmd should have recorded a capture (a-failing.qmd's engine fails)"
        );

        // Positive assertion (mandatory — binds continue-on-error):
        // doc B's capture WAS recorded despite doc A's failure.
        assert!(
            ctx.index().has_capture("b-echo.qmd"),
            "doc B's capture must be recorded even though doc A's engine failed"
        );
        let b_entry = ctx.index().get_capture("b-echo.qmd").unwrap();
        let b_captures = read_capture_from_doc(&ctx, &b_entry.capture_doc_id)
            .await
            .expect("doc B's capture doc round-trips")
            .into_captures();
        assert_eq!(b_captures.len(), 1);
        assert_eq!(b_captures[0].engine_name, "test-passthrough");

        // Doc A: no capture recorded, and Q-PREVIEW-CAP-1 reached the
        // diagnostic sink.
        assert!(
            !ctx.index().has_capture("a-failing.qmd"),
            "doc A's capture must NOT be recorded — its engine failed"
        );
        let a_diags = sink.get_for_page("a-failing.qmd");
        assert_eq!(
            a_diags.len(),
            1,
            "exactly one diagnostic expected for a-failing.qmd; got {a_diags:?}"
        );
        assert_eq!(a_diags[0].code.as_deref(), Some("Q-PREVIEW-CAP-1"));
        let problem = a_diags[0]
            .problem
            .as_ref()
            .expect("diagnostic should carry the engine's error message");
        assert!(
            problem
                .as_str()
                .contains("synthetic PC3 failing engine failure"),
            "problem field should surface the engine's error; got: {}",
            problem.as_str()
        );
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

    /// Pre-existing watcher scope limit, **not a regression** (EXR-14 scope note): the
    /// preview watcher recomputes staleness only for the path that changed
    /// (`on_file_changed` in `lib.rs`), so an edit to an *included* file never recomputes
    /// the includer. Recomputing the includer itself does see the edit; only the trigger
    /// is missing. Fixing the watcher is outside the execution-records epic.
    #[tokio::test]
    async fn editing_an_included_file_does_not_recompute_the_includer_known_watcher_limit() {
        let (tmp, ctx, runtime) = build_ctx_with_files(&[
            (
                "doc.qmd",
                "---\nengine: test-passthrough\n---\n\n{{< include _inc.qmd >}}\n",
            ),
            ("_inc.qmd", "```{test-passthrough}\nfirst\n```\n"),
        ])
        .await;
        let count = record_eager_captures(
            ctx.clone(),
            runtime.clone(),
            Some(make_registry()),
            EnginePolicy::Manual,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert!(count >= 1, "the includer has a capture");

        std::fs::write(
            tmp.path().join("_inc.qmd"),
            "```{test-passthrough}\nsecond\n```\n",
        )
        .unwrap();

        // What the watcher does for the changed path: recompute that path only.
        recompute_staleness(
            ctx.clone(),
            runtime.clone(),
            "_inc.qmd",
            EnginePolicy::Manual,
            None,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        assert_eq!(
            ctx.index().get_capture("doc.qmd").unwrap().staleness,
            Some(false),
            "the includer is not recomputed by an edit to its include"
        );

        // The includer itself would see the edit if something recomputed it.
        assert!(
            recompute_staleness(
                ctx.clone(),
                runtime,
                "doc.qmd",
                EnginePolicy::Manual,
                None,
                &cache_dir_for_test(),
            )
            .await
            .unwrap(),
            "recomputing the includer does see the include's edit"
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

    /// An engine that records the units it owns, as the Jupyter engine does (EXR-4's
    /// `RecordBuilder` over the stage's units and author texts). The passthrough engine
    /// above builds no record.
    struct RecordingTestEngine;

    impl ExecutionEngine for RecordingTestEngine {
        fn name(&self) -> &str {
            "test-recording"
        }

        fn execute(
            &self,
            input: &str,
            ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            use quarto_core::record::{EngineEntryInput, RecordBuilder, UnitResult};
            let mut result = ExecuteResult::passthrough(input);
            if ctx.want_record {
                let fail = |e: &dyn std::fmt::Display| {
                    ExecutionError::execution_failed("test-recording", e.to_string())
                };
                let def = self.hash_definition();
                let mut builder =
                    RecordBuilder::new(self.name(), &def, &ctx.units, &ctx.author_texts)
                        .map_err(|e| fail(&e))?;
                for (i, unit) in ctx.units.iter().enumerate() {
                    if ctx.owned_languages.iter().any(|l| l == unit.lang()) {
                        builder
                            .push(i, UnitResult::default())
                            .map_err(|e| fail(&e))?;
                    }
                }
                result.record = Some(
                    builder
                        .finish(EngineEntryInput {
                            owned_languages: ctx.owned_languages.clone(),
                            document_metadata: ctx.document_metadata_json(),
                            ..Default::default()
                        })
                        .map_err(|e| fail(&e))?,
                );
            }
            Ok(result)
        }

        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> quarto_core::engine::LanguageClaim {
            if language == "test-recording" {
                quarto_core::engine::LanguageClaim::Primary(1)
            } else {
                quarto_core::engine::LanguageClaim::None
            }
        }
    }

    fn make_recording_registry() -> Arc<EngineRegistry> {
        let mut reg = EngineRegistry::new();
        reg.register(Arc::new(RecordingTestEngine));
        Arc::new(reg)
    }

    // ──────────────────────────────────────────────────────────────
    // EXR-15: record-level staleness (decision 5)
    // ──────────────────────────────────────────────────────────────

    /// Like `build_ctx_record_then_return_root`, but the sidecar points at a *record*
    /// doc (writer gate on), built from a real pre-engine run.
    async fn build_ctx_record_doc_then_return_root(
        files: &[(&str, &str)],
        rel: &str,
    ) -> (TempDir, Arc<HubContext>, Arc<dyn SystemRuntime>) {
        let (tmp, ctx, runtime) = build_ctx_with_files(files).await;
        let abs = tmp.path().join(rel);
        let project = ProjectContext::discover(&abs, runtime.as_ref()).unwrap();
        let run = quarto_core::engine::preview_record::record_capture_run(
            &abs,
            &project,
            runtime.clone(),
            Some(make_recording_registry()),
        )
        .await
        .unwrap();
        assert!(run.record.is_some(), "the engine stage emits a record");
        let id = write_capture_doc_gated(&ctx, rel, &run, true)
            .await
            .unwrap();
        ctx.index()
            .set_capture(
                rel,
                &CaptureRef {
                    capture_doc_id: id,
                    staleness: Some(false),
                    state: None,
                    last_error: None,
                },
            )
            .unwrap();
        (tmp, ctx, runtime)
    }

    async fn staleness_after_edit(
        ctx: &Arc<HubContext>,
        runtime: &Arc<dyn SystemRuntime>,
        rel: &str,
    ) -> Option<bool> {
        recompute_staleness(
            ctx.clone(),
            runtime.clone(),
            rel,
            EnginePolicy::Manual,
            None,
            &cache_dir_for_test(),
        )
        .await
        .unwrap();
        ctx.index().get_capture(rel).unwrap().staleness
    }

    const RECORD_DOC: &str =
        "---\nengine: test-recording\n---\n\nA paragraph.\n\n```{test-recording}\nx\n```\n";

    #[tokio::test]
    async fn record_prose_only_edit_stays_fresh() {
        let (tmp, ctx, runtime) =
            build_ctx_record_doc_then_return_root(&[("doc.qmd", RECORD_DOC)], "doc.qmd").await;
        std::fs::write(
            tmp.path().join("doc.qmd"),
            RECORD_DOC.replace("A paragraph.", "A different paragraph."),
        )
        .unwrap();
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(false),
            "a prose edit does not stale a record"
        );
    }

    #[tokio::test]
    async fn record_code_edit_flips_staleness() {
        let (tmp, ctx, runtime) =
            build_ctx_record_doc_then_return_root(&[("doc.qmd", RECORD_DOC)], "doc.qmd").await;
        std::fs::write(
            tmp.path().join("doc.qmd"),
            RECORD_DOC.replace("\nx\n", "\ny\n"),
        )
        .unwrap();
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(true)
        );
    }

    #[tokio::test]
    async fn record_unchanged_document_stays_fresh() {
        let (_tmp, ctx, runtime) =
            build_ctx_record_doc_then_return_root(&[("doc.qmd", RECORD_DOC)], "doc.qmd").await;
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(false)
        );
    }

    /// The function, not the watcher: recomputing the includer after an edit inside an
    /// included file that holds code sets `staleness`, because the hash comes from the
    /// expanded pre-engine units, not from file bytes. (The watcher never makes this call;
    /// see the known-limit test above.)
    #[tokio::test]
    async fn record_function_sees_a_code_edit_inside_an_include() {
        let (tmp, ctx, runtime) = build_ctx_record_doc_then_return_root(
            &[
                (
                    "doc.qmd",
                    "---\nengine: test-recording\n---\n\n{{< include _inc.qmd >}}\n",
                ),
                ("_inc.qmd", "```{test-recording}\nfirst\n```\n"),
            ],
            "doc.qmd",
        )
        .await;
        std::fs::write(
            tmp.path().join("_inc.qmd"),
            "```{test-recording}\nsecond\n```\n",
        )
        .unwrap();
        assert_eq!(
            staleness_after_edit(&ctx, &runtime, "doc.qmd").await,
            Some(true)
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
