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

use crate::engine::{EngineRegistry, ExecutionContext, ExecutionEngine, detect_engine_sequence};
use crate::stage::{
    DocumentAst, EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage,
    StageContext,
};
use crate::trace_event;

/// Stable [`PipelineObserver::on_auxiliary_data`] kind tag for the
/// engine capture emitted by this stage (bd-45yw).
///
/// `JsonTraceObserver` recognizes this kind and appends the payload to
/// `TraceDocument.engine_captures` (the typed slot — one per engine, in
/// execution order) instead of to the generic pipeline aux channel. The
/// payload's JSON shape matches [`quarto_trace::EngineCapture`].
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

        // Step 1: Detect the (ordered, de-duplicated) engine sequence.
        // bd-5yff4: `engine:` may be an array; engines run in declared
        // order, each consuming the AST the previous engine produced.
        let sequence = detect_engine_sequence(&doc_ast.ast.meta);

        // Distinct engines are required; report any duplicates the merge
        // produced (first occurrence kept). See `EngineSequence`.
        for dropped in &sequence.dropped_duplicates {
            ctx.add_diagnostics(vec![DiagnosticMessage::warning(format!(
                "Duplicate engine '{dropped}' in engine sequence ignored \
                 (engines must be distinct; the first occurrence is kept — \
                 use the !prefer merge tag to replace the list)"
            ))]);
        }

        trace_event!(
            ctx,
            EventLevel::Debug,
            "detected engine sequence: [{}]",
            sequence
                .engines
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );

        // Step 2: Resolve each detected engine to an implementation (with
        // fallback), dropping markdown — it's a no-op, so a markdown
        // engine anywhere in the sequence is skipped (this also covers
        // unavailable engines that fall back to markdown).
        let mut engine_warnings = Vec::new();
        let mut to_run: Vec<(
            Arc<dyn ExecutionEngine>,
            Option<quarto_pandoc_types::ConfigValue>,
        )> = Vec::new();
        for detected in &sequence.engines {
            let engine = self.get_engine_with_fallback(&detected.name, &mut engine_warnings);
            if engine.name() == "markdown" {
                trace_event!(
                    ctx,
                    EventLevel::Debug,
                    "engine '{}' resolved to markdown (no-op) — skipping",
                    detected.name
                );
                continue;
            }
            to_run.push((engine, detected.config.clone()));
        }
        if !engine_warnings.is_empty() {
            ctx.add_diagnostics(engine_warnings);
        }

        // Step 3: Fast path — nothing to execute (all markdown / no
        // engines). Pass the AST through unchanged, exactly as the single
        // markdown-engine optimization did.
        if to_run.is_empty() {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "no executable engines — passing through unchanged"
            );
            return Ok(PipelineData::DocumentAst(doc_ast));
        }

        // Thread the AST, the merged (multi-slot) ASTContext, and warnings
        // through the sequence. Slot 0 is the original `.qmd`; each engine
        // appends one intermediate slot. `merged_context.filenames.len()`
        // is the next FileId (files are added in lock-step with filenames).
        let DocumentAst {
            path,
            ast,
            ast_context,
            source_context,
            warnings,
            recorded_includes,
        } = doc_ast;
        let mut ast = ast;
        let mut merged_context = ast_context;
        let mut warnings = warnings;
        // Source context for engine error attribution. Constant across the
        // sequence: it resolves the original document's FileIds exactly
        // (as in the single-engine path). Offsets into content produced by
        // an *earlier* engine don't resolve here (their FileIds live only
        // in `merged_context`); engine error mapping is best-effort and
        // treats an unresolved offset as "no precise location", so this is
        // adequate. Precise cross-engine attribution is a possible
        // follow-up.
        let source_context_arc = std::sync::Arc::new(source_context.clone());

        for (run_index, (engine, engine_config)) in to_run.into_iter().enumerate() {
            // Serialize the current AST to QMD for this engine.
            let (qmd, qmd_source_info) = serialize_ast_to_qmd(&ast)?;
            trace_event!(
                ctx,
                EventLevel::Debug,
                "[{}] serialized AST to {} bytes of QMD",
                engine.name(),
                qmd.len()
            );

            let exec_context = ExecutionContext::new(
                ctx.temp_dir.clone(),
                ctx.project.dir.clone(),
                path.clone(),
                &ctx.format.identifier.to_string(),
            )
            .with_project_dir(if ctx.project.is_single_file {
                None
            } else {
                Some(ctx.project.dir.clone())
            })
            .with_engine_config(engine_config)
            .with_source_info(qmd_source_info, source_context_arc.clone());

            trace_event!(ctx, EventLevel::Info, "executing engine: {}", engine.name());
            let mut result = engine
                .execute(&qmd, &exec_context)
                .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;
            trace_event!(
                ctx,
                EventLevel::Debug,
                "[{}] engine produced {} bytes of markdown",
                engine.name(),
                result.markdown.len()
            );

            // bd-45yw / bd-5yff4: emit one engine capture per engine, with
            // `run_index` distinguishing engines within this stage so the
            // trace can carry an ordered capture per engine. Emitted before
            // `result` is drained so the capture reflects the full output.
            // JsonTraceObserver routes ENGINE_CAPTURE_KIND to the trace's
            // engine capture(s); other observers ignore the kind.
            match serde_json::to_value(&result) {
                Ok(result_json) => {
                    let payload = serde_json::json!({
                        "engine_name": engine.name(),
                        "input_qmd": qmd,
                        "result": result_json,
                    });
                    ctx.observer.on_auxiliary_data(
                        self.name(),
                        run_index,
                        ENGINE_CAPTURE_KIND,
                        &payload,
                    );
                }
                Err(e) => {
                    // Should not happen — ExecuteResult is fully
                    // serializable — but if it ever does we surface it
                    // without breaking the render path: the capture is a
                    // recording-time concern, not a correctness concern.
                    trace_event!(
                        ctx,
                        EventLevel::Warn,
                        "failed to serialize ExecuteResult for trace capture: {}",
                        e
                    );
                }
            }

            // Accumulate engine-produced includes and supporting files
            // (append-only; later engines add to earlier ones). See
            // bd-o8pr for the supporting-files / resource-report contract.
            ctx.includes
                .header_includes
                .extend(result.includes.header_includes);
            ctx.includes
                .include_before
                .extend(result.includes.include_before);
            ctx.includes
                .include_after
                .extend(result.includes.include_after);
            if !result.supporting_files.is_empty() {
                ctx.resource_report.add_engine_files(
                    engine.name(),
                    &path,
                    std::mem::take(&mut result.supporting_files),
                );
            }

            // Parse the executed markdown back to AST against a per-engine
            // intermediate filename (`<stem>.<engine>.rmarkdown`) so
            // engine-produced blocks attribute to a distinct slot. The
            // SourceInfo offsets index into the engine's output buffer; for
            // the current use (filename attribution in the trace) this is
            // adequate.
            let intermediate_name = intermediate_filename(&path, engine.name());
            let (mut executed_ast, executed_ast_context, parse_warnings) =
                pampa::readers::qmd::read(
                    result.markdown.as_bytes(),
                    false, // loose mode
                    &intermediate_name,
                    &mut std::io::sink(),
                    true, // track source locations
                    None, // file_id
                )
                .map_err(|diagnostics| {
                    PipelineError::stage_error_with_diagnostics(self.name(), diagnostics)
                })?;

            // Register this engine's intermediate as a new file slot. The
            // next FileId is the current file count; `filenames` grows in
            // lock-step with the source context.
            let new_slot = quarto_source_map::FileId(merged_context.filenames.len());
            if let Some(intermediate_file) = executed_ast_context
                .source_context
                .get_file(quarto_source_map::FileId(0))
                .cloned()
            {
                if let Some(info) = intermediate_file.file_info {
                    merged_context
                        .source_context
                        .add_file_with_info(intermediate_name.clone(), info);
                } else {
                    merged_context
                        .source_context
                        .add_file(intermediate_name.clone(), None);
                }
            } else {
                merged_context
                    .source_context
                    .add_file(intermediate_name.clone(), None);
            }
            merged_context.filenames.push(intermediate_name);
            // Carry the executed parse's example-list counter forward; the
            // last engine's value wins so downstream numbering is coherent.
            merged_context
                .example_list_counter
                .set(executed_ast_context.example_list_counter.get());

            // Remap the executed AST's `FileId(0)` to this engine's slot.
            // Kept original blocks keep their (lower) FileIds; new blocks
            // reference `new_slot`.
            quarto_ast_reconcile::remap_file_ids(&mut executed_ast, &|id| {
                quarto_source_map::FileId(id.0 + new_slot.0)
            });

            // Reconcile: keep unchanged content's source locations, take
            // new content from the executed AST. The "original" side may
            // already carry FileIds from earlier engines; reconcile decides
            // keep/replace by content, so mixed provenance is fine.
            let (reconciled_ast, reconciliation_plan) =
                quarto_ast_reconcile::reconcile(ast, executed_ast);
            ast = reconciled_ast;
            warnings.extend(parse_warnings);

            trace_event!(
                ctx,
                EventLevel::Debug,
                "[{}] reconciliation: {} kept, {} replaced, {} recursed",
                engine.name(),
                reconciliation_plan.stats.blocks_kept,
                reconciliation_plan.stats.blocks_replaced,
                reconciliation_plan.stats.blocks_recursed
            );

            // bd-5yff4: record one AST snapshot per engine so a trace can
            // show the sequence step by step (`engine:<name>`). The
            // observer ignores this unless trace recording is active.
            ctx.observer
                .on_engine_data(engine.name(), run_index, &ast, &merged_context);
        }

        Ok(PipelineData::DocumentAst(DocumentAst {
            path,
            ast,
            ast_context: merged_context,
            source_context,
            warnings,
            recorded_includes,
        }))
    }
}

