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

use tracing::{debug, warn};

use quarto_system_runtime::SystemRuntime;

use crate::error::QuartoError;
use crate::format::{extract_format_metadata, Format};
use crate::pipeline::{render_qmd_to_html, HtmlRenderConfig, RenderOutput};
use crate::project::{DocumentInfo, ProjectContext};
use crate::render::{BinaryDependencies, RenderContext};
use crate::resources::{self, HtmlResourcePaths};
use crate::Result;

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

    let input_str = std::str::from_utf8(&input_bytes)
        .map_err(|e| QuartoError::other(format!("Input file contains invalid UTF-8: {}", e)))?;

    // Use provided project or discover
    let discovered_project;
    let project = match project {
        Some(p) => p,
        None => {
            discovered_project = ProjectContext::discover(input_path, runtime.as_ref())?;
            &discovered_project
        }
    };

    // Extract format-specific metadata from frontmatter (toc, theme, etc.)
    let format_metadata = extract_format_metadata(input_str, format).unwrap_or_else(|e| {
        warn!("Failed to extract format metadata: {}. Using defaults.", e);
        serde_json::Value::Null
    });

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

    // Write resources (CSS) with theme support
    let resource_paths = write_themed_resources(
        input_str,
        input_path,
        &output_dir,
        &output_stem,
        runtime.as_ref(),
        options.quiet,
    )?;

    // Set up render context with format that includes extracted metadata
    let doc_info = DocumentInfo::from_path(input_path);
    let render_format = format_from_name(format).with_metadata(format_metadata);
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

// ============================================================================
// Theme Support (Native Only)
// ============================================================================

/// Write HTML resources with theme support.
///
/// Extracts theme configuration from frontmatter and compiles SASS accordingly.
/// Falls back to default CSS if no theme is specified or if compilation fails.
#[cfg(not(target_arch = "wasm32"))]
fn write_themed_resources(
    content: &str,
    input_path: &Path,
    output_dir: &Path,
    stem: &str,
    runtime: &dyn SystemRuntime,
    quiet: bool,
) -> Result<HtmlResourcePaths> {
    use quarto_sass::ThemeContext;

    // Try to extract theme config from frontmatter
    let theme_config = match extract_theme_config(content) {
        Ok(Some(config)) => {
            if !quiet {
                debug!("Theme configuration found: {:?}", config);
            }
            config
        }
        Ok(None) => {
            debug!("No theme specified, using default CSS");
            return resources::write_html_resources(output_dir, stem, runtime);
        }
        Err(e) => {
            warn!(
                "Failed to parse theme configuration: {}. Using default CSS.",
                e
            );
            return resources::write_html_resources(output_dir, stem, runtime);
        }
    };

    // Create theme context with the document's directory
    let document_dir = input_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let context = ThemeContext::new(document_dir, runtime);

    // Try to compile themed CSS
    match resources::write_html_resources_with_sass(
        output_dir,
        stem,
        &theme_config,
        &context,
        runtime,
    ) {
        Ok(paths) => {
            if !quiet {
                debug!("Compiled theme CSS successfully");
            }
            Ok(paths)
        }
        Err(e) => {
            warn!("Theme CSS compilation failed: {}. Using default CSS.", e);
            resources::write_html_resources(output_dir, stem, runtime)
        }
    }
}

/// Extract theme configuration from QMD frontmatter.
///
/// Parses the YAML frontmatter and extracts the `format.html.theme` value.
/// Returns `Ok(None)` if no theme is specified.
///
/// TODO(ConfigValue): DELETE THIS FUNCTION. Replace all calls with:
/// ```ignore
/// let theme_config = ThemeConfig::from_config_value(&merged_config)?;
/// ```
#[cfg(not(target_arch = "wasm32"))]
fn extract_theme_config(content: &str) -> Result<Option<quarto_sass::ThemeConfig>> {
    // Find YAML frontmatter
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Ok(None);
    }

    // Find closing ---
    let after_first = &trimmed[3..];
    let end_pos = match after_first.find("\n---") {
        Some(pos) => pos,
        None => return Ok(None),
    };

    // Parse YAML
    let yaml_str = &after_first[..end_pos].trim();
    let yaml_value: serde_yaml::Value = serde_yaml::from_str(yaml_str)
        .map_err(|e| QuartoError::other(format!("Failed to parse YAML frontmatter: {}", e)))?;

    // Navigate to format.html.theme
    let theme_value = yaml_value
        .get("format")
        .and_then(|f| f.get("html"))
        .and_then(|h| h.get("theme"));

    let theme_value = match theme_value {
        Some(v) => v,
        None => return Ok(None),
    };

    // Convert to ThemeConfig
    let config = theme_value_to_config(theme_value)?;
    Ok(Some(config))
}

/// Convert a serde_yaml::Value theme specification to ThemeConfig.
///
/// TODO(ConfigValue): DELETE THIS FUNCTION.
#[cfg(not(target_arch = "wasm32"))]
fn theme_value_to_config(value: &serde_yaml::Value) -> Result<quarto_sass::ThemeConfig> {
    use quarto_sass::{ThemeConfig, ThemeSpec};

    match value {
        serde_yaml::Value::String(s) => {
            let spec = ThemeSpec::parse(s)
                .map_err(|e| QuartoError::other(format!("Invalid theme '{}': {}", s, e)))?;
            Ok(ThemeConfig::new(vec![spec], true))
        }
        serde_yaml::Value::Sequence(arr) => {
            let mut themes = Vec::new();
            for v in arr {
                if let Some(s) = v.as_str() {
                    let spec = ThemeSpec::parse(s)
                        .map_err(|e| QuartoError::other(format!("Invalid theme '{}': {}", s, e)))?;
                    themes.push(spec);
                }
            }
            if themes.is_empty() {
                return Err(QuartoError::other("Empty theme array"));
            }
            Ok(ThemeConfig::new(themes, true))
        }
        serde_yaml::Value::Null => Ok(ThemeConfig::default_bootstrap()),
        _ => Err(QuartoError::other(
            "Invalid theme value: expected string, array, or null",
        )),
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

        // Check CSS was generated
        let css_path = result.resources_dir.join("styles.css");
        assert!(css_path.exists());

        // Check it contains Bootstrap classes (from cosmo theme)
        let css = fs::read_to_string(&css_path).unwrap();
        assert!(css.contains(".btn"));
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
