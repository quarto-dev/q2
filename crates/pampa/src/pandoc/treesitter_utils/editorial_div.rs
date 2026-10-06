/*
 * editorial_div.rs
 *
 * Functions for processing block-level editorial mark nodes
 * (`::: ++`, `::: --`, `::: >>`, `::: !!`) in the tree-sitter AST.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::{Attr, AttrSourceInfo};
use crate::pandoc::block::{Block, Div};
use crate::pandoc::location::node_source_info_with_context;
use hashlink::LinkedHashMap;

use super::pandocnativeintermediate::PandocNativeIntermediate;

/// The class a block editorial mark carries, keyed by its marker's
/// delimiter node name. These are the classes the inline marks desugar to
/// in postprocess (`with_insert` & co.), so every consumer of the inline
/// spans (CSS, comment extraction, the qmd writer) sees the same shape.
pub fn editorial_class_for_delimiter(delimiter: &str) -> Option<&'static str> {
    match delimiter {
        "insert_delimiter" => Some("quarto-insert"),
        "delete_delimiter" => Some("quarto-delete"),
        "edit_comment_delimiter" => Some("quarto-edit-comment"),
        "highlight_delimiter" => Some("quarto-highlight"),
        _ => None,
    }
}

/// Lower an `editorial_div` straight to a `Div` whose first class is the
/// mark's `quarto-*` class, followed by the user's own attributes. There is
/// no dedicated block AST node: the Div is what the inline marks' spans
/// are, one level up.
pub fn process_editorial_div(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut marker: Option<(&'static str, quarto_source_map::SourceInfo)> = None;
    let mut attr: Attr = (String::new(), vec![], LinkedHashMap::new());
    let mut attr_source = AttrSourceInfo::empty();
    let mut content: Vec<Block> = Vec::new();

    for (node_name, child) in children {
        if let Some(class) = editorial_class_for_delimiter(&node_name) {
            if let PandocNativeIntermediate::IntermediateUnknown(range) = child {
                marker = Some((
                    class,
                    quarto_source_map::SourceInfo::from_range(context.current_file_id(), range),
                ));
            }
            continue;
        }
        match child {
            PandocNativeIntermediate::IntermediateAttr(a, as_, _) => {
                attr = a;
                attr_source = as_;
            }
            PandocNativeIntermediate::IntermediateBlock(block) => {
                content.push(block);
            }
            PandocNativeIntermediate::IntermediateSection(blocks) => {
                content.extend(blocks);
            }
            _ => {
                // block_continuation markers and other delimiters
            }
        }
    }

    let (class, marker_source) =
        marker.expect("editorial_div always has a marker child (grammar invariant)");

    // The mark's class goes first; a user-written duplicate of it (as in
    // `::: -- {.quarto-delete}`) is dropped so the class appears once.
    // `attr_source.classes` stays aligned with `attr.1`: the synthesized
    // class points at the marker bytes that produced it.
    let mut classes = vec![class.to_string()];
    let mut class_sources = vec![Some(marker_source)];
    let user_class_sources = std::mem::take(&mut attr_source.classes);
    for (i, user_class) in std::mem::take(&mut attr.1).into_iter().enumerate() {
        if user_class == class {
            continue;
        }
        classes.push(user_class);
        class_sources.push(user_class_sources.get(i).cloned().flatten());
    }
    attr.1 = classes;
    attr_source.classes = class_sources;

    PandocNativeIntermediate::IntermediateBlock(Block::Div(Div {
        attr,
        content,
        source_info: node_source_info_with_context(node, context),
        attr_source,
    }))
}
