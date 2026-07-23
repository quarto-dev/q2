/*
 * quarto-project-create
 * Copyright (c) 2025 Posit, PBC
 *
 * Project scaffolding for Quarto projects.
 *
 * This crate provides functionality to create new Quarto projects with
 * appropriate scaffold files. It is platform-agnostic: templates are
 * embedded at compile time via `include_str!()` and rendered with
 * `quarto-doctemplate` (Pandoc template syntax), which is pure Rust and
 * works identically on native and wasm32 targets — no JS runtime involved.
 *
 * # Usage
 *
 * ```ignore
 * use quarto_project_create::{CreateFromChoiceOptions, create_project_from_choice};
 *
 * let options = CreateFromChoiceOptions::new("website", "My Website");
 * let files = create_project_from_choice(options)?;
 *
 * for file in files {
 *     println!("Create: {}", file.path().display());
 * }
 * ```
 */

mod choices;
mod scaffold;
mod templates;
mod types;

pub use choices::{
    ProjectChoice, ProjectTypeWithTemplate, available_choices, find_choice,
    find_implemented_choice, implemented_choices,
};
pub use scaffold::{
    ProjectScaffold, ScaffoldContent, ScaffoldFileDef, ScaffoldedFile, get_scaffold,
};
pub use types::{CreateError, ProjectType};

use quarto_doctemplate::{Template, TemplateContext, TemplateValue};

/// Escape a string for interpolation inside a YAML double-quoted scalar.
///
/// Scaffold templates interpolate `$title$` only inside double-quoted YAML
/// strings (`title: "$title$"`), so the value must be escaped for that
/// context — otherwise a title containing `"` or a newline would produce
/// invalid YAML.
fn yaml_escape_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Build the template context shared by all scaffold templates.
fn template_context(title: &str, project_type: &str, template: Option<&str>) -> TemplateContext {
    let mut ctx = TemplateContext::new();
    ctx.insert(
        "title",
        TemplateValue::String(yaml_escape_double_quoted(title)),
    );
    ctx.insert(
        "projectType",
        TemplateValue::String(project_type.to_string()),
    );
    if let Some(template) = template {
        ctx.insert("template", TemplateValue::String(template.to_string()));
    }
    ctx
}

/// Compile and render a single scaffold template.
fn render_template(template: &str, ctx: &TemplateContext) -> Result<String, CreateError> {
    let compiled =
        Template::compile(template).map_err(|e| CreateError::TemplateRender(e.to_string()))?;
    compiled
        .render(ctx)
        .map_err(|e| CreateError::TemplateRender(e.to_string()))
}

/// Options for creating a project from a choice.
#[derive(Debug, Clone)]
pub struct CreateFromChoiceOptions {
    /// The choice ID (e.g., "website", "blog")
    pub choice_id: String,

    /// Project title (used in templates)
    pub title: String,
}

impl CreateFromChoiceOptions {
    /// Create new options.
    pub fn new(choice_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            choice_id: choice_id.into(),
            title: title.into(),
        }
    }
}

/// Create a new project from a user-facing choice.
///
/// This is the primary API for creating projects with template aliasing support.
/// The `choice_id` maps to a `ProjectChoice` which may resolve to a different
/// internal project type (e.g., "blog" → website:blog).
///
/// # Arguments
///
/// * `options` - Project creation options (choice ID, title)
///
/// # Returns
///
/// A list of `ScaffoldedFile` structs containing text and/or binary content.
///
/// # Errors
///
/// Returns `CreateError::UnknownProjectType` if the choice ID is not found.
/// Returns `CreateError::InvalidConfig` if the choice is not implemented.
/// Returns `CreateError::TemplateRender` if template rendering fails.
///
/// # Example
///
/// ```ignore
/// let files = create_project_from_choice(
///     CreateFromChoiceOptions::new("website", "My Website")
/// )?;
/// ```
pub fn create_project_from_choice(
    options: CreateFromChoiceOptions,
) -> Result<Vec<ScaffoldedFile>, CreateError> {
    // Look up the choice
    let choice = find_choice(&options.choice_id)
        .ok_or_else(|| CreateError::UnknownProjectType(options.choice_id.clone()))?;

    // Check if implemented
    if !choice.implemented {
        return Err(CreateError::InvalidConfig(format!(
            "Project type '{}' is not yet implemented",
            choice.name
        )));
    }

    // Get the scaffold
    let scaffold_opt = get_scaffold(&choice.target);
    let scaffold = scaffold_opt.ok_or_else(|| {
        CreateError::InvalidConfig(format!(
            "No scaffold defined for {}",
            choice.target.to_id_string()
        ))
    })?;

    // Render the scaffold
    create_scaffolded_files(&scaffold, &options.title)
}

