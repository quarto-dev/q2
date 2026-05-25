/*
 * incremental.rs
 *
 * Incremental QMD writer: converts localized AST changes into localized string edits.
 * When a change occurs in the AST, only the affected portion of the QMD string is
 * rewritten, preserving the rest of the original source text verbatim.
 *
 * See: claude-notes/plans/2026-02-07-incremental-writer.md
 *
 * Copyright (c) 2026 Posit, PBC
 */

use crate::pandoc::{Block, Inline, Pandoc};
use quarto_ast_reconcile::types::{
    BlockAlignment, InlineAlignment, InlineReconciliationPlan, ReconciliationPlan,
};
use quarto_ast_reconcile::{structural_eq_blocks, structural_eq_inlines};
use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue, ConfigValueKind};
use quarto_pandoc_types::is_atomic_custom_node;
use quarto_source_map::{FileId, SourceInfo};
use std::ops::Range;

use super::qmd;

// =============================================================================
// Types
// =============================================================================

/// A text edit: replace a byte range in the original string with new text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// Byte range in the original string to replace.
    pub range: Range<usize>,
    /// Replacement text.
    pub replacement: String,
}

/// An entry in the coarsened plan: either copy verbatim, rewrite, or inline-splice.
#[derive(Debug)]
enum CoarsenedEntry {
    /// Copy this byte range verbatim from original_qmd.
    /// The text includes the block content + trailing \n.
    Verbatim {
        byte_range: Range<usize>,
        /// Index of this block in original_ast.blocks (for gap computation)
        orig_idx: usize,
    },
    /// Rewrite this block using the standard writer.
    Rewrite {
        /// Index into new_ast.blocks
        new_idx: usize,
    },
    /// Splice inlines within a block without rewriting the entire block.
    /// The block structure (prefix, suffix) is preserved from the original;
    /// only the inline content region is replaced with assembled new content.
    InlineSplice {
        /// Pre-computed block text: original block with inline content replaced.
        block_text: String,
        /// Index of this block in original_ast.blocks (for gap computation)
        orig_idx: usize,
    },
}

// =============================================================================
// Editability gate (Plan 7)
// =============================================================================

/// Decide whether the *interior* of `block` is editable, with respect to the
/// active document `target_file_id`.
///
/// "Editable inside" means: the user can type into this node's content and
/// have their edit round-trip back to source bytes. Three reasons content is
/// **not** editable inside:
///
/// 1. The block is an atomic `CustomNode` (per
///    [`quarto_pandoc_types::is_atomic_custom_node`]). Atomic nodes are
///    replaceable wholesale via a React-side component menu but have no
///    editable text region. Today: `"CrossrefResolvedRef"`.
/// 2. The block carries `SourceInfo::Generated` with an atomic-kind `by`
///    (shortcode / filter / title-block / tree-sitter-postprocess).
///    Content is the resolved value of an invocation token; the user's
///    source-side knob is the token, not the resolved bytes.
/// 3. The block's source_info has no preimage in `target_file_id`
///    (synthesized-from-metadata containers, cross-file Original chains).
///    There are no bytes in the target file to map an inner edit back to.
///
/// **Returns `true` for everything else.** Used by `coarsen`'s soft-drop
/// logic; the React-side hand-mirror lives at
/// `ts-packages/preview-renderer/src/utils/atomicCustomNodes.ts` plus a
/// parallel `is_editable_inside` predicate to be added in a follow-up.
///
/// See Plan 7 §"Unified editability predicate".
pub fn is_editable_inside_block(block: &Block, target_file_id: FileId) -> bool {
    if let Block::Custom(cn) = block
        && is_atomic_custom_node(&cn.type_name)
    {
        return false;
    }
    is_editable_inside_source_info(block.source_info(), target_file_id)
}

/// Inline-side counterpart of [`is_editable_inside_block`].
///
/// Same three reasons content is not editable inside; for `Inline::Custom`
/// the atomic-CustomNode check applies (some atomic types live in the
/// inline arm — `CrossrefResolvedRef` is one).
pub fn is_editable_inside_inline(inline: &Inline, target_file_id: FileId) -> bool {
    if let Inline::Custom(cn) = inline
        && is_atomic_custom_node(&cn.type_name)
    {
        return false;
    }
    is_editable_inside_source_info(inline.source_info(), target_file_id)
}

