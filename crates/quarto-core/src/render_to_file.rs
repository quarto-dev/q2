/*
 * render_to_file.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * High-level render orchestration that writes output to files.
 */

//! High-level render-to-file orchestration.
//!
//! This module provides the complete render pipeline that:
//! 1. Reads input document
//! 2. Creates output directory and resources (CSS, etc.)
//! 3. Runs the render pipeline
//! 4. Writes output files
//!
//! This is the function that both the CLI and test infrastructure use,
//! ensuring consistent behavior across all render paths.
//!
//! # Simple Usage
//!
//! For simple cases (single file, auto-discover project):
//!
//! ```ignore
//! use quarto_core::render_to_file::{render_to_file, RenderToFileOptions};
//! use quarto_system_runtime::NativeRuntime;
//! use std::sync::Arc;
//!
//! let runtime = Arc::new(NativeRuntime::new());
//! let options = RenderToFileOptions::default();
//!
//! let result = render_to_file(
//!     Path::new("document.qmd"),
//!     "html",
//!     &options,
//!     runtime,
//! )?;
//! ```
//!
//! # Advanced Usage (CLI)
//!
//! For multi-file projects where you want to discover once and render many:
//!
//! ```ignore
//! use quarto_core::render_to_file::{render_document_to_file, RenderToFileOptions};
//! use quarto_core::project::ProjectContext;
//!
//! // Discover project once
//! let project = ProjectContext::discover(&input_path, &runtime)?;
//!
//! // Render each document
//! for doc in &project.files {
//!     let result = render_document_to_file(
//!         &doc.input,
//!         "html",
//!         &options,
//!         Some(&project),
//!         runtime.clone(),
//!     )?;
//! }
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::debug;

use quarto_system_runtime::SystemRuntime;

use crate::Result;
use crate::error::QuartoError;
use crate::format::Format;
use crate::pipeline::{HtmlRenderConfig, RenderOutput, render_qmd_to_html};
use crate::project::{DocumentInfo, ProjectContext};
use crate::render::{BinaryDependencies, RenderContext};
use crate::resources;

/// Options for rendering a document to a file.
#[derive(Debug, Clone, Default)]
pub struct RenderToFileOptions {
    /// Explicit output path. If None, derived from input path.
    pub output_path: Option<PathBuf>,
    /// Explicit output directory. If None, same directory as input.
    pub output_dir: Option<PathBuf>,
    /// Suppress informational messages (logging).
    pub quiet: bool,
}

/// Result of rendering a document to a file.
#[derive(Debug)]
pub struct RenderToFileResult {
    /// Path to the output file.
    pub output_path: PathBuf,
    /// Path to the resources directory (e.g., `document_files/`).
    pub resources_dir: PathBuf,
    /// The full render output including HTML and diagnostics.
    pub render_output: RenderOutput,
}

/// Render a QMD document to a file (simple API).
///
/// This is the simplest entry point for rendering documents. It automatically
/// discovers the project context and handles all setup.
///
/// For multi-file projects where you want to discover once and render many files,
/// use [`render_document_to_file`] instead.
///
/// # Arguments
///
/// * `input_path` - Path to the input QMD file
/// * `format` - Output format name (e.g., "html")
/// * `options` - Render options
/// * `runtime` - System runtime for file operations
///
/// # Returns
///
/// Returns the render result with output path and diagnostics.
#[cfg(not(target_arch = "wasm32"))]
pub fn render_to_file(
    input_path: &Path,
    format: &str,
    options: &RenderToFileOptions,
    runtime: Arc<dyn SystemRuntime>,
) -> Result<RenderToFileResult> {
    render_document_to_file(input_path, format, options, None, runtime)
}

