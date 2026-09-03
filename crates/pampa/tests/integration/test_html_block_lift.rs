/*
 * test_html_block_lift.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * A paragraph that begins with a block-level raw HTML tag is lifted to a
 * `Block::RawBlock` instead of staying a `Paragraph` full of `RawInline`s.
 *
 * Without the lift, a standalone `<div>` or `<details>` is emitted inside a
 * `<p>`. That is invalid HTML — both are flow content, not phrasing content —
 * so a browser's HTML5 parser force-closes the `<p>` and reparents everything
 * that follows, giving the page a DOM that the markup does not describe.
 *
 * Naked HTML remains an unsupported authoring form in qmd (see
 * `crates/pampa/README.md` "Important differences" and
 * `dev-docs/syntax-notes.md` "No naked HTML support"): the documented
 * spellings are a `{=html}` raw block/inline or a `::: {.class}` fenced div.
 * The Q-2-9 warning still fires on a lifted block — this makes the fallback
 * emit *valid* HTML, it does not promote naked HTML to a supported feature.
 *
 * bd-block-html-wrapped-in-p-w8qebxig
 */

use pampa::pandoc::{ASTContext, Block, treesitter_to_pandoc};
use pampa::readers;
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use pampa::writers::html::write_blocks_to;
use tree_sitter_qmd::MarkdownParser;

/// Parse a qmd body (no front matter) into blocks.
fn blocks(qmd: &str) -> Vec<Block> {
    let input_bytes = qmd.as_bytes();
    let mut parser = MarkdownParser::default();
    let tree = parser.parse(input_bytes, None).expect("failed to parse");
    let mut error_collector = DiagnosticCollector::new();
    treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        input_bytes,
        &ASTContext::anonymous(),
        &mut error_collector,
    )
    .unwrap()
    .blocks
}