/// Create files from a project scaffold.
///
/// This is a lower-level API that takes a `ProjectScaffold` directly.
/// Use `create_project_from_choice` for the higher-level API with
/// template aliasing support.
///
/// # Arguments
///
/// * `scaffold` - The project scaffold definition
/// * `title` - Project title (used in templates)
///
/// # Returns
///
/// A list of `ScaffoldedFile` structs ready to be written to disk or VFS.
pub fn create_scaffolded_files(
    scaffold: &ProjectScaffold,
    title: &str,
) -> Result<Vec<ScaffoldedFile>, CreateError> {
    let ctx = template_context(
        title,
        scaffold.target.project_type.id(),
        scaffold.target.template.as_deref(),
    );

    let mut files = Vec::with_capacity(scaffold.files.len());

    for file_def in &scaffold.files {
        let path = file_def.full_path();

        match &file_def.content {
            ScaffoldContent::Template(template) => {
                let content = render_template(template, &ctx)?;

                files.push(ScaffoldedFile::Text { path, content });
            }
            ScaffoldContent::StaticText(text) => {
                files.push(ScaffoldedFile::Text {
                    path,
                    content: (*text).to_string(),
                });
            }
            ScaffoldContent::Binary { content, mime_type } => {
                files.push(ScaffoldedFile::Binary {
                    path,
                    content: content.to_vec(),
                    mime_type: (*mime_type).to_string(),
                });
            }
        }
    }

    Ok(files)
}

/// Get information about available project types.
///
/// Returns information useful for building UI selection dialogs.
pub fn available_project_types() -> Vec<ProjectTypeInfo> {
    ProjectType::implemented()
        .iter()
        .map(|pt| ProjectTypeInfo {
            id: pt.id().to_string(),
            name: pt.display_name().to_string(),
            description: project_type_description(*pt).to_string(),
        })
        .collect()
}

/// Information about a project type for UI display.
#[derive(Debug, Clone)]
pub struct ProjectTypeInfo {
    /// Lowercase identifier (e.g., "website")
    pub id: String,
    /// Display name (e.g., "Website")
    pub name: String,
    /// Short description
    pub description: String,
}

