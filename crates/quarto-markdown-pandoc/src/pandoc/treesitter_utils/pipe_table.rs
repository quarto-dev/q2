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
use hashlink::LinkedHashMap;

use super::pandocnativeintermediate::PandocNativeIntermediate;
use super::postprocess::trim_inlines;

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
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut row = Row {
        attr: empty_attr(),
        cells: Vec::new(),
        source_info: node_source_info_with_context(node, context),
        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
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
        attr: ("".to_string(), vec![], LinkedHashMap::new()),
        content: vec![],
        source_info: node_source_info_with_context(node, context),
        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
    };
    for (_node, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateInline(inline) => {
                plain_content.push(inline);
            }
            PandocNativeIntermediate::IntermediateInlines(inlines) => {
                plain_content.extend(inlines);
            }
            _ => {
                // Skip other intermediate types (e.g., markers)
            }
        }
    }

    // Trim leading and trailing spaces from cell content to match Pandoc behavior
    plain_content = trim_inlines(plain_content).0;

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
    let mut attr = empty_attr();
    let mut attr_source = crate::pandoc::attr::AttrSourceInfo::empty();
    let mut header: Option<Row> = None;
    let mut colspec: Vec<ColSpec> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();
    let mut caption_inlines: Option<Inlines> = None;
    let mut caption_source_info: Option<quarto_source_map::SourceInfo> = None;
    for (node, child) in children {
        if node == "block_continuation" {
            continue; // skip block continuation nodes
        }
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
        } else if node == "caption" {
            match child {
                PandocNativeIntermediate::IntermediateBlock(Block::CaptionBlock(caption_block)) => {
                    let mut inlines = caption_block.content;
                    // Store the caption's source info to extend the table's range
                    caption_source_info = Some(caption_block.source_info.clone());

                    // Extract Inline::Attr if present at the end (for soft-break captions)
                    if let Some(crate::pandoc::inline::Inline::Attr(
                        caption_attr,
                        caption_attr_source,
                    )) = inlines.last()
                    {
                        attr = caption_attr.clone();
                        attr_source = caption_attr_source.clone();
                        inlines.pop();

                        // Trim trailing space before the attribute
                        inlines = trim_inlines(inlines).0;
                    }

                    caption_inlines = Some(inlines);
                }
                _ => panic!("Expected CaptionBlock in caption, got {:?}", child),
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

    // Construct caption from caption_inlines if present
    // Per design decision: use empty range at end of table for absent caption
    let caption = if let Some(inlines) = caption_inlines {
        Caption {
            short: None,
            long: Some(vec![Block::Plain(Plain {
                content: inlines,
                source_info: node_source_info_with_context(node, context),
            })]),
            source_info: node_source_info_with_context(node, context),
        }
    } else {
        // Empty caption: use zero-length range at end of table
        Caption {
            short: None,
            long: None,
            source_info: node_source_info_with_context(node, context),
        }
    };

    // Calculate the table's source_info: if there's a caption, extend to include it
    let table_source_info = if let Some(ref cap_info) = caption_source_info {
        // Extend from start of table node to end of caption
        let table_start = node_source_info_with_context(node, context);
        let start_offset = table_start.start_offset();
        let end_offset = cap_info.end_offset();
        // Extract file_id from the table's source info
        let file_id = match &table_start {
            quarto_source_map::SourceInfo::Original { file_id, .. } => *file_id,
            quarto_source_map::SourceInfo::Substring { parent, .. } => {
                // Recursively extract from parent (should always reach Original eventually)
                match **parent {
                    quarto_source_map::SourceInfo::Original { file_id, .. } => file_id,
                    _ => quarto_source_map::FileId(0), // Fallback
                }
            }
            quarto_source_map::SourceInfo::Concat { pieces } => {
                // Use first piece's file_id
                if let Some(piece) = pieces.first() {
                    match &piece.source_info {
                        quarto_source_map::SourceInfo::Original { file_id, .. } => *file_id,
                        _ => quarto_source_map::FileId(0), // Fallback
                    }
                } else {
                    quarto_source_map::FileId(0) // Fallback
                }
            }
            quarto_source_map::SourceInfo::FilterProvenance { .. } => {
                quarto_source_map::FileId(0) // Fallback - filter-created tables shouldn't reach this
            }
        };
        // Create a new SourceInfo spanning from table start to caption end
        quarto_source_map::SourceInfo::original(file_id, start_offset, end_offset)
    } else {
        node_source_info_with_context(node, context)
    };

    PandocNativeIntermediate::IntermediateBlock(Block::Table(Table {
        attr,
        caption,
        colspec,
        head: TableHead {
            attr: empty_attr(),
            rows: thead_rows,
            source_info: node_source_info_with_context(node, context),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        },
        bodies: vec![TableBody {
            attr: empty_attr(),
            rowhead_columns: 0,
            head: vec![],
            body: body_rows,
            source_info: node_source_info_with_context(node, context),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        }],
        foot: TableFoot {
            attr: empty_attr(),
            rows: vec![],
            source_info: node_source_info_with_context(node, context),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        },
        source_info: table_source_info,
        attr_source,
    }))
}
