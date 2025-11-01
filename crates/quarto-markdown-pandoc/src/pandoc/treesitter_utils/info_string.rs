/*
 * info_string.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::AttrSourceInfo;
use crate::pandoc::location::node_location;
use crate::pandoc::source_map_compat::range_to_source_info_with_context;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;
use std::collections::HashMap;
use tree_sitter::Node;

/// Process info_string to extract language as an attribute
/// In the new grammar, info_string is a leaf node containing the language name directly
pub fn process_info_string(
    node: &Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // Extract the language name from the node text
    let lang_text = node.utf8_text(input_bytes).unwrap().to_string();

    // Track source location for the language specifier
    let range = node_location(node);
    let lang_source = range_to_source_info_with_context(&range, context);

    let mut attr_source = AttrSourceInfo::empty();
    attr_source.classes.push(Some(lang_source));

    PandocNativeIntermediate::IntermediateAttr(
        ("".to_string(), vec![lang_text], HashMap::new()),
        attr_source,
    )
}
