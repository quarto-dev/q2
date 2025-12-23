/*
 * render.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Render command implementation
 */

//! Render command implementation.
//!
//! This module implements the `quarto render` command, which renders
//! QMD files to various output formats.
//!
//! ## MVP Scope
//!
//! The initial implementation supports:
//! - Single file rendering
//! - HTML output (native Rust pipeline, no Pandoc)
//! - Basic document structure
//!
//! Not yet supported:
//! - Code execution
//! - SASS compilation (uses pre-compiled CSS)
//! - Navigation (navbar, sidebar, footer)
//! - Multi-file projects
//! - Non-HTML formats

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{debug, info};

use quarto_core::{
    BinaryDependencies, CalloutResolveTransform, CalloutTransform, DocumentInfo, Format,
    FormatIdentifier, MetadataNormalizeTransform, ProjectContext, RenderContext, RenderOptions,
    ResourceCollectorTransform, TransformPipeline,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Arguments for the render command
#[derive(Debug)]
pub struct RenderArgs {
    /// Input file or project directory
    pub input: Option<String>,
    /// Output format
    pub to: Option<String>,
    /// Output file path
    pub output: Option<String>,
    /// Output directory
    pub output_dir: Option<String>,
    /// Suppress console output
    pub quiet: bool,
    /// Leave intermediate files (not yet implemented)
    #[allow(dead_code)]
    pub debug: bool,
}

/// Execute the render command
pub fn execute(args: RenderArgs) -> Result<()> {
    // Create the system runtime
    let runtime = NativeRuntime::new();

    // Determine input path
    let input_path = match &args.input {
        Some(input) => PathBuf::from(input),
        None => {
            // Default to current directory
            runtime
                .cwd()
                .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?
        }
    };

    // Validate input exists
    let path_exists = runtime
        .path_exists(&input_path, None)
        .map_err(|e| anyhow::anyhow!("Failed to check input path: {}", e))?;
    if !path_exists {
        anyhow::bail!("Input path does not exist: {}", input_path.display());
    }

    // Determine format
    let format = match &args.to {
        Some(format_str) => resolve_format(format_str)?,
        None => Format::html(), // Default to HTML
    };

    // Only HTML is supported in MVP
    if !format.identifier.is_native() {
        anyhow::bail!(
            "Format '{}' is not yet supported. Only HTML is available in this version.",
            format.identifier
        );
    }

    // Discover project context
    let project = ProjectContext::discover(&input_path, &runtime)
        .context("Failed to discover project context")?;

    if !args.quiet {
        if project.is_single_file {
            info!("Rendering single file: {}", input_path.display());
        } else {
            info!(
                "Rendering project: {} (type: {})",
                project.dir.display(),
                project.project_type().as_str()
            );
        }
    }

    // Set up binary dependencies
    let binaries = BinaryDependencies::discover(&runtime);

    // Render each file
    for doc_info in &project.files {
        render_document(&doc_info, &project, &format, &binaries, &args, &runtime)?;
    }

    Ok(())
}

/// Resolve format string to Format
fn resolve_format(format_str: &str) -> Result<Format> {
    let identifier =
        FormatIdentifier::try_from(format_str).map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(Format {
        identifier,
        output_extension: match identifier {
            FormatIdentifier::Html => "html",
            FormatIdentifier::Pdf => "pdf",
            FormatIdentifier::Docx => "docx",
            FormatIdentifier::Epub => "epub",
            FormatIdentifier::Typst => "pdf",
            FormatIdentifier::Revealjs => "html",
            FormatIdentifier::Gfm => "md",
            FormatIdentifier::CommonMark => "md",
            FormatIdentifier::Custom(_) => "html",
        }
        .to_string(),
        native_pipeline: identifier.is_native(),
        metadata: serde_json::Value::Null,
    })
}

/// Render a single document
fn render_document(
    doc_info: &DocumentInfo,
    project: &ProjectContext,
    format: &Format,
    binaries: &BinaryDependencies,
    args: &RenderArgs,
    runtime: &dyn SystemRuntime,
) -> Result<()> {
    debug!("Rendering: {}", doc_info.input.display());

    // Create render context
    let options = RenderOptions {
        verbose: !args.quiet,
        execute: false, // MVP: no code execution
        use_freeze: false,
        output_path: args.output.as_ref().map(PathBuf::from),
    };

    let mut ctx = RenderContext::new(project, doc_info, format, binaries).with_options(options);

    // Read input file
    let input_content = fs::read(&doc_info.input)
        .with_context(|| format!("Failed to read input file: {}", doc_info.input.display()))?;

    // Parse QMD to Pandoc AST
    let mut output_stream = std::io::sink();
    let input_path_str = doc_info.input.to_string_lossy();

    let (mut pandoc, context, warnings) = pampa::readers::qmd::read(
        &input_content,
        false, // loose mode
        &input_path_str,
        &mut output_stream,
        true, // track source locations
        None, // file_id
    )
    .map_err(|diagnostics| {
        // Format error messages
        let error_text = diagnostics
            .iter()
            .map(|d| d.to_text(None))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::anyhow!("Parse errors:\n{}", error_text)
    })?;

    // Report warnings
    if !args.quiet && !warnings.is_empty() {
        for warning in &warnings {
            eprintln!("Warning: {}", warning.to_text(None));
        }
    }

    // Build and execute transform pipeline
    let pipeline = build_transform_pipeline();
    pipeline
        .execute(&mut pandoc, &mut ctx)
        .map_err(|e| anyhow::anyhow!("Transform error: {}", e))?;

    // Determine output path
    let output_path = determine_output_path(&ctx, args)?;

    // Create output directory if needed
    let output_dir = output_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Could not determine output directory"))?;
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "Failed to create output directory: {}",
            output_dir.display()
        )
    })?;

    // Get the output stem for resource directory naming
    let output_stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("Could not determine output filename stem"))?;

    // Write static resources (CSS, JS) to output directory
    let resource_paths =
        quarto_core::resources::write_html_resources(output_dir, output_stem, runtime)
            .context("Failed to write HTML resources")?;

    // Render to HTML with resource paths
    let html_output = render_to_html(&pandoc, &context, &resource_paths.css)?;

    // Write output
    let mut output_file = fs::File::create(&output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;

    output_file
        .write_all(html_output.as_bytes())
        .with_context(|| format!("Failed to write output file: {}", output_path.display()))?;

    if !args.quiet {
        info!("Output: {}", output_path.display());
    }

    Ok(())
}

