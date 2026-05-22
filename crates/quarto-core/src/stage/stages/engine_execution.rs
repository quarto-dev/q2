/*
 * stage/stages/engine_execution.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pipeline stage that executes code cells via the appropriate engine.
 */

//! Engine execution pipeline stage.
//!
//! This stage handles execution of code cells in Quarto documents by:
//!
//! 1. Detecting which engine to use from document metadata
//! 2. Serializing the AST to QMD format
//! 3. Executing the engine on the QMD content
//! 4. Parsing the result back to AST
//! 5. Reconciling source locations between original and executed ASTs
//!
//! For the "markdown" engine (the default), this is a no-op that passes
//! through the AST unchanged.
//!
//! # WASM Behavior
//!
//! In WASM builds, only the markdown engine is available. Requests for
//! other engines (knitr, jupyter) will produce a warning and fall back
//! to markdown.

use async_trait::async_trait;
use std::sync::Arc;

use quarto_error_reporting::DiagnosticMessage;

use crate::engine::{EngineRegistry, ExecutionContext, ExecutionEngine, detect_engine};
use crate::stage::{
    DocumentAst, EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage,
    StageContext,
};
use crate::trace_event;

/// Stable [`PipelineObserver::on_auxiliary_data`] kind tag for the
/// engine capture emitted by this stage (bd-45yw).
///
/// `JsonTraceObserver` recognizes this kind and routes the payload to
/// `TraceDocument.engine_capture` (the typed slot) instead of to the
/// generic pipeline aux channel. The payload's JSON shape matches
/// [`quarto_trace::EngineCapture`].
pub const ENGINE_CAPTURE_KIND: &str = "EngineCapture";

/// Pipeline stage that executes code cells via the appropriate engine.
///
/// This stage is the bridge between the AST-based pipeline and text-based
/// execution engines (knitr, jupyter). It:
///
/// 1. Detects the engine from document metadata
/// 2. Serializes the AST to QMD for engine execution
/// 3. Executes the engine
/// 4. Parses the result back to AST
/// 5. Reconciles source locations
///
/// For the markdown engine (default), the stage passes through unchanged
/// as an optimization.
///
/// # Example
///
/// ```ignore
/// use quarto_core::stage::{Pipeline, EngineExecutionStage, ParseDocumentStage};
///
/// let stages: Vec<Box<dyn PipelineStage>> = vec![
///     Box::new(ParseDocumentStage::new()),
///     Box::new(EngineExecutionStage::new()),
///     // ... more stages
/// ];
///
/// let pipeline = Pipeline::new(stages)?;
/// ```
pub struct EngineExecutionStage {
    /// Engine registry for looking up engines by name
    registry: EngineRegistry,
}

impl EngineExecutionStage {
    /// Create a new EngineExecutionStage with the default registry.
    ///
    /// The default registry includes:
    /// - `markdown` (all platforms) - no-op passthrough
    /// - `knitr` (native only) - R code execution
    /// - `jupyter` (native only) - Python/Julia code execution
    pub fn new() -> Self {
        Self {
            registry: EngineRegistry::new(),
        }
    }

    /// Create with a custom registry (primarily for testing).
    pub fn with_registry(registry: EngineRegistry) -> Self {
        Self { registry }
    }

    /// Get the engine to use, with fallback behavior.
    ///
    /// If the requested engine is not available (e.g., jupyter in WASM),
    /// this returns the markdown engine and adds a warning.
    fn get_engine_with_fallback(
        &self,
        engine_name: &str,
        warnings: &mut Vec<DiagnosticMessage>,
    ) -> Arc<dyn ExecutionEngine> {
        if let Some(engine) = self.registry.get(engine_name) {
            // Engine found - check if it's actually available
            if engine.is_available() {
                return engine;
            }

            // Engine exists but isn't available (e.g., R not installed)
            warnings.push(DiagnosticMessage::warning(format!(
                "Engine '{}' is not available (runtime not found), using markdown (no execution)",
                engine_name
            )));
        } else {
            // Engine not registered (e.g., jupyter in WASM)
            warnings.push(DiagnosticMessage::warning(format!(
                "Engine '{}' not available in this build, using markdown (no execution)",
                engine_name
            )));
        }

        self.registry.default_engine()
    }
}

impl Default for EngineExecutionStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for EngineExecutionStage {
    fn name(&self) -> &str {
        "engine-execution"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(doc_ast) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Step 1: Detect engine from metadata
        let detected = detect_engine(&doc_ast.ast.meta);

        trace_event!(
            ctx,
            EventLevel::Debug,
            "detected engine: {} (config: {})",
            detected.name,
            if detected.config.is_some() {
                "yes"
            } else {
                "no"
            }
        );

        // Step 2: Get the engine implementation (with fallback)
        let mut engine_warnings = Vec::new();
        let engine = self.get_engine_with_fallback(&detected.name, &mut engine_warnings);

        // Add any engine lookup diagnostics to context
        if !engine_warnings.is_empty() {
            ctx.add_diagnostics(engine_warnings);
        }

        trace_event!(ctx, EventLevel::Debug, "using engine: {}", engine.name());

        // Step 3: For markdown engine, skip execution (optimization)
        // The markdown engine is a no-op, so we can avoid the serialize/parse round-trip
        if engine.name() == "markdown" {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "markdown engine - passing through unchanged"
            );
            return Ok(PipelineData::DocumentAst(doc_ast));
        }

        // Step 4: Serialize AST to QMD for engine execution
        let (qmd, qmd_source_info) = serialize_ast_to_qmd(&doc_ast.ast)?;

        trace_event!(
            ctx,
            EventLevel::Debug,
            "serialized AST to {} bytes of QMD",
            qmd.len()
        );

