//! `/api/preview/re-execute` endpoint (Phase C.5, bd-kw93.5).
//!
//! POST with body `{ "path": "rel/path.qmd" }`. Server validates the
//! path is tracked in the index, claims an in-flight slot, kicks off
//! a `record_capture` run on a blocking worker, writes the new
//! capture binary doc, and updates the sidecar entry with the new
//! `captureDocId` and `staleness: false`. Returns 202 with the new
//! `captureDocId` as soon as the slot is claimed; the actual work
//! finishes asynchronously and lands in the sidecar via the existing
//! samod sync.
//!
//! Concurrency: a process-wide `Mutex<HashSet<String>>` tracks the
//! set of paths currently being re-executed. A second POST for an
//! already-executing path gets a 409 Conflict.
//!
//! Per plan §Q-C5: auth is loopback-only (inherited from Phase A);
//! the handler doesn't add its own auth layer because the whole
//! server is bound to 127.0.0.1.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quarto_core::engine::EngineRegistry;
use quarto_core::project::ProjectContext;
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_hub::HubContext;
use quarto_hub::context::SharedContext;
use quarto_hub::index::{CaptureRef, CaptureState};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use quarto_trace::EngineCapture;
use serde::{Deserialize, Serialize};

use crate::cache::record_capture_cached;
use crate::diagnostics;

/// Process-wide set of paths currently being re-executed. Used to
/// detect concurrent POSTs for the same path (→ 409). Lazy because
/// the handler may never be called.
static IN_FLIGHT: OnceLock<Arc<Mutex<HashSet<String>>>> = OnceLock::new();

/// Process-wide cache directory for the Phase C.7 filesystem cache.
/// Set once by `lib.rs::run_with_on_ready` and read by the HTTP
/// `re_execute_handler` (axum can't extract user data from the
/// router state without re-typing the State across the whole
/// `extend_with_preview` surface; OnceLock matches the existing
/// `SPA_DIR_OVERRIDE` pattern). Tests bypass this via the
/// `re_execute_with_registry_and_cache` entry point.
static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Set the process-wide cache directory used by the HTTP handler.
/// Idempotent (first writer wins); subsequent calls in the same
/// process are ignored. Called by [`crate::run_with_on_ready`].
pub fn set_cache_dir(dir: PathBuf) {
    let _ = CACHE_DIR.set(dir);
}

fn handler_cache_dir() -> Option<&'static Path> {
    CACHE_DIR.get().map(|p| p.as_path())
}

fn in_flight() -> Arc<Mutex<HashSet<String>>> {
    IN_FLIGHT
        .get_or_init(|| Arc::new(Mutex::new(HashSet::new())))
        .clone()
}

/// Test seam: clear the in-flight tracker. Called between tests so
/// state from one test doesn't leak into another.
#[cfg(test)]
pub(crate) fn reset_in_flight_for_tests() {
    if let Some(set) = IN_FLIGHT.get() {
        set.lock().unwrap().clear();
    }
}

