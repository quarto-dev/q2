//! Promotion of reserved key-value attributes (`id="..."`, `class="..."`)
//! into the identifier and classes slots of `Attr`, matching pandoc.
//!
//! Regression coverage for bd-heading-id-attr-duplicated-xbpcmejr: q2 used
//! to leave the kv form in the attribute map (attr.2), so a heading written
//! `## H {id="x"}` counted as unidentified, received an auto slug, and the
//! sectionized HTML carried two `id=` attributes — an HTML parse error.
//!
//! Pandoc semantics (probed against pandoc 3.x; measurement tables in
//! `claude-notes/plans/heading-id-kv-attribute-investigation/observed-2026-08-18.md`):
//! - kv `id` fills the identifier slot; the **last** id wins across both
//!   forms and duplicates. (q2's grammar orders components id → classes →
//!   kv, so the only reachable mixed case is `{#short id="kv"}`.)
//! - kv `class` values are split on whitespace and appended to classes in
//!   source order.
//! - Only `id` and `class` are reserved; other keys stay in the map.

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

fn sole_header(doc: &Pandoc) -> &pampa::pandoc::block::Header {
    let mut headers = doc.blocks.iter().filter_map(|b| match b {
        Block::Header(h) => Some(h),
        _ => None,
    });
    let h = headers.next().expect("expected a heading");
    assert!(headers.next().is_none(), "expected exactly one heading");
    h
}

fn write_qmd(doc: &Pandoc) -> String {
    let mut buf = Vec::new();
    pampa::writers::qmd::write(doc, &mut buf).expect("Failed to write QMD");
    String::from_utf8(buf).expect("utf8")
}

// ============================================================================
// Heading: kv id fills the identifier slot
// ============================================================================

