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
