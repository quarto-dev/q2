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
use crate::record::{Record, read_record_value};
use crate::stage::stages::{ENGINE_CAPTURE_KIND, ENGINE_RECORD_KIND};
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
    /// EXR-5: the document's coalesced execution record (one per run).
    record: Arc<Mutex<Option<Record>>>,
}

impl CaptureCollector {
    fn new() -> Self {
        Self {
            captured: Arc::new(Mutex::new(Vec::new())),
            record: Arc::new(Mutex::new(None)),
        }
    }

    fn handle(&self) -> Arc<Mutex<Vec<EngineCapture>>> {
        Arc::clone(&self.captured)
    }

    fn record_handle(&self) -> Arc<Mutex<Option<Record>>> {
        Arc::clone(&self.record)
    }
}

/// What one capture run produced (EXR-5): the legacy per-engine captures, in
/// execution order, and the document's coalesced execution record. The capture
/// writers (the preview cache, the hub guest writer, `provide-hub`; the exact
/// list is EXR-13's) take this; `record_capture` returns only `captures` for the
/// callers that predate the record.
#[derive(Debug, Clone, Default)]
pub struct CaptureRun {
    pub captures: Vec<EngineCapture>,
    /// `None` when no engine ran.
    pub record: Option<Record>,
}

impl CaptureRun {
    /// Whether there is anything to publish: legacy captures, or a record that describes
    /// something. An engine run has captures; an imported record (no engines, no captures,
    /// cells) has only the second, so "no captures" must not mean "nothing to show".
    pub fn has_content(&self) -> bool {
        !self.captures.is_empty()
            || self
                .record
                .as_ref()
                .is_some_and(|r| r.describes_something())
    }
}

