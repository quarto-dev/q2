/*
 * test_fenced_div_no_space.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * A fenced div's colons do not have to be followed by whitespace. Pandoc and
 * Quarto 1 accept `:::{.foo}` and `:::foo` as readily as `::: {.foo}`, and so
 * must we. The scanner used to swallow a paragraph-following colon run into
 * the soft-line-break token, which erased the opening line — silently when no
 * closing fence followed it, and as a parse error at the *closing* fence when
 * one did (bd-div-attr-no-space-ne0fudkw).
 */

use pampa::pandoc::{Block, Inline};
use pampa::readers;

fn parse(input: &str) -> Vec<Block> {
    let (pandoc, _context, _warnings) = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .unwrap_or_else(|diags| {
        panic!(
            "expected a clean parse, got: {:?}",
            diags
                .iter()
                .map(|d| (d.code.clone(), d.problem.clone()))
                .collect::<Vec<_>>()
        )
    });
    pandoc.blocks
}

/// The classes of the first top-level `Div`. The bug's signature is that the
/// opening line disappears and the body degrades to a bare paragraph, so
/// insisting on a *top-level* div is the point of the assertion.
fn first_div_classes(blocks: &[Block]) -> Vec<String> {
    for block in blocks {
        if let Block::Div(div) = block {
            return div.attr.1.clone();
        }
    }
    panic!("no Div among the top-level blocks: {blocks:#?}");
}

fn plain_text(blocks: &[Block]) -> String {
    fn walk(inlines: &[Inline], out: &mut String) {
        for inline in inlines {
            match inline {
                Inline::Str(s) => out.push_str(&s.text),
                Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_) => out.push(' '),
                Inline::Code(c) => out.push_str(&c.text),
                Inline::Emph(e) => walk(&e.content, out),
                Inline::Strong(s) => walk(&s.content, out),
                Inline::Span(s) => walk(&s.content, out),
                _ => {}
            }
        }
    }
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                walk(&p.content, &mut out);
                out.push('\n');
            }
            Block::Plain(p) => {
                walk(&p.content, &mut out);
                out.push('\n');
            }
            Block::Div(d) => out.push_str(&plain_text(&d.content)),
            _ => {}
        }
    }
    out
}

#[test]
fn attribute_block_with_no_space_opens_a_div() {
    let blocks = parse("Before.\n\n:::{.myclass}\nInside.\n:::\n");
    assert_eq!(first_div_classes(&blocks), vec!["myclass".to_string()]);
}

#[test]
fn bare_info_string_with_no_space_opens_a_div() {
    let blocks = parse("Before.\n\n:::foo\nInside.\n:::\n");
    assert_eq!(first_div_classes(&blocks), vec!["foo".to_string()]);
}

#[test]
fn spaced_forms_still_open_a_div() {
    assert_eq!(
        first_div_classes(&parse("Before.\n\n::: {.foo}\nInside.\n:::\n")),
        vec!["foo".to_string()]
    );
    assert_eq!(
        first_div_classes(&parse("Before.\n\n::: foo\nInside.\n:::\n")),
        vec!["foo".to_string()]
    );
}

/// The silent half of the bug: with no closing fence the parse succeeded, the
/// class vanished, and the body came out as bare prose.
#[test]
fn no_space_and_no_closing_fence_still_opens_a_div() {
    let blocks = parse("Before.\n\n:::{.myclass}\nInside.\n");
    assert_eq!(first_div_classes(&blocks), vec!["myclass".to_string()]);
    let text = plain_text(&blocks);
    assert!(text.contains("Inside."), "body text was dropped: {text:?}");
}

/// A div opened without a space still fences off what follows it, so a code
/// block after the div is a code block and not a cascade of parse errors
/// pointing into its contents.
#[test]
fn a_code_fence_after_a_tight_div_is_unaffected() {
    let blocks = parse("Before.\n\n:::{.foo}\nInside.\n:::\n\n```json\n{ \"a\": 1 }\n```\n");
    assert_eq!(first_div_classes(&blocks), vec!["foo".to_string()]);
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b, Block::CodeBlock(c) if c.text.contains("\"a\": 1"))),
        "expected a json code block among {blocks:#?}"
    );
}

/// Colon runs that open nothing are ordinary text and must survive verbatim.
/// The same swallow that ate `:::` ate these — `:hello` rendered as `hello`.
#[test]
fn colon_runs_that_open_no_block_stay_in_the_paragraph() {
    let text = plain_text(&parse("Before.\n\n:hello\n::world\n"));
    assert!(
        text.contains(":hello"),
        "leading colon was dropped: {text:?}"
    );
    assert!(
        text.contains("::world"),
        "leading colons were dropped: {text:?}"
    );
}

/// A single colon followed by a space is a caption marker, and must keep
/// interrupting the paragraph above it rather than being absorbed as
/// continuation text. (A caption with nothing to attach to is reported and
/// dropped downstream, which is why this asserts on what does *not* reach the
/// paragraph.)
#[test]
fn a_caption_still_interrupts_a_paragraph() {
    let text = plain_text(&parse("Before.\n\n: A caption\n"));
    assert!(
        !text.contains("A caption"),
        "the caption line was absorbed into the paragraph: {text:?}"
    );
}
