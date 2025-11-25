/*
 * qmd_error_message_table.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! QMD-specific error table module.
//!
//! This module provides the error table for the QMD parser by:
//! 1. Re-exporting types from quarto-parse-errors
//! 2. Providing get_error_table() that embeds the QMD error corpus
//! 3. Providing convenience lookup functions

// Re-export types from quarto-parse-errors
pub use quarto_parse_errors::{ErrorTableEntry, ProcessMessage};

use quarto_error_message_macros::include_error_table;

/// Get the error table for the QMD parser.
///
/// This embeds the error corpus at compile time.
pub fn get_error_table() -> &'static [ErrorTableEntry] {
    include_error_table!(
        "./resources/error-corpus/_autogen-table.json",
        "quarto_parse_errors"
    )
}

/// Look up an error message by parser state and symbol.
///
/// Convenience wrapper that uses the QMD error table.
pub fn lookup_error_message(process_message: &ProcessMessage) -> Option<&'static str> {
    quarto_parse_errors::lookup_error_message(get_error_table(), process_message)
}

/// Look up error table entries by parser state and symbol.
///
/// Convenience wrapper that uses the QMD error table.
pub fn lookup_error_entry(process_message: &ProcessMessage) -> Vec<&'static ErrorTableEntry> {
    quarto_parse_errors::lookup_error_entry(get_error_table(), process_message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_table_loading() {
        let table = get_error_table();
        assert!(table.len() > 0, "Error table should not be empty");
    }
}
