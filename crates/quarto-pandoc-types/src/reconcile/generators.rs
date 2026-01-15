/*
 * generators.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Property-based test generators for AST reconciliation.
 *
 * Uses a depth-limited recursive generation strategy to produce any valid AST
 * with positive probability while ensuring termination. Feature flags allow
 * targeted testing of specific AST subsets.
 *
 * Design principles:
 * - Every AST node type should be reachable with positive probability
 * - Depth parameter prevents infinite recursion in container types
 * - At depth=0, only leaf nodes are generated
 * - At depth>0, containers can be generated with depth-1 for children
 * - Feature flags allow selective enabling/disabling of node types
 */

use crate::attr::{Attr, AttrSourceInfo, TargetSourceInfo, empty_attr};
use crate::caption::Caption;
use crate::inline::{
    Citation, CitationMode, Cite, Delete, EditComment, Highlight, Insert, Note, NoteReference,
};
use crate::list::{ListAttributes, ListNumberDelim, ListNumberStyle};
use crate::shortcode::{Shortcode, ShortcodeArg};
use crate::{
    Block, BlockQuote, Blocks, BulletList, CodeBlock, DefinitionList, Div, Emph, Figure, Header,
    HorizontalRule, Inline, Inlines, LineBlock, LineBreak, Link, Math, MathType, OrderedList,
    Pandoc, Paragraph, Plain, QuoteType, Quoted, RawBlock, RawInline, SoftBreak, Space, Span, Str,
    Strikeout, Strong, Subscript, Superscript, Underline,
};
use crate::{CaptionBlock, NoteDefinitionFencedBlock, NoteDefinitionPara};
use crate::{Code, Image, SmallCaps, Target};
use hashlink::LinkedHashMap;
use proptest::prelude::*;
use quarto_source_map::{FileId, SourceInfo};

// =============================================================================
// Generation Configuration
// =============================================================================

/// Configuration for AST generation.
///
/// Controls depth limits, breadth limits, and which node types are enabled.
#[derive(Clone, Debug)]
pub struct GenConfig {
    /// Maximum recursion depth for container nodes.
    /// At depth=0, only leaf nodes are generated.
    /// At depth>0, containers can recurse with depth-1.
    pub max_depth: usize,

    /// Maximum number of children at each level.
    pub max_children: usize,

    /// Which block types can be generated.
    pub block_features: BlockFeatures,

    /// Which inline types can be generated.
    pub inline_features: InlineFeatures,
}

impl Default for GenConfig {
    fn default() -> Self {
        Self {
            max_depth: 2,
            max_children: 4,
            block_features: BlockFeatures::default(),
            inline_features: InlineFeatures::default(),
        }
    }
}

impl GenConfig {
    /// Create a minimal config for simple testing (paragraphs with plain text).
    pub fn minimal() -> Self {
        Self {
            max_depth: 0,
            max_children: 3,
            block_features: BlockFeatures::para_only(),
            inline_features: InlineFeatures::plain_text(),
        }
    }

    /// Create a config with all features enabled.
    pub fn full() -> Self {
        Self {
            max_depth: 3,
            max_children: 4,
            block_features: BlockFeatures::full(),
            inline_features: InlineFeatures::full(),
        }
    }

    /// Create a config for testing lists specifically.
    pub fn with_lists() -> Self {
        Self {
            max_depth: 2,
            max_children: 4,
            block_features: BlockFeatures::para_only().with_lists(),
            inline_features: InlineFeatures::plain_text(),
        }
    }

    /// Return a new config with depth decremented by 1 (for recursive calls).
    pub fn descend(&self) -> Self {
        Self {
            max_depth: self.max_depth.saturating_sub(1),
            ..self.clone()
        }
    }

    /// Check if we can generate container types at this depth.
    pub fn can_recurse(&self) -> bool {
        self.max_depth > 0
    }
}

// =============================================================================
// Feature Sets
// =============================================================================

/// Features available for inline generation.
#[derive(Clone, Debug, Default)]
pub struct InlineFeatures {
    // Leaf inlines (always available at any depth)
    pub str_: bool,
    pub space: bool,
    pub soft_break: bool,
    pub line_break: bool,
    pub code: bool,
    pub math: bool,
    pub raw_inline: bool,
    pub note_reference: bool,
    pub shortcode: bool, // Quarto extension - leaf inline with recursive args

    // Container inlines (require depth > 0)
    pub emph: bool,
    pub strong: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub superscript: bool,
    pub subscript: bool,
    pub smallcaps: bool,
    pub quoted: bool,
    pub span: bool,
    pub link: bool,
    pub image: bool,

    // Special container inlines
    pub cite: bool,
    pub note: bool, // Note is special: inline that contains blocks

    // CriticMarkup inlines (containers with attr + inlines)
    pub insert: bool,
    pub delete: bool,
    pub highlight: bool,
    pub edit_comment: bool,
}

impl InlineFeatures {
    /// No features - just Str and Space.
    pub fn plain_text() -> Self {
        Self {
            str_: true,
            space: true,
            ..Default::default()
        }
    }

    /// All leaf inlines (no containers).
    pub fn all_leaves() -> Self {
        Self {
            str_: true,
            space: true,
            soft_break: true,
            line_break: true,
            code: true,
            math: true,
            raw_inline: true,
            note_reference: true,
            shortcode: true,
            ..Default::default()
        }
    }

