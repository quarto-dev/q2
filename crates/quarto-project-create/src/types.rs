/*
 * types.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Core types for Quarto project creation.
 */

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Error type for project creation operations.
#[derive(Debug, Error)]
pub enum CreateError {
    /// Template rendering failed
    #[error("Template rendering failed: {0}")]
    TemplateRender(String),

    /// Invalid project configuration
    #[error("Invalid project configuration: {0}")]
    InvalidConfig(String),

    /// Unknown project type
    #[error("Unknown project type: {0}")]
    UnknownProjectType(String),
}

/// Type of Quarto project to create.
///
/// Each project type has a different set of scaffold files and configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    /// Minimal project with just `_quarto.yml`
    Default,

    /// Website project with navigation and index page
    Website,

    /// Blog project (website with blog template)
    Blog,

    /// Academic manuscript
    Manuscript,

    /// Multi-chapter book
    Book,
}

impl ProjectType {
    /// Get the display name for this project type.
    pub fn display_name(&self) -> &'static str {
        match self {
            ProjectType::Default => "Default",
            ProjectType::Website => "Website",
            ProjectType::Blog => "Blog",
            ProjectType::Manuscript => "Manuscript",
            ProjectType::Book => "Book",
        }
    }

    /// Get the lowercase identifier for this project type.
    pub fn id(&self) -> &'static str {
        match self {
            ProjectType::Default => "default",
            ProjectType::Website => "website",
            ProjectType::Blog => "blog",
            ProjectType::Manuscript => "manuscript",
            ProjectType::Book => "book",
        }
    }

    /// Parse a project type from a string identifier.
    pub fn from_id(id: &str) -> Result<Self, CreateError> {
        match id.to_lowercase().as_str() {
            "default" => Ok(ProjectType::Default),
            "website" => Ok(ProjectType::Website),
            "blog" => Ok(ProjectType::Blog),
            "manuscript" => Ok(ProjectType::Manuscript),
            "book" => Ok(ProjectType::Book),
            _ => Err(CreateError::UnknownProjectType(id.to_string())),
        }
    }

    /// List all available project types.
    pub fn all() -> &'static [ProjectType] {
        &[
            ProjectType::Default,
            ProjectType::Website,
            ProjectType::Blog,
            ProjectType::Manuscript,
            ProjectType::Book,
        ]
    }

    /// List project types currently implemented.
    ///
    /// Some project types are defined but not yet implemented.
    pub fn implemented() -> &'static [ProjectType] {
        &[ProjectType::Default, ProjectType::Website]
    }
}

impl std::fmt::Display for ProjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl std::str::FromStr for ProjectType {
    type Err = CreateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ProjectType::from_id(s)
    }
}

/// A file to be created as part of a project.
#[derive(Debug, Clone)]
pub struct ProjectFile {
    /// Relative path within the project directory
    pub path: PathBuf,

    /// File content (already rendered from template)
    pub content: String,
}

impl ProjectFile {
    /// Create a new project file.
    pub fn new(path: impl Into<PathBuf>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
        }
    }
}

/// Options for creating a new project.
#[derive(Debug, Clone)]
pub struct CreateProjectOptions {
    /// The type of project to create
    pub project_type: ProjectType,

    /// Project title (used in `_quarto.yml` and document titles)
    pub title: String,
}

impl CreateProjectOptions {
    /// Create options for a new project.
    pub fn new(project_type: ProjectType, title: impl Into<String>) -> Self {
        Self {
            project_type,
            title: title.into(),
        }
    }
}
