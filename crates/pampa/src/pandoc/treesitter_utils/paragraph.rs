/*
 * paragraph.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::{Block, Paragraph, Plain, RawBlock};
use crate::pandoc::inline::{Inline, InlineAttr};
use crate::pandoc::location::node_source_info_with_context;
use crate::pandoc::treesitter_utils::html_block::{starts_block_html, starts_raw_text_element};
use crate::pandoc::treesitter_utils::pandocnativeintermediate::PandocNativeIntermediate;
use quarto_source_map::{FileId, SourceInfo};

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
            let source_info = node_source_info_with_context(node, context);

            // Runs of tags stay raw; the text between them goes back through
            // the inline parser. This is pandoc's `markdown_in_html_blocks`.
            // Raw-text elements (`<pre>`, `<script>`, …) are exempt: their
            // content is not markdown in the first place.
            if !paragraph_starts_raw_text_element(&inlines) {
                let parts = split_html_block_runs(&inlines, &source_info);
                if parts.len() > 1 {
                    return PandocNativeIntermediate::IntermediateSection(parts);
                }
            }

            return PandocNativeIntermediate::IntermediateBlock(Block::RawBlock(RawBlock {
                format: "html".to_string(),
                text,
                source_info,
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
/// The text is trimmed before the test, as in [`is_block_html`] and
/// [`paragraph_starts_raw_text_element`]: the grammar happens to emit leading
/// whitespace as its own `Space` inline, so it makes no difference here today,
/// but three predicates asking the same question should not answer it three
/// different ways.
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
                return raw.format == "html" && starts_block_html(raw.text.trim_start());
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

/// Does this paragraph open a raw-text element (`<pre>`, `<script>`, …)?
///
/// Their content is not markdown, so the whole paragraph stays one verbatim
/// `RawBlock` rather than being split. Mirrors the leading-whitespace skipping
/// in [`paragraph_starts_block_html`].
fn paragraph_starts_raw_text_element(inlines: &[Inline]) -> bool {
    for inline in inlines {
        match inline {
            Inline::Space(_) | Inline::SoftBreak(_) => continue,
            Inline::RawInline(raw) => {
                return raw.format == "html" && starts_raw_text_element(raw.text.trim_start());
            }
            _ => return false,
        }
    }
    false
}

/// Is this inline a block-level raw HTML tag?
///
/// The text is trimmed first: only the *first* inline of a paragraph starts at
/// a token boundary. tree-sitter folds the whitespace preceding a later inline
/// into that inline's text, so `<!-- a --> <!-- b -->` yields a second
/// `RawInline` whose text is `" <!-- b -->"` — which `starts_block_html`
/// rejects, since it tests for a leading `<`.
fn is_block_html(inline: &Inline) -> bool {
    matches!(inline, Inline::RawInline(raw)
        if raw.format == "html" && starts_block_html(raw.text.trim_start()))
}

/// The span covering `inlines`, in the offset space of `fallback`.
///
/// Each part of a split needs its own range: handing every part the whole
/// paragraph's `SourceInfo` gives sibling blocks identical overlapping spans,
/// which misleads the incremental writer about what text each block owns.
fn span_of<'a>(infos: impl Iterator<Item = &'a SourceInfo>, fallback: &SourceInfo) -> SourceInfo {
    let mut first: Option<&SourceInfo> = None;
    let mut bounds: Option<(FileId, usize, usize)> = None;
    let mut all_original = true;

    for info in infos {
        if first.is_none() {
            first = Some(info);
        }
        match info {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => {
                bounds = Some(match bounds {
                    None => (*file_id, *start_offset, *end_offset),
                    Some((f, s, e)) if f == *file_id => {
                        (f, s.min(*start_offset), e.max(*end_offset))
                    }
                    // Offsets from two files would union into nonsense.
                    Some(prev) => {
                        all_original = false;
                        prev
                    }
                });
            }
            _ => all_original = false,
        }
    }

    match (all_original, bounds) {
        // The common case: every inline came from the document, so union them
        // into one tight range.
        (true, Some((file_id, start_offset, end_offset))) => SourceInfo::Original {
            file_id,
            start_offset,
            end_offset,
        },
        // Not everything here carries a file offset. Metadata block scalars
        // reach this arm: `include-in-header: - text: |` is parsed as markdown,
        // and its inlines carry the YAML value's provenance rather than a range
        // in the document. Take the run's *first* span rather than the whole
        // paragraph's — it is distinct per run, so sibling blocks do not all
        // collide on one range the way the paragraph's would.
        _ => first.cloned().unwrap_or_else(|| fallback.clone()),
    }
}

/// Split a lifted paragraph into raw-HTML runs and parsed content.
///
/// Consecutive block-level raw HTML tags coalesce into one `RawBlock`, joined
/// by newlines so the original line structure survives; everything else
/// becomes a `Plain`. Both decisions are local — each inline is classified on
/// its own, with no tag matching and no lookahead — so this stays inside the
/// no-backtracking constraint in `dev-docs/syntax-notes.md`.
///
/// The interiors are always `Plain`, never `Paragraph`. Pandoc picks between
/// the two by tracking which element is still open, which needs the balanced
/// matching we do not do; the difference is only whether a `<p>` wrapper
/// appears, which is the `native_divs` gap qmd already accepts. See
/// `claude-notes/research/2026-09-03-block-html-adjacent-markdown-unparsed.md`.
fn split_html_block_runs(inlines: &[Inline], paragraph_si: &SourceInfo) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut raw_run: Vec<&Inline> = Vec::new();
    let mut content_run: Vec<Inline> = Vec::new();

    fn flush_raw(run: &mut Vec<&Inline>, out: &mut Vec<Block>, fallback: &SourceInfo) {
        if run.is_empty() {
            return;
        }
        let text = run
            .iter()
            .filter_map(|i| match i {
                Inline::RawInline(raw) => Some(raw.text.trim()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let source_info = span_of(run.iter().map(|i| i.source_info()), fallback);
        out.push(Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text,
            source_info,
        }));
        run.clear();
    }

    fn flush_content(run: &mut Vec<Inline>, out: &mut Vec<Block>, fallback: &SourceInfo) {
        while matches!(
            run.last(),
            Some(Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_))
        ) {
            run.pop();
        }
        if run.is_empty() {
            return;
        }
        let source_info = span_of(run.iter().map(|i| i.source_info()), fallback);
        out.push(Block::Plain(Plain {
            content: std::mem::take(run),
            source_info,
        }));
    }

    for inline in inlines {
        if is_block_html(inline) {
            flush_content(&mut content_run, &mut blocks, paragraph_si);
            raw_run.push(inline);
        } else if matches!(
            inline,
            Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_)
        ) {
            // A separator between two raw tags is the line break the join
            // above re-creates, so it only matters inside a content run.
            //
            // `LineBreak` belongs here as much as `SoftBreak` does: trailing
            // spaces on a tag line are invisible in the source but make the
            // reader emit a hard break, and treating that as content rendered
            // a spurious `<br />` before the text — or, with nothing between
            // the tags, a `Plain` holding the break alone.
            if !content_run.is_empty() {
                content_run.push(inline.clone());
            }
        } else {
            flush_raw(&mut raw_run, &mut blocks, paragraph_si);
            content_run.push(inline.clone());
        }
    }
    flush_content(&mut content_run, &mut blocks, paragraph_si);
    flush_raw(&mut raw_run, &mut blocks, paragraph_si);

    blocks
}
