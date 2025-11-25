/*
 * lib.rs
 *
 * Copyright (c) 2025 Posit, PBC
 *
 * quarto-treesitter-ast: Generic tree-sitter AST utilities for Quarto parsers.
 *
 * This crate provides shared infrastructure for working with tree-sitter
 * parse trees, including:
 *
 * - Generic traversal utilities (top-down and bottom-up)
 * - (Future) Error table types and lookup
 * - (Future) Tree-sitter log observer for error state capture
 * - (Future) Diagnostic message generation from parse errors
 */

pub mod traversals;

// Re-export commonly used items at crate root
pub use traversals::{
    bottomup_traverse_concrete_tree, bottomup_traverse_concrete_tree_no_context,
    topdown_traverse_concrete_tree, BottomUpTraversePhase, TraversePhase,
};
