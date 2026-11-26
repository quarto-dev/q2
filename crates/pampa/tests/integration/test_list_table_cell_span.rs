/*
 * test_list_table_cell_span.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * A list-table cell takes `colspan`/`rowspan` only from an *empty* span
 * at its start (`[]{colspan=2} text`). A non-empty one
 * (`[text]{colspan=2}`) stays an ordinary span, so the reader warns
 * Q-2-61 and suggests the empty-span form. GH #1338.
 *
 * Run with: cargo nextest run -p pampa test_list_table_cell_span
 */

use pampa::readers;
use quarto_error_reporting::DiagnosticMessage;

fn warnings(input: &str) -> Vec<DiagnosticMessage> {
    let (_pandoc, _context, warnings) = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("input should parse");
    warnings
}

fn q2_61(warnings: &[DiagnosticMessage]) -> Vec<&DiagnosticMessage> {
    warnings
        .iter()
        .filter(|w| w.code.as_deref() == Some("Q-2-61"))
        .collect()
}

#[test]
fn a_non_empty_colspan_span_warns_and_suggests_the_empty_span_form() {
    let warnings = warnings(
        "::: {.list-table}
- - [Merged]{colspan=2}
- - a
  - b
:::
",
    );
    let found = q2_61(&warnings);
    assert_eq!(found.len(), 1, "{warnings:?}");
    let text = found[0].to_text(None);
    assert!(text.contains("[]{colspan=2} Merged"), "{text}");
}

#[test]
fn a_non_empty_rowspan_span_warns() {
    let warnings = warnings(
        "::: {.list-table}
- - [Tall]{rowspan=2}
  - x
- - y
:::
",
    );
    assert_eq!(q2_61(&warnings).len(), 1, "{warnings:?}");
}

#[test]
fn an_empty_colspan_span_does_not_warn() {
    let warnings = warnings(
        "::: {.list-table}
- - []{colspan=2} Merged
- - a
  - b
:::
",
    );
    assert!(q2_61(&warnings).is_empty(), "{warnings:?}");
}

#[test]
fn a_non_empty_span_without_span_attributes_does_not_warn() {
    let warnings = warnings(
        "::: {.list-table}
- - [styled]{.note}
  - b
:::
",
    );
    assert!(q2_61(&warnings).is_empty(), "{warnings:?}");
}

#[test]
fn a_span_later_in_the_cell_does_not_warn() {
    let warnings = warnings(
        "::: {.list-table}
- - text [Merged]{colspan=2}
  - b
:::
",
    );
    assert!(q2_61(&warnings).is_empty(), "{warnings:?}");
}
