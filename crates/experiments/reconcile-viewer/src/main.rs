/*
 * reconcile-viewer
 * Copyright (c) 2025 Posit, PBC
 *
 * Experimental tool for viewing QMD reconciliation plans in a human-readable JSON format.
 * Unlike the raw reconciliation plan output, this shows actual content snippets
 * alongside the alignment decisions.
 */
#![feature(trim_prefix_suffix)]

use clap::Parser;
use hashlink::LinkedHashMap;
use pampa::readers;
use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::inline::{Inline, Inlines};
use quarto_pandoc_types::reconcile::{
    BlockAlignment, InlineAlignment, InlineReconciliationPlan, ListItemAlignment,
    ReconciliationPlan, compute_reconciliation,
};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Parser, Debug)]
#[command(name = "reconcile-viewer")]
#[command(about = "View QMD reconciliation plans in human-readable JSON format")]
struct Args {
    /// The first qmd file (before)
    #[arg(short = 'b', long = "before")]
    before: String,

    /// The second qmd file (after)
    #[arg(short = 'a', long = "after")]
    after: String,

    /// Maximum content snippet length (default: 60)
    #[arg(short = 's', long = "snippet-len", default_value = "60")]
    snippet_len: usize,
}

/// Human-readable reconciliation report
#[derive(Serialize, Deserialize, Clone, Debug)]
struct ReadableReport {
    before_file: String,
    after_file: String,
    block_operations: Vec<ReadableBlockOp>,
}

/// Human-readable block operation
#[derive(Serialize, Deserialize, Clone, Debug)]
struct ReadableBlockOp {
    /// Position in the result
    result_index: usize,
    /// The action taken
    action: String,
    /// Type of block in result
    block_type: String,
    /// Index in the before blocks (for keep_before and recurse)
    #[serde(skip_serializing_if = "Option::is_none")]
    before_idx: Option<usize>,
    /// Index in the after blocks (for use_after and recurse)
    #[serde(skip_serializing_if = "Option::is_none")]
    after_idx: Option<usize>,
    /// Nested block operations (for containers like Div, BlockQuote)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    nested_block_ops: Vec<ReadableBlockOp>,
    /// Per-item operations for lists (BulletList, OrderedList)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    list_item_ops: Vec<ReadableListItemOp>,
    /// Nested inline operations
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    inline_ops: Vec<ReadableInlineOp>,
}

/// Human-readable list item operation
#[derive(Serialize, Deserialize, Clone, Debug)]
struct ReadableListItemOp {
    /// Index in the result list
    result_index: usize,
    /// Action taken for this list item (keep_original, reconcile, use_executed)
    action: String,
    /// Index in the before list (if item existed in before)
    #[serde(skip_serializing_if = "Option::is_none")]
    before_idx: Option<usize>,
    /// Block operations within this list item (only for reconcile action)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    block_ops: Vec<ReadableBlockOp>,
}

/// Human-readable inline operation
#[derive(Serialize, Deserialize, Clone, Debug)]
struct ReadableInlineOp {
    result_index: usize,
    action: String,
    inline_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    before_idx: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after_idx: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    nested_inline_ops: Vec<ReadableInlineOp>,
}

/// Get the type name of a block
fn block_type_name(block: &Block) -> &'static str {
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

/// Get the type name of an inline
fn inline_type_name(inline: &Inline) -> &'static str {
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
        Inline::Attr(_, _) => "Attr",
        Inline::Insert(_) => "Insert",
        Inline::Delete(_) => "Delete",
        Inline::Highlight(_) => "Highlight",
        Inline::EditComment(_) => "EditComment",
        Inline::Custom(_) => "Custom",
    }
}

