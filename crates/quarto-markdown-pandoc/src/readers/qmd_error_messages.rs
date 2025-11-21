/*
 * qmd_error_messages.rs
 * Copyright (c) 2025 Posit, PBC
 */

use std::collections::HashSet;

use crate::utils::tree_sitter_log_observer::ConsumedToken;
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::Location;

/// Produce structured DiagnosticMessage objects from parse errors
/// Uses the SourceContext to properly calculate source locations
pub fn produce_diagnostic_messages(
    input_bytes: &[u8],
    tree_sitter_log: &crate::utils::tree_sitter_log_observer::TreeSitterLogObserver,
    filename: &str,
    source_context: &quarto_source_map::SourceContext,
) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    assert!(tree_sitter_log.had_errors());
    assert!(tree_sitter_log.parses.len() > 0);

    let mut result: Vec<quarto_error_reporting::DiagnosticMessage> = vec![];
    let mut seen_errors: std::collections::HashSet<(usize, usize)> =
        std::collections::HashSet::new();

    for parse in &tree_sitter_log.parses {
        for (_, process_log) in &parse.processes {
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
                    filename,
                    source_context,
                );
                result.push(diagnostic);
            }
        }
    }

    // Sort diagnostics by file position (start offset)
    result.sort_by_key(|diag| {
        diag.location
            .as_ref()
            .map(|loc| loc.start_offset())
            .unwrap_or(0)
    });

    return result;
}

fn appears_not_after(
    token: &ConsumedToken,
    parse_state: &crate::utils::tree_sitter_log_observer::ProcessMessage,
) -> bool {
    token.row < parse_state.row
        || (token.row == parse_state.row && token.column <= parse_state.column)
}

fn find_matching_token<'a>(
    consumed_tokens: &'a [ConsumedToken],
    capture: &crate::readers::qmd_error_message_table::ErrorCapture,
    parse_state: &crate::utils::tree_sitter_log_observer::ProcessMessage,
) -> Option<&'a ConsumedToken> {
    // Find a token that matches both the lr_state and sym from the capture
    consumed_tokens.iter().rev().find(|token| {
        token.lr_state == capture.lr_state
            && token.sym == capture.sym
            && appears_not_after(token, parse_state)
    })
}

pub fn diagnostic_score(diag: &DiagnosticMessage) -> usize {
    diag.hints.len() + diag.details.len() + diag.code.as_ref().map(|_| 1).unwrap_or(0)
}

