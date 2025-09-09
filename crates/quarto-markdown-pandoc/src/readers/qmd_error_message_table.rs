/*
 * qmd_error_message_table.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::utils::tree_sitter_log_observer::ProcessMessage;
use error_message_macros::include_error_table;

#[derive(Debug)]
pub struct ErrorCapture {
    pub column: usize,
    pub lr_state: usize,
    pub row: usize,
    pub size: usize,
    pub sym: &'static str,
    pub label: &'static str,
}

#[derive(Debug)]
pub struct ErrorNote {
    pub message: &'static str,
    pub label: Option<&'static str>,
    pub note_type: Option<&'static str>,
}

#[derive(Debug)]
pub struct ErrorInfo {
    pub title: &'static str,
    pub message: &'static str,
    pub captures: &'static [ErrorCapture],
    pub notes: &'static [ErrorNote],
}

#[derive(Debug)]
pub struct ErrorTableEntry {
    pub state: usize,
    pub sym: &'static str,
    pub row: usize,
    pub column: usize,
    pub error_info: ErrorInfo,
    pub name: &'static str,
}

pub fn get_error_table() -> &'static [ErrorTableEntry] {
    include_error_table!("./resources/error-corpus/_autogen-table.json")
}

pub fn lookup_error_message(process_message: &ProcessMessage) -> Option<&'static str> {
    let table = get_error_table();

    for entry in table {
        if entry.state == process_message.state && entry.sym == process_message.sym {
            return Some(entry.error_info.message);
        }
    }

    None
}

pub fn lookup_error_entry(process_message: &ProcessMessage) -> Option<&'static ErrorTableEntry> {
    let table = get_error_table();

    for entry in table {
        if entry.state == process_message.state && entry.sym == process_message.sym {
            return Some(entry);
        }
    }

    None
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
