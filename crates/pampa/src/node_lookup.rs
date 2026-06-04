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
/// Two strategies, tried in order:
///
/// 1. **Exact value match** — `block.source_info() == target`.
///    Works for `Original` and `Substring` nodes (the majority of editable
///    text-bearing blocks).
///
/// 2. **`preimage_in` fallback** — when no exact match exists (e.g. `target`
///    is a `Generated` node from a resolved shortcode), we compute
///    `target.preimage_in(file_id)` to get the source byte range of the
///    invocation, then find the block whose own `preimage_in` contains
///    that range.
///
/// Returns the index into `ast.blocks`, or `None` if no block is found or
/// the target has no usable source-info (pure synthetic, non-contiguous
/// `Concat`, etc.).
///
/// Tiebreak: when multiple blocks match, return the one with the smallest
/// index (first occurrence).  In practice, distinct-range blocks never tie
/// on the exact-match path; ties on the preimage path mean the target falls
/// inside a coarser block, and the smallest-index heuristic is conservative.
///
/// v1 scope: searches only the top-level block list.  Nested blocks (inside
/// a fenced div, list item, etc.) are not reached; editing them is deferred.
pub fn lookup_block(ast: &Pandoc, target: &SourceInfo, file_id: FileId) -> Option<usize> {
    // Pass 1 — exact SourceInfo value equality.
    let exact: Vec<usize> = ast
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.source_info() == target)
        .map(|(i, _)| i)
        .collect();

    if !exact.is_empty() {
        return Some(exact[0]);
    }

    // Pass 2 — preimage_in fallback for Generated nodes.
    let target_range = target.preimage_in(file_id)?;

    let covering: Vec<usize> = ast
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let block_range = b.source_info().preimage_in(file_id)?;
            if block_range.start <= target_range.start && block_range.end >= target_range.end {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    covering.into_iter().next()
}