/// Build readable inline operations from a plan
fn build_inline_ops(
    plan: &InlineReconciliationPlan,
    before_inlines: &Inlines,
    after_inlines: &Inlines,
    snippet_len: usize,
) -> Vec<ReadableInlineOp> {
    let mut ops = Vec::new();

    for (result_idx, alignment) in plan.inline_alignments.iter().enumerate() {
        let (action, before_idx, after_idx) = match alignment {
            InlineAlignment::KeepBefore(idx) => ("keep_before", Some(*idx), None),
            InlineAlignment::UseAfter(idx) => ("use_after", None, Some(*idx)),
            InlineAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => ("recurse", Some(*before_idx), Some(*after_idx)),
        };

        let before_inline = before_idx.and_then(|i| before_inlines.get(i));
        let after_inline = after_idx.and_then(|i| after_inlines.get(i));

        let inline_type = before_inline
            .or(after_inline)
            .map(inline_type_name)
            .unwrap_or("Unknown");

        // Handle nested inline plans for containers
        let nested_inline_ops =
            if let Some(nested_plan) = plan.inline_container_plans.get(&result_idx) {
                let nested_before = before_inline.map(get_inline_children).unwrap_or_default();
                let nested_after = after_inline.map(get_inline_children).unwrap_or_default();
                build_inline_ops(nested_plan, &nested_before, &nested_after, snippet_len)
            } else {
                Vec::new()
            };

        ops.push(ReadableInlineOp {
            result_index: result_idx,
            action: action.to_string(),
            inline_type: inline_type.to_string(),
            before_idx,
            after_idx,
            nested_inline_ops,
        });
    }

    ops
}

/// Get children of an inline container
fn get_inline_children(inline: &Inline) -> Inlines {
    match inline {
        Inline::Emph(e) => e.content.clone(),
        Inline::Strong(s) => s.content.clone(),
        Inline::Underline(u) => u.content.clone(),
        Inline::Strikeout(s) => s.content.clone(),
        Inline::Superscript(s) => s.content.clone(),
        Inline::Subscript(s) => s.content.clone(),
        Inline::SmallCaps(s) => s.content.clone(),
        Inline::Quoted(q) => q.content.clone(),
        Inline::Link(l) => l.content.clone(),
        Inline::Image(i) => i.content.clone(),
        Inline::Span(s) => s.content.clone(),
        Inline::Insert(i) => i.content.clone(),
        Inline::Delete(d) => d.content.clone(),
        Inline::Highlight(h) => h.content.clone(),
        Inline::EditComment(c) => c.content.clone(),
        _ => Vec::new(),
    }
}

/// Get inline content of a block (for Paragraph, Plain, Header, etc.)
fn get_block_inlines(block: &Block) -> Option<&Inlines> {
    match block {
        Block::Plain(p) => Some(&p.content),
        Block::Paragraph(p) => Some(&p.content),
        Block::Header(h) => Some(&h.content),
        _ => None,
    }
}

/// Get children of a container block
fn get_block_children(block: &Block) -> Blocks {
    match block {
        Block::BlockQuote(b) => b.content.clone(),
        Block::Div(d) => d.content.clone(),
        Block::Figure(f) => f.content.clone(),
        _ => Vec::new(),
    }
}

/// Get list items from a block (for BulletList and OrderedList)
fn get_list_items(block: &Block) -> Option<&Vec<Vec<Block>>> {
    match block {
        Block::BulletList(l) => Some(&l.content),
        Block::OrderedList(l) => Some(&l.content),
        _ => None,
    }
}

/// Build readable list item operations from list_item_alignments
fn build_list_item_ops(
    plan: &ReconciliationPlan,
    before_items: Option<&Vec<Vec<Block>>>,
    after_items: Option<&Vec<Vec<Block>>>,
    snippet_len: usize,
) -> Vec<ReadableListItemOp> {
    let mut ops = Vec::new();

    // Each entry in list_item_alignments corresponds to an item in the after list
    for (result_idx, alignment) in plan.list_item_alignments.iter().enumerate() {
        let after_item = after_items.and_then(|items| items.get(result_idx));

        let (action, before_idx, block_ops) = match alignment {
            ListItemAlignment::KeepOriginal(orig_idx) => {
                // Exact match - use original item entirely
                ("keep_original".to_string(), Some(*orig_idx), Vec::new())
            }
            ListItemAlignment::Reconcile(orig_idx) => {
                // Need to reconcile - get nested plan if available
                let before_item = before_items.and_then(|items| items.get(*orig_idx));
                let nested_ops = if let Some(item_plan) = plan.list_item_plans.get(&result_idx) {
                    build_block_ops(
                        item_plan,
                        before_item.map(|v| v.as_slice()).unwrap_or(&[]),
                        after_item.map(|v| v.as_slice()).unwrap_or(&[]),
                        snippet_len,
                    )
                } else {
                    Vec::new()
                };
                ("reconcile".to_string(), Some(*orig_idx), nested_ops)
            }
            ListItemAlignment::UseExecuted => {
                // No match - use executed item as-is
                ("use_executed".to_string(), None, Vec::new())
            }
        };

        ops.push(ReadableListItemOp {
            result_index: result_idx,
            action,
            before_idx,
            block_ops,
        });
    }

    ops
}

