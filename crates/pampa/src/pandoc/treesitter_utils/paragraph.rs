/*
 * paragraph.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, Paragraph, RawBlock};
use crate::pandoc::inline::{Inline, InlineAttr};
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::treesitter_utils::html_block::starts_block_html;
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;

/// Process a paragraph node, collecting inlines and filtering out block continuations
pub fn process_paragraph(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut inlines: Vec<Inline> = Vec::new();
    for (node, child) in children {
        if node == "block_continuation" {
            continue; // skip block continuation nodes
        }
        if let PandocNativeIntermediate::IntermediateInline(inline) = child {
            inlines.push(inline);
        } else if let PandocNativeIntermediate::IntermediateInlines(inner_inlines) = child {
            inlines.extend(inner_inlines);
        } else if let PandocNativeIntermediate::IntermediateAttr(attr, attr_source, attr_si) = child
        {
            // Attributes can appear in paragraphs (e.g., after math expressions)
            // They will be processed by postprocess.rs to create Spans
            inlines.push(Inline::Attr(InlineAttr::new(attr, attr_source, attr_si)));
        }
    }

    if paragraph_starts_block_html(&inlines) {
        // A block-level raw HTML tag opens this paragraph, so the paragraph is
        // really an HTML block: emit it verbatim as a RawBlock rather than
        // wrapping it in a <p> it is not allowed to sit inside. The extent is
        // unchanged — a paragraph and a CommonMark type-6 HTML block both run
        // to the next blank line. See html_block.rs and
        // bd-block-html-wrapped-in-p-w8qebxig.
        let text = verbatim_text(node, input_bytes);
        if !text.is_empty() {
            return PandocNativeIntermediate::IntermediateBlock(Block::RawBlock(RawBlock {
                format: "html".to_string(),
                text,
                source_info: node_source_info_with_context(node, context),
            }));
        }
    }

    PandocNativeIntermediate::IntermediateBlock(Block::Paragraph(Paragraph {
        content: inlines,
        source_info: node_source_info_with_context(node, context),
    }))
}

/// Does this paragraph open with raw HTML that belongs in a block position?
///
/// Leading whitespace is skipped: the grammar admits `_inline_whitespace`
/// before the first element, and CommonMark allows an HTML block to be
/// indented. Anything else before the tag — ordinary text, a link, an
/// emphasis run — means the tag is mid-paragraph, where pandoc would
/// interrupt the paragraph and we deliberately do not.
fn paragraph_starts_block_html(inlines: &[Inline]) -> bool {
    for inline in inlines {
        match inline {
            Inline::Space(_) | Inline::SoftBreak(_) => continue,
            Inline::RawInline(raw) => {
                return raw.format == "html" && starts_block_html(&raw.text);
            }
            _ => return false,
        }
    }
    false
}

/// The paragraph's source text, with container prefixes removed.
///
/// A plain `node.utf8_text` would drag the `> ` of an enclosing block quote —
/// or the gutter of an enclosing list item — into the raw HTML, because the
/// paragraph's byte range spans them.
///
/// There is no separate `block_continuation` node to skip: the container
/// prefix is absorbed into the line-break node itself. `> <div>\n> <span>`
/// parses as
///
/// ```text
/// (pandoc_paragraph [0, 2] - [2, 0]
///   (html_element     [0, 2] - [0, 17])
///   (pandoc_soft_break [0, 17] - [1, 2])   <- spans "\n> "
///   (html_element     [1, 2] - [1, 8]))
/// ```
///
/// so each break is emitted as a bare newline and everything else is copied
/// verbatim. At the top level a soft break spans exactly the newline, which
/// makes this a no-op there.
fn verbatim_text(node: &tree_sitter::Node, input_bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut pos = node.start_byte();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.start_byte() > pos {
            out.push_str(&String::from_utf8_lossy(
                &input_bytes[pos..child.start_byte()],
            ));
        }
        match child.kind() {
            "pandoc_soft_break" | "pandoc_line_break" | "block_continuation" => out.push('\n'),
            _ => out.push_str(&String::from_utf8_lossy(
                &input_bytes[child.start_byte()..child.end_byte()],
            )),
        }
        pos = pos.max(child.end_byte());
    }
    if pos < node.end_byte() {
        out.push_str(&String::from_utf8_lossy(&input_bytes[pos..node.end_byte()]));
    }

    out.trim().to_string()
}
