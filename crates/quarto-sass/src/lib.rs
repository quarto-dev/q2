//! SASS compilation infrastructure for Quarto.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This crate provides:
//! - Core types (SassLayer, SassBundleLayers, SassBundle)
//! - Layer parsing from SCSS content with boundary markers
//! - Layer merging with correct precedence handling

mod error;
mod layer;
mod types;

pub use error::SassError;
pub use layer::{merge_layers, parse_layer, parse_layer_from_parts};
pub use types::{SassBundle, SassBundleDark, SassBundleLayers, SassLayer};
