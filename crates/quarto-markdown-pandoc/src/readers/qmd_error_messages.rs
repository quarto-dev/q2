/*
 * qmd_error_messages.rs
 * Copyright (c) 2025 Posit, PBC
 */

use std::collections::HashSet;

use crate::utils::tree_sitter_log_observer::ConsumedToken;
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

    if let Some(entry) = error_entry {
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
                            let token_span_end = {
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
                            let token_location_end = quarto_source_map::utils::offset_to_location(
                                &input_str,
                                token_span_end,
                            )
                            .unwrap_or(quarto_source_map::Location {
                                offset: token_span_end,
                                row: token.row,
                                column: token.column + token.size.max(1),
                            });
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

        builder.build()
    } else {
        // Fallback for errors not in the table
        DiagnosticMessageBuilder::error("Parse error")
            .with_location(source_info)
            .problem("unexpected character or token here")
            .build()
    }
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
