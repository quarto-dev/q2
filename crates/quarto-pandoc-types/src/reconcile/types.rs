/*
 * types.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Types for AST reconciliation plans.
 */

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

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
    RecurseIntoContainer {
        before_idx: usize,
        after_idx: usize,
    },
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
    RecurseIntoContainer {
        before_idx: usize,
        after_idx: usize,
    },
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
    #[serde(skip_serializing_if = "FxHashMap::is_empty", default)]
    pub inline_container_plans: FxHashMap<usize, InlineReconciliationPlan>,

    /// For Note inlines, which contain Blocks.
    /// Key: index into inline_alignments.
    #[serde(skip_serializing_if = "FxHashMap::is_empty", default)]
    pub note_block_plans: FxHashMap<usize, ReconciliationPlan>,
}

/// Complete plan for reconciling a Pandoc AST.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReconciliationPlan {
    /// Block-level alignments for this scope.
    pub block_alignments: Vec<BlockAlignment>,

    /// Nested plans for block containers (Div, BlockQuote, etc.).
    /// Key: index into block_alignments where alignment is RecurseIntoContainer.
    #[serde(skip_serializing_if = "FxHashMap::is_empty", default)]
    pub block_container_plans: FxHashMap<usize, ReconciliationPlan>,

    /// Inline plans for blocks with inline content (Paragraph, Header, etc.).
    /// Key: index into block_alignments.
    #[serde(skip_serializing_if = "FxHashMap::is_empty", default)]
    pub inline_plans: FxHashMap<usize, InlineReconciliationPlan>,

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
