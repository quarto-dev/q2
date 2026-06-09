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

use crate::node_lookup::lookup_block;
use crate::readers::json::read as json_read;
use crate::writers::incremental::incremental_write;
use quarto_ast_reconcile::compute_reconciliation;
use quarto_source_map::{FileId, SourceInfo};
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
    let Some(idx) = lookup_block(&a_u, &target_si, FileId(0)) else {
        eprintln!(
            "[apply_node_edit] destination block not found in untransformed AST; \
             returning original content unchanged (stale-AST race)"
        );
        return Ok(content.to_string());
    };

    // Step 4: Deserialize the modified subtree (metadata is ignored; only
    //         blocks are used as the replacement).
    let mut cursor = Cursor::new(modified_subtree_json.as_bytes());
    let (subtree, _) = json_read(&mut cursor)
        .map_err(|e| ApplyNodeEditError::DeserializeModifiedSubtree(format!("{e:?}")))?;

    // Step 5: Splice → A_u'.  Replaces the single block at `idx` with the
    //         (potentially multi-block) replacement.
    let mut a_u_prime = a_u.clone();
    a_u_prime.blocks.splice(idx..=idx, subtree.blocks);

    // Step 6: Reconcile and write.
    let plan = compute_reconciliation(&a_u, &a_u_prime);
    let new_qmd = incremental_write(content, &a_u, &a_u_prime, &plan)
        .map_err(|e| ApplyNodeEditError::IncrementalWrite(format!("{e:?}")))?;

    Ok(new_qmd)
}
