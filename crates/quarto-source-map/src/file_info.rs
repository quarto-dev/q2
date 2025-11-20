//! Efficient file information for location lookups

use crate::types::Location;
use serde::{Deserialize, Serialize};

/// Efficient file content analysis for location lookups
///
/// This struct stores metadata about a file that enables fast conversion
/// from byte offsets to (row, column) positions without storing the full
/// file content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileInformation {
    /// Byte offsets of each newline character in the file
    line_breaks: Vec<usize>,

    /// Total length of the file in bytes
    total_length: usize,
}

impl FileInformation {
    /// Create file information by analyzing content
    ///
    /// Scans the content once to build an index of line break positions.
    /// This enables O(log n) offset-to-location lookups via binary search.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_source_map::FileInformation;
    ///
    /// let info = FileInformation::new("line 1\nline 2\nline 3");
    /// ```
    pub fn new(content: &str) -> Self {
        let line_breaks: Vec<usize> = content
            .char_indices()
            .filter_map(|(idx, ch)| if ch == '\n' { Some(idx) } else { None })
            .collect();

        FileInformation {
            line_breaks,
            total_length: content.len(),
        }
    }

    /// Create file information from pre-computed parts
    ///
    /// This is useful when deserializing from formats that store
    /// line break information directly (like JSON).
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_source_map::FileInformation;
    ///
    /// let info = FileInformation::from_parts(vec![6, 13], 20);
    /// ```
    pub fn from_parts(line_breaks: Vec<usize>, total_length: usize) -> Self {
        FileInformation {
            line_breaks,
            total_length,
        }
    }

    /// Convert a byte offset to a Location with row and column
    ///
    /// Uses binary search to find which line contains the offset.
    /// Runs in O(log n) time where n is the number of lines.
    ///
    /// The column is computed as character count (not byte count) from the start
    /// of the line to the offset, which requires the content parameter.
    ///
    /// Returns None if the offset is out of bounds.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_source_map::FileInformation;
    ///
    /// let content = "hello\nworld";
    /// let info = FileInformation::new(content);
    /// let loc = info.offset_to_location(6, content).unwrap();
    /// assert_eq!(loc.row, 1);
    /// assert_eq!(loc.column, 0);
    /// ```
    pub fn offset_to_location(&self, offset: usize, content: &str) -> Option<Location> {
        if offset > self.total_length {
            return None;
        }

        // Binary search to find which line the offset is on
        // line_breaks[i] is the position of the i-th newline (0-indexed)
        // So line 0 contains [0, line_breaks[0])
        // Line 1 contains [line_breaks[0]+1, line_breaks[1])
        // etc.

        let row = match self.line_breaks.binary_search(&offset) {
            // Offset is exactly at a newline character
            // That newline belongs to the line it terminates, not the next line
            Ok(idx) => idx,
            // Offset is between line breaks (or before the first, or after the last)
            Err(idx) => idx,
        };

        // Column is distance from the start of this line
        let line_start = if row == 0 {
            0
        } else {
            self.line_breaks[row - 1] + 1 // +1 to skip past the '\n'
        };

        // Count characters (not bytes) from line_start to offset
        // This ensures the column is a character count, not a byte count
        let column = content[line_start..offset].chars().count();

        Some(Location {
            offset,
            row,
            column,
        })
    }

    /// Get the total length of the file in bytes
    pub fn total_length(&self) -> usize {
        self.total_length
    }

    /// Get the line breaks array (byte offsets of newline characters)
    pub fn line_breaks(&self) -> &[usize] {
        &self.line_breaks
    }

    /// Get the number of lines in the file
    pub fn line_count(&self) -> usize {
        // If there are no newlines, there's 1 line
        // If there are n newlines, there are n+1 lines
        self.line_breaks.len() + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_file() {
        let content = "";
        let info = FileInformation::new(content);
        assert_eq!(info.total_length(), 0);
        assert_eq!(info.line_count(), 1);

        let loc = info.offset_to_location(0, content).unwrap();
        assert_eq!(loc.offset, 0);
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 0);
    }

    #[test]
    fn test_single_line() {
        let content = "hello world";
        let info = FileInformation::new(content);
        assert_eq!(info.total_length(), 11);
        assert_eq!(info.line_count(), 1);

        // Start of line
        let loc = info.offset_to_location(0, content).unwrap();
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 0);

