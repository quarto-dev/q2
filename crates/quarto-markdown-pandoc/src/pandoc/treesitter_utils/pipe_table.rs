/*
 * pipe_table.rs
 *
 * Functions for processing pipe table-related nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::empty_attr;
use crate::pandoc::block::{Block, Plain};
use crate::pandoc::caption::Caption;
use crate::pandoc::inline::Inlines;
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::table::{
    Alignment, Cell, ColSpec, ColWidth, Row, Table, TableBody, TableFoot, TableHead,
};
use std::collections::HashMap;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_pipe_table_delimiter_cell(
    children: Vec<(String, PandocNativeIntermediate)>,
    _context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut has_starter_colon = false;
    let mut has_ending_colon = false;
    for (node, _) in children {
        if node == "pipe_table_align_right" {
            has_ending_colon = true;
        } else if node == "pipe_table_align_left" {
            has_starter_colon = true;
        } else if node == "-" {
            continue;
        } else {
            panic!("Unexpected node in pipe_table_delimiter_cell: {}", node);
        }
    }
    PandocNativeIntermediate::IntermediatePipeTableDelimiterCell(
        match (has_starter_colon, has_ending_colon) {
            (true, true) => Alignment::Center,
            (true, false) => Alignment::Left,
            (false, true) => Alignment::Right,
            (false, false) => Alignment::Default,
        },
    )
}

pub fn process_pipe_table_header_or_row(
    children: Vec<(String, PandocNativeIntermediate)>,
    _context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut row = Row {
        attr: empty_attr(),
        cells: Vec::new(),
    };
    for (node, child) in children {
        if node == "|" {
            // This is a marker node, we don't need to do anything with it
            continue;
        } else if node == "pipe_table_cell" {
            if let PandocNativeIntermediate::IntermediateCell(cell) = child {
                row.cells.push(cell);
            } else {
                panic!("Expected Cell in pipe_table_row, got {:?}", child);
            }
        } else {
            panic!(
                "Expected pipe_table_cell in pipe_table_row, got {:?} {:?}",
                node, child
            );
        }
    }
    PandocNativeIntermediate::IntermediateRow(row)
}

pub fn process_pipe_table_delimiter_row(
    children: Vec<(String, PandocNativeIntermediate)>,
    _context: &ASTContext,
) -> PandocNativeIntermediate {
    // This is a row of delimiters, we don't need to do anything with it
    // but we need to return an empty row
    PandocNativeIntermediate::IntermediatePipeTableDelimiterRow(
        children
            .into_iter()
            .filter(|(node, _)| node != "|") // skip the marker nodes
            .map(|(node, child)| match child {
                PandocNativeIntermediate::IntermediatePipeTableDelimiterCell(alignment) => {
                    alignment
                }
                _ => panic!(
                    "Unexpected node in pipe_table_delimiter_row: {} {:?}",
                    node, child
                ),
            })
            .collect(),
    )
}

pub fn process_pipe_table_cell(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut plain_content: Inlines = Vec::new();
    let mut table_cell = Cell {
        alignment: Alignment::Default,
        col_span: 1,
        row_span: 1,
        attr: ("".to_string(), vec![], HashMap::new()),
        content: vec![],
    };
    for (node, child) in children {
        if node == "inline" {
            match child {
                PandocNativeIntermediate::IntermediateInlines(inlines) => {
                    plain_content.extend(inlines);
                }
                _ => panic!("Expected Inlines in pipe_table_cell, got {:?}", child),
            }
        } else {
            panic!(
                "Expected Inlines in pipe_table_cell, got {:?} {:?}",
                node, child
            );
        }
    }

    // Trim trailing spaces from cell content to match Pandoc behavior
    while let Some(last) = plain_content.last() {
        if matches!(last, crate::pandoc::inline::Inline::Space(_)) {
            plain_content.pop();
        } else {
            break;
        }
    }

    table_cell.content.push(Block::Plain(Plain {
        content: plain_content,
        source_info: node_source_info_with_context(node, context),
    }));
    PandocNativeIntermediate::IntermediateCell(table_cell)
}

pub fn process_pipe_table(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let attr = empty_attr();
    let mut header: Option<Row> = None;
    let mut colspec: Vec<ColSpec> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();
    for (node, child) in children {
        if node == "pipe_table_header" {
            if let PandocNativeIntermediate::IntermediateRow(row) = child {
                header = Some(row);
            } else {
                panic!("Expected Row in pipe_table_header, got {:?}", child);
            }
        } else if node == "pipe_table_delimiter_row" {
            match child {
                PandocNativeIntermediate::IntermediatePipeTableDelimiterRow(row) => {
                    row.into_iter().for_each(|alignment| {
                        colspec.push((alignment, ColWidth::Default));
                    });
                }
                _ => panic!(
                    "Expected PipeTableDelimiterRow in pipe_table_delimiter_row, got {:?}",
                    child
                ),
            }
        } else if node == "pipe_table_row" {
            if let PandocNativeIntermediate::IntermediateRow(row) = child {
                rows.push(row);
            } else {
                panic!("Expected Row in pipe_table_row, got {:?}", child);
            }
        } else {
            panic!("Unexpected node in pipe_table: {}", node);
        }
    }

    // Check if header row has all empty cells
    let header_is_empty = header.as_ref().map_or(false, |h| {
        h.cells.iter().all(|cell| {
            cell.content.iter().all(|block| {
                if let Block::Plain(plain) = block {
                    plain.content.is_empty()
                } else {
                    false
                }
            })
        })
    });

    // If header is empty, discard it and use empty thead
    let (thead_rows, body_rows) = if header_is_empty {
        (vec![], rows)
    } else {
        (vec![header.unwrap()], rows)
    };

    PandocNativeIntermediate::IntermediateBlock(Block::Table(Table {
        attr,
        caption: Caption {
            short: None,
            long: None,
        },
        colspec,
        head: TableHead {
            attr: empty_attr(),
            rows: thead_rows,
        },
        bodies: vec![TableBody {
            attr: empty_attr(),
            rowhead_columns: 0,
            head: vec![],
            body: body_rows,
        }],
        foot: TableFoot {
            attr: empty_attr(),
            rows: vec![],
        },
        source_info: node_source_info_with_context(node, context),
    }))
}