    /// All inline features enabled.
    pub fn full() -> Self {
        Self {
            str_: true,
            space: true,
            soft_break: true,
            line_break: true,
            code: true,
            math: true,
            raw_inline: true,
            note_reference: true,
            shortcode: true,
            emph: true,
            strong: true,
            underline: true,
            strikeout: true,
            superscript: true,
            subscript: true,
            smallcaps: true,
            quoted: true,
            span: true,
            link: true,
            image: true,
            cite: true,
            note: true,
            insert: true,
            delete: true,
            highlight: true,
            edit_comment: true,
        }
    }

    /// Check if any container features are enabled.
    pub fn has_containers(&self) -> bool {
        self.emph
            || self.strong
            || self.underline
            || self.strikeout
            || self.superscript
            || self.subscript
            || self.smallcaps
            || self.quoted
            || self.span
            || self.link
            || self.image
            || self.cite
            || self.note
            || self.insert
            || self.delete
            || self.highlight
            || self.edit_comment
    }
}

/// Features available for block generation.
#[derive(Clone, Debug, Default)]
pub struct BlockFeatures {
    // Leaf blocks (always available)
    pub paragraph: bool,
    pub plain: bool,
    pub code_block: bool,
    pub raw_block: bool,
    pub horizontal_rule: bool,
    pub header: bool,
    pub line_block: bool,

    // Special leaf blocks (Quarto extensions)
    pub caption_block: bool,
    pub note_definition_para: bool,
    pub note_definition_fenced: bool,

    // Container blocks (require depth > 0)
    pub blockquote: bool,
    pub bullet_list: bool,
    pub ordered_list: bool,
    pub div: bool,
    pub definition_list: bool,
    pub figure: bool,
}

impl BlockFeatures {
    /// Just paragraphs.
    pub fn para_only() -> Self {
        Self {
            paragraph: true,
            ..Default::default()
        }
    }

    /// All leaf blocks (no containers).
    pub fn all_leaves() -> Self {
        Self {
            paragraph: true,
            plain: true,
            code_block: true,
            raw_block: true,
            horizontal_rule: true,
            header: true,
            line_block: true,
            caption_block: true,
            note_definition_para: true,
            note_definition_fenced: true,
            ..Default::default()
        }
    }

    /// All block features.
    pub fn full() -> Self {
        Self {
            paragraph: true,
            plain: true,
            code_block: true,
            raw_block: true,
            horizontal_rule: true,
            header: true,
            line_block: true,
            caption_block: true,
            note_definition_para: true,
            note_definition_fenced: true,
            blockquote: true,
            bullet_list: true,
            ordered_list: true,
            div: true,
            definition_list: true,
            figure: true,
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
        self.blockquote
            || self.bullet_list
            || self.ordered_list
            || self.div
            || self.definition_list
            || self.figure
            || self.note_definition_fenced
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
#[allow(dead_code)]
fn other_source() -> SourceInfo {
    SourceInfo::original(FileId(1), 0, 0)
}

// =============================================================================
// Helper Generators
// =============================================================================

/// Generate safe text that won't create markdown syntax.
fn gen_safe_text() -> impl Strategy<Value = String> {
    // Use simple alphanumeric strings to avoid markdown parsing issues
    "[a-zA-Z]{1,10}".prop_filter("non-empty", |s| !s.is_empty())
}

/// Generate a simple identifier (for IDs, class names, etc.).
fn gen_identifier() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()), // Empty is valid
        "[a-z]{1,8}",        // Simple lowercase
    ]
}

/// Generate a class name.
fn gen_class() -> impl Strategy<Value = String> {
    "[a-z]{1,6}"
}

/// Generate an Attr (id, classes, attributes).
fn gen_attr() -> impl Strategy<Value = Attr> {
    (
        gen_identifier(),
        proptest::collection::vec(gen_class(), 0..=2),
        Just(LinkedHashMap::new()), // Keep attributes simple for now
    )
}

/// Generate an empty Attr.
fn gen_empty_attr() -> impl Strategy<Value = Attr> {
    Just(empty_attr())
}

/// Generate a Target (url, title).
fn gen_target() -> impl Strategy<Value = Target> {
    prop_oneof![
        // Simple URL without title
        gen_safe_text().prop_map(|url| (url, String::new())),
        // URL with title
        (gen_safe_text(), gen_safe_text()).prop_map(|(url, title)| (url, title)),
    ]
}

/// Generate ListAttributes for ordered lists.
fn gen_list_attributes() -> impl Strategy<Value = ListAttributes> {
    (
        1..=10usize, // Starting number
        prop_oneof![
            Just(ListNumberStyle::Default),
            Just(ListNumberStyle::Decimal),
            Just(ListNumberStyle::LowerAlpha),
            Just(ListNumberStyle::UpperAlpha),
            Just(ListNumberStyle::LowerRoman),
            Just(ListNumberStyle::UpperRoman),
        ],
        prop_oneof![
            Just(ListNumberDelim::Default),
            Just(ListNumberDelim::Period),
            Just(ListNumberDelim::OneParen),
            Just(ListNumberDelim::TwoParens),
        ],
    )
}

/// Generate a QuoteType.
fn gen_quote_type() -> impl Strategy<Value = QuoteType> {
    prop_oneof![Just(QuoteType::SingleQuote), Just(QuoteType::DoubleQuote),]
}

/// Generate a MathType.
fn gen_math_type() -> impl Strategy<Value = MathType> {
    prop_oneof![Just(MathType::InlineMath), Just(MathType::DisplayMath),]
}

/// Generate a format string for raw blocks/inlines.
fn gen_format() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("html".to_string()),
        Just("latex".to_string()),
        Just("tex".to_string()),
    ]
}

/// Generate code/math content (simple alphanumeric).
fn gen_code_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_ ]{1,20}"
}