/// Shared editability rules driven by `SourceInfo` alone (the
/// atomic-CustomNode gate is applied by the block / inline callers above).
fn is_editable_inside_source_info(si: &SourceInfo, target_file_id: FileId) -> bool {
    // Atomic-kind Generated (shortcode, filter, title-block,
    // tree-sitter-postprocess): the content is pipeline-resolved; the
    // user's source-side knob is the invocation token, not the bytes
    // inside.
    if let SourceInfo::Generated { by, .. } = si
        && by.is_atomic_kind()
    {
        return false;
    }
    // Catch-all: editable iff the region has byte-traceable preimage in
    // the target file. Covers:
    //   - Original in target → editable. ✓
    //   - Substring chain resolving in target → editable. ✓
    //   - Original/Substring rooted outside target → not editable.
    //   - Generated with empty anchors (sectionize, footnotes,
    //     appendix containers) → preimage_in returns None → not editable.
    //   - Generated with only ValueSource/Dispatch/Other anchors → not
    //     editable (preimage_in walks Invocation only).
    //   - Non-atomic Generated with Invocation anchor in target →
    //     editable.
    si.preimage_in(target_file_id).is_some()
}

// =============================================================================
// Public API
// =============================================================================

/// Incrementally write an AST, producing a new QMD string that preserves
/// unchanged portions of the original text.
///
/// # Arguments
/// * `original_qmd` - The original QMD source text
/// * `original_ast` - The AST produced by reading `original_qmd` (has accurate source spans)
/// * `new_ast` - The modified AST (what the user wants written)
/// * `plan` - A reconciliation plan describing alignment between original_ast and new_ast
///
/// # Returns
/// A new QMD string where:
/// - Unchanged blocks are preserved verbatim from `original_qmd`
/// - Changed blocks are rewritten using the standard writer
/// - The result round-trips: `read(result) ≡ new_ast` (structural equality)
pub fn incremental_write(
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    plan: &ReconciliationPlan,
) -> Result<String, Vec<quarto_error_reporting::DiagnosticMessage>> {
    // The QMD reader internally pads input with '\n' when it doesn't end with
    // one, producing source spans relative to the padded input. We must use the
    // same padded string so that block source spans are valid byte indices.
    let mut padded_storage = None;
    let (qmd, did_pad) = ensure_trailing_newline(original_qmd, &mut padded_storage);

    // Step 1: Coarsen the reconciliation plan
    let coarsened = coarsen(qmd, original_ast, new_ast, plan)?;

    // Step 2: Assemble the result string
    let mut result = assemble(qmd, original_ast, new_ast, &coarsened)?;

    // If we padded the input, strip the trailing '\n' from the result so that
    // the output preserves the original document's trailing-newline convention.
    if did_pad && result.ends_with('\n') {
        result.pop();
    }

    Ok(result)
}

/// Compute minimal text edits to transform `original_qmd` into the incremental write result.
///
/// Each TextEdit describes a byte range in `original_qmd` to replace and the replacement text.
/// Edits are sorted by range.start and non-overlapping.
pub fn compute_incremental_edits(
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    plan: &ReconciliationPlan,
) -> Result<Vec<TextEdit>, Vec<quarto_error_reporting::DiagnosticMessage>> {
    // Same trailing-newline normalization as incremental_write (see comment there).
    let mut padded_storage = None;
    let (qmd, did_pad) = ensure_trailing_newline(original_qmd, &mut padded_storage);

    let coarsened = coarsen(qmd, original_ast, new_ast, plan)?;
    let mut edits = compute_edits_from_coarsened(qmd, original_ast, new_ast, &coarsened)?;

    if did_pad {
        // Edits reference the padded string. Adjust ranges and replacement text
        // so they apply to the original (unpadded) string.
        for edit in &mut edits {
            if edit.range.end > original_qmd.len() {
                edit.range.end = original_qmd.len();
            }
            if edit.replacement.ends_with('\n') {
                edit.replacement.pop();
            }
        }
    }

    Ok(edits)
}

// =============================================================================
// Step 1: Coarsen the Reconciliation Plan
// =============================================================================

/// Convert a hierarchical ReconciliationPlan into a flat Vec<CoarsenedEntry>.
///
/// Phase 5 strategy: for RecurseIntoContainer blocks that are inline-content blocks
/// (Paragraph, Plain, Header) with inline plans that pass the safety check,
/// produce InlineSplice entries. All other RecurseIntoContainer become Rewrite.
fn coarsen(
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    plan: &ReconciliationPlan,
) -> Result<Vec<CoarsenedEntry>, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut entries = Vec::with_capacity(plan.block_alignments.len());

    for (result_idx, alignment) in plan.block_alignments.iter().enumerate() {
        let entry = match alignment {
            BlockAlignment::KeepBefore(orig_idx) => {
                let span = block_source_span(&original_ast.blocks[*orig_idx]);
                CoarsenedEntry::Verbatim {
                    byte_range: span,
                    orig_idx: *orig_idx,
                }
            }
            BlockAlignment::UseAfter(_after_idx) => CoarsenedEntry::Rewrite {
                new_idx: result_idx,
            },
            BlockAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => {
                // Check if this block has an inline plan and is safe to splice
                if let Some(inline_plan) = plan.inline_plans.get(&result_idx) {
                    let orig_block = &original_ast.blocks[*before_idx];
                    let new_block = &new_ast.blocks[*after_idx];

                    if let (Some(orig_inlines), Some(new_inlines)) =
                        (block_inlines(orig_block), block_inlines(new_block))
                    {
                        if !orig_inlines.is_empty()
                            && is_inline_splice_safe(new_inlines, inline_plan)
                            && block_attrs_eq(orig_block, new_block)
                        {
                            // Safe to splice — assemble the patched block text
                            let block_text = assemble_inline_splice(
                                original_qmd,
                                orig_block,
                                orig_inlines,
                                new_inlines,
                                inline_plan,
                            )?;
                            CoarsenedEntry::InlineSplice {
                                block_text,
                                orig_idx: *before_idx,
                            }
                        } else {
                            CoarsenedEntry::Rewrite {
                                new_idx: result_idx,
                            }
                        }
                    } else {
                        // Not an inline-content block — fall back to Rewrite
                        CoarsenedEntry::Rewrite {
                            new_idx: result_idx,
                        }
                    }
                } else {
                    // No inline plan — this is a block container (Div, BlockQuote, etc.)
                    // Fall back to Rewrite
                    CoarsenedEntry::Rewrite {
                        new_idx: result_idx,
                    }
                }
            }
        };
        entries.push(entry);
    }

    Ok(entries)
}

