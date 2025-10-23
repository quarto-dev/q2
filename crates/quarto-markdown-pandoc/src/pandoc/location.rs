/*
 * location.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use serde::{Deserialize, Serialize};

////////////////////////////////////////////////////////////////////////////////////////////////////
// Source location tracking

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Location {
    pub offset: usize,
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Range {
    pub start: Location,
    pub end: Location,
}

/// Encapsulates source location information for AST nodes
/// The filename field now holds an index into the ASTContext.filenames vector
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub filename_index: Option<usize>,
    pub range: Range,
}

impl SourceInfo {
    pub fn new(filename_index: Option<usize>, range: Range) -> Self {
        SourceInfo {
            filename_index,
            range,
        }
    }

    pub fn with_range(range: Range) -> Self {
        SourceInfo {
            filename_index: None,
            range,
        }
    }

    pub fn combine(&self, other: &SourceInfo) -> SourceInfo {
        SourceInfo {
            filename_index: self.filename_index.or(other.filename_index),
            range: Range {
                start: if self.range.start < other.range.start {
                    self.range.start.clone()
                } else {
                    other.range.start.clone()
                },
                end: if self.range.end > other.range.end {
                    self.range.end.clone()
                } else {
                    other.range.end.clone()
                },
            },
        }
    }

    /// Get the start offset
    pub fn start_offset(&self) -> usize {
        self.range.start.offset
    }

    /// Get the end offset
    pub fn end_offset(&self) -> usize {
        self.range.end.offset
    }

    /// Convert to quarto-source-map::SourceInfo (temporary conversion helper)
    ///
    /// This helper bridges between pandoc::location types and quarto-source-map types.
    /// Long-term, code should use quarto-source-map types directly.
    ///
    /// Creates an Original mapping with a dummy FileId(0).
    /// For proper filename support, use to_source_map_info_with_mapping with a real FileId.
    pub fn to_source_map_info(&self) -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::from_range(
            quarto_source_map::FileId(0),
            quarto_source_map::Range {
                start: quarto_source_map::Location {
                    offset: self.range.start.offset,
                    row: self.range.start.row,
                    column: self.range.start.column,
                },
                end: quarto_source_map::Location {
                    offset: self.range.end.offset,
                    row: self.range.end.row,
                    column: self.range.end.column,
                },
            },
        )
    }

    /// Convert to quarto-source-map::SourceInfo with proper FileId (temporary conversion helper)
    ///
    /// This helper bridges between pandoc::location types and quarto-source-map types.
    /// Use this when you have a proper FileId mapping from your context.
    pub fn to_source_map_info_with_mapping(
        &self,
        file_id: quarto_source_map::FileId,
    ) -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::from_range(
            file_id,
            quarto_source_map::Range {
                start: quarto_source_map::Location {
                    offset: self.range.start.offset,
                    row: self.range.start.row,
                    column: self.range.start.column,
                },
                end: quarto_source_map::Location {
                    offset: self.range.end.offset,
                    row: self.range.end.row,
                    column: self.range.end.column,
                },
            },
        )
    }
}

pub fn node_location(node: &tree_sitter::Node) -> quarto_source_map::Range {
    let start = node.start_position();
    let end = node.end_position();
    quarto_source_map::Range {
        start: quarto_source_map::Location {
            offset: node.start_byte(),
            row: start.row,
            column: start.column,
        },
        end: quarto_source_map::Location {
            offset: node.end_byte(),
            row: end.row,
            column: end.column,
        },
    }
}

pub fn node_source_info(node: &tree_sitter::Node) -> quarto_source_map::SourceInfo {
    quarto_source_map::SourceInfo::from_range(quarto_source_map::FileId(0), node_location(node))
}

pub fn node_source_info_with_context(
    node: &tree_sitter::Node,
    context: &ASTContext,
) -> quarto_source_map::SourceInfo {
    quarto_source_map::SourceInfo::from_range(context.current_file_id(), node_location(node))
}

pub fn empty_range() -> Range {
    Range {
        start: Location {
            offset: 0,
            row: 0,
            column: 0,
        },
        end: Location {
            offset: 0,
            row: 0,
            column: 0,
        },
    }
}

pub fn empty_source_info() -> quarto_source_map::SourceInfo {
    quarto_source_map::SourceInfo::from_range(
        quarto_source_map::FileId(0),
        quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: quarto_source_map::Location {
                offset: 0,
                row: 0,
                column: 0,
            },
        },
    )
}

/// Extract filename index from quarto_source_map::SourceInfo by walking to Original mapping
pub fn extract_filename_index(info: &quarto_source_map::SourceInfo) -> Option<usize> {
    match info {
        quarto_source_map::SourceInfo::Original { file_id, .. } => Some(file_id.0),
        quarto_source_map::SourceInfo::Substring { parent, .. } => extract_filename_index(parent),
        quarto_source_map::SourceInfo::Concat { pieces } => {
            // Return first non-None filename_index from pieces
            pieces
                .iter()
                .find_map(|p| extract_filename_index(&p.source_info))
        }
    }
}
