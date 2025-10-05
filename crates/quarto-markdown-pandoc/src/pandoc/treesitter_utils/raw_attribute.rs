/*
 * raw_attribute.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::parse_context::ParseContext;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process raw_attribute to extract the format specifier
pub fn process_raw_attribute(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ParseContext,
) -> PandocNativeIntermediate {
    let range = node_source_info_with_context(node, context).range;
    for (_, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateBaseText(raw, _) => {
                return PandocNativeIntermediate::IntermediateRawFormat(raw, range);
            }
            _ => {}
        }
    }
    panic!("Expected raw_attribute to have a format, but found none");
}
