/*
 * engine/context.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Execution context and result types for engines.
 */

//! Execution context and result types for engines.

use std::path::PathBuf;

use quarto_pandoc_types::ConfigValue;

use crate::stage::PandocIncludes;

/// Context provided to execution engines.
///
/// This contains all the information an engine needs to execute code cells
/// in a document, including paths, configuration, and engine-specific options.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Temporary directory for engine use.
    ///
    /// Engines can create intermediate files here. The directory is
    /// cleaned up after rendering completes.
    pub temp_dir: PathBuf,

    /// Working directory for execution.
    ///
    /// This is typically the directory containing the source document,
    /// so relative paths in code cells resolve correctly.
    pub cwd: PathBuf,

    /// Project directory, if rendering within a Quarto project.
    ///
    /// `None` for single-file renders (no `_quarto.yml`).
    pub project_dir: Option<PathBuf>,

    /// Path to the source document being rendered.
    pub source_path: PathBuf,

    /// Target output format identifier (e.g., "html", "pdf").
    pub format: String,

    /// Whether to run quietly (suppress engine output).
    pub quiet: bool,

    /// Engine-specific configuration from document metadata.
    ///
    /// This is a clone of the ConfigValue found under the engine key.
    /// For example, for `engine: { jupyter: { kernel: python3 } }`,
    /// this would contain the `{ kernel: python3 }` map.
    pub engine_config: Option<ConfigValue>,
}

impl ExecutionContext {
    /// Create a new execution context with required fields.
    pub fn new(
        temp_dir: PathBuf,
        cwd: PathBuf,
        source_path: PathBuf,
        format: impl Into<String>,
    ) -> Self {
        Self {
            temp_dir,
            cwd,
            project_dir: None,
            source_path,
            format: format.into(),
            quiet: false,
            engine_config: None,
        }
    }

    /// Set the project directory.
    pub fn with_project_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.project_dir = dir;
        self
    }

    /// Set quiet mode.
    pub fn with_quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    /// Set engine configuration.
    pub fn with_engine_config(mut self, config: Option<ConfigValue>) -> Self {
        self.engine_config = config;
        self
    }
}

/// Result of engine execution.
///
/// Contains the transformed markdown with execution outputs,
/// along with any supporting files and metadata produced.
#[derive(Debug, Clone, Default)]
pub struct ExecuteResult {
    /// The transformed markdown content with execution outputs.
    ///
    /// Code cells have been replaced with their outputs (text, images, etc.).
    pub markdown: String,

    /// Supporting files produced during execution.
    ///
    /// These are typically figure images, data files, or other resources
    /// that need to be included in the final output.
    pub supporting_files: Vec<PathBuf>,

    /// Pandoc filters to apply to the document.
    ///
    /// Some engines require specific filters to process their output
    /// (e.g., the "quarto" filter for processing Quarto extensions).
    pub filters: Vec<String>,

    /// Content to inject at specific locations in the document.
    ///
    /// Engines can add CSS, JavaScript, or other content to the
    /// document header, before the body, or after the body.
    pub includes: PandocIncludes,

    /// Whether the output requires post-processing.
    ///
    /// If true, additional processing steps may be needed after
    /// the main rendering is complete.
    pub needs_postprocess: bool,
}

impl ExecuteResult {
    /// Create a new result with just markdown content.
    pub fn new(markdown: impl Into<String>) -> Self {
        Self {
            markdown: markdown.into(),
            ..Default::default()
        }
    }

    /// Create a passthrough result (input unchanged).
    pub fn passthrough(input: &str) -> Self {
        Self::new(input)
    }

    /// Add supporting files.
    pub fn with_supporting_files(mut self, files: Vec<PathBuf>) -> Self {
        self.supporting_files = files;
        self
    }

    /// Add filters.
    pub fn with_filters(mut self, filters: Vec<String>) -> Self {
        self.filters = filters;
        self
    }

    /// Add includes.
    pub fn with_includes(mut self, includes: PandocIncludes) -> Self {
        self.includes = includes;
        self
    }

    /// Set post-processing flag.
    pub fn with_postprocess(mut self, needs: bool) -> Self {
        self.needs_postprocess = needs;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === ExecutionContext tests ===

    #[test]
    fn test_execution_context_new() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        );

        assert_eq!(ctx.temp_dir, PathBuf::from("/tmp"));
        assert_eq!(ctx.cwd, PathBuf::from("/project"));
        assert_eq!(ctx.source_path, PathBuf::from("/project/doc.qmd"));
        assert_eq!(ctx.format, "html");
        assert!(!ctx.quiet);
        assert!(ctx.project_dir.is_none());
        assert!(ctx.engine_config.is_none());
    }

    #[test]
    fn test_execution_context_with_project_dir() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        )
        .with_project_dir(Some(PathBuf::from("/project")));

        assert_eq!(ctx.project_dir, Some(PathBuf::from("/project")));
    }

    #[test]
    fn test_execution_context_with_quiet() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        )
        .with_quiet(true);

        assert!(ctx.quiet);
    }

    #[test]
    fn test_execution_context_clone() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        );
        let cloned = ctx.clone();

        assert_eq!(ctx.temp_dir, cloned.temp_dir);
        assert_eq!(ctx.format, cloned.format);
    }

    // === ExecuteResult tests ===

    #[test]
    fn test_execute_result_new() {
        let result = ExecuteResult::new("# Hello\n\nWorld");

        assert_eq!(result.markdown, "# Hello\n\nWorld");
        assert!(result.supporting_files.is_empty());
        assert!(result.filters.is_empty());
        assert!(!result.needs_postprocess);
    }

    #[test]
    fn test_execute_result_passthrough() {
        let input = "Some markdown content";
        let result = ExecuteResult::passthrough(input);

        assert_eq!(result.markdown, input);
    }

    #[test]
    fn test_execute_result_with_supporting_files() {
        let result = ExecuteResult::new("content").with_supporting_files(vec![
            PathBuf::from("figure1.png"),
            PathBuf::from("figure2.png"),
        ]);

        assert_eq!(result.supporting_files.len(), 2);
    }

    #[test]
    fn test_execute_result_with_filters() {
        let result = ExecuteResult::new("content").with_filters(vec!["quarto".to_string()]);

        assert_eq!(result.filters, vec!["quarto"]);
    }

    #[test]
    fn test_execute_result_with_postprocess() {
        let result = ExecuteResult::new("content").with_postprocess(true);

        assert!(result.needs_postprocess);
    }

    #[test]
    fn test_execute_result_default() {
        let result = ExecuteResult::default();

        assert!(result.markdown.is_empty());
        assert!(result.supporting_files.is_empty());
        assert!(result.filters.is_empty());
        assert!(!result.needs_postprocess);
    }

    #[test]
    fn test_execute_result_builder_chain() {
        let result = ExecuteResult::new("# Title")
            .with_supporting_files(vec![PathBuf::from("img.png")])
            .with_filters(vec!["filter1".to_string()])
            .with_postprocess(true);

        assert_eq!(result.markdown, "# Title");
        assert_eq!(result.supporting_files.len(), 1);
        assert_eq!(result.filters.len(), 1);
        assert!(result.needs_postprocess);
    }
}