// =============================================================================
// Step 2: Assemble the Result String
// =============================================================================

/// Assemble the output string from the coarsened plan.
fn assemble(
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    coarsened: &[CoarsenedEntry],
) -> Result<String, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut result = String::new();

    // 2a. Metadata prefix
    let _has_meta_prefix =
        emit_metadata_prefix(&mut result, original_qmd, original_ast, new_ast, coarsened)?;

    // 2b. Walk coarsened entries and assemble blocks with separators
    let mut prev_entry: Option<&CoarsenedEntry> = None;
    let mut prev_block_text: Option<String> = None;

    for entry in coarsened {
        // 2c. Separator between blocks
        // Note: we only add a separator when there's a previous block.
        // The metadata prefix already includes the gap to the first block,
        // so we must NOT add an extra separator after it.
        if prev_entry.is_some() {
            let separator = compute_separator(
                original_qmd,
                original_ast,
                prev_entry,
                entry,
                prev_block_text.as_deref(),
            );
            result.push_str(separator);
        }

        // Emit block text
        let block_text = match entry {
            CoarsenedEntry::Verbatim { byte_range, .. } => {
                original_qmd[byte_range.clone()].to_string()
            }
            CoarsenedEntry::Rewrite { new_idx } => {
                write_block_to_string(&new_ast.blocks[*new_idx])?
            }
            CoarsenedEntry::InlineSplice { block_text, .. } => block_text.clone(),
        };

        result.push_str(&block_text);
        prev_block_text = Some(block_text);
        prev_entry = Some(entry);
    }

    Ok(result)
}

/// Emit the metadata prefix (YAML front matter region).
///
/// Returns true if a metadata prefix was emitted.
fn emit_metadata_prefix(
    result: &mut String,
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    _coarsened: &[CoarsenedEntry],
) -> Result<bool, Vec<quarto_error_reporting::DiagnosticMessage>> {
    // Determine where the metadata region ends by looking at the first
    // ORIGINAL block's start offset. We must NOT use the first coarsened
    // entry's offset — when blocks are removed from the beginning, the
    // first coarsened block may reference a later original block whose
    // start > 0, falsely triggering the metadata prefix logic.
    let first_block_start = if !original_ast.blocks.is_empty() {
        Some(block_source_span(&original_ast.blocks[0]).start)
    } else {
        None
    };

    // Check if there's a metadata region before the first block
    if let Some(start) = first_block_start {
        if start > 0 {
            // There is a metadata prefix region
            if metadata_content_eq(&original_ast.meta, &new_ast.meta) {
                // Metadata unchanged — copy verbatim
                result.push_str(&original_qmd[..start]);
            } else {
                // Metadata changed — rewrite the front matter, but preserve
                // the original gap (blank lines) between the closing --- and
                // the first block.
                let meta_str = write_metadata_to_string(&new_ast.meta)?;
                result.push_str(&meta_str);

                // Find where the original front matter content ends (the closing ---)
                // and preserve the gap between it and the first block.
                let gap = find_metadata_trailing_gap(original_qmd, start);
                result.push_str(gap);
            }
            return Ok(true);
        }
    }

    // No metadata prefix
    Ok(false)
}

