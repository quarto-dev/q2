/*
 * resources.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Static resource management for Quarto.
 */

//! Static resource management for Quarto.
//!
//! This module provides two types of resource management:
//!
//! ## HTML Output Resources
//!
//! Static resources (CSS, JS) that are bundled with the Quarto binary and
//! written to the output directory during rendering. These follow the Quarto
//! convention of a `{document}_files/` directory alongside the HTML output.
//!
//! ```ignore
//! use quarto_core::resources::{write_html_resources, HtmlResourcePaths};
//!
//! let paths = write_html_resources(&output_dir, "document", &runtime)?;
//! // Creates document_files/styles.css
//! ```
//!
//! ## Embedded Resource Bundles (Native Only)
//!
//! Resources embedded at compile time using `include_dir!` that are extracted
//! to a temporary directory on first access. Used by execution engines (knitr,
//! jupyter) for their supporting scripts.
//!
//! ```ignore
//! use include_dir::{include_dir, Dir};
//! use quarto_core::resources::ResourceBundle;
//!
//! // Define a bundle with embedded directory
//! static SCRIPTS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/scripts");
//! pub static SCRIPTS: ResourceBundle = ResourceBundle::new("scripts", &SCRIPTS_DIR);
//!
//! // Get extracted path (lazily extracts on first call)
//! let path = SCRIPTS.path()?;
//! let script = path.join("myscript.py");
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

// ============================================================================
// Embedded Resource Bundles (Native Only)
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
mod embedded {
    use std::io;
    use std::path::Path;
    use std::sync::OnceLock;

    use include_dir::Dir;
    use tempfile::TempDir;
    use thiserror::Error;

    /// Errors that can occur during resource extraction.
    #[derive(Debug, Error)]
    pub enum ResourceError {
        /// Failed to create the temporary directory.
        #[error("Failed to create temp directory: {0}")]
        TempDir(#[from] io::Error),

        /// Failed to extract resources to disk.
        #[error("Failed to extract resources: {0}")]
        Extract(String),
    }

    /// A bundle of embedded resources from a directory.
    ///
    /// Resources are embedded at compile time using `include_dir!` and extracted
    /// to a temporary directory on first access. The temp directory persists for
    /// the lifetime of the process and is automatically cleaned up on exit.
    ///
    /// # Directory Structure Preservation
    ///
    /// The relative paths within the embedded directory are preserved when
    /// extracted. For example, if you embed a directory containing `rmd/rmd.R`,
    /// the extracted path will be `{temp_dir}/rmd/rmd.R`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use include_dir::{include_dir, Dir};
    /// use quarto_core::resources::ResourceBundle;
    ///
    /// // Embed a directory at compile time
    /// static SCRIPTS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/engine/knitr/resources");
    /// pub static KNITR_RESOURCES: ResourceBundle = ResourceBundle::new("knitr", &SCRIPTS_DIR);
    ///
    /// // Get the extraction path (extracts lazily on first call)
    /// let base_path = KNITR_RESOURCES.path()?;
    /// let rmd_script = base_path.join("rmd/rmd.R");
    /// ```
    pub struct ResourceBundle {
        /// Name of this bundle (used for temp directory prefix).
        name: &'static str,
        /// The embedded directory tree.
        dir: &'static Dir<'static>,
        /// Lazily-initialized extraction directory.
        extracted: OnceLock<TempDir>,
    }

    impl ResourceBundle {
        /// Create a new resource bundle.
        ///
        /// This is `const` so it can be used in static initialization.
        ///
        /// # Arguments
        ///
        /// * `name` - Name for this bundle (used in temp directory prefix)
        /// * `dir` - Reference to the embedded directory from `include_dir!`
        pub const fn new(name: &'static str, dir: &'static Dir<'static>) -> Self {
            Self {
                name,
                dir,
                extracted: OnceLock::new(),
            }
        }

        /// Get the path to the extracted resources directory.
        ///
        /// On first call, extracts all embedded resources to a temp directory.
        /// Subsequent calls return the same directory.
        ///
        /// # Errors
        ///
        /// Returns an error if the temp directory cannot be created or if
        /// resource extraction fails.
        pub fn path(&self) -> Result<&Path, ResourceError> {
            let temp_dir = self
                .extracted
                .get_or_init(|| self.extract().expect("Failed to extract resources"));

            Ok(temp_dir.path())
        }

        /// Get the name of this bundle.
        pub fn name(&self) -> &str {
            self.name
        }

        /// Check if resources have been extracted yet.
        pub fn is_extracted(&self) -> bool {
            self.extracted.get().is_some()
        }

        /// Extract all resources to a temp directory.
        fn extract(&self) -> Result<TempDir, ResourceError> {
            let temp_dir = tempfile::Builder::new()
                .prefix(&format!("quarto-{}-", self.name))
                .tempdir()?;

            self.dir
                .extract(temp_dir.path())
                .map_err(|e| ResourceError::Extract(e.to_string()))?;

            Ok(temp_dir)
        }
    }

    // ResourceBundle is Send + Sync because:
    // - name and dir are 'static references (inherently Send + Sync)
    // - OnceLock<TempDir> is Send + Sync when TempDir is Send
    // - TempDir is Send (it's just a PathBuf internally)
    unsafe impl Send for ResourceBundle {}
    unsafe impl Sync for ResourceBundle {}

    #[cfg(test)]
    mod tests {
        use super::*;
        use include_dir::include_dir;

        // Create a test bundle using the actual knitr resources
        static TEST_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/engine/knitr/resources");
        static TEST_BUNDLE: ResourceBundle = ResourceBundle::new("test", &TEST_DIR);

        #[test]
        fn test_bundle_name() {
            assert_eq!(TEST_BUNDLE.name(), "test");
        }

        #[test]
        fn test_bundle_path_creates_directory() {
            let path = TEST_BUNDLE.path().expect("Failed to get path");
            assert!(path.exists());
            assert!(path.is_dir());
        }

        #[test]
        fn test_bundle_path_idempotent() {
            let path1 = TEST_BUNDLE.path().expect("Failed to get path");
            let path2 = TEST_BUNDLE.path().expect("Failed to get path");
            assert_eq!(path1, path2);
        }

        #[test]
        fn test_bundle_extracts_files() {
            let path = TEST_BUNDLE.path().expect("Failed to get path");

            // Should have extracted rmd/rmd.R (once we restructure)
            // For now, check that files exist at the root
            let entries: Vec<_> = std::fs::read_dir(path)
                .expect("Failed to read dir")
                .collect();
            assert!(!entries.is_empty(), "No files extracted");
        }

        #[test]
        fn test_bundle_is_extracted() {
            // Create a fresh bundle for this test
            static FRESH_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/engine/knitr/resources");
            static FRESH_BUNDLE: ResourceBundle = ResourceBundle::new("fresh", &FRESH_DIR);

            // Note: Can't reliably test is_extracted() == false because other tests
            // may have already triggered extraction. Just verify the method works.
            let _ = FRESH_BUNDLE.path();
            assert!(FRESH_BUNDLE.is_extracted());
        }

        #[test]
        fn test_resource_bundle_is_send_sync() {
            fn assert_send_sync<T: Send + Sync>() {}
            assert_send_sync::<ResourceBundle>();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use embedded::{ResourceBundle, ResourceError};
