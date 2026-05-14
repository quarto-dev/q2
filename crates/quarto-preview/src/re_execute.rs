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
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_hub::HubContext;
use quarto_hub::context::SharedContext;
use quarto_hub::index::{CaptureRef, CaptureState};
use quarto_hub::resource::create_binary_document;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use quarto_trace::EngineCapture;
use serde::{Deserialize, Serialize};

use crate::capture_driver::CAPTURE_MIME_TYPE;

/// Process-wide set of paths currently being re-executed. Used to
/// detect concurrent POSTs for the same path (→ 409). Lazy because
/// the handler may never be called.
static IN_FLIGHT: OnceLock<Arc<Mutex<HashSet<String>>>> = OnceLock::new();

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
    re_execute_with_registry(ctx, body, None).await
}

/// Internal entry point that takes an engine_registry override.
/// Production uses `None` (default registry); tests substitute a
/// passthrough engine to exercise the full path without a real
/// jupyter/knitr runtime.
pub(crate) async fn re_execute_with_registry(
    ctx: SharedContext,
    body: ReExecuteRequest,
    registry: Option<EngineRegistry>,
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

    // Concurrency guard: refuse a second re-execute for the same
    // path while the first is in flight.
    let in_flight_set = in_flight();
    {
        let mut guard = in_flight_set.lock().expect("in-flight mutex poisoned");
        if !guard.insert(rel_path.clone()) {
            return (
                StatusCode::CONFLICT,
                format!("Capture for '{rel_path}' is already being re-executed"),
            )
                .into_response();
        }
    }

    // Mark `state: running` on the sidecar so the SPA can show a
    // spinner. If there's no existing entry yet (unusual but
    // possible), we create one with a placeholder captureDocId
    // pointing at the eventual binary doc; the producer below writes
    // the real value.
    let previous_capture_doc_id = ctx
        .index()
        .get_capture(&rel_path)
        .map(|c| c.capture_doc_id.clone());

    if let Some(existing) = ctx.index().get_capture(&rel_path) {
        let running = CaptureRef {
            capture_doc_id: existing.capture_doc_id.clone(),
            staleness: existing.staleness,
            state: Some(CaptureState::Running),
            last_error: None,
        };
        if let Err(e) = ctx.index().set_capture(&rel_path, &running) {
            // Release the in-flight slot before returning.
            in_flight_set
                .lock()
                .expect("in-flight mutex poisoned")
                .remove(&rel_path);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to mark capture as running: {e}"),
            )
                .into_response();
        }
    }

    // Spawn the engine run on a blocking worker. The pipeline futures
    // are `?Send` so we use `pollster::block_on` inside
    // `spawn_blocking` — same pattern as the C.1 eager-capture
    // driver.
    let ctx_for_task = ctx.clone();
    let rel_path_for_task = rel_path.clone();
    let in_flight_for_task = in_flight_set.clone();
    let registry_for_task = registry;
    tokio::task::spawn_blocking(move || {
        let result = pollster::block_on(perform_re_execute(
            ctx_for_task.clone(),
            &rel_path_for_task,
            registry_for_task,
        ));
        // Always release the in-flight slot, regardless of outcome.
        in_flight_for_task
            .lock()
            .expect("in-flight mutex poisoned")
            .remove(&rel_path_for_task);

        // On failure, transition the sidecar to `state: error` with
        // the message so the SPA can surface it. Success path
        // already wrote `state: idle`.
        if let Err(e) = result {
            let msg = format!("{e}");
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
            tracing::warn!(rel_path = %rel_path_for_task, error = %msg, "re-execute failed");
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(ReExecuteAccepted {
            previous_capture_doc_id,
        }),
    )
        .into_response()
}

/// Drive the actual engine run, write the new binary doc, and
/// update the sidecar with the new captureDocId + state: idle.
async fn perform_re_execute(
    ctx: SharedContext,
    rel_path: &str,
    registry: Option<EngineRegistry>,
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

    let capture = record_capture(&abs_path, &project, runtime.clone(), registry)
        .await
        .map_err(|e| format!("engine pipeline failed: {e}"))?
        .ok_or_else(|| "engine produced no capture (no code cells?)".to_string())?;

    let new_doc_id = write_capture_doc(&ctx, &capture)
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
/// shared `CAPTURE_MIME_TYPE` constant.
async fn write_capture_doc(
    ctx: &Arc<HubContext>,
    capture: &EngineCapture,
) -> Result<String, String> {
    let json = serde_json::to_vec(capture).map_err(|e| format!("serialize: {e}"))?;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&json)
        .map_err(|e| format!("gzip write: {e}"))?;
    let gzipped = enc.finish().map_err(|e| format!("gzip finish: {e}"))?;
    let doc = create_binary_document(&gzipped, CAPTURE_MIME_TYPE)
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
    }

    fn make_registry() -> EngineRegistry {
        let mut r = EngineRegistry::new();
        r.register(Arc::new(PassthroughTestEngine));
        r
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
    async fn seed_capture(ctx: SharedContext) {
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let count =
            crate::capture_driver::record_eager_captures(ctx, runtime, Some(make_registry()))
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

        let response = re_execute_with_registry(
            ctx,
            ReExecuteRequest {
                path: "nonexistent.qmd".to_string(),
            },
            Some(make_registry()),
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
        seed_capture(ctx.clone()).await;

        let initial = ctx.index().get_capture("doc.qmd").unwrap();
        let initial_doc_id = initial.capture_doc_id.clone();

        let response = re_execute_with_registry(
            ctx.clone(),
            ReExecuteRequest {
                path: "doc.qmd".to_string(),
            },
            Some(make_registry()),
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

    #[tokio::test]
    async fn re_execute_concurrent_returns_409() {
        reset_in_flight_for_tests();
        let (_tmp, ctx) = build_ctx_with_file(
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n",
        )
        .await;
        seed_capture(ctx.clone()).await;

        // Manually claim the in-flight slot, then call the handler;
        // it should return 409 without queuing another run.
        in_flight().lock().unwrap().insert("doc.qmd".to_string());

        let response = re_execute_with_registry(
            ctx,
            ReExecuteRequest {
                path: "doc.qmd".to_string(),
            },
            Some(make_registry()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
}