#[derive(Debug, Deserialize)]
pub struct ReExecuteRequest {
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct ReExecuteAccepted {
    /// The samod document ID of the *previous* capture (still valid
    /// for replay while the new run is in flight). When the new
    /// capture lands, the sidecar updates and the SPA sees it via
    /// `onCapturesChange`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_capture_doc_id: Option<String>,
}

/// Axum handler for `POST /api/preview/re-execute`.
pub async fn re_execute_handler(
    State(ctx): State<SharedContext>,
    Json(body): Json<ReExecuteRequest>,
) -> Response {
    // The handler reads the process-wide cache directory set at server
    // startup. If it isn't set (server-less context, e.g. a unit test
    // calling the handler directly), fall back to a temp dir under
    // `data_dir/captures` — but production routes through `set_cache_dir`
    // in `run_with_on_ready`, so this branch is unusual.
    let cache_dir = handler_cache_dir().map_or_else(
        || std::env::temp_dir().join("q2-preview-captures-unset"),
        Path::to_path_buf,
    );
    re_execute_with_registry_and_cache(ctx, body, None, &cache_dir).await
}

/// Internal entry point that takes an engine_registry override.
/// Production uses `None` (default registry); tests substitute a
/// passthrough engine to exercise the full path without a real
/// jupyter/knitr runtime.
///
/// `cache_dir` is the Phase C.7 capture cache root (typically
/// `<data_dir>/captures/`).
pub(crate) async fn re_execute_with_registry_and_cache(
    ctx: SharedContext,
    body: ReExecuteRequest,
    registry: Option<Arc<EngineRegistry>>,
    cache_dir: &Path,
) -> Response {
    let rel_path = body.path;

    // Path validation: must be tracked in the index. Files outside
    // the project (or unknown paths) → 400. This also implicitly
    // rejects empty / "." / ".." since none are in the index.
    if !ctx.index().has_file(&rel_path) {
        return (
            StatusCode::BAD_REQUEST,
            format!("Path '{rel_path}' is not in the project index"),
        )
            .into_response();
    }

    let previous_capture_doc_id = ctx
        .index()
        .get_capture(&rel_path)
        .map(|c| c.capture_doc_id.clone());

    match claim_and_spawn(ctx, rel_path.clone(), registry, cache_dir.to_path_buf()) {
        ClaimOutcome::Spawned => (
            StatusCode::ACCEPTED,
            Json(ReExecuteAccepted {
                previous_capture_doc_id,
            }),
        )
            .into_response(),
        ClaimOutcome::AlreadyInFlight => (
            StatusCode::CONFLICT,
            format!("Capture for '{rel_path}' is already being re-executed"),
        )
            .into_response(),
        ClaimOutcome::MarkRunningFailed(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to mark capture as running: {e}"),
        )
            .into_response(),
    }
}

/// Auto-mode (Phase C.6) entry point: kick off a re-execute from the
/// file-watcher hook when the user's `preview.engine: auto` config
/// said so. Caller has already confirmed staleness; we only need to
/// claim the in-flight slot and spawn the worker.
///
/// This is the same machinery as the HTTP handler minus the response
/// shape — if a run is already in flight for this path, we silently
/// skip (a debounced flurry of edits collapses to one engine run at
/// a time per doc, which is what users want).
pub(crate) fn trigger_auto_re_execute(
    ctx: SharedContext,
    rel_path: String,
    registry: Option<Arc<EngineRegistry>>,
    cache_dir: PathBuf,
) {
    match claim_and_spawn(ctx, rel_path.clone(), registry, cache_dir) {
        ClaimOutcome::Spawned => {
            tracing::debug!(rel_path = %rel_path, "auto re-execute kicked off");
        }
        ClaimOutcome::AlreadyInFlight => {
            tracing::debug!(rel_path = %rel_path, "auto re-execute skipped: already in flight");
        }
        ClaimOutcome::MarkRunningFailed(e) => {
            tracing::warn!(rel_path = %rel_path, error = %e, "auto re-execute failed to mark running");
        }
    }
}

enum ClaimOutcome {
    Spawned,
    AlreadyInFlight,
    MarkRunningFailed(String),
}

/// Shared claim-and-spawn: validates that the in-flight slot is
/// available, marks the sidecar `state: running`, and spawns the
/// blocking engine worker. Returns the outcome so the caller can
/// shape the response (HTTP handler) or log (auto path).
fn claim_and_spawn(
    ctx: SharedContext,
    rel_path: String,
    registry: Option<Arc<EngineRegistry>>,
    cache_dir: PathBuf,
) -> ClaimOutcome {
    let in_flight_set = in_flight();
    {
        let mut guard = in_flight_set.lock().expect("in-flight mutex poisoned");
        if !guard.insert(rel_path.clone()) {
            return ClaimOutcome::AlreadyInFlight;
        }
    }

    if let Some(existing) = ctx.index().get_capture(&rel_path) {
        let running = CaptureRef {
            capture_doc_id: existing.capture_doc_id.clone(),
            staleness: existing.staleness,
            state: Some(CaptureState::Running),
            last_error: None,
        };
        if let Err(e) = ctx.index().set_capture(&rel_path, &running) {
            in_flight_set
                .lock()
                .expect("in-flight mutex poisoned")
                .remove(&rel_path);
            return ClaimOutcome::MarkRunningFailed(format!("{e}"));
        }
    }

    let ctx_for_task = ctx.clone();
    let rel_path_for_task = rel_path.clone();
    let in_flight_for_task = in_flight_set.clone();
    tokio::task::spawn_blocking(move || {
        let result = pollster::block_on(perform_re_execute(
            ctx_for_task.clone(),
            &rel_path_for_task,
            registry,
            &cache_dir,
        ));
        in_flight_for_task
            .lock()
            .expect("in-flight mutex poisoned")
            .remove(&rel_path_for_task);

        if let Err(e) = result {
            let msg = e.clone();
            if let Some(existing) = ctx_for_task.index().get_capture(&rel_path_for_task) {
                let errored = CaptureRef {
                    capture_doc_id: existing.capture_doc_id.clone(),
                    staleness: existing.staleness,
                    state: Some(CaptureState::Error),
                    last_error: Some(msg.clone()),
                };
                let _ = ctx_for_task
                    .index()
                    .set_capture(&rel_path_for_task, &errored);
            }
            // bd-b9kzg: emit a structured diagnostic to the sink so
            // the SPA's diagnostics overlay can render the failure
            // alongside other page-scoped issues. The existing
            // `CaptureRef.last_error` write above continues to feed
            // the StaleCaptureOverlay (Phase C.5) — both surfaces
            // see engine errors today.
            if let Some(sink) = diagnostics::current_sink() {
                sink.emit(
                    &rel_path_for_task,
                    DiagnosticMessageBuilder::warning("Engine re-execute failed")
                        .with_code("Q-PREVIEW-RE-1")
                        .problem(msg.clone())
                        .build(),
                );
            } else {
                tracing::warn!(rel_path = %rel_path_for_task, error = %msg, "re-execute failed");
            }
        }
    });

    ClaimOutcome::Spawned
}

/// Drive the actual engine run, write the new binary doc, and
/// update the sidecar with the new captureDocId + state: idle.
async fn perform_re_execute(
    ctx: SharedContext,
    rel_path: &str,
    registry: Option<Arc<EngineRegistry>>,
    cache_dir: &Path,
) -> Result<(), String> {
    let project_root = ctx
        .storage()
        .project_root()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "no project root (standalone mode?)".to_string())?;
    let abs_path = project_root.join(rel_path);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    let project = ProjectContext::discover(&abs_path, runtime.as_ref())
        .map_err(|e| format!("project discovery failed: {e}"))?;

