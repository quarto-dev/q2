/*
 * pipeline.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Unified render pipeline for Quarto.
 */

//! Unified render pipeline.
//!
//! This module provides the core render pipeline used by both the CLI and WASM
//! clients. By using the same pipeline, we ensure feature parity between
//! different rendering contexts.
//!
//! ## Pipeline Stages
//!
//! 1. **Parse**: QMD source → Pandoc AST (via `pampa`)
//! 2. **Engine execution**: Execute code cells (Jupyter, Knitr, or markdown passthrough)
//! 3. **Transform**: Apply Quarto-specific transforms (callouts, metadata, etc.)
//! 4. **Render body**: Pandoc AST → HTML body (via `pampa`)
//! 5. **Apply template**: Wrap body with HTML template
//!
//! ## Usage
//!
//! The main entry point is the async [`render_qmd_to_html`] function:
//!
//! ```ignore
//! use quarto_core::pipeline::{render_qmd_to_html, HtmlRenderConfig};
//!
//! // Async usage (WASM or native async context)
//! let output = render_qmd_to_html(
//!     content.as_bytes(),
//!     "input.qmd",
//!     &mut render_ctx,
//!     &HtmlRenderConfig::default(),
//! ).await?;
//!
//! // Sync usage on native (CLI)
//! let output = pollster::block_on(render_qmd_to_html(
//!     content.as_bytes(),
//!     "input.qmd",
//!     &mut render_ctx,
//!     &HtmlRenderConfig::default(),
//! ))?;
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use quarto_doctemplate::Template;
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceContext;

use crate::Result;
use crate::render::RenderContext;
use crate::stage::stages::ApplyTemplateConfig;
use crate::stage::{
    ApplyTemplateStage, AstTransformsStage, EngineExecutionStage, LoadedSource, ParseDocumentStage,
    Pipeline, PipelineData, PipelineStage, RenderHtmlBodyStage, StageContext,
};
use crate::transform::TransformPipeline;
use crate::transforms::{
    AppendixStructureTransform, CalloutResolveTransform, CalloutTransform, FootnotesTransform,
    MetadataNormalizeTransform, ResourceCollectorTransform, SectionizeTransform,
    TitleBlockTransform, TocGenerateTransform, TocRenderTransform,
};

/// Well-known path for the default CSS artifact in WASM context.
///
/// This path is used by both the render pipeline (to store the artifact)
/// and the browser post-processor (to resolve the CSS reference).
pub const DEFAULT_CSS_ARTIFACT_PATH: &str = "/.quarto/project-artifacts/styles.css";

/// Configuration for HTML rendering.
#[derive(Debug, Default)]
pub struct HtmlRenderConfig<'a> {
    /// CSS paths to include in the document (relative to the output HTML).
    /// If empty, the default CSS artifact will be used.
    pub css_paths: &'a [String],

    /// Custom template to use. If `None`, the built-in HTML5 template is used.
    pub template: Option<&'a Template>,
}

impl<'a> HtmlRenderConfig<'a> {
    /// Create a new configuration with custom CSS paths.
    pub fn with_css(css_paths: &'a [String]) -> Self {
        Self {
            css_paths,
            template: None,
        }
    }

    /// Create a new configuration with a custom template.
    pub fn with_template(template: &'a Template) -> Self {
        Self {
            css_paths: &[],
            template: Some(template),
        }
    }
}

/// Output from the render pipeline.
#[derive(Debug)]
pub struct RenderOutput {
    /// The rendered HTML content.
    pub html: String,
    /// Non-fatal warnings collected during rendering.
    pub warnings: Vec<DiagnosticMessage>,
    /// Source context for mapping locations in diagnostics.
    pub source_context: SourceContext,
}

/// Build the standard HTML pipeline stages.
///
/// Returns the stages as a vector, allowing callers to customize before
/// creating the pipeline. For most uses, prefer [`build_html_pipeline`].
///
/// This creates stages for:
/// 1. `ParseDocumentStage` - Parse QMD to Pandoc AST
/// 2. `EngineExecutionStage` - Execute code cells (jupyter, knitr, or markdown passthrough)
/// 3. `AstTransformsStage` - Run Quarto transforms (callouts, metadata, etc.)
/// 4. `RenderHtmlBodyStage` - Render AST to HTML body
/// 5. `ApplyTemplateStage` - Apply HTML template
pub fn build_html_pipeline_stages() -> Vec<Box<dyn PipelineStage>> {
    vec![
        Box::new(ParseDocumentStage::new()),
        Box::new(EngineExecutionStage::new()),
        Box::new(AstTransformsStage::new()),
        Box::new(RenderHtmlBodyStage::new()),
        Box::new(ApplyTemplateStage::new()),
    ]
}

