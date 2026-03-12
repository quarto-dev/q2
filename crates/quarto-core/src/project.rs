/*
 * project.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Project context for Quarto rendering.
 */

//! Project context management.
//!
//! A project context represents either:
//! - A Quarto project (with `_quarto.yml`)
//! - A single-file "pseudo-project" (no configuration file)
//!
//! The project context provides:
//! - Project root directory
//! - Parsed configuration
//! - List of input files
//! - Output directory resolution

use std::path::{Path, PathBuf};

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_system_runtime::SystemRuntime;

use crate::error::{QuartoError, Result};

/// Find and parse all `_metadata.yml` files between project root and document directory.
///
/// Walks the directory hierarchy from project root to the document's parent directory,
/// looking for `_metadata.yml` or `_metadata.yaml` files. Each found file is parsed
/// and returned as a ConfigValue layer.
///
/// # Arguments
///
/// * `project` - The project context (provides project root directory)
/// * `document_path` - Path to the document being rendered
///
/// # Returns
///
/// A vector of `ConfigValue` layers, ordered from project root to document directory.
/// Each layer contains the parsed metadata from that directory's `_metadata.yml` file.
/// Directories without `_metadata.yml` are skipped.
///
/// # Behavior
///
/// - Walks directories between project root and document's parent directory
/// - Does NOT include the project root directory itself (matches TS Quarto behavior)
/// - Returns empty vec for single-file projects (no project config)
/// - Returns empty vec if document is directly in project root
///
/// # Errors
///
/// Returns an error if:
/// - A `_metadata.yml` file contains invalid YAML syntax
/// - File I/O errors occur
///
/// # Example
///
/// Given project structure:
/// ```text
/// project/
///   _quarto.yml
///   _metadata.yml          # NOT included (project root)
///   chapters/
///     _metadata.yml        # Layer 0: { toc: true }
///     intro/
///       _metadata.yml      # Layer 1: { toc-depth: 2 }
///       chapter1.qmd       # Document being rendered
/// ```
///
/// Returns: [layer0, layer1] - deeper directories later in vec
pub fn directory_metadata_for_document(
    project: &ProjectContext,
    document_path: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<Vec<ConfigValue>> {
    use pampa::pandoc::yaml_to_config_value;
    use pampa::utils::diagnostic_collector::DiagnosticCollector;
    use quarto_config::InterpretationContext;

    // Single-file projects don't have directory metadata
    if project.is_single_file {
        return Ok(Vec::new());
    }

    // Canonicalize the document path so strip_prefix works reliably.
    // project.dir is always canonical (from ProjectContext::discover), but
    // callers may pass relative paths (e.g., WASM render_qmd with VFS paths).
    let document_path = runtime
        .canonicalize(document_path)
        .unwrap_or_else(|_| document_path.to_path_buf());

    let project_dir = &project.dir;
    let document_dir = document_path
        .parent()
        .ok_or_else(|| QuartoError::Other("Document has no parent directory".into()))?;

    // Get relative path from project root to document directory
    let relative_path = match document_dir.strip_prefix(project_dir) {
        Ok(rel) => rel,
        Err(_) => {
            // Document is not under project directory
            return Ok(Vec::new());
        }
    };

    // Split into directory components
    let components: Vec<_> = relative_path.components().collect();
    if components.is_empty() {
        // Document is in project root, no directories to walk
        return Ok(Vec::new());
    }

    let mut layers = Vec::new();
    let mut current_dir = project_dir.clone();

    // Walk through each directory from project root toward document
    // (but not including project root itself - we start from first subdir)
    for component in components {
        current_dir = current_dir.join(component);

        // Look for _metadata.yml or _metadata.yaml
        let metadata_path = find_metadata_file(&current_dir, runtime);

        if let Some(path) = metadata_path {
            // Parse the metadata file
            let content = runtime.file_read_string(&path).map_err(|e| {
                QuartoError::Other(format!("Failed to read {}: {}", path.display(), e))
            })?;

            let filename = path.to_string_lossy().to_string();
            let yaml = quarto_yaml::parse_file(&content, &filename).map_err(|e| {
                QuartoError::Other(format!(
                    "Directory metadata validation failed for {}: {}",
                    path.display(),
                    e
                ))
            })?;

            // Convert to ConfigValue with ProjectConfig interpretation context
            let mut diagnostics = DiagnosticCollector::new();
            let mut metadata =
                yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);

            // Adjust !path values to be relative to document directory
            adjust_paths_to_document_dir(&mut metadata, &current_dir, document_dir);

            layers.push(metadata);
        }
    }

    Ok(layers)
}

