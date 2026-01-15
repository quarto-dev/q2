/*
 * types.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Types for AST reconciliation plans.
 */

use hashlink::LinkedHashMap;
use serde::{Deserialize, Serialize};

/// Plan for reconciling a CustomNode's slots.
///
/// CustomNodes have named slots that can contain blocks or inlines.
/// This plan describes how to reconcile each slot's content, using
/// slot names as keys (analogous to React's key-based reconciliation).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CustomNodeSlotPlan {
    /// Plans for slots containing blocks (Slot::Block or Slot::Blocks).
    /// Key: slot name.
    /// Absence means either:
    /// - Content is identical (use original slot entirely)
    /// - Slot doesn't exist in original (use executed slot)
    /// - Slot type changed (use executed slot)
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub block_slot_plans: LinkedHashMap<String, ReconciliationPlan>,

    /// Plans for slots containing inlines (Slot::Inline or Slot::Inlines).
    /// Key: slot name.
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub inline_slot_plans: LinkedHashMap<String, InlineReconciliationPlan>,
}

/// Position of a cell within a table.
///
/// Tables have a complex nested structure: head, multiple bodies, and foot.
/// Each section contains rows, and each row contains cells. This enum
/// identifies a cell's position using indices at each level.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableCellPosition {
    /// Cell in the table head section.
    Head { row: usize, cell: usize },
    /// Cell in the head rows of a table body (TableBody.head).
    BodyHead {
        body: usize,
        row: usize,
        cell: usize,
    },
    /// Cell in the body rows of a table body (TableBody.body).
    BodyBody {
        body: usize,
        row: usize,
        cell: usize,
    },
    /// Cell in the table foot section.
    Foot { row: usize, cell: usize },
}

/// Plan for reconciling a Table's nested content.
///
/// Tables contain nested block content in cells. This plan describes how to
/// reconcile cell content using position-based matching: cells at the same
/// (section, row, column) position in both tables are matched and their
/// content is recursively reconciled.
///
/// # Position-Based Matching
///
/// Unlike list items which can be reordered, table cells have semantic positions.
/// A cell at row 2, column 3 in the original table corresponds to row 2, column 3
/// in the executed table. If the table structure changes (rows/columns added or
/// removed), unmatched cells simply use the executed table's content.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableReconciliationPlan {
    /// Plan for the caption's long content (caption.long: Option<Blocks>).
    /// Only present if both tables have long captions.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub caption_plan: Option<Box<ReconciliationPlan>>,

    /// Plans for cell content, keyed by cell position.
    /// Only contains entries for cells that exist in both tables at the same position.
    /// Absence means either:
    /// - Cell doesn't exist in original (use executed cell entirely)
    /// - Cell doesn't exist in executed (cell is gone)
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub cell_plans: LinkedHashMap<TableCellPosition, ReconciliationPlan>,
}

/// Alignment decision for a single block in the result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockAlignment {
    /// Keep the before block (hashes matched exactly).
    /// Action: MOVE from before (preserves source location, zero-copy).
    #[serde(rename = "use_before")]
    KeepBefore(usize), // Index into before blocks

    /// Use the after block (no match found).
    /// Action: MOVE from after (gets engine output source location, zero-copy).
    #[serde(rename = "use_after")]
    UseAfter(usize), // Index into after blocks

    /// Container with same type but different hash (children changed).
    /// Action: MOVE container from before, but recurse into children.
    /// The nested ReconciliationPlan specifies how to reconcile children.
    #[serde(rename = "recurse")]
    RecurseIntoContainer { before_idx: usize, after_idx: usize },
}

/// Alignment decision for a single inline in the result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InlineAlignment {
    /// Keep the before inline (hashes matched exactly).
    #[serde(rename = "use_before")]
    KeepBefore(usize),

    /// Use the after inline (no match found).
    #[serde(rename = "use_after")]
    UseAfter(usize),

    /// Container inline (Emph, Strong, Link, etc.) with changed children.
    #[serde(rename = "recurse")]
    RecurseIntoContainer { before_idx: usize, after_idx: usize },
}

/// Statistics about the reconciliation process.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReconciliationStats {
    pub blocks_kept: usize,
    pub blocks_replaced: usize,
    pub blocks_recursed: usize,
    pub inlines_kept: usize,
    pub inlines_replaced: usize,
    pub inlines_recursed: usize,
}

