/*
 * qmd_error_messages.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::utils::tree_sitter_log_observer::ConsumedToken;
use quarto_source_map::Location;
use serde_json::json;

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
                    filename,
                    source_context,
                );
                result.push(diagnostic);
            }
        }
    }

    return result;
}

pub fn json_error_message_from_parse_state(
    input_bytes: &[u8],
    parse_state: &crate::utils::tree_sitter_log_observer::ProcessMessage,
    consumed_tokens: &[ConsumedToken],
    filename: &str,
) -> serde_json::Value {
    // Look up the error entry from the table
    let error_entry = crate::readers::qmd_error_message_table::lookup_error_entry(parse_state);

    if let Some(entry) = error_entry {
        // Convert input to string for calculating positions
        let input_str = String::from_utf8_lossy(input_bytes);

        // Calculate byte offset from row/column
        let byte_offset = calculate_byte_offset(&input_str, parse_state.row, parse_state.column);

        // Create the main error location
        let mut error_json = json!({
            "filename": filename,
            "title": entry.error_info.title,
            "message": entry.error_info.message,
            "location": {
                "row": parse_state.row + 1,  // Convert to 1-based
                "column": parse_state.column + 1,  // Convert to 1-based
                "byte_offset": byte_offset,
                "size": parse_state.size.max(1)
            }
        });

        // Add notes with their corresponding captures
        let mut notes = Vec::new();
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
                        if let Some(token) = find_matching_token(consumed_tokens, capture) {
                            // Calculate the span for this token
                            let mut token_byte_offset =
                                calculate_byte_offset(&input_str, token.row, token.column);
                            let mut char_offset = 0;

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
                                    token_byte_offset += this_offset;
                                    char_offset += this_offset;
                                    if input_str.get(token_byte_offset..).is_none() {
                                        break;
                                    }
                                }
                            }

                            notes.push(json!({
                                "message": note.message,
                                "noteType": note.note_type,
                                "location": {
                                    "row": token.row + 1,  // Convert to 1-based
                                    "column": token.column + 1 + char_offset,  // Convert to 1-based
                                    "byte_offset": token_byte_offset,
                                    "size": token.size.max(1)
                                }
                            }));
                        }
                    }
                }
                "label-range" => {
                    // Find the begin and end captures
                    let begin_capture = note.label_begin.and_then(|label| {
                        entry.error_info.captures.iter().find(|c| c.label == label)
                    });
                    let end_capture = note.label_end.and_then(|label| {
                        entry.error_info.captures.iter().find(|c| c.label == label)
                    });

                    if let (Some(begin_cap), Some(end_cap)) = (begin_capture, end_capture) {
                        // Find the consumed tokens that match these captures
                        let begin_token = find_matching_token(consumed_tokens, begin_cap);
                        let end_token = find_matching_token(consumed_tokens, end_cap);

                        if let (Some(begin_tok), Some(end_tok)) = (begin_token, end_token) {
                            // Calculate the span from the beginning of begin_token to the end of end_token
                            let begin_byte_offset =
                                calculate_byte_offset(&input_str, begin_tok.row, begin_tok.column);
                            let end_byte_offset =
                                calculate_byte_offset(&input_str, end_tok.row, end_tok.column);

                            notes.push(json!({
                                "message": note.message,
                                "noteType": note.note_type,
                                "range": {
                                    "start": {
                                        "row": begin_tok.row + 1,  // Convert to 1-based
                                        "column": begin_tok.column + 1,  // Convert to 1-based
                                        "byte_offset": begin_byte_offset
                                    },
                                    "end": {
                                        "row": end_tok.row + 1,  // Convert to 1-based
                                        "column": end_tok.column + 1,  // Convert to 1-based
                                        "byte_offset": end_byte_offset + end_tok.size.max(1)
                                    }
                                }
                            }));
                        }
                    }
                }
                _ => {
                    // Unknown note type, skip
                }
            }
        }

        if !notes.is_empty() {
            error_json["notes"] = json!(notes);
        }

        error_json
    } else {
        // Fallback for errors not in the table
        let input_str = String::from_utf8_lossy(input_bytes);
        let byte_offset = calculate_byte_offset(&input_str, parse_state.row, parse_state.column);

        json!({
            "filename": filename,
            "title": "Parse error",
            "message": "unexpected",
            "location": {
                "row": parse_state.row + 1,
                "column": parse_state.column + 1,
                "byte_offset": byte_offset,
                "size": parse_state.size.max(1)
            }
        })
    }
}

fn find_matching_token<'a>(
    consumed_tokens: &'a [ConsumedToken],
    capture: &crate::readers::qmd_error_message_table::ErrorCapture,
) -> Option<&'a ConsumedToken> {
    // Find a token that matches both the lr_state and sym from the capture
    consumed_tokens
        .iter()
        .find(|token| token.lr_state == capture.lr_state && token.sym == capture.sym)
}

/// Convert a parse state error into a structured DiagnosticMessage
fn error_diagnostic_from_parse_state(
    input_bytes: &[u8],
    parse_state: &crate::utils::tree_sitter_log_observer::ProcessMessage,
    consumed_tokens: &[ConsumedToken],
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
    let span_end = byte_offset + parse_state.size.max(1);

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
                        if let Some(token) = find_matching_token(consumed_tokens, capture) {
                            // Calculate the byte offset for this token
                            let mut token_byte_offset =
                                calculate_byte_offset(&input_str, token.row, token.column);
                            let token_span_end = token_byte_offset + token.size.max(1);

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
                "label-range" => {
                    // Find the begin and end captures
                    let begin_capture = note.label_begin.and_then(|label| {
                        entry.error_info.captures.iter().find(|c| c.label == label)
                    });
                    let end_capture = note.label_end.and_then(|label| {
                        entry.error_info.captures.iter().find(|c| c.label == label)
                    });

                    if let (Some(begin_cap), Some(end_cap)) = (begin_capture, end_capture) {
                        // Find the consumed tokens that match these captures
                        let begin_token = find_matching_token(consumed_tokens, begin_cap);
                        let end_token = find_matching_token(consumed_tokens, end_cap);

                        if let (Some(begin_tok), Some(end_tok)) = (begin_token, end_token) {
                            // Calculate the span from the beginning of begin_token to the end of end_token
                            let begin_byte_offset =
                                calculate_byte_offset(&input_str, begin_tok.row, begin_tok.column);
                            let end_byte_offset =
                                calculate_byte_offset(&input_str, end_tok.row, end_tok.column);
                            let range_span_end = end_byte_offset + end_tok.size.max(1);

                            // Create SourceInfo for this range location
                            // Use from_range to create an Original SourceInfo since the range
                            // is in the same file as the main error, not a substring of it
                            let range_location_start =
                                quarto_source_map::utils::offset_to_location(
                                    &input_str,
                                    begin_byte_offset,
                                )
                                .unwrap_or(
                                    quarto_source_map::Location {
                                        offset: begin_byte_offset,
                                        row: begin_tok.row,
                                        column: begin_tok.column,
                                    },
                                );
                            let range_location_end = quarto_source_map::utils::offset_to_location(
                                &input_str,
                                range_span_end,
                            )
                            .unwrap_or(quarto_source_map::Location {
                                offset: range_span_end,
                                row: end_tok.row,
                                column: end_tok.column + end_tok.size.max(1),
                            });
                            let range_source_info = quarto_source_map::SourceInfo::from_range(
                                quarto_source_map::FileId(0),
                                quarto_source_map::Range {
                                    start: range_location_start,
                                    end: range_location_end,
                                },
                            );

                            // Add as info detail with location
                            builder = builder.add_info_at(note.message, range_source_info);
                        }
                    }
                }
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
    let mut current_row = 0;
    let mut current_col = 0;

    for (i, ch) in input.char_indices() {
        if current_row == row && current_col == column {
            return i;
        }

        if ch == '\n' {
            current_col += 1;
            // Check if the target is at the newline position (end of line)
            if current_row == row && current_col == column {
                return i;
            }
            current_row += 1;
            current_col = 0;
        } else {
            current_col += 1;
        }
    }

    // If we're looking for EOF position, return the length
    if current_row == row && current_col == column {
        return input.len();
    }

    // If we couldn't find the position, clamp to EOF
    input.len()
}

// Helper function to produce JSON-formatted error messages for use as a closure
pub fn produce_json_error_messages(
    input_bytes: &[u8],
    tree_sitter_log: &crate::utils::tree_sitter_log_observer::TreeSitterLogObserver,
    filename: &str,
) -> Vec<String> {
    assert!(tree_sitter_log.had_errors());
    assert!(tree_sitter_log.parses.len() > 0);

    let mut json_errors = Vec::new();
    for parse in &tree_sitter_log.parses {
        // there was an error in the block structure; report that.
        for (_, process_log) in &parse.processes {
            for state in process_log.error_states.iter() {
                let error_json = json_error_message_from_parse_state(
                    input_bytes,
                    state,
                    &parse.consumed_tokens,
                    filename,
                );
                json_errors.push(error_json);
            }
        }
    }

    // Return JSON array as a single string
    let json_array = serde_json::json!(json_errors);
    vec![serde_json::to_string_pretty(&json_array).unwrap()]
}

// we call this in the stage where we're building the matching between
// the corpus of error messages and the parser states
// so that we can produce structured error messages later
pub fn produce_error_message_json(
    tree_sitter_log: &crate::utils::tree_sitter_log_observer::TreeSitterLogObserver,
) -> Vec<String> {
    for parse in &tree_sitter_log.parses {
        for (_, process_log) in &parse.processes {
            if process_log.is_good() {
                continue;
            }
            let mut tokens: Vec<serde_json::Value> = vec![];
            let mut error_states: Vec<serde_json::Value> = vec![];
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
                error_states.push(serde_json::json!({
                    "state": state.state,
                    "sym": state.sym,
                    "row": state.row,
                    "column": state.column,
                }));
            }

            if error_states.len() > 0 {
                // when erroring, produce the errors only for the
                // first failing state.
                return serde_json::to_string_pretty(&serde_json::json!({
                    "tokens": tokens,
                    "errorStates": error_states,
                }))
                .unwrap()
                .lines()
                .map(|s| s.to_string())
                .collect();
            }
        }
    }
    vec![]
}
