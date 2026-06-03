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

/// An entry in the coarsened plan.
///
/// Plan 7 adds `Transparent` and `Omit` to the original three variants
/// (`Verbatim`, `Rewrite`, `InlineSplice`).
#[derive(Debug)]
enum CoarsenedEntry {
    /// Copy this byte range verbatim from original_qmd.
    /// The text includes the block content + trailing \n.
    Verbatim {
        byte_range: Range<usize>,
        /// Index of this block in original_ast.blocks (for gap computation).
        /// `None` for entries that came from a `Transparent` recursion — those
        /// children aren't top-level blocks so they have no top-level index;
        /// `compute_separator`'s original-gap optimization falls back to the
        /// standard separator for them.
        orig_idx: Option<usize>,
    },
    /// Rewrite this block using the standard writer.
    ///
    /// `block_text` is pre-computed at coarsen time so the entry stays
    /// self-contained regardless of nesting depth. (Earlier the variant
    /// carried `new_idx: usize` and looked up the block at emit time,
    /// but that indexed `new_ast.blocks` top-level — wrong for entries
    /// produced inside a `Transparent` recursion, where the relevant
    /// block lives in a child slice. See
    /// `claude-notes/designs/incremental-writer-internals.md` for the
    /// "every variant is self-contained" contract.)
    Rewrite {
        /// Pre-computed block text — same bytes `write_block_to_string`
        /// would produce on the corresponding block.
        block_text: String,
    },
    /// Splice inlines within a block without rewriting the entire block.
    /// The block structure (prefix, suffix) is preserved from the original;
    /// only the inline content region is replaced with assembled new content.
    InlineSplice {
        /// Pre-computed block text: original block with inline content replaced.
        block_text: String,
        /// Index of this block in original_ast.blocks (for gap computation).
        /// Same `Option` semantics as `Verbatim::orig_idx`.
        orig_idx: Option<usize>,
    },
    /// Plan 7: a non-atomic `Generated` wrapper with empty anchors AND
    /// source-bearing children. The wrapper contributes no bytes; its
    /// children produce the output. Used for sectionize wrappers,
    /// footnotes container, appendix container — synthesizers whose
    /// container shell has no preimage but whose inner content does.
    Transparent { child_entries: Vec<CoarsenedEntry> },
    /// Plan 7: drop this node from output entirely. The next pipeline run
    /// regenerates it from baseline content. Used for atomic-kind
    /// `Generated` nodes with no Invocation anchor (filter constructions,
    /// title-block synthesis, tree-sitter postprocess space) and for
    /// no-preimage `Generated` containers replaced via React.
    Omit,
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
///
/// On success: `(new_qmd, warnings)`. The qmd preserves unchanged blocks
/// verbatim from `original_qmd`, rewrites changed blocks via the standard
/// writer, and soft-drops bad edits to non-editable regions (atomic
/// CustomNodes, atomic-kind Generated, no-preimage Generated containers).
/// Each soft-drop pushes a Q-3-42 / Q-3-43 warning into the returned vec;
/// the overall write still succeeds.
///
/// On failure: `Err(fatal_errors)` — genuine structural failure (UTF-8
/// error, inline-splice impossibility, etc.). Soft-drop substitutions
/// never reach this arm.
pub fn incremental_write(
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    plan: &ReconciliationPlan,
) -> Result<
    (String, Vec<quarto_error_reporting::DiagnosticMessage>),
    Vec<quarto_error_reporting::DiagnosticMessage>,
> {
    // The QMD reader internally pads input with '\n' when it doesn't end with
    // one, producing source spans relative to the padded input. We must use the
    // same padded string so that block source spans are valid byte indices.
    let mut padded_storage = None;
    let (qmd, did_pad) = ensure_trailing_newline(original_qmd, &mut padded_storage);

    // Step 1: Coarsen the reconciliation plan. Soft-drop warnings collect
    // into this sink; coarsen never returns Err for soft-drop cases.
    let mut warnings: Vec<quarto_error_reporting::DiagnosticMessage> = Vec::new();
    let coarsened = coarsen(qmd, original_ast, new_ast, plan, &mut warnings)?;

    // Step 2: Assemble the result string
    let mut result = assemble(qmd, original_ast, new_ast, &coarsened)?;

    // If we padded the input, strip the trailing '\n' from the result so that
    // the output preserves the original document's trailing-newline convention.
    if did_pad && result.ends_with('\n') {
        result.pop();
    }

    Ok((result, warnings))
}

/// Compute minimal text edits to transform `original_qmd` into the incremental write result.
///
/// Each TextEdit describes a byte range in `original_qmd` to replace and the replacement text.
/// Edits are sorted by range.start and non-overlapping.
///
/// Like [`incremental_write`], returns a tuple `(edits, warnings)` on
/// success; soft-drop warnings (Q-3-42 / Q-3-43) ride alongside.
pub fn compute_incremental_edits(
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    plan: &ReconciliationPlan,
) -> Result<
    (
        Vec<TextEdit>,
        Vec<quarto_error_reporting::DiagnosticMessage>,
    ),
    Vec<quarto_error_reporting::DiagnosticMessage>,
> {
    // Same trailing-newline normalization as incremental_write (see comment there).
    let mut padded_storage = None;
    let (qmd, did_pad) = ensure_trailing_newline(original_qmd, &mut padded_storage);

    let mut warnings: Vec<quarto_error_reporting::DiagnosticMessage> = Vec::new();
    let coarsened = coarsen(qmd, original_ast, new_ast, plan, &mut warnings)?;
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

    Ok((edits, warnings))
}

// =============================================================================
// Step 1: Coarsen the Reconciliation Plan
// =============================================================================

/// Convert a hierarchical ReconciliationPlan into a flat Vec<CoarsenedEntry>.
///
/// Phase 5 strategy: for RecurseIntoContainer blocks that are inline-content blocks
/// (Paragraph, Plain, Header) with inline plans that pass the safety check,
/// produce InlineSplice entries. All other RecurseIntoContainer become Rewrite.
///
/// Plan 7: soft-drop warnings push into `warnings`. Bad-edit cases
/// (atomic-CustomNode interior edit, atomic-Generated edit, no-preimage
/// Generated edit) substitute a safe alignment AND record a Q-3-42 /
/// Q-3-43 warning; coarsen never returns `Err` for these cases. `Err` is
/// reserved for genuine structural failures (UTF-8 errors, inline-splice
/// impossibility from assemble_inline_splice).
fn coarsen(
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    plan: &ReconciliationPlan,
    warnings: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> Result<Vec<CoarsenedEntry>, Vec<quarto_error_reporting::DiagnosticMessage>> {
    // The "target file" for editability decisions is the file
    // `original_qmd` was parsed from. Derived by descending past any
    // synthesized first blocks (title-block, sectionize wrappers,
    // footnotes / appendix containers) so we get the user's real
    // qmd FileId rather than the FileId(0) fallback by accident.
    // Closes Plan 7c Phase 8.
    let target_file_id = derive_target_file_id(&original_ast.blocks);

    coarsen_blocks(
        original_qmd,
        &original_ast.blocks,
        &new_ast.blocks,
        plan,
        target_file_id,
        warnings,
    )
}

/// Recurse into the children of a non-atomic Generated wrapper whose
/// own bytes are synthesized but whose children carry real source
/// preimage. Used by the RecurseIntoContainer arm of `coarsen_blocks`
/// when soft-dropping the wrapper would silently delete real user
/// content. The wrapper's index resolves the nested
/// `block_container_plans` entry; that plan describes alignment
/// between the wrapper's `orig` and `new` children.
///
/// Per the `Verbatim`/`InlineSplice::orig_idx` contract (see the
/// `CoarsenedEntry` doc comments), child entries returned to a
/// `Transparent` wrapper must carry `orig_idx: None` — their indices
/// are children-relative, not top-level, so `compute_separator`'s
/// "consecutive in original" optimization can't use them.
fn coarsen_children(
    original_qmd: &str,
    orig_children: &[Block],
    new_children: &[Block],
    child_plan: &ReconciliationPlan,
    target_file_id: quarto_source_map::FileId,
    warnings: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> Result<Vec<CoarsenedEntry>, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut entries = coarsen_blocks(
        original_qmd,
        orig_children,
        new_children,
        child_plan,
        target_file_id,
        warnings,
    )?;
    for entry in &mut entries {
        clear_orig_idx_for_transparent_child(entry);
    }
    Ok(entries)
}

/// Walk a `CoarsenedEntry` tree and set `orig_idx` to `None` on every
/// `Verbatim` / `InlineSplice`. Used when promoting entries into a
/// `Transparent` wrapper, where the indices no longer refer to
/// top-level positions.
fn clear_orig_idx_for_transparent_child(entry: &mut CoarsenedEntry) {
    match entry {
        CoarsenedEntry::Verbatim { orig_idx, .. } => *orig_idx = None,
        CoarsenedEntry::InlineSplice { orig_idx, .. } => *orig_idx = None,
        CoarsenedEntry::Transparent { child_entries } => {
            for child in child_entries {
                clear_orig_idx_for_transparent_child(child);
            }
        }
        CoarsenedEntry::Rewrite { .. } | CoarsenedEntry::Omit => {}
    }
}

/// Coarsen a block-alignment plan against the given original/new
/// block slices. Extracted from `coarsen` so the RecurseIntoContainer
/// path can recurse into a non-atomic Generated wrapper's children
/// using the nested `block_container_plans` plan.
fn coarsen_blocks(
    original_qmd: &str,
    original_blocks: &[Block],
    new_blocks: &[Block],
    plan: &ReconciliationPlan,
    target_file_id: quarto_source_map::FileId,
    warnings: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> Result<Vec<CoarsenedEntry>, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut entries = Vec::with_capacity(plan.block_alignments.len());

    for (result_idx, alignment) in plan.block_alignments.iter().enumerate() {
        let entry = match alignment {
            BlockAlignment::KeepBefore(orig_idx) => coarsen_keep_before_block(
                &original_blocks[*orig_idx],
                target_file_id,
                Some(*orig_idx),
            )?,
            BlockAlignment::UseAfter(after_idx) => {
                let new_block = &new_blocks[*after_idx];
                let new_si = new_block.source_info();

                let is_atomic_cn = matches!(new_block, Block::Custom(cn)
                    if is_atomic_custom_node(&cn.type_name));
                let atomic_generated_preimage = match new_si {
                    SourceInfo::Generated { by, .. } if by.is_atomic_kind() => {
                        new_si.preimage_in(target_file_id)
                    }
                    _ => None,
                };
                let no_preimage_generated = matches!(new_si, SourceInfo::Generated { .. })
                    && new_si.preimage_in(target_file_id).is_none();

                if let Some(range) = atomic_generated_preimage {
                    // User edited inside an atomic-kind Generated block
                    // (shortcode / filter / title-block / tree-sitter-
                    // postprocess). The reconciler split the edit into
                    // a deleted-original + new-block; the new block
                    // still carries the token's Invocation anchor, so
                    // its preimage IS the source-side knob. Emit the
                    // token bytes verbatim and soft-drop the edit;
                    // without this branch the let-user-win Rewrite
                    // below would write the resolved bytes (the edit
                    // applied to the generated content) back into qmd,
                    // poisoning the source. See
                    // `claude-notes/designs/incremental-writer-internals.md`
                    // for why this lives at the writer, not the gate.
                    warnings.push(diagnostic_q3_43_block(new_block));
                    CoarsenedEntry::Verbatim {
                        byte_range: range,
                        // No original block paired with this entry —
                        // the UseAfter alignment implicitly deleted
                        // the original; compute_separator's
                        // consecutive-in-original optimization can't
                        // use a top-level orig_idx here.
                        orig_idx: None,
                    }
                } else if !is_atomic_cn && no_preimage_generated {
                    // User replaced a synthesized-from-metadata container
                    // wholesale via React. No source position to anchor a
                    // Rewrite at; soft-drop with Q-3-43.
                    warnings.push(diagnostic_q3_43_block(new_block));
                    CoarsenedEntry::Omit
                } else {
                    // Let-user-win — including for atomic CustomNodes (the
                    // user replaced an include / CrossrefResolvedRef via a
                    // component menu; the qmd writer's CustomNode arm
                    // serializes the fresh plain_data).
                    CoarsenedEntry::Rewrite {
                        block_text: write_block_to_string(new_block)?,
                    }
                }
            }
            BlockAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => {
                let orig_block = &original_blocks[*before_idx];

                // Plan 7: if the original container is not editable inside,
                // soft-drop the inner edit. Substitutions:
                //   - atomic CustomNode with preimage → Verbatim wrapper bytes
                //   - non-atomic Generated wrapper with source-bearing
                //     children → recurse Transparent into the children (the
                //     wrapper's bytes are synthesized but the children carry
                //     real preimage; mirrors the unchanged-wrapper Transparent
                //     path in `coarsen_keep_before_block` at line ~459)
                //   - everything else (no-preimage Generated container with
                //     no source-bearing children, etc.) → Omit
                if !is_editable_inside_block(orig_block, target_file_id) {
                    // First: atomic CustomNode with preimage → keep the
                    // wrapper bytes verbatim (the user-side edit is lost,
                    // but the wrapper text survives).
                    if let Some(range) = orig_block.source_info().preimage_in(target_file_id) {
                        warnings.push(diagnostic_q3_43_block(orig_block));
                        entries.push(CoarsenedEntry::Verbatim {
                            byte_range: range,
                            orig_idx: Some(*before_idx),
                        });
                        continue;
                    }

                    // Second: non-atomic Generated wrapper (sectionize,
                    // footnotes-container, appendix-container, ...). If it
                    // has source-bearing children AND the reconciler built a
                    // container plan for this index, recurse coarsen on the
                    // children. The user's edit is *inside* the wrapper —
                    // soft-dropping the wrapper would silently delete real
                    // user content.
                    if let SourceInfo::Generated { by, .. } = orig_block.source_info()
                        && !by.is_atomic_kind()
                        && let (Some(orig_children), Some(new_children)) = (
                            block_block_children(orig_block),
                            block_block_children(&new_blocks[*after_idx]),
                        )
                        && orig_children
                            .iter()
                            .any(|c| c.source_info().preimage_in(target_file_id).is_some())
                        && let Some(child_plan) = plan.block_container_plans.get(&result_idx)
                    {
                        let child_entries = coarsen_children(
                            original_qmd,
                            orig_children,
                            new_children,
                            child_plan,
                            target_file_id,
                            warnings,
                        )?;
                        entries.push(CoarsenedEntry::Transparent { child_entries });
                        continue;
                    }

                    // Last resort: no preimage, no recursable children →
                    // soft-drop with Q-3-43.
                    warnings.push(diagnostic_q3_43_block(orig_block));
                    entries.push(CoarsenedEntry::Omit);
                    continue;
                }

                // Existing recurse logic: try inline-splice if the block has
                // an inline plan and is safe to splice; else Rewrite.
                if let Some(inline_plan) = plan.inline_plans.get(&result_idx) {
                    let new_block = &new_blocks[*after_idx];

                    if let (Some(orig_inlines), Some(new_inlines)) =
                        (block_inlines(orig_block), block_inlines(new_block))
                        && !orig_inlines.is_empty()
                        && is_inline_splice_safe(new_inlines, inline_plan)
                        && block_attrs_eq(orig_block, new_block)
                    {
                        // Safe to splice — assemble the patched block text.
                        // `assemble_inline_splice` returns `None` when the edge
                        // inlines have no usable preimage to anchor the
                        // prefix/suffix boundaries (Concat/Generated-led, etc.);
                        // in that case fall back to full re-serialization.
                        match assemble_inline_splice(
                            original_qmd,
                            orig_block,
                            orig_inlines,
                            new_inlines,
                            inline_plan,
                            target_file_id,
                            warnings,
                        )? {
                            Some(block_text) => CoarsenedEntry::InlineSplice {
                                block_text,
                                orig_idx: Some(*before_idx),
                            },
                            None => CoarsenedEntry::Rewrite {
                                block_text: write_block_to_string(new_block)?,
                            },
                        }
                    } else {
                        CoarsenedEntry::Rewrite {
                            block_text: write_block_to_string(new_block)?,
                        }
                    }
                } else {
                    // No inline plan — this is a block container (Div,
                    // BlockQuote, etc.). Fall back to Rewrite.
                    CoarsenedEntry::Rewrite {
                        block_text: write_block_to_string(&new_blocks[*after_idx])?,
                    }
                }
            }
        };
        entries.push(entry);
    }

    Ok(entries)
}

/// Classify a single `KeepBefore` block per Plan 7's cascade:
///
/// 1. **Verbatim** if `preimage_in(target)` returns `Some(range)` — covers
///    `Original`/`Substring`/contiguous-`Concat`/`Generated`-via-Invocation.
///    The atomic-kind shortcode case lands here too (its Invocation anchor
///    resolves to the token bytes).
/// 2. **Omit** if the source_info is `Generated` with `is_atomic_kind()`
///    and no Invocation anchor — filter constructions, title-block
///    synthesis, tree-sitter-postprocess space. Belt-and-suspenders
///    `debug_assert!` against shortcode-with-empty-from (Plan 6 stamper
///    invariant: every shortcode resolution must carry an Invocation).
/// 3. **Transparent** if the source_info is a non-atomic `Generated`
///    wrapper with source-bearing children (sectionize wrapper,
///    footnotes-container, appendix-container). Recurses into the
///    children.
/// 4. **Rewrite** catch-all — re-serializes the unchanged block through
///    the qmd writer. Lossy at the byte level but preserves content.
///    Handles cross-file Original chains (no Plan-8 wrapper yet),
///    Substring rooted outside target, gappy Concat.
///
/// `top_level_orig_idx` is `Some(idx)` for top-level blocks (used by
/// `compute_separator`'s original-gap optimization) and `None` for
/// children of a `Transparent` (whose indices don't reference
/// `original_ast.blocks` directly).
///
/// KeepBefore implies the original block at this position and the new
/// block at the same position are structurally equivalent (that's what
/// the reconciler's KeepBefore alignment *means*). So when we fall
/// through to Rewrite, serializing the original `block` yields the
/// same bytes as serializing the new one — by referential transparency
/// of `write_block_to_string`. We pick the original to avoid threading
/// the new slice down here.
fn coarsen_keep_before_block(
    block: &Block,
    target_file_id: quarto_source_map::FileId,
    top_level_orig_idx: Option<usize>,
) -> Result<CoarsenedEntry, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let si = block.source_info();

    if let Some(range) = si.preimage_in(target_file_id) {
        return Ok(CoarsenedEntry::Verbatim {
            byte_range: range,
            orig_idx: top_level_orig_idx,
        });
    }

    if let SourceInfo::Generated { by, .. } = si {
        if by.is_atomic_kind() {
            // Atomic-kind Generated with no Invocation anchor.
            debug_assert!(
                !by.is_kind("shortcode"),
                "Generated {{ by: shortcode, from: [] }} reached the writer — \
                 Plan 6's stamper must always attach an Invocation anchor for \
                 shortcode resolutions. \
                 Block: {:?}",
                block,
            );
            return Ok(CoarsenedEntry::Omit);
        }

        // Non-atomic Generated wrapper. If it has source-bearing children,
        // recurse Transparent. Else fall through to Rewrite.
        if let Some(children) = block_block_children(block)
            && children
                .iter()
                .any(|c| c.source_info().preimage_in(target_file_id).is_some())
        {
            let child_entries = children
                .iter()
                .map(|child| {
                    // Children of a Transparent wrapper aren't top-level
                    // blocks — pass orig_idx=None so compute_separator
                    // doesn't try the original-gap optimization on them.
                    coarsen_keep_before_block(child, target_file_id, None)
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(CoarsenedEntry::Transparent { child_entries });
        }
    }

    // Catch-all: cross-file Original, Substring rooted outside target,
    // gappy Concat, Generated wrapper without source-bearing children.
    Ok(CoarsenedEntry::Rewrite {
        block_text: write_block_to_string(block)?,
    })
}

/// Return the inner block children of a block, if the block is a
/// recognized block container.
///
/// Today's Plan-6 synthesizers produce `Div`-shaped wrappers (sectionize,
/// footnotes-container, appendix-container). Other block containers
/// (BlockQuote, Figure, NoteDefinitionFencedBlock) round out the set so
/// the Transparent cascade applies uniformly when those carry Generated
/// source_info. List-shaped containers (BulletList, OrderedList,
/// DefinitionList) return `None` — their `content` is `Vec<Blocks>`
/// (lists of lists), which isn't the Transparent shape.
fn block_block_children(block: &Block) -> Option<&[Block]> {
    match block {
        Block::Div(d) => Some(&d.content),
        Block::BlockQuote(b) => Some(&b.content),
        Block::Figure(f) => Some(&f.content),
        Block::NoteDefinitionFencedBlock(n) => Some(&n.content),
        _ => None,
    }
}

// =============================================================================
// Transparent wrappers
// =============================================================================
//
// A *transparent wrapper* is a block that's structurally part of the
// AST but has no source bytes of its own — the user's actual content
// lives in its children. Sectionize Divs, footnotes containers, and
// appendix containers (from `pampa::pandoc::sugar::SectionizeTransform`
// and friends) are the canonical examples. A Lua filter that wraps
// user content in a Div is another: the wrapper has `Generated`
// source_info, but the children preserve their original positions.
//
// Code that asks "where do the user's source bytes live?" must
// **descend through transparent wrappers** rather than reading
// `blocks[0]` directly. The `first_in_user_tree` walker below is the
// reference implementation; `derive_target_file_id` /
// `first_target_anchored_start_in` are thin specializations.
//
// See `claude-notes/designs/transparent-wrappers.md` for the full
// contract and the rationale (why the predicate is structural rather
// than opt-in: filter authors don't have to register anything — the
// AST shape they emit *is* the declaration).

/// Walk `blocks` depth-first, applying `extract` to each block. On a
/// `Some` result, stop and return it. On `None`, descend through
/// `block_block_children` and try again. This is how we see through
/// transparent wrappers: a wrapper has no source position of its
/// own, so `extract` returns `None` for it; the walker then looks
/// inside.
///
/// Used by `derive_target_file_id` and `first_target_anchored_start_in`,
/// and intended as the building block for any future code that needs
/// to find "the first user block matching X" without having to
/// re-derive the descent.
fn first_in_user_tree<T>(blocks: &[Block], extract: &impl Fn(&Block) -> Option<T>) -> Option<T> {
    for block in blocks {
        if let Some(v) = extract(block) {
            return Some(v);
        }
        if let Some(children) = block_block_children(block)
            && let Some(v) = first_in_user_tree(children, extract)
        {
            return Some(v);
        }
    }
    None
}

/// Returns `true` iff `block` is a transparent wrapper with respect
/// to `target_file_id`:
///
/// 1. its `SourceInfo` is `Generated` with no `Invocation` anchor (so
///    it has no source token of its own), AND
/// 2. it has block-children (`block_block_children` recognises it as
///    a container — Div, BlockQuote, Figure, NoteDefinitionFencedBlock), AND
/// 3. at least one descendant has real `preimage_in(target_file_id)`
///    (so there's actual user content under it).
///
/// Condition (3) is what makes this *structural* rather than opt-in:
/// a Lua filter that wraps existing user content in a Div produces a
/// Generated wrapper whose children carry their original preimage —
/// transparent. A filter that constructs a fresh Div from metadata
/// has no source-bearing children — atomic. Authors don't have to
/// declare anything; the AST shape declares it for them.
///
/// Available to callers that need to make an explicit decision
/// (e.g. a future Q-3-44 diagnostic that hints "your filter walked
/// into the sectionize wrapper"). Routine source-position lookups
/// should use `first_in_user_tree` directly.
#[allow(dead_code)]
fn is_transparent_wrapper(block: &Block, target_file_id: quarto_source_map::FileId) -> bool {
    if !matches!(block.source_info(), SourceInfo::Generated { .. }) {
        return false;
    }
    if block.source_info().invocation_anchor().is_some() {
        return false;
    }
    let Some(children) = block_block_children(block) else {
        return false;
    };
    first_in_user_tree(children, &|b| b.source_info().preimage_in(target_file_id)).is_some()
}

/// Derive the "target file" — the file that `original_qmd` was parsed
/// from, used for editability and preimage checks throughout the
/// writer.
///
/// Descends through transparent wrappers via `first_in_user_tree`,
/// so a synthesized first block (title-block, sectionize wrapper,
/// footnotes / appendix container) is skipped and the user's real
/// qmd `FileId` is returned. Falls back to `FileId(0)` only for the
/// genuinely-empty document.
///
/// Closes Plan 7c Phase 8.
fn derive_target_file_id(blocks: &[Block]) -> quarto_source_map::FileId {
    first_in_user_tree(blocks, &|b| b.source_info().root_file_id())
        .unwrap_or(quarto_source_map::FileId(0))
}

/// Find the start offset (in `target` bytes) of the first block in
/// `blocks` whose `source_info().preimage_in(target)` is `Some`,
/// descending through transparent wrappers. Used by
/// `emit_metadata_prefix` to locate the boundary between the YAML
/// frontmatter region and the first source-anchored user block.
fn first_target_anchored_start_in(
    blocks: &[Block],
    target: quarto_source_map::FileId,
) -> Option<usize> {
    first_in_user_tree(blocks, &|b| {
        b.source_info().preimage_in(target).map(|r| r.start)
    })
}

// =============================================================================
// Soft-drop diagnostic builders (Plan 7)
// =============================================================================

/// Build a `Q-3-42` warning for an inline-level edit that targeted
/// atomic-Generated content (typically a shortcode resolution). The
/// source location is the inline's `Invocation` anchor when available
/// (the token bytes), falling back to the inline's own source_info.
fn diagnostic_q3_42_inline(inline: &Inline) -> quarto_error_reporting::DiagnosticMessage {
    let location = inline
        .source_info()
        .invocation_anchor()
        .map(|arc| arc.as_ref().clone())
        .unwrap_or_else(|| inline.source_info().clone());

    quarto_error_reporting::DiagnosticMessageBuilder::warning("Shortcode edit dropped")
        .with_code("Q-3-42")
        .with_location(location)
        .problem(
            "An edit to shortcode-resolved (or other atomic-Generated) \
             content was reverted.",
        )
        .add_hint(
            "The resolved text is read-only; edit the invocation token \
             (e.g. `{{< meta foo >}}`) in source instead.",
        )
        .build()
}

/// Build a `Q-3-43` warning for a block-level edit dropped because the
/// container is not editable inside.
///
/// Three emission paths share this builder (per Plan 7
/// §"Diagnostic codes"):
/// - Block RecurseIntoContainer on an atomic CustomNode — wrapper's
///   source_info is `Original` pointing at the token bytes;
///   `with_location` highlights the include / crossref in Monaco.
/// - Block RecurseIntoContainer on a no-preimage Generated container —
///   the wrapper's source_info is `Generated` with no Invocation; the
///   diagnostic lands without a Monaco squiggle and surfaces via the
///   diagnostics banner.
/// - Block UseAfter on a no-preimage Generated container — same as
///   the previous case.
fn diagnostic_q3_43_block(block: &Block) -> quarto_error_reporting::DiagnosticMessage {
    quarto_error_reporting::DiagnosticMessageBuilder::warning("Generated content edit dropped")
        .with_code("Q-3-43")
        .with_location(block.source_info().clone())
        .problem("An edit to pipeline-generated content was reverted.")
        .add_hint(
            "This content has no editable source position in this file; \
             edit its upstream definition (an include, a metadata key, \
             or other source) instead.",
        )
        .build()
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

    // 2b. Walk coarsened entries and assemble blocks with separators.
    // Transparent entries recursively re-enter this loop on their children;
    // Omit entries contribute nothing.
    let mut prev_entry: Option<&CoarsenedEntry> = None;
    let mut prev_block_text: Option<String> = None;
    emit_entries(
        &mut result,
        original_qmd,
        original_ast,
        new_ast,
        coarsened,
        &mut prev_entry,
        &mut prev_block_text,
    )?;

    Ok(result)
}

/// Recursive helper that walks a slice of `CoarsenedEntry` and emits each
/// one's bytes into `result`, threading `prev_entry` / `prev_block_text`
/// across siblings.
///
/// `Transparent` re-enters this loop with its children, sharing the same
/// `prev_entry` / `prev_block_text` state so separators compose across the
/// wrapper boundary as if the wrapper weren't there. `Omit` is a no-op —
/// no bytes, no separator update; the next sibling's separator is computed
/// against the entry before the `Omit`.
fn emit_entries<'e>(
    result: &mut String,
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    entries: &'e [CoarsenedEntry],
    prev_entry: &mut Option<&'e CoarsenedEntry>,
    prev_block_text: &mut Option<String>,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {
    for entry in entries {
        match entry {
            CoarsenedEntry::Omit => {
                // Contributes nothing; leave prev_entry / prev_block_text alone
                // so the next sibling's separator is computed against the
                // entry before this Omit.
                continue;
            }
            CoarsenedEntry::Transparent { child_entries } => {
                // Recurse into children with shared prev_* state so separator
                // semantics compose through the wrapper.
                emit_entries(
                    result,
                    original_qmd,
                    original_ast,
                    new_ast,
                    child_entries,
                    prev_entry,
                    prev_block_text,
                )?;
                continue;
            }
            _ => {}
        }

        // Separator between blocks (only if there's a previous emitting entry).
        // The metadata prefix already includes the gap to the first block,
        // so we must NOT add an extra separator after it.
        if prev_entry.is_some() {
            let separator = compute_separator(
                original_qmd,
                original_ast,
                *prev_entry,
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
            CoarsenedEntry::Rewrite { block_text } => block_text.clone(),
            CoarsenedEntry::InlineSplice { block_text, .. } => block_text.clone(),
            // Transparent + Omit were handled above; coarsen never emits
            // any other variant.
            CoarsenedEntry::Transparent { .. } | CoarsenedEntry::Omit => unreachable!(),
        };

        result.push_str(&block_text);
        *prev_block_text = Some(block_text);
        *prev_entry = Some(entry);
    }
    Ok(())
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
    // Determine where the metadata region ends by finding the start
    // offset of the first source-anchored ORIGINAL block in the target
    // file. We must NOT use the first coarsened entry's offset — when
    // blocks are removed from the beginning, the first coarsened block
    // may reference a later original block whose start > 0, falsely
    // triggering the metadata prefix logic.
    //
    // We must also NOT use `original_ast.blocks[0]`'s offset directly:
    // when the post-pipeline AST wraps user content in a synthesized
    // top-level container (sectionize Div, title-block, footnotes /
    // appendix wrappers), `blocks[0].start_offset()` is 0 (Generated,
    // no preimage), which would falsely conclude "no metadata region"
    // and silently delete the YAML frontmatter. Descend through
    // `block_block_children` of any such wrapper to find the first
    // block with real preimage in the target file.
    let target_file_id = derive_target_file_id(&original_ast.blocks);
    let first_block_start = first_target_anchored_start_in(&original_ast.blocks, target_file_id);

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
    // Try to use original gap for consecutive blocks that preserve original
    // positions. Transparent/Omit entries don't carry a top-level orig_idx —
    // they fall through to the standard separator.
    let prev_orig_idx: Option<usize> = match prev_entry {
        Some(CoarsenedEntry::Verbatim { orig_idx, .. }) => *orig_idx,
        Some(CoarsenedEntry::InlineSplice { orig_idx, .. }) => *orig_idx,
        _ => None,
    };
    let curr_orig_idx: Option<usize> = match curr_entry {
        CoarsenedEntry::Verbatim { orig_idx, .. } => *orig_idx,
        CoarsenedEntry::InlineSplice { orig_idx, .. } => *orig_idx,
        _ => None,
    };
    if let (Some(prev_idx), Some(curr_idx)) = (prev_orig_idx, curr_orig_idx)
        && curr_idx == prev_idx + 1
    {
        // Consecutive in original — use original gap
        let prev_span = block_source_span(&original_ast.blocks[prev_idx]);
        let curr_span = block_source_span(&original_ast.blocks[curr_idx]);
        return &original_qmd[prev_span.end..curr_span.start];
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
    target_file_id: quarto_source_map::FileId,
    warnings: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> Result<Option<String>, Vec<quarto_error_reporting::DiagnosticMessage>> {
    // Boundaries must come from `preimage_in`, NOT `start_offset()`/`end_offset()`.
    // A `Concat`/`Generated`-led inline reports the sentinel `0` for
    // `start_offset()` — e.g. `Str "Table:"` parses as a contiguous
    // `Concat[Original "Table" ++ Original ":"]` — which made the prefix slice
    // `original_qmd[block.start .. 0]` reverse and panic (Plan 7g Phase 8).
    // `preimage_in` resolves the real (hull) byte range. When any boundary is
    // unavailable (non-contiguous `Concat`, anchorless `Generated`), we cannot
    // splice safely: return `None` so the caller re-serializes the whole block.
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

    // Guard ordering so a stray provenance can never produce a reversed slice:
    // block ⊇ [first.start, last.end] and first.start ≤ last.end.
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
        warnings,
    )?;

    Ok(Some(format!("{}{}{}", prefix, inline_content, suffix)))
}

/// Assemble the inline content from a reconciliation plan.
///
/// Walks the inline alignments and produces the result text by:
/// - KeepBefore: copying the original inline's bytes verbatim
/// - UseAfter: writing the new inline to a string
/// - RecurseIntoContainer: preserving delimiters, recursing into children
///
/// Plan 7: inline-level soft-drop substitutes `KeepBefore` for `UseAfter`
/// / `RecurseIntoContainer` alignments that target a non-editable original
/// inline (atomic-CustomNode, atomic-kind Generated, no-preimage
/// Generated). Each substitution pushes a `Q-3-42` warning. The
/// substitution uses the *new-side* index as the positional proxy for the
/// "original inline at the same position" — exact for in-place retypings
/// (the common shortcode-edit case), approximate for arbitrary
/// insertions/deletions.
///
/// Plan 7 also adds multi-inline dedupe: consecutive `KeepBefore` entries
/// whose original inlines' `Invocation` anchors are `PartialEq`-equal
/// emit a single combined byte range, so a multi-inline shortcode
/// resolution (`{{< meta footer >}}` → `[Strong[Str], Space, Str]`)
/// emits the shortcode token bytes once.
fn assemble_inline_content(
    original_qmd: &str,
    orig_inlines: &[Inline],
    new_inlines: &[Inline],
    plan: &InlineReconciliationPlan,
    target_file_id: quarto_source_map::FileId,
    warnings: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> Result<String, Vec<quarto_error_reporting::DiagnosticMessage>> {
    // Phase 1: apply soft-drop substitutions. Walk alignments and rewrite
    // UseAfter/RecurseIntoContainer that target non-editable original
    // inlines into KeepBefore(original-position).
    let mut effective: Vec<InlineAlignment> = Vec::with_capacity(plan.inline_alignments.len());
    for (result_idx, alignment) in plan.inline_alignments.iter().enumerate() {
        match alignment {
            InlineAlignment::UseAfter(_) => {
                // Use result_idx (positional proxy) to find the
                // corresponding original inline.
                if let Some(orig) = orig_inlines.get(result_idx)
                    && !is_editable_inside_inline(orig, target_file_id)
                {
                    warnings.push(diagnostic_q3_42_inline(orig));
                    effective.push(InlineAlignment::KeepBefore(result_idx));
                    continue;
                }
                effective.push(alignment.clone());
            }
            InlineAlignment::RecurseIntoContainer { before_idx, .. } => {
                let orig = &orig_inlines[*before_idx];
                if !is_editable_inside_inline(orig, target_file_id) {
                    warnings.push(diagnostic_q3_42_inline(orig));
                    effective.push(InlineAlignment::KeepBefore(*before_idx));
                    continue;
                }
                effective.push(alignment.clone());
            }
            InlineAlignment::KeepBefore(_) => effective.push(alignment.clone()),
        }
    }

    // Phase 2: emit, with multi-inline dedupe for consecutive
    // KeepBefore entries whose Invocation anchors are PartialEq-equal.
    let mut result = String::new();
    let mut i = 0;
    while i < effective.len() {
        match &effective[i] {
            InlineAlignment::KeepBefore(orig_idx) => {
                let first_si = orig_inlines[*orig_idx].source_info();
                let first_invocation = first_si.invocation_anchor().cloned();

                // Try to extend the run: gather all consecutive KeepBefore
                // entries whose invocation_anchor() is PartialEq-equal to
                // first_invocation. Only consider runs of length >= 2 for
                // dedupe; a single inline emits via the normal path.
                let mut j = i + 1;
                if first_invocation.is_some() {
                    while j < effective.len() {
                        let InlineAlignment::KeepBefore(next_orig_idx) = &effective[j] else {
                            break;
                        };
                        let next_invocation = orig_inlines[*next_orig_idx]
                            .source_info()
                            .invocation_anchor()
                            .cloned();
                        if next_invocation != first_invocation {
                            break;
                        }
                        j += 1;
                    }
                }

                if j > i + 1 {
                    // Dedupe: the whole group shares one Invocation anchor.
                    // Emit the Invocation source's preimage bytes once,
                    // not the individual inlines' ranges. Use the anchor
                    // source_info's preimage in the target file when
                    // available; fall back to the first inline's range.
                    let anchor_arc = first_invocation.unwrap();
                    if let Some(range) = anchor_arc.preimage_in(target_file_id) {
                        result.push_str(&original_qmd[range]);
                    } else {
                        // Fall back: emit each inline's bytes individually.
                        // Shouldn't happen — KeepBefore implies preimage_in
                        // succeeded for the surrounding block. Keep
                        // structurally safe behavior just in case.
                        for k in i..j {
                            let InlineAlignment::KeepBefore(idx) = &effective[k] else {
                                unreachable!()
                            };
                            let span = inline_source_span(&orig_inlines[*idx]);
                            result.push_str(&original_qmd[span]);
                        }
                    }
                    i = j;
                    continue;
                }

                // Singleton KeepBefore — emit the inline's preimage in
                // the target file when available (covers Generated inlines
                // whose Invocation anchor resolves into target), falling
                // back to the inline's literal source span for Original
                // inlines (the common case; identical bytes either way).
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
                    plan.inline_container_plans.get(&i),
                    target_file_id,
                    warnings,
                )?;
                result.push_str(&text);
            }
        }
        i += 1;
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
    target_file_id: quarto_source_map::FileId,
    warnings: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
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
        warnings,
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

#[cfg(test)]
mod coarsen_plan7_tests {
    //! Plan 7: coarsen behavior under the new soft-drop + cascade rules.
    //!
    //! These tests construct `Pandoc` + `ReconciliationPlan` fixtures by
    //! hand to exercise the new code paths directly. The existing
    //! `incremental_writer_tests.rs` integration tests cover the
    //! end-to-end (parse → reconcile → write) flow; these tests pin
    //! coarsen's specific classification + soft-drop behavior.

    use super::*;
    use quarto_ast_reconcile::types::{
        BlockAlignment, InlineAlignment, InlineReconciliationPlan, ReconciliationPlan,
    };
    use quarto_pandoc_types::{Block, CustomNode, Div, Inline, Paragraph, Str, attr::empty_attr};
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

    fn para(content: Vec<Inline>, si: SourceInfo) -> Block {
        Block::Paragraph(Paragraph {
            content,
            source_info: si,
        })
    }

    // -------------------------------------------------------------------------
    // KeepBefore cascade
    // -------------------------------------------------------------------------

    #[test]
    fn keep_before_with_original_in_target_emits_verbatim() {
        let block = para(vec![], SourceInfo::original(TARGET, 10, 25));
        let ast = quarto_pandoc_types::Pandoc {
            blocks: vec![block],
            meta: ConfigValue::default(),
        };
        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::KeepBefore(0)],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let qmd = "0123456789012345678901234567890";
        let entries = coarsen(qmd, &ast, &ast, &plan, &mut warnings).unwrap();

        assert_eq!(entries.len(), 1);
        match &entries[0] {
            CoarsenedEntry::Verbatim { byte_range, .. } => {
                assert_eq!(byte_range, &(10..25));
            }
            other => panic!("expected Verbatim, got {:?}", other),
        }
        assert!(warnings.is_empty());
    }

    #[test]
    fn keep_before_with_atomic_kind_generated_no_anchor_emits_omit() {
        // Filter construction: Generated { by: filter, from: [] }.
        // Atomic-kind, no Invocation → Omit (next pipeline run
        // regenerates the decoration).
        let block = para(vec![], SourceInfo::generated(By::filter("upper.lua", 14)));
        let ast = quarto_pandoc_types::Pandoc {
            blocks: vec![block],
            meta: ConfigValue::default(),
        };
        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::KeepBefore(0)],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let entries = coarsen("", &ast, &ast, &plan, &mut warnings).unwrap();

        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], CoarsenedEntry::Omit));
        // KeepBefore branch doesn't emit warnings.
        assert!(warnings.is_empty());
    }

    #[test]
    fn keep_before_with_atomic_kind_generated_with_invocation_emits_verbatim() {
        // Shortcode resolution: atomic-kind, Invocation in target → Verbatim.
        let token = SourceInfo::original(TARGET, 100, 120);
        let mut gen_info = SourceInfo::generated(By::shortcode("meta"));
        gen_info.append_anchor(AnchorRole::Invocation, Arc::new(token));
        let block = para(vec![], gen_info);
        let ast = quarto_pandoc_types::Pandoc {
            blocks: vec![block],
            meta: ConfigValue::default(),
        };
        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::KeepBefore(0)],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let qmd = "0".repeat(200);
        let entries = coarsen(&qmd, &ast, &ast, &plan, &mut warnings).unwrap();

        assert_eq!(entries.len(), 1);
        match &entries[0] {
            CoarsenedEntry::Verbatim { byte_range, .. } => {
                assert_eq!(byte_range, &(100..120));
            }
            other => panic!("expected Verbatim, got {:?}", other),
        }
    }

    #[test]
    fn keep_before_with_nonatomic_generated_wrapper_emits_transparent() {
        // Sectionize wrapper: Div with Generated { by: sectionize, from: [] }
        // and source-bearing children (one Para in target).
        let child = para(
            vec![make_str("hi", SourceInfo::original(TARGET, 10, 12))],
            SourceInfo::original(TARGET, 10, 12),
        );
        let div = Block::Div(Div {
            attr: empty_attr(),
            content: vec![child],
            source_info: SourceInfo::generated(By::sectionize()),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        let ast = quarto_pandoc_types::Pandoc {
            blocks: vec![div],
            meta: ConfigValue::default(),
        };
        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::KeepBefore(0)],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let qmd = "0".repeat(30);
        let entries = coarsen(&qmd, &ast, &ast, &plan, &mut warnings).unwrap();

        assert_eq!(entries.len(), 1);
        match &entries[0] {
            CoarsenedEntry::Transparent { child_entries } => {
                assert_eq!(child_entries.len(), 1);
                match &child_entries[0] {
                    CoarsenedEntry::Verbatim {
                        byte_range,
                        orig_idx,
                    } => {
                        assert_eq!(byte_range, &(10..12));
                        // Children of Transparent get None for orig_idx.
                        assert_eq!(orig_idx, &None);
                    }
                    other => panic!("expected Verbatim child, got {:?}", other),
                }
            }
            other => panic!("expected Transparent, got {:?}", other),
        }
    }

    #[test]
    fn keep_before_cross_file_original_falls_back_to_rewrite() {
        // Block whose source_info points at a different file (no preimage
        // in target) AND isn't Generated → Rewrite (catch-all).
        let block = para(vec![], SourceInfo::original(OTHER, 0, 10));
        let ast = quarto_pandoc_types::Pandoc {
            blocks: vec![block],
            meta: ConfigValue::default(),
        };
        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::KeepBefore(0)],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        // Note: target_file_id is derived from the first block's
        // root_file_id, which for this AST is OTHER (FileId 1) — so
        // preimage_in(OTHER) succeeds. To exercise the catch-all path
        // we need a block whose source_info doesn't resolve in *its
        // own* root file_id. Use a separate AST whose first-block
        // file-id sets target = TARGET, but this block points at OTHER.
        let target_setter = para(vec![], SourceInfo::original(TARGET, 0, 5));
        let block_cross = para(vec![], SourceInfo::original(OTHER, 0, 10));
        let ast2 = quarto_pandoc_types::Pandoc {
            blocks: vec![target_setter, block_cross],
            meta: ConfigValue::default(),
        };
        let plan2 = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::KeepBefore(0), BlockAlignment::KeepBefore(1)],
            ..Default::default()
        };
        let qmd = "0".repeat(30);
        let entries = coarsen(&qmd, &ast2, &ast2, &plan2, &mut warnings).unwrap();

