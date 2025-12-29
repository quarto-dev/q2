/*
 * generators.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Property-based test generators for CommonMark-compatible Pandoc AST subsets.
 *
 * Uses a feature set approach: generators take a set of enabled features,
 * and when using a feature (like Emph), recurse with that feature removed.
 * This prevents invalid nesting (Emph-inside-Emph) and naturally limits depth.
 */

use hashlink::LinkedHashMap;
use proptest::prelude::*;
use quarto_pandoc_types::{
    Block, BulletList, Code, CodeBlock, ConfigValue, Emph, Header, HorizontalRule, Image, Inline,
    Link, OrderedList, Pandoc, Paragraph, Plain, Str, Strong,
    attr::{AttrSourceInfo, TargetSourceInfo},
};
use quarto_source_map::{FileId, SourceInfo};

/// Create an empty source info for generated AST nodes.
fn empty_source_info() -> SourceInfo {
    SourceInfo::original(FileId(0), 0, 0)
}

/// Create an empty attribute tuple.
fn empty_attr() -> quarto_pandoc_types::Attr {
    (String::new(), vec![], LinkedHashMap::new())
}

// ============================================================================
// Feature Set Types
// ============================================================================

/// Features available for inline generation.
///
/// When a feature is used, it should be disabled before recursing to prevent
/// invalid nesting (e.g., Emph inside Emph).
#[derive(Clone, Debug, Default)]
pub struct InlineFeatures {
    pub emph: bool,
    pub strong: bool,
    pub code: bool,
    pub link: bool,
    pub image: bool,
    pub autolink: bool,
    pub linebreak: bool,
}

impl InlineFeatures {
    /// Level 0: No features - just Str, Space, SoftBreak (plain text)
    pub fn plain_text() -> Self {
        Self::default()
    }

    /// Level 1: Add Emph
    pub fn with_emph() -> Self {
        Self {
            emph: true,
            ..Self::default()
        }
    }

    /// Level 2: Add Strong
    pub fn with_strong() -> Self {
        Self {
            emph: true,
            strong: true,
            ..Self::default()
        }
    }

    /// Level 3: Add Code
    pub fn with_code() -> Self {
        Self {
            emph: true,
            strong: true,
            code: true,
            ..Self::default()
        }
    }

    /// Level 4: Add Link
    pub fn with_link() -> Self {
        Self {
            emph: true,
            strong: true,
            code: true,
            link: true,
            ..Self::default()
        }
    }

    /// Level 5: Add Image
    pub fn with_image() -> Self {
        Self {
            emph: true,
            strong: true,
            code: true,
            link: true,
            image: true,
            ..Self::default()
        }
    }

    /// Level 6: Add Autolink
    pub fn with_autolink() -> Self {
        Self {
            emph: true,
            strong: true,
            code: true,
            link: true,
            image: true,
            autolink: true,
            ..Self::default()
        }
    }

    /// Level 7: All inline features (except problematic ones)
    pub fn full() -> Self {
        Self {
            emph: true,
            strong: true,
            code: true,
            link: true,
            image: true,
            autolink: false, // Disabled: creates deeply nested structures that comrak can't parse
            linebreak: false, // Disabled: LineBreak as first/only content differs between parsers
        }
    }

    /// Remove emph feature for recursion into Emph content.
    /// Also disables strong since nested Emph/Strong is parsed differently
    /// by pampa and comrak.
    /// Also disables linebreak since `*\<newline>*` is invalid markdown.
    pub fn without_emph(&self) -> Self {
        Self {
            emph: false,
            strong: false,    // Avoid nested emphasis ambiguity
            linebreak: false, // Hard breaks inside emphasis are problematic
            ..self.clone()
        }
    }

    /// Remove strong feature for recursion into Strong content.
    /// Also disables emph since nested Strong/Emph is parsed differently
    /// by pampa and comrak.
    /// Also disables linebreak since `**\<newline>**` is invalid markdown.
    pub fn without_strong(&self) -> Self {
        Self {
            emph: false, // Avoid nested emphasis ambiguity
            strong: false,
            linebreak: false, // Hard breaks inside strong are problematic
            ..self.clone()
        }
    }

