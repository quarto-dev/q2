//! List items keep their `<p>` when the item is a `Para` (bd-nnvvomg6).
//!
//! Pandoc's HTML writer decides on `<p>` from the block kind alone: a tight
//! item is `Plain` and renders bare (`<li>a</li>`), and a loose item is
//! `Para` and renders `<li><p>a</p></li>`. The reader already produces
//! Pandoc's AST for both cases (see `list_looseness.rs`). These tests pin the
//! writer side, which once unwrapped any single-`Para` item. A multi-block
//! item keeps the writer's `\n</li>` whitespace; that difference from Pandoc
//! is deliberate and has no visual effect.

use pampa::readers;
use pampa::writers;

fn to_html(input: &str) -> String {
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("Failed to parse QMD");
    let (pandoc, ctx) = (result.0, result.1);
    let mut buf = Vec::new();
    writers::html::write(&pandoc, &ctx, &mut buf).expect("html write");
    String::from_utf8(buf).expect("utf8")
}

#[test]
fn loose_bullet_items_keep_their_paragraphs() {
    let html = to_html("- a\n\n- b\n");
    assert!(html.contains("<li><p>a</p>"), "loose item a: {html}");
    assert!(html.contains("<li><p>b</p>"), "loose item b: {html}");
}

#[test]
fn loose_ordered_items_keep_their_paragraphs() {
    let html = to_html("1. one\n\n2. two\n");
    assert!(html.contains("<li><p>one</p>"), "loose item one: {html}");
    assert!(html.contains("<li><p>two</p>"), "loose item two: {html}");
}

#[test]
fn single_paragraph_item_in_a_loose_list_keeps_its_paragraph() {
    // The first item has two paragraphs, which makes the whole list loose;
    // the second item is a lone `Para` and must still get its `<p>`.
    let html = to_html("- a\n\n  second\n- b\n");
    assert!(html.contains("<li><p>a</p>"), "first item: {html}");
    assert!(html.contains("<p>second</p>"), "first item, para 2: {html}");
    assert!(html.contains("<li><p>b</p>"), "second item: {html}");
}

#[test]
fn tight_items_render_bare() {
    let html = to_html("- a\n- b\n");
    assert!(html.contains("<li>a</li>"), "tight item a: {html}");
    assert!(html.contains("<li>b</li>"), "tight item b: {html}");
    assert!(!html.contains("<p>"), "a tight list has no <p>: {html}");
}

#[test]
fn hoisted_attr_on_a_loose_item_stays_on_the_li() {
    // A trailing block attribute is hoisted onto the `<li>` (bd-aeyss6p5);
    // the inner `<p>` must not carry it too.
    let html = to_html("- a {.extra}\n\n- b\n");
    assert!(
        html.contains("<li class=\"extra\"><p>a</p>"),
        "attr on the li, bare p inside: {html}"
    );
}
