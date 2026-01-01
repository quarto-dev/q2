/*
 * text.rs
 * Copyright (c) 2025 Posit, PBC
 */

pub fn build_row_column_index(input: &str) -> Vec<usize> {
    let mut index = vec![0]; // The first line starts at byte offset 0
    for (i, c) in input.char_indices() {
        if c == '\n' {
            index.push(i + 1); // The next line starts after the newline character
        }
    }
    index
}

pub fn byte_offset_to_row_column(index: &Vec<usize>, byte_offset: usize) -> (usize, usize) {
    let row = match index.binary_search(&byte_offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    (row, byte_offset - index[row])
}

pub fn row_column_to_byte_offset(index: &Vec<usize>, row: usize, column: usize) -> Option<usize> {
    if row < index.len() {
        Some(index[row] + column)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // build_row_column_index tests
    // ========================================================================

    #[test]
    fn test_build_index_empty_string() {
        let index = build_row_column_index("");
        assert_eq!(index, vec![0]);
    }

    #[test]
    fn test_build_index_single_line_no_newline() {
        let index = build_row_column_index("hello");
        assert_eq!(index, vec![0]);
    }

    #[test]
    fn test_build_index_single_line_with_newline() {
        let index = build_row_column_index("hello\n");
        assert_eq!(index, vec![0, 6]);
    }

    #[test]
    fn test_build_index_multiple_lines() {
        let index = build_row_column_index("hello\nworld\n");
        assert_eq!(index, vec![0, 6, 12]);
    }

    #[test]
    fn test_build_index_empty_lines() {
        let index = build_row_column_index("\n\n\n");
        assert_eq!(index, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_build_index_unicode() {
        // "héllo" has 6 bytes (é is 2 bytes in UTF-8)
        let index = build_row_column_index("héllo\nworld");
        assert_eq!(index, vec![0, 7]); // 6 bytes for "héllo" + 1 for newline
    }

    // ========================================================================
    // byte_offset_to_row_column tests
    // ========================================================================

    #[test]
    fn test_offset_to_row_col_start_of_first_line() {
        let index = build_row_column_index("hello\nworld\n");
        assert_eq!(byte_offset_to_row_column(&index, 0), (0, 0));
    }

    #[test]
    fn test_offset_to_row_col_middle_of_first_line() {
        let index = build_row_column_index("hello\nworld\n");
        assert_eq!(byte_offset_to_row_column(&index, 3), (0, 3));
    }

    #[test]
    fn test_offset_to_row_col_start_of_second_line() {
        let index = build_row_column_index("hello\nworld\n");
        assert_eq!(byte_offset_to_row_column(&index, 6), (1, 0));
    }

    #[test]
    fn test_offset_to_row_col_middle_of_second_line() {
        let index = build_row_column_index("hello\nworld\n");
        assert_eq!(byte_offset_to_row_column(&index, 8), (1, 2));
    }

    #[test]
    fn test_offset_to_row_col_at_newline() {
        let index = build_row_column_index("hello\nworld\n");
        // Offset 5 is the newline character at end of "hello"
        assert_eq!(byte_offset_to_row_column(&index, 5), (0, 5));
    }

    #[test]
    fn test_offset_to_row_col_single_line() {
        let index = build_row_column_index("hello");
        assert_eq!(byte_offset_to_row_column(&index, 3), (0, 3));
    }

    // ========================================================================
    // row_column_to_byte_offset tests
    // ========================================================================

    #[test]
    fn test_row_col_to_offset_start() {
        let index = build_row_column_index("hello\nworld\n");
        assert_eq!(row_column_to_byte_offset(&index, 0, 0), Some(0));
    }

    #[test]
    fn test_row_col_to_offset_middle_first_line() {
        let index = build_row_column_index("hello\nworld\n");
        assert_eq!(row_column_to_byte_offset(&index, 0, 3), Some(3));
    }

    #[test]
    fn test_row_col_to_offset_start_second_line() {
        let index = build_row_column_index("hello\nworld\n");
        assert_eq!(row_column_to_byte_offset(&index, 1, 0), Some(6));
    }

    #[test]
    fn test_row_col_to_offset_middle_second_line() {
        let index = build_row_column_index("hello\nworld\n");
        assert_eq!(row_column_to_byte_offset(&index, 1, 2), Some(8));
    }

    #[test]
    fn test_row_col_to_offset_invalid_row() {
        let index = build_row_column_index("hello\nworld\n");
        assert_eq!(row_column_to_byte_offset(&index, 10, 0), None);
    }

    #[test]
    fn test_row_col_to_offset_empty_string() {
        let index = build_row_column_index("");
        assert_eq!(row_column_to_byte_offset(&index, 0, 0), Some(0));
        assert_eq!(row_column_to_byte_offset(&index, 1, 0), None);
    }

    // ========================================================================
    // Roundtrip tests
    // ========================================================================

    #[test]
    fn test_roundtrip_offset_to_row_col_and_back() {
        let input = "hello\nworld\nfoo\nbar";
        let index = build_row_column_index(input);

        for offset in 0..input.len() {
            let (row, col) = byte_offset_to_row_column(&index, offset);
            let back = row_column_to_byte_offset(&index, row, col);
            assert_eq!(back, Some(offset), "Roundtrip failed for offset {}", offset);
        }
    }
}
