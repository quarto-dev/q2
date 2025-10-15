/*
 * block.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::impl_source_location;
use crate::pandoc::Meta;
use crate::pandoc::attr::Attr;
use crate::pandoc::caption::Caption;
use crate::pandoc::inline::Inlines;
use crate::pandoc::list::ListAttributes;
use crate::pandoc::location::Range;
use crate::pandoc::location::SourceInfo;
use crate::pandoc::location::SourceLocation;
use crate::pandoc::location::node_source_info;
use crate::pandoc::table::Table;

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Plain(Plain),
    Paragraph(Paragraph),
    LineBlock(LineBlock),
    CodeBlock(CodeBlock),
    RawBlock(RawBlock),
    BlockQuote(BlockQuote),
    OrderedList(OrderedList),
    BulletList(BulletList),
    DefinitionList(DefinitionList),
    Header(Header),
    HorizontalRule(HorizontalRule),
    Table(Table),
    Figure(Figure),
    Div(Div),
    // quarto extensions
    BlockMetadata(MetaBlock),
    NoteDefinitionPara(NoteDefinitionPara),
    NoteDefinitionFencedBlock(NoteDefinitionFencedBlock),
    CaptionBlock(CaptionBlock),
}

pub type Blocks = Vec<Block>;

#[derive(Debug, Clone, PartialEq)]
pub struct Plain {
    pub content: Inlines,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    pub content: Inlines,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineBlock {
    pub content: Vec<Inlines>,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodeBlock {
    pub attr: Attr,
    pub text: String,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawBlock {
    pub format: String,
    pub text: String,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockQuote {
    pub content: Blocks,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderedList {
    pub attr: ListAttributes,
    pub content: Vec<Blocks>,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BulletList {
    pub content: Vec<Blocks>,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DefinitionList {
    pub content: Vec<(Inlines, Vec<Blocks>)>,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub level: usize,
    pub attr: Attr,
    pub content: Inlines,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HorizontalRule {
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub attr: Attr,
    pub caption: Caption,
    pub content: Blocks,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Div {
    pub attr: Attr,
    pub content: Blocks,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetaBlock {
    pub meta: Meta,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteDefinitionPara {
    pub id: String,
    pub content: Inlines,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteDefinitionFencedBlock {
    pub id: String,
    pub content: Blocks,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptionBlock {
    pub content: Inlines,
    pub source_info: SourceInfo,
}

impl_source_location!(
    // blocks
    Plain,
    Paragraph,
    LineBlock,
    CodeBlock,
    RawBlock,
    BlockQuote,
    OrderedList,
    BulletList,
    DefinitionList,
    Header,
    HorizontalRule,
    Table,
    Figure,
    Div,
    // quarto extensions
    MetaBlock,
    NoteDefinitionPara,
    NoteDefinitionFencedBlock,
    CaptionBlock
);

fn make_block_leftover(node: &tree_sitter::Node, input_bytes: &[u8]) -> Block {
    let text = node.utf8_text(input_bytes).unwrap().to_string();
    Block::RawBlock(RawBlock {
        format: "quarto-internal-leftover".to_string(),
        text,
        source_info: node_source_info(node),
    })
}