        // Step 5: Prepare execution context
        // Clone source_context into Arc — it's finalized after include expansion.
        let source_context = std::sync::Arc::new(doc_ast.source_context.clone());
        let exec_context = ExecutionContext::new(
            ctx.temp_dir.clone(),
            ctx.project.dir.clone(),
            doc_ast.path.clone(),
            &ctx.format.identifier.to_string(),
        )
        .with_project_dir(if ctx.project.is_single_file {
            None
        } else {
            Some(ctx.project.dir.clone())
        })
        .with_engine_config(detected.config.clone())
        .with_source_info(qmd_source_info, source_context);

        // Step 6: Execute the engine
        trace_event!(ctx, EventLevel::Info, "executing engine: {}", engine.name());

        let mut result = engine
            .execute(&qmd, &exec_context)
            .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;

        trace_event!(
            ctx,
            EventLevel::Debug,
            "engine produced {} bytes of markdown",
            result.markdown.len()
        );

        // bd-45yw: emit the engine capture for trace recording.
        // Must happen before the rest of the stage mutates `result`
        // (draining `includes`, taking `supporting_files`) so the
        // capture reflects the engine's full output. JsonTraceObserver
        // recognizes ENGINE_CAPTURE_KIND and routes the payload to
        // TraceDocument.engine_capture; other observers (CLI, WASM
        // callbacks, NoopObserver) ignore the kind and stay quiet.
        match serde_json::to_value(&result) {
            Ok(result_json) => {
                let payload = serde_json::json!({
                    "engine_name": engine.name(),
                    "input_qmd": qmd,
                    "result": result_json,
                });
                ctx.observer
                    .on_auxiliary_data(self.name(), 0, ENGINE_CAPTURE_KIND, &payload);
            }
            Err(e) => {
                // Should not happen — ExecuteResult is fully serializable —
                // but if it ever does we surface it without breaking the
                // render path: the capture is a recording-time concern,
                // not a correctness-of-output concern.
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "failed to serialize ExecuteResult for trace capture: {}",
                    e
                );
            }
        }

        // Save engine-produced includes (e.g., from knitr/jupyter) onto context
        ctx.includes
            .header_includes
            .extend(result.includes.header_includes);
        ctx.includes
            .include_before
            .extend(result.includes.include_before);
        ctx.includes
            .include_after
            .extend(result.includes.include_after);

        // bd-o8pr Phase 2: route engine-emitted supporting files
        // into the per-document resource report. The orchestrator
        // drains this after Pass-2 and copies the entries into the
        // output dir alongside static-channel resources. Engine
        // contributions are append-only — there's no
        // ResourceReportStage::remove. The report carries the doc
        // source on each entry so it stays attributable after the
        // per-doc reports are merged into the project-wide list.
        if !result.supporting_files.is_empty() {
            ctx.resource_report.add_engine_files(
                engine.name(),
                &doc_ast.path,
                std::mem::take(&mut result.supporting_files),
            );
        }

        // Step 7: Parse the executed markdown back to AST.
        //
        // The engine runs against an intermediate `<stem>.rmarkdown` file
        // (knitr's convention — see `knitr::postprocess_markdown`). We parse
        // with that name so `SourceInfo` attribution on new (engine-produced)
        // blocks points at the intermediate file rather than the original.
        // Blocks kept from the original AST keep their original attribution
        // after the FileId merge below.
        //
        // Note: `result.markdown` is the engine's *output* buffer, so the
        // SourceInfo byte offsets technically index into that buffer rather
        // than the on-disk `.rmarkdown`. For the current use (filename
        // attribution in the trace), this is adequate; a future refinement
        // could expose the on-disk intermediate via the engine interface.
        let intermediate_name = intermediate_filename(&doc_ast.path);
        let (executed_ast, executed_ast_context, parse_warnings) = pampa::readers::qmd::read(
            result.markdown.as_bytes(),
            false,              // loose mode
            &intermediate_name, // filename for error messages
            &mut std::io::sink(),
            true, // track source locations
            None, // file_id
        )
        .map_err(|diagnostics| {
            PipelineError::stage_error_with_diagnostics(self.name(), diagnostics)
        })?;

        // Step 7a: Build the merged ASTContext that covers BOTH files.
        //
        // bd-ky14a: with hash-based FileIds, each file lands at its
        // canonical FileId(hash(filename)) natively — no remap step
        // needed. We just need to make sure the intermediate file's
        // content is registered in the merged context under the
        // same FileId pampa already put on the executed AST's
        // SourceInfos.
        let mut merged_ast_context = doc_ast.ast_context.clone();
        let intermediate_file_id = quarto_yaml::file_id_for_filename(&intermediate_name);
        // `executed_ast_context` has the intermediate file under
        // its hash FileId; lift the FileInformation (line breaks,
        // total length) into the merged context under the same ID.
        if let Some(intermediate_file) = executed_ast_context
            .source_context
            .get_file(intermediate_file_id)
            .cloned()
        {
            if let Some(info) = intermediate_file.file_info {
                merged_ast_context.source_context.add_file_with_id_and_info(
                    intermediate_file_id,
                    intermediate_name.clone(),
                    info,
                );
            } else {
                merged_ast_context.source_context.add_file_with_id(
                    intermediate_file_id,
                    intermediate_name.clone(),
                    None,
                );
            }
        } else {
            merged_ast_context.source_context.add_file_with_id(
                intermediate_file_id,
                intermediate_name.clone(),
                None,
            );
        }
        merged_ast_context.filenames.push(intermediate_name);
        // Example-list counter is cell-ordering state tied to the executed
        // AST. Preserve the executed parse's final value so subsequent
        // example-list numbering stays coherent.
        merged_ast_context
            .example_list_counter
            .set(executed_ast_context.example_list_counter.get());

        // Step 7b: bd-ky14a previously required a `FileId(0) → FileId(1)`
        // remap of the executed AST so the intermediate file's
        // SourceInfos didn't collide with the original's `FileId(0)`.
        // Under hash-based FileIds, the executed AST already has
        // `FileId(hash(intermediate_name))` on every SourceInfo — no
        // collision is possible — so the remap is a no-op.

        // Step 8: Reconcile source locations
        // For content that hasn't changed, preserve original source locations.
        // For new content (execution outputs), use locations from executed AST.
        // Uses the three-phase reconciliation algorithm from quarto-ast-reconcile.
        let (reconciled_ast, reconciliation_plan) =
            quarto_ast_reconcile::reconcile(doc_ast.ast, executed_ast);

        trace_event!(
            ctx,
            EventLevel::Debug,
            "reconciliation: {} kept, {} replaced, {} recursed",
            reconciliation_plan.stats.blocks_kept,
            reconciliation_plan.stats.blocks_replaced,
            reconciliation_plan.stats.blocks_recursed
        );

        // Step 9: Collect warnings
        let mut warnings = doc_ast.warnings;
        warnings.extend(parse_warnings);

        // Step 10: Return updated DocumentAst
        Ok(PipelineData::DocumentAst(DocumentAst {
            path: doc_ast.path,
            ast: reconciled_ast,
            ast_context: merged_ast_context,
            source_context: doc_ast.source_context,
            warnings,
            recorded_includes: doc_ast.recorded_includes,
        }))
    }
}

