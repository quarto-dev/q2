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

use crate::pandoc::{Block, Pandoc};
use quarto_ast_reconcile::types::{BlockAlignment, ReconciliationPlan};
use quarto_ast_reconcile::{structural_eq_blocks, structural_eq_inlines};
use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue, ConfigValueKind};
use quarto_source_map::SourceInfo;
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

/// An entry in the coarsened plan: either copy verbatim or rewrite.
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
    // Step 1: Coarsen the reconciliation plan
    let coarsened = coarsen(original_ast, plan);

    // Step 2: Assemble the result string
    assemble(original_qmd, original_ast, new_ast, &coarsened)
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
    // For now, compute the full result string and diff at block-span granularity.
    // A future optimization could compute edits directly from the coarsened plan.
    let coarsened = coarsen(original_ast, plan);
    compute_edits_from_coarsened(original_qmd, original_ast, new_ast, &coarsened)
}

// =============================================================================
// Step 1: Coarsen the Reconciliation Plan
// =============================================================================

/// Convert a hierarchical ReconciliationPlan into a flat Vec<CoarsenedEntry>.
///
/// Conservative strategy (Phase 2): all RecurseIntoContainer become Rewrite.
/// Future optimization: for non-boundary containers (Div, Figure, NoteDefinitionFencedBlock),
/// could recursively coarsen inner blocks.
fn coarsen(original_ast: &Pandoc, plan: &ReconciliationPlan) -> Vec<CoarsenedEntry> {
    plan.block_alignments
        .iter()
        .enumerate()
        .map(|(result_idx, alignment)| match alignment {
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
            BlockAlignment::RecurseIntoContainer { .. } => {
                // Conservative: rewrite the entire block if any children changed
                CoarsenedEntry::Rewrite {
                    new_idx: result_idx,
                }
            }
        })
        .collect()
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
    // Try to use original gap for consecutive verbatim blocks
    if let (
        Some(CoarsenedEntry::Verbatim {
            orig_idx: prev_idx, ..
        }),
        CoarsenedEntry::Verbatim {
            orig_idx: curr_idx, ..
        },
    ) = (prev_entry, curr_entry)
    {
        if *curr_idx == *prev_idx + 1 {
            // Consecutive in original — use original gap
            let prev_span = block_source_span(&original_ast.blocks[*prev_idx]);
            let curr_span = block_source_span(&original_ast.blocks[*curr_idx]);
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

/// Extract the byte range (start..end) from a Block's source_info.
fn block_source_span(block: &Block) -> Range<usize> {
    let si = block_source_info(block);
    si.start_offset()..si.end_offset()
}

/// Extract the SourceInfo from a Block.
fn block_source_info(block: &Block) -> &SourceInfo {
    match block {
        Block::Paragraph(p) => &p.source_info,
        Block::Header(h) => &h.source_info,
        Block::CodeBlock(cb) => &cb.source_info,
        Block::BlockQuote(bq) => &bq.source_info,
        Block::BulletList(bl) => &bl.source_info,
        Block::OrderedList(ol) => &ol.source_info,
        Block::Div(d) => &d.source_info,
        Block::HorizontalRule(hr) => &hr.source_info,
        Block::Table(t) => &t.source_info,
        Block::RawBlock(rb) => &rb.source_info,
        Block::Plain(p) => &p.source_info,
        Block::LineBlock(lb) => &lb.source_info,
        Block::DefinitionList(dl) => &dl.source_info,
        Block::Figure(f) => &f.source_info,
        Block::BlockMetadata(m) => &m.source_info,
        Block::NoteDefinitionPara(nd) => &nd.source_info,
        Block::NoteDefinitionFencedBlock(nd) => &nd.source_info,
        Block::CaptionBlock(cb) => &cb.source_info,
        Block::Custom(cn) => &cn.source_info,
    }
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
