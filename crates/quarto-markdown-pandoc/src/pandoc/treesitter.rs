/*
 * treesitter.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::treesitter_utils;
use crate::pandoc::treesitter_utils::attribute::process_attribute;
use crate::pandoc::treesitter_utils::backslash_escape::process_backslash_escape;
use crate::pandoc::treesitter_utils::commonmark_attribute::process_commonmark_attribute;
use crate::pandoc::treesitter_utils::info_string::process_info_string;
use crate::pandoc::treesitter_utils::key_value_specifier::process_key_value_specifier;
use crate::pandoc::treesitter_utils::language_attribute::process_language_attribute;
use crate::pandoc::treesitter_utils::link_title::process_link_title;
use crate::pandoc::treesitter_utils::list_marker::process_list_marker;
use crate::pandoc::treesitter_utils::numeric_character_reference::process_numeric_character_reference;
use crate::pandoc::treesitter_utils::paragraph::process_paragraph;
use crate::pandoc::treesitter_utils::postprocess::{desugar, merge_strs};
use crate::pandoc::treesitter_utils::quoted_span::process_quoted_span;
use crate::pandoc::treesitter_utils::raw_attribute::process_raw_attribute;
use crate::pandoc::treesitter_utils::raw_specifier::process_raw_specifier;
use crate::pandoc::treesitter_utils::shortcode::{
    process_shortcode, process_shortcode_boolean, process_shortcode_keyword_param,
    process_shortcode_number, process_shortcode_string, process_shortcode_string_arg,
};
use crate::pandoc::treesitter_utils::text_helpers::*;
use crate::pandoc::treesitter_utils::thematic_break::process_thematic_break;

use crate::pandoc::attr::{Attr, empty_attr, is_empty_attr};
use crate::pandoc::block::{
    Block, BlockQuote, Blocks, BulletList, CodeBlock, Div, Header, OrderedList, Paragraph, Plain,
    RawBlock,
};
use crate::pandoc::caption::Caption;
use crate::pandoc::inline::{
    Citation, CitationMode, Cite, Code, Delete, EditComment, Emph, Highlight, Inline, Inlines,
    Insert, Link, Math, MathType, Note, NoteReference, RawInline, Space, Str, Strikeout, Strong,
    Subscript, Superscript, is_empty_target,
};

use crate::pandoc::inline::{make_cite_inline, make_span_inline};
use crate::pandoc::list::{ListAttributes, ListNumberDelim, ListNumberStyle};
use crate::pandoc::location::{
    Range, SourceInfo, empty_source_info, node_location, node_source_info,
};
use crate::pandoc::meta::Meta;
use crate::pandoc::pandoc::Pandoc;
use crate::pandoc::table::{
    Alignment, Cell, ColSpec, ColWidth, Row, Table, TableBody, TableFoot, TableHead,
};
use core::panic;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::io::Write;

use crate::traversals::bottomup_traverse_concrete_tree;

use treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

// Helper function to process document nodes
fn process_document(children: Vec<(String, PandocNativeIntermediate)>) -> PandocNativeIntermediate {
    let mut blocks: Vec<Block> = Vec::new();
    children.into_iter().for_each(|(_, child)| {
        match child {
            PandocNativeIntermediate::IntermediateBlock(block) => blocks.push(block),
            PandocNativeIntermediate::IntermediateSection(section) => {
                blocks.extend(section);
            }
            PandocNativeIntermediate::IntermediateMetadataString(text, range) => {
                // for now we assume it's metadata and emit it as a rawblock
                blocks.push(Block::RawBlock(RawBlock {
                    format: "quarto_minus_metadata".to_string(),
                    text,
                    source_info: SourceInfo::with_range(range),
                }));
            }
            _ => panic!("Expected Block or Section, got {:?}", child),
        }
    });
    PandocNativeIntermediate::IntermediatePandoc(Pandoc {
        meta: Meta::default(),
        blocks,
    })
}

// Helper function to process indented_code_block nodes
fn process_indented_code_block(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
    indent_re: &Regex,
) -> PandocNativeIntermediate {
    let mut content: String = String::new();
    let outer_range = node_location(node);
    // first, find the beginning of the contents in the node itself
    let outer_string = node.utf8_text(input_bytes).unwrap().to_string();
    let mut start_offset = indent_re.find(&outer_string).map_or(0, |m| m.end());

    for (node, children) in children {
        if node == "block_continuation" {
            // append all content up to the beginning of this continuation
            match children {
                PandocNativeIntermediate::IntermediateUnknown(range) => {
                    // Calculate the relative offset of the continuation within outer_string
                    let continuation_start =
                        range.start.offset.saturating_sub(outer_range.start.offset);
                    let continuation_end =
                        range.end.offset.saturating_sub(outer_range.start.offset);

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
        source_info: SourceInfo::with_range(outer_range),
    }))
}

// Helper function to process fenced_code_block nodes
fn process_fenced_code_block(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut content: String = String::new();
    let mut attr: Attr = empty_attr();
    let mut raw_format: Option<String> = None;
    for (node, child) in children {
        if node == "block_continuation" {
            continue; // skip block continuation nodes
        }
        if node == "code_fence_content" {
            let PandocNativeIntermediate::IntermediateBaseText(text, _) = child else {
                panic!("Expected BaseText in code_fence_content, got {:?}", child)
            };
            content = text;
        } else if node == "commonmark_attribute" {
            let PandocNativeIntermediate::IntermediateAttr(a) = child else {
                panic!("Expected Attr in commonmark_attribute, got {:?}", child)
            };
            attr = a;
        } else if node == "raw_attribute" {
            let PandocNativeIntermediate::IntermediateRawFormat(format, _) = child else {
                panic!("Expected RawFormat in raw_attribute, got {:?}", child)
            };
            raw_format = Some(format);
        } else if node == "language_attribute" {
            let PandocNativeIntermediate::IntermediateBaseText(lang, _) = child else {
                panic!("Expected BaseText in language_attribute, got {:?}", child)
            };
            attr.1.push(lang); // set the language
        } else if node == "info_string" {
            let PandocNativeIntermediate::IntermediateAttr(inner_attr) = child else {
                panic!("Expected Attr in info_string, got {:?}", child)
            };
            attr = inner_attr;
        }
    }
    let location = node_location(node);

    // it might be the case (because of tree-sitter error recovery)
    // that the content does not end with a newline, so we ensure it does before popping
    if content.ends_with('\n') {
        content.pop(); // remove the trailing newline
    }

    if let Some(format) = raw_format {
        PandocNativeIntermediate::IntermediateBlock(Block::RawBlock(RawBlock {
            format,
            text: content,
            source_info: SourceInfo::with_range(location),
        }))
    } else {
        PandocNativeIntermediate::IntermediateBlock(Block::CodeBlock(CodeBlock {
            attr,
            text: content,
            source_info: SourceInfo::with_range(location),
        }))
    }
}

// Helper function to process section nodes
fn process_section(children: Vec<(String, PandocNativeIntermediate)>) -> PandocNativeIntermediate {
    let mut blocks: Vec<Block> = Vec::new();
    children.into_iter().for_each(|(node, child)| {
        if node == "block_continuation" {
            return;
        }
        match child {
            PandocNativeIntermediate::IntermediateBlock(block) => blocks.push(block),
            PandocNativeIntermediate::IntermediateSection(section) => {
                blocks.extend(section);
            }
            PandocNativeIntermediate::IntermediateMetadataString(text, range) => {
                // for now we assume it's metadata and emit it as a rawblock
                blocks.push(Block::RawBlock(RawBlock {
                    format: "quarto_minus_metadata".to_string(),
                    text,
                    source_info: SourceInfo::with_range(range),
                }));
            }
            _ => panic!("Expected Block or Section, got {:?}", child),
        }
    });
    PandocNativeIntermediate::IntermediateSection(blocks)
}

fn process_code_fence_content(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
) -> PandocNativeIntermediate {
    let start = node.range().start_byte;
    let end = node.range().end_byte;

    // This is a code block, we need to extract the content
    // by removing block_continuation markers
    let mut current_location = start;

    let mut content = String::new();
    for (child_node, child) in children {
        if child_node == "block_continuation" {
            let PandocNativeIntermediate::IntermediateUnknown(child_range) = child else {
                panic!(
                    "Expected IntermediateUnknown in block_continuation, got {:?}",
                    child
                )
            };
            let slice_before_continuation =
                &input_bytes[current_location..child_range.start.offset];
            content.push_str(std::str::from_utf8(slice_before_continuation).unwrap());
            current_location = child_range.end.offset;
        }
    }
    // Add the remaining content after the last block_continuation
    if current_location < end {
        let slice_after_continuation = &input_bytes[current_location..end];
        content.push_str(std::str::from_utf8(slice_after_continuation).unwrap());
    }
    PandocNativeIntermediate::IntermediateBaseText(content, node_location(node))
}

fn process_note_reference(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut id = String::new();
    for (node, child) in children {
        if node == "note_reference_delimiter" {
            // This is a marker node, we don't need to do anything with it
        } else if node == "note_reference_id" {
            if let PandocNativeIntermediate::IntermediateBaseText(text, _) = child {
                id = text;
            } else {
                panic!("Expected BaseText in note_reference_id, got {:?}", child);
            }
        } else {
            panic!("Unexpected note_reference node: {}", node);
        }
    }
    PandocNativeIntermediate::IntermediateInline(Inline::NoteReference(NoteReference {
        id,
        range: node_location(node),
    }))
}

fn process_citation<F>(
    node: &tree_sitter::Node,
    node_text: F,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate
where
    F: Fn() -> String,
{
    let mut citation_type = CitationMode::NormalCitation;
    let mut citation_id = String::new();
    for (node, child) in children {
        if node == "citation_id_suppress_author" {
            citation_type = CitationMode::SuppressAuthor;
            if let PandocNativeIntermediate::IntermediateBaseText(id, _) = child {
                citation_id = id;
            } else {
                panic!(
                    "Expected BaseText in citation_id_suppress_author, got {:?}",
                    child
                );
            }
        } else if node == "citation_id_author_in_text" {
            citation_type = CitationMode::AuthorInText;
            if let PandocNativeIntermediate::IntermediateBaseText(id, _) = child {
                citation_id = id;
            } else {
                panic!(
                    "Expected BaseText in citation_id_author_in_text, got {:?}",
                    child
                );
            }
        }
    }
    PandocNativeIntermediate::IntermediateInline(Inline::Cite(Cite {
        citations: vec![Citation {
            id: citation_id,
            prefix: vec![],
            suffix: vec![],
            mode: citation_type,
            note_num: 1, // Pandoc expects citations to be numbered from 1
            hash: 0,
        }],
        content: vec![Inline::Str(Str {
            text: node_text(),
            source_info: node_source_info(node),
        })],
        source_info: node_source_info(node),
    }))
}

fn process_inline_link<T: Write, F>(
    link_buf: &mut T,
    node_text: F,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate
where
    F: Fn() -> String,
{
    let mut attr: Attr = ("".to_string(), vec![], HashMap::new());
    let mut target = ("".to_string(), "".to_string());
    let mut content: Vec<Inline> = Vec::new();

    for (node, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateRawFormat(_, _) => {
                // TODO show position of this error
                let _ = writeln!(
                    link_buf,
                    "Raw attribute specifiers are unsupported in links and spans: {}. Ignoring.",
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
                        link_buf,
                        "Language specifiers are unsupported in links and spans: {}. Ignoring.",
                        node_text()
                    );
                } else {
                    panic!("Unexpected inline_link node: {}", node);
                }
            }
            PandocNativeIntermediate::IntermediateUnknown(_) => {}
            PandocNativeIntermediate::IntermediateInlines(inlines) => content.extend(inlines),
            PandocNativeIntermediate::IntermediateInline(inline) => content.push(inline),
            _ => panic!("Unexpected child in inline_link: {:?}", child),
        }
    }
    let has_citations = content
        .iter()
        .any(|inline| matches!(inline, Inline::Cite(_)));

    // an inline link might be a Cite if it has citations, no destination, and no title
    // and no attributes
    let is_cite = has_citations && is_empty_target(&target) && is_empty_attr(&attr);

    PandocNativeIntermediate::IntermediateInline(if is_cite {
        make_cite_inline(attr, target, content, empty_source_info())
    } else {
        make_span_inline(attr, target, content, empty_source_info())
    })
}

fn process_uri_autolink(node: &tree_sitter::Node, input_bytes: &[u8]) -> PandocNativeIntermediate {
    // This is a URI autolink, we need to extract the content
    // by removing the angle brackets
    let text = node.utf8_text(input_bytes).unwrap();
    if text.len() < 2 || !text.starts_with('<') || !text.ends_with('>') {
        panic!("Invalid URI autolink: {}", text);
    }
    let content = &text[1..text.len() - 1]; // remove the angle brackets
    let mut attr = ("".to_string(), vec![], HashMap::new());
    // pandoc adds the class "uri" to autolinks
    attr.1.push("uri".to_string());
    PandocNativeIntermediate::IntermediateInline(Inline::Link(Link {
        content: vec![Inline::Str(Str {
            text: content.to_string(),
            source_info: node_source_info(node),
        })],
        attr,
        target: (content.to_string(), "".to_string()),
        source_info: node_source_info(node),
    }))
}

fn process_setext_heading<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut content = Vec::new();
    let mut level = 1;
    for (_, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateBlock(Block::Paragraph(Paragraph {
                content: inner_content,
                ..
            })) => {
                content = inner_content;
            }
            PandocNativeIntermediate::IntermediateSetextHeadingLevel(l) => {
                level = l;
            }
            _ => {
                writeln!(
                    buf,
                    "[setext_heading] Warning: Unhandled node kind: {}",
                    node.kind()
                )
                .unwrap();
            }
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::Header(Header {
        level,
        attr: empty_attr(),
        content,
        source_info: SourceInfo::with_range(node_location(node)),
    }))
}

fn process_atx_heading<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut level = 0;
    let mut content: Vec<Inline> = Vec::new();
    let mut attr: Attr = ("".to_string(), vec![], HashMap::new());
    for (node, child) in children {
        if node == "block_continuation" {
            continue;
            // This is a marker node, we don't need to do anything with it
        } else if node == "atx_h1_marker" {
            level = 1;
        } else if node == "atx_h2_marker" {
            level = 2;
        } else if node == "atx_h3_marker" {
            level = 3;
        } else if node == "atx_h4_marker" {
            level = 4;
        } else if node == "atx_h5_marker" {
            level = 5;
        } else if node == "atx_h6_marker" {
            level = 6;
        } else if node == "inline" {
            if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                content.extend(inlines);
            } else {
                panic!("Expected Inlines in atx_heading, got {:?}", child);
            }
        } else if node == "attribute" {
            if let PandocNativeIntermediate::IntermediateAttr(inner_attr) = child {
                attr = inner_attr;
            } else {
                panic!("Expected Attr in attribute, got {:?}", child);
            }
        } else {
            writeln!(buf, "Warning: Unhandled node kind in atx_heading: {}", node).unwrap();
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::Header(Header {
        level,
        attr,
        content,
        source_info: SourceInfo::with_range(node_location(node)),
    }))
}

fn process_fenced_div_block<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut attr: Attr = ("".to_string(), vec![], HashMap::new());
    let mut content: Vec<Block> = Vec::new();
    for (node, child) in children {
        if node == "block_continuation" {
            continue;
        }
        match child {
            PandocNativeIntermediate::IntermediateBaseText(_, _) => {
                if node == "language_attribute" {
                    writeln!(
                        buf,
                        "Warning: language attribute unsupported in divs: {:?} {:?}",
                        node, child
                    )
                    .unwrap();
                } else {
                    writeln!(
                        buf,
                        "Warning: Unexpected base text in div, ignoring: {:?} {:?}",
                        node, child
                    )
                    .unwrap();
                }
            }
            PandocNativeIntermediate::IntermediateRawFormat(_, _) => {
                writeln!(
                    buf,
                    "Warning: Raw attribute specifiers are not supported in divs: {:?} {:?}",
                    node, child
                )
                .unwrap();
            }
            PandocNativeIntermediate::IntermediateAttr(a) => {
                attr = a;
            }
            PandocNativeIntermediate::IntermediateBlock(block) => {
                content.push(block);
            }
            PandocNativeIntermediate::IntermediateSection(blocks) => {
                content.extend(blocks);
            }
            PandocNativeIntermediate::IntermediateMetadataString(text, range) => {
                // for now we assume it's metadata and emit it as a rawblock
                content.push(Block::RawBlock(RawBlock {
                    format: "quarto_minus_metadata".to_string(),
                    text,
                    source_info: SourceInfo::with_range(range),
                }));
            }
            _ => {
                writeln!(
                    buf,
                    "Warning: Unhandled node kind in fenced_div_block: {:?} {:?}",
                    node, child
                )
                .unwrap();
            }
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::Div(Div {
        attr,
        content,
        source_info: SourceInfo::with_range(node_location(node)),
    }))
}

fn process_block_quote<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut content: Blocks = Vec::new();
    for (node_type, child) in children {
        if node_type == "block_quote_marker" {
            if matches!(child, PandocNativeIntermediate::IntermediateUnknown(_)) {
                if node_type != "block_continuation" {
                    writeln!(
                        buf,
                        "Warning: Unhandled node kind in block_quote: {}, {:?}",
                        node_type, child,
                    )
                    .unwrap();
                }
            }
            continue;
        }
        match child {
            PandocNativeIntermediate::IntermediateBlock(block) => {
                content.push(block);
            }
            PandocNativeIntermediate::IntermediateSection(section) => {
                content.extend(section);
            }
            PandocNativeIntermediate::IntermediateMetadataString(text, range) => {
                // for now we assume it's metadata and emit it as a rawblock
                content.push(Block::RawBlock(RawBlock {
                    format: "quarto_minus_metadata".to_string(),
                    text,
                    source_info: SourceInfo::with_range(range),
                }));
            }
            _ => {
                writeln!(
                buf,
                "[block_quote] Will ignore unknown node. Expected Block or Section in block_quote, got {:?}",
                child
                ).unwrap();
            }
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::BlockQuote(BlockQuote {
        content,
        source_info: SourceInfo::with_range(node_location(node)),
    }))
}

fn process_code_span<T: Write>(
    buf: &mut T,
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut is_raw: Option<String> = None;
    let mut attr = ("".to_string(), vec![], HashMap::new());
    let mut language_attribute: Option<String> = None;
    let mut inlines: Vec<_> = children
        .into_iter()
        .map(|(node_name, child)| {
            let range = node_location(node);
            match child {
                PandocNativeIntermediate::IntermediateAttr(a) => {
                    attr = a;
                    // IntermediateUnknown here "consumes" the node
                    (
                        node_name,
                        PandocNativeIntermediate::IntermediateUnknown(range),
                    )
                }
                PandocNativeIntermediate::IntermediateRawFormat(raw, _) => {
                    is_raw = Some(raw);
                    // IntermediateUnknown here "consumes" the node
                    (
                        node_name,
                        PandocNativeIntermediate::IntermediateUnknown(range),
                    )
                }
                PandocNativeIntermediate::IntermediateBaseText(text, range) => {
                    if node_name == "language_attribute" {
                        language_attribute = Some(text);
                        // IntermediateUnknown here "consumes" the node
                        (
                            node_name,
                            PandocNativeIntermediate::IntermediateUnknown(range),
                        )
                    } else {
                        (
                            node_name,
                            PandocNativeIntermediate::IntermediateBaseText(text, range),
                        )
                    }
                }
                _ => (node_name, child),
            }
        })
        .filter(|(_, child)| {
            match child {
                PandocNativeIntermediate::IntermediateUnknown(_) => false, // skip unknown nodes
                _ => true,                                                 // keep other nodes
            }
        })
        .collect();
    if inlines.len() == 0 {
        writeln!(
            buf,
            "Warning: Expected exactly one inline in code_span, got none"
        )
        .unwrap();
        return PandocNativeIntermediate::IntermediateInline(Inline::Code(Code {
            attr,
            text: "".to_string(),
            source_info: node_source_info(node),
        }));
    }
    let (_, child) = inlines.remove(0);
    if inlines.len() > 0 {
        writeln!(
            buf,
            "Warning: Expected exactly one inline in code_span, got {}. Will ignore the rest.",
            inlines.len() + 1
        )
        .unwrap();
    }
    let text = match child {
        PandocNativeIntermediate::IntermediateBaseText(text, _) => text,
        _ => {
            writeln!(
                buf,
                "Warning: Expected BaseText in code_span, got {:?}. Will ignore.",
                child
            )
            .unwrap();
            "".to_string()
        }
    };
    if let Some(raw) = is_raw {
        PandocNativeIntermediate::IntermediateInline(Inline::RawInline(RawInline {
            format: raw,
            text,
            source_info: node_source_info(node),
        }))
    } else {
        match language_attribute {
            Some(lang) => PandocNativeIntermediate::IntermediateInline(Inline::Code(Code {
                attr,
                text: lang + &" " + &text,
                source_info: node_source_info(node),
            })),
            None => PandocNativeIntermediate::IntermediateInline(Inline::Code(Code {
                attr,
                text,
                source_info: node_source_info(node),
            })),
        }
    }
}

fn process_latex_span(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut is_inline_math = false;
    let mut is_display_math = false;
    let mut inlines: Vec<_> = children
        .into_iter()
        .filter(|(_, child)| {
            if matches!(
                child,
                PandocNativeIntermediate::IntermediateLatexInlineDelimiter(_)
            ) {
                is_inline_math = true;
                false // skip the delimiter
            } else if matches!(
                child,
                PandocNativeIntermediate::IntermediateLatexDisplayDelimiter(_)
            ) {
                is_display_math = true;
                false // skip the delimiter
            } else {
                true // keep other nodes
            }
        })
        .collect();
    assert!(
        inlines.len() == 1,
        "Expected exactly one inline in latex_span, got {}",
        inlines.len()
    );
    if is_inline_math && is_display_math {
        panic!("Unexpected both inline and display math in latex_span");
    }
    if !is_inline_math && !is_display_math {
        panic!("Expected either inline or display math in latex_span, got neither");
    }
    let math_type = if is_inline_math {
        MathType::InlineMath
    } else {
        MathType::DisplayMath
    };
    let (_, child) = inlines.remove(0);
    let PandocNativeIntermediate::IntermediateBaseText(text, _) = child else {
        panic!("Expected BaseText in latex_span, got {:?}", child)
    };
    PandocNativeIntermediate::IntermediateInline(Inline::Math(Math {
        math_type: math_type,
        text,
        source_info: node_source_info(node),
    }))
}

fn process_list(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    // a list is loose if it has at least one loose item
    // an item is loose if
    //   - it has more than one paragraph in the list
    //   - it is a single paragraph with space between it and the next
    //     beginning of list item. There must be a next item for this to be true
    //     but the next item might not itself be a paragraph.

    let mut has_loose_item = false;
    let mut last_para_range: Option<Range> = None;
    let mut list_items: Vec<Blocks> = Vec::new();
    let mut is_ordered_list: Option<ListAttributes> = None;

    for (node, child) in children {
        if node == "block_continuation" {
            // this is a marker node, we don't need to do anything with it
            continue;
        }
        if node == "list_marker_parenthesis" || node == "list_marker_dot" {
            // this is an ordered list, so we need to set the flag
            let PandocNativeIntermediate::IntermediateOrderedListMarker(marker_number, _) = child
            else {
                panic!("Expected OrderedListMarker in list, got {:?}", child);
            };

            is_ordered_list = Some((
                marker_number,
                ListNumberStyle::Decimal,
                match node.as_str() {
                    "list_marker_parenthesis" => ListNumberDelim::OneParen,
                    "list_marker_dot" => ListNumberDelim::Period,
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

        // is the last item loose? Check the last paragraph range
        if let Some(ref last_range) = last_para_range {
            if last_range.end.row != child_range.start.row {
                // if the last paragraph ends on a different line than the current item starts,
                // then the last item was loose, mark it
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
            // last paragraph range after setting has_loose_item,
            // but we do it in case we want to use it later
            last_para_range = None;
            list_items.push(blocks);
            continue;
        }

        // is this item possibly loose?
        if blocks.len() == 1 {
            if let Some(Block::Paragraph(para)) = blocks.first() {
                // yes, so store the range and wait to finish the check on
                // next item
                last_para_range = Some(para.source_info.range.clone());
            } else {
                // if the first block is not a paragraph, it's not loose
                last_para_range = None;
            }
        } else {
            // if the item has multiple blocks (but not multiple paragraphs,
            // which would have been caught above), we need to reset the
            // last_para_range since this item can't participate in loose detection
            last_para_range = None;
        }
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
        Some(attr) => {
            PandocNativeIntermediate::IntermediateBlock(Block::OrderedList(OrderedList {
                attr,
                content,
                source_info: SourceInfo::with_range(node_location(node)),
            }))
        }
        None => PandocNativeIntermediate::IntermediateBlock(Block::BulletList(BulletList {
            content,
            source_info: SourceInfo::with_range(node_location(node)),
        })),
    }
}

fn process_list_item(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut list_attr: Option<ListAttributes> = None;
    let children = children
        .into_iter()
        .filter_map(|(node, child)| {
            if node == "list_marker_dot" || node == "list_marker_parenthesis" {
                // this is an ordered list, so we need to set the flag
                let PandocNativeIntermediate::IntermediateOrderedListMarker(marker_number, _) =
                    child
                else {
                    panic!("Expected OrderedListMarker in list_item, got {:?}", child);
                };
                list_attr = Some((
                    marker_number,
                    ListNumberStyle::Decimal,
                    match node.as_str() {
                        "list_marker_parenthesis" => ListNumberDelim::OneParen,
                        "list_marker_dot" => ListNumberDelim::Period,
                        _ => panic!("Unexpected list marker node: {}", node),
                    },
                ));
                return None; // skip the marker node
            }
            match child {
                PandocNativeIntermediate::IntermediateBlock(block) => Some(block),
                PandocNativeIntermediate::IntermediateMetadataString(text, range) => {
                    // for now we assume it's metadata and emit it as a rawblock
                    Some(Block::RawBlock(RawBlock {
                        format: "quarto_minus_metadata".to_string(),
                        text,
                        source_info: SourceInfo::with_range(range),
                    }))
                }
                _ => None,
            }
        })
        .collect();
    PandocNativeIntermediate::IntermediateListItem(children, node_location(node), list_attr)
}

fn process_pipe_table_delimiter_cell(
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut has_starter_colon = false;
    let mut has_ending_colon = false;
    for (node, _) in children {
        if node == "pipe_table_align_right" {
            has_ending_colon = true;
        } else if node == "pipe_table_align_left" {
            has_starter_colon = true;
        } else if node == "-" {
            continue;
        } else {
            panic!("Unexpected node in pipe_table_delimiter_cell: {}", node);
        }
    }
    PandocNativeIntermediate::IntermediatePipeTableDelimiterCell(
        match (has_starter_colon, has_ending_colon) {
            (true, true) => Alignment::Center,
            (true, false) => Alignment::Left,
            (false, true) => Alignment::Right,
            (false, false) => Alignment::Default,
        },
    )
}

fn process_pipe_table_header_or_row(
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut row = Row {
        attr: empty_attr(),
        cells: Vec::new(),
    };
    for (node, child) in children {
        if node == "|" {
            // This is a marker node, we don't need to do anything with it
            continue;
        } else if node == "pipe_table_cell" {
            if let PandocNativeIntermediate::IntermediateCell(cell) = child {
                row.cells.push(cell);
            } else {
                panic!("Expected Cell in pipe_table_row, got {:?}", child);
            }
        } else {
            panic!(
                "Expected pipe_table_cell in pipe_table_row, got {:?} {:?}",
                node, child
            );
        }
    }
    PandocNativeIntermediate::IntermediateRow(row)
}

fn process_pipe_table_delimiter_row(
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    // This is a row of delimiters, we don't need to do anything with it
    // but we need to return an empty row
    PandocNativeIntermediate::IntermediatePipeTableDelimiterRow(
        children
            .into_iter()
            .filter(|(node, _)| node != "|") // skip the marker nodes
            .map(|(node, child)| match child {
                PandocNativeIntermediate::IntermediatePipeTableDelimiterCell(alignment) => {
                    alignment
                }
                _ => panic!(
                    "Unexpected node in pipe_table_delimiter_row: {} {:?}",
                    node, child
                ),
            })
            .collect(),
    )
}

fn process_pipe_table_cell(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut plain_content: Inlines = Vec::new();
    let mut table_cell = Cell {
        alignment: Alignment::Left,
        col_span: 1,
        row_span: 1,
        attr: ("".to_string(), vec![], HashMap::new()),
        content: vec![],
    };
    for (node, child) in children {
        if node == "inline" {
            match child {
                PandocNativeIntermediate::IntermediateInlines(inlines) => {
                    plain_content.extend(inlines);
                }
                _ => panic!("Expected Inlines in pipe_table_cell, got {:?}", child),
            }
        } else {
            panic!(
                "Expected Inlines in pipe_table_cell, got {:?} {:?}",
                node, child
            );
        }
    }
    table_cell.content.push(Block::Plain(Plain {
        content: plain_content,
        source_info: SourceInfo::with_range(node_location(node)),
    }));
    PandocNativeIntermediate::IntermediateCell(table_cell)
}

fn process_pipe_table(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let attr = empty_attr();
    let mut header: Option<Row> = None;
    let mut colspec: Vec<ColSpec> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();
    for (node, child) in children {
        if node == "pipe_table_header" {
            if let PandocNativeIntermediate::IntermediateRow(row) = child {
                header = Some(row);
            } else {
                panic!("Expected Row in pipe_table_header, got {:?}", child);
            }
        } else if node == "pipe_table_delimiter_row" {
            match child {
                PandocNativeIntermediate::IntermediatePipeTableDelimiterRow(row) => {
                    row.into_iter().for_each(|alignment| {
                        colspec.push((alignment, ColWidth::Default));
                    });
                }
                _ => panic!(
                    "Expected PipeTableDelimiterRow in pipe_table_delimiter_row, got {:?}",
                    child
                ),
            }
        } else if node == "pipe_table_row" {
            if let PandocNativeIntermediate::IntermediateRow(row) = child {
                rows.push(row);
            } else {
                panic!("Expected Row in pipe_table_row, got {:?}", child);
            }
        } else {
            panic!("Unexpected node in pipe_table: {}", node);
        }
    }
    PandocNativeIntermediate::IntermediateBlock(Block::Table(Table {
        attr,
        caption: Caption {
            short: None,
            long: None,
        },
        colspec,
        head: TableHead {
            attr: empty_attr(),
            rows: vec![header.unwrap()],
        },
        bodies: vec![TableBody {
            attr: empty_attr(),
            rowhead_columns: 0,
            head: vec![],
            body: rows,
        }],
        foot: TableFoot {
            attr: empty_attr(),
            rows: vec![],
        },
        source_info: SourceInfo::with_range(node_location(node)),
    }))
}

// Macro for simple emphasis-like inline processing
macro_rules! emphasis_inline {
    ($node:expr, $children:expr, $delimiter:expr, $native_inline:expr, $inline_type:ident) => {
        process_emphasis_inline(
            $node,
            $children,
            $delimiter,
            $native_inline,
            |inlines, node| {
                Inline::$inline_type($inline_type {
                    content: inlines,
                    source_info: node_source_info(node),
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
) -> Inline {
    match child {
        PandocNativeIntermediate::IntermediateInline(inline) => inline,
        PandocNativeIntermediate::IntermediateBaseText(text, range) => {
            if let Some(_) = whitespace_re.find(&text) {
                Inline::Space(Space {
                    source_info: SourceInfo::with_range(range),
                })
            } else {
                Inline::Str(Str {
                    text: apply_smart_quotes(text),
                    source_info: SourceInfo::with_range(range),
                })
            }
        }
        // as a special inline, we need to allow commonmark attributes
        // to show up in the document, so we can appropriately attach attributes
        // to headings and tables (through their captions) as needed
        //
        // see tests/cursed/002.qmd for why this cannot be parsed directly in
        // the block grammar.
        PandocNativeIntermediate::IntermediateAttr(attr) => Inline::Attr(attr),
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
                source_info: empty_source_info(),
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
                source_info: empty_source_info(),
            })
        }
    }
}

// Standalone function to process a collection of children into a vector of Inline objects
fn process_native_inlines<T: Write>(
    children: Vec<(String, PandocNativeIntermediate)>,
    whitespace_re: &Regex,
    inlines_buf: &mut T,
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
                        source_info: SourceInfo::with_range(range),
                    }))
                } else {
                    inlines.push(Inline::Str(Str {
                        text,
                        source_info: SourceInfo::with_range(range),
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
    let native_inline = |(node_name, child)| {
        process_native_inline(
            node_name,
            child,
            &whitespace_re,
            &mut inline_buf,
            &node_text,
        )
    };
    let mut native_inlines =
        |children| process_native_inlines(children, &whitespace_re, &mut inlines_buf);

    let result = match node.kind() {
        "numeric_character_reference" => process_numeric_character_reference(node, input_bytes),

        "language"
        | "note_reference_id"
        | "citation_id_suppress_author"
        | "citation_id_author_in_text"
        | "link_destination"
        | "key_value_key"
        | "code_content"
        | "latex_content"
        | "text_base" => create_base_text_from_node_text(node, input_bytes),
        "document" => process_document(children),
        "section" => process_section(children),
        "paragraph" => process_paragraph(node, children),
        "indented_code_block" => {
            process_indented_code_block(node, children, input_bytes, &indent_re)
        }
        "fenced_code_block" => process_fenced_code_block(node, children),
        "attribute" => process_attribute(children),
        "commonmark_attribute" => process_commonmark_attribute(children),
        "class_specifier" | "id_specifier" => create_specifier_base_text(node, input_bytes),
        "shortcode_naked_string" | "shortcode_name" => {
            process_shortcode_string_arg(node, input_bytes)
        }
        "shortcode_string" => process_shortcode_string(&string_as_base_text, node),
        "key_value_value" => string_as_base_text(),
        "link_title" => process_link_title(node, input_bytes),
        "link_text" => PandocNativeIntermediate::IntermediateInlines(native_inlines(children)),
        "image" => treesitter_utils::image::process_image(&mut image_buf, node_text, children),
        "image_description" => {
            PandocNativeIntermediate::IntermediateInlines(native_inlines(children))
        }
        "inline_link" => process_inline_link(&mut link_buf, node_text, children),
        "key_value_specifier" => process_key_value_specifier(buf, children),
        "raw_specifier" => process_raw_specifier(node, input_bytes),
        "emphasis" => emphasis_inline!(node, children, "emphasis_delimiter", native_inline, Emph),
        "strong_emphasis" => {
            emphasis_inline!(node, children, "emphasis_delimiter", native_inline, Strong)
        }
        "inline" => {
            let inlines: Vec<Inline> = children.into_iter().map(native_inline).collect();
            PandocNativeIntermediate::IntermediateInlines(inlines)
        }
        "citation" => process_citation(node, node_text, children),
        "note_reference" => process_note_reference(node, children),
        "shortcode" | "shortcode_escaped" => process_shortcode(node, children),
        "shortcode_keyword_param" => process_shortcode_keyword_param(buf, node, children),
        "shortcode_boolean" => process_shortcode_boolean(node, input_bytes),
        "shortcode_number" => process_shortcode_number(node, input_bytes),
        "code_fence_content" => process_code_fence_content(node, children, input_bytes),
        "list_marker_parenthesis" | "list_marker_dot" => process_list_marker(node, input_bytes),
        // These are marker nodes, we don't need to do anything with it
        "block_quote_marker"
        | "list_marker_minus"
        | "list_marker_star"
        | "list_marker_plus"
        | "block_continuation"
        | "fenced_code_block_delimiter"
        | "note_reference_delimiter"
        | "shortcode_delimiter"
        | "citation_delimiter"
        | "code_span_delimiter"
        | "single_quoted_span_delimiter"
        | "double_quoted_span_delimiter"
        | "superscript_delimiter"
        | "subscript_delimiter"
        | "strikeout_delimiter"
        | "emphasis_delimiter"
        | "insert_delimiter"
        | "delete_delimiter"
        | "highlight_delimiter"
        | "edit_comment_delimiter" => {
            PandocNativeIntermediate::IntermediateUnknown(node_location(node))
        }
        "soft_line_break" => create_line_break_inline(node, false),
        "hard_line_break" => create_line_break_inline(node, true),
        "latex_span_delimiter" => {
            let str = node.utf8_text(input_bytes).unwrap();
            let range = node_location(node);
            if str == "$" {
                PandocNativeIntermediate::IntermediateLatexInlineDelimiter(range)
            } else if str == "$$" {
                PandocNativeIntermediate::IntermediateLatexDisplayDelimiter(range)
            } else {
                writeln!(
                    buf,
                    "Warning: Unrecognized latex_span_delimiter: {} Will assume inline delimiter",
                    str
                )
                .unwrap();
                PandocNativeIntermediate::IntermediateLatexInlineDelimiter(range)
            }
        }
        "inline_note" => process_emphasis_inline_with_node(
            node,
            children,
            "inline_note_delimiter",
            native_inline,
            |inlines, node| {
                Inline::Note(Note {
                    content: vec![Block::Paragraph(Paragraph {
                        content: inlines,
                        source_info: SourceInfo::with_range(node_location(node)),
                    })],
                    source_info: node_source_info(node),
                })
            },
        ),
        "superscript" => emphasis_inline!(
            node,
            children,
            "superscript_delimiter",
            native_inline,
            Superscript
        ),
        "subscript" => emphasis_inline!(
            node,
            children,
            "subscript_delimiter",
            native_inline,
            Subscript
        ),
        "strikeout" => emphasis_inline!(
            node,
            children,
            "strikeout_delimiter",
            native_inline,
            Strikeout
        ),
        "insert" => emphasis_inline!(node, children, "insert_delimiter", native_inline, Insert),
        "delete" => emphasis_inline!(node, children, "delete_delimiter", native_inline, Delete),
        "highlight" => emphasis_inline!(
            node,
            children,
            "highlight_delimiter",
            native_inline,
            Highlight
        ),
        "edit_comment" => emphasis_inline!(
            node,
            children,
            "edit_comment_delimiter",
            native_inline,
            EditComment
        ),

        "quoted_span" => process_quoted_span(node, children, native_inline),
        "code_span" => process_code_span(buf, node, children),
        "latex_span" => process_latex_span(node, children),
        "list" => process_list(node, children),
        "list_item" => process_list_item(node, children),
        "info_string" => process_info_string(children),
        "language_attribute" => process_language_attribute(children),
        "raw_attribute" => process_raw_attribute(node, children),
        "block_quote" => process_block_quote(buf, node, children),
        "fenced_div_block" => process_fenced_div_block(buf, node, children),
        "atx_heading" => process_atx_heading(buf, node, children),
        "thematic_break" => process_thematic_break(node),
        "backslash_escape" => process_backslash_escape(node, input_bytes),
        "minus_metadata" => {
            let text = node.utf8_text(input_bytes).unwrap();
            PandocNativeIntermediate::IntermediateMetadataString(
                text.to_string(),
                node_location(node),
            )
        }
        "uri_autolink" => process_uri_autolink(node, input_bytes),
        "pipe_table_delimiter_cell" => process_pipe_table_delimiter_cell(children),
        "pipe_table_header" | "pipe_table_row" => process_pipe_table_header_or_row(children),
        "pipe_table_delimiter_row" => process_pipe_table_delimiter_row(children),
        "pipe_table_cell" => process_pipe_table_cell(node, children),
        "pipe_table" => process_pipe_table(node, children),
        "setext_h1_underline" => PandocNativeIntermediate::IntermediateSetextHeadingLevel(1),
        "setext_h2_underline" => PandocNativeIntermediate::IntermediateSetextHeadingLevel(2),
        "setext_heading" => process_setext_heading(buf, node, children),
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
) -> Result<Pandoc, Vec<String>> {
    let result = bottomup_traverse_concrete_tree(
        &mut tree.walk(),
        &mut |node, children, input_bytes| native_visitor(buf, node, children, input_bytes),
        &input_bytes,
    );
    let (_, PandocNativeIntermediate::IntermediatePandoc(pandoc)) = result else {
        panic!("Expected Pandoc, got {:?}", result)
    };
    let result = desugar(pandoc)?;
    let result = merge_strs(result);
    Ok(result)
}
