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
