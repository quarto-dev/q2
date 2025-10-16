/*
 * editorial_marks.rs
 *
 * Functions for processing editorial mark nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Delete, EditComment, Highlight, Inline, Inlines, Insert, Space, Str};
use crate::pandoc::location::{SourceInfo, node_source_info_with_context};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::io::Write;

use super::pandocnativeintermediate::PandocNativeIntermediate;
use super::text_helpers::apply_smart_quotes;

macro_rules! process_editorial_mark {
    ($struct_name:ident) => {
        paste::paste! {
            pub fn [<process_ $struct_name:lower>]<T: Write>(
                buf: &mut T,
                node: &tree_sitter::Node,
                children: Vec<(String, PandocNativeIntermediate)>,
                context: &ASTContext,
            ) -> PandocNativeIntermediate {
                let whitespace_re: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
                let mut attr = ("".to_string(), vec![], HashMap::new());
                let mut content: Inlines = vec![];

                for (_node_name, child) in children {
                    match child {
                        PandocNativeIntermediate::IntermediateAttr(a) => {
                            attr = a;
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
                                    source_info: SourceInfo::new(
                                        if context.filenames.is_empty() {
                                            None
                                        } else {
                                            Some(0)
                                        },
                                        range,
                                    ),
                                }))
                            } else {
                                content.push(Inline::Str(Str {
                                    text: apply_smart_quotes(text),
                                    source_info: SourceInfo::new(
                                        if context.filenames.is_empty() {
                                            None
                                        } else {
                                            Some(0)
                                        },
                                        range,
                                    ),
                                }))
                            }
                        }
                        PandocNativeIntermediate::IntermediateUnknown(_) => {
                            // Skip unknown nodes (delimiters, etc.)
                        }
                        _ => {
                            writeln!(
                                buf,
                                "Warning: Unexpected node type in {}: {:?}",
                                stringify!($struct_name:lower),
                                _node_name
                            )
                            .unwrap();
                        }
                    }
                }

                PandocNativeIntermediate::IntermediateInline(Inline::$struct_name($struct_name {
                    attr,
                    content,
                    source_info: node_source_info_with_context(node, context),
                }))
            }
        }
    };
}

process_editorial_mark!(Insert);
process_editorial_mark!(Delete);
process_editorial_mark!(Highlight);
process_editorial_mark!(EditComment);
