/*
 * template/render.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Template rendering orchestration.
//!
//! This module provides high-level functions for rendering Pandoc documents
//! using templates.

use crate::pandoc::Pandoc;
use crate::pandoc::ast_context::ASTContext;
use crate::template::bundle::{BundleError, TemplateBundle};
use crate::template::config_merge::merged_metadata_to_context;
use crate::template::context::MetaWriter;
use crate::writers::{html, plaintext};
use quarto_doctemplate::{PartialResolver, Template, TemplateError};
use quarto_error_reporting::DiagnosticMessage;
use std::path::Path;

/// Error type for template rendering.
#[derive(Debug)]
pub enum TemplateRenderError {
    /// Bundle parsing or compilation failed.
    Bundle(BundleError),
    /// Template evaluation failed.
    Template(TemplateError),
    /// Body rendering failed.
    BodyRender(String),
}

impl std::fmt::Display for TemplateRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateRenderError::Bundle(e) => write!(f, "bundle error: {}", e),
            TemplateRenderError::Template(e) => write!(f, "template error: {}", e),
            TemplateRenderError::BodyRender(e) => write!(f, "body render error: {}", e),
        }
    }
}

impl std::error::Error for TemplateRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TemplateRenderError::Bundle(e) => Some(e),
            TemplateRenderError::Template(e) => Some(e),
            TemplateRenderError::BodyRender(_) => None,
        }
    }
}

impl From<BundleError> for TemplateRenderError {
    fn from(e: BundleError) -> Self {
        TemplateRenderError::Bundle(e)
    }
}

impl From<TemplateError> for TemplateRenderError {
    fn from(e: TemplateError) -> Self {
        TemplateRenderError::Template(e)
    }
}

/// Output format for body rendering.
#[derive(Debug, Clone, Copy, Default)]
pub enum BodyFormat {
    /// Render body as HTML.
    #[default]
    Html,
    /// Render body as plain text.
    Plaintext,
}

impl BodyFormat {
    /// Get the corresponding MetaWriter for this body format.
    ///
    /// This ensures consistency: if body is HTML, metadata inlines are also HTML.
    pub fn meta_writer(&self) -> MetaWriter {
        match self {
            BodyFormat::Html => MetaWriter::Html,
            BodyFormat::Plaintext => MetaWriter::Plaintext,
        }
    }
}

/// Render the document body to a string.
fn render_body(
    pandoc: &Pandoc,
    _context: &ASTContext,
    format: BodyFormat,
) -> Result<(String, Vec<DiagnosticMessage>), TemplateRenderError> {
    let mut buf = Vec::new();
    let diagnostics;

    match format {
        BodyFormat::Html => {
            html::write_blocks(&pandoc.blocks, &mut buf)
                .map_err(|e| TemplateRenderError::BodyRender(e.to_string()))?;
            diagnostics = vec![];
        }
        BodyFormat::Plaintext => {
            let (text, diags) = plaintext::blocks_to_string(&pandoc.blocks);
            buf = text.into_bytes();
            diagnostics = diags;
        }
    }

    let body = String::from_utf8_lossy(&buf).into_owned();
    Ok((body, diagnostics))
}

/// Render a Pandoc document using a template bundle.
///
/// This is the primary entry point for template rendering. It:
/// 1. Compiles the template bundle
/// 2. Renders the document body using the specified format
/// 3. Converts metadata to template values
/// 4. Evaluates the template with the context
///
/// # Arguments
///
/// * `pandoc` - The parsed Pandoc document
/// * `context` - The AST context (for source tracking)
/// * `bundle` - The template bundle
/// * `body_format` - The format to use for body and metadata rendering
///
/// # Returns
///
/// The rendered output string, or an error.
///
/// # Example
///
/// ```ignore
/// let bundle = TemplateBundle::from_json(r#"{"version": "1.0.0", "main": "<html>$body$</html>"}"#)?;
/// let output = render_with_bundle(&pandoc, &mut context, &bundle, "my-template.html", BodyFormat::Html)?;
/// ```
pub fn render_with_bundle(
    pandoc: &Pandoc,
    context: &mut ASTContext,
    bundle: &TemplateBundle,
    template_name: &str,
    body_format: BodyFormat,
) -> Result<(String, Vec<DiagnosticMessage>), TemplateRenderError> {
    // Compile template using the shared source context.
    // This ensures template file IDs are unique within the same context as the main document,
    // allowing diagnostics from templates to be correctly attributed to their source files.
    let template = bundle.compile_with_context(template_name, &mut context.source_context)?;
    let resolver = bundle.to_resolver();
    render_with_compiled_template(pandoc, context, &template, &resolver, body_format)
}

/// Render a Pandoc document using a compiled template and custom resolver.
///
/// This is a lower-level entry point for advanced use cases where you want
/// to control template compilation and partial resolution separately.
///
/// # Arguments
///
/// * `pandoc` - The parsed Pandoc document
/// * `context` - The AST context
/// * `template` - A pre-compiled template
/// * `resolver` - The partial resolver to use
/// * `body_format` - The format to use for body and metadata rendering
pub fn render_with_compiled_template<R: PartialResolver>(
    pandoc: &Pandoc,
    context: &ASTContext,
    template: &Template,
    _resolver: &R,
    body_format: BodyFormat,
) -> Result<(String, Vec<DiagnosticMessage>), TemplateRenderError> {
    let mut all_diagnostics = Vec::new();

    // Render the body
    let (body, body_diags) = render_body(pandoc, context, body_format)?;
    all_diagnostics.extend(body_diags);

    // Convert metadata to template context using the merged config system.
    // This merges template defaults (lang, pagetitle) with document metadata.
    let meta_writer = body_format.meta_writer();
    let (template_ctx, meta_diags) = merged_metadata_to_context(&pandoc.meta, body, meta_writer);
    all_diagnostics.extend(meta_diags);

    // Render the template
    let (result, template_diags) = template.render_with_diagnostics(&template_ctx);

    // Template diagnostics are already DiagnosticMessage from quarto-error-reporting
    all_diagnostics.extend(template_diags);

    // Handle the result - the error type is () so we need to construct a meaningful error
    let output = result.map_err(|()| {
        TemplateRenderError::Template(TemplateError::EvaluationError {
            message: "Template evaluation failed (see diagnostics for details)".to_string(),
        })
    })?;
    Ok((output, all_diagnostics))
}

/// Render a Pandoc document using a template source and custom resolver.
///
/// This compiles the template on-the-fly and renders with the given resolver.
/// Use this when you have a template source string and want to control
/// partial resolution (e.g., using `MemoryResolver` or `FileSystemResolver`).
///
/// # Arguments
///
/// * `pandoc` - The parsed Pandoc document
/// * `context` - The AST context
/// * `template_source` - The template source string
/// * `template_path` - Path for the template (used for partial resolution and error messages)
/// * `resolver` - The partial resolver to use
/// * `body_format` - The format to use for body and metadata rendering
pub fn render_with_resolver<R: PartialResolver>(
    pandoc: &Pandoc,
    context: &ASTContext,
    template_source: &str,
    template_path: &Path,
    resolver: &R,
    body_format: BodyFormat,
) -> Result<(String, Vec<DiagnosticMessage>), TemplateRenderError> {
    let template = Template::compile_with_resolver(template_source, template_path, resolver, 0)?;
    render_with_compiled_template(pandoc, context, &template, resolver, body_format)
}

/// Render a Pandoc document using a template file from the filesystem.
///
/// This is a convenience function that loads a template from a file and
/// uses `FileSystemResolver` for partial resolution.
///
/// # Arguments
///
/// * `pandoc` - The parsed Pandoc document
/// * `context` - The AST context
/// * `template_path` - Path to the template file
/// * `body_format` - The format to use for body and metadata rendering
///
/// # Feature
///
/// This function requires the `template-fs` feature.
#[cfg(feature = "template-fs")]
pub fn render_with_template_file(
    pandoc: &Pandoc,
    context: &ASTContext,
    template_path: &Path,
    body_format: BodyFormat,
) -> Result<(String, Vec<DiagnosticMessage>), TemplateRenderError> {
    use quarto_doctemplate::FileSystemResolver;

    let template = Template::compile_from_file(template_path)?;
    let resolver = FileSystemResolver;
    render_with_compiled_template(pandoc, context, &template, &resolver, body_format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pandoc::block::{Block, Paragraph};
    use crate::pandoc::inline::{Inline, Space, Str};
    use quarto_pandoc_types::meta::{MetaMapEntry, MetaValueWithSourceInfo};
    use quarto_source_map::SourceInfo;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::default()
    }

    fn make_simple_pandoc() -> (Pandoc, ASTContext) {
        let meta = MetaValueWithSourceInfo::MetaMap {
            entries: vec![MetaMapEntry {
                key: "title".to_string(),
                key_source: dummy_source_info(),
                value: MetaValueWithSourceInfo::MetaString {
                    value: "Test Title".to_string(),
                    source_info: dummy_source_info(),
                },
            }],
            source_info: dummy_source_info(),
        };

        let blocks = vec![Block::Paragraph(Paragraph {
            content: vec![
                Inline::Str(Str {
                    text: "Hello".to_string(),
                    source_info: dummy_source_info(),
                }),
                Inline::Space(Space {
                    source_info: dummy_source_info(),
                }),
                Inline::Str(Str {
                    text: "World".to_string(),
                    source_info: dummy_source_info(),
                }),
            ],
            source_info: dummy_source_info(),
        })];

        let pandoc = Pandoc { meta, blocks };
        let context = ASTContext::default();

        (pandoc, context)
    }

    #[test]
    fn test_render_with_bundle_simple() {
        let (pandoc, mut context) = make_simple_pandoc();
        let bundle = TemplateBundle::new(
            "<html><head><title>$title$</title></head><body>$body$</body></html>",
        );

        let (output, diags) = render_with_bundle(
            &pandoc,
            &mut context,
            &bundle,
            "test.html",
            BodyFormat::Html,
        )
        .unwrap();

        assert!(diags.is_empty());
        assert!(output.contains("<title>Test Title</title>"));
        assert!(output.contains("Hello"));
        assert!(output.contains("World"));
    }

    #[test]
    fn test_render_with_bundle_plaintext() {
        let (pandoc, mut context) = make_simple_pandoc();
        let bundle = TemplateBundle::new("Title: $title$\n\n$body$");

        let (output, _diags) = render_with_bundle(
            &pandoc,
            &mut context,
            &bundle,
            "test.txt",
            BodyFormat::Plaintext,
        )
        .unwrap();

        assert!(output.contains("Title: Test Title"));
        assert!(output.contains("Hello"));
        assert!(output.contains("World"));
    }

    #[test]
    fn test_render_with_bundle_partials() {
        let (pandoc, mut context) = make_simple_pandoc();
        let bundle = TemplateBundle::new("$header()$\n$body$\n$footer()$")
            .with_partial("header", "<header>$title$</header>")
            .with_partial("footer", "<footer>End</footer>");

        let (output, diags) = render_with_bundle(
            &pandoc,
            &mut context,
            &bundle,
            "test.html",
            BodyFormat::Html,
        )
        .unwrap();

        assert!(diags.is_empty());
        assert!(output.contains("<header>Test Title</header>"));
        assert!(output.contains("<footer>End</footer>"));
    }

    #[test]
    fn test_render_with_bundle_conditional() {
        let (pandoc, mut context) = make_simple_pandoc();
        let bundle = TemplateBundle::new("$if(title)$Has title: $title$$endif$");

        let (output, _diags) = render_with_bundle(
            &pandoc,
            &mut context,
            &bundle,
            "test.html",
            BodyFormat::Html,
        )
        .unwrap();

        assert!(output.contains("Has title: Test Title"));
    }

    #[test]
    fn test_render_with_resolver() {
        let (pandoc, context) = make_simple_pandoc();
        let template_source = "Title: $title$";
        let resolver = quarto_doctemplate::NullResolver;

        let (output, _diags) = render_with_resolver(
            &pandoc,
            &context,
            template_source,
            Path::new("test.html"),
            &resolver,
            BodyFormat::Html,
        )
        .unwrap();

        assert!(output.contains("Title: Test Title"));
    }

    #[test]
    fn test_body_format_meta_writer_consistency() {
        // HTML body format should use HTML meta writer
        assert!(matches!(BodyFormat::Html.meta_writer(), MetaWriter::Html));

        // Plaintext body format should use plaintext meta writer
        assert!(matches!(
            BodyFormat::Plaintext.meta_writer(),
            MetaWriter::Plaintext
        ));
    }
}
