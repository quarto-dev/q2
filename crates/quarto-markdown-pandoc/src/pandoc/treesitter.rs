/*
 * treesitter.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::treesitter_utils;
use crate::pandoc::treesitter_utils::attribute::process_attribute;
use crate::pandoc::treesitter_utils::atx_heading::process_atx_heading;
use crate::pandoc::treesitter_utils::backslash_escape::process_backslash_escape;
use crate::pandoc::treesitter_utils::block_quote::process_block_quote;
use crate::pandoc::treesitter_utils::caption::process_caption;
use crate::pandoc::treesitter_utils::citation::process_citation;
use crate::pandoc::treesitter_utils::code_fence_content::process_code_fence_content;
use crate::pandoc::treesitter_utils::code_span::process_code_span;
use crate::pandoc::treesitter_utils::commonmark_attribute::process_commonmark_attribute;
use crate::pandoc::treesitter_utils::document::process_document;
use crate::pandoc::treesitter_utils::editorial_marks::{
    process_delete, process_editcomment, process_highlight, process_insert,
};
use crate::pandoc::treesitter_utils::fenced_code_block::process_fenced_code_block;
use crate::pandoc::treesitter_utils::fenced_div_block::process_fenced_div_block;
use crate::pandoc::treesitter_utils::html_comment::process_html_comment;
use crate::pandoc::treesitter_utils::indented_code_block::process_indented_code_block;
use crate::pandoc::treesitter_utils::info_string::process_info_string;
use crate::pandoc::treesitter_utils::inline_link::process_inline_link;
use crate::pandoc::treesitter_utils::key_value_specifier::process_key_value_specifier;
use crate::pandoc::treesitter_utils::language_attribute::process_language_attribute;
use crate::pandoc::treesitter_utils::latex_span::process_latex_span;
use crate::pandoc::treesitter_utils::link_title::process_link_title;
use crate::pandoc::treesitter_utils::list_marker::process_list_marker;
use crate::pandoc::treesitter_utils::note_definition_fenced_block::process_note_definition_fenced_block;
use crate::pandoc::treesitter_utils::note_definition_para::process_note_definition_para;
use crate::pandoc::treesitter_utils::note_reference::process_note_reference;
use crate::pandoc::treesitter_utils::numeric_character_reference::process_numeric_character_reference;
use crate::pandoc::treesitter_utils::paragraph::process_paragraph;
use crate::pandoc::treesitter_utils::pipe_table::{
    process_pipe_table, process_pipe_table_cell, process_pipe_table_delimiter_cell,
    process_pipe_table_delimiter_row, process_pipe_table_header_or_row,
};
use crate::pandoc::treesitter_utils::postprocess::{merge_strs, postprocess};
use crate::pandoc::treesitter_utils::quoted_span::process_quoted_span;
use crate::pandoc::treesitter_utils::raw_attribute::process_raw_attribute;
use crate::pandoc::treesitter_utils::raw_specifier::process_raw_specifier;
use crate::pandoc::treesitter_utils::section::process_section;
use crate::pandoc::treesitter_utils::setext_heading::process_setext_heading;
use crate::pandoc::treesitter_utils::shortcode::{
    process_shortcode, process_shortcode_boolean, process_shortcode_keyword_param,
    process_shortcode_number, process_shortcode_string, process_shortcode_string_arg,
};
use crate::pandoc::treesitter_utils::text_helpers::*;
use crate::pandoc::treesitter_utils::thematic_break::process_thematic_break;
use crate::pandoc::treesitter_utils::uri_autolink::process_uri_autolink;

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, Blocks, BulletList, OrderedList, Paragraph, Plain, RawBlock};
use crate::pandoc::inline::{
    Emph, Inline, Note, RawInline, Space, Str, Strikeout, Strong, Subscript, Superscript,
};
use crate::pandoc::list::{ListAttributes, ListNumberDelim, ListNumberStyle};
use crate::pandoc::location::{node_location, node_source_info, node_source_info_with_context};
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

// Macro for simple emphasis-like inline processing
macro_rules! emphasis_inline {
    ($node:expr, $children:expr, $delimiter:expr, $native_inline:expr, $inline_type:ident, $context:expr) => {
        process_emphasis_inline(
            $node,
            $children,
            $delimiter,
            $native_inline,
            |inlines, node| {
                Inline::$inline_type($inline_type {
                    content: inlines,
                    source_info: node_source_info_with_context(node, $context),
                })
            },
        )
    };
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
    let mut inlines_buf = Vec::<u8>::new();
    let mut link_buf = Vec::<u8>::new();
    let mut image_buf = Vec::<u8>::new();

    let whitespace_re: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
    let indent_re: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]+").unwrap());

    let node_text = || node.utf8_text(input_bytes).unwrap().to_string();

    let string_as_base_text = || {
        let location = node_location(node);
        let value = node_text();
        PandocNativeIntermediate::IntermediateBaseText(extract_quoted_text(&value), location)
    };
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
    let mut native_inlines =
        |children| process_native_inlines(children, &whitespace_re, &mut inlines_buf, context);

    let result = match node.kind() {
        // "numeric_character_reference" => {
        //     process_numeric_character_reference(node, input_bytes, context)
        // }
        // "language"
        // | "note_reference_id"
        // | "ref_id_specifier"
        // | "fenced_div_note_id"
        // | "citation_id_suppress_author"
        // | "citation_id_author_in_text"
        // | "link_destination"
        // | "key_value_key"
        // | "code_content"
        // | "latex_content"
        // | "text_base" => create_base_text_from_node_text(node, input_bytes),
        "document" => process_document(node, children, context),
        "section" => process_section(node, children, context),
        "pandoc_paragraph" => process_paragraph(node, children, context),
        "pandoc_str" => {
            let text = node.utf8_text(input_bytes).unwrap().to_string();
            PandocNativeIntermediate::IntermediateInline(Inline::Str(Str {
                text: apply_smart_quotes(text),
                source_info: node_source_info_with_context(node, context),
            }))
        }
        "pandoc_space" => {
            PandocNativeIntermediate::IntermediateInline(Inline::Space(Space {
                source_info: node_source_info_with_context(node, context),
            }))
        }
        // "indented_code_block" => {
        //     process_indented_code_block(node, children, input_bytes, &indent_re, context)
        // }
        // "fenced_code_block" => process_fenced_code_block(node, children, context),
        // "attribute" => process_attribute(children, context),
        // "commonmark_attribute" => process_commonmark_attribute(children, context),
        // "class_specifier" | "id_specifier" => create_specifier_base_text(node, input_bytes),
        // "shortcode_naked_string" | "shortcode_name" | "shortcode_key_name_and_equals" => {
        //     process_shortcode_string_arg(node, input_bytes, context)
        // }
        // "shortcode_string" => process_shortcode_string(&string_as_base_text, node, context),
        // "key_value_value" => string_as_base_text(),
        // "link_title" => process_link_title(node, input_bytes, context),
        // "link_text" => PandocNativeIntermediate::IntermediateInlines(native_inlines(children)),
        // "image" => treesitter_utils::image::process_image(
        //     node,
        //     &mut image_buf,
        //     node_text,
        //     children,
        //     context,
        // ),
        // "image_description" => {
        //     PandocNativeIntermediate::IntermediateInlines(native_inlines(children))
        // }
        // "inline_link" => process_inline_link(node, &mut link_buf, node_text, children, context),
        // "key_value_specifier" => process_key_value_specifier(buf, children, context),
        // "raw_specifier" => process_raw_specifier(node, input_bytes, context),
        // "emphasis" => emphasis_inline!(
        //     node,
        //     children,
        //     "emphasis_delimiter",
        //     native_inline,
        //     Emph,
        //     context
        // ),
        // "strong_emphasis" => {
        //     emphasis_inline!(
        //         node,
        //         children,
        //         "emphasis_delimiter",
        //         native_inline,
        //         Strong,
        //         context
        //     )
        // }
        // "inline" => {
        //     let inlines: Vec<Inline> = children.into_iter().map(native_inline).collect();
        //     PandocNativeIntermediate::IntermediateInlines(inlines)
        // }
        // "citation" => process_citation(node, node_text, children, context),
        // "note_reference" => process_note_reference(node, children, context),
        // "inline_ref_def" => process_note_definition_para(node, children, context),
        // "note_definition_fenced_block" => {
        //     process_note_definition_fenced_block(node, children, context)
        // }
        // "shortcode" | "shortcode_escaped" => process_shortcode(node, children, context),
        // "shortcode_keyword_param" => process_shortcode_keyword_param(buf, node, children, context),
        // "shortcode_boolean" => process_shortcode_boolean(node, input_bytes, context),
        // "shortcode_number" => process_shortcode_number(node, input_bytes, context),
        // "code_fence_content" => process_code_fence_content(node, children, input_bytes, context),
        // "list_marker_parenthesis" | "list_marker_dot" | "list_marker_example" => {
        //     process_list_marker(node, input_bytes, context)
        // }
        // // These are marker nodes, we don't need to do anything with it
        // "block_quote_marker"
        // | "list_marker_minus"
        // | "list_marker_star"
        // | "list_marker_plus"
        // | "block_continuation"
        // | "fenced_code_block_delimiter"
        // | "note_reference_delimiter"
        // | "shortcode_delimiter"
        // | "citation_delimiter"
        // | "code_span_delimiter"
        // | "single_quoted_span_delimiter"
        // | "double_quoted_span_delimiter"
        // | "superscript_delimiter"
        // | "subscript_delimiter"
        // | "strikeout_delimiter"
        // | "emphasis_delimiter"
        // | "insert_delimiter"
        // | "delete_delimiter"
        // | "highlight_delimiter"
        // | "edit_comment_delimiter" => {
        //     PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        // }
        // "soft_line_break" => create_line_break_inline(node, false),
        // "hard_line_break" => create_line_break_inline(node, true),
        // "latex_span_delimiter" => {
        //     let str = node.utf8_text(input_bytes).unwrap();
        //     let range = node_location(node);
        //     if str == "$" {
        //         PandocNativeIntermediate::IntermediateLatexInlineDelimiter(range)
        //     } else if str == "$$" {
        //         PandocNativeIntermediate::IntermediateLatexDisplayDelimiter(range)
        //     } else {
        //         writeln!(
        //             buf,
        //             "Warning: Unrecognized latex_span_delimiter: {} Will assume inline delimiter",
        //             str
        //         )
        //         .unwrap();
        //         PandocNativeIntermediate::IntermediateLatexInlineDelimiter(range)
        //     }
        // }
        // "inline_note" => process_emphasis_inline_with_node(
        //     node,
        //     children,
        //     "inline_note_delimiter",
        //     native_inline,
        //     |inlines, node| {
        //         Inline::Note(Note {
        //             content: vec![Block::Paragraph(Paragraph {
        //                 content: inlines,
        //                 source_info: node_source_info(node),
        //             })],
        //             source_info: node_source_info(node),
        //         })
        //     },
        // ),
        // "superscript" => emphasis_inline!(
        //     node,
        //     children,
        //     "superscript_delimiter",
        //     native_inline,
        //     Superscript,
        //     context
        // ),
        // "subscript" => emphasis_inline!(
        //     node,
        //     children,
        //     "subscript_delimiter",
        //     native_inline,
        //     Subscript,
        //     context
        // ),
        // "strikeout" => emphasis_inline!(
        //     node,
        //     children,
        //     "strikeout_delimiter",
        //     native_inline,
        //     Strikeout,
        //     context
        // ),
        // "insert" => process_insert(buf, node, children, context),
        // "delete" => process_delete(buf, node, children, context),
        // "highlight" => process_highlight(buf, node, children, context),
        // "edit_comment" => process_editcomment(buf, node, children, context),

        // "quoted_span" => process_quoted_span(node, children, native_inline, context),
        // "code_span" => process_code_span(buf, node, children, context),
        // "latex_span" => process_latex_span(node, children, context),
        // "html_comment" => process_html_comment(node, input_bytes, context),
        // "list" => process_list(node, children, context),
        // "list_item" => process_list_item(node, children, context),
        // "info_string" => process_info_string(children, context),
        // "language_attribute" => process_language_attribute(children, context),
        // "raw_attribute" => process_raw_attribute(node, children, context),
        // "block_quote" => process_block_quote(buf, node, children, context),
        // "fenced_div_block" => process_fenced_div_block(buf, node, children, context),
        // "atx_heading" => process_atx_heading(buf, node, children, context),
        // "thematic_break" => process_thematic_break(node, context),
        // "backslash_escape" => process_backslash_escape(node, input_bytes, context),
        // "minus_metadata" => {
        //     let text = node.utf8_text(input_bytes).unwrap();
        //     PandocNativeIntermediate::IntermediateMetadataString(
        //         text.to_string(),
        //         node_location(node),
        //     )
        // }
        // "uri_autolink" => process_uri_autolink(node, input_bytes, context),
        // "pipe_table_delimiter_cell" => process_pipe_table_delimiter_cell(children, context),
        // "pipe_table_header" | "pipe_table_row" => {
        //     process_pipe_table_header_or_row(node, children, context)
        // }
        // "pipe_table_delimiter_row" => process_pipe_table_delimiter_row(children, context),
        // "pipe_table_cell" => process_pipe_table_cell(node, children, context),
        // "caption" => process_caption(node, children, context),
        // "pipe_table" => process_pipe_table(node, children, context),
        // "setext_h1_underline" => PandocNativeIntermediate::IntermediateSetextHeadingLevel(1),
        // "setext_h2_underline" => PandocNativeIntermediate::IntermediateSetextHeadingLevel(2),
        // "setext_heading" => process_setext_heading(buf, node, children, context),
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
    buf.write_all(&inlines_buf).unwrap();
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
