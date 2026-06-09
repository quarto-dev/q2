/*
 * node_lookup.rs
 *
 * Destination-node lookup for apply_node_edit (Phase 2 of the
 * target-incremental-writes plan, 2026-06-04).
 *
 * Given an untransformed AST and a SourceInfo value from the transformed AST,
 * finds the block in the untransformed AST that should receive the edit.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use crate::pandoc::Pandoc;
use quarto_source_map::{FileId, SourceInfo};

/// Find the top-level block in `ast` that corresponds to `target`.
///
/// **Exact value match only** — `block.source_info() == target`.
/// Works for `Original` nodes (the only type the v1 edit surface gates through
/// `decode_compact_source_info`).  `Generated` nodes are rejected upstream by
/// the type check, so they never reach this function; a covering-range fallback
/// would only silently replace a container and is therefore removed (Plan 2b).
///
/// Returns the index into `ast.blocks`, or `None` if no block is found.
///
/// Tiebreak: when multiple blocks match, return the one with the smallest
/// index (first occurrence).  Distinct-range blocks never tie on the
/// exact-match path in practice.
///
/// v1 scope: searches only the top-level block list.  Nested blocks (inside
/// a fenced div, list item, etc.) are not reached; editing them is deferred.
pub fn lookup_block(ast: &Pandoc, target: &SourceInfo, _file_id: FileId) -> Option<usize> {
    ast.blocks
        .iter()
        .enumerate()
        .find(|(_, b)| b.source_info() == target)
        .map(|(i, _)| i)
}
