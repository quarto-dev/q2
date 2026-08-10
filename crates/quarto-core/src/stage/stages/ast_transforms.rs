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

use crate::pipeline::{build_q2_preview_transform_pipeline, build_transform_pipeline};
use crate::render::{BinaryDependencies, RenderContext};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;
use crate::transform::TransformPipeline;

/// Apply AST transforms to the document.
///
/// This stage runs the Quarto-specific AST transformations on the parsed
/// document (callouts, metadata normalization, title block, TOC, etc.).
///
/// Metadata merging is handled by the upstream [`MetadataMergeStage`] —
/// by the time this stage runs, `doc.ast.meta` already contains the
/// fully merged and format-flattened config.
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
    /// Custom pipeline (set via `with_pipeline`). If `None`, the pipeline
    /// is built just-in-time in `run()` using `StageContext` data.
    custom_pipeline: Option<TransformPipeline>,
}

impl AstTransformsStage {
    /// Create an AstTransformsStage that builds the pipeline at run time.
    ///
    /// The pipeline is constructed in `run()` using data from `StageContext`
    /// (extensions, runtime, format) needed by the shortcode transform.
    pub fn new() -> Self {
        Self {
            custom_pipeline: None,
        }
    }

    /// Create an AstTransformsStage with a custom transform pipeline.
    ///
    /// The provided pipeline is used as-is, bypassing `StageContext`-based
    /// construction. Used in tests that only need built-in handlers.
    pub fn with_pipeline(pipeline: TransformPipeline) -> Self {
        Self {
            custom_pipeline: Some(pipeline),
        }
    }
}

impl Default for AstTransformsStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
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

        // Build the JIT pipeline if no custom pipeline was provided.
        //
        // Dispatch on `ctx.format.pipeline_kind` (added in Plan 1
        // commit 3): `Some("preview")` builds the q2-preview transform
        // list, everything else builds the standard HTML one. The
        // `target_format` argument carries the original string
        // (e.g. `"q2-preview"`, not the base `"html"`) so shortcode
        // resolution and downstream transforms see the user-facing
        // format identity, not the pseudo-format's base.
        let jit_pipeline;
        let pipeline = if let Some(ref p) = self.custom_pipeline {
            p
        } else {
            // Build pipeline JIT using StageContext data needed by shortcode transform
            let document_dir = ctx
                .document
                .input
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let shortcode_paths =
                crate::transforms::extract_shortcode_paths(&doc.ast.meta, document_dir);
            jit_pipeline = match ctx.format.pipeline_kind {
                Some("preview") => build_q2_preview_transform_pipeline(
                    shortcode_paths,
                    ctx.extensions.clone(),
                    ctx.runtime.clone(),
                    ctx.format.target_format.clone(),
                    ctx.variables.clone(),
                    ctx.project_env.clone(),
                ),
                _ => build_transform_pipeline(
                    shortcode_paths,
                    ctx.extensions.clone(),
                    ctx.runtime.clone(),
                    ctx.format.target_format.clone(),
                    ctx.variables.clone(),
                    ctx.project_env.clone(),
                ),
            };
            &jit_pipeline
        };

        let transform_count = pipeline.len();
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

        // Transfer mutable state to the RenderContext
        render_ctx.artifacts = std::mem::take(&mut ctx.artifacts);
        render_ctx.includes = std::mem::take(&mut ctx.includes);
        render_ctx.ref_type_registry = ctx.ref_type_registry.take();
        render_ctx.crossref_index = ctx.crossref_index.take();
        render_ctx.observer = ctx.observer.clone();
        // `project_index` is read-only to transforms, so we clone the
        // `Arc` instead of moving it. Leaving `ctx.project_index`
        // untouched means later stages in the pipeline still see it.
        render_ctx.project_index = ctx.project_index.clone();
        // Phase 6: bridge the resource resolver in the same way —
        // read-only to transforms, the AST-side body-link rewriter
        // (`LinkRewriteTransform`) consumes it to compute
        // page-relative URLs.
        render_ctx.resource_resolver = ctx.resource_resolver.clone();
        // bd-qor9a: bridge the document's `SourceContext` so transforms
        // can resolve a `SourceInfo.FileId` back to the originating
        // file path. Used by sidebar/navbar/footer/page-nav Generate
        // transforms to determine which YAML file a given href was
        // authored in (so frontmatter-rooted paths resolve relative
        // to the frontmatter's directory, not the project root).
        render_ctx.source_context = Some(&doc.ast_context.source_context);
        // Attribution: bridge both the opt-in provider AND the
        // sidecar already populated by the upstream
        // `AttributionGenerateStage` into the inner `RenderContext`.
        //
        // `attribution_data` is the load-bearing field —
        // `AttributionRenderTransform` reads it to bake the
        // writer-side lookup. The provider is forwarded only for
        // historical-compat callers that build a fresh inner
        // pipeline outside the normal stage flow; no transform here
        // calls `build()` itself anymore.
        render_ctx.attribution_provider = ctx.attribution_provider.clone();
        render_ctx.attribution_data = ctx.attribution_data.clone();
        // bd-cfl67: resource-copy intents collected by transforms
        // (notably `ResourceCollectorTransform`) need to surface to
        // the outer renderer for the final sink flush. Move ownership
        // across the bridge in both directions, same as `artifacts`.
        render_ctx.resource_copies = std::mem::take(&mut ctx.resource_copies);

        // Execute the transform pipeline
        let result = pipeline
            .execute(&mut doc.ast, &doc.ast_context, &mut render_ctx)
            .await;

        // Transfer mutable state back to StageContext
        ctx.artifacts = render_ctx.artifacts;
        ctx.resource_copies = render_ctx.resource_copies;
        ctx.includes = render_ctx.includes;
        ctx.ref_type_registry = render_ctx.ref_type_registry;
        ctx.crossref_index = render_ctx.crossref_index;
        // Bridge `format_options` back so downstream stages
        // (`RenderHtmlBodyStage`, future JSON writer entry) see the
        // attribution lookup + identities populated by
        // `AttributionRenderTransform`. No-op when attribution is off
        // (default `FormatOptions` has all `None` fields).
        ctx.format_options = render_ctx.format_options;

        // Transfer any diagnostics collected during transforms
        ctx.diagnostics.extend(render_ctx.diagnostics);

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

    #[tokio::test]
    async fn test_ast_transforms_empty_pipeline() {
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

        let mut ctx = StageContext::new(runtime, format, project, doc).unwrap();

        // Use an empty pipeline for testing
        let stage = AstTransformsStage::with_pipeline(TransformPipeline::new());

        let doc_ast = DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc::default(),
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
        };

        let input = PipelineData::DocumentAst(doc_ast);
        let output = stage.run(input, &mut ctx).await.unwrap();

        assert!(output.into_document_ast().is_some());
    }
}
