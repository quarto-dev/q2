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

mod templates;
mod types;

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
}
