/*
 * treesitter-utils.rs
 * Copyright (c) 2025 Posit, PBC
 */

use std::{collections::HashMap, io::Write};

use crate::pandoc::{
    Image, Inline, ast_context::ASTContext, inline::Target,
    location::node_source_info_with_context,
    treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate,
};

pub fn process_image<T: Write, F>(
    node: &tree_sitter::Node,
    image_buf: &mut T,
    node_text: F,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate
where
    F: Fn() -> String,
{
    let mut attr = ("".to_string(), vec![], HashMap::new());
    let mut attr_source = crate::pandoc::attr::AttrSourceInfo::empty();
    let mut target: Target = ("".to_string(), "".to_string());
    let mut target_source = crate::pandoc::attr::TargetSourceInfo::empty();
    let mut content: Vec<Inline> = Vec::new();
    for (node, child) in children {
        if node == "image_description" {
            let PandocNativeIntermediate::IntermediateInlines(inlines) = child else {
                panic!("Expected inlines in image_description, got {:?}", child)
            };
            content.extend(inlines);
            continue;
        }
        match child {
            PandocNativeIntermediate::IntermediateRawFormat(_, _) => {
                // TODO show position of this error
                let _ = writeln!(
                    image_buf,
                    "Raw specifiers are unsupported in images: {}. Will ignore.",
                    node_text()
                );
            }
            PandocNativeIntermediate::IntermediateAttr(a, as_) => {
                attr = a;
                attr_source = as_;
            }
            PandocNativeIntermediate::IntermediateBaseText(text, range) => {
                if node == "link_destination" {
                    target.0 = text; // URL
                    target_source.url = Some(crate::pandoc::source_map_compat::range_to_source_info_with_context(&range, context));
                } else if node == "link_title" {
                    target.1 = text; // Title
                    target_source.title = Some(crate::pandoc::source_map_compat::range_to_source_info_with_context(&range, context));
                } else if node == "language_attribute" {
                    // TODO show position of this error
                    let _ = writeln!(
                        image_buf,
                        "Language specifiers are unsupported in images: {}",
                        node_text()
                    );
                } else {
                    panic!("Unexpected image node: {}", node);
                }
            }
            PandocNativeIntermediate::IntermediateUnknown(_) => {}
            PandocNativeIntermediate::IntermediateInlines(inlines) => content.extend(inlines),
            PandocNativeIntermediate::IntermediateInline(inline) => content.push(inline),
            _ => panic!("Unexpected child in inline_link: {:?}", child),
        }
    }
    PandocNativeIntermediate::IntermediateInline(Inline::Image(Image {
        attr,
        content,
        target,
        source_info: node_source_info_with_context(node, context),
        attr_source,
        target_source,
    }))
}
