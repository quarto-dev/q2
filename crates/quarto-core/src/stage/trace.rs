/*
 * stage/trace.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pipeline tracing observers for debugging and diagnostics.
 */

//! Concrete [`PipelineObserver`] implementations for tracing pipeline execution.
//!
//! - [`JsonTraceObserver`]: Captures full pipeline state at each stage boundary
//!   and writes a JSON trace file to `.quarto/trace/`.
//! - [`SummaryTraceObserver`]: Prints a human-readable summary to stderr.
//!
//! Both observers use `std::time::Instant` and `std::fs`, which are
//! unavailable on `wasm32-unknown-unknown`, so the module is gated to
//! native targets. On WASM, `activate_trace_from_metadata` installs a
//! no-op observer instead (see `metadata_merge.rs`).

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use quarto_trace::{RenderInfo, StageErrorInfo, StageStatus, TraceDocument, TraceEntry};

use super::data::{PipelineData, PipelineDataKind};
use super::error::PipelineError;
use super::observer::{EventLevel, PipelineObserver};
use super::stages::ENGINE_CAPTURE_KIND;

// ─── JsonTraceObserver ───────────────────────────────────────────────────────

/// Internal mutable state for `JsonTraceObserver`.
#[derive(Debug)]
struct JsonTraceState {
    doc: TraceDocument,
    /// When the current stage started (set in on_stage_start).
    stage_start: Option<Instant>,
    /// When the pipeline started (for total_duration_ms).
    pipeline_start: Option<Instant>,
}

/// Observer that captures full pipeline state at each stage boundary.
///
/// After the pipeline completes (or errors), the observer flushes the
/// accumulated [`TraceDocument`] to its output path via
/// [`quarto_trace::write::write_trace`].
pub struct JsonTraceObserver {
    state: Mutex<JsonTraceState>,
    output_path: PathBuf,
}

impl JsonTraceObserver {
    /// Create a new JSON trace observer.
    ///
    /// `render` carries what the call site already knows (input path,
    /// format target, git hash); the observer fills in the rest
    /// (`started_at_unix_ms`, `total_duration_ms`, `output_path`) as the
    /// pipeline runs.
    pub fn new(output_path: PathBuf, render: RenderInfo) -> Self {
        Self {
            state: Mutex::new(JsonTraceState {
                doc: TraceDocument::new(render),
                stage_start: None,
                pipeline_start: None,
            }),
            output_path,
        }
    }

    /// Write the collected trace to the output file.
    pub fn write_trace(&self) -> std::io::Result<()> {
        let state = self.state.lock().unwrap();
        quarto_trace::write::write_trace(&state.doc, &self.output_path).map_err(|e| match e {
            quarto_trace::write::WriteError::Io { source, .. } => source,
            quarto_trace::write::WriteError::Json(e) => std::io::Error::other(e),
        })
    }

    /// Get the output path.
    pub fn output_path(&self) -> &PathBuf {
        &self.output_path
    }

    fn push_entry(state: &mut JsonTraceState, entry: TraceEntry) {
        // If this entry's data is a FinalOutput, grab the output path for
        // RenderInfo while we have it.
        if state.doc.render.output_path.is_none()
            && let Some(data) = &entry.data
            && let Some(output_path) = data.get("output_path").and_then(|v| v.as_str())
        {
            // Only FinalOutput entries emit a top-level "output_path"
            // field via serialize_pipeline_data below, so this is a
            // safe heuristic.
            if entry.data_kind.as_deref() == Some("FinalOutput") {
                state.doc.render.output_path = Some(output_path.to_string());
            }
        }
        state.doc.pipeline.push(entry);
    }
}

impl PipelineObserver for JsonTraceObserver {
    fn on_pipeline_start(&self, _total_stages: usize) {
        let mut state = self.state.lock().unwrap();
        state.pipeline_start = Some(Instant::now());
        if state.doc.render.started_at_unix_ms.is_none() {
            state.doc.render.started_at_unix_ms = Some(now_unix_ms());
        }
    }

    fn on_stage_start(&self, _name: &str, _index: usize, _total: usize) {
        let mut state = self.state.lock().unwrap();
        state.stage_start = Some(Instant::now());
    }

