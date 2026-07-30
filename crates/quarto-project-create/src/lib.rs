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
fn template_context(
    title: &str,
    project_type: &str,
    template: Option<&str>,
    today: Option<time::Date>,
) -> TemplateContext {
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
    // Post-date stamping (blog scaffold, bd-r1by4u2a). Mirrors Q1:
    // the second post is dated today, the first three days earlier,
    // so a fresh blog's listing sorts sensibly under `date desc`.
    let today = today.unwrap_or_else(default_today);
    let first = today.checked_sub(time::Duration::days(3)).unwrap_or(today);
    ctx.insert("second-post-date", TemplateValue::String(iso_date(today)));
    ctx.insert("first-post-date", TemplateValue::String(iso_date(first)));
    ctx
}

/// Today's date. On wasm32 this reads the JS clock via time's
/// `wasm-bindgen` feature; on native, the system clock.
fn default_today() -> time::Date {
    time::OffsetDateTime::now_utc().date()
}

/// Format a date as `YYYY-MM-DD`.
fn iso_date(d: time::Date) -> String {
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    d.format(&fmt).expect("static format description")
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

    /// "Today" for date-stamped scaffold content (blog post dates).
    /// `None` (the default) reads the clock; tests pass a fixed date
    /// for determinism.
    pub today: Option<time::Date>,
}

