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
    BlockAlignment, CustomNodeSlotPlan, InlineAlignment, InlineReconciliationPlan,
    ReconciliationPlan,
};
use crate::custom::{CustomNode, Slot};
use crate::{Block, Inline, Pandoc};
use hashlink::LinkedHashMap;

/// Apply a reconciliation plan to produce a merged Pandoc AST.
///
/// Both inputs are consumed, enabling zero-copy moves.
/// The plan must have been computed from these same ASTs.
pub fn apply_reconciliation(
    original: Pandoc,
    executed: Pandoc,
    plan: &ReconciliationPlan,
) -> Pandoc {
    let result_blocks = apply_reconciliation_to_blocks(original.blocks, executed.blocks, plan);

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

                // Check if this is a block container, inline container, or custom node
                if let Some(nested_plan) = plan.block_container_plans.get(&alignment_idx) {
                    // Block container (Div, BlockQuote, etc.)
                    apply_block_container_reconciliation(orig_block, exec_block, nested_plan)
                } else if let Some(inline_plan) = plan.inline_plans.get(&alignment_idx) {
                    // Block with inline content (Paragraph, Header, etc.)
                    apply_inline_block_reconciliation(orig_block, exec_block, inline_plan)
                } else if let Some(slot_plan) = plan.custom_node_plans.get(&alignment_idx) {
                    // Custom node (Callout, PanelTabset, etc.)
                    apply_custom_node_block_reconciliation(orig_block, exec_block, slot_plan)
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
                    *orig_def =
                        apply_reconciliation_to_blocks(std::mem::take(orig_def), exec_def, plan);
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
    orig_items: Vec<Vec<Block>>,
    exec_items: Vec<Vec<Block>>,
    plan: &ReconciliationPlan,
) -> Vec<Vec<Block>> {
    let mut result = Vec::with_capacity(exec_items.len());

    // Use the per-item plans from list_item_plans
    for (i, exec_item) in exec_items.into_iter().enumerate() {
        let item_plan = plan.list_item_plans.get(i);

        if let Some(orig_item) = orig_items.get(i).cloned() {
            // Have both original and executed - apply the item's plan
            if let Some(nested_plan) = item_plan {
                result.push(apply_reconciliation_to_blocks(
                    orig_item,
                    exec_item,
                    nested_plan,
                ));
            } else {
                // No plan for this item (shouldn't happen if compute is correct)
                result.push(exec_item);
            }
        } else {
            // Extra item from executed (no original) - use as-is
            result.push(exec_item);
        }
    }

    result
}

/// Apply reconciliation to a block with inline content (Paragraph, Header, etc.).
fn apply_inline_block_reconciliation(
    orig_block: Block,
    exec_block: Block,
    inline_plan: &InlineReconciliationPlan,
) -> Block {
    match (orig_block, exec_block) {
        (Block::Paragraph(mut orig), Block::Paragraph(exec)) => {
            orig.content = apply_reconciliation_to_inlines(orig.content, exec.content, inline_plan);
            Block::Paragraph(orig)
        }
        (Block::Plain(mut orig), Block::Plain(exec)) => {
            orig.content = apply_reconciliation_to_inlines(orig.content, exec.content, inline_plan);
            Block::Plain(orig)
        }
        (Block::Header(mut orig), Block::Header(exec)) => {
            orig.content = apply_reconciliation_to_inlines(orig.content, exec.content, inline_plan);
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
            InlineAlignment::KeepBefore(orig_idx) => orig_slots[*orig_idx]
                .take()
                .expect("Original inline already used"),
            InlineAlignment::UseAfter(exec_idx) => exec_slots[*exec_idx]
                .take()
                .expect("Executed inline already used"),
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

                // Check for Note (contains Blocks), Custom node, or other inline containers
                if let Some(block_plan) = plan.note_block_plans.get(&alignment_idx) {
                    apply_note_reconciliation(orig_inline, exec_inline, block_plan)
                } else if let Some(slot_plan) = plan.custom_node_plans.get(&alignment_idx) {
                    // Custom inline node
                    apply_custom_node_inline_reconciliation(orig_inline, exec_inline, slot_plan)
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
            orig.content = apply_reconciliation_to_blocks(orig.content, exec.content, block_plan);
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

/// Apply reconciliation to a CustomNode block.
fn apply_custom_node_block_reconciliation(
    orig_block: Block,
    exec_block: Block,
    slot_plan: &CustomNodeSlotPlan,
) -> Block {
    match (orig_block, exec_block) {
        (Block::Custom(orig), Block::Custom(exec)) => {
            Block::Custom(apply_custom_node_reconciliation(orig, exec, slot_plan))
        }
        // Fallback: shouldn't happen, return original
        (orig, _) => orig,
    }
}

/// Apply reconciliation to a CustomNode inline.
fn apply_custom_node_inline_reconciliation(
    orig_inline: Inline,
    exec_inline: Inline,
    slot_plan: &CustomNodeSlotPlan,
) -> Inline {
    match (orig_inline, exec_inline) {
        (Inline::Custom(orig), Inline::Custom(exec)) => {
            Inline::Custom(apply_custom_node_reconciliation(orig, exec, slot_plan))
        }
        // Fallback: shouldn't happen, return original
        (orig, _) => orig,
    }
}

/// Apply reconciliation to a CustomNode's slots.
///
/// This produces a new CustomNode with:
/// - source_info and attr from original (preserves source location)
/// - type_name from original (should match exec)
/// - plain_data from executed (use executed's config)
/// - slots reconciled by name
fn apply_custom_node_reconciliation(
    orig: CustomNode,
    exec: CustomNode,
    slot_plan: &CustomNodeSlotPlan,
) -> CustomNode {
    // Drain orig slots into a map for selective taking
    let mut orig_slots: LinkedHashMap<String, Slot> = orig.slots.into_iter().collect();

    // Build result slots following executed's slot structure (preserves order)
    let mut result_slots = LinkedHashMap::new();

    for (name, exec_slot) in exec.slots {
        let result_slot = if let Some(block_plan) = slot_plan.block_slot_plans.get(&name) {
            // Apply block plan to this slot
            let orig_slot = orig_slots.remove(&name);
            apply_block_slot_reconciliation(orig_slot, exec_slot, block_plan)
        } else if let Some(inline_plan) = slot_plan.inline_slot_plans.get(&name) {
            // Apply inline plan to this slot
            let orig_slot = orig_slots.remove(&name);
            apply_inline_slot_reconciliation(orig_slot, exec_slot, inline_plan)
        } else if let Some(orig_slot) = orig_slots.remove(&name) {
            // No plan - check if we can use original (same type = content matches)
            if std::mem::discriminant(&orig_slot) == std::mem::discriminant(&exec_slot) {
                // Same type, content must match (otherwise we'd have a plan)
                orig_slot
            } else {
                // Type mismatch, use executed
                exec_slot
            }
        } else {
            // No original slot, use executed
            exec_slot
        };

        result_slots.insert(name, result_slot);
    }

    CustomNode {
        type_name: orig.type_name, // Should equal exec.type_name
        slots: result_slots,
        plain_data: exec.plain_data,   // Use executed's plain_data
        attr: orig.attr,               // Preserve original attr (source info)
        source_info: orig.source_info, // Preserve original source
    }
}

/// Apply reconciliation to a block slot.
fn apply_block_slot_reconciliation(
    orig_slot: Option<Slot>,
    exec_slot: Slot,
    plan: &ReconciliationPlan,
) -> Slot {
    match (orig_slot, exec_slot) {
        (Some(Slot::Block(orig_b)), Slot::Block(exec_b)) => {
            let reconciled = apply_reconciliation_to_blocks(vec![*orig_b], vec![*exec_b], plan);
            Slot::Block(Box::new(
                reconciled.into_iter().next().expect("Expected one block"),
            ))
        }
        (Some(Slot::Blocks(orig_bs)), Slot::Blocks(exec_bs)) => {
            let reconciled = apply_reconciliation_to_blocks(orig_bs, exec_bs, plan);
            Slot::Blocks(reconciled)
        }
        // Type mismatch or no original, use executed
        (_, exec_slot) => exec_slot,
    }
}

/// Apply reconciliation to an inline slot.
fn apply_inline_slot_reconciliation(
    orig_slot: Option<Slot>,
    exec_slot: Slot,
    plan: &InlineReconciliationPlan,
) -> Slot {
    match (orig_slot, exec_slot) {
        (Some(Slot::Inline(orig_i)), Slot::Inline(exec_i)) => {
            let reconciled = apply_reconciliation_to_inlines(vec![*orig_i], vec![*exec_i], plan);
            Slot::Inline(Box::new(
                reconciled.into_iter().next().expect("Expected one inline"),
            ))
        }
        (Some(Slot::Inlines(orig_is)), Slot::Inlines(exec_is)) => {
            let reconciled = apply_reconciliation_to_inlines(orig_is, exec_is, plan);
            Slot::Inlines(reconciled)
        }
        // Type mismatch or no original, use executed
        (_, exec_slot) => exec_slot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconcile::compute::compute_reconciliation;
    use crate::reconcile::types::{
        BlockAlignment, InlineAlignment, InlineReconciliationPlan, ReconciliationPlan,
    };
    use crate::{
        AttrSourceInfo, BlockQuote, BulletList, Div, Emph, Header, Note, OrderedList, Paragraph,
        Plain, Str, Strong,
    };
    use crate::{ListNumberDelim, ListNumberStyle};
    use hashlink::LinkedHashMap;
    use quarto_source_map::{FileId, SourceInfo};

    fn source_a() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 10)
    }

    fn source_b() -> SourceInfo {
        SourceInfo::original(FileId(1), 100, 200)
    }

    fn make_str(text: &str, source: SourceInfo) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: source,
        })
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

    // ==================== Basic Apply Tests ====================

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

    // ==================== Block Container Tests ====================

    #[test]
    fn test_blockquote_reconciliation() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::BlockQuote(BlockQuote {
                content: vec![make_para_with_source("quoted", source_a())],
                source_info: source_a(),
            })],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::BlockQuote(BlockQuote {
                content: vec![make_para_with_source("quoted", source_b())],
                source_info: source_b(),
            })],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        if let Block::BlockQuote(bq) = &result.blocks[0] {
            assert_eq!(bq.source_info, source_a());
        } else {
            panic!("Expected BlockQuote");
        }
    }

    #[test]
    fn test_bullet_list_reconciliation() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::BulletList(BulletList {
                content: vec![vec![make_para_with_source("item1", source_a())]],
                source_info: source_a(),
            })],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::BulletList(BulletList {
                content: vec![vec![make_para_with_source("item1", source_b())]],
                source_info: source_b(),
            })],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        if let Block::BulletList(bl) = &result.blocks[0] {
            assert_eq!(bl.source_info, source_a());
        } else {
            panic!("Expected BulletList");
        }
    }

    #[test]
    fn test_ordered_list_reconciliation() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::OrderedList(OrderedList {
                attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
                content: vec![vec![make_para_with_source("item1", source_a())]],
                source_info: source_a(),
            })],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::OrderedList(OrderedList {
                attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
                content: vec![vec![make_para_with_source("item1", source_b())]],
                source_info: source_b(),
            })],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        if let Block::OrderedList(ol) = &result.blocks[0] {
            assert_eq!(ol.source_info, source_a());
        } else {
            panic!("Expected OrderedList");
        }
    }

    // ==================== Inline Block Tests ====================

    #[test]
    fn test_header_inline_reconciliation() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Header(Header {
                level: 1,
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![make_str("Title", source_a())],
                source_info: source_a(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Header(Header {
                level: 1,
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![make_str("Title", source_b())],
                source_info: source_b(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        if let Block::Header(h) = &result.blocks[0] {
            assert_eq!(h.source_info, source_a());
        } else {
            panic!("Expected Header");
        }
    }

    #[test]
    fn test_plain_inline_reconciliation() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Plain(Plain {
                content: vec![make_str("text", source_a())],
                source_info: source_a(),
            })],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Plain(Plain {
                content: vec![make_str("text", source_b())],
                source_info: source_b(),
            })],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        if let Block::Plain(p) = &result.blocks[0] {
            assert_eq!(p.source_info, source_a());
        } else {
            panic!("Expected Plain");
        }
    }

    // ==================== Inline Container Tests ====================

    #[test]
    fn test_emph_inline_reconciliation() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Emph(Emph {
                    content: vec![make_str("emphasized", source_a())],
                    source_info: source_a(),
                })],
                source_info: source_a(),
            })],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Emph(Emph {
                    content: vec![make_str("emphasized", source_b())],
                    source_info: source_b(),
                })],
                source_info: source_b(),
            })],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        if let Block::Paragraph(p) = &result.blocks[0] {
            if let Inline::Emph(e) = &p.content[0] {
                assert_eq!(e.source_info, source_a());
            } else {
                panic!("Expected Emph");
            }
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_strong_inline_reconciliation() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Strong(Strong {
                    content: vec![make_str("bold", source_a())],
                    source_info: source_a(),
                })],
                source_info: source_a(),
            })],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Strong(Strong {
                    content: vec![make_str("bold", source_b())],
                    source_info: source_b(),
                })],
                source_info: source_b(),
            })],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        if let Block::Paragraph(p) = &result.blocks[0] {
            if let Inline::Strong(s) = &p.content[0] {
                assert_eq!(s.source_info, source_a());
            } else {
                panic!("Expected Strong");
            }
        } else {
            panic!("Expected Paragraph");
        }
    }

    // ==================== Note Reconciliation Tests ====================

    #[test]
    fn test_note_reconciliation() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Note(Note {
                    content: vec![make_para_with_source("footnote", source_a())],
                    source_info: source_a(),
                })],
                source_info: source_a(),
            })],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Note(Note {
                    content: vec![make_para_with_source("footnote", source_b())],
                    source_info: source_b(),
                })],
                source_info: source_b(),
            })],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        if let Block::Paragraph(p) = &result.blocks[0] {
            if let Inline::Note(n) = &p.content[0] {
                assert_eq!(n.source_info, source_a());
            } else {
                panic!("Expected Note");
            }
        } else {
            panic!("Expected Paragraph");
        }
    }

    // ==================== Direct Apply Function Tests ====================

    #[test]
    fn test_apply_reconciliation_to_blocks_keep_before() {
        let original = vec![make_para_with_source("hello", source_a())];
        let executed = vec![make_para_with_source("hello", source_b())];

        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::KeepBefore(0)],
            ..Default::default()
        };

        let result = apply_reconciliation_to_blocks(original, executed, &plan);

        assert_eq!(result.len(), 1);
        if let Block::Paragraph(p) = &result[0] {
            assert_eq!(p.source_info, source_a());
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_apply_reconciliation_to_blocks_use_after() {
        let original = vec![make_para_with_source("hello", source_a())];
        let executed = vec![make_para_with_source("world", source_b())];

        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::UseAfter(0)],
            ..Default::default()
        };

        let result = apply_reconciliation_to_blocks(original, executed, &plan);

        assert_eq!(result.len(), 1);
        if let Block::Paragraph(p) = &result[0] {
            assert_eq!(p.source_info, source_b());
            if let Inline::Str(s) = &p.content[0] {
                assert_eq!(s.text, "world");
            }
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_apply_reconciliation_to_inlines() {
        let original = vec![make_str("hello", source_a())];
        let executed = vec![make_str("hello", source_b())];

        let plan = InlineReconciliationPlan {
            inline_alignments: vec![InlineAlignment::KeepBefore(0)],
            ..Default::default()
        };

        let result = apply_reconciliation_to_inlines(original, executed, &plan);

        assert_eq!(result.len(), 1);
        if let Inline::Str(s) = &result[0] {
            assert_eq!(s.source_info, source_a());
        } else {
            panic!("Expected Str");
        }
    }

    #[test]
    fn test_apply_reconciliation_to_inlines_use_after() {
        let original = vec![make_str("hello", source_a())];
        let executed = vec![make_str("world", source_b())];

        let plan = InlineReconciliationPlan {
            inline_alignments: vec![InlineAlignment::UseAfter(0)],
            ..Default::default()
        };

        let result = apply_reconciliation_to_inlines(original, executed, &plan);

        assert_eq!(result.len(), 1);
        if let Inline::Str(s) = &result[0] {
            assert_eq!(s.source_info, source_b());
            assert_eq!(s.text, "world");
        } else {
            panic!("Expected Str");
        }
    }

    // ==================== Empty Cases ====================

    #[test]
    fn test_empty_reconciliation() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original, executed, &plan);

        assert!(result.blocks.is_empty());
    }

    #[test]
    fn test_multiple_blocks_reconciliation() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para_with_source("first", source_a()),
                make_para_with_source("second", source_a()),
                make_para_with_source("third", source_a()),
            ],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para_with_source("first", source_b()),
                make_para_with_source("changed", source_b()),
                make_para_with_source("third", source_b()),
            ],
        };

        let plan = compute_reconciliation(&original, &executed);
        let result = apply_reconciliation(original.clone(), executed, &plan);

        assert_eq!(result.blocks.len(), 3);

        // First block should have source_a (kept)
        if let Block::Paragraph(p) = &result.blocks[0] {
            assert_eq!(p.source_info, source_a());
        }

        // Second block should have source_b (replaced)
        if let Block::Paragraph(p) = &result.blocks[1] {
            assert_eq!(p.source_info, source_b());
        }

        // Third block should have source_a (kept)
        if let Block::Paragraph(p) = &result.blocks[2] {
            assert_eq!(p.source_info, source_a());
        }
    }
}
