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
//! - SASS theme compilation (Bootstrap/Bootswatch themes)
//!
//! Not yet supported:
//! - Code execution
//! - Navigation (navbar, sidebar, footer)
//! - Multi-file projects
//! - Non-HTML formats

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use quarto_core::{
    BinaryDependencies, DocumentInfo, Format, FormatIdentifier, HtmlRenderConfig, ProjectContext,
    QuartoError, RenderContext, RenderOptions, extract_format_metadata, render_qmd_to_html,
};
use quarto_sass::{ThemeConfig, ThemeContext, ThemeSpec};
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

/// Resolve format string to Format (without metadata)
fn resolve_format(format_str: &str) -> Result<Format> {
    resolve_format_with_metadata(format_str, serde_json::Value::Null)
}

/// Resolve format string to Format with metadata
fn resolve_format_with_metadata(format_str: &str, metadata: serde_json::Value) -> Result<Format> {
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
        metadata,
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

    // Read input file early (we need it for format metadata extraction)
    let input_bytes = runtime.file_read(&doc_info.input).map_err(|e| {
        anyhow::anyhow!(
            "Failed to read input file {}: {}",
            doc_info.input.display(),
            e
        )
    })?;

    // Convert to string for metadata extraction (needed for YAML parsing)
    let input_str =
        std::str::from_utf8(&input_bytes).context("Input file contains invalid UTF-8")?;

    // Extract format-specific metadata from frontmatter (e.g., toc, toc-depth)
    let format_metadata = extract_format_metadata(input_str, "html").unwrap_or_else(|e| {
        warn!("Failed to extract format metadata: {}. Using defaults.", e);
        serde_json::Value::Null
    });

    // Create format with the extracted metadata
    let format_with_metadata = Format {
        identifier: format.identifier.clone(),
        output_extension: format.output_extension.clone(),
        native_pipeline: format.native_pipeline,
        metadata: format_metadata,
    };

    // Create render context with the format that has metadata
    let options = RenderOptions {
        verbose: !args.quiet,
        execute: false, // MVP: no code execution
        use_freeze: false,
        output_path: args.output.as_ref().map(PathBuf::from),
    };

    let mut ctx = RenderContext::new(project, doc_info, &format_with_metadata, binaries)
        .with_options(options);

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

    // Extract theme configuration from frontmatter and write resources
    let resource_paths = write_themed_resources(
        input_str,
        &doc_info.input,
        output_dir,
        output_stem,
        runtime,
        args.quiet,
    )?;

    // Use the unified pipeline to render
    let input_path_str = doc_info.input.to_string_lossy();
    let config = HtmlRenderConfig {
        css_paths: &resource_paths.css,
        template: None,
    };

    // Create Arc runtime for the async pipeline
    let runtime_arc: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    // Use pollster to run the async pipeline synchronously (expects bytes)
    let output = match pollster::block_on(render_qmd_to_html(
        &input_bytes,
        &input_path_str,
        &mut ctx,
        &config,
        runtime_arc,
    )) {
        Ok(output) => output,
        Err(QuartoError::Parse(parse_error)) => {
            // Parse errors have rich ariadne formatting with their own "Error:" prefix.
            // Print directly to avoid anyhow adding a duplicate prefix.
            eprintln!("{}", parse_error);
            std::process::exit(1);
        }
        Err(e) => return Err(anyhow::anyhow!("{}", e)),
    };

    // Report diagnostics with full ariadne-style source context
    if !args.quiet && !output.diagnostics.is_empty() {
        for diagnostic in &output.diagnostics {
            // Use the source context for rich error rendering with source snippets
            eprintln!("{}", diagnostic.to_text(Some(&output.source_context)));
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

// ============================================================================
// Theme Support
// ============================================================================
//
// TODO(ConfigValue): This section manually parses YAML frontmatter to extract
// theme configuration. When the render pipeline adopts ConfigValue-based
// configuration (merged project + document config), replace:
//
// 1. `extract_theme_config()` - Use `ThemeConfig::from_config_value(&merged_config)`
//    instead of manual YAML parsing
//
// 2. `theme_value_to_config()` - No longer needed; ThemeConfig::from_config_value
//    handles all the conversion logic
//
// 3. The `input_str` parameter to `write_themed_resources()` - Replace with
//    a reference to the merged ConfigValue from RenderContext
//
// See: quarto-sass/src/config.rs for the ConfigValue-based API
// Plan: claude-notes/plans/2026-01-24-phase7-sass-render-integration.md
// ============================================================================

/// Write HTML resources with theme support.
///
/// Extracts theme configuration from frontmatter and compiles SASS accordingly.
/// Falls back to default CSS if no theme is specified or if compilation fails.
///
/// TODO(ConfigValue): Replace `content` parameter with `config: &ConfigValue`
/// from merged project/document configuration, then use
/// `ThemeConfig::from_config_value(config)` instead of `extract_theme_config()`.
fn write_themed_resources(
    content: &str,
    input_path: &Path,
    output_dir: &Path,
    stem: &str,
    runtime: &dyn SystemRuntime,
    quiet: bool,
) -> Result<quarto_core::resources::HtmlResourcePaths> {
    // Try to extract theme config from frontmatter
    let theme_config = match extract_theme_config(content) {
        Ok(Some(config)) => {
            if !quiet {
                debug!("Theme configuration found: {:?}", config);
            }
            config
        }
        Ok(None) => {
            // No theme specified - use default static CSS
            debug!("No theme specified, using default CSS");
            return quarto_core::resources::write_html_resources(output_dir, stem, runtime)
                .context("Failed to write default HTML resources");
        }
        Err(e) => {
            // Failed to parse - warn and fall back to default
            warn!(
                "Failed to parse theme configuration: {}. Using default CSS.",
                e
            );
            return quarto_core::resources::write_html_resources(output_dir, stem, runtime)
                .context("Failed to write default HTML resources");
        }
    };

    // Create theme context with the document's directory
    let document_dir = input_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let context = ThemeContext::new(document_dir, runtime);

    // Try to compile themed CSS
    match quarto_core::resources::write_html_resources_with_sass(
        output_dir,
        stem,
        &theme_config,
        &context,
        runtime,
    ) {
        Ok(paths) => {
            if !quiet {
                info!("Compiled theme CSS successfully");
            }
            Ok(paths)
        }
        Err(e) => {
            // Compilation failed - warn and fall back to default
            warn!("Theme CSS compilation failed: {}. Using default CSS.", e);
            quarto_core::resources::write_html_resources(output_dir, stem, runtime)
                .context("Failed to write fallback HTML resources")
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
/// The merged ConfigValue from RenderContext already has the parsed and
/// merged configuration from both project (_quarto.yml) and document frontmatter.
fn extract_theme_config(content: &str) -> Result<Option<ThemeConfig>> {
    // Find YAML frontmatter
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Ok(None);
    }

    // Find closing ---
    let after_first = &trimmed[3..];
    let end_pos = match after_first.find("\n---") {
        Some(pos) => pos,
        None => return Ok(None), // Unclosed frontmatter
    };

    // Parse YAML
    let yaml_str = &after_first[..end_pos].trim();
    let yaml_value: serde_yaml::Value =
        serde_yaml::from_str(yaml_str).context("Failed to parse YAML frontmatter")?;

    // Navigate to format.html.theme
    let theme_value = yaml_value
        .get("format")
        .and_then(|f| f.get("html"))
        .and_then(|h| h.get("theme"));

    let theme_value = match theme_value {
        Some(v) => v,
        None => return Ok(None), // No theme specified
    };

    // Convert to ThemeConfig
    let config = theme_value_to_config(theme_value)?;
    Ok(Some(config))
}

/// Convert a serde_yaml::Value theme specification to ThemeConfig.
///
/// TODO(ConfigValue): DELETE THIS FUNCTION. This duplicates logic already in
/// `ThemeConfig::from_config_value()` in quarto-sass/src/config.rs.
fn theme_value_to_config(value: &serde_yaml::Value) -> Result<ThemeConfig> {
    match value {
        serde_yaml::Value::String(s) => {
            // Single theme name: "darkly"
            let spec =
                ThemeSpec::parse(s).map_err(|e| anyhow::anyhow!("Invalid theme '{}': {}", s, e))?;
            Ok(ThemeConfig::new(vec![spec], true))
        }
        serde_yaml::Value::Sequence(arr) => {
            // Array of themes: ["darkly", "custom.scss"]
            let mut themes = Vec::new();
            for v in arr {
                if let Some(s) = v.as_str() {
                    let spec = ThemeSpec::parse(s)
                        .map_err(|e| anyhow::anyhow!("Invalid theme '{}': {}", s, e))?;
                    themes.push(spec);
                }
            }
            if themes.is_empty() {
                anyhow::bail!("Empty theme array");
            }
            Ok(ThemeConfig::new(themes, true))
        }
        serde_yaml::Value::Null => {
            // Explicit null - use default Bootstrap
            Ok(ThemeConfig::default_bootstrap())
        }
        _ => {
            anyhow::bail!("Invalid theme value: expected string, array, or null");
        }
    }
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
