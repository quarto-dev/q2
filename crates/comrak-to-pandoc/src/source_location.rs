/*
 * source_location.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Convert comrak's Sourcepos to quarto-source-map's SourceInfo.
 *
 * Key findings about comrak's Sourcepos:
 * - Line and column are 1-based
 * - Columns are byte-based (not character-based)
 * - End position is inclusive (points to last byte of content)
 *
 * quarto-source-map's SourceInfo:
 * - Uses byte offsets
 * - End offset is exclusive (one past the last byte)
 */

use comrak::nodes::{LineColumn, Sourcepos};
use quarto_source_map::{FileId, SourceInfo};

/// Context for converting comrak Sourcepos to quarto-source-map SourceInfo
pub struct SourceLocationContext {
    /// Precomputed line start offsets (byte offset of each line start)
    line_offsets: Vec<usize>,
    /// File ID for the source file
    file_id: FileId,
}

impl SourceLocationContext {
    /// Create a new context from source text
    pub fn new(source: &str, file_id: FileId) -> Self {
        // Precompute line start offsets
        // Line 1 starts at offset 0
        let mut line_offsets = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_offsets.push(i + 1);
            }
        }
        Self {
            line_offsets,
            file_id,
        }
    }

    /// Convert a comrak Sourcepos to a quarto-source-map SourceInfo
    ///
    /// Note: comrak's end position is inclusive, but SourceInfo's end_offset is exclusive.
    pub fn sourcepos_to_source_info(&self, sourcepos: &Sourcepos) -> SourceInfo {
        let start_offset = self.start_offset(sourcepos);
        let end_offset = self.end_offset(sourcepos);
        SourceInfo::original(self.file_id, start_offset, end_offset)
    }

    /// Get the start byte offset for a sourcepos
    pub fn start_offset(&self, sourcepos: &Sourcepos) -> usize {
        self.line_column_to_offset(&sourcepos.start)
    }

    /// Get the end byte offset for a sourcepos (exclusive)
    ///
    /// Note: comrak's end is inclusive, so we add 1 to make it exclusive
    pub fn end_offset(&self, sourcepos: &Sourcepos) -> usize {
        // comrak end is inclusive (points to last byte)
        // SourceInfo end is exclusive (one past last byte)
        self.line_column_to_offset(&sourcepos.end) + 1
    }

    /// Convert 1-based (line, column) to byte offset
    ///
    /// Since comrak columns are byte-based (verified by testing),
    /// we just need to find the line start and add the column offset.
    fn line_column_to_offset(&self, lc: &LineColumn) -> usize {
        // comrak uses 1-based line numbers
        let line_idx = lc.line.saturating_sub(1);
        let line_start = self.line_offsets.get(line_idx).copied().unwrap_or(0);
        // Column is also 1-based; convert to 0-based
        line_start + lc.column.saturating_sub(1)
    }

    /// Get the file ID
    pub fn file_id(&self) -> FileId {
        self.file_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_column_to_offset_single_line() {
        let source = "hello world\n";
        let ctx = SourceLocationContext::new(source, FileId(0));

        // Line 1, column 1 = offset 0 ('h')
        assert_eq!(
            ctx.line_column_to_offset(&LineColumn { line: 1, column: 1 }),
            0
        );
        // Line 1, column 5 = offset 4 ('o' in "hello")
        assert_eq!(
            ctx.line_column_to_offset(&LineColumn { line: 1, column: 5 }),
            4
        );
        // Line 1, column 7 = offset 6 ('w' in "world")
        assert_eq!(
            ctx.line_column_to_offset(&LineColumn { line: 1, column: 7 }),
            6
        );
    }

    #[test]
    fn test_line_column_to_offset_multi_line() {
        let source = "line1\nline2\nline3\n";
        let ctx = SourceLocationContext::new(source, FileId(0));

        // Line 1, column 1 = offset 0
        assert_eq!(
            ctx.line_column_to_offset(&LineColumn { line: 1, column: 1 }),
            0
        );
        // Line 1, column 5 = offset 4 ('1' in "line1")
        assert_eq!(
            ctx.line_column_to_offset(&LineColumn { line: 1, column: 5 }),
            4
        );
        // Line 2, column 1 = offset 6 ('l' in "line2")
        assert_eq!(
            ctx.line_column_to_offset(&LineColumn { line: 2, column: 1 }),
            6
        );
        // Line 3, column 1 = offset 12 ('l' in "line3")
        assert_eq!(
            ctx.line_column_to_offset(&LineColumn { line: 3, column: 1 }),
            12
        );
    }

    #[test]
    fn test_sourcepos_to_source_info() {
        let source = "hello world\n";
        let ctx = SourceLocationContext::new(source, FileId(0));

        // comrak's end is inclusive: column 11 points to 'd' in "world"
        // SourceInfo's end is exclusive: should be 11 (one past 'd')
        let sourcepos = Sourcepos {
            start: LineColumn { line: 1, column: 1 },
            end: LineColumn {
                line: 1,
                column: 11,
            },
        };

        let info = ctx.sourcepos_to_source_info(&sourcepos);
        assert_eq!(info.start_offset(), 0); // 'h' at offset 0
        assert_eq!(info.end_offset(), 11); // exclusive end, one past 'd'
    }

    #[test]
    fn test_sourcepos_utf8() {
        // "héllo" has 6 bytes (é is 2 bytes)
        let source = "héllo\n";
        let ctx = SourceLocationContext::new(source, FileId(0));

        // comrak reports byte-based columns, so end column = 6 (pointing to 'o')
        let sourcepos = Sourcepos {
            start: LineColumn { line: 1, column: 1 },
            end: LineColumn { line: 1, column: 6 },
        };

        let info = ctx.sourcepos_to_source_info(&sourcepos);
        assert_eq!(info.start_offset(), 0);
        assert_eq!(info.end_offset(), 6); // exclusive end
    }

    #[test]
    fn test_start_and_end_offset() {
        let source = "hello\n";
        let ctx = SourceLocationContext::new(source, FileId(0));

        let sourcepos = Sourcepos {
            start: LineColumn { line: 1, column: 1 },
            end: LineColumn { line: 1, column: 5 },
        };

        assert_eq!(ctx.start_offset(&sourcepos), 0);
        assert_eq!(ctx.end_offset(&sourcepos), 5); // exclusive
    }
}
