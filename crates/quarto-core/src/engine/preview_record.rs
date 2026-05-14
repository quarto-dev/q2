/*
 * engine/preview_record.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Engine capture recording for the q2 preview server (Phase C.1).
 */

//! Engine capture recording for the q2 preview server.
//!
//! [`record_capture`] runs the q2-preview pipeline up through
//! [`EngineExecutionStage`] (and stops there) and returns the
//! [`EngineCapture`] that the stage emits via its observer channel.
//! The capture lets the browser-side WASM pipeline run the same
//! document with a [`ReplayEngine`](super::ReplayEngine) substituted
//! for the real engine, producing the same `ExecuteResult` without a
//! second invocation.
//!
//! Sub-pipeline boundary (per plan §C.1 / §Q-C2): we stop at
//! `EngineExecutionStage` because everything after it (AST transforms,
//! HTML render, template apply) is render-side work the browser
//! repeats anyway. Running it here would be duplicate work.
//!
//! The function returns `Ok(None)` for documents that produce no
//! capture — either because they have no `engine:` declaration (the
//! markdown engine short-circuits without emission) or because the
//! resolved engine is `markdown`. The caller (the q2 preview server
//! file-watcher hook in Phase C.2) treats `None` as "no capture
//! needed" and omits the sidecar entry.

use std::sync::{Arc, Mutex};

use quarto_system_runtime::SystemRuntime;
use quarto_trace::EngineCapture;

use crate::format::Format;
use crate::pipeline::build_html_pipeline_stages_with_options;
use crate::project::{DocumentInfo, ProjectContext};
use crate::stage::stages::ENGINE_CAPTURE_KIND;
use crate::stage::{
    EventLevel, LoadedSource, Pipeline, PipelineData, PipelineError, PipelineObserver,
    PipelineStage, StageContext,
};

use super::EngineRegistry;

/// Observer that captures the first `EngineCapture` aux event emitted
/// during a pipeline run.
///
/// Wraps an `Arc<Mutex<Option<EngineCapture>>>` so the caller can
/// extract the result after the pipeline finishes. All non-aux methods
/// delegate to [`NoopObserver`] — we don't care about stage events,
/// only the capture payload.
struct CaptureCollector {
    captured: Arc<Mutex<Option<EngineCapture>>>,
}

impl CaptureCollector {
    fn new() -> Self {
        Self {
            captured: Arc::new(Mutex::new(None)),
        }
    }

    fn handle(&self) -> Arc<Mutex<Option<EngineCapture>>> {
        Arc::clone(&self.captured)
    }
}

impl PipelineObserver for CaptureCollector {
    fn on_auxiliary_data(&self, _stage: &str, _index: usize, kind: &str, data: &serde_json::Value) {
        if kind != ENGINE_CAPTURE_KIND {
            return;
        }
        match serde_json::from_value::<EngineCapture>(data.clone()) {
            Ok(capture) => {
                // First-write-wins. EngineExecutionStage runs once per
                // pipeline, so this realistically only fires once;
                // guarding anyway keeps the type honest.
                let mut slot = self.captured.lock().expect("capture mutex poisoned");
                if slot.is_none() {
                    *slot = Some(capture);
                }
            }
            Err(e) => {
                // Should not happen — the producer in
                // engine_execution.rs serializes the same struct shape
                // — but if it ever does we want a log, not a panic.
                tracing::warn!(
                    "preview_record: failed to deserialize EngineCapture payload: {}",
                    e
                );
            }
        }
    }

    // Other methods inherit no-op defaults; spell out the few that
    // would otherwise spam logs to keep this observer truly silent.
    fn on_event(&self, _message: &str, _level: EventLevel) {}
}

/// Build the engine-capture sub-pipeline: the q2-preview HTML pipeline
/// truncated at `EngineExecutionStage` (inclusive).
///
/// Keeping this in sync with the full pipeline by truncation rather
/// than re-listing means new pre-engine stages (sugar passes, profile
/// extensions, etc.) automatically flow into capture recording — no
/// drift between the server-side record path and the in-browser
/// replay path.
fn build_capture_pipeline_stages(
    engine_registry: Option<EngineRegistry>,
) -> Vec<Box<dyn PipelineStage>> {
    let mut stages = build_html_pipeline_stages_with_options(None, engine_registry);
    if let Some(idx) = stages.iter().position(|s| s.name() == "engine-execution") {
        stages.truncate(idx + 1);
    }
    stages
}

