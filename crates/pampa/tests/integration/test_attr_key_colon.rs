//! Attribute keys containing `:` (GH #1027, bd-aidsrl7n).
//!
//! Pandoc's attribute-key grammar admits `-`, `_`, `:` and `.` after the
//! first character, and extensions such as quarto-prism key attributes
//! by output format (`html:style="..."`). q2's external scanner used to
//! stop at `:`, so no key token was produced and the whole document
//! failed with a generic parse error.

use pampa::pandoc::{Block, Inline, Pandoc};

fn parse(input: &str) -> Pandoc {
    let (pandoc, _, _) = pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("Failed to parse QMD");
    pandoc
}

fn write_qmd(doc: &Pandoc) -> String {
    let mut buf = Vec::new();
    pampa::writers::qmd::write(doc, &mut buf).expect("Failed to write QMD");
    String::from_utf8(buf).expect("utf8")
}

#[test]
fn test_div_attr_key_with_colon() {
    // The reproducer from the issue.
    let doc = parse("::: {html:style=\"color: red;\"}\nText.\n:::\n");
    let Some(Block::Div(div)) = doc.blocks.first() else {
        panic!("expected a Div, got {:?}", doc.blocks);
    };
    assert_eq!(
        div.attr.2.get("html:style").map(String::as_str),
        Some("color: red;")
    );
}

#[test]
fn test_div_attr_colon_keys_alongside_other_attrs() {
    let doc = parse("::: {#d .c html:style=\"a\" latex:style=\"b\" k=v}\nText.\n:::\n");
    let Some(Block::Div(div)) = doc.blocks.first() else {
        panic!("expected a Div, got {:?}", doc.blocks);
    };
    assert_eq!(div.attr.0, "d");
    assert_eq!(div.attr.1, vec!["c".to_string()]);
    assert_eq!(div.attr.2.get("html:style").map(String::as_str), Some("a"));
    assert_eq!(div.attr.2.get("latex:style").map(String::as_str), Some("b"));
    assert_eq!(div.attr.2.get("k").map(String::as_str), Some("v"));
}

#[test]
fn test_span_attr_key_with_colon() {
    let doc = parse("[x]{html:style=\"color: red;\"}\n");
    let Some(Block::Paragraph(p)) = doc.blocks.first() else {
        panic!("expected a Paragraph, got {:?}", doc.blocks);
    };
    let Some(Inline::Span(span)) = p.content.first() else {
        panic!("expected a Span, got {:?}", p.content);
    };
    assert_eq!(
        span.attr.2.get("html:style").map(String::as_str),
        Some("color: red;")
    );
}

#[test]
fn test_code_block_attr_key_with_colon() {
    let doc = parse("```{.python html:style=\"x\"}\nprint(1)\n```\n");
    let Some(Block::CodeBlock(cb)) = doc.blocks.first() else {
        panic!("expected a CodeBlock, got {:?}", doc.blocks);
    };
    assert_eq!(cb.attr.2.get("html:style").map(String::as_str), Some("x"));
}

#[test]
fn test_attr_key_with_colon_roundtrips_through_qmd_writer() {
    let doc = parse("::: {html:style=\"color: red;\"}\nText.\n:::\n");
    let out = write_qmd(&doc);
    assert!(
        out.contains("html:style=\"color: red;\""),
        "writer output lost the key: {out}"
    );
    let again = parse(&out);
    let Some(Block::Div(div)) = again.blocks.first() else {
        panic!("expected a Div after roundtrip, got {:?}", again.blocks);
    };
    assert_eq!(
        div.attr.2.get("html:style").map(String::as_str),
        Some("color: red;")
    );
}
