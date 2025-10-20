//! Source information with transformation tracking

use crate::types::{FileId, Location, Range};
use serde::{Deserialize, Serialize};
use std::rc::Rc;

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
    Original { file_id: FileId },
    /// Substring extraction from a parent source
    Substring {
        parent: Rc<SourceInfo>,
        offset: usize,
    },
    /// Concatenation of multiple sources
    Concat { pieces: Vec<SourcePiece> },
    /// Transformed text with piecewise mapping
    Transformed {
        parent: Rc<SourceInfo>,
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
                parent: Rc::new(parent),
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
        let total_length = mapping.iter().map(|m| m.from_end).max().unwrap_or(0);

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
                parent: Rc::new(parent),
                mapping,
            },
        }
    }

    /// Combine two SourceInfo objects representing adjacent text
    ///
    /// This creates a Concat mapping that preserves both sources.
    /// The resulting SourceInfo spans from the start of self to the end of other.
    pub fn combine(&self, other: &SourceInfo) -> Self {
        let self_length = self.range.end.offset - self.range.start.offset;
        let other_length = other.range.end.offset - other.range.start.offset;

        SourceInfo::concat(vec![
            (self.clone(), self_length),
            (other.clone(), other_length),
        ])
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

        let info = SourceInfo::original(file_id, range);
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

        let info2 = SourceInfo::original(
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
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 50,
                    row: 0,
                    column: 50,
                },
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
        match &transformed.mapping {
            SourceMapping::Transformed { parent, .. } => match &parent.mapping {
                SourceMapping::Substring {
                    parent: grandparent,
                    offset,
                } => {
                    assert_eq!(*offset, 10);
                    match &grandparent.mapping {
                        SourceMapping::Original { file_id: id } => {
                            assert_eq!(*id, file_id);
                        }
                        _ => panic!("Expected Original at root"),
                    }
                }
                _ => panic!("Expected Substring as parent"),
            },
            _ => panic!("Expected Transformed at top level"),
        }
    }

    #[test]
    fn test_combine_two_sources() {
        let file_id = FileId(0);

        // Create two separate source info objects
        let info1 = SourceInfo::original(
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

        let info2 = SourceInfo::original(
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
        assert_eq!(combined.range.start.offset, 0);
        assert_eq!(combined.range.end.offset, 20);

        match combined.mapping {
            SourceMapping::Concat { pieces } => {
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

        let info1 = SourceInfo::original(
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

        let info2 = SourceInfo::original(
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
        match combined.mapping {
            SourceMapping::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);

                // First piece should come from file_id1
                match &pieces[0].source_info.mapping {
                    SourceMapping::Original { file_id } => assert_eq!(*file_id, file_id1),
                    _ => panic!("Expected Original mapping for first piece"),
                }

                // Second piece should come from file_id2
                match &pieces[1].source_info.mapping {
                    SourceMapping::Original { file_id } => assert_eq!(*file_id, file_id2),
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

        let info = SourceInfo::original(file_id, range);
        let json = serde_json::to_value(&info).unwrap();

        // Verify JSON structure
        assert_eq!(json["range"]["start"]["offset"], 10);
        assert_eq!(json["range"]["start"]["row"], 1);
        assert_eq!(json["range"]["start"]["column"], 5);
        assert_eq!(json["range"]["end"]["offset"], 50);
        assert_eq!(json["range"]["end"]["row"], 3);
        assert_eq!(json["range"]["end"]["column"], 10);
        assert_eq!(json["mapping"]["Original"]["file_id"], 0);

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
        let parent = SourceInfo::original(file_id, parent_range);

        let substring = SourceInfo::substring(parent, 10, 30);
        let json = serde_json::to_value(&substring).unwrap();

        // Verify JSON structure
        assert_eq!(json["range"]["start"]["offset"], 0);
        assert_eq!(json["range"]["end"]["offset"], 20); // length = 30 - 10 = 20
        assert_eq!(json["mapping"]["Substring"]["offset"], 10);

        // Verify parent is serialized (with Rc, it's a full copy in JSON)
        assert!(json["mapping"]["Substring"]["parent"].is_object());
        assert_eq!(
            json["mapping"]["Substring"]["parent"]["mapping"]["Original"]["file_id"],
            0
        );

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
        let file_info = SourceInfo::original(file_id, file_range);

        // Level 2: YAML frontmatter (substring of file)
        let yaml_info = SourceInfo::substring(file_info, 4, 150);

        // Level 3: YAML value (substring of frontmatter)
        let value_info = SourceInfo::substring(yaml_info, 20, 35);

        let json = serde_json::to_value(&value_info).unwrap();

        // Verify nested structure
        assert_eq!(json["mapping"]["Substring"]["offset"], 20);
        assert_eq!(
            json["mapping"]["Substring"]["parent"]["mapping"]["Substring"]["offset"],
            4
        );
        assert_eq!(
            json["mapping"]["Substring"]["parent"]["mapping"]["Substring"]["parent"]["mapping"]["Original"]
                ["file_id"],
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

        let info1 = SourceInfo::original(
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

        let info2 = SourceInfo::original(
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
        assert!(json["mapping"]["Concat"]["pieces"].is_array());
        let pieces = json["mapping"]["Concat"]["pieces"].as_array().unwrap();
        assert_eq!(pieces.len(), 2);

        // First piece
        assert_eq!(pieces[0]["offset_in_concat"], 0);
        assert_eq!(pieces[0]["length"], 10);
        assert_eq!(
            pieces[0]["source_info"]["mapping"]["Original"]["file_id"],
            0
        );

        // Second piece
        assert_eq!(pieces[1]["offset_in_concat"], 10);
        assert_eq!(pieces[1]["length"], 10);
        assert_eq!(
            pieces[1]["source_info"]["mapping"]["Original"]["file_id"],
            1
        );

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(combined, deserialized);
    }

    /// Test JSON serialization of Transformed mapping
    #[test]
    fn test_json_serialization_transformed() {
        use crate::RangeMapping;

        let file_id = FileId(0);
        let parent = SourceInfo::original(
            file_id,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 20,
                    row: 0,
                    column: 20,
                },
            },
        );

        // Create a transformed source with range mappings
        let mappings = vec![
            RangeMapping {
                from_start: 0,
                from_end: 5,
                to_start: 0,
                to_end: 5,
            },
            RangeMapping {
                from_start: 5,
                from_end: 10,
                to_start: 10,
                to_end: 15,
            },
        ];

        let transformed = SourceInfo {
            range: Range {
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
            mapping: SourceMapping::Transformed {
                parent: Rc::new(parent),
                mapping: mappings.clone(),
            },
        };

        let json = serde_json::to_value(&transformed).unwrap();

        // Verify JSON structure
        assert!(json["mapping"]["Transformed"]["mapping"].is_array());
        let json_mappings = json["mapping"]["Transformed"]["mapping"]
            .as_array()
            .unwrap();
        assert_eq!(json_mappings.len(), 2);

        // Verify first mapping
        assert_eq!(json_mappings[0]["from_start"], 0);
        assert_eq!(json_mappings[0]["from_end"], 5);
        assert_eq!(json_mappings[0]["to_start"], 0);
        assert_eq!(json_mappings[0]["to_end"], 5);

        // Verify second mapping
        assert_eq!(json_mappings[1]["from_start"], 5);
        assert_eq!(json_mappings[1]["from_end"], 10);
        assert_eq!(json_mappings[1]["to_start"], 10);
        assert_eq!(json_mappings[1]["to_end"], 15);

        // Verify parent is serialized
        assert_eq!(
            json["mapping"]["Transformed"]["parent"]["mapping"]["Original"]["file_id"],
            0
        );

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(transformed, deserialized);
    }

    /// Test JSON serialization of complex nested structure (real-world example)
    #[test]
    fn test_json_serialization_complex_nested() {
        let file_id = FileId(0);

        // Simulate a .qmd file structure
        let qmd_file = SourceInfo::original(
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
        assert!(json["mapping"]["Concat"].is_object());

        // Verify round-trip
        let deserialized: SourceInfo = serde_json::from_value(json).unwrap();
        assert_eq!(combined, deserialized);
    }
}