/// Find the gap (whitespace) between the end of the YAML front matter and the
/// first block. The `first_block_start` is the byte offset where the first
/// block begins. We look backwards from that offset to find where the
/// closing `---\n` ends, and return the gap between them.
fn find_metadata_trailing_gap(original_qmd: &str, first_block_start: usize) -> &str {
    // The metadata region is original_qmd[..first_block_start].
    // The closing `---` is followed by `\n`, and then there may be blank lines
    // before the first block. The write_metadata_to_string function already
    // emits `---\n` at the end, so we need to find just the extra whitespace.
    //
    // Look for the last occurrence of "---\n" in the metadata region.
    let meta_region = &original_qmd[..first_block_start];
    if let Some(closing_pos) = meta_region.rfind("---\n") {
        let after_closing = closing_pos + 4; // skip past "---\n"
        &original_qmd[after_closing..first_block_start]
    } else {
        // No closing --- found (shouldn't happen for valid front matter).
        // Fall back to a single newline separator.
        "\n"
    }
}

/// Compute the separator between two adjacent blocks in the result.
///
/// For consecutive Verbatim blocks from consecutive original positions, use the
/// original gap verbatim (preserves exact whitespace for idempotence).
/// Otherwise, use "\n" unless the previous block already ends with "\n\n".
fn compute_separator<'a>(
    original_qmd: &'a str,
    original_ast: &Pandoc,
    prev_entry: Option<&CoarsenedEntry>,
    curr_entry: &CoarsenedEntry,
    prev_block_text: Option<&str>,
) -> &'a str {
    // Try to use original gap for consecutive blocks that preserve original positions
    let prev_orig_idx = match prev_entry {
        Some(CoarsenedEntry::Verbatim { orig_idx, .. }) => Some(*orig_idx),
        Some(CoarsenedEntry::InlineSplice { orig_idx, .. }) => Some(*orig_idx),
        _ => None,
    };
    let curr_orig_idx = match curr_entry {
        CoarsenedEntry::Verbatim { orig_idx, .. } => Some(*orig_idx),
        CoarsenedEntry::InlineSplice { orig_idx, .. } => Some(*orig_idx),
        _ => None,
    };
    if let (Some(prev_idx), Some(curr_idx)) = (prev_orig_idx, curr_orig_idx) {
        if curr_idx == prev_idx + 1 {
            // Consecutive in original — use original gap
            let prev_span = block_source_span(&original_ast.blocks[prev_idx]);
            let curr_span = block_source_span(&original_ast.blocks[curr_idx]);
            return &original_qmd[prev_span.end..curr_span.start];
        }
    }

    // Standard separator — but check if previous block already ends with \n\n
    if let Some(text) = prev_block_text {
        if text.ends_with("\n\n") {
            return "";
        }
    }

    "\n"
}

// =============================================================================
// Step 3: Compute Edits (derived from coarsened plan)
// =============================================================================

/// Compute TextEdit operations from the coarsened plan.
///
/// Identifies unchanged regions (runs of consecutive Verbatim blocks from
/// consecutive original positions) and produces edits for everything else.
fn compute_edits_from_coarsened(
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    coarsened: &[CoarsenedEntry],
) -> Result<Vec<TextEdit>, Vec<quarto_error_reporting::DiagnosticMessage>> {
    // For Phase 2, use the simple approach: build the result string
    // and produce a single edit if it differs from the original.
    let result = assemble(original_qmd, original_ast, new_ast, coarsened)?;

    if result == original_qmd {
        return Ok(vec![]);
    }

    // For now, a single edit replacing the entire document.
    // Future: compute minimal edits by analyzing coarsened runs.
    Ok(vec![TextEdit {
        range: 0..original_qmd.len(),
        replacement: result,
    }])
}

// =============================================================================
// Helpers
// =============================================================================

/// Ensure `original_qmd` ends with `'\n'`, returning either the original
/// string or a padded copy stored in `storage`.
///
/// The QMD reader internally pads input with `'\n'` if missing, so source
/// spans in the resulting AST reference the padded byte length. This helper
/// lets callers work with the same padded string without allocating when the
/// input already ends with `'\n'` (the common case).
///
/// Returns `(normalized_str, did_pad)`.
fn ensure_trailing_newline<'a>(
    original_qmd: &'a str,
    storage: &'a mut Option<String>,
) -> (&'a str, bool) {
    if original_qmd.ends_with('\n') {
        (original_qmd, false)
    } else {
        let padded = format!("{}\n", original_qmd);
        *storage = Some(padded);
        (storage.as_ref().unwrap().as_str(), true)
    }
}

/// Extract the byte range (start..end) from a Block's source_info.
fn block_source_span(block: &Block) -> Range<usize> {
    let si = block.source_info();
    si.start_offset()..si.end_offset()
}

/// Write a single block to a string using the standard QMD writer.
fn write_block_to_string(
    block: &Block,
) -> Result<String, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut buf = Vec::new();
    qmd::write_single_block(block, &mut buf)?;
    String::from_utf8(buf).map_err(|e| {
        vec![
            quarto_error_reporting::DiagnosticMessageBuilder::error("UTF-8 error during write")
                .with_code("Q-3-2")
                .problem(format!("Block writer produced invalid UTF-8: {}", e))
                .build(),
        ]
    })
}