        assert_eq!(entries.len(), 2);
        // First entry resolves in target via preimage_in.
        assert!(matches!(entries[0], CoarsenedEntry::Verbatim { .. }));
        // Second entry doesn't resolve in target → Rewrite catch-all.
        assert!(matches!(entries[1], CoarsenedEntry::Rewrite { .. }));
        assert!(warnings.is_empty());
        // Silence unused: plan was for the single-block AST scenario above.
        let _ = (ast, plan);
    }

    // -------------------------------------------------------------------------
    // UseAfter soft-drop / let-user-win
    // -------------------------------------------------------------------------

    #[test]
    fn use_after_on_atomic_custom_node_is_let_user_win_rewrite() {
        // User replaced a CrossrefResolvedRef wholesale via a component
        // menu. The new-side block IS the atomic CustomNode; we let the
        // user win and Rewrite (no warning).
        let new_cn = CustomNode::new(
            "CrossrefResolvedRef",
            empty_attr(),
            SourceInfo::original(TARGET, 0, 10),
        );
        let new_ast = quarto_pandoc_types::Pandoc {
            blocks: vec![Block::Custom(new_cn)],
            meta: ConfigValue::default(),
        };
        let orig_block = para(vec![], SourceInfo::original(TARGET, 0, 0));
        let original_ast = quarto_pandoc_types::Pandoc {
            blocks: vec![orig_block],
            meta: ConfigValue::default(),
        };
        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::UseAfter(0)],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let qmd = "0".repeat(20);
        let entries = coarsen(&qmd, &original_ast, &new_ast, &plan, &mut warnings).unwrap();

        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], CoarsenedEntry::Rewrite { .. }));
        assert!(
            warnings.is_empty(),
            "let-user-win on atomic CustomNode must not emit a warning"
        );
    }

    #[test]
    fn use_after_on_no_preimage_generated_soft_drops_to_omit() {
        // User replaced a synthesized-from-metadata container wholesale.
        // The new-side block is Generated with no Invocation anchor
        // → no source position to anchor a Rewrite → Omit + Q-3-43.
        let new_block = Block::Div(Div {
            attr: empty_attr(),
            content: vec![],
            source_info: SourceInfo::generated(By::appendix()),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        let new_ast = quarto_pandoc_types::Pandoc {
            blocks: vec![new_block],
            meta: ConfigValue::default(),
        };
        let orig_block = para(vec![], SourceInfo::original(TARGET, 0, 0));
        let original_ast = quarto_pandoc_types::Pandoc {
            blocks: vec![orig_block],
            meta: ConfigValue::default(),
        };
        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::UseAfter(0)],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let qmd = "0".repeat(20);
        let entries = coarsen(&qmd, &original_ast, &new_ast, &plan, &mut warnings).unwrap();

        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], CoarsenedEntry::Omit));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code.as_deref(), Some("Q-3-43"));
    }

    // -------------------------------------------------------------------------
    // RecurseIntoContainer soft-drop on non-editable original block
    // -------------------------------------------------------------------------

    #[test]
    fn recurse_into_atomic_custom_node_soft_drops_to_verbatim() {
        // User typed inside a CrossrefResolvedRef. Substitute Verbatim
        // (wrapper's preimage bytes) + Q-3-43.
        let orig_cn = CustomNode::new(
            "CrossrefResolvedRef",
            empty_attr(),
            SourceInfo::original(TARGET, 5, 25),
        );
        let new_cn = CustomNode::new(
            "CrossrefResolvedRef",
            empty_attr(),
            SourceInfo::original(TARGET, 5, 25),
        );
        let original_ast = quarto_pandoc_types::Pandoc {
            blocks: vec![Block::Custom(orig_cn)],
            meta: ConfigValue::default(),
        };
        let new_ast = quarto_pandoc_types::Pandoc {
            blocks: vec![Block::Custom(new_cn)],
            meta: ConfigValue::default(),
        };
        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::RecurseIntoContainer {
                before_idx: 0,
                after_idx: 0,
            }],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let qmd = "0".repeat(30);
        let entries = coarsen(&qmd, &original_ast, &new_ast, &plan, &mut warnings).unwrap();

        assert_eq!(entries.len(), 1);
        match &entries[0] {
            CoarsenedEntry::Verbatim { byte_range, .. } => {
                assert_eq!(byte_range, &(5..25));
            }
            other => panic!("expected Verbatim, got {:?}", other),
        }
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code.as_deref(), Some("Q-3-43"));
    }

    #[test]
    fn recurse_into_no_preimage_generated_soft_drops_to_omit() {
        // User typed inside a synthesized appendix container (Generated
        // with no Invocation anchor, no preimage in target).
        let orig_div = Block::Div(Div {
            attr: empty_attr(),
            content: vec![para(vec![], SourceInfo::original(TARGET, 0, 5))],
            source_info: SourceInfo::generated(By::appendix()),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        let new_div = Block::Div(Div {
            attr: empty_attr(),
            content: vec![para(vec![], SourceInfo::original(TARGET, 0, 5))],
            source_info: SourceInfo::generated(By::appendix()),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });
        // Force target_file_id to TARGET by giving the AST another block
        // whose source_info is Original in TARGET.
        let target_setter = para(vec![], SourceInfo::original(TARGET, 0, 5));
        let original_ast = quarto_pandoc_types::Pandoc {
            blocks: vec![target_setter.clone(), orig_div],
            meta: ConfigValue::default(),
        };
        let new_ast = quarto_pandoc_types::Pandoc {
            blocks: vec![target_setter, new_div],
            meta: ConfigValue::default(),
        };
        let plan = ReconciliationPlan {
            block_alignments: vec![
                BlockAlignment::KeepBefore(0),
                BlockAlignment::RecurseIntoContainer {
                    before_idx: 1,
                    after_idx: 1,
                },
            ],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let qmd = "0".repeat(30);
        let entries = coarsen(&qmd, &original_ast, &new_ast, &plan, &mut warnings).unwrap();

        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0], CoarsenedEntry::Verbatim { .. }));
        assert!(matches!(entries[1], CoarsenedEntry::Omit));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code.as_deref(), Some("Q-3-43"));
    }

    // -------------------------------------------------------------------------
    // Inline-level multi-inline dedupe + soft-drop
    // -------------------------------------------------------------------------

    fn shortcode_inline(text: &str, token_si: SourceInfo) -> Inline {
        let mut gen_info = SourceInfo::generated(By::shortcode("meta"));
        gen_info.append_anchor(AnchorRole::Invocation, Arc::new(token_si));
        make_str(text, gen_info)
    }

    #[test]
    fn multi_inline_dedupe_emits_token_once_when_invocation_shared() {
        // Three inlines sharing the same Invocation anchor (a multi-inline
        // shortcode resolution). The original qmd has the shortcode token
        // at bytes 0..18. Expected output: those 18 bytes once.
        let qmd = "{{< meta footer >}}";
        assert_eq!(qmd.len(), 19);
        let token_si = SourceInfo::original(TARGET, 0, 19);

        let orig_inlines = vec![
            shortcode_inline("Hello", token_si.clone()),
            shortcode_inline(" ", token_si.clone()),
            shortcode_inline("World", token_si.clone()),
        ];
        let new_inlines = orig_inlines.clone();
        let plan = InlineReconciliationPlan {
            inline_alignments: vec![
                InlineAlignment::KeepBefore(0),
                InlineAlignment::KeepBefore(1),
                InlineAlignment::KeepBefore(2),
            ],
            ..Default::default()
        };

        let mut warnings = Vec::new();
        let out = assemble_inline_content(
            qmd,
            &orig_inlines,
            &new_inlines,
            &plan,
            TARGET,
            &mut warnings,
        )
        .unwrap();

        assert_eq!(
            out, qmd,
            "Three shared-Invocation inlines must emit the token bytes once"
        );
    }

    #[test]
    fn multi_inline_no_dedupe_when_invocations_differ() {
        // Two inlines, each pointing at a *different* token range — no
        // dedupe; each emits its own range.
        let qmd = "AB";
        let orig_inlines = vec![
            shortcode_inline("A", SourceInfo::original(TARGET, 0, 1)),
            shortcode_inline("B", SourceInfo::original(TARGET, 1, 2)),
        ];
        let new_inlines = orig_inlines.clone();
        let plan = InlineReconciliationPlan {
            inline_alignments: vec![
                InlineAlignment::KeepBefore(0),
                InlineAlignment::KeepBefore(1),
            ],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let out = assemble_inline_content(
            qmd,
            &orig_inlines,
            &new_inlines,
            &plan,
            TARGET,
            &mut warnings,
        )
        .unwrap();

        // No dedupe: each inline's bytes emit.
        assert_eq!(out, "AB");
    }

    #[test]
    fn multi_inline_dedupe_with_value_source_difference_still_dedupes() {
        // Forward-compat with Plan 9: two inlines whose Invocation anchors
        // are PartialEq-equal but whose ValueSource anchors differ — still
        // dedupes (dedupe consults Invocation only).
        let qmd = "{{< meta foo >}}";
        let token_si = SourceInfo::original(TARGET, 0, qmd.len());

        let mut si_a = SourceInfo::generated(By::shortcode("meta"));
        si_a.append_anchor(AnchorRole::Invocation, Arc::new(token_si.clone()));
        si_a.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(TARGET, 100, 110)),
        );

        let mut si_b = SourceInfo::generated(By::shortcode("meta"));
        si_b.append_anchor(AnchorRole::Invocation, Arc::new(token_si));
        si_b.append_anchor(
            AnchorRole::ValueSource,
            Arc::new(SourceInfo::original(TARGET, 200, 215)),
        );

        let orig_inlines = vec![make_str("a", si_a), make_str("b", si_b)];
        let new_inlines = orig_inlines.clone();
        let plan = InlineReconciliationPlan {
            inline_alignments: vec![
                InlineAlignment::KeepBefore(0),
                InlineAlignment::KeepBefore(1),
            ],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let out = assemble_inline_content(
            qmd,
            &orig_inlines,
            &new_inlines,
            &plan,
            TARGET,
            &mut warnings,
        )
        .unwrap();

        // Still dedupes — emit the token once.
        assert_eq!(out, qmd);
    }

    #[test]
    fn inline_use_after_on_atomic_generated_soft_drops_to_keep_before_with_q3_42() {
        // User retyped over a shortcode-resolved inline. UseAfter
        // → KeepBefore(0) (the positional proxy) + Q-3-42.
        let qmd = "{{< meta foo >}}";
        let token_si = SourceInfo::original(TARGET, 0, qmd.len());
        let mut gen_info = SourceInfo::generated(By::shortcode("meta"));
        gen_info.append_anchor(AnchorRole::Invocation, Arc::new(token_si));

        let orig_inlines = vec![make_str("Resolved", gen_info)];
        // New-side inline: a plain user edit (no Invocation anchor).
        let new_inlines = vec![make_str("Retyped", SourceInfo::for_test())];
        let plan = InlineReconciliationPlan {
            inline_alignments: vec![InlineAlignment::UseAfter(0)],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let out = assemble_inline_content(
            qmd,
            &orig_inlines,
            &new_inlines,
            &plan,
            TARGET,
            &mut warnings,
        )
        .unwrap();

        // Soft-drop: emit the original inline's bytes (its preimage maps
        // to the whole shortcode token).
        assert_eq!(out, qmd);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code.as_deref(), Some("Q-3-42"));
    }
}

// =============================================================================
// Plan 7g Phase 1 — tiling auditor logic tests
// =============================================================================
//
// These tests verify the auditor's grouping and descent logic on hand-crafted
// SourceInfo fixture cases WITHOUT requiring a real parse. They are the
// mandatory pre-corpus check called out in the Phase 1 spec:
//
//   (a) Same-Invocation sibling pair → collapsed to one unit, no false-positive overlap.
//   (b) Whitespace-gap None-Concat → `WhitespaceGapConcat` finding.
//   (c) Non-resolvable-piece None-Concat → `ScatteredConcat` finding.
//   (d) Plain Original sibling pair with a gap → passes non-overlap check.

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
