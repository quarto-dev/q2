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
//! The actual render logic is in `quarto_core::render_to_file`. This module
//! handles CLI-specific concerns: argument parsing, console output, and
//! special error handling.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;

use quarto_core::{
    Format, ProjectContext, QuartoError, RenderToFileOptions, render_document_to_file,
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
    let format_str = args.to.as_deref().unwrap_or("html");
    let format = resolve_format(format_str)?;

    // Only HTML is supported in MVP
    if !format.identifier.is_native() {
        anyhow::bail!(
            "Format '{}' is not yet supported. Only HTML is available in this version.",
            format.identifier
        );
    }

    // Discover project context (once for all files)
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

    // Set up render options
    let options = RenderToFileOptions {
        output_path: args.output.as_ref().map(PathBuf::from),
        output_dir: args.output_dir.as_ref().map(PathBuf::from),
        quiet: args.quiet,
    };

    // Create Arc runtime for the render function, with cache dir for SASS caching
    let runtime_arc: Arc<dyn quarto_system_runtime::SystemRuntime> = if project.is_single_file {
        Arc::new(NativeRuntime::new())
    } else {
        Arc::new(NativeRuntime::with_cache_dir(
            project.dir.join(".quarto/cache"),
        ))
    };

    // Render each file in the project
    for doc_info in &project.files {
        // Use the shared render function
        let result = match render_document_to_file(
            &doc_info.input,
            format_str,
            &options,
            Some(&project),
            runtime_arc.clone(),
        ) {
            Ok(result) => result,
            Err(QuartoError::Parse(parse_error)) => {
                // Parse errors have rich ariadne formatting with their own "Error:" prefix.
                // Print directly to avoid anyhow adding a duplicate prefix.
                eprintln!("{}", parse_error);
                std::process::exit(1);
            }
            Err(e) => return Err(anyhow::anyhow!("{}", e)),
        };

        // Report diagnostics with full ariadne-style source context
        if !args.quiet && !result.render_output.diagnostics.is_empty() {
            for diagnostic in &result.render_output.diagnostics {
                // Use the source context for rich error rendering with source snippets
                eprintln!(
                    "{}",
                    diagnostic.to_text(Some(&result.render_output.source_context))
                );
            }
        }

        if !args.quiet {
            info!("Output: {}", result.output_path.display());
        }
    }

    Ok(())
}

/// Resolve format string to Format (without metadata)
fn resolve_format(format_str: &str) -> Result<Format> {
    Format::from_format_string(format_str).map_err(|e| anyhow::anyhow!("{}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_core::FormatIdentifier;

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
}
