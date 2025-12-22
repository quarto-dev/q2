//! Source information with transformation tracking

use crate::types::{FileId, Range};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Source information tracking a location and its transformation history
///
/// This enum stores only byte offsets. Row and column information is computed
/// on-demand via `map_offset()` using the FileInformation line break index.
///
/// Design notes:
/// - Original: Points directly to a file with byte offsets
/// - Substring: Points to a range within a parent SourceInfo (offsets are relative to parent)
/// - Concat: Combines multiple SourceInfo pieces (preserves provenance when coalescing text)
/// - FilterProvenance: Tracks elements created by Lua filters for diagnostics
///
/// The Transformed variant was removed because it's not used in production code.
/// Text transformations (smart quotes, em-dashes) use Original SourceInfo pointing
/// to the pre-transformation text, accepting that the byte offsets are approximate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceInfo {
    /// Direct position in an original file
    ///
    /// Stores only byte offsets. Use `map_offset()` to get row/column information.
    Original {
        file_id: FileId,
        start_offset: usize,
        end_offset: usize,
    },
    /// Substring extraction from a parent source
    ///
    /// Offsets are relative to the parent's text.
    /// The chain of Substrings always resolves to an Original.
    Substring {
        parent: Arc<SourceInfo>,
        start_offset: usize,
        end_offset: usize,
    },
    /// Concatenation of multiple sources
    ///
    /// Used when coalescing adjacent text nodes while preserving
    /// the fact that they came from different source locations.
    Concat { pieces: Vec<SourcePiece> },
    /// Provenance from a Lua filter
    ///
    /// Used to track elements created by Lua filters for diagnostic messages.
    /// Contains the filter file path and line number where the element was created.
    FilterProvenance {
        /// Path to the Lua filter file (from debug.getinfo source)
        filter_path: String,
        /// Line number in the filter where the element was created
        line: usize,
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

impl Default for SourceInfo {
    fn default() -> Self {
        SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 0,
        }
    }
}

impl SourceInfo {
    /// Create source info for a position in an original file (from offsets)
    pub fn original(file_id: FileId, start_offset: usize, end_offset: usize) -> Self {
        SourceInfo::Original {
            file_id,
            start_offset,
            end_offset,
        }
    }

    /// Create source info for a position in an original file (from Range)
    ///
    /// This is a compatibility helper for code that still uses Range.
    /// The row and column information in the Range is ignored; only offsets are stored.
    pub fn from_range(file_id: FileId, range: Range) -> Self {
        SourceInfo::Original {
            file_id,
            start_offset: range.start.offset,
            end_offset: range.end.offset,
        }
    }

    /// Create source info for a substring extraction
    pub fn substring(parent: SourceInfo, start: usize, end: usize) -> Self {
        SourceInfo::Substring {
            parent: Arc::new(parent),
            start_offset: start,
            end_offset: end,
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

        SourceInfo::Concat {
            pieces: pieces_with_offsets,
        }
    }

    /// Create source info for a filter-created element
    ///
    /// Used to track the provenance of elements created by Lua filters.
    /// The filter_path should be the path to the filter file (from debug.getinfo source).
    /// The line should be the line number where the element was created.
    pub fn filter_provenance(filter_path: impl Into<String>, line: usize) -> Self {
        SourceInfo::FilterProvenance {
            filter_path: filter_path.into(),
            line,
        }
    }

    /// Combine two SourceInfo objects representing adjacent text
    ///
    /// This creates a Concat mapping that preserves both sources.
    /// The resulting SourceInfo spans from the start of self to the end of other.
    pub fn combine(&self, other: &SourceInfo) -> Self {
        let self_length = self.length();
        let other_length = other.length();

        SourceInfo::concat(vec![
            (self.clone(), self_length),
            (other.clone(), other_length),
        ])
    }

    /// Get the length (in bytes) represented by this SourceInfo
    pub fn length(&self) -> usize {
        match self {
            SourceInfo::Original {
                start_offset,
                end_offset,
                ..
            } => end_offset - start_offset,
            SourceInfo::Substring {
                start_offset,
                end_offset,
                ..
            } => end_offset - start_offset,
            SourceInfo::Concat { pieces } => pieces.iter().map(|p| p.length).sum(),
            SourceInfo::FilterProvenance { .. } => 0,
        }
    }

    /// Get the start offset for this SourceInfo
    ///
    /// For Original and Substring, returns the start_offset field.
    /// For Concat, returns 0 (the concat represents a new text starting at 0).
    /// For FilterProvenance, returns 0.
    pub fn start_offset(&self) -> usize {
        match self {
            SourceInfo::Original { start_offset, .. } => *start_offset,
            SourceInfo::Substring { start_offset, .. } => *start_offset,
            SourceInfo::Concat { .. } => 0,
            SourceInfo::FilterProvenance { .. } => 0,
        }
    }

    /// Get the end offset for this SourceInfo
    ///
    /// For Original and Substring, returns the end_offset field.
    /// For Concat, returns the total length.
    /// For FilterProvenance, returns 0.
    pub fn end_offset(&self) -> usize {
        match self {
            SourceInfo::Original { end_offset, .. } => *end_offset,
            SourceInfo::Substring { end_offset, .. } => *end_offset,
            SourceInfo::Concat { .. } => self.length(),
            SourceInfo::FilterProvenance { .. } => 0,
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
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 10,
                row: 0,
                column: 10,
            },
        };

        let info = SourceInfo::from_range(file_id, range.clone());

        assert_eq!(info.start_offset(), 0);
        assert_eq!(info.end_offset(), 10);
        assert_eq!(info.length(), 10);
        match info {
            SourceInfo::Original {
                file_id: mapped_id, ..
            } => {
                assert_eq!(mapped_id, file_id);
            }
            _ => panic!("Expected Original mapping"),
        }
    }

