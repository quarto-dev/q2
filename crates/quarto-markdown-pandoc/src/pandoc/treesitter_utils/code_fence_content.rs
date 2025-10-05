/*
 * code_fence_content.rs
 *
 * Functions for processing code fence content nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::parse_context::ParseContext;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_code_fence_content(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
    context: &ParseContext,
) -> PandocNativeIntermediate {
    let start = node.range().start_byte;
    let end = node.range().end_byte;

    // This is a code block, we need to extract the content
    // by removing block_continuation markers
    let mut current_location = start;

    let mut content = String::new();
    for (child_node, child) in children {
        if child_node == "block_continuation" {
            let PandocNativeIntermediate::IntermediateUnknown(child_range) = child else {
                panic!(
                    "Expected IntermediateUnknown in block_continuation, got {:?}",
                    child
                )
            };
            let slice_before_continuation =
                &input_bytes[current_location..child_range.start.offset];
            content.push_str(std::str::from_utf8(slice_before_continuation).unwrap());
            current_location = child_range.end.offset;
        }
    }
    // Add the remaining content after the last block_continuation
    if current_location < end {
        let slice_after_continuation = &input_bytes[current_location..end];
        content.push_str(std::str::from_utf8(slice_after_continuation).unwrap());
    }
    PandocNativeIntermediate::IntermediateBaseText(
        content,
        node_source_info_with_context(node, context).range,
    )
}
