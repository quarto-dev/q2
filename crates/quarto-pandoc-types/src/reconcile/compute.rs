/*
 * compute.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Compute phase of AST reconciliation.
 *
 * This module computes a ReconciliationPlan by comparing two ASTs.
 * The plan describes how to merge them without actually mutating anything.
 */

use super::hash::{
    HashCache, compute_block_hash_fresh, compute_inline_hash_fresh, structural_eq_block,
    structural_eq_inline,
};
use super::types::{
    BlockAlignment, CustomNodeSlotPlan, InlineAlignment, InlineReconciliationPlan,
    ReconciliationPlan, ReconciliationStats,
};
use crate::custom::{CustomNode, Slot};
use crate::{Block, Inline, Pandoc};
use rustc_hash::{FxHashMap, FxHashSet};

/// Compute a reconciliation plan for two Pandoc ASTs.
///
/// This function is pure - it doesn't mutate either AST.
/// The returned plan can be inspected or applied later.
pub fn compute_reconciliation(original: &Pandoc, executed: &Pandoc) -> ReconciliationPlan {
    let mut cache = HashCache::new();
    compute_reconciliation_for_blocks(&original.blocks, &executed.blocks, &mut cache)
}

/// Compute reconciliation plan for two block sequences.
pub fn compute_reconciliation_for_blocks<'a>(
    original: &'a [Block],
    executed: &[Block],
    cache: &mut HashCache<'a>,
) -> ReconciliationPlan {
    // Early exit: if both are empty, nothing to do
    if original.is_empty() && executed.is_empty() {
        return ReconciliationPlan::new();
    }

    // Compute hashes for original blocks (cached)
    let original_hashes: Vec<u64> = original.iter().map(|b| cache.hash_block(b)).collect();

    // Build hash → indices multimap for original blocks
    let mut hash_to_indices: FxHashMap<u64, Vec<usize>> = FxHashMap::default();
    for (idx, &hash) in original_hashes.iter().enumerate() {
        hash_to_indices.entry(hash).or_default().push(idx);
    }

    let mut alignments = Vec::with_capacity(executed.len());
    let mut block_container_plans = FxHashMap::default();
    let mut inline_plans = FxHashMap::default();
    let mut custom_node_plans = FxHashMap::default();
    let mut used_original: FxHashSet<usize> = FxHashSet::default();
    let mut stats = ReconciliationStats::default();

    // For each executed block, find a matching original
    for (exec_idx, exec_block) in executed.iter().enumerate() {
        let exec_hash = compute_block_hash_fresh(exec_block);

        // Step 1: Try exact hash match first
        if let Some(indices) = hash_to_indices.get(&exec_hash) {
            if let Some(&orig_idx) = indices.iter().find(|&&i| !used_original.contains(&i)) {
                // Verify with structural equality (guards against hash collisions)
                if structural_eq_block(&original[orig_idx], exec_block) {
                    used_original.insert(orig_idx);
                    alignments.push(BlockAlignment::KeepBefore(orig_idx));
                    stats.blocks_kept += 1;
                    continue;
                }
                // Hash collision - fall through to type-based matching
            }
        }

        // Step 2: No exact match - try type-based matching for containers
        let exec_discriminant = std::mem::discriminant(exec_block);
        let type_match = original
            .iter()
            .enumerate()
            .filter(|(i, _)| !used_original.contains(i))
            .find(|(_, orig_block)| {
                if std::mem::discriminant(*orig_block) != exec_discriminant {
                    return false;
                }
                if !is_container_block(orig_block) {
                    return false;
                }
                // For Custom blocks, also check type_name matches
                match (*orig_block, exec_block) {
                    (Block::Custom(o), Block::Custom(e)) => o.type_name == e.type_name,
                    _ => true,
                }
            });

        if let Some((orig_idx, orig_block)) = type_match {
            // Container with same type but different hash: recurse
            used_original.insert(orig_idx);

            // Pre-compute the nested reconciliation plan for this container
            let alignment_idx = alignments.len();

            // Handle Custom blocks specially (slot-based reconciliation)
            if let (Block::Custom(orig_cn), Block::Custom(exec_cn)) = (orig_block, exec_block) {
                let slot_plan = compute_custom_node_slot_plan(orig_cn, exec_cn, cache);
                custom_node_plans.insert(alignment_idx, slot_plan);
            } else {
                let nested_plan = compute_container_plan(orig_block, exec_block, cache);
                stats.merge(&nested_plan.stats);
                block_container_plans.insert(alignment_idx, nested_plan);
            }

            alignments.push(BlockAlignment::RecurseIntoContainer {
                before_idx: orig_idx,
                after_idx: exec_idx,
            });
            stats.blocks_recursed += 1;
            continue;
        }

        // Step 3: Try type-based matching for blocks with inline content
        let type_match_inline = original
            .iter()
            .enumerate()
            .filter(|(i, _)| !used_original.contains(i))
            .find(|(_, orig_block)| {
                std::mem::discriminant(*orig_block) == exec_discriminant
                    && has_inline_content(orig_block)
            });

        if let Some((orig_idx, orig_block)) = type_match_inline {
            // Block with inline content (Paragraph, Header, etc.)
            // Compute inline reconciliation plan to see if any inlines match
            if let Some(inline_plan) = compute_inline_plan_for_block(orig_block, exec_block, cache)
            {
                // Check if any inlines are kept from original
                let has_kept_inlines = inline_plan
                    .inline_alignments
                    .iter()
                    .any(|a| matches!(a, InlineAlignment::KeepBefore(_)));

                if has_kept_inlines {
                    // Some inlines match - recurse to preserve them
                    used_original.insert(orig_idx);
                    let alignment_idx = alignments.len();
                    inline_plans.insert(alignment_idx, inline_plan);
                    alignments.push(BlockAlignment::RecurseIntoContainer {
                        before_idx: orig_idx,
                        after_idx: exec_idx,
                    });
                    stats.blocks_recursed += 1;
                    continue;
                }
                // No inlines kept - fall through to use executed block
            }
        }

        // Step 4: No match at all - use executed block
        alignments.push(BlockAlignment::UseAfter(exec_idx));
        stats.blocks_replaced += 1;
    }

    ReconciliationPlan {
        block_alignments: alignments,
        block_container_plans,
        inline_plans,
        custom_node_plans,
        stats,
    }
}

