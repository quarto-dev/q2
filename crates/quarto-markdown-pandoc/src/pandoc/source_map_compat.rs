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

    SourceInfo::from_range(
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

/// Convert old pandoc::location::SourceInfo to new quarto-source-map::SourceInfo.
///
/// This is a bridge function for gradual migration. It converts the old SourceInfo
/// (with filename_index) to the new SourceInfo (with FileId) using ASTContext.
///
/// # Arguments
/// * `old_info` - The old SourceInfo from pandoc::location
/// * `ctx` - The ASTContext to resolve filename_index to FileId
///
/// # Returns
/// A new SourceInfo with Original mapping to the appropriate file
pub fn old_to_new_source_info(
    old_info: &crate::pandoc::location::SourceInfo,
    ctx: &ASTContext,
) -> SourceInfo {
    // Convert filename_index to FileId
    // If the old info has a filename_index, try to get the corresponding FileId
    // Otherwise, use the primary file or FileId(0) as fallback
    let file_id = if let Some(filename_idx) = old_info.filename_index {
        // Try to map filename_index to FileId
        // For now, we'll use the primary file as a reasonable default
        // TODO: In Phase 3, we'll have proper mapping from filename_index to FileId
        ctx.primary_file_id().unwrap_or(FileId(filename_idx))
    } else {
        ctx.primary_file_id().unwrap_or(FileId(0))
    };

    // Convert the Range (both use the same Location structure)
    SourceInfo::from_range(
        file_id,
        Range {
            start: Location {
                offset: old_info.start_offset(),
                row: old_info.range.start.row,
                column: old_info.range.start.column,
            },
            end: Location {
                offset: old_info.end_offset(),
                row: old_info.range.end.row,
                column: old_info.range.end.column,
            },
        },
    )
}

/// Convert quarto-source-map::SourceInfo to a quarto_source_map::Range, with a fallback if mapping fails.
///
/// This is for use with PandocNativeIntermediate which uses quarto_source_map::Range.
/// Provides a fallback Range with zero row/column values if the mapping fails.
///
/// # Arguments
/// * `source_info` - The SourceInfo to convert
/// * `ctx` - The ASTContext containing the source context
///
/// # Returns
/// A quarto_source_map::Range with row/column information if available, or a Range with offsets only
pub fn source_info_to_qsm_range_or_fallback(
    source_info: &SourceInfo,
    ctx: &ASTContext,
) -> quarto_source_map::Range {
    let start_mapped = source_info.map_offset(0, &ctx.source_context);
    let end_mapped = source_info.map_offset(source_info.length(), &ctx.source_context);

    match (start_mapped, end_mapped) {
        (Some(start), Some(end)) => quarto_source_map::Range {
            start: start.location,
            end: end.location,
        },
        _ => quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: source_info.start_offset(),
                row: 0,
                column: 0,
            },
            end: quarto_source_map::Location {
                offset: source_info.end_offset(),
                row: 0,
                column: 0,
            },
        },
    }
}

// Note: Tests for these functions will be validated through integration tests
// when they're used in actual parsing modules. The tree-sitter-qmd parser
// setup is too complex to mock in unit tests here.