/// Build the transform pipeline for HTML rendering.
fn build_transform_pipeline() -> TransformPipeline {
    let mut pipeline = TransformPipeline::new();

    // Add transforms in order:
    // 1. CalloutTransform - convert callout Divs to CustomNodes
    // 2. CalloutResolveTransform - resolve Callout CustomNodes to standard Div structure
    // 3. MetadataNormalizeTransform - normalize metadata (pagetitle, etc.)
    // 4. ResourceCollectorTransform - collect image dependencies
    pipeline.push(Box::new(CalloutTransform::new()));
    pipeline.push(Box::new(CalloutResolveTransform::new()));
    pipeline.push(Box::new(MetadataNormalizeTransform::new()));
    pipeline.push(Box::new(ResourceCollectorTransform::new()));

    pipeline
}

/// Determine the output path for a render
fn determine_output_path(ctx: &RenderContext, args: &RenderArgs) -> Result<PathBuf> {
    // Priority: --output > --output-dir > format default
    if let Some(output) = &args.output {
        return Ok(PathBuf::from(output));
    }

    let base_output = ctx.output_path();

    if let Some(output_dir) = &args.output_dir {
        // Use output-dir with the filename from the input
        let filename = base_output
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Could not determine output filename"))?;
        return Ok(PathBuf::from(output_dir).join(filename));
    }

    Ok(base_output)
}

/// Render Pandoc AST to HTML string with external resources
fn render_to_html(
    pandoc: &pampa::pandoc::Pandoc,
    ast_context: &pampa::pandoc::ASTContext,
    css_paths: &[String],
) -> Result<String> {
    // First, render the body content using pampa's HTML writer
    // This is metadata-aware and will include source location attributes
    // if format.html.source-location: full is set in the document
    let mut body_buf = Vec::new();
    pampa::writers::html::write(pandoc, ast_context, &mut body_buf)
        .context("Failed to write HTML body")?;
    let body = String::from_utf8_lossy(&body_buf).into_owned();

    // Then wrap it with the template, passing metadata and resource paths
    quarto_core::template::render_with_resources(&body, &pandoc.meta, css_paths)
        .context("Failed to render template")
}

/// Escape HTML special characters
#[cfg(test)]
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_format_html() {
        let format = resolve_format("html").unwrap();
        assert_eq!(format.identifier, FormatIdentifier::Html);
        assert_eq!(format.output_extension, "html");
        assert!(format.native_pipeline);
    }

    #[test]
    fn test_resolve_format_pdf() {
        let format = resolve_format("pdf").unwrap();
        assert_eq!(format.identifier, FormatIdentifier::Pdf);
        assert_eq!(format.output_extension, "pdf");
        assert!(!format.native_pipeline);
    }

    #[test]
    fn test_resolve_format_unknown() {
        let result = resolve_format("unknown");
        assert!(result.is_err());
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("Hello & World"), "Hello &amp; World");
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
    }
}
