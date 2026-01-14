/*
 * mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * AST reconciliation module.
 *
 * This module provides reconciliation of two Pandoc ASTs, enabling
 * selective node replacement to preserve source locations for unchanged
 * content after engine execution.
 *
 * The reconciliation is split into two phases:
 * 1. Compute: Analyze both ASTs and produce a ReconciliationPlan
 * 2. Apply: Execute the plan to produce a merged AST
 *
 * This design is inspired by React 15's reconciliation algorithm,
 * using structural hashes as "virtual keys" for node matching.
 */

mod apply;
mod compute;
#[cfg(test)]
mod generators;
mod hash;
mod types;

pub use apply::apply_reconciliation;
pub use compute::compute_reconciliation;
pub use hash::{
    HashCache, compute_block_hash_fresh, compute_inline_hash_fresh, structural_eq_block,
    structural_eq_blocks, structural_eq_inline,
};
pub use types::{
    BlockAlignment, InlineAlignment, InlineReconciliationPlan, ReconciliationPlan,
    ReconciliationStats,
};

use crate::Pandoc;

/// Reconcile two Pandoc ASTs, producing a merged result.
///
/// This is the main entry point for AST reconciliation. It:
/// 1. Computes a reconciliation plan by comparing the ASTs
/// 2. Applies the plan to produce a merged AST
///
/// # Arguments
/// * `original` - The pre-engine AST with original source locations
/// * `executed` - The post-engine AST with engine output source locations
///
/// # Returns
/// A tuple of (merged AST, reconciliation plan with statistics)
///
/// # Source Location Semantics
/// - Unchanged nodes keep their original source locations
/// - Changed nodes get the engine output source locations
/// - Container nodes keep their original source locations while
///   their children may be reconciled individually
///
/// # Example
/// ```ignore
/// let (merged, plan) = reconcile(original_ast, executed_ast);
/// println!("Kept {} blocks, replaced {}", plan.stats.blocks_kept, plan.stats.blocks_replaced);
/// ```
pub fn reconcile(original: Pandoc, executed: Pandoc) -> (Pandoc, ReconciliationPlan) {
    let plan = compute_reconciliation(&original, &executed);
    let result = apply_reconciliation(original, executed, &plan);
    (result, plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::{CustomNode, Slot};
    use crate::{CodeBlock, Div, Header, Paragraph, Str};
    use hashlink::LinkedHashMap;
    use quarto_source_map::{FileId, SourceInfo};

    fn source_original() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 100)
    }

    fn source_executed() -> SourceInfo {
        SourceInfo::original(FileId(1), 0, 100)
    }

    fn make_para(text: &str, source: SourceInfo) -> crate::Block {
        crate::Block::Paragraph(Paragraph {
            content: vec![crate::Inline::Str(Str {
                text: text.to_string(),
                source_info: source.clone(),
            })],
            source_info: source,
        })
    }

    fn make_header(level: usize, text: &str, source: SourceInfo) -> crate::Block {
        crate::Block::Header(Header {
            level,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![crate::Inline::Str(Str {
                text: text.to_string(),
                source_info: source.clone(),
            })],
            source_info: source,
            attr_source: crate::AttrSourceInfo::empty(),
        })
    }

    fn make_code_block(code: &str, source: SourceInfo) -> crate::Block {
        crate::Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec!["{r}".to_string()], LinkedHashMap::new()),
            text: code.to_string(),
            source_info: source,
            attr_source: crate::AttrSourceInfo::empty(),
        })
    }

    fn make_div(blocks: Vec<crate::Block>, source: SourceInfo) -> crate::Block {
        crate::Block::Div(Div {
            attr: (
                String::new(),
                vec!["cell".to_string()],
                LinkedHashMap::new(),
            ),
            content: blocks,
            source_info: source,
            attr_source: crate::AttrSourceInfo::empty(),
        })
    }

    #[test]
    fn test_reconcile_preserves_unchanged() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para("unchanged", source_original()),
                make_para("also unchanged", source_original()),
            ],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para("unchanged", source_executed()),
                make_para("also unchanged", source_executed()),
            ],
        };

        let (result, plan) = reconcile(original, executed);

        assert_eq!(plan.stats.blocks_kept, 2);
        assert_eq!(plan.stats.blocks_replaced, 0);

        // Both blocks should have original source locations
        for block in &result.blocks {
            if let crate::Block::Paragraph(p) = block {
                assert_eq!(p.source_info, source_original());
            }
        }
    }

    #[test]
    fn test_reconcile_replaces_changed() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_para("original text", source_original())],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_para("changed text", source_executed())],
        };

        let (result, plan) = reconcile(original, executed);

        // Changed block should be replaced
        assert!(plan.stats.blocks_replaced > 0 || plan.stats.blocks_recursed > 0);

        // Result should have executed source location
        if let crate::Block::Paragraph(p) = &result.blocks[0] {
            assert_eq!(p.source_info, source_executed());
        }
    }

    #[test]
    fn test_reconcile_handles_new_blocks() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_para("original", source_original())],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para("original", source_executed()),
                make_para("new block", source_executed()),
            ],
        };

        let (result, plan) = reconcile(original, executed);

        assert_eq!(result.blocks.len(), 2);
        assert_eq!(plan.stats.blocks_kept, 1);
        assert_eq!(plan.stats.blocks_replaced, 1);

        // First block should keep original source
        if let crate::Block::Paragraph(p) = &result.blocks[0] {
            assert_eq!(p.source_info, source_original());
        }

        // New block should have executed source
        if let crate::Block::Paragraph(p) = &result.blocks[1] {
            assert_eq!(p.source_info, source_executed());
        }
    }

    /// Test scenario based on resources/ast-reconciliation-examples/01:
    /// A document with a code block that gets expanded into a cell div.
    /// - Headers and unchanged paragraphs should keep original source
    /// - The code block replaced by cell div should use executed source
    #[test]
    fn test_code_block_expansion_to_cell_div() {
        // Original: [Header, Para, Para, CodeBlock, Header, Para, Para]
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_header(2, "This is a section", source_original()),
                make_para("There's some stuff here.", source_original()),
                make_para("Repeat.", source_original()),
                make_code_block("cat(\"Hello world\")", source_original()),
                make_header(2, "This is another section", source_original()),
                make_para("There's more stuff here.", source_original()),
                make_para("Repeat.", source_original()),
            ],
        };

        // Executed: [Header, Para, Para, Div(cell), Header, Para, Para]
        // The CodeBlock is replaced by a Div containing the cell output
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_header(2, "This is a section", source_executed()),
                make_para("There's some stuff here.", source_executed()),
                make_para("Repeat.", source_executed()),
                make_div(
                    vec![
                        make_code_block("cat(\"Hello world\")", source_executed()),
                        make_para("Hello world", source_executed()),
                    ],
                    source_executed(),
                ),
                make_header(2, "This is another section", source_executed()),
                make_para("There's more stuff here.", source_executed()),
                make_para("Repeat.", source_executed()),
            ],
        };

        let (result, plan) = reconcile(original, executed);

        // Should have same number of top-level blocks
        assert_eq!(result.blocks.len(), 7);

        // Check statistics - 6 blocks kept (2 headers + 4 paras), 1 replaced (CodeBlock → Div)
        assert_eq!(plan.stats.blocks_kept, 6);
        assert_eq!(plan.stats.blocks_replaced, 1);

        // First header should keep original source
        if let crate::Block::Header(h) = &result.blocks[0] {
            assert_eq!(h.source_info, source_original());
        } else {
            panic!("Expected Header at position 0");
        }

        // Paragraphs before the cell should keep original source
        if let crate::Block::Paragraph(p) = &result.blocks[1] {
            assert_eq!(p.source_info, source_original());
        }
        if let crate::Block::Paragraph(p) = &result.blocks[2] {
            assert_eq!(p.source_info, source_original());
        }

        // The cell div (replaced from code block) should have executed source
        if let crate::Block::Div(d) = &result.blocks[3] {
            assert_eq!(d.source_info, source_executed());
        } else {
            panic!("Expected Div at position 3");
        }

        // Second header should keep original source
        if let crate::Block::Header(h) = &result.blocks[4] {
            assert_eq!(h.source_info, source_original());
        }

        // Last paragraphs should keep original source
        if let crate::Block::Paragraph(p) = &result.blocks[5] {
            assert_eq!(p.source_info, source_original());
        }
        if let crate::Block::Paragraph(p) = &result.blocks[6] {
            assert_eq!(p.source_info, source_original());
        }
    }

    /// Test scenario based on resources/ast-reconciliation-examples/02:
    /// Inline code execution - text around inline code should be preserved,
    /// but when inline content changes entirely, the block uses executed source.
    #[test]
    fn test_inline_code_replaced_with_result() {
        // Original paragraph has: "This has inline code in it, `r 23 * 37`. Let's see what happens."
        // We simulate this as multiple inline elements
        let original_para = crate::Block::Paragraph(Paragraph {
            content: vec![
                crate::Inline::Str(Str {
                    text: "Value: ".to_string(),
                    source_info: source_original(),
                }),
                crate::Inline::Code(crate::Code {
                    attr: (String::new(), vec![], LinkedHashMap::new()),
                    text: "r 23 * 37".to_string(),
                    source_info: source_original(),
                    attr_source: crate::AttrSourceInfo::empty(),
                }),
            ],
            source_info: source_original(),
        });

        // Executed paragraph has: "Value: 851" (inline code replaced with result)
        let executed_para = crate::Block::Paragraph(Paragraph {
            content: vec![
                crate::Inline::Str(Str {
                    text: "Value: ".to_string(),
                    source_info: source_executed(),
                }),
                crate::Inline::Str(Str {
                    text: "851".to_string(),
                    source_info: source_executed(),
                }),
            ],
            source_info: source_executed(),
        });

        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_header(2, "Inline code execution", source_original()),
                original_para,
                make_header(2, "Another section", source_original()),
                make_para("Regular text.", source_original()),
            ],
        };

        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_header(2, "Inline code execution", source_executed()),
                executed_para,
                make_header(2, "Another section", source_executed()),
                make_para("Regular text.", source_executed()),
            ],
        };

        let (result, plan) = reconcile(original, executed);

        assert_eq!(result.blocks.len(), 4);

        // First header should keep original source (unchanged)
        if let crate::Block::Header(h) = &result.blocks[0] {
            assert_eq!(h.source_info, source_original());
        }

        // The paragraph with inline code replacement:
        // Since "Value: " matches but the Code→Str change means not all inlines match,
        // the whole paragraph uses executed source
        if let crate::Block::Paragraph(p) = &result.blocks[1] {
            // Check that at least the first inline (matching "Value: ") has original source
            if let crate::Inline::Str(s) = &p.content[0] {
                assert_eq!(s.text, "Value: ");
                // This inline matches, so should have original source
                assert_eq!(s.source_info, source_original());
            }
        }

        // Second header should keep original source
        if let crate::Block::Header(h) = &result.blocks[2] {
            assert_eq!(h.source_info, source_original());
        }

        // Last paragraph should keep original source (unchanged)
        if let crate::Block::Paragraph(p) = &result.blocks[3] {
            assert_eq!(p.source_info, source_original());
        }

        // At least some blocks should be kept
        assert!(
            plan.stats.blocks_kept >= 2,
            "Expected at least 2 blocks kept"
        );
    }

    /// Test that multiple identical paragraphs at different positions are matched correctly.
    #[test]
    fn test_duplicate_blocks_matched_in_order() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para("Repeat.", source_original()),
                make_para("Different.", source_original()),
                make_para("Repeat.", source_original()),
            ],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para("Repeat.", source_executed()),
                make_para("Different.", source_executed()),
                make_para("Repeat.", source_executed()),
            ],
        };

        let (result, plan) = reconcile(original, executed);

        // All blocks should be kept (same content, different sources)
        assert_eq!(plan.stats.blocks_kept, 3);
        assert_eq!(plan.stats.blocks_replaced, 0);

        // All should have original source
        for block in &result.blocks {
            if let crate::Block::Paragraph(p) = block {
                assert_eq!(p.source_info, source_original());
            }
        }
    }

    /// Test that blocks are matched even when reordered (first match wins).
    #[test]
    fn test_reordered_blocks() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para("First", source_original()),
                make_para("Second", source_original()),
                make_para("Third", source_original()),
            ],
        };
        // Executed has same blocks but in different order
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para("Second", source_executed()),
                make_para("Third", source_executed()),
                make_para("First", source_executed()),
            ],
        };

        let (result, plan) = reconcile(original, executed);

        // All three blocks should still be kept (just from different positions)
        assert_eq!(plan.stats.blocks_kept, 3);
        assert_eq!(result.blocks.len(), 3);

        // The order follows executed, but sources come from original
        // "Second" in executed matches "Second" in original
        if let crate::Block::Paragraph(p) = &result.blocks[0] {
            if let crate::Inline::Str(s) = &p.content[0] {
                assert_eq!(s.text, "Second");
            }
            assert_eq!(p.source_info, source_original());
        }
    }

    // =========================================================================
    // CustomNode Reconciliation Tests
    // =========================================================================

    /// Helper to create a CustomNode (like a Callout) with a content slot.
    fn make_custom_node(
        type_name: &str,
        title_text: &str,
        content_text: &str,
        source: SourceInfo,
    ) -> CustomNode {
        let mut slots = LinkedHashMap::new();

        // Title slot (Inlines)
        slots.insert(
            "title".to_string(),
            Slot::Inlines(vec![crate::Inline::Str(Str {
                text: title_text.to_string(),
                source_info: source.clone(),
            })]),
        );

        // Content slot (Blocks)
        slots.insert(
            "content".to_string(),
            Slot::Blocks(vec![crate::Block::Paragraph(Paragraph {
                content: vec![crate::Inline::Str(Str {
                    text: content_text.to_string(),
                    source_info: source.clone(),
                })],
                source_info: source.clone(),
            })]),
        );

        CustomNode {
            type_name: type_name.to_string(),
            slots,
            plain_data: serde_json::json!({"type": "note"}),
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: source,
        }
    }

    /// Test: CustomNode with identical content is kept (preserves source location).
    #[test]
    fn test_custom_node_identical_content_kept() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Custom(make_custom_node(
                "Callout",
                "Note",
                "This is important.",
                source_original(),
            ))],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Custom(make_custom_node(
                "Callout",
                "Note",
                "This is important.",
                source_executed(),
            ))],
        };

        let (result, plan) = reconcile(original, executed);

        // CustomNode should be kept (identical content)
        assert_eq!(plan.stats.blocks_kept, 1);
        assert_eq!(plan.stats.blocks_replaced, 0);

        // Source should be from original
        if let crate::Block::Custom(cn) = &result.blocks[0] {
            assert_eq!(cn.source_info, source_original());
            assert_eq!(cn.type_name, "Callout");
        } else {
            panic!("Expected Custom block");
        }
    }

    /// Test: CustomNode with changed slot content gets slot-level reconciliation.
    #[test]
    fn test_custom_node_slot_content_changed() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Custom(make_custom_node(
                "Callout",
                "Note",
                "Original content.",
                source_original(),
            ))],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Custom(make_custom_node(
                "Callout",
                "Note",             // Title unchanged
                "Changed content.", // Content changed
                source_executed(),
            ))],
        };

        let (result, plan) = reconcile(original, executed);

        // Should recurse into the CustomNode
        assert_eq!(plan.stats.blocks_recursed, 1);

        if let crate::Block::Custom(cn) = &result.blocks[0] {
            // CustomNode itself should preserve original source
            assert_eq!(cn.source_info, source_original());

            // Title slot should preserve original source (unchanged)
            if let Some(Slot::Inlines(title)) = cn.slots.get("title") {
                if let crate::Inline::Str(s) = &title[0] {
                    assert_eq!(s.text, "Note");
                    assert_eq!(s.source_info, source_original());
                }
            } else {
                panic!("Expected title slot");
            }

            // Content slot should have executed source (changed)
            if let Some(Slot::Blocks(content)) = cn.slots.get("content") {
                if let crate::Block::Paragraph(p) = &content[0] {
                    assert_eq!(p.source_info, source_executed());
                    if let crate::Inline::Str(s) = &p.content[0] {
                        assert_eq!(s.text, "Changed content.");
                    }
                }
            } else {
                panic!("Expected content slot");
            }
        } else {
            panic!("Expected Custom block");
        }
    }

    /// Test: CustomNodes with different type_name are not reconciled.
    #[test]
    fn test_custom_node_different_type_not_reconciled() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Custom(make_custom_node(
                "Callout",
                "Note",
                "Content",
                source_original(),
            ))],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Custom(make_custom_node(
                "PanelTabset", // Different type!
                "Note",
                "Content",
                source_executed(),
            ))],
        };

        let (result, plan) = reconcile(original, executed);

        // Should be replaced, not recursed (different type_name)
        assert_eq!(plan.stats.blocks_replaced, 1);
        assert_eq!(plan.stats.blocks_recursed, 0);

        // Result should have executed's CustomNode
        if let crate::Block::Custom(cn) = &result.blocks[0] {
            assert_eq!(cn.type_name, "PanelTabset");
            assert_eq!(cn.source_info, source_executed());
        } else {
            panic!("Expected Custom block");
        }
    }

    /// Test: CustomNode plain_data is taken from executed.
    #[test]
    fn test_custom_node_plain_data_from_executed() {
        let mut orig_cn = make_custom_node("Callout", "Note", "Content", source_original());
        orig_cn.plain_data = serde_json::json!({"type": "note", "collapse": false});

        let mut exec_cn = make_custom_node("Callout", "Note", "Changed content", source_executed());
        exec_cn.plain_data = serde_json::json!({"type": "note", "collapse": true}); // Different!

        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Custom(orig_cn)],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Custom(exec_cn)],
        };

        let (result, _plan) = reconcile(original, executed);

        if let crate::Block::Custom(cn) = &result.blocks[0] {
            // plain_data should come from executed
            assert_eq!(cn.plain_data["collapse"], true);
            // But source_info should be from original
            assert_eq!(cn.source_info, source_original());
        } else {
            panic!("Expected Custom block");
        }
    }

    /// Test: CustomNode mixed with regular blocks.
    #[test]
    fn test_custom_node_mixed_with_regular_blocks() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para("Before callout", source_original()),
                crate::Block::Custom(make_custom_node(
                    "Callout",
                    "Warning",
                    "Be careful!",
                    source_original(),
                )),
                make_para("After callout", source_original()),
            ],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![
                make_para("Before callout", source_executed()),
                crate::Block::Custom(make_custom_node(
                    "Callout",
                    "Warning",
                    "Be careful!",
                    source_executed(),
                )),
                make_para("After callout", source_executed()),
            ],
        };

        let (result, plan) = reconcile(original, executed);

        // All blocks should be kept
        assert_eq!(plan.stats.blocks_kept, 3);
        assert_eq!(plan.stats.blocks_replaced, 0);

        // First para
        if let crate::Block::Paragraph(p) = &result.blocks[0] {
            assert_eq!(p.source_info, source_original());
        }

        // CustomNode
        if let crate::Block::Custom(cn) = &result.blocks[1] {
            assert_eq!(cn.source_info, source_original());
        }

        // Last para
        if let crate::Block::Paragraph(p) = &result.blocks[2] {
            assert_eq!(p.source_info, source_original());
        }
    }

    /// Test: CustomNode with multiple slots, some changed and some unchanged.
    #[test]
    fn test_custom_node_partial_slot_changes() {
        // Create a more complex CustomNode with multiple block slots
        fn make_complex_callout(
            title: &str,
            body1: &str,
            body2: &str,
            source: SourceInfo,
        ) -> CustomNode {
            let mut slots = LinkedHashMap::new();

            slots.insert(
                "title".to_string(),
                Slot::Inlines(vec![crate::Inline::Str(Str {
                    text: title.to_string(),
                    source_info: source.clone(),
                })]),
            );

            slots.insert(
                "content".to_string(),
                Slot::Blocks(vec![
                    crate::Block::Paragraph(Paragraph {
                        content: vec![crate::Inline::Str(Str {
                            text: body1.to_string(),
                            source_info: source.clone(),
                        })],
                        source_info: source.clone(),
                    }),
                    crate::Block::Paragraph(Paragraph {
                        content: vec![crate::Inline::Str(Str {
                            text: body2.to_string(),
                            source_info: source.clone(),
                        })],
                        source_info: source.clone(),
                    }),
                ]),
            );

            CustomNode {
                type_name: "Callout".to_string(),
                slots,
                plain_data: serde_json::json!({}),
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: source,
            }
        }

        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Custom(make_complex_callout(
                "Important",
                "First paragraph unchanged.",
                "Second paragraph will change.",
                source_original(),
            ))],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Custom(make_complex_callout(
                "Important",                  // Title unchanged
                "First paragraph unchanged.", // First para unchanged
                "Second paragraph CHANGED!",  // Second para changed
                source_executed(),
            ))],
        };

        let (result, plan) = reconcile(original, executed);

        // Should recurse
        assert_eq!(plan.stats.blocks_recursed, 1);

        if let crate::Block::Custom(cn) = &result.blocks[0] {
            // CustomNode keeps original source
            assert_eq!(cn.source_info, source_original());

            // Title should keep original
            if let Some(Slot::Inlines(title)) = cn.slots.get("title")
                && let crate::Inline::Str(s) = &title[0]
            {
                assert_eq!(s.source_info, source_original());
            }

            // Content slot - first para unchanged, second changed
            if let Some(Slot::Blocks(content)) = cn.slots.get("content") {
                // First paragraph should keep original source
                if let crate::Block::Paragraph(p1) = &content[0] {
                    assert_eq!(p1.source_info, source_original());
                }

                // Second paragraph should have executed source
                if let crate::Block::Paragraph(p2) = &content[1] {
                    assert_eq!(p2.source_info, source_executed());
                    if let crate::Inline::Str(s) = &p2.content[0] {
                        assert_eq!(s.text, "Second paragraph CHANGED!");
                    }
                }
            }
        }
    }

    /// Test: Inline CustomNode reconciliation.
    #[test]
    fn test_inline_custom_node_reconciliation() {
        fn make_inline_custom(text: &str, source: SourceInfo) -> crate::Inline {
            let mut slots = LinkedHashMap::new();
            slots.insert(
                "content".to_string(),
                Slot::Inlines(vec![crate::Inline::Str(Str {
                    text: text.to_string(),
                    source_info: source.clone(),
                })]),
            );

            crate::Inline::Custom(CustomNode {
                type_name: "Shortcode".to_string(),
                slots,
                plain_data: serde_json::json!({"name": "video"}),
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: source,
            })
        }

        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Paragraph(Paragraph {
                content: vec![
                    crate::Inline::Str(Str {
                        text: "Text before ".to_string(),
                        source_info: source_original(),
                    }),
                    make_inline_custom("unchanged", source_original()),
                    crate::Inline::Str(Str {
                        text: " text after".to_string(),
                        source_info: source_original(),
                    }),
                ],
                source_info: source_original(),
            })],
        };

        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![crate::Block::Paragraph(Paragraph {
                content: vec![
                    crate::Inline::Str(Str {
                        text: "Text before ".to_string(),
                        source_info: source_executed(),
                    }),
                    make_inline_custom("unchanged", source_executed()),
                    crate::Inline::Str(Str {
                        text: " text after".to_string(),
                        source_info: source_executed(),
                    }),
                ],
                source_info: source_executed(),
            })],
        };

        let (result, _plan) = reconcile(original, executed);

        if let crate::Block::Paragraph(p) = &result.blocks[0] {
            // Paragraph should keep original source (content unchanged)
            assert_eq!(p.source_info, source_original());

            // Inline Custom should keep original source
            if let crate::Inline::Custom(cn) = &p.content[1] {
                assert_eq!(cn.source_info, source_original());
            }
        }
    }
}