    /// Remove link feature for recursion into Link content.
    /// Also disables autolink since CommonMark doesn't allow links inside links.
    /// Also disables linebreak since hard breaks in link text are problematic.
    pub fn without_link(&self) -> Self {
        Self {
            link: false,
            autolink: false,  // CommonMark: no links inside links
            linebreak: false, // Hard breaks inside link text are problematic
            ..self.clone()
        }
    }

    /// Remove image feature for recursion into Image alt text.
    /// Also disables linebreak since pampa can't parse hard breaks in image alt.
    pub fn without_image(&self) -> Self {
        Self {
            image: false,
            linebreak: false, // Hard breaks not allowed in image alt text
            ..self.clone()
        }
    }
}

/// Features available for block generation.
#[derive(Clone, Debug, Default)]
pub struct BlockFeatures {
    pub header: bool,
    pub code_block: bool,
    pub blockquote: bool,
    pub bullet_list: bool,
    pub ordered_list: bool,
    pub horizontal_rule: bool,
}

impl BlockFeatures {
    /// Level B0: Paragraph only
    pub fn para_only() -> Self {
        Self::default()
    }

    /// Level B1: Add Header
    pub fn with_header() -> Self {
        Self {
            header: true,
            ..Self::default()
        }
    }

    /// Level B2: Add CodeBlock
    pub fn with_code_block() -> Self {
        Self {
            header: true,
            code_block: true,
            ..Self::default()
        }
    }

    /// Level B3: Add HorizontalRule
    pub fn with_hr() -> Self {
        Self {
            header: true,
            code_block: true,
            horizontal_rule: true,
            ..Self::default()
        }
    }

    /// Level B4: Add BlockQuote
    pub fn with_blockquote() -> Self {
        Self {
            header: true,
            code_block: true,
            horizontal_rule: true,
            blockquote: true,
            ..Self::default()
        }
    }

    /// Level B5: Add BulletList
    pub fn with_bullet_list() -> Self {
        Self {
            header: true,
            code_block: true,
            horizontal_rule: true,
            blockquote: true,
            bullet_list: true,
            ..Self::default()
        }
    }

    /// Level B6: All block features
    pub fn full() -> Self {
        Self {
            header: true,
            code_block: true,
            horizontal_rule: true,
            blockquote: true,
            bullet_list: true,
            ordered_list: true,
        }
    }
}

// ============================================================================
// Helper Strategies
// ============================================================================

/// Generate safe text that won't accidentally create markdown syntax.
/// Only alphanumeric characters and single spaces.
pub fn safe_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9]{1,15}"
        .prop_map(|s| s)
        .prop_filter("non-empty", |s| !s.is_empty())
}

/// Generate a safe word (no spaces).
pub fn safe_word() -> impl Strategy<Value = String> {
    "[a-zA-Z]{2,10}"
}

/// Generate a safe URL.
pub fn gen_url() -> impl Strategy<Value = String> {
    "[a-z]{3,8}".prop_map(|s| format!("https://{}.example.com", s))
}

/// Generate an optional title (empty or safe text).
pub fn gen_title() -> impl Strategy<Value = String> {
    prop_oneof![Just(String::new()), safe_text(),]
}

/// Generate an optional language for code blocks.
pub fn gen_lang() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        Just(None),
        Just(Some("python".to_string())),
        Just(Some("rust".to_string())),
        Just(Some("javascript".to_string())),
    ]
}

// ============================================================================
// Inline Generators
// ============================================================================

/// Generate a Str inline with safe text.
fn gen_str() -> impl Strategy<Value = Inline> {
    safe_word().prop_map(|text| {
        Inline::Str(Str {
            text,
            source_info: empty_source_info(),
        })
    })
}

/// Generate a sequence of inlines with the given features.
///
/// This is the main inline generator. It builds a list of possible inline
/// types based on enabled features, then selects from them.
pub fn gen_inlines(features: InlineFeatures) -> impl Strategy<Value = Vec<Inline>> {
    // We need to build a strategy that generates a sequence of inlines.
    // The approach: generate a sequence of "words" separated by spaces.
    gen_inline_sequence(features, 1, 5)
}