/// Generate a note ID (for note definitions and references).
fn gen_note_id() -> impl Strategy<Value = String> {
    "[a-z]{1,6}"
}

/// Generate a CitationMode.
fn gen_citation_mode() -> impl Strategy<Value = CitationMode> {
    prop_oneof![
        Just(CitationMode::NormalCitation),
        Just(CitationMode::AuthorInText),
        Just(CitationMode::SuppressAuthor),
    ]
}

/// Generate a Citation.
fn gen_citation(config: GenConfig) -> impl Strategy<Value = Citation> {
    (
        gen_identifier(),                  // id
        gen_inlines_inner(config.clone()), // prefix
        gen_inlines_inner(config),         // suffix
        gen_citation_mode(),               // mode
        0..10usize,                        // note_num
        0..1000usize,                      // hash
    )
        .prop_map(|(id, prefix, suffix, mode, note_num, hash)| Citation {
            id,
            prefix,
            suffix,
            mode,
            note_num,
            hash,
            id_source: None,
        })
}

/// Generate a Caption (for figures and tables).
fn gen_caption(config: GenConfig) -> impl Strategy<Value = Caption> {
    (
        proptest::option::of(gen_inlines_inner(config.clone())), // short
        proptest::option::of(gen_blocks_inner(config)),          // long
    )
        .prop_map(|(short, long)| Caption {
            short,
            long,
            source_info: dummy_source(),
        })
}

// =============================================================================
// Leaf Inline Generators
// =============================================================================

