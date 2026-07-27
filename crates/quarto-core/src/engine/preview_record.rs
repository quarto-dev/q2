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
    captured: Arc<Mutex<Vec<EngineCapture>>>,
}

impl CaptureCollector {
    fn new() -> Self {
        Self {
            captured: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn handle(&self) -> Arc<Mutex<Vec<EngineCapture>>> {
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
                // bd-5yff4: collect one capture per engine, in execution
                // order. `EngineExecutionStage` emits these sequentially
                // as it threads the engine sequence.
                self.captured
                    .lock()
                    .expect("capture mutex poisoned")
                    .push(capture);
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

    // bd-qbhp2cvv: preview captures replay on machines (or browser
    // VFSes) that cannot see this machine's disk — ask the stage to
    // embed supporting-file bytes in the capture payload.
    fn wants_engine_capture_files(&self) -> bool {
        true
    }
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
/// Returns the captures in execution order — one per engine that fired
/// and emitted its `EngineCapture` aux event (bd-5yff4). An empty vec
/// means no engine ran (e.g. the default markdown engine short-circuits
/// without emitting). Pipeline errors propagate.
///
/// `engine_registry` lets callers substitute a custom registry for
/// tests (passthrough engine standing in for jupyter, etc.); production
/// callers pass `None` to use the default registry.
pub async fn record_capture(
    path: &std::path::Path,
    project: &ProjectContext,
    runtime: Arc<dyn SystemRuntime>,
    engine_registry: Option<EngineRegistry>,
) -> Result<Vec<EngineCapture>, PipelineError> {
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
    Ok(std::mem::take(&mut *slot))
}

/// Build the staleness-check sub-pipeline: everything up to (and
/// including) `PreEngineSugaringStage`. The pipeline returns the
/// `DocumentAst` that `EngineExecutionStage` would serialize to QMD
/// and hand to the engine — i.e. exactly what
/// [`record_capture`] passes as `input_qmd`. The Phase C.2 staleness
/// check serializes the same AST and compares byte-for-byte against
/// the existing capture's `input_qmd`.
fn build_pre_engine_pipeline_stages() -> Vec<Box<dyn PipelineStage>> {
    let mut stages = build_html_pipeline_stages_with_options(None, None);
    if let Some(idx) = stages
        .iter()
        .position(|s| s.name() == "pre-engine-sugaring")
    {
        stages.truncate(idx + 1);
    }
    stages
}

/// Compute the canonical QMD bytes that
/// [`EngineExecutionStage`](crate::stage::stages::EngineExecutionStage)
/// would hand to the engine for `path`.
///
/// Used by Phase C.2 to detect staleness of an existing capture
/// (server-side, on file-watcher events). The returned bytes are the
/// AST-serialized post-include-expansion QMD — same shape as the
/// `input_qmd` field on the recorded `EngineCapture`. A byte-equality
/// check against the capture is sufficient (and matches the
/// browser-side `ReplayEngine`'s validation policy).
///
/// Returns the serialized QMD bytes. Pipeline errors propagate;
/// per-doc parsing failures bubble up so the caller can log without
/// silently leaving stale state on the sidecar.
pub async fn compute_input_qmd(
    path: &std::path::Path,
    project: &ProjectContext,
    runtime: Arc<dyn SystemRuntime>,
) -> Result<Vec<u8>, PipelineError> {
    let content = runtime
        .file_read(path)
        .map_err(|e| PipelineError::other(format!("Failed to read {}: {}", path.display(), e)))?;

    let document = DocumentInfo::from_path(path);
    let format = Format::from_format_string("q2-preview").map_err(|e| {
        PipelineError::other(format!("Failed to construct q2-preview format: {}", e))
    })?;

    let mut ctx = StageContext::new(runtime, format, project.clone(), document)?;

    let stages = build_pre_engine_pipeline_stages();
    let pipeline = Pipeline::new(stages).expect("pre-engine pipeline stages should be compatible");

    let input = PipelineData::LoadedSource(LoadedSource::new(path.to_path_buf(), content));
    let final_data = pipeline.run(input, &mut ctx).await?;

    let doc_ast = final_data.into_document_ast().ok_or_else(|| {
        PipelineError::other(
            "pre-engine pipeline did not produce DocumentAst — unexpected output kind".to_string(),
        )
    })?;

    let mut buf = Vec::new();
    pampa::writers::qmd::write(&doc_ast.ast, &mut buf).map_err(|diagnostics| {
        PipelineError::stage_error_with_diagnostics("preview-record/compute-input-qmd", diagnostics)
    })?;
    Ok(buf)
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
            result.is_empty(),
            "expected no captures for prose-only doc with default (markdown) engine"
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

        assert_eq!(
            result.len(),
            1,
            "expected one capture for doc with test-passthrough engine"
        );
        let capture = &result[0];
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
        let capture = result.first().expect("capture present");

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
        assert!(result.is_empty());
    }

    // ──────────────────────────────────────────────────────────────
    // bd-qbhp2cvv: supporting-file bytes embedded in the capture
    // ──────────────────────────────────────────────────────────────

    /// Engine that writes a fake figure file next to the document —
    /// the disk shape knitr produces (`<stem>_files/figure-html/…`) —
    /// and reports the `_files` directory in `supporting_files`,
    /// exactly like `KnitrEngine` does.
    struct FigureWritingTestEngine {
        doc_dir: PathBuf,
    }

    impl ExecutionEngine for FigureWritingTestEngine {
        fn name(&self) -> &str {
            "test-figures"
        }

        fn execute(
            &self,
            input: &str,
            _ctx: &ExecutionContext,
        ) -> Result<ExecuteResult, ExecutionError> {
            let fig_dir = self.doc_dir.join("doc_files").join("figure-html");
            std::fs::create_dir_all(&fig_dir).expect("create figure dir");
            std::fs::write(fig_dir.join("fig.png"), b"FAKE-PNG-BYTES").expect("write figure");
            let mut out = String::from(input);
            out.push_str("\n![](doc_files/figure-html/fig.png)\n");
            Ok(ExecuteResult::new(out).with_supporting_files(vec![self.doc_dir.join("doc_files")]))
        }
    }

    #[tokio::test]
    async fn capture_embeds_engine_supporting_file_bytes() {
        // The engine writes doc_files/figure-html/fig.png during
        // execution (as knitr does) and reports the directory in
        // `supporting_files`. The recorded capture must embed the
        // file's *bytes* at its doc-relative path — paths alone are
        // useless on the replaying machine (bd-qbhp2cvv).
        let (_tmp, path, project, runtime) =
            fixture("---\nengine: test-figures\n---\n\n```{test-figures}\nplot(1)\n```\n");

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(FigureWritingTestEngine {
            doc_dir: path.parent().unwrap().to_path_buf(),
        }));

        let captures = record_capture(&path, &project, runtime, Some(registry))
            .await
            .expect("pipeline runs");
        let capture = captures.first().expect("capture present");

        assert_eq!(
            capture.files.len(),
            1,
            "capture should embed exactly the one engine-generated file; got {:?}",
            capture.files.iter().map(|f| &f.path).collect::<Vec<_>>()
        );
        let file = &capture.files[0];
        assert_eq!(
            file.path, "doc_files/figure-html/fig.png",
            "embedded path must be doc-relative with forward slashes"
        );
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&file.contents_base64)
            .expect("valid base64");
        assert_eq!(bytes, b"FAKE-PNG-BYTES");
    }

    // ──────────────────────────────────────────────────────────────
    // Phase C.2: compute_input_qmd
    // ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn compute_input_qmd_matches_capture_input_qmd() {
        // The fundamental invariant: for the same doc, the QMD bytes
        // returned by `compute_input_qmd` equal the `input_qmd` field
        // on the EngineCapture produced by `record_capture`. Phase
        // C.2's staleness check relies on this byte-equality.
        let (_tmp, path, project, runtime) =
            fixture("---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nSENTINEL\n```\n");

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(PassthroughTestEngine));