/// Generate a sequence of inlines with bounded length.
fn gen_inline_sequence(
    features: InlineFeatures,
    min_items: usize,
    max_items: usize,
) -> impl Strategy<Value = Vec<Inline>> {
    // Generate individual items, then flatten with spaces between
    prop::collection::vec(gen_single_inline(features), min_items..=max_items).prop_map(|items| {
        let mut result = Vec::new();
        for (i, item) in items.into_iter().enumerate() {
            if i > 0 {
                result.push(Inline::Space(quarto_pandoc_types::Space {
                    source_info: empty_source_info(),
                }));
            }
            result.push(item);
        }
        result
    })
}

/// Generate a single inline element based on features.
fn gen_single_inline(features: InlineFeatures) -> impl Strategy<Value = Inline> {
    // Base case: if no features, just generate Str
    let mut choices: Vec<(u32, BoxedStrategy<Inline>)> = vec![(5, gen_str().boxed())];

    // Add feature-based choices
    if features.code {
        choices.push((
            1,
            safe_word()
                .prop_map(|text| {
                    Inline::Code(Code {
                        text,
                        attr: empty_attr(),
                        source_info: empty_source_info(),
                        attr_source: AttrSourceInfo::empty(),
                    })
                })
                .boxed(),
        ));
    }

    if features.emph {
        let inner_features = features.without_emph();
        choices.push((
            1,
            gen_inline_sequence(inner_features, 1, 3)
                .prop_map(|content| {
                    Inline::Emph(Emph {
                        content,
                        source_info: empty_source_info(),
                    })
                })
                .boxed(),
        ));
    }

    if features.strong {
        let inner_features = features.without_strong();
        choices.push((
            1,
            gen_inline_sequence(inner_features, 1, 3)
                .prop_map(|content| {
                    Inline::Strong(Strong {
                        content,
                        source_info: empty_source_info(),
                    })
                })
                .boxed(),
        ));
    }

    if features.link {
        let inner_features = features.without_link();
        choices.push((
            1,
            (
                gen_inline_sequence(inner_features, 1, 3),
                gen_url(),
                gen_title(),
            )
                .prop_map(|(content, url, title)| {
                    Inline::Link(Link {
                        content,
                        target: (url, title),
                        attr: empty_attr(),
                        source_info: empty_source_info(),
                        attr_source: AttrSourceInfo::empty(),
                        target_source: TargetSourceInfo::empty(),
                    })
                })
                .boxed(),
        ));
    }

    if features.image {
        let inner_features = features.without_image();
        choices.push((
            1,
            (
                gen_inline_sequence(inner_features, 1, 2),
                gen_url(),
                gen_title(),
            )
                .prop_map(|(alt, url, title)| {
                    Inline::Image(Image {
                        content: alt,
                        target: (url, title),
                        attr: empty_attr(),
                        source_info: empty_source_info(),
                        attr_source: AttrSourceInfo::empty(),
                        target_source: TargetSourceInfo::empty(),
                    })
                })
                .boxed(),
        ));
    }

    if features.autolink {
        choices.push((
            1,
            gen_url()
                .prop_map(|url| {
                    Inline::Link(Link {
                        content: vec![Inline::Str(Str {
                            text: url.clone(),
                            source_info: empty_source_info(),
                        })],
                        target: (url, String::new()),
                        attr: empty_attr(),
                        source_info: empty_source_info(),
                        attr_source: AttrSourceInfo::empty(),
                        target_source: TargetSourceInfo::empty(),
                    })
                })
                .boxed(),
        ));
    }

    if features.linebreak {
        choices.push((
            1,
            Just(Inline::LineBreak(quarto_pandoc_types::LineBreak {
                source_info: empty_source_info(),
            }))
            .boxed(),
        ));
    }

    // Select one of the available choices with weights
    prop::strategy::Union::new_weighted(choices).boxed()
}

