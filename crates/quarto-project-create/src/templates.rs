/*
 * templates.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Template loading for project scaffolding.
 *
 * Templates are embedded at compile time via `include_str!()`.
 * This works for both native and WASM targets.
 */

use crate::types::ProjectType;

/// Templates for default project type.
pub mod default {
    /// `_quarto.yml` template for default projects.
    pub const QUARTO_YML: &str = include_str!("../resources/templates/default/_quarto.yml.ejs");
}

/// Templates for website project type.
pub mod website {
    /// `_quarto.yml` template for website projects.
    pub const QUARTO_YML: &str = include_str!("../resources/templates/website/_quarto.yml.ejs");

    /// `index.qmd` template for website projects.
    pub const INDEX_QMD: &str = include_str!("../resources/templates/website/index.qmd.ejs");
}

/// A template file with its target path.
#[derive(Debug, Clone)]
pub struct TemplateFile {
    /// Relative path where the file should be created
    pub path: &'static str,
    /// EJS template content
    pub template: &'static str,
}

impl TemplateFile {
    const fn new(path: &'static str, template: &'static str) -> Self {
        Self { path, template }
    }
}

/// Get the template files for a project type.
///
/// Returns a list of template files that should be rendered and created
/// for the given project type.
pub fn get_templates(project_type: ProjectType) -> &'static [TemplateFile] {
    match project_type {
        ProjectType::Default => &DEFAULT_TEMPLATES,
        ProjectType::Website => &WEBSITE_TEMPLATES,
        // Not yet implemented - fall back to default
        ProjectType::Blog | ProjectType::Manuscript | ProjectType::Book => &DEFAULT_TEMPLATES,
    }
}

/// Templates for default project type.
static DEFAULT_TEMPLATES: [TemplateFile; 1] =
    [TemplateFile::new("_quarto.yml", default::QUARTO_YML)];

/// Templates for website project type.
static WEBSITE_TEMPLATES: [TemplateFile; 2] = [
    TemplateFile::new("_quarto.yml", website::QUARTO_YML),
    TemplateFile::new("index.qmd", website::INDEX_QMD),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_templates_exist() {
        let templates = get_templates(ProjectType::Default);
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].path, "_quarto.yml");
        assert!(templates[0].template.contains("project:"));
    }

    #[test]
    fn test_website_templates_exist() {
        let templates = get_templates(ProjectType::Website);
        assert_eq!(templates.len(), 2);

        let paths: Vec<_> = templates.iter().map(|t| t.path).collect();
        assert!(paths.contains(&"_quarto.yml"));
        assert!(paths.contains(&"index.qmd"));
    }

    #[test]
    fn test_templates_are_valid_ejs() {
        // Check that templates contain EJS syntax
        for project_type in ProjectType::implemented() {
            for template in get_templates(*project_type) {
                // All our templates use <%= title %> at minimum
                assert!(
                    template.template.contains("<%"),
                    "Template {} for {:?} should contain EJS syntax",
                    template.path,
                    project_type
                );
            }
        }
    }
}