/// Get a short description for a project type.
fn project_type_description(project_type: ProjectType) -> &'static str {
    match project_type {
        ProjectType::Default => "A minimal Quarto project",
        ProjectType::Website => "A Quarto website with navigation",
        ProjectType::Blog => "A blog using the Quarto blog template",
        ProjectType::Manuscript => "An academic manuscript",
        ProjectType::Book => "A multi-chapter book",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_type_from_str() {
        assert_eq!(
            "website".parse::<ProjectType>().unwrap(),
            ProjectType::Website
        );
        assert_eq!(
            "default".parse::<ProjectType>().unwrap(),
            ProjectType::Default
        );
        assert!("invalid".parse::<ProjectType>().is_err());
    }

    #[test]
    fn test_project_type_display() {
        assert_eq!(ProjectType::Website.to_string(), "Website");
        assert_eq!(ProjectType::Default.to_string(), "Default");
    }

    #[test]
    fn test_available_project_types() {
        let types = available_project_types();
        assert!(!types.is_empty());

        // Should have at least default and website
        let ids: Vec<_> = types.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"default"));
        assert!(ids.contains(&"website"));
    }

    #[test]
    fn test_yaml_escape_double_quoted() {
        assert_eq!(yaml_escape_double_quoted("plain title"), "plain title");
        assert_eq!(yaml_escape_double_quoted(r#"a "b" c"#), r#"a \"b\" c"#);
        assert_eq!(yaml_escape_double_quoted(r"back\slash"), r"back\\slash");
        assert_eq!(yaml_escape_double_quoted("a\nb"), r"a\nb");
        assert_eq!(yaml_escape_double_quoted("a\tb"), r"a\tb");
        assert_eq!(yaml_escape_double_quoted("a\r\nb"), r"a\r\nb");
    }
}

// Rendering tests for the doctemplate-based scaffolding path.
//
// These run on every platform: template rendering is pure Rust
// (quarto-doctemplate), with no JS runtime involved. Assertions parse
// the rendered `_quarto.yml` with serde_yaml so they check field
// *values* (and prove the output is valid YAML), not just substrings.
#[cfg(test)]
mod render_tests {
    use super::*;

    /// Panic-on-missing lookup of a text file's content in a scaffold result.
    fn file_content<'a>(files: &'a [ScaffoldedFile], path: &str) -> &'a str {
        files
            .iter()
            .find_map(|f| match f {
                ScaffoldedFile::Text { path: p, content } if p.to_str() == Some(path) => {
                    Some(content.as_str())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected text file {path} in scaffold output"))
    }

    fn parse_yaml(src: &str) -> serde_yaml::Value {
        serde_yaml::from_str(src).expect("scaffolded _quarto.yml must be valid YAML")
    }

    #[test]
    fn default_scaffold_produces_config_and_starter_doc() {
        let files =
            create_project_from_choice(CreateFromChoiceOptions::new("default", "Test Project"))
                .unwrap();

        let paths: Vec<_> = files.iter().map(|f| f.path().to_str().unwrap()).collect();
        assert_eq!(paths, ["_quarto.yml", "index.qmd"]);

        let yml = parse_yaml(file_content(&files, "_quarto.yml"));
        assert_eq!(yml["project"]["title"].as_str(), Some("Test Project"));

        let index = file_content(&files, "index.qmd");
        assert!(
            index.starts_with("---\ntitle: \"Test Project\"\n---\n"),
            "starter doc must carry the title in front matter; got:\n{index}"
        );
        assert!(index.contains("## Quarto"));
        assert!(index.contains("<https://quarto.org>"));
    }

    #[test]
    fn website_scaffold_produces_q1_familiar_file_set() {
        let files =
            create_project_from_choice(CreateFromChoiceOptions::new("website", "My Website"))
                .unwrap();

        let paths: Vec<_> = files.iter().map(|f| f.path().to_str().unwrap()).collect();
        assert_eq!(
            paths,
            ["_quarto.yml", "index.qmd", "about.qmd", "styles.css"]
        );

        let yml_src = file_content(&files, "_quarto.yml");
        let yml = parse_yaml(yml_src);

        assert_eq!(yml["project"]["type"].as_str(), Some("website"));
        // Q2's website pipeline reads `website.title` (website_config.rs);
        // the title must live there, not under `project:`.
        assert_eq!(yml["website"]["title"].as_str(), Some("My Website"));
        assert!(yml["project"].get("title").is_none());

        let left = &yml["website"]["navbar"]["left"];
        assert_eq!(left[0]["href"].as_str(), Some("index.qmd"));
        assert_eq!(left[0]["text"].as_str(), Some("Home"));
        assert_eq!(left[1].as_str(), Some("about.qmd"));

        assert_eq!(yml["format"]["html"]["theme"].as_str(), Some("cosmo"));
        assert_eq!(yml["format"]["html"]["css"].as_str(), Some("styles.css"));
        assert_eq!(yml["format"]["html"]["toc"].as_bool(), Some(true));

        // styles.css must be declared a project resource so it is
        // copied into _site/ — Q2 does not (yet) treat `css:`-referenced
        // files as implicit resources (bd-b87tmmi4).
        assert_eq!(yml["project"]["resources"][0].as_str(), Some("styles.css"));

        // Q2 hard-errors (Q-14-1) on a `brand` theme marker with no brand
        // configured; the scaffold must not emit one.
        assert!(
            !yml_src.contains("brand"),
            "scaffolded _quarto.yml must not reference brand:\n{yml_src}"
        );

        let index = file_content(&files, "index.qmd");
        assert!(index.contains("title: \"My Website\""));
        assert!(index.contains("This is a Quarto website"));

        let about = file_content(&files, "about.qmd");
        assert!(about.contains("title: \"About\""));
        assert!(about.contains("About this site"));

        assert_eq!(file_content(&files, "styles.css"), "/* css styles */\n");

        for f in &files {
            if let ScaffoldedFile::Text { path, content } = f {
                assert!(
                    !content.contains("$title$") && !content.contains("<%"),
                    "template residue in {}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn special_characters_title_stays_valid_yaml() {
        let title = r#"R & D "quoted" \ backslash"#;
        let files =
            create_project_from_choice(CreateFromChoiceOptions::new("website", title)).unwrap();

        let yml_src = file_content(&files, "_quarto.yml");
        // `&` passes through raw (the old EJS path HTML-escaped it).
        assert!(!yml_src.contains("&amp;"));
        // Escaping must keep the config valid YAML that round-trips to
        // the original title.
        let yml = parse_yaml(yml_src);
        assert_eq!(yml["website"]["title"].as_str(), Some(title));
    }

    #[test]
    fn newline_title_stays_valid_yaml() {
        let files = create_project_from_choice(CreateFromChoiceOptions::new(
            "default",
            "Line one\nLine two",
        ))
        .unwrap();

        let yml = parse_yaml(file_content(&files, "_quarto.yml"));
        assert_eq!(yml["project"]["title"].as_str(), Some("Line one\nLine two"));
    }

    #[test]
    fn unknown_choice_is_rejected() {
        let result = create_project_from_choice(CreateFromChoiceOptions::new("nonexistent", "T"));
        assert!(matches!(
            result.unwrap_err(),
            CreateError::UnknownProjectType(_)
        ));
    }

    #[test]
    fn unimplemented_choice_is_rejected() {
        // "blog" is defined but marked as unimplemented
        let result = create_project_from_choice(CreateFromChoiceOptions::new("blog", "My Blog"));
        assert!(matches!(result.unwrap_err(), CreateError::InvalidConfig(_)));
    }

    #[test]
    fn implemented_choices_are_usable() {
        for choice in implemented_choices() {
            let result = create_project_from_choice(CreateFromChoiceOptions::new(
                &choice.id,
                "Test Project",
            ));
            assert!(
                result.is_ok(),
                "implemented choice '{}' failed: {:?}",
                choice.id,
                result.err()
            );
        }
    }
}