/// Generate a Str inline.
fn gen_str() -> impl Strategy<Value = Inline> {
    gen_safe_text().prop_map(|text| {
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

/// Generate a SoftBreak inline.
fn gen_soft_break() -> impl Strategy<Value = Inline> {
    Just(Inline::SoftBreak(SoftBreak {
        source_info: dummy_source(),
    }))
}

/// Generate a LineBreak inline.
fn gen_line_break() -> impl Strategy<Value = Inline> {
    Just(Inline::LineBreak(LineBreak {
        source_info: dummy_source(),
    }))
}

/// Generate a Code inline.
fn gen_code_inline() -> impl Strategy<Value = Inline> {
    (gen_attr(), gen_code_text()).prop_map(|(attr, text)| {
        Inline::Code(Code {
            attr,
            text,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Generate a Math inline.
fn gen_math() -> impl Strategy<Value = Inline> {
    (gen_math_type(), gen_code_text()).prop_map(|(math_type, text)| {
        Inline::Math(Math {
            math_type,
            text,
            source_info: dummy_source(),
        })
    })
}

/// Generate a RawInline.
fn gen_raw_inline() -> impl Strategy<Value = Inline> {
    (gen_format(), gen_code_text()).prop_map(|(format, text)| {
        Inline::RawInline(RawInline {
            format,
            text,
            source_info: dummy_source(),
        })
    })
}

/// Generate a NoteReference inline.
fn gen_note_reference() -> impl Strategy<Value = Inline> {
    gen_note_id().prop_map(|id| {
        Inline::NoteReference(NoteReference {
            id,
            source_info: dummy_source(),
        })
    })
}

/// Generate a ShortcodeArg (recursive structure, depth-limited).
///
/// At depth 0, only generates leaf args (String, Number, Boolean).
/// At depth > 0, can also generate nested Shortcodes and KeyValue maps.
fn gen_shortcode_arg(depth: usize) -> BoxedStrategy<ShortcodeArg> {
    if depth == 0 {
        // Leaf args only
        prop_oneof![
            "[a-z]{1,10}".prop_map(ShortcodeArg::String),
            (-100.0f64..100.0f64).prop_map(ShortcodeArg::Number),
            proptest::bool::ANY.prop_map(ShortcodeArg::Boolean),
        ]
        .boxed()
    } else {
        // Include recursive variants
        prop_oneof![
            3 => "[a-z]{1,10}".prop_map(ShortcodeArg::String),
            2 => (-100.0f64..100.0f64).prop_map(ShortcodeArg::Number),
            2 => proptest::bool::ANY.prop_map(ShortcodeArg::Boolean),
            1 => gen_shortcode_inner(depth - 1).prop_map(ShortcodeArg::Shortcode),
            1 => gen_shortcode_keyvalue(depth - 1).prop_map(ShortcodeArg::KeyValue),
        ]
        .boxed()
    }
}

/// Generate a KeyValue map for ShortcodeArg::KeyValue.
fn gen_shortcode_keyvalue(
    depth: usize,
) -> impl Strategy<Value = std::collections::HashMap<String, ShortcodeArg>> {
    proptest::collection::hash_map("[a-z]{1,8}", gen_shortcode_arg(depth), 0..3)
}

/// Generate a Shortcode (inner, for recursion).
fn gen_shortcode_inner(depth: usize) -> impl Strategy<Value = Shortcode> {
    let pos_args = proptest::collection::vec(gen_shortcode_arg(depth), 0..3);
    let kw_args = proptest::collection::hash_map("[a-z]{1,8}", gen_shortcode_arg(depth), 0..3);

    (proptest::bool::ANY, "[a-z]{1,12}", pos_args, kw_args).prop_map(
        |(is_escaped, name, positional_args, keyword_args)| Shortcode {
            is_escaped,
            name,
            positional_args,
            keyword_args,
        },
    )
}

/// Generate a Shortcode inline (Quarto extension).
///
/// Shortcodes are leaf inlines (no nested Inlines), but they can have
/// recursive args (ShortcodeArg can contain nested Shortcodes).
fn gen_shortcode() -> impl Strategy<Value = Inline> {
    // Use depth 2 to allow some nesting in args
    gen_shortcode_inner(2).prop_map(Inline::Shortcode)
}

// =============================================================================
// Container Inline Generators (require recursion)
// =============================================================================

/// Generate an Emph inline containing other inlines.
fn gen_emph(config: GenConfig) -> impl Strategy<Value = Inline> {
    gen_inlines_inner(config).prop_map(|content| {
        Inline::Emph(Emph {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a Strong inline containing other inlines.
fn gen_strong(config: GenConfig) -> impl Strategy<Value = Inline> {
    gen_inlines_inner(config).prop_map(|content| {
        Inline::Strong(Strong {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate an Underline inline containing other inlines.
fn gen_underline(config: GenConfig) -> impl Strategy<Value = Inline> {
    gen_inlines_inner(config).prop_map(|content| {
        Inline::Underline(Underline {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a Strikeout inline containing other inlines.
fn gen_strikeout(config: GenConfig) -> impl Strategy<Value = Inline> {
    gen_inlines_inner(config).prop_map(|content| {
        Inline::Strikeout(Strikeout {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a Superscript inline containing other inlines.
fn gen_superscript(config: GenConfig) -> impl Strategy<Value = Inline> {
    gen_inlines_inner(config).prop_map(|content| {
        Inline::Superscript(Superscript {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a Subscript inline containing other inlines.
fn gen_subscript(config: GenConfig) -> impl Strategy<Value = Inline> {
    gen_inlines_inner(config).prop_map(|content| {
        Inline::Subscript(Subscript {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a SmallCaps inline containing other inlines.
fn gen_smallcaps(config: GenConfig) -> impl Strategy<Value = Inline> {
    gen_inlines_inner(config).prop_map(|content| {
        Inline::SmallCaps(SmallCaps {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a Quoted inline containing other inlines.
fn gen_quoted(config: GenConfig) -> impl Strategy<Value = Inline> {
    (gen_quote_type(), gen_inlines_inner(config)).prop_map(|(quote_type, content)| {
        Inline::Quoted(Quoted {
            quote_type,
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a Span inline containing other inlines.
fn gen_span(config: GenConfig) -> impl Strategy<Value = Inline> {
    (gen_attr(), gen_inlines_inner(config)).prop_map(|(attr, content)| {
        Inline::Span(Span {
            attr,
            content,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Generate a Link inline containing other inlines.
fn gen_link(config: GenConfig) -> impl Strategy<Value = Inline> {
    (gen_attr(), gen_inlines_inner(config), gen_target()).prop_map(|(attr, content, target)| {
        Inline::Link(Link {
            attr,
            content,
            target,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    })
}

/// Generate an Image inline containing other inlines (for alt text).
fn gen_image(config: GenConfig) -> impl Strategy<Value = Inline> {
    (gen_attr(), gen_inlines_inner(config), gen_target()).prop_map(|(attr, content, target)| {
        Inline::Image(Image {
            attr,
            content,
            target,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    })
}

/// Generate a Cite inline containing citations.
fn gen_cite(config: GenConfig) -> impl Strategy<Value = Inline> {
    let max_children = config.max_children;
    (
        proptest::collection::vec(gen_citation(config.clone()), 1..=max_children),
        gen_inlines_inner(config),
    )
        .prop_map(|(citations, content)| {
            Inline::Cite(Cite {
                citations,
                content,
                source_info: dummy_source(),
            })
        })
}

/// Generate a Note inline containing blocks.
/// Note is special: it's an inline that contains blocks (for footnotes).
fn gen_note(config: GenConfig) -> impl Strategy<Value = Inline> {
    gen_blocks_inner(config).prop_map(|content| {
        Inline::Note(Note {
            content,
            source_info: dummy_source(),
        })
    })
}

// =============================================================================
// CriticMarkup Inline Generators
// =============================================================================

/// Generate an Insert inline (CriticMarkup addition).
fn gen_insert(config: GenConfig) -> impl Strategy<Value = Inline> {
    (gen_attr(), gen_inlines_inner(config)).prop_map(|(attr, content)| {
        Inline::Insert(Insert {
            attr,
            content,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Generate a Delete inline (CriticMarkup deletion).
fn gen_delete(config: GenConfig) -> impl Strategy<Value = Inline> {
    (gen_attr(), gen_inlines_inner(config)).prop_map(|(attr, content)| {
        Inline::Delete(Delete {
            attr,
            content,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Generate a Highlight inline (CriticMarkup highlight).
fn gen_highlight(config: GenConfig) -> impl Strategy<Value = Inline> {
    (gen_attr(), gen_inlines_inner(config)).prop_map(|(attr, content)| {
        Inline::Highlight(Highlight {
            attr,
            content,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Generate an EditComment inline (CriticMarkup comment).
fn gen_edit_comment(config: GenConfig) -> impl Strategy<Value = Inline> {
    (gen_attr(), gen_inlines_inner(config)).prop_map(|(attr, content)| {
        Inline::EditComment(EditComment {
            attr,
            content,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

// =============================================================================
// Inline Collection Generators
// =============================================================================

/// Internal: Generate a single inline based on config and depth.
fn gen_inline(config: &GenConfig) -> BoxedStrategy<Inline> {
    let features = &config.inline_features;
    let mut choices: Vec<BoxedStrategy<Inline>> = vec![];

    // Always add leaf inlines based on features
    if features.str_ {
        choices.push(gen_str().boxed());
    }
    if features.space {
        choices.push(gen_space().boxed());
    }
    if features.soft_break {
        choices.push(gen_soft_break().boxed());
    }
    if features.line_break {
        choices.push(gen_line_break().boxed());
    }
    if features.code {
        choices.push(gen_code_inline().boxed());
    }
    if features.math {
        choices.push(gen_math().boxed());
    }
    if features.raw_inline {
        choices.push(gen_raw_inline().boxed());
    }
    if features.note_reference {
        choices.push(gen_note_reference().boxed());
    }
    if features.shortcode {
        choices.push(gen_shortcode().boxed());
    }

    // Add container inlines only if we can recurse
    if config.can_recurse() {
        let child_config = config.descend();

        if features.emph {
            choices.push(gen_emph(child_config.clone()).boxed());
        }
        if features.strong {
            choices.push(gen_strong(child_config.clone()).boxed());
        }
        if features.underline {
            choices.push(gen_underline(child_config.clone()).boxed());
        }
        if features.strikeout {
            choices.push(gen_strikeout(child_config.clone()).boxed());
        }
        if features.superscript {
            choices.push(gen_superscript(child_config.clone()).boxed());
        }
        if features.subscript {
            choices.push(gen_subscript(child_config.clone()).boxed());
        }
        if features.smallcaps {
            choices.push(gen_smallcaps(child_config.clone()).boxed());
        }
        if features.quoted {
            choices.push(gen_quoted(child_config.clone()).boxed());
        }
        if features.span {
            choices.push(gen_span(child_config.clone()).boxed());
        }
        if features.link {
            choices.push(gen_link(child_config.clone()).boxed());
        }
        if features.image {
            choices.push(gen_image(child_config.clone()).boxed());
        }
        if features.cite {
            choices.push(gen_cite(child_config.clone()).boxed());
        }
        if features.note {
            choices.push(gen_note(child_config.clone()).boxed());
        }
        // CriticMarkup inlines
        if features.insert {
            choices.push(gen_insert(child_config.clone()).boxed());
        }
        if features.delete {
            choices.push(gen_delete(child_config.clone()).boxed());
        }
        if features.highlight {
            choices.push(gen_highlight(child_config.clone()).boxed());
        }
        if features.edit_comment {
            choices.push(gen_edit_comment(child_config.clone()).boxed());
        }
    }

    // Default to Str if no features enabled
    if choices.is_empty() {
        choices.push(gen_str().boxed());
    }

    proptest::strategy::Union::new(choices).boxed()
}

/// Internal: Generate inlines for use inside containers.
fn gen_inlines_inner(config: GenConfig) -> impl Strategy<Value = Inlines> {
    let max_children = config.max_children;
    let strategy = gen_inline(&config);
    proptest::collection::vec(strategy, 1..=max_children)
}

/// Generate inlines with the given config.
pub fn gen_inlines(config: GenConfig) -> impl Strategy<Value = Inlines> {
    gen_inlines_inner(config)
}

/// Generate a single inline sequence (for simpler testing).
pub fn gen_simple_inlines() -> impl Strategy<Value = Inlines> {
    gen_inlines(GenConfig::minimal())
}

// =============================================================================
// Leaf Block Generators
// =============================================================================

/// Generate a Paragraph block.
fn gen_paragraph(config: GenConfig) -> impl Strategy<Value = Block> {
    gen_inlines_inner(config).prop_map(|content| {
        Block::Paragraph(Paragraph {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a Plain block.
fn gen_plain(config: GenConfig) -> impl Strategy<Value = Block> {
    gen_inlines_inner(config).prop_map(|content| {
        Block::Plain(Plain {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a CodeBlock.
fn gen_code_block() -> impl Strategy<Value = Block> {
    (gen_attr(), gen_code_text()).prop_map(|(attr, text)| {
        Block::CodeBlock(CodeBlock {
            attr,
            text,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Generate a RawBlock.
fn gen_raw_block() -> impl Strategy<Value = Block> {
    (gen_format(), gen_code_text()).prop_map(|(format, text)| {
        Block::RawBlock(RawBlock {
            format,
            text,
            source_info: dummy_source(),
        })
    })
}

/// Generate a HorizontalRule.
fn gen_horizontal_rule() -> impl Strategy<Value = Block> {
    Just(Block::HorizontalRule(HorizontalRule {
        source_info: dummy_source(),
    }))
}

/// Generate a Header block.
fn gen_header(config: GenConfig) -> impl Strategy<Value = Block> {
    (1..=6usize, gen_attr(), gen_inlines_inner(config)).prop_map(|(level, attr, content)| {
        Block::Header(Header {
            level,
            attr,
            content,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Generate a LineBlock.
fn gen_line_block(config: GenConfig) -> impl Strategy<Value = Block> {
    let max_children = config.max_children;
    proptest::collection::vec(gen_inlines_inner(config), 1..=max_children).prop_map(|content| {
        Block::LineBlock(LineBlock {
            content,
            source_info: dummy_source(),
        })
    })
}

// =============================================================================
// Container Block Generators (require recursion)
// =============================================================================

/// Generate a BlockQuote containing blocks.
fn gen_blockquote(config: GenConfig) -> impl Strategy<Value = Block> {
    gen_blocks_inner(config).prop_map(|content| {
        Block::BlockQuote(BlockQuote {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a BulletList block.
fn gen_bullet_list(config: GenConfig) -> impl Strategy<Value = Block> {
    let max_children = config.max_children;
    // Each list item is a Vec<Block>
    proptest::collection::vec(gen_blocks_inner(config), 1..=max_children).prop_map(|content| {
        Block::BulletList(BulletList {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a BulletList with a specific number of items.
pub fn gen_bullet_list_with_n_items(n: usize) -> impl Strategy<Value = Block> {
    let config = GenConfig::minimal();
    let item_gen = gen_paragraph(config).prop_map(|p| vec![p]);

    proptest::collection::vec(item_gen, n..=n).prop_map(|content| {
        Block::BulletList(BulletList {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate an OrderedList block.
fn gen_ordered_list(config: GenConfig) -> impl Strategy<Value = Block> {
    let max_children = config.max_children;
    (
        gen_list_attributes(),
        proptest::collection::vec(gen_blocks_inner(config), 1..=max_children),
    )
        .prop_map(|(attr, content)| {
            Block::OrderedList(OrderedList {
                attr,
                content,
                source_info: dummy_source(),
            })
        })
}

/// Generate a Div block.
fn gen_div(config: GenConfig) -> impl Strategy<Value = Block> {
    (gen_attr(), gen_blocks_inner(config)).prop_map(|(attr, content)| {
        Block::Div(Div {
            attr,
            content,
            source_info: dummy_source(),
            attr_source: AttrSourceInfo::empty(),
        })
    })
}

/// Generate a DefinitionList block.
/// Each item is a (term, definitions) pair where term is Inlines and definitions is Vec<Blocks>.
fn gen_definition_list(config: GenConfig) -> impl Strategy<Value = Block> {
    let max_children = config.max_children;
    // Generate a single definition item: (term, definitions)
    let item_gen = (
        gen_inlines_inner(config.clone()),
        proptest::collection::vec(gen_blocks_inner(config.clone()), 1..=2),
    );
    proptest::collection::vec(item_gen, 1..=max_children).prop_map(|content| {
        Block::DefinitionList(DefinitionList {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a Figure block with caption and content.
fn gen_figure(config: GenConfig) -> impl Strategy<Value = Block> {
    (
        gen_attr(),
        gen_caption(config.clone()),
        gen_blocks_inner(config),
    )
        .prop_map(|(attr, caption, content)| {
            Block::Figure(Figure {
                attr,
                caption,
                content,
                source_info: dummy_source(),
                attr_source: AttrSourceInfo::empty(),
            })
        })
}

// =============================================================================
// Special Block Generators (Quarto extensions)
// =============================================================================

/// Generate a CaptionBlock (leaf block with inline content).
fn gen_caption_block(config: GenConfig) -> impl Strategy<Value = Block> {
    gen_inlines_inner(config).prop_map(|content| {
        Block::CaptionBlock(CaptionBlock {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a NoteDefinitionPara (leaf block with ID and inline content).
fn gen_note_definition_para(config: GenConfig) -> impl Strategy<Value = Block> {
    (gen_note_id(), gen_inlines_inner(config)).prop_map(|(id, content)| {
        Block::NoteDefinitionPara(NoteDefinitionPara {
            id,
            content,
            source_info: dummy_source(),
        })
    })
}

/// Generate a NoteDefinitionFencedBlock (container block with ID and block content).
fn gen_note_definition_fenced(config: GenConfig) -> impl Strategy<Value = Block> {
    (gen_note_id(), gen_blocks_inner(config)).prop_map(|(id, content)| {
        Block::NoteDefinitionFencedBlock(NoteDefinitionFencedBlock {
            id,
            content,
            source_info: dummy_source(),
        })
    })
}

// =============================================================================
// Block Collection Generators
// =============================================================================

/// Internal: Generate a single block based on config.
fn gen_block(config: &GenConfig) -> BoxedStrategy<Block> {
    let features = &config.block_features;
    let mut choices: Vec<BoxedStrategy<Block>> = vec![];

    // Add leaf blocks based on features
    if features.paragraph {
        choices.push(gen_paragraph(config.clone()).boxed());
    }
    if features.plain {
        choices.push(gen_plain(config.clone()).boxed());
    }
    if features.code_block {
        choices.push(gen_code_block().boxed());
    }
    if features.raw_block {
        choices.push(gen_raw_block().boxed());
    }
    if features.horizontal_rule {
        choices.push(gen_horizontal_rule().boxed());
    }
    if features.header {
        choices.push(gen_header(config.clone()).boxed());
    }
    if features.line_block {
        choices.push(gen_line_block(config.clone()).boxed());
    }
    // Special leaf blocks (Quarto extensions)
    if features.caption_block {
        choices.push(gen_caption_block(config.clone()).boxed());
    }
    if features.note_definition_para {
        choices.push(gen_note_definition_para(config.clone()).boxed());
    }

    // Add container blocks only if we can recurse
    if config.can_recurse() {
        let child_config = config.descend();

        if features.blockquote {
            choices.push(gen_blockquote(child_config.clone()).boxed());
        }
        if features.bullet_list {
            choices.push(gen_bullet_list(child_config.clone()).boxed());
        }
        if features.ordered_list {
            choices.push(gen_ordered_list(child_config.clone()).boxed());
        }
        if features.div {
            choices.push(gen_div(child_config.clone()).boxed());
        }
        if features.definition_list {
            choices.push(gen_definition_list(child_config.clone()).boxed());
        }
        if features.figure {
            choices.push(gen_figure(child_config.clone()).boxed());
        }
        if features.note_definition_fenced {
            choices.push(gen_note_definition_fenced(child_config.clone()).boxed());
        }
    }

    // Default to paragraph if no features enabled
    if choices.is_empty() {
        choices.push(gen_paragraph(config.clone()).boxed());
    }

    proptest::strategy::Union::new(choices).boxed()
}

/// Internal: Generate blocks for use inside containers.
fn gen_blocks_inner(config: GenConfig) -> impl Strategy<Value = Blocks> {
    let max_children = config.max_children;
    let strategy = gen_block(&config);
    proptest::collection::vec(strategy, 1..=max_children)
}

/// Generate blocks with the given config.
pub fn gen_blocks(config: GenConfig) -> impl Strategy<Value = Blocks> {
    gen_blocks_inner(config)
}

/// Generate a single block (paragraph only).
pub fn gen_single_paragraph() -> impl Strategy<Value = Block> {
    gen_paragraph(GenConfig::minimal())
}

// =============================================================================
// Pandoc AST Generator
// =============================================================================

/// Generate a complete Pandoc AST with the given config.
pub fn gen_pandoc(config: GenConfig) -> impl Strategy<Value = Pandoc> {
    gen_blocks_inner(config).prop_map(|blocks| Pandoc {
        meta: Default::default(),
        blocks,
    })
}

/// Generate a simple Pandoc AST (paragraphs only, plain text).
pub fn gen_simple_pandoc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(GenConfig::minimal())
}

/// Generate a Pandoc AST with all features enabled.
pub fn gen_full_pandoc() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(GenConfig::full())
}

// =============================================================================
// Convenience Functions for Complexity Levels
// =============================================================================

/// B0/I0: Single paragraph with plain text inlines.
pub fn gen_pandoc_b0_i0() -> impl Strategy<Value = Pandoc> {
    gen_simple_pandoc()
}

/// B1/I0: Multiple paragraphs with plain text inlines.
pub fn gen_pandoc_b1_i0() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(GenConfig {
        max_depth: 0,
        max_children: 5,
        block_features: BlockFeatures::para_only(),
        inline_features: InlineFeatures::plain_text(),
    })
}

/// Pandoc with all leaf blocks (no containers).
pub fn gen_pandoc_all_leaf_blocks() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(GenConfig {
        max_depth: 0,
        max_children: 4,
        block_features: BlockFeatures::all_leaves(),
        inline_features: InlineFeatures::plain_text(),
    })
}

/// Pandoc with all leaf inlines (no containers).
pub fn gen_pandoc_all_leaf_inlines() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(GenConfig {
        max_depth: 0,
        max_children: 4,
        block_features: BlockFeatures::para_only(),
        inline_features: InlineFeatures::all_leaves(),
    })
}

/// B5: Pandoc with a single BulletList containing 1-4 items (simple: only paragraphs in items).
/// This is the original test case for list length reconciliation.
pub fn gen_pandoc_with_list() -> impl Strategy<Value = Pandoc> {
    // Use minimal config (depth=0) so list items only contain paragraphs, not nested lists.
    // This matches the original behavior for testing list length changes.
    gen_bullet_list_simple().prop_map(|list| Pandoc {
        meta: Default::default(),
        blocks: vec![list],
    })
}

/// Generate a simple BulletList where each item is just a Paragraph (no nesting).
fn gen_bullet_list_simple() -> impl Strategy<Value = Block> {
    let item_gen = gen_paragraph(GenConfig::minimal()).prop_map(|p| vec![p]);
    proptest::collection::vec(item_gen, 1..=4).prop_map(|content| {
        Block::BulletList(BulletList {
            content,
            source_info: dummy_source(),
        })
    })
}

/// Pandoc with nested lists (more complex: lists can contain lists).
/// This tests deeper reconciliation.
pub fn gen_pandoc_with_nested_lists() -> impl Strategy<Value = Pandoc> {
    let config = GenConfig::with_lists();
    gen_bullet_list(config.descend()).prop_map(|list| Pandoc {
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

/// Generate Pandoc with shallow containers (depth=1).
pub fn gen_pandoc_shallow_containers() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(GenConfig {
        max_depth: 1,
        max_children: 3,
        block_features: BlockFeatures::full(),
        inline_features: InlineFeatures::full(),
    })
}

/// Generate Pandoc with nested containers (depth=2).
pub fn gen_pandoc_nested_containers() -> impl Strategy<Value = Pandoc> {
    gen_pandoc(GenConfig {
        max_depth: 2,
        max_children: 3,
        block_features: BlockFeatures::full(),
        inline_features: InlineFeatures::full(),
    })
}

// =============================================================================
// Legacy compatibility
// =============================================================================

/// Legacy: Generate blocks with the given feature sets.
#[deprecated(note = "Use gen_blocks(GenConfig) instead")]
pub fn gen_blocks_legacy(
    block_features: BlockFeatures,
    inline_features: InlineFeatures,
) -> impl Strategy<Value = Blocks> {
    gen_blocks(GenConfig {
        max_depth: 1,
        max_children: 4,
        block_features,
        inline_features,
    })
}

/// Legacy: Generate inlines with the given feature set.
#[deprecated(note = "Use gen_inlines(GenConfig) instead")]
pub fn gen_inlines_legacy(features: InlineFeatures) -> impl Strategy<Value = Inlines> {
    gen_inlines(GenConfig {
        max_depth: 1,
        max_children: 5,
        block_features: BlockFeatures::default(),
        inline_features: features,
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

        // Phase 1: Test leaf blocks
        #[test]
        fn gen_code_block_produces_code_block(block in gen_code_block()) {
            match block {
                Block::CodeBlock(cb) => {
                    prop_assert!(!cb.text.is_empty());
                }
                _ => prop_assert!(false, "Expected CodeBlock"),
            }
        }

        #[test]
        fn gen_raw_block_produces_raw_block(block in gen_raw_block()) {
            match block {
                Block::RawBlock(rb) => {
                    prop_assert!(!rb.format.is_empty());
                }
                _ => prop_assert!(false, "Expected RawBlock"),
            }
        }

        #[test]
        fn gen_horizontal_rule_produces_hr(block in gen_horizontal_rule()) {
            match block {
                Block::HorizontalRule(_) => {}
                _ => prop_assert!(false, "Expected HorizontalRule"),
            }
        }

        // Phase 1: Test leaf inlines
        #[test]
        fn gen_code_inline_produces_code(inline in gen_code_inline()) {
            match inline {
                Inline::Code(c) => {
                    prop_assert!(!c.text.is_empty());
                }
                _ => prop_assert!(false, "Expected Code inline"),
            }
        }

        #[test]
        fn gen_math_produces_math(inline in gen_math()) {
            match inline {
                Inline::Math(m) => {
                    prop_assert!(!m.text.is_empty());
                }
                _ => prop_assert!(false, "Expected Math inline"),
            }
        }

        #[test]
        fn gen_raw_inline_produces_raw(inline in gen_raw_inline()) {
            match inline {
                Inline::RawInline(r) => {
                    prop_assert!(!r.format.is_empty());
                }
                _ => prop_assert!(false, "Expected RawInline"),
            }
        }

        // Test that all leaf blocks config generates variety
        #[test]
        fn gen_all_leaf_blocks_produces_blocks(ast in gen_pandoc_all_leaf_blocks()) {
            prop_assert!(!ast.blocks.is_empty());
        }

        // Test that all leaf inlines config generates variety
        #[test]
        fn gen_all_leaf_inlines_produces_inlines(ast in gen_pandoc_all_leaf_inlines()) {
            prop_assert!(!ast.blocks.is_empty());
            // Check that at least one block has content
            for block in &ast.blocks {
                if let Block::Paragraph(p) = block {
                    prop_assert!(!p.content.is_empty());
                }
            }
        }

        // Phase 2: Test inline containers
        #[test]
        fn gen_emph_produces_emph(inline in gen_emph(GenConfig::minimal())) {
            match inline {
                Inline::Emph(e) => {
                    prop_assert!(!e.content.is_empty());
                }
                _ => prop_assert!(false, "Expected Emph inline"),
            }
        }

        #[test]
        fn gen_strong_produces_strong(inline in gen_strong(GenConfig::minimal())) {
            match inline {
                Inline::Strong(s) => {
                    prop_assert!(!s.content.is_empty());
                }
                _ => prop_assert!(false, "Expected Strong inline"),
            }
        }

        #[test]
        fn gen_link_produces_link(inline in gen_link(GenConfig::minimal())) {
            match inline {
                Inline::Link(l) => {
                    prop_assert!(!l.content.is_empty());
                }
                _ => prop_assert!(false, "Expected Link inline"),
            }
        }

        // Phase 3: Test block containers
        #[test]
        fn gen_blockquote_produces_blockquote(block in gen_blockquote(GenConfig::minimal())) {
            match block {
                Block::BlockQuote(bq) => {
                    prop_assert!(!bq.content.is_empty());
                }
                _ => prop_assert!(false, "Expected BlockQuote"),
            }
        }

        #[test]
        fn gen_bullet_list_produces_list(block in gen_bullet_list(GenConfig::minimal())) {
            match block {
                Block::BulletList(bl) => {
                    prop_assert!(!bl.content.is_empty());
                }
                _ => prop_assert!(false, "Expected BulletList"),
            }
        }

        #[test]
        fn gen_div_produces_div(block in gen_div(GenConfig::minimal())) {
            match block {
                Block::Div(d) => {
                    prop_assert!(!d.content.is_empty());
                }
                _ => prop_assert!(false, "Expected Div"),
            }
        }

        // Test shallow containers (depth=1)
        #[test]
        fn gen_shallow_containers_produces_valid_ast(ast in gen_pandoc_shallow_containers()) {
            prop_assert!(!ast.blocks.is_empty());
        }

        // Test nested containers (depth=2)
        #[test]
        fn gen_nested_containers_produces_valid_ast(ast in gen_pandoc_nested_containers()) {
            prop_assert!(!ast.blocks.is_empty());
        }

        // Test full pandoc generation
        #[test]
        fn gen_full_pandoc_produces_valid_ast(ast in gen_full_pandoc()) {
            prop_assert!(!ast.blocks.is_empty());
        }
    }

    // Non-proptest unit tests for specific behaviors
    #[test]
    fn config_descend_decrements_depth() {
        let config = GenConfig {
            max_depth: 3,
            ..GenConfig::default()
        };
        let descended = config.descend();
        assert_eq!(descended.max_depth, 2);
    }

    #[test]
    fn config_descend_at_zero_stays_zero() {
        let config = GenConfig {
            max_depth: 0,
            ..GenConfig::default()
        };
        let descended = config.descend();
        assert_eq!(descended.max_depth, 0);
    }

    #[test]
    fn config_can_recurse_true_at_positive_depth() {
        let config = GenConfig {
            max_depth: 1,
            ..GenConfig::default()
        };
        assert!(config.can_recurse());
    }

    #[test]
    fn config_can_recurse_false_at_zero_depth() {
        let config = GenConfig {
            max_depth: 0,
            ..GenConfig::default()
        };
        assert!(!config.can_recurse());
    }

    #[test]
    fn inline_features_has_containers_detects_containers() {
        let features = InlineFeatures {
            emph: true,
            ..Default::default()
        };
        assert!(features.has_containers());

        let features = InlineFeatures::plain_text();
        assert!(!features.has_containers());
    }

    #[test]
    fn block_features_has_containers_detects_containers() {
        let features = BlockFeatures {
            bullet_list: true,
            ..Default::default()
        };
        assert!(features.has_containers());

        let features = BlockFeatures::para_only();
        assert!(!features.has_containers());
    }
}