    fn on_pipeline_input(&self, data: &PipelineData) {
        let data_json = serialize_pipeline_data(data);
        let mut state = self.state.lock().unwrap();
        // Populate input_path from the initial input if not already set.
        if state.doc.render.input_path.is_none()
            && let PipelineData::LoadedSource(s) = data
        {
            state.doc.render.input_path = Some(s.path.display().to_string());
        }
        Self::push_entry(
            &mut state,
            TraceEntry {
                stage: "__input".to_string(),
                index: 0,
                data_kind: Some(data.kind().to_string()),
                data: Some(data_json),
                duration_ms: None,
                status: StageStatus::Ok,
                error: None,
            },
        );
    }

    fn on_stage_data(&self, name: &str, index: usize, data: &PipelineData) {
        let data_json = serialize_pipeline_data(data);
        let mut state = self.state.lock().unwrap();
        let duration_ms = state
            .stage_start
            .map(|start| start.elapsed().as_secs_f64() * 1000.0);
        Self::push_entry(
            &mut state,
            TraceEntry {
                stage: name.to_string(),
                index,
                data_kind: Some(data.kind().to_string()),
                data: Some(data_json),
                duration_ms,
                status: StageStatus::Ok,
                error: None,
            },
        );
    }

    fn on_stage_error(&self, name: &str, index: usize, error: &PipelineError) {
        let mut state = self.state.lock().unwrap();
        let duration_ms = state
            .stage_start
            .map(|start| start.elapsed().as_secs_f64() * 1000.0);
        Self::push_entry(
            &mut state,
            TraceEntry {
                stage: name.to_string(),
                index,
                data_kind: None,
                data: None,
                duration_ms,
                status: StageStatus::Error,
                error: Some(StageErrorInfo {
                    message: error.to_string(),
                }),
            },
        );
    }

    fn on_transform_data(
        &self,
        name: &str,
        index: usize,
        _total: usize,
        ast: &quarto_pandoc_types::pandoc::Pandoc,
        ast_context: &pampa::pandoc::ASTContext,
    ) {
        let data_json = serialize_pandoc_ast(ast, ast_context);
        let mut state = self.state.lock().unwrap();
        Self::push_entry(
            &mut state,
            TraceEntry {
                stage: format!("transform:{}", name),
                index,
                data_kind: Some(PipelineDataKind::DocumentAst.to_string()),
                data: Some(data_json),
                duration_ms: None,
                status: StageStatus::Ok,
                error: None,
            },
        );
    }

    fn on_engine_data(
        &self,
        engine_name: &str,
        index: usize,
        ast: &quarto_pandoc_types::pandoc::Pandoc,
        ast_context: &pampa::pandoc::ASTContext,
    ) {
        // bd-5yff4: one AST snapshot per engine in a multi-engine
        // sequence, recorded as an `engine:<name>` entry — mirrors the
        // `transform:<name>` sub-entries. The dedup pass collapses
        // snapshots that are byte-identical to a neighbor, so the size
        // cost is bounded.
        let data_json = serialize_pandoc_ast(ast, ast_context);
        let mut state = self.state.lock().unwrap();
        Self::push_entry(
            &mut state,
            TraceEntry {
                stage: format!("engine:{}", engine_name),
                index,
                data_kind: Some(PipelineDataKind::DocumentAst.to_string()),
                data: Some(data_json),
                duration_ms: None,
                status: StageStatus::Ok,
                error: None,
            },
        );
    }

    fn on_auxiliary_data(&self, stage: &str, index: usize, kind: &str, data: &serde_json::Value) {
        let mut state = self.state.lock().unwrap();
        // bd-45yw: route the typed engine capture to the dedicated
        // slot on TraceDocument so the trace doubles as a replay
        // fixture. Other kinds stay on the open-ended pipeline aux
        // channel.
        if kind == ENGINE_CAPTURE_KIND {
            match serde_json::from_value::<quarto_trace::EngineCapture>(data.clone()) {
                Ok(capture) => {
                    // bd-5yff4: one capture per engine, in execution order.
                    // The stage emits these sequentially (run_index order),
                    // so push preserves order.
                    state.doc.engine_captures.push(capture);
                    return;
                }
                Err(e) => {
                    // Malformed capture: fall through to recording it
                    // as a generic aux entry so investigators see the
                    // payload and a paired error message.
                    eprintln!(
                        "Warning: malformed EngineCapture aux payload from stage '{stage}': {e}"
                    );
                }
            }
        }
        Self::push_entry(
            &mut state,
            TraceEntry {
                stage: format!("aux:{}", stage),
                index,
                data_kind: Some(kind.to_string()),
                data: Some(data.clone()),
                duration_ms: None,
                status: StageStatus::Ok,
                error: None,
            },
        );
    }