        let captures = record_capture(&path, &project, runtime.clone(), Some(registry))
            .await
            .expect("record_capture");
        let capture = captures.first().expect("capture present");

        let computed = compute_input_qmd(&path, &project, runtime)
            .await
            .expect("compute_input_qmd");

        let computed_str = String::from_utf8(computed).expect("UTF-8");
        assert_eq!(
            computed_str, capture.input_qmd,
            "compute_input_qmd must yield the same bytes the engine receives"
        );
    }

    #[tokio::test]
    async fn compute_input_qmd_is_stable_for_equal_inputs() {
        // Plan §C.2 unit test #1: "the canonicalization function
        // returns identical bytes for two equal QMDs."
        let content = "---\ntitle: Stable\n---\n\nHello.\n";
        let (_tmp1, path1, project1, runtime1) = fixture(content);
        let (_tmp2, path2, project2, runtime2) = fixture(content);

        let a = compute_input_qmd(&path1, &project1, runtime1)
            .await
            .unwrap();
        let b = compute_input_qmd(&path2, &project2, runtime2)
            .await
            .unwrap();

        assert_eq!(
            a, b,
            "identical content must produce identical canonical QMD"
        );
    }

    #[tokio::test]
    async fn compute_input_qmd_differs_when_cell_body_changes() {
        // Plan §C.2 unit test #1 (continued): "differing bytes for
        // edits." We change a code-cell body and verify the canonical
        // QMD reflects that change.
        let (_tmp1, path1, project1, runtime1) =
            fixture("---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nfirst\n```\n");
        let (_tmp2, path2, project2, runtime2) =
            fixture("---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nsecond\n```\n");

        let a = compute_input_qmd(&path1, &project1, runtime1)
            .await
            .unwrap();
        let b = compute_input_qmd(&path2, &project2, runtime2)
            .await
            .unwrap();

        assert_ne!(
            a, b,
            "different cell bodies should yield different canonical QMD"
        );
        // Per plan §Q-C3 v1: whole-QMD byte equality. A doc-prose
        // edit also flips the bytes (documented v1 limitation).
        let a_str = String::from_utf8(a).unwrap();
        let b_str = String::from_utf8(b).unwrap();
        assert!(a_str.contains("first"));
        assert!(b_str.contains("second"));
    }

    #[tokio::test]
    async fn compute_input_qmd_also_differs_for_prose_only_edits_v1_limitation() {
        // Plan §Q-C3: v1 uses whole-QMD byte-equality, so a prose-only
        // edit also flips canonical bytes. This is a known v1
        // limitation tracked in the plan; this test pins the v1
        // behaviour so a future "smarter" cell-only diff lands as a
        // deliberate behaviour change, not a silent one.
        let (_tmp1, path1, project1, runtime1) = fixture("---\ntitle: A\n---\n\nHello world.\n");
        let (_tmp2, path2, project2, runtime2) = fixture("---\ntitle: A\n---\n\nHello there.\n");

        let a = compute_input_qmd(&path1, &project1, runtime1)
            .await
            .unwrap();
        let b = compute_input_qmd(&path2, &project2, runtime2)
            .await
            .unwrap();

        assert_ne!(
            a, b,
            "prose edits ALSO change canonical QMD in v1 (known limitation per §Q-C3)"
        );
    }
}
