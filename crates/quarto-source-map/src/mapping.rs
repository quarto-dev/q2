//! Position mapping through transformation chains

use crate::types::{FileId, Location};
use crate::{SourceContext, SourceInfo};

/// Result of mapping a position back to an original file
#[derive(Debug, Clone, PartialEq)]
pub struct MappedLocation {
    /// The original file
    pub file_id: FileId,
    /// Location in the original file
    pub location: Location,
}

impl SourceInfo {
    /// Map an offset in the current text back to original source
    pub fn map_offset(&self, offset: usize, ctx: &SourceContext) -> Option<MappedLocation> {
        match self {
            SourceInfo::Original {
                file_id,
                start_offset,
                ..
            } => {
                // Direct mapping to original file
                let file = ctx.get_file(*file_id)?;
                let file_info = file.file_info.as_ref()?;

                // Compute the absolute offset in the file
                let absolute_offset = start_offset + offset;

                // Get file content: use stored content for ephemeral files, or read from disk
                let content = match &file.content {
                    Some(c) => c.clone(),
                    None => std::fs::read_to_string(&file.path).ok()?,
                };

                // Convert offset to Location with row/column using efficient binary search
                let location = file_info.offset_to_location(absolute_offset, &content)?;

                Some(MappedLocation {
                    file_id: *file_id,
                    location,
                })
            }
            SourceInfo::Substring {
                parent,
                start_offset,
                ..
            } => {
                // Map to parent coordinates and recurse
                let parent_offset = start_offset + offset;
                parent.map_offset(parent_offset, ctx)
            }
            SourceInfo::Concat { pieces } => {
                // Find which piece contains this offset
                for piece in pieces {
                    let piece_start = piece.offset_in_concat;
                    let piece_end = piece_start + piece.length;

                    if offset >= piece_start && offset < piece_end {
                        // Offset is within this piece
                        let offset_in_piece = offset - piece_start;
                        return piece.source_info.map_offset(offset_in_piece, ctx);
                    }
                }
                None // Offset not found in any piece
            }
        }
    }

    /// Map a range in the current text back to original source
    pub fn map_range(
        &self,
        start: usize,
        end: usize,
        ctx: &SourceContext,
    ) -> Option<(MappedLocation, MappedLocation)> {
        let start_mapped = self.map_offset(start, ctx)?;
        let end_mapped = self.map_offset(end, ctx)?;
        Some((start_mapped, end_mapped))
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{Location, Range};
    use crate::{SourceContext, SourceInfo};

    #[test]
    fn test_map_offset_original() {
        let mut ctx = SourceContext::new();
        let file_id = ctx.add_file("test.qmd".to_string(), Some("hello\nworld".to_string()));

        let info = SourceInfo::from_range(
            file_id,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 11,
                    row: 1,
                    column: 5,
                },
            },
        );

        // Test mapping offset 0 (start of first line)
        let mapped = info.map_offset(0, &ctx).unwrap();
        assert_eq!(mapped.file_id, file_id);
        assert_eq!(mapped.location.offset, 0);
        assert_eq!(mapped.location.row, 0);
        assert_eq!(mapped.location.column, 0);

        // Test mapping offset 6 (start of second line)
        let mapped = info.map_offset(6, &ctx).unwrap();
        assert_eq!(mapped.file_id, file_id);
        assert_eq!(mapped.location.offset, 6);
        assert_eq!(mapped.location.row, 1);
        assert_eq!(mapped.location.column, 0);
    }

    #[test]
    fn test_map_offset_substring() {
        let mut ctx = SourceContext::new();
        let file_id = ctx.add_file("test.qmd".to_string(), Some("0123456789".to_string()));

        let original = SourceInfo::from_range(
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

        // Extract substring from offset 3 to 7 ("3456")
        let substring = SourceInfo::substring(original, 3, 7);

        // Map offset 0 in substring (should be '3' at offset 3 in original)
        let mapped = substring.map_offset(0, &ctx).unwrap();
        assert_eq!(mapped.file_id, file_id);
        assert_eq!(mapped.location.offset, 3);

        // Map offset 2 in substring (should be '5' at offset 5 in original)
        let mapped = substring.map_offset(2, &ctx).unwrap();
        assert_eq!(mapped.file_id, file_id);
        assert_eq!(mapped.location.offset, 5);
    }

    #[test]
    fn test_map_offset_concat() {
        let mut ctx = SourceContext::new();
        let file_id1 = ctx.add_file("first.qmd".to_string(), Some("AAA".to_string()));
        let file_id2 = ctx.add_file("second.qmd".to_string(), Some("BBB".to_string()));

        let info1 = SourceInfo::from_range(
            file_id1,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 3,
                    row: 0,
                    column: 3,
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
                    offset: 3,
                    row: 0,
                    column: 3,
                },
            },
        );

        // Concatenate: "AAABBB"
        let concat = SourceInfo::concat(vec![(info1, 3), (info2, 3)]);

        // Map offset 1 (should be in first piece, second 'A')
        let mapped = concat.map_offset(1, &ctx).unwrap();
        assert_eq!(mapped.file_id, file_id1);
        assert_eq!(mapped.location.offset, 1);

        // Map offset 4 (should be in second piece, second 'B')
        let mapped = concat.map_offset(4, &ctx).unwrap();
        assert_eq!(mapped.file_id, file_id2);
        assert_eq!(mapped.location.offset, 1);
    }

    #[test]
    fn test_map_range() {
        let mut ctx = SourceContext::new();
        let file_id = ctx.add_file("test.qmd".to_string(), Some("hello\nworld".to_string()));

        let info = SourceInfo::from_range(
            file_id,
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 11,
                    row: 1,
                    column: 5,
                },
            },
        );

        // Map range [0, 5) which is "hello"
        let (start, end) = info.map_range(0, 5, &ctx).unwrap();
        assert_eq!(start.file_id, file_id);
        assert_eq!(start.location.offset, 0);
        assert_eq!(end.file_id, file_id);
        assert_eq!(end.location.offset, 5);
    }
}
