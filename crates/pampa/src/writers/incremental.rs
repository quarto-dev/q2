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
    let target_file_id = derive_target_file_id(&original_ast.blocks);
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
                            // Safe to splice — assemble the patched block text.
                            // Returns None when edge inlines have no usable preimage
                            // (Concat/Generated-led); fall back to full re-serialization.
                            match assemble_inline_splice(
                                original_qmd,
                                orig_block,
                                orig_inlines,
                                new_inlines,
                                inline_plan,
                                target_file_id,
                            )? {
                                Some(block_text) => CoarsenedEntry::InlineSplice {
                                    block_text,
                                    orig_idx: *before_idx,
                                },
                                None => CoarsenedEntry::Rewrite {
                                    new_idx: result_idx,
                                },
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

/// Derive the file ID of the source file being edited, used for
/// `preimage_in` lookups in the incremental writer.
///
/// Scans the top-level blocks for the first one whose `source_info`
/// reports a real file ID via `root_file_id()`. Falls back to
/// `FileId(0)` for the empty document.
fn derive_target_file_id(blocks: &[Block]) -> FileId {
    blocks
        .iter()
        .find_map(|b| b.source_info().root_file_id())
        .unwrap_or(FileId(0))
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
    target_file_id: FileId,
) -> Result<Option<String>, Vec<quarto_error_reporting::DiagnosticMessage>> {
    // Boundaries must come from `preimage_in`, NOT `start_offset()`/`end_offset()`.
    // A `Concat`/`Generated`-led inline reports the sentinel `0` for
    // `start_offset()` — e.g. `Str "Table:"` parses as a contiguous
    // `Concat[Original "Table" ++ Original ":"]` — which made the prefix slice
    // `original_qmd[block.start .. 0]` reverse and panic (b43fadef).
    // `preimage_in` resolves the real (hull) byte range. When any boundary is
    // unavailable, we cannot splice safely: return `None` so the caller
    // re-serializes the whole block.
    let (Some(block_range), Some(first_range), Some(last_range)) = (
        orig_block.source_info().preimage_in(target_file_id),
        orig_inlines
            .first()
            .and_then(|i| i.source_info().preimage_in(target_file_id)),
        orig_inlines
            .last()
            .and_then(|i| i.source_info().preimage_in(target_file_id)),
    ) else {
        return Ok(None);
    };

    // Guard ordering so a stray provenance can never produce a reversed slice.
    if block_range.start > first_range.start
        || last_range.end > block_range.end
        || first_range.start > last_range.end
    {
        return Ok(None);
    }

    // Block prefix: bytes before the first inline (e.g., "## " for headers).
    // Block suffix: bytes after the last inline (e.g., "\n"). `.get()` keeps
    // this structurally safe even if the guards above ever miss a case.
    let (Some(prefix), Some(suffix)) = (
        original_qmd.get(block_range.start..first_range.start),
        original_qmd.get(last_range.end..block_range.end),
    ) else {
        return Ok(None);
    };

    // Assemble the new inline content
    let inline_content = assemble_inline_content(
        original_qmd,
        orig_inlines,
        new_inlines,
        plan,
        target_file_id,
    )?;

    Ok(Some(format!("{}{}{}", prefix, inline_content, suffix)))
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
    target_file_id: FileId,
) -> Result<String, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut result = String::new();

    for (result_idx, alignment) in plan.inline_alignments.iter().enumerate() {
        match alignment {
            InlineAlignment::KeepBefore(orig_idx) => {
                // Use preimage_in so Concat/Generated inlines (which return the
                // sentinel 0 from start_offset()/end_offset()) copy the correct
                // byte hull rather than an empty slice. Falls back to
                // inline_source_span for Original inlines (identical bytes).
                let range = orig_inlines[*orig_idx]
                    .source_info()
                    .preimage_in(target_file_id)
                    .unwrap_or_else(|| inline_source_span(&orig_inlines[*orig_idx]));
                result.push_str(&original_qmd[range]);
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
                    target_file_id,
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
    target_file_id: FileId,
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
    let children_text = assemble_inline_content(
        original_qmd,
        orig_children,
        new_children,
        plan,
        target_file_id,
    )?;

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
// Plan 7g Phase 1 — Source-range tiling auditor
// =============================================================================

/// A finding emitted by [`audit_source_range_tiling`].
///
/// Usable both as a census row (tally by `kind`) and as a test-failure
/// message (`message` includes node type, byte ranges, and relevant context).
///
/// See Plan 7g § Phase 1.
#[derive(Debug, Clone)]
pub struct TilingFinding {
    /// What kind of issue this is.
    pub kind: TilingFindingKind,
    /// Human-readable description: node type, ranges, context.
    pub message: String,
}

/// Kinds of finding emitted by [`audit_source_range_tiling`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TilingFindingKind {
    /// **(a)** Two sibling units claim overlapping source bytes.
    SiblingOverlap,
    /// **(b)** A child's range is not contained in its parent's range.
    ContainmentViolation,
    /// **(c)** An inline node's boundary byte is `' '` or `'\t'`.
    TightnessViolation,
    /// Non-contiguous `Concat` whose every piece-gap contains only
    /// `' '`/`'\t'` (no newlines). A producer bug: fix with a
    /// contiguous hull (Plan 7g Phase 4b template).
    WhitespaceGapConcat,
    /// Non-contiguous `Concat` with a gap that contains non-whitespace or
    /// a newline, or a piece that fails to resolve to `target`.
    /// Needs Phase-2 World 1 / World 2 triage; **always stop and report
    /// to the user before acting** (unconditional gate in Phase 2).
    ScatteredConcat,
    /// `Generated` node with no `Invocation` anchor.
    /// Census tally only; the node makes no contiguous source claim.
    GeneratedNoInvocation,
    /// `Attr` kv / class count mismatches `AttrSourceInfo` length —
    /// attr range auditing skipped for this node (bd-3aolj / bd-1e6a5).
    AttrAlignmentSkipped,
}

// ─── Internal audit unit ─────────────────────────────────────────────────────

/// One unit in the sibling disjointness check.
///
/// Same-`Invocation` siblings are collapsed to one unit (P4 refinement 2).
/// Nodes whose `SourceInfo::preimage_in(target)` returns `None` are
/// excluded entirely — their census findings are emitted separately.
struct AuditUnit {
    node_type: &'static str,
    range: std::ops::Range<usize>,
    /// `Some(inv)` when this unit represents a collapsed same-`Invocation`
    /// group. The `Arc<SourceInfo>` is the shared anchor; equality is
    /// tested via `PartialEq` on the pointed-to value (same as the writer
    /// at `assemble_inline_content` ~line 1424).
    invocation_key: Option<std::sync::Arc<SourceInfo>>,
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Audit the source-range tiling properties of `ast` against `source`.
///
/// # Per-level check matrix
///
/// | Check           | Block level | Inline level |
/// |-----------------|-------------|--------------|
/// | (a) disjoint    | ✓           | ✓            |
/// | (b) containment | ✓           | ✓            |
/// | (c) tightness   | —           | ✓            |
///
/// Block ranges legitimately abut newlines/blank lines; tightness is only
/// meaningful for inlines. Tightness checks space/tab boundary bytes only —
/// a newline at a boundary is **not** a violation (decided 2026-06-03).
///
/// # `None`-preimage classification
///
/// Nodes whose `preimage_in(target)` returns `None` are excluded from
/// checks (a)/(b)/(c) but emit census rows:
/// - `WhitespaceGapConcat` — non-contiguous `Concat` with whitespace-only gaps.
/// - `ScatteredConcat` — content/newline gap or unresolvable piece.
/// - `GeneratedNoInvocation` — `Generated` with no `Invocation` anchor.
///
/// # Same-`Invocation` grouping (P4 refinement 2)
///
/// `Generated` siblings sharing the same `Invocation` anchor (compared via
/// `PartialEq` on the `SourceInfo` value) are collapsed to one unit.
/// This is the same predicate the incremental writer uses at ~line 1424.
///
/// # Attr ranges
///
/// For each attr-bearing node the kv key/value `SourceInfo`s are checked
/// for tightness (c), containment (b), and mutual disjointness (a),
/// guarded by the alignment check (`kvs.len() == attributes.len()`).
///
/// See `claude-notes/plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md`.
pub fn audit_source_range_tiling(ast: &Pandoc, source: &str) -> Vec<TilingFinding> {
    let target = derive_target_file_id(&ast.blocks);
    let src = source.as_bytes();
    let mut findings = Vec::new();
    audit_block_siblings(&ast.blocks, None, target, src, &mut findings);
    findings
}

// ─── Block-level walk ────────────────────────────────────────────────────────

fn audit_block_siblings(
    blocks: &[Block],
    parent_range: Option<std::ops::Range<usize>>,
    target: FileId,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) {
    let units = resolve_units_from_iter(
        blocks.iter().map(|b| (block_node_type(b), b.source_info())),
        target,
        src,
        findings,
    );

    // (a) Sibling disjointness between units.
    check_sibling_disjointness(&units, findings);

    // (b) Containment of each unit against parent.
    if let Some(ref par) = parent_range {
        for u in &units {
            check_containment(par, &u.range, u.node_type, findings);
        }
    }

    // Recurse into each block's children.
    for block in blocks {
        let block_range = block.source_info().preimage_in(target);
        audit_block_node_children(block, block_range, target, src, findings);
    }
}

fn audit_block_node_children(
    block: &Block,
    block_range: Option<std::ops::Range<usize>>,
    target: FileId,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) {
    match block {
        Block::Plain(p) => {
            audit_inline_siblings(&p.content, block_range, target, src, findings);
        }
        Block::Paragraph(p) => {
            audit_inline_siblings(&p.content, block_range, target, src, findings);
        }
        Block::LineBlock(l) => {
            for line in &l.content {
                audit_inline_siblings(line, block_range.clone(), target, src, findings);
            }
        }
        Block::CodeBlock(c) => {
            audit_attr_source(
                &c.attr_source,
                c.attr.2.len(),
                c.attr.1.len(),
                block_range,
                target,
                src,
                findings,
            );
        }
        Block::RawBlock(_) => {}
        Block::BlockQuote(b) => {
            audit_block_siblings(&b.content, block_range, target, src, findings);
        }
        Block::OrderedList(l) => {
            for item in &l.content {
                audit_block_siblings(item, block_range.clone(), target, src, findings);
            }
        }
        Block::BulletList(l) => {
            for item in &l.content {
                audit_block_siblings(item, block_range.clone(), target, src, findings);
            }
        }
        Block::DefinitionList(d) => {
            for (term, defs) in &d.content {
                audit_inline_siblings(term, block_range.clone(), target, src, findings);
                for def in defs {
                    audit_block_siblings(def, block_range.clone(), target, src, findings);
                }
            }
        }
        Block::Header(h) => {
            audit_attr_source(
                &h.attr_source,
                h.attr.2.len(),
                h.attr.1.len(),
                block_range.clone(),
                target,
                src,
                findings,
            );
            audit_inline_siblings(&h.content, block_range, target, src, findings);
        }
        Block::HorizontalRule(_) => {}
        Block::Table(t) => {
            audit_attr_source(
                &t.attr_source,
                t.attr.2.len(),
                t.attr.1.len(),
                block_range.clone(),
                target,
                src,
                findings,
            );
            audit_table(t, target, src, findings);
        }
        Block::Figure(f) => {
            audit_attr_source(
                &f.attr_source,
                f.attr.2.len(),
                f.attr.1.len(),
                block_range.clone(),
                target,
                src,
                findings,
            );
            if let Some(ref short) = f.caption.short {
                audit_inline_siblings(short, block_range.clone(), target, src, findings);
            }
            if let Some(ref long) = f.caption.long {
                audit_block_siblings(long, block_range.clone(), target, src, findings);
            }
            audit_block_siblings(&f.content, block_range, target, src, findings);
        }
        Block::Div(d) => {
            audit_attr_source(
                &d.attr_source,
                d.attr.2.len(),
                d.attr.1.len(),
                block_range.clone(),
                target,
                src,
                findings,
            );
            audit_block_siblings(&d.content, block_range, target, src, findings);
        }
        Block::BlockMetadata(_) => {}
        Block::NoteDefinitionPara(n) => {
            audit_inline_siblings(&n.content, block_range, target, src, findings);
        }
        Block::NoteDefinitionFencedBlock(n) => {
            audit_block_siblings(&n.content, block_range, target, src, findings);
        }
        Block::CaptionBlock(c) => {
            audit_inline_siblings(&c.content, block_range, target, src, findings);
        }
        Block::Custom(c) => {
            audit_custom_node(c, block_range, target, src, findings);
        }
    }
}

fn audit_table(
    t: &quarto_pandoc_types::table::Table,
    target: FileId,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) {
    macro_rules! audit_rows {
        ($rows:expr) => {
            for row in $rows {
                let row_range = row.source_info.preimage_in(target);
                audit_attr_source(
                    &row.attr_source,
                    row.attr.2.len(),
                    row.attr.1.len(),
                    row_range.clone(),
                    target,
                    src,
                    findings,
                );
                for cell in &row.cells {
                    let cell_range = cell.source_info.preimage_in(target);
                    audit_attr_source(
                        &cell.attr_source,
                        cell.attr.2.len(),
                        cell.attr.1.len(),
                        cell_range.clone(),
                        target,
                        src,
                        findings,
                    );
                    audit_block_siblings(&cell.content, cell_range, target, src, findings);
                }
            }
        };
    }

    audit_rows!(&t.head.rows);
    for body in &t.bodies {
        audit_rows!(&body.head);
        audit_rows!(&body.body);
    }
    audit_rows!(&t.foot.rows);
}

fn audit_custom_node(
    node: &quarto_pandoc_types::custom::CustomNode,
    parent_range: Option<std::ops::Range<usize>>,
    target: FileId,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) {
    use quarto_pandoc_types::custom::Slot;
    for (_, slot) in &node.slots {
        match slot {
            Slot::Block(b) => {
                let child_range = b.source_info().preimage_in(target);
                if let (Some(par), Some(child)) = (&parent_range, &child_range) {
                    check_containment(par, child, "CustomNode.slot.Block", findings);
                }
                audit_block_node_children(b, child_range, target, src, findings);
            }
            Slot::Inline(i) => {
                let child_range = i.source_info().preimage_in(target);
                if let (Some(par), Some(child)) = (&parent_range, &child_range) {
                    check_containment(par, child, "CustomNode.slot.Inline", findings);
                }
                if let Some(ref r) = child_range {
                    check_tightness(inline_node_type(i), r, src, findings);
                }
                audit_inline_node_children(i, child_range, target, src, findings);
            }
            Slot::Blocks(bs) => {
                audit_block_siblings(bs, parent_range.clone(), target, src, findings);
            }
            Slot::Inlines(is) => {
                audit_inline_siblings(is, parent_range.clone(), target, src, findings);
            }
        }
    }
}

// ─── Inline-level walk ───────────────────────────────────────────────────────

fn audit_inline_siblings(
    inlines: &[Inline],
    parent_range: Option<std::ops::Range<usize>>,
    target: FileId,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) {
    let units = resolve_units_from_iter(
        inlines
            .iter()
            .map(|i| (inline_node_type(i), i.source_info())),
        target,
        src,
        findings,
    );

    // (a) Sibling disjointness.
    check_sibling_disjointness(&units, findings);

    // (b) Containment of each unit against parent.
    if let Some(ref par) = parent_range {
        for u in &units {
            check_containment(par, &u.range, u.node_type, findings);
        }
    }

    // (c) Tightness — inline level only.
    // Space, SoftBreak, LineBreak are excluded: their ranges *correctly*
    // contain whitespace by definition (a Space node IS a space character).
    // Overlap check (a) already catches the case where a Space node wrongly
    // absorbs non-whitespace bytes from a sibling.
    for inline in inlines {
        if matches!(
            inline,
            Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_)
        ) {
            continue;
        }
        if let Some(ref r) = inline.source_info().preimage_in(target) {
            check_tightness(inline_node_type(inline), r, src, findings);
        }
    }

    // Recurse into each inline's children.
    for inline in inlines {
        let inline_range = inline.source_info().preimage_in(target);
        audit_inline_node_children(inline, inline_range, target, src, findings);
    }
}

fn audit_inline_node_children(
    inline: &Inline,
    inline_range: Option<std::ops::Range<usize>>,
    target: FileId,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) {
    match inline {
        Inline::Emph(e) => {
            audit_inline_siblings(&e.content, inline_range, target, src, findings);
        }
        Inline::Underline(u) => {
            audit_inline_siblings(&u.content, inline_range, target, src, findings);
        }
        Inline::Strong(s) => {
            audit_inline_siblings(&s.content, inline_range, target, src, findings);
        }
        Inline::Strikeout(s) => {
            audit_inline_siblings(&s.content, inline_range, target, src, findings);
        }
        Inline::Superscript(s) => {
            audit_inline_siblings(&s.content, inline_range, target, src, findings);
        }
        Inline::Subscript(s) => {
            audit_inline_siblings(&s.content, inline_range, target, src, findings);
        }
        Inline::SmallCaps(s) => {
            audit_inline_siblings(&s.content, inline_range, target, src, findings);
        }
        Inline::Quoted(q) => {
            audit_inline_siblings(&q.content, inline_range, target, src, findings);
        }
        Inline::Cite(c) => {
            audit_inline_siblings(&c.content, inline_range, target, src, findings);
        }
        Inline::Code(c) => {
            audit_attr_source(
                &c.attr_source,
                c.attr.2.len(),
                c.attr.1.len(),
                inline_range,
                target,
                src,
                findings,
            );
        }
        Inline::Link(l) => {
            audit_attr_source(
                &l.attr_source,
                l.attr.2.len(),
                l.attr.1.len(),
                inline_range.clone(),
                target,
                src,
                findings,
            );
            audit_inline_siblings(&l.content, inline_range, target, src, findings);
        }
        Inline::Image(i) => {
            audit_attr_source(
                &i.attr_source,
                i.attr.2.len(),
                i.attr.1.len(),
                inline_range.clone(),
                target,
                src,
                findings,
            );
            audit_inline_siblings(&i.content, inline_range, target, src, findings);
        }
        Inline::Span(s) => {
            audit_attr_source(
                &s.attr_source,
                s.attr.2.len(),
                s.attr.1.len(),
                inline_range.clone(),
                target,
                src,
                findings,
            );
            audit_inline_siblings(&s.content, inline_range, target, src, findings);
        }
        Inline::Note(n) => {
            audit_block_siblings(&n.content, inline_range, target, src, findings);
        }
        Inline::Insert(i) => {
            audit_attr_source(
                &i.attr_source,
                i.attr.2.len(),
                i.attr.1.len(),
                inline_range.clone(),
                target,
                src,
                findings,
            );
            audit_inline_siblings(&i.content, inline_range, target, src, findings);
        }
        Inline::Delete(d) => {
            audit_attr_source(
                &d.attr_source,
                d.attr.2.len(),
                d.attr.1.len(),
                inline_range.clone(),
                target,
                src,
                findings,
            );
            audit_inline_siblings(&d.content, inline_range, target, src, findings);
        }
        Inline::Highlight(h) => {
            audit_attr_source(
                &h.attr_source,
                h.attr.2.len(),
                h.attr.1.len(),
                inline_range.clone(),
                target,
                src,
                findings,
            );
            audit_inline_siblings(&h.content, inline_range, target, src, findings);
        }
        Inline::EditComment(e) => {
            audit_attr_source(
                &e.attr_source,
                e.attr.2.len(),
                e.attr.1.len(),
                inline_range.clone(),
                target,
                src,
                findings,
            );
            audit_inline_siblings(&e.content, inline_range, target, src, findings);
        }
        Inline::Attr(a) => {
            audit_attr_source(
                &a.attr_source,
                a.attr.2.len(),
                a.attr.1.len(),
                inline_range,
                target,
                src,
                findings,
            );
        }
        Inline::Custom(c) => {
            audit_custom_node(c, inline_range, target, src, findings);
        }
        // Leaves — no children: Str, Space, SoftBreak, LineBreak, Math,
        // RawInline, Shortcode, NoteReference.
        _ => {}
    }
}

// ─── Attr sidecar audit ───────────────────────────────────────────────────────

fn audit_attr_source(
    attr_source: &quarto_pandoc_types::attr::AttrSourceInfo,
    kvs_len: usize,
    classes_len: usize,
    parent_range: Option<std::ops::Range<usize>>,
    target: FileId,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) {
    // Alignment guard (bd-3aolj / bd-1e6a5).
    if kvs_len != attr_source.attributes.len() || classes_len != attr_source.classes.len() {
        findings.push(TilingFinding {
            kind: TilingFindingKind::AttrAlignmentSkipped,
            message: format!(
                "AttrAlignmentSkipped: kvs_len={kvs_len} attributes.len()={} / \
                 classes_len={classes_len} classes.len()={}",
                attr_source.attributes.len(),
                attr_source.classes.len(),
            ),
        });
        return;
    }

    // Collect all kv source-info ranges that resolve, checking tightness and
    // containment on each, then check mutual disjointness among them.
    let mut kv_units: Vec<AuditUnit> = Vec::new();
    for (key_src, val_src) in &attr_source.attributes {
        for (label, opt_si) in [
            ("attr-key", key_src as &Option<SourceInfo>),
            ("attr-val", val_src),
        ] {
            let Some(si) = opt_si else { continue };
            let Some(range) = si.preimage_in(target) else {
                continue;
            };
            check_tightness(label, &range, src, findings);
            if let Some(ref par) = parent_range {
                check_containment(par, &range, label, findings);
            }
            kv_units.push(AuditUnit {
                node_type: label,
                range,
                invocation_key: None,
            });
        }
    }
    check_sibling_disjointness(&kv_units, findings);
}

// ─── Unit resolution (same-Invocation grouping + None-preimage census) ────────

/// Resolve each `(node_type, SourceInfo)` pair into an [`AuditUnit`], emitting
/// census findings for `None`-preimage nodes.
///
/// `Generated` siblings that share the same `Invocation` anchor are collapsed
/// to one unit (P4 refinement 2). The grouping predicate is `PartialEq` on
/// the `SourceInfo` value of the anchor — the same predicate the incremental
/// writer uses at `assemble_inline_content` ~line 1424.
fn resolve_units_from_iter<'a>(
    nodes: impl Iterator<Item = (&'static str, &'a SourceInfo)>,
    target: FileId,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) -> Vec<AuditUnit> {
    let mut units: Vec<AuditUnit> = Vec::new();

    for (node_type, si) in nodes {
        match si.preimage_in(target) {
            Some(range) => {
                // Same-Invocation grouping: if this node is Generated with an
                // Invocation anchor already represented in `units`, skip it.
                let invocation_key = si.invocation_anchor().cloned();
                if let Some(ref inv) = invocation_key {
                    let already_grouped = units.iter().any(|u| {
                        u.invocation_key
                            .as_ref()
                            .is_some_and(|k| k.as_ref() == inv.as_ref())
                    });
                    if already_grouped {
                        continue;
                    }
                }
                units.push(AuditUnit {
                    node_type,
                    range,
                    invocation_key,
                });
            }
            None => {
                classify_none_preimage(node_type, si, target, src, findings);
            }
        }
    }

    units
}

// ─── None-preimage classification ────────────────────────────────────────────

fn classify_none_preimage(
    node_type: &str,
    si: &SourceInfo,
    target: FileId,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) {
    match si {
        SourceInfo::Generated { .. } => {
            if si.invocation_anchor().is_none() {
                findings.push(TilingFinding {
                    kind: TilingFindingKind::GeneratedNoInvocation,
                    message: format!(
                        "GeneratedNoInvocation: `{node_type}` has no Invocation anchor"
                    ),
                });
            }
            // Invocation exists but resolves to None (different file) — skip silently.
        }
        SourceInfo::Concat { pieces } => {
            classify_none_concat(node_type, pieces, target, src, findings);
        }
        // Original / Substring in a different file — expected, no finding.
        _ => {}
    }
}

fn classify_none_concat(
    node_type: &str,
    pieces: &[quarto_source_map::SourcePiece],
    target: FileId,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) {
    if pieces.is_empty() {
        return;
    }

    // Try to resolve every piece to target.
    let piece_ranges: Option<Vec<std::ops::Range<usize>>> = pieces
        .iter()
        .map(|p| p.source_info.preimage_in(target))
        .collect();

    let Some(ranges) = piece_ranges else {
        // At least one piece doesn't resolve → ScatteredConcat.
        findings.push(TilingFinding {
            kind: TilingFindingKind::ScatteredConcat,
            message: format!(
                "ScatteredConcat `{node_type}`: one or more Concat pieces \
                 fail to resolve to target"
            ),
        });
        return;
    };

    // All pieces resolved — inspect inter-piece gaps.
    // Whitespace for this check is space/tab only; a newline disqualifies.
    let all_whitespace_gaps = ranges.windows(2).all(|w| {
        let gap_start = w[0].end;
        let gap_end = w[1].start;
        if gap_start > gap_end {
            return false; // Pieces overlap or misordered.
        }
        let gap = src.get(gap_start..gap_end).unwrap_or(&[]);
        gap.iter().all(|&b| b == b' ' || b == b'\t')
    });

    if all_whitespace_gaps {
        findings.push(TilingFinding {
            kind: TilingFindingKind::WhitespaceGapConcat,
            message: format!(
                "WhitespaceGapConcat `{node_type}`: non-contiguous Concat with \
                 whitespace-only gaps (producer bug, Phase 4b class)"
            ),
        });
    } else {
        findings.push(TilingFinding {
            kind: TilingFindingKind::ScatteredConcat,
            message: format!(
                "ScatteredConcat `{node_type}`: non-contiguous Concat with \
                 content/newline gaps (needs Phase-2 World 1/2 triage)"
            ),
        });
    }
}

// ─── Check helpers ────────────────────────────────────────────────────────────

/// **(a)** Pairwise sibling non-overlap between units.
fn check_sibling_disjointness(units: &[AuditUnit], findings: &mut Vec<TilingFinding>) {
    for i in 0..units.len() {
        for j in (i + 1)..units.len() {
            let a = &units[i];
            let b = &units[j];
            let overlap_start = a.range.start.max(b.range.start);
            let overlap_end = a.range.end.min(b.range.end);
            if overlap_start < overlap_end {
                findings.push(TilingFinding {
                    kind: TilingFindingKind::SiblingOverlap,
                    message: format!(
                        "SiblingOverlap: `{}` [{}..{}] overlaps `{}` [{}..{}] \
                         (shared bytes [{}..{}])",
                        a.node_type,
                        a.range.start,
                        a.range.end,
                        b.node_type,
                        b.range.start,
                        b.range.end,
                        overlap_start,
                        overlap_end,
                    ),
                });
            }
        }
    }
}

/// **(b)** Assert `child ⊆ parent` (non-strict: equality is allowed).
fn check_containment(
    parent: &std::ops::Range<usize>,
    child: &std::ops::Range<usize>,
    child_type: &str,
    findings: &mut Vec<TilingFinding>,
) {
    if child.start < parent.start || child.end > parent.end {
        findings.push(TilingFinding {
            kind: TilingFindingKind::ContainmentViolation,
            message: format!(
                "ContainmentViolation: child `{child_type}` [{}..{}] not contained \
                 in parent [{}..{}]",
                child.start, child.end, parent.start, parent.end,
            ),
        });
    }
}

/// **(c)** Tightness: boundary bytes of `range` must not be `' '` or `'\t'`.
///
/// Inline level only. Newlines at boundaries are **not** violations
/// (decided 2026-06-03 round-2 review). Empty ranges are vacuously tight.
fn check_tightness(
    node_type: &str,
    range: &std::ops::Range<usize>,
    src: &[u8],
    findings: &mut Vec<TilingFinding>,
) {
    if range.is_empty() {
        return;
    }
    let (s, e) = (range.start, range.end);
    for (boundary, byte_opt) in [
        ("leading", src.get(s).copied()),
        (
            "trailing",
            if e > 0 { src.get(e - 1).copied() } else { None },
        ),
    ] {
        if let Some(b) = byte_opt {
            if b == b' ' || b == b'\t' {
                findings.push(TilingFinding {
                    kind: TilingFindingKind::TightnessViolation,
                    message: format!(
                        "TightnessViolation: `{node_type}` [{}..{}] has {boundary} \
                         space/tab byte ({:?})",
                        s, e, b as char,
                    ),
                });
            }
        }
    }
}

// ─── Node type names ──────────────────────────────────────────────────────────

fn block_node_type(block: &Block) -> &'static str {
    match block {
        Block::Plain(_) => "Plain",
        Block::Paragraph(_) => "Paragraph",
        Block::LineBlock(_) => "LineBlock",
        Block::CodeBlock(_) => "CodeBlock",
        Block::RawBlock(_) => "RawBlock",
        Block::BlockQuote(_) => "BlockQuote",
        Block::OrderedList(_) => "OrderedList",
        Block::BulletList(_) => "BulletList",
        Block::DefinitionList(_) => "DefinitionList",
        Block::Header(_) => "Header",
        Block::HorizontalRule(_) => "HorizontalRule",
        Block::Table(_) => "Table",
        Block::Figure(_) => "Figure",
        Block::Div(_) => "Div",
        Block::BlockMetadata(_) => "BlockMetadata",
        Block::NoteDefinitionPara(_) => "NoteDefinitionPara",
        Block::NoteDefinitionFencedBlock(_) => "NoteDefinitionFencedBlock",
        Block::CaptionBlock(_) => "CaptionBlock",
        Block::Custom(_) => "Custom",
    }
}

fn inline_node_type(inline: &Inline) -> &'static str {
    match inline {
        Inline::Str(_) => "Str",
        Inline::Emph(_) => "Emph",
        Inline::Underline(_) => "Underline",
        Inline::Strong(_) => "Strong",
        Inline::Strikeout(_) => "Strikeout",
        Inline::Superscript(_) => "Superscript",
        Inline::Subscript(_) => "Subscript",
        Inline::SmallCaps(_) => "SmallCaps",
        Inline::Quoted(_) => "Quoted",
        Inline::Cite(_) => "Cite",
        Inline::Code(_) => "Code",
        Inline::Space(_) => "Space",
        Inline::SoftBreak(_) => "SoftBreak",
        Inline::LineBreak(_) => "LineBreak",
        Inline::Math(_) => "Math",
        Inline::RawInline(_) => "RawInline",
        Inline::Link(_) => "Link",
        Inline::Image(_) => "Image",
        Inline::Note(_) => "Note",
        Inline::Span(_) => "Span",
        Inline::Shortcode(_) => "Shortcode",
        Inline::NoteReference(_) => "NoteReference",
        Inline::Attr(_) => "Attr",
        Inline::Insert(_) => "Insert",
        Inline::Delete(_) => "Delete",
        Inline::Highlight(_) => "Highlight",
        Inline::EditComment(_) => "EditComment",
        Inline::Custom(_) => "Custom",
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tiling_auditor_tests {
    use super::*;
    use quarto_pandoc_types::{Block, Plain, Str};
    use quarto_source_map::source_info::{AnchorRole, By};
    use std::sync::Arc;

    const T: FileId = FileId(0);

    fn make_str(text: &str, si: SourceInfo) -> Inline {
        Inline::Str(Str {
            text: text.into(),
            source_info: si,
        })
    }

    fn para_ast(inlines: Vec<Inline>, source: &str) -> Pandoc {
        let para_si = SourceInfo::original(T, 0, source.len());
        quarto_pandoc_types::Pandoc {
            blocks: vec![Block::Plain(Plain {
                content: inlines,
                source_info: para_si,
            })],
            meta: Default::default(),
        }
    }

    // ── (a) Same-Invocation sibling pair ─────────────────────────────────────
    //
    // Two Str inlines both stamped Generated with the *same* Invocation anchor.
    // They both resolve to [0..5). After grouping they form one unit — the
    // auditor must NOT emit a SiblingOverlap finding.

    #[test]
    fn same_invocation_siblings_not_flagged_as_overlap() {
        let source = "hello";
        let token_si = SourceInfo::original(T, 0, source.len());

        let mut si_a = SourceInfo::generated(By::shortcode("test"));
        si_a.append_anchor(AnchorRole::Invocation, Arc::new(token_si.clone()));
        let mut si_b = SourceInfo::generated(By::shortcode("test"));
        si_b.append_anchor(AnchorRole::Invocation, Arc::new(token_si));

        let ast = para_ast(vec![make_str("A", si_a), make_str("B", si_b)], source);
        let findings = audit_source_range_tiling(&ast, source);

        assert!(
            !findings
                .iter()
                .any(|f| f.kind == TilingFindingKind::SiblingOverlap),
            "same-Invocation siblings must not produce SiblingOverlap; got: {:#?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>(),
        );
    }

    // ── (b) Whitespace-gap None-Concat ────────────────────────────────────────
    //
    // A Str whose SourceInfo is a non-contiguous Concat with a whitespace-only
    // inter-piece gap (the "Dr. Smith" abbreviation-coalesce case).
    // The auditor must emit exactly one WhitespaceGapConcat finding.

    #[test]
    fn whitespace_gap_concat_emits_finding() {
        // "Dr. Smith": "Dr."=[0..3), " "=[3..4), "Smith"=[4..9)
        // Pieces: [0..3) and [4..9) — gap [3..4) is a single space.
        let source = "Dr. Smith";
        let concat_si = SourceInfo::concat(vec![
            (SourceInfo::original(T, 0, 3), 3), // "Dr."
            (SourceInfo::original(T, 4, 9), 5), // "Smith"
        ]);
        // preimage_in → None (pieces not adjacent: 3 ≠ 4).
        assert!(
            concat_si.preimage_in(T).is_none(),
            "should not be contiguous"
        );

        let ast = para_ast(vec![make_str("Dr.\u{a0}Smith", concat_si)], source);
        let findings = audit_source_range_tiling(&ast, source);

        let wgc_count = findings
            .iter()
            .filter(|f| f.kind == TilingFindingKind::WhitespaceGapConcat)
            .count();
        assert_eq!(
            wgc_count,
            1,
            "expected exactly 1 WhitespaceGapConcat; got {wgc_count}. All findings: {:#?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>(),
        );
    }

    // ── (c) Non-resolvable-piece None-Concat ──────────────────────────────────
    //
    // A Str whose Concat has a piece in a *different* file — it cannot be
    // resolved to target. The auditor must emit a ScatteredConcat finding.

    #[test]
    fn non_resolvable_piece_concat_emits_scattered() {
        let source = "hello";
        const OTHER: FileId = FileId(1);
        // Piece 1 resolves to T; piece 2 is in OTHER → overall None.
        let concat_si = SourceInfo::concat(vec![
            (SourceInfo::original(T, 0, 3), 3),
            (SourceInfo::original(OTHER, 0, 2), 2), // different file — won't resolve to T
        ]);
        assert!(concat_si.preimage_in(T).is_none(), "should not resolve");

        let ast = para_ast(vec![make_str("he", concat_si)], source);
        let findings = audit_source_range_tiling(&ast, source);

        assert!(
            findings
                .iter()
                .any(|f| f.kind == TilingFindingKind::ScatteredConcat),
            "expected ScatteredConcat; got: {:#?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>(),
        );
    }

    // ── (d) Plain Original sibling pair with a gap ─────────────────────────────
    //
    // Two Str inlines with non-overlapping Original ranges: [0..5) and [6..11).
    // The auditor must NOT emit a SiblingOverlap finding.

    #[test]
    fn non_overlapping_original_siblings_pass() {
        let source = "Hello world";
        let si_a = SourceInfo::original(T, 0, 5); // "Hello"
        let si_b = SourceInfo::original(T, 6, 11); // "world"

        let ast = para_ast(
            vec![make_str("Hello", si_a), make_str("world", si_b)],
            source,
        );
        let findings = audit_source_range_tiling(&ast, source);

        assert!(
            !findings
                .iter()
                .any(|f| f.kind == TilingFindingKind::SiblingOverlap),
            "non-overlapping Original siblings must not produce SiblingOverlap; got: {:#?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>(),
        );
    }
}
