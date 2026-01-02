/*
 * error_table.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Error table types for mapping parser states to diagnostic messages.
//!
//! This module defines the data structures used to represent error messages
//! and their associated metadata. The error table is populated at compile-time
//! using the `include_error_table!` macro.

use crate::tree_sitter_log::ProcessMessage;

/// A capture identifies a specific token in the error context.
///
/// Captures are used to highlight relevant tokens in error messages,
/// linking them to specific notes and explanations.
#[derive(Debug)]
pub struct ErrorCapture {
    pub column: usize,
    pub lr_state: usize, // LR parser state when this token was consumed
    pub row: usize,
    pub size: usize,         // Size of the token in characters
    pub sym: &'static str,   // Token symbol from the parser
    pub label: &'static str, // Label used to reference this capture in notes
}

/// Additional contextual information attached to an error message.
#[derive(Debug)]
pub struct ErrorNote {
    pub message: &'static str,
    pub label: Option<&'static str>, // References an ErrorCapture by label
    pub note_type: &'static str,     // Type of note (e.g., "simple", "label-range")
    pub label_begin: Option<&'static str>,
    pub label_end: Option<&'static str>,
    pub trim_leading_space: Option<bool>,
    pub trim_trailing_space: Option<bool>,
}

/// Complete information for generating a diagnostic message.
#[derive(Debug)]
pub struct ErrorInfo {
    pub code: Option<&'static str>,        // Error code (e.g., "Q-2-1")
    pub title: &'static str,               // Short error title
    pub message: &'static str,             // Main error message
    pub captures: &'static [ErrorCapture], // Tokens to highlight
    pub notes: &'static [ErrorNote],       // Additional context
    pub hints: &'static [&'static str],    // Suggestions for fixing
}

/// Entry in the error table mapping a parser state to diagnostic information.
///
/// The combination of `(state, sym)` uniquely identifies the parser configuration
/// that triggered this error.
#[derive(Debug)]
pub struct ErrorTableEntry {
    pub state: usize,      // LR parser state
    pub sym: &'static str, // Lookahead symbol
    pub row: usize,        // Row in test case (for debugging)
    pub column: usize,     // Column in test case (for debugging)
    pub error_info: ErrorInfo,
    pub name: &'static str, // Test case name (for debugging)
}

/// Look up an error message by parser state and symbol.
///
/// Returns the error message string if found, or None if this (state, sym)
/// combination is not in the error table.
pub fn lookup_error_message(
    table: &[ErrorTableEntry],
    process_message: &ProcessMessage,
) -> Option<&'static str> {
    for entry in table {
        if entry.state == process_message.state && entry.sym == process_message.sym {
            return Some(entry.error_info.message);
        }
    }
    None
}

/// Look up error table entries by parser state and symbol.
///
/// Returns all matching entries. Multiple entries for the same (state, sym)
/// can exist when the same parser state should produce different error messages
/// in different contexts.
pub fn lookup_error_entry<'a>(
    table: &'a [ErrorTableEntry],
    process_message: &ProcessMessage,
) -> Vec<&'a ErrorTableEntry> {
    table
        .iter()
        .filter(|entry| entry.state == process_message.state && entry.sym == process_message.sym)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_entry(state: usize, sym: &'static str, message: &'static str) -> ErrorTableEntry {
        ErrorTableEntry {
            state,
            sym,
            row: 0,
            column: 0,
            error_info: ErrorInfo {
                code: Some("TEST-1"),
                title: "Test Error",
                message,
                captures: &[],
                notes: &[],
                hints: &[],
            },
            name: "test_case",
        }
    }

    fn make_process_message(state: usize, sym: &str) -> ProcessMessage {
        ProcessMessage {
            version: 1,
            state,
            row: 0,
            column: 0,
            sym: sym.to_string(),
            size: 1,
        }
    }

    // === ErrorCapture tests ===

    #[test]
    fn test_error_capture_debug() {
        let capture = ErrorCapture {
            column: 5,
            lr_state: 42,
            row: 1,
            size: 3,
            sym: "IDENTIFIER",
            label: "var_name",
        };
        let debug = format!("{:?}", capture);
        assert!(debug.contains("ErrorCapture"));
        assert!(debug.contains("IDENTIFIER"));
        assert!(debug.contains("var_name"));
    }

    // === ErrorNote tests ===

    #[test]
    fn test_error_note_debug() {
        let note = ErrorNote {
            message: "Did you mean to use a different keyword?",
            label: Some("keyword"),
            note_type: "simple",
            label_begin: None,
            label_end: None,
            trim_leading_space: None,
            trim_trailing_space: None,
        };
        let debug = format!("{:?}", note);
        assert!(debug.contains("ErrorNote"));
        assert!(debug.contains("Did you mean"));
    }

    #[test]
    fn test_error_note_with_range() {
        let note = ErrorNote {
            message: "This range is problematic",
            label: None,
            note_type: "label-range",
            label_begin: Some("start"),
            label_end: Some("end"),
            trim_leading_space: Some(true),
            trim_trailing_space: Some(false),
        };
        assert_eq!(note.label_begin, Some("start"));
        assert_eq!(note.label_end, Some("end"));
        assert_eq!(note.trim_leading_space, Some(true));
    }

    // === ErrorInfo tests ===

    #[test]
    fn test_error_info_debug() {
        let info = ErrorInfo {
            code: Some("Q-1-1"),
            title: "Syntax Error",
            message: "Unexpected token",
            captures: &[],
            notes: &[],
            hints: &["Try removing the extra character"],
        };
        let debug = format!("{:?}", info);
        assert!(debug.contains("ErrorInfo"));
        assert!(debug.contains("Q-1-1"));
        assert!(debug.contains("Syntax Error"));
    }

    #[test]
    fn test_error_info_with_captures() {
        static CAPTURES: [ErrorCapture; 1] = [ErrorCapture {
            column: 0,
            lr_state: 1,
            row: 0,
            size: 5,
            sym: "TOKEN",
            label: "tok",
        }];

        let info = ErrorInfo {
            code: None,
            title: "Error",
            message: "Error message",
            captures: &CAPTURES,
            notes: &[],
            hints: &[],
        };
        assert_eq!(info.captures.len(), 1);
        assert_eq!(info.captures[0].label, "tok");
    }

    // === ErrorTableEntry tests ===

    #[test]
    fn test_error_table_entry_debug() {
        let entry = make_test_entry(42, "EOF", "Unexpected end of file");
        let debug = format!("{:?}", entry);
        assert!(debug.contains("ErrorTableEntry"));
        assert!(debug.contains("42"));
        assert!(debug.contains("EOF"));
    }

    // === lookup_error_message tests ===

    #[test]
    fn test_lookup_error_message_found() {
        let table = [
            make_test_entry(1, "NEWLINE", "Unexpected newline"),
            make_test_entry(2, "EOF", "Unexpected end of file"),
            make_test_entry(3, "IDENTIFIER", "Expected identifier"),
        ];

        let msg = make_process_message(2, "EOF");
        let result = lookup_error_message(&table, &msg);

        assert_eq!(result, Some("Unexpected end of file"));
    }

    #[test]
    fn test_lookup_error_message_not_found() {
        let table = [
            make_test_entry(1, "NEWLINE", "Unexpected newline"),
            make_test_entry(2, "EOF", "Unexpected end of file"),
        ];

        let msg = make_process_message(99, "UNKNOWN");
        let result = lookup_error_message(&table, &msg);

        assert_eq!(result, None);
    }

    #[test]
    fn test_lookup_error_message_empty_table() {
        let table: [ErrorTableEntry; 0] = [];
        let msg = make_process_message(1, "EOF");
        let result = lookup_error_message(&table, &msg);

        assert_eq!(result, None);
    }

    #[test]
    fn test_lookup_error_message_state_mismatch() {
        let table = [make_test_entry(1, "EOF", "Error 1")];

        // Same symbol, different state
        let msg = make_process_message(2, "EOF");
        let result = lookup_error_message(&table, &msg);

        assert_eq!(result, None);
    }

    #[test]
    fn test_lookup_error_message_sym_mismatch() {
        let table = [make_test_entry(1, "EOF", "Error 1")];

        // Same state, different symbol
        let msg = make_process_message(1, "NEWLINE");
        let result = lookup_error_message(&table, &msg);

        assert_eq!(result, None);
    }

    // === lookup_error_entry tests ===

    #[test]
    fn test_lookup_error_entry_found() {
        let table = [
            make_test_entry(1, "NEWLINE", "Error 1"),
            make_test_entry(2, "EOF", "Error 2"),
        ];

        let msg = make_process_message(1, "NEWLINE");
        let result = lookup_error_entry(&table, &msg);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].error_info.message, "Error 1");
    }

    #[test]
    fn test_lookup_error_entry_multiple_matches() {
        let table = [
            make_test_entry(1, "EOF", "First EOF error"),
            make_test_entry(1, "EOF", "Second EOF error"),
            make_test_entry(2, "EOF", "Different state"),
        ];

        let msg = make_process_message(1, "EOF");
        let result = lookup_error_entry(&table, &msg);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].error_info.message, "First EOF error");
        assert_eq!(result[1].error_info.message, "Second EOF error");
    }

    #[test]
    fn test_lookup_error_entry_not_found() {
        let table = [make_test_entry(1, "EOF", "Error 1")];

        let msg = make_process_message(99, "UNKNOWN");
        let result = lookup_error_entry(&table, &msg);

        assert!(result.is_empty());
    }

    #[test]
    fn test_lookup_error_entry_empty_table() {
        let table: [ErrorTableEntry; 0] = [];
        let msg = make_process_message(1, "EOF");
        let result = lookup_error_entry(&table, &msg);

        assert!(result.is_empty());
    }
}
