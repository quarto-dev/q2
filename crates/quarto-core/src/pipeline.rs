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
//! 2. **Transform**: Apply Quarto-specific transforms (callouts, metadata, etc.)
//! 3. **Render body**: Pandoc AST → HTML body (via `pampa`)
//! 4. **Apply template**: Wrap body with HTML template
//!
//! ## Usage
//!
//! ```ignore
//! use quarto_core::pipeline::{render_qmd_to_html, HtmlRenderConfig};
//!
//! let output = render_qmd_to_html(
//!     content.as_bytes(),
//!     "input.qmd",
//!     &mut render_ctx,
//!     &HtmlRenderConfig::default(),
//! )?;
//! ```

use std::path::PathBuf;

use quarto_doctemplate::Template;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::artifact::Artifact;
use crate::render::RenderContext;
use crate::resources::DEFAULT_CSS;
use crate::transform::TransformPipeline;
use crate::transforms::{
    CalloutResolveTransform, CalloutTransform, MetadataNormalizeTransform,
    ResourceCollectorTransform, TitleBlockTransform,
};
use crate::Result;

/// Well-known path for the default CSS artifact in WASM context.
///
/// This path is used by both the render pipeline (to store the artifact)
/// and the browser post-processor (to resolve the CSS reference).
pub const DEFAULT_CSS_ARTIFACT_PATH: &str = "/.quarto/project-artifacts/styles.css";

/// Configuration for HTML rendering.
#[derive(Debug, Default)]
pub struct HtmlRenderConfig<'a> {
    /// CSS paths to include in the document (relative to the output HTML).
    ///
    /// For CLI: These are paths to CSS files written to disk.
    /// For WASM: These might be empty, inline, or pointing to bundled resources.
    pub css_paths: &'a [String],

    /// Custom template to use instead of the built-in default.
    ///
    /// If `None`, uses the built-in HTML5 template.
    pub template: Option<&'a Template>,
}

/// Output from the render pipeline.
#[derive(Debug)]
pub struct RenderOutput {
    /// The rendered HTML document.
    pub html: String,

    /// Warnings generated during parsing.
    ///
    /// Note: Collected artifacts (e.g., image dependencies) are stored in
    /// `ctx.artifacts` and can be accessed after rendering completes.
    pub warnings: Vec<ParseWarning>,
}

/// A warning from the parsing stage.
#[derive(Debug, Clone)]
pub struct ParseWarning {
    /// The warning message.
    pub message: String,
}

