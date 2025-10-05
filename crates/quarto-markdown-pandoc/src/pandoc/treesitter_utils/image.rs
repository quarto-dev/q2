/*
 * treesitter-utils.rs
 * Copyright (c) 2025 Posit, PBC
 */

use std::{collections::HashMap, io::Write};

use crate::pandoc::{
    Image, Inline, inline::Target, location::empty_source_info, parse_context::ParseContext,
    treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate,
};

pub fn process_image<T: Write, F>(
    image_buf: &mut T,
    node_text: F,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ParseContext,
) -> PandocNativeIntermediate
where
    F: Fn() -> String,
{
    let mut attr = ("".to_string(), vec![], HashMap::new());
    let mut target: Target = ("".to_string(), "".to_string());
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
            PandocNativeIntermediate::IntermediateAttr(a) => attr = a,
            PandocNativeIntermediate::IntermediateBaseText(text, _) => {
                if node == "link_destination" {
                    target.0 = text; // URL
                } else if node == "link_title" {
                    target.1 = text; // Title
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
        source_info: empty_source_info(),
    }))
}
