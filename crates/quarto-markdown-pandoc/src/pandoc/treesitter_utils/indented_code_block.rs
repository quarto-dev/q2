/*
 * indented_code_block.rs
 *
 * Functions for processing indented code block nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::empty_attr;
use crate::pandoc::block::{Block, CodeBlock};
use crate::pandoc::location::node_source_info_with_context;
use regex::Regex;

use super::pandocnativeintermediate::PandocNativeIntermediate;

pub fn process_indented_code_block(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
    indent_re: &Regex,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut content: String = String::new();
    let outer_range = node_source_info_with_context(node, context);
    // first, find the beginning of the contents in the node itself
    let outer_string = node.utf8_text(input_bytes).unwrap().to_string();
    let mut start_offset = indent_re.find(&outer_string).map_or(0, |m| m.end());

    for (node, children) in children {
        if node == "block_continuation" {
            // append all content up to the beginning of this continuation
            match children {
                PandocNativeIntermediate::IntermediateUnknown(range) => {
                    // Calculate the relative offset of the continuation within outer_string
                    let continuation_start = range
                        .start
                        .offset
                        .saturating_sub(outer_range.start_offset());
                    let continuation_end = range
                        .end
                        .offset
                        .saturating_sub(outer_range.start_offset());

                    // Append content before this continuation
                    if continuation_start > start_offset && continuation_start <= outer_string.len()
                    {
                        content.push_str(&outer_string[start_offset..continuation_start]);
                    }

                    // Update start_offset to after this continuation
                    start_offset = continuation_end.min(outer_string.len());
                }
                _ => panic!("Unexpected {:?} inside indented_code_block", children),
            }
        }
    }
    // append the remaining content after the last continuation
    content.push_str(&outer_string[start_offset..]);
    // TODO this will require careful encoding of the source map when we get to that point
    PandocNativeIntermediate::IntermediateBlock(Block::CodeBlock(CodeBlock {
        attr: empty_attr(),
        text: content.trim_end().to_string(),
        source_info: outer_range,
    }))
}