    let captures = record_capture_cached(cache_dir, &abs_path, &project, runtime.clone(), registry)
        .await
        .map_err(|e| format!("engine pipeline failed: {e}"))?;
    if captures.is_empty() {
        return Err("engine produced no capture (no code cells?)".to_string());
    }

    let new_doc_id = write_capture_doc(&ctx, rel_path, &captures)
        .await
        .map_err(|e| format!("failed to store capture binary doc: {e}"))?;

    let updated = CaptureRef {
        capture_doc_id: new_doc_id,
        staleness: Some(false),
        state: Some(CaptureState::Idle),
        last_error: None,
    };
    ctx.index()
        .set_capture(rel_path, &updated)
        .map_err(|e| format!("failed to update sidecar: {e}"))?;

    Ok(())
}

/// Mirror of `capture_driver::write_capture_doc` (private there).
/// Duplicated here to avoid widening that module's public surface
/// for the C.5-only handler. Both call sites stay in sync via the
/// shared `CAPTURE_MIME_TYPE` constant and the shared wire-format
/// helper `gzip_captures` (serialize + gzip + 10MB size warning,
/// bd-qbhp2cvv).
async fn write_capture_doc(
    ctx: &Arc<HubContext>,
    rel_path: &str,
    captures: &[EngineCapture],
) -> Result<String, String> {
    let gzipped = quarto_core::engine::capture_files::gzip_captures(captures)
        .map_err(|e| format!("serialize/gzip: {e}"))?;
    let meta = quarto_hub::resource::CaptureDocMeta {
        source_path: rel_path.to_string(),
        engines: captures.iter().map(|c| c.engine_name.clone()).collect(),
    };
    let doc = quarto_hub::resource::create_capture_document(&gzipped, &meta)
        .map_err(|e| format!("binary doc: {e}"))?;
    let handle = ctx
        .repo()
        .create(doc)
        .await
        .map_err(|_| "samod repo stopped".to_string())?;
    Ok(handle.document_id().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use quarto_core::engine::{ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError};
    use quarto_hub::HubContext;
    use quarto_hub::context::HubConfig;
    use quarto_hub::storage::StorageManager;
    use tempfile::TempDir;

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
            out.push_str("\n<!-- re-execute -->\n");
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
        let mut r = EngineRegistry::new();
        r.register(Arc::new(PassthroughTestEngine));
        Arc::new(r)
    }

    async fn build_ctx_with_file(content: &str) -> (TempDir, SharedContext) {
        let project = TempDir::with_prefix("c5-test-").unwrap();
        let project_root = project.path().canonicalize().unwrap();
        std::fs::write(project_root.join("doc.qmd"), content).unwrap();
        let storage = StorageManager::new(&project_root).unwrap();
        let ctx = Arc::new(
            HubContext::new(storage, HubConfig::default())
                .await
                .unwrap(),
        );
        (project, ctx)
    }

    /// Spin up the eager-capture driver so the sidecar has a
    /// captureRef to operate on. (Some C.5 paths require an
    /// existing capture; others test the no-capture branch.)
    async fn seed_capture(ctx: SharedContext, cache_dir: &Path) {
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let count = crate::capture_driver::record_eager_captures(
            ctx,
            runtime,
            Some(make_registry()),
            crate::config::EnginePolicy::Manual,
            cache_dir,
        )
        .await
        .unwrap();
        assert_eq!(count, 1, "fixture must seed a capture");
    }

    #[tokio::test]
    async fn re_execute_invalid_path_returns_400() {
        reset_in_flight_for_tests();
        let (_tmp, ctx) = build_ctx_with_file(
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n",
        )
        .await;
        let cache_tmp = TempDir::with_prefix("c5-cache-").unwrap();

        let response = re_execute_with_registry_and_cache(
            ctx,
            ReExecuteRequest {
                path: "nonexistent.qmd".to_string(),
            },
            Some(make_registry()),
            cache_tmp.path(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn re_execute_returns_202_then_writes_new_capture() {
        reset_in_flight_for_tests();
        let (_tmp, ctx) = build_ctx_with_file(
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n",
        )
        .await;
        let cache_tmp = TempDir::with_prefix("c5-cache-").unwrap();
        seed_capture(ctx.clone(), cache_tmp.path()).await;

        let initial = ctx.index().get_capture("doc.qmd").unwrap();
        let initial_doc_id = initial.capture_doc_id.clone();

        let response = re_execute_with_registry_and_cache(
            ctx.clone(),
            ReExecuteRequest {
                path: "doc.qmd".to_string(),
            },
            Some(make_registry()),
            cache_tmp.path(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        // The blocking worker is now running. Poll for the sidecar
        // to flip to state: idle with a new captureDocId.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let entry = ctx.index().get_capture("doc.qmd").unwrap();
            if entry.state == Some(CaptureState::Idle) && entry.capture_doc_id != initial_doc_id {
                break;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "sidecar did not update within 10s; still {:?} {:?}",
                    entry.state, entry.capture_doc_id
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        let final_entry = ctx.index().get_capture("doc.qmd").unwrap();
        assert_eq!(final_entry.state, Some(CaptureState::Idle));
        assert_eq!(final_entry.staleness, Some(false));
        assert_eq!(final_entry.last_error, None);
        assert_ne!(final_entry.capture_doc_id, initial_doc_id);
    }

    /// PC8 — a failing engine on re-execute must flip the sidecar to
    /// `CaptureState::Error`, set `last_error`, and emit
    /// `Q-PREVIEW-RE-1` to the diagnostic sink.
    ///
    /// `FailingReExecuteEngine` shares the doc's declared engine name
    /// (`test-passthrough`) but always fails — it replaces the
    /// registry wholesale for the re-execute call (registry override
    /// is a full replacement, not a merge — see
    /// `quarto_core::engine::preview_record::record_capture`), so the
    /// seed run (which used the real `PassthroughTestEngine`) is
    /// unaffected. A separate cache dir for the re-execute call is
    /// REQUIRED: `record_capture_cached`'s key is
    /// `sha256(input_qmd)` only (content, not engine — cache.rs:150-163),
    /// and the doc's content is unchanged between seed and re-execute,
    /// so reusing the seed's cache dir would replay the cached
    /// SUCCESSFUL result and never invoke the failing engine.
    ///
    /// Fail-on-revert binding (see task-p3-report.md for the
    /// transcript): commenting out the `ctx_for_task.index().set_capture(&rel_path_for_task, &errored)`
    /// write turns the `CaptureState::Error`/`last_error` assertions
    /// RED (sidecar stays at whatever state `claim_and_spawn` set
    /// before the run — never reaches `Error`).
    struct FailingReExecuteEngine;
    impl ExecutionEngine for FailingReExecuteEngine {
        fn name(&self) -> &str {
            "test-passthrough"
        }
        fn execute(
            &self,
            _input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            Err(ExecutionError::Other(
                "synthetic PC8 re-execute failure".to_string(),
            ))
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

    fn make_failing_registry() -> Arc<EngineRegistry> {
        let mut r = EngineRegistry::new();
        r.register(Arc::new(FailingReExecuteEngine));
        Arc::new(r)
    }

    #[tokio::test]
    async fn pc8_re_execute_failure_sets_error_state_and_emits_diagnostic() {
        reset_in_flight_for_tests();
        let (_tmp, ctx) = build_ctx_with_file(
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n",
        )
        .await;
        let seed_cache = TempDir::with_prefix("c8-seed-cache-").unwrap();
        seed_capture(ctx.clone(), seed_cache.path()).await;

        let initial = ctx.index().get_capture("doc.qmd").unwrap();
        let initial_doc_id = initial.capture_doc_id.clone();

        let sink = Arc::new(diagnostics::DiagnosticSink::new());
        diagnostics::set_sink(sink.clone());

        // A fresh cache dir forces a miss — see the doc comment above.
        let re_execute_cache = TempDir::with_prefix("c8-reexec-cache-").unwrap();
        let response = re_execute_with_registry_and_cache(
            ctx.clone(),
            ReExecuteRequest {
                path: "doc.qmd".to_string(),
            },
            Some(make_failing_registry()),
            re_execute_cache.path(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let final_entry = loop {
            let entry = ctx.index().get_capture("doc.qmd").unwrap();
            if entry.state == Some(CaptureState::Error) {
                break entry;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "sidecar did not reach CaptureState::Error within 10s; still {:?}",
                    entry
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };

        assert_eq!(
            final_entry.capture_doc_id, initial_doc_id,
            "a failed re-execute must not replace the previous (still-valid) capture doc"
        );
        let last_error = final_entry
            .last_error
            .as_deref()
            .expect("last_error must be set on a failed re-execute");
        assert!(
            last_error.contains("synthetic PC8 re-execute failure"),
            "last_error should surface the engine's message; got: {last_error}"
        );

        let diags = sink.get_for_page("doc.qmd");
        assert_eq!(
            diags.len(),
            1,
            "exactly one diagnostic expected for doc.qmd; got {diags:?}"
        );
        assert_eq!(diags[0].code.as_deref(), Some("Q-PREVIEW-RE-1"));
    }

    #[tokio::test]
    async fn re_execute_concurrent_returns_409() {
        reset_in_flight_for_tests();
        let (_tmp, ctx) = build_ctx_with_file(
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n",
        )
        .await;
        let cache_tmp = TempDir::with_prefix("c5-cache-").unwrap();
        seed_capture(ctx.clone(), cache_tmp.path()).await;

        // Manually claim the in-flight slot, then call the handler;
        // it should return 409 without queuing another run.
        in_flight().lock().unwrap().insert("doc.qmd".to_string());

        let response = re_execute_with_registry_and_cache(
            ctx,
            ReExecuteRequest {
                path: "doc.qmd".to_string(),
            },
            Some(make_registry()),
            cache_tmp.path(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
}
