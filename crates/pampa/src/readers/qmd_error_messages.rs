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
/// This is a QMD-specific wrapper that provides the error table automatically
/// and applies QMD-specific span adjustments after the generic generator runs.
pub fn produce_diagnostic_messages(
    input_bytes: &[u8],
    tree_sitter_log: &TreeSitterLogObserver,
    filename: &str,
    source_context: &quarto_source_map::SourceContext,
) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    let mut diagnostics = quarto_parse_errors::produce_diagnostic_messages(
        input_bytes,
        tree_sitter_log,
        get_error_table(),
        filename,
        source_context,
    );

    for diag in &mut diagnostics {
        if matches!(diag.code.as_deref(), Some("Q-2-35") | Some("Q-2-36")) {
            widen_diagnostic_to_line(diag, input_bytes);
        }
        if diag.code.as_deref() == Some("Q-2-2") {
            upgrade_q22_to_q237_if_in_blockquote(diag, input_bytes);
        }
    }

    diagnostics
}

/// Upgrade a generic Q-2-2 attribute-specifier diagnostic to Q-2-37 when the
/// failing `{` sits on a blockquote-prefixed line.
///
/// Background: with bd-rfqz the tree-sitter grammar accepts multi-line inline
/// `{...}` attribute lists everywhere except inside a blockquote. The block /
/// inline split means the inline parser only ever sees the first physical line
/// of the attribute list — the scanner short-circuits SOFT_LINE_ENDING when
/// the next line begins with `>` (`scanner.c:2380-2407`) — so the same
/// `(state=2587, sym="_close_block")` error fires for both unclosed
/// top-level `{...}` *and* the in-blockquote multi-line case. We can't
/// distinguish those at the `(lr_state, sym)` lookup level, but we can
/// distinguish them by looking at the source line of the opener.
///
/// Detection rule: if the line containing the diagnostic's reported position,
/// after stripping leading whitespace, begins with `>` (a blockquote prefix),
/// the user almost certainly hit the blockquote limitation rather than an
/// arbitrary unclosed `{`. Rewrite the diagnostic to point at Q-2-37 with a
/// blockquote-specific message and hint.
///
/// Q-2-37 is defined in `resources/error-corpus/Q-2-37.json` (with empty
/// `cases:`, so the auto-generated state table does not contain it — this
/// override is the sole emitter).
fn upgrade_q22_to_q237_if_in_blockquote(
    diag: &mut quarto_error_reporting::DiagnosticMessage,
    input_bytes: &[u8],
) {
    use quarto_source_map::SourceInfo;

    let Some(loc) = diag.location.as_ref() else {
        return;
    };
    let SourceInfo::Original { start_offset, .. } = loc else {
        return;
    };

    let pivot = (*start_offset).min(input_bytes.len());
    let line_start = input_bytes[..pivot]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let line_end = input_bytes[pivot..]
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .map(|p| pivot + p)
        .unwrap_or(input_bytes.len());

    let first_non_ws = input_bytes[line_start..line_end]
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .map(|p| line_start + p)
        .unwrap_or(line_end);

    if input_bytes.get(first_non_ws).copied() != Some(b'>') {
        return;
    }

    diag.code = Some("Q-2-37".to_string());
    diag.title = "Multi-line inline attribute list inside blockquote".to_string();
    diag.problem = Some(
        "Inside a blockquote, an inline `{...}` attribute list cannot span multiple lines.".into(),
    );
    // Drop inherited Q-2-2 notes ("The attribute specifier starts here.") —
    // the new problem text is the explanation, not a generic hand-off to a
    // secondary marker.
    diag.details.clear();
    diag.hints = vec![
        "Put the attribute list on a single line, or move this construct out of the blockquote."
            .into(),
    ];
}

/// Widen a diagnostic's location to span the entire line containing the
/// reported position. Used when the underlying parse error reports a narrow
/// per-token position, but the user-meaningful unit of the diagnostic is the
/// whole line.
///
/// - Q-2-35 (indented code blocks): the scanner-emitted token lands after the
///   whitespace consumption loop, so the reported column is past the leading
///   indentation. Widening covers the indentation too.
/// - Q-2-36 (knitr-style chunk options, Merr-mapped path-B forms — bare label
///   `{r test}`, comma form `{r, …}`): the tree-sitter parse error points at
///   the first offending token (`test`, `r`, etc.); widening spreads the
///   highlight across the full chunk header, matching the path-A site in
///   `treesitter.rs` that emits the same code with an already-line-clipped
///   location.
fn widen_diagnostic_to_line(
    diag: &mut quarto_error_reporting::DiagnosticMessage,
    input_bytes: &[u8],
) {
    use quarto_source_map::SourceInfo;

    let Some(loc) = diag.location.as_ref() else {
        return;
    };
    let SourceInfo::Original {
        file_id,
        start_offset,
        ..
    } = loc
    else {
        return;
    };

    let pivot = (*start_offset).min(input_bytes.len());
    let line_start = input_bytes[..pivot]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let line_end = input_bytes[pivot..]
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .map(|p| pivot + p)
        .unwrap_or(input_bytes.len());

    diag.location = Some(SourceInfo::original(*file_id, line_start, line_end));
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
