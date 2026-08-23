/*
 * error_generation.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Error generation from tree-sitter parse states.
//!
//! This module converts tree-sitter parse errors into user-friendly diagnostic messages
//! using the error table to look up appropriate messages and context.

use std::collections::HashSet;

use crate::error_table::{ErrorCapture, ErrorTableEntry, lookup_error_entry};
use crate::tree_sitter_log::{ConsumedToken, TreeSitterLogObserver};
use quarto_error_reporting::DiagnosticMessage;

/// Produce structured DiagnosticMessage objects from parse errors.
///
/// Uses the error table to map parser states to meaningful error messages.
/// The SourceContext is used to properly calculate source locations for multi-file scenarios.
///
/// # Arguments
///
/// * `input_bytes` - The input source code as bytes
/// * `tree_sitter_log` - Captured parse log with error states
/// * `error_table` - Table mapping (state, sym) to error messages
/// * `filename` - Name of the file being parsed
/// * `source_context` - Source mapping context for location calculation
pub fn produce_diagnostic_messages(
    input_bytes: &[u8],
    tree_sitter_log: &TreeSitterLogObserver,
    error_table: &[ErrorTableEntry],
    filename: &str,
    source_context: &quarto_source_map::SourceContext,
) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    assert!(tree_sitter_log.had_errors());
    assert!(!tree_sitter_log.parses.is_empty());

    let mut result: Vec<quarto_error_reporting::DiagnosticMessage> = vec![];
    let mut seen_errors: std::collections::HashSet<(usize, usize)> =
        std::collections::HashSet::new();

    for parse in &tree_sitter_log.parses {
        for process_log in parse.processes.values() {
            for state in process_log.error_states.iter() {
                if seen_errors.contains(&(state.row, state.column)) {
                    continue;
                }
                seen_errors.insert((state.row, state.column));
                let diagnostic = error_diagnostic_from_parse_state(
                    input_bytes,
                    state,
                    &parse.consumed_tokens,
                    &parse.all_tokens,
                    error_table,
                    filename,
                    source_context,
                );
                result.push(diagnostic);
            }
        }
    }

    // Sort diagnostics by file position (start offset)
    result.sort_by_key(|diag| diag.location.as_ref().map_or(0, |loc| loc.start_offset()));

    result
}

fn appears_not_after(
    token: &ConsumedToken,
    parse_state: &crate::tree_sitter_log::ProcessMessage,
) -> bool {
    token.row < parse_state.row
        || (token.row == parse_state.row && token.column <= parse_state.column)
}

fn find_matching_token<'a>(
    consumed_tokens: &'a [ConsumedToken],
    capture: &ErrorCapture,
    parse_state: &crate::tree_sitter_log::ProcessMessage,
) -> Option<&'a ConsumedToken> {
    // Find a token that matches both the lr_state and sym from the capture
    consumed_tokens.iter().rev().find(|token| {
        token.lr_state == capture.lr_state
            && token.sym == capture.sym
            && appears_not_after(token, parse_state)
    })
}

pub fn diagnostic_score(diag: &DiagnosticMessage) -> usize {
    diag.hints.len() + diag.details.len() + diag.code.as_ref().map_or(0, |_| 1)
}

/// Convert a parse state error into a structured DiagnosticMessage
fn error_diagnostic_from_parse_state(
    input_bytes: &[u8],
    parse_state: &crate::tree_sitter_log::ProcessMessage,
    consumed_tokens: &[ConsumedToken],
    all_tokens: &[ConsumedToken],
    error_table: &[ErrorTableEntry],
    _filename: &str,
    _source_context: &quarto_source_map::SourceContext,
) -> quarto_error_reporting::DiagnosticMessage {
    use quarto_error_reporting::DiagnosticMessageBuilder;

    // Look up the error entry from the table
    let error_entry = lookup_error_entry(error_table, parse_state);

    // All offset arithmetic operates on `input_bytes` directly.
    // Tree-sitter reports `parse_state.row` / `parse_state.column` as
    // byte counts in the source it was given, which agrees with
    // `input_bytes`. Mixing in a `String::from_utf8_lossy(input_bytes)`
    // shifts offsets whenever the input has invalid UTF-8 (each bad
    // byte expands to a 3-byte `U+FFFD`), and slicing at a tree-sitter
    // offset then panics on a non-char-boundary (bd-6qbto).

    let byte_offset = calculate_byte_offset(input_bytes, parse_state.row, parse_state.column);
    let span_end = advance_chars(input_bytes, byte_offset, parse_state.size.max(1));

    let start_location =
        offset_to_location_bytes(input_bytes, byte_offset).unwrap_or(quarto_source_map::Location {
            offset: byte_offset,
            row: parse_state.row,
            column: parse_state.column,
        });
    let end_location =
        offset_to_location_bytes(input_bytes, span_end).unwrap_or(quarto_source_map::Location {
            offset: span_end,
            row: parse_state.row,
            column: parse_state.column + parse_state.size.max(1),
        });

    // Create SourceInfo for the error location
    let range = quarto_source_map::Range {
        start: start_location,
        end: end_location,
    };
    let source_info = quarto_source_map::SourceInfo::from_range(
        quarto_source_map::FileId(0), // File ID 0 (set up in ASTContext)
        range,
    );

    error_entry
        .into_iter()
        .map(|entry| {
            // if let Some(entry) = error_entry {
            // Build diagnostic from error table entry
            let mut builder = DiagnosticMessageBuilder::error(entry.error_info.title)
                .with_location(source_info.clone())
                .problem(entry.error_info.message);

            // Add error code if present
            if let Some(code) = entry.error_info.code {
                builder = builder.with_code(code);
            }

            // Add notes with their corresponding source locations
            for note in entry.error_info.notes {
                match note.note_type {
                    "simple" => {
                        // Find the capture that this note refers to
                        if let Some(capture) =
                            entry.error_info.captures.iter().find(|c| match note.label {
                                None => false,
                                Some(l) => c.label == l,
                            })
                        {
                            // Find the consumed token that matches this capture
                            if let Some(token) =
                                find_matching_token(consumed_tokens, capture, parse_state)
                                    .or(find_matching_token(all_tokens, capture, parse_state))
                            {
                                // All offset math is in the input_bytes domain.
                                // See the note at the top of this function.
                                let mut token_byte_offset =
                                    calculate_byte_offset(input_bytes, token.row, token.column);
                                let mut token_span_end = advance_chars(
                                    input_bytes,
                                    token_byte_offset,
                                    token.size.max(1),
                                );

                                // Trim ASCII spaces at the edges. b' ' is one byte
                                // and never appears inside a multi-byte sequence, so
                                // a byte-level walk is correct.
                                if note.trim_leading_space.unwrap_or_default() {
                                    while token_byte_offset < token_span_end
                                        && input_bytes[token_byte_offset] == b' '
                                    {
                                        token_byte_offset += 1;
                                    }
                                }
                                if note.trim_trailing_space.unwrap_or_default() {
                                    while token_span_end > token_byte_offset
                                        && input_bytes[token_span_end - 1] == b' '
                                    {
                                        token_span_end -= 1;
                                    }
                                }

                                let token_location_start =
                                    offset_to_location_bytes(input_bytes, token_byte_offset)
                                        .unwrap_or(quarto_source_map::Location {
                                            offset: token_byte_offset,
                                            row: token.row,
                                            column: token.column,
                                        });
                                let token_location_end =
                                    offset_to_location_bytes(input_bytes, token_span_end)
                                        .unwrap_or(quarto_source_map::Location {
                                            offset: token_span_end,
                                            row: token.row,
                                            column: token.column + token.size.max(1),
                                        });

                                let token_source_info = quarto_source_map::SourceInfo::from_range(
                                    quarto_source_map::FileId(0),
                                    quarto_source_map::Range {
                                        start: token_location_start,
                                        end: token_location_end,
                                    },
                                );

                                // Add as info detail with location (will show as blue label in Ariadne)
                                builder = builder.add_info_at(note.message, token_source_info);
                            }
                        }
                    }
                    "label-range" => panic!("unsupported!"),
                    _ => {}
                }
            }

            // Add hints
            for hint in entry.error_info.hints {
                builder = builder.add_hint(*hint);
            }

            builder.build()
        })
        .max_by(|diag1, diag2| diagnostic_score(diag1).cmp(&diagnostic_score(diag2)))
        .unwrap_or(
            // Fallback for errors not in the table
            DiagnosticMessageBuilder::error("Parse error")
                .with_location(source_info)
                .problem("unexpected character or token here")
                .build(),
        )
}

/// Compute a byte offset into `input` given tree-sitter's (row,
/// column) coordinates. Tree-sitter reports `column` as a *byte*
/// offset from the start of the line, not a character offset, so a
/// byte-level walk is the natural fit. Walking bytes also avoids the
/// `from_utf8_lossy` offset drift that crashed the previous
/// implementation on invalid UTF-8 input (bd-6qbto).
fn calculate_byte_offset(input: &[u8], row: usize, column: usize) -> usize {
    let mut current_row = 0_usize;
    let mut line_start = 0_usize;
    for (i, &b) in input.iter().enumerate() {
        if b == b'\n' {
            if current_row == row {
                return (line_start + column).min(i);
            }
            current_row += 1;
            line_start = i + 1;
        }
    }
    // Last line (no trailing newline) or empty input.
    if current_row == row {
        return (line_start + column).min(input.len());
    }
    // Row not reachable — clamp to EOF.
    input.len()
}

/// Advance `size` characters from `start` within `input`, returning
/// the resulting byte offset. Decodes UTF-8 a codepoint at a time;
/// each byte that fails to start a valid sequence counts as one
/// "character" and advances by one byte, keeping all offsets in the
/// original byte domain.
///
/// **This is not `from_utf8_lossy`'s rule, despite an earlier version of
/// this comment claiming it was** (corrected 2026-08-23, Plan 3 Phase 2).
/// `from_utf8_lossy` folds each maximal ill-formed *subpart* into one
/// `U+FFFD`, while this walker advances **one byte per ill-formed byte**.
///
/// So the two coincide exactly when every maximal ill-formed subpart is a
/// **single byte**, and diverge as soon as one spans two or more.
/// `[0xFF, 0xFE, 0xFD]` coincides (three one-byte subparts), and so do
/// `[0xC2]` and `[0xC2, 0x41]` — `0xC2` is a *valid* 2-byte lead, merely
/// truncated or mis-continued, and its maximal subpart is still just
/// `[0xC2]`. `[0xE2, 0x82]` diverges 2 vs 1, because a valid 2-byte prefix
/// of a 3-byte sequence is a 2-byte subpart. (An earlier version of this
/// sentence said the rules coincide "only when every ill-formed byte is
/// independently invalid" — false, as `[0xC2]` shows; corrected in fix
/// round 2.) Exhaustively pinned by
/// `ill_formed_counting_diverges_exactly_when_a_subpart_spans_multiple_bytes`.
///
/// Counting each byte separately is the deliberate choice: it keeps a
/// character step from ever crossing more bytes than it can account for.
/// [`offset_to_location_bytes`] uses this same walker, so the two agree with
/// each other — which is what actually matters.
fn advance_chars(input: &[u8], start: usize, size: usize) -> usize {
    let mut pos = start.min(input.len());
    let mut count = 0_usize;
    while count < size && pos < input.len() {
        let step = next_codepoint_size(&input[pos..]);
        if step == 0 {
            break;
        }
        pos += step;
        count += 1;
    }
    pos.min(input.len())
}

/// Return the byte length of the next codepoint at the start of
/// `bytes`. Returns 1 for any invalid lead, truncated continuation,
/// or otherwise ill-formed sequence — so the caller can treat each
/// invalid byte as a single "character". Returns 0 only when `bytes`
/// is empty.
fn next_codepoint_size(bytes: &[u8]) -> usize {
    let Some(&b) = bytes.first() else {
        return 0;
    };
    let expected_len = match b {
        0x00..=0x7F => 1,
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return 1, // invalid lead → consume the lone byte
    };
    if bytes.len() < expected_len {
        return 1; // truncated → consume the lone byte
    }
    match std::str::from_utf8(&bytes[..expected_len]) {
        Ok(_) => expected_len,
        Err(_) => 1, // invalid continuation → consume the lone byte
    }
}

/// Bytes-aware sibling of `quarto_source_map::utils::offset_to_location`.
/// Returns `None` if the offset is out of bounds.
///
/// Column is the number of characters preceding `offset` on its line,
/// counted with [`next_codepoint_size`] — the same walker
/// [`advance_chars`] uses, so the two agree on what a "character" is.
/// **Each ill-formed byte counts as one**, and this differs from
/// `from_utf8_lossy`, which folds a maximal ill-formed subpart into a
/// single `U+FFFD`: `[0xE2, 0x82]` is one replacement character to it and
/// **two** characters here. Pinned by
/// `offset_to_location_bytes_counts_each_ill_formed_byte_separately`.
///
/// An `offset` landing *inside* a character is **floored** to that
/// character's first byte, and that character is not counted. Both
/// `quarto-source-map` implementations do this — `utils::offset_to_location`
/// and `FileInformation::offset_to_location`, which Plan 1 measured
/// disagreeing by one column here and made agree — and a consumer that
/// slices with the returned offset needs a char boundary. Pinned by
/// `offset_to_location_bytes_agrees_with_source_map_on_mid_character_offsets`.
///
/// **Flooring is not unconditionally safe on corrupt input.** It cannot
/// panic and `start <= end` always holds, but flooring the *end* of a span
/// can collapse it: for a 4-byte codepoint at `0..4` with `byte_offset = 2`
/// and `size = 1`, `advance_chars` yields `span_end = 3` and both ends floor
/// to `0` — a zero-width span. That is strictly better than the pre-fix
/// panic and is only reachable when a tree-sitter offset lands mid-character,
/// which valid UTF-8 does not produce.
///
/// **Only `offset` reaches production.** All four call sites (`:122`, `:128`,
/// `:203`, `:210`) hand the result to `SourceInfo::from_range`, which keeps
/// `range.start.offset` and `range.end.offset` and discards `row` and
/// `column` (`quarto-source-map-0.1.3/src/source_info.rs:185-191`). The
/// column rule below is therefore documentation and test surface, not
/// something a diagnostic renders today; the floored **offset** is the half
/// that actually ships.
///
/// *Measured 2026-08-23 (Plan 3 Phase 2, `bd-mxa44voa`).* Before that test,
/// this function returned the raw offset and a column overcounted by one for
/// a mid-character offset — slicing mid-character leaves a truncated tail
/// that `from_utf8_lossy` renders as a single `U+FFFD`. The doc comment
/// nevertheless claimed it "matches the source-map utility exactly" on valid
/// UTF-8. It did not; the claim had never been exercised.
fn offset_to_location_bytes(input: &[u8], offset: usize) -> Option<quarto_source_map::Location> {
    if offset > input.len() {
        return None;
    }
    let mut row = 0_usize;
    let mut line_start = 0_usize;
    for (i, &b) in input[..offset].iter().enumerate() {
        if b == b'\n' {
            row += 1;
            line_start = i + 1;
        }
    }
    // Walk the line a character at a time. `pos` ends on the first byte of
    // the character containing `offset` (== `offset` when it is already a
    // boundary), which is the floor both source-map implementations return.
    let mut column = 0_usize;
    let mut pos = line_start;
    while pos < offset {
        let step = next_codepoint_size(&input[pos..]);
        // `pos < offset <= input.len()` keeps the slice non-empty, so `step`
        // is always >= 1; the `== 0` arm is belt-and-braces against a spin.
        if step == 0 || pos + step > offset {
            break; // `offset` is inside this character — floor to its start.
        }
        pos += step;
        column += 1;
    }
    Some(quarto_source_map::Location {
        offset: pos,
        row,
        column,
    })
}

// we call this in the stage where we're building the matching between
// the corpus of error messages and the parser states
// so that we can produce structured error messages later
pub fn produce_error_message_json(
    tree_sitter_log: &crate::tree_sitter_log::TreeSitterLogObserver,
) -> Vec<String> {
    let mut seen_errors: HashSet<(String, usize)> = HashSet::new();

    for parse in &tree_sitter_log.parses {
        let process_log = &parse.processes[&0];
        // for (_, process_log) in &parse.processes {
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

/// Collect ERROR nodes from tree with position info
/// Returns Vec of (start_offset, end_offset) for each ERROR node
pub fn collect_error_node_ranges(tree: &tree_sitter::Tree) -> Vec<(usize, usize)> {
    let mut error_nodes = Vec::new();
    collect_error_nodes_recursive(&mut tree.walk(), &mut error_nodes);
    error_nodes
}

fn collect_error_nodes_recursive(
    cursor: &mut tree_sitter::TreeCursor,
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

/// Filter to outermost (non-nested) ERROR nodes
/// Returns indices into the error_nodes vector of nodes that are not contained within any other node
pub fn get_outer_error_nodes(error_nodes: &[(usize, usize)]) -> Vec<usize> {
    let mut outer_errors = Vec::new();

    for i in 0..error_nodes.len() {
        let (start_i, end_i) = error_nodes[i];
        let mut is_outer = true;

        for (j, &(start_j, end_j)) in error_nodes.iter().enumerate() {
            if i == j {
                continue;
            }

            // Check if node i is contained within node j
            if start_i >= start_j && end_i <= end_j {
                is_outer = false;
                break;
            }
        }

        if is_outer {
            outer_errors.push(i);
        }
    }

    outer_errors
}

/// Calculate the gap distance between two ranges
/// Returns 0 if ranges overlap, otherwise returns minimum byte gap
fn range_gap_distance(r1_start: usize, r1_end: usize, r2_start: usize, r2_end: usize) -> usize {
    if r1_end <= r2_start {
        // r1 is before r2
        r2_start - r1_end
    } else {
        r1_start.saturating_sub(r2_end)
    }
}

/// Collect all location ranges from a diagnostic (main location + detail locations)
fn collect_all_location_ranges(
    diag: &quarto_error_reporting::DiagnosticMessage,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();

    // Add main location
    if let Some(loc) = &diag.location {
        ranges.push((loc.start_offset(), loc.end_offset()));
    }

    // Add detail locations
    for detail in &diag.details {
        if let Some(loc) = &detail.location {
            ranges.push((loc.start_offset(), loc.end_offset()));
        }
    }

    ranges
}

/// Check if any of the diagnostic ranges overlaps with the given ERROR node range
fn any_range_overlaps(ranges: &[(usize, usize)], err_start: usize, err_end: usize) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| start < err_end && end > err_start)
}

/// Find the closest ERROR node to the diagnostic by minimum gap distance
/// Returns the index into error_nodes, or None if no nodes exist
fn find_closest_error_node(
    diag_ranges: &[(usize, usize)],
    error_nodes: &[(usize, usize)],
) -> Option<usize> {
    if error_nodes.is_empty() {
        return None;
    }

    // Find ERROR node with minimum distance to ANY of the diagnostic's ranges
    error_nodes
        .iter()
        .enumerate()
        .min_by_key(|&(_, &(err_start, err_end))| {
            // Minimum distance from this ERROR node to any diagnostic range
            diag_ranges
                .iter()
                .map(|&(diag_start, diag_end)| {
                    range_gap_distance(err_start, err_end, diag_start, diag_end)
                })
                .min()
                .unwrap_or(usize::MAX)
        })
        .map(|(idx, _)| idx)
}

/// Prune diagnostics based on ERROR node ranges
/// Strategy:
/// 1. Assign each error diagnostic to the closest ERROR node (by overlap or distance)
/// 2. For each ERROR node, keep only the EARLIEST error (tiebreak with score)
/// 3. Never discard any diagnostics - all errors are assigned to some node
pub fn prune_diagnostics_by_error_nodes(
    diagnostics: Vec<DiagnosticMessage>,
    error_nodes: &[(usize, usize)],
    outer_node_indices: &[usize],
) -> Vec<DiagnosticMessage> {
    // If no ERROR nodes, keep all diagnostics as fallback
    if outer_node_indices.is_empty() {
        return diagnostics;
    }

    // Build the outer error ranges
    let outer_ranges: Vec<(usize, usize)> = outer_node_indices
        .iter()
        .map(|&idx| error_nodes[idx])
        .collect();

    // Assign diagnostics to ERROR nodes
    use std::collections::BTreeMap;
    let mut diagnostics_by_range: BTreeMap<usize, Vec<usize>> = BTreeMap::new();

    for (diag_idx, diag) in diagnostics.iter().enumerate() {
        // Only process error diagnostics (skip warnings)
        if diag.kind != quarto_error_reporting::DiagnosticKind::Error {
            continue;
        }

        // Collect all location ranges from this diagnostic
        let diag_ranges = collect_all_location_ranges(diag);

        if diag_ranges.is_empty() {
            // No location info - can't assign, but keep it anyway
            continue;
        }

        // Try to find overlapping ERROR node first
        let mut assigned = false;
        for (err_idx, &(err_start, err_end)) in outer_ranges.iter().enumerate() {
            if any_range_overlaps(&diag_ranges, err_start, err_end) {
                diagnostics_by_range
                    .entry(err_idx)
                    .or_default()
                    .push(diag_idx);
                assigned = true;
                break; // Assign to first overlapping node
            }
        }

        // If no overlap, find closest ERROR node by distance
        if !assigned && let Some(closest_idx) = find_closest_error_node(&diag_ranges, &outer_ranges)
        {
            diagnostics_by_range
                .entry(closest_idx)
                .or_default()
                .push(diag_idx);
        }
        // If still not assigned, diagnostic has no location or ERROR nodes are empty
    }

    // For each ERROR node, keep only the earliest diagnostic (tiebreak with score)
    let mut kept_indices = Vec::new();

    for (_range_idx, diag_indices) in diagnostics_by_range.iter() {
        if diag_indices.is_empty() {
            continue;
        }

        // Find the earliest diagnostic in this range
        let best_idx = diag_indices
            .iter()
            .min_by_key(|&&idx| {
                let diag = &diagnostics[idx];
                let start_offset = diag.location.as_ref().map_or(0, |loc| loc.start_offset());
                // Primary: earliest start offset
                // Secondary: highest score (negated for min_by_key)
                let score = diagnostic_score(diag);
                (start_offset, usize::MAX - score)
            })
            .copied()
            .unwrap();

        kept_indices.push(best_idx);
    }

    // Add any error diagnostics that weren't assigned (defensive - shouldn't happen often)
    // This ensures we never discard diagnostics
    for (idx, diag) in diagnostics.iter().enumerate() {
        if diag.kind == quarto_error_reporting::DiagnosticKind::Error
            && !diagnostics_by_range.values().any(|v| v.contains(&idx))
        {
            kept_indices.push(idx);
        }
    }

    // Sort to maintain original order
    kept_indices.sort_unstable();

    // Build result: kept error diagnostics + all non-error diagnostics
    let mut result = Vec::new();
    let kept_set: HashSet<usize> = kept_indices.iter().copied().collect();

    for (idx, diag) in diagnostics.into_iter().enumerate() {
        // Keep if: (1) it's in the kept set, OR (2) it's not an error (e.g., warning)
        if kept_set.contains(&idx) || diag.kind != quarto_error_reporting::DiagnosticKind::Error {
            result.push(diag);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter_log::{
        ProcessMessage, TreeSitterLogObserver, TreeSitterParseLog, TreeSitterProcessLog,
    };
    use hashlink::LinkedHashMap as HashMap;

    /// Pins the exact boundary between this crate's character rule and
    /// `from_utf8_lossy`'s, as a biconditional rather than an example.
    ///
    /// Written because this phase asserted the boundary in prose **twice** and
    /// got it wrong both times: first that the two rules matched, then that
    /// they "coincide only when every ill-formed byte is independently
    /// invalid". `[0xC2]` refutes the second — `0xC2` is a perfectly valid
    /// 2-byte lead, merely truncated by end of input, and the rules coincide
    /// there anyway.
    ///
    /// The real rule is about subpart *length*: [`next_codepoint_size`]
    /// advances one byte for every ill-formed byte, while `from_utf8_lossy`
    /// emits one `U+FFFD` per maximal ill-formed subpart. The counts are
    /// therefore equal iff every such subpart is exactly one byte long.
    ///
    /// The predicate is derived from `from_utf8_lossy`'s own output rather
    /// than reimplemented: bytes not accounted for by a non-replacement
    /// character are the ill-formed ones, and each `U+FFFD` is one subpart,
    /// so "every subpart is one byte" is `ill_formed_bytes == n_replacements`.
    /// The alphabet deliberately excludes a genuine `U+FFFD`
    /// (`[0xEF, 0xBF, 0xBD]`), which would otherwise be counted as a
    /// replacement it is not.
    #[test]
    fn ill_formed_counting_diverges_exactly_when_a_subpart_spans_multiple_bytes() {
        // One representative of each class the `next_codepoint_size` arms
        // distinguish: ASCII, a 2-/3-/4-byte lead, a bare continuation byte,
        // and a byte that is no lead at all.
        const ALPHABET: [u8; 6] = [0x41, 0xC2, 0xE2, 0xF0, 0x82, 0xFF];

        let mut sequences: Vec<Vec<u8>> = ALPHABET.iter().map(|&b| vec![b]).collect();
        let mut all = sequences.clone();
        for _ in 0..3 {
            sequences = sequences
                .iter()
                .flat_map(|seq| {
                    ALPHABET.iter().map(move |&b| {
                        let mut next = seq.clone();
                        next.push(b);
                        next
                    })
                })
                .collect();
            all.extend(sequences.iter().cloned());
        }

        let mut diverging = 0_usize;
        for bytes in &all {
            let column = offset_to_location_bytes(bytes, bytes.len())
                .expect("offset is in bounds at the end of input")
                .column;

            let lossy = String::from_utf8_lossy(bytes);
            let replacements = lossy
                .chars()
                .filter(|&c| c == char::REPLACEMENT_CHARACTER)
                .count();
            let valid_bytes: usize = lossy
                .chars()
                .filter(|&c| c != char::REPLACEMENT_CHARACTER)
                .map(char::len_utf8)
                .sum();
            let ill_formed_bytes = bytes.len() - valid_bytes;

            let every_subpart_is_one_byte = ill_formed_bytes == replacements;
            let counts_agree = column == lossy.chars().count();
            if !counts_agree {
                diverging += 1;
            }

            assert_eq!(
                counts_agree, every_subpart_is_one_byte,
                "{bytes:02X?}: column {column}, from_utf8_lossy {:?} \
                 ({replacements} replacement(s) over {ill_formed_bytes} ill-formed byte(s))",
                lossy,
            );
        }

        // Anti-vacuity: the biconditional would hold trivially over an
        // alphabet on which the two rules never disagree.
        assert!(
            diverging > 0,
            "alphabet no longer produces any divergence, so the biconditional above proves nothing",
        );
    }

    /// The invalid-UTF-8 half of the column rule, which the valid-UTF-8
    /// agreement test above cannot reach.
    ///
    /// **This behaviour changed in Plan 3 Phase 2 and was undocumented as a
    /// change.** The old implementation counted with `from_utf8_lossy`, which
    /// folds a maximal ill-formed *subpart* into one `U+FFFD`; the walk
    /// counts each ill-formed byte separately. The rules coincide only when
    /// every bad byte is independently invalid — which is exactly what the
    /// pre-existing `bd-6qbto` regression fixture (`[0xFF, 0xFE, 0xFD]`) is,
    /// so that test passing across the change proved nothing about it.
    ///
    /// The truncated-but-well-formed prefixes below are the discriminating
    /// cases. Each asserts the column *and* what `from_utf8_lossy` would have
    /// said, so the divergence is pinned rather than merely described.
    #[test]
    fn offset_to_location_bytes_counts_each_ill_formed_byte_separately() {
        // (bytes, expected column, what `from_utf8_lossy` would count)
        let cases: [(&[u8], usize, usize); 4] = [
            // Truncated 3- and 4-byte sequences: one `U+FFFD` to
            // `from_utf8_lossy`, one character per byte here.
            (&[0xE2, 0x82], 2, 1),
            (&[0xF0, 0x9F, 0x98], 3, 1),
            // A bad lead followed by valid ASCII, and three independently
            // invalid bytes: the two rules agree on both.
            (&[0xE2, 0x41, 0x42], 3, 3),
            (&[0xFF, 0xFE, 0xFD], 3, 3),
        ];

        for (bytes, expected_column, lossy_column) in cases {
            let location = offset_to_location_bytes(bytes, bytes.len())
                .expect("offset is in bounds at the end of input");
            assert_eq!(location.column, expected_column, "column for {bytes:02X?}",);
            assert_eq!(
                String::from_utf8_lossy(bytes).chars().count(),
                lossy_column,
                "from_utf8_lossy baseline for {bytes:02X?} — if this moves, the \
                 divergence documented on offset_to_location_bytes has changed",
            );
        }
    }

    /// `offset_to_location_bytes` documents itself as the bytes-aware sibling
    /// of `quarto_source_map::utils::offset_to_location` that "for valid UTF-8
    /// … matches the source-map utility exactly". **Measured 2026-08-23 (Plan
    /// 3 Phase 2): it did not.** Both source-map implementations floor a
    /// mid-character offset to the start of the enclosing character and do not
    /// count that character in the column. This one returned the raw offset and
    /// a column overcounted by one, because slicing mid-character leaves a
    /// truncated tail that `from_utf8_lossy` renders as one `U+FFFD`.
    ///
    /// Plan 1 measured the two `quarto-source-map` implementations disagreeing
    /// by one column on exactly this input and made them agree; this test pins
    /// the third implementation to the same rule.
    #[test]
    fn offset_to_location_bytes_agrees_with_source_map_on_mid_character_offsets() {
        // First fixture: 'a' at 0, 'é' spanning 1..3, ' ' at 3, 'b' at 4.
        // Second: the same shape on row 1, so the `line_start != 0` path is
        // exercised too.
        for content in ["aé b", "xx\nyé z"] {
            let bytes = content.as_bytes();
            let file_info = quarto_source_map::FileInformation::new(content);
            for offset in 0..=bytes.len() {
                let measured =
                    offset_to_location_bytes(bytes, offset).expect("offset is in bounds");
                let utils = quarto_source_map::utils::offset_to_location(content, offset)
                    .expect("offset is in bounds");
                let via_file_info = file_info
                    .offset_to_location(offset, content)
                    .expect("offset is in bounds");
                assert_eq!(
                    measured, utils,
                    "utils::offset_to_location disagrees at offset {offset} of {content:?}",
                );
                assert_eq!(
                    measured, via_file_info,
                    "FileInformation::offset_to_location disagrees at offset {offset} of {content:?}",
                );
            }
        }

        // The one discriminating offset, stated outright so a reader need not
        // rerun the loop to see what the agreement is about: offset 2 lands
        // inside 'é'.
        let mid = offset_to_location_bytes("aé b".as_bytes(), 2).expect("offset is in bounds");
        assert_eq!(
            mid.offset, 1,
            "a mid-character offset floors to the char start"
        );
        assert_eq!(
            mid.column, 1,
            "the character the offset lands inside is not counted"
        );
    }

    /// Build a minimal observer whose parse log carries one error
    /// state at the given (row, column). The state-machine numbers
    /// don't need to match a real tree-sitter run — the path under
    /// test only reads `(row, column, size, sym, state)` from the
    /// ProcessMessage and looks up the error table by `(state, sym)`,
    /// which falls through to the generic "Parse error" diagnostic
    /// when the table is empty.
    fn observer_with_one_error(row: usize, column: usize, size: usize) -> TreeSitterLogObserver {
        let process_log = TreeSitterProcessLog {
            found_accept: false,
            found_bad_message: false,
            error_states: vec![ProcessMessage {
                version: 0,
                state: 0,
                row,
                column,
                sym: "ERROR".to_string(),
                size,
            }],
            current_message: None,
        };
        let mut processes = HashMap::new();
        processes.insert(0_usize, process_log);
        let parse = TreeSitterParseLog {
            messages: vec![],
            current_process: None,
            current_lookahead: None,
            processes,
            all_tokens: vec![],
            consumed_tokens: vec![],
        };
        let mut observer = TreeSitterLogObserver::default();
        observer.parses.push(parse);
        observer
    }

    /// Regression test for bd-6qbto: diagnostic generation must not
    /// panic on invalid UTF-8 input. Pre-fix, `&input_str[byte_offset..]`
    /// at line 121 panics with "byte index N is not a char boundary;
    /// it is inside '�'" whenever the tree-sitter offset lands inside
    /// the 3-byte expansion `from_utf8_lossy` produces for an invalid
    /// byte.
    #[test]
    fn produce_diagnostic_messages_does_not_panic_on_invalid_utf8() {
        // 100 'a's then three invalid bytes then a newline, then "bc\n".
        // Tree-sitter would report row=0, column=N with N somewhere
        // inside the invalid run; we hand it column=102, which is in
        // the middle of the 0xFE byte.
        let mut input_bytes = vec![b'a'; 100];
        input_bytes.extend_from_slice(&[0xFF, 0xFE, 0xFD, b'\n', b'b', b'c', b'\n']);

        let observer = observer_with_one_error(0, 102, 1);
        let table: Vec<ErrorTableEntry> = Vec::new();
        let source_context = quarto_source_map::SourceContext::new();

        // The call must return a Vec without panicking. We do not
        // assert anything about the produced location — on invalid
        // input it is necessarily a degraded best-effort value.
        let diagnostics =
            produce_diagnostic_messages(&input_bytes, &observer, &table, "x.qmd", &source_context);
        assert!(
            !diagnostics.is_empty(),
            "fallback diagnostic should be produced for unrecognized parse state",
        );
    }

    /// Regression guard for the common case: on valid UTF-8, the
    /// diagnostic's location should match the parse state's row.
    /// This is the no-drift test that protects against the rewrite
    /// of `calculate_byte_offset` accidentally shifting locations
    /// on well-formed input.
    #[test]
    fn produce_diagnostic_messages_location_correct_on_valid_utf8() {
        // Two lines, ASCII. Error reported at row=1, column=2 (the
        // 'l' in "Hello" if we're 0-indexed-on-bytes within the line).
        let input_bytes = b"first line\nHello world\n";

        let observer = observer_with_one_error(1, 2, 1);
        let table: Vec<ErrorTableEntry> = Vec::new();
        let source_context = quarto_source_map::SourceContext::new();

        let diagnostics = produce_diagnostic_messages(
            &input_bytes[..],
            &observer,
            &table,
            "x.qmd",
            &source_context,
        );
        let diag = diagnostics
            .first()
            .expect("expected at least one diagnostic");
        let location = diag.location.as_ref().expect("expected a location");

        // Row should be 1 (the second line). Column is whatever
        // offset_to_location decided based on chars-from-line-start;
        // for ASCII that's identical to bytes-from-line-start, so 2.
        assert_eq!(location.start_offset(), b"first line\n".len() + 2);
    }
}
