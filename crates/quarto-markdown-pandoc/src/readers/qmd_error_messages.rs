/*
 * qmd_error_messages.rs
 * Copyright (c) 2025 Posit, PBC
 */

use ariadne::{Color, Label, Report, ReportKind, Source};
use crate::utils::tree_sitter_log_observer::ConsumedToken;
use serde_json::json;

/*
this will eventually have to produce a structured error message
with the coordinate systems of the error in a format that can be retargeted so that
we can produce good error messages from inside metadata parses, etc
*/
pub fn produce_error_message(
    input_bytes: &[u8],
    tree_sitter_log: &crate::utils::tree_sitter_log_observer::TreeSitterLogObserver,
    filename: &str,
) -> Vec<String> {
    assert!(tree_sitter_log.had_errors());
    assert!(tree_sitter_log.parses.len() > 0);

    let mut result: Vec<String> = vec![];

    for parse in &tree_sitter_log.parses {
        // there was an error in the block structure; report that.
        for state in &parse.error_states {
            let mut msg = error_message_from_parse_state(
                input_bytes, 
                state, 
                &parse.consumed_tokens,
                filename
            );
            result.append(&mut msg);
        }
    }

    return result;
}

fn error_message_from_parse_state(
    input_bytes: &[u8],
    parse_state: &crate::utils::tree_sitter_log_observer::ProcessMessage,
    consumed_tokens: &[ConsumedToken],
    filename: &str,
) -> Vec<String> {
    // Look up the error entry from the table
    let error_entry = crate::readers::qmd_error_message_table::lookup_error_entry(parse_state);
    
    if let Some(entry) = error_entry {
        // Convert input to string for ariadne
        let input_str = String::from_utf8_lossy(input_bytes);
        
        // Calculate byte offset from row/column
        let byte_offset = calculate_byte_offset(&input_str, parse_state.row, parse_state.column);
        let span = byte_offset..(byte_offset + parse_state.size.max(1));
        
        // Build the ariadne report
        let mut report = Report::build(ReportKind::Error, filename, byte_offset)
            .with_message(&entry.error_info.title)
            .with_label(
                Label::new((filename, span.clone()))
                    .with_message(&entry.error_info.message)
                    .with_color(Color::Red)
            );
        
        // Add notes with their corresponding captures
        for note in entry.error_info.notes {
            // Find the capture that this note refers to
            if let Some(capture) = entry.error_info.captures.iter().find(|c| c.label == note.label) {
                // Find the consumed token that matches this capture
                if let Some(token) = find_matching_token(consumed_tokens, capture) {
                    // Calculate the span for this token
                    let token_byte_offset = calculate_byte_offset(&input_str, token.row, token.column);
                    let token_span = token_byte_offset..(token_byte_offset + token.size.max(1));
                    
                    // Add a label for this note
                    report = report.with_label(
                        Label::new((filename, token_span))
                            .with_message(note.message)
                            .with_color(Color::Blue)
                    );
                }
            }
        }
        
        let report = report.finish();
        
        // Generate the formatted error message
        let mut output = Vec::new();
        report.write(
            (filename, Source::from(&input_str)),
            &mut output
        ).unwrap_or_else(|_| {
            // Fallback to simple format if ariadne fails
            return;
        });
        
        // Convert output to string and split into lines
        let output_str = String::from_utf8_lossy(&output);
        return output_str.lines().map(|s| s.to_string()).collect();
    } else {
        // Fallback for errors not in the table
        return vec![format!(
            "{}:{}:{}: error: unexpected",
            filename,
            parse_state.row + 1,
            parse_state.column + 1,
        )];
    }
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
            // Find the capture that this note refers to
            if let Some(capture) = entry.error_info.captures.iter().find(|c| c.label == note.label) {
                // Find the consumed token that matches this capture
                if let Some(token) = find_matching_token(consumed_tokens, capture) {
                    // Calculate the span for this token
                    let token_byte_offset = calculate_byte_offset(&input_str, token.row, token.column);
                    
                    notes.push(json!({
                        "message": note.message,
                        "location": {
                            "row": token.row + 1,  // Convert to 1-based
                            "column": token.column + 1,  // Convert to 1-based
                            "byte_offset": token_byte_offset,
                            "size": token.size.max(1)
                        }
                    }));
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
    consumed_tokens.iter().find(|token| {
        token.lr_state == capture.lr_state && token.sym == capture.sym
    })
}

fn calculate_byte_offset(input: &str, row: usize, column: usize) -> usize {
    let mut current_row = 0;
    let mut current_col = 0;
    let mut byte_offset = 0;
    
    for (i, ch) in input.char_indices() {
        if current_row == row && current_col == column {
            return i;
        }
        
        if ch == '\n' {
            current_row += 1;
            current_col = 0;
        } else {
            current_col += 1;
        }
        byte_offset = i;
    }
    
    // Return the position even if we're past the end
    byte_offset + 1
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
        for state in &parse.error_states {
            let error_json = json_error_message_from_parse_state(
                input_bytes,
                state,
                &parse.consumed_tokens,
                filename
            );
            json_errors.push(error_json);
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
    assert!(tree_sitter_log.had_errors());
    assert!(tree_sitter_log.parses.len() > 0);

    let mut tokens: Vec<serde_json::Value> = vec![];
    let mut error_states: Vec<serde_json::Value> = vec![];

    for parse in &tree_sitter_log.parses {
        if parse.found_accept && parse.error_states.is_empty() {
            continue;
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
        for state in &parse.error_states {
            error_states.push(serde_json::json!({
                "state": state.state,
                "sym": state.sym,
                "row": state.row,
                "column": state.column,
            }));
        }
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
