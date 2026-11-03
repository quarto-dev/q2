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
use std::sync::{Arc, Mutex, OnceLock};

use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quarto_core::engine::EngineRegistry;
use quarto_core::project::ProjectContext;
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_hub::context::SharedContext;
use quarto_hub::index::{CaptureRef, CaptureState};
use quarto_system_runtime::SystemRuntime;
use serde::{Deserialize, Serialize};

use crate::cache::record_capture_run_cached;
use crate::config::ReExecuteConfig;
use crate::diagnostics;

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
///
/// The [`ReExecuteConfig`] arrives as an axum `Extension` layered on by `lib.rs`.
pub async fn re_execute_handler(
    State(ctx): State<SharedContext>,
    cfg: Option<Extension<ReExecuteConfig>>,
    Json(body): Json<ReExecuteRequest>,
) -> Response {
    // Fail closed: without the config the handler cannot know whether the engine may run, and
    // it must not guess (it used to fall back to a temp directory and run anyway).
    let Some(Extension(cfg)) = cfg else {
        return (
            StatusCode::FORBIDDEN,
            "Re-execute is not configured on this server".to_string(),
        )
            .into_response();
    };
    re_execute_with_registry_and_cache(ctx, body, None, &cfg).await
}

/// Internal entry point that takes an engine_registry override.
/// Production uses `None` (default registry); tests substitute a
/// passthrough engine to exercise the full path without a real
/// jupyter/knitr runtime.
///
/// `cfg` carries the policy, the record sources, the writer gate and the Phase C.7 capture
/// cache root (typically `<data_dir>/captures/`).
pub(crate) async fn re_execute_with_registry_and_cache(
    ctx: SharedContext,
    body: ReExecuteRequest,
    registry: Option<Arc<EngineRegistry>>,
    cfg: &ReExecuteConfig,
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

    match claim_and_spawn(ctx, rel_path.clone(), registry, cfg.clone()) {
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
        ClaimOutcome::Refused => (StatusCode::FORBIDDEN, refusal_message(cfg)).into_response(),
    }
}