/// Build the standard HTML pipeline.
///
/// This creates a pipeline with the following stages:
/// 1. `ParseDocumentStage` - Parse QMD to Pandoc AST
/// 2. `EngineExecutionStage` - Execute code cells (jupyter, knitr, or markdown passthrough)
/// 3. `AstTransformsStage` - Run Quarto transforms (callouts, metadata, etc.)
/// 4. `RenderHtmlBodyStage` - Render AST to HTML body
/// 5. `ApplyTemplateStage` - Apply HTML template
///
/// # Returns
///
/// A validated `Pipeline` ready for execution.
///
/// # Panics
///
/// Panics if the pipeline stages have incompatible types (should never happen
/// with the standard stages).
pub fn build_html_pipeline() -> Pipeline {
    Pipeline::new(build_html_pipeline_stages()).expect("HTML pipeline stages should be compatible")
}

/// Build a WASM-compatible HTML pipeline (no engine execution).
///
/// This creates a pipeline suitable for browser environments where code
/// execution is not available. It includes all AST transforms for feature
/// parity with native rendering (callouts, TOC, sectionize, etc.), but
/// skips the engine execution stage.
///
/// Stages:
/// 1. `ParseDocumentStage` - Parse QMD to Pandoc AST
/// 2. `AstTransformsStage` - Run Quarto transforms (callouts, metadata, TOC, etc.)
/// 3. `RenderHtmlBodyStage` - Render AST to HTML body
/// 4. `ApplyTemplateStage` - Apply HTML template
///
/// # Returns
///
/// A validated `Pipeline` ready for execution.
///
/// # Panics
///
/// Panics if the pipeline stages have incompatible types (should never happen
/// with the standard stages).
pub fn build_wasm_html_pipeline() -> Pipeline {
    let stages: Vec<Box<dyn PipelineStage>> = vec![
        Box::new(ParseDocumentStage::new()),
        // No EngineExecutionStage - code cells pass through as-is
        Box::new(AstTransformsStage::new()),
        Box::new(RenderHtmlBodyStage::new()),
        Box::new(ApplyTemplateStage::new()),
    ];

    Pipeline::new(stages).expect("WASM HTML pipeline stages should be compatible")
}

/// Build an HTML pipeline from custom stages.
///
/// This allows full control over which stages are included in the pipeline.
/// Use this when you need a specialized pipeline configuration.
///
/// # Arguments
///
/// * `stages` - The stages to include in the pipeline
///
/// # Returns
///
/// A `Result` containing the validated `Pipeline`, or an error if the
/// stages have incompatible input/output types.
///
/// # Example
///
/// ```ignore
/// use quarto_core::pipeline::build_html_pipeline_with_stages;
/// use quarto_core::stage::{ParseDocumentStage, AstTransformsStage, RenderHtmlBodyStage};
///
/// // Build a minimal pipeline without template application
/// let stages: Vec<Box<dyn PipelineStage>> = vec![
///     Box::new(ParseDocumentStage::new()),
///     Box::new(AstTransformsStage::new()),
///     Box::new(RenderHtmlBodyStage::new()),
/// ];
/// let pipeline = build_html_pipeline_with_stages(stages)?;
/// ```
pub fn build_html_pipeline_with_stages(
    stages: Vec<Box<dyn PipelineStage>>,
) -> std::result::Result<Pipeline, crate::stage::PipelineValidationError> {
    Pipeline::new(stages)
}

