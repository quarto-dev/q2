//! Utility functions for working with source positions

use crate::types::{Location, Range};

/// Convert a byte offset to a Location with line and column info
///
/// Returns None if the offset is out of bounds.
pub fn offset_to_location(source: &str, offset: usize) -> Option<Location> {
    if offset > source.len() {
        return None;
    }

    let mut row = 0;
    let mut column = 0;
    let mut current_offset = 0;

    for ch in source.chars() {
        if current_offset >= offset {
            break;
        }

        if ch == '\n' {
            row += 1;
            column = 0;
        } else {
            column += 1;
        }

        current_offset += ch.len_utf8();
    }

    Some(Location {
        offset,
        row,
        column,
    })
}

/// Convert line and column numbers to a byte offset
///
/// Line and column are 0-indexed. Returns None if out of bounds.
pub fn line_col_to_offset(source: &str, line: usize, col: usize) -> Option<usize> {
    let mut current_line = 0;
    let mut current_col = 0;
    let mut offset = 0;

    for ch in source.chars() {
        if current_line == line && current_col == col {
            return Some(offset);
        }

        if ch == '\n' {
            current_line += 1;
            current_col = 0;
        } else {
            current_col += 1;
        }

        offset += ch.len_utf8();
    }

    // Check if we're at the end position
    if current_line == line && current_col == col {
        return Some(offset);
    }

    None
}

/// Create a Range from start and end byte offsets
///
/// This is a helper that creates a Range with Location structs
/// that only have offsets filled in (row and column are 0).
/// Use `offset_to_location` to get full Location info.
pub fn range_from_offsets(start: usize, end: usize) -> Range {
    Range {
        start: Location {
            offset: start,
            row: 0,
            column: 0,
        },
        end: Location {
            offset: end,
            row: 0,
            column: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_to_location_simple() {
        let source = "hello\nworld";

        // Beginning
        let loc = offset_to_location(source, 0).unwrap();
        assert_eq!(loc.offset, 0);
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 0);

        // Middle of first line
        let loc = offset_to_location(source, 3).unwrap();
        assert_eq!(loc.offset, 3);
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 3);

        // After newline (beginning of second line)
        let loc = offset_to_location(source, 6).unwrap();
        assert_eq!(loc.offset, 6);
        assert_eq!(loc.row, 1);
        assert_eq!(loc.column, 0);

        // Middle of second line
        let loc = offset_to_location(source, 9).unwrap();
        assert_eq!(loc.offset, 9);
        assert_eq!(loc.row, 1);
        assert_eq!(loc.column, 3);
    }

    #[test]
    fn test_offset_to_location_out_of_bounds() {
        let source = "hello";
        assert!(offset_to_location(source, 100).is_none());
    }

    #[test]
    fn test_offset_to_location_end() {
        let source = "hello";
        let loc = offset_to_location(source, 5).unwrap();
        assert_eq!(loc.offset, 5);
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 5);
    }

    #[test]
    fn test_line_col_to_offset_simple() {
        let source = "hello\nworld";

        // Beginning
        let offset = line_col_to_offset(source, 0, 0).unwrap();
        assert_eq!(offset, 0);

        // Middle of first line
        let offset = line_col_to_offset(source, 0, 3).unwrap();
        assert_eq!(offset, 3);

        // Beginning of second line
        let offset = line_col_to_offset(source, 1, 0).unwrap();
        assert_eq!(offset, 6);

        // Middle of second line
        let offset = line_col_to_offset(source, 1, 3).unwrap();
        assert_eq!(offset, 9);
    }

    #[test]
    fn test_line_col_to_offset_out_of_bounds() {
        let source = "hello\nworld";
        assert!(line_col_to_offset(source, 10, 0).is_none());
        assert!(line_col_to_offset(source, 0, 100).is_none());
    }

    #[test]
    fn test_line_col_to_offset_end() {
        let source = "hello";
        let offset = line_col_to_offset(source, 0, 5).unwrap();
        assert_eq!(offset, 5);
    }

    #[test]
    fn test_roundtrip() {
        let source = "hello\nworld\ntest";

        // Test various positions
        for test_offset in [0, 3, 6, 10, 16] {
            let loc = offset_to_location(source, test_offset).unwrap();
            let back_to_offset = line_col_to_offset(source, loc.row, loc.column).unwrap();
            assert_eq!(test_offset, back_to_offset);
        }
    }

    #[test]
    fn test_range_from_offsets() {
        let range = range_from_offsets(10, 20);
        assert_eq!(range.start.offset, 10);
        assert_eq!(range.end.offset, 20);
        assert_eq!(range.start.row, 0);
        assert_eq!(range.start.column, 0);
    }

    #[test]
    fn test_offset_to_location_multiline() {
        let source = "line1\nline2\nline3";

        // Test each line start
        let loc = offset_to_location(source, 0).unwrap();
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 0);

        let loc = offset_to_location(source, 6).unwrap();
        assert_eq!(loc.row, 1);
        assert_eq!(loc.column, 0);

        let loc = offset_to_location(source, 12).unwrap();
        assert_eq!(loc.row, 2);
        assert_eq!(loc.column, 0);
    }
}