// =========================================================================
// List Length Change Tests
// =========================================================================

#[cfg(test)]
mod list_length_tests {
    use super::*;
    use crate::reconcile::hash::structural_eq_blocks;
    use crate::{BulletList, Paragraph, Str};
    use quarto_source_map::{FileId, SourceInfo};

    fn source_orig() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 100)
    }

    fn source_exec() -> SourceInfo {
        SourceInfo::original(FileId(1), 0, 100)
    }

    fn make_list_item(text: &str, source: SourceInfo) -> Vec<crate::Block> {
        vec![crate::Block::Paragraph(Paragraph {
            content: vec![crate::Inline::Str(Str {
                text: text.to_string(),
                source_info: source.clone(),
            })],
            source_info: source,
        })]
    }

    fn make_bullet_list(items: Vec<Vec<crate::Block>>, source: SourceInfo) -> crate::Block {
        crate::Block::BulletList(BulletList {
            content: items,
            source_info: source,
        })
    }

    /// Test: List with same number of items - structural equality preserved.
    #[test]
    fn list_same_length_preserves_structure() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_bullet_list(
                vec![
                    make_list_item("one", source_orig()),
                    make_list_item("two", source_orig()),
                    make_list_item("three", source_orig()),
                ],
                source_orig(),
            )],
        };
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_bullet_list(
                vec![
                    make_list_item("one", source_exec()),
                    make_list_item("two", source_exec()),
                    make_list_item("three", source_exec()),
                ],
                source_exec(),
            )],
        };

        let after_clone = executed.clone();
        let (result, _plan) = reconcile(original, executed);

        // Result must be structurally equal to after
        assert!(
            structural_eq_blocks(&result.blocks, &after_clone.blocks),
            "Same-length list should preserve structure"
        );

        // Verify 3 items in result
        if let crate::Block::BulletList(bl) = &result.blocks[0] {
            assert_eq!(bl.content.len(), 3);
        } else {
            panic!("Expected BulletList");
        }
    }

    /// Test: List item removed - result should have 2 items, not 3.
    #[test]
    fn list_item_removed_produces_correct_length() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_bullet_list(
                vec![
                    make_list_item("one", source_orig()),
                    make_list_item("two", source_orig()),
                    make_list_item("three", source_orig()),
                ],
                source_orig(),
            )],
        };
        // Executed has only 2 items (middle one removed)
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_bullet_list(
                vec![
                    make_list_item("one", source_exec()),
                    make_list_item("three", source_exec()),
                ],
                source_exec(),
            )],
        };

        let after_clone = executed.clone();
        let (result, _plan) = reconcile(original, executed);

        // Result must be structurally equal to after
        assert!(
            structural_eq_blocks(&result.blocks, &after_clone.blocks),
            "List with removed item should match after structure"
        );

        // Verify 2 items in result, NOT 3
        if let crate::Block::BulletList(bl) = &result.blocks[0] {
            assert_eq!(
                bl.content.len(),
                2,
                "Result should have 2 items, not 3 (item was removed)"
            );
        } else {
            panic!("Expected BulletList");
        }
    }

    /// Test: List items added - result should have all new items.
    #[test]
    fn list_items_added_produces_correct_length() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_bullet_list(
                vec![make_list_item("one", source_orig())],
                source_orig(),
            )],
        };
        // Executed has 3 items (2 added)
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_bullet_list(
                vec![
                    make_list_item("one", source_exec()),
                    make_list_item("two", source_exec()),
                    make_list_item("three", source_exec()),
                ],
                source_exec(),
            )],
        };

        let after_clone = executed.clone();
        let (result, _plan) = reconcile(original, executed);

        // Result must be structurally equal to after
        assert!(
            structural_eq_blocks(&result.blocks, &after_clone.blocks),
            "List with added items should match after structure"
        );

        // Verify 3 items in result
        if let crate::Block::BulletList(bl) = &result.blocks[0] {
            assert_eq!(
                bl.content.len(),
                3,
                "Result should have 3 items (items were added)"
            );
        } else {
            panic!("Expected BulletList");
        }
    }

    /// Test: List becomes empty - result should be empty list.
    #[test]
    fn list_all_items_removed_produces_empty_list() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_bullet_list(
                vec![
                    make_list_item("one", source_orig()),
                    make_list_item("two", source_orig()),
                ],
                source_orig(),
            )],
        };
        // Executed has empty list
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_bullet_list(vec![], source_exec())],
        };

        let after_clone = executed.clone();
        let (result, _plan) = reconcile(original, executed);

        // Result must be structurally equal to after
        assert!(
            structural_eq_blocks(&result.blocks, &after_clone.blocks),
            "Empty list should match after structure"
        );

        // Verify empty list
        if let crate::Block::BulletList(bl) = &result.blocks[0] {
            assert_eq!(bl.content.len(), 0, "Result should be empty list");
        } else {
            panic!("Expected BulletList");
        }
    }

    /// Test: Empty list gains items - result should have new items.
    #[test]
    fn list_from_empty_produces_correct_items() {
        let original = Pandoc {
            meta: Default::default(),
            blocks: vec![make_bullet_list(vec![], source_orig())],
        };
        // Executed has 2 items
        let executed = Pandoc {
            meta: Default::default(),
            blocks: vec![make_bullet_list(
                vec![
                    make_list_item("new one", source_exec()),
                    make_list_item("new two", source_exec()),
                ],
                source_exec(),
            )],
        };

        let after_clone = executed.clone();
        let (result, _plan) = reconcile(original, executed);

        // Result must be structurally equal to after
        assert!(
            structural_eq_blocks(&result.blocks, &after_clone.blocks),
            "List from empty should match after structure"
        );

        // Verify 2 items
        if let crate::Block::BulletList(bl) = &result.blocks[0] {
            assert_eq!(
                bl.content.len(),
                2,
                "Result should have 2 items from empty list"
            );
        } else {
            panic!("Expected BulletList");
        }
    }
}