/// Render QMD content to HTML.
///
/// This is the unified async render pipeline used by both CLI and WASM. It:
/// 1. Parses the QMD content to a Pandoc AST
/// 2. Runs the transform pipeline (callouts, metadata normalization, etc.)
/// 3. Renders the AST to HTML body
/// 4. Applies the HTML template
///
/// # Arguments
///
/// * `content` - The QMD source content as bytes
/// * `source_name` - Name of the source file (for error messages)
/// * `ctx` - Render context containing project, document, format info
/// * `config` - HTML render configuration (CSS paths, template)
/// * `runtime` - System runtime for filesystem operations
///
/// # Returns
///
/// A `RenderOutput` containing the HTML and any collected artifacts.
///
/// # Errors
///
/// Returns an error if parsing fails, transforms fail, or rendering fails.
///
/// # Example
///
/// ```ignore
/// // WASM usage (async)
/// let output = render_qmd_to_html(
///     content, "input.qmd", &mut ctx, &config, runtime
/// ).await?;
///
/// // Native CLI usage (sync via pollster)
/// let output = pollster::block_on(render_qmd_to_html(
///     content, "input.qmd", &mut ctx, &config, runtime
/// ))?;
/// ```
pub async fn render_qmd_to_html(
    content: &[u8],
    source_name: &str,
    ctx: &mut RenderContext<'_>,
    config: &HtmlRenderConfig<'_>,
    runtime: Arc<dyn quarto_system_runtime::SystemRuntime>,
) -> Result<RenderOutput> {
    // Create StageContext from RenderContext data
    let mut stage_ctx = StageContext::new(
        runtime,
        ctx.format.clone(),
        ctx.project.clone(),
        ctx.document.clone(),
    )
    .map_err(|e| crate::error::QuartoError::Other(e.to_string()))?;

    // Transfer artifacts from RenderContext to StageContext
    stage_ctx.artifacts = std::mem::take(&mut ctx.artifacts);

    // Create input from content
    let input = PipelineData::LoadedSource(LoadedSource::new(
        PathBuf::from(source_name),
        content.to_vec(),
    ));

    // Build pipeline based on config
    // If custom CSS or template is specified, use a customized ApplyTemplateStage
    let pipeline = if config.template.is_some() || !config.css_paths.is_empty() {
        let apply_config = ApplyTemplateConfig::new().with_css_paths(config.css_paths.to_vec());
        // If custom template is provided, we'd need to pass it too
        // For now, css_paths is the main customization needed

        let stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(ParseDocumentStage::new()),
            Box::new(EngineExecutionStage::new()),
            Box::new(AstTransformsStage::new()),
            Box::new(RenderHtmlBodyStage::new()),
            Box::new(ApplyTemplateStage::with_config(apply_config)),
        ];
        Pipeline::new(stages).expect("HTML pipeline stages should be compatible")
    } else {
        build_html_pipeline()
    };

    // Run the async pipeline
    let result = pipeline.run(input, &mut stage_ctx).await;

    // Transfer artifacts back to RenderContext
    ctx.artifacts = stage_ctx.artifacts;

    // Handle result
    let output = result.map_err(|e| match e {
        crate::stage::PipelineError::StageError { diagnostics, .. } if !diagnostics.is_empty() => {
            // Create a SourceContext for the parse error
            let mut source_context = SourceContext::new();
            let content_str = String::from_utf8_lossy(content).to_string();
            source_context.add_file(source_name.to_string(), Some(content_str));
            crate::error::QuartoError::Parse(crate::error::ParseError::new(
                diagnostics,
                source_context,
            ))
        }
        other => crate::error::QuartoError::Other(other.to_string()),
    })?;

    // Extract the rendered output
    let rendered = output.into_rendered_output().ok_or_else(|| {
        crate::error::QuartoError::Other("Pipeline did not produce RenderedOutput".to_string())
    })?;

    // Collect warnings from the pipeline
    let warnings = stage_ctx.warnings;

    // Create source context for the output
    let mut source_context = SourceContext::new();
    let content_str = String::from_utf8_lossy(content).to_string();
    source_context.add_file(source_name.to_string(), Some(content_str));

    Ok(RenderOutput {
        html: rendered.content,
        warnings,
        source_context,
    })
}

