/*
 * generators.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Property-based test generators for AST reconciliation.
 *
 * Uses feature sets to control what AST nodes can be generated,
 * enabling progressive complexity testing.
 */

use crate::{Block, Blocks, BulletList, Inline, Inlines, Pandoc, Paragraph, Space, Str};
use proptest::prelude::*;
use quarto_source_map::{FileId, SourceInfo};

// =============================================================================
// Feature Sets
// =============================================================================

/// Features available for inline generation.
#[derive(Clone, Debug, Default)]
pub struct InlineFeatures {
    pub emph: bool,
    pub strong: bool,
    pub code: bool,
    pub link: bool,
}

impl InlineFeatures {
    /// No features - just Str and Space.
    pub fn plain_text() -> Self {
        Self::default()
    }

    /// All inline features enabled.
    pub fn full() -> Self {
        Self {
            emph: true,
            strong: true,
            code: true,
            link: true,
        }
    }

    /// Check if any container features are enabled.
    pub fn has_containers(&self) -> bool {
        self.emph || self.strong || self.link
    }

    /// Remove emph for recursion.
    pub fn without_emph(&self) -> Self {
        Self {
            emph: false,
            ..self.clone()
        }
    }

    /// Remove strong for recursion.
    pub fn without_strong(&self) -> Self {
        Self {
            strong: false,
            ..self.clone()
        }
    }

    /// Remove link for recursion.
    pub fn without_link(&self) -> Self {
        Self {
            link: false,
            ..self.clone()
        }
    }
}

/// Features available for block generation.
#[derive(Clone, Debug, Default)]
pub struct BlockFeatures {
    pub paragraph: bool,
    pub header: bool,
    pub code_block: bool,
    pub blockquote: bool,
    pub bullet_list: bool,
    pub ordered_list: bool,
    pub div: bool,
}

impl BlockFeatures {
    /// Just paragraphs.
    pub fn para_only() -> Self {
        Self {
            paragraph: true,
            ..Default::default()
        }
    }

    /// All block features.
    pub fn full() -> Self {
        Self {
            paragraph: true,
            header: true,
            code_block: true,
            blockquote: true,
            bullet_list: true,
            ordered_list: true,
            div: true,
        }
    }

    /// Add lists to existing features.
    pub fn with_lists(mut self) -> Self {
        self.bullet_list = true;
        self.ordered_list = true;
        self
    }

    /// Check if any container features are enabled.
    pub fn has_containers(&self) -> bool {
        self.blockquote || self.bullet_list || self.ordered_list || self.div
    }
}

// =============================================================================
// Source Info Generation
// =============================================================================

/// Generate a dummy source info (we don't care about actual locations for testing).
fn dummy_source() -> SourceInfo {
    SourceInfo::original(FileId(0), 0, 0)
}

/// Generate a different source info (to simulate "executed" AST).
fn other_source() -> SourceInfo {
    SourceInfo::original(FileId(1), 0, 0)
}

// =============================================================================
// Text Generation
// =============================================================================

/// Generate safe text that won't create markdown syntax.
fn safe_text() -> impl Strategy<Value = String> {
    // Use simple alphanumeric strings to avoid markdown parsing issues
    "[a-zA-Z]{1,10}".prop_filter("non-empty", |s| !s.is_empty())
}

// =============================================================================
// Inline Generators
// =============================================================================

/// Generate a Str inline.
fn gen_str() -> impl Strategy<Value = Inline> {
    safe_text().prop_map(|text| {
        Inline::Str(Str {
            text,
            source_info: dummy_source(),
        })
    })
}

/// Generate a Space inline.
fn gen_space() -> impl Strategy<Value = Inline> {
    Just(Inline::Space(Space {
        source_info: dummy_source(),
    }))
}

/// Generate inlines with the given feature set.
pub fn gen_inlines(features: InlineFeatures) -> impl Strategy<Value = Inlines> {
    // For now, just generate simple text sequences
    // We always have Str and Space available
    let choices: Vec<BoxedStrategy<Inline>> = vec![gen_str().boxed(), gen_space().boxed()];

    // Build a sequence of 1-5 inlines
    proptest::collection::vec(proptest::strategy::Union::new(choices), 1..=5)
}

/// Generate a single inline sequence (for simpler testing).
pub fn gen_simple_inlines() -> impl Strategy<Value = Inlines> {
    gen_inlines(InlineFeatures::plain_text())
}

// =============================================================================
// Block Generators
// =============================================================================