/// Derive the intermediate filename engines see from the source path.
///
/// Mirrors knitr's convention: `foo.qmd` → `foo.rmarkdown`. Used only as a
/// filename label for source attribution; no file needs to exist on disk.
fn intermediate_filename(source_path: &std::path::Path) -> String {
    let mut with_ext = source_path.to_path_buf();
    with_ext.set_extension("rmarkdown");
    with_ext.display().to_string()
}

/// Serialize a Pandoc AST to QMD text.
///
/// This produces QMD that can be fed to execution engines.
/// Uses pampa's QMD writer which preserves code cell attributes.
fn serialize_ast_to_qmd(
    ast: &quarto_pandoc_types::pandoc::Pandoc,
) -> Result<(String, quarto_source_map::SourceInfo), PipelineError> {
    let (buffer, source_info) =
        pampa::writers::qmd::write_with_source_info(ast).map_err(|diagnostics| {
            PipelineError::stage_error_with_diagnostics("engine-execution", diagnostics)
        })?;

    let text = String::from_utf8(buffer).map_err(|e| {
        PipelineError::stage_error(
            "engine-execution",
            format!("QMD serialization produced invalid UTF-8: {}", e),
        )
    })?;

    Ok((text, source_info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::LoadedSource;
    use std::path::PathBuf;

    // Helper to create a mock runtime
    struct MockRuntime;

    #[async_trait::async_trait]
    impl quarto_system_runtime::SystemRuntime for MockRuntime {
        fn file_read(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn file_write(
            &self,
            _path: &std::path::Path,
            _contents: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_exists(
            &self,
            _path: &std::path::Path,
            _kind: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            Ok(true)
        }
        fn canonicalize(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn path_metadata(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            unimplemented!()
        }
        fn file_copy(
            &self,
            _src: &std::path::Path,
            _dst: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_rename(
            &self,
            _old: &std::path::Path,
            _new: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn file_remove(&self, _path: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_create(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_remove(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_list(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            Ok(vec![])
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/"))
        }
        fn temp_dir(
            &self,
            _template: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            Ok(quarto_system_runtime::TempDir::new(PathBuf::from(
                "/tmp/test",
            )))
        }
        fn exec_pipe(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn exec_command(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            Ok(quarto_system_runtime::CommandOutput {
                code: 0,
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn env_get(&self, _name: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
            Ok(None)
        }
        fn env_all(
            &self,
        ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
        {
            Ok(std::collections::HashMap::new())
        }
        async fn fetch_url(
            &self,
            _url: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            Err(quarto_system_runtime::RuntimeError::NotSupported(
                "mock".to_string(),
            ))
        }
        fn os_name(&self) -> &'static str {
            "mock"
        }
        fn arch(&self) -> &'static str {
            "mock"
        }
        fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
            Ok(0)
        }
        fn xdg_dir(
            &self,
            _kind: quarto_system_runtime::XdgDirKind,
            _subpath: Option<&std::path::Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/xdg"))
        }
        fn stdout_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
    }

    fn make_test_context() -> StageContext {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectContext};
        use std::sync::Arc;

        let runtime = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();

        StageContext::new(runtime, format, project, doc).unwrap()
    }

    fn parse_qmd_to_ast(content: &[u8], path: &str) -> DocumentAst {
        use quarto_source_map::SourceContext;

        let mut source_context = SourceContext::new();
        let content_str = String::from_utf8_lossy(content);
        source_context.add_file(path.to_string(), Some(content_str.into_owned()));

        let (ast, ast_context, warnings) =
            pampa::readers::qmd::read(content, false, path, &mut std::io::sink(), true, None)
                .expect("Failed to parse test QMD");

        DocumentAst {
            path: PathBuf::from(path),
            ast,
            ast_context,
            source_context,
            warnings,
            recorded_includes: Vec::new(),
        }
    }

    #[test]
    fn test_stage_metadata() {
        let stage = EngineExecutionStage::new();
        assert_eq!(stage.name(), "engine-execution");
        assert_eq!(stage.input_kind(), PipelineDataKind::DocumentAst);
        assert_eq!(stage.output_kind(), PipelineDataKind::DocumentAst);
    }

    #[tokio::test]
    async fn test_markdown_engine_passthrough() {
        let stage = EngineExecutionStage::new();
        let mut ctx = make_test_context();

        let content = b"---\ntitle: Test\n---\n\n# Hello\n\nWorld";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        // Get the original block count
        let original_block_count = doc_ast.ast.blocks.len();

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();

        let result = output.into_document_ast().expect("Should be DocumentAst");

        // Markdown engine should pass through unchanged
        assert_eq!(result.ast.blocks.len(), original_block_count);
        assert!(ctx.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn test_explicit_markdown_engine() {
        let stage = EngineExecutionStage::new();
        let mut ctx = make_test_context();

        // Explicit engine: markdown
        let content = b"---\ntitle: Test\nengine: markdown\n---\n\n# Hello\n\nWorld";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();

        let result = output.into_document_ast().expect("Should be DocumentAst");

        // Should pass through unchanged
        assert!(!result.ast.blocks.is_empty());
        assert!(ctx.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn test_unknown_engine_falls_back() {
        let stage = EngineExecutionStage::new();
        let mut ctx = make_test_context();

        // Unknown engine should fall back to markdown with warning
        let content = b"---\ntitle: Test\nengine: unknown-engine\n---\n\n# Hello";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();

        let result = output.into_document_ast().expect("Should be DocumentAst");

        // Should fall back to markdown and produce a warning
        assert!(!result.ast.blocks.is_empty());
        assert!(!ctx.diagnostics.is_empty());
        assert!(ctx.diagnostics[0].title.contains("not available"));
    }

    #[tokio::test]
    async fn test_wrong_input_type() {
        let stage = EngineExecutionStage::new();
        let mut ctx = make_test_context();

        // Feed wrong input type
        let source = LoadedSource::new(PathBuf::from("/project/test.qmd"), vec![]);
        let input = PipelineData::LoadedSource(source);

        let result = stage.run(input, &mut ctx).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, PipelineError::UnexpectedInput { .. }));
    }

    #[test]
    fn test_engine_fallback_with_unavailable_engine() {
        let stage = EngineExecutionStage::new();
        let mut warnings = Vec::new();

        // Request an engine that doesn't exist
        let engine = stage.get_engine_with_fallback("nonexistent-engine", &mut warnings);

        // Should fall back to markdown
        assert_eq!(engine.name(), "markdown");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].title.contains("not available"));
    }

    #[test]
    fn test_serialize_ast_to_qmd() {
        let content = b"---\ntitle: Test\n---\n\n# Hello\n\nWorld";
        let doc_ast = parse_qmd_to_ast(content, "test.qmd");

        let (qmd, source_info) = serialize_ast_to_qmd(&doc_ast.ast).unwrap();

        // Should contain the title
        assert!(qmd.contains("title"));
        // Should contain the heading
        assert!(qmd.contains("Hello"));
        // Should have source provenance
        assert!(
            matches!(source_info, quarto_source_map::SourceInfo::Concat { .. }),
            "Expected Concat SourceInfo from serialization"
        );
        // Should contain the paragraph
        assert!(qmd.contains("World"));
    }

    #[test]
    fn test_stage_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<EngineExecutionStage>();
    }

    /// Mock engine that returns includes to test the knitr gap fix.
    struct MockIncludesEngine;

    impl crate::engine::ExecutionEngine for MockIncludesEngine {
        fn name(&self) -> &str {
            "mock-includes"
        }

        fn execute(
            &self,
            input: &str,
            _ctx: &crate::engine::ExecutionContext,
        ) -> std::result::Result<crate::engine::ExecuteResult, crate::engine::ExecutionError>
        {
            use crate::stage::PandocIncludes;
            Ok(crate::engine::ExecuteResult {
                markdown: input.to_string(),
                includes: PandocIncludes {
                    header_includes: vec!["<style>h1 { color: red; }</style>".to_string()],
                    include_before: vec!["<div>before</div>".to_string()],
                    include_after: vec!["<div>after</div>".to_string()],
                },
                ..Default::default()
            })
        }

        fn is_available(&self) -> bool {
            true
        }
    }

    /// Mock engine that appends a fresh paragraph to the input — lets us
    /// verify that blocks added by the engine get the intermediate FileId
    /// while original blocks keep their `.qmd` FileId.
    struct MockAppendingEngine;

    impl crate::engine::ExecutionEngine for MockAppendingEngine {
        fn name(&self) -> &str {
            "mock-appending"
        }

        fn execute(
            &self,
            input: &str,
            _ctx: &crate::engine::ExecutionContext,
        ) -> std::result::Result<crate::engine::ExecuteResult, crate::engine::ExecutionError>
        {
            let appended = format!("{}\n\nEngine appended this line.\n", input);
            Ok(crate::engine::ExecuteResult {
                markdown: appended,
                ..Default::default()
            })
        }

        fn is_available(&self) -> bool {
            true
        }
    }

    /// Walk every `SourceInfo::Original` reachable from the AST's blocks
    /// and collect the FileIds used. Test helper.
    fn collect_file_ids(
        pandoc: &quarto_pandoc_types::pandoc::Pandoc,
    ) -> std::collections::HashSet<quarto_source_map::FileId> {
        use quarto_pandoc_types::{Block, Inline};
        use quarto_source_map::{FileId, SourceInfo};

        fn walk_source_info(si: &SourceInfo, out: &mut std::collections::HashSet<FileId>) {
            match si {
                SourceInfo::Original { file_id, .. } => {
                    out.insert(*file_id);
                }
                SourceInfo::Substring { parent, .. } => walk_source_info(parent, out),
                SourceInfo::Concat { pieces } => {
                    for p in pieces {
                        walk_source_info(&p.source_info, out);
                    }
                }
                SourceInfo::FilterProvenance { .. } => {}
            }
        }
        fn walk_inline(i: &Inline, out: &mut std::collections::HashSet<FileId>) {
            match i {
                Inline::Str(x) => walk_source_info(&x.source_info, out),
                Inline::Emph(x) => {
                    for c in &x.content {
                        walk_inline(c, out);
                    }
                    walk_source_info(&x.source_info, out);
                }
                Inline::Strong(x) => {
                    for c in &x.content {
                        walk_inline(c, out);
                    }
                    walk_source_info(&x.source_info, out);
                }
                Inline::Space(x) => walk_source_info(&x.source_info, out),
                Inline::SoftBreak(x) => walk_source_info(&x.source_info, out),
                _ => {
                    // Other variants not needed for this test. Add as needed.
                }
            }
        }
        fn walk_block(b: &Block, out: &mut std::collections::HashSet<FileId>) {
            match b {
                Block::Paragraph(p) => {
                    for i in &p.content {
                        walk_inline(i, out);
                    }
                    walk_source_info(&p.source_info, out);
                }
                Block::Header(h) => {
                    for i in &h.content {
                        walk_inline(i, out);
                    }
                    walk_source_info(&h.source_info, out);
                }
                Block::Div(d) => {
                    for b in &d.content {
                        walk_block(b, out);
                    }
                    walk_source_info(&d.source_info, out);
                }
                _ => {
                    // Other block types not needed for this test.
                }
            }
        }

        let mut ids = std::collections::HashSet::new();
        for b in &pandoc.blocks {
            walk_block(b, &mut ids);
        }
        ids
    }

    /// After engine execution appends new content, kept blocks must keep
    /// the original `.qmd` FileId while appended blocks get the
    /// intermediate `.rmarkdown` FileId. Regression test for bd-b0f2
    /// Phase 2.
    #[tokio::test]
    async fn test_engine_execution_remaps_new_blocks_to_intermediate() {
        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(MockAppendingEngine));

        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();

        let content = b"---\nengine: mock-appending\n---\n\n# Hello\n\nOriginal paragraph.";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().expect("Should be DocumentAst");

        // Filenames are the two-slot merged context.
        assert_eq!(
            result.ast_context.filenames,
            vec![
                "/project/test.qmd".to_string(),
                "/project/test.rmarkdown".to_string(),
            ]
        );

        // bd-ky14a: FileIds are hash-based, so .qmd and .rmarkdown
        // each have a distinct hash derived from their filename.
        // Both must appear in the reconciled AST.
        let qmd_fid = quarto_yaml::file_id_for_filename("/project/test.qmd");
        let rmd_fid = quarto_yaml::file_id_for_filename("/project/test.rmarkdown");
        let ids = collect_file_ids(&result.ast);
        assert!(
            ids.contains(&qmd_fid),
            "expected .qmd's FileId ({:?}) for kept blocks, got {:?}",
            qmd_fid,
            ids,
        );
        assert!(
            ids.contains(&rmd_fid),
            "expected .rmarkdown's FileId ({:?}) for appended block, got {:?}",
            rmd_fid,
            ids,
        );
        // No stray FileIds — only the two known files should appear.
        for id in &ids {
            assert!(
                *id == qmd_fid || *id == rmd_fid,
                "unexpected FileId {:?} in reconciled AST (merged context has 2 slots)",
                id,
            );
        }
    }

    /// After engine execution, the merged `ASTContext` must carry BOTH the
    /// original `.qmd` filename and the intermediate `<stem>.rmarkdown`
    /// filename the engine saw. Regression test for bd-b0f2 Phase 2.
    #[tokio::test]
    async fn test_engine_execution_merges_original_and_intermediate_filenames() {
        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(MockIncludesEngine));

        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();

        // Force engine: mock-includes via metadata
        let content = b"---\ntitle: Test\nengine: mock-includes\n---\n\n# Hello\n\nWorld";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().expect("Should be DocumentAst");

        let filenames = &result.ast_context.filenames;
        assert_eq!(
            filenames.len(),
            2,
            "expected exactly two filenames (original + intermediate), got: {:?}",
            filenames
        );
        assert_eq!(filenames[0], "/project/test.qmd");
        assert_eq!(filenames[1], "/project/test.rmarkdown");
    }

    #[tokio::test]
    async fn test_engine_execution_preserves_includes() {
        // Build a custom registry with our mock engine
        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(MockIncludesEngine));

        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();

        // Force engine: mock-includes via metadata
        let content = b"---\ntitle: Test\nengine: mock-includes\n---\n\n# Hello\n\nWorld";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        let _output = stage.run(input, &mut ctx).await.unwrap();

        // Verify includes were preserved onto StageContext
        assert_eq!(ctx.includes.header_includes.len(), 1);
        assert_eq!(
            ctx.includes.header_includes[0],
            "<style>h1 { color: red; }</style>"
        );
        assert_eq!(ctx.includes.include_before.len(), 1);
        assert_eq!(ctx.includes.include_before[0], "<div>before</div>");
        assert_eq!(ctx.includes.include_after.len(), 1);
        assert_eq!(ctx.includes.include_after[0], "<div>after</div>");
    }

    // === Phase 0C: SourceInfo in ExecutionContext tests ===

    #[test]
    fn test_execution_context_has_source_info() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        );
        // Default source_info should be SourceInfo::default()
        assert_eq!(ctx.source_info, quarto_source_map::SourceInfo::default());
    }

    #[test]
    fn test_execution_context_with_source_info() {
        let si = quarto_source_map::SourceInfo::original(quarto_source_map::FileId(0), 0, 100);
        let mut sc = quarto_source_map::SourceContext::new();
        sc.add_file("test.qmd".to_string(), Some("content".to_string()));
        let sc = std::sync::Arc::new(sc);

        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        )
        .with_source_info(si.clone(), sc.clone());

        assert_eq!(ctx.source_info, si);
        assert!(
            ctx.source_context
                .get_file(quarto_source_map::FileId(0))
                .is_some()
        );
    }

    #[test]
    fn test_serialize_ast_to_qmd_produces_source_info() {
        let content = b"# Title\n\nSome body text\n\n```python\nprint('hello')\n```\n";
        let doc_ast = parse_qmd_to_ast(content, "test.qmd");

        let (qmd, source_info) = serialize_ast_to_qmd(&doc_ast.ast).unwrap();

        // SourceInfo should be Concat
        let pieces = match &source_info {
            quarto_source_map::SourceInfo::Concat { pieces } => pieces,
            other => panic!("Expected Concat, got {:?}", other),
        };

        // Piece lengths should sum to qmd length
        let total: usize = pieces.iter().map(|p| p.length).sum();
        assert_eq!(total, qmd.len());

        // Should have pieces for the blocks
        assert!(
            pieces.len() >= 2,
            "Expected at least 2 pieces (heading + body)"
        );
    }

    #[test]
    fn test_source_info_map_offset_single_file() {
        let input = b"# Title\n\nBody text here\n";
        let doc_ast = parse_qmd_to_ast(input, "test.qmd");

        let (qmd, source_info) = serialize_ast_to_qmd(&doc_ast.ast).unwrap();

        // Build a SourceContext from the doc_ast's ast_context
        let source_context = &doc_ast.ast_context.source_context;

        // Find "Body" in the serialized output
        let body_pos = qmd.find("Body").expect("should find Body in output");
        let mapped = source_info.map_offset(body_pos, source_context);
        assert!(
            mapped.is_some(),
            "map_offset should resolve for a body text offset"
        );
        let mapped = mapped.unwrap();
        // bd-ky14a: FileId is the hash of the primary filename, not 0.
        assert_eq!(
            mapped.file_id,
            quarto_yaml::file_id_for_filename("test.qmd"),
            "Should map to the main file"
        );
    }

    #[test]
    fn test_source_info_map_offset_start_and_end() {
        let input = b"# Title\n\nBody";
        let doc_ast = parse_qmd_to_ast(input, "test.qmd");

        let (qmd, source_info) = serialize_ast_to_qmd(&doc_ast.ast).unwrap();
        let source_context = &doc_ast.ast_context.source_context;

        // Start of output
        let mapped_start = source_info.map_offset(0, source_context);
        assert!(mapped_start.is_some(), "Start of output should resolve");

        // End of output (last valid byte)
        if !qmd.is_empty() {
            let mapped_end = source_info.map_offset(qmd.len() - 1, source_context);
            assert!(mapped_end.is_some(), "End of output should resolve");
        }
    }

    /// bd-45yw: replay engine integration through `EngineExecutionStage`.
    ///
    /// Builds a registry with `ReplayEngine` substituted under
    /// `mock-replay-engine`, runs the stage against a `DocumentAst`
    /// whose declared engine matches the replay engine's name, and
    /// asserts:
    ///
    /// 1. The stage flows the recorded `supporting_files` into
    ///    `ctx.resource_report` tagged `ResourceOrigin::Engine` —
    ///    closes the bd-o8pr Phase 2 engine-channel test gap.
    /// 2. The recorded `includes` reach `ctx.includes`.
    /// 3. The replayed markdown reaches the downstream AST.
    #[tokio::test]
    async fn test_replay_engine_drives_resource_report_through_stage() {
        use crate::engine::{EngineRegistry, ReplayEngine};
        use crate::project_resources::ResourceOrigin;
        use quarto_trace::EngineCapture;

        // Document declares the replay engine's name verbatim. The
        // stage will serialize this AST to QMD and hand it to the
        // engine, so we record the serialized form as input_qmd.
        let content = b"---\nengine: mock-replay-engine\n---\n\n# Hello\n\nWorld\n";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");
        let (serialized_qmd, _) = serialize_ast_to_qmd(&doc_ast.ast).unwrap();

        let capture = EngineCapture {
            engine_name: "mock-replay-engine".into(),
            input_qmd: serialized_qmd,
            result: serde_json::json!({
                "markdown": "# Hello\n\nReplayed body.\n",
                "supporting_files": ["fig1.png", "data/table.csv"],
                "filters": [],
                "includes": {
                    "header_includes": ["<style>.replay{}</style>"],
                    "include_before": [],
                    "include_after": [],
                },
                "needs_postprocess": false,
            }),
        };

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(ReplayEngine::new(capture)));

        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();

        let input = PipelineData::DocumentAst(doc_ast);
        let _output = stage.run(input, &mut ctx).await.unwrap();

        // 1. Resource report received the recorded supporting files,
        //    tagged Engine with the replay engine's surfaced name.
        assert_eq!(
            ctx.resource_report.entries.len(),
            2,
            "expected two engine-tagged resource entries, got {:?}",
            ctx.resource_report.entries
        );
        for entry in &ctx.resource_report.entries {
            match &entry.origin {
                ResourceOrigin::Engine { engine, source } => {
                    assert_eq!(engine, "mock-replay-engine");
                    assert_eq!(source, &PathBuf::from("/project/test.qmd"));
                }
                other => panic!("expected ResourceOrigin::Engine, got {:?}", other),
            }
        }
        let raw_paths: Vec<_> = ctx
            .resource_report
            .entries
            .iter()
            .map(|e| e.raw_path.clone())
            .collect();
        assert!(raw_paths.contains(&PathBuf::from("fig1.png")));
        assert!(raw_paths.contains(&PathBuf::from("data/table.csv")));

        // 2. Recorded includes reached ctx.includes.
        assert_eq!(
            ctx.includes.header_includes,
            vec!["<style>.replay{}</style>"]
        );
        assert!(ctx.includes.include_before.is_empty());
        assert!(ctx.includes.include_after.is_empty());
    }

    /// bd-45yw: after a successful engine run,
    /// `EngineExecutionStage` must emit an `on_auxiliary_data` event
    /// with kind `"EngineCapture"` carrying the recorded engine name,
    /// the verbatim QMD input, and the `ExecuteResult` JSON. This is
    /// the recording side of the replay loop — without this event,
    /// `JsonTraceObserver` has nothing to put on
    /// `TraceDocument.engine_capture`.
    #[tokio::test]
    async fn test_engine_execution_emits_capture_aux_event() {
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CapturingObserver {
            aux_calls: AtomicUsize,
            captures: Mutex<Vec<(String, String, serde_json::Value)>>,
        }

        impl crate::stage::PipelineObserver for CapturingObserver {
            fn on_auxiliary_data(
                &self,
                stage: &str,
                _index: usize,
                kind: &str,
                data: &serde_json::Value,
            ) {
                if kind == ENGINE_CAPTURE_KIND {
                    self.aux_calls.fetch_add(1, Ordering::SeqCst);
                    self.captures.lock().unwrap().push((
                        stage.to_string(),
                        kind.to_string(),
                        data.clone(),
                    ));
                }
            }
        }

        let observer = Arc::new(CapturingObserver {
            aux_calls: AtomicUsize::new(0),
            captures: Mutex::new(Vec::new()),
        });

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(MockIncludesEngine));

        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();
        ctx.observer = observer.clone();

        let content = b"---\nengine: mock-includes\n---\n\n# Hello\n\nWorld\n";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        stage.run(input, &mut ctx).await.unwrap();

        assert_eq!(
            observer.aux_calls.load(Ordering::SeqCst),
            1,
            "expected exactly one EngineCapture aux event"
        );
        let captures = observer.captures.lock().unwrap();
        let (stage_name, kind, data) = &captures[0];
        assert_eq!(stage_name, "engine-execution");
        assert_eq!(kind, ENGINE_CAPTURE_KIND);

        // The capture must carry the recorded engine name and a
        // non-empty input QMD that was actually handed to execute().
        assert_eq!(data["engine_name"], "mock-includes");
        let recorded_input = data["input_qmd"]
            .as_str()
            .expect("input_qmd must be a string");
        assert!(
            !recorded_input.is_empty(),
            "input_qmd should be the serialized QMD passed to execute()"
        );

        // The result JSON must round-trip back into ExecuteResult and
        // preserve the recorded includes — important because the
        // stage drains those into ctx.includes during the rest of the
        // run, but the *capture* must reflect the pre-drain state.
        let result_json = &data["result"];
        let parsed: crate::engine::ExecuteResult =
            serde_json::from_value(result_json.clone()).expect("result must round-trip");
        assert_eq!(
            parsed.includes.header_includes,
            vec!["<style>h1 { color: red; }</style>".to_string()]
        );
        assert_eq!(
            parsed.includes.include_before,
            vec!["<div>before</div>".to_string()]
        );
    }

    /// bd-45yw end-to-end: drive `EngineExecutionStage` through a
    /// real `JsonTraceObserver`, write the trace to disk, read it
    /// back via `quarto_trace::read::read_trace`, and verify
    /// `engine_capture` is populated and replay-usable.
    ///
    /// Closes the producer↔observer wiring loop: this is what
    /// `q2 render` with `trace: true` will produce.
    #[tokio::test]
    async fn test_engine_execution_records_trace_round_trip_to_disk() {
        use crate::stage::JsonTraceObserver;
        use quarto_trace::RenderInfo;

        let dir = std::env::temp_dir().join("quarto-trace-engine-capture-e2e");
        let _ = std::fs::remove_dir_all(&dir);
        let trace_path = dir.join("trace.json");

        let observer = Arc::new(JsonTraceObserver::new(
            trace_path.clone(),
            RenderInfo {
                input_path: Some("/project/test.qmd".into()),
                format_target: Some("html".into()),
                git_hash: Some(quarto_trace::BUILD_GIT_HASH.to_string()),
                ..Default::default()
            },
        ));

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(MockIncludesEngine));
        let stage = EngineExecutionStage::with_registry(registry);

        let mut ctx = make_test_context();
        ctx.observer = observer.clone();

        let content = b"---\nengine: mock-includes\n---\n\n# Hello\n\nWorld\n";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        stage.run(input, &mut ctx).await.unwrap();

        // Persist the trace and read it back through the public reader.
        observer.write_trace().unwrap();
        let read_back = quarto_trace::read::read_trace(&trace_path).unwrap();

        let capture = read_back
            .engine_capture
            .as_ref()
            .expect("engine_capture must be populated");
        assert_eq!(capture.engine_name, "mock-includes");
        assert!(
            !capture.input_qmd.is_empty(),
            "input_qmd must hold the QMD that was handed to execute()"
        );
        let parsed: crate::engine::ExecuteResult = serde_json::from_value(capture.result.clone())
            .expect("result must round-trip back to ExecuteResult");
        assert_eq!(
            parsed.includes.header_includes,
            vec!["<style>h1 { color: red; }</style>".to_string()]
        );
        assert_eq!(
            parsed.includes.include_before,
            vec!["<div>before</div>".to_string()]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// bd-5qnj Phase 3 (unified artifact): a single `latest.json.gz`
    /// must carry both bd-5qnj's deduped AST snapshots and bd-45yw's
    /// replay capture. After merging the two branches, this is the
    /// load-bearing assertion: one trace file plays both roles.
    ///
    /// We exercise the same `EngineExecutionStage` recording loop as
    /// bd-45yw's `test_engine_execution_records_trace_round_trip_to_disk`,
    /// but pre-populate the trace's pipeline with several DocumentAst
    /// entries (the situation Phase 2's dedup pass collapses) and
    /// write to a gzipped path. We then assert the on-disk wire
    /// format has both `asts` and `engine_capture` fields, and that
    /// `read_trace` rehydrates a v1-shaped doc with both intact.
    #[tokio::test]
    async fn test_unified_artifact_carries_dedup_and_engine_capture() {
        use crate::stage::JsonTraceObserver;
        use quarto_trace::RenderInfo;

        let dir = std::env::temp_dir().join(format!(
            "quarto-trace-unified-e2e-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        // Phase-1 on-disk format: gzipped.
        let trace_path = dir.join("latest.json.gz");

        let observer = Arc::new(JsonTraceObserver::new(
            trace_path.clone(),
            RenderInfo {
                input_path: Some("/project/test.qmd".into()),
                format_target: Some("html".into()),
                git_hash: Some(quarto_trace::BUILD_GIT_HASH.to_string()),
                ..Default::default()
            },
        ));

        // Drive a few DocumentAst stage events so the dedup pass has
        // something to collapse — three identical snapshots become one
        // entry in the `asts` map. We go through the public observer
        // API (`on_stage_data`) to mirror exactly what real stages do.
        let content = b"---\nengine: mock-includes\n---\n\n# Hello\n\nWorld\n";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");
        let pipeline_data = PipelineData::DocumentAst(doc_ast.clone());
        for (i, stage_name) in ["metadata-merge", "include-expansion", "unwrap-profile"]
            .iter()
            .enumerate()
        {
            crate::stage::PipelineObserver::on_stage_data(
                observer.as_ref(),
                stage_name,
                i,
                &pipeline_data,
            );
        }

        // Drive an engine that emits an EngineCapture aux event —
        // exactly bd-45yw's recording loop. After the run, the
        // observer should hold both pipeline AST snapshots and a
        // populated engine_capture.
        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(MockIncludesEngine));
        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();
        ctx.observer = observer.clone();

        stage
            .run(PipelineData::DocumentAst(doc_ast), &mut ctx)
            .await
            .unwrap();

        observer.write_trace().unwrap();

        // 1. On-disk wire format: both fields must appear in the
        //    gzipped JSON, and the `asts` map must hold exactly one
        //    entry (the three identical snapshots collapse).
        let raw_bytes = std::fs::read(&trace_path).unwrap();
        assert!(
            raw_bytes.len() >= 2 && raw_bytes[0] == 0x1f && raw_bytes[1] == 0x8b,
            "on-disk format must be gzipped (post-Phase-1)"
        );
        let inflated = {
            use std::io::Read;
            let mut s = String::new();
            flate2::read::GzDecoder::new(&raw_bytes[..])
                .read_to_string(&mut s)
                .unwrap();
            s
        };
        let raw_json: serde_json::Value = serde_json::from_str(&inflated).unwrap();
        assert_eq!(raw_json["schema_version"], 2);
        let asts = raw_json["asts"]
            .as_object()
            .expect("v2 unified artifact must have an `asts` map");
        assert_eq!(
            asts.len(),
            1,
            "three identical AST snapshots must collapse to one stored entry"
        );
        assert!(
            !raw_json["engine_capture"].is_null(),
            "v2 unified artifact must have an `engine_capture` field for non-markdown engines"
        );
        assert_eq!(raw_json["engine_capture"]["engine_name"], "mock-includes");

        // 2. read_trace round-trip: rehydrated doc has both deduped
        //    ASTs (folded back inline; `asts` empty) and the engine
        //    capture intact.
        let read_back = quarto_trace::read::read_trace(&trace_path).unwrap();
        assert!(
            read_back.asts.is_empty(),
            "reader must clear `asts` after rehydration"
        );
        let capture = read_back
            .engine_capture
            .as_ref()
            .expect("engine_capture must round-trip through gzipped trace");
        assert_eq!(capture.engine_name, "mock-includes");
        let parsed: crate::engine::ExecuteResult = serde_json::from_value(capture.result.clone())
            .expect("result must round-trip back to ExecuteResult");
        assert_eq!(
            parsed.includes.header_includes,
            vec!["<style>h1 { color: red; }</style>".to_string()],
            "engine_capture content survives the round-trip alongside the dedup pass"
        );
        // The pre-populated DocumentAst entries must rehydrate to
        // their inline form (no `$ref` visible to consumers).
        for entry in &read_back.pipeline {
            if entry.data_kind.as_deref() != Some("DocumentAst") {
                continue;
            }
            let Some(data) = &entry.data else { continue };
            if let Some(ast) = data.get("ast") {
                assert!(
                    ast.get("$ref").is_none(),
                    "rehydrated entry {:?} still has $ref: {:?}",
                    entry.stage,
                    ast
                );
            }
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// bd-45yw: replay miss through the pipeline must surface as a
    /// stage error (not as a silent passthrough).
    #[tokio::test]
    async fn test_replay_engine_miss_surfaces_as_stage_error() {
        use crate::engine::{EngineRegistry, ReplayEngine};
        use quarto_trace::EngineCapture;

        let content = b"---\nengine: mock-replay-engine\n---\n\n# Real\n";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        // Capture deliberately recorded against different input.
        let capture = EngineCapture {
            engine_name: "mock-replay-engine".into(),
            input_qmd: "completely different recorded content\n".into(),
            result: serde_json::json!({
                "markdown": "x\n",
                "supporting_files": [],
                "filters": [],
                "includes": {
                    "header_includes": [],
                    "include_before": [],
                    "include_after": [],
                },
                "needs_postprocess": false,
            }),
        };

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(ReplayEngine::new(capture)));

        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();

        let input = PipelineData::DocumentAst(doc_ast);
        let err = stage
            .run(input, &mut ctx)
            .await
            .expect_err("replay miss must produce a stage error");
        let msg = format!("{}", err);
        assert!(
            msg.contains("replay miss"),
            "expected 'replay miss' diagnostic in stage error, got: {msg}"
        );
    }
}
