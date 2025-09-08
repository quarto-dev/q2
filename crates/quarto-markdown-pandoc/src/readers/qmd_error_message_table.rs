/*
 * qmd_error_message_table.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::utils::tree_sitter_log_observer::ProcessMessage;
use error_message_macros::include_error_table;

#[derive(Debug)]
pub struct ErrorTableEntry {
    pub state: usize,
    pub sym: &'static str,
    pub row: usize,
    pub column: usize,
    pub error_msg: &'static str,
}

pub fn get_error_table() -> &'static [ErrorTableEntry] {
    include_error_table!("./resources/error-corpus/_autogen-table.json")
}

pub fn lookup_error_message(process_message: &ProcessMessage) -> Option<&'static str> {
    let table = get_error_table();
    
    for entry in table {
        if entry.state == process_message.state && entry.sym == process_message.sym {
            return Some(entry.error_msg);
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
    
    #[test]
    fn test_lookup_existing_error() {
        let test_message = ProcessMessage {
            version: 0,
            state: 933,
            row: 0,
            column: 14,
            sym: "end".to_string(),
            size: 0,
        };
        
        let error_msg = lookup_error_message(&test_message);
        assert!(error_msg.is_some());
        assert_eq!(
            error_msg.unwrap(),
            "Reached end of file before finding closing ']' for span."
        );
    }
    
    #[test]
    fn test_lookup_non_existing_error() {
        let test_message = ProcessMessage {
            version: 0,
            state: 99999,
            row: 0,
            column: 0,
            sym: "nonexistent".to_string(),
            size: 0,
        };
        
        let error_msg = lookup_error_message(&test_message);
        assert!(error_msg.is_none());
    }
}