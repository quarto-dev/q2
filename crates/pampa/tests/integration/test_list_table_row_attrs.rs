/*
 * test_list_table_row_attrs.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * List-table row attributes (GH #1336). pandoc-ext/list-table: "If the
 * first block element of a table row contains (only) an empty Span, it is
 * removed and its attributes are transferred to the table row." The reader
 * moves the span's attributes onto `Row.attr`, the HTML writer emits them
 * on `<tr>`, and the qmd writer writes them back as a `[]{...}` span.
 */

use pampa::pandoc::{Block, Pandoc};
use pampa::readers;
use pampa::writers;
use quarto_pandoc_types::table::Row;

const SRC: &str = "::: {.list-table}\n\
- - A\n  - B\n\
- []{#r2 .myrow data-x=\"1\"}\n  - c\n  - d\n\
:::\n";

fn read(src: &str) -> (Pandoc, pampa::pandoc::ASTContext) {
    let (pandoc, ctx, warnings) = readers::qmd::read(
        src.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("clean parse");
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:#?}");
    (pandoc, ctx)
}

fn rows(pandoc: &Pandoc) -> Vec<&Row> {
    let [Block::Table(table)] = pandoc.blocks.as_slice() else {
        panic!("expected a single Table: {:#?}", pandoc.blocks);
    };
    table
        .head
        .rows
        .iter()
        .chain(table.bodies.iter().flat_map(|b| b.body.iter()))
        .collect()
}

fn assert_row_attrs(pandoc: &Pandoc) {
    let rows = rows(pandoc);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].attr.0.is_empty() && rows[0].attr.1.is_empty());
    let attr = &rows[1].attr;
    assert_eq!(attr.0, "r2");
    assert_eq!(attr.1, vec!["myrow".to_string()]);
    assert_eq!(attr.2.get("data-x").map(String::as_str), Some("1"));
    assert_eq!(rows[1].cells.len(), 2, "the span is not a cell");
}

#[test]
fn leading_empty_span_becomes_row_attr() {
    let (pandoc, _ctx) = read(SRC);
    assert_row_attrs(&pandoc);
}

#[test]
fn html_writer_emits_row_attr_after_stripe_class() {
    let (pandoc, ctx) = read(SRC);
    let mut buf = Vec::new();
    writers::html::write(&pandoc, &ctx, &mut buf).expect("html write");
    let html = String::from_utf8(buf).expect("utf8");
    assert!(
        html.contains("<tr id=\"r2\" class=\"odd myrow\" data-x=\"1\">"),
        "row attr missing from <tr>: {html}"
    );
}

#[test]
fn qmd_writer_round_trips_row_attr() {
    let (pandoc, _ctx) = read(SRC);
    let mut buf = Vec::new();
    writers::qmd::write(&pandoc, &mut buf).expect("qmd write");
    let qmd = String::from_utf8(buf).expect("utf8");
    assert!(
        qmd.contains("[]{#r2 .myrow data-x=\"1\"}"),
        "row attr missing from qmd: {qmd}"
    );
    let (again, _ctx) = read(&qmd);
    assert_row_attrs(&again);
}

#[test]
fn row_with_non_empty_span_is_still_rejected() {
    let src = "::: {.list-table}\n- [x]{.myrow}\n  - c\n:::\n";
    let (pandoc, _ctx, warnings) = readers::qmd::read(
        src.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("parse");
    assert!(
        !matches!(pandoc.blocks.as_slice(), [Block::Table(_)]),
        "a span with content is not a row-attr span"
    );
    assert!(!warnings.is_empty(), "expected a Q-2-35 warning");
}
