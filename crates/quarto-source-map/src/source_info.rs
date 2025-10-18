//! Source information with transformation tracking

use crate::types::{FileId, Location, Range};
use serde::{Deserialize, Serialize};

/// Source information tracking a location and its transformation history
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// The range in the immediate/current text
    pub range: Range,
    /// How this range maps to its source
    pub mapping: SourceMapping,
}

/// Describes how source content was transformed
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceMapping {
    /// Direct position in an original file
    Original {
        file_id: FileId,
    },
    /// Substring extraction from a parent source
    Substring {
        parent: Box<SourceInfo>,
        offset: usize,
    },
    /// Concatenation of multiple sources
    Concat {
        pieces: Vec<SourcePiece>,
    },
    /// Transformed text with piecewise mapping
    Transformed {
        parent: Box<SourceInfo>,
        mapping: Vec<RangeMapping>,
    },
}

/// A piece of a concatenated source
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourcePiece {
    /// Source information for this piece
    pub source_info: SourceInfo,
    /// Where this piece starts in the concatenated string
    pub offset_in_concat: usize,
    /// Length of this piece
    pub length: usize,
}

/// Maps a range in transformed text to parent text
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeMapping {
    /// Start offset in transformed text
    pub from_start: usize,
    /// End offset in transformed text
    pub from_end: usize,
    /// Start offset in parent text
    pub to_start: usize,
    /// End offset in parent text
    pub to_end: usize,
}

impl Default for SourceInfo {
    fn default() -> Self {
        SourceInfo::original(
            FileId(0),
            Range {
                start: Location { offset: 0, row: 0, column: 0 },
                end: Location { offset: 0, row: 0, column: 0 },
            },
        )
    }
}

impl SourceInfo {
    /// Create source info for a position in an original file
    pub fn original(file_id: FileId, range: Range) -> Self {
        SourceInfo {
            range,
            mapping: SourceMapping::Original { file_id },
        }
    }

