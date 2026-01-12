/*
 * quarto-project-create
 * Copyright (c) 2025 Posit, PBC
 *
 * Project scaffolding for Quarto projects.
 *
 * This crate provides functionality to create new Quarto projects with
 * appropriate scaffold files. It is platform-agnostic and works with
 * both native (deno_core) and WASM (browser JS) environments via the
 * `SystemRuntime` abstraction.
 *
 * # Architecture
 *
 * Templates are embedded at compile time via `include_str!()`. The
 * creation logic uses `SystemRuntime::render_ejs()` to render templates,
 * which abstracts over the actual JavaScript execution environment:
 *
 * - **Native**: Uses deno_core with embedded V8 runtime
 * - **WASM**: Calls out to browser JavaScript via wasm-bindgen
 *
 * This design allows the same template rendering code to work across
 * all target platforms without any platform-specific code in this crate.
 *
 * # Usage
 *
 * ```ignore
 * use quarto_project_create::{create_project, CreateProjectOptions, ProjectType};
 * use quarto_system_runtime::NativeRuntime;
 *
 * let runtime = NativeRuntime::new();
 * let options = CreateProjectOptions::new(ProjectType::Website, "My Website");
 * let files = create_project(&runtime, options).await?;
 *
 * for file in files {
 *     println!("Create: {} ({} bytes)", file.path.display(), file.content.len());
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
pub use types::{CreateError, CreateProjectOptions, ProjectFile, ProjectType};

use quarto_system_runtime::SystemRuntime;
use serde_json::json;
use std::path::PathBuf;

/// Create a new Quarto project with the given options.
///
/// This renders all template files for the specified project type and returns
/// the list of files to be created. The caller is responsible for writing
/// the files to disk or VFS.
///
/// # Arguments
///
/// * `runtime` - The system runtime to use for EJS template rendering
/// * `options` - Project creation options (type, title, etc.)
///
/// # Returns
///
/// A list of `ProjectFile` structs containing the path and rendered content
/// for each file in the project scaffold.
///
/// # Errors
///
/// Returns `CreateError::TemplateRender` if template rendering fails.
///
/// # Example
///
/// ```ignore
/// let files = create_project(&runtime, CreateProjectOptions::new(
///     ProjectType::Website,
///     "My Website"
/// )).await?;
/// ```
pub async fn create_project(
    runtime: &dyn SystemRuntime,
    options: CreateProjectOptions,
) -> Result<Vec<ProjectFile>, CreateError> {
    // Check if JS execution is available
    if !runtime.js_available() {
        return Err(CreateError::TemplateRender(
            "JavaScript execution is not available for template rendering".to_string(),
        ));
    }

    // Build template data
    let data = json!({
        "title": options.title,
        "projectType": options.project_type.id(),
    });

    // Get templates for this project type
    let template_files = templates::get_templates(options.project_type);

    // Render each template
    let mut files = Vec::with_capacity(template_files.len());

    for template_file in template_files {
        let content = runtime
            .render_ejs(template_file.template, &data)
            .await
            .map_err(|e| CreateError::TemplateRender(e.to_string()))?;

        files.push(ProjectFile {
            path: PathBuf::from(template_file.path),
            content,
        });
    }

    Ok(files)
}

// ============================================================================
// New scaffold-based API
// ============================================================================

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
/// * `runtime` - The system runtime to use for EJS template rendering
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
///     &runtime,
///     CreateFromChoiceOptions::new("website", "My Website")
/// ).await?;
/// ```
pub async fn create_project_from_choice(
    runtime: &dyn SystemRuntime,
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
    create_scaffolded_files(runtime, &scaffold, &options.title).await
}

/// Create files from a project scaffold.
///
/// This is a lower-level API that takes a `ProjectScaffold` directly.
/// Use `create_project_from_choice` for the higher-level API with
/// template aliasing support.
///
/// # Arguments
///
/// * `runtime` - The system runtime to use for EJS template rendering
/// * `scaffold` - The project scaffold definition
/// * `title` - Project title (used in templates)
///
/// # Returns
///
/// A list of `ScaffoldedFile` structs ready to be written to disk or VFS.
pub async fn create_scaffolded_files(
    runtime: &dyn SystemRuntime,
    scaffold: &ProjectScaffold,
    title: &str,
) -> Result<Vec<ScaffoldedFile>, CreateError> {
    // Check if JS execution is available (needed for EJS templates)
    if !runtime.js_available() {
        return Err(CreateError::TemplateRender(
            "JavaScript execution is not available for template rendering".to_string(),
        ));
    }

    // Build template data
    let data = json!({
        "title": title,
        "projectType": scaffold.target.project_type.id(),
        "template": scaffold.target.template,
    });

    let mut files = Vec::with_capacity(scaffold.files.len());

    for file_def in &scaffold.files {
        let path = file_def.full_path();

        match &file_def.content {
            ScaffoldContent::Template(template) => {
                let content = runtime
                    .render_ejs(template, &data)
                    .await
                    .map_err(|e| CreateError::TemplateRender(e.to_string()))?;

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
    fn test_create_project_options() {
        let options = CreateProjectOptions::new(ProjectType::Website, "My Site");
        assert_eq!(options.project_type, ProjectType::Website);
        assert_eq!(options.title, "My Site");
    }
}

// Integration tests that require the native runtime
#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod integration_tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;

    #[test]
    fn test_create_default_project() {
        let runtime = NativeRuntime::new();
        let options = CreateProjectOptions::new(ProjectType::Default, "Test Project");

        let files = pollster::block_on(create_project(&runtime, options)).unwrap();

        // Should have exactly one file: _quarto.yml
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.to_str().unwrap(), "_quarto.yml");

        // Content should contain the title
        assert!(files[0].content.contains("Test Project"));
        assert!(files[0].content.contains("project:"));
    }

    #[test]
    fn test_create_website_project() {
        let runtime = NativeRuntime::new();
        let options = CreateProjectOptions::new(ProjectType::Website, "My Website");

        let files = pollster::block_on(create_project(&runtime, options)).unwrap();

        // Should have two files: _quarto.yml and index.qmd
        assert_eq!(files.len(), 2);

        let paths: Vec<_> = files.iter().map(|f| f.path.to_str().unwrap()).collect();
        assert!(paths.contains(&"_quarto.yml"));
        assert!(paths.contains(&"index.qmd"));

        // Check _quarto.yml content
        let quarto_yml = files
            .iter()
            .find(|f| f.path.to_str() == Some("_quarto.yml"))
            .unwrap();
        assert!(quarto_yml.content.contains("My Website"));
        assert!(quarto_yml.content.contains("type: website"));

        // Check index.qmd content
        let index_qmd = files
            .iter()
            .find(|f| f.path.to_str() == Some("index.qmd"))
            .unwrap();
        assert!(index_qmd.content.contains("My Website"));
        assert!(index_qmd.content.contains("Quarto website"));
    }

    #[test]
    fn test_create_project_special_characters_in_title() {
        let runtime = NativeRuntime::new();
        let options = CreateProjectOptions::new(
            ProjectType::Default,
            "Project with \"quotes\" & <special> chars",
        );

        let files = pollster::block_on(create_project(&runtime, options)).unwrap();

        // Should succeed and contain the title
        assert_eq!(files.len(), 1);
        // The title should be present (EJS handles escaping)
        assert!(files[0].content.contains("quotes"));
    }

    // ========================================================================
    // New scaffold-based API tests
    // ========================================================================

    #[test]
    fn test_create_project_from_choice_website() {
        let runtime = NativeRuntime::new();
        let options = CreateFromChoiceOptions::new("website", "My Website");

        let files = pollster::block_on(create_project_from_choice(&runtime, options)).unwrap();

        // Should have two files: _quarto.yml and index.qmd
        assert_eq!(files.len(), 2);

        // Check we have the expected files
        let text_files: Vec<_> = files
            .iter()
            .filter_map(|f| match f {
                ScaffoldedFile::Text { path, content } => {
                    Some((path.to_str().unwrap(), content.as_str()))
                }
                ScaffoldedFile::Binary { .. } => None,
            })
            .collect();

        let paths: Vec<_> = text_files.iter().map(|(p, _)| *p).collect();
        assert!(paths.contains(&"_quarto.yml"));
        assert!(paths.contains(&"index.qmd"));

        // Check _quarto.yml content
        let (_, quarto_yml) = text_files
            .iter()
            .find(|(p, _)| *p == "_quarto.yml")
            .unwrap();
        assert!(quarto_yml.contains("My Website"));
        assert!(quarto_yml.contains("type: website"));
    }

    #[test]
    fn test_create_project_from_choice_default() {
        let runtime = NativeRuntime::new();
        let options = CreateFromChoiceOptions::new("default", "Test Project");

        let files = pollster::block_on(create_project_from_choice(&runtime, options)).unwrap();

        // Should have one file: _quarto.yml
        assert_eq!(files.len(), 1);
        assert!(files[0].is_text());

        if let ScaffoldedFile::Text { path, content } = &files[0] {
            assert_eq!(path.to_str().unwrap(), "_quarto.yml");
            assert!(content.contains("Test Project"));
        } else {
            panic!("Expected text file");
        }
    }

    #[test]
    fn test_create_project_from_choice_unknown() {
        let runtime = NativeRuntime::new();
        let options = CreateFromChoiceOptions::new("nonexistent", "Test");

        let result = pollster::block_on(create_project_from_choice(&runtime, options));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreateError::UnknownProjectType(_)
        ));
    }

    #[test]
    fn test_create_project_from_choice_unimplemented() {
        let runtime = NativeRuntime::new();
        // "blog" is defined but marked as unimplemented
        let options = CreateFromChoiceOptions::new("blog", "My Blog");

        let result = pollster::block_on(create_project_from_choice(&runtime, options));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CreateError::InvalidConfig(_)));
    }

    #[test]
    fn test_implemented_choices_are_usable() {
        let runtime = NativeRuntime::new();

        // All implemented choices should successfully create projects
        for choice in implemented_choices() {
            let options = CreateFromChoiceOptions::new(&choice.id, "Test Project");
            let result = pollster::block_on(create_project_from_choice(&runtime, options));

            assert!(
                result.is_ok(),
                "Failed to create project for implemented choice '{}': {:?}",
                choice.id,
                result.err()
            );
        }
    }
}
