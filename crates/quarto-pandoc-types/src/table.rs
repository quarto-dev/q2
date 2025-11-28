/*
 * table.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::attr::{Attr, AttrSourceInfo};
use crate::block::Blocks;
use crate::caption::Caption;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Alignment {
    Left,
    Center,
    Right,
    Default,
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ColWidth {
    Default,
    Percentage(f64),
}

pub type ColSpec = (Alignment, ColWidth);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub attr: Attr,
    pub cells: Vec<Cell>,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableHead {
    pub attr: Attr,
    pub rows: Vec<Row>,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableBody {
    pub attr: Attr,
    pub rowhead_columns: usize,
    pub head: Vec<Row>,
    pub body: Vec<Row>,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableFoot {
    pub attr: Attr,
    pub rows: Vec<Row>,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub attr: Attr,
    pub alignment: Alignment,
    pub row_span: usize,
    pub col_span: usize,
    pub content: Blocks,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub attr: Attr,
    pub caption: Caption,
    pub colspec: Vec<ColSpec>,
    pub head: TableHead,
    pub bodies: Vec<TableBody>,
    pub foot: TableFoot,
    pub source_info: quarto_source_map::SourceInfo,
    pub attr_source: AttrSourceInfo,
}