impl ReconciliationStats {
    /// Merge another stats into this one.
    pub fn merge(&mut self, other: &ReconciliationStats) {
        self.blocks_kept += other.blocks_kept;
        self.blocks_replaced += other.blocks_replaced;
        self.blocks_recursed += other.blocks_recursed;
        self.inlines_kept += other.inlines_kept;
        self.inlines_replaced += other.inlines_replaced;
        self.inlines_recursed += other.inlines_recursed;
    }
}

/// Plan for reconciling inline content.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InlineReconciliationPlan {
    /// Alignment decisions for each inline in the result.
    pub inline_alignments: Vec<InlineAlignment>,

    /// Nested plans for inline containers (Emph, Strong, Link, etc.).
    /// Key: index into inline_alignments where alignment is RecurseIntoContainer.
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub inline_container_plans: LinkedHashMap<usize, InlineReconciliationPlan>,

    /// For Note inlines, which contain Blocks.
    /// Key: index into inline_alignments.
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub note_block_plans: LinkedHashMap<usize, ReconciliationPlan>,

    /// Plans for inline CustomNode slots (Inline::Custom).
    /// Key: index into inline_alignments where alignment is RecurseIntoContainer
    /// and the inline is a Custom node.
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub custom_node_plans: LinkedHashMap<usize, CustomNodeSlotPlan>,
}

/// Complete plan for reconciling a Pandoc AST.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReconciliationPlan {
    /// Block-level alignments for this scope.
    pub block_alignments: Vec<BlockAlignment>,

    /// Nested plans for block containers (Div, BlockQuote, etc.).
    /// Key: index into block_alignments where alignment is RecurseIntoContainer.
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub block_container_plans: LinkedHashMap<usize, ReconciliationPlan>,

    /// Inline plans for blocks with inline content (Paragraph, Header, etc.).
    /// Key: index into block_alignments.
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub inline_plans: LinkedHashMap<usize, InlineReconciliationPlan>,

    /// Plans for CustomNode slots (Block::Custom).
    /// Key: index into block_alignments where alignment is RecurseIntoContainer
    /// and the block is a Custom node.
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub custom_node_plans: LinkedHashMap<usize, CustomNodeSlotPlan>,

    /// Plans for Table cell content (Block::Table).
    /// Key: index into block_alignments where alignment is RecurseIntoContainer
    /// and the block is a Table.
    #[serde(skip_serializing_if = "LinkedHashMap::is_empty", default)]
    pub table_plans: LinkedHashMap<usize, TableReconciliationPlan>,

    /// Per-item plans for lists (BulletList, OrderedList).
    /// Each entry corresponds to an item in the executed list.
    /// Used when this plan represents a list container's reconciliation.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub list_item_plans: Vec<ReconciliationPlan>,

    /// Diagnostics.
    pub stats: ReconciliationStats,
}