/// Check if a block is a container (has block children that need reconciliation).
fn is_container_block(block: &Block) -> bool {
    matches!(
        block,
        Block::Div(_)
            | Block::BlockQuote(_)
            | Block::OrderedList(_)
            | Block::BulletList(_)
            | Block::DefinitionList(_)
            | Block::Figure(_)
            | Block::Custom(_)
    )
}

/// Check if a block has inline content that might need reconciliation.
fn has_inline_content(block: &Block) -> bool {
    matches!(
        block,
        Block::Paragraph(_) | Block::Plain(_) | Block::Header(_)
    )
}

/// Compute a nested reconciliation plan for a container block's children.
fn compute_container_plan<'a>(
    orig_block: &'a Block,
    exec_block: &Block,
    cache: &mut HashCache<'a>,
) -> ReconciliationPlan {
    match (orig_block, exec_block) {
        (Block::Div(orig), Block::Div(exec)) => {
            compute_reconciliation_for_blocks(&orig.content, &exec.content, cache)
        }
        (Block::BlockQuote(orig), Block::BlockQuote(exec)) => {
            compute_reconciliation_for_blocks(&orig.content, &exec.content, cache)
        }
        (Block::OrderedList(orig), Block::OrderedList(exec)) => {
            // For lists, reconcile each item pairwise
            compute_list_plan(&orig.content, &exec.content, cache)
        }
        (Block::BulletList(orig), Block::BulletList(exec)) => {
            compute_list_plan(&orig.content, &exec.content, cache)
        }
        (Block::Figure(orig), Block::Figure(exec)) => {
            compute_reconciliation_for_blocks(&orig.content, &exec.content, cache)
        }
        (Block::DefinitionList(orig), Block::DefinitionList(exec)) => {
            // Simplified: just compare definitions pairwise
            let mut plan = ReconciliationPlan::new();
            for ((_, orig_defs), (_, exec_defs)) in orig.content.iter().zip(&exec.content) {
                for (orig_def, exec_def) in orig_defs.iter().zip(exec_defs) {
                    let nested = compute_reconciliation_for_blocks(orig_def, exec_def, cache);
                    plan.stats.merge(&nested.stats);
                }
            }
            plan
        }
        _ => {
            // Should not happen if is_container_block is correct
            ReconciliationPlan::new()
        }
    }
}