impl CreateFromChoiceOptions {
    /// Create new options.
    pub fn new(choice_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            choice_id: choice_id.into(),
            title: title.into(),
            today: None,
        }
    }

    /// Pin "today" to a fixed date (deterministic tests).
    pub fn with_today(mut self, today: time::Date) -> Self {
        self.today = Some(today);
        self
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
    create_scaffolded_files(&scaffold, &options.title, options.today)
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
/// * `today` - Optional fixed "today" for date-stamped content
///   (`None` reads the clock)
///
/// # Returns
///
/// A list of `ScaffoldedFile` structs ready to be written to disk or VFS.
pub fn create_scaffolded_files(
    scaffold: &ProjectScaffold,
    title: &str,
    today: Option<time::Date>,
) -> Result<Vec<ScaffoldedFile>, CreateError> {
    let ctx = template_context(
        title,
        scaffold.target.project_type.id(),
        scaffold.target.template.as_deref(),
        today,
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

/// The starter `_brand.yml` written by `q2 use brand` when no source
/// is given (bd-1vlw8).
///
/// Returned as text rather than written, matching this crate's
/// no-filesystem contract — the caller (CLI or hub client) decides
/// where it goes.
pub fn starter_brand_yml() -> &'static str {
    templates::brand::BRAND_YML
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

    /// Forward-slash form of a scaffolded path (Windows uses `\`).
    fn norm(p: &std::path::Path) -> String {
        p.to_str().unwrap().replace('\\', "/")
    }

    /// Panic-on-missing lookup of a text file's content in a scaffold result.
    fn file_content<'a>(files: &'a [ScaffoldedFile], path: &str) -> &'a str {
        files
            .iter()
            .find_map(|f| match f {
                ScaffoldedFile::Text { path: p, content } if norm(p) == path => {
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

        let paths: Vec<_> = files.iter().map(|f| norm(f.path())).collect();
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

        let paths: Vec<_> = files.iter().map(|f| norm(f.path())).collect();
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
    fn starter_brand_is_valid_yaml_with_usable_defaults() {
        let yml = parse_yaml(starter_brand_yml());

        // The palette-plus-reference shape is the thing we are teaching
        // by example; if it regresses into literal hex values in the
        // slots, the starter stops demonstrating the idea.
        assert_eq!(yml["color"]["palette"]["accent"].as_str(), Some("#2c6fbb"));
        assert_eq!(yml["color"]["primary"].as_str(), Some("accent"));
        assert_eq!(
            yml["typography"]["base"]["family"].as_str(),
            Some("Open Sans")
        );

        // Logos are commented out: an uncommented `logo:` would point at
        // image files that do not exist, and every render would warn.
        assert!(yml.get("logo").is_none(), "logo slots must stay commented");
    }

    /// Extract and parse the YAML front matter of a `---`-fenced qmd.
    fn front_matter(src: &str) -> serde_yaml::Value {
        let rest = src
            .strip_prefix("---\n")
            .unwrap_or_else(|| panic!("no front matter fence in:\n{src}"));
        let end = rest.find("\n---").expect("unterminated front matter");
        serde_yaml::from_str(&rest[..end]).expect("front matter must be valid YAML")
    }

    /// Panic-on-missing lookup of a binary file in a scaffold result.
    fn binary_file<'a>(files: &'a [ScaffoldedFile], path: &str) -> (&'a [u8], &'a str) {
        files
            .iter()
            .find_map(|f| match f {
                ScaffoldedFile::Binary {
                    path: p,
                    content,
                    mime_type,
                } if norm(p) == path => Some((content.as_slice(), mime_type.as_str())),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected binary file {path} in scaffold output"))
    }

    #[test]
    fn blog_scaffold_produces_q1_familiar_file_set() {
        let files = create_project_from_choice(
            CreateFromChoiceOptions::new("blog", "My Blog")
                .with_today(time::macros::date!(2026 - 07 - 29)),
        )
        .unwrap();

        let paths: Vec<_> = files.iter().map(|f| norm(f.path())).collect();
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

        // ---- _quarto.yml --------------------------------------------
        let yml_src = file_content(&files, "_quarto.yml");
        let yml = parse_yaml(yml_src);
        assert_eq!(yml["project"]["type"].as_str(), Some("website"));
        assert_eq!(yml["website"]["title"].as_str(), Some("My Blog"));
        assert_eq!(
            yml["website"]["description"].as_str(),
            Some("A blog built with Quarto")
        );
        // Q2's feed completion silently no-ops without a site-url;
        // the scaffold must ship the Q1 placeholder.
        assert!(
            yml["website"]["site-url"]
                .as_str()
                .is_some_and(|u| u.starts_with("https://")),
            "site-url placeholder required for RSS"
        );
        let right = &yml["website"]["navbar"]["right"];
        assert_eq!(right[0].as_str(), Some("about.qmd"));
        assert_eq!(right[1]["icon"].as_str(), Some("github"));
        assert_eq!(right[2]["icon"].as_str(), Some("bluesky"));
        assert_eq!(yml["format"]["html"]["theme"].as_str(), Some("cosmo"));
        assert_eq!(yml["format"]["html"]["css"].as_str(), Some("styles.css"));
        // styles.css must be a declared resource (bd-b87tmmi4).
        assert_eq!(yml["project"]["resources"][0].as_str(), Some("styles.css"));
        // No brand marker (Q-14-1), no editor:, no freeze anywhere.
        assert!(!yml_src.contains("brand"), "no brand marker:\n{yml_src}");
        assert!(!yml_src.contains("editor"), "no editor knob:\n{yml_src}");

        // ---- index.qmd (the listing page) ---------------------------
        let index = file_content(&files, "index.qmd");
        let fm = front_matter(index);
        assert_eq!(fm["title"].as_str(), Some("My Blog"));
        assert_eq!(fm["listing"]["contents"].as_str(), Some("posts"));
        assert_eq!(fm["listing"]["sort"].as_str(), Some("date desc"));
        assert_eq!(fm["listing"]["type"].as_str(), Some("default"));
        assert_eq!(fm["listing"]["categories"].as_bool(), Some(true));
        assert_eq!(fm["listing"]["feed"].as_bool(), Some(true));
        assert_eq!(fm["listing"]["sort-ui"].as_bool(), Some(false));
        assert_eq!(fm["listing"]["filter-ui"].as_bool(), Some(false));
        assert_eq!(fm["page-layout"].as_str(), Some("full"));
        assert_eq!(fm["title-block-banner"].as_bool(), Some(true));

        // ---- posts/_metadata.yml ------------------------------------
        let meta = file_content(&files, "posts/_metadata.yml");
        let meta_yml = parse_yaml(meta);
        assert_eq!(meta_yml["title-block-banner"].as_bool(), Some(true));
        // Q2 has no freeze implementation (bd-mx5x609r); the knob is
        // deliberately dropped from Q1's shape.
        assert!(!meta.contains("freeze"), "no freeze knob:\n{meta}");

        // ---- posts --------------------------------------------------
        let welcome = file_content(&files, "posts/welcome/index.qmd");
        let wfm = front_matter(welcome);
        assert_eq!(wfm["title"].as_str(), Some("Welcome To My Blog"));
        assert_eq!(wfm["author"].as_str(), Some("Tristan O'Malley"));
        assert_eq!(wfm["date"].as_str(), Some("2026-07-26"), "today - 3 days");
        assert_eq!(wfm["categories"][0].as_str(), Some("news"));
        assert!(welcome.contains("![](thumbnail.jpg)"));

        let post = file_content(&files, "posts/post-with-code/index.qmd");
        let pfm = front_matter(post);
        assert_eq!(pfm["title"].as_str(), Some("Post With Code"));
        assert_eq!(pfm["author"].as_str(), Some("Harlow Malloc"));
        assert_eq!(pfm["date"].as_str(), Some("2026-07-29"), "today");
        assert_eq!(pfm["image"].as_str(), Some("image.jpg"));
        assert_eq!(pfm["categories"][1].as_str(), Some("code"));

        // ---- binaries (the ScaffoldContent::Binary path) ------------
        for path in [
            "posts/welcome/thumbnail.jpg",
            "posts/post-with-code/image.jpg",
        ] {
            let (bytes, mime) = binary_file(&files, path);
            assert_eq!(mime, "image/jpeg");
            assert!(
                bytes.len() > 1000 && bytes.starts_with(&[0xFF, 0xD8]),
                "{path} must carry real JPEG bytes"
            );
        }

        // ---- about.qmd (simplified — no Q2 about-page feature) ------
        let about = file_content(&files, "about.qmd");
        assert!(about.contains("title: \"About\""));
        assert!(about.contains("About this blog"));
        assert!(
            !about.contains("about:") && !about.contains("profile.jpg"),
            "Q1's about: block is dropped until bd-5xmy5lle lands:\n{about}"
        );

        // No template residue anywhere.
        for f in &files {
            if let ScaffoldedFile::Text { path, content } = f {
                assert!(
                    !content.contains('$') && !content.contains("<%"),
                    "template residue in {}:\n{content}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn blog_scaffold_defaults_today_to_now() {
        // Without an explicit `today`, the crate stamps the current
        // date. Only sanity-check the shape (YYYY-MM-DD) and the
        // three-day stagger, to keep the test clock-independent.
        let files =
            create_project_from_choice(CreateFromChoiceOptions::new("blog", "My Blog")).unwrap();
        let welcome = front_matter(file_content(&files, "posts/welcome/index.qmd"));
        let post = front_matter(file_content(&files, "posts/post-with-code/index.qmd"));
        let wdate = welcome["date"].as_str().unwrap();
        let pdate = post["date"].as_str().unwrap();
        let iso = |s: &str| {
            time::Date::parse(
                s,
                &time::macros::format_description!("[year]-[month]-[day]"),
            )
            .unwrap_or_else(|e| panic!("bad date {s}: {e}"))
        };
        assert_eq!(iso(pdate) - iso(wdate), time::Duration::days(3));
    }

    #[test]
    fn blog_choice_is_implemented() {
        let ids: Vec<_> = implemented_choices().into_iter().map(|c| c.id).collect();
        assert!(ids.contains(&"blog".to_string()), "choices: {ids:?}");
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
        // "manuscript" is defined but marked as unimplemented
        let result =
            create_project_from_choice(CreateFromChoiceOptions::new("manuscript", "My Paper"));
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
