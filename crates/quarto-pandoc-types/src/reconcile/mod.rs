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
mod hash;
mod types;

pub use apply::apply_reconciliation;
pub use compute::compute_reconciliation;
pub use hash::{
    compute_block_hash_fresh, compute_inline_hash_fresh, structural_eq_block, structural_eq_inline,
    HashCache,
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
            attr: (String::new(), vec!["cell".to_string()], LinkedHashMap::new()),
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
        assert!(plan.stats.blocks_kept >= 2, "Expected at least 2 blocks kept");
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
}