/// Compute reconciliation for list items (Vec<Vec<Block>>).
fn compute_list_plan<'a>(
    orig_items: &'a [Vec<Block>],
    exec_items: &[Vec<Block>],
    cache: &mut HashCache<'a>,
) -> ReconciliationPlan {
    let mut plan = ReconciliationPlan::new();

    // Simple pairwise matching for now
    for (orig_item, exec_item) in orig_items.iter().zip(exec_items) {
        let nested = compute_reconciliation_for_blocks(orig_item, exec_item, cache);
        plan.stats.merge(&nested.stats);
    }

    // Handle extra items in executed (new items from engine)
    if exec_items.len() > orig_items.len() {
        plan.stats.blocks_replaced += exec_items.len() - orig_items.len();
    }

    plan
}

/// Compute reconciliation plan for a CustomNode's slots.
///
/// This uses slot names as keys for matching. For each slot in the executed
/// CustomNode, we check if the original has a matching slot and compute
/// a reconciliation plan for its content.
fn compute_custom_node_slot_plan<'a>(
    orig: &'a CustomNode,
    exec: &CustomNode,
    cache: &mut HashCache<'a>,
) -> CustomNodeSlotPlan {
    use super::hash::{structural_eq_block, structural_eq_inline};

    let mut block_slot_plans = FxHashMap::default();
    let mut inline_slot_plans = FxHashMap::default();

    // For each slot in executed, check if we need reconciliation
    for (name, exec_slot) in &exec.slots {
        // Try to find matching slot in original
        let Some(orig_slot) = orig.slots.get(name) else {
            // No original slot - will use executed (no plan needed)
            continue;
        };

        // Check if slot types match and compute appropriate plan
        match (orig_slot, exec_slot) {
            (Slot::Block(orig_b), Slot::Block(exec_b)) => {
                // Check if content matches exactly
                let orig_hash = cache.hash_block(orig_b);
                let exec_hash = compute_block_hash_fresh(exec_b);

                if orig_hash != exec_hash || !structural_eq_block(orig_b, exec_b) {
                    // Content differs - compute plan for single-element sequence
                    let plan = compute_reconciliation_for_blocks(
                        std::slice::from_ref(orig_b.as_ref()),
                        std::slice::from_ref(exec_b.as_ref()),
                        cache,
                    );
                    block_slot_plans.insert(name.clone(), plan);
                }
                // If equal, no plan needed (implicit KeepOriginal)
            }
            (Slot::Blocks(orig_bs), Slot::Blocks(exec_bs)) => {
                // Compute plan for block sequences
                let plan = compute_reconciliation_for_blocks(orig_bs, exec_bs, cache);

                // Only store if there's actual reconciliation work to do
                let needs_plan = plan
                    .block_alignments
                    .iter()
                    .any(|a| !matches!(a, BlockAlignment::KeepBefore(_)))
                    || !plan.block_container_plans.is_empty()
                    || !plan.inline_plans.is_empty()
                    || !plan.custom_node_plans.is_empty();

                if needs_plan {
                    block_slot_plans.insert(name.clone(), plan);
                }
            }
            (Slot::Inline(orig_i), Slot::Inline(exec_i)) => {
                let orig_hash = cache.hash_inline(orig_i);
                let exec_hash = compute_inline_hash_fresh(exec_i);

                if orig_hash != exec_hash || !structural_eq_inline(orig_i, exec_i) {
                    let plan = compute_inline_alignments(
                        std::slice::from_ref(orig_i.as_ref()),
                        std::slice::from_ref(exec_i.as_ref()),
                        cache,
                    );
                    inline_slot_plans.insert(name.clone(), plan);
                }
            }
            (Slot::Inlines(orig_is), Slot::Inlines(exec_is)) => {
                let plan = compute_inline_alignments(orig_is, exec_is, cache);

                let needs_plan = plan
                    .inline_alignments
                    .iter()
                    .any(|a| !matches!(a, InlineAlignment::KeepBefore(_)))
                    || !plan.inline_container_plans.is_empty()
                    || !plan.note_block_plans.is_empty()
                    || !plan.custom_node_plans.is_empty();

                if needs_plan {
                    inline_slot_plans.insert(name.clone(), plan);
                }
            }
            _ => {
                // Slot type changed - no reconciliation possible
                // Will use executed slot entirely (no plan entry)
            }
        }
    }

    CustomNodeSlotPlan {
        block_slot_plans,
        inline_slot_plans,
    }
}

