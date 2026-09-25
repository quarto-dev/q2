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

    /// Whether the hub seeds this choice into a new user's "Examples /
    /// Templates" collection on first run (bd-3fwtdhil). Defaults to false.
    #[serde(default)]
    pub seed: bool,

    /// Hierarchical group labels, outermost first (bd-q33ylfxf). The hub's
    /// New menu renders each level as a submenu and `q2 create --list`
    /// indents by it. Empty means top level. Ids stay flat and unique; the
    /// path is presentation only.
    #[serde(default)]
    pub path: Vec<String>,
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
            seed: false,
            path: Vec::new(),
        }
    }

    /// Place this choice under a hierarchical path of group labels,
    /// outermost first (e.g. `["Templates"]`).
    pub fn in_path<I, S>(mut self, path: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.path = path.into_iter().map(Into::into).collect();
        self
    }

    /// Seed this choice into a new user's "Examples / Templates"
    /// collection on first run. Only meaningful for hub choices.
    pub fn seed(mut self) -> Self {
        self.seed = true;
        self
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
    // Two groups (bd-q33ylfxf). "Templates" are skeletons: sparse, with
    // enough structure to get going, and they interpolate `$title$`.
    // "Examples" are populated projects whose content shows what people do
    // in that format; their titles are fixed and the typed name is ignored.
    vec![
        ProjectChoice::new(
            "default",
            "Default",
            "A minimal Quarto project",
            ProjectTypeWithTemplate::new(ProjectType::Default),
        )
        .in_path(["Templates"]),
        ProjectChoice::new(
            "website",
            "Website",
            "A Quarto website with navigation",
            ProjectTypeWithTemplate::new(ProjectType::Website),
        )
        .in_path(["Templates"]),
        ProjectChoice::new(
            "blog",
            "Blog",
            "A blog using the Quarto blog template",
            ProjectTypeWithTemplate::with_template(ProjectType::Website, "blog"),
        )
        .in_path(["Templates"]),
        ProjectChoice::new(
            "presentation",
            "Presentation",
            "A reveal.js slide deck",
            ProjectTypeWithTemplate::with_template(ProjectType::Default, "presentation"),
        )
        .in_path(["Templates"]),
        ProjectChoice::new(
            "manuscript",
            "Manuscript",
            "An academic manuscript",
            ProjectTypeWithTemplate::new(ProjectType::Manuscript),
        )
        .unimplemented()
        .in_path(["Templates"]),
        ProjectChoice::new(
            "book",
            "Book",
            "A multi-chapter book",
            ProjectTypeWithTemplate::new(ProjectType::Book),
        )
        .unimplemented()
        .in_path(["Templates"]),
        // The welcome tour (bd-d147nkqx): the first hub-only template.
        ProjectChoice::new(
            "hub-placeholder",
            "Welcome to the Quarto-Hub preview",
            "Get started here",
            ProjectTypeWithTemplate::with_template(ProjectType::Website, "hub-placeholder"),
        )
        .hub_only()
        .in_path(["Examples"]),
        // The four example projects a new user finds in the "Examples /
        // Templates" collection (bd-3fwtdhil). Hub-only, and seeded in this
        // order. Each is a short instructional project with fixed content;
        // the name the user types is not interpolated.
        ProjectChoice::new(
            "example-meeting-notes",
            "Meeting Notes",
            "One page per meeting, written together during the call",
            ProjectTypeWithTemplate::with_template(ProjectType::Default, "example-meeting-notes"),
        )
        .hub_only()
        .seed()
        .in_path(["Examples"]),
        ProjectChoice::new(
            "example-website",
            "Website",
            "A few pages with shared navigation and a tour of page features",
            ProjectTypeWithTemplate::with_template(ProjectType::Website, "example-website"),
        )
        .hub_only()
        .seed()
        .in_path(["Examples"]),
        ProjectChoice::new(
            "example-article",
            "Article",
            "A short article with citations, equations, figures, and cross-references",
            ProjectTypeWithTemplate::with_template(ProjectType::Default, "example-article"),
        )
        .hub_only()
        .seed()
        .in_path(["Examples"]),
        ProjectChoice::new(
            "example-presentation",
            "Presentation",
            "A short reveal.js deck with a custom theme",
            ProjectTypeWithTemplate::with_template(ProjectType::Default, "example-presentation"),
        )
        .hub_only()
        .seed()
        .in_path(["Examples"]),
    ]
}

