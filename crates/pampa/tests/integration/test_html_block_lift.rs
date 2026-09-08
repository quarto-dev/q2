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
fn details_summary_splits_into_raw_tags_and_parsed_text() {
    // The shape that hit the Positron docs' download.qmd. The two tag lines are
    // one paragraph, and the lift keeps that extent — but the summary's text is
    // content, so it lifts out between the tags. Matches
    // `pandoc -f markdown-native_divs`, which gives the same four blocks.
    let blocks = blocks(
        "<details class=\"checksums-section\">\n<summary>SHA-256 checksums</summary>\n\nText inside the details.\n\n</details>\n",
    );

    // The opening tags are consecutive raw inlines, so they coalesce into one
    // RawBlock that keeps their original line break.
    match &blocks[0] {
        Block::RawBlock(r) => {
            assert_eq!(r.format, "html");
            assert_eq!(r.text, "<details class=\"checksums-section\">\n<summary>");
        }
        other => panic!(
            "expected a RawBlock spanning both tag lines, got {:?}",
            other
        ),
    }
    assert_eq!(inline_shape_text(&blocks[1]), "SHA-256 checksums");
    match &blocks[2] {
        Block::RawBlock(r) => assert_eq!(r.text, "</summary>"),
        other => panic!("expected the closing summary tag, got {:?}", other),
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
    let Block::BlockQuote(q) = &blocks[0] else {
        panic!("expected BlockQuote, got {:?}\n{}", blocks[0], flat);
    };

    // `<span>` is not block-level, so it is not part of the raw run: the quote
    // splits into the `<div>` tag and the span as content. Pandoc splits here
    // too (`RawBlock "<div class=\"a\">"`, then the span as a block).
    match &q.content[0] {
        Block::RawBlock(r) => assert_eq!(r.text, "<div class=\"a\">"),
        other => panic!("expected RawBlock inside the quote, got {:?}", other),
    }

    // The point of the test: no `> ` gutter anywhere in what the quote holds.
    assert!(
        !flat.contains("> <"),
        "the block-quote gutter leaked into the quoted content:\n{}",
        flat
    );
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
    // The summary's tags stay raw and its text is parsed, so it renders across
    // three lines. Quarto 1 and pandoc do the same; the whitespace is
    // insignificant inside `<summary>`, which is phrasing content.
    assert!(
        html.contains("<summary>\nSHA-256 checksums\n</summary>"),
        "summary tags should be raw with parsed text between them:\n{}",
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
fn naked_block_round_trips_as_written() {
    // Naked HTML never round-tripped as written — before the lift it came back
    // as an inline `` `<div …>`{=html} `` span, and the lift first normalised
    // it to a ```{=html} fence, the documented spelling.
    //
    // It now comes back verbatim, matching pandoc. The normalisation was
    // reversed deliberately: a fence around each tag separates it from the
    // `Plain` in a split `<div>`/text/`</div>`, so the text would come back as
    // a `Paragraph` and the round-trip would not be stable. Writing the tag
    // bare is also self-consistent — exactly the texts written bare here are
    // the ones the reader lifts back to a `RawBlock`.
    //
    // bd-block-html-adjacent-markdown-unparsed-0qnjuwuy
    let src = "<div class=\"a\">\n\nbody\n\n</div>\n";
    let out = roundtrip(src);
    assert_eq!(out, src, "naked block HTML should round-trip as written");

    // The blank lines are load-bearing: they keep `body` a Paragraph, so it
    // must not be pulled tight against the tags the way a `Plain` would be.
    let reparsed = blocks(&out);
    assert!(
        matches!(reparsed[1], Block::Paragraph(_)),
        "a blank-line-separated body stays a Paragraph, got {:?}",
        reparsed[1]
    );
}

// =============================================================================
// Split: markdown adjacent to a raw HTML block line still reaches the inline
// parser (pandoc's `markdown_in_html_blocks`)
//
// The lift above reproduces the HTML block's *extent* correctly, but it
// originally emitted the paragraph verbatim, which also switched off inline
// parsing inside it. Dropping `native_divs` costs the `<p>` wrapper and the
// `Div` node — intended. Dropping `markdown_in_html_blocks` costs inline
// markdown parsing — not intended, and a parity break against Quarto 1.
//
// Target shape, matching `pandoc -f markdown-native_divs`: runs of block-level
// raw HTML stay `RawBlock`, the text between them becomes `Plain`.
//
// bd-block-html-adjacent-markdown-unparsed-0qnjuwuy
// =============================================================================

/// The `Code` and `Emph` inlines in `block`, as a terse shape string.
fn inline_shape(block: &Block) -> String {
    let content = match block {
        Block::Plain(p) => &p.content,
        Block::Paragraph(p) => &p.content,
        other => panic!("expected Plain or Paragraph, got {:?}", other),
    };
    content
        .iter()
        .filter_map(|i| match i {
            pampa::pandoc::Inline::Code(c) => Some(format!("Code({})", c.text)),
            pampa::pandoc::Inline::Emph(_) => Some("Emph".to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn tight_div_splits_and_parses_its_interior() {
    let blocks =
        blocks("<div class=\"case-b\">\nText with a `code span` and *emphasis*.\n</div>\n");

    assert_eq!(
        blocks.len(),
        3,
        "expected RawBlock/Plain/RawBlock: {:#?}",
        blocks
    );
    match &blocks[0] {
        Block::RawBlock(r) => assert_eq!(r.text, "<div class=\"case-b\">"),
        other => panic!("expected RawBlock open tag, got {:?}", other),
    }
    assert!(
        matches!(blocks[1], Block::Plain(_)),
        "interior should be Plain, got {:?}",
        blocks[1]
    );
    assert_eq!(inline_shape(&blocks[1]), "Code(code span),Emph");
    match &blocks[2] {
        Block::RawBlock(r) => assert_eq!(r.text, "</div>"),
        other => panic!("expected RawBlock close tag, got {:?}", other),
    }
}

#[test]
fn details_summary_interior_is_parsed() {
    // The shape from the real bug report: Positron docs
    // assistant-chat-instructions.qmd:61-63 rendered literal backticks.
    let html = render_html(
        "<details>\n<summary>Example custom instructions</summary>\nThis uses a `quarto.instructions.md` file.\n</details>\n",
    );
    assert!(
        html.contains("<code>quarto.instructions.md</code>"),
        "the code span should be parsed, got:\n{}",
        html
    );
    assert!(
        !html.contains("`quarto.instructions.md`"),
        "no literal backticks should survive, got:\n{}",
        html
    );
}

#[test]
fn consecutive_tags_on_one_line_stay_one_raw_run() {
    // A non-first `RawInline` carries its leading whitespace in `text`, so the
    // block-tag test has to be applied to the trimmed text or the second and
    // third comments here read as ordinary content.
    let blocks = blocks("<!-- first --> <!-- second --> <!-- third -->\n");

    assert_eq!(blocks.len(), 1, "expected one RawBlock: {:#?}", blocks);
    assert!(
        matches!(&blocks[0], Block::RawBlock(_)),
        "expected RawBlock, got {:?}",
        blocks[0]
    );
}

#[test]
fn raw_text_elements_keep_their_content_verbatim() {
    // CommonMark's HTML block type 1: `pre`, `script`, `style` and `textarea`
    // are raw-text elements. Pandoc does not parse markdown inside them, so
    // neither do we — even though all four are in BLOCK_TAGS.
    for tag in ["pre", "script", "style", "textarea"] {
        let qmd = format!("<{tag}>\ntext with a `code span` and *emphasis*.\n</{tag}>\n");
        let blocks = blocks(&qmd);
        assert_eq!(
            blocks.len(),
            1,
            "<{tag}> should stay one verbatim RawBlock: {:#?}",
            blocks
        );
        let raw = only_raw_block(&blocks);
        assert!(
            raw.text.contains("`code span`"),
            "<{tag}> content should stay verbatim, got: {:?}",
            raw.text
        );
    }
}

#[test]
fn tag_only_paragraph_still_emits_a_single_raw_block() {
    // PR #646's shape, unchanged by the split: with nothing but tags in the
    // paragraph there is no interior to lift out.
    let blocks = blocks(
        "<details class=\"checksums-section\">\n<summary>SHA-256 checksums</summary>\n\nBody.\n\n</details>\n",
    );

    match &blocks[0] {
        Block::RawBlock(r) => assert!(
            r.text.starts_with("<details class=\"checksums-section\">"),
            "expected the details open tag, got {:?}",
            r.text
        ),
        other => panic!("expected RawBlock, got {:?}", other),
    }
}

#[test]
fn split_parts_carry_their_own_source_info() {
    // Cloning the paragraph's SourceInfo onto every part gives sibling blocks
    // identical overlapping ranges, which misleads the incremental writer.
    let blocks = blocks("<div>\nText inside.\n</div>\n");
    assert_eq!(blocks.len(), 3, "expected three blocks: {:#?}", blocks);

    let spans: Vec<(usize, usize)> = blocks
        .iter()
        .map(|b| {
            let si = match b {
                Block::RawBlock(r) => &r.source_info,
                Block::Plain(p) => &p.source_info,
                other => panic!("unexpected block {:?}", other),
            };
            match si {
                quarto_source_map::SourceInfo::Original {
                    start_offset,
                    end_offset,
                    ..
                } => (*start_offset, *end_offset),
                other => panic!("expected an Original source info, got {:?}", other),
            }
        })
        .collect();

    for (i, w) in spans.windows(2).enumerate() {
        assert!(
            w[0].1 <= w[1].0,
            "part {} [{}..{}] overlaps part {} [{}..{}]: {:?}",
            i,
            w[0].0,
            w[0].1,
            i + 1,
            w[1].0,
            w[1].1,
            spans
        );
    }
}

#[test]
fn tight_div_round_trips_to_a_stable_shape() {
    // A block-level html RawBlock writes verbatim rather than as a ```{=html}
    // fence, so the tags survive as written. What does *not* survive is the
    // tightness: the writer separates blocks, so the interior comes back as a
    // `Paragraph` rather than the `Plain` the reader produced.
    //
    // That is unavoidable without recording which blocks shared a paragraph,
    // which the AST does not do — and the tight rule that faked it merged
    // blocks that were never one paragraph. See the round-trip note in the
    // plan. The shape reaches a fixed point immediately, and the tags stay in
    // block position, which is what the lift exists to guarantee.
    let src = "<div class=\"case-b\">\nText with a `code span` and *emphasis*.\n</div>\n";
    let once = roundtrip(src);
    let twice = roundtrip(&once);
    assert_eq!(
        once, twice,
        "the round-trip should reach a fixed point:\n{}",
        once
    );

    let reparsed = blocks(&once);
    assert_eq!(
        reparsed.len(),
        3,
        "the tags must stay their own blocks: {:#?}",
        reparsed
    );
    assert!(matches!(reparsed[0], Block::RawBlock(_)));
    assert!(
        matches!(reparsed[1], Block::Paragraph(_)),
        "the interior canonicalizes to a Paragraph, got {:?}",
        reparsed[1]
    );
    assert!(matches!(reparsed[2], Block::RawBlock(_)));
    assert_eq!(inline_shape(&reparsed[1]), "Code(code span),Emph");
}

#[test]
fn split_round_trips_inside_a_block_quote() {
    // A container must keep the comment in block position and the text parsed.
    // As at the top level, the interior canonicalizes to a `Paragraph`.
    let out = roundtrip("> <!-- comment -->\n> Text after.\n");
    let reparsed = blocks(&out);

    let quoted = match &reparsed[0] {
        Block::BlockQuote(q) => &q.content,
        other => panic!("expected a BlockQuote, got {:?}", other),
    };
    assert!(
        matches!(quoted[0], Block::RawBlock(_)),
        "the comment should stay a RawBlock, got {:?}",
        quoted[0]
    );
    assert!(
        matches!(quoted[1], Block::Paragraph(_)),
        "the text should be its own parsed block, got {:?}",
        quoted[1]
    );
}

/// The plain-text content of a `Plain`/`Paragraph`, for terse assertions.
fn inline_shape_text(block: &Block) -> String {
    let content = match block {
        Block::Plain(p) => &p.content,
        Block::Paragraph(p) => &p.content,
        other => panic!("expected Plain or Paragraph, got {:?}", other),
    };
    content
        .iter()
        .map(|i| match i {
            pampa::pandoc::Inline::Str(s) => s.text.clone(),
            pampa::pandoc::Inline::Space(_) => " ".to_string(),
            _ => String::new(),
        })
        .collect()
}

// =============================================================================
// Regressions found in review of the split
// =============================================================================

#[test]
fn a_hard_line_break_after_the_tag_is_not_content() {
    // Trailing spaces on the tag line make the reader emit `Inline::LineBreak`
    // rather than `SoftBreak`. It is a separator like any other, so it must not
    // fall into the content run — doing so rendered a spurious `<br />` before
    // the text. The spaces are invisible in the source, so this looks to an
    // author like the engine inventing markup.
    let html = render_html("<div class=\"x\">  \ntext with a `code span`\n</div>\n");
    assert!(
        !html.contains("<br"),
        "a hard break at the tag boundary must not survive:\n{}",
        html
    );
    assert!(
        html.contains("<code>code span</code>"),
        "the interior should still be parsed:\n{}",
        html
    );
}

#[test]
fn a_hard_line_break_between_tags_invents_no_content() {
    // The same break with nothing between the tags must leave the pair a single
    // raw run, not a `Plain` holding the break alone — which rendered a stray
    // backslash. Pandoc emits `<div>\n</div>` and nothing else.
    let blocks = blocks("<div>   \n</div>\n");
    assert_eq!(
        blocks.len(),
        1,
        "an empty div should stay one raw run: {:#?}",
        blocks
    );
    let raw = only_raw_block(&blocks);
    assert_eq!(raw.text, "<div>\n</div>");
}

#[test]
fn an_authored_raw_fence_keeps_its_fence() {
    // Writing block-level raw HTML bare is only sound for the shapes the *lift*
    // produces — a pure run of tags, or a raw-text element. An author-written
    // ```{=html} fence can hold anything, and stripping its fence means the
    // next read parses its contents: a backtick becomes `Code`, and a `#` line
    // after a blank line becomes a real `Header`. That is content corruption
    // through any AST -> qmd -> read cycle.
    let src =
        "```{=html}\n<div class=\"authored\">\nstill `raw` inside\n\n# not a header\n</div>\n```\n";
    let out = roundtrip(src);
    assert!(
        out.contains("```{=html}"),
        "an authored fence with interior content must keep its fence:\n{}",
        out
    );

    let reparsed = blocks(&out);
    assert_eq!(
        reparsed.len(),
        1,
        "it must come back as one RawBlock, not parsed apart: {:#?}",
        reparsed
    );
    let raw = only_raw_block(&reparsed);
    assert!(
        raw.text.contains("still `raw` inside"),
        "the backticks must survive verbatim, got: {:?}",
        raw.text
    );
    assert!(
        raw.text.contains("# not a header"),
        "the hash line must survive verbatim, got: {:?}",
        raw.text
    );
}

#[test]
fn a_raw_text_element_still_round_trips_bare() {
    // The counterpart: `<pre>` and friends keep their interior verbatim on the
    // next read, so they are safe to write bare and should stay that way.
    let src = "<pre>\ntext with a `code span`\n</pre>\n";
    let out = roundtrip(src);
    assert_eq!(out, src, "a raw-text element should round-trip as written");
}

#[test]
fn split_round_trips_inside_an_ordered_list_item() {
    // An ordered item has its own block loop in the writer, separate from a
    // bullet item's; both must keep a split's tags in block position and its
    // interior parsed.
    let out = roundtrip("1. item\n\n   <div>\n   text `code`\n   </div>\n");
    let reparsed = blocks(&out);

    let items = match &reparsed[0] {
        Block::OrderedList(l) => &l.content,
        other => panic!("expected an OrderedList, got {:?}", other),
    };
    let split = &items[0];
    assert!(
        split.iter().any(|b| matches!(b, Block::RawBlock(_))),
        "the div's tags should stay in block position: {:#?}",
        split
    );
    assert!(
        split.iter().any(|b| matches!(
            b,
            Block::Paragraph(p)
                if p.content.iter().any(|i| matches!(i, pampa::pandoc::Inline::Code(_)))
        )),
        "the interior should still be parsed markdown: {:#?}",
        split
    );
}

#[test]
fn raw_text_elements_are_exempt_inside_a_container_too() {
    // The exemption is a property of the paragraph, not of the top level, so
    // it has to hold wherever a lift can happen. A block quote is the awkward
    // one — its `> ` gutter is stripped on the way in.
    let blocks = blocks("> <pre>\n> raw `code` *em*\n> </pre>\n");
    let Block::BlockQuote(q) = &blocks[0] else {
        panic!("expected a BlockQuote, got {:?}", blocks[0]);
    };
    assert_eq!(
        q.content.len(),
        1,
        "a <pre> inside a quote should stay one verbatim RawBlock: {:#?}",
        q.content
    );
    match &q.content[0] {
        Block::RawBlock(r) => assert!(
            r.text.contains("raw `code` *em*"),
            "the interior should stay verbatim, got: {:?}",
            r.text
        ),
        other => panic!("expected RawBlock, got {:?}", other),
    }
}

#[test]
fn a_closing_raw_text_tag_does_not_start_an_exemption() {
    // CommonMark's type-1 start condition is the *opening* tag. A paragraph
    // opening with `</pre>` is an ordinary tag run, so its interior is parsed.
    let blocks = blocks("</pre>\ntext with a `code span`\n");
    assert!(
        blocks.len() > 1,
        "a closing tag should not exempt the paragraph: {:#?}",
        blocks
    );
    assert_eq!(inline_shape(&blocks[1]), "Code(code span)");
}

#[test]
fn a_metadata_block_scalar_splits_without_a_document_span() {
    // Not every inline the split sees carries a file offset. A metadata block
    // scalar — `include-in-header: - text: |` — is parsed as markdown, and its
    // inlines carry the YAML value's provenance instead. `span_of` has to cope
    // rather than assume `SourceInfo::Original`; assuming it panicked the whole
    // render (quarto-core's `text_block_markdown_reaches_head_via_full_pipeline`).
    let qmd = "---\ninclude-in-header:\n  - text: |\n      <meta name=\"a\" content=\"1\">\n\n      <meta name=\"b\" content=\"2\">\n---\n\nBody.\n";
    let (pandoc, _context, _warnings) = readers::qmd::read(
        qmd.as_bytes(),
        false,
        "test.md",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("a metadata block scalar holding raw HTML should parse");

    assert!(
        pandoc.meta.get("include-in-header").is_some(),
        "the metadata key should survive: {:?}",
        pandoc.meta
    );
}

// =============================================================================
// The writer keeps blocks apart
//
// A split trio cannot be written *tight* to preserve its `Plain`s. Tightness
// needs to know that two blocks came from one paragraph, and the AST does not
// record that — a `Plain` next to a `RawBlock` may be a split interior or two
// unrelated blocks, and a filter can produce either. Writing them tight merged
// blocks that were never one paragraph, which is how a `<script>`'s contents
// came to be parsed as markdown.
//
// So the writer always separates blocks, and a split interior comes back as a
// `Paragraph`. See the round-trip note in the plan.
// =============================================================================

#[test]
fn a_following_raw_text_element_keeps_its_own_paragraph() {
    // The regression that killed the exemption: pulling a `Plain` tight against
    // the next paragraph's `<script>` merged them, so `<script>` no longer
    // *began* a paragraph and its contents were parsed. JavaScript is not
    // markdown; a backtick in it must not become a code span.
    let src = "<div class=\"note\">\nSee below.\n\n<script>\nconst t = `hi`;\n</script>\n";
    let out = roundtrip(src);
    let reparsed = blocks(&out);

    let script = reparsed
        .iter()
        .find_map(|b| match b {
            Block::RawBlock(r) if r.text.contains("script") => Some(r),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a <script> RawBlock after a round-trip: {:#?}",
                reparsed
            )
        });
    assert!(
        script.text.contains("const t = `hi`;"),
        "the script body must survive verbatim, got: {:?}",
        script.text
    );
}

#[test]
fn a_round_trip_does_not_reopen_the_paragraph_wrapper_bug() {
    // Tightness let a closing tag fall back into a paragraph, restoring the
    // `<p><div>` shape bd-block-html-wrapped-in-p-w8qebxig fixed. Any raw HTML
    // in the source, however awkward, must still land in block position after a
    // write/read cycle. The `>` inside the comment is the awkward part.
    let out = roundtrip("<div>\n<!-- fix: a -> b -->\ntext\n</div>\n");
    let html = {
        let mut buf = Vec::new();
        write_blocks_to(&blocks(&out), &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    };
    assert!(
        !html.contains("<p><div") && !html.contains("</div></p>"),
        "a round-trip must not put block HTML inside a paragraph:\n{}",
        html
    );
}

#[test]
fn unrelated_blocks_are_not_merged_by_the_writer() {
    // A `Plain` adjacent to a `RawBlock` need not have come from one paragraph —
    // a filter can emit any sequence, and this one is built directly rather than
    // parsed so that it really is `Plain`. Writing them tight collapsed all four
    // blocks into a single `Paragraph` holding both tags as inlines.
    use pampa::pandoc::block::{Plain, RawBlock};
    use pampa::pandoc::inline::Str;
    use quarto_source_map::{FileId, SourceInfo};

    let span = || SourceInfo::original(FileId(0), 0, 0);
    let plain = |text: &str| {
        Block::Plain(Plain {
            content: vec![pampa::pandoc::Inline::Str(Str {
                text: text.to_string(),
                source_info: span(),
            })],
            source_info: span(),
        })
    };
    let raw = |text: &str| {
        Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text: text.to_string(),
            source_info: span(),
        })
    };

    let pandoc = pampa::pandoc::Pandoc {
        meta: Default::default(),
        blocks: vec![plain("alpha"), raw("<div>"), plain("beta"), raw("</div>")],
    };
    let mut out = Vec::new();
    pampa::writers::qmd::write(&pandoc, &mut out).expect("should write");
    let out = String::from_utf8(out).unwrap();

    let reparsed = blocks(&out);
    assert_eq!(
        reparsed.len(),
        4,
        "the four blocks must stay four after a round-trip, got:\n{}\n{:#?}",
        out,
        reparsed
    );
}

#[test]
fn a_comment_containing_an_angle_bracket_still_writes_bare() {
    // `line_is_only_tags` scanned to the first `>`, so the `>` in `a -> b` ended
    // the "tag" early and the run was fenced unnecessarily. Harmless to the
    // rendered HTML, but it churns the source for no reason.
    let out = roundtrip("<div>\n<!-- fix: a -> b -->\n\ntext\n\n</div>\n");
    assert!(
        !out.contains("```{=html}"),
        "a pure tag run should stay bare even when a comment holds a `>`:\n{}",
        out
    );
}