/// Find `_metadata.yml` or `_metadata.yaml` in a directory.
///
/// Returns the path to the metadata file if found, preferring `.yml` over `.yaml`.
fn find_metadata_file(dir: &Path, runtime: &dyn SystemRuntime) -> Option<PathBuf> {
    let yml_path = dir.join("_metadata.yml");
    if runtime.is_file(&yml_path).unwrap_or(false) {
        return Some(yml_path);
    }

    let yaml_path = dir.join("_metadata.yaml");
    if runtime.is_file(&yaml_path).unwrap_or(false) {
        return Some(yaml_path);
    }

    None
}

/// Adjust `!path` values in metadata to be relative to document directory.
///
/// Walks the ConfigValue tree and for each `ConfigValueKind::Path`:
/// - Computes absolute path relative to metadata_dir
/// - Recomputes relative path from document_dir
///
/// Leaves other values (strings, globs, etc.) unchanged.
pub(crate) fn adjust_paths_to_document_dir(
    metadata: &mut ConfigValue,
    metadata_dir: &Path,
    document_dir: &Path,
) {
    adjust_paths_recursive(metadata, metadata_dir, document_dir);
}

/// Recursively walk ConfigValue, adjusting Path variants.
fn adjust_paths_recursive(value: &mut ConfigValue, metadata_dir: &Path, document_dir: &Path) {
    match &mut value.value {
        ConfigValueKind::Path(path_str) => {
            let path = PathBuf::from(&*path_str);
            // Only adjust relative paths (not absolute, not URLs)
            if path.is_relative()
                && !path_str.starts_with("http://")
                && !path_str.starts_with("https://")
            {
                let abs_path = metadata_dir.join(&path);
                if let Some(adjusted) = pathdiff::diff_paths(&abs_path, document_dir) {
                    *path_str = adjusted.to_string_lossy().into_owned();
                }
            }
        }
        ConfigValueKind::Array(items) => {
            for item in items {
                adjust_paths_recursive(item, metadata_dir, document_dir);
            }
        }
        ConfigValueKind::Map(entries) => {
            for entry in entries {
                adjust_paths_recursive(&mut entry.value, metadata_dir, document_dir);
            }
        }
        // All other kinds (Scalar, PandocInlines, Glob, Expr, etc.) - no adjustment
        _ => {}
    }
}

/// Project type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectType {
    /// Default project (individual documents)
    #[default]
    Default,
    /// Website project
    Website,
    /// Book project
    Book,
    /// Manuscript project
    Manuscript,
}

impl ProjectType {
    /// Get the project type name
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectType::Default => "default",
            ProjectType::Website => "website",
            ProjectType::Book => "book",
            ProjectType::Manuscript => "manuscript",
        }
    }
}

impl TryFrom<&str> for ProjectType {
    type Error = String;

    fn try_from(s: &str) -> std::result::Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "default" => Ok(ProjectType::Default),
            "website" => Ok(ProjectType::Website),
            "book" => Ok(ProjectType::Book),
            "manuscript" => Ok(ProjectType::Manuscript),
            _ => Err(format!("Unknown project type: {}", s)),
        }
    }
}

/// Parsed project configuration from `_quarto.yml`
#[derive(Debug, Clone, Default)]
pub struct ProjectConfig {
    /// Project type
    pub project_type: ProjectType,

    /// Output directory (relative to project root)
    pub output_dir: Option<PathBuf>,

    /// Input file patterns (glob patterns)
    pub render_patterns: Vec<String>,

    /// Full project metadata as ConfigValue with source tracking.
    ///
    /// This is the entire `_quarto.yml` parsed with `InterpretationContext::ProjectConfig`,
    /// meaning strings are kept literal by default (no markdown parsing).
    ///
    /// Used by the render pipeline to merge project-level settings with document metadata.
    /// Format-specific settings (e.g., `format.html.toc`) are extracted using
    /// `quarto_config::resolve_format_config()` before merging.
    pub metadata: Option<ConfigValue>,
}

impl ProjectConfig {
    /// Create a ProjectConfig with metadata.
    ///
    /// This is useful for programmatically creating a project config
    /// (e.g., in WASM) with specific settings.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Create project config with format settings
    /// let metadata = ConfigValue::from_path(&["format", "html", "source-location"], "full");
    /// let config = ProjectConfig::with_metadata(metadata);
    /// ```
    pub fn with_metadata(metadata: ConfigValue) -> Self {
        Self {
            metadata: Some(metadata),
            ..Default::default()
        }
    }
}

/// Information about a document to be rendered
#[derive(Debug, Clone)]
pub struct DocumentInfo {
    /// Input file path (absolute)
    pub input: PathBuf,

