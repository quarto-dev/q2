/*
 * scaffold.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Declarative file specification for project scaffolding.
 *
 * This module provides a data-driven approach to defining project files,
 * ported from TypeScript Quarto's ScaffoldFile pattern. It supports:
 *
 * - **Template files**: doctemplate (Pandoc template syntax) files rendered with project data
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
    /// Doctemplate source to be rendered with project data
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
        ProjectType::Default => match target.template.as_deref() {
            None => Some(
                ProjectScaffold::new(ProjectType::Default)
                    .add_file(ScaffoldFileDef::template(
                        "_quarto.yml",
                        templates::default::QUARTO_YML,
                    ))
                    .add_file(ScaffoldFileDef::template(
                        "index.qmd",
                        templates::default::INDEX_QMD,
                    )),
            ),
            // Presentation skeleton (bd-q33ylfxf): both files take `$title$`.
            Some("presentation") => Some(
                ProjectScaffold::with_template(ProjectType::Default, "presentation")
                    .add_file(ScaffoldFileDef::template(
                        "_quarto.yml",
                        templates::presentation::QUARTO_YML,
                    ))
                    .add_file(ScaffoldFileDef::template(
                        "index.qmd",
                        templates::presentation::INDEX_QMD,
                    )),
            ),
            // Seeded examples (bd-3fwtdhil): fixed content, nothing interpolates.
            Some("example-meeting-notes") => {
                use templates::examples::meeting_notes as t;
                Some(
                    ProjectScaffold::with_template(ProjectType::Default, "example-meeting-notes")
                        .add_file(ScaffoldFileDef::static_text("_quarto.yml", t::QUARTO_YML))
                        .add_file(ScaffoldFileDef::static_text("index.qmd", t::INDEX_QMD))
                        .add_file(ScaffoldFileDef::static_text("tweaks.scss", t::TWEAKS_SCSS))
                        .add_file(
                            ScaffoldFileDef::static_text(
                                "2026-09-17.qmd",
                                t::MEETING_2026_09_17_QMD,
                            )
                            .in_subdirectory("team-sync"),
                        )
                        .add_file(
                            ScaffoldFileDef::static_text(
                                "team-meeting.qmd",
                                t::TEAM_MEETING_TEMPLATE_QMD,
                            )
                            .in_subdirectory("_quarto-hub-templates"),
                        ),
                )
            }
            Some("example-article") => {
                use templates::examples::article as t;
                Some(
                    ProjectScaffold::with_template(ProjectType::Default, "example-article")
                        .add_file(ScaffoldFileDef::static_text("_quarto.yml", t::QUARTO_YML))
                        .add_file(ScaffoldFileDef::static_text("index.qmd", t::INDEX_QMD))
                        .add_file(ScaffoldFileDef::static_text(
                            "figure-1.svg",
                            t::FIGURE_1_SVG,
                        ))
                        .add_file(ScaffoldFileDef::static_text(
                            "fork-icon.svg",
                            t::FORK_ICON_SVG,
                        )),
                )
            }
            Some("example-presentation") => {
                use templates::examples::presentation as t;
                Some(
                    ProjectScaffold::with_template(ProjectType::Default, "example-presentation")
                        .add_file(ScaffoldFileDef::static_text("_quarto.yml", t::QUARTO_YML))
                        .add_file(ScaffoldFileDef::static_text("index.qmd", t::INDEX_QMD))
                        .add_file(ScaffoldFileDef::static_text("styles.scss", t::STYLES_SCSS))
                        .add_file(ScaffoldFileDef::static_text(
                            "quarto-icon.svg",
                            t::QUARTO_ICON_SVG,
                        ))
                        .add_file(ScaffoldFileDef::static_text(
                            "sample-chart.svg",
                            t::SAMPLE_CHART_SVG,
                        ))
                        .add_file(ScaffoldFileDef::static_text(
                            "fork-icon.svg",
                            t::FORK_ICON_SVG,
                        )),
                )
            }
            Some(_) => None, // Unknown template
        },
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
                        ))
                        .add_file(ScaffoldFileDef::static_text(
                            "about.qmd",
                            templates::website::ABOUT_QMD,
                        ))
                        .add_file(ScaffoldFileDef::static_text(
                            "styles.css",
                            templates::website::STYLES_CSS,
                        )),
                ),
                Some("blog") => Some(
                    ProjectScaffold::with_template(ProjectType::Website, "blog")
                        .add_file(ScaffoldFileDef::template(
                            "_quarto.yml",
                            templates::blog::QUARTO_YML,
                        ))
                        .add_file(ScaffoldFileDef::template(
                            "index.qmd",
                            templates::blog::INDEX_QMD,
                        ))
                        .add_file(ScaffoldFileDef::static_text(
                            "about.qmd",
                            templates::blog::ABOUT_QMD,
                        ))
                        .add_file(ScaffoldFileDef::static_text(
                            "styles.css",
                            templates::website::STYLES_CSS,
                        ))
                        .add_file(
                            ScaffoldFileDef::static_text(
                                "_metadata.yml",
                                templates::blog::POSTS_METADATA_YML,
                            )
                            .in_subdirectory("posts"),
                        )
                        .add_file(
                            ScaffoldFileDef::template("index.qmd", templates::blog::WELCOME_QMD)
                                .in_subdirectory("posts/welcome"),
                        )
                        .add_file(
                            ScaffoldFileDef::binary(
                                "thumbnail.jpg",
                                templates::blog::THUMBNAIL_JPG,
                                "image/jpeg",
                            )
                            .in_subdirectory("posts/welcome"),
                        )
                        .add_file(
                            ScaffoldFileDef::template(
                                "index.qmd",
                                templates::blog::POST_WITH_CODE_QMD,
                            )
                            .in_subdirectory("posts/post-with-code"),
                        )
                        .add_file(
                            ScaffoldFileDef::binary(
                                "image.jpg",
                                templates::blog::IMAGE_JPG,
                                "image/jpeg",
                            )
                            .in_subdirectory("posts/post-with-code"),
                        ),
                ),
                Some("hub-placeholder") => Some(
                    ProjectScaffold::with_template(ProjectType::Website, "hub-placeholder")
                        .add_file(ScaffoldFileDef::static_text(
                            "_quarto.yml",
                            templates::hub_placeholder::QUARTO_YML,
                        ))
                        .add_file(ScaffoldFileDef::static_text(
                            "index.qmd",
                            templates::hub_placeholder::INDEX_QMD,
                        ))
                        .add_file(ScaffoldFileDef::static_text(
                            "qmd-changes.qmd",
                            templates::hub_placeholder::QMD_CHANGES_QMD,
                        )),
                ),
                // Seeded example (bd-3fwtdhil): fixed content, nothing interpolates.
                Some("example-website") => {
                    use templates::examples::website as t;
                    Some(
                        ProjectScaffold::with_template(ProjectType::Website, "example-website")
                            .add_file(ScaffoldFileDef::static_text("_quarto.yml", t::QUARTO_YML))
                            .add_file(ScaffoldFileDef::static_text("index.qmd", t::INDEX_QMD))
                            .add_file(ScaffoldFileDef::static_text(
                                "features.qmd",
                                t::FEATURES_QMD,
                            ))
                            .add_file(ScaffoldFileDef::static_text("about.qmd", t::ABOUT_QMD))
                            .add_file(ScaffoldFileDef::static_text("styles.css", t::STYLES_CSS))
                            .add_file(ScaffoldFileDef::static_text(
                                "fork-icon.svg",
                                t::FORK_ICON_SVG,
                            )),
                    )
                }
                Some(_) => None, // Unknown template
            }
        }
        ProjectType::Book => match target.template.as_deref() {
            None => Some(
                ProjectScaffold::new(ProjectType::Book)
                    .add_file(ScaffoldFileDef::template(
                        "_quarto.yml",
                        templates::book::QUARTO_YML,
                    ))
                    .add_file(ScaffoldFileDef::static_text(
                        "index.qmd",
                        templates::book::INDEX_QMD,
                    ))
                    .add_file(ScaffoldFileDef::static_text(
                        "intro.qmd",
                        templates::book::INTRO_QMD,
                    ))
                    .add_file(ScaffoldFileDef::static_text(
                        "summary.qmd",
                        templates::book::SUMMARY_QMD,
                    ))
                    .add_file(ScaffoldFileDef::static_text(
                        "references.qmd",
                        templates::book::REFERENCES_QMD,
                    ))
                    .add_file(ScaffoldFileDef::static_text(
                        "references.json",
                        templates::book::REFERENCES_JSON,
                    ))
                    .add_file(ScaffoldFileDef::binary(
                        "cover.png",
                        templates::book::COVER_PNG,
                        "image/png",
                    )),
            ),
            Some(_) => None, // Unknown template
        },
        // Not yet implemented
        ProjectType::Blog | ProjectType::Manuscript => None,
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

        let paths: Vec<_> = scaffold.files.iter().map(|f| f.path).collect();
        assert_eq!(paths, ["_quarto.yml", "index.qmd"]);
    }

    #[test]
    fn test_get_scaffold_website() {
        let target = ProjectTypeWithTemplate::new(ProjectType::Website);
        let scaffold = get_scaffold(&target).unwrap();

        let paths: Vec<_> = scaffold.files.iter().map(|f| f.path).collect();
        assert_eq!(
            paths,
            ["_quarto.yml", "index.qmd", "about.qmd", "styles.css"]
        );

        // about.qmd and styles.css carry no interpolation — they must be
        // static so they never fail template compilation.
        for f in &scaffold.files {
            if matches!(f.path, "about.qmd" | "styles.css") {
                assert!(
                    matches!(f.content, ScaffoldContent::StaticText(_)),
                    "{} should be static text",
                    f.path
                );
            }
        }
    }

    /// Every template in every implemented choice's scaffold must compile
    /// with the doctemplate engine, interpolate at least one context
    /// variable (a Template with none should be StaticText), and carry
    /// no EJS residue. (Static files are checked for EJS residue only.)
    #[test]
    fn test_all_scaffold_templates_compile() {
        for choice in crate::choices::implemented_choices() {
            let scaffold = get_scaffold(&choice.target)
                .unwrap_or_else(|| panic!("implemented choice '{}' has no scaffold", choice.id));
            for f in &scaffold.files {
                match f.content {
                    ScaffoldContent::Template(t) => {
                        quarto_doctemplate::Template::compile(t).unwrap_or_else(|e| {
                            panic!(
                                "template {} for '{}' failed to compile: {e}",
                                f.path, choice.id
                            )
                        });
                        assert!(
                            t.contains('$'),
                            "template {} for '{}' interpolates nothing — make it StaticText",
                            f.path,
                            choice.id
                        );
                        assert!(!t.contains("<%"), "EJS residue in {}", f.path);
                    }
                    ScaffoldContent::StaticText(t) => {
                        assert!(!t.contains("<%"), "EJS residue in {}", f.path);
                    }
                    ScaffoldContent::Binary { .. } => {}
                }
            }
        }
    }

    #[test]
    fn test_get_scaffold_blog() {
        let target = ProjectTypeWithTemplate::with_template(ProjectType::Website, "blog");
        let scaffold = get_scaffold(&target).expect("blog scaffold");

        let paths: Vec<_> = scaffold
            .files
            .iter()
            .map(|f| f.full_path().to_str().unwrap().replace('\\', "/"))
            .collect();
        assert_eq!(
            paths,
            [
                "_quarto.yml",
                "index.qmd",
                "about.qmd",
                "styles.css",
                "posts/_metadata.yml",
                "posts/welcome/index.qmd",
                "posts/welcome/thumbnail.jpg",
                "posts/post-with-code/index.qmd",
                "posts/post-with-code/image.jpg",
            ]
        );

        // The two post images are the first users of the Binary path.
        let binaries: Vec<_> = scaffold
            .files
            .iter()
            .filter(|f| matches!(f.content, ScaffoldContent::Binary { .. }))
            .map(|f| f.full_path())
            .collect();
        assert_eq!(binaries.len(), 2, "exactly the two post images");
    }

    #[test]
    fn test_get_scaffold_unknown_template() {
        let target = ProjectTypeWithTemplate::with_template(ProjectType::Website, "nonexistent");
        assert!(get_scaffold(&target).is_none());
    }

    #[test]
    fn test_get_scaffold_presentation_is_a_two_file_skeleton() {
        // The Presentation skeleton (bd-q33ylfxf): a `Default` project whose
        // two files are templates, so the typed title lands in both.
        let target = ProjectTypeWithTemplate::with_template(ProjectType::Default, "presentation");
        let scaffold = get_scaffold(&target).expect("presentation scaffold");
        let paths: Vec<_> = scaffold
            .files
            .iter()
            .map(|f| f.full_path().to_str().unwrap().replace('\\', "/"))
            .collect();
        assert_eq!(paths, ["_quarto.yml", "index.qmd"]);
        for f in &scaffold.files {
            assert!(
                matches!(f.content, ScaffoldContent::Template(_)),
                "{} must be a template",
                f.path
            );
        }
    }

    #[test]
    fn test_get_scaffold_default_unknown_template_is_none() {
        // The Default arm gained templates for the examples (bd-3fwtdhil);
        // an unknown one must fall through like it does for Website.
        let target = ProjectTypeWithTemplate::with_template(ProjectType::Default, "nonexistent");
        assert!(get_scaffold(&target).is_none());
    }

    #[test]
    fn test_get_scaffold_examples_are_static_and_complete() {
        // The four seeded examples (bd-3fwtdhil). Every file is static text:
        // the content carries its own titles, so nothing interpolates.
        let cases: [(ProjectType, &str, &[&str]); 4] = [
            (
                ProjectType::Default,
                "example-meeting-notes",
                &[
                    "_quarto.yml",
                    "index.qmd",
                    "tweaks.scss",
                    "team-sync/2026-09-17.qmd",
                    "_quarto-hub-templates/team-meeting.qmd",
                ],
            ),
            (
                ProjectType::Website,
                "example-website",
                &[
                    "_quarto.yml",
                    "index.qmd",
                    "features.qmd",
                    "about.qmd",
                    "styles.css",
                    "fork-icon.svg",
                ],
            ),
            (
                ProjectType::Default,
                "example-article",
                &["_quarto.yml", "index.qmd", "figure-1.svg", "fork-icon.svg"],
            ),
            (
                ProjectType::Default,
                "example-presentation",
                &[
                    "_quarto.yml",
                    "index.qmd",
                    "styles.scss",
                    "quarto-icon.svg",
                    "sample-chart.svg",
                    "fork-icon.svg",
                ],
            ),
        ];
        for (project_type, template, expected) in cases {
            let target = ProjectTypeWithTemplate::with_template(project_type, template);
            let scaffold =
                get_scaffold(&target).unwrap_or_else(|| panic!("no scaffold for {template}"));
            assert_eq!(scaffold.target.project_type, project_type, "{template}");
            let paths: Vec<_> = scaffold
                .files
                .iter()
                .map(|f| f.full_path().to_str().unwrap().replace('\\', "/"))
                .collect();
            assert_eq!(paths, expected, "{template}");
            for f in &scaffold.files {
                assert!(
                    matches!(f.content, ScaffoldContent::StaticText(_)),
                    "{template}: {} must be static text",
                    f.path
                );
            }
        }
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
