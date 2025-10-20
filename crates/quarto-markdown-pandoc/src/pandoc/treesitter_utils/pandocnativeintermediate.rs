/*
 * pandocnativeintermediate.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::attr::Attr;
use crate::pandoc::block::{Block, Blocks};
use crate::pandoc::inline::{Inline, Inlines};

use crate::pandoc::list::ListAttributes;
use crate::pandoc::pandoc::Pandoc;
use crate::pandoc::shortcode::ShortcodeArg;
use crate::pandoc::table::{Alignment, Cell, Row};
use quarto_source_map::Range;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum PandocNativeIntermediate {
    IntermediatePandoc(Pandoc),
    IntermediateAttr(Attr),
    IntermediateSection(Vec<Block>),
    IntermediateBlock(Block),
    IntermediateInline(Inline),
    IntermediateInlines(Inlines),
    IntermediateBaseText(String, Range),
    IntermediateLatexInlineDelimiter(Range),
    IntermediateLatexDisplayDelimiter(Range),
    IntermediateKeyValueSpec(HashMap<String, String>),
    IntermediateRawFormat(String, Range),
    IntermediateShortcodeArg(ShortcodeArg, Range),
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