/// Build readable block operations from a plan
fn build_block_ops(
    plan: &ReconciliationPlan,
    before_blocks: &[Block],
    after_blocks: &[Block],
    snippet_len: usize,
) -> Vec<ReadableBlockOp> {
    let mut ops = Vec::new();

    for (result_idx, alignment) in plan.block_alignments.iter().enumerate() {
        let (action, before_idx, after_idx) = match alignment {
            BlockAlignment::KeepBefore(idx) => ("keep_before", Some(*idx), None),
            BlockAlignment::UseAfter(idx) => ("use_after", None, Some(*idx)),
            BlockAlignment::RecurseIntoContainer {
                before_idx,
                after_idx,
            } => ("recurse", Some(*before_idx), Some(*after_idx)),
        };

        let before_block = before_idx.and_then(|i| before_blocks.get(i));
        let after_block = after_idx.and_then(|i| after_blocks.get(i));

        let block_type = before_block
            .or(after_block)
            .map(block_type_name)
            .unwrap_or("Unknown");

        // Handle nested block plans for containers (Div, BlockQuote, Figure)
        let nested_block_ops =
            if let Some(nested_plan) = plan.block_container_plans.get(&result_idx) {
                let nested_before = before_block.map(get_block_children).unwrap_or_default();
                let nested_after = after_block.map(get_block_children).unwrap_or_default();
                build_block_ops(nested_plan, &nested_before, &nested_after, snippet_len)
            } else {
                Vec::new()
            };

        // Handle list item plans (BulletList, OrderedList)
        let list_item_ops = if let Some(nested_plan) = plan.block_container_plans.get(&result_idx) {
            let before_items = before_block.and_then(get_list_items);
            let after_items = after_block.and_then(get_list_items);
            if before_items.is_some() || after_items.is_some() {
                build_list_item_ops(nested_plan, before_items, after_items, snippet_len)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Handle inline plans
        let inline_ops = if let Some(inline_plan) = plan.inline_plans.get(&result_idx) {
            let before_inlines = before_block
                .and_then(get_block_inlines)
                .cloned()
                .unwrap_or_default();
            let after_inlines = after_block
                .and_then(get_block_inlines)
                .cloned()
                .unwrap_or_default();
            build_inline_ops(inline_plan, &before_inlines, &after_inlines, snippet_len)
        } else {
            Vec::new()
        };

        ops.push(ReadableBlockOp {
            result_index: result_idx,
            action: action.to_string(),
            block_type: block_type.to_string(),
            before_idx,
            after_idx,
            nested_block_ops,
            list_item_ops,
            inline_ops,
        });
    }

    ops
}

// =============================================================================
// Plan Reconstruction from JSON (used in tests)
// =============================================================================

/// Reconstruct a ReconciliationPlan from readable block operations.
#[allow(dead_code)] // Used in tests
fn plan_from_block_ops(ops: &[ReadableBlockOp]) -> ReconciliationPlan {
    let mut block_alignments = Vec::with_capacity(ops.len());
    let mut block_container_plans = LinkedHashMap::new();
    let mut inline_plans = LinkedHashMap::new();

    for op in ops {
        // Convert action back to BlockAlignment
        let alignment = match op.action.as_str() {
            "keep_before" => BlockAlignment::KeepBefore(op.before_idx.unwrap_or(0)),
            "use_after" => BlockAlignment::UseAfter(op.after_idx.unwrap_or(0)),
            "recurse" => BlockAlignment::RecurseIntoContainer {
                before_idx: op.before_idx.unwrap_or(0),
                after_idx: op.after_idx.unwrap_or(0),
            },
            _ => BlockAlignment::UseAfter(op.after_idx.unwrap_or(0)),
        };
        block_alignments.push(alignment);

        // Handle nested block ops (for Div, BlockQuote, Figure)
        if !op.nested_block_ops.is_empty() {
            let nested_plan = plan_from_block_ops(&op.nested_block_ops);
            block_container_plans.insert(op.result_index, nested_plan);
        }

        // Handle list item ops (for BulletList, OrderedList)
        if !op.list_item_ops.is_empty() {
            let container_plan = plan_from_list_item_ops(&op.list_item_ops);
            block_container_plans.insert(op.result_index, container_plan);
        }

        // Handle inline ops
        if !op.inline_ops.is_empty() {
            let inline_plan = plan_from_inline_ops(&op.inline_ops);
            inline_plans.insert(op.result_index, inline_plan);
        }
    }

    ReconciliationPlan {
        block_alignments,
        block_container_plans,
        inline_plans,
        custom_node_plans: LinkedHashMap::new(),
        table_plans: LinkedHashMap::new(),
        list_item_alignments: Vec::new(),
        list_item_plans: LinkedHashMap::new(),
        stats: Default::default(),
    }
}

/// Reconstruct a ReconciliationPlan from list item operations.
#[allow(dead_code)] // Used in tests
fn plan_from_list_item_ops(ops: &[ReadableListItemOp]) -> ReconciliationPlan {
    let mut list_item_alignments = Vec::with_capacity(ops.len());
    let mut list_item_plans = LinkedHashMap::new();

    for op in ops {
        // Convert action back to ListItemAlignment
        let alignment = match op.action.as_str() {
            "keep_original" => ListItemAlignment::KeepOriginal(op.before_idx.unwrap_or(0)),
            "reconcile" => {
                // Each list item op contains block_ops that describe the item's blocks
                let item_plan = plan_from_block_ops(&op.block_ops);
                list_item_plans.insert(op.result_index, item_plan);
                ListItemAlignment::Reconcile(op.before_idx.unwrap_or(0))
            }
            _ => ListItemAlignment::UseExecuted,
        };
        list_item_alignments.push(alignment);
    }

    ReconciliationPlan {
        block_alignments: Vec::new(),
        block_container_plans: LinkedHashMap::new(),
        inline_plans: LinkedHashMap::new(),
        custom_node_plans: LinkedHashMap::new(),
        table_plans: LinkedHashMap::new(),
        list_item_alignments,
        list_item_plans,
        stats: Default::default(),
    }
}

/// Reconstruct an InlineReconciliationPlan from readable inline operations.
#[allow(dead_code)] // Used in tests
fn plan_from_inline_ops(ops: &[ReadableInlineOp]) -> InlineReconciliationPlan {
    let mut inline_alignments = Vec::with_capacity(ops.len());
    let mut inline_container_plans = LinkedHashMap::new();

    for op in ops {
        // Convert action back to InlineAlignment
        let alignment = match op.action.as_str() {
            "keep_before" => InlineAlignment::KeepBefore(op.before_idx.unwrap_or(0)),
            "use_after" => InlineAlignment::UseAfter(op.after_idx.unwrap_or(0)),
            "recurse" => InlineAlignment::RecurseIntoContainer {
                before_idx: op.before_idx.unwrap_or(0),
                after_idx: op.after_idx.unwrap_or(0),
            },
            _ => InlineAlignment::UseAfter(op.after_idx.unwrap_or(0)),
        };
        inline_alignments.push(alignment);

        // Handle nested inline ops
        if !op.nested_inline_ops.is_empty() {
            let nested_plan = plan_from_inline_ops(&op.nested_inline_ops);
            inline_container_plans.insert(op.result_index, nested_plan);
        }
    }

    InlineReconciliationPlan {
        inline_alignments,
        inline_container_plans,
        note_block_plans: LinkedHashMap::new(),
        custom_node_plans: LinkedHashMap::new(),
    }
}

/// Reconstruct a ReconciliationPlan from a ReadableReport.
#[allow(dead_code)] // Used in tests
fn plan_from_report(report: &ReadableReport) -> ReconciliationPlan {
    plan_from_block_ops(&report.block_operations)
}

fn main() {
    let args = Args::parse();

    // Read before file
    let before_content = std::fs::read_to_string(&args.before).unwrap_or_else(|e| {
        eprintln!("Error reading before file '{}': {}", args.before, e);
        std::process::exit(1);
    });

    // Read after file
    let after_content = std::fs::read_to_string(&args.after).unwrap_or_else(|e| {
        eprintln!("Error reading after file '{}': {}", args.after, e);
        std::process::exit(1);
    });

    // Ensure files end with newline
    let before_content = if before_content.ends_with('\n') {
        before_content
    } else {
        format!("{}\n", before_content)
    };
    let after_content = if after_content.ends_with('\n') {
        after_content
    } else {
        format!("{}\n", after_content)
    };

    // Parse before file
    let mut sink = io::sink();
    let (before_ast, _, _) = match readers::qmd::read(
        before_content.as_bytes(),
        false,
        &args.before,
        &mut sink,
        true,
        None,
    ) {
        Ok(result) => result,
        Err(diagnostics) => {
            eprintln!("Error parsing before file '{}':", args.before);
            for diag in diagnostics {
                eprintln!("  {}", diag.to_text(None));
            }
            std::process::exit(1);
        }
    };

    // Parse after file
    let (after_ast, _, _) = match readers::qmd::read(
        after_content.as_bytes(),
        false,
        &args.after,
        &mut sink,
        true,
        None,
    ) {
        Ok(result) => result,
        Err(diagnostics) => {
            eprintln!("Error parsing after file '{}':", args.after);
            for diag in diagnostics {
                eprintln!("  {}", diag.to_text(None));
            }
            std::process::exit(1);
        }
    };

    // Compute reconciliation plan
    let plan = compute_reconciliation(&before_ast, &after_ast);

    // Build readable report
    let block_operations = build_block_ops(
        &plan,
        &before_ast.blocks,
        &after_ast.blocks,
        args.snippet_len,
    );

    let report = ReadableReport {
        before_file: args.before,
        after_file: args.after,
        block_operations,
    };

    // Output pretty JSON
    match serde_json::to_string_pretty(&report) {
        Ok(s) => println!("{}", s),
        Err(e) => {
            eprintln!("Error serializing to JSON: {}", e);
            std::process::exit(1);
        }
    }
}

// =============================================================================
// Property Tests for JSON Round-Trip
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use quarto_pandoc_types::reconcile::{apply_reconciliation, compute_reconciliation};
    use quarto_pandoc_types::{BulletList, Pandoc, Paragraph, Space, Str};
    use quarto_source_map::{FileId, SourceInfo};

    // Simple generators for testing

    fn dummy_source() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn gen_str() -> impl Strategy<Value = Inline> {
        "[a-zA-Z]{1,10}"
            .prop_filter("non-empty", |s: &String| !s.is_empty())
            .prop_map(|text| {
                Inline::Str(Str {
                    text,
                    source_info: dummy_source(),
                })
            })
    }

    fn gen_space() -> impl Strategy<Value = Inline> {
        Just(Inline::Space(Space {
            source_info: dummy_source(),
        }))
    }

    fn gen_inlines() -> impl Strategy<Value = Vec<Inline>> {
        proptest::collection::vec(prop_oneof![gen_str(), gen_space()], 1..=5)
    }

    fn gen_paragraph() -> impl Strategy<Value = Block> {
        gen_inlines().prop_map(|content| {
            Block::Paragraph(Paragraph {
                content,
                source_info: dummy_source(),
            })
        })
    }

    fn gen_bullet_list() -> impl Strategy<Value = Block> {
        let item_gen = gen_paragraph().prop_map(|p| vec![p]);
        proptest::collection::vec(item_gen, 1..=4).prop_map(|content| {
            Block::BulletList(BulletList {
                content,
                source_info: dummy_source(),
            })
        })
    }

    fn gen_pandoc_simple() -> impl Strategy<Value = Pandoc> {
        proptest::collection::vec(gen_paragraph(), 1..=3).prop_map(|blocks| Pandoc {
            meta: Default::default(),
            blocks,
        })
    }

    fn gen_pandoc_with_list() -> impl Strategy<Value = Pandoc> {
        gen_bullet_list().prop_map(|list| Pandoc {
            meta: Default::default(),
            blocks: vec![list],
        })
    }

    /// Helper: Check if two block sequences are structurally equal (ignoring source_info)
    fn blocks_structurally_equal(a: &[Block], b: &[Block]) -> bool {
        use quarto_pandoc_types::reconcile::structural_eq_blocks;
        structural_eq_blocks(a, b)
    }

    proptest! {
        /// Property: JSON round-trip preserves reconciliation semantics.
        ///
        /// For any pair of documents:
        /// 1. Compute the reconciliation plan
        /// 2. Apply it to get result_1
        /// 3. Serialize plan to JSON, deserialize back to plan_2
        /// 4. Apply plan_2 to get result_2
        /// 5. result_1 and result_2 should be structurally equal
        #[test]
        fn json_roundtrip_preserves_reconciliation_simple(
            before in gen_pandoc_simple(),
            after in gen_pandoc_simple(),
        ) {
            // Compute plan and apply
            let plan = compute_reconciliation(&before, &after);
            let result_1 = apply_reconciliation(before.clone(), after.clone(), &plan);

            // Build readable report (JSON serialization)
            let block_operations = build_block_ops(
                &plan,
                &before.blocks,
                &after.blocks,
                60,
            );
            let report = ReadableReport {
                before_file: "test_before".to_string(),
                after_file: "test_after".to_string(),
                block_operations,
            };

            // Serialize to JSON and back
            let json = serde_json::to_string(&report).expect("Failed to serialize");
            let report_2: ReadableReport = serde_json::from_str(&json).expect("Failed to deserialize");

            // Reconstruct plan from JSON
            let plan_2 = plan_from_report(&report_2);

            // Apply reconstructed plan
            let result_2 = apply_reconciliation(before, after, &plan_2);

            // Both results should be structurally equal
            prop_assert!(
                blocks_structurally_equal(&result_1.blocks, &result_2.blocks),
                "JSON round-trip should preserve reconciliation semantics.\n\
                 Result 1 blocks: {}\n\
                 Result 2 blocks: {}",
                result_1.blocks.len(),
                result_2.blocks.len()
            );
        }

        #[test]
        fn json_roundtrip_preserves_reconciliation_with_lists(
            before in gen_pandoc_with_list(),
            after in gen_pandoc_with_list(),
        ) {
            // Compute plan and apply
            let plan = compute_reconciliation(&before, &after);
            let result_1 = apply_reconciliation(before.clone(), after.clone(), &plan);

            // Build readable report (JSON serialization)
            let block_operations = build_block_ops(
                &plan,
                &before.blocks,
                &after.blocks,
                60,
            );
            let report = ReadableReport {
                before_file: "test_before".to_string(),
                after_file: "test_after".to_string(),
                block_operations,
            };

            // Serialize to JSON and back
            let json = serde_json::to_string(&report).expect("Failed to serialize");
            let report_2: ReadableReport = serde_json::from_str(&json).expect("Failed to deserialize");

            // Reconstruct plan from JSON
            let plan_2 = plan_from_report(&report_2);

            // Apply reconstructed plan
            let result_2 = apply_reconciliation(before, after, &plan_2);

            // Both results should be structurally equal
            prop_assert!(
                blocks_structurally_equal(&result_1.blocks, &result_2.blocks),
                "JSON round-trip should preserve reconciliation semantics for lists.\n\
                 Result 1 blocks: {}\n\
                 Result 2 blocks: {}",
                result_1.blocks.len(),
                result_2.blocks.len()
            );
        }
    }
}