        // Middle of line
        let loc = info.offset_to_location(6, content).unwrap();
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 6);

        // End of line
        let loc = info.offset_to_location(11, content).unwrap();
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 11);
    }

    #[test]
    fn test_multiple_lines() {
        let content = "line 1\nline 2\nline 3";
        let info = FileInformation::new(content);
        assert_eq!(info.line_count(), 3);

        // First line
        let loc = info.offset_to_location(0, content).unwrap();
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 0);

        // At first newline (offset 6 is '\n')
        let loc = info.offset_to_location(6, content).unwrap();
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 6);

        // Start of second line (offset 7 is 'l' in "line 2")
        let loc = info.offset_to_location(7, content).unwrap();
        assert_eq!(loc.row, 1);
        assert_eq!(loc.column, 0);

        // At second newline (offset 13 is '\n')
        let loc = info.offset_to_location(13, content).unwrap();
        assert_eq!(loc.row, 1);
        assert_eq!(loc.column, 6);

        // Start of third line (offset 14 is 'l' in "line 3")
        let loc = info.offset_to_location(14, content).unwrap();
        assert_eq!(loc.row, 2);
        assert_eq!(loc.column, 0);

        // End of file
        let loc = info.offset_to_location(20, content).unwrap();
        assert_eq!(loc.row, 2);
        assert_eq!(loc.column, 6);
    }

    #[test]
    fn test_out_of_bounds() {
        let content = "hello";
        let info = FileInformation::new(content);
        assert!(info.offset_to_location(100, content).is_none());
    }

    #[test]
    fn test_unicode_content() {
        // "café" - 'é' is 2 bytes in UTF-8
        let content = "café\nwörld"; // 4 chars + 1 newline + 5 chars = but more bytes
        let info = FileInformation::new(content);

        // Verify we're working with byte offsets for positioning, but character counts for columns
        // "café" is 5 bytes: c(1) a(1) f(1) é(2)
        // newline is 1 byte
        // So second line starts at byte offset 6
        let loc = info.offset_to_location(6, content).unwrap();
        assert_eq!(loc.row, 1);
        assert_eq!(loc.column, 0);
    }

    #[test]
    fn test_file_ending_with_newline() {
        let content = "line 1\nline 2\n";
        let info = FileInformation::new(content);
        assert_eq!(info.line_count(), 3); // Empty third line

        // The final newline
        let loc = info.offset_to_location(13, content).unwrap();
        assert_eq!(loc.row, 1);
        assert_eq!(loc.column, 6);

        // After the final newline (empty line 3)
        let loc = info.offset_to_location(14, content).unwrap();
        assert_eq!(loc.row, 2);
        assert_eq!(loc.column, 0);
    }

    #[test]
    fn test_consecutive_newlines() {
        let content = "a\n\n\nb";
        let info = FileInformation::new(content);
        assert_eq!(info.line_count(), 4);

        // First line
        let loc = info.offset_to_location(0, content).unwrap();
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 0);

        // First newline (offset 1)
        let loc = info.offset_to_location(1, content).unwrap();
        assert_eq!(loc.row, 0);
        assert_eq!(loc.column, 1);

        // Empty second line (offset 2)
        let loc = info.offset_to_location(2, content).unwrap();
        assert_eq!(loc.row, 1);
        assert_eq!(loc.column, 0);

        // Empty third line (offset 3)
        let loc = info.offset_to_location(3, content).unwrap();
        assert_eq!(loc.row, 2);
        assert_eq!(loc.column, 0);

        // Fourth line 'b' (offset 4)
        let loc = info.offset_to_location(4, content).unwrap();
        assert_eq!(loc.row, 3);
        assert_eq!(loc.column, 0);
    }

    #[test]
    fn test_multibyte_utf8_column_should_be_character_count() {
        // This test verifies that column is character count, not byte offset
        // Swedish text with multi-byte UTF-8 characters (å = 2 bytes, ä = 2 bytes, ö = 2 bytes)
        let content = "Gällande frågorna om något";
        // Character positions: G=0, ä=1, l=2, l=3, a=4, n=5, d=6, e=7, space=8, f=9, r=10, å=11, g=12, ...
        // Byte positions:      G=0, ä=1-2, l=3, l=4, a=5, n=6, d=7, e=8, space=9, f=10, r=11, å=12-13, g=14, ...

        let info = FileInformation::new(content);

        // Test position at "å" in "frågorna" (character 11, byte offset starts at 12)
        // The byte offset 12 is where "å" starts (it's 2 bytes: 12-13)
        let loc = info.offset_to_location(12, content).unwrap();
        assert_eq!(loc.row, 0);
        // With the fix, this should return 11 (character count), not 12 (byte offset)
        assert_eq!(
            loc.column, 11,
            "Column should be character count (11), not byte offset (12)"
        );

        // Test position at "g" after "å" in "frågorna" (character 12, byte offset 14)
        let loc = info.offset_to_location(14, content).unwrap();
        assert_eq!(loc.row, 0);
        // Should return 12 (character count), not 14 (byte offset)
        assert_eq!(
            loc.column, 12,
            "Column should be character count (12), not byte offset (14)"
        );
    }
}
