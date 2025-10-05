/*
 * link_title.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::location::node_location;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a link title by removing surrounding quotes
pub fn process_link_title(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
) -> PandocNativeIntermediate {
    let title = node.utf8_text(input_bytes).unwrap().to_string();
    let title = title[1..title.len() - 1].to_string();
    PandocNativeIntermediate::IntermediateBaseText(title, node_location(node))
}