/// Render a qmd body to an HTML body fragment.
fn render_html(qmd: &str) -> String {
    let mut output = Vec::new();
    write_blocks_to(&blocks(qmd), &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

/// The one `RawBlock` in `blocks`, panicking if there isn't exactly one.
fn only_raw_block(blocks: &[Block]) -> &pampa::pandoc::block::RawBlock {
    let found: Vec<_> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::RawBlock(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one RawBlock, got blocks: {:#?}",
        blocks
    );
    found[0]
}

// =============================================================================
// Lifted: a paragraph beginning with a block-level tag becomes a RawBlock
// =============================================================================

#[test]
fn bare_div_becomes_raw_block() {
    let blocks = blocks("<div class=\"index-section\">\n\nText inside the div.\n\n</div>\n");

    // open tag, paragraph, close tag
    assert_eq!(blocks.len(), 3, "unexpected blocks: {:#?}", blocks);

    match &blocks[0] {
        Block::RawBlock(r) => {
            assert_eq!(r.format, "html");
            assert_eq!(r.text, "<div class=\"index-section\">");
        }
        other => panic!("expected RawBlock for the open tag, got {:?}", other),
    }
    assert!(
        matches!(blocks[1], Block::Paragraph(_)),
        "the text between the tags should stay a Paragraph: {:?}",
        blocks[1]
    );
    match &blocks[2] {
        Block::RawBlock(r) => assert_eq!(r.text, "</div>"),
        other => panic!("expected RawBlock for the close tag, got {:?}", other),
    }
}

#[test]
fn details_summary_becomes_one_raw_block_verbatim() {
    // The shape that hit the Positron docs' download.qmd. The whole paragraph —
    // both source lines — becomes one RawBlock, which is also what CommonMark's
    // HTML block type 6 produces (it runs to the next blank line, exactly as a
    // paragraph does).
    let blocks = blocks(
        "<details class=\"checksums-section\">\n<summary>SHA-256 checksums</summary>\n\nText inside the details.\n\n</details>\n",
    );

    match &blocks[0] {
        Block::RawBlock(r) => {
            assert_eq!(r.format, "html");
            assert_eq!(
                r.text,
                "<details class=\"checksums-section\">\n<summary>SHA-256 checksums</summary>"
            );
        }
        other => panic!("expected a RawBlock spanning both lines, got {:?}", other),
    }
}

#[test]
fn standalone_html_comment_becomes_raw_block() {
    let blocks = blocks("<!--Hero banner section-->\n");
    let raw = only_raw_block(&blocks);
    assert_eq!(raw.format, "html");
    assert_eq!(raw.text, "<!--Hero banner section-->");
}

#[test]
fn closing_tag_alone_becomes_raw_block() {
    let blocks = blocks("text\n\n</section>\n");
    match &blocks[1] {
        Block::RawBlock(r) => assert_eq!(r.text, "</section>"),
        other => panic!("expected RawBlock for a lone closing tag, got {:?}", other),
    }
}

// NOTE: `<!DOCTYPE html>` and `<?xml …?>` are recognised by
// `html_block::starts_block_html` (CommonMark HTML block types 3 and 4), but
// the reader never sees them as raw HTML: the scanner routes a leading `<!`
// into comment lexing, which fails, and the document errors out with
// "unexpected character or token here". That is a pre-existing scanner gap,
// present on released q2 0.28.0, and out of scope for the block lift — it is
// upstream of any paragraph handling. Tracked separately.

#[test]
fn container_prefixes_are_stripped_from_a_multi_line_block() {
    // A block quote's `> ` gutter must not leak into the raw HTML. It has no
    // `block_continuation` node to skip — the prefix is absorbed into the
    // `pandoc_soft_break` that spans the line ending, so `verbatim_text` emits
    // a bare newline for each break rather than copying its source text.
    let blocks = blocks("> <div class=\"a\">\n> <span>x</span>\n");
    let flat = format!("{:#?}", blocks);
    match &blocks[0] {
        Block::BlockQuote(q) => match &q.content[0] {
            Block::RawBlock(r) => {
                assert_eq!(r.text, "<div class=\"a\">\n<span>x</span>");
            }
            other => panic!("expected RawBlock inside the quote, got {:?}", other),
        },
        other => panic!("expected BlockQuote, got {:?}\n{}", other, flat),
    }
}

#[test]
fn lift_happens_inside_a_list_item() {
    let blocks = blocks("- item\n\n  <div class=\"a\">\n");
    let flat = format!("{:?}", blocks);
    assert!(
        flat.contains("RawBlock"),
        "a block-level tag inside a list item should still lift: {:#?}",
        blocks
    );
}

// =============================================================================
// Not lifted: shapes that stay inline, matching Quarto 1 / pandoc
// =============================================================================

#[test]
fn inline_tags_in_running_text_stay_inline() {
    let blocks = blocks("<b>hello world</b>\n");
    assert!(
        matches!(blocks[0], Block::Paragraph(_)),
        "an inline element must not lift: {:?}",
        blocks[0]
    );
}

#[test]
fn img_alone_on_a_line_stays_inline() {
    // `img` is not a block-level tag for pandoc's markdown reader, so Quarto 1
    // wraps it in <p>. Lifting it would be a *new* parity break, in a common
    // shape — so it must stay inline. (CommonMark's HTML block type 7 would
    // lift it; we deliberately do not implement type 7.)
    let blocks = blocks("<img src=\"a.png\">\n");
    assert!(
        matches!(blocks[0], Block::Paragraph(_)),
        "a lone <img> must stay in a paragraph for Q1 parity: {:?}",
        blocks[0]
    );
}

#[test]
fn span_alone_on_a_line_stays_inline() {
    let blocks = blocks("<span class=\"x\">\nfoo\n</span>\n");
    assert!(
        matches!(blocks[0], Block::Paragraph(_)),
        "a lone <span> must stay in a paragraph for Q1 parity: {:?}",
        blocks[0]
    );
}

#[test]
fn block_tag_mid_paragraph_does_not_lift() {
    // The tag is not the start of the paragraph, so the paragraph is left
    // alone. (Pandoc would interrupt the paragraph here; documented gap.)
    let blocks = blocks("text before <div>x</div> text after\n");
    assert!(
        matches!(blocks[0], Block::Paragraph(_)),
        "expected a single paragraph, got {:?}",
        blocks[0]
    );
}

// =============================================================================
// HTML output
// =============================================================================

#[test]
fn lifted_block_is_not_wrapped_in_p() {
    let html = render_html("<div class=\"index-section\">\n\nText inside the div.\n\n</div>\n");
    assert!(
        !html.contains("<p><div"),
        "the div must not be wrapped in a paragraph:\n{}",
        html
    );
    assert!(
        !html.contains("</div></p>"),
        "the closing div must not be wrapped in a paragraph:\n{}",
        html
    );
    assert!(
        html.contains("<div class=\"index-section\">"),
        "the div should be emitted verbatim:\n{}",
        html
    );
    assert!(
        html.contains("<p>Text inside the div.</p>"),
        "the body text should still be a paragraph:\n{}",
        html
    );
}

#[test]
fn details_renders_without_paragraph_wrappers() {
    let html = render_html(
        "<details class=\"checksums-section\">\n<summary>SHA-256 checksums</summary>\n\nText inside the details.\n\n</details>\n",
    );
    assert!(
        !html.contains("<p><details"),
        "details must not be wrapped in a paragraph:\n{}",
        html
    );
    assert!(
        !html.contains("</details></p>"),
        "closing details must not be wrapped in a paragraph:\n{}",
        html
    );
    assert!(
        html.contains("<summary>SHA-256 checksums</summary>"),
        "summary should be emitted verbatim:\n{}",
        html
    );
}

// =============================================================================
// Diagnostics: naked HTML is still deprecated, so the warning stays
// =============================================================================

#[test]
fn lifted_block_still_warns_q_2_9() {
    let (_pandoc, _context, warnings) = readers::qmd::read(
        "<div class=\"index-section\">\n".as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("should parse");

    let q29: Vec<_> = warnings
        .iter()
        .filter(|w| w.code.as_deref() == Some("Q-2-9"))
        .collect();
    assert!(
        !q29.is_empty(),
        "a lifted block must still warn — naked HTML stays unsupported; got {:?}",
        warnings
    );
}

#[test]
fn explicit_raw_block_does_not_warn() {
    // The documented spelling stays warning-free.
    let (pandoc, _context, warnings) = readers::qmd::read(
        "```{=html}\n<div class=\"index-section\">\n```\n".as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("should parse");

    assert!(
        !warnings.iter().any(|w| w.code.as_deref() == Some("Q-2-9")),
        "an explicit {{=html}} block must not warn: {:?}",
        warnings
    );
    assert!(
        matches!(pandoc.blocks[0], Block::RawBlock(_)),
        "expected RawBlock: {:?}",
        pandoc.blocks[0]
    );
}

// =============================================================================
// qmd round-trip
// =============================================================================

/// Read qmd and write it back out as qmd.
fn roundtrip(qmd: &str) -> String {
    let (pandoc, _context, _warnings) = pampa::readers::qmd::read(
        qmd.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("should parse");

    let mut output = Vec::new();
    pampa::writers::qmd::write(&pandoc, &mut output).expect("should write");
    String::from_utf8(output).unwrap()
}

#[test]
fn standalone_comment_round_trips_in_native_syntax() {
    // bd-1066 made HTML comments round-trip as `<!-- … -->` rather than as a
    // ```{=html} fence. A comment on its own line is now a RawBlock rather than
    // a Paragraph of RawInlines, so the block writer needs the same case.
    let out = roundtrip("para one\n\n<!-- a standalone comment -->\n\npara two\n");
    assert!(
        out.contains("<!-- a standalone comment -->"),
        "comment should round-trip in native syntax, got:\n{}",
        out
    );
    assert!(
        !out.contains("{=html}"),
        "comment should not become a raw fence, got:\n{}",
        out
    );
}

#[test]
fn naked_block_round_trips_as_an_explicit_raw_fence() {
    // Naked HTML never round-tripped as written — before the lift it came back
    // as an inline `` `<div …>`{=html} `` span. It now comes back as a block
    // fence, which is the documented spelling for block-level raw HTML.
    let out = roundtrip("<div class=\"a\">\n\nbody\n\n</div>\n");
    assert!(
        out.contains("```{=html}"),
        "expected an explicit raw block, got:\n{}",
        out
    );
    assert!(
        out.contains("<div class=\"a\">"),
        "the tag text should survive, got:\n{}",
        out
    );
}