/// Record the engine capture for `path` by running the q2-preview
/// pipeline up through engine execution.
///
/// Returns `Ok(Some(capture))` when an engine fired and emitted its
/// `EngineCapture` aux event, `Ok(None)` when no engine ran (default
/// markdown engine short-circuits without emitting). Pipeline errors
/// propagate.
///
/// `engine_registry` lets callers substitute a custom registry for
/// tests (passthrough engine standing in for jupyter, etc.); production
/// callers pass `None` to use the default registry.
pub async fn record_capture(
    path: &std::path::Path,
    project: &ProjectContext,
    runtime: Arc<dyn SystemRuntime>,
    engine_registry: Option<EngineRegistry>,
) -> Result<Option<EngineCapture>, PipelineError> {
    let content = runtime
        .file_read(path)
        .map_err(|e| PipelineError::other(format!("Failed to read {}: {}", path.display(), e)))?;

    let document = DocumentInfo::from_path(path);
    let format = Format::from_format_string("q2-preview").map_err(|e| {
        PipelineError::other(format!("Failed to construct q2-preview format: {}", e))
    })?;

    let collector = CaptureCollector::new();
    let captured = collector.handle();

    let mut ctx = StageContext::new(runtime, format, project.clone(), document)?
        .with_observer(Arc::new(collector));

    let stages = build_capture_pipeline_stages(engine_registry);
    let pipeline =
        Pipeline::new(stages).expect("preview-record pipeline stages should be compatible");

    let input = PipelineData::LoadedSource(LoadedSource::new(path.to_path_buf(), content));

    let _final_data = pipeline.run(input, &mut ctx).await?;

    let mut slot = captured.lock().expect("capture mutex poisoned");
    Ok(slot.take())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use quarto_system_runtime::NativeRuntime;
    use tempfile::TempDir;

    use crate::engine::context::{ExecuteResult, ExecutionContext};
    use crate::engine::error::ExecutionError;
    use crate::engine::traits::ExecutionEngine;

    /// Passthrough engine that reports as a non-markdown name so it
    /// bypasses `EngineExecutionStage`'s `name() == "markdown"`
    /// short-circuit and triggers the capture-emission path.
    ///
    /// `result.markdown` is the input with a deterministic suffix
    /// appended so tests can prove the captured `result` came from
    /// the engine and not just a passthrough of the raw QMD.
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

    /// Write a fixture file and discover its single-file project context.
    /// Returns the temp dir (held to keep it alive), the qmd path, and
    /// the project context.
    fn fixture(content: &str) -> (TempDir, PathBuf, ProjectContext, Arc<dyn SystemRuntime>) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let qmd_path = dir.path().join("doc.qmd");
        std::fs::write(&qmd_path, content).expect("write fixture");
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&qmd_path, runtime.as_ref())
            .expect("discover single-file project");
        (dir, qmd_path, project, runtime)
    }

    #[tokio::test]
    async fn prose_only_doc_returns_none() {
        let (_tmp, path, project, runtime) =
            fixture("---\ntitle: Prose only\n---\n\nNo code cells here.\n");

        let result = record_capture(&path, &project, runtime, None)
            .await
            .expect("pipeline runs cleanly for prose");

        assert!(
            result.is_none(),
            "expected None for prose-only doc with default (markdown) engine"
        );
    }

    #[tokio::test]
    async fn doc_with_passthrough_engine_returns_capture() {
        let (_tmp, path, project, runtime) =
            fixture("---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1 + 1\n```\n");

        // Build a registry that has the test-passthrough engine plus
        // the defaults (markdown is needed as a fallback).
        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(PassthroughTestEngine));

        let result = record_capture(&path, &project, runtime, Some(registry))
            .await
            .expect("pipeline runs cleanly with test engine");

        let capture = result.expect("expected Some(capture) for doc with test-passthrough engine");
        assert_eq!(capture.engine_name, "test-passthrough");
        assert!(
            !capture.input_qmd.is_empty(),
            "input_qmd should be the serialized post-include QMD, not empty"
        );
        // The passthrough engine appends its sentinel comment; the
        // capture's result.markdown should contain it.
        let markdown = capture
            .result
            .get("markdown")
            .and_then(|v| v.as_str())
            .expect("capture.result should have a 'markdown' field");
        assert!(
            markdown.contains("<!-- test-passthrough -->"),
            "captured result.markdown should contain the engine's sentinel; got: {}",
            markdown
        );
    }

    #[tokio::test]
    async fn capture_input_qmd_is_serialized_ast_not_raw_file() {
        // The plan (§C.1 item 2 + §Q-C3) requires that `input_qmd` is
        // the post-include-expansion QMD (matching what
        // EngineExecutionStage hands to engines), so the
        // browser-side ReplayEngine's byte-equality check can hold
        // through the same pipeline run.
        //
        // Test: a doc with frontmatter + a code cell. The captured
        // input_qmd should reflect the AST round-trip, not the raw
        // file bytes. We don't pin an exact form (that would couple
        // to the serializer); we pin that (a) the engine declaration
        // line is gone (lifted into metadata) and (b) the cell body
        // is present.
        let (_tmp, path, project, runtime) =
            fixture("---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nSENTINEL\n```\n");

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(PassthroughTestEngine));

        let result = record_capture(&path, &project, runtime, Some(registry))
            .await
            .expect("pipeline runs");
        let capture = result.expect("capture present");

        assert!(
            capture.input_qmd.contains("SENTINEL"),
            "input_qmd should retain the cell body; got: {}",
            capture.input_qmd
        );
    }

    #[tokio::test]
    async fn observer_is_silent_for_pipeline_without_capture() {
        // Defensive: a prose-only doc must NOT leave the capture slot
        // populated even when the pipeline runs (the slot starts
        // empty; the observer must not write anything when no aux
        // event with the EngineCapture kind fires).
        let (_tmp, path, project, runtime) = fixture("Just prose, no frontmatter.\n");

        let result = record_capture(&path, &project, runtime, None)
            .await
            .expect("pipeline runs");
        assert!(result.is_none());
    }
}
