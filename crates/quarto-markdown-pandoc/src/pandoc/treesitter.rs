/*
 * treesitter.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::treesitter_utils;
use crate::pandoc::treesitter_utils::atx_heading::process_atx_heading;
use crate::pandoc::treesitter_utils::block_quote::process_block_quote;
use crate::pandoc::treesitter_utils::caption::process_caption;
use crate::pandoc::treesitter_utils::citation::process_citation;
use crate::pandoc::treesitter_utils::code_fence_content::process_code_fence_content;
use crate::pandoc::treesitter_utils::code_span_helpers::process_pandoc_code_span;
use crate::pandoc::treesitter_utils::commonmark_attribute::process_commonmark_attribute;
use crate::pandoc::treesitter_utils::document::process_document;
use crate::pandoc::treesitter_utils::editorial_marks::{
    process_delete, process_editcomment, process_highlight, process_insert,
};
use crate::pandoc::treesitter_utils::fenced_code_block::process_fenced_code_block;
use crate::pandoc::treesitter_utils::fenced_div_block::process_fenced_div_block;
use crate::pandoc::treesitter_utils::info_string::process_info_string;
use crate::pandoc::treesitter_utils::language_attribute::process_language_attribute;
use crate::pandoc::treesitter_utils::list_marker::process_list_marker;
use crate::pandoc::treesitter_utils::note_definition_fenced_block::process_note_definition_fenced_block;
use crate::pandoc::treesitter_utils::note_definition_para::process_note_definition_para;
use crate::pandoc::treesitter_utils::paragraph::process_paragraph;
use crate::pandoc::treesitter_utils::pipe_table::{
    process_pipe_table, process_pipe_table_cell, process_pipe_table_delimiter_cell,
    process_pipe_table_delimiter_row, process_pipe_table_header_or_row,
};
use crate::pandoc::treesitter_utils::postprocess::{merge_strs, postprocess};
use crate::pandoc::treesitter_utils::quote_helpers::process_quoted;
use crate::pandoc::treesitter_utils::raw_attribute::process_raw_attribute;
use crate::pandoc::treesitter_utils::section::process_section;
use crate::pandoc::treesitter_utils::shortcode::{
    process_shortcode, process_shortcode_boolean, process_shortcode_keyword_param,
    process_shortcode_number, process_shortcode_string, process_shortcode_string_arg,
};
use crate::pandoc::treesitter_utils::span_link_helpers::{
    process_content_node, process_pandoc_image, process_pandoc_span, process_target,
};
use crate::pandoc::treesitter_utils::text_helpers::*;
use crate::pandoc::treesitter_utils::thematic_break::process_thematic_break;

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::AttrSourceInfo;
use crate::pandoc::block::{Block, Blocks, BulletList, OrderedList, Paragraph, Plain, RawBlock};
use crate::pandoc::inline::{
    Emph, Inline, Math, MathType, Note, NoteReference, QuoteType, RawInline, SoftBreak, Space, Str,
    Strikeout, Strong, Subscript, Superscript,
};
use crate::pandoc::list::{ListAttributes, ListNumberDelim, ListNumberStyle};
use crate::pandoc::location::{node_location, node_source_info_with_context};
use crate::pandoc::pandoc::Pandoc;
use core::panic;
use once_cell::sync::Lazy;
use regex::Regex;
use std::io::Write;

use crate::traversals::bottomup_traverse_concrete_tree;

use treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

fn get_block_source_info(block: &Block) -> &quarto_source_map::SourceInfo {
    match block {
        Block::Plain(b) => &b.source_info,
        Block::Paragraph(b) => &b.source_info,
        Block::LineBlock(b) => &b.source_info,
        Block::CodeBlock(b) => &b.source_info,
        Block::RawBlock(b) => &b.source_info,
        Block::BlockQuote(b) => &b.source_info,
        Block::OrderedList(b) => &b.source_info,
        Block::BulletList(b) => &b.source_info,
        Block::DefinitionList(b) => &b.source_info,
        Block::Header(b) => &b.source_info,
        Block::HorizontalRule(b) => &b.source_info,
        Block::Table(b) => &b.source_info,
        Block::Figure(b) => &b.source_info,
        Block::Div(b) => &b.source_info,
        Block::BlockMetadata(b) => &b.source_info,
        Block::NoteDefinitionPara(b) => &b.source_info,
        Block::NoteDefinitionFencedBlock(b) => &b.source_info,
        Block::CaptionBlock(b) => &b.source_info,
    }
}

fn process_list(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // a list is loose if it has at least one loose item
    // an item is loose if
    //   - it has more than one paragraph in the list
    //   - it is a single paragraph with space between it and the next
    //     beginning of list item. There must be a next item for this to be true
    //     but the next item might not itself be a paragraph.

    let mut has_loose_item = false;
    let mut last_para_end_row: Option<usize> = None;
    let mut last_item_end_row: Option<usize> = None;
    let mut list_items: Vec<Blocks> = Vec::new();
    let mut is_ordered_list: Option<ListAttributes> = None;

    for (node, child) in children {
        if node == "block_continuation" {
            // this is a marker node, we don't need to do anything with it
            continue;
        }
        if node == "list_marker_parenthesis"
            || node == "list_marker_dot"
            || node == "list_marker_example"
        {
            // this is an ordered list, so we need to set the flag
            let PandocNativeIntermediate::IntermediateOrderedListMarker(marker_number, _) = child
            else {
                panic!("Expected OrderedListMarker in list, got {:?}", child);
            };

            is_ordered_list = Some((
                marker_number,
                match node.as_str() {
                    "list_marker_example" => ListNumberStyle::Example,
                    _ => ListNumberStyle::Decimal,
                },
                match node.as_str() {
                    "list_marker_parenthesis" => ListNumberDelim::OneParen,
                    "list_marker_dot" => ListNumberDelim::Period,
                    "list_marker_example" => ListNumberDelim::TwoParens,
                    _ => panic!("Unexpected list marker node: {}", node),
                },
            ));
        }

        if node != "list_item" {
            panic!("Expected list_item in list, got {}", node);
        }
        let PandocNativeIntermediate::IntermediateListItem(blocks, child_range, ordered_list) =
            child
        else {
            panic!("Expected Blocks in list_item, got {:?}", child);
        };
        if is_ordered_list == None {
            match ordered_list {
                attr @ Some(_) => is_ordered_list = attr,
                _ => {}
            }
        }

        // is the last item loose? Check the last paragraph end row
        if let Some(last_para_end) = last_para_end_row {
            if last_para_end != child_range.start.row {
                // if the last paragraph ends on a different line than the current item starts,
                // then the last item was loose, mark it
                has_loose_item = true;
            }
        }

        // Check if there's a blank line between the last item and this item
        if let Some(last_end) = last_item_end_row {
            if child_range.start.row > last_end {
                // There's at least one blank line between items
                has_loose_item = true;
            }
        }

        // is this item definitely loose?
        if blocks
            .iter()
            .filter(|block| {
                if let Block::Paragraph(_) = block {
                    true
                } else {
                    false
                }
            })
            .count()
            > 1
        {
            has_loose_item = true;

            // technically, we don't need to worry about
            // last paragraph end row after setting has_loose_item,
            // but we do it in case we want to use it later
            last_para_end_row = None;
            last_item_end_row = blocks.last().and_then(|b| {
                let source_info = get_block_source_info(b);
                source_info
                    .map_offset(source_info.length(), &context.source_context)
                    .map(|mapped| mapped.location.row)
            });
            list_items.push(blocks);
            continue;
        }

        // is this item possibly loose?
        if blocks.len() == 1 {
            if let Some(Block::Paragraph(para)) = blocks.first() {
                // yes, so store the end row and wait to finish the check on
                // next item
                last_para_end_row = para
                    .source_info
                    .map_offset(para.source_info.length(), &context.source_context)
                    .map(|mapped| mapped.location.row);
            } else {
                // if the first block is not a paragraph, it's not loose
                last_para_end_row = None;
            }
        } else {
            // if the item has multiple blocks (but not multiple paragraphs,
            // which would have been caught above), we need to reset the
            // last_para_end_row since this item can't participate in loose detection
            last_para_end_row = None;
        }
        last_item_end_row = blocks.last().and_then(|b| {
            let source_info = get_block_source_info(b);
            source_info
                .map_offset(source_info.length(), &context.source_context)
                .map(|mapped| mapped.location.row)
        });
        list_items.push(blocks);
    }

    let content = if has_loose_item {
        // the AST representation of a loose bullet list is
        // the same as what we've been building, so just return it
        list_items
    } else {
        // turn list into tight list by replacing eligible Paragraph nodes
        // Plain nodes.
        list_items
            .into_iter()
            .map(|mut blocks| {
                if blocks.is_empty() {
                    return blocks;
                }
                // Convert the first block if it's a Paragraph
                let first = blocks.remove(0);
                let Block::Paragraph(Paragraph {
                    content,
                    source_info,
                }) = first
                else {
                    blocks.insert(0, first);
                    return blocks;
                };
                let mut result = vec![Block::Plain(Plain {
                    content: content,
                    source_info: source_info,
                })];
                result.extend(blocks);
                result
            })
            .collect()
    };

    match is_ordered_list {
        Some(mut attr) => {
            // For example lists, use and update the global counter
            if attr.1 == ListNumberStyle::Example {
                let start_num = context.example_list_counter.get();
                attr.0 = start_num;
                // Increment counter by the number of items in this list
                context.example_list_counter.set(start_num + content.len());
            }
            PandocNativeIntermediate::IntermediateBlock(Block::OrderedList(OrderedList {
                attr,
                content,
                source_info: node_source_info_with_context(node, context),
            }))
        }
        None => PandocNativeIntermediate::IntermediateBlock(Block::BulletList(BulletList {
            content,
            source_info: node_source_info_with_context(node, context),
        })),
    }
}

fn process_list_item(
    list_item_node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut list_attr: Option<ListAttributes> = None;
    let children = children
        .into_iter()
        .filter_map(|(node, child)| {
            if node == "list_marker_dot"
                || node == "list_marker_parenthesis"
                || node == "list_marker_example"
            {
                // this is an ordered list, so we need to set the flag
                let PandocNativeIntermediate::IntermediateOrderedListMarker(marker_number, _) =
                    child
                else {
                    panic!("Expected OrderedListMarker in list_item, got {:?}", child);
                };
                list_attr = Some((
                    marker_number,
                    match node.as_str() {
                        "list_marker_example" => ListNumberStyle::Example,
                        _ => ListNumberStyle::Decimal,
                    },
                    match node.as_str() {
                        "list_marker_parenthesis" => ListNumberDelim::OneParen,
                        "list_marker_dot" => ListNumberDelim::Period,
                        "list_marker_example" => ListNumberDelim::TwoParens,
                        _ => panic!("Unexpected list marker node: {}", node),
                    },
                ));
                return None; // skip the marker node
            }
            match child {
                PandocNativeIntermediate::IntermediateBlock(block) => Some(block),
                PandocNativeIntermediate::IntermediateMetadataString(text, _range) => {
                    // for now we assume it's metadata and emit it as a rawblock
                    Some(Block::RawBlock(RawBlock {
                        format: "quarto_minus_metadata".to_string(),
                        text,
                        source_info: node_source_info_with_context(list_item_node, context),
                    }))
                }
                _ => None,
            }
        })
        .collect();
    PandocNativeIntermediate::IntermediateListItem(
        children,
        node_location(list_item_node),
        list_attr,
    )
}

// Standalone function to process intermediate inline elements into Inline objects
fn process_native_inline<T: Write>(
    node_name: String,
    child: PandocNativeIntermediate,
    whitespace_re: &Regex,
    inline_buf: &mut T,
    node_text_fn: impl Fn() -> String,
    node_source_info_fn: impl Fn() -> quarto_source_map::SourceInfo,
    context: &ASTContext,
) -> Inline {
    match child {
        PandocNativeIntermediate::IntermediateInline(inline) => inline,
        PandocNativeIntermediate::IntermediateBaseText(text, range) => {
            if let Some(_) = whitespace_re.find(&text) {
                Inline::Space(Space {
                    source_info: quarto_source_map::SourceInfo::from_range(
                        context.current_file_id(),
                        range,
                    ),
                })
            } else {
                Inline::Str(Str {
                    text: apply_smart_quotes(text),
                    source_info: quarto_source_map::SourceInfo::from_range(
                        context.current_file_id(),
                        range,
                    ),
                })
            }
        }
        // as a special inline, we need to allow commonmark attributes
        // to show up in the document, so we can appropriately attach attributes
        // to headings and tables (through their captions) as needed
        //
        // see tests/cursed/002.qmd for why this cannot be parsed directly in
        // the block grammar.
        PandocNativeIntermediate::IntermediateAttr(attr, attr_source) => {
            Inline::Attr(attr, attr_source)
        }
        PandocNativeIntermediate::IntermediateUnknown(range) => {
            writeln!(
                inline_buf,
                "Ignoring unexpected unknown node in native inline at ({}:{}): {:?}.",
                range.start.row + 1,
                range.start.column + 1,
                node_name
            )
            .unwrap();
            Inline::RawInline(RawInline {
                format: "quarto-internal-leftover".to_string(),
                text: node_text_fn(),
                source_info: node_source_info_fn(),
            })
        }
        other => {
            writeln!(
                inline_buf,
                "Ignoring unexpected unknown node in native_inline {:?}.",
                other
            )
            .unwrap();
            Inline::RawInline(RawInline {
                format: "quarto-internal-leftover".to_string(),
                text: node_text_fn(),
                source_info: node_source_info_fn(),
            })
        }
    }
}

// Standalone function to process a collection of children into a vector of Inline objects
fn process_native_inlines<T: Write>(
    children: Vec<(String, PandocNativeIntermediate)>,
    whitespace_re: &Regex,
    inlines_buf: &mut T,
    context: &ASTContext,
) -> Vec<Inline> {
    let mut inlines: Vec<Inline> = Vec::new();
    for (_, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateInline(inline) => inlines.push(inline),
            PandocNativeIntermediate::IntermediateInlines(inner_inlines) => {
                inlines.extend(inner_inlines)
            }
            PandocNativeIntermediate::IntermediateBaseText(text, range) => {
                if let Some(_) = whitespace_re.find(&text) {
                    inlines.push(Inline::Space(Space {
                        source_info: quarto_source_map::SourceInfo::from_range(
                            context.current_file_id(),
                            range,
                        ),
                    }))
                } else {
                    inlines.push(Inline::Str(Str {
                        text: apply_smart_quotes(text),
                        source_info: quarto_source_map::SourceInfo::from_range(
                            context.current_file_id(),
                            range,
                        ),
                    }))
                }
            }
            other => {
                writeln!(
                    inlines_buf,
                    "Ignoring unexpected unknown node in native_inlines {:?}.",
                    other
                )
                .unwrap();
            }
        }
    }
    inlines
}

fn native_visitor<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    // TODO What sounded like a good idea with two buffers
    // is becoming annoying now...
    let mut inline_buf = Vec::<u8>::new();
    let link_buf = Vec::<u8>::new();
    let image_buf = Vec::<u8>::new();

    let whitespace_re: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
    let node_text = || node.utf8_text(input_bytes).unwrap().to_string();

    let node_source_info_fn = || node_source_info_with_context(node, context);
    let native_inline = |(node_name, child)| {
        process_native_inline(
            node_name,
            child,
            &whitespace_re,
            &mut inline_buf,
            &node_text,
            &node_source_info_fn,
            context,
        )
    };

    let result = match node.kind() {
        "fenced_div_note_id" => create_base_text_from_node_text(node, input_bytes),
        "document" => process_document(node, children, context),
        "section" => process_section(node, children, context),
        "pandoc_paragraph" => process_paragraph(node, children, context),
        "atx_heading" => process_atx_heading(buf, node, children, context),
        "atx_h1_marker" | "atx_h2_marker" | "atx_h3_marker" | "atx_h4_marker" | "atx_h5_marker"
        | "atx_h6_marker" => {
            // Marker nodes - these are processed by the parent atx_heading node
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "$" | "$$" => {
            // Math delimiters - these are processed by parent math nodes
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "pandoc_math" => {
            // Extract math content (text between $ delimiters)
            // Node structure: '$' content '$'
            // Get the full text and strip the delimiters
            let full_text = node.utf8_text(input_bytes).unwrap();
            let content = &full_text[1..full_text.len() - 1]; // Strip leading and trailing $

            PandocNativeIntermediate::IntermediateInline(Inline::Math(Math {
                math_type: MathType::InlineMath,
                text: content.to_string(),
                source_info: node_source_info_with_context(node, context),
            }))
        }
        "pandoc_display_math" => {
            // Extract display math content (text between $$ delimiters)
            // Node structure: '$$' content '$$'
            // Get the full text and strip the delimiters
            let full_text = node.utf8_text(input_bytes).unwrap();
            let content = &full_text[2..full_text.len() - 2]; // Strip leading and trailing $$

            PandocNativeIntermediate::IntermediateInline(Inline::Math(Math {
                math_type: MathType::DisplayMath,
                text: content.to_string(),
                source_info: node_source_info_with_context(node, context),
            }))
        }
        "pandoc_str" => {
            let text = node.utf8_text(input_bytes).unwrap().to_string();
            // Process backslash escapes first, then apply smart quotes
            let text = process_backslash_escapes(text);
            PandocNativeIntermediate::IntermediateInline(Inline::Str(Str {
                text: apply_smart_quotes(text),
                source_info: node_source_info_with_context(node, context),
            }))
        }
        "pandoc_space" => PandocNativeIntermediate::IntermediateInline(Inline::Space(Space {
            source_info: node_source_info_with_context(node, context),
        })),
        "pandoc_soft_break" => {
            PandocNativeIntermediate::IntermediateInline(Inline::SoftBreak(SoftBreak {
                source_info: node_source_info_with_context(node, context),
            }))
        }
        "emphasis_delimiter" => {
            // This is a marker node, we don't need to process it
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "pandoc_emph" => process_inline_with_delimiter_spaces(
            node,
            children,
            "emphasis_delimiter",
            input_bytes,
            context,
            native_inline,
            |inlines| {
                Inline::Emph(Emph {
                    content: inlines,
                    source_info: node_source_info_with_context(node, context),
                })
            },
        ),
        "strong_emphasis_delimiter" => {
            // This is a marker node, we don't need to process it
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "pandoc_strong" => process_inline_with_delimiter_spaces(
            node,
            children,
            "strong_emphasis_delimiter",
            input_bytes,
            context,
            native_inline,
            |inlines| {
                Inline::Strong(Strong {
                    content: inlines,
                    source_info: node_source_info_with_context(node, context),
                })
            },
        ),
        "strikeout_delimiter" => PandocNativeIntermediate::IntermediateUnknown(node_location(node)),
        "pandoc_strikeout" => process_inline_with_delimiter_spaces(
            node,
            children,
            "strikeout_delimiter",
            input_bytes,
            context,
            native_inline,
            |inlines| {
                Inline::Strikeout(Strikeout {
                    content: inlines,
                    source_info: node_source_info_with_context(node, context),
                })
            },
        ),
        "superscript_delimiter" => {
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "pandoc_superscript" => process_inline_with_delimiter_spaces(
            node,
            children,
            "superscript_delimiter",
            input_bytes,
            context,
            native_inline,
            |inlines| {
                Inline::Superscript(Superscript {
                    content: inlines,
                    source_info: node_source_info_with_context(node, context),
                })
            },
        ),
        "subscript_delimiter" => PandocNativeIntermediate::IntermediateUnknown(node_location(node)),
        "pandoc_subscript" => process_inline_with_delimiter_spaces(
            node,
            children,
            "subscript_delimiter",
            input_bytes,
            context,
            native_inline,
            |inlines| {
                Inline::Subscript(Subscript {
                    content: inlines,
                    source_info: node_source_info_with_context(node, context),
                })
            },
        ),
        // Editorial marks
        "insert_delimiter" => PandocNativeIntermediate::IntermediateUnknown(node_location(node)),
        "insert" => process_insert(buf, node, children, context),
        "delete_delimiter" => PandocNativeIntermediate::IntermediateUnknown(node_location(node)),
        "delete" => process_delete(buf, node, children, context),
        "highlight_delimiter" => PandocNativeIntermediate::IntermediateUnknown(node_location(node)),
        "highlight" => process_highlight(buf, node, children, context),
        "edit_comment_delimiter" => {
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "edit_comment" => process_editcomment(buf, node, children, context),
        // Shortcode nodes
        "shortcode_delimiter" => PandocNativeIntermediate::IntermediateUnknown(node_location(node)),
        "shortcode_name" => process_shortcode_string_arg(node, input_bytes, context),
        "shortcode_naked_string" => process_shortcode_string_arg(node, input_bytes, context),
        "shortcode_string" => {
            // Extract the quoted text from the child
            let extract_quoted_text = || {
                if let Some(child) = node.child(0) {
                    let text = child.utf8_text(input_bytes).unwrap().to_string();
                    let range =
                        crate::pandoc::source_map_compat::source_info_to_qsm_range_or_fallback(
                            &node_source_info_with_context(&child, context),
                            context,
                        );
                    PandocNativeIntermediate::IntermediateBaseText(text, range)
                } else {
                    let range =
                        crate::pandoc::source_map_compat::source_info_to_qsm_range_or_fallback(
                            &node_source_info_with_context(node, context),
                            context,
                        );
                    PandocNativeIntermediate::IntermediateBaseText(String::new(), range)
                }
            };
            process_shortcode_string(&extract_quoted_text, node, context)
        }
        "shortcode_number" => process_shortcode_number(node, input_bytes, context),
        "shortcode_boolean" => process_shortcode_boolean(node, input_bytes, context),
        "shortcode_keyword_param" => process_shortcode_keyword_param(buf, node, children, context),
        "shortcode" | "shortcode_escaped" => process_shortcode(node, children, context),
        // Citation nodes
        "citation_delimiter" => PandocNativeIntermediate::IntermediateUnknown(node_location(node)),
        "citation_id_author_in_text" => {
            let id = node.utf8_text(input_bytes).unwrap().to_string();
            let range = node_location(node);
            PandocNativeIntermediate::IntermediateBaseText(id, range)
        }
        "citation_id_suppress_author" => {
            let id = node.utf8_text(input_bytes).unwrap().to_string();
            let range = node_location(node);
            PandocNativeIntermediate::IntermediateBaseText(id, range)
        }
        "citation" => {
            let node_text = || node.utf8_text(input_bytes).unwrap().to_string();
            process_citation(node, node_text, children, context)
        }
        "code_span_delimiter" => {
            // Marker node, no processing needed
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "pandoc_code_span" => process_pandoc_code_span(node, children, input_bytes, context),
        // Inline note nodes
        "inline_note_delimiter" => {
            // Marker node, no processing needed
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "inline_note" => {
            // Collect inline content from children (excluding delimiters)
            let mut inlines: Vec<Inline> = Vec::new();
            for (node_name, child) in children {
                if node_name == "inline_note_delimiter" {
                    continue; // Skip delimiter markers
                }
                match child {
                    PandocNativeIntermediate::IntermediateInline(inline) => inlines.push(inline),
                    PandocNativeIntermediate::IntermediateInlines(mut inner_inlines) => {
                        inlines.append(&mut inner_inlines);
                    }
                    _ => {} // Ignore other types
                }
            }

            // Wrap inlines in a Paragraph block, then wrap in Note inline
            PandocNativeIntermediate::IntermediateInline(Inline::Note(Note {
                content: vec![Block::Paragraph(Paragraph {
                    content: inlines,
                    source_info: node_source_info_with_context(node, context),
                })],
                source_info: node_source_info_with_context(node, context),
            }))
        }
        // Note reference node
        "inline_note_reference" => {
            // Extract the note reference text (e.g., " [^id]" or "[^id]")
            // Tree-sitter may include leading whitespace in the node
            let text = node.utf8_text(input_bytes).unwrap();

            // Check for leading whitespace before trimming
            let has_leading_space = text.starts_with(char::is_whitespace);

            // Trim to extract the actual reference
            let trimmed = text.trim();

            // Verify format and extract ID
            if trimmed.starts_with("[^") && trimmed.ends_with("]") {
                let id = trimmed[2..trimmed.len() - 1].to_string();
                let note_ref = Inline::NoteReference(NoteReference {
                    id,
                    source_info: node_source_info_with_context(node, context),
                });

                // Build result with leading Space if needed to distinguish
                // "Hi [^ref]" from "Hi[^ref]"
                if has_leading_space {
                    PandocNativeIntermediate::IntermediateInlines(vec![
                        Inline::Space(Space {
                            source_info: node_source_info_with_context(node, context),
                        }),
                        note_ref,
                    ])
                } else {
                    PandocNativeIntermediate::IntermediateInline(note_ref)
                }
            } else {
                // Shouldn't happen with tree-sitter grammar, but handle gracefully
                eprintln!(
                    "Warning: unexpected inline_note_reference format: '{}'",
                    trimmed
                );
                PandocNativeIntermediate::IntermediateUnknown(node_location(node))
            }
        }
        // Note definition nodes
        "ref_id_specifier" => {
            // Extract the ref ID specifier text (e.g., "[^id]:")
            let text = node.utf8_text(input_bytes).unwrap().to_string();
            PandocNativeIntermediate::IntermediateBaseText(text, node_location(node))
        }
        "inline_ref_def" => process_note_definition_para(node, children, context),
        // Quote-related nodes
        "single_quote" | "double_quote" => {
            // Delimiter nodes for quotes - marker only
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "pandoc_single_quote" => process_quoted(
            node,
            children,
            QuoteType::SingleQuote,
            "single_quote",
            context,
        ),
        "pandoc_double_quote" => process_quoted(
            node,
            children,
            QuoteType::DoubleQuote,
            "double_quote",
            context,
        ),
        "content" => process_content_node(node, children),
        // Attribute-related nodes
        "{" | "}" | "=" => {
            // Delimiter nodes for attributes - marker only
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "attribute_id" => {
            // Extract ID, strip leading '#'
            let text = node.utf8_text(input_bytes).unwrap();
            let id = if text.starts_with('#') {
                &text[1..]
            } else {
                text
            };
            PandocNativeIntermediate::IntermediateBaseText(id.to_string(), node_location(node))
        }
        "attribute_class" => {
            // Extract class, strip leading '.'
            let text = node.utf8_text(input_bytes).unwrap();
            let class = if text.starts_with('.') {
                &text[1..]
            } else {
                text
            };
            PandocNativeIntermediate::IntermediateBaseText(class.to_string(), node_location(node))
        }
        "key_value_key" => {
            // Extract key name and trim whitespace
            let text = node.utf8_text(input_bytes).unwrap().trim().to_string();
            PandocNativeIntermediate::IntermediateBaseText(text, node_location(node))
        }
        "key_value_value" => {
            // Extract value, strip quotes if present
            let text = node.utf8_text(input_bytes).unwrap();
            let value = extract_quoted_text(text);
            PandocNativeIntermediate::IntermediateBaseText(value, node_location(node))
        }
        "key_value_specifier" => {
            // Collect key and value from children
            let mut key = String::new();
            let mut value = String::new();
            let mut key_range = node_location(node);
            let mut value_range = node_location(node);

            for (node_name, child) in children {
                match node_name.as_str() {
                    "key_value_key" => {
                        if let PandocNativeIntermediate::IntermediateBaseText(text, range) = child {
                            key = text;
                            key_range = range;
                        }
                    }
                    "key_value_value" => {
                        if let PandocNativeIntermediate::IntermediateBaseText(text, range) = child {
                            value = text;
                            value_range = range;
                        }
                    }
                    "=" => {} // Ignore delimiter
                    _ => {}
                }
            }

            PandocNativeIntermediate::IntermediateKeyValueSpec(vec![(
                key,
                value,
                key_range,
                value_range,
            )])
        }
        "commonmark_specifier" => {
            // Process commonmark attributes (id, classes, key-value pairs)
            process_commonmark_attribute(children, context)
        }
        "attribute_specifier" => {
            // Filter out delimiter nodes and pass through the commonmark_specifier or raw_specifier result
            for (node_name, child) in children {
                if node_name == "commonmark_specifier" {
                    return child; // Should be IntermediateAttr
                } else if node_name == "raw_specifier" {
                    return child; // Should be IntermediateRawFormat
                }
            }
            // If no commonmark_specifier or raw_specifier found, return empty attr
            use std::collections::HashMap;
            PandocNativeIntermediate::IntermediateAttr(
                ("".to_string(), vec![], HashMap::new()),
                AttrSourceInfo::empty(),
            )
        }
        // Link, span, and image-related nodes
        "[" | "]" | "](" | ")" => {
            // Delimiter nodes for links/spans/images - marker only
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "url" => {
            // Extract URL text directly
            let text = node.utf8_text(input_bytes).unwrap().to_string();
            PandocNativeIntermediate::IntermediateBaseText(text, node_location(node))
        }
        "title" => {
            // Extract title text, strip quotes
            let text = node.utf8_text(input_bytes).unwrap();
            let title = extract_quoted_text(text);
            PandocNativeIntermediate::IntermediateBaseText(title, node_location(node))
        }
        "target" => process_target(children),
        "pandoc_span" => process_pandoc_span(node, children, context),
        "pandoc_image" => process_pandoc_image(node, children, context),
        "note_definition_fenced_block" => {
            process_note_definition_fenced_block(node, children, context)
        }
        "code_fence_content" => process_code_fence_content(node, children, input_bytes, context),
        "fenced_code_block_delimiter" => {
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "list_marker_parenthesis" | "list_marker_dot" | "list_marker_example" => {
            process_list_marker(node, input_bytes, context)
        }
        // These are marker nodes, we don't need to do anything with them
        "block_quote_marker" | "list_marker_minus" | "list_marker_star" | "list_marker_plus" => {
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "pandoc_list" => process_list(node, children, context),
        "list_item" => process_list_item(node, children, context),
        "info_string" => process_info_string(node, input_bytes, context),
        "language_attribute" => process_language_attribute(children, context),
        "raw_attribute" => process_raw_attribute(node, children, context),
        "raw_specifier" => {
            // Extract raw format from raw_specifier node (e.g., "=html")
            let text = std::str::from_utf8(&input_bytes[node.byte_range()])
                .unwrap()
                .to_string();
            // Remove the leading '=' to get the format name
            let format = text.strip_prefix('=').unwrap_or(&text).to_string();
            let source_info = node_source_info_with_context(node, context);
            let range = crate::pandoc::source_map_compat::source_info_to_qsm_range_or_fallback(
                &source_info,
                context,
            );
            PandocNativeIntermediate::IntermediateRawFormat(format, range)
        }
        "block_continuation" => PandocNativeIntermediate::IntermediateUnknown(node_location(node)),
        "pandoc_block_quote" => process_block_quote(buf, node, children, context),
        "pandoc_horizontal_rule" => process_thematic_break(node, context),
        "pandoc_code_block" => process_fenced_code_block(node, children, context),
        "pandoc_div" => process_fenced_div_block(buf, node, children, context),
        "pipe_table_delimiter_cell" => process_pipe_table_delimiter_cell(children, context),
        "pipe_table_header" | "pipe_table_row" => {
            process_pipe_table_header_or_row(node, children, context)
        }
        "pipe_table_delimiter_row" => process_pipe_table_delimiter_row(children, context),
        "pipe_table_cell" => process_pipe_table_cell(node, children, context),
        "caption" => process_caption(node, children, context),
        "pipe_table" => process_pipe_table(node, children, context),
        _ => {
            writeln!(
                buf,
                "[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: {}",
                node.kind()
            )
            .unwrap();
            let range = node_location(node);
            PandocNativeIntermediate::IntermediateUnknown(range)
        }
    };
    buf.write_all(&inline_buf).unwrap();
    buf.write_all(&link_buf).unwrap();
    buf.write_all(&image_buf).unwrap();
    result
}

pub fn treesitter_to_pandoc<T: Write>(
    buf: &mut T,
    tree: &tree_sitter_qmd::MarkdownTree,
    input_bytes: &[u8],
    context: &ASTContext,
    error_collector: &mut crate::utils::diagnostic_collector::DiagnosticCollector,
) -> Result<Pandoc, Vec<quarto_error_reporting::DiagnosticMessage>> {
    let result = bottomup_traverse_concrete_tree(
        &mut tree.walk(),
        &mut |node, children, input_bytes, context| {
            native_visitor(buf, node, children, input_bytes, context)
        },
        &input_bytes,
        context,
    );
    let (_, PandocNativeIntermediate::IntermediatePandoc(pandoc)) = result else {
        panic!("Expected Pandoc, got {:?}", result)
    };
    let result = match postprocess(pandoc, error_collector) {
        Ok(doc) => doc,
        Err(()) => {
            // Postprocess found errors, return the diagnostics from the collector
            // We need to get the diagnostics out - let's use a temporary collector
            // Actually, we can't consume the collector here because it's borrowed
            // We need to get a copy of the diagnostics
            let diagnostics = error_collector.diagnostics().to_vec();
            return Err(diagnostics);
        }
    };
    let result = merge_strs(result);
    Ok(result)
}