    /// Output file path (absolute, determined by format)
    pub output: Option<PathBuf>,

    /// Document title (from front matter, if available)
    pub title: Option<String>,

    /// Document ID (for cross-references)
    pub id: Option<String>,
}

impl DocumentInfo {
    /// Create document info from an input path
    pub fn from_path(input: impl Into<PathBuf>) -> Self {
        Self {
            input: input.into(),
            output: None,
            title: None,
            id: None,
        }
    }

    /// Set the output path
    pub fn with_output(mut self, output: impl Into<PathBuf>) -> Self {
        self.output = Some(output.into());
        self
    }

    /// Get the file name without extension
    pub fn stem(&self) -> Option<&str> {
        self.input.file_stem().and_then(|s| s.to_str())
    }
}

/// Project context for rendering
#[derive(Debug, Clone)]
pub struct ProjectContext {
    /// Project root directory (directory containing `_quarto.yml`, or input file directory)
    pub dir: PathBuf,

    /// Parsed project configuration.
    ///
    /// Always present: real projects get their parsed `_quarto.yml`,
    /// single-file renders get `ProjectConfig::default()`.
    pub config: ProjectConfig,

    /// Is this a single-file pseudo-project?
    pub is_single_file: bool,

    /// List of input files to render
    pub files: Vec<DocumentInfo>,

    /// Output directory (resolved, absolute path)
    pub output_dir: PathBuf,
}

impl ProjectContext {
    /// Discover project context from a path.
    ///
    /// If the path is a file, looks for `_quarto.yml` in parent directories.
    /// If the path is a directory, looks for `_quarto.yml` in that directory and parents.
    ///
    /// If no `_quarto.yml` is found, creates a single-file pseudo-project.
    pub fn discover(path: impl AsRef<Path>, runtime: &dyn SystemRuntime) -> Result<Self> {
        let path = path.as_ref();

        // Canonicalize the path
        let path = runtime
            .canonicalize(path)
            .map_err(|e| QuartoError::Other(format!("Failed to canonicalize path: {}", e)))?;

        // Determine if this is a file or directory
        let is_file = runtime
            .is_file(&path)
            .map_err(|e| QuartoError::Other(format!("Failed to check path type: {}", e)))?;
        let is_dir = runtime
            .is_dir(&path)
            .map_err(|e| QuartoError::Other(format!("Failed to check path type: {}", e)))?;

        let (search_dir, input_file) = if is_file {
            (
                path.parent()
                    .ok_or_else(|| QuartoError::Other("Input file has no parent directory".into()))?
                    .to_path_buf(),
                Some(path.clone()),
            )
        } else if is_dir {
            (path.clone(), None)
        } else {
            return Err(QuartoError::Other(format!(
                "Path does not exist: {}",
                path.display()
            )));
        };

        // Search for _quarto.yml
        let (project_dir, config) = Self::find_project_config(&search_dir, runtime)?;

        // Determine if this is a single-file project
        let is_single_file = config.is_none() && input_file.is_some();

        // Use project dir if found, otherwise use search dir
        let dir = project_dir.unwrap_or(search_dir);

        // Determine output directory
        let output_dir = config
            .as_ref()
            .and_then(|c| c.output_dir.as_ref())
            .map_or_else(|| dir.clone(), |o| dir.join(o));

        // Build file list
        let files = if let Some(input) = input_file {
            vec![DocumentInfo::from_path(input)]
        } else {
            // TODO: Discover files based on project configuration
            Vec::new()
        };

        Ok(Self {
            dir,
            config: config.unwrap_or_default(),
            is_single_file,
            files,
            output_dir,
        })
    }

