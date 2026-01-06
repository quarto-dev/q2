/*
 * stage/stages/ast_transforms.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Apply AST transforms to the document.
 */

//! Apply AST transforms to the document.
//!
//! This stage runs the Quarto-specific AST transformations on the parsed
//! document, including callouts, cross-references, metadata normalization, etc.

use async_trait::async_trait;
use quarto_config::MergedConfig;

use crate::pipeline::build_transform_pipeline;
use crate::render::{BinaryDependencies, RenderContext};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;
use crate::transform::TransformPipeline;

/// Apply AST transforms to the document.
///
/// This stage:
/// 1. Takes a parsed DocumentAst
/// 2. Merges project config with document metadata (if project config exists)
/// 3. Runs the standard transform pipeline (callouts, metadata, title block, etc.)
/// 4. Returns the transformed DocumentAst
///
/// # Transform Pipeline
///
/// By default, this stage uses the standard transform pipeline from
/// [`build_transform_pipeline`]. You can provide a custom pipeline
/// for specialized use cases.
///
/// # Bridging to RenderContext
///
/// The existing `TransformPipeline` API uses `RenderContext<'a>` which has
/// lifetime parameters. This stage creates a temporary `RenderContext` from
/// the owned `StageContext` data using `std::mem::take` to transfer artifacts,
/// then restores them after transforms complete.
///
/// # Input
///
/// - `DocumentAst` - Parsed Pandoc AST with source context
///
/// # Output
///
/// - `DocumentAst` - Transformed AST (same structure, modified content)
///
/// # Errors
///
/// Returns an error if any transform in the pipeline fails.
pub struct AstTransformsStage {
    pipeline: TransformPipeline,
}

impl AstTransformsStage {
    /// Create an AstTransformsStage with the standard transform pipeline.
    pub fn new() -> Self {
        Self {
            pipeline: build_transform_pipeline(),
        }
    }

    /// Create an AstTransformsStage with a custom transform pipeline.
    pub fn with_pipeline(pipeline: TransformPipeline) -> Self {
        Self { pipeline }
    }
}

impl Default for AstTransformsStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PipelineStage for AstTransformsStage {
    fn name(&self) -> &str {
        "ast-transforms"
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
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Merge project config with document metadata.
        // Project format_config provides defaults that document metadata can override.
        // This enables WASM to inject settings like `format.html.source-location: full`.
        if let Some(format_config) = ctx
            .project
            .config
            .as_ref()
            .and_then(|c| c.format_config.as_ref())
        {
            // MergedConfig: later layers (document) override earlier layers (project)
            let merged = MergedConfig::new(vec![format_config, &doc.ast.meta]);
            if let Ok(materialized) = merged.materialize() {
                trace_event!(
                    ctx,
                    EventLevel::Debug,
                    "merged project config with document metadata"
                );
                doc.ast.meta = materialized;
            }
            // Note: If materialization fails (shouldn't happen with well-formed configs),
            // we silently continue with the original document metadata.
        }

        let transform_count = self.pipeline.len();
        trace_event!(
            ctx,
            EventLevel::Debug,
            "running {} AST transforms",
            transform_count
        );

        // Discover binary dependencies from the runtime
        let binaries = BinaryDependencies::discover(ctx.runtime.as_ref());

        // Create a RenderContext from StageContext data.
        // We use std::mem::take to temporarily transfer ownership of artifacts.
        let mut render_ctx =
            RenderContext::new(&ctx.project, &ctx.document, &ctx.format, &binaries);

        // Transfer artifacts to the RenderContext
        render_ctx.artifacts = std::mem::take(&mut ctx.artifacts);

        // Execute the transform pipeline
        let result = self.pipeline.execute(&mut doc.ast, &mut render_ctx);

        // Transfer artifacts back to StageContext
        ctx.artifacts = render_ctx.artifacts;

        // Handle result
        result.map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;

        trace_event!(ctx, EventLevel::Debug, "AST transforms complete");

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::stage::DocumentAst;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::SourceContext;
    use quarto_system_runtime::TempDir;
    use std::path::PathBuf;
    use std::sync::Arc;

    // Mock runtime for testing
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
        fn temp_dir(&self, _template: &str) -> quarto_system_runtime::RuntimeResult<TempDir> {
            Ok(TempDir::new(PathBuf::from("/tmp/test")))
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

    #[tokio::test]
    async fn test_ast_transforms_empty_pipeline() {
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

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();

        // Use an empty pipeline for testing
        let stage = AstTransformsStage::with_pipeline(TransformPipeline::new());

        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc::default(),
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();

        assert!(output.into_document_ast().is_some());
    }
}