/// Write metadata (front matter) to a string.
fn write_metadata_to_string(
    meta: &quarto_pandoc_types::ConfigValue,
) -> Result<String, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut buf = Vec::new();
    qmd::write_metadata(meta, &mut buf)?;
    // Add trailing newline after the closing ---
    // The separator to the first block will be handled by the assembly step
    String::from_utf8(buf).map_err(|e| {
        vec![
            quarto_error_reporting::DiagnosticMessageBuilder::error("UTF-8 error during write")
                .with_code("Q-3-2")
                .problem(format!("Metadata writer produced invalid UTF-8: {}", e))
                .build(),
        ]
    })
}

/// Compare two ConfigValue metadata structures for content equality,
/// ignoring source_info and merge_op at all levels.
///
/// This is needed because the incremental writer may compare an AST parsed
/// from QMD (with real source positions) against one deserialized from JSON
/// (with default source positions). The derived PartialEq on ConfigValue
/// includes source_info, which would incorrectly report them as different.
fn metadata_content_eq(a: &ConfigValue, b: &ConfigValue) -> bool {
    config_value_content_eq(a, b)
}

/// Recursively compare two ConfigValues, ignoring source_info and merge_op.
fn config_value_content_eq(a: &ConfigValue, b: &ConfigValue) -> bool {
    config_value_kind_content_eq(&a.value, &b.value)
}

/// Compare two ConfigValueKind values for content equality.
fn config_value_kind_content_eq(a: &ConfigValueKind, b: &ConfigValueKind) -> bool {
    match (a, b) {
        (ConfigValueKind::Scalar(a), ConfigValueKind::Scalar(b)) => a == b,
        (ConfigValueKind::PandocInlines(a), ConfigValueKind::PandocInlines(b)) => {
            structural_eq_inlines(a, b)
        }
        (ConfigValueKind::PandocBlocks(a), ConfigValueKind::PandocBlocks(b)) => {
            structural_eq_blocks(a, b)
        }
        (ConfigValueKind::Path(a), ConfigValueKind::Path(b)) => a == b,
        (ConfigValueKind::Glob(a), ConfigValueKind::Glob(b)) => a == b,
        (ConfigValueKind::Expr(a), ConfigValueKind::Expr(b)) => a == b,
        (ConfigValueKind::Array(a), ConfigValueKind::Array(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|(a, b)| config_value_content_eq(a, b))
        }
        (ConfigValueKind::Map(a), ConfigValueKind::Map(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|(a, b)| config_map_entry_content_eq(a, b))
        }
        _ => false, // Different variants
    }
}

/// Compare two ConfigMapEntry values for content equality, ignoring key_source.
fn config_map_entry_content_eq(a: &ConfigMapEntry, b: &ConfigMapEntry) -> bool {
    a.key == b.key && config_value_content_eq(&a.value, &b.value)
}

// =============================================================================
// Inline Splicing (Phase 5)
// =============================================================================

/// Check whether two blocks have equal source-visible attributes.
///
/// InlineSplice preserves the original block's prefix and suffix verbatim.
/// The suffix includes any explicit attribute text (e.g., `{.feature status="todo"}`).
/// If that text needs to change, InlineSplice produces wrong output.
///
/// We compare classes and key-value pairs (which are always in the source when present).
/// For the ID (attr.0), we only compare when the original block has an explicitly written
/// ID (`attr_source.id.is_some()`). Auto-generated IDs (derived from header text) are not
/// in the source, so changes to them don't affect the suffix and don't need Rewrite.
fn block_attrs_eq(a: &Block, b: &Block) -> bool {
    match (a, b) {
        (Block::Header(ha), Block::Header(hb)) => {
            let id_eq = if ha.attr_source.id.is_some() {
                ha.attr.0 == hb.attr.0
            } else {
                true
            };
            id_eq && ha.attr.1 == hb.attr.1 && ha.attr.2 == hb.attr.2
        }
        (Block::CodeBlock(ca), Block::CodeBlock(cb)) => {
            let id_eq = if ca.attr_source.id.is_some() {
                ca.attr.0 == cb.attr.0
            } else {
                true
            };
            id_eq && ca.attr.1 == cb.attr.1 && ca.attr.2 == cb.attr.2
        }
        (Block::Div(da), Block::Div(db)) => {
            let id_eq = if da.attr_source.id.is_some() {
                da.attr.0 == db.attr.0
            } else {
                true
            };
            id_eq && da.attr.1 == db.attr.1 && da.attr.2 == db.attr.2
        }
        // Blocks without attributes are always attr-equal
        _ => true,
    }
}

/// Extract the inline content of a block, if it's an inline-content block.
///
/// Returns `Some(&[Inline])` for Paragraph, Plain, and Header blocks;
/// `None` for all other block types (which contain blocks or are leaf blocks).
fn block_inlines(block: &Block) -> Option<&[Inline]> {
    match block {
        Block::Paragraph(p) => Some(&p.content),
        Block::Plain(p) => Some(&p.content),
        Block::Header(h) => Some(&h.content),
        _ => None,
    }
}