impl ReconciliationPlan {
    /// Create an empty plan.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a plan where all blocks are kept from before.
    pub fn all_kept(count: usize) -> Self {
        Self {
            block_alignments: (0..count).map(BlockAlignment::KeepBefore).collect(),
            stats: ReconciliationStats {
                blocks_kept: count,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

impl InlineReconciliationPlan {
    /// Create an empty plan.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a plan where all inlines are kept from before.
    pub fn all_kept(count: usize) -> Self {
        Self {
            inline_alignments: (0..count).map(InlineAlignment::KeepBefore).collect(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== ReconciliationStats Tests ====================

    #[test]
    fn test_stats_default() {
        let stats = ReconciliationStats::default();
        assert_eq!(stats.blocks_kept, 0);
        assert_eq!(stats.blocks_replaced, 0);
        assert_eq!(stats.blocks_recursed, 0);
        assert_eq!(stats.inlines_kept, 0);
        assert_eq!(stats.inlines_replaced, 0);
        assert_eq!(stats.inlines_recursed, 0);
    }

    #[test]
    fn test_stats_merge() {
        let mut stats1 = ReconciliationStats {
            blocks_kept: 5,
            blocks_replaced: 2,
            blocks_recursed: 1,
            inlines_kept: 10,
            inlines_replaced: 3,
            inlines_recursed: 2,
        };

        let stats2 = ReconciliationStats {
            blocks_kept: 3,
            blocks_replaced: 1,
            blocks_recursed: 2,
            inlines_kept: 5,
            inlines_replaced: 2,
            inlines_recursed: 1,
        };

        stats1.merge(&stats2);

        assert_eq!(stats1.blocks_kept, 8);
        assert_eq!(stats1.blocks_replaced, 3);
        assert_eq!(stats1.blocks_recursed, 3);
        assert_eq!(stats1.inlines_kept, 15);
        assert_eq!(stats1.inlines_replaced, 5);
        assert_eq!(stats1.inlines_recursed, 3);
    }

    #[test]
    fn test_stats_merge_with_empty() {
        let mut stats = ReconciliationStats {
            blocks_kept: 10,
            blocks_replaced: 5,
            blocks_recursed: 3,
            inlines_kept: 20,
            inlines_replaced: 8,
            inlines_recursed: 4,
        };

        let empty = ReconciliationStats::default();
        stats.merge(&empty);

        // Values should remain unchanged
        assert_eq!(stats.blocks_kept, 10);
        assert_eq!(stats.blocks_replaced, 5);
        assert_eq!(stats.blocks_recursed, 3);
        assert_eq!(stats.inlines_kept, 20);
        assert_eq!(stats.inlines_replaced, 8);
        assert_eq!(stats.inlines_recursed, 4);
    }

    // ==================== ReconciliationPlan Tests ====================

    #[test]
    fn test_plan_new() {
        let plan = ReconciliationPlan::new();
        assert!(plan.block_alignments.is_empty());
        assert!(plan.block_container_plans.is_empty());
        assert!(plan.inline_plans.is_empty());
        assert!(plan.custom_node_plans.is_empty());
        assert_eq!(plan.stats, ReconciliationStats::default());
    }

    #[test]
    fn test_plan_default() {
        let plan: ReconciliationPlan = Default::default();
        assert!(plan.block_alignments.is_empty());
        assert!(plan.block_container_plans.is_empty());
    }

    #[test]
    fn test_plan_all_kept() {
        let plan = ReconciliationPlan::all_kept(5);

        assert_eq!(plan.block_alignments.len(), 5);
        for (i, alignment) in plan.block_alignments.iter().enumerate() {
            assert_eq!(*alignment, BlockAlignment::KeepBefore(i));
        }

        assert_eq!(plan.stats.blocks_kept, 5);
        assert_eq!(plan.stats.blocks_replaced, 0);
        assert_eq!(plan.stats.blocks_recursed, 0);
    }

    #[test]
    fn test_plan_all_kept_zero() {
        let plan = ReconciliationPlan::all_kept(0);
        assert!(plan.block_alignments.is_empty());
        assert_eq!(plan.stats.blocks_kept, 0);
    }

    // ==================== InlineReconciliationPlan Tests ====================

    #[test]
    fn test_inline_plan_new() {
        let plan = InlineReconciliationPlan::new();
        assert!(plan.inline_alignments.is_empty());
        assert!(plan.inline_container_plans.is_empty());
        assert!(plan.note_block_plans.is_empty());
        assert!(plan.custom_node_plans.is_empty());
    }

    #[test]
    fn test_inline_plan_default() {
        let plan: InlineReconciliationPlan = Default::default();
        assert!(plan.inline_alignments.is_empty());
    }

    #[test]
    fn test_inline_plan_all_kept() {
        let plan = InlineReconciliationPlan::all_kept(3);

        assert_eq!(plan.inline_alignments.len(), 3);
        for (i, alignment) in plan.inline_alignments.iter().enumerate() {
            assert_eq!(*alignment, InlineAlignment::KeepBefore(i));
        }
    }

    #[test]
    fn test_inline_plan_all_kept_zero() {
        let plan = InlineReconciliationPlan::all_kept(0);
        assert!(plan.inline_alignments.is_empty());
    }

    // ==================== CustomNodeSlotPlan Tests ====================

    #[test]
    fn test_custom_node_slot_plan_default() {
        let plan: CustomNodeSlotPlan = Default::default();
        assert!(plan.block_slot_plans.is_empty());
        assert!(plan.inline_slot_plans.is_empty());
    }

    // ==================== Alignment Enum Tests ====================

    #[test]
    fn test_block_alignment_keep_before() {
        let alignment = BlockAlignment::KeepBefore(5);
        match alignment {
            BlockAlignment::KeepBefore(idx) => assert_eq!(idx, 5),
            _ => panic!("Expected KeepBefore"),
        }
    }

    #[test]
    fn test_block_alignment_use_after() {
        let alignment = BlockAlignment::UseAfter(10);
        match alignment {
            BlockAlignment::UseAfter(idx) => assert_eq!(idx, 10),
            _ => panic!("Expected UseAfter"),
        }
    }

    #[test]
    fn test_block_alignment_recurse() {
        let alignment = BlockAlignment::RecurseIntoContainer {
            before_idx: 2,
            after_idx: 3,
        };
        match alignment {
            BlockAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => {
                assert_eq!(before_idx, 2);
                assert_eq!(after_idx, 3);
            }
            _ => panic!("Expected RecurseIntoContainer"),
        }
    }

    #[test]
    fn test_inline_alignment_keep_before() {
        let alignment = InlineAlignment::KeepBefore(7);
        match alignment {
            InlineAlignment::KeepBefore(idx) => assert_eq!(idx, 7),
            _ => panic!("Expected KeepBefore"),
        }
    }

    #[test]
    fn test_inline_alignment_use_after() {
        let alignment = InlineAlignment::UseAfter(15);
        match alignment {
            InlineAlignment::UseAfter(idx) => assert_eq!(idx, 15),
            _ => panic!("Expected UseAfter"),
        }
    }

    #[test]
    fn test_inline_alignment_recurse() {
        let alignment = InlineAlignment::RecurseIntoContainer {
            before_idx: 4,
            after_idx: 5,
        };
        match alignment {
            InlineAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => {
                assert_eq!(before_idx, 4);
                assert_eq!(after_idx, 5);
            }
            _ => panic!("Expected RecurseIntoContainer"),
        }
    }

    // ==================== Serialization Tests ====================

    #[test]
    fn test_block_alignment_serialization() {
        let alignment = BlockAlignment::KeepBefore(3);
        let json = serde_json::to_string(&alignment).unwrap();
        assert!(json.contains("use_before"));

        let deserialized: BlockAlignment = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, alignment);
    }

    #[test]
    fn test_inline_alignment_serialization() {
        let alignment = InlineAlignment::UseAfter(5);
        let json = serde_json::to_string(&alignment).unwrap();
        assert!(json.contains("use_after"));

        let deserialized: InlineAlignment = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, alignment);
    }

    #[test]
    fn test_reconciliation_plan_serialization() {
        let plan = ReconciliationPlan::all_kept(2);
        let json = serde_json::to_string(&plan).unwrap();
        let deserialized: ReconciliationPlan = serde_json::from_str(&json).unwrap();

        assert_eq!(
            deserialized.block_alignments.len(),
            plan.block_alignments.len()
        );
        assert_eq!(deserialized.stats.blocks_kept, plan.stats.blocks_kept);
    }

    #[test]
    fn test_inline_plan_serialization() {
        let plan = InlineReconciliationPlan::all_kept(3);
        let json = serde_json::to_string(&plan).unwrap();
        let deserialized: InlineReconciliationPlan = serde_json::from_str(&json).unwrap();

        assert_eq!(
            deserialized.inline_alignments.len(),
            plan.inline_alignments.len()
        );
    }

    #[test]
    fn test_stats_equality() {
        let stats1 = ReconciliationStats {
            blocks_kept: 5,
            blocks_replaced: 2,
            blocks_recursed: 1,
            inlines_kept: 10,
            inlines_replaced: 3,
            inlines_recursed: 2,
        };

        let stats2 = ReconciliationStats {
            blocks_kept: 5,
            blocks_replaced: 2,
            blocks_recursed: 1,
            inlines_kept: 10,
            inlines_replaced: 3,
            inlines_recursed: 2,
        };

        let stats3 = ReconciliationStats {
            blocks_kept: 6, // Different
            blocks_replaced: 2,
            blocks_recursed: 1,
            inlines_kept: 10,
            inlines_replaced: 3,
            inlines_recursed: 2,
        };

        assert_eq!(stats1, stats2);
        assert_ne!(stats1, stats3);
    }

    #[test]
    fn test_empty_plan_serialization_skips_empty_maps() {
        let plan = ReconciliationPlan::new();
        let json = serde_json::to_string(&plan).unwrap();

        // Empty HashMaps should be skipped due to skip_serializing_if
        assert!(!json.contains("block_container_plans"));
        assert!(!json.contains("inline_plans"));
        assert!(!json.contains("custom_node_plans"));
    }

    #[test]
    fn test_custom_node_slot_plan_serialization() {
        let plan = CustomNodeSlotPlan::default();
        let json = serde_json::to_string(&plan).unwrap();
        let deserialized: CustomNodeSlotPlan = serde_json::from_str(&json).unwrap();

        assert!(deserialized.block_slot_plans.is_empty());
        assert!(deserialized.inline_slot_plans.is_empty());
    }
}