/// Derive the per-engine intermediate filename from the source path.
///
/// `foo.qmd` + engine `knitr` → `foo.knitr.rmarkdown`. Echoes knitr's
/// `.rmarkdown` convention but tags the label with the engine name so a
/// multi-engine sequence (bd-5yff4) gives each engine's produced blocks a
/// distinct provenance slot. Used only as a filename label for source
/// attribution; no file needs to exist on disk.
fn intermediate_filename(source_path: &std::path::Path, engine_name: &str) -> String {
    let stem = source_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    source_path
        .with_file_name(format!("{stem}.{engine_name}.rmarkdown"))
        .display()
        .to_string()
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
        use quarto_source_map::FileId;

        fn walk_inline(i: &Inline, out: &mut std::collections::HashSet<FileId>) {
            match i {
                Inline::Str(x) => x.source_info.collect_file_ids(out),
                Inline::Emph(x) => {
                    for c in &x.content {
                        walk_inline(c, out);
                    }
                    x.source_info.collect_file_ids(out);
                }
                Inline::Strong(x) => {
                    for c in &x.content {
                        walk_inline(c, out);
                    }
                    x.source_info.collect_file_ids(out);
                }
                Inline::Space(x) => x.source_info.collect_file_ids(out),
                Inline::SoftBreak(x) => x.source_info.collect_file_ids(out),
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
                    p.source_info.collect_file_ids(out);
                }
                Block::Header(h) => {
                    for i in &h.content {
                        walk_inline(i, out);
                    }
                    h.source_info.collect_file_ids(out);
                }
                Block::Div(d) => {
                    for b in &d.content {
                        walk_block(b, out);
                    }
                    d.source_info.collect_file_ids(out);
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

        // Filenames are the two-slot merged context (per-engine label).
        assert_eq!(
            result.ast_context.filenames,
            vec![
                "/project/test.qmd".to_string(),
                "/project/test.mock-appending.rmarkdown".to_string(),
            ]
        );

        // Both FileId(0) (.qmd, kept blocks) and FileId(1) (.rmarkdown,
        // appended block) should appear in the reconciled AST.
        let ids = collect_file_ids(&result.ast);
        assert!(
            ids.contains(&quarto_source_map::FileId(0)),
            "expected FileId(0) for kept blocks, got {:?}",
            ids
        );
        assert!(
            ids.contains(&quarto_source_map::FileId(1)),
            "expected FileId(1) for appended block, got {:?}",
            ids
        );
        // No stray FileIds.
        for id in &ids {
            assert!(
                id.0 < 2,
                "unexpected FileId {:?} in reconciled AST (merged context has 2 slots)",
                id
            );
        }
    }

    // ──────────────────────────────────────────────────────────────
    // bd-5yff4 Phase 1: sequential multi-engine execution
    // ──────────────────────────────────────────────────────────────

    /// Two engines run in declared order, and the first engine's output
    /// feeds the second: `fixture-a` fills its cell with a `{fixture-b}`
    /// cell, which `fixture-b` (next in the sequence) then fills. The
    /// final AST must contain only the second engine's output, with both
    /// engines' source cells consumed.
    #[tokio::test]
    async fn test_two_engines_run_in_sequence_with_handoff() {
        use crate::engine::FixtureEngine;

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(FixtureEngine::with_results(
            "fixture-a",
            vec!["```{fixture-b}\nhanded-off\n```".to_string()],
        )));
        registry.register(Arc::new(FixtureEngine::with_results(
            "fixture-b",
            vec!["Final output from B.".to_string()],
        )));

        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();

        let content = b"---\nengine: [fixture-a, fixture-b]\n---\n\n```{fixture-a}\nseed\n```\n";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().expect("DocumentAst");

        let qmd = serialize_ast_to_qmd(&result.ast).unwrap().0;
        assert!(
            qmd.contains("Final output from B."),
            "second engine's output should be present; got:\n{qmd}"
        );
        assert!(
            !qmd.contains("{fixture-a}"),
            "fixture-a cell should be consumed; got:\n{qmd}"
        );
        assert!(
            !qmd.contains("{fixture-b}"),
            "fixture-b cell should be consumed; got:\n{qmd}"
        );
    }

    /// FileId coherence across a two-engine sequence: kept original blocks
    /// keep `FileId(0)` (the `.qmd`), blocks produced by the first engine
    /// get `FileId(1)` (its intermediate), and blocks produced by the
    /// second get `FileId(2)` (its intermediate). No stray FileIds.
    #[tokio::test]
    async fn test_two_engines_assign_distinct_file_ids() {
        use crate::engine::FixtureEngine;

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(FixtureEngine::with_results(
            "fixture-a",
            vec!["Para from A.".to_string()],
        )));
        registry.register(Arc::new(FixtureEngine::with_results(
            "fixture-b",
            vec!["Para from B.".to_string()],
        )));

        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();

        // One kept prose block + one cell per engine (both present from
        // the start; fixture-a leaves fixture-b's cell untouched).
        let content = b"---\nengine: [fixture-a, fixture-b]\n---\n\nOriginal prose.\n\n```{fixture-a}\na\n```\n\n```{fixture-b}\nb\n```\n";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().expect("DocumentAst");

        // Three slots: .qmd, fixture-a intermediate, fixture-b intermediate.
        assert_eq!(
            result.ast_context.filenames,
            vec![
                "/project/test.qmd".to_string(),
                "/project/test.fixture-a.rmarkdown".to_string(),
                "/project/test.fixture-b.rmarkdown".to_string(),
            ],
            "expected three file slots (original + one per engine)"
        );

        let ids = collect_file_ids(&result.ast);
        assert!(
            ids.contains(&quarto_source_map::FileId(0)),
            "FileId(0): {ids:?}"
        );
        assert!(
            ids.contains(&quarto_source_map::FileId(1)),
            "FileId(1): {ids:?}"
        );
        assert!(
            ids.contains(&quarto_source_map::FileId(2)),
            "FileId(2): {ids:?}"
        );
        for id in &ids {
            assert!(id.0 < 3, "unexpected FileId {id:?} (3 slots): {ids:?}");
        }
    }

    /// A duplicate engine in the sequence is de-duplicated (run once) and
    /// produces a "Duplicate engine" diagnostic.
    #[tokio::test]
    async fn test_duplicate_engine_dedups_and_warns() {
        use crate::engine::FixtureEngine;

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(FixtureEngine::with_results(
            "fixture-a",
            vec!["spliced once".to_string()],
        )));

        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();

        // engine declared twice; one fixture-a cell. If dedup failed,
        // fixture-a would run twice and the second run's count check
        // (0 cells vs 1 result) would error.
        let content = b"---\nengine: [fixture-a, fixture-a]\n---\n\n```{fixture-a}\nx\n```\n";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().expect("DocumentAst");

        let qmd = serialize_ast_to_qmd(&result.ast).unwrap().0;
        assert!(qmd.contains("spliced once"), "got:\n{qmd}");
        // Only one intermediate slot — the engine ran exactly once.
        assert_eq!(result.ast_context.filenames.len(), 2);
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.title.contains("Duplicate engine 'fixture-a'")),
            "expected a duplicate-engine diagnostic; got: {:?}",
            ctx.diagnostics
        );
    }

    /// A markdown engine anywhere in the sequence is skipped (no-op),
    /// while the other engines still run.
    #[tokio::test]
    async fn test_markdown_in_sequence_is_skipped() {
        use crate::engine::FixtureEngine;

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(FixtureEngine::with_results(
            "fixture-a",
            vec!["from A".to_string()],
        )));

        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();

        let content = b"---\nengine: [markdown, fixture-a]\n---\n\n```{fixture-a}\nx\n```\n";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();
        let result = output.into_document_ast().expect("DocumentAst");

        let qmd = serialize_ast_to_qmd(&result.ast).unwrap().0;
        assert!(qmd.contains("from A"), "got:\n{qmd}");
        // Only fixture-a added a slot; markdown was skipped.
        assert_eq!(
            result.ast_context.filenames,
            vec![
                "/project/test.qmd".to_string(),
                "/project/test.fixture-a.rmarkdown".to_string(),
            ]
        );
    }

    /// bd-5yff4 Phase 3: a multi-engine run records one `EngineCapture`
    /// per engine (in order) AND one `engine:<name>` AST snapshot per
    /// engine in the trace pipeline. Round-trips through a real gzipped
    /// trace on disk.
    #[tokio::test]
    async fn test_multi_engine_trace_records_per_engine_snapshots_and_captures() {
        use crate::engine::FixtureEngine;
        use crate::stage::JsonTraceObserver;
        use quarto_trace::RenderInfo;

        let dir = std::env::temp_dir().join(format!(
            "quarto-trace-multi-engine-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
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

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(FixtureEngine::with_results(
            "fixture-a",
            vec!["```{fixture-b}\nfrom-a\n```".to_string()],
        )));
        registry.register(Arc::new(FixtureEngine::with_results(
            "fixture-b",
            vec!["Final B".to_string()],
        )));
        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();
        ctx.observer = observer.clone();

        let content = b"---\nengine: [fixture-a, fixture-b]\n---\n\n```{fixture-a}\nseed\n```\n";
        let doc_ast = parse_qmd_to_ast(content, "/project/test.qmd");
        stage
            .run(PipelineData::DocumentAst(doc_ast), &mut ctx)
            .await
            .unwrap();

        observer.write_trace().unwrap();
        let read_back = quarto_trace::read::read_trace(&trace_path).unwrap();

        // One capture per engine, in execution order.
        assert_eq!(
            read_back
                .engine_captures
                .iter()
                .map(|c| c.engine_name.as_str())
                .collect::<Vec<_>>(),
            vec!["fixture-a", "fixture-b"]
        );

        // One AST snapshot per engine, in order.
        let engine_entries: Vec<_> = read_back
            .pipeline
            .iter()
            .filter(|e| e.stage.starts_with("engine:"))
            .map(|e| e.stage.as_str())
            .collect();
        assert_eq!(engine_entries, vec!["engine:fixture-a", "engine:fixture-b"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// bd-5yff4 Phase 3: record the captures from a two-engine run, then
    /// replay them via `with_replay_many` against the same document. The
    /// replay must be byte-clean — engine N+1's input is engine N's
    /// reconciled-then-reserialized output, so this is the determinism
    /// invariant the sequence relies on — and produce identical output.
    #[tokio::test]
    async fn test_multi_engine_record_then_replay_is_byte_clean() {
        use crate::engine::{EngineRegistry, FixtureEngine, ReplayEngine};
        use crate::stage::JsonTraceObserver;
        use quarto_trace::RenderInfo;

        let content = b"---\nengine: [fixture-a, fixture-b]\n---\n\n```{fixture-a}\nseed\n```\n";

        // --- Record pass: real fixture engines + trace observer. ---
        let dir = std::env::temp_dir().join(format!(
            "quarto-replay-multi-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let trace_path = dir.join("latest.json.gz");
        let observer = Arc::new(JsonTraceObserver::new(
            trace_path.clone(),
            RenderInfo::default(),
        ));

        let mut registry = EngineRegistry::new();
        registry.register(Arc::new(FixtureEngine::with_results(
            "fixture-a",
            vec!["```{fixture-b}\nfrom-a\n```".to_string()],
        )));
        registry.register(Arc::new(FixtureEngine::with_results(
            "fixture-b",
            vec!["Final B".to_string()],
        )));
        let stage = EngineExecutionStage::with_registry(registry);
        let mut ctx = make_test_context();
        ctx.observer = observer.clone();

        let output_recorded = stage
            .run(
                PipelineData::DocumentAst(parse_qmd_to_ast(content, "/project/test.qmd")),
                &mut ctx,
            )
            .await
            .unwrap()
            .into_document_ast()
            .unwrap();
        let recorded_qmd = serialize_ast_to_qmd(&output_recorded.ast).unwrap().0;

        observer.write_trace().unwrap();
        let captures = quarto_trace::read::read_trace(&trace_path)
            .unwrap()
            .engine_captures;
        assert_eq!(captures.len(), 2);

        // --- Replay pass: substitute a ReplayEngine per recorded engine. ---
        let mut replay_registry = EngineRegistry::new();
        for capture in captures {
            replay_registry.register(Arc::new(ReplayEngine::new(capture)));
        }
        let replay_stage = EngineExecutionStage::with_registry(replay_registry);
        let mut replay_ctx = make_test_context();

        let output_replayed = replay_stage
            .run(
                PipelineData::DocumentAst(parse_qmd_to_ast(content, "/project/test.qmd")),
                &mut replay_ctx,
            )
            .await
            .expect("replay of a recorded two-engine sequence must succeed byte-clean")
            .into_document_ast()
            .unwrap();
        let replayed_qmd = serialize_ast_to_qmd(&output_replayed.ast).unwrap().0;

        assert_eq!(
            replayed_qmd, recorded_qmd,
            "replay output must match the recorded run exactly"
        );
        assert!(replayed_qmd.contains("Final B"));

        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(filenames[1], "/project/test.mock-includes.rmarkdown");
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
        // ExecutionContext::new uses SourceInfo::default() as the "no
        // source location known yet" sentinel; `with_source_info` later
        // overwrites it with the real qmd serialization range. This test
        // pins that production behavior — Phase 7's deprecation will
        // force engine/context.rs:92 onto an explicit `By::*` kind at
        // which point this assertion gets updated alongside.
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
        assert_eq!(
            mapped.file_id,
            quarto_source_map::FileId(0),
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

        assert_eq!(
            read_back.engine_captures.len(),
            1,
            "exactly one engine capture must be recorded"
        );
        let capture = &read_back.engine_captures[0];
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
        // The three identical pre-populated DocumentAst snapshots collapse
        // to one stored entry; the per-engine `on_engine_data` snapshot
        // (bd-5yff4) is a distinct post-reconcile AST, so two are stored.
        // The key invariant is that dedup happened — without it the three
        // identical snapshots alone would store three entries.
        assert_eq!(
            asts.len(),
            2,
            "3 identical snapshots collapse to 1; the per-engine snapshot adds 1"
        );
        let captures = raw_json["engine_captures"].as_array().expect(
            "v2 unified artifact must have an `engine_captures` array for non-markdown engines",
        );
        assert_eq!(captures.len(), 1, "one engine ran → one capture");
        assert_eq!(captures[0]["engine_name"], "mock-includes");

        // 2. read_trace round-trip: rehydrated doc has both deduped
        //    ASTs (folded back inline; `asts` empty) and the engine
        //    capture intact.
        let read_back = quarto_trace::read::read_trace(&trace_path).unwrap();
        assert!(
            read_back.asts.is_empty(),
            "reader must clear `asts` after rehydration"
        );
        assert_eq!(
            read_back.engine_captures.len(),
            1,
            "engine capture must round-trip through gzipped trace"
        );
        let capture = &read_back.engine_captures[0];
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