    fn on_pipeline_complete(&self) {
        {
            let mut state = self.state.lock().unwrap();
            if let Some(start) = state.pipeline_start {
                state.doc.render.total_duration_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        if let Err(e) = self.write_trace() {
            eprintln!("Warning: failed to write pipeline trace: {}", e);
        }
    }

    fn on_pipeline_error(&self, _error: &PipelineError) {
        // Still write what we have on error. The errored stage has already
        // been recorded via on_stage_error.
        {
            let mut state = self.state.lock().unwrap();
            if let Some(start) = state.pipeline_start {
                state.doc.render.total_duration_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        if let Err(e) = self.write_trace() {
            eprintln!("Warning: failed to write pipeline trace: {}", e);
        }
    }
}

impl std::fmt::Debug for JsonTraceObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonTraceObserver")
            .field("output_path", &self.output_path)
            .finish()
    }
}

// ─── SummaryTraceObserver ────────────────────────────────────────────────────

/// Internal mutable state for SummaryTraceObserver.
#[derive(Debug)]
struct SummaryTraceState {
    stage_start: Option<Instant>,
    pipeline_start: Option<Instant>,
}

/// Observer that prints a human-readable summary of pipeline execution to stderr.
pub struct SummaryTraceObserver {
    state: Mutex<SummaryTraceState>,
}

impl SummaryTraceObserver {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SummaryTraceState {
                stage_start: None,
                pipeline_start: None,
            }),
        }
    }
}

impl Default for SummaryTraceObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineObserver for SummaryTraceObserver {
    fn on_pipeline_start(&self, total_stages: usize) {
        let mut state = self.state.lock().unwrap();
        state.pipeline_start = Some(Instant::now());
        eprintln!("[trace] Pipeline starting ({} stages)", total_stages);
    }

    fn on_stage_start(&self, name: &str, index: usize, total: usize) {
        let mut state = self.state.lock().unwrap();
        state.stage_start = Some(Instant::now());
        eprintln!("[trace] [{}/{}] {} ...", index + 1, total, name);
    }

    fn on_stage_data(&self, name: &str, _index: usize, data: &PipelineData) {
        let state = self.state.lock().unwrap();
        let duration_str = match state.stage_start {
            Some(start) => format!(" ({:.1}ms)", start.elapsed().as_secs_f64() * 1000.0),
            None => String::new(),
        };

        let detail = pipeline_data_summary(data);
        eprintln!("[trace]   -> {}: {}{}", name, detail, duration_str);
    }

    fn on_transform_data(
        &self,
        name: &str,
        index: usize,
        total: usize,
        ast: &quarto_pandoc_types::pandoc::Pandoc,
        _ast_context: &pampa::pandoc::ASTContext,
    ) {
        let block_count = ast.blocks.len();
        eprintln!(
            "[trace]     transform [{}/{}] {}: {} blocks",
            index + 1,
            total,
            name,
            block_count
        );
    }

    fn on_pipeline_complete(&self) {
        let state = self.state.lock().unwrap();
        let duration_str = match state.pipeline_start {
            Some(start) => format!(" in {:.1}ms", start.elapsed().as_secs_f64() * 1000.0),
            None => String::new(),
        };
        eprintln!("[trace] Pipeline complete{}", duration_str);
    }

    fn on_pipeline_error(&self, error: &PipelineError) {
        eprintln!("[trace] Pipeline failed: {}", error);
    }

    fn on_event(&self, message: &str, level: EventLevel) {
        eprintln!("[trace] [{}] {}", level.as_str(), message);
    }
}

impl std::fmt::Debug for SummaryTraceObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SummaryTraceObserver").finish()
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn now_unix_ms() -> f64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as f64) * 1000.0 + (d.subsec_nanos() as f64) / 1_000_000.0,
        Err(_) => 0.0,
    }
}

