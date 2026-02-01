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

#[async_trait]
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
        let qmd = serialize_ast_to_qmd(&doc_ast.ast)?;

        trace_event!(
            ctx,
            EventLevel::Debug,
            "serialized AST to {} bytes of QMD",
            qmd.len()
        );

        // Step 5: Prepare execution context
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
        .with_engine_config(detected.config.clone());

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

        // Step 7: Parse the executed markdown back to AST
        let source_name = doc_ast.path.display().to_string();
        let (executed_ast, new_ast_context, parse_warnings) = pampa::readers::qmd::read(
            result.markdown.as_bytes(),
            false,        // loose mode
            &source_name, // filename for error messages
            &mut std::io::sink(),
            true, // track source locations
            None, // file_id
        )
        .map_err(|diagnostics| {
            PipelineError::stage_error_with_diagnostics(self.name(), diagnostics)
        })?;

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
            ast_context: new_ast_context,
            source_context: doc_ast.source_context,
            warnings,
        }))
    }
}

/// Serialize a Pandoc AST to QMD text.
///
/// This produces QMD that can be fed to execution engines.
/// Uses pampa's QMD writer which preserves code cell attributes.
fn serialize_ast_to_qmd(
    ast: &quarto_pandoc_types::pandoc::Pandoc,
) -> Result<String, PipelineError> {
    let mut buffer = Vec::new();
    pampa::writers::qmd::write(ast, &mut buffer).map_err(|diagnostics| {
        PipelineError::stage_error_with_diagnostics("engine-execution", diagnostics)
    })?;

    String::from_utf8(buffer).map_err(|e| {
        PipelineError::stage_error(
            "engine-execution",
            format!("QMD serialization produced invalid UTF-8: {}", e),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::LoadedSource;
    use std::path::PathBuf;

    // Helper to create a mock runtime
    struct MockRuntime;

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
        fn fetch_url(&self, _url: &str) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
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
            config: None,
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

        let qmd = serialize_ast_to_qmd(&doc_ast.ast).unwrap();

        // Should contain the title
        assert!(qmd.contains("title"));
        // Should contain the heading
        assert!(qmd.contains("Hello"));
        // Should contain the paragraph
        assert!(qmd.contains("World"));
    }

    #[test]
    fn test_stage_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<EngineExecutionStage>();
    }
}
