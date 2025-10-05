/*
 * link_title.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::parse_context::ParseContext;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a link title by removing surrounding quotes
pub fn process_link_title(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ParseContext,
) -> PandocNativeIntermediate {
    let title = node.utf8_text(input_bytes).unwrap().to_string();
    let title = title[1..title.len() - 1].to_string();
    PandocNativeIntermediate::IntermediateBaseText(
        title,
        node_source_info_with_context(node, context).range,
    )
}