impl ParseWarning {
    /// Create a new parse warning.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Render QMD content to HTML.
///
/// This is the unified render pipeline used by both CLI and WASM. It:
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
///
/// # Returns
///
/// A `RenderOutput` containing the HTML and any collected artifacts.
///
/// # Errors
///
/// Returns an error if parsing fails, transforms fail, or rendering fails.
pub fn render_qmd_to_html(
    content: &[u8],
    source_name: &str,
    ctx: &mut RenderContext,
    config: &HtmlRenderConfig,
) -> Result<RenderOutput> {
    // Stage 1: Parse QMD to Pandoc AST
    let (mut pandoc, ast_context, parse_warnings) = parse_qmd(content, source_name)?;

    // Stage 2: Run transform pipeline
    let pipeline = build_transform_pipeline();
    pipeline.execute(&mut pandoc, ctx)?;

    // Stage 3: Render body HTML
    let body = render_body_html(&pandoc, &ast_context)?;

    // Stage 4: Store CSS artifact for WASM consumption
    // The CSS is stored at a well-known path that the browser post-processor can resolve.
    ctx.artifacts.store(
        "css:default",
        Artifact::from_string(DEFAULT_CSS, "text/css")
            .with_path(PathBuf::from(DEFAULT_CSS_ARTIFACT_PATH)),
    );

    // Stage 5: Apply template
    // When no explicit CSS paths are provided and using default template,
    // include the default CSS artifact path.
    let html = apply_template(&body, &pandoc, config, ctx)?;

    // Collect results
    let warnings = parse_warnings
        .into_iter()
        .map(|w| ParseWarning::new(w.to_text(None)))
        .collect();

    Ok(RenderOutput { html, warnings })
}

/// Parse QMD content to Pandoc AST.
fn parse_qmd(
    content: &[u8],
    source_name: &str,
) -> Result<(
    Pandoc,
    pampa::pandoc::ASTContext,
    Vec<quarto_error_reporting::DiagnosticMessage>,
)> {
    let mut output_stream = std::io::sink();

    pampa::readers::qmd::read(
        content,
        false,       // loose mode
        source_name, // filename for error messages
        &mut output_stream,
        true, // track source locations
        None, // file_id
    )
    .map_err(|diagnostics| {
        let error_text = diagnostics
            .iter()
            .map(|d| d.to_text(None))
            .collect::<Vec<_>>()
            .join("\n");
        crate::error::QuartoError::Parse(error_text)
    })
}

/// Build the standard transform pipeline.
///
/// The transforms are applied in this order:
/// 1. `CalloutTransform` - Convert callout Divs to CustomNodes
/// 2. `CalloutResolveTransform` - Resolve CustomNodes to structured Divs
/// 3. `MetadataNormalizeTransform` - Add derived metadata (pagetitle, etc.)
/// 4. `TitleBlockTransform` - Add title header from metadata if not present
/// 5. `ResourceCollectorTransform` - Collect image dependencies
pub fn build_transform_pipeline() -> TransformPipeline {
    let mut pipeline = TransformPipeline::new();

    pipeline.push(Box::new(CalloutTransform::new()));
    pipeline.push(Box::new(CalloutResolveTransform::new()));
    pipeline.push(Box::new(MetadataNormalizeTransform::new()));
    pipeline.push(Box::new(TitleBlockTransform::new()));
    pipeline.push(Box::new(ResourceCollectorTransform::new()));

    pipeline
}

/// Render Pandoc AST to HTML body string.
fn render_body_html(pandoc: &Pandoc, ast_context: &pampa::pandoc::ASTContext) -> Result<String> {
    let mut body_buf = Vec::new();
    pampa::writers::html::write(pandoc, ast_context, &mut body_buf).map_err(|e| {
        crate::error::QuartoError::Render(format!("Failed to write HTML body: {}", e))
    })?;

    String::from_utf8(body_buf)
        .map_err(|e| crate::error::QuartoError::Render(format!("Invalid UTF-8 in HTML body: {}", e)))
}

/// Apply template to rendered body.
///
/// When using the default template and no explicit CSS paths are provided,
/// this function automatically includes the default CSS artifact path.
fn apply_template(
    body: &str,
    pandoc: &Pandoc,
    config: &HtmlRenderConfig,
    _ctx: &RenderContext,
) -> Result<String> {
    match config.template {
        Some(template) => {
            crate::template::render_with_custom_template(template, body, &pandoc.meta)
        }
        None => {
            // When no CSS paths are provided, use the default CSS artifact path.
            // This ensures WASM renders get the default styling.
            let css_paths: Vec<String> = if config.css_paths.is_empty() {
                vec![DEFAULT_CSS_ARTIFACT_PATH.to_string()]
            } else {
                config.css_paths.to_vec()
            };
            crate::template::render_with_resources(body, &pandoc.meta, &css_paths)
        }
    }
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

    #[test]
    fn test_render_simple_document() {
        let content = b"---\ntitle: Test\n---\n\nHello, world!";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let output = render_qmd_to_html(content, "test.qmd", &mut ctx, &config).unwrap();

        assert!(output.html.contains("Hello, world!"));
        assert!(output.html.contains("<!DOCTYPE html>"));
        assert!(output.html.contains("<title>Test</title>"));
    }

    #[test]
    fn test_render_with_callout() {
        let content = b"---\ntitle: Test\n---\n\n::: {.callout-warning}\n## Watch Out\nBe careful!\n:::";

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let config = HtmlRenderConfig::default();
        let output = render_qmd_to_html(content, "test.qmd", &mut ctx, &config).unwrap();

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

        let css_paths = vec!["styles/main.css".to_string()];
        let config = HtmlRenderConfig {
            css_paths: &css_paths,
            template: None,
        };
        let output = render_qmd_to_html(content, "test.qmd", &mut ctx, &config).unwrap();

        assert!(output.html.contains(r#"href="styles/main.css""#));
    }

    #[test]
    fn test_build_transform_pipeline() {
        let pipeline = build_transform_pipeline();
        // The pipeline should have 5 transforms
        assert_eq!(pipeline.len(), 5);
    }
}
