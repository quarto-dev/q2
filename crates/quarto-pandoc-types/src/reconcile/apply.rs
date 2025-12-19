/*
 * apply.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Apply phase of AST reconciliation.
 *
 * This module applies a ReconciliationPlan to produce a merged AST.
 * Both input ASTs are consumed, enabling zero-copy moves.
 */

use super::types::{
    BlockAlignment, InlineAlignment, InlineReconciliationPlan, ReconciliationPlan,
};
use crate::{Block, Inline, Pandoc};

/// Apply a reconciliation plan to produce a merged Pandoc AST.
///
/// Both inputs are consumed, enabling zero-copy moves.
/// The plan must have been computed from these same ASTs.
pub fn apply_reconciliation(
    original: Pandoc,
    executed: Pandoc,
    plan: &ReconciliationPlan,
) -> Pandoc {
    let result_blocks =
        apply_reconciliation_to_blocks(original.blocks, executed.blocks, plan);

    Pandoc {
        // For v1, executed metadata wins entirely
        meta: executed.meta,
        blocks: result_blocks,
    }
}

/// Apply reconciliation to block sequences.
pub fn apply_reconciliation_to_blocks(
    original: Vec<Block>,
    executed: Vec<Block>,
    plan: &ReconciliationPlan,
) -> Vec<Block> {
    // Convert to Option<Block> so we can take ownership of individual blocks
    let mut orig_slots: Vec<Option<Block>> = original.into_iter().map(Some).collect();
    let mut exec_slots: Vec<Option<Block>> = executed.into_iter().map(Some).collect();

    let mut result = Vec::with_capacity(plan.block_alignments.len());

    for (alignment_idx, alignment) in plan.block_alignments.iter().enumerate() {
        let block = match alignment {
            BlockAlignment::KeepBefore(orig_idx) => {
                // MOVE from original (zero-copy)
                orig_slots[*orig_idx]
                    .take()
                    .expect("Original block already used")
            }
            BlockAlignment::UseAfter(exec_idx) => {
                // MOVE from executed (zero-copy)
                exec_slots[*exec_idx]
                    .take()
                    .expect("Executed block already used")
            }
            BlockAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => {
                let orig_block = orig_slots[*before_idx]
                    .take()
                    .expect("Original block already used");
                let exec_block = exec_slots[*after_idx]
                    .take()
                    .expect("Executed block already used");

                // Check if this is a block container or inline container
                if let Some(nested_plan) = plan.block_container_plans.get(&alignment_idx) {
                    // Block container (Div, BlockQuote, etc.)
                    apply_block_container_reconciliation(orig_block, exec_block, nested_plan)
                } else if let Some(inline_plan) = plan.inline_plans.get(&alignment_idx) {
                    // Block with inline content (Paragraph, Header, etc.)
                    apply_inline_block_reconciliation(orig_block, exec_block, inline_plan)
                } else {
                    // No nested plan - just use original (shouldn't happen)
                    orig_block
                }
            }
        };
        result.push(block);
    }

    result
}

/// Apply reconciliation to a block container (Div, BlockQuote, etc.).
fn apply_block_container_reconciliation(
    orig_container: Block,
    exec_container: Block,
    plan: &ReconciliationPlan,
) -> Block {
    match (orig_container, exec_container) {
        (Block::Div(mut orig), Block::Div(exec)) => {
            orig.content = apply_reconciliation_to_blocks(orig.content, exec.content, plan);
            Block::Div(orig)
        }
        (Block::BlockQuote(mut orig), Block::BlockQuote(exec)) => {
            orig.content = apply_reconciliation_to_blocks(orig.content, exec.content, plan);
            Block::BlockQuote(orig)
        }
        (Block::OrderedList(mut orig), Block::OrderedList(exec)) => {
            orig.content = apply_list_reconciliation(orig.content, exec.content, plan);
            Block::OrderedList(orig)
        }
        (Block::BulletList(mut orig), Block::BulletList(exec)) => {
            orig.content = apply_list_reconciliation(orig.content, exec.content, plan);
            Block::BulletList(orig)
        }
        (Block::Figure(mut orig), Block::Figure(exec)) => {
            orig.content = apply_reconciliation_to_blocks(orig.content, exec.content, plan);
            Block::Figure(orig)
        }
        (Block::DefinitionList(mut orig), Block::DefinitionList(exec)) => {
            // Simplified: merge pairwise
            for ((_, orig_defs), (_, exec_defs)) in
                orig.content.iter_mut().zip(exec.content.into_iter())
            {
                for (orig_def, exec_def) in orig_defs.iter_mut().zip(exec_defs.into_iter()) {
                    *orig_def = apply_reconciliation_to_blocks(
                        std::mem::take(orig_def),
                        exec_def,
                        plan,
                    );
                }
            }
            Block::DefinitionList(orig)
        }
        // Fallback: shouldn't happen, return original
        (orig, _) => orig,
    }
}

