/*
 * render.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Render context for pipeline execution.
 */

//! Render context for pipeline execution.
//!
//! The `RenderContext` is the mutable state passed through all pipeline stages:
//! - Transforms can read and write to the artifact store
//! - Transforms can access project configuration and format settings
//! - Writers use the context to determine output paths

use std::path::PathBuf;

use quarto_system_runtime::SystemRuntime;

use crate::artifact::ArtifactStore;
use crate::format::Format;
use crate::project::{DocumentInfo, ProjectContext};

/// Binary dependencies available for rendering
#[derive(Debug, Clone, Default)]
pub struct BinaryDependencies {
    /// dart-sass binary path (for SASS compilation)
    pub dart_sass: Option<PathBuf>,

    /// esbuild binary path (for JS bundling)
    pub esbuild: Option<PathBuf>,

    /// Pandoc binary path (for non-native formats)
    pub pandoc: Option<PathBuf>,

    /// Typst binary path
    pub typst: Option<PathBuf>,
}

impl BinaryDependencies {
    /// Create empty binary dependencies
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover binary dependencies from environment and PATH
    pub fn discover(runtime: &dyn SystemRuntime) -> Self {
        Self {
            dart_sass: runtime.find_binary("sass", "QUARTO_DART_SASS"),
            esbuild: runtime.find_binary("esbuild", "QUARTO_ESBUILD"),
            pandoc: runtime.find_binary("pandoc", "QUARTO_PANDOC"),
            typst: runtime.find_binary("typst", "QUARTO_TYPST"),
        }
    }

    /// Check if dart-sass is available
    pub fn has_sass(&self) -> bool {
        self.dart_sass.is_some()
    }

    /// Check if Pandoc is available
    pub fn has_pandoc(&self) -> bool {
        self.pandoc.is_some()
    }
}

/// Context for a single document render operation.
///
/// This is the mutable state passed through all pipeline stages.
/// It contains:
/// - References to project and document configuration (immutable borrows)
/// - The artifact store (mutable, for collecting dependencies and intermediates)
/// - The target format
/// - Binary dependencies
pub struct RenderContext<'a> {
    /// Artifact store for dependencies and intermediates
    pub artifacts: ArtifactStore,

    /// Project context (configuration, paths)
    pub project: &'a ProjectContext,

    /// Information about the document being rendered
    pub document: &'a DocumentInfo,

    /// Target format for this render
    pub format: &'a Format,

    /// Binary dependencies
    pub binaries: &'a BinaryDependencies,

    /// Render options
    pub options: RenderOptions,
}

/// Options for rendering
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Whether to enable verbose/debug output
    pub verbose: bool,

    /// Whether to execute code cells (false for markdown-only engine)
    pub execute: bool,

    /// Whether to use cached execution results
    pub use_freeze: bool,

    /// Custom output path (overrides format-determined path)
    pub output_path: Option<PathBuf>,
}

impl<'a> RenderContext<'a> {
    /// Create a new render context
    pub fn new(
        project: &'a ProjectContext,
        document: &'a DocumentInfo,
        format: &'a Format,
        binaries: &'a BinaryDependencies,
    ) -> Self {
        Self {
            artifacts: ArtifactStore::new(),
            project,
            document,
            format,
            binaries,
            options: RenderOptions::default(),
        }
    }

    /// Create with custom options
    pub fn with_options(mut self, options: RenderOptions) -> Self {
        self.options = options;
        self
    }

    /// Get the output path for this render
    ///
    /// Priority:
    /// 1. Custom output path from options
    /// 2. Document's output path
    /// 3. Format-determined path from input
    pub fn output_path(&self) -> PathBuf {
        if let Some(ref path) = self.options.output_path {
            return path.clone();
        }

        if let Some(ref path) = self.document.output {
            return path.clone();
        }

        // Determine from format
        let output = self.format.output_path(&self.document.input);

        // If project has output_dir, make path relative to that
        if self.project.output_dir != self.project.dir
            && let Ok(relative) = self.document.input.strip_prefix(&self.project.dir) {
                let mut result = self.project.output_dir.join(relative);
                result.set_extension(&self.format.output_extension);
                return result;
            }

        output
    }

    /// Get a metadata value from the format configuration
    pub fn format_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        if self.format.metadata.is_null() {
            return None;
        }
        self.format.metadata.get(key)
    }

    /// Check if this is a native Rust pipeline render
    pub fn is_native(&self) -> bool {
        self.format.native_pipeline
    }
}

/// Result of a render operation
#[derive(Debug)]
pub struct RenderResult {
    /// Primary output file
    pub output_file: PathBuf,

    /// Additional files produced (lib/, resources, etc.)
    pub supporting_files: Vec<PathBuf>,

    /// Warnings generated during rendering
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::DocumentInfo;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: None,
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    #[test]
    fn test_binary_dependencies_new() {
        let deps = BinaryDependencies::new();
        assert!(deps.dart_sass.is_none());
        assert!(deps.pandoc.is_none());
        assert!(!deps.has_sass());
        assert!(!deps.has_pandoc());
    }

    #[test]
    fn test_render_context_output_path() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();

        let ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let output = ctx.output_path();

        assert_eq!(output, PathBuf::from("/project/doc.html"));
    }

    #[test]
    fn test_render_context_custom_output() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();

        let options = RenderOptions {
            output_path: Some(PathBuf::from("/custom/output.html")),
            ..Default::default()
        };

        let ctx = RenderContext::new(&project, &doc, &format, &binaries).with_options(options);
        let output = ctx.output_path();

        assert_eq!(output, PathBuf::from("/custom/output.html"));
    }
}
