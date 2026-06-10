/*
 * node_lookup.rs
 *
 * Destination-node lookup for apply_node_edit (Phase 2 of the
 * target-incremental-writes plan, 2026-06-04; Plan 3 recursive extension
 * 2026-06-09).
 *
 * Given an untransformed AST and a SourceInfo value from the transformed AST,
 * finds the block in the untransformed AST that should receive the edit, and
 * returns a `NodePath` describing the route from the root to that block.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use crate::pandoc::Pandoc;
use crate::pandoc::block::{Block, Blocks};
use quarto_source_map::{FileId, SourceInfo};

/// One step along the path from a top-level block to a nested target.
///
/// The first `usize` in each variant is the **container's index in the
/// *current* `Blocks` level** — needed by `resolve_to_blocks` to select
/// which block to enter.
#[derive(Debug, Clone, PartialEq)]
pub enum ContainerStep {
    /// `root[i]` is a `Div`, `BlockQuote`, or `Figure` — descend into `.content`.
    Blocks(usize),
    /// `root[i]` is a `BulletList` or `OrderedList` — descend into `.content[item]`.
    ListItem(usize, usize),
    /// `root[i]` is a `DefinitionList` — descend into `.content[def].1[body]`.
    DefBody(usize, usize, usize),
}

/// A path from the document root to a specific block.
///
/// `steps` is the sequence of container-descent steps; `leaf_idx` is the
/// index of the target block within the final container's `Blocks` slice.
/// For a top-level block, `steps` is empty and `leaf_idx` is the index in
/// `ast.blocks`.
#[derive(Debug, Clone, PartialEq)]
pub struct NodePath {
    pub steps: Vec<ContainerStep>,
    pub leaf_idx: usize,
}

impl NodePath {
    /// Construct a top-level (non-nested) path: `steps = []`, `leaf_idx = i`.
    pub fn top_level(i: usize) -> Self {
        NodePath {
            steps: vec![],
            leaf_idx: i,
        }
    }
}

/// Find the block in `ast` that corresponds to `target` and return its path.
///
/// Uses an **unconditional pre-order DFS**: visit each block, check for an
/// exact `source_info` match, and recurse into every descendable container
/// regardless of ranges — no containment check reintroduced (Plan 2b rule).
///
/// **Exact value match only** — `block.source_info() == target`.  `Generated`
/// nodes are rejected upstream; they never reach here.
///
/// The match is unique because distinct byte ranges guarantee at most one
/// exact hit in a well-formed source map (`duplicate_blocks_have_distinct_source_info`
/// invariant).  Returns `None` for a stale-AST miss.
///
/// # Descendable container types (Plan 3)
/// - `Div`, `BlockQuote`, `Figure` → via `ContainerStep::Blocks`
/// - `BulletList`, `OrderedList` → via `ContainerStep::ListItem`
/// - `DefinitionList` → via `ContainerStep::DefBody`
///
/// `NoteDefinitionFencedBlock`, `Table` (cells/caption), and all leaf block
/// types are not descended.
pub fn lookup_block(ast: &Pandoc, target: &SourceInfo, _file_id: FileId) -> Option<NodePath> {
    lookup_in_blocks(&ast.blocks, target)
}

fn lookup_in_blocks(blocks: &Blocks, target: &SourceInfo) -> Option<NodePath> {
    for (i, block) in blocks.iter().enumerate() {
        // Pre-order: check exact match at this node first.
        if block.source_info() == target {
            return Some(NodePath::top_level(i));
        }
        // Then try descending into descendable containers.
        if let Some(path) = descend_into(block, target, i) {
            return Some(path);
        }
    }
    None
}

/// Try to find `target` inside the children of a descendable container block.
/// Returns `None` if `block` is not a descendable container, or if the target
/// is not found anywhere in its subtree.
fn descend_into(block: &Block, target: &SourceInfo, container_idx: usize) -> Option<NodePath> {
    match block {
        Block::Div(d) => lookup_in_blocks(&d.content, target)
            .map(|mut p| prepend_blocks_step(&mut p, container_idx)),
        Block::BlockQuote(bq) => lookup_in_blocks(&bq.content, target)
            .map(|mut p| prepend_blocks_step(&mut p, container_idx)),
        Block::Figure(f) => lookup_in_blocks(&f.content, target)
            .map(|mut p| prepend_blocks_step(&mut p, container_idx)),
        Block::BulletList(bl) => {
            for (item_idx, item) in bl.content.iter().enumerate() {
                if let Some(mut p) = lookup_in_blocks(item, target) {
                    p.steps
                        .insert(0, ContainerStep::ListItem(container_idx, item_idx));
                    return Some(p);
                }
            }
            None
        }
        Block::OrderedList(ol) => {
            for (item_idx, item) in ol.content.iter().enumerate() {
                if let Some(mut p) = lookup_in_blocks(item, target) {
                    p.steps
                        .insert(0, ContainerStep::ListItem(container_idx, item_idx));
                    return Some(p);
                }
            }
            None
        }
        Block::DefinitionList(dl) => {
            for (def_idx, (_, bodies)) in dl.content.iter().enumerate() {
                for (body_idx, body) in bodies.iter().enumerate() {
                    if let Some(mut p) = lookup_in_blocks(body, target) {
                        p.steps
                            .insert(0, ContainerStep::DefBody(container_idx, def_idx, body_idx));
                        return Some(p);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn prepend_blocks_step(path: &mut NodePath, container_idx: usize) -> NodePath {
    path.steps.insert(0, ContainerStep::Blocks(container_idx));
    path.clone()
}