/// Assemble the block text for an InlineSplice entry.
///
/// Takes the original block text and replaces the inline content region
/// with the assembled new inline content from the reconciliation plan.
///
/// The block structure (prefix and suffix) is preserved from the original.
/// For example, a header's `## ` prefix and trailing `\n` are kept verbatim.
fn assemble_inline_splice(
    original_qmd: &str,
    orig_block: &Block,
    orig_inlines: &[Inline],
    new_inlines: &[Inline],
    plan: &InlineReconciliationPlan,
) -> Result<String, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let block_span = block_source_span(orig_block);

    // Compute the inline content region within the block
    let inline_start = inline_source_span(&orig_inlines[0]).start;
    let inline_end = inline_source_span(orig_inlines.last().unwrap()).end;

    // Block prefix: bytes before the first inline (e.g., "## " for headers)
    let prefix = &original_qmd[block_span.start..inline_start];
    // Block suffix: bytes after the last inline (e.g., "\n")
    let suffix = &original_qmd[inline_end..block_span.end];

    // Assemble the new inline content
    let inline_content = assemble_inline_content(original_qmd, orig_inlines, new_inlines, plan)?;

    Ok(format!("{}{}{}", prefix, inline_content, suffix))
}

/// Assemble the inline content from a reconciliation plan.
///
/// Walks the inline alignments and produces the result text by:
/// - KeepBefore: copying the original inline's bytes verbatim
/// - UseAfter: writing the new inline to a string
/// - RecurseIntoContainer: preserving delimiters, recursing into children
fn assemble_inline_content(
    original_qmd: &str,
    orig_inlines: &[Inline],
    new_inlines: &[Inline],
    plan: &InlineReconciliationPlan,
) -> Result<String, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut result = String::new();

    for (result_idx, alignment) in plan.inline_alignments.iter().enumerate() {
        match alignment {
            InlineAlignment::KeepBefore(orig_idx) => {
                let span = inline_source_span(&orig_inlines[*orig_idx]);
                result.push_str(&original_qmd[span]);
            }
            InlineAlignment::UseAfter(after_idx) => {
                let text = write_inline_to_string(&new_inlines[*after_idx])?;
                result.push_str(&text);
            }
            InlineAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => {
                let text = assemble_recursed_container(
                    original_qmd,
                    &orig_inlines[*before_idx],
                    &new_inlines[*after_idx],
                    plan.inline_container_plans.get(&result_idx),
                )?;
                result.push_str(&text);
            }
        }
    }

    Ok(result)
}

/// Assemble the text for a recursed container inline.
///
/// Preserves the container's delimiters from the original source and
/// recursively assembles the children from the nested plan.
fn assemble_recursed_container(
    original_qmd: &str,
    orig_inline: &Inline,
    new_inline: &Inline,
    nested_plan: Option<&InlineReconciliationPlan>,
) -> Result<String, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let orig_span = inline_source_span(orig_inline);

    let Some(plan) = nested_plan else {
        // No nested plan — container content is structurally identical.
        // Keep the original container bytes verbatim.
        return Ok(original_qmd[orig_span].to_string());
    };

    let orig_children = inline_children(orig_inline);
    let new_children = inline_children(new_inline);

    if orig_children.is_empty() {
        // No children to recurse into — keep original verbatim
        return Ok(original_qmd[orig_span].to_string());
    }

    // Opening delimiter: bytes from container start to first child start
    let first_child_start = inline_source_span(&orig_children[0]).start;
    let opening = &original_qmd[orig_span.start..first_child_start];

    // Closing delimiter: bytes from last child end to container end
    let last_child_end = inline_source_span(orig_children.last().unwrap()).end;
    let closing = &original_qmd[last_child_end..orig_span.end];

    // Recursively assemble children
    let children_text = assemble_inline_content(original_qmd, orig_children, new_children, plan)?;

    Ok(format!("{}{}{}", opening, children_text, closing))
}

// =============================================================================
// Inline Splicing: Safety Check
// =============================================================================

