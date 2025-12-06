/*
 * list_marker.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a list marker, extracting the marker number
pub fn process_list_marker(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // we need to extract the marker number
    let marker_text = node
        .utf8_text(input_bytes)
        .unwrap()
        // we trim both ends instead of just trim_end()
        // because the lexer might hand us a marker with tabs at the beginning,
        // as a result of weird mixed-spaces-and-tabs cases like "> \t1."
        .trim();

    // Check if this is an example list marker (@)
    if marker_text == "(@)" {
        // For example lists, we use 1 as the starting number
        // The actual numbering will be handled in postprocessing
        let source_info = node_source_info_with_context(node, context);
        let range =
            crate::pandoc::location::source_info_to_qsm_range_or_fallback(&source_info, context);
        return PandocNativeIntermediate::IntermediateOrderedListMarker(1, range);
    }

    let marker_text = marker_text
        .trim_end_matches('.')
        .trim_end_matches(')')
        .to_string();
    let marker_number: usize = marker_text
        .parse()
        .unwrap_or_else(|_| panic!("Invalid list marker number: {}", marker_text));
    let source_info = node_source_info_with_context(node, context);
    let range =
        crate::pandoc::location::source_info_to_qsm_range_or_fallback(&source_info, context);
    PandocNativeIntermediate::IntermediateOrderedListMarker(marker_number, range)
}