impl PipelineObserver for CaptureCollector {
    fn on_auxiliary_data(&self, _stage: &str, _index: usize, kind: &str, data: &serde_json::Value) {
        if kind == ENGINE_RECORD_KIND {
            match read_record_value(data.clone()) {
                Ok(record) => *self.record.lock().expect("record mutex poisoned") = Some(record),
                Err(e) => tracing::warn!(
                    "preview_record: failed to deserialize the execution record payload: {}",
                    e
                ),
            }
            return;
        }
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

    // EXR-5: the preview and hub writers carry the record (EXR-13, EXR-14).
    fn wants_execution_record(&self) -> bool {
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
///
/// The engine registry is no longer a stage-level concern (Task 8 of
/// ts-engine-extensions); `record_capture` sets `ctx.registry` directly
/// on the `StageContext` after construction.
fn build_capture_pipeline_stages() -> Vec<Box<dyn PipelineStage>> {
    let mut stages = build_html_pipeline_stages_with_options(None);
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
    engine_registry: Option<Arc<EngineRegistry>>,
) -> Result<Vec<EngineCapture>, PipelineError> {
    record_capture_run(path, project, runtime, engine_registry)
        .await
        .map(|run| run.captures)
}

/// [`record_capture`] with the document's execution record too (EXR-5): the
/// same pipeline, the same captures, plus the coalesced record the engine stage
/// emitted (`None` when no engine ran).
pub async fn record_capture_run(
    path: &std::path::Path,
    project: &ProjectContext,
    runtime: Arc<dyn SystemRuntime>,
    engine_registry: Option<Arc<EngineRegistry>>,
) -> Result<CaptureRun, PipelineError> {
    let content = runtime
        .file_read(path)
        .map_err(|e| PipelineError::other(format!("Failed to read {}: {}", path.display(), e)))?;

    let document = DocumentInfo::from_path(path);
    let format = Format::from_format_string("q2-preview").map_err(|e| {
        PipelineError::other(format!("Failed to construct q2-preview format: {}", e))
    })?;

    let collector = CaptureCollector::new();
    let captured = collector.handle();
    let recorded = collector.record_handle();

    let mut ctx = StageContext::new(runtime, format, project.clone(), document)?
        .with_observer(Arc::new(collector));
    // Apply the engine registry override when supplied (test seam); otherwise
    // the project's default registry (already on ctx.registry from
    // StageContext::new()) is used.  This mirrors the run_pipeline pattern:
    // StageContext::new() sets the default; callers override via the field.
    if let Some(reg) = engine_registry {
        ctx.registry = reg;
    }

    let stages = build_capture_pipeline_stages();
    let pipeline =
        Pipeline::new(stages).expect("preview-record pipeline stages should be compatible");

    let input = PipelineData::LoadedSource(LoadedSource::new(path.to_path_buf(), content));

    let _final_data = pipeline.run(input, &mut ctx).await?;

    let captures = std::mem::take(&mut *captured.lock().expect("capture mutex poisoned"));
    let record = recorded.lock().expect("record mutex poisoned").take();
    Ok(CaptureRun { captures, record })
}

/// Build the staleness-check sub-pipeline: every stage before
/// `EngineExecutionStage` (`PreEngineSugaringStage`, `LmdDesugarStage`,
/// and whatever else is inserted there later). The pipeline returns the
/// `DocumentAst` that `EngineExecutionStage` would serialize to QMD
/// and hand to the engine — i.e. exactly what
/// [`record_capture`] passes as `input_qmd`. The Phase C.2 staleness
/// check serializes the same AST and compares byte-for-byte against
/// the existing capture's `input_qmd`.
fn build_pre_engine_pipeline_stages() -> Vec<Box<dyn PipelineStage>> {
    let mut stages = build_html_pipeline_stages_with_options(None);
    if let Some(idx) = stages.iter().position(|s| s.name() == "engine-execution") {
        stages.truncate(idx);
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
    compute_source_inputs(path, project, runtime)
        .await
        .map(|inputs| inputs.input_qmd)
}

/// What the engine stage reads from the source (EXR-5): the canonical QMD bytes
/// of [`compute_input_qmd`] and the merged document metadata as the record's
/// document hash reads it (`ExecutionContext::document_metadata_json`).
#[derive(Debug, Clone, Default)]
pub struct SourceInputs {
    pub input_qmd: Vec<u8>,
    pub metadata: std::collections::BTreeMap<String, serde_json::Value>,
    /// The pre-sugaring snapshot of the same run: the author's text of each braced block, so
    /// the staleness check hashes units the way the record writer did (EXR-15).
    pub sugared_blocks: Vec<crate::record::SugaredBlock>,
    /// Language to owning engine, from the engine registry's resolution of the document.
    /// `None` unless the caller asked for it ([`SourceInputsOptions::ownership`]): only the
    /// Positron import needs it (the language gate), so every other caller pays nothing.
    pub ownership: Option<std::collections::BTreeMap<String, String>>,
}

impl SourceInputs {
    /// The engine that owns `lang`, case-insensitively; empty when nobody does (or when
    /// ownership was not requested), which no gate admits.
    pub fn owner_of(&self, lang: &str) -> String {
        let Some(map) = &self.ownership else {
            return String::new();
        };
        map.get(lang)
            .or_else(|| map.get(&lang.to_ascii_lowercase()))
            .cloned()
            .unwrap_or_default()
    }
}

/// What [`compute_source_inputs_with`] adds to the default run.
#[derive(Default, Clone)]
pub struct SourceInputsOptions {
    /// Also resolve the document's engines and return the language ownership map.
    pub ownership: bool,
    /// Replace the project's engine registry for the resolution (tests; the preview and the
    /// provider pass `None` and use the project's discovered registry, which is what a real
    /// run uses). It reaches the resolution through `StageContext::registry`.
    pub registry: Option<Arc<crate::engine::EngineRegistry>>,
}

/// [`compute_input_qmd`] plus the merged metadata, from one pre-engine run.
/// `provide-hub` calls it on the overlay-free runtime to store the source's
/// staleness key and recompute the record's hashes without the `-M` overlay
/// (`record::rehash_record`).
pub async fn compute_source_inputs(
    path: &std::path::Path,
    project: &ProjectContext,
    runtime: Arc<dyn SystemRuntime>,
) -> Result<SourceInputs, PipelineError> {
    compute_source_inputs_with(path, project, runtime, SourceInputsOptions::default()).await
}

/// [`compute_source_inputs`] with options (RIU-3a): the language ownership map and a registry
/// override.
pub async fn compute_source_inputs_with(
    path: &std::path::Path,
    project: &ProjectContext,
    runtime: Arc<dyn SystemRuntime>,
    options: SourceInputsOptions,
) -> Result<SourceInputs, PipelineError> {
    let content = runtime
        .file_read(path)
        .map_err(|e| PipelineError::other(format!("Failed to read {}: {}", path.display(), e)))?;

    let document = DocumentInfo::from_path(path);
    let format = Format::from_format_string("q2-preview").map_err(|e| {
        PipelineError::other(format!("Failed to construct q2-preview format: {}", e))
    })?;

    let mut ctx = StageContext::new(runtime, format, project.clone(), document)?;
    if let Some(registry) = options.registry {
        ctx.registry = registry;
    }

    let stages = build_pre_engine_pipeline_stages();
    let pipeline = Pipeline::new(stages).expect("pre-engine pipeline stages should be compatible");

    let input = PipelineData::LoadedSource(LoadedSource::new(path.to_path_buf(), content));
    let final_data = pipeline.run(input, &mut ctx).await?;

    let mut doc_ast = final_data.into_document_ast().ok_or_else(|| {
        PipelineError::other(
            "pre-engine pipeline did not produce DocumentAst — unexpected output kind".to_string(),
        )
    })?;

    // The merged metadata, lowered as `EngineExecutionStage` lowers it for the
    // engines (before masking, which rewrites spans in front-matter values).
    let metadata = match serde_json::to_value(crate::project::document_metadata_to_ts_map(
        &doc_ast.ast.meta,
    )) {
        Ok(serde_json::Value::Object(map)) => map.into_iter().collect(),
        _ => std::collections::BTreeMap::new(),
    };

    // The owner of each language, resolved on the parsed document before it is masked, exactly
    // as `EngineExecutionStage` resolves it. Resolution may load extension engines but never
    // runs a cell.
    let ownership = options.ownership.then(|| {
        crate::engine::resolve_engines_for_document(
            &doc_ast.ast.meta,
            &doc_ast.ast,
            &ctx.registry,
            ctx.claimed_engine_name.as_deref(),
            Some(crate::engine::DOCUMENT_OWN_FILE_ID),
        )
        .ownership
        .into_iter()
        .collect()
    });

    // nested-cell-masking (spec § "The engine capture — asymmetric, by
    // necessity" / `compute_input_qmd`'s three-consumer table): these bytes
    // must match what `EngineExecutionStage` hands the engine as
    // `input_qmd` — masked. The two consumers that read this function's
    // output directly (the staleness compare in `capture_driver.rs` and the
    // cache key in `cache.rs`) both want masked bytes; only
    // `write_review_file` (`quarto-hub-provider/src/execute.rs`) unmasks,
    // and it does so itself after calling this function.
    crate::engine::nested_cell_mask::mask_document(&mut doc_ast.ast);

    let mut buf = Vec::new();
    pampa::writers::qmd::write(&doc_ast.ast, &mut buf).map_err(|diagnostics| {
        PipelineError::stage_error_with_diagnostics("preview-record/compute-input-qmd", diagnostics)
    })?;
    Ok(SourceInputs {
        input_qmd: buf,
        metadata,
        sugared_blocks: std::mem::take(&mut ctx.sugared_blocks),
        ownership,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A run has content when it has captures or a record that describes something. An import
    /// has the second and never the first.
    #[test]
    fn a_run_has_content_with_captures_or_a_record_that_describes_something() {
        let record =
            crate::record::read_record(include_str!("../../tests/fixtures/records/basic.json"))
                .unwrap();
        let mut nothing = record.clone();
        nothing.metadata.quarto_engines.clear();
        nothing.cells.clear();
        let capture: EngineCapture = serde_json::from_value(serde_json::json!({
            "engine_name": "jupyter", "input_qmd": "x", "result": {}
        }))
        .unwrap();

        assert!(!CaptureRun::default().has_content());
        assert!(
            !CaptureRun {
                captures: Vec::new(),
                record: Some(nothing.clone())
            }
            .has_content()
        );
        assert!(
            CaptureRun {
                captures: Vec::new(),
                record: Some(record.clone())
            }
            .has_content()
        );
        assert!(
            CaptureRun {
                captures: vec![capture],
                record: Some(nothing)
            }
            .has_content()
        );
    }

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

        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> crate::engine::LanguageClaim {
            if language == "test-passthrough" {
                crate::engine::LanguageClaim::Primary(1)
            } else {
                crate::engine::LanguageClaim::None
            }
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

        let result = record_capture(&path, &project, runtime, Some(Arc::new(registry)))
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

        let result = record_capture(&path, &project, runtime, Some(Arc::new(registry)))
            .await
            .expect("pipeline runs");
        let capture = result.first().expect("capture present");

        assert!(
            capture.input_qmd.contains("SENTINEL"),
            "input_qmd should retain the cell body; got: {}",
            capture.input_qmd
        );
    }

    /// F8-D9: a `-M` overlay on the runtime reaches the engine (the input it
    /// receives is the merged document), and the overlay-free recompute
    /// differs from what was recorded, which is why `provide-hub` stores the
    /// recompute as the staleness key.
    #[tokio::test]
    async fn runtime_metadata_reaches_the_engine_input() {
        use crate::overlay_runtime::RenderOverlay;

        let (_tmp, path, project, plain) =
            fixture("---\nengine: test-passthrough\n---\n\n```{test-passthrough}\n1\n```\n");
        let overlay = Arc::new(RenderOverlay::with_metadata(
            serde_json::json!({ "subtitle": "ZZOVERLAY" }),
        ));
        let runtime = overlay.apply(plain.clone());
        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(PassthroughTestEngine));

        let captures = record_capture(&path, &project, runtime, Some(Arc::new(registry)))
            .await
            .expect("pipeline runs");
        let recorded = &captures[0].input_qmd;
        assert!(recorded.contains("ZZOVERLAY"), "{recorded}");
        let markdown = captures[0].result.get("markdown").and_then(|v| v.as_str());
        assert!(markdown.unwrap().contains("ZZOVERLAY"));

        let source = compute_input_qmd(&path, &project, plain).await.unwrap();
        assert_ne!(source, recorded.as_bytes());
        assert!(!String::from_utf8(source).unwrap().contains("ZZOVERLAY"));
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

        fn claims_language(
            &self,
            language: &str,
            _first_class: Option<&str>,
        ) -> crate::engine::LanguageClaim {
            if language == "test-figures" {
                crate::engine::LanguageClaim::Primary(1)
            } else {
                crate::engine::LanguageClaim::None
            }
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

        let captures = record_capture(&path, &project, runtime, Some(Arc::new(registry)))
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

        let captures = record_capture(&path, &project, runtime.clone(), Some(Arc::new(registry)))
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

    /// T20 (hunk H10): `compute_input_qmd` must mask executable fence
    /// openers nested inside a *displayed* code block — the same masked
    /// bytes the engine-execution loop hands to the engine as `input_qmd`
    /// (once H8 lands there too). `compute_input_qmd_matches_capture_input_qmd`
    /// above uses a fixture with **no** display block, so it stays green
    /// through a regression here; this fixture adds one, closing that gap.
    ///
    /// The first assertion (masked marker present) is what makes this test
    /// genuinely red today: neither `compute_input_qmd` nor `record_capture`
    /// masks anything yet, so today's bytes carry the bare `{r}` opener, not
    /// `q2-nested-executable`. The second assertion protects the
    /// fully-implemented invariant (compute == capture) once H8 and H10 both
    /// land.
    #[tokio::test]
    async fn compute_input_qmd_masks_nested_display_fence() {
        let (_tmp, path, project, runtime) = fixture(
            "---\nengine: test-passthrough\n---\n\n```{test-passthrough}\nSENTINEL\n```\n\n```markdown\n```{r}\n1 + 1\n```\n```\n",
        );

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(PassthroughTestEngine));

        let captures = record_capture(&path, &project, runtime.clone(), Some(Arc::new(registry)))
            .await
            .expect("record_capture");
        let capture = captures.first().expect("capture present");

        let computed = compute_input_qmd(&path, &project, runtime)
            .await
            .expect("compute_input_qmd");
        let computed_str = String::from_utf8(computed).expect("UTF-8");

        assert!(
            computed_str.contains("q2-nested-executable"),
            "compute_input_qmd must mask the nested `{{r}}` opener inside the \
             display block so quarto-preview's staleness compare and cache key \
             see masked bytes (spec: \"The engine capture — asymmetric, by \
             necessity\"); got:\n{computed_str}"
        );
        assert_eq!(
            computed_str, capture.input_qmd,
            "compute_input_qmd must yield the same (masked) bytes the engine \
             receives, even for docs with a display block"
        );
    }

    /// EXR-2 decision 11: the stage masks spans in front-matter values the
    /// document does not own (here a `_quarto.yml` value), so
    /// `compute_input_qmd` must too, or the staleness compare and the cache key
    /// would see different bytes than the engine does.
    #[tokio::test]
    async fn compute_input_qmd_masks_foreign_front_matter_spans() {
        let dir = tempfile::tempdir().expect("create tempdir");
        std::fs::write(
            dir.path().join("_quarto.yml"),
            "project:\n  type: default\nsubtitle: \"proj `{python} 2`\"\n",
        )
        .unwrap();
        let path = dir.path().join("doc.qmd");
        std::fs::write(
            &path,
            "---\nengine: test-passthrough\ntitle: \"own `{python} 1`\"\n---\n\n```{test-passthrough}\nSENTINEL\n```\n",
        )
        .unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(dir.path(), runtime.as_ref()).expect("discover");

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(PassthroughTestEngine));
        let captures = record_capture(&path, &project, runtime.clone(), Some(Arc::new(registry)))
            .await
            .expect("record_capture");
        let capture = captures.first().expect("capture present");

        let computed = String::from_utf8(
            compute_input_qmd(&path, &project, runtime)
                .await
                .expect("compute_input_qmd"),
        )
        .unwrap();

        assert!(
            computed.contains("`q2-nested-executable {python} 2`"),
            "the project's span must be masked:\n{computed}"
        );
        assert!(
            computed.contains("own `{python} 1`"),
            "the document's own span must not be:\n{computed}"
        );
        assert_eq!(computed, capture.input_qmd);
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