/// The choices the hub seeds into a new user's "Examples / Templates"
/// collection on first run, in the order they are seeded (bd-3fwtdhil).
pub fn seed_choices() -> Vec<ProjectChoice> {
    implemented_choices()
        .into_iter()
        .filter(|c| c.seed)
        .collect()
}

/// What a group of choices is for, keyed by its hierarchical path
/// (bd-q33ylfxf). The hub shows this as subtext under the group's menu
/// item, next to the per-choice descriptions, so the two kinds of entry
/// are told apart before the group opens. Lives beside the registry so
/// a label and its explanation cannot drift apart.
pub fn path_description(path: &[String]) -> Option<&'static str> {
    match path {
        [group] if group == "Templates" => {
            Some("Bare skeletons with just enough structure to start writing")
        }
        [group] if group == "Examples" => {
            Some("Filled-in projects that show what each format can do")
        }
        _ => None,
    }
}

/// A run of choices sharing one hierarchical path, in registry order.
#[derive(Debug, Clone)]
pub struct ChoiceGroup {
    pub path: Vec<String>,
    /// See [`path_description`].
    pub description: Option<&'static str>,
    pub choices: Vec<ProjectChoice>,
}

/// The implemented choices offered on `surface`, grouped by their exact
/// path in order of first appearance (bd-q33ylfxf). Consumers that want a
/// deeper tree (the hub menu) build it from `path` themselves; this flat
/// grouping is what `q2 create --list` prints.
pub fn choices_grouped_by_path(surface: Surface) -> Vec<ChoiceGroup> {
    let mut groups: Vec<ChoiceGroup> = Vec::new();
    for choice in choices_for(surface) {
        match groups.iter_mut().find(|g| g.path == choice.path) {
            Some(g) => g.choices.push(choice),
            None => groups.push(ChoiceGroup {
                description: path_description(&choice.path),
                path: choice.path.clone(),
                choices: vec![choice],
            }),
        }
    }
    groups
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
            5,
            "expected the placeholder plus the four seeded examples, got {:?}",
            hub_only.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
    }

    // ----------------------------------------------------------------
    // Seeded example projects (bd-3fwtdhil)
    // ----------------------------------------------------------------

    const EXAMPLE_IDS: [&str; 4] = [
        "example-meeting-notes",
        "example-website",
        "example-article",
        "example-presentation",
    ];

    #[test]
    fn seed_choices_are_the_four_examples_in_registry_order() {
        let ids: Vec<String> = seed_choices().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, EXAMPLE_IDS);
    }

    #[test]
    fn seed_choices_are_implemented_and_hub_only() {
        for c in seed_choices() {
            assert!(c.seed, "{} must carry the seed flag", c.id);
            assert!(c.implemented, "{} must be implemented", c.id);
            assert!(c.available_on(Surface::Hub), "{} must be on the hub", c.id);
            assert!(
                !c.available_on(Surface::Cli),
                "{} must not be on the CLI",
                c.id
            );
        }
    }

    #[test]
    fn example_names_are_the_collection_labels() {
        let names: Vec<String> = seed_choices().into_iter().map(|c| c.name).collect();
        assert_eq!(
            names,
            ["Meeting Notes", "Website", "Article", "Presentation"]
        );
    }

    #[test]
    fn non_example_choices_are_not_seeded() {
        for id in [
            "default",
            "website",
            "blog",
            "manuscript",
            "book",
            "hub-placeholder",
        ] {
            let c = find_choice(id).unwrap_or_else(|| panic!("missing {id}"));
            assert!(!c.seed, "{id} must not be seeded");
        }
    }

    #[test]
    fn choice_json_without_seed_field_deserializes_as_not_seeded() {
        let json = r#"{"id":"x","name":"X","description":"d","target":{"project_type":"website"},"implemented":true}"#;
        let c: ProjectChoice = serde_json::from_str(json).unwrap();
        assert!(!c.seed);
    }

    #[test]
    fn seeded_choice_serializes_seed_true() {
        let c = ProjectChoice::new(
            "x",
            "X",
            "d",
            ProjectTypeWithTemplate::new(ProjectType::Default),
        )
        .hub_only()
        .seed();
        let v: serde_json::Value = serde_json::to_value(&c).unwrap();
        assert_eq!(v["seed"], serde_json::json!(true));
    }

    #[test]
    fn choices_for_cli_excludes_hub_only_and_unimplemented() {
        let cli_ids: Vec<String> = choices_for(Surface::Cli)
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(cli_ids, ["default", "website", "blog", "presentation"]);
        for hub_only in hub_only_choices() {
            assert!(!cli_ids.contains(&hub_only.id), "cli ids: {cli_ids:?}");
        }
    }

    // ----------------------------------------------------------------
    // Hierarchical path and the Presentation skeleton (bd-q33ylfxf)
    // ----------------------------------------------------------------

    #[test]
    fn every_choice_sits_under_templates_or_examples() {
        for c in available_choices() {
            assert_eq!(c.path.len(), 1, "{}: {:?}", c.id, c.path);
            assert!(
                c.path[0] == "Templates" || c.path[0] == "Examples",
                "{}: {:?}",
                c.id,
                c.path
            );
        }
    }

    #[test]
    fn templates_are_skeletons_and_examples_are_populated() {
        let under = |group: &str| -> Vec<String> {
            available_choices()
                .into_iter()
                .filter(|c| c.path == [group])
                .map(|c| c.id)
                .collect()
        };
        assert_eq!(
            under("Templates"),
            [
                "default",
                "website",
                "blog",
                "presentation",
                "manuscript",
                "book"
            ]
        );
        assert_eq!(
            under("Examples"),
            [
                "hub-placeholder",
                "example-meeting-notes",
                "example-website",
                "example-article",
                "example-presentation",
            ]
        );
    }

    #[test]
    fn choice_json_without_path_field_deserializes_as_top_level() {
        let json = r#"{"id":"x","name":"X","description":"d","target":{"project_type":"website"},"implemented":true}"#;
        let c: ProjectChoice = serde_json::from_str(json).unwrap();
        assert!(c.path.is_empty());
    }

    #[test]
    fn in_path_serializes_the_path_array_and_nests_arbitrarily_deep() {
        let c = ProjectChoice::new(
            "x",
            "X",
            "d",
            ProjectTypeWithTemplate::new(ProjectType::Default),
        )
        .in_path(["Templates", "Decks"]);
        assert_eq!(c.path, ["Templates", "Decks"]);
        let v: serde_json::Value = serde_json::to_value(&c).unwrap();
        assert_eq!(v["path"], serde_json::json!(["Templates", "Decks"]));
    }

    #[test]
    fn every_group_in_the_registry_has_a_description() {
        // The hub shows the description as subtext under the group's
        // menu item, so a group without one would render a bare label.
        for group in choices_grouped_by_path(Surface::Hub) {
            assert!(
                group.description.is_some(),
                "group {:?} has no description",
                group.path
            );
        }
        assert!(
            path_description(&["Templates".to_string()])
                .unwrap()
                .to_lowercase()
                .contains("skeleton")
        );
        assert!(
            path_description(&["Examples".to_string()])
                .unwrap()
                .to_lowercase()
                .contains("format")
        );
        assert_eq!(path_description(&["Nope".to_string()]), None);
        assert_eq!(path_description(&[]), None);
    }

    #[test]
    fn grouped_by_path_keeps_registry_order_within_and_across_groups() {
        let hub = choices_grouped_by_path(Surface::Hub);
        let paths: Vec<&[String]> = hub.iter().map(|g| g.path.as_slice()).collect();
        assert_eq!(
            paths,
            [
                &["Templates".to_string()][..],
                &["Examples".to_string()][..]
            ]
        );
        let template_ids: Vec<&str> = hub[0].choices.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(template_ids, ["default", "website", "blog", "presentation"]);
        let example_ids: Vec<&str> = hub[1].choices.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(example_ids[0], "hub-placeholder");
        assert_eq!(example_ids.len(), 5);

        // The CLI has no examples, so it gets a single group.
        let cli = choices_grouped_by_path(Surface::Cli);
        assert_eq!(
            cli.len(),
            1,
            "{:?}",
            cli.iter().map(|g| &g.path).collect::<Vec<_>>()
        );
        assert_eq!(cli[0].path, ["Templates"]);
    }

    #[test]
    fn presentation_is_a_default_project_skeleton_on_every_surface() {
        let c = find_choice("presentation").expect("presentation choice");
        assert_eq!(c.name, "Presentation");
        assert_eq!(
            c.target,
            ProjectTypeWithTemplate::with_template(ProjectType::Default, "presentation")
        );
        assert!(c.implemented);
        assert!(c.available_on(Surface::Cli));
        assert!(c.available_on(Surface::Hub));
        assert!(!c.seed);
        assert_eq!(c.path, ["Templates"]);
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
