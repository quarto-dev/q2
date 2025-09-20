/*
 * table.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::attr::Attr;
use crate::pandoc::block::Blocks;
use crate::pandoc::caption::Caption;
use crate::pandoc::location::SourceInfo;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Alignment {
    Left,
    Center,
    Right,
    Default,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum ColWidth {
    Default,
    Percentage(f64),
}

pub type ColSpec = (Alignment, ColWidth);

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub attr: Attr,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableHead {
    pub attr: Attr,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableBody {
    pub attr: Attr,
    pub rowhead_columns: usize,
    pub head: Vec<Row>,
    pub body: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableFoot {
    pub attr: Attr,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub attr: Attr,
    pub alignment: Alignment,
    pub row_span: usize,
    pub col_span: usize,
    pub content: Blocks,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub attr: Attr,
    pub caption: Caption,
    pub colspec: Vec<ColSpec>,
    pub head: TableHead,
    pub bodies: Vec<TableBody>,
    pub foot: TableFoot,
    pub source_info: SourceInfo,
}