/// Check if an inline reconciliation plan can be safely spliced without
/// indentation context.
///
/// Safe iff every inline we'd actually write (UseAfter or rewritten within
/// RecurseIntoContainer) has a break-free subtree. This guarantees that no
/// inline patch output contains a `\n` character, so indentation prefixes
/// from enclosing BlockQuote/BulletList/OrderedList contexts are preserved.
///
/// See: claude-notes/plans/2026-02-10-inline-splicing.md
pub fn is_inline_splice_safe(new_inlines: &[Inline], plan: &InlineReconciliationPlan) -> bool {
    for (result_idx, alignment) in plan.inline_alignments.iter().enumerate() {
        match alignment {
            InlineAlignment::KeepBefore(_) => {
                // Preserved verbatim from original source — always safe.
                // The original bytes already contain correct indentation.
            }
            InlineAlignment::UseAfter(after_idx) => {
                // We'll write this inline fresh into a plain buffer.
                // If its subtree contains SoftBreak/LineBreak, the written
                // output will contain \n without indentation prefixes.
                if inline_subtree_has_break(&new_inlines[*after_idx]) {
                    return false;
                }
            }
            InlineAlignment::RecurseIntoContainer { after_idx, .. } => {
                // We'll recursively patch this container's children.
                // Check the nested plan: any child we write must also be break-free.
                if let Some(nested_plan) = plan.inline_container_plans.get(&result_idx) {
                    let children = inline_children(&new_inlines[*after_idx]);
                    if !is_inline_splice_safe(children, nested_plan) {
                        return false;
                    }
                }
                // If no nested plan, the container content is structurally
                // identical — it will be kept verbatim (safe).
            }
        }
    }
    true
}

/// Returns true if the inline or any descendant is SoftBreak or LineBreak.
pub fn inline_subtree_has_break(inline: &Inline) -> bool {
    matches!(inline, Inline::SoftBreak(_) | Inline::LineBreak(_))
        || inline_children(inline)
            .iter()
            .any(|child| inline_subtree_has_break(child))
}

/// Extract the child inlines of a container inline.
///
/// Returns an empty slice for leaf inlines (Str, Space, Code, etc.)
/// and for Note inlines (which contain Blocks, not Inlines).
pub fn inline_children(inline: &Inline) -> &[Inline] {
    match inline {
        // Container inlines with inline content
        Inline::Emph(e) => &e.content,
        Inline::Strong(s) => &s.content,
        Inline::Underline(u) => &u.content,
        Inline::Strikeout(s) => &s.content,
        Inline::Superscript(s) => &s.content,
        Inline::Subscript(s) => &s.content,
        Inline::SmallCaps(s) => &s.content,
        Inline::Quoted(q) => &q.content,
        Inline::Cite(c) => &c.content,
        Inline::Link(l) => &l.content,
        Inline::Image(i) => &i.content,
        Inline::Span(s) => &s.content,
        Inline::Insert(i) => &i.content,
        Inline::Delete(d) => &d.content,
        Inline::Highlight(h) => &h.content,
        Inline::EditComment(e) => &e.content,
        // Leaf inlines and special cases — no inline children
        Inline::Str(_)
        | Inline::Code(_)
        | Inline::Space(_)
        | Inline::SoftBreak(_)
        | Inline::LineBreak(_)
        | Inline::Math(_)
        | Inline::RawInline(_)
        | Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_)
        | Inline::Note(_) // Note contains Blocks, not Inlines
        | Inline::Custom(_) => &[],
    }
}

/// Extract the byte range (start..end) from an Inline's source_info.
pub fn inline_source_span(inline: &Inline) -> Range<usize> {
    let si = inline.source_info();
    si.start_offset()..si.end_offset()
}