/// Compute inline reconciliation plan for a block with inline content.
fn compute_inline_plan_for_block<'a>(
    orig_block: &'a Block,
    exec_block: &Block,
    cache: &mut HashCache<'a>,
) -> Option<InlineReconciliationPlan> {
    match (orig_block, exec_block) {
        (Block::Paragraph(orig), Block::Paragraph(exec)) => Some(compute_inline_alignments(
            &orig.content,
            &exec.content,
            cache,
        )),
        (Block::Plain(orig), Block::Plain(exec)) => Some(compute_inline_alignments(
            &orig.content,
            &exec.content,
            cache,
        )),
        (Block::Header(orig), Block::Header(exec)) => Some(compute_inline_alignments(
            &orig.content,
            &exec.content,
            cache,
        )),
        _ => None,
    }
}

/// Compute alignment for inline sequences.
fn compute_inline_alignments<'a>(
    original: &'a [Inline],
    executed: &[Inline],
    cache: &mut HashCache<'a>,
) -> InlineReconciliationPlan {
    if original.is_empty() && executed.is_empty() {
        return InlineReconciliationPlan::new();
    }

    // Compute hashes for original inlines
    let original_hashes: Vec<u64> = original.iter().map(|i| cache.hash_inline(i)).collect();

    // Build hash → indices multimap
    let mut hash_to_indices: FxHashMap<u64, Vec<usize>> = FxHashMap::default();
    for (idx, &hash) in original_hashes.iter().enumerate() {
        hash_to_indices.entry(hash).or_default().push(idx);
    }

    let mut alignments = Vec::with_capacity(executed.len());
    let mut inline_container_plans = FxHashMap::default();
    let mut note_block_plans = FxHashMap::default();
    let mut custom_node_plans = FxHashMap::default();
    let mut used_original: FxHashSet<usize> = FxHashSet::default();

    for (exec_idx, exec_inline) in executed.iter().enumerate() {
        let exec_hash = compute_inline_hash_fresh(exec_inline);

        // Step 1: Try exact hash match
        if let Some(indices) = hash_to_indices.get(&exec_hash) {
            if let Some(&orig_idx) = indices.iter().find(|&&i| !used_original.contains(&i)) {
                if structural_eq_inline(&original[orig_idx], exec_inline) {
                    used_original.insert(orig_idx);
                    alignments.push(InlineAlignment::KeepBefore(orig_idx));
                    continue;
                }
            }
        }

        // Step 2: Type-based matching for container inlines
        let exec_discriminant = std::mem::discriminant(exec_inline);
        let type_match = original
            .iter()
            .enumerate()
            .filter(|(i, _)| !used_original.contains(i))
            .find(|(_, orig_inline)| {
                if std::mem::discriminant(*orig_inline) != exec_discriminant {
                    return false;
                }
                if !is_container_inline(orig_inline) {
                    return false;
                }
                // For Custom inlines, also check type_name matches
                match (*orig_inline, exec_inline) {
                    (Inline::Custom(o), Inline::Custom(e)) => o.type_name == e.type_name,
                    _ => true,
                }
            });

        if let Some((orig_idx, orig_inline)) = type_match {
            used_original.insert(orig_idx);

            let alignment_idx = alignments.len();

            // Handle Note specially (contains Blocks)
            if let (Inline::Note(orig_note), Inline::Note(exec_note)) = (orig_inline, exec_inline) {
                let nested_cache = &mut HashCache::new();
                let block_plan = compute_reconciliation_for_blocks(
                    &orig_note.content,
                    &exec_note.content,
                    nested_cache,
                );
                note_block_plans.insert(alignment_idx, block_plan);
            } else if let (Inline::Custom(orig_cn), Inline::Custom(exec_cn)) =
                (orig_inline, exec_inline)
            {
                // Handle Custom inline (has named slots with blocks/inlines)
                let slot_plan = compute_custom_node_slot_plan(orig_cn, exec_cn, cache);
                custom_node_plans.insert(alignment_idx, slot_plan);
            } else {
                // Other container inlines have inline children
                if let Some(nested_plan) =
                    compute_inline_container_plan(orig_inline, exec_inline, cache)
                {
                    inline_container_plans.insert(alignment_idx, nested_plan);
                }
            }

            alignments.push(InlineAlignment::RecurseIntoContainer {
                before_idx: orig_idx,
                after_idx: exec_idx,
            });
            continue;
        }

        // Step 3: No match - use executed
        alignments.push(InlineAlignment::UseAfter(exec_idx));
    }

    InlineReconciliationPlan {
        inline_alignments: alignments,
        inline_container_plans,
        note_block_plans,
        custom_node_plans,
    }
}

