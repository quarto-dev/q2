/*
 * test_fenced_div_sigils.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Fenced-div "sigils": the token right after `::: ` that turns a fenced div
 * opener into a different construct (`::: ^id` note definitions, and the
 * block-level editorial marks `::: ++`, `::: --`, `::: >>`, `::: !!`). See
 * `parse_fenced_div_sigil` in tree-sitter-qmd's scanner.c.
 */

use pampa::pandoc::{Block, Div, Inline};
use pampa::readers;
use pampa::writers;

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

fn note_definition_ids(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::NoteDefinitionFencedBlock(n) => Some(n.id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn note_definition_id_lf() {
    let blocks = parse("::: ^foo\n\nBody.\n\n:::\n");
    assert_eq!(note_definition_ids(&blocks), vec!["foo".to_string()]);
}

/// With CRLF line endings the id used to run on to the `\r`, yielding
/// `"foo\r"`, which then never matched its `[^foo]` reference.
#[test]
fn note_definition_id_crlf_excludes_carriage_return() {
    let blocks = parse("::: ^foo\r\n\r\nBody.\r\n\r\n:::\r\n");
    assert_eq!(note_definition_ids(&blocks), vec!["foo".to_string()]);
}

fn only_div(blocks: &[Block]) -> &Div {
    match blocks {
        [Block::Div(div)] => div,
        _ => panic!("expected exactly one top-level Div, got {blocks:#?}"),
    }
}

fn to_qmd(input: &str) -> String {
    let (pandoc, _context, _warnings) = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("clean parse");
    let mut buf = Vec::new();
    writers::qmd::write(&pandoc, &mut buf).expect("qmd write");
    String::from_utf8(buf).unwrap()
}

#[test]
fn editorial_div_markers_lower_to_classed_divs() {
    for (marker, class) in [
        ("++", "quarto-insert"),
        ("--", "quarto-delete"),
        (">>", "quarto-edit-comment"),
        ("!!", "quarto-highlight"),
    ] {
        let blocks = parse(&format!("::: {marker}\n\nBody.\n\n:::\n"));
        let div = only_div(&blocks);
        assert_eq!(div.attr.0, "", "{marker}");
        assert_eq!(div.attr.1, vec![class.to_string()], "{marker}");
        assert!(div.attr.2.is_empty(), "{marker}");
        assert_eq!(div.content.len(), 1, "{marker}");
    }
}

/// The mark's class comes first, then the user's attributes in order;
/// `attr_source.classes` stays aligned, and the synthesized class points at
/// the marker bytes that produced it.
#[test]
fn editorial_div_attributes_follow_the_mark_class() {
    let src = "::: >> {#c .extra author=\"cs\"}\n\nWhy?\n\n:::\n";
    let blocks = parse(src);
    let div = only_div(&blocks);
    assert_eq!(div.attr.0, "c");
    assert_eq!(
        div.attr.1,
        vec!["quarto-edit-comment".to_string(), "extra".to_string()]
    );
    assert_eq!(div.attr.2.get("author").map(String::as_str), Some("cs"));

    let sources = &div.attr_source.classes;
    assert_eq!(
        sources.len(),
        div.attr.1.len(),
        "class sources stay aligned"
    );
    let text_of = |i: usize| {
        let si = sources[i].as_ref().expect("class has a source");
        &src[si.start_offset()..si.end_offset()]
    };
    assert_eq!(text_of(0), ">>");
    // Class sources include the `.`, as everywhere in the attr reader.
    assert_eq!(text_of(1), ".extra");
}

#[test]
fn editorial_div_drops_a_user_duplicate_of_its_class() {
    let blocks = parse("::: -- {.quarto-delete .x}\n\nGone.\n\n:::\n");
    let div = only_div(&blocks);
    assert_eq!(
        div.attr.1,
        vec!["quarto-delete".to_string(), "x".to_string()]
    );
    assert_eq!(div.attr_source.classes.len(), 2);
}

#[test]
fn editorial_div_attributes_without_a_space() {
    let blocks = parse("::: ++{.x}\n\nAdded.\n\n:::\n");
    let div = only_div(&blocks);
    assert_eq!(
        div.attr.1,
        vec!["quarto-insert".to_string(), "x".to_string()]
    );
}

#[test]
fn editorial_div_crlf() {
    let blocks = parse("::: --\r\n\r\nGone.\r\n\r\n:::\r\n");
    let div = only_div(&blocks);
    assert_eq!(div.attr.1, vec!["quarto-delete".to_string()]);
    assert_eq!(div.content.len(), 1);
}

#[test]
fn qmd_writer_round_trips_editorial_divs() {
    let src = "::: -- {#x .a key=\"v\"}\n\nGone.\n\n:::\n";
    assert_eq!(to_qmd(src), src);
    let src = "::: >>\n\nWhy?\n\n:::\n";
    assert_eq!(to_qmd(src), src);
}

/// A hand-written div whose *first* class is a mark's class is the same AST
/// as the marker form, so the writer canonicalizes it to the marker.
#[test]
fn qmd_writer_canonicalizes_a_leading_mark_class_to_the_marker() {
    assert_eq!(
        to_qmd("::: {.quarto-delete .x}\n\nGone.\n\n:::\n"),
        "::: -- {.x}\n\nGone.\n\n:::\n"
    );
    // Not first: an ordinary class, written as such.
    assert_eq!(
        to_qmd("::: {.x .quarto-delete}\n\nGone.\n\n:::\n"),
        "::: {.x .quarto-delete}\n\nGone.\n\n:::\n"
    );
}

/// Parity with the inline marks, which lower to Spans in postprocess: the
/// Span's attr sidecar stays aligned with its classes (the synthesized
/// class has no source) and keeps the user's attribute provenance. It used
/// to be dropped wholesale, which the tiling audit reports as
/// AttrAlignmentSkipped.
#[test]
fn inline_editorial_mark_keeps_an_aligned_attr_sidecar() {
    let src = "Text [++ added]{.x key=\"v\"} here.\n";
    let blocks = parse(src);
    let Block::Paragraph(para) = &blocks[0] else {
        panic!("expected a paragraph: {blocks:#?}");
    };
    let span = para
        .content
        .iter()
        .find_map(|i| match i {
            Inline::Span(s) => Some(s),
            _ => None,
        })
        .expect("the insert mark lowers to a Span");
    assert_eq!(
        span.attr.1,
        vec!["quarto-insert".to_string(), "x".to_string()]
    );
    let sources = &span.attr_source.classes;
    assert_eq!(sources.len(), 2, "class sources stay aligned");
    assert!(sources[0].is_none(), "the synthesized class has no source");
    let x = sources[1]
        .as_ref()
        .expect("the user class keeps its source");
    assert_eq!(&src[x.start_offset()..x.end_offset()], ".x");
    assert_eq!(span.attr_source.attributes.len(), 1);
}
