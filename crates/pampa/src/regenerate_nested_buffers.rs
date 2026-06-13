/*
 * regenerate_nested_buffers.rs
 *
 * Produces a table of clean QMD buffers for each block that:
 *   (1) has a prefixing ancestor (BlockQuote, BulletList, OrderedList, DefinitionList), AND
 *   (2) is multi-line in source (contains an internal newline — a '\n' other than the
 *       single trailing newline that every block's offset range includes).
 *
 * Used by the depth-cursor mode of the block editor so the frontend can
 * descend into a nested block and edit it without the container's `> ` or
 * list-indent prefix appearing in the editor buffer.
 *
 * Fenced/column-0 containers (Div, Figure, NoteDefinitionFencedBlock, Custom)
 * are NOT prefixing — we descend through them but entering one does not set
 * the prefixing flag.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use crate::pandoc::Pandoc;
use crate::pandoc::block::{Block, Blocks};
use crate::pandoc::custom::{CustomNode, Slot};
use crate::readers::json::read as json_read;
use crate::writers::qmd::write_single_block;
use quarto_source_map::SourceInfo;
use std::collections::BTreeMap;
use std::io::Cursor;

// =============================================================================
// Public API
// =============================================================================

/// Pure, native-testable core: walk `ast`, return siKey -> cleanQmd.
///
/// A block enters the table iff BOTH:
///   1. It has a prefixing ancestor (BlockQuote, BulletList, OrderedList, DefinitionList).
///   2. It is multi-line: content.as_bytes()[start..end] contains an internal newline
///      (a b'\n' other than the single trailing newline that every block range includes).
///
/// Only blocks with SourceInfo::Original source info are included (skip Substring/Generated).
/// The outermost prefixing container itself has no prefixing ancestor, so it is excluded.
pub fn regenerate_nested_buffers_ast(content: &str, ast: &Pandoc) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    walk_blocks(&ast.blocks, false, content, &mut out);
    out
}

/// JSON boundary: parse the untransformed AST JSON, run the walk, serialize the map.
pub fn regenerate_nested_buffers(
    content: &str,
    untransformed_ast_json: &str,
) -> Result<String, String> {
    let mut cursor = Cursor::new(untransformed_ast_json.as_bytes());
    let (ast, _ctx) =
        json_read(&mut cursor).map_err(|e| format!("Failed to parse AST JSON: {e:?}"))?;
    let map = regenerate_nested_buffers_ast(content, &ast);
    serde_json::to_string(&map).map_err(|e| format!("Failed to serialize map: {e}"))
}

// =============================================================================
// Internal walk
// =============================================================================

/// Walk a block list. `has_prefixing_ancestor` is true if any enclosing
/// container is a prefixing type (BlockQuote/BulletList/OrderedList/DefinitionList).
fn walk_blocks(
    blocks: &Blocks,
    has_prefixing_ancestor: bool,
    content: &str,
    out: &mut BTreeMap<String, String>,
) {
    for block in blocks {
        process_block(block, has_prefixing_ancestor, content, out);
    }
}

fn process_block(
    block: &Block,
    has_prefixing_ancestor: bool,
    content: &str,
    out: &mut BTreeMap<String, String>,
) {
    // If this block qualifies (Original, has prefixing ancestor, multi-line),
    // emit it into the map.
    if has_prefixing_ancestor {
        maybe_emit(block, content, out);
    }

    // Recurse into children, updating the prefixing flag appropriately.
    match block {
        // Prefixing containers: set flag to true for their children.
        Block::BlockQuote(bq) => {
            walk_blocks(&bq.content, true, content, out);
        }
        Block::BulletList(bl) => {
            for item in &bl.content {
                walk_blocks(item, true, content, out);
            }
        }
        Block::OrderedList(ol) => {
            for item in &ol.content {
                walk_blocks(item, true, content, out);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &dl.content {
                for def in defs {
                    walk_blocks(def, true, content, out);
                }
            }
        }

        // Non-prefixing containers: propagate the existing flag.
        Block::Div(d) => {
            walk_blocks(&d.content, has_prefixing_ancestor, content, out);
        }
        Block::Figure(f) => {
            walk_blocks(&f.content, has_prefixing_ancestor, content, out);
        }
        Block::NoteDefinitionFencedBlock(n) => {
            walk_blocks(&n.content, has_prefixing_ancestor, content, out);
        }
        Block::Custom(cn) => {
            walk_custom(cn, has_prefixing_ancestor, content, out);
        }

        // Leaf blocks with no block children: nothing to recurse into.
        Block::Plain(_)
        | Block::Paragraph(_)
        | Block::LineBlock(_)
        | Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::Header(_)
        | Block::HorizontalRule(_)
        | Block::Table(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::CaptionBlock(_) => {}
    }
}

fn walk_custom(
    cn: &CustomNode,
    has_prefixing_ancestor: bool,
    content: &str,
    out: &mut BTreeMap<String, String>,
) {
    // Custom serializes as a fenced div — treat as non-prefixing.
    for slot in cn.slots.values() {
        match slot {
            Slot::Block(b) => process_block(b, has_prefixing_ancestor, content, out),
            Slot::Blocks(bs) => walk_blocks(bs, has_prefixing_ancestor, content, out),
            // Inline slots have no block descendants.
            Slot::Inline(_) | Slot::Inlines(_) => {}
        }
    }
}

/// If `block` is Original and multi-line, regenerate it and insert into `out`.
fn maybe_emit(block: &Block, content: &str, out: &mut BTreeMap<String, String>) {
    let si = block.source_info();
    let (start, end, file_id) = match si {
        SourceInfo::Original {
            start_offset,
            end_offset,
            file_id,
        } => (*start_offset, *end_offset, file_id.0),
        // Skip Substring and Generated.
        _ => return,
    };

    // Multi-line check: the byte slice contains a '\n' that is NOT the
    // final character. Every block offset range includes the block's trailing
    // newline, so `slice.contains('\n')` would match all single-line blocks.
    // We skip the last byte to detect only genuine continuation lines.
    let is_multiline = content.as_bytes().get(start..end).map_or(false, |b| {
        let body = if b.last() == Some(&b'\n') {
            &b[..b.len() - 1]
        } else {
            b
        };
        body.contains(&b'\n')
    });

    if !is_multiline {
        return;
    }

    // Regenerate clean QMD (no `> ` or list-indent prefix) via write_single_block.
    let mut buf = Vec::new();
    if write_single_block(block, &mut buf).is_err() {
        return;
    }
    let clean_qmd = match String::from_utf8(buf) {
        Ok(s) => s.trim_end().to_string(),
        Err(_) => return,
    };

    // siKey mirrors the frontend's serializeSourceEntry: "t:r[0]-r[1]:d"
    // For Original, t=0, d=file_id (always 0 for single-file docs).
    let key = format!("0:{start}-{end}:{file_id}");
    out.insert(key, clean_qmd);
}
