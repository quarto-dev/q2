/*
 * raw_attribute.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process raw_attribute to extract the format specifier
pub fn process_raw_attribute(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let source_info = node_source_info_with_context(node, context);
    let range =
        crate::pandoc::location::source_info_to_qsm_range_or_fallback(&source_info, context);
    for (_, child) in children {
        if let PandocNativeIntermediate::IntermediateBaseText(raw, _) = child {
            return PandocNativeIntermediate::IntermediateRawFormat(raw, range);
        }
    }
    panic!("Expected raw_attribute to have a format, but found none");
}
