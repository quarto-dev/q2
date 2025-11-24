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

/// Options for extracting source info from tree-sitter nodes
#[derive(Debug, Clone, Default)]
pub struct SourceInfoOptions {
    /// Whether to trim leading whitespace from the source location
    pub trim_leading_whitespace: bool,
    /// Whether to trim trailing whitespace from the source location
    pub trim_trailing_whitespace: bool,
}

impl SourceInfoOptions {
    /// Create options with no trimming (default behavior)
    pub fn none() -> Self {
        Self::default()
    }

    /// Create options that trim both leading and trailing whitespace
    pub fn trim_all() -> Self {
        Self {
            trim_leading_whitespace: true,
            trim_trailing_whitespace: true,
        }
    }

    /// Create options that trim only leading whitespace
    pub fn trim_leading() -> Self {
        Self {
            trim_leading_whitespace: true,
            trim_trailing_whitespace: false,
        }
    }

    /// Create options that trim only trailing whitespace
    pub fn trim_trailing() -> Self {
        Self {
            trim_leading_whitespace: false,
            trim_trailing_whitespace: true,
        }
    }
}

pub fn node_source_info(node: &tree_sitter::Node) -> quarto_source_map::SourceInfo {
    quarto_source_map::SourceInfo::from_range(quarto_source_map::FileId(0), node_location(node))
}

pub fn node_source_info_with_context(
    node: &tree_sitter::Node,
    context: &ASTContext,
) -> quarto_source_map::SourceInfo {
    node_source_info_with_options(node, context, &SourceInfoOptions::none())
}

/// Extract source info from a tree-sitter node with additional options
///
/// This is the most general form that allows trimming whitespace from the resulting
/// source location. Use `node_source_info_with_context` for the common case where
/// no trimming is needed.
///
/// # Arguments
/// * `node` - The tree-sitter node
/// * `context` - The AST context containing file and parent information
/// * `options` - Options for extracting the source info (e.g., whitespace trimming)
///
/// # Returns
/// A SourceInfo that may be trimmed based on the provided options
pub fn node_source_info_with_options(
    node: &tree_sitter::Node,
    context: &ASTContext,
    options: &SourceInfoOptions,
) -> quarto_source_map::SourceInfo {
    // First, get the base source info (same logic as node_source_info_with_context)
    let base_source_info = if let Some(parent) = &context.parent_source_info {
        quarto_source_map::SourceInfo::substring(parent.clone(), node.start_byte(), node.end_byte())
    } else {
        quarto_source_map::SourceInfo::from_range(context.current_file_id(), node_location(node))
    };

    // Apply trimming if requested
    if options.trim_leading_whitespace || options.trim_trailing_whitespace {
        let input_text = context
            .source_context
            .get_file(context.current_file_id())
            .and_then(|f| f.content.as_ref())
            .map(|s| s.as_str())
            .unwrap_or("");

        crate::utils::trim_source_location::trim_whitespace(
            &base_source_info,
            input_text,
            options.trim_leading_whitespace,
            options.trim_trailing_whitespace,
        )
    } else {
        base_source_info
    }
}

/// Convert a Range to SourceInfo using the context's primary file ID.
///
/// # Arguments
/// * `range` - The Range to convert
/// * `ctx` - The ASTContext to get the file ID from
///
/// # Returns
/// A SourceInfo with Original mapping to the primary file
pub fn range_to_source_info_with_context(
    range: &quarto_source_map::Range,
    ctx: &ASTContext,
) -> quarto_source_map::SourceInfo {
    let file_id = ctx
        .primary_file_id()
        .unwrap_or(quarto_source_map::FileId(0));
    quarto_source_map::SourceInfo::from_range(file_id, range.clone())
}

/// Convert quarto-source-map::SourceInfo to a quarto_source_map::Range, with a fallback if mapping fails.
///
/// This is for use with PandocNativeIntermediate which uses quarto_source_map::Range.
/// Provides a fallback Range with zero row/column values if the mapping fails.
///
/// # Arguments
/// * `source_info` - The SourceInfo to convert
/// * `ctx` - The ASTContext containing the source context
///
/// # Returns
/// A quarto_source_map::Range with row/column information if available, or a Range with offsets only
pub fn source_info_to_qsm_range_or_fallback(
    source_info: &quarto_source_map::SourceInfo,
    ctx: &ASTContext,
) -> quarto_source_map::Range {
    let start_mapped = source_info.map_offset(0, &ctx.source_context);
    let end_mapped = source_info.map_offset(source_info.length(), &ctx.source_context);

    match (start_mapped, end_mapped) {
        (Some(start), Some(end)) => quarto_source_map::Range {
            start: start.location,
            end: end.location,
        },
        _ => quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: source_info.start_offset(),
                row: 0,
                column: 0,
            },
            end: quarto_source_map::Location {
                offset: source_info.end_offset(),
                row: 0,
                column: 0,
            },
        },
    }
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
