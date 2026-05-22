/*
 * block.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::attr::{Attr, AttrSourceInfo};
use crate::caption::Caption;
use crate::config_value::ConfigValue;
use crate::custom::CustomNode;
use crate::inline::Inlines;
use crate::list::ListAttributes;
use crate::table::Table;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Custom node for Quarto extensions (callouts, tabsets, etc.)
    ///
    /// Parsed from Divs with special class names. When serialized to Pandoc JSON,
    /// these are converted to wrapper Divs with `__quarto_custom_node` class.
    Custom(CustomNode),
}

impl Block {
    pub fn source_info(&self) -> &quarto_source_map::SourceInfo {
        match self {
            Block::Plain(b) => &b.source_info,
            Block::Paragraph(b) => &b.source_info,
            Block::LineBlock(b) => &b.source_info,
            Block::CodeBlock(b) => &b.source_info,
            Block::RawBlock(b) => &b.source_info,
            Block::BlockQuote(b) => &b.source_info,
            Block::OrderedList(b) => &b.source_info,
            Block::BulletList(b) => &b.source_info,
            Block::DefinitionList(b) => &b.source_info,
            Block::Header(b) => &b.source_info,
            Block::HorizontalRule(b) => &b.source_info,
            Block::Table(b) => &b.source_info,
            Block::Figure(b) => &b.source_info,
            Block::Div(b) => &b.source_info,
            Block::BlockMetadata(b) => &b.source_info,
            Block::NoteDefinitionPara(b) => &b.source_info,
            Block::NoteDefinitionFencedBlock(b) => &b.source_info,
            Block::CaptionBlock(b) => &b.source_info,
            Block::Custom(b) => &b.source_info,
        }
    }

    /// Mutable counterpart to [`source_info`]. Mechanical mirror of the read
    /// accessor; lets Plan-6 stamping rewrite the per-node `source_info` field
    /// through the enum without holding a typed variant reference.
    pub fn source_info_mut(&mut self) -> &mut quarto_source_map::SourceInfo {
        match self {
            Block::Plain(b) => &mut b.source_info,
            Block::Paragraph(b) => &mut b.source_info,
            Block::LineBlock(b) => &mut b.source_info,
            Block::CodeBlock(b) => &mut b.source_info,
            Block::RawBlock(b) => &mut b.source_info,
            Block::BlockQuote(b) => &mut b.source_info,
            Block::OrderedList(b) => &mut b.source_info,
            Block::BulletList(b) => &mut b.source_info,
            Block::DefinitionList(b) => &mut b.source_info,
            Block::Header(b) => &mut b.source_info,
            Block::HorizontalRule(b) => &mut b.source_info,
            Block::Table(b) => &mut b.source_info,
            Block::Figure(b) => &mut b.source_info,
            Block::Div(b) => &mut b.source_info,
            Block::BlockMetadata(b) => &mut b.source_info,
            Block::NoteDefinitionPara(b) => &mut b.source_info,
            Block::NoteDefinitionFencedBlock(b) => &mut b.source_info,
            Block::CaptionBlock(b) => &mut b.source_info,
            Block::Custom(b) => &mut b.source_info,
        }
    }
}

pub type Blocks = Vec<Block>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plain {
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paragraph {
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineBlock {
    pub content: Vec<Inlines>,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeBlock {
    pub attr: Attr,
    pub text: String,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawBlock {
    pub format: String,
    pub text: String,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockQuote {
    pub content: Blocks,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderedList {
    pub attr: ListAttributes,
    pub content: Vec<Blocks>,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulletList {
    pub content: Vec<Blocks>,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefinitionList {
    pub content: Vec<(Inlines, Vec<Blocks>)>,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub level: usize,
    pub attr: Attr,
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizontalRule {
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Figure {
    pub attr: Attr,
    pub caption: Caption,
    pub content: Blocks,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Div {
    pub attr: Attr,
    pub content: Blocks,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetaBlock {
    pub meta: ConfigValue,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteDefinitionPara {
    pub id: String,
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteDefinitionFencedBlock {
    pub id: String,
    pub content: Blocks,
    pub source_info: quarto_source_map::SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionBlock {
    pub content: Inlines,
    pub source_info: quarto_source_map::SourceInfo,
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{FileId, SourceInfo};

    fn test_si(file: usize, start: usize, end: usize) -> SourceInfo {
        SourceInfo::original(FileId(file), start, end)
    }

    #[test]
    fn source_info_plain() {
        let si = test_si(0, 0, 10);
        let block = Block::Plain(Plain {
            content: vec![],
            source_info: si.clone(),
        });
        assert_eq!(block.source_info(), &si);
    }

    #[test]
    fn source_info_paragraph() {
        let si = test_si(1, 10, 20);
        let block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: si.clone(),
        });
        assert_eq!(block.source_info(), &si);
    }

    #[test]
    fn source_info_codeblock() {
        let si = test_si(2, 20, 30);
        let block = Block::CodeBlock(CodeBlock {
            attr: crate::attr::empty_attr(),
            text: String::new(),
            source_info: si.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(block.source_info(), &si);
    }

    #[test]
    fn source_info_header() {
        let si = test_si(3, 30, 40);
        let block = Block::Header(Header {
            level: 1,
            attr: crate::attr::empty_attr(),
            content: vec![],
            source_info: si.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(block.source_info(), &si);
    }

    #[test]
    fn source_info_div() {
        let si = test_si(4, 40, 50);
        let block = Block::Div(Div {
            attr: crate::attr::empty_attr(),
            content: vec![],
            source_info: si.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(block.source_info(), &si);
    }

    #[test]
    fn source_info_horizontal_rule() {
        let si = test_si(5, 50, 53);
        let block = Block::HorizontalRule(HorizontalRule {
            source_info: si.clone(),
        });
        assert_eq!(block.source_info(), &si);
    }

    #[test]
    fn source_info_mut_round_trip_paragraph() {
        let original = test_si(0, 0, 10);
        let updated = test_si(9, 200, 220);
        let mut block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: original.clone(),
        });
        assert_eq!(block.source_info(), &original);
        *block.source_info_mut() = updated.clone();
        assert_eq!(block.source_info(), &updated);
    }
}
