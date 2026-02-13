/*
 * pandocnativeintermediate.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{
    Alignment, Attr, AttrSourceInfo, Block, Blocks, Cell, Inline, Inlines, ListAttributes, Pandoc,
    Row, ShortcodeArg,
};
use quarto_source_map::Range;

#[derive(Debug, Clone, PartialEq)]
pub enum PandocNativeIntermediate {
    IntermediatePandoc(Pandoc),
    IntermediateAttr(Attr, AttrSourceInfo),
    IntermediateSection(Vec<Block>),
    IntermediateBlock(Block),
    IntermediateInline(Inline),
    IntermediateInlines(Inlines),
    IntermediateBaseText(String, Range),
    IntermediateLatexInlineDelimiter(Range),
    IntermediateLatexDisplayDelimiter(Range),
    // Vec of (key, value, key_range, value_range) tuples
    IntermediateKeyValueSpec(Vec<(String, String, Range, Range)>),
    IntermediateRawFormat(String, Range),
    IntermediateShortcodeArg(ShortcodeArg, Range),
    // Target for links and images: (url, title, url_range, title_range)
    IntermediateTarget(String, String, Range, Range),
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
