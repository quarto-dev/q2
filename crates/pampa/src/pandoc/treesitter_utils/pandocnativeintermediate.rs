/*
 * pandocnativeintermediate.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{
    Alignment, Attr, AttrSourceInfo, Block, Blocks, Cell, Inline, Inlines, ListAttributes, Pandoc,
    Row, ShortcodeArg,
};
use quarto_source_map::{Range, SourceInfo};

#[derive(Debug, Clone, PartialEq)]
pub enum PandocNativeIntermediate {
    IntermediatePandoc(Pandoc),
    /// Attribute intermediate carrying the parsed `Attr`, its
    /// per-field `AttrSourceInfo`, *and* an explicit `SourceInfo`
    /// covering the whole attribute span. The third field exists so
    /// consumers can hand it straight to
    /// [`crate::pandoc::inline::InlineAttr::new`] (Plan 7f Phase 6.5)
    /// rather than recomputing the union at every call site.
    IntermediateAttr(Attr, AttrSourceInfo, SourceInfo),
    IntermediateSection(Vec<Block>),
    IntermediateBlock(Block),
    IntermediateInline(Inline),
    IntermediateInlines(Inlines),
    IntermediateBaseText(String, Range),
    /// A decoded string plus the **content provenance** of the bytes it
    /// was decoded from — i.e. a `SourceInfo` whose offset space is the
    /// decoded string's, not the raw node's.
    ///
    /// Unlike [`Self::IntermediateBaseText`], which carries a raw
    /// `Range` over the node as it appears on disk, this variant is for
    /// leaves whose text was *rewritten* on the way out (quote stripping
    /// and `\X` escape collapsing in
    /// [`crate::pandoc::treesitter_utils::text_helpers::extract_quoted_text`]).
    /// A raw range cannot describe such a decode: it is not affine, and
    /// `quarto_source_map::Range` is a start/end pair with no way to
    /// express a `Concat`.
    IntermediateDecodedText(String, SourceInfo),
    IntermediateLatexInlineDelimiter(Range),
    IntermediateLatexDisplayDelimiter(Range),
    /// Vec of (key, value, key_range, value_content_source) tuples.
    ///
    /// The value slot carries **content** provenance (see
    /// [`Self::IntermediateDecodedText`]), because a quoted attribute
    /// value's decoded text is not a substring of its own raw span.
    IntermediateKeyValueSpec(Vec<(String, String, Range, SourceInfo)>),
    IntermediateRawFormat(String, Range),
    IntermediateShortcodeArg(ShortcodeArg, Range),
    /// Target for links and images: (url, title, url_range,
    /// title_content_source). The title slot carries **content**
    /// provenance for the same reason as
    /// [`Self::IntermediateKeyValueSpec`]'s value slot.
    IntermediateTarget(String, String, Range, SourceInfo),
    IntermediateUnknown(Range),
    /// (blocks, range, ordered_list_attrs, has_blank_line_between_blocks)
    IntermediateListItem(Blocks, Range, Option<ListAttributes>, bool),
    IntermediateOrderedListMarker(usize, Range),
    IntermediateMetadataString(String, Range),
    IntermediateCell(Cell),
    IntermediateRow(Row),
    IntermediatePipeTableDelimiterCell(Alignment),
    IntermediatePipeTableDelimiterRow(Vec<Alignment>),
    IntermediateSetextHeadingLevel(usize),
}
