//! Core types for source mapping

use serde::{Deserialize, Serialize};

/// A unique identifier for a source file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub usize);

/// A location in source text (0-indexed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Location {
    /// Byte offset from start of source
    pub offset: usize,
    /// Row number (0-indexed)
    pub row: usize,
    /// Column number (0-indexed, in characters not bytes)
    pub column: usize,
}

/// A range in source text from start to end
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// Start location (inclusive)
    pub start: Location,
    /// End location (exclusive)
    pub end: Location,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_id_equality() {
        let id1 = FileId(0);
        let id2 = FileId(0);
        let id3 = FileId(1);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_location_ordering() {
        let loc1 = Location {
            offset: 0,
            row: 0,
            column: 0,
        };
        let loc2 = Location {
            offset: 5,
            row: 0,
            column: 5,
        };
        let loc3 = Location {
            offset: 10,
            row: 1,
            column: 0,
        };

        assert!(loc1 < loc2);
        assert!(loc2 < loc3);
        assert!(loc1 < loc3);
    }

    #[test]
    fn test_location_equality() {
        let loc1 = Location {
            offset: 5,
            row: 0,
            column: 5,
        };
        let loc2 = Location {
            offset: 5,
            row: 0,
            column: 5,
        };
        let loc3 = Location {
            offset: 6,
            row: 0,
            column: 6,
        };

        assert_eq!(loc1, loc2);
        assert_ne!(loc1, loc3);
    }

    #[test]
    fn test_range_equality() {
        let range1 = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 5,
                row: 0,
                column: 5,
            },
        };
        let range2 = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 5,
                row: 0,
                column: 5,
            },
        };
        let range3 = Range {
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

        assert_eq!(range1, range2);
        assert_ne!(range1, range3);
    }

    #[test]
    fn test_serialization_file_id() {
        let id = FileId(42);
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: FileId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn test_serialization_location() {
        let loc = Location {
            offset: 100,
            row: 5,
            column: 10,
        };
        let json = serde_json::to_string(&loc).unwrap();
        let deserialized: Location = serde_json::from_str(&json).unwrap();
        assert_eq!(loc, deserialized);
    }

    #[test]
    fn test_serialization_range() {
        let range = Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 50,
                row: 2,
                column: 10,
            },
        };
        let json = serde_json::to_string(&range).unwrap();
        let deserialized: Range = serde_json::from_str(&json).unwrap();
        assert_eq!(range, deserialized);
    }
}