/// Build the standard transform pipeline.
///
/// The transforms are applied in this order:
///
/// ## Normalization Phase
/// 1. `CalloutTransform` - Convert callout Divs to CustomNodes
/// 2. `CalloutResolveTransform` - Resolve CustomNodes to structured Divs
/// 3. `MetadataNormalizeTransform` - Add derived metadata (pagetitle, etc.)
/// 4. `TitleBlockTransform` - Add title header from metadata if not present
/// 5. `SectionizeTransform` - Wrap headers in section Divs (for HTML semantic structure)
/// 6. `FootnotesTransform` - Extract footnotes and create footnotes section
///
/// ## TOC Phase
/// 7. `TocGenerateTransform` - Generate TOC from headers (if toc: true)
/// 8. `TocRenderTransform` - Render TOC to HTML for template insertion
///
/// ## Finalization Phase
/// 9. `AppendixStructureTransform` - Consolidate appendix content into container
/// 10. `ResourceCollectorTransform` - Collect image dependencies
pub fn build_transform_pipeline() -> TransformPipeline {
    let mut pipeline = TransformPipeline::new();

    // === NORMALIZATION PHASE ===
    pipeline.push(Box::new(CalloutTransform::new()));
    pipeline.push(Box::new(CalloutResolveTransform::new()));
    pipeline.push(Box::new(MetadataNormalizeTransform::new()));
    pipeline.push(Box::new(TitleBlockTransform::new()));
    pipeline.push(Box::new(SectionizeTransform::new()));
    pipeline.push(Box::new(FootnotesTransform::new()));

    // === TOC PHASE ===
    // Must run after SectionizeTransform so section IDs are available
    pipeline.push(Box::new(TocGenerateTransform::new()));
    pipeline.push(Box::new(TocRenderTransform::new()));

    // === FINALIZATION PHASE ===
    pipeline.push(Box::new(AppendixStructureTransform::new()));
    pipeline.push(Box::new(ResourceCollectorTransform::new()));

    pipeline
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::BinaryDependencies;
    use std::path::PathBuf;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: None,
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/test.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    fn make_test_runtime() -> Arc<dyn quarto_system_runtime::SystemRuntime> {
        Arc::new(quarto_system_runtime::NativeRuntime::new())
    }

    #[test]
    fn test_render_simple_document() {
        let content = b"---\ntitle: Test\n---\n\nHello, world!";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        assert!(output.html.contains("Hello, world!"));
        assert!(output.html.contains("<!DOCTYPE html>"));
        assert!(output.html.contains("<title>Test</title>"));
    }

    #[test]
    fn test_render_with_callout() {
        let content =
            b"---\ntitle: Test\n---\n\n::: {.callout-warning}\n## Watch Out\nBe careful!\n:::";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        // Verify callout was transformed
        assert!(output.html.contains("callout"));
        assert!(output.html.contains("callout-warning"));
        assert!(output.html.contains("Watch Out"));
        assert!(output.html.contains("Be careful!"));
    }

    #[test]
    fn test_render_with_css_paths() {
        let content = b"---\ntitle: Test\n---\n\nContent";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let css_paths = vec!["custom.css".to_string()];
        let config = HtmlRenderConfig::with_css(&css_paths);
        let runtime = make_test_runtime();
        let output = pollster::block_on(render_qmd_to_html(
            content, "test.qmd", &mut ctx, &config, runtime,
        ))
        .unwrap();

        // Custom CSS should be in the output
        assert!(output.html.contains("custom.css"));
    }

    #[test]
    #[ignore = "pampa parser is too forgiving - need to find input that produces parse error"]
    fn test_parse_error_has_structured_diagnostics() {
        // NOTE: This test is ignored because pampa's parser is very forgiving
        // and doesn't produce parse errors for most malformed inputs.
        // The YAML parser panics on malformed YAML instead of returning errors.
        // TODO: Find a way to test parse error propagation
        let content = b"---\ntitle: Test\n---\n\nSome content";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/about.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let runtime = make_test_runtime();
        let result = pollster::block_on(render_qmd_to_html(
            content,
            "about.qmd",
            &mut ctx,
            &config,
            runtime,
        ));

        // Should fail with a parse error
        assert!(result.is_err());

        // The error should be a Parse error with diagnostics
        if let Err(crate::error::QuartoError::Parse(parse_error)) = result {
            // Should have at least one diagnostic
            assert!(
                !parse_error.diagnostics.is_empty(),
                "Parse error should contain diagnostics"
            );
        } else {
            panic!("Expected QuartoError::Parse, got {:?}", result);
        }
    }

    // === Pipeline builder tests ===

    #[test]
    fn test_build_html_pipeline_stages() {
        let stages = build_html_pipeline_stages();
        assert_eq!(stages.len(), 5);
        assert_eq!(stages[0].name(), "parse-document");
        assert_eq!(stages[1].name(), "engine-execution");
        assert_eq!(stages[2].name(), "ast-transforms");
        assert_eq!(stages[3].name(), "render-html-body");
        assert_eq!(stages[4].name(), "apply-template");
    }

    #[test]
    fn test_build_html_pipeline() {
        let pipeline = build_html_pipeline();
        assert_eq!(pipeline.len(), 5);
    }

    #[test]
    fn test_build_wasm_html_pipeline() {
        let pipeline = build_wasm_html_pipeline();
        // WASM pipeline has 4 stages (no engine execution)
        assert_eq!(pipeline.len(), 4);
    }

    #[test]
    fn test_build_html_pipeline_with_stages() {
        use crate::stage::PipelineDataKind;

        let stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(ParseDocumentStage::new()),
            Box::new(AstTransformsStage::new()),
            Box::new(RenderHtmlBodyStage::new()),
        ];

        let result = build_html_pipeline_with_stages(stages);
        assert!(result.is_ok());

        let pipeline = result.unwrap();
        assert_eq!(pipeline.len(), 3);
        assert_eq!(pipeline.expected_input(), PipelineDataKind::LoadedSource);
        assert_eq!(pipeline.expected_output(), PipelineDataKind::RenderedOutput);
    }

    #[test]
    fn test_build_html_pipeline_with_stages_invalid() {
        // Try to create a pipeline with incompatible consecutive stages
        // ParseDocumentStage outputs DocumentAst, but ApplyTemplateStage expects RenderedOutput
        let stages: Vec<Box<dyn PipelineStage>> = vec![
            Box::new(ParseDocumentStage::new()),
            Box::new(ApplyTemplateStage::new()), // Expects RenderedOutput, not DocumentAst
        ];

        let result = build_html_pipeline_with_stages(stages);
        assert!(result.is_err());
    }
}
