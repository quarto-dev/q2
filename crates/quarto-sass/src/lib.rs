//! SASS compilation infrastructure for Quarto.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This crate provides:
//! - Core types (SassLayer)
//! - Layer parsing from SCSS content with boundary markers
//! - Layer merging with correct precedence handling
//! - Embedded Bootstrap 5.3.1 SCSS resources
//! - Bootswatch theme support
//! - Bundle assembly for compilation
//! - Theme configuration extraction from ConfigValue

pub mod brand_layer;
pub mod bundle;
pub mod compile;
pub mod config;
mod error;
mod layer;
pub mod resources;
pub mod themes;
mod types;

/// SHA-256 hash (first 16 hex chars) of all `.scss` files under `resources/scss/`.
///
/// Computed at build time. Changes to any built-in SCSS resource (Bootstrap,
/// Quarto customizations, built-in themes) will produce a different hash.
/// Exposed for callers that specifically need "did a SCSS file change?".
///
/// For cache-invalidation use, prefer [`CSS_BUILD_ID`] — it also covers
/// Rust-side changes to SCSS assembly logic.
pub const SCSS_RESOURCES_HASH: &str =
    include_str!(concat!(env!("OUT_DIR"), "/scss_resources_hash.txt"));

/// SHA-256 hash (first 16 hex chars) combining [`SCSS_RESOURCES_HASH`]
/// with a hash of every `.rs` file under `crates/quarto-sass/src/`.
///
/// Use this for CSS-cache invalidation (e.g., the IndexedDB
/// cross-session cache in `CompileThemeCssStage`). It changes whenever
/// either a `.scss` resource OR any Rust file that influences SCSS
/// assembly changes. That covers the full surface of inputs that can
/// affect the compiled CSS.
///
/// Why this matters: `SCSS_RESOURCES_HASH` alone is not enough.
/// Modifications to `compile_default_css`, `assemble_theme_scss`, or
/// the layer-loading bundle — anything that changes *which* layers
/// get assembled or *how* — don't touch any `.scss` file, so the
/// SCSS hash stays stable. A persistent IndexedDB cache keyed only
/// on `SCSS_RESOURCES_HASH` would then serve pre-change CSS to users
/// who upgrade their WASM binary. Including the Rust sources in the
/// hash fixes that by construction. See
/// `claude-notes/plans/2026-04-20-syntax-highlighting-phase-3.md`
/// for the incident that motivated this.
pub const CSS_BUILD_ID: &str = include_str!(concat!(env!("OUT_DIR"), "/css_build_id.txt"));

pub use brand_layer::brand_to_layers;
pub use bundle::{KNOWN_HIGHLIGHT_PALETTES, is_known_highlight_palette};
pub use bundle::{
    REVEAL_BUILTIN_THEMES, assemble_bootstrap, assemble_reveal_scss, assemble_scss,
    assemble_themes, assemble_with_theme, assemble_with_user_layers, load_bootstrap_framework,
    load_quarto_layer, load_quarto_reveal_layer, load_reveal_framework, load_reveal_theme_layer,
    load_theme, load_title_block_layer, resolve_reveal_theme_name,
};
pub use compile::{
    assemble_theme_scss, compile_css_from_config, compile_default_css, compile_reveal_theme_css,
    compile_theme_css, compile_with_doc_vars,
};
pub use config::{
    DarkThemeConfig, HighlightStyle, ResolvedThemeConfig, ThemeConfig, resolve_brand,
    resolve_brand_layers,
};
pub use error::SassError;
pub use layer::{merge_layers, parse_layer, parse_layer_from_parts};
pub use resources::{
    BOOTSTRAP_RESOURCES, CombinedResources, EmbeddedResources, QUARTO_BOOTSTRAP_RESOURCES,
    RESOURCE_PATH_PREFIX, SASS_UTILS_RESOURCES, TEMPLATES_RESOURCES, THEMES_RESOURCES,
    all_resources, default_load_paths,
};
pub use themes::{
    BuiltInTheme, ResolvedTheme, ThemeContext, ThemeLayerResult, ThemeSpec, load_custom_theme,
    load_quarto_customization_layer, load_theme_layer, process_theme_specs, resolve_theme,
    resolve_theme_spec,
};
pub use types::SassLayer;
