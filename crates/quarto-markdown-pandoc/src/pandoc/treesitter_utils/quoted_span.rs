/*
 * quoted_span.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::inline::{Inline, QuoteType, Quoted};
use crate::pandoc::location::node_source_info;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a quoted span (single or double quotes)
pub fn process_quoted_span<F>(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    native_inline: F,
) -> PandocNativeIntermediate
where
    F: FnMut((String, PandocNativeIntermediate)) -> Inline,
{
    let mut quote_type = QuoteType::SingleQuote;
    let inlines: Vec<_> = children
        .into_iter()
        .filter(|(node, intermediate)| {
            if node == "single_quoted_span_delimiter" {
                quote_type = QuoteType::SingleQuote;
                false // skip the opening delimiter
            } else if node == "double_quoted_span_delimiter" {
                quote_type = QuoteType::DoubleQuote;
                false // skip the opening delimiter
            } else {
                match intermediate {
                    PandocNativeIntermediate::IntermediateInline(_) => true,
                    PandocNativeIntermediate::IntermediateBaseText(_, _) => true,
                    _ => false,
                }
            }
        })
        .map(native_inline)
        .collect();
    PandocNativeIntermediate::IntermediateInline(Inline::Quoted(Quoted {
        quote_type,
        content: inlines,
        source_info: node_source_info(node),
    }))
}
