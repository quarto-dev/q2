//! SASS compilation infrastructure for Quarto.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This crate provides:
//! - Core types (SassLayer, SassBundleLayers, SassBundle)
//! - Layer parsing from SCSS content with boundary markers
//! - Layer merging with correct precedence handling
//! - Embedded Bootstrap 5.3.1 SCSS resources
//! - Bootswatch theme support
//! - Bundle assembly for compilation
//! - Theme configuration extraction from ConfigValue

pub mod bundle;
pub mod compile;
pub mod config;
mod error;
mod layer;
pub mod resources;
pub mod themes;
mod types;

pub use bundle::{
    assemble_bootstrap, assemble_scss, assemble_themes, assemble_with_theme,
    assemble_with_user_layers, load_bootstrap_framework, load_quarto_layer, load_theme,
};
pub use compile::{compile_css_from_config, compile_default_css, compile_theme_css};
pub use config::ThemeConfig;
pub use error::SassError;
pub use layer::{merge_layers, parse_layer, parse_layer_from_parts};
pub use resources::{
    BOOTSTRAP_RESOURCES, CombinedResources, EmbeddedResources, QUARTO_BOOTSTRAP_RESOURCES,
    RESOURCE_PATH_PREFIX, SASS_UTILS_RESOURCES, THEMES_RESOURCES, all_resources,
    default_load_paths,
};
pub use themes::{
    BuiltInTheme, ResolvedTheme, ThemeContext, ThemeLayerResult, ThemeSpec, load_custom_theme,
    load_quarto_customization_layer, load_theme_layer, process_theme_specs, resolve_theme,
    resolve_theme_spec,
};
pub use types::{SassBundle, SassBundleDark, SassBundleLayers, SassLayer};