/// Check if an inline is a container (has inline children).
fn is_container_inline(inline: &Inline) -> bool {
    matches!(
        inline,
        Inline::Emph(_)
            | Inline::Strong(_)
            | Inline::Underline(_)
            | Inline::Strikeout(_)
            | Inline::Superscript(_)
            | Inline::Subscript(_)
            | Inline::SmallCaps(_)
            | Inline::Quoted(_)
            | Inline::Cite(_)
            | Inline::Link(_)
            | Inline::Image(_)
            | Inline::Span(_)
            | Inline::Note(_)
            | Inline::Insert(_)
            | Inline::Delete(_)
            | Inline::Highlight(_)
            | Inline::EditComment(_)
            | Inline::Custom(_)
    )
}

/// Compute nested plan for inline container children.
fn compute_inline_container_plan<'a>(
    orig_inline: &'a Inline,
    exec_inline: &Inline,
    cache: &mut HashCache<'a>,
) -> Option<InlineReconciliationPlan> {
    match (orig_inline, exec_inline) {
        (Inline::Emph(o), Inline::Emph(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Strong(o), Inline::Strong(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Underline(o), Inline::Underline(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Strikeout(o), Inline::Strikeout(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Superscript(o), Inline::Superscript(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Subscript(o), Inline::Subscript(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::SmallCaps(o), Inline::SmallCaps(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Quoted(o), Inline::Quoted(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Cite(o), Inline::Cite(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Link(o), Inline::Link(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Image(o), Inline::Image(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Span(o), Inline::Span(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Insert(o), Inline::Insert(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Delete(o), Inline::Delete(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::Highlight(o), Inline::Highlight(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        (Inline::EditComment(o), Inline::EditComment(e)) => {
            Some(compute_inline_alignments(&o.content, &e.content, cache))
        }
        // Note is handled separately (contains Blocks)
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Div, Paragraph, Str};
    use hashlink::LinkedHashMap;
    use quarto_source_map::{FileId, SourceInfo};

    fn dummy_source() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn make_para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: dummy_source(),
            })],
            source_info: dummy_source(),
        })
    }

    fn make_div(blocks: Vec<Block>) -> Block {
        Block::Div(Div {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: blocks,
            source_info: dummy_source(),
            attr_source: crate::AttrSourceInfo::empty(),
        })
    }

    #[test]
    fn test_identical_asts_all_kept() {
        let blocks = vec![make_para("hello"), make_para("world")];
        let original = Pandoc {
            meta: Default::default(),
            blocks: blocks.clone(),
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks,
        };

        let plan = compute_reconciliation(&original, &executed);

        assert_eq!(plan.block_alignments.len(), 2);
        assert!(matches!(
            plan.block_alignments[0],
            BlockAlignment::KeepBefore(0)
        ));
        assert!(matches!(
            plan.block_alignments[1],
            BlockAlignment::KeepBefore(1)
        ));
        assert_eq!(plan.stats.blocks_kept, 2);
        assert_eq!(plan.stats.blocks_replaced, 0);
    }

    #[test]
    fn test_new_block_uses_executed() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_para("hello")],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_para("hello"), make_para("new")],
        };

        let plan = compute_reconciliation(&original, &executed);

        assert_eq!(plan.block_alignments.len(), 2);
        assert!(matches!(
            plan.block_alignments[0],
            BlockAlignment::KeepBefore(0)
        ));
        assert!(matches!(
            plan.block_alignments[1],
            BlockAlignment::UseAfter(1)
        ));
    }

    #[test]
    fn test_container_recursion() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_div(vec![make_para("hello"), make_para("world")])],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_div(vec![make_para("hello"), make_para("changed")])],
        };

        let plan = compute_reconciliation(&original, &executed);

        // The Div should trigger recursion since children changed
        assert_eq!(plan.block_alignments.len(), 1);
        assert!(matches!(
            plan.block_alignments[0],
            BlockAlignment::RecurseIntoContainer { .. }
        ));

        // Should have a nested plan for the Div's children
        assert!(plan.block_container_plans.contains_key(&0));
    }
}
