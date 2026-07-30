/*
 * templates.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Embedded scaffold file contents.
 *
 * Files are embedded at compile time via `include_str!()`, which works
 * for both native and WASM targets. `*.template` files are doctemplate
 * (Pandoc template syntax) sources rendered with project data; the
 * rest are static files copied as-is.
 *
 * The single registry mapping these constants to scaffold file sets is
 * `scaffold::get_scaffold` — there is deliberately no second, parallel
 * per-project-type file list here.
 */

/// Scaffold contents for the default project type.
pub mod default {
    /// `_quarto.yml` template for default projects.
    pub const QUARTO_YML: &str =
        include_str!("../resources/templates/default/_quarto.yml.template");

    /// Starter `index.qmd` template for default projects.
    pub const INDEX_QMD: &str = include_str!("../resources/templates/default/index.qmd.template");
}

/// Scaffold contents for `q2 use brand` (bd-1vlw8).
///
/// Not a project type — a single starter file added to an *existing*
/// project. It lives here rather than in the `quarto` binary so the
/// hub client can offer the same starter brand without duplicating it.
pub mod brand {
    /// Starter `_brand.yml`. Static, not a template: nothing in a
    /// starter brand depends on the project's title or type, and a
    /// literal file is what the user will read and edit.
    pub const BRAND_YML: &str = include_str!("../resources/templates/brand/_brand.yml");
}

/// Scaffold contents for the blog template (`website:blog`,
/// bd-r1by4u2a). The two post images are the first users of
/// `ScaffoldContent::Binary`; they are one-time copies of Q1's
/// `resources/projects/website/templates/blog/` images (per the
/// external-sources policy).
pub mod blog {
    /// `_quarto.yml` template for blog projects.
    pub const QUARTO_YML: &str =
        include_str!("../resources/templates/website/blog/_quarto.yml.template");

    /// Listing-page `index.qmd` template for blog projects.
    pub const INDEX_QMD: &str =
        include_str!("../resources/templates/website/blog/index.qmd.template");

    /// Static `about.qmd` page for blog projects.
    pub const ABOUT_QMD: &str = include_str!("../resources/templates/website/blog/about.qmd");

    /// Static directory metadata for `posts/` (banner title blocks).
    /// Q1's `freeze: true` entry is deliberately dropped — Q2 has no
    /// freeze implementation (bd-mx5x609r).
    pub const POSTS_METADATA_YML: &str =
        include_str!("../resources/templates/website/blog/posts/_metadata.yml");

    /// Welcome post template (interpolates `$first-post-date$`).
    pub const WELCOME_QMD: &str =
        include_str!("../resources/templates/website/blog/posts/welcome/index.qmd.template");

    /// Welcome post thumbnail (embedded binary).
    pub const THUMBNAIL_JPG: &[u8] =
        include_bytes!("../resources/templates/website/blog/posts/welcome/thumbnail.jpg");

    /// Post-with-code template (interpolates `$second-post-date$`).
    pub const POST_WITH_CODE_QMD: &str =
        include_str!("../resources/templates/website/blog/posts/post-with-code/index.qmd.template");

    /// Post-with-code listing image (embedded binary).
    pub const IMAGE_JPG: &[u8] =
        include_bytes!("../resources/templates/website/blog/posts/post-with-code/image.jpg");
}

/// Scaffold contents for the website project type.
pub mod website {
    /// `_quarto.yml` template for website projects.
    pub const QUARTO_YML: &str =
        include_str!("../resources/templates/website/_quarto.yml.template");

    /// `index.qmd` template for website projects.
    pub const INDEX_QMD: &str = include_str!("../resources/templates/website/index.qmd.template");

    /// Static `about.qmd` page for website projects.
    pub const ABOUT_QMD: &str = include_str!("../resources/templates/website/about.qmd");

    /// Static starter stylesheet for website projects.
    pub const STYLES_CSS: &str = include_str!("../resources/templates/website/styles.css");
}