/// Property-based tests for reconciliation correctness.
///
/// These tests verify the fundamental property:
/// apply_reconciliation(before, after, plan) is structurally equal to after
#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::reconcile::generators::{
        gen_full_pandoc, gen_pandoc_b0_i0, gen_pandoc_with_list, gen_pandoc_with_nested_lists,
    };
    use crate::reconcile::hash::structural_eq_blocks;
    use proptest::prelude::*;

    proptest! {
        /// B0/I0: Simple paragraphs with plain text.
        /// This should pass with the current implementation.
        #[test]
        fn reconciliation_preserves_structure_b0_i0(
            before in gen_pandoc_b0_i0(),
            after in gen_pandoc_b0_i0(),
        ) {
            let after_clone = after.clone();
            let plan = compute_reconciliation(&before, &after);
            let result = apply_reconciliation(before, after, &plan);

            prop_assert!(
                structural_eq_blocks(&result.blocks, &after_clone.blocks),
                "Result should be structurally equal to 'after'.\n\
                 Result blocks: {}\n\
                 After blocks: {}",
                result.blocks.len(),
                after_clone.blocks.len()
            );
        }

        /// B5: Lists with varying numbers of items (simple: only paragraphs in items).
        #[test]
        fn reconciliation_preserves_structure_with_lists(
            before in gen_pandoc_with_list(),
            after in gen_pandoc_with_list(),
        ) {
            let after_clone = after.clone();
            let plan = compute_reconciliation(&before, &after);
            let result = apply_reconciliation(before, after, &plan);

            prop_assert!(
                structural_eq_blocks(&result.blocks, &after_clone.blocks),
                "Result should be structurally equal to 'after'.\n\
                 Result: {:?}\n\
                 After: {:?}",
                result.blocks,
                after_clone.blocks
            );
        }

        /// Nested lists: lists can contain other lists as items.
        /// Tests reconciliation when block types inside list items differ.
        #[test]
        fn reconciliation_preserves_structure_with_nested_lists(
            before in gen_pandoc_with_nested_lists(),
            after in gen_pandoc_with_nested_lists(),
        ) {
            let after_clone = after.clone();
            let plan = compute_reconciliation(&before, &after);
            let result = apply_reconciliation(before, after, &plan);

            prop_assert!(
                structural_eq_blocks(&result.blocks, &after_clone.blocks),
                "Result should be structurally equal to 'after'.\n\
                 Result: {:?}\n\
                 After: {:?}",
                result.blocks,
                after_clone.blocks
            );
        }

        /// FULL AST: Tests with all block and inline types enabled.
        /// This is the ultimate test - if this passes with gen_full_pandoc(),
        /// the reconciliation handles all AST combinations correctly.
        #[test]
        fn reconciliation_preserves_structure_full_ast(
            before in gen_full_pandoc(),
            after in gen_full_pandoc(),
        ) {
            let after_clone = after.clone();
            let plan = compute_reconciliation(&before, &after);
            let result = apply_reconciliation(before, after, &plan);

            prop_assert!(
                structural_eq_blocks(&result.blocks, &after_clone.blocks),
                "Result should be structurally equal to 'after'.\n\
                 Result: {:?}\n\
                 After: {:?}",
                result.blocks,
                after_clone.blocks
            );
        }
    }
}