/// Render a QMD document to a file (advanced API).
///
/// This function accepts an optional pre-discovered `ProjectContext`, which is
/// useful when rendering multiple files in a project - you discover once and
/// render many times without re-discovering for each file.
///
/// # Arguments
///
/// * `input_path` - Path to the input QMD file
/// * `format` - Output format name (e.g., "html")
/// * `options` - Render options
/// * `project` - Optional pre-discovered project context. If None, discovers automatically.
/// * `runtime` - System runtime for file operations
///
/// # Returns
///
/// Returns the render result with output path and diagnostics.
///
/// # Errors
///
/// Returns an error if:
/// - The input file cannot be read
/// - Project discovery fails (when project is None)
/// - Resource writing fails
/// - The render pipeline fails
/// - The output file cannot be written
#[cfg(not(target_arch = "wasm32"))]
pub fn render_document_to_file(
    input_path: &Path,
    format: &str,
    options: &RenderToFileOptions,
    project: Option<&ProjectContext>,
    runtime: Arc<dyn SystemRuntime>,
) -> Result<RenderToFileResult> {
    debug!("Rendering: {}", input_path.display());

    // Read input file
    let input_bytes = runtime.file_read(input_path).map_err(|e| {
        QuartoError::other(format!(
            "Failed to read input file {}: {}",
            input_path.display(),
            e
        ))
    })?;

    // Use provided project or discover
    let discovered_project;
    let project = match project {
        Some(p) => p,
        None => {
            discovered_project = ProjectContext::discover(input_path, runtime.as_ref())?;
            &discovered_project
        }
    };

    // Determine output paths
    let (output_path, output_dir, output_stem) =
        determine_output_paths(input_path, format, options)?;

    // Create output directory
    runtime.dir_create(&output_dir, true).map_err(|e| {
        QuartoError::other(format!(
            "Failed to create output directory {}: {}",
            output_dir.display(),
            e
        ))
    })?;

    // Prepare resource directory (creates {stem}_files/ but does not write CSS)
    let resource_paths =
        resources::prepare_html_resources(&output_dir, &output_stem, runtime.as_ref())?;

    // Set up render context
    let doc_info = DocumentInfo::from_path(input_path);
    let render_format = format_from_name(format);
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(project, &doc_info, &render_format, &binaries);

    // Configure the pipeline with CSS paths
    let config = HtmlRenderConfig {
        css_paths: &resource_paths.css,
        template: None,
    };

    // Run the render pipeline
    let render_output = pollster::block_on(render_qmd_to_html(
        &input_bytes,
        &input_path.to_string_lossy(),
        &mut ctx,
        &config,
        runtime.clone(),
    ))?;

    // Write CSS from pipeline artifact (CompileThemeCssStage always produces this)
    let css_content = ctx
        .artifacts
        .get("css:default")
        .and_then(|a| a.as_str())
        .unwrap_or(resources::DEFAULT_CSS);
    let css_path = resource_paths.resource_dir.join("styles.css");
    runtime
        .file_write(&css_path, css_content.as_bytes())
        .map_err(|e| {
            QuartoError::other(format!(
                "Failed to write CSS to {}: {}",
                css_path.display(),
                e
            ))
        })?;

    // Write output HTML
    runtime
        .file_write(&output_path, render_output.html.as_bytes())
        .map_err(|e| {
            QuartoError::other(format!(
                "Failed to write output file {}: {}",
                output_path.display(),
                e
            ))
        })?;

    debug!("Output: {}", output_path.display());

    Ok(RenderToFileResult {
        output_path,
        resources_dir: resource_paths.resource_dir,
        render_output,
    })
}

/// Determine output paths from input path and options.
fn determine_output_paths(
    input_path: &Path,
    format: &str,
    options: &RenderToFileOptions,
) -> Result<(PathBuf, PathBuf, String)> {
    // Determine file extension
    let extension = match format {
        "html" => "html",
        "pdf" => "pdf",
        "docx" => "docx",
        "latex" | "tex" => "tex",
        "typst" => "typ",
        _ => "html",
    };

    // Get input stem
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| QuartoError::other("Could not determine input filename stem"))?
        .to_string();

    // Determine output path
    let output_path = if let Some(ref explicit_path) = options.output_path {
        explicit_path.clone()
    } else {
        let base_dir = options
            .output_dir
            .clone()
            .or_else(|| input_path.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        base_dir.join(format!("{}.{}", stem, extension))
    };

    // Determine output directory
    let output_dir = output_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    // Get output stem (may differ from input stem if explicit output path given)
    let output_stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&stem)
        .to_string();

    Ok((output_path, output_dir, output_stem))
}

