/*
 * qmd_error_messages.rs
 * Copyright (c) 2025 Posit, PBC
 */

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
            let mut msg = error_message_from_parse_state(input_bytes, state, filename);
            result.append(&mut msg);
        }
    }

    return result;
}

fn error_message_from_parse_state(
    _input_bytes: &[u8],
    parse_state: &crate::utils::tree_sitter_log_observer::ProcessMessage,
    filename: &str,
) -> Vec<String> {
    // let index = crate::utils::text::build_row_column_index(&String::from_utf8_lossy(input_bytes));
    // let offset =
    //     crate::utils::text::row_column_to_byte_offset(&index, parse_state.row, parse_state.column)
    //         .unwrap_or(0);

    let error_msg = crate::readers::qmd_error_message_table::lookup_error_message(parse_state)
        .unwrap_or("unexpected");

    return vec![format!(
        "{}:{}:{}: error: {}",
        filename,
        parse_state.row + 1,
        parse_state.column + 1,
        error_msg,
    )];
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