/// Apply reconciliation to list items.
fn apply_list_reconciliation(
    mut orig_items: Vec<Vec<Block>>,
    exec_items: Vec<Vec<Block>>,
    plan: &ReconciliationPlan,
) -> Vec<Vec<Block>> {
    // Simple pairwise reconciliation
    for (orig_item, exec_item) in orig_items.iter_mut().zip(exec_items.into_iter()) {
        *orig_item =
            apply_reconciliation_to_blocks(std::mem::take(orig_item), exec_item, plan);
    }
    orig_items
}

/// Apply reconciliation to a block with inline content (Paragraph, Header, etc.).
fn apply_inline_block_reconciliation(
    orig_block: Block,
    exec_block: Block,
    inline_plan: &InlineReconciliationPlan,
) -> Block {
    match (orig_block, exec_block) {
        (Block::Paragraph(mut orig), Block::Paragraph(exec)) => {
            orig.content =
                apply_reconciliation_to_inlines(orig.content, exec.content, inline_plan);
            Block::Paragraph(orig)
        }
        (Block::Plain(mut orig), Block::Plain(exec)) => {
            orig.content =
                apply_reconciliation_to_inlines(orig.content, exec.content, inline_plan);
            Block::Plain(orig)
        }
        (Block::Header(mut orig), Block::Header(exec)) => {
            orig.content =
                apply_reconciliation_to_inlines(orig.content, exec.content, inline_plan);
            Block::Header(orig)
        }
        // Fallback
        (orig, _) => orig,
    }
}

/// Apply reconciliation to inline sequences.
fn apply_reconciliation_to_inlines(
    original: Vec<Inline>,
    executed: Vec<Inline>,
    plan: &InlineReconciliationPlan,
) -> Vec<Inline> {
    let mut orig_slots: Vec<Option<Inline>> = original.into_iter().map(Some).collect();
    let mut exec_slots: Vec<Option<Inline>> = executed.into_iter().map(Some).collect();

    let mut result = Vec::with_capacity(plan.inline_alignments.len());

    for (alignment_idx, alignment) in plan.inline_alignments.iter().enumerate() {
        let inline = match alignment {
            InlineAlignment::KeepBefore(orig_idx) => {
                orig_slots[*orig_idx]
                    .take()
                    .expect("Original inline already used")
            }
            InlineAlignment::UseAfter(exec_idx) => {
                exec_slots[*exec_idx]
                    .take()
                    .expect("Executed inline already used")
            }
            InlineAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => {
                let orig_inline = orig_slots[*before_idx]
                    .take()
                    .expect("Original inline already used");
                let exec_inline = exec_slots[*after_idx]
                    .take()
                    .expect("Executed inline already used");

                // Check for Note (contains Blocks) or other inline containers
                if let Some(block_plan) = plan.note_block_plans.get(&alignment_idx) {
                    apply_note_reconciliation(orig_inline, exec_inline, block_plan)
                } else if let Some(nested_plan) = plan.inline_container_plans.get(&alignment_idx) {
                    apply_inline_container_reconciliation(orig_inline, exec_inline, nested_plan)
                } else {
                    orig_inline
                }
            }
        };
        result.push(inline);
    }

    result
}

/// Apply reconciliation to Note inline (contains Blocks).
fn apply_note_reconciliation(
    orig_inline: Inline,
    exec_inline: Inline,
    block_plan: &ReconciliationPlan,
) -> Inline {
    match (orig_inline, exec_inline) {
        (Inline::Note(mut orig), Inline::Note(exec)) => {
            orig.content =
                apply_reconciliation_to_blocks(orig.content, exec.content, block_plan);
            Inline::Note(orig)
        }
        (orig, _) => orig,
    }
}