/// Write a single inline to a String using the standard QMD writer.
///
/// This writes without indentation context — only safe for inlines whose
/// subtree contains no SoftBreak/LineBreak (as guaranteed by
/// `is_inline_splice_safe`).
pub fn write_inline_to_string(
    inline: &Inline,
) -> Result<String, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut buf = Vec::new();
    qmd::write_single_inline(inline, &mut buf)?;
    let result = String::from_utf8(buf).map_err(|e| {
        vec![
            quarto_error_reporting::DiagnosticMessageBuilder::error("UTF-8 error during write")
                .with_code("Q-3-2")
                .problem(format!("Inline writer produced invalid UTF-8: {}", e))
                .build(),
        ]
    })?;
    // Debug assertion: the safety check should have ensured no newlines
    debug_assert!(
        !result.contains('\n'),
        "write_inline_to_string produced output with newline: {:?}. \
         This inline should have been rejected by is_inline_splice_safe.",
        result,
    );
    Ok(result)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod editability_tests {
    use super::*;
    use quarto_pandoc_types::{Block, CustomNode, Inline, Paragraph, Plain, Str, attr::empty_attr};
    use quarto_source_map::source_info::{AnchorRole, By};
    use std::sync::Arc;

    const TARGET: FileId = FileId(0);
    const OTHER: FileId = FileId(1);

    fn make_str(text: &str, si: SourceInfo) -> Inline {
        Inline::Str(Str {
            text: text.into(),
            source_info: si,
        })
    }

    // -------------------------------------------------------------------------
    // is_editable_inside_block
    // -------------------------------------------------------------------------

    #[test]
    fn editable_block_with_original_in_target() {
        let block = Block::Paragraph(Paragraph {
            content: vec![make_str("hello", SourceInfo::original(TARGET, 0, 5))],
            source_info: SourceInfo::original(TARGET, 0, 5),
        });
        assert!(is_editable_inside_block(&block, TARGET));
    }

    #[test]
    fn not_editable_block_with_original_outside_target() {
        // Original points at a different file (cross-file reference, no
        // wrapper). preimage_in(TARGET) returns None.
        let block = Block::Paragraph(Paragraph {
            content: vec![make_str("hi", SourceInfo::original(OTHER, 0, 2))],
            source_info: SourceInfo::original(OTHER, 0, 2),
        });
        assert!(!is_editable_inside_block(&block, TARGET));
    }

    #[test]
    fn not_editable_atomic_custom_node_block() {
        // CrossrefResolvedRef is in ATOMIC_CUSTOM_NODES even though its
        // source_info Original is in the target file.
        let cn = CustomNode::new(
            "CrossrefResolvedRef",
            empty_attr(),
            SourceInfo::original(TARGET, 0, 10),
        );
        let block = Block::Custom(cn);
        assert!(!is_editable_inside_block(&block, TARGET));
    }

    #[test]
    fn editable_non_atomic_custom_node_block() {
        // Non-atomic CustomNode (e.g., Callout) with source_info in target
        // → editable.
        let cn = CustomNode::new("Callout", empty_attr(), SourceInfo::original(TARGET, 0, 20));
        let block = Block::Custom(cn);
        assert!(is_editable_inside_block(&block, TARGET));
    }

    #[test]
    fn not_editable_atomic_kind_generated_block() {
        // Shortcode-resolved Para: Generated{by: shortcode, from: [Invocation]}.
        // Even though Invocation resolves to a token in TARGET (so
        // preimage_in returns Some), is_atomic_kind() shortcode means the
        // user can't edit the *resolved content* — only the token.
        let token = SourceInfo::original(TARGET, 100, 120);
        let mut gen_info = SourceInfo::generated(By::shortcode("meta"));
        gen_info.append_anchor(AnchorRole::Invocation, Arc::new(token));
        let block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: gen_info,
        });
        assert!(!is_editable_inside_block(&block, TARGET));
    }

    #[test]
    fn not_editable_no_preimage_generated_block() {
        // Synthesized-from-metadata container: Generated with empty
        // anchors (sectionize / footnotes / appendix container shape).
        // preimage_in returns None → not editable.
        let block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: SourceInfo::generated(By::sectionize()),
        });
        assert!(!is_editable_inside_block(&block, TARGET));
    }

    #[test]
    fn not_editable_value_source_only_generated_block() {
        // Plan 9 shape: Generated with only ValueSource anchor (no
        // Invocation). The ValueSource points into the target file's
        // YAML metadata range, but the writer must NOT treat the
        // interior as editable — those bytes are YAML, not body.
        let meta_si = SourceInfo::original(TARGET, 10, 25);
        let mut gen_info = SourceInfo::generated(By::appendix());
        gen_info.append_anchor(AnchorRole::ValueSource, Arc::new(meta_si));
        let block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: gen_info,
        });
        assert!(!is_editable_inside_block(&block, TARGET));
    }

    // -------------------------------------------------------------------------
    // is_editable_inside_inline
    // -------------------------------------------------------------------------

    #[test]
    fn editable_inline_with_original_in_target() {
        let inline = make_str("hi", SourceInfo::original(TARGET, 0, 2));
        assert!(is_editable_inside_inline(&inline, TARGET));
    }

    #[test]
    fn not_editable_atomic_custom_node_inline() {
        let cn = CustomNode::new(
            "CrossrefResolvedRef",
            empty_attr(),
            SourceInfo::original(TARGET, 0, 8),
        );
        let inline = Inline::Custom(cn);
        assert!(!is_editable_inside_inline(&inline, TARGET));
    }

    #[test]
    fn not_editable_atomic_kind_generated_inline() {
        let token = SourceInfo::original(TARGET, 100, 120);
        let mut gen_info = SourceInfo::generated(By::shortcode("meta"));
        gen_info.append_anchor(AnchorRole::Invocation, Arc::new(token));
        let inline = make_str("resolved", gen_info);
        assert!(!is_editable_inside_inline(&inline, TARGET));
    }

    #[test]
    fn not_editable_inline_with_original_outside_target() {
        let inline = make_str("hi", SourceInfo::original(OTHER, 0, 2));
        assert!(!is_editable_inside_inline(&inline, TARGET));
    }

    // -------------------------------------------------------------------------
    // Sanity: Plain (non-Para) block carries the same predicate behaviour.
    // -------------------------------------------------------------------------

    #[test]
    fn editable_plain_block_with_original_in_target() {
        let block = Block::Plain(Plain {
            content: vec![make_str("hi", SourceInfo::original(TARGET, 0, 2))],
            source_info: SourceInfo::original(TARGET, 0, 2),
        });
        assert!(is_editable_inside_block(&block, TARGET));
    }
}
