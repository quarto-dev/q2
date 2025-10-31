/*
 * pandocnativeintermediate.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::attr::{Attr, AttrSourceInfo};
use crate::pandoc::block::{Block, Blocks};
use crate::pandoc::inline::{Inline, Inlines};

use crate::pandoc::list::ListAttributes;
use crate::pandoc::pandoc::Pandoc;
use crate::pandoc::shortcode::ShortcodeArg;
use crate::pandoc::table::{Alignment, Cell, Row};
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
    // Target for links and images: (url, title, range)
    IntermediateTarget(String, String, Range),
    IntermediateUnknown(Range),
    IntermediateListItem(Blocks, Range, Option<ListAttributes>),
    IntermediateOrderedListMarker(usize, Range),
    IntermediateMetadataString(String, Range),
    IntermediateCell(Cell),
    IntermediateRow(Row),
    IntermediatePipeTableDelimiterCell(Alignment),
    IntermediatePipeTableDelimiterRow(Vec<Alignment>),
    IntermediateSetextHeadingLevel(usize),
}