/// Convert a parse state error into a structured DiagnosticMessage
fn error_diagnostic_from_parse_state(
    input_bytes: &[u8],
    parse_state: &crate::utils::tree_sitter_log_observer::ProcessMessage,
    consumed_tokens: &[ConsumedToken],
    all_tokens: &[ConsumedToken],
    _filename: &str,
    _source_context: &quarto_source_map::SourceContext,
) -> quarto_error_reporting::DiagnosticMessage {
    use quarto_error_reporting::DiagnosticMessageBuilder;

    // Look up the error entry from the table
    let error_entry = crate::readers::qmd_error_message_table::lookup_error_entry(parse_state);

    // Convert input to string for offset calculation
    let input_str = String::from_utf8_lossy(input_bytes);

    // Calculate byte offset and create proper locations using quarto-source-map utilities
    let byte_offset = calculate_byte_offset(&input_str, parse_state.row, parse_state.column);

    // Calculate span_end by advancing parse_state.size characters (not bytes!) from byte_offset
    // This is critical for handling multi-byte UTF-8 characters correctly
    let span_end = {
        let size = parse_state.size.max(1);
        let substring = &input_str[byte_offset..];
        let mut char_count = 0;
        let mut byte_count = 0;

        for ch in substring.chars() {
            if char_count >= size {
                break;
            }
            byte_count += ch.len_utf8();
            char_count += 1;
        }

        (byte_offset + byte_count).min(input_str.len())
    };

    // Use quarto_source_map::utils::offset_to_location to properly calculate locations
    let start_location = quarto_source_map::utils::offset_to_location(&input_str, byte_offset)
        .unwrap_or(quarto_source_map::Location {
            offset: byte_offset,
            row: parse_state.row,
            column: parse_state.column,
        });
    let end_location = quarto_source_map::utils::offset_to_location(&input_str, span_end)
        .unwrap_or(quarto_source_map::Location {
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
                                // Calculate the byte offset for this token
                                let mut token_byte_offset =
                                    calculate_byte_offset(&input_str, token.row, token.column);

                                // Calculate token_span_end by advancing token.size characters (not bytes!)
                                // This is critical for handling multi-byte UTF-8 characters correctly
                                let mut token_span_end = {
                                    let size = token.size.max(1);
                                    let substring = &input_str[token_byte_offset..];
                                    let mut char_count = 0;
                                    let mut byte_count = 0;

                                    for ch in substring.chars() {
                                        if char_count >= size {
                                            break;
                                        }
                                        byte_count += ch.len_utf8();
                                        char_count += 1;
                                    }

                                    (token_byte_offset + byte_count).min(input_str.len())
                                };

                                // Create SourceInfo for this token location
                                // Use from_range to create an Original SourceInfo since the token
                                // is in the same file as the main error, not a substring of it
                                let mut token_location_start =
                                    quarto_source_map::utils::offset_to_location(
                                        &input_str,
                                        token_byte_offset,
                                    )
                                    .unwrap_or(
                                        quarto_source_map::Location {
                                            offset: token_byte_offset,
                                            row: token.row,
                                            column: token.column,
                                        },
                                    );
                                let mut token_location_end =
                                    quarto_source_map::utils::offset_to_location(
                                        &input_str,
                                        token_span_end,
                                    )
                                    .unwrap_or(
                                        quarto_source_map::Location {
                                            offset: token_span_end,
                                            row: token.row,
                                            column: token.column + token.size.max(1),
                                        },
                                    );
                                if note.trim_leading_space.unwrap_or_default() {
                                    // Advance token_byte_offset while trimming leading spaces
                                    loop {
                                        let current_character = input_str
                                            .get(token_byte_offset..)
                                            .and_then(|s| s.chars().next())
                                            .map(|c| c)
                                            .unwrap_or('\0');
                                        if current_character != ' ' {
                                            break;
                                        }
                                        let this_offset = current_character.len_utf8();
                                        token_location_start = Location {
                                            offset: token_location_start.offset + this_offset,
                                            row: token_location_start.row,
                                            column: token_location_start.row + this_offset,
                                        };
                                        token_byte_offset += this_offset;
                                        if input_str.get(token_byte_offset..).is_none() {
                                            break;
                                        }
                                    }
                                }
                                if note.trim_trailing_space.unwrap_or_default() {
                                    // Move token_span_end backward while trimming trailing spaces
                                    loop {
                                        if token_span_end == 0
                                            || token_span_end <= token_byte_offset
                                        {
                                            break;
                                        }
                                        // Get the character just before token_span_end
                                        let slice_before_end =
                                            input_str.get(..token_span_end).unwrap_or("");
                                        let last_character =
                                            slice_before_end.chars().last().unwrap_or('\0');
                                        if last_character != ' ' {
                                            break;
                                        }
                                        let this_offset = last_character.len_utf8();
                                        token_location_end = Location {
                                            offset: token_location_end
                                                .offset
                                                .saturating_sub(this_offset),
                                            row: token_location_end.row,
                                            column: token_location_end
                                                .column
                                                .saturating_sub(this_offset),
                                        };
                                        token_span_end = token_span_end.saturating_sub(this_offset);
                                    }
                                }

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

fn calculate_byte_offset(input: &str, row: usize, column: usize) -> usize {
    // Tree-sitter reports column as a BYTE offset within the line, not a character offset
    let mut current_row = 0;
    let mut byte_offset = 0;

    for ch in input.chars() {
        // Check if we've reached the target position
        if current_row == row && byte_offset >= column {
            // column is a byte offset within the current line
            // We need to find the exact byte position
            break;
        }

        if ch == '\n' {
            byte_offset += ch.len_utf8();
            // Check if target is at newline
            if current_row == row && byte_offset >= column {
                break;
            }
            current_row += 1;
            // Reset byte offset to 0 for the new line... but wait, tree-sitter
            // columns are within-line offsets, so we need to track line starts
        } else {
            byte_offset += ch.len_utf8();
        }
    }

    // Actually, let's reconsider: tree-sitter column is byte offset WITHIN THE LINE
    // So we need to find the start of the target row, then add column bytes
    let mut current_row = 0;
    let mut line_start_offset = 0;

    for (i, ch) in input.char_indices() {
        if ch == '\n' {
            if current_row == row {
                // Found the target row, column is byte offset from line_start_offset
                return (line_start_offset + column).min(i);
            }
            current_row += 1;
            line_start_offset = i + ch.len_utf8();
        }
    }

    // If we're on the last line (no trailing newline) or at EOF
    if current_row == row {
        return (line_start_offset + column).min(input.len());
    }

    // Couldn't find the position, clamp to EOF
    input.len()
}

// we call this in the stage where we're building the matching between
// the corpus of error messages and the parser states
// so that we can produce structured error messages later
pub fn produce_error_message_json(
    tree_sitter_log: &crate::utils::tree_sitter_log_observer::TreeSitterLogObserver,
) -> Vec<String> {
    let mut seen_errors: HashSet<(String, usize)> = HashSet::new();

    for parse in &tree_sitter_log.parses {
        let process_log = parse.processes.get(&0).unwrap();
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

        if error_states.len() == 0 {
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

/// Filter to outermost (non-nested) ERROR nodes
/// Returns indices into the error_nodes vector of nodes that are not contained within any other node
pub fn get_outer_error_nodes(error_nodes: &[(usize, usize)]) -> Vec<usize> {
    let mut outer_errors = Vec::new();

    for i in 0..error_nodes.len() {
        let (start_i, end_i) = error_nodes[i];
        let mut is_outer = true;

        for j in 0..error_nodes.len() {
            if i == j {
                continue;
            }
            let (start_j, end_j) = error_nodes[j];

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
    } else if r2_end <= r1_start {
        // r2 is before r1
        r1_start - r2_end
    } else {
        // Overlapping
        0
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
                    .or_insert_with(Vec::new)
                    .push(diag_idx);
                assigned = true;
                break; // Assign to first overlapping node
            }
        }

        // If no overlap, find closest ERROR node by distance
        if !assigned {
            if let Some(closest_idx) = find_closest_error_node(&diag_ranges, &outer_ranges) {
                diagnostics_by_range
                    .entry(closest_idx)
                    .or_insert_with(Vec::new)
                    .push(diag_idx);
            }
            // If still not assigned, diagnostic has no location or ERROR nodes are empty
        }
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
                let start_offset = diag
                    .location
                    .as_ref()
                    .map(|loc| loc.start_offset())
                    .unwrap_or(0);
                // Primary: earliest start offset
                // Secondary: highest score (negated for min_by_key)
                let score = diagnostic_score(diag);
                (start_offset, std::usize::MAX - score)
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
    kept_indices.sort();

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