/// Serialize `PipelineData` to a JSON value for tracing.
fn serialize_pipeline_data(data: &PipelineData) -> serde_json::Value {
    match data {
        PipelineData::LoadedSource(s) => serde_json::json!({
            "path": s.path.display().to_string(),
            // `Some(Qmd)` → `"Qmd"`, `None` → JSON `null`. Rendering the
            // Option's Debug form directly would put the string
            // `"Some(Qmd)"` on the wire; consumers want the variant.
            // `null` is the honest encoding of "unknown extension, not yet
            // converted" (see `LoadedSource::source_type`).
            "source_type": s.source_type.map(|t| format!("{t:?}")),
            "content_length": s.content.len(),
        }),
        PipelineData::DocumentSource(s) => serde_json::json!({
            "path": s.path.display().to_string(),
            "markdown_length": s.markdown.len(),
            "markdown": s.markdown,
        }),
        PipelineData::DocumentAst(doc) => {
            let ast_json = serialize_pandoc_ast(&doc.ast, &doc.ast_context);
            serde_json::json!({
                "path": doc.path.display().to_string(),
                "ast": ast_json,
                "warnings_count": doc.warnings.len(),
            })
        }
        PipelineData::AtProfile(bundle) => {
            let ast_json = serialize_pandoc_ast(&bundle.ast.ast, &bundle.ast.ast_context);
            serde_json::json!({
                "path": bundle.ast.path.display().to_string(),
                "ast": ast_json,
                "warnings_count": bundle.ast.warnings.len(),
                "profile": &bundle.profile,
            })
        }
        PipelineData::ExecutedDocument(doc) => serde_json::json!({
            "path": doc.path.display().to_string(),
            "markdown_length": doc.markdown.len(),
            "markdown": doc.markdown,
            "supporting_files": doc.supporting_files.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>(),
            "filters": doc.filters,
        }),
        PipelineData::RenderedOutput(r) => serde_json::json!({
            "input_path": r.input_path.display().to_string(),
            "output_path": r.output_path.display().to_string(),
            "format": format!("{:?}", r.format.identifier),
            "content_length": r.content.len(),
            "content": r.content,
            "is_intermediate": r.is_intermediate,
            "supporting_files": r.supporting_files.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>(),
        }),
        PipelineData::FinalOutput(f) => serde_json::json!({
            "input_path": f.input_path.display().to_string(),
            "output_path": f.output_path.display().to_string(),
            "format": format!("{:?}", f.format.identifier),
            "supporting_files": f.supporting_files.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>(),
            "warnings_count": f.warnings.len(),
        }),
    }
}

/// Serialize a Pandoc AST to a JSON value using pampa's JSON writer.
///
/// The caller supplies the real [`ASTContext`] so source-attribution metadata
/// (filenames, source info pool) is preserved in the trace. Passing an
/// anonymous context here would produce bogus `astContext.files` entries.
fn serialize_pandoc_ast(
    ast: &quarto_pandoc_types::pandoc::Pandoc,
    context: &pampa::pandoc::ASTContext,
) -> serde_json::Value {
    let mut buf = Vec::new();
    match pampa::writers::json::write(ast, context, &mut buf) {
        Ok(()) => serde_json::from_slice(&buf).unwrap_or_else(|_| {
            serde_json::json!({
                "__error": "Failed to parse JSON output",
                "block_count": ast.blocks.len(),
            })
        }),
        Err(_) => serde_json::json!({
            "__error": "Failed to serialize AST to JSON",
            "block_count": ast.blocks.len(),
        }),
    }
}

