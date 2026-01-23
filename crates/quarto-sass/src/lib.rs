//! SASS compilation infrastructure for Quarto.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This crate provides:
//! - Core types (SassLayer, SassBundleLayers, SassBundle)
//! - Layer parsing from SCSS content with boundary markers
//! - Layer merging with correct precedence handling
//! - Embedded Bootstrap 5.3.1 SCSS resources

mod error;
mod layer;
pub mod resources;
mod types;

pub use error::SassError;
pub use layer::{merge_layers, parse_layer, parse_layer_from_parts};
pub use resources::{
    BOOTSTRAP_RESOURCES, EmbeddedResources, RESOURCE_PATH_PREFIX, default_load_paths,
};
pub use types::{SassBundle, SassBundleDark, SassBundleLayers, SassLayer};
