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

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{debug, info};

use quarto_core::{
    BinaryDependencies, DocumentInfo, Format, FormatIdentifier, HtmlRenderConfig, ProjectContext,
    QuartoError, RenderContext, RenderOptions, render_qmd_to_html,
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
        render_document(doc_info, &project, &format, &binaries, &args, &runtime)?;
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

    // Determine output path (needed before rendering for CSS resource paths)
    let output_path = determine_output_path(&ctx, args)?;

    // Create output directory if needed
    let output_dir = output_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Could not determine output directory"))?;
    runtime.dir_create(output_dir, true).map_err(|e| {
        anyhow::anyhow!(
            "Failed to create output directory {}: {}",
            output_dir.display(),
            e
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

    // Read input file
    let input_content = runtime.file_read(&doc_info.input).map_err(|e| {
        anyhow::anyhow!(
            "Failed to read input file {}: {}",
            doc_info.input.display(),
            e
        )
    })?;

    // Use the unified pipeline to render
    let input_path_str = doc_info.input.to_string_lossy();
    let config = HtmlRenderConfig {
        css_paths: &resource_paths.css,
        template: None,
    };

    let output = match render_qmd_to_html(&input_content, &input_path_str, &mut ctx, &config) {
        Ok(output) => output,
        Err(QuartoError::Parse(parse_error)) => {
            // Parse errors have rich ariadne formatting with their own "Error:" prefix.
            // Print directly to avoid anyhow adding a duplicate prefix.
            eprintln!("{}", parse_error);
            std::process::exit(1);
        }
        Err(e) => return Err(anyhow::anyhow!("{}", e)),
    };

    // Report warnings with full ariadne-style source context
    if !args.quiet && !output.warnings.is_empty() {
        for warning in &output.warnings {
            // Use the source context for rich error rendering with source snippets
            eprintln!("{}", warning.to_text(Some(&output.source_context)));
        }
    }

    // Write output
    runtime
        .file_write(&output_path, output.html.as_bytes())
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to write output file {}: {}",
                output_path.display(),
                e
            )
        })?;

    if !args.quiet {
        info!("Output: {}", output_path.display());
    }

    Ok(())
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