    #[test]
    fn test_source_info_serialization() {
        let file_id = FileId(0);
        let range = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 10,
                row: 0,
                column: 10,
            },
        };

        let info = SourceInfo::from_range(file_id, range);
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SourceInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info, deserialized);
    }

    #[test]
    fn test_substring_source_info() {
        let file_id = FileId(0);
        let parent_range = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 100,
                row: 0,
                column: 100,
            },
        };
        let parent = SourceInfo::from_range(file_id, parent_range);

        let substring = SourceInfo::substring(parent, 10, 20);

        assert_eq!(substring.start_offset(), 10);
        assert_eq!(substring.end_offset(), 20);
        assert_eq!(substring.length(), 10);

        match substring {
            SourceInfo::Substring {
                start_offset,
                end_offset,
                ..
            } => {
                assert_eq!(start_offset, 10);
                assert_eq!(end_offset, 20);
            }
            _ => panic!("Expected Substring mapping"),
        }
    }

    #[test]
    fn test_concat_source_info() {
        let file_id1 = FileId(0);
        let file_id2 = FileId(1);

        let info1 = SourceInfo::from_range(
            file_id1,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 10,
                    row: 0,
                    column: 10,
                },
            },
        );

        let info2 = SourceInfo::from_range(
            file_id2,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 15,
                    row: 0,
                    column: 15,
                },
            },
        );

        let concat = SourceInfo::concat(vec![(info1, 10), (info2, 15)]);

        assert_eq!(concat.start_offset(), 0);
        assert_eq!(concat.end_offset(), 25); // 10 + 15
        assert_eq!(concat.length(), 25);

        match concat {
            SourceInfo::Concat { pieces } => {
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
    fn test_combine_two_sources() {
        let file_id = FileId(0);

        // Create two separate source info objects
        let info1 = SourceInfo::from_range(
            file_id,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 10,
                    row: 0,
                    column: 10,
                },
            },
        );

        let info2 = SourceInfo::from_range(
            file_id,
            Range {
                start: Location {
                    offset: 15,
                    row: 0,
                    column: 15,
                },
                end: Location {
                    offset: 25,
                    row: 0,
                    column: 25,
                },
            },
        );

        // Combine them
        let combined = info1.combine(&info2);

        // Should create a Concat with total length = 10 + 10 = 20
        assert_eq!(combined.start_offset(), 0);
        assert_eq!(combined.end_offset(), 20);
        assert_eq!(combined.length(), 20);

        match combined {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(pieces[0].length, 10);
                assert_eq!(pieces[0].offset_in_concat, 0);
                assert_eq!(pieces[1].length, 10);
                assert_eq!(pieces[1].offset_in_concat, 10);
            }
            _ => panic!("Expected Concat mapping"),
        }
    }

    #[test]
    fn test_combine_preserves_source_tracking() {
        // Combine sources from different files
        let file_id1 = FileId(5);
        let file_id2 = FileId(10);

        let info1 = SourceInfo::from_range(
            file_id1,
            Range {
                start: Location {
                    offset: 100,
                    row: 5,
                    column: 0,
                },
                end: Location {
                    offset: 105,
                    row: 5,
                    column: 5,
                },
            },
        );

        let info2 = SourceInfo::from_range(
            file_id2,
            Range {
                start: Location {
                    offset: 200,
                    row: 10,
                    column: 0,
                },
                end: Location {
                    offset: 207,
                    row: 10,
                    column: 7,
                },
            },
        );

        let combined = info1.combine(&info2);

        // Verify both sources are preserved in the Concat
        match combined {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);

                // First piece should come from file_id1
                match &pieces[0].source_info {
                    SourceInfo::Original { file_id, .. } => assert_eq!(*file_id, file_id1),
                    _ => panic!("Expected Original mapping for first piece"),
                }

                // Second piece should come from file_id2
                match &pieces[1].source_info {
                    SourceInfo::Original { file_id, .. } => assert_eq!(*file_id, file_id2),
                    _ => panic!("Expected Original mapping for second piece"),
                }
            }
            _ => panic!("Expected Concat mapping"),
        }
    }

    /// Test JSON serialization of Original mapping
    #[test]
    fn test_json_serialization_original() {
        let file_id = FileId(0);
        let range = Range {
            start: Location {
                offset: 10,
                row: 1,
                column: 5,
            },
            end: Location {
                offset: 50,
                row: 3,
                column: 10,
            },
        };

        let info = SourceInfo::from_range(file_id, range);
        let json = serde_json::to_value(&info).unwrap();

        // Verify JSON structure
        assert_eq!(json["Original"]["file_id"], 0);
        assert_eq!(json["Original"]["start_offset"], 10);
        assert_eq!(json["Original"]["end_offset"], 50);

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(info, deserialized);
    }

    /// Test JSON serialization of Substring mapping
    #[test]
    fn test_json_serialization_substring() {
        let file_id = FileId(0);
        let parent_range = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 100,
                row: 5,
                column: 20,
            },
        };
        let parent = SourceInfo::from_range(file_id, parent_range);

        let substring = SourceInfo::substring(parent, 10, 30);
        let json = serde_json::to_value(&substring).unwrap();

        // Verify JSON structure
        assert_eq!(json["Substring"]["start_offset"], 10);
        assert_eq!(json["Substring"]["end_offset"], 30);

        // Verify parent is serialized (with Rc, it's a full copy in JSON)
        assert!(json["Substring"]["parent"].is_object());
        assert_eq!(json["Substring"]["parent"]["Original"]["file_id"], 0);

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(substring, deserialized);
    }

    /// Test JSON serialization of nested Substring mappings (simulates .qmd frontmatter)
    #[test]
    fn test_json_serialization_nested_substring() {
        let file_id = FileId(0);

        // Level 1: Original file
        let file_range = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 200,
                row: 10,
                column: 0,
            },
        };
        let file_info = SourceInfo::from_range(file_id, file_range);

        // Level 2: YAML frontmatter (substring of file)
        let yaml_info = SourceInfo::substring(file_info, 4, 150);

        // Level 3: YAML value (substring of frontmatter)
        let value_info = SourceInfo::substring(yaml_info, 20, 35);

        let json = serde_json::to_value(&value_info).unwrap();

        // Verify nested structure
        assert_eq!(json["Substring"]["start_offset"], 20);
        assert_eq!(json["Substring"]["end_offset"], 35);
        assert_eq!(json["Substring"]["parent"]["Substring"]["start_offset"], 4);
        assert_eq!(
            json["Substring"]["parent"]["Substring"]["parent"]["Original"]["file_id"],
            0
        );

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(value_info, deserialized);
    }

    /// Test JSON serialization of Concat mapping
    #[test]
    fn test_json_serialization_concat() {
        let file_id1 = FileId(0);
        let file_id2 = FileId(1);

        let info1 = SourceInfo::from_range(
            file_id1,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 10,
                    row: 0,
                    column: 10,
                },
            },
        );

        let info2 = SourceInfo::from_range(
            file_id2,
            Range {
                start: Location {
                    offset: 20,
                    row: 2,
                    column: 0,
                },
                end: Location {
                    offset: 30,
                    row: 2,
                    column: 10,
                },
            },
        );

        let combined = info1.combine(&info2);
        let json = serde_json::to_value(&combined).unwrap();

        // Verify JSON structure
        assert!(json["Concat"]["pieces"].is_array());
        let pieces = json["Concat"]["pieces"].as_array().unwrap();
        assert_eq!(pieces.len(), 2);

        // First piece
        assert_eq!(pieces[0]["offset_in_concat"], 0);
        assert_eq!(pieces[0]["length"], 10);
        assert_eq!(pieces[0]["source_info"]["Original"]["file_id"], 0);

        // Second piece
        assert_eq!(pieces[1]["offset_in_concat"], 10);
        assert_eq!(pieces[1]["length"], 10);
        assert_eq!(pieces[1]["source_info"]["Original"]["file_id"], 1);

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(combined, deserialized);
    }

    /// Test JSON serialization of complex nested structure (real-world example)
    #[test]
    fn test_json_serialization_complex_nested() {
        let file_id = FileId(0);

        // Simulate a .qmd file structure
        let qmd_file = SourceInfo::from_range(
            file_id,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 500,
                    row: 20,
                    column: 0,
                },
            },
        );

        // YAML frontmatter is a substring
        let yaml_frontmatter = SourceInfo::substring(qmd_file.clone(), 4, 200);

        // A YAML key is a substring of frontmatter
        let yaml_key = SourceInfo::substring(yaml_frontmatter.clone(), 10, 20);

        // A YAML value is another substring of frontmatter
        let yaml_value = SourceInfo::substring(yaml_frontmatter, 25, 50);

        // Combine key and value (simulating metadata entry)
        let combined = yaml_key.combine(&yaml_value);

        let json = serde_json::to_value(&combined).unwrap();

        // Verify this complex structure serializes
        assert!(json.is_object());
        assert!(json["Concat"].is_object());

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(combined, deserialized);
    }
}
