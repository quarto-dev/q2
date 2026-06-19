/*
 * apply_node_edit.rs
 *
 * Core backend for apply_node_edit (Phase 3 of the
 * target-incremental-writes plan, 2026-06-04).
 *
 * Takes the render's own untransformed AST (round-tripped from the frontend),
 * splices a pure replacement subtree in at the destination block, and runs
 * compute_reconciliation + incremental_write to produce new QMD.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use crate::node_lookup::{ContainerStep, NodePath, lookup_block};
use crate::pandoc::block::{Block, Blocks, Plain};
use crate::readers::json::{read as json_read, read_completing_source_info};
use crate::writers::incremental::incremental_write;
use quarto_ast_reconcile::compute_reconciliation;
use quarto_source_map::{By, FileId, SourceInfo};
use std::io::Cursor;

/// Convert a compact pool-entry value `{"t":N,"r":[s,e],"d":...}` to a `SourceInfo`.
///
/// This is the inverse of the JSON writer's `SourceInfoJson` serialization.
/// v1 handles `Original` (t=0) only — the only type emitted for plain
/// paragraphs and headings that the v1 edit surface supports.
fn decode_compact_source_info(v: serde_json::Value) -> Result<SourceInfo, ApplyNodeEditError> {
    let err = |msg: &str| {
        ApplyNodeEditError::DeserializeSourceInfo(format!("compact source_info: {msg}"))
    };

    let type_code = v
        .get("t")
        .and_then(|t| t.as_u64())
        .ok_or_else(|| err("missing t"))? as usize;

    let range = v
        .get("r")
        .and_then(|r| r.as_array())
        .ok_or_else(|| err("missing r"))?;
    let start_offset = range
        .first()
        .and_then(|x| x.as_u64())
        .ok_or_else(|| err("invalid r[0]"))? as usize;
    let end_offset = range
        .get(1)
        .and_then(|x| x.as_u64())
        .ok_or_else(|| err("invalid r[1]"))? as usize;

    let data = v.get("d").ok_or_else(|| err("missing d"))?;

    match type_code {
        0 => {
            // Original: d is file_id (number)
            let file_id = data.as_u64().unwrap_or(0) as usize;
            Ok(SourceInfo::Original {
                file_id: FileId(file_id),
                start_offset,
                end_offset,
            })
        }
        _ => Err(ApplyNodeEditError::DeserializeSourceInfo(format!(
            "compact source_info type {type_code} not supported for v1 editing; \
             only Original (t=0) blocks can be targeted in this version"
        ))),
    }
}

/// Error variants for [`apply_node_edit`].
#[derive(Debug)]
pub enum ApplyNodeEditError {
    DeserializeUntransformedAst(String),
    DeserializeSourceInfo(String),
    DeserializeModifiedSubtree(String),
    IncrementalWrite(String),
}

impl std::fmt::Display for ApplyNodeEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeserializeUntransformedAst(e) => {
                write!(f, "failed to deserialize untransformed AST: {e}")
            }
            Self::DeserializeSourceInfo(e) => {
                write!(f, "failed to deserialize destination source_info: {e}")
            }
            Self::DeserializeModifiedSubtree(e) => {
                write!(f, "failed to deserialize modified subtree: {e}")
            }
            Self::IncrementalWrite(e) => write!(f, "incremental_write failed: {e}"),
        }
    }
}

/// Splice a pure replacement subtree into the untransformed AST at the
/// destination block and produce new QMD via `compute_reconciliation` +
/// `incremental_write`.
///
/// # Arguments
/// * `content`                     — original QMD source text (verbatim bytes)
/// * `untransformed_ast_json`      — the render's own pre-pipeline AST,
///                                   serialized as Pandoc JSON (round-tripped
///                                   from the frontend)
/// * `destination_source_info_json` — the resolved `SourceInfo` VALUE of the
///                                    edited node (not a bare pool id); JSON-
///                                    serialized `SourceInfo` struct
/// * `modified_subtree_json`       — pure replacement block(s) as a full
///                                   Pandoc JSON document (e.g. the output of
///                                   `parse_qmd_content`); metadata is ignored
///
/// # Returns
/// The new QMD string on success, or an [`ApplyNodeEditError`] on failure.
pub fn apply_node_edit(
    content: &str,
    untransformed_ast_json: &str,
    destination_source_info_json: &str,
    modified_subtree_json: &str,
) -> Result<String, ApplyNodeEditError> {
    // Step 1: Deserialize the untransformed AST (A_u).
    let mut cursor = Cursor::new(untransformed_ast_json.as_bytes());
    let (a_u, _ctx) = json_read(&mut cursor)
        .map_err(|e| ApplyNodeEditError::DeserializeUntransformedAst(format!("{e:?}")))?;

    // Step 2: Deserialize the destination SourceInfo value.
    // Accepts two formats:
    //  - Compact wire format from the AST pool: {"t":0,"r":[start,end],"d":file_id}
    //  - Serde enum format (used by tests): {"Original":{"file_id":0,...}}
    let si_json: serde_json::Value = serde_json::from_str(destination_source_info_json)
        .map_err(|e| ApplyNodeEditError::DeserializeSourceInfo(format!("{e}")))?;
    let target_si: SourceInfo = if si_json.get("t").and_then(|v| v.as_u64()).is_some() {
        decode_compact_source_info(si_json)?
    } else {
        serde_json::from_value(si_json)
            .map_err(|e| ApplyNodeEditError::DeserializeSourceInfo(format!("{e}")))?
    };

    // Step 3: Locate the destination block in A_u.
    // v1 assumption: single-file document → FileId(0).
    // A None result means a stale-AST race (the block was removed between
    // the last render and this edit); degrade gracefully by returning the
    // original content unchanged rather than surfacing an error.
    let Some(path) = lookup_block(&a_u, &target_si, FileId(0)) else {
        eprintln!(
            "[apply_node_edit] destination block not found in untransformed AST; \
             returning original content unchanged (stale-AST race)"
        );
        return Ok(content.to_string());
    };

    // Step 4: Deserialize the modified subtree (metadata is ignored; only
    //         blocks are used as the replacement).
    // Use read_completing_source_info (lenient) instead of strict json_read so
    // render-component edits can pass blocks cloned from the untransformed AST
    // without having to carry a valid pool.  Any missing or stale `s:` field is
    // filled with Generated(by=DirectWrite) — the incremental writer then
    // assigns fresh SourceInfo on the next render.
    let mut cursor = Cursor::new(modified_subtree_json.as_bytes());
    let completing_by = By {
        kind: "direct-write".to_string(),
        data: serde_json::Value::Null,
    };
    let (subtree, _) = read_completing_source_info(&mut cursor, completing_by)
        .map_err(|e| ApplyNodeEditError::DeserializeModifiedSubtree(format!("{e:?}")))?;

    // Step 5: Splice → A_u'.  Navigate via the path and replace the single
    //         block at leaf_idx with the (potentially multi-block) replacement.
    //         For top-level blocks (path.steps = []) this is the same as the
    //         old direct splice; for nested blocks the path guides descent.
    let mut a_u_prime = a_u.clone();
    splice_at_path(&mut a_u_prime.blocks, &path, subtree.blocks);

    // Step 6: Reconcile and write.
    let plan = compute_reconciliation(&a_u, &a_u_prime);
    let new_qmd = incremental_write(content, &a_u, &a_u_prime, &plan)
        .map_err(|e| ApplyNodeEditError::IncrementalWrite(format!("{e:?}")))?;

    Ok(new_qmd)
}

/// Splice `replacement` into the block tree at the position described by
/// `path`.  Walks `steps` to reach the parent `Blocks` slice, then calls
/// `Vec::splice` at `leaf_idx`.
///
/// `unreachable!` on a step/variant mismatch — the invariant is guaranteed
/// by `lookup_block`, which only produces valid paths; a panic here indicates
/// a logic bug, not a runtime condition.
fn splice_at_path(root: &mut Blocks, path: &NodePath, replacement: Vec<Block>) {
    splice_in_blocks(root, &path.steps, path.leaf_idx, replacement);
}

fn splice_in_blocks(
    current: &mut Blocks,
    steps: &[ContainerStep],
    leaf_idx: usize,
    replacement: Vec<Block>,
) {
    let Some((head, tail)) = steps.split_first() else {
        let replacement = preserve_leaf_variant(current.get(leaf_idx), replacement);
        current.splice(leaf_idx..=leaf_idx, replacement);
        return;
    };
    match head {
        ContainerStep::Blocks(i) => {
            let child = match &mut current[*i] {
                Block::Div(d) => &mut d.content,
                Block::BlockQuote(bq) => &mut bq.content,
                Block::Figure(f) => &mut f.content,
                b => unreachable!(
                    "splice_in_blocks: Blocks step at index {i} but got {:?}",
                    std::mem::discriminant(b)
                ),
            };
            splice_in_blocks(child, tail, leaf_idx, replacement);
        }
        ContainerStep::ListItem(i, item_idx) => {
            let child = match &mut current[*i] {
                Block::BulletList(bl) => &mut bl.content[*item_idx],
                Block::OrderedList(ol) => &mut ol.content[*item_idx],
                b => unreachable!(
                    "splice_in_blocks: ListItem step at index {i} but got {:?}",
                    std::mem::discriminant(b)
                ),
            };
            splice_in_blocks(child, tail, leaf_idx, replacement);
        }
        ContainerStep::DefBody(i, def_idx, body_idx) => {
            let child = match &mut current[*i] {
                Block::DefinitionList(dl) => &mut dl.content[*def_idx].1[*body_idx],
                b => unreachable!(
                    "splice_in_blocks: DefBody step at index {i} but got {:?}",
                    std::mem::discriminant(b)
                ),
            };
            splice_in_blocks(child, tail, leaf_idx, replacement);
        }
    }
}

/// Coerce a single-`Para` replacement back to `Plain` when the original block
/// was `Plain`.
///
/// An inline-text edit (text channel) re-parses the bare text as a standalone
/// paragraph → `Para`. If the original block was `Plain` (a tight list item),
/// splicing in a `Para` turns the list loose. This function restores the
/// original variant when:
///   1. The original block was `Block::Plain`, AND
///   2. The replacement is exactly one block, AND
///   3. That block is `Block::Paragraph`.
///
/// The `len() == 1` guard is load-bearing: a multi-block replacement (e.g. two
/// paragraphs from a structural edit) must pass through unchanged so the list
/// legitimately loosens. See T10 in the test suite.
fn preserve_leaf_variant(original: Option<&Block>, mut replacement: Vec<Block>) -> Vec<Block> {
    if matches!(original, Some(Block::Plain(_)))
        && replacement.len() == 1
        && matches!(replacement[0], Block::Paragraph(_))
        && let Block::Paragraph(para) = replacement.remove(0)
    {
        return vec![Block::Plain(Plain {
            content: para.content,
            source_info: para.source_info,
        })];
    }
    replacement
}
