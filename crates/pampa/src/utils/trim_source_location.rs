/*
 * trim_source_location.rs
 * Copyright (c) 2025 Posit, PBC
 */

use quarto_source_map::SourceInfo;

/// Trim leading and/or trailing whitespace from a SourceInfo
///
/// This adjusts the start and end offsets of a SourceInfo to exclude leading
/// and trailing whitespace characters.
///
/// # Arguments
/// * `source_info` - The SourceInfo to trim
/// * `input_text` - The full input text that the SourceInfo refers to
/// * `trim_leading` - Whether to trim leading whitespace
/// * `trim_trailing` - Whether to trim trailing whitespace
///
/// # Returns
/// A new SourceInfo with trimmed offsets
pub fn trim_whitespace(
    source_info: &SourceInfo,
    input_text: &str,
    trim_leading: bool,
    trim_trailing: bool,
) -> SourceInfo {
    let start = source_info.start_offset();
    let end = source_info.end_offset();

    // Extract the text slice for this source info
    let text = match input_text.get(start..end) {
        Some(t) => t,
        None => return source_info.clone(), // Can't trim if range is invalid
    };

    if text.is_empty() {
        return source_info.clone();
    }

    let mut trimmed_start = 0;
    let mut trimmed_end = text.len();

    // Trim leading whitespace
    if trim_leading {
        for ch in text.chars() {
            if ch.is_whitespace() {
                trimmed_start += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    // Trim trailing whitespace
    if trim_trailing {
        while trimmed_end > trimmed_start {
            // Get the slice up to trimmed_end
            if let Some(slice) = text.get(..trimmed_end) {
                if let Some(last_ch) = slice.chars().last() {
                    if last_ch.is_whitespace() {
                        trimmed_end -= last_ch.len_utf8();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    // If we trimmed everything, return a zero-length range at the start
    if trimmed_start >= trimmed_end {
        return match source_info {
            SourceInfo::Original {
                file_id,
                start_offset: orig_start,
                ..
            } => SourceInfo::Original {
                file_id: *file_id,
                start_offset: *orig_start,
                end_offset: *orig_start,
            },
            SourceInfo::Substring {
                parent,
                start_offset: sub_start,
                ..
            } => SourceInfo::Substring {
                parent: parent.clone(),
                start_offset: *sub_start,
                end_offset: *sub_start,
            },
            SourceInfo::Concat { .. } => {
                // For concat, just return as-is for now (edge case)
                source_info.clone()
            }
            SourceInfo::FilterProvenance { .. } => {
                // For filter provenance, just return as-is
                source_info.clone()
            }
        };
    }

    // Create new SourceInfo with adjusted offsets
    match source_info {
        SourceInfo::Original {
            file_id,
            start_offset: orig_start,
            ..
        } => SourceInfo::Original {
            file_id: *file_id,
            start_offset: orig_start + trimmed_start,
            end_offset: orig_start + trimmed_end,
        },
        SourceInfo::Substring {
            parent,
            start_offset: sub_start,
            ..
        } => SourceInfo::Substring {
            parent: parent.clone(),
            start_offset: sub_start + trimmed_start,
            end_offset: sub_start + trimmed_end,
        },
        SourceInfo::Concat { .. } => {
            // For concat, just return as-is for now (edge case)
            // Proper handling would require splitting/adjusting pieces
            source_info.clone()
        }
        SourceInfo::FilterProvenance { .. } => {
            // For filter provenance, just return as-is
            source_info.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::FileId;

    #[test]
    fn test_trim_leading_whitespace() {
        let input = "   hello";
        let file_id = FileId(0);

        let source_info = SourceInfo::Original {
            file_id,
            start_offset: 0,
            end_offset: 8,
        };

        let trimmed = trim_whitespace(&source_info, input, true, false);

        // Should skip 3 leading spaces
        assert_eq!(trimmed.start_offset(), 3);
        assert_eq!(trimmed.end_offset(), 8);
    }

    #[test]
    fn test_trim_trailing_whitespace() {
        let input = "hello   ";
        let file_id = FileId(0);

        let source_info = SourceInfo::Original {
            file_id,
            start_offset: 0,
            end_offset: 8,
        };

        let trimmed = trim_whitespace(&source_info, input, false, true);

        // Should skip 3 trailing spaces
        assert_eq!(trimmed.start_offset(), 0);
        assert_eq!(trimmed.end_offset(), 5);
    }

    #[test]
    fn test_trim_both_whitespace() {
        let input = "  hello  ";
        let file_id = FileId(0);

        let source_info = SourceInfo::Original {
            file_id,
            start_offset: 0,
            end_offset: 9,
        };

        let trimmed = trim_whitespace(&source_info, input, true, true);

        assert_eq!(trimmed.start_offset(), 2);
        assert_eq!(trimmed.end_offset(), 7);
    }

    #[test]
    fn test_trim_all_whitespace() {
        let input = "   ";
        let file_id = FileId(0);

        let source_info = SourceInfo::Original {
            file_id,
            start_offset: 0,
            end_offset: 3,
        };

        let trimmed = trim_whitespace(&source_info, input, true, true);

        // Should return zero-length range at start
        assert_eq!(trimmed.start_offset(), 0);
        assert_eq!(trimmed.end_offset(), 0);
    }

    #[test]
    fn test_trim_no_whitespace() {
        let input = "hello";
        let file_id = FileId(0);

        let source_info = SourceInfo::Original {
            file_id,
            start_offset: 0,
            end_offset: 5,
        };

        let trimmed = trim_whitespace(&source_info, input, true, true);

        // Should remain unchanged
        assert_eq!(trimmed.start_offset(), 0);
        assert_eq!(trimmed.end_offset(), 5);
    }

    #[test]
    fn test_trim_substring() {
        let input = "prefix  hello  suffix";
        let file_id = FileId(0);

        let parent = SourceInfo::Original {
            file_id,
            start_offset: 0,
            end_offset: 21,
        };

        // Substring covering "  hello  " (offsets 6-15 in parent)
        let substring = SourceInfo::substring(parent, 6, 15);

        let trimmed = trim_whitespace(&substring, input, true, true);

        // Should trim to just "hello" (offsets 8-13 in parent)
        assert_eq!(trimmed.start_offset(), 8);
        assert_eq!(trimmed.end_offset(), 13);
    }

    #[test]
    fn test_trim_utf8_multibyte() {
        let input = "  café  ";
        let file_id = FileId(0);

        let source_info = SourceInfo::Original {
            file_id,
            start_offset: 0,
            end_offset: 9, // "  café  " is 9 bytes (é is 2 bytes)
        };

        let trimmed = trim_whitespace(&source_info, input, true, true);

        // Should trim to just "café"
        assert_eq!(trimmed.start_offset(), 2);
        assert_eq!(trimmed.end_offset(), 7); // 2 + 5 bytes for "café"
    }

    #[test]
    fn test_trim_partial_range() {
        let input = "But HTML elements are <b>discouraged</b>.";
        let file_id = FileId(0);

        // Source info covering " <b>" (with leading space)
        let source_info = SourceInfo::Original {
            file_id,
            start_offset: 21, // Position of the space before <b>
            end_offset: 25,   // Position after >
        };

        let trimmed = trim_whitespace(&source_info, input, true, true);

        // Should trim to just "<b>" without the leading space
        assert_eq!(trimmed.start_offset(), 22);
        assert_eq!(trimmed.end_offset(), 25);
    }
}
