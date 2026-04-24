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

        let result = engine
            .execute(&qmd, &exec_context)
            .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;

        trace_event!(
            ctx,
            EventLevel::Debug,
            "engine produced {} bytes of markdown",
            result.markdown.len()
        );

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
        let (mut executed_ast, executed_ast_context, parse_warnings) = pampa::readers::qmd::read(
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
        // Slot 0 = original `.qmd` (FileId(0) in original AST). Slot 1 =
        // intermediate `.rmarkdown` (FileId(0) in executed AST — remapped
        // to FileId(1) below). This keeps the reconcile contract intact:
        // FileId in the reconciled AST identifies provenance.
        let mut merged_ast_context = doc_ast.ast_context.clone();
        // `executed_ast_context` has the intermediate file as FileId(0) with
        // the right FileInformation (line breaks, total length) — carry that
        // into the merged context at slot 1.
        if let Some(intermediate_file) = executed_ast_context
            .source_context
            .get_file(quarto_source_map::FileId(0))
            .cloned()
        {
            if let Some(info) = intermediate_file.file_info {
                merged_ast_context
                    .source_context
                    .add_file_with_info(intermediate_name.clone(), info);
            } else {
                merged_ast_context
                    .source_context
                    .add_file(intermediate_name.clone(), None);
            }
        } else {
            merged_ast_context
                .source_context
                .add_file(intermediate_name.clone(), None);
        }
        merged_ast_context.filenames.push(intermediate_name);
        // Example-list counter is cell-ordering state tied to the executed
        // AST. Preserve the executed parse's final value so subsequent
        // example-list numbering stays coherent.
        merged_ast_context
            .example_list_counter
            .set(executed_ast_context.example_list_counter.get());

        // Step 7b: Pre-remap the executed AST so its `FileId(0)` references
        // become `FileId(1)` (the intermediate file's slot in the merged
        // context). After this, kept original blocks still reference
        // `FileId(0)` (the `.qmd`) and new executed blocks reference
        // `FileId(1)` (the intermediate).
        quarto_ast_reconcile::remap_file_ids(&mut executed_ast, &|id| {
            quarto_source_map::FileId(id.0 + 1)
        });

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
}