    /// Create a single-file project context directly
    pub fn single_file(input: impl AsRef<Path>, runtime: &dyn SystemRuntime) -> Result<Self> {
        let input = input.as_ref();

        let input = runtime
            .canonicalize(input)
            .map_err(|e| QuartoError::Other(format!("Failed to canonicalize path: {}", e)))?;

        let dir = input
            .parent()
            .ok_or_else(|| QuartoError::Other("Input file has no parent directory".into()))?
            .to_path_buf();

        Ok(Self {
            dir: dir.clone(),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path(input)],
            output_dir: dir,
        })
    }

    /// Search for `_quarto.yml` in directory and parents
    fn find_project_config(
        start_dir: &Path,
        runtime: &dyn SystemRuntime,
    ) -> Result<(Option<PathBuf>, Option<ProjectConfig>)> {
        let mut current = start_dir.to_path_buf();

        loop {
            let config_path = current.join("_quarto.yml");
            let exists = runtime
                .path_exists(&config_path, None)
                .map_err(|e| QuartoError::Other(format!("Failed to check config path: {}", e)))?;
            if exists {
                // Found config file - parse it
                let config = Self::parse_config(&config_path, runtime)?;
                return Ok((Some(current), Some(config)));
            }

            // Also check for _quarto.yaml (alternate extension)
            let config_path_yaml = current.join("_quarto.yaml");
            let exists_yaml = runtime
                .path_exists(&config_path_yaml, None)
                .map_err(|e| QuartoError::Other(format!("Failed to check config path: {}", e)))?;
            if exists_yaml {
                let config = Self::parse_config(&config_path_yaml, runtime)?;
                return Ok((Some(current), Some(config)));
            }

            // Move to parent directory
            if let Some(parent) = current.parent() {
                current = parent.to_path_buf();
            } else {
                // Reached root, no config found
                return Ok((None, None));
            }
        }
    }

    /// Parse a `_quarto.yml` file
    fn parse_config(path: &Path, runtime: &dyn SystemRuntime) -> Result<ProjectConfig> {
        use pampa::pandoc::yaml_to_config_value;
        use pampa::utils::diagnostic_collector::DiagnosticCollector;
        use quarto_config::InterpretationContext;

        let content = runtime
            .file_read_string(path)
            .map_err(|e| QuartoError::Other(format!("Failed to read config file: {}", e)))?;

        let filename = path.to_string_lossy().to_string();

        // Parse YAML with source tracking
        let yaml = quarto_yaml::parse_file(&content, &filename).map_err(|e| {
            QuartoError::Other(format!("Failed to parse {}: {}", path.display(), e))
        })?;

        // Convert to ConfigValue with ProjectConfig interpretation context
        // (strings are kept literal, not parsed as markdown)
        let mut diagnostics = DiagnosticCollector::new();
        let metadata =
            yaml_to_config_value(yaml, InterpretationContext::ProjectConfig, &mut diagnostics);

        // Extract project-specific settings from metadata
        let project_type = metadata
            .get("project")
            .and_then(|p| p.get("type"))
            .and_then(|t| t.as_str())
            .and_then(|s| ProjectType::try_from(s).ok())
            .unwrap_or_default();

        let output_dir = metadata
            .get("project")
            .and_then(|p| p.get("output-dir"))
            .and_then(|o| o.as_str())
            .map(PathBuf::from);

        let render_patterns = metadata
            .get("project")
            .and_then(|p| p.get("render"))
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(ProjectConfig {
            project_type,
            output_dir,
            render_patterns,
            metadata: Some(metadata),
        })
    }

    /// Get the project type
    pub fn project_type(&self) -> ProjectType {
        self.config.project_type
    }

    /// Check if this is a multi-document project
    pub fn is_multi_document(&self) -> bool {
        !self.is_single_file
            && matches!(
                self.project_type(),
                ProjectType::Website | ProjectType::Book | ProjectType::Manuscript
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === ProjectType tests ===

    #[test]
    fn test_project_type_from_string() {
        assert_eq!(
            ProjectType::try_from("website").unwrap(),
            ProjectType::Website
        );
        assert_eq!(ProjectType::try_from("book").unwrap(), ProjectType::Book);
        assert_eq!(
            ProjectType::try_from("default").unwrap(),
            ProjectType::Default
        );
        assert!(ProjectType::try_from("unknown").is_err());
    }

    #[test]
    fn test_project_type_from_string_manuscript() {
        assert_eq!(
            ProjectType::try_from("manuscript").unwrap(),
            ProjectType::Manuscript
        );
    }

    #[test]
    fn test_project_type_from_string_case_insensitive() {
        // Test uppercase
        assert_eq!(
            ProjectType::try_from("WEBSITE").unwrap(),
            ProjectType::Website
        );
        assert_eq!(ProjectType::try_from("BOOK").unwrap(), ProjectType::Book);
        assert_eq!(
            ProjectType::try_from("DEFAULT").unwrap(),
            ProjectType::Default
        );
        assert_eq!(
            ProjectType::try_from("MANUSCRIPT").unwrap(),
            ProjectType::Manuscript
        );

        // Test mixed case
        assert_eq!(
            ProjectType::try_from("WebSite").unwrap(),
            ProjectType::Website
        );
        assert_eq!(ProjectType::try_from("Book").unwrap(), ProjectType::Book);
    }

    #[test]
    fn test_project_type_from_string_error_message() {
        let result = ProjectType::try_from("invalid");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unknown project type"));
        assert!(err.contains("invalid"));
    }

    #[test]
    fn test_project_type_as_str() {
        assert_eq!(ProjectType::Default.as_str(), "default");
        assert_eq!(ProjectType::Website.as_str(), "website");
        assert_eq!(ProjectType::Book.as_str(), "book");
        assert_eq!(ProjectType::Manuscript.as_str(), "manuscript");
    }

    #[test]
    fn test_project_type_default() {
        let default_type: ProjectType = Default::default();
        assert_eq!(default_type, ProjectType::Default);
    }

    #[test]
    fn test_project_type_clone_and_copy() {
        let original = ProjectType::Website;
        let cloned = original.clone();
        let copied = original; // Copy trait
        assert_eq!(original, cloned);
        assert_eq!(original, copied);
    }

    #[test]
    fn test_project_type_eq() {
        assert_eq!(ProjectType::Website, ProjectType::Website);
        assert_ne!(ProjectType::Website, ProjectType::Book);
    }

    // === ProjectConfig tests ===

    #[test]
    fn test_project_config_default() {
        let config = ProjectConfig::default();
        assert_eq!(config.project_type, ProjectType::Default);
        assert!(config.output_dir.is_none());
        assert!(config.render_patterns.is_empty());
        assert!(config.metadata.is_none());
    }

    #[test]
    fn test_project_config_with_metadata() {
        use quarto_pandoc_types::ConfigValue;
        use quarto_source_map::SourceInfo;

        let metadata = ConfigValue::new_string("test", SourceInfo::default());
        let config = ProjectConfig::with_metadata(metadata.clone());

        assert_eq!(config.project_type, ProjectType::Default);
        assert!(config.output_dir.is_none());
        assert!(config.render_patterns.is_empty());
        assert!(config.metadata.is_some());
    }

    // === DocumentInfo tests ===

    #[test]
    fn test_document_info() {
        let doc = DocumentInfo::from_path("/path/to/doc.qmd").with_output("/path/to/doc.html");

        assert_eq!(doc.input, PathBuf::from("/path/to/doc.qmd"));
        assert_eq!(doc.output, Some(PathBuf::from("/path/to/doc.html")));
        assert_eq!(doc.stem(), Some("doc"));
    }

    #[test]
    fn test_document_info_from_path_only() {
        let doc = DocumentInfo::from_path("/path/to/file.qmd");

        assert_eq!(doc.input, PathBuf::from("/path/to/file.qmd"));
        assert!(doc.output.is_none());
        assert!(doc.title.is_none());
        assert!(doc.id.is_none());
    }

    #[test]
    fn test_document_info_stem_no_extension() {
        let doc = DocumentInfo::from_path("/path/to/README");
        assert_eq!(doc.stem(), Some("README"));
    }

    #[test]
    fn test_document_info_stem_hidden_file() {
        let doc = DocumentInfo::from_path("/path/to/.gitignore");
        assert_eq!(doc.stem(), Some(".gitignore"));
    }

    #[test]
    fn test_document_info_stem_multiple_dots() {
        let doc = DocumentInfo::from_path("/path/to/file.test.qmd");
        assert_eq!(doc.stem(), Some("file.test"));
    }

    #[test]
    fn test_document_info_with_output_chaining() {
        let doc = DocumentInfo::from_path("/input.qmd").with_output("/output.html");

        assert_eq!(doc.input, PathBuf::from("/input.qmd"));
        assert_eq!(doc.output, Some(PathBuf::from("/output.html")));
    }

    #[test]
    fn test_document_info_clone() {
        let doc = DocumentInfo::from_path("/path/to/doc.qmd").with_output("/path/to/doc.html");
        let cloned = doc.clone();

        assert_eq!(doc.input, cloned.input);
        assert_eq!(doc.output, cloned.output);
    }

    // === ProjectContext tests (unit tests for methods that don't need runtime) ===

    #[test]
    fn test_project_context_project_type_with_config() {
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_type: ProjectType::Website,
                ..Default::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project/_site"),
        };

        assert_eq!(context.project_type(), ProjectType::Website);
    }

    #[test]
    fn test_project_context_project_type_without_config() {
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };

        assert_eq!(context.project_type(), ProjectType::Default);
    }

    #[test]
    fn test_project_context_is_multi_document_website() {
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_type: ProjectType::Website,
                ..Default::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project/_site"),
        };

        assert!(context.is_multi_document());
    }

    #[test]
    fn test_project_context_is_multi_document_book() {
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_type: ProjectType::Book,
                ..Default::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project/_book"),
        };

        assert!(context.is_multi_document());
    }

    #[test]
    fn test_project_context_is_multi_document_manuscript() {
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_type: ProjectType::Manuscript,
                ..Default::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project/_manuscript"),
        };

        assert!(context.is_multi_document());
    }

    #[test]
    fn test_project_context_is_multi_document_default_type() {
        // Default project type is NOT multi-document
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_type: ProjectType::Default,
                ..Default::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };

        assert!(!context.is_multi_document());
    }

    #[test]
    fn test_project_context_is_multi_document_single_file() {
        // Single file projects are never multi-document, even if type is Website
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig {
                project_type: ProjectType::Website,
                ..Default::default()
            },
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/index.qmd")],
            output_dir: PathBuf::from("/project"),
        };

        assert!(!context.is_multi_document());
    }

    #[test]
    fn test_project_context_is_multi_document_no_config() {
        // No config means single-file pseudo-project, not multi-document
        let context = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };

        assert!(!context.is_multi_document());
    }

    // === ProjectContext::discover and ::single_file tests ===

    mod discover_tests {
        use super::*;
        use quarto_system_runtime::NativeRuntime;
        use std::fs;
        use tempfile::TempDir;

        #[test]
        fn test_discover_without_quarto_yml_has_default_config() {
            // A path with no _quarto.yml should get a default config, not None
            let temp = TempDir::new().unwrap();
            let qmd_path = temp.path().join("doc.qmd");
            fs::write(&qmd_path, "# Hello\n").unwrap();

            let runtime = NativeRuntime::new();
            let ctx = ProjectContext::discover(&qmd_path, &runtime).unwrap();

            assert!(ctx.is_single_file);
            // Config should be default with no metadata
            assert_eq!(ctx.config.project_type, ProjectType::Default);
            assert!(ctx.config.metadata.is_none());
        }

        #[test]
        fn test_single_file_has_default_config() {
            let temp = TempDir::new().unwrap();
            let qmd_path = temp.path().join("doc.qmd");
            fs::write(&qmd_path, "# Hello\n").unwrap();

            let runtime = NativeRuntime::new();
            let ctx = ProjectContext::single_file(&qmd_path, &runtime).unwrap();

            assert!(ctx.is_single_file);
            // Config should be default with no metadata
            assert_eq!(ctx.config.project_type, ProjectType::Default);
            assert!(ctx.config.metadata.is_none());
        }
    }

    // === Directory Metadata tests ===

    mod directory_metadata_tests {
        use super::*;
        use quarto_system_runtime::NativeRuntime;
        use std::fs;
        use tempfile::TempDir;

        /// Helper to create a project context for testing.
        /// Canonicalizes the dir to match what ProjectContext::discover does,
        /// ensuring strip_prefix works correctly (e.g., on macOS where
        /// /tmp symlinks to /private/tmp).
        fn test_project_context(dir: &Path) -> ProjectContext {
            let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
            ProjectContext {
                dir: canonical.clone(),
                config: ProjectConfig::default(),
                is_single_file: false,
                files: vec![],
                output_dir: canonical,
            }
        }

        fn native_runtime() -> NativeRuntime {
            NativeRuntime::new()
        }

        #[test]
        fn test_directory_metadata_empty() {
            // Project with no _metadata.yml files returns empty vec
            let temp = TempDir::new().unwrap();
            let project = test_project_context(temp.path());
            let doc_path = temp.path().join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert!(result.is_empty());
        }

        #[test]
        fn test_directory_metadata_single_file_in_subdir() {
            // project/
            //   chapters/
            //     _metadata.yml  { toc: true }
            //     doc.qmd
            // Returns: [{ toc: true }]
            let temp = TempDir::new().unwrap();
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "toc: true\n").unwrap();
            fs::write(chapters.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = chapters.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            assert_eq!(result[0].get("toc").unwrap().as_bool(), Some(true));
        }

        #[test]
        fn test_directory_metadata_hierarchy() {
            // project/
            //   _metadata.yml     { theme: "cosmo" }
            //   chapters/
            //     _metadata.yml   { toc: true }
            //     intro/
            //       _metadata.yml { toc-depth: 2 }
            //       doc.qmd
            // Returns: [{ theme }, { toc }, { toc-depth }] in order
            let temp = TempDir::new().unwrap();

            // Root _metadata.yml - NOTE: TS Quarto walks from first subdir, not root
            // But we should include root if document is in subdir
            fs::write(temp.path().join("_metadata.yml"), "theme: cosmo\n").unwrap();

            // chapters/_metadata.yml
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "toc: true\n").unwrap();

            // chapters/intro/_metadata.yml
            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("_metadata.yml"), "toc-depth: 2\n").unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            // Should have 3 layers (root is NOT included based on TS behavior)
            // Actually, re-reading TS code: it walks from projectDir to inputDir
            // using relativePath.split(SEP_PATTERN), so if doc is in chapters/intro,
            // relativePath is "chapters/intro", split gives ["chapters", "intro"]
            // and it joins from projectDir: project/chapters, project/chapters/intro
            // So root is NOT included. Let me verify this...
            //
            // Wait, the TS code starts with currentDir = projectDir, then does:
            //   currentDir = join(currentDir, dir) for each dir in dirs
            // So if dirs = ["chapters", "intro"], it processes:
            //   project/chapters, project/chapters/intro
            // Root (project/) is NOT processed.
            //
            // So our test should expect 2 layers, not 3.
            assert_eq!(result.len(), 2);
            assert_eq!(result[0].get("toc").unwrap().as_bool(), Some(true));
            assert_eq!(result[1].get("toc-depth").unwrap().as_int(), Some(2));
        }

        #[test]
        fn test_directory_metadata_skips_missing() {
            // project/
            //   _metadata.yml     { theme: "cosmo" } -- not included (root)
            //   chapters/
            //     intro/          # No _metadata.yml here
            //       deep/
            //         _metadata.yml { toc: true }
            //         doc.qmd
            // Returns: [{ toc }] - skips chapters/ and intro/
            let temp = TempDir::new().unwrap();

            fs::write(temp.path().join("_metadata.yml"), "theme: cosmo\n").unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            // No _metadata.yml in chapters/

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            // No _metadata.yml in intro/

            let deep = intro.join("deep");
            fs::create_dir(&deep).unwrap();
            fs::write(deep.join("_metadata.yml"), "toc: true\n").unwrap();
            fs::write(deep.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = deep.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            // Only the deep/_metadata.yml should be found
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].get("toc").unwrap().as_bool(), Some(true));
        }

        #[test]
        fn test_directory_metadata_yaml_extension() {
            // Test that _metadata.yaml (not just .yml) is recognized
            let temp = TempDir::new().unwrap();
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yaml"), "toc: true\n").unwrap();
            fs::write(chapters.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = chapters.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            assert_eq!(result[0].get("toc").unwrap().as_bool(), Some(true));
        }

        #[test]
        fn test_directory_metadata_invalid_yaml_fails() {
            // _metadata.yml with YAML syntax error should fail
            let temp = TempDir::new().unwrap();
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "invalid: yaml: : syntax\n").unwrap();
            fs::write(chapters.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = chapters.join("doc.qmd");

            let result = directory_metadata_for_document(&project, &doc_path, &native_runtime());

            assert!(result.is_err());
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("metadata") || err.contains("parse") || err.contains("yaml"),
                "Error should mention metadata/parse/yaml: {}",
                err
            );
        }

        #[test]
        fn test_directory_metadata_document_at_root() {
            // Document directly in project root should return empty vec
            // (no directories to walk)
            let temp = TempDir::new().unwrap();
            fs::write(temp.path().join("_metadata.yml"), "toc: true\n").unwrap();
            fs::write(temp.path().join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = temp.path().join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            // Document at root means relativePath is "", dirs is empty or [""]
            // TS behavior: no directories to process, returns empty config
            assert!(result.is_empty());
        }

        #[test]
        fn test_directory_metadata_single_file_project() {
            // Single-file project (default config) should return empty vec
            let temp = TempDir::new().unwrap();
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "toc: true\n").unwrap();
            fs::write(chapters.join("doc.qmd"), "# Test\n").unwrap();

            // Single-file project has default config
            let project = ProjectContext {
                dir: temp.path().to_path_buf(),
                config: ProjectConfig::default(),
                is_single_file: true,
                files: vec![],
                output_dir: temp.path().to_path_buf(),
            };
            let doc_path = chapters.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            // Per TS behavior: directory metadata requires project context
            assert!(result.is_empty());
        }

        // === Path adjustment tests ===
        //
        // These tests verify that `!path` values in _metadata.yml are adjusted
        // to be relative to the document directory, not the metadata file directory.

        #[test]
        fn test_path_adjusted_for_subdirectory() {
            // project/
            //   shared/
            //     styles.css        # The actual file (not required to exist)
            //   chapters/
            //     _metadata.yml     # css: !path ../shared/styles.css
            //     intro/
            //       doc.qmd
            //
            // When rendering doc.qmd, css should become "../../shared/styles.css"
            let temp = TempDir::new().unwrap();

            // Create shared directory (file doesn't need to exist)
            let shared = temp.path().join("shared");
            fs::create_dir(&shared).unwrap();

            // Create chapters/_metadata.yml with a !path value
            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(
                chapters.join("_metadata.yml"),
                "css: !path ../shared/styles.css\n",
            )
            .unwrap();

            // Create chapters/intro/doc.qmd
            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let css_value = result[0].get("css").expect("should have css key");

            // The path should be adjusted from ../shared/styles.css to ../../shared/styles.css
            // because we went one directory deeper (chapters/intro instead of chapters/)
            assert_eq!(
                css_value.as_str(),
                Some("../../shared/styles.css"),
                "Path should be adjusted relative to document directory"
            );
        }

        #[test]
        fn test_path_same_directory_unchanged() {
            // project/
            //   chapters/
            //     _metadata.yml     # css: !path ./local.css
            //     doc.qmd           # Same directory
            //
            // Path stays "./local.css" (or normalized equivalent)
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "css: !path ./local.css\n").unwrap();
            fs::write(chapters.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = chapters.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let css_value = result[0].get("css").expect("should have css key");

            // Path should remain equivalent (pathdiff may normalize ./local.css to local.css)
            let path_str = css_value.as_str().expect("should be a string path");
            assert!(
                path_str == "./local.css" || path_str == "local.css",
                "Path should stay relative to same directory: got '{}'",
                path_str
            );
        }

        #[test]
        fn test_plain_string_not_adjusted() {
            // project/
            //   chapters/
            //     _metadata.yml     # theme: cosmo (plain string, not !path)
            //     intro/
            //       doc.qmd
            //
            // "cosmo" must NOT be changed to "../cosmo" or anything else
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(chapters.join("_metadata.yml"), "theme: cosmo\n").unwrap();

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let theme_value = result[0].get("theme").expect("should have theme key");

            // Plain string should NOT be adjusted
            assert_eq!(
                theme_value.as_str(),
                Some("cosmo"),
                "Plain strings should not be adjusted"
            );
        }

        #[test]
        fn test_absolute_path_unchanged() {
            // css: !path /usr/share/styles/base.css
            // Should pass through unchanged
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(
                chapters.join("_metadata.yml"),
                "css: !path /usr/share/styles/base.css\n",
            )
            .unwrap();

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let css_value = result[0].get("css").expect("should have css key");

            // Absolute path should be unchanged
            assert_eq!(
                css_value.as_str(),
                Some("/usr/share/styles/base.css"),
                "Absolute paths should not be adjusted"
            );
        }

        #[test]
        fn test_array_of_paths_all_adjusted() {
            // css:
            //   - !path ../shared/a.css
            //   - !path ../shared/b.css
            // Both should be adjusted
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(
                chapters.join("_metadata.yml"),
                "css:\n  - !path ../shared/a.css\n  - !path ../shared/b.css\n",
            )
            .unwrap();

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let css_array = result[0]
                .get("css")
                .expect("should have css key")
                .as_array()
                .expect("css should be an array");

            assert_eq!(css_array.len(), 2);
            assert_eq!(
                css_array[0].as_str(),
                Some("../../shared/a.css"),
                "First path should be adjusted"
            );
            assert_eq!(
                css_array[1].as_str(),
                Some("../../shared/b.css"),
                "Second path should be adjusted"
            );
        }

        #[test]
        fn test_glob_not_adjusted() {
            // resources: !glob ../images/*.png
            // Globs are patterns, not paths - should NOT be adjusted
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(
                chapters.join("_metadata.yml"),
                "resources: !glob ../images/*.png\n",
            )
            .unwrap();

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let resources = result[0]
                .get("resources")
                .expect("should have resources key");

            // Glob should NOT be adjusted (globs need separate handling)
            assert_eq!(
                resources.as_str(),
                Some("../images/*.png"),
                "Globs should not be adjusted"
            );
        }

        #[test]
        fn test_nested_map_path_adjusted() {
            // Test that paths nested in maps are also adjusted
            // format:
            //   html:
            //     css: !path ../shared/styles.css
            let temp = TempDir::new().unwrap();

            let chapters = temp.path().join("chapters");
            fs::create_dir(&chapters).unwrap();
            fs::write(
                chapters.join("_metadata.yml"),
                "format:\n  html:\n    css: !path ../shared/styles.css\n",
            )
            .unwrap();

            let intro = chapters.join("intro");
            fs::create_dir(&intro).unwrap();
            fs::write(intro.join("doc.qmd"), "# Test\n").unwrap();

            let project = test_project_context(temp.path());
            let doc_path = intro.join("doc.qmd");

            let result =
                directory_metadata_for_document(&project, &doc_path, &native_runtime()).unwrap();

            assert_eq!(result.len(), 1);
            let css_value = result[0]
                .get("format")
                .and_then(|f| f.get("html"))
                .and_then(|h| h.get("css"))
                .expect("should have format.html.css");

            assert_eq!(
                css_value.as_str(),
                Some("../../shared/styles.css"),
                "Nested path should be adjusted"
            );
        }
    }
}
