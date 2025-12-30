/*
 * mod.rs
 * Copyright (c) 2025 Posit, PBC
 */

pub mod ast_context;
pub mod location;
pub mod meta;
pub mod shortcode;
pub mod treesitter;
pub mod treesitter_utils;

// Re-export all types from quarto-pandoc-types
pub use quarto_pandoc_types::*;

// Re-export modules from quarto-pandoc-types for backward compatibility with module paths
pub use quarto_pandoc_types::attr;
pub use quarto_pandoc_types::block;
pub use quarto_pandoc_types::caption;
pub use quarto_pandoc_types::inline;
pub use quarto_pandoc_types::list;
pub use quarto_pandoc_types::pandoc;
pub use quarto_pandoc_types::table;

// Re-export parsing and conversion functions from local modules
// These are public API used by integration tests and external crates
// Phase 5: Removed legacy MetaValueWithSourceInfo-based functions
#[allow(unused_imports)]
pub use crate::pandoc::meta::{rawblock_to_config_value, yaml_to_config_value};
#[allow(unused_imports)]
pub use crate::pandoc::shortcode::shortcode_to_span;

// Re-export from local modules that stay here
pub use crate::pandoc::ast_context::ASTContext;
pub use crate::pandoc::treesitter::treesitter_to_pandoc;