/// Why a re-execute was refused, naming the setting the user would change.
fn refusal_message(cfg: &ReExecuteConfig) -> String {
    if cfg.policy == crate::config::EnginePolicy::Off {
        "Re-execute is disabled: preview.engine is off".to_string()
    } else {
        "Re-execute is disabled: the engine is not in --records, so this preview never runs code"
            .to_string()
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
    cfg: ReExecuteConfig,
) {
    match claim_and_spawn(ctx, rel_path.clone(), registry, cfg) {
        ClaimOutcome::Spawned => {
            tracing::debug!(rel_path = %rel_path, "auto re-execute kicked off");
        }
        ClaimOutcome::AlreadyInFlight => {
            tracing::debug!(rel_path = %rel_path, "auto re-execute skipped: already in flight");
        }
        ClaimOutcome::MarkRunningFailed(e) => {
            tracing::warn!(rel_path = %rel_path, error = %e, "auto re-execute failed to mark running");
        }
        ClaimOutcome::Refused => {
            tracing::debug!(rel_path = %rel_path, "auto re-execute refused: the engine may not run");
        }
    }
}

enum ClaimOutcome {
    Spawned,
    AlreadyInFlight,
    MarkRunningFailed(String),
    /// The policy is `off` or `engine` is not in the record sources (epic decision 5).
    Refused,
}

/// Shared claim-and-spawn: validates that the in-flight slot is
/// available, marks the sidecar `state: running`, and spawns the
/// blocking engine worker. Returns the outcome so the caller can
/// shape the response (HTTP handler) or log (auto path).
fn claim_and_spawn(
    ctx: SharedContext,
    rel_path: String,
    registry: Option<Arc<EngineRegistry>>,
    cfg: ReExecuteConfig,
) -> ClaimOutcome {
    // The one guard every kernel start passes through: the HTTP handler (a share guest reaches
    // it through the tunnel) and the automatic path both come here. It sits before the in-flight
    // insert, so a refusal cannot leak the slot and answer 409 to every later POST.
    if !cfg.allows_engine() {
        return ClaimOutcome::Refused;
    }
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
            &cfg,
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
    cfg: &ReExecuteConfig,
) -> Result<(), String> {
    let project_root = ctx
        .storage()
        .project_root()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "no project root (standalone mode?)".to_string())?;
    let abs_path = project_root.join(rel_path);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(crate::render_flags::native_runtime());

    let project = ProjectContext::discover(&abs_path, runtime.as_ref())
        .map_err(|e| format!("project discovery failed: {e}"))?;

    let run = record_capture_run_cached(
        &cfg.cache_dir,
        &abs_path,
        &project,
        runtime.clone(),
        registry,
    )
    .await
    .map_err(|e| format!("engine pipeline failed: {e}"))?;
    if !run.has_content() {
        return Err("engine produced no capture (no code cells?)".to_string());
    }

    let new_doc_id =
        crate::capture_driver::write_capture_doc_gated(&ctx, rel_path, &run, cfg.record_gate)
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

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;
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
    async fn seed_capture(ctx: SharedContext, cache_dir: &std::path::Path) {
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let count = crate::capture_driver::record_eager_captures(
            ctx,
            runtime,
            Some(make_registry()),
            &crate::config::ReExecuteConfig::new(
                crate::config::EnginePolicy::Manual,
                cache_dir.to_path_buf(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(count, 1, "fixture must seed a capture");
    }

    // ── RIU-2 Phase B3: the execution guard ───────────────────────────────

    const GUARD_DOC: &str = "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n";

    /// A passthrough engine that counts executions, to prove no engine ran.
    struct CountingEngine {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl ExecutionEngine for CountingEngine {
        fn name(&self) -> &str {
            "test-passthrough"
        }
        fn execute(
            &self,
            input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ExecuteResult::passthrough(input))
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

    fn counting_registry() -> (Arc<EngineRegistry>, Arc<std::sync::atomic::AtomicUsize>) {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut r = EngineRegistry::new();
        r.register(Arc::new(CountingEngine {
            calls: calls.clone(),
        }));
        (Arc::new(r), calls)
    }

    fn cfg_with(
        policy: crate::config::EnginePolicy,
        sources: Vec<quarto_core::record::RecordSource>,
        cache: &TempDir,
    ) -> ReExecuteConfig {
        let mut c = ReExecuteConfig::new(policy, cache.path().to_path_buf());
        c.sources = quarto_core::record::RecordSources::new(sources);
        c
    }

    async fn body_text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Wait for the spawned re-execute to finish, yielding so the runtime can drive the samod
    /// repo the blocking worker writes to. A test that returns first deadlocks at runtime
    /// teardown: the worker needs the runtime the test body is shutting down.
    async fn wait_until_idle() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !in_flight().lock().unwrap().is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "re-execute never finished"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }

    async fn post(
        ctx: &SharedContext,
        registry: Arc<EngineRegistry>,
        cfg: &ReExecuteConfig,
    ) -> Response {
        re_execute_with_registry_and_cache(
            ctx.clone(),
            ReExecuteRequest {
                path: "doc.qmd".to_string(),
            },
            Some(registry),
            cfg,
        )
        .await
    }

    /// Re-execute runs the engine and does not consult the other sources: a `.quarto` record that
    /// would be chosen at start-up never replaces the fresh engine result.
    #[tokio::test]
    async fn re_execute_runs_the_engine_and_ignores_a_q2_record() {
        reset_in_flight_for_tests();
        let (tmp, ctx) = build_ctx_with_file(GUARD_DOC).await;
        let root = tmp.path().canonicalize().unwrap();
        let cache = TempDir::with_prefix("riu2-reexec-").unwrap();
        seed_capture(ctx.clone(), cache.path()).await;
        let initial = ctx.index().get_capture("doc.qmd").unwrap().capture_doc_id;

        // A `.quarto` record sits there, and the code changed, so the engine must run.
        let file = quarto_core::record::execution_record_path(&root, &root.join("doc.qmd"));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let record = quarto_core::record::read_record(include_str!(
            "../../quarto-core/tests/fixtures/records/basic.json"
        ))
        .unwrap();
        std::fs::write(&file, quarto_core::record::write_record(&record).unwrap()).unwrap();
        std::fs::write(root.join("doc.qmd"), GUARD_DOC.replace("\n1\n", "\n2\n")).unwrap();

        let (registry, calls) = counting_registry();
        let cfg = ReExecuteConfig::new(
            crate::config::EnginePolicy::Manual,
            cache.path().to_path_buf(),
        );
        assert_eq!(
            post(&ctx, registry, &cfg).await.status(),
            StatusCode::ACCEPTED
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let e = ctx.index().get_capture("doc.qmd").unwrap();
            if e.state == Some(CaptureState::Idle) && e.capture_doc_id != initial {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "sidecar never updated"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        wait_until_idle().await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let id = ctx.index().get_capture("doc.qmd").unwrap().capture_doc_id;
        let payload = crate::capture_driver::read_capture_from_doc(&ctx, &id)
            .await
            .unwrap();
        assert!(
            matches!(
                payload,
                quarto_core::pandoc_request::captures::CapturePayload::Legacy(_)
            ),
            "the engine's own result is published, not the q2 record"
        );
    }

    /// The sidecar is untouched by a refused request, and the in-flight slot was not claimed
    /// (a refusal after the insert would leak it and every later POST would answer 409).
    fn assert_untouched(ctx: &SharedContext, before: &CaptureRef) {
        let now = ctx.index().get_capture("doc.qmd").unwrap();
        assert_eq!(now.capture_doc_id, before.capture_doc_id);
        assert_ne!(now.state, Some(CaptureState::Running));
        assert!(
            in_flight().lock().unwrap().is_empty(),
            "a refusal must not claim the in-flight slot"
        );
    }

    #[tokio::test]
    async fn re_execute_post_is_refused_under_policy_off() {
        reset_in_flight_for_tests();
        let (_tmp, ctx) = build_ctx_with_file(GUARD_DOC).await;
        let cache = TempDir::with_prefix("riu2-guard-").unwrap();
        seed_capture(ctx.clone(), cache.path()).await;
        let before = ctx.index().get_capture("doc.qmd").unwrap();
        let (registry, calls) = counting_registry();

        let cfg = cfg_with(
            crate::config::EnginePolicy::Off,
            quarto_core::record::RecordSources::preview_default()
                .iter()
                .cloned()
                .collect(),
            &cache,
        );
        let response = post(&ctx, registry, &cfg).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let msg = body_text(response).await;
        assert!(msg.contains("preview.engine"), "{msg}");
        assert_untouched(&ctx, &before);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn re_execute_post_is_refused_when_engine_is_not_in_the_source_list() {
        use quarto_core::record::RecordSource;
        reset_in_flight_for_tests();
        let (_tmp, ctx) = build_ctx_with_file(GUARD_DOC).await;
        let cache = TempDir::with_prefix("riu2-guard-").unwrap();
        seed_capture(ctx.clone(), cache.path()).await;
        let before = ctx.index().get_capture("doc.qmd").unwrap();
        let (registry, calls) = counting_registry();

        let cfg = cfg_with(
            crate::config::EnginePolicy::Manual,
            vec![RecordSource::Cache, RecordSource::Q2],
            &cache,
        );
        let response = post(&ctx, registry, &cfg).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let msg = body_text(response).await;
        assert!(msg.contains("--records"), "{msg}");
        assert_untouched(&ctx, &before);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// The automatic path reaches `claim_and_spawn` without the handler, so the guard must be
    /// there. With `engine` absent from the list no kernel starts after a staleness flip.
    #[tokio::test]
    async fn auto_re_execute_starts_nothing_without_engine_in_the_list() {
        use quarto_core::record::RecordSource;
        reset_in_flight_for_tests();
        let (_tmp, ctx) = build_ctx_with_file(GUARD_DOC).await;
        let cache = TempDir::with_prefix("riu2-guard-").unwrap();
        seed_capture(ctx.clone(), cache.path()).await;
        let before = ctx.index().get_capture("doc.qmd").unwrap();
        let (registry, calls) = counting_registry();

        let cfg = cfg_with(
            crate::config::EnginePolicy::Auto,
            vec![RecordSource::Cache],
            &cache,
        );
        trigger_auto_re_execute(ctx.clone(), "doc.qmd".to_string(), Some(registry), cfg);
        // A spawned worker would have flipped the sidecar and run the engine well inside this.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert_untouched(&ctx, &before);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// With `engine` in the list, `manual` still runs the engine.
    #[tokio::test]
    async fn re_execute_runs_when_the_policy_and_list_allow_it() {
        reset_in_flight_for_tests();
        let (tmp, ctx) = build_ctx_with_file(GUARD_DOC).await;
        let cache = TempDir::with_prefix("riu2-guard-").unwrap();
        seed_capture(ctx.clone(), cache.path()).await;
        // Change the code so the content-keyed cache misses; an identical document is a cache
        // hit and would (correctly) run no engine.
        std::fs::write(
            tmp.path().canonicalize().unwrap().join("doc.qmd"),
            GUARD_DOC.replace("\n1\n", "\n2\n"),
        )
        .unwrap();
        let (registry, calls) = counting_registry();
        let cfg = ReExecuteConfig::new(
            crate::config::EnginePolicy::Manual,
            cache.path().to_path_buf(),
        );
        let response = post(&ctx, registry, &cfg).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while calls.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            assert!(std::time::Instant::now() < deadline, "engine never ran");
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        wait_until_idle().await;
    }

    /// Two configs in one process each follow their own; the old process-wide static made the
    /// first writer win for everyone.
    #[tokio::test]
    async fn two_configs_in_one_process_each_follow_their_own() {
        reset_in_flight_for_tests();
        let (_tmp, ctx) = build_ctx_with_file(GUARD_DOC).await;
        let cache = TempDir::with_prefix("riu2-guard-").unwrap();
        seed_capture(ctx.clone(), cache.path()).await;
        let (registry, _) = counting_registry();
        let off =
            ReExecuteConfig::new(crate::config::EnginePolicy::Off, cache.path().to_path_buf());
        let manual = ReExecuteConfig::new(
            crate::config::EnginePolicy::Manual,
            cache.path().to_path_buf(),
        );
        assert_eq!(
            post(&ctx, registry.clone(), &off).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            post(&ctx, registry, &manual).await.status(),
            StatusCode::ACCEPTED
        );
        wait_until_idle().await;
    }

    /// A request that reaches the handler with no config extension is refused: it fails closed
    /// and never falls back to a temp directory.
    #[tokio::test]
    async fn the_handler_fails_closed_with_no_config() {
        reset_in_flight_for_tests();
        let (_tmp, ctx) = build_ctx_with_file(GUARD_DOC).await;
        let cache = TempDir::with_prefix("riu2-guard-").unwrap();
        seed_capture(ctx.clone(), cache.path()).await;
        let before = ctx.index().get_capture("doc.qmd").unwrap();
        let response = re_execute_handler(
            State(ctx.clone()),
            None,
            Json(ReExecuteRequest {
                path: "doc.qmd".to_string(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_untouched(&ctx, &before);
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
            &ReExecuteConfig::new(
                crate::config::EnginePolicy::Manual,
                cache_tmp.path().to_path_buf(),
            ),
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
            &ReExecuteConfig::new(
                crate::config::EnginePolicy::Manual,
                cache_tmp.path().to_path_buf(),
            ),
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
            &ReExecuteConfig::new(
                crate::config::EnginePolicy::Manual,
                re_execute_cache.path().to_path_buf(),
            ),
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
            &ReExecuteConfig::new(
                crate::config::EnginePolicy::Manual,
                cache_tmp.path().to_path_buf(),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
}