/// Generate a Paragraph block.
fn gen_paragraph(inline_features: InlineFeatures) -> impl Strategy<Value = Block> {
    gen_inlines(inline_features).prop_map(|content| {
        Block::Paragraph(Paragraph {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a BulletList block with 1-4 items.
fn gen_bullet_list(inline_features: InlineFeatures) -> impl Strategy<Value = Block> {
    // Each list item is a Vec<Block> (usually just one paragraph)
    let item_gen = gen_paragraph(inline_features).prop_map(|p| vec![p]);

    // Generate 1-4 list items
    proptest::collection::vec(item_gen, 1..=4).prop_map(|content| {
        Block::BulletList(BulletList {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a BulletList with a specific number of items.
/// This is useful for testing list length changes.
pub fn gen_bullet_list_with_n_items(n: usize) -> impl Strategy<Value = Block> {
    let item_gen = gen_paragraph(InlineFeatures::plain_text()).prop_map(|p| vec![p]);

    proptest::collection::vec(item_gen, n..=n).prop_map(|content| {
        Block::BulletList(BulletList {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate blocks with the given feature sets.
pub fn gen_blocks(
    block_features: BlockFeatures,
    inline_features: InlineFeatures,
) -> impl Strategy<Value = Blocks> {
    // Build list of available block generators based on features
    let mut choices: Vec<BoxedStrategy<Block>> = vec![];

    if block_features.paragraph {
        choices.push(gen_paragraph(inline_features.clone()).boxed());
    }

    // For now, just paragraph - we'll add more in later phases

    // If no features enabled, default to paragraph
    if choices.is_empty() {
        choices.push(gen_paragraph(inline_features).boxed());
    }

    // Build a sequence of 1-5 blocks
    proptest::collection::vec(proptest::strategy::Union::new(choices), 1..=5)
}

/// Generate a single block (paragraph only).
pub fn gen_single_paragraph() -> impl Strategy<Value = Block> {
    gen_paragraph(InlineFeatures::plain_text())
}

// =============================================================================
// Pandoc AST Generator
// =============================================================================

/// Generate a complete Pandoc AST with the given feature sets.
pub fn gen_pandoc(
    block_features: BlockFeatures,
    inline_features: InlineFeatures,
) -> impl Strategy<Value = Pandoc> {
    gen_blocks(block_features, inline_features).prop_map(|blocks| Pandoc {
        meta: Default::default(),
        blocks,
    })
}

/// Generate a simple Pandoc AST (paragraphs only, plain text).
pub fn gen_simple_pandoc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::para_only(), InlineFeatures::plain_text())
}

// =============================================================================
// Complexity Levels
// =============================================================================

/// B0/I0: Single paragraph with plain text inlines.
pub fn gen_pandoc_b0_i0() -> impl Strategy<Value = Pandoc> {
    gen_simple_pandoc()
}

/// B1/I0: Multiple paragraphs with plain text inlines.
pub fn gen_pandoc_b1_i0() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::para_only(), InlineFeatures::plain_text())
}

/// B5: Pandoc with a single BulletList containing 1-4 items.
/// This level tests list reconciliation.
pub fn gen_pandoc_with_list() -> impl Strategy<Value = Pandoc> {
    gen_bullet_list(InlineFeatures::plain_text()).prop_map(|list| Pandoc {
        meta: Default::default(),
        blocks: vec![list],
    })
}

/// Generate a Pandoc with a BulletList of exactly n items.
pub fn gen_pandoc_with_list_n_items(n: usize) -> impl Strategy<Value = Pandoc> {
    gen_bullet_list_with_n_items(n).prop_map(|list| Pandoc {
        meta: Default::default(),
        blocks: vec![list],
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        #[test]
        fn gen_str_produces_valid_inline(inline in gen_str()) {
            if let Inline::Str(s) = inline {
                prop_assert!(!s.text.is_empty());
            } else {
                prop_assert!(false, "Expected Str inline");
            }
        }

        #[test]
        fn gen_simple_inlines_produces_non_empty(inlines in gen_simple_inlines()) {
            prop_assert!(!inlines.is_empty());
        }

        #[test]
        fn gen_single_paragraph_produces_paragraph(block in gen_single_paragraph()) {
            match block {
                Block::Paragraph(_) => {}
                _ => prop_assert!(false, "Expected Paragraph block"),
            }
        }

        #[test]
        fn gen_simple_pandoc_produces_non_empty(ast in gen_simple_pandoc()) {
            prop_assert!(!ast.blocks.is_empty());
        }
    }
}
