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
            .map_or("", |s| s.as_str());

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
        quarto_source_map::SourceInfo::FilterProvenance { .. } => {
            // Filter provenance doesn't have a filename index
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a Location
    fn loc(offset: usize, row: usize, column: usize) -> Location {
        Location {
            offset,
            row,
            column,
        }
    }

    // Helper to create a Range
    fn range(
        start_offset: usize,
        start_row: usize,
        start_col: usize,
        end_offset: usize,
        end_row: usize,
        end_col: usize,
    ) -> Range {
        Range {
            start: loc(start_offset, start_row, start_col),
            end: loc(end_offset, end_row, end_col),
        }
    }

    // Tests for Location
    #[test]
    fn test_location_equality() {
        let loc1 = loc(10, 1, 5);
        let loc2 = loc(10, 1, 5);
        let loc3 = loc(20, 2, 0);
        assert_eq!(loc1, loc2);
        assert_ne!(loc1, loc3);
    }

    #[test]
    fn test_location_ordering() {
        let loc1 = loc(10, 1, 5);
        let loc2 = loc(20, 2, 0);
        let loc3 = loc(10, 1, 6); // Same offset but higher column
        assert!(loc1 < loc2);
        assert!(loc1 < loc3); // Ordering is lexicographic: offset, then row, then column
    }

    // Tests for Range
    #[test]
    fn test_range_equality() {
        let r1 = range(0, 0, 0, 10, 0, 10);
        let r2 = range(0, 0, 0, 10, 0, 10);
        let r3 = range(0, 0, 0, 20, 1, 5);
        assert_eq!(r1, r2);
        assert_ne!(r1, r3);
    }

    // Tests for SourceInfo
    #[test]
    fn test_source_info_new() {
        let r = range(0, 0, 0, 10, 0, 10);
        let si = SourceInfo::new(Some(42), r.clone());
        assert_eq!(si.filename_index, Some(42));
        assert_eq!(si.range, r);
    }

    #[test]
    fn test_source_info_with_range() {
        let r = range(5, 1, 0, 15, 1, 10);
        let si = SourceInfo::with_range(r.clone());
        assert_eq!(si.filename_index, None);
        assert_eq!(si.range, r);
    }

    #[test]
    fn test_source_info_start_offset() {
        let r = range(100, 5, 0, 200, 10, 0);
        let si = SourceInfo::with_range(r);
        assert_eq!(si.start_offset(), 100);
    }

    #[test]
    fn test_source_info_end_offset() {
        let r = range(100, 5, 0, 200, 10, 0);
        let si = SourceInfo::with_range(r);
        assert_eq!(si.end_offset(), 200);
    }

    #[test]
    fn test_source_info_combine_takes_min_start() {
        let si1 = SourceInfo::with_range(range(10, 1, 0, 20, 1, 10));
        let si2 = SourceInfo::with_range(range(5, 0, 5, 15, 1, 5));
        let combined = si1.combine(&si2);
        // Should take si2's start (5, 0, 5) because it's smaller
        assert_eq!(combined.range.start.offset, 5);
        assert_eq!(combined.range.start.row, 0);
        assert_eq!(combined.range.start.column, 5);
    }

    #[test]
    fn test_source_info_combine_takes_max_end() {
        let si1 = SourceInfo::with_range(range(10, 1, 0, 20, 1, 10));
        let si2 = SourceInfo::with_range(range(5, 0, 5, 15, 1, 5));
        let combined = si1.combine(&si2);
        // Should take si1's end (20, 1, 10) because it's larger
        assert_eq!(combined.range.end.offset, 20);
        assert_eq!(combined.range.end.row, 1);
        assert_eq!(combined.range.end.column, 10);
    }

    #[test]
    fn test_source_info_combine_preserves_filename_index_from_first() {
        let si1 = SourceInfo::new(Some(1), range(10, 1, 0, 20, 1, 10));
        let si2 = SourceInfo::new(Some(2), range(5, 0, 5, 15, 1, 5));
        let combined = si1.combine(&si2);
        // Should take si1's filename_index because it's not None
        assert_eq!(combined.filename_index, Some(1));
    }

    #[test]
    fn test_source_info_combine_uses_second_filename_if_first_is_none() {
        let si1 = SourceInfo::with_range(range(10, 1, 0, 20, 1, 10)); // filename_index is None
        let si2 = SourceInfo::new(Some(42), range(5, 0, 5, 15, 1, 5));
        let combined = si1.combine(&si2);
        // Should fall back to si2's filename_index
        assert_eq!(combined.filename_index, Some(42));
    }

    #[test]
    fn test_source_info_to_source_map_info() {
        let r = range(100, 5, 10, 200, 10, 20);
        let si = SourceInfo::with_range(r);
        let qsm_info = si.to_source_map_info();
        // Verify it creates an Original mapping with correct offsets
        match &qsm_info {
            quarto_source_map::SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => {
                assert_eq!(file_id.0, 0); // Dummy FileId
                assert_eq!(*start_offset, 100);
                assert_eq!(*end_offset, 200);
            }
            _ => panic!("Expected Original variant"),
        }
    }

    #[test]
    fn test_source_info_to_source_map_info_with_mapping() {
        let r = range(50, 2, 5, 100, 4, 10);
        let si = SourceInfo::with_range(r);
        let file_id = quarto_source_map::FileId(123);
        let qsm_info = si.to_source_map_info_with_mapping(file_id);
        // Verify it uses the provided FileId
        match &qsm_info {
            quarto_source_map::SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => {
                assert_eq!(file_id.0, 123);
                assert_eq!(*start_offset, 50);
                assert_eq!(*end_offset, 100);
            }
            _ => panic!("Expected Original variant"),
        }
    }

    // Tests for SourceInfoOptions
    #[test]
    fn test_source_info_options_none() {
        let opts = SourceInfoOptions::none();
        assert!(!opts.trim_leading_whitespace);
        assert!(!opts.trim_trailing_whitespace);
    }

    #[test]
    fn test_source_info_options_trim_all() {
        let opts = SourceInfoOptions::trim_all();
        assert!(opts.trim_leading_whitespace);
        assert!(opts.trim_trailing_whitespace);
    }

    #[test]
    fn test_source_info_options_trim_leading() {
        let opts = SourceInfoOptions::trim_leading();
        assert!(opts.trim_leading_whitespace);
        assert!(!opts.trim_trailing_whitespace);
    }

    #[test]
    fn test_source_info_options_trim_trailing() {
        let opts = SourceInfoOptions::trim_trailing();
        assert!(!opts.trim_leading_whitespace);
        assert!(opts.trim_trailing_whitespace);
    }

    #[test]
    fn test_source_info_options_default() {
        let opts = SourceInfoOptions::default();
        assert!(!opts.trim_leading_whitespace);
        assert!(!opts.trim_trailing_whitespace);
    }

    // Tests for helper functions
    #[test]
    fn test_empty_range() {
        let r = empty_range();
        assert_eq!(r.start.offset, 0);
        assert_eq!(r.start.row, 0);
        assert_eq!(r.start.column, 0);
        assert_eq!(r.end.offset, 0);
        assert_eq!(r.end.row, 0);
        assert_eq!(r.end.column, 0);
    }

    #[test]
    fn test_empty_source_info() {
        let si = empty_source_info();
        assert_eq!(si.start_offset(), 0);
        assert_eq!(si.end_offset(), 0);
        assert_eq!(si.length(), 0);
    }

    #[test]
    fn test_extract_filename_index_original() {
        let si = quarto_source_map::SourceInfo::from_range(
            quarto_source_map::FileId(42),
            quarto_source_map::Range {
                start: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: quarto_source_map::Location {
                    offset: 10,
                    row: 0,
                    column: 10,
                },
            },
        );
        assert_eq!(extract_filename_index(&si), Some(42));
    }

    #[test]
    fn test_extract_filename_index_substring() {
        let parent = quarto_source_map::SourceInfo::from_range(
            quarto_source_map::FileId(99),
            quarto_source_map::Range {
                start: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: quarto_source_map::Location {
                    offset: 100,
                    row: 5,
                    column: 0,
                },
            },
        );
        let substring = quarto_source_map::SourceInfo::substring(parent, 10, 50);
        assert_eq!(extract_filename_index(&substring), Some(99));
    }

    #[test]
    fn test_extract_filename_index_concat() {
        let piece1 = quarto_source_map::SourceInfo::from_range(
            quarto_source_map::FileId(7),
            quarto_source_map::Range {
                start: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: quarto_source_map::Location {
                    offset: 10,
                    row: 0,
                    column: 10,
                },
            },
        );
        let piece2 = quarto_source_map::SourceInfo::from_range(
            quarto_source_map::FileId(8),
            quarto_source_map::Range {
                start: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: quarto_source_map::Location {
                    offset: 20,
                    row: 1,
                    column: 0,
                },
            },
        );
        // concat takes Vec<(SourceInfo, usize)> - pairs of source info and length
        let concat = quarto_source_map::SourceInfo::concat(vec![(piece1, 10), (piece2, 20)]);
        // Should return the first piece's file_id
        assert_eq!(extract_filename_index(&concat), Some(7));
    }

    #[test]
    fn test_extract_filename_index_filter_provenance() {
        // filter_provenance takes filter_path and line number
        let filter_prov = quarto_source_map::SourceInfo::filter_provenance("test-filter.lua", 42);
        // FilterProvenance doesn't have a filename index
        assert_eq!(extract_filename_index(&filter_prov), None);
    }

    #[test]
    fn test_source_info_combine_takes_self_start_when_smaller() {
        // Test case where self.range.start < other.range.start (covers line 53)
        let si1 = SourceInfo::with_range(range(5, 0, 5, 20, 1, 10)); // start at 5
        let si2 = SourceInfo::with_range(range(10, 1, 0, 15, 1, 5)); // start at 10
        let combined = si1.combine(&si2);
        // Should take si1's start (5, 0, 5) because it's smaller
        assert_eq!(combined.range.start.offset, 5);
        assert_eq!(combined.range.start.row, 0);
        assert_eq!(combined.range.start.column, 5);
    }

    #[test]
    fn test_source_info_combine_takes_other_end_when_larger() {
        // Test case where self.range.end <= other.range.end (covers line 60)
        let si1 = SourceInfo::with_range(range(10, 1, 0, 15, 1, 5)); // end at 15
        let si2 = SourceInfo::with_range(range(5, 0, 5, 20, 1, 10)); // end at 20
        let combined = si1.combine(&si2);
        // Should take si2's end (20, 1, 10) because it's larger
        assert_eq!(combined.range.end.offset, 20);
        assert_eq!(combined.range.end.row, 1);
        assert_eq!(combined.range.end.column, 10);
    }
}