/// Apply reconciliation to inline containers (Emph, Strong, etc.).
fn apply_inline_container_reconciliation(
    orig_inline: Inline,
    exec_inline: Inline,
    plan: &InlineReconciliationPlan,
) -> Inline {
    match (orig_inline, exec_inline) {
        (Inline::Emph(mut o), Inline::Emph(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Emph(o)
        }
        (Inline::Strong(mut o), Inline::Strong(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Strong(o)
        }
        (Inline::Underline(mut o), Inline::Underline(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Underline(o)
        }
        (Inline::Strikeout(mut o), Inline::Strikeout(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Strikeout(o)
        }
        (Inline::Superscript(mut o), Inline::Superscript(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Superscript(o)
        }
        (Inline::Subscript(mut o), Inline::Subscript(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Subscript(o)
        }
        (Inline::SmallCaps(mut o), Inline::SmallCaps(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::SmallCaps(o)
        }
        (Inline::Quoted(mut o), Inline::Quoted(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Quoted(o)
        }
        (Inline::Cite(mut o), Inline::Cite(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Cite(o)
        }
        (Inline::Link(mut o), Inline::Link(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Link(o)
        }
        (Inline::Image(mut o), Inline::Image(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Image(o)
        }
        (Inline::Span(mut o), Inline::Span(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Span(o)
        }
        (Inline::Insert(mut o), Inline::Insert(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Insert(o)
        }
        (Inline::Delete(mut o), Inline::Delete(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Delete(o)
        }
        (Inline::Highlight(mut o), Inline::Highlight(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::Highlight(o)
        }
        (Inline::EditComment(mut o), Inline::EditComment(e)) => {
            o.content = apply_reconciliation_to_inlines(o.content, e.content, plan);
            Inline::EditComment(o)
        }
        // Fallback
        (orig, _) => orig,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Div, Paragraph, Str};
    use crate::reconcile::compute::compute_reconciliation;
    use hashlink::LinkedHashMap;
    use quarto_source_map::{FileId, SourceInfo};

    fn source_a() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 10)
    }

    fn source_b() -> SourceInfo {
        SourceInfo::original(FileId(1), 100, 200)
    }

    fn make_para_with_source(text: &str, source: SourceInfo) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: source.clone(),
            })],
            source_info: source,
        })
    }

    fn make_div_with_source(blocks: Vec<Block>, source: SourceInfo) -> Block {
        Block::Div(Div {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: blocks,
            source_info: source,
            attr_source: crate::AttrSourceInfo::empty(),
        })
    }

    #[test]
    fn test_kept_blocks_preserve_source() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_para_with_source("hello", source_a())],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_para_with_source("hello", source_b())],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        // The result should have source_a (from original)
        if let Block::Paragraph(p) = &result.blocks[0] {
            assert_eq!(p.source_info, source_a());
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_replaced_blocks_use_executed_source() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_para_with_source("hello", source_a())],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_para_with_source("changed", source_b())],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        // The result should have source_b (from executed)
        if let Block::Paragraph(p) = &result.blocks[0] {
            assert_eq!(p.source_info, source_b());
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_container_preserves_source_while_reconciling_children() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_div_with_source(
                vec![
                    make_para_with_source("kept", source_a()),
                    make_para_with_source("changed", source_a()),
                ],
                source_a(),
            )],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_div_with_source(
                vec![
                    make_para_with_source("kept", source_b()),
                    make_para_with_source("new content", source_b()),
                ],
                source_b(),
            )],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        // The Div should keep source_a
        if let Block::Div(d) = &result.blocks[0] {
            assert_eq!(d.source_info, source_a());

            // First child (kept) should have source_a
            if let Block::Paragraph(p) = &d.content[0] {
                assert_eq!(p.source_info, source_a());
            }

            // Second child (changed) should have source_b
            if let Block::Paragraph(p) = &d.content[1] {
                assert_eq!(p.source_info, source_b());
            }
        } else {
            panic!("Expected Div");
        }
    }
}
