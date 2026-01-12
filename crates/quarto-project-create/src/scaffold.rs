/*
 * scaffold.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Declarative file specification for project scaffolding.
 *
 * This module provides a data-driven approach to defining project files,
 * ported from TypeScript Quarto's ScaffoldFile pattern. It supports:
 *
 * - **Template files**: EJS templates that are rendered with project data
 * - **Supporting files**: Static resources (binary or text) that are copied as-is
 *
 * # Binary File Handling
 *
 * Binary files (images, fonts, etc.) are embedded at compile time and
 * represented as `&'static [u8]`. When creating a project, these are
 * returned as `ScaffoldedFile::Binary` with the raw bytes and MIME type.
 *
 * The hub-client can then convert these to Automerge binary documents
 * using its existing `BinaryDocumentContent` infrastructure.
 */

use crate::choices::ProjectTypeWithTemplate;
use crate::types::ProjectType;
use std::path::PathBuf;

/// Content type for a scaffold file definition.
#[derive(Debug, Clone)]
pub enum ScaffoldContent {
    /// EJS template to be rendered with project data
    Template(&'static str),

    /// Static text file to be copied as-is
    StaticText(&'static str),

    /// Static binary file to be copied as-is
    Binary {
        /// Raw binary content
        content: &'static [u8],
        /// MIME type (e.g., "image/png")
        mime_type: &'static str,
    },
}

/// A scaffold file definition.
///
/// This is the declarative specification for a file to be created.
/// It defines where the file should go and what content it should have.
#[derive(Debug, Clone)]
pub struct ScaffoldFileDef {
    /// Relative path within the project directory
    pub path: &'static str,

    /// File content (template, static text, or binary)
    pub content: ScaffoldContent,

    /// Optional subdirectory to place the file in
    /// If Some, the file will be placed in `{subdirectory}/{path}`
    pub subdirectory: Option<&'static str>,
}

impl ScaffoldFileDef {
    /// Create a new template file definition.
    pub const fn template(path: &'static str, template: &'static str) -> Self {
        Self {
            path,
            content: ScaffoldContent::Template(template),
            subdirectory: None,
        }
    }

    /// Create a new static text file definition.
    pub const fn static_text(path: &'static str, content: &'static str) -> Self {
        Self {
            path,
            content: ScaffoldContent::StaticText(content),
            subdirectory: None,
        }
    }

    /// Create a new binary file definition.
    pub const fn binary(
        path: &'static str,
        content: &'static [u8],
        mime_type: &'static str,
    ) -> Self {
        Self {
            path,
            content: ScaffoldContent::Binary { content, mime_type },
            subdirectory: None,
        }
    }

    /// Set the subdirectory for this file.
    pub const fn in_subdirectory(mut self, subdirectory: &'static str) -> Self {
        self.subdirectory = Some(subdirectory);
        self
    }

    /// Get the full path including subdirectory.
    pub fn full_path(&self) -> PathBuf {
        match self.subdirectory {
            Some(subdir) => PathBuf::from(subdir).join(self.path),
            None => PathBuf::from(self.path),
        }
    }
}

/// A scaffolded file ready to be written.
///
/// This is the result of processing a `ScaffoldFileDef` - templates have
/// been rendered, and the file is ready to be written to disk or VFS.
#[derive(Debug, Clone)]
pub enum ScaffoldedFile {
    /// A text file (rendered template or static text)
    Text {
        /// Relative path within the project directory
        path: PathBuf,
        /// File content
        content: String,
    },

    /// A binary file
    Binary {
        /// Relative path within the project directory
        path: PathBuf,
        /// Raw binary content
        content: Vec<u8>,
        /// MIME type
        mime_type: String,
    },
}

impl ScaffoldedFile {
    /// Get the path for this file.
    pub fn path(&self) -> &PathBuf {
        match self {
            ScaffoldedFile::Text { path, .. } => path,
            ScaffoldedFile::Binary { path, .. } => path,
        }
    }

    /// Check if this is a text file.
    pub fn is_text(&self) -> bool {
        matches!(self, ScaffoldedFile::Text { .. })
    }

    /// Check if this is a binary file.
    pub fn is_binary(&self) -> bool {
        matches!(self, ScaffoldedFile::Binary { .. })
    }
}

/// A project scaffold definition.
///
/// This is the complete definition for scaffolding a project type,
/// including all files to be created and any metadata.
#[derive(Debug, Clone)]
pub struct ProjectScaffold {
    /// The project type with optional template
    pub target: ProjectTypeWithTemplate,

    /// List of files to create
    pub files: Vec<ScaffoldFileDef>,
}

impl ProjectScaffold {
    /// Create a new project scaffold for a base project type.
    pub fn new(project_type: ProjectType) -> Self {
        Self {
            target: ProjectTypeWithTemplate::new(project_type),
            files: Vec::new(),
        }
    }

    /// Create a new project scaffold for a project type with template.
    pub fn with_template(project_type: ProjectType, template: &str) -> Self {
        Self {
            target: ProjectTypeWithTemplate::with_template(project_type, template),
            files: Vec::new(),
        }
    }

    /// Add a file to this scaffold.
    pub fn add_file(mut self, file: ScaffoldFileDef) -> Self {
        self.files.push(file);
        self
    }
}

/// Get the project scaffold for a given project type with optional template.
///
/// This is the main entry point for retrieving scaffold definitions.
/// It returns the list of files that should be created for the given
/// project type and template combination.
pub fn get_scaffold(target: &ProjectTypeWithTemplate) -> Option<ProjectScaffold> {
    use crate::templates;

    match target.project_type {
        ProjectType::Default => Some(ProjectScaffold::new(ProjectType::Default).add_file(
            ScaffoldFileDef::template("_quarto.yml", templates::default::QUARTO_YML),
        )),
        ProjectType::Website => {
            match target.template.as_deref() {
                None => Some(
                    ProjectScaffold::new(ProjectType::Website)
                        .add_file(ScaffoldFileDef::template(
                            "_quarto.yml",
                            templates::website::QUARTO_YML,
                        ))
                        .add_file(ScaffoldFileDef::template(
                            "index.qmd",
                            templates::website::INDEX_QMD,
                        )),
                ),
                Some("blog") => {
                    // Blog template - not yet implemented, will be added later
                    None
                }
                Some(_) => None, // Unknown template
            }
        }
        // Not yet implemented
        ProjectType::Blog | ProjectType::Manuscript | ProjectType::Book => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scaffold_file_def_template() {
        let file = ScaffoldFileDef::template("_quarto.yml", "project:\n  title: Test");
        assert_eq!(file.path, "_quarto.yml");
        assert!(matches!(file.content, ScaffoldContent::Template(_)));
        assert!(file.subdirectory.is_none());
    }

    #[test]
    fn test_scaffold_file_def_binary() {
        let png_data: &[u8] = &[0x89, 0x50, 0x4E, 0x47]; // PNG magic bytes
        let file = ScaffoldFileDef::binary("logo.png", png_data, "image/png");
        assert_eq!(file.path, "logo.png");
        assert!(matches!(
            file.content,
            ScaffoldContent::Binary {
                mime_type: "image/png",
                ..
            }
        ));
    }

    #[test]
    fn test_scaffold_file_def_subdirectory() {
        let file = ScaffoldFileDef::template("style.css", "body {}").in_subdirectory("assets");
        assert_eq!(file.full_path(), PathBuf::from("assets/style.css"));
    }

    #[test]
    fn test_get_scaffold_default() {
        let target = ProjectTypeWithTemplate::new(ProjectType::Default);
        let scaffold = get_scaffold(&target).unwrap();
        assert_eq!(scaffold.files.len(), 1);
        assert_eq!(scaffold.files[0].path, "_quarto.yml");
    }

    #[test]
    fn test_get_scaffold_website() {
        let target = ProjectTypeWithTemplate::new(ProjectType::Website);
        let scaffold = get_scaffold(&target).unwrap();
        assert_eq!(scaffold.files.len(), 2);

        let paths: Vec<_> = scaffold.files.iter().map(|f| f.path).collect();
        assert!(paths.contains(&"_quarto.yml"));
        assert!(paths.contains(&"index.qmd"));
    }

    #[test]
    fn test_get_scaffold_unknown_template() {
        let target = ProjectTypeWithTemplate::with_template(ProjectType::Website, "nonexistent");
        assert!(get_scaffold(&target).is_none());
    }

    #[test]
    fn test_scaffolded_file_text() {
        let file = ScaffoldedFile::Text {
            path: PathBuf::from("test.qmd"),
            content: "# Hello".to_string(),
        };
        assert!(file.is_text());
        assert!(!file.is_binary());
    }

    #[test]
    fn test_scaffolded_file_binary() {
        let file = ScaffoldedFile::Binary {
            path: PathBuf::from("logo.png"),
            content: vec![0x89, 0x50, 0x4E, 0x47],
            mime_type: "image/png".to_string(),
        };
        assert!(file.is_binary());
        assert!(!file.is_text());
    }
}
