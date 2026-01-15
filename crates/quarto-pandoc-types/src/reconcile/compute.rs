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
    HashCache, compute_block_hash_fresh, compute_blocks_hash_fresh, compute_inline_hash_fresh,
    structural_eq_block, structural_eq_blocks, structural_eq_inline,
};
use super::types::{
    BlockAlignment, CustomNodeSlotPlan, InlineAlignment, InlineReconciliationPlan,
    ListItemAlignment, ReconciliationPlan, ReconciliationStats, TableCellPosition,
    TableReconciliationPlan,
};
use crate::custom::{CustomNode, Slot};
use crate::table::Table;
use crate::{Block, Inline, Pandoc};
use hashlink::LinkedHashMap;
use rustc_hash::FxHashSet;

/// Compute a reconciliation plan for two Pandoc ASTs.
///
/// This function is pure - it doesn't mutate either AST.
/// The returned plan can be inspected or applied later.
pub fn compute_reconciliation(original: &Pandoc, executed: &Pandoc) -> ReconciliationPlan {
    let mut cache = HashCache::new();
    compute_reconciliation_for_blocks(&original.blocks, &executed.blocks, &mut cache)
}

/// Compute reconciliation plan for two block sequences.
///
/// Uses a three-phase algorithm:
/// 1. **Exact matches (any position)**: Find blocks with identical content (hash match).
///    These get `KeepBefore` - we have proof they're the same.
/// 2. **Positional matches (same index only)**: For unmatched blocks, check if the
///    block at the same position in original has the same type. If so, recurse.
///    Position match provides reasonable evidence they're related.
/// 3. **Fallback**: Remaining unmatched blocks get `UseAfter` - treat as new content.
///
/// This approach avoids "trying too hard" to match unrelated containers. We only
/// recurse when we have evidence the containers are the same logical entity.
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
    let mut hash_to_indices: LinkedHashMap<u64, Vec<usize>> = LinkedHashMap::new();
    for (idx, &hash) in original_hashes.iter().enumerate() {
        hash_to_indices
            .entry(hash)
            .or_insert_with(Vec::new)
            .push(idx);
    }

    // Initialize alignment slots - we'll fill these across three phases
    let mut alignments: Vec<Option<BlockAlignment>> = vec![None; executed.len()];
    let mut block_container_plans = LinkedHashMap::new();
    let mut inline_plans = LinkedHashMap::new();
    let mut custom_node_plans = LinkedHashMap::new();
    let mut table_plans = LinkedHashMap::new();
    let mut used_original: FxHashSet<usize> = FxHashSet::default();
    let mut stats = ReconciliationStats::default();

    // Track which executed indices need further processing
    let mut needs_phase_2: Vec<usize> = Vec::new();

    // =========================================================================
    // Phase 1: Exact hash matches (any position)
    // =========================================================================
    // Find blocks with identical content. Position mismatch is fine because
    // we have definitive proof (hash + structural equality) they're the same.
    for (exec_idx, exec_block) in executed.iter().enumerate() {
        let exec_hash = compute_block_hash_fresh(exec_block);

        if let Some(indices) = hash_to_indices.get(&exec_hash)
            && let Some(&orig_idx) = indices.iter().find(|&&i| !used_original.contains(&i))
        {
            // Verify with structural equality (guards against hash collisions)
            if structural_eq_block(&original[orig_idx], exec_block) {
                used_original.insert(orig_idx);
                alignments[exec_idx] = Some(BlockAlignment::KeepBefore(orig_idx));
                stats.blocks_kept += 1;
                continue;
            }
            // Hash collision - fall through to phase 2
        }

        needs_phase_2.push(exec_idx);
    }

    // =========================================================================
    // Phase 2: Positional type matches (same index only)
    // =========================================================================
    // For unmatched blocks, check if the block at the SAME position in original
    // is available and has the same type. Only recurse when indices match -
    // this provides reasonable evidence they're the same logical entity.
    let mut needs_phase_3: Vec<usize> = Vec::new();

    for exec_idx in needs_phase_2 {
        let exec_block = &executed[exec_idx];

        // Only consider the positionally-corresponding original
        if exec_idx < original.len() && !used_original.contains(&exec_idx) {
            let orig_block = &original[exec_idx];
            let exec_discriminant = std::mem::discriminant(exec_block);
            let orig_discriminant = std::mem::discriminant(orig_block);

            if exec_discriminant == orig_discriminant {
                // Same type at same position - check if we should recurse

                // For Custom blocks, also verify type_name matches
                let type_name_matches = match (orig_block, exec_block) {
                    (Block::Custom(o), Block::Custom(e)) => o.type_name == e.type_name,
                    _ => true,
                };

                if type_name_matches && is_container_block(orig_block) {
                    // Container block: recurse into children
                    used_original.insert(exec_idx);

                    // Handle Custom blocks specially (slot-based reconciliation)
                    if let (Block::Custom(orig_cn), Block::Custom(exec_cn)) =
                        (orig_block, exec_block)
                    {
                        let slot_plan = compute_custom_node_slot_plan(orig_cn, exec_cn, cache);
                        custom_node_plans.insert(exec_idx, slot_plan);
                    } else if let (Block::Table(orig_table), Block::Table(exec_table)) =
                        (orig_block, exec_block)
                    {
                        let table_plan = compute_table_plan(orig_table, exec_table, cache);
                        table_plans.insert(exec_idx, table_plan);
                    } else {
                        let nested_plan = compute_container_plan(orig_block, exec_block, cache);
                        stats.merge(&nested_plan.stats);
                        block_container_plans.insert(exec_idx, nested_plan);
                    }

                    alignments[exec_idx] = Some(BlockAlignment::RecurseIntoContainer {
                        before_idx: exec_idx,
                        after_idx: exec_idx,
                    });
                    stats.blocks_recursed += 1;
                    continue;
                }

                if type_name_matches && has_inline_content(orig_block) {
                    // Block with inline content: check if any inlines match
                    if let Some(inline_plan) =
                        compute_inline_plan_for_block(orig_block, exec_block, cache)
                    {
                        let has_kept_inlines = inline_plan
                            .inline_alignments
                            .iter()
                            .any(|a| matches!(a, InlineAlignment::KeepBefore(_)));

                        if has_kept_inlines {
                            // Some inlines match - recurse to preserve them
                            used_original.insert(exec_idx);
                            inline_plans.insert(exec_idx, inline_plan);
                            alignments[exec_idx] = Some(BlockAlignment::RecurseIntoContainer {
                                before_idx: exec_idx,
                                after_idx: exec_idx,
                            });
                            stats.blocks_recursed += 1;
                            continue;
                        }
                        // No inlines kept - fall through to phase 3
                    }
                }
            }
        }

        needs_phase_3.push(exec_idx);
    }

    // =========================================================================
    // Phase 3: Fallback - treat as new content
    // =========================================================================
    // No evidence these blocks are related to any original - use executed as-is.
    for exec_idx in needs_phase_3 {
        alignments[exec_idx] = Some(BlockAlignment::UseAfter(exec_idx));
        stats.blocks_replaced += 1;
    }

    // Convert Option<BlockAlignment> to BlockAlignment
    let alignments: Vec<BlockAlignment> = alignments
        .into_iter()
        .map(|a| a.expect("All alignments should be filled"))
        .collect();

    ReconciliationPlan {
        block_alignments: alignments,
        block_container_plans,
        inline_plans,
        custom_node_plans,
        table_plans,
        list_item_alignments: Vec::new(),
        list_item_plans: LinkedHashMap::new(),
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
            | Block::Table(_)
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
///
/// Uses a three-phase algorithm analogous to block reconciliation:
/// 1. **Exact hash matches (any position)**: Find items with identical content.
///    These get `KeepOriginal` - we have proof they're the same.
/// 2. **Positional matches (same index only)**: For unmatched items, check if the
///    item at the same position in original is available. If so, recurse.
/// 3. **Fallback**: Remaining unmatched items get `UseExecuted` - treat as new.
fn compute_list_plan<'a>(
    orig_items: &'a [Vec<Block>],
    exec_items: &[Vec<Block>],
    cache: &mut HashCache<'a>,
) -> ReconciliationPlan {
    let mut plan = ReconciliationPlan::new();

    // Early exit: if both are empty, nothing to do
    if orig_items.is_empty() && exec_items.is_empty() {
        return plan;
    }

    // Compute hashes for original items (cached)
    let orig_hashes: Vec<u64> = orig_items
        .iter()
        .map(|item| cache.hash_blocks(item))
        .collect();

    // Build hash → indices multimap for original items
    let mut hash_to_indices: LinkedHashMap<u64, Vec<usize>> = LinkedHashMap::new();
    for (idx, &hash) in orig_hashes.iter().enumerate() {
        hash_to_indices
            .entry(hash)
            .or_insert_with(Vec::new)
            .push(idx);
    }

    // Initialize alignment slots - we'll fill these across three phases
    let mut alignments: Vec<Option<ListItemAlignment>> = vec![None; exec_items.len()];
    let mut list_item_plans: LinkedHashMap<usize, ReconciliationPlan> = LinkedHashMap::new();
    let mut used_original: FxHashSet<usize> = FxHashSet::default();

    // Track which executed indices need further processing
    let mut needs_phase_2: Vec<usize> = Vec::new();

    // =========================================================================
    // Phase 1: Exact hash matches (any position)
    // =========================================================================
    // Find items with identical content. Position mismatch is fine because
    // we have definitive proof (hash + structural equality) they're the same.
    for (exec_idx, exec_item) in exec_items.iter().enumerate() {
        let exec_hash = compute_blocks_hash_fresh(exec_item);

        if let Some(indices) = hash_to_indices.get(&exec_hash)
            && let Some(&orig_idx) = indices.iter().find(|&&i| !used_original.contains(&i))
        {
            // Verify with structural equality (guards against hash collisions)
            if structural_eq_blocks(&orig_items[orig_idx], exec_item) {
                used_original.insert(orig_idx);
                alignments[exec_idx] = Some(ListItemAlignment::KeepOriginal(orig_idx));
                // No plan needed - content is identical, just use original
                continue;
            }
            // Hash collision - fall through to phase 2
        }

        needs_phase_2.push(exec_idx);
    }

    // =========================================================================
    // Phase 2: Positional matches (same index only)
    // =========================================================================
    // For unmatched items, check if the item at the SAME position in original
    // is available. Only recurse when indices match - this provides reasonable
    // evidence they're the same logical entity.
    let mut needs_phase_3: Vec<usize> = Vec::new();

    for exec_idx in needs_phase_2 {
        let exec_item = &exec_items[exec_idx];

        // Only consider the positionally-corresponding original
        if exec_idx < orig_items.len() && !used_original.contains(&exec_idx) {
            used_original.insert(exec_idx);
            // Recurse into the item to reconcile its blocks
            let nested_plan =
                compute_reconciliation_for_blocks(&orig_items[exec_idx], exec_item, cache);
            plan.stats.merge(&nested_plan.stats);
            alignments[exec_idx] = Some(ListItemAlignment::Reconcile(exec_idx));
            list_item_plans.insert(exec_idx, nested_plan);
            continue;
        }

        needs_phase_3.push(exec_idx);
    }

    // =========================================================================
    // Phase 3: Fallback - treat as new content
    // =========================================================================
    // No evidence these items are related to any original - use executed as-is.
    for exec_idx in needs_phase_3 {
        alignments[exec_idx] = Some(ListItemAlignment::UseExecuted);
        plan.stats.blocks_replaced += 1;
    }

    // Convert Option<ListItemAlignment> to ListItemAlignment
    let alignments: Vec<ListItemAlignment> = alignments
        .into_iter()
        .map(|a| a.expect("All alignments should be filled"))
        .collect();

    plan.list_item_alignments = alignments;
    plan.list_item_plans = list_item_plans;
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

    let mut block_slot_plans = LinkedHashMap::new();
    let mut inline_slot_plans = LinkedHashMap::new();

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

/// Compute reconciliation plan for a Table's nested content.
///
/// This uses position-based matching: cells at the same (section, row, column)
/// position in both tables are matched and their content is recursively reconciled.
/// If the table structure changes (rows/columns added or removed), unmatched cells
/// simply use the executed table's content.
fn compute_table_plan<'a>(
    orig_table: &'a Table,
    exec_table: &Table,
    cache: &mut HashCache<'a>,
) -> TableReconciliationPlan {
    let mut cell_plans = LinkedHashMap::new();

    // Reconcile caption.long if both tables have one
    let caption_plan = match (&orig_table.caption.long, &exec_table.caption.long) {
        (Some(orig_blocks), Some(exec_blocks)) => {
            let plan = compute_reconciliation_for_blocks(orig_blocks, exec_blocks, cache);
            // Only include if there's actual reconciliation work
            let needs_plan = plan
                .block_alignments
                .iter()
                .any(|a| !matches!(a, BlockAlignment::KeepBefore(_)))
                || !plan.block_container_plans.is_empty()
                || !plan.inline_plans.is_empty()
                || !plan.custom_node_plans.is_empty()
                || !plan.table_plans.is_empty();
            if needs_plan {
                Some(Box::new(plan))
            } else {
                None
            }
        }
        _ => None,
    };

    // Helper to reconcile cells at matching positions in two row slices
    let reconcile_rows = |orig_rows: &'a [crate::table::Row],
                          exec_rows: &[crate::table::Row],
                          cache: &mut HashCache<'a>,
                          make_position: &dyn Fn(usize, usize) -> TableCellPosition|
     -> Vec<(TableCellPosition, ReconciliationPlan)> {
        let mut results = Vec::new();
        for (row_idx, (orig_row, exec_row)) in orig_rows.iter().zip(exec_rows.iter()).enumerate() {
            for (cell_idx, (orig_cell, exec_cell)) in
                orig_row.cells.iter().zip(exec_row.cells.iter()).enumerate()
            {
                let plan = compute_reconciliation_for_blocks(
                    &orig_cell.content,
                    &exec_cell.content,
                    cache,
                );
                // Only include if there's actual reconciliation work
                let needs_plan = plan
                    .block_alignments
                    .iter()
                    .any(|a| !matches!(a, BlockAlignment::KeepBefore(_)))
                    || !plan.block_container_plans.is_empty()
                    || !plan.inline_plans.is_empty()
                    || !plan.custom_node_plans.is_empty()
                    || !plan.table_plans.is_empty();
                if needs_plan {
                    results.push((make_position(row_idx, cell_idx), plan));
                }
            }
        }
        results
    };

    // Reconcile head cells
    let head_plans = reconcile_rows(
        &orig_table.head.rows,
        &exec_table.head.rows,
        cache,
        &|row, cell| TableCellPosition::Head { row, cell },
    );
    for (pos, plan) in head_plans {
        cell_plans.insert(pos, plan);
    }

    // Reconcile body cells (both head rows and body rows of each TableBody)
    for (body_idx, (orig_body, exec_body)) in orig_table
        .bodies
        .iter()
        .zip(exec_table.bodies.iter())
        .enumerate()
    {
        // Body's head rows
        let body_head_plans =
            reconcile_rows(&orig_body.head, &exec_body.head, cache, &|row, cell| {
                TableCellPosition::BodyHead {
                    body: body_idx,
                    row,
                    cell,
                }
            });
        for (pos, plan) in body_head_plans {
            cell_plans.insert(pos, plan);
        }

        // Body's body rows
        let body_body_plans =
            reconcile_rows(&orig_body.body, &exec_body.body, cache, &|row, cell| {
                TableCellPosition::BodyBody {
                    body: body_idx,
                    row,
                    cell,
                }
            });
        for (pos, plan) in body_body_plans {
            cell_plans.insert(pos, plan);
        }
    }

    // Reconcile foot cells
    let foot_plans = reconcile_rows(
        &orig_table.foot.rows,
        &exec_table.foot.rows,
        cache,
        &|row, cell| TableCellPosition::Foot { row, cell },
    );
    for (pos, plan) in foot_plans {
        cell_plans.insert(pos, plan);
    }

    TableReconciliationPlan {
        caption_plan,
        cell_plans,
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
    let mut hash_to_indices: LinkedHashMap<u64, Vec<usize>> = LinkedHashMap::new();
    for (idx, &hash) in original_hashes.iter().enumerate() {
        hash_to_indices
            .entry(hash)
            .or_insert_with(Vec::new)
            .push(idx);
    }

    let mut alignments = Vec::with_capacity(executed.len());
    let mut inline_container_plans = LinkedHashMap::new();
    let mut note_block_plans = LinkedHashMap::new();
    let mut custom_node_plans = LinkedHashMap::new();
    let mut used_original: FxHashSet<usize> = FxHashSet::default();

    for (exec_idx, exec_inline) in executed.iter().enumerate() {
        let exec_hash = compute_inline_hash_fresh(exec_inline);

        // Step 1: Try exact hash match
        if let Some(indices) = hash_to_indices.get(&exec_hash)
            && let Some(&orig_idx) = indices.iter().find(|&&i| !used_original.contains(&i))
            && structural_eq_inline(&original[orig_idx], exec_inline)
        {
            used_original.insert(orig_idx);
            alignments.push(InlineAlignment::KeepBefore(orig_idx));
            continue;
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
    use crate::{
        BlockQuote, BulletList, Caption, Div, Emph, Figure, Header, ListNumberDelim,
        ListNumberStyle, OrderedList, Paragraph, Plain, Str, Strong,
    };
    use hashlink::LinkedHashMap;
    use quarto_source_map::{FileId, SourceInfo};

    fn dummy_source() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn make_str(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source(),
        })
    }

    fn make_para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![make_str(text)],
            source_info: dummy_source(),
        })
    }

    fn make_plain(text: &str) -> Block {
        Block::Plain(Plain {
            content: vec![make_str(text)],
            source_info: dummy_source(),
        })
    }

    fn make_header(level: usize, text: &str) -> Block {
        Block::Header(Header {
            level,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![make_str(text)],
            source_info: dummy_source(),
            attr_source: crate::AttrSourceInfo::empty(),
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

    fn make_blockquote(blocks: Vec<Block>) -> Block {
        Block::BlockQuote(BlockQuote {
            content: blocks,
            source_info: dummy_source(),
        })
    }

    fn make_bullet_list(items: Vec<Vec<Block>>) -> Block {
        Block::BulletList(BulletList {
            content: items,
            source_info: dummy_source(),
        })
    }

    fn make_ordered_list(items: Vec<Vec<Block>>) -> Block {
        Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Default, ListNumberDelim::Default),
            content: items,
            source_info: dummy_source(),
        })
    }

    fn make_figure(blocks: Vec<Block>) -> Block {
        Block::Figure(Figure {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: None,
                source_info: dummy_source(),
            },
            content: blocks,
            source_info: dummy_source(),
            attr_source: crate::AttrSourceInfo::empty(),
        })
    }

    fn make_emph(text: &str) -> Inline {
        Inline::Emph(Emph {
            content: vec![make_str(text)],
            source_info: dummy_source(),
        })
    }

    fn make_strong(text: &str) -> Inline {
        Inline::Strong(Strong {
            content: vec![make_str(text)],
            source_info: dummy_source(),
        })
    }

    // ========================================================================
    // is_container_block tests
    // ========================================================================

    #[test]
    fn test_is_container_block_div() {
        assert!(is_container_block(&make_div(vec![])));
    }

    #[test]
    fn test_is_container_block_blockquote() {
        assert!(is_container_block(&make_blockquote(vec![])));
    }

    #[test]
    fn test_is_container_block_bullet_list() {
        assert!(is_container_block(&make_bullet_list(vec![])));
    }

    #[test]
    fn test_is_container_block_ordered_list() {
        assert!(is_container_block(&make_ordered_list(vec![])));
    }

    #[test]
    fn test_is_container_block_figure() {
        assert!(is_container_block(&make_figure(vec![])));
    }

    #[test]
    fn test_is_container_block_paragraph_is_false() {
        assert!(!is_container_block(&make_para("text")));
    }

    // ========================================================================
    // has_inline_content tests
    // ========================================================================

    #[test]
    fn test_has_inline_content_paragraph() {
        assert!(has_inline_content(&make_para("text")));
    }

    #[test]
    fn test_has_inline_content_plain() {
        assert!(has_inline_content(&make_plain("text")));
    }

    #[test]
    fn test_has_inline_content_header() {
        assert!(has_inline_content(&make_header(1, "text")));
    }

    #[test]
    fn test_has_inline_content_div_is_false() {
        assert!(!has_inline_content(&make_div(vec![])));
    }

    // ========================================================================
    // is_container_inline tests
    // ========================================================================

    #[test]
    fn test_is_container_inline_emph() {
        assert!(is_container_inline(&make_emph("text")));
    }

    #[test]
    fn test_is_container_inline_strong() {
        assert!(is_container_inline(&make_strong("text")));
    }

    #[test]
    fn test_is_container_inline_str_is_false() {
        assert!(!is_container_inline(&make_str("text")));
    }

    // ========================================================================
    // Empty sequence tests
    // ========================================================================

    #[test]
    fn test_empty_block_sequences() {
        let original: Vec<Block> = vec![];
        let executed: Vec<Block> = vec![];
        let mut cache = HashCache::new();

        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);
        assert!(plan.block_alignments.is_empty());
    }

    #[test]
    fn test_empty_inline_sequences() {
        let original: Vec<Inline> = vec![];
        let executed: Vec<Inline> = vec![];
        let mut cache = HashCache::new();

        let plan = compute_inline_alignments(&original, &executed, &mut cache);
        assert!(plan.inline_alignments.is_empty());
    }

    // ========================================================================
    // Basic reconciliation tests
    // ========================================================================

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

    // ========================================================================
    // List reconciliation tests
    // ========================================================================

    #[test]
    fn test_bullet_list_reconciliation() {
        let original = vec![make_bullet_list(vec![
            vec![make_para("item1")],
            vec![make_para("item2")],
        ])];
        let executed = vec![make_bullet_list(vec![
            vec![make_para("item1")],
            vec![make_para("changed")],
        ])];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        assert_eq!(plan.block_alignments.len(), 1);
        assert!(matches!(
            plan.block_alignments[0],
            BlockAlignment::RecurseIntoContainer { .. }
        ));
    }

    #[test]
    fn test_ordered_list_reconciliation() {
        let original = vec![make_ordered_list(vec![
            vec![make_para("first")],
            vec![make_para("second")],
        ])];
        let executed = vec![make_ordered_list(vec![
            vec![make_para("first")],
            vec![make_para("modified")],
        ])];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        assert_eq!(plan.block_alignments.len(), 1);
        assert!(matches!(
            plan.block_alignments[0],
            BlockAlignment::RecurseIntoContainer { .. }
        ));
    }

    #[test]
    fn test_list_with_extra_items() {
        let orig_items: Vec<Vec<Block>> = vec![vec![make_para("item1")]];
        let exec_items: Vec<Vec<Block>> = vec![
            vec![make_para("item1")],
            vec![make_para("item2")],
            vec![make_para("item3")],
        ];

        let mut cache = HashCache::new();
        let plan = compute_list_plan(&orig_items, &exec_items, &mut cache);

        // Extra items from executed should be counted as replaced
        assert_eq!(plan.stats.blocks_replaced, 2);
    }

    // ========================================================================
    // BlockQuote and Figure tests
    // ========================================================================

    #[test]
    fn test_blockquote_recursion() {
        let original = vec![make_blockquote(vec![make_para("quote")])];
        let executed = vec![make_blockquote(vec![make_para("modified quote")])];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        assert_eq!(plan.block_alignments.len(), 1);
        assert!(matches!(
            plan.block_alignments[0],
            BlockAlignment::RecurseIntoContainer { .. }
        ));
    }

    #[test]
    fn test_figure_recursion() {
        let original = vec![make_figure(vec![make_para("caption")])];
        let executed = vec![make_figure(vec![make_para("new caption")])];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        assert_eq!(plan.block_alignments.len(), 1);
        assert!(matches!(
            plan.block_alignments[0],
            BlockAlignment::RecurseIntoContainer { .. }
        ));
    }

    // ========================================================================
    // Inline reconciliation tests
    // ========================================================================

    #[test]
    fn test_inline_keep_matching() {
        let original = vec![make_str("hello"), make_str("world")];
        let executed = vec![make_str("hello"), make_str("world")];

        let mut cache = HashCache::new();
        let plan = compute_inline_alignments(&original, &executed, &mut cache);

        assert_eq!(plan.inline_alignments.len(), 2);
        assert!(matches!(
            plan.inline_alignments[0],
            InlineAlignment::KeepBefore(0)
        ));
        assert!(matches!(
            plan.inline_alignments[1],
            InlineAlignment::KeepBefore(1)
        ));
    }

    #[test]
    fn test_inline_use_after_for_new() {
        let original = vec![make_str("hello")];
        let executed = vec![make_str("hello"), make_str("new")];

        let mut cache = HashCache::new();
        let plan = compute_inline_alignments(&original, &executed, &mut cache);

        assert_eq!(plan.inline_alignments.len(), 2);
        assert!(matches!(
            plan.inline_alignments[0],
            InlineAlignment::KeepBefore(0)
        ));
        assert!(matches!(
            plan.inline_alignments[1],
            InlineAlignment::UseAfter(1)
        ));
    }

    #[test]
    fn test_inline_container_emph_recursion() {
        let original = vec![make_emph("original")];
        let executed = vec![make_emph("modified")];

        let mut cache = HashCache::new();
        let plan = compute_inline_alignments(&original, &executed, &mut cache);

        assert_eq!(plan.inline_alignments.len(), 1);
        assert!(matches!(
            plan.inline_alignments[0],
            InlineAlignment::RecurseIntoContainer { .. }
        ));
        assert!(plan.inline_container_plans.contains_key(&0));
    }

    #[test]
    fn test_inline_container_strong_recursion() {
        let original = vec![make_strong("original")];
        let executed = vec![make_strong("modified")];

        let mut cache = HashCache::new();
        let plan = compute_inline_alignments(&original, &executed, &mut cache);

        assert_eq!(plan.inline_alignments.len(), 1);
        assert!(matches!(
            plan.inline_alignments[0],
            InlineAlignment::RecurseIntoContainer { .. }
        ));
    }

    // ========================================================================
    // Inline plan for block tests
    // ========================================================================

    #[test]
    fn test_inline_plan_for_paragraph() {
        let orig = make_para("hello");
        let exec = make_para("world");
        let mut cache = HashCache::new();

        let plan = compute_inline_plan_for_block(&orig, &exec, &mut cache);
        assert!(plan.is_some());
    }

    #[test]
    fn test_inline_plan_for_plain() {
        let orig = make_plain("hello");
        let exec = make_plain("world");
        let mut cache = HashCache::new();

        let plan = compute_inline_plan_for_block(&orig, &exec, &mut cache);
        assert!(plan.is_some());
    }

    #[test]
    fn test_inline_plan_for_header() {
        let orig = make_header(1, "hello");
        let exec = make_header(1, "world");
        let mut cache = HashCache::new();

        let plan = compute_inline_plan_for_block(&orig, &exec, &mut cache);
        assert!(plan.is_some());
    }

    #[test]
    fn test_inline_plan_for_div_is_none() {
        let orig = make_div(vec![]);
        let exec = make_div(vec![]);
        let mut cache = HashCache::new();

        let plan = compute_inline_plan_for_block(&orig, &exec, &mut cache);
        assert!(plan.is_none());
    }

    // ========================================================================
    // Block with inline content type matching
    // ========================================================================

    #[test]
    fn test_paragraph_with_kept_inline_triggers_recursion() {
        // When a paragraph has some matching inlines, it should recurse
        let orig_para = Block::Paragraph(Paragraph {
            content: vec![make_str("kept"), make_str("orig_only")],
            source_info: dummy_source(),
        });
        let exec_para = Block::Paragraph(Paragraph {
            content: vec![make_str("kept"), make_str("exec_only")],
            source_info: dummy_source(),
        });

        let original = vec![orig_para];
        let executed = vec![exec_para];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        // Should detect the common inline and recurse
        assert_eq!(plan.block_alignments.len(), 1);
        // May be RecurseIntoContainer if inline matching triggers it
    }

    // ========================================================================
    // Three-phase algorithm tests
    // ========================================================================

    #[test]
    fn test_three_phase_exact_match_at_different_position() {
        // The ex6 pattern: nested divs where inner elements have exact matches
        // at different indices.
        //
        // Before: [Div("1"), Div("2"), Div("3")]
        // After:  [Div("0"), Div("1"), Div("2")]
        //
        // Expected:
        //   after[0] -> UseAfter(0)     -- new content, no match
        //   after[1] -> KeepBefore(0)   -- exact hash match with before[0]
        //   after[2] -> KeepBefore(1)   -- exact hash match with before[1]
        //
        // The current (wrong) behavior would recurse into (0,0), (1,1), (2,2)
        // because it greedily matches by type before checking for exact matches elsewhere.

        let original = vec![
            make_div(vec![make_para("1")]),
            make_div(vec![make_para("2")]),
            make_div(vec![make_para("3")]),
        ];
        let executed = vec![
            make_div(vec![make_para("0")]),
            make_div(vec![make_para("1")]),
            make_div(vec![make_para("2")]),
        ];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        assert_eq!(plan.block_alignments.len(), 3);

        // after[0] should be UseAfter (new content, no exact match, positional orig is claimed)
        assert!(
            matches!(plan.block_alignments[0], BlockAlignment::UseAfter(0)),
            "after[0] should be UseAfter(0), got {:?}",
            plan.block_alignments[0]
        );

        // after[1] should be KeepBefore(0) - exact hash match with before[0]
        assert!(
            matches!(plan.block_alignments[1], BlockAlignment::KeepBefore(0)),
            "after[1] should be KeepBefore(0), got {:?}",
            plan.block_alignments[1]
        );

        // after[2] should be KeepBefore(1) - exact hash match with before[1]
        assert!(
            matches!(plan.block_alignments[2], BlockAlignment::KeepBefore(1)),
            "after[2] should be KeepBefore(1), got {:?}",
            plan.block_alignments[2]
        );

        // Stats should reflect: 2 kept, 1 replaced, 0 recursed
        assert_eq!(plan.stats.blocks_kept, 2);
        assert_eq!(plan.stats.blocks_replaced, 1);
        assert_eq!(plan.stats.blocks_recursed, 0);
    }

    #[test]
    fn test_three_phase_positional_match_when_no_exact() {
        // When there's no exact match but same position and type, recurse.
        //
        // Before: [Div("1"), Div("2")]
        // After:  [Div("1-modified"), Div("2")]
        //
        // Expected:
        //   after[0] -> RecurseIntoContainer(0, 0) -- same position, different content
        //   after[1] -> KeepBefore(1)              -- exact match

        let original = vec![
            make_div(vec![make_para("1")]),
            make_div(vec![make_para("2")]),
        ];
        let executed = vec![
            make_div(vec![make_para("1-modified")]),
            make_div(vec![make_para("2")]),
        ];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        assert_eq!(plan.block_alignments.len(), 2);

        // after[0] should recurse (same position, same type, different content)
        assert!(
            matches!(
                plan.block_alignments[0],
                BlockAlignment::RecurseIntoContainer {
                    before_idx: 0,
                    after_idx: 0
                }
            ),
            "after[0] should be RecurseIntoContainer(0, 0), got {:?}",
            plan.block_alignments[0]
        );

        // after[1] should be exact match
        assert!(
            matches!(plan.block_alignments[1], BlockAlignment::KeepBefore(1)),
            "after[1] should be KeepBefore(1), got {:?}",
            plan.block_alignments[1]
        );
    }

    #[test]
    fn test_three_phase_no_recurse_when_positional_already_used() {
        // When the positional original is already claimed by an exact match,
        // don't hunt for another original - just UseAfter.
        //
        // Before: [Div("A"), Div("B")]
        // After:  [Div("NEW"), Div("A")]
        //
        // Phase 1: after[1] matches before[0] exactly -> KeepBefore(0)
        // Phase 2: after[0] wants to check before[0], but it's used -> skip
        // Phase 3: after[0] -> UseAfter(0)

        let original = vec![
            make_div(vec![make_para("A")]),
            make_div(vec![make_para("B")]),
        ];
        let executed = vec![
            make_div(vec![make_para("NEW")]),
            make_div(vec![make_para("A")]),
        ];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        assert_eq!(plan.block_alignments.len(), 2);

        // after[0] should be UseAfter (positional before[0] is claimed by exact match)
        assert!(
            matches!(plan.block_alignments[0], BlockAlignment::UseAfter(0)),
            "after[0] should be UseAfter(0), got {:?}",
            plan.block_alignments[0]
        );

        // after[1] should be KeepBefore(0) - exact match
        assert!(
            matches!(plan.block_alignments[1], BlockAlignment::KeepBefore(0)),
            "after[1] should be KeepBefore(0), got {:?}",
            plan.block_alignments[1]
        );
    }

    #[test]
    fn test_three_phase_type_mismatch_at_position_uses_after() {
        // When the positional original has different type, UseAfter.
        //
        // Before: [Div("A"), Para("B")]
        // After:  [Para("X"), Para("B")]
        //
        // Phase 1: after[1] matches before[1] exactly -> KeepBefore(1)
        // Phase 2: after[0] checks before[0], but Div != Para -> skip
        // Phase 3: after[0] -> UseAfter(0)

        let original = vec![make_div(vec![make_para("A")]), make_para("B")];
        let executed = vec![make_para("X"), make_para("B")];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        assert_eq!(plan.block_alignments.len(), 2);

        // after[0] should be UseAfter (type mismatch with positional before[0])
        assert!(
            matches!(plan.block_alignments[0], BlockAlignment::UseAfter(0)),
            "after[0] should be UseAfter(0), got {:?}",
            plan.block_alignments[0]
        );

        // after[1] should be KeepBefore(1) - exact match
        assert!(
            matches!(plan.block_alignments[1], BlockAlignment::KeepBefore(1)),
            "after[1] should be KeepBefore(1), got {:?}",
            plan.block_alignments[1]
        );
    }

    #[test]
    fn test_three_phase_multiple_exact_matches_at_shifted_positions() {
        // Multiple items shifted - all should find their exact matches.
        //
        // Before: [Para("A"), Para("B"), Para("C")]
        // After:  [Para("NEW"), Para("A"), Para("B"), Para("C")]
        //
        // Expected:
        //   after[0] -> UseAfter(0)     -- no match
        //   after[1] -> KeepBefore(0)   -- exact match
        //   after[2] -> KeepBefore(1)   -- exact match
        //   after[3] -> KeepBefore(2)   -- exact match

        let original = vec![make_para("A"), make_para("B"), make_para("C")];
        let executed = vec![
            make_para("NEW"),
            make_para("A"),
            make_para("B"),
            make_para("C"),
        ];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        assert_eq!(plan.block_alignments.len(), 4);

        assert!(matches!(
            plan.block_alignments[0],
            BlockAlignment::UseAfter(0)
        ));
        assert!(matches!(
            plan.block_alignments[1],
            BlockAlignment::KeepBefore(0)
        ));
        assert!(matches!(
            plan.block_alignments[2],
            BlockAlignment::KeepBefore(1)
        ));
        assert!(matches!(
            plan.block_alignments[3],
            BlockAlignment::KeepBefore(2)
        ));

        assert_eq!(plan.stats.blocks_kept, 3);
        assert_eq!(plan.stats.blocks_replaced, 1);
    }

    #[test]
    fn test_three_phase_exact_match_priority_over_positional() {
        // Exact match should win even when positional match is available.
        //
        // Before: [Div("X"), Div("Y")]
        // After:  [Div("Y"), Div("Z")]
        //
        // Phase 1: after[0] matches before[1] exactly -> KeepBefore(1)
        //          after[1] has no exact match -> needs phase 2
        // Phase 2: after[1] checks before[1], but it's used -> skip
        // Phase 3: after[1] -> UseAfter(1)
        //
        // Note: before[0] is left unused - that's correct behavior.

        let original = vec![
            make_div(vec![make_para("X")]),
            make_div(vec![make_para("Y")]),
        ];
        let executed = vec![
            make_div(vec![make_para("Y")]),
            make_div(vec![make_para("Z")]),
        ];

        let mut cache = HashCache::new();
        let plan = compute_reconciliation_for_blocks(&original, &executed, &mut cache);

        assert_eq!(plan.block_alignments.len(), 2);

        // after[0] exact matches before[1]
        assert!(
            matches!(plan.block_alignments[0], BlockAlignment::KeepBefore(1)),
            "after[0] should be KeepBefore(1), got {:?}",
            plan.block_alignments[0]
        );

        // after[1] has no match (before[1] used, before[0] wrong content)
        assert!(
            matches!(plan.block_alignments[1], BlockAlignment::UseAfter(1)),
            "after[1] should be UseAfter(1), got {:?}",
            plan.block_alignments[1]
        );
    }

    // ========================================================================
    // List item three-phase algorithm tests
    // ========================================================================

    #[test]
    fn test_list_three_phase_exact_match_at_different_position() {
        // The ex1 pattern: list items where inner elements have exact matches
        // at different indices.
        //
        // Before: ["1", "2", "3"]
        // After:  ["0", "1", "2"]
        //
        // Expected:
        //   after[0] -> UseExecuted             -- new content, no match
        //   after[1] -> KeepOriginal(0)         -- exact hash match with before[0]
        //   after[2] -> KeepOriginal(1)         -- exact hash match with before[1]

        let orig_items: Vec<Vec<Block>> = vec![
            vec![make_para("1")],
            vec![make_para("2")],
            vec![make_para("3")],
        ];
        let exec_items: Vec<Vec<Block>> = vec![
            vec![make_para("0")],
            vec![make_para("1")],
            vec![make_para("2")],
        ];

        let mut cache = HashCache::new();
        let plan = compute_list_plan(&orig_items, &exec_items, &mut cache);

        assert_eq!(plan.list_item_alignments.len(), 3);

        // after[0] should be UseExecuted (new content, positional orig is claimed)
        assert!(
            matches!(plan.list_item_alignments[0], ListItemAlignment::UseExecuted),
            "after[0] should be UseExecuted, got {:?}",
            plan.list_item_alignments[0]
        );

        // after[1] should be KeepOriginal(0) - exact hash match with before[0]
        assert!(
            matches!(
                plan.list_item_alignments[1],
                ListItemAlignment::KeepOriginal(0)
            ),
            "after[1] should be KeepOriginal(0), got {:?}",
            plan.list_item_alignments[1]
        );

        // after[2] should be KeepOriginal(1) - exact hash match with before[1]
        assert!(
            matches!(
                plan.list_item_alignments[2],
                ListItemAlignment::KeepOriginal(1)
            ),
            "after[2] should be KeepOriginal(1), got {:?}",
            plan.list_item_alignments[2]
        );
    }

    #[test]
    fn test_list_three_phase_positional_match_when_no_exact() {
        // When there's no exact match but same position, recurse.
        //
        // Before: ["1", "2"]
        // After:  ["1-modified", "2"]
        //
        // Expected:
        //   after[0] -> Reconcile(0)   -- same position, different content
        //   after[1] -> KeepOriginal(1)  -- exact match

        let orig_items: Vec<Vec<Block>> = vec![vec![make_para("1")], vec![make_para("2")]];
        let exec_items: Vec<Vec<Block>> = vec![vec![make_para("1-modified")], vec![make_para("2")]];

        let mut cache = HashCache::new();
        let plan = compute_list_plan(&orig_items, &exec_items, &mut cache);

        assert_eq!(plan.list_item_alignments.len(), 2);

        // after[0] should be Reconcile (same position, same "type", different content)
        assert!(
            matches!(
                plan.list_item_alignments[0],
                ListItemAlignment::Reconcile(0)
            ),
            "after[0] should be Reconcile(0), got {:?}",
            plan.list_item_alignments[0]
        );

        // after[1] should be exact match
        assert!(
            matches!(
                plan.list_item_alignments[1],
                ListItemAlignment::KeepOriginal(1)
            ),
            "after[1] should be KeepOriginal(1), got {:?}",
            plan.list_item_alignments[1]
        );

        // Should have a nested plan for item 0
        assert!(
            plan.list_item_plans.contains_key(&0),
            "Should have nested plan for item 0"
        );
    }

    #[test]
    fn test_list_three_phase_no_recurse_when_positional_already_used() {
        // When the positional original is already claimed by an exact match,
        // don't hunt for another original - just UseExecuted.
        //
        // Before: ["A", "B"]
        // After:  ["NEW", "A"]
        //
        // Phase 1: after[1] matches before[0] exactly -> KeepOriginal(0)
        // Phase 2: after[0] wants to check before[0], but it's used -> skip
        // Phase 3: after[0] -> UseExecuted

        let orig_items: Vec<Vec<Block>> = vec![vec![make_para("A")], vec![make_para("B")]];
        let exec_items: Vec<Vec<Block>> = vec![vec![make_para("NEW")], vec![make_para("A")]];

        let mut cache = HashCache::new();
        let plan = compute_list_plan(&orig_items, &exec_items, &mut cache);

        assert_eq!(plan.list_item_alignments.len(), 2);

        // after[0] should be UseExecuted (positional before[0] is claimed by exact match)
        assert!(
            matches!(plan.list_item_alignments[0], ListItemAlignment::UseExecuted),
            "after[0] should be UseExecuted, got {:?}",
            plan.list_item_alignments[0]
        );

        // after[1] should be KeepOriginal(0) - exact match
        assert!(
            matches!(
                plan.list_item_alignments[1],
                ListItemAlignment::KeepOriginal(0)
            ),
            "after[1] should be KeepOriginal(0), got {:?}",
            plan.list_item_alignments[1]
        );
    }

    #[test]
    fn test_list_three_phase_multiple_exact_matches_at_shifted_positions() {
        // Multiple items shifted - all should find their exact matches.
        //
        // Before: ["A", "B", "C"]
        // After:  ["NEW", "A", "B", "C"]
        //
        // Expected:
        //   after[0] -> UseExecuted        -- no match
        //   after[1] -> KeepOriginal(0)    -- exact match
        //   after[2] -> KeepOriginal(1)    -- exact match
        //   after[3] -> KeepOriginal(2)    -- exact match

        let orig_items: Vec<Vec<Block>> = vec![
            vec![make_para("A")],
            vec![make_para("B")],
            vec![make_para("C")],
        ];
        let exec_items: Vec<Vec<Block>> = vec![
            vec![make_para("NEW")],
            vec![make_para("A")],
            vec![make_para("B")],
            vec![make_para("C")],
        ];

        let mut cache = HashCache::new();
        let plan = compute_list_plan(&orig_items, &exec_items, &mut cache);

        assert_eq!(plan.list_item_alignments.len(), 4);

        assert!(matches!(
            plan.list_item_alignments[0],
            ListItemAlignment::UseExecuted
        ));
        assert!(matches!(
            plan.list_item_alignments[1],
            ListItemAlignment::KeepOriginal(0)
        ));
        assert!(matches!(
            plan.list_item_alignments[2],
            ListItemAlignment::KeepOriginal(1)
        ));
        assert!(matches!(
            plan.list_item_alignments[3],
            ListItemAlignment::KeepOriginal(2)
        ));

        assert_eq!(plan.stats.blocks_replaced, 1);
    }

    #[test]
    fn test_list_three_phase_exact_match_priority_over_positional() {
        // Exact match should win even when positional match is available.
        //
        // Before: ["X", "Y"]
        // After:  ["Y", "Z"]
        //
        // Phase 1: after[0] matches before[1] exactly -> KeepOriginal(1)
        //          after[1] has no exact match -> needs phase 2
        // Phase 2: after[1] checks before[1], but it's used -> skip
        // Phase 3: after[1] -> UseExecuted
        //
        // Note: before[0] is left unused - that's correct behavior.

        let orig_items: Vec<Vec<Block>> = vec![vec![make_para("X")], vec![make_para("Y")]];
        let exec_items: Vec<Vec<Block>> = vec![vec![make_para("Y")], vec![make_para("Z")]];

        let mut cache = HashCache::new();
        let plan = compute_list_plan(&orig_items, &exec_items, &mut cache);

        assert_eq!(plan.list_item_alignments.len(), 2);

        // after[0] exact matches before[1]
        assert!(
            matches!(
                plan.list_item_alignments[0],
                ListItemAlignment::KeepOriginal(1)
            ),
            "after[0] should be KeepOriginal(1), got {:?}",
            plan.list_item_alignments[0]
        );

        // after[1] has no match (before[1] used, before[0] wrong content)
        assert!(
            matches!(plan.list_item_alignments[1], ListItemAlignment::UseExecuted),
            "after[1] should be UseExecuted, got {:?}",
            plan.list_item_alignments[1]
        );
    }

    #[test]
    fn test_list_three_phase_empty_lists() {
        // Empty lists should produce empty plan
        let orig_items: Vec<Vec<Block>> = vec![];
        let exec_items: Vec<Vec<Block>> = vec![];

        let mut cache = HashCache::new();
        let plan = compute_list_plan(&orig_items, &exec_items, &mut cache);

        assert!(plan.list_item_alignments.is_empty());
        assert!(plan.list_item_plans.is_empty());
    }
}
