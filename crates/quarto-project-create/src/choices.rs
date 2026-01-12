/*
 * choices.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Data-driven project choice infrastructure.
 *
 * This module provides the infrastructure for template aliasing and
 * user-facing project choices. The pattern is ported from TypeScript
 * Quarto's ArtifactCreator system, enabling both CLI (native) and
 * React UI (WASM) to use the same declarative project definitions.
 *
 * # Template Aliasing
 *
 * Users see friendly names like "Blog" in the UI, but internally
 * this maps to `website:blog` - a website project with the blog
 * template applied. This allows for a richer set of choices without
 * creating separate project types for each variation.
 *
 * # Architecture
 *
 * ```text
 * ProjectChoice (user-facing)
 *     ├── id: "blog"
 *     ├── name: "Blog"
 *     ├── description: "A blog using the Quarto blog template"
 *     └── ProjectTypeWithTemplate
 *           ├── project_type: Website
 *           └── template: Some("blog")
 * ```
 */

use crate::types::ProjectType;
use serde::{Deserialize, Serialize};

/// A project type with an optional template modifier.
///
/// This represents the internal form of a project choice. For example:
/// - `Website` with no template → standard website
/// - `Website` with `Some("blog")` → website with blog template
/// - `Default` with `Some("confluence")` → default project with confluence format
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTypeWithTemplate {
    /// The base project type
    pub project_type: ProjectType,

    /// Optional template modifier (e.g., "blog", "confluence")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

impl ProjectTypeWithTemplate {
    /// Create a new project type without a template modifier.
    pub fn new(project_type: ProjectType) -> Self {
        Self {
            project_type,
            template: None,
        }
    }

    /// Create a new project type with a template modifier.
    pub fn with_template(project_type: ProjectType, template: impl Into<String>) -> Self {
        Self {
            project_type,
            template: Some(template.into()),
        }
    }

    /// Parse from a string like "website" or "website:blog".
    pub fn parse(s: &str) -> Result<Self, String> {
        if let Some((type_str, template)) = s.split_once(':') {
            let project_type = ProjectType::from_id(type_str).map_err(|e| e.to_string())?;
            Ok(Self::with_template(project_type, template))
        } else {
            let project_type = ProjectType::from_id(s).map_err(|e| e.to_string())?;
            Ok(Self::new(project_type))
        }
    }

    /// Convert to the canonical string form (e.g., "website" or "website:blog").
    pub fn to_id_string(&self) -> String {
        match &self.template {
            Some(template) => format!("{}:{}", self.project_type.id(), template),
            None => self.project_type.id().to_string(),
        }
    }
}

impl std::fmt::Display for ProjectTypeWithTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_id_string())
    }
}

/// A user-facing project choice.
///
/// This is what gets displayed in UI dropdowns and CLI help text.
/// Each choice maps to a `ProjectTypeWithTemplate` internally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectChoice {
    /// Unique identifier for this choice (e.g., "blog", "website")
    pub id: String,

    /// Display name shown to users (e.g., "Blog", "Website")
    pub name: String,

    /// Short description for help text and tooltips
    pub description: String,

    /// The internal project type and template this choice maps to
    pub target: ProjectTypeWithTemplate,

    /// Whether this choice is currently implemented
    #[serde(default)]
    pub implemented: bool,
}

impl ProjectChoice {
    /// Create a new project choice.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        target: ProjectTypeWithTemplate,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            target,
            implemented: true,
        }
    }

    /// Mark this choice as not yet implemented.
    pub fn unimplemented(mut self) -> Self {
        self.implemented = false;
        self
    }
}

/// Get all available project choices.
///
/// This is the single source of truth for what project types are available
/// to users. Both CLI and UI should consume this list.
pub fn available_choices() -> Vec<ProjectChoice> {
    vec![
        ProjectChoice::new(
            "default",
            "Default",
            "A minimal Quarto project",
            ProjectTypeWithTemplate::new(ProjectType::Default),
        ),
        ProjectChoice::new(
            "website",
            "Website",
            "A Quarto website with navigation",
            ProjectTypeWithTemplate::new(ProjectType::Website),
        ),
        ProjectChoice::new(
            "blog",
            "Blog",
            "A blog using the Quarto blog template",
            ProjectTypeWithTemplate::with_template(ProjectType::Website, "blog"),
        )
        .unimplemented(),
        ProjectChoice::new(
            "manuscript",
            "Manuscript",
            "An academic manuscript",
            ProjectTypeWithTemplate::new(ProjectType::Manuscript),
        )
        .unimplemented(),
        ProjectChoice::new(
            "book",
            "Book",
            "A multi-chapter book",
            ProjectTypeWithTemplate::new(ProjectType::Book),
        )
        .unimplemented(),
    ]
}

/// Get only the implemented project choices.
pub fn implemented_choices() -> Vec<ProjectChoice> {
    available_choices()
        .into_iter()
        .filter(|c| c.implemented)
        .collect()
}

/// Look up a project choice by its ID.
pub fn find_choice(id: &str) -> Option<ProjectChoice> {
    available_choices().into_iter().find(|c| c.id == id)
}

/// Look up a project choice by its ID, returning only implemented choices.
pub fn find_implemented_choice(id: &str) -> Option<ProjectChoice> {
    implemented_choices().into_iter().find(|c| c.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_type_with_template_parse() {
        let website = ProjectTypeWithTemplate::parse("website").unwrap();
        assert_eq!(website.project_type, ProjectType::Website);
        assert!(website.template.is_none());

        let blog = ProjectTypeWithTemplate::parse("website:blog").unwrap();
        assert_eq!(blog.project_type, ProjectType::Website);
        assert_eq!(blog.template.as_deref(), Some("blog"));
    }

    #[test]
    fn test_project_type_with_template_to_string() {
        let website = ProjectTypeWithTemplate::new(ProjectType::Website);
        assert_eq!(website.to_id_string(), "website");

        let blog = ProjectTypeWithTemplate::with_template(ProjectType::Website, "blog");
        assert_eq!(blog.to_id_string(), "website:blog");
    }

    #[test]
    fn test_available_choices() {
        let choices = available_choices();
        assert!(!choices.is_empty());

        // Should have at least default and website
        let ids: Vec<_> = choices.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"default"));
        assert!(ids.contains(&"website"));
        assert!(ids.contains(&"blog"));
    }

    #[test]
    fn test_implemented_choices() {
        let choices = implemented_choices();

        // All returned choices should be implemented
        for choice in &choices {
            assert!(choice.implemented, "{} should be implemented", choice.id);
        }

        // Should have at least default and website
        let ids: Vec<_> = choices.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"default"));
        assert!(ids.contains(&"website"));
    }

    #[test]
    fn test_find_choice() {
        let blog = find_choice("blog").unwrap();
        assert_eq!(blog.name, "Blog");
        assert_eq!(blog.target.project_type, ProjectType::Website);
        assert_eq!(blog.target.template.as_deref(), Some("blog"));

        let nonexistent = find_choice("nonexistent");
        assert!(nonexistent.is_none());
    }

    #[test]
    fn test_blog_maps_to_website_template() {
        let blog = find_choice("blog").unwrap();
        // "Blog" in the UI maps to website:blog internally
        assert_eq!(blog.target.to_id_string(), "website:blog");
    }
}
