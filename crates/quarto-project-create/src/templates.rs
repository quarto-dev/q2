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

/// Scaffold contents for the hub-only welcome tour
/// (`website:hub-placeholder`, bd-d147nkqx). All static: the tour's
/// documents carry fixed titles rather than the project name, so
/// nothing here interpolates.
pub mod hub_placeholder {
    /// Static `_quarto.yml` for the welcome tour.
    pub const QUARTO_YML: &str =
        include_str!("../resources/templates/website/hub-placeholder/_quarto.yml");

    /// Static landing page of the tour.
    pub const INDEX_QMD: &str =
        include_str!("../resources/templates/website/hub-placeholder/index.qmd");

    /// Static tour page on qmd syntax changes, linked from `index.qmd`.
    pub const QMD_CHANGES_QMD: &str =
        include_str!("../resources/templates/website/hub-placeholder/qmd-changes.qmd");
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

/// Scaffold contents for the book project type (book-projects P7).
/// Chapter content and `references.bib` are one-time copies of Q1's
/// `resources/projects/book/` defaults (per the external-sources
/// policy; see `templates::blog`'s doc comment for the same pattern);
/// `_quarto.yml` is a doctemplate port of Q1's `_quarto.ejs.yml`.
pub mod book {
    /// `_quarto.yml` template for book projects.
    pub const QUARTO_YML: &str = include_str!("../resources/templates/book/_quarto.yml.template");

    /// Unnumbered preface chapter (`{.unnumbered}` first heading).
    pub const INDEX_QMD: &str = include_str!("../resources/templates/book/index.qmd");

    /// Introduction chapter; cites `references.bib`'s one entry.
    pub const INTRO_QMD: &str = include_str!("../resources/templates/book/intro.qmd");

    /// Summary chapter.
    pub const SUMMARY_QMD: &str = include_str!("../resources/templates/book/summary.qmd");

    /// Unnumbered references chapter: no code, just the `{#refs}` div
    /// the book's merged bibliography is spliced into.
    pub const REFERENCES_QMD: &str = include_str!("../resources/templates/book/references.qmd");

    /// Starter bibliography, CSL-JSON (one entry, cited from
    /// `intro.qmd`). Q2's citeproc filter only parses CSL-JSON
    /// (`pampa::citeproc_filter::load_bibliography`) — unlike Q1,
    /// there is no BibTeX parser, so this is JSON content rather than
    /// Q1's `references.bib`, even though the same entry.
    pub const REFERENCES_JSON: &str = include_str!("../resources/templates/book/references.json");

    /// Cover image (embedded binary), copied from Q1's book resources.
    pub const COVER_PNG: &[u8] = include_bytes!("../resources/templates/book/cover.png");
}

/// The four example projects seeded into a new user's "Examples /
/// Templates" collection (bd-3fwtdhil). All static: each example is a short
/// instructional project whose documents carry fixed titles, so nothing here
/// interpolates the project name. Content was authored live on
/// quarto-hub.com and copied in verbatim; the plan records the source
/// project ids.
pub mod examples {
    /// Meeting Notes: a home page listing dated meeting pages, one
    /// in-progress meeting, and a file template for the New File dialog.
    pub mod meeting_notes {
        pub const QUARTO_YML: &str =
            include_str!("../resources/templates/examples/meeting-notes/_quarto.yml");
        pub const INDEX_QMD: &str =
            include_str!("../resources/templates/examples/meeting-notes/index.qmd");
        pub const TWEAKS_SCSS: &str =
            include_str!("../resources/templates/examples/meeting-notes/tweaks.scss");
        pub const MEETING_2026_09_17_QMD: &str =
            include_str!("../resources/templates/examples/meeting-notes/team-sync/2026-09-17.qmd");
        pub const TEAM_MEETING_TEMPLATE_QMD: &str = include_str!(
            "../resources/templates/examples/meeting-notes/_quarto-hub-templates/team-meeting.qmd"
        );
    }

    /// Website: three pages with a navbar; the home page carries the
    /// editorial-marks review demo and Features tours page-level features.
    pub mod website {
        pub const QUARTO_YML: &str =
            include_str!("../resources/templates/examples/website/_quarto.yml");
        pub const INDEX_QMD: &str =
            include_str!("../resources/templates/examples/website/index.qmd");
        pub const FEATURES_QMD: &str =
            include_str!("../resources/templates/examples/website/features.qmd");
        pub const ABOUT_QMD: &str =
            include_str!("../resources/templates/examples/website/about.qmd");
        pub const STYLES_CSS: &str =
            include_str!("../resources/templates/examples/website/styles.css");
        pub const FORK_ICON_SVG: &str =
            include_str!("../resources/templates/examples/website/fork-icon.svg");
    }

    /// Article: one document with a title block, citations (inline
    /// references), an equation, a theorem, a figure, a table, and
    /// cross-references.
    pub mod article {
        pub const QUARTO_YML: &str =
            include_str!("../resources/templates/examples/article/_quarto.yml");
        pub const INDEX_QMD: &str =
            include_str!("../resources/templates/examples/article/index.qmd");
        pub const FIGURE_1_SVG: &str =
            include_str!("../resources/templates/examples/article/figure-1.svg");
        pub const FORK_ICON_SVG: &str =
            include_str!("../resources/templates/examples/article/fork-icon.svg");
    }

    /// Presentation: a short reveal.js deck with a custom SCSS theme, a
    /// footer logo, and a sample chart.
    pub mod presentation {
        pub const QUARTO_YML: &str =
            include_str!("../resources/templates/examples/presentation/_quarto.yml");
        pub const INDEX_QMD: &str =
            include_str!("../resources/templates/examples/presentation/index.qmd");
        pub const STYLES_SCSS: &str =
            include_str!("../resources/templates/examples/presentation/styles.scss");
        pub const QUARTO_ICON_SVG: &str =
            include_str!("../resources/templates/examples/presentation/quarto-icon.svg");
        pub const SAMPLE_CHART_SVG: &str =
            include_str!("../resources/templates/examples/presentation/sample-chart.svg");
        pub const FORK_ICON_SVG: &str =
            include_str!("../resources/templates/examples/presentation/fork-icon.svg");
    }
}

/// Scaffold contents for the Presentation skeleton (`default:presentation`,
/// bd-q33ylfxf): a reveal.js deck with a title slide and a few empty slides.
/// Both files interpolate `$title$`.
pub mod presentation {
    /// `_quarto.yml` template for presentation projects.
    pub const QUARTO_YML: &str =
        include_str!("../resources/templates/presentation/_quarto.yml.template");

    /// `index.qmd` template: the deck itself.
    pub const INDEX_QMD: &str =
        include_str!("../resources/templates/presentation/index.qmd.template");
}