fn pipeline_data_summary(data: &PipelineData) -> String {
    match data {
        PipelineData::LoadedSource(s) => format!(
            "LoadedSource({}, {:?}, {} bytes)",
            s.path.display(),
            s.source_type,
            s.content.len()
        ),
        PipelineData::DocumentSource(s) => format!(
            "DocumentSource({}, {} chars)",
            s.path.display(),
            s.markdown.len()
        ),
        PipelineData::DocumentAst(doc) => format!(
            "DocumentAst({}, {} blocks)",
            doc.path.display(),
            doc.ast.blocks.len()
        ),
        PipelineData::AtProfile(bundle) => format!(
            "AtProfile({}, {} blocks, profile_v{})",
            bundle.ast.path.display(),
            bundle.ast.ast.blocks.len(),
            bundle.profile.profile_version,
        ),
        PipelineData::ExecutedDocument(doc) => format!(
            "ExecutedDocument({}, {} chars, {} supporting files)",
            doc.path.display(),
            doc.markdown.len(),
            doc.supporting_files.len()
        ),
        PipelineData::RenderedOutput(r) => format!(
            "RenderedOutput({}, {} chars)",
            r.output_path.display(),
            r.content.len()
        ),
        PipelineData::FinalOutput(f) => format!("FinalOutput({})", f.output_path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::data::LoadedSource;

    #[test]
    fn test_serialize_loaded_source() {
        let data = PipelineData::LoadedSource(LoadedSource::new(
            PathBuf::from("test.qmd"),
            b"# Hello".to_vec(),
        ));

        let json = serialize_pipeline_data(&data);
        assert_eq!(json["path"], "test.qmd");
        assert_eq!(json["content_length"], 7);
        assert_eq!(json["source_type"], "Qmd");
    }

    #[test]
    fn test_serialize_document_ast() {
        let ast = quarto_pandoc_types::pandoc::Pandoc::default();
        let doc = crate::stage::DocumentAst {
            path: PathBuf::from("test.qmd"),
            ast,
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: quarto_source_map::SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
        };

        let data = PipelineData::DocumentAst(doc);
        let json = serialize_pipeline_data(&data);
        assert_eq!(json["path"], "test.qmd");
        assert!(json["ast"].is_object());
        assert_eq!(json["warnings_count"], 0);
    }

    /// Trace serializer must use the real ASTContext from DocumentAst,
    /// not a fresh anonymous one. Regression test for bd-b0f2.
    #[test]
    fn test_document_ast_trace_preserves_filenames() {
        let ast = quarto_pandoc_types::pandoc::Pandoc::default();
        let ast_context = pampa::pandoc::ASTContext::with_filename("hello.qmd");
        let doc = crate::stage::DocumentAst {
            path: PathBuf::from("hello.qmd"),
            ast,
            ast_context,
            source_context: quarto_source_map::SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
        };

        let data = PipelineData::DocumentAst(doc);
        let json = serialize_pipeline_data(&data);

        let files = &json["ast"]["astContext"]["files"];
        assert!(files.is_array(), "astContext.files should be an array");
        let files = files.as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0]["name"], "hello.qmd",
            "serialized trace must carry the real filename, got: {:?}",
            files[0]
        );
    }

    /// Trace observer must preserve the filename on `on_transform_data`
    /// entries. Regression test for bd-b0f2 covering per-transform
    /// observability inside `AstTransformsStage`.
    #[test]
    fn test_on_transform_data_preserves_filenames() {
        let observer =
            JsonTraceObserver::new(PathBuf::from("/tmp/test-trace.json"), RenderInfo::default());

        let ast = quarto_pandoc_types::pandoc::Pandoc::default();
        let ast_context = pampa::pandoc::ASTContext::with_filename("foo.qmd");

        observer.on_transform_data("callout", 0, 1, &ast, &ast_context);

        let state = observer.state.lock().unwrap();
        assert_eq!(state.doc.pipeline.len(), 1);
        let entry = &state.doc.pipeline[0];
        assert_eq!(entry.stage, "transform:callout");
        let data = entry.data.as_ref().expect("entry should have data");
        let files = &data["astContext"]["files"];
        assert!(files.is_array(), "astContext.files should be an array");
        let files = files.as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["name"], "foo.qmd");
    }

    #[test]
    fn test_pipeline_data_summary() {
        let data = PipelineData::LoadedSource(LoadedSource::new(
            PathBuf::from("test.qmd"),
            b"hello".to_vec(),
        ));

        let summary = pipeline_data_summary(&data);
        assert!(summary.contains("LoadedSource"));
        assert!(summary.contains("test.qmd"));
        assert!(summary.contains("5 bytes"));
    }

    #[test]
    fn test_json_trace_observer_collects_entries() {
        let observer =
            JsonTraceObserver::new(PathBuf::from("/tmp/test-trace.json"), RenderInfo::default());

        let data = PipelineData::LoadedSource(LoadedSource::new(
            PathBuf::from("test.qmd"),
            b"# Hello".to_vec(),
        ));

        observer.on_pipeline_start(2);
        observer.on_pipeline_input(&data);
        observer.on_stage_start("parse", 0, 2);
        observer.on_stage_data("parse", 0, &data);
        observer.on_stage_start("transform", 1, 2);
        observer.on_stage_data("transform", 1, &data);

        let state = observer.state.lock().unwrap();
        // __input + parse + transform = 3 entries
        assert_eq!(state.doc.pipeline.len(), 3);
        assert_eq!(state.doc.pipeline[0].stage, "__input");
        assert_eq!(state.doc.pipeline[1].stage, "parse");
        assert_eq!(state.doc.pipeline[2].stage, "transform");
        assert_eq!(state.doc.render.input_path.as_deref(), Some("test.qmd"));
    }

    /// bd-45yw: an `on_auxiliary_data` event with kind
    /// `"EngineCapture"` must land on `TraceDocument.engine_captures`
    /// (the typed slot), not as a generic pipeline aux entry. Recording
    /// the engine output is what makes a trace double as a replay
    /// fixture.
    #[test]
    fn test_json_trace_observer_routes_engine_capture_aux_to_typed_slot() {
        let observer =
            JsonTraceObserver::new(PathBuf::from("/tmp/test-trace.json"), RenderInfo::default());

        let capture_json = serde_json::json!({
            "engine_name": "jupyter",
            "input_qmd": "---\nengine: jupyter\n---\n",
            "result": {
                "markdown": "out\n",
                "supporting_files": ["fig.png"],
                "filters": [],
                "includes": {
                    "header_includes": [],
                    "include_before": [],
                    "include_after": [],
                },
                "needs_postprocess": false,
            }
        });

        observer.on_auxiliary_data("engine-execution", 1, "EngineCapture", &capture_json);

        let state = observer.state.lock().unwrap();
        // No generic aux entry should be appended for the typed kind.
        assert!(
            !state
                .doc
                .pipeline
                .iter()
                .any(|e| e.stage == "aux:engine-execution"),
            "EngineCapture should be routed to engine_captures, not pipeline aux"
        );
        // The typed slot must hold the capture.
        assert_eq!(
            state.doc.engine_captures.len(),
            1,
            "exactly one capture should be recorded"
        );
        let cap = &state.doc.engine_captures[0];
        assert_eq!(cap.engine_name, "jupyter");
        assert_eq!(cap.input_qmd, "---\nengine: jupyter\n---\n");
        assert_eq!(cap.result["markdown"], "out\n");
    }

    /// Other aux kinds remain unaffected — the engine-capture special
    /// case must not regress the generic open-ended aux channel.
    #[test]
    fn test_json_trace_observer_keeps_other_aux_in_pipeline() {
        let observer =
            JsonTraceObserver::new(PathBuf::from("/tmp/test-trace.json"), RenderInfo::default());

        observer.on_auxiliary_data(
            "crossref-index",
            0,
            "CrossrefIndex",
            &serde_json::json!({"entries": []}),
        );

        let state = observer.state.lock().unwrap();
        assert!(state.doc.engine_captures.is_empty());
        assert_eq!(state.doc.pipeline.len(), 1);
        assert_eq!(state.doc.pipeline[0].stage, "aux:crossref-index");
        assert_eq!(
            state.doc.pipeline[0].data_kind.as_deref(),
            Some("CrossrefIndex")
        );
    }

    #[test]
    fn test_json_trace_observer_records_error() {
        let observer = JsonTraceObserver::new(
            PathBuf::from("/tmp/test-trace-err.json"),
            RenderInfo::default(),
        );

        let data = PipelineData::LoadedSource(LoadedSource::new(
            PathBuf::from("test.qmd"),
            b"# Hello".to_vec(),
        ));

        observer.on_pipeline_start(2);
        observer.on_pipeline_input(&data);
        observer.on_stage_start("parse", 0, 2);
        let err = PipelineError::stage_error("parse", "boom");
        observer.on_stage_error("parse", 0, &err);

        let state = observer.state.lock().unwrap();
        // __input + parse(errored) = 2 entries
        assert_eq!(state.doc.pipeline.len(), 2);
        let errored = &state.doc.pipeline[1];
        assert_eq!(errored.stage, "parse");
        assert_eq!(errored.status, StageStatus::Error);
        assert!(errored.data.is_none());
        assert!(errored.error.is_some());
    }

    #[test]
    fn test_json_trace_observer_writes_file() {
        let dir = std::env::temp_dir().join("quarto-trace-test-core");
        let output_path = dir.join("trace.json");

        let _ = std::fs::remove_dir_all(&dir);

        let observer = JsonTraceObserver::new(
            output_path.clone(),
            RenderInfo {
                input_path: Some("test.qmd".into()),
                format_target: Some("html".into()),
                git_hash: Some(quarto_trace::BUILD_GIT_HASH.to_string()),
                ..Default::default()
            },
        );

        let data = PipelineData::LoadedSource(LoadedSource::new(
            PathBuf::from("test.qmd"),
            b"# Hello".to_vec(),
        ));

        observer.on_pipeline_start(1);
        observer.on_pipeline_input(&data);
        observer.on_stage_start("parse", 0, 1);
        observer.on_stage_data("parse", 0, &data);

        observer.write_trace().unwrap();

        // Verify the file round-trips through quarto-trace's reader.
        let doc = quarto_trace::read::read_trace(&output_path).unwrap();
        assert_eq!(doc.schema_version, quarto_trace::SCHEMA_VERSION);
        assert_eq!(doc.render.format_target.as_deref(), Some("html"));
        assert_eq!(doc.pipeline.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
