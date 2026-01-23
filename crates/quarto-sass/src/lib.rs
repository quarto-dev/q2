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

pub mod bundle;
mod error;
mod layer;
pub mod resources;
pub mod themes;
mod types;

pub use bundle::{
    assemble_bootstrap, assemble_scss, assemble_with_theme, load_bootstrap_framework,
    load_quarto_layer, load_theme,
};
pub use error::SassError;
pub use layer::{merge_layers, parse_layer, parse_layer_from_parts};
pub use resources::{
    BOOTSTRAP_RESOURCES, EmbeddedResources, QUARTO_BOOTSTRAP_RESOURCES, RESOURCE_PATH_PREFIX,
    SASS_UTILS_RESOURCES, THEMES_RESOURCES, all_resources, default_load_paths,
};
pub use themes::{BuiltInTheme, load_theme_layer, resolve_theme};
pub use types::{SassBundle, SassBundleDark, SassBundleLayers, SassLayer};
