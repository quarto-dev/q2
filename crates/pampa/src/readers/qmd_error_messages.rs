/*
 * qmd_error_messages.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! QMD-specific error message generation.
//!
//! This module provides QMD-specific wrappers around the generic
//! quarto-parse-errors functionality.

use std::collections::HashSet;

// Re-export generic functions from quarto-parse-errors
pub use quarto_parse_errors::{get_outer_error_nodes, prune_diagnostics_by_error_nodes};

// Import types we need
use quarto_parse_errors::TreeSitterLogObserver;

use crate::readers::qmd_error_message_table::get_error_table;

/// Produce structured DiagnosticMessage objects from parse errors.
///
/// This is a QMD-specific wrapper that provides the error table automatically.
pub fn produce_diagnostic_messages(
    input_bytes: &[u8],
    tree_sitter_log: &TreeSitterLogObserver,
    filename: &str,
    source_context: &quarto_source_map::SourceContext,
) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    quarto_parse_errors::produce_diagnostic_messages(
        input_bytes,
        tree_sitter_log,
        get_error_table(),
        filename,
        source_context,
    )
}

/// Produce error message JSON for corpus building.
///
/// This is used during the error table generation process to capture
/// parser states from error examples.
pub fn produce_error_message_json(tree_sitter_log: &TreeSitterLogObserver) -> Vec<String> {
    let mut seen_errors: HashSet<(String, usize)> = HashSet::new();

    for parse in &tree_sitter_log.parses {
        let process_log = &parse.processes[&0];
        if process_log.is_good() {
            continue;
        }
        let mut tokens: Vec<serde_json::Value> = vec![];
        let mut error_states: Vec<serde_json::Value> = vec![];
        for token in &parse.all_tokens {
            tokens.push(serde_json::json!({
                "row": token.row,
                "column": token.column,
                "size": token.size,
                "lrState": token.lr_state,
                "sym": token.sym,
            }));
        }
        for token in &parse.consumed_tokens {
            tokens.push(serde_json::json!({
                "row": token.row,
                "column": token.column,
                "size": token.size,
                "lrState": token.lr_state,
                "sym": token.sym,
            }));
        }
        for state in process_log.error_states.iter() {
            let parser_state = (state.sym.clone(), state.state);

            if seen_errors.contains(&parser_state) && state.sym == "ERROR" {
                continue;
            }
            if state.sym != "ERROR" {
                seen_errors.insert(parser_state);
            }
            error_states.push(serde_json::json!({
                "state": state.state,
                "sym": state.sym,
                "row": state.row,
                "column": state.column,
            }));
        }

        if error_states.is_empty() {
            panic!("We should have found an error");
        }
        return serde_json::to_string_pretty(&serde_json::json!({
            "tokens": tokens,
            "errorStates": error_states,
        }))
        .unwrap()
        .lines()
        .map(|s| s.to_string())
        .collect();
    }
    vec![]
}

/// Collect ERROR nodes from QMD tree with position info.
///
/// This is QMD-specific because it uses `MarkdownTree` instead of
/// plain `tree_sitter::Tree`.
///
/// Returns Vec of (start_offset, end_offset) for each ERROR node.
pub fn collect_error_node_ranges(tree: &tree_sitter_qmd::MarkdownTree) -> Vec<(usize, usize)> {
    let mut error_nodes = Vec::new();
    collect_error_nodes_recursive(&mut tree.walk(), &mut error_nodes);
    error_nodes
}

fn collect_error_nodes_recursive(
    cursor: &mut tree_sitter_qmd::MarkdownCursor,
    errors: &mut Vec<(usize, usize)>,
) {
    let node = cursor.node();

    if node.kind() == "ERROR" {
        let start = node.start_byte();
        let end = node.end_byte();
        errors.push((start, end));
    }

    // Recurse to children
    if cursor.goto_first_child() {
        loop {
            collect_error_nodes_recursive(cursor, errors);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}
