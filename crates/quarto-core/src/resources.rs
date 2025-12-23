/*
 * resources.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Static resource management for HTML rendering.
 */

//! Static resource management for HTML output.
//!
//! This module manages static resources (CSS, JS) that are bundled with the
//! Quarto binary and written to the output directory during rendering.
//!
//! ## Architecture
//!
//! Resources are embedded at compile time using `include_str!` and written
//! to a `{document}_files/` directory alongside the HTML output. This follows
//! the Quarto convention for supporting files.
//!
//! ## Usage
//!
//! ```ignore
//! use quarto_core::resources::{write_html_resources, HtmlResourcePaths};
//! use quarto_system_runtime::NativeRuntime;
//!
//! let runtime = NativeRuntime::new();
//!
//! // During render, write resources and get paths
//! let paths = write_html_resources(&output_dir, "document", &runtime)?;
//!
//! // paths.css contains relative paths for template
//! // e.g., ["document_files/styles.css"]
//! ```

use std::path::{Path, PathBuf};

use quarto_system_runtime::SystemRuntime;

use crate::Result;

/// Default CSS styles, embedded at compile time.
pub const DEFAULT_CSS: &str = include_str!("../resources/styles.css");

/// Paths to HTML resources, relative to the output HTML file.
///
/// These paths are suitable for use in HTML `<link>` and `<script>` tags.
#[derive(Debug, Clone)]
pub struct HtmlResourcePaths {
    /// Relative paths to CSS files
    pub css: Vec<String>,
    /// Relative paths to JS files (for future use)
    pub js: Vec<String>,
    /// The resource directory path (absolute)
    pub resource_dir: PathBuf,
}

impl HtmlResourcePaths {
    /// Create an empty resource paths structure.
    pub fn empty() -> Self {
        Self {
            css: Vec::new(),
            js: Vec::new(),
            resource_dir: PathBuf::new(),
        }
    }
}

/// Write HTML resources to the output directory.
///
/// Creates a `{stem}_files/` directory alongside the output and writes
/// static resources (CSS, JS) there.
///
/// # Arguments
/// * `output_dir` - Directory containing the output HTML file
/// * `stem` - The stem of the output filename (e.g., "document" for "document.html")
/// * `runtime` - The system runtime for file operations
///
/// # Returns
/// Paths to the written resources, relative to the output HTML file.
///
/// # Example
/// ```ignore
/// // For output at /output/document.html
/// let paths = write_html_resources(Path::new("/output"), "document", &runtime)?;
/// // Creates /output/document_files/styles.css
/// // Returns paths.css = ["document_files/styles.css"]
/// ```
pub fn write_html_resources(
    output_dir: &Path,
    stem: &str,
    runtime: &dyn SystemRuntime,
) -> Result<HtmlResourcePaths> {
    // Create resource directory: {stem}_files/
    let resource_dir_name = format!("{}_files", stem);
    let resource_dir = output_dir.join(&resource_dir_name);

    runtime.dir_create(&resource_dir, true).map_err(|e| {
        crate::error::QuartoError::other(format!(
            "Failed to create resource directory {}: {}",
            resource_dir.display(),
            e
        ))
    })?;

    // Write CSS
    let css_filename = "styles.css";
    let css_path = resource_dir.join(css_filename);
    runtime
        .file_write(&css_path, DEFAULT_CSS.as_bytes())
        .map_err(|e| {
            crate::error::QuartoError::other(format!(
                "Failed to write CSS to {}: {}",
                css_path.display(),
                e
            ))
        })?;

    // Build relative paths for template
    let css_relative = format!("{}/{}", resource_dir_name, css_filename);

    Ok(HtmlResourcePaths {
        css: vec![css_relative],
        js: Vec::new(), // No JS resources yet
        resource_dir,
    })
}

/// Get the resource directory name for a given output stem.
///
/// This follows the Quarto convention of `{stem}_files/`.
pub fn resource_dir_name(stem: &str) -> String {
    format!("{}_files", stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_default_css_is_embedded() {
        assert!(!DEFAULT_CSS.is_empty());
        assert!(DEFAULT_CSS.contains("/* ===== Base Styles ===== */"));
        assert!(DEFAULT_CSS.contains(".callout"));
    }

    #[test]
    fn test_write_html_resources_creates_directory() {
        let runtime = NativeRuntime::new();
        let temp = TempDir::new().unwrap();
        let paths = write_html_resources(temp.path(), "document", &runtime).unwrap();

        assert!(paths.resource_dir.exists());
        assert!(paths.resource_dir.ends_with("document_files"));
    }

    #[test]
    fn test_write_html_resources_writes_css() {
        let runtime = NativeRuntime::new();
        let temp = TempDir::new().unwrap();
        let _paths = write_html_resources(temp.path(), "mydoc", &runtime).unwrap();

        let css_path = temp.path().join("mydoc_files/styles.css");
        assert!(css_path.exists());

        let content = fs::read_to_string(&css_path).unwrap();
        assert!(content.contains(".callout"));
    }

    #[test]
    fn test_write_html_resources_returns_relative_paths() {
        let runtime = NativeRuntime::new();
        let temp = TempDir::new().unwrap();
        let paths = write_html_resources(temp.path(), "test", &runtime).unwrap();

        assert_eq!(paths.css.len(), 1);
        assert_eq!(paths.css[0], "test_files/styles.css");
    }

    #[test]
    fn test_resource_dir_name() {
        assert_eq!(resource_dir_name("document"), "document_files");
        assert_eq!(resource_dir_name("my-doc"), "my-doc_files");
    }

    #[test]
    fn test_html_resource_paths_empty() {
        let paths = HtmlResourcePaths::empty();
        assert!(paths.css.is_empty());
        assert!(paths.js.is_empty());
    }
}