// ============================================================================
// Block Generators
// ============================================================================

/// Generate a Paragraph block.
fn gen_paragraph(inline_features: InlineFeatures) -> impl Strategy<Value = Block> {
    gen_inlines(inline_features).prop_map(|content| {
        Block::Paragraph(Paragraph {
            content,
            source_info: empty_source_info(),
        })
    })
}

/// Generate a Plain block (used in tight lists).
fn gen_plain(inline_features: InlineFeatures) -> impl Strategy<Value = Block> {
    gen_inlines(inline_features).prop_map(|content| {
        Block::Plain(Plain {
            content,
            source_info: empty_source_info(),
        })
    })
}

/// Generate a Header block.
/// Disables linebreak since comrak treats `\` in headers as literal text,
/// while pampa interprets it as LineBreak.
fn gen_header(inline_features: InlineFeatures) -> impl Strategy<Value = Block> {
    let header_features = InlineFeatures {
        linebreak: false, // Comrak treats \ in headers as literal, pampa as LineBreak
        ..inline_features
    };
    (1..=6usize, gen_inlines(header_features)).prop_map(|(level, content)| {
        Block::Header(Header {
            level,
            content,
            attr: empty_attr(),
            source_info: empty_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Generate a CodeBlock.
fn gen_code_block() -> impl Strategy<Value = Block> {
    (safe_text(), gen_lang()).prop_map(|(text, lang)| {
        let classes = lang.map(|l| vec![l]).unwrap_or_default();
        Block::CodeBlock(CodeBlock {
            text,
            attr: (String::new(), classes, LinkedHashMap::new()),
            source_info: empty_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Generate a HorizontalRule.
fn gen_horizontal_rule() -> impl Strategy<Value = Block> {
    Just(Block::HorizontalRule(HorizontalRule {
        source_info: empty_source_info(),
    }))
}

/// Generate a sequence of blocks with the given features.
pub fn gen_blocks(
    block_features: BlockFeatures,
    inline_features: InlineFeatures,
) -> impl Strategy<Value = Vec<Block>> {
    gen_block_sequence(block_features, inline_features, 1, 5)
}

/// Generate a sequence of blocks with bounded length.
/// Post-processes to ensure no two consecutive blocks are both lists
/// (consecutive lists merge in markdown, which is ambiguous).
fn gen_block_sequence(
    block_features: BlockFeatures,
    inline_features: InlineFeatures,
    min_items: usize,
    max_items: usize,
) -> impl Strategy<Value = Vec<Block>> {
    prop::collection::vec(
        gen_single_block(block_features, inline_features),
        min_items..=max_items,
    )
    .prop_map(remove_consecutive_lists)
}

/// Remove consecutive lists from a block sequence.
/// In markdown, consecutive lists merge into one, so we can't roundtrip them.
fn remove_consecutive_lists(blocks: Vec<Block>) -> Vec<Block> {
    let mut result = Vec::new();
    let mut prev_was_list = false;

    for block in blocks {
        let is_list = matches!(block, Block::BulletList(_) | Block::OrderedList(_));

        if is_list && prev_was_list {
            // Skip this list - it would merge with the previous one
            continue;
        }

        prev_was_list = is_list;
        result.push(block);
    }

    // Ensure we have at least one block
    if result.is_empty() {
        result.push(Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "placeholder".to_string(),
                source_info: empty_source_info(),
            })],
            source_info: empty_source_info(),
        }));
    }

    result
}

/// Generate a single block element based on features.
fn gen_single_block(
    block_features: BlockFeatures,
    inline_features: InlineFeatures,
) -> impl Strategy<Value = Block> {
    // Base case: always have Paragraph available
    let mut choices: Vec<(u32, BoxedStrategy<Block>)> =
        vec![(5, gen_paragraph(inline_features.clone()).boxed())];

    if block_features.header {
        choices.push((1, gen_header(inline_features.clone()).boxed()));
    }

    if block_features.code_block {
        choices.push((1, gen_code_block().boxed()));
    }

    if block_features.horizontal_rule {
        choices.push((1, gen_horizontal_rule().boxed()));
    }

    if block_features.blockquote {
        // For blockquote, we recurse but:
        // - No nested blockquotes (prevent infinite recursion at strategy construction)
        // - No HR inside blockquote (pampa interprets --- as YAML delimiters,
        //   so HR inside blockquote + HR outside causes parsing issues)
        let inner_block_features = BlockFeatures {
            blockquote: false,
            horizontal_rule: false,
            ..block_features.clone()
        };
        choices.push((
            1,
            gen_block_sequence(inner_block_features, inline_features.clone(), 1, 2)
                .prop_map(|content| {
                    Block::BlockQuote(quarto_pandoc_types::BlockQuote {
                        content,
                        source_info: empty_source_info(),
                    })
                })
                .boxed(),
        ));
    }

    if block_features.bullet_list {
        // Generate tight bullet list (items are Plain, not Paragraph)
        choices.push((
            1,
            prop::collection::vec(
                gen_plain(inline_features.clone()).prop_map(|block| vec![block]),
                2..=4,
            )
            .prop_map(|content| {
                Block::BulletList(BulletList {
                    content,
                    source_info: empty_source_info(),
                })
            })
            .boxed(),
        ));
    }

    if block_features.ordered_list {
        // Generate tight ordered list
        choices.push((
            1,
            prop::collection::vec(
                gen_plain(inline_features.clone()).prop_map(|block| vec![block]),
                2..=4,
            )
            .prop_map(|content| {
                Block::OrderedList(OrderedList {
                    content,
                    attr: (
                        1, // start number
                        quarto_pandoc_types::ListNumberStyle::Decimal,
                        quarto_pandoc_types::ListNumberDelim::Period,
                    ),
                    source_info: empty_source_info(),
                })
            })
            .boxed(),
        ));
    }

    // Weight Paragraph more heavily
    prop::strategy::Union::new_weighted(choices).boxed()
}

/// Generate a complete Pandoc document.
pub fn gen_pandoc(
    block_features: BlockFeatures,
    inline_features: InlineFeatures,
) -> impl Strategy<Value = Pandoc> {
    gen_blocks(block_features, inline_features).prop_map(|blocks| Pandoc {
        meta: ConfigValue::default(),
        blocks,
    })
}

// ============================================================================
// Preset Generators
// ============================================================================

/// Generate a plain text document (L0, B0).
pub fn gen_plain_text_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::para_only(), InlineFeatures::plain_text())
}

/// Generate a document with emphasis (L1, B0).
pub fn gen_with_emph_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::para_only(), InlineFeatures::with_emph())
}

/// Generate a document with strong emphasis (L2, B0).
pub fn gen_with_strong_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::para_only(), InlineFeatures::with_strong())
}

