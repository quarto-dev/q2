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
    pub lr_state: usize,  // LR parser state when this token was consumed
    pub row: usize,
    pub size: usize,      // Size of the token in characters
    pub sym: &'static str, // Token symbol from the parser
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
    pub code: Option<&'static str>,  // Error code (e.g., "Q-2-1")
    pub title: &'static str,          // Short error title
    pub message: &'static str,        // Main error message
    pub captures: &'static [ErrorCapture], // Tokens to highlight
    pub notes: &'static [ErrorNote],  // Additional context
    pub hints: &'static [&'static str], // Suggestions for fixing
}

/// Entry in the error table mapping a parser state to diagnostic information.
///
/// The combination of `(state, sym)` uniquely identifies the parser configuration
/// that triggered this error.
#[derive(Debug)]
pub struct ErrorTableEntry {
    pub state: usize,         // LR parser state
    pub sym: &'static str,    // Lookahead symbol
    pub row: usize,           // Row in test case (for debugging)
    pub column: usize,        // Column in test case (for debugging)
    pub error_info: ErrorInfo,
    pub name: &'static str,   // Test case name (for debugging)
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