#[test]
fn test_heading_kv_id_slashy_becomes_identifier() {
    // The Connect-docs form: slashes are inexpressible in {#...} shorthand.
    let doc = parse(r#"## List API keys {id="get-/v1/users/-guid-/keys"}"#);
    let h = sole_header(&doc);
    assert_eq!(h.attr.0, "get-/v1/users/-guid-/keys");
    assert!(
        !h.attr.2.contains_key("id"),
        "id must not remain in the kv map: {:?}",
        h.attr.2
    );
}

#[test]
fn test_heading_kv_id_plain_becomes_identifier() {
    // The slash is irrelevant — a plain word duplicates too (strand repro).
    let doc = parse(r#"## Hello {id="plain-explicit"}"#);
    let h = sole_header(&doc);
    assert_eq!(h.attr.0, "plain-explicit");
    assert!(!h.attr.2.contains_key("id"));
}

#[test]
fn test_heading_kv_id_suppresses_auto_slug() {
    // Before the fix the identifier slot got the auto slug "hello".
    let doc = parse(r#"## Hello {id="x"}"#);
    let h = sole_header(&doc);
    assert_ne!(h.attr.0, "hello");
    assert_eq!(h.attr.0, "x");
}

#[test]
fn test_heading_kv_id_marks_attr_source_id_explicit() {
    // The qmd writer suppresses ids whose attr_source.id is None (treated
    // as auto-generated); a promoted kv id is author-written and must
    // carry its source.
    let doc = parse(r#"## Hello {id="x"}"#);
    let h = sole_header(&doc);
    assert!(
        h.attr_source.id.is_some(),
        "promoted kv id must record a source span"
    );
}

// ============================================================================
// Last id wins (pandoc parity)
// ============================================================================

#[test]
fn test_heading_shorthand_then_kv_id_last_wins() {
    // pandoc: {#short id="kv"} -> "kv"
    let doc = parse(r#"## A {#short id="kv"}"#);
    assert_eq!(sole_header(&doc).attr.0, "kv");
}

#[test]
fn test_heading_duplicate_kv_id_last_wins() {
    // pandoc: {id="one" id="two"} -> "two"
    let doc = parse(r#"## C {id="one" id="two"}"#);
    let h = sole_header(&doc);
    assert_eq!(h.attr.0, "two");
    assert!(!h.attr.2.contains_key("id"));
}

// ============================================================================
// kv class merges into the classes slot
// ============================================================================

#[test]
fn test_kv_class_splits_on_whitespace_in_source_order() {
    // pandoc: {.y class="x z"} -> classes ["y", "x", "z"]
    let doc = parse(r#"## D {.y class="x z"}"#);
    let h = sole_header(&doc);
    assert_eq!(h.attr.1, vec!["y", "x", "z"]);
    assert!(
        !h.attr.2.contains_key("class"),
        "class must not remain in the kv map: {:?}",
        h.attr.2
    );
}

#[test]
fn test_kv_class_alone_still_gets_auto_id() {
    // pandoc: {class="x"} with no id of either form -> auto slug.
    let doc = parse(r#"## D {class="x"}"#);
    let h = sole_header(&doc);
    assert_eq!(h.attr.0, "d");
    assert_eq!(h.attr.1, vec!["x"]);
}

#[test]
fn test_other_kv_keys_stay_in_the_map() {
    let doc = parse(r#"## E {id="x" data-foo="bar"}"#);
    let h = sole_header(&doc);
    assert_eq!(h.attr.0, "x");
    assert_eq!(h.attr.2.get("data-foo").map(String::as_str), Some("bar"));
}

// ============================================================================
// Other element types sharing the choke point
// ============================================================================

#[test]
fn test_span_kv_id_and_class_promoted() {
    // pandoc: [span]{id="sp" class="c1 c2"} -> Span ("sp", ["c1","c2"], [])
    let doc = parse(r#"[span]{id="sp" class="c1 c2"}"#);
    let Some(Block::Paragraph(para)) = doc.blocks.first() else {
        panic!("expected paragraph, got {:?}", doc.blocks.first());
    };
    let Some(Inline::Span(span)) = para.content.first() else {
        panic!("expected span, got {:?}", para.content.first());
    };
    assert_eq!(span.attr.0, "sp");
    assert_eq!(span.attr.1, vec!["c1", "c2"]);
    assert!(span.attr.2.is_empty(), "kv map: {:?}", span.attr.2);
}

#[test]
fn test_div_kv_id_promoted() {
    let doc = parse("::: {id=\"dv\"}\ncontent\n:::\n");
    let Some(Block::Div(div)) = doc.blocks.first() else {
        panic!("expected div, got {:?}", doc.blocks.first());
    };
    assert_eq!(div.attr.0, "dv");
    assert!(!div.attr.2.contains_key("id"));
}

#[test]
fn test_code_span_kv_id_promoted() {
    let doc = parse(r#"`code`{id="cd"}"#);
    let Some(Block::Paragraph(para)) = doc.blocks.first() else {
        panic!("expected paragraph, got {:?}", doc.blocks.first());
    };
    let Some(Inline::Code(code)) = para.content.first() else {
        panic!("expected code, got {:?}", para.content.first());
    };
    assert_eq!(code.attr.0, "cd");
    assert!(!code.attr.2.contains_key("id"));
}

#[test]
fn test_fenced_code_block_kv_id_promoted() {
    let doc = parse("``` {.python id=\"cb\"}\nx = 1\n```\n");
    let Some(Block::CodeBlock(cb)) = doc.blocks.first() else {
        panic!("expected code block, got {:?}", doc.blocks.first());
    };
    assert_eq!(cb.attr.0, "cb");
    assert!(!cb.attr.2.contains_key("id"));
}

// ============================================================================
// qmd writer round-trip
// ============================================================================

#[test]
fn test_slashy_id_roundtrips_via_kv_form() {
    // The shorthand charset is [._A-Za-z0-9-]+; a slash cannot appear in
    // {#...}, so the writer must fall back to the kv form.
    let doc = parse(r#"## Hello {id="get-/v1/x"}"#);
    let written = write_qmd(&doc);
    assert!(
        written.contains(r#"{id="get-/v1/x"}"#),
        "slashy id must be written in kv form; got:\n{written}"
    );
    assert!(
        !written.contains("{#get-/v1/x"),
        "slashy id must not use the shorthand (it cannot re-parse); got:\n{written}"
    );

    let reparsed = parse(&written);
    assert_eq!(sole_header(&reparsed).attr.0, "get-/v1/x");
}

#[test]
fn test_plain_kv_id_normalizes_to_shorthand_and_reparses() {
    // A shorthand-expressible id may normalize to {#...}; the invariant is
    // that re-parsing reproduces the same identifier.
    let doc = parse(r#"## Hello {id="plain"}"#);
    let written = write_qmd(&doc);
    let reparsed = parse(&written);
    let h = sole_header(&reparsed);
    assert_eq!(h.attr.0, "plain");
    assert!(!h.attr.2.contains_key("id"));
}
