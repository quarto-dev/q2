/*
 * numeric_character_reference.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::parse_context::ParseContext;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process numeric character references to their corresponding characters
/// Converts &#x0040; => @, &#64; => @, etc
pub fn process_numeric_character_reference(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ParseContext,
) -> PandocNativeIntermediate {
    let text = node.utf8_text(input_bytes).unwrap().to_string();
    let char_value = if text.starts_with("&#x") || text.starts_with("&#X") {
        // Hexadecimal reference
        let hex_str = &text[3..text.len() - 1];
        u32::from_str_radix(hex_str, 16).ok()
    } else if text.starts_with("&#") {
        // Decimal reference
        let dec_str = &text[2..text.len() - 1];
        dec_str.parse::<u32>().ok()
    } else {
        None
    };

    let result_text = match char_value.and_then(char::from_u32) {
        Some(ch) => ch.to_string(),
        None => text, // If we can't parse it, return the original text
    };

    PandocNativeIntermediate::IntermediateBaseText(
        result_text,
        node_source_info_with_context(node, context).range,
    )
}
