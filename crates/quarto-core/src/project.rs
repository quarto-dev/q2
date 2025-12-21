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

use crate::error::{QuartoError, Result};

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

    /// Raw configuration value for format-specific settings
    pub raw: serde_json::Value,
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
#[derive(Debug)]
pub struct ProjectContext {
    /// Project root directory (directory containing `_quarto.yml`, or input file directory)
    pub dir: PathBuf,

    /// Parsed project configuration (if `_quarto.yml` exists)
    pub config: Option<ProjectConfig>,

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
    pub fn discover(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Canonicalize the path
        let path = path
            .canonicalize()
            .map_err(|e| QuartoError::Io(e))?;

        // Determine if this is a file or directory
        let (search_dir, input_file) = if path.is_file() {
            (
                path.parent()
                    .ok_or_else(|| QuartoError::Other("Input file has no parent directory".into()))?
                    .to_path_buf(),
                Some(path.clone()),
            )
        } else if path.is_dir() {
            (path.clone(), None)
        } else {
            return Err(QuartoError::Other(format!(
                "Path does not exist: {}",
                path.display()
            )));
        };

        // Search for _quarto.yml
        let (project_dir, config) = Self::find_project_config(&search_dir)?;

        // Determine if this is a single-file project
        let is_single_file = config.is_none() && input_file.is_some();

        // Use project dir if found, otherwise use search dir
        let dir = project_dir.unwrap_or(search_dir);

        // Determine output directory
        let output_dir = config
            .as_ref()
            .and_then(|c| c.output_dir.as_ref())
            .map(|o| dir.join(o))
            .unwrap_or_else(|| dir.clone());

        // Build file list
        let files = if let Some(input) = input_file {
            vec![DocumentInfo::from_path(input)]
        } else {
            // TODO: Discover files based on project configuration
            Vec::new()
        };

        Ok(Self {
            dir,
            config,
            is_single_file,
            files,
            output_dir,
        })
    }

    /// Create a single-file project context directly
    pub fn single_file(input: impl AsRef<Path>) -> Result<Self> {
        let input = input.as_ref();

        let input = input
            .canonicalize()
            .map_err(|e| QuartoError::Io(e))?;

        let dir = input
            .parent()
            .ok_or_else(|| QuartoError::Other("Input file has no parent directory".into()))?
            .to_path_buf();

        Ok(Self {
            dir: dir.clone(),
            config: None,
            is_single_file: true,
            files: vec![DocumentInfo::from_path(input)],
            output_dir: dir,
        })
    }

    /// Search for `_quarto.yml` in directory and parents
    fn find_project_config(start_dir: &Path) -> Result<(Option<PathBuf>, Option<ProjectConfig>)> {
        let mut current = start_dir.to_path_buf();

        loop {
            let config_path = current.join("_quarto.yml");
            if config_path.exists() {
                // Found config file - parse it
                let config = Self::parse_config(&config_path)?;
                return Ok((Some(current), Some(config)));
            }

            // Also check for _quarto.yaml (alternate extension)
            let config_path_yaml = current.join("_quarto.yaml");
            if config_path_yaml.exists() {
                let config = Self::parse_config(&config_path_yaml)?;
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
    fn parse_config(path: &Path) -> Result<ProjectConfig> {
        let content = std::fs::read_to_string(path).map_err(QuartoError::Io)?;

        // Parse YAML
        let value: serde_json::Value = serde_yaml::from_str(&content)
            .map_err(|e| QuartoError::Other(format!("Failed to parse {}: {}", path.display(), e)))?;

        // Extract project configuration
        let project = value.get("project").cloned().unwrap_or(serde_json::Value::Null);

        let project_type = project
            .get("type")
            .and_then(|v| v.as_str())
            .and_then(|s| ProjectType::try_from(s).ok())
            .unwrap_or_default();

        let output_dir = project
            .get("output-dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        let render_patterns = project
            .get("render")
            .and_then(|v| v.as_array())
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
            raw: value,
        })
    }

    /// Get the project type
    pub fn project_type(&self) -> ProjectType {
        self.config
            .as_ref()
            .map(|c| c.project_type)
            .unwrap_or_default()
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

    #[test]
    fn test_project_type_from_string() {
        assert_eq!(ProjectType::try_from("website").unwrap(), ProjectType::Website);
        assert_eq!(ProjectType::try_from("book").unwrap(), ProjectType::Book);
        assert_eq!(ProjectType::try_from("default").unwrap(), ProjectType::Default);
        assert!(ProjectType::try_from("unknown").is_err());
    }

    #[test]
    fn test_document_info() {
        let doc = DocumentInfo::from_path("/path/to/doc.qmd")
            .with_output("/path/to/doc.html");

        assert_eq!(doc.input, PathBuf::from("/path/to/doc.qmd"));
        assert_eq!(doc.output, Some(PathBuf::from("/path/to/doc.html")));
        assert_eq!(doc.stem(), Some("doc"));
    }
}
