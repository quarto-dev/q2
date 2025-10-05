/*
 * location.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;

////////////////////////////////////////////////////////////////////////////////////////////////////
// Source location tracking

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location {
    pub offset: usize,
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Range {
    pub start: Location,
    pub end: Location,
}

/// Encapsulates source location information for AST nodes
/// The filename field now holds an index into the ASTContext.filenames vector
#[derive(Debug, Clone, PartialEq)]
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
}

pub trait SourceLocation {
    fn filename_index(&self) -> Option<usize>;
    fn range(&self) -> Range;

    /// Resolve the filename from the ASTContext using the stored index
    fn filename<'a>(&self, context: &'a ASTContext) -> Option<&'a String> {
        self.filename_index()
            .and_then(|idx| context.filenames.get(idx))
    }
}

pub fn node_location(node: &tree_sitter::Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range {
        start: Location {
            offset: node.start_byte(),
            row: start.row,
            column: start.column,
        },
        end: Location {
            offset: node.end_byte(),
            row: end.row,
            column: end.column,
        },
    }
}

pub fn node_source_info(node: &tree_sitter::Node) -> SourceInfo {
    SourceInfo::with_range(node_location(node))
}

pub fn node_source_info_with_context(node: &tree_sitter::Node, context: &ASTContext) -> SourceInfo {
    // If the context has at least one filename, use index 0
    let filename_index = if context.filenames.is_empty() {
        None
    } else {
        Some(0)
    };
    SourceInfo::new(filename_index, node_location(node))
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

pub fn empty_source_info() -> SourceInfo {
    SourceInfo::with_range(empty_range())
}

#[macro_export]
macro_rules! impl_source_location {
    ($($type:ty),*) => {
        $(
            impl SourceLocation for $type {
                fn filename_index(&self) -> Option<usize> {
                    self.source_info.filename_index
                }

                fn range(&self) -> Range {
                    self.source_info.range.clone()
                }
            }
        )*
    };
}
