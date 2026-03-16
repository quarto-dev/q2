/*
 * extension/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Quarto extension discovery, parsing, and metadata contribution.
 */

//! Quarto extension support.
//!
//! Extensions are discovered from `_extensions/` directories in the project
//! hierarchy and parsed from `_extension.yml` files. They can contribute
//! format-specific metadata, filters, shortcodes, and other resources.

pub mod discover;
pub mod read;
pub mod types;

pub use discover::{discover_extensions, find_extension};
pub use read::read_extension;
pub use types::{Contributes, Extension, ExtensionFilter, ExtensionId};
