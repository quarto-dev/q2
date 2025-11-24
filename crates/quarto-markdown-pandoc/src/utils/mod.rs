/*
 * mod.rs
 * Copyright (c) 2025 Posit, PBC
 */

pub mod autoid;
pub mod concrete_tree_depth;
pub mod diagnostic_collector;
pub mod output;
pub mod text;
pub mod trim_source_location;

// Note: tree_sitter_log_observer functionality has been moved to quarto-parse-errors crate.
// Import from quarto_parse_errors::{TreeSitterLogObserver, TreeSitterLogObserverTrait, ...} instead.
