/*
 * source_map_compat.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Compatibility helpers for converting tree-sitter Nodes to quarto-source-map types.
//!
//! This module provides bridge functions to convert from tree-sitter's Node type
//! to quarto-source-map's SourceInfo, enabling gradual migration from the old
//! pandoc::location types.

use quarto_source_map::{FileId, Location, Range, SourceInfo};
use tree_sitter::Node;

use crate::pandoc::ast_context::ASTContext;

/// Convert a tree-sitter Node to a SourceInfo with an explicit FileId.
///
/// This is the low-level conversion function that directly translates tree-sitter
/// positions to quarto-source-map coordinates.
///
/// # Arguments
/// * `node` - The tree-sitter Node to convert
/// * `file_id` - The FileId of the source file this node comes from
///
/// # Returns
/// A SourceInfo with Original mapping to the specified file
pub fn node_to_source_info(node: &Node, file_id: FileId) -> SourceInfo {
    let start_pos = node.start_position();
    let end_pos = node.end_position();

    SourceInfo::original(
        file_id,
        Range {
            start: Location {
                offset: node.start_byte(),
                row: start_pos.row,
                column: start_pos.column,
            },
            end: Location {
                offset: node.end_byte(),
                row: end_pos.row,
                column: end_pos.column,
            },
        },
    )
}

/// Convert a tree-sitter Node to a SourceInfo using the primary file from ASTContext.
///
/// This is the high-level conversion function that uses the context's primary file.
/// Most parsing code should use this variant.
///
/// # Arguments
/// * `node` - The tree-sitter Node to convert
/// * `ctx` - The ASTContext containing the source context
///
/// # Returns
/// A SourceInfo with Original mapping to the context's primary file.
/// If the context has no primary file, uses FileId(0) as a fallback.
pub fn node_to_source_info_with_context(node: &Node, ctx: &ASTContext) -> SourceInfo {
    let file_id = ctx.primary_file_id().unwrap_or(FileId(0));
    node_to_source_info(node, file_id)
}

// Note: Tests for these functions will be validated through integration tests
// when they're used in actual parsing modules. The tree-sitter-qmd parser
// setup is too complex to mock in unit tests here.
