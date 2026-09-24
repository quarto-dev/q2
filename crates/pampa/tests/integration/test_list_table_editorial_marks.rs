/*
 * test_list_table_editorial_marks.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Inline editorial marks inside a list-table used to panic the reader: a
 * filter pass with no handler for the editorial-mark inlines reached them
 * inside the table, and `traverse_inline_nonterminal` had no arm for them
 * (bd-2281lkrx).
 *
 * The marks are not yet lowered to their `quarto-*` Spans inside
 * list-table cells, because postprocess never visits cell content
 * (bd-9fy9p4zl); this test accepts either form so it holds before and
 * after that fix.
 */

use pampa::pandoc::{Block, Inline};
use pampa::readers;

fn mark_class(inline: &Inline) -> Option<&str> {
    match inline {
        Inline::Insert(_) => Some("quarto-insert"),
        Inline::Delete(_) => Some("quarto-delete"),
        Inline::Highlight(_) => Some("quarto-highlight"),
        Inline::EditComment(_) => Some("quarto-edit-comment"),
        Inline::Span(s) => s.attr.1.first().map(String::as_str),
        _ => None,
    }
}

#[test]
fn list_table_cells_accept_inline_editorial_marks() {
    let src = "::: list-table\n\n\
* * a\n  * [++ ins] [-- del] [!! hl] [>> com]\n\n\
:::\n";
    let (pandoc, _context, _warnings) = readers::qmd::read(
        src.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("clean parse");

    let [Block::Table(table)] = pandoc.blocks.as_slice() else {
        panic!("expected a single Table: {:#?}", pandoc.blocks);
    };
    let cell = table
        .head
        .rows
        .iter()
        .chain(table.bodies.iter().flat_map(|b| b.body.iter()))
        .flat_map(|row| row.cells.iter())
        .nth(1)
        .expect("the row's second cell");
    let inlines = match cell.content.as_slice() {
        [Block::Plain(p)] => &p.content,
        [Block::Paragraph(p)] => &p.content,
        other => panic!("unexpected cell content: {other:#?}"),
    };
    let classes: Vec<&str> = inlines.iter().filter_map(mark_class).collect();
    assert_eq!(
        classes,
        vec![
            "quarto-insert",
            "quarto-delete",
            "quarto-highlight",
            "quarto-edit-comment"
        ]
    );
}