    /// Create source info for a substring extraction
    pub fn substring(parent: SourceInfo, start: usize, end: usize) -> Self {
        let length = end - start;
        SourceInfo {
            range: Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: length,
                    row: 0,
                    column: 0,
                },
            },
            mapping: SourceMapping::Substring {
                parent: Box::new(parent),
                offset: start,
            },
        }
    }

    /// Create source info for concatenated sources
    pub fn concat(pieces: Vec<(SourceInfo, usize)>) -> Self {
        let source_pieces: Vec<SourcePiece> = pieces
            .into_iter()
            .map(|(source_info, length)| SourcePiece {
                source_info,
                offset_in_concat: 0, // Will be calculated based on cumulative lengths
                length,
            })
            .collect();

        // Calculate cumulative offsets
        let mut cumulative_offset = 0;
        let pieces_with_offsets: Vec<SourcePiece> = source_pieces
            .into_iter()
            .map(|mut piece| {
                piece.offset_in_concat = cumulative_offset;
                cumulative_offset += piece.length;
                piece
            })
            .collect();

        let total_length = cumulative_offset;

        SourceInfo {
            range: Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: total_length,
                    row: 0,
                    column: 0,
                },
            },
            mapping: SourceMapping::Concat {
                pieces: pieces_with_offsets,
            },
        }
    }

    /// Create source info for transformed text
    pub fn transformed(parent: SourceInfo, mapping: Vec<RangeMapping>) -> Self {
        // Find the max end offset in the transformed text
        let total_length = mapping
            .iter()
            .map(|m| m.from_end)
            .max()
            .unwrap_or(0);

        SourceInfo {
            range: Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: total_length,
                    row: 0,
                    column: 0,
                },
            },
            mapping: SourceMapping::Transformed {
                parent: Box::new(parent),
                mapping,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileId, Location, Range};

    #[test]
    fn test_original_source_info() {
        let file_id = FileId(0);
        let range = Range {
            start: Location { offset: 0, row: 0, column: 0 },
            end: Location { offset: 10, row: 0, column: 10 },
        };

        let info = SourceInfo::original(file_id, range.clone());

        assert_eq!(info.range, range);
        match info.mapping {
            SourceMapping::Original { file_id: mapped_id } => {
                assert_eq!(mapped_id, file_id);
            }
            _ => panic!("Expected Original mapping"),
        }
    }

    #[test]
    fn test_source_info_serialization() {
        let file_id = FileId(0);
        let range = Range {
            start: Location { offset: 0, row: 0, column: 0 },
            end: Location { offset: 10, row: 0, column: 10 },
        };

        let info = SourceInfo::original(file_id, range);
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SourceInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info, deserialized);
    }

    #[test]
    fn test_substring_source_info() {
        let file_id = FileId(0);
        let parent_range = Range {
            start: Location { offset: 0, row: 0, column: 0 },
            end: Location { offset: 100, row: 0, column: 100 },
        };
        let parent = SourceInfo::original(file_id, parent_range);

        let substring = SourceInfo::substring(parent, 10, 20);

        assert_eq!(substring.range.start.offset, 0);
        assert_eq!(substring.range.end.offset, 10); // length = 20 - 10 = 10

        match substring.mapping {
            SourceMapping::Substring { offset, .. } => {
                assert_eq!(offset, 10);
            }
            _ => panic!("Expected Substring mapping"),
        }
    }

    #[test]
    fn test_concat_source_info() {
        let file_id1 = FileId(0);
        let file_id2 = FileId(1);

        let info1 = SourceInfo::original(
            file_id1,
            Range {
                start: Location { offset: 0, row: 0, column: 0 },
                end: Location { offset: 10, row: 0, column: 10 },
            },
        );

        let info2 = SourceInfo::original(
            file_id2,
            Range {
                start: Location { offset: 0, row: 0, column: 0 },
                end: Location { offset: 15, row: 0, column: 15 },
            },
        );

        let concat = SourceInfo::concat(vec![(info1, 10), (info2, 15)]);

        assert_eq!(concat.range.start.offset, 0);
        assert_eq!(concat.range.end.offset, 25); // 10 + 15

        match concat.mapping {
            SourceMapping::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(pieces[0].offset_in_concat, 0);
                assert_eq!(pieces[0].length, 10);
                assert_eq!(pieces[1].offset_in_concat, 10);
                assert_eq!(pieces[1].length, 15);
            }
            _ => panic!("Expected Concat mapping"),
        }
    }

    #[test]
    fn test_transformed_source_info() {
        let file_id = FileId(0);
        let parent = SourceInfo::original(
            file_id,
            Range {
                start: Location { offset: 0, row: 0, column: 0 },
                end: Location { offset: 50, row: 0, column: 50 },
            },
        );

        let mapping = vec![
            RangeMapping {
                from_start: 0,
                from_end: 10,
                to_start: 0,
                to_end: 10,
            },
            RangeMapping {
                from_start: 10,
                from_end: 20,
                to_start: 20,
                to_end: 30,
            },
        ];

        let transformed = SourceInfo::transformed(parent, mapping.clone());

        assert_eq!(transformed.range.start.offset, 0);
        assert_eq!(transformed.range.end.offset, 20); // max from_end

        match transformed.mapping {
            SourceMapping::Transformed { mapping: m, .. } => {
                assert_eq!(m, mapping);
            }
            _ => panic!("Expected Transformed mapping"),
        }
    }

    #[test]
    fn test_nested_transformations() {
        let file_id = FileId(0);
        let original = SourceInfo::original(
            file_id,
            Range {
                start: Location { offset: 0, row: 0, column: 0 },
                end: Location { offset: 100, row: 0, column: 100 },
            },
        );

        // Extract a substring
        let substring = SourceInfo::substring(original, 10, 50);

        // Then transform it
        let transformed = SourceInfo::transformed(
            substring,
            vec![RangeMapping {
                from_start: 0,
                from_end: 10,
                to_start: 0,
                to_end: 10,
            }],
        );

        // Verify the chain: Original -> Substring -> Transformed
        match transformed.mapping {
            SourceMapping::Transformed { parent, .. } => match parent.mapping {
                SourceMapping::Substring { parent: grandparent, offset } => {
                    assert_eq!(offset, 10);
                    match grandparent.mapping {
                        SourceMapping::Original { file_id: id } => {
                            assert_eq!(id, file_id);
                        }
                        _ => panic!("Expected Original at root"),
                    }
                }
                _ => panic!("Expected Substring as parent"),
            },
            _ => panic!("Expected Transformed at top level"),
        }
    }
}
