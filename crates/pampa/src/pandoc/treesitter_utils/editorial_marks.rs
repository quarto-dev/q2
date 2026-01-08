/*
 * editorial_marks.rs
 *
 * Functions for processing editorial mark nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Delete, EditComment, Highlight, Inline, Inlines, Insert, Space, Str};
use crate::pandoc::location::node_location;
use hashlink::LinkedHashMap;
use once_cell::sync::Lazy;
use regex::Regex;

use super::pandocnativeintermediate::PandocNativeIntermediate;
use super::text_helpers::{
    apply_smart_quotes, extract_delimiter_space_info, wrap_inline_with_delimiter_spaces,
};

macro_rules! process_editorial_mark {
    ($struct_name:ident, $delimiter_name:literal) => {
        paste::paste! {
            pub fn [<process_ $struct_name:lower>](
                node: &tree_sitter::Node,
                children: Vec<(String, PandocNativeIntermediate)>,
                input_bytes: &[u8],
                context: &ASTContext,
            ) -> PandocNativeIntermediate {
                let whitespace_re: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
                let mut attr = ("".to_string(), vec![], LinkedHashMap::new());
                let mut attr_source = crate::pandoc::attr::AttrSourceInfo::empty();
                let mut content: Inlines = vec![];

                // Extract delimiter space information for leading/trailing Space injection
                let space_info = extract_delimiter_space_info(
                    &children,
                    $delimiter_name,
                    input_bytes,
                    node_location(node),
                );

                for (_node_name, child) in children {
                    match child {
                        PandocNativeIntermediate::IntermediateAttr(a, as_) => {
                            attr = a;
                            attr_source = as_;
                        }
                        PandocNativeIntermediate::IntermediateInline(inline) => {
                            content.push(inline);
                        }
                        PandocNativeIntermediate::IntermediateInlines(mut inlines) => {
                            content.append(&mut inlines);
                        }
                        PandocNativeIntermediate::IntermediateBaseText(text, range) => {
                            if let Some(_) = whitespace_re.find(&text) {
                                content.push(Inline::Space(Space {
                                    source_info: quarto_source_map::SourceInfo::from_range(context.current_file_id(), range),
                                }))
                            } else {
                                content.push(Inline::Str(Str {
                                    text: apply_smart_quotes(text),
                                    source_info: quarto_source_map::SourceInfo::from_range(context.current_file_id(), range),
                                }))
                            }
                        }
                        PandocNativeIntermediate::IntermediateUnknown(_) => {
                            // Skip unknown nodes (delimiters, etc.)
                        }
                        _ => {
                            // Skip unexpected intermediates (shouldn't happen in practice)
                        }
                    }
                }

                // Create the editorial mark inline element with adjusted source info
                let adjusted_source_info = quarto_source_map::SourceInfo::from_range(
                    context.current_file_id(),
                    space_info.adjusted_range.clone(),
                );
                let inline = Inline::$struct_name($struct_name {
                    attr,
                    content,
                    source_info: adjusted_source_info,
                    attr_source,
                });

                // Wrap with Space nodes as needed (returns IntermediateInlines)
                wrap_inline_with_delimiter_spaces(inline, &space_info, context)
            }
        }
    };
}

process_editorial_mark!(Insert, "insert_delimiter");
process_editorial_mark!(Delete, "delete_delimiter");
process_editorial_mark!(Highlight, "highlight_delimiter");
process_editorial_mark!(EditComment, "edit_comment_delimiter");