/// Convert a format name to a Format instance.
fn format_from_name(name: &str) -> Format {
    match name {
        "html" => Format::html(),
        "pdf" => Format::pdf(),
        "docx" => Format::docx(),
        _ => Format::html(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_determine_output_paths_default() {
        let input = Path::new("/project/doc.qmd");
        let options = RenderToFileOptions::default();

        let (output, dir, stem) = determine_output_paths(input, "html", &options).unwrap();

        assert_eq!(output, PathBuf::from("/project/doc.html"));
        assert_eq!(dir, PathBuf::from("/project"));
        assert_eq!(stem, "doc");
    }

    #[test]
    fn test_determine_output_paths_explicit_output() {
        let input = Path::new("/project/doc.qmd");
        let options = RenderToFileOptions {
            output_path: Some(PathBuf::from("/out/custom.html")),
            ..Default::default()
        };

        let (output, dir, stem) = determine_output_paths(input, "html", &options).unwrap();

        assert_eq!(output, PathBuf::from("/out/custom.html"));
        assert_eq!(dir, PathBuf::from("/out"));
        assert_eq!(stem, "custom");
    }

    #[test]
    fn test_determine_output_paths_output_dir() {
        let input = Path::new("/project/doc.qmd");
        let options = RenderToFileOptions {
            output_dir: Some(PathBuf::from("/out")),
            ..Default::default()
        };

        let (output, dir, stem) = determine_output_paths(input, "html", &options).unwrap();

        assert_eq!(output, PathBuf::from("/out/doc.html"));
        assert_eq!(dir, PathBuf::from("/out"));
        assert_eq!(stem, "doc");
    }

    #[test]
    fn test_render_to_file_creates_output() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("test.qmd");

        // Create a minimal QMD file
        fs::write(
            &input_path,
            r#"---
title: Test
---

Hello world.
"#,
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let options = RenderToFileOptions::default();

        let result = render_to_file(&input_path, "html", &options, runtime).unwrap();

        // Check output file was created
        assert!(result.output_path.exists());
        assert!(result.output_path.ends_with("test.html"));

        // Check resources directory was created
        assert!(result.resources_dir.exists());
        assert!(result.resources_dir.ends_with("test_files"));

        // Check HTML contains expected content
        let html = fs::read_to_string(&result.output_path).unwrap();
        assert!(html.contains("Hello world"));
        assert!(html.contains("<title>"));
    }

    #[test]
    fn test_render_to_file_with_theme() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("themed.qmd");

        // Create a QMD file with theme
        fs::write(
            &input_path,
            r#"---
title: Themed Doc
format:
  html:
    theme: cosmo
---

Themed content.
"#,
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());
        let options = RenderToFileOptions::default();

        let result = render_to_file(&input_path, "html", &options, runtime).unwrap();

        // Check CSS was written
        let css_path = result.resources_dir.join("styles.css");
        assert!(css_path.exists());

        let css = fs::read_to_string(&css_path).unwrap();
        assert!(
            css.contains(".btn"),
            "CSS should contain compiled Bootstrap from cosmo theme"
        );
    }

    #[test]
    fn test_render_document_to_file_with_project() {
        let temp = TempDir::new().unwrap();
        let input_path = temp.path().join("doc.qmd");

        fs::write(
            &input_path,
            r#"---
title: Doc
---

Content.
"#,
        )
        .unwrap();

        let runtime = Arc::new(NativeRuntime::new());

        // Pre-discover project
        let project = ProjectContext::discover(&input_path, runtime.as_ref()).unwrap();

        let options = RenderToFileOptions::default();

        // Render with pre-discovered project
        let result =
            render_document_to_file(&input_path, "html", &options, Some(&project), runtime)
                .unwrap();

        assert!(result.output_path.exists());
    }
}
