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
 * # Surfaces
 *
 * `available_choices()` is the single registry, but not every entry
 * is offered everywhere: each choice lists the `Surface`s it appears
 * on, and a consumer asks for its own surface with `choices_for`
 * (the CLI asks for `Surface::Cli`, the hub client's WASM entry point
 * for `Surface::Hub`). A hub-only template (bd-d147nkqx) is a
 * registry entry marked `.hub_only()`; `q2 create project` neither
 * lists nor accepts it, while the hub's "New project" menu does.
 * Scaffolding is surface-agnostic — the gate is the consumer's job.
 *
 * # Architecture
 *
 * ```text
 * ProjectChoice (user-facing)
 *     ├── id: "blog"
 *     ├── name: "Blog"
 *     ├── description: "A blog using the Quarto blog template"
 *     ├── surfaces: [Cli, Hub]
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

/// A place where project choices are offered to users.
///
/// Every consumer of the registry asks for the choices of *its* surface
/// (`choices_for`). A choice defaults to all surfaces; a template meant
/// for the hub only (bd-d147nkqx) opts out of the CLI with
/// `ProjectChoice::hub_only`. Scaffolding itself is surface-agnostic —
/// `create_project_from_choice` never checks this — so the gate lives
/// at each surface's front door, not in the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    /// `q2 create project`
    Cli,
    /// The Quarto Hub web client's "New project" menu
    Hub,
}

impl Surface {
    /// Every surface, in registry-declaration order.
    pub fn all() -> Vec<Surface> {
        vec![Surface::Cli, Surface::Hub]
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

    /// Surfaces this choice is offered on. Defaults to every surface.
    #[serde(default = "Surface::all")]
    pub surfaces: Vec<Surface>,
}

impl ProjectChoice {
    /// Create a new project choice, offered on every surface.
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
            surfaces: Surface::all(),
        }
    }

    /// Mark this choice as not yet implemented.
    pub fn unimplemented(mut self) -> Self {
        self.implemented = false;
        self
    }

    /// Offer this choice on the hub only, never on the CLI.
    pub fn hub_only(mut self) -> Self {
        self.surfaces = vec![Surface::Hub];
        self
    }

    /// Whether this choice is offered on `surface`.
    pub fn available_on(&self, surface: Surface) -> bool {
        self.surfaces.contains(&surface)
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
        ),
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
        // Placeholder for the first hub-only template (bd-d147nkqx). It
        // exists so the surface gate is exercised end to end; the real
        // hub template replaces it — id, name, description, and
        // scaffold — before the feature ships.
        ProjectChoice::new(
            "hub-placeholder",
            "Hub placeholder",
            "A placeholder for the first hub-only project template",
            ProjectTypeWithTemplate::with_template(ProjectType::Website, "hub-placeholder"),
        )
        .hub_only(),
    ]
}

/// Get only the implemented project choices.
pub fn implemented_choices() -> Vec<ProjectChoice> {
    available_choices()
        .into_iter()
        .filter(|c| c.implemented)
        .collect()
}

/// The implemented choices offered on `surface`. This is what a
/// surface's menu, prompt, or listing should show.
pub fn choices_for(surface: Surface) -> Vec<ProjectChoice> {
    implemented_choices()
        .into_iter()
        .filter(|c| c.available_on(surface))
        .collect()
}

/// Look up a project choice by its ID.
pub fn find_choice(id: &str) -> Option<ProjectChoice> {
    available_choices().into_iter().find(|c| c.id == id)
}

/// Look up the project choice that maps to `target`.
///
/// The CLI accepts the colon form (`website:blog`) as well as choice
/// ids, so it needs to get from a parsed target back to the choice
/// that owns it to apply the choice's surface gate.
pub fn find_choice_by_target(target: &ProjectTypeWithTemplate) -> Option<ProjectChoice> {
    available_choices()
        .into_iter()
        .find(|c| c.target == *target)
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

    // ----------------------------------------------------------------
    // Surface gating (bd-d147nkqx)
    // ----------------------------------------------------------------

    /// The registry's implemented, hub-only choices. Tests derive the
    /// hub-only id from the registry rather than hardcoding it, so
    /// swapping the placeholder template for a real one needs no test
    /// edits here.
    fn hub_only_choices() -> Vec<ProjectChoice> {
        available_choices()
            .into_iter()
            .filter(|c| {
                c.implemented && c.available_on(Surface::Hub) && !c.available_on(Surface::Cli)
            })
            .collect()
    }

    #[test]
    fn new_choice_is_offered_on_every_surface() {
        let c = ProjectChoice::new(
            "x",
            "X",
            "d",
            ProjectTypeWithTemplate::new(ProjectType::Website),
        );
        assert!(c.available_on(Surface::Cli));
        assert!(c.available_on(Surface::Hub));
        assert_eq!(c.surfaces, Surface::all());
    }

    #[test]
    fn hub_only_choice_is_offered_on_hub_but_not_cli() {
        let c = ProjectChoice::new(
            "x",
            "X",
            "d",
            ProjectTypeWithTemplate::new(ProjectType::Website),
        )
        .hub_only();
        assert!(c.available_on(Surface::Hub));
        assert!(!c.available_on(Surface::Cli));
        assert_eq!(c.surfaces, vec![Surface::Hub]);
    }

    #[test]
    fn registry_has_an_implemented_hub_only_choice() {
        let hub_only = hub_only_choices();
        assert_eq!(
            hub_only.len(),
            1,
            "expected exactly one hub-only choice (the placeholder), got {:?}",
            hub_only.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn choices_for_cli_excludes_hub_only_and_unimplemented() {
        let cli_ids: Vec<String> = choices_for(Surface::Cli)
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(cli_ids, ["default", "website", "blog"]);
        for hub_only in hub_only_choices() {
            assert!(!cli_ids.contains(&hub_only.id), "cli ids: {cli_ids:?}");
        }
    }

    #[test]
    fn choices_for_hub_is_cli_choices_plus_hub_only() {
        let hub_ids: Vec<String> = choices_for(Surface::Hub)
            .into_iter()
            .map(|c| c.id)
            .collect();
        let mut expected: Vec<String> = choices_for(Surface::Cli)
            .into_iter()
            .map(|c| c.id)
            .collect();
        expected.extend(hub_only_choices().into_iter().map(|c| c.id));
        assert_eq!(hub_ids, expected);
        for c in choices_for(Surface::Hub) {
            assert!(c.implemented, "{} must be implemented", c.id);
        }
    }

    #[test]
    fn choice_json_without_surfaces_field_deserializes_as_all_surfaces() {
        let json = r#"{"id":"x","name":"X","description":"d","target":{"project_type":"website"},"implemented":true}"#;
        let c: ProjectChoice = serde_json::from_str(json).unwrap();
        assert_eq!(c.surfaces, Surface::all());
    }

    #[test]
    fn hub_only_choice_serializes_surfaces_as_lowercase_names() {
        let c = ProjectChoice::new(
            "x",
            "X",
            "d",
            ProjectTypeWithTemplate::new(ProjectType::Website),
        )
        .hub_only();
        let v: serde_json::Value = serde_json::to_value(&c).unwrap();
        assert_eq!(v["surfaces"], serde_json::json!(["hub"]));
    }

    #[test]
    fn find_choice_by_target_resolves_aliases_and_hub_only_targets() {
        let blog = find_choice_by_target(&ProjectTypeWithTemplate::with_template(
            ProjectType::Website,
            "blog",
        ))
        .unwrap();
        assert_eq!(blog.id, "blog");

        let website =
            find_choice_by_target(&ProjectTypeWithTemplate::new(ProjectType::Website)).unwrap();
        assert_eq!(website.id, "website");

        for hub_only in hub_only_choices() {
            let found = find_choice_by_target(&hub_only.target).unwrap();
            assert_eq!(found.id, hub_only.id);
            assert!(!found.available_on(Surface::Cli));
        }

        assert!(
            find_choice_by_target(&ProjectTypeWithTemplate::with_template(
                ProjectType::Website,
                "solitaire"
            ))
            .is_none()
        );
    }
}