/// Generate a document with inline code (L3, B0).
pub fn gen_with_code_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::para_only(), InlineFeatures::with_code())
}

/// Generate a document with links (L4, B0).
pub fn gen_with_link_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::para_only(), InlineFeatures::with_link())
}

/// Generate a document with images (L5, B0).
pub fn gen_with_image_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::para_only(), InlineFeatures::with_image())
}

/// Generate a document with headers (L3, B1).
pub fn gen_with_header_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::with_header(), InlineFeatures::with_code())
}

/// Generate a document with code blocks (L3, B2).
pub fn gen_with_code_block_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(
        BlockFeatures::with_code_block(),
        InlineFeatures::with_code(),
    )
}

/// Generate a document with all non-recursive blocks (L3, B3).
pub fn gen_with_hr_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::with_hr(), InlineFeatures::with_code())
}

/// Generate a document with blockquotes (L3, B4).
pub fn gen_with_blockquote_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(
        BlockFeatures::with_blockquote(),
        InlineFeatures::with_code(),
    )
}

/// Generate a document with bullet lists (L3, B5).
pub fn gen_with_bullet_list_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(
        BlockFeatures::with_bullet_list(),
        InlineFeatures::with_code(),
    )
}

/// Generate a full document (all features).
pub fn gen_full_doc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(BlockFeatures::full(), InlineFeatures::full())
}
