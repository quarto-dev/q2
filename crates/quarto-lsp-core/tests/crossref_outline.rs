//! Integration tests for crossref entries in the document outline.
//!
//! Decisions baked into these tests (see
//! `claude-notes/plans/2026-04-17-crossref-outline.md`):
//!
//! - **Q1** Full-pipeline numbering — the outline shows the same numbers
//!   the render pipeline assigns (`Figure 1`, `Theorem 1.2`, ...).
//! - **Q2** `Symbol.name` is the identifier (`fig-one`); `Symbol.detail`
//!   is the rendered label (`Figure 1: caption text`). Kind is `Class`.
//! - **Q3** All twelve built-in ref types participate.
//! - **Q4** The walker does not descend into `FloatRefTarget` / `Theorem` /
//!   `Proof` custom slots — inner headers of a theorem (e.g. `## Line`)
//!   are absorbed into the target's `title` and must not appear as
//!   standalone outline entries.

use quarto_lsp_core::{Document, Symbol, SymbolKind, analyze_document};

/// Flatten a symbol tree depth-first so assertions can scan by name
/// regardless of parent/child nesting.
fn flatten(symbols: &[Symbol]) -> Vec<(String, SymbolKind, Option<String>)> {
    fn go(acc: &mut Vec<(String, SymbolKind, Option<String>)>, symbols: &[Symbol]) {
        for s in symbols {
            acc.push((s.name.clone(), s.kind, s.detail.clone()));
            go(acc, &s.children);
        }
    }
    let mut acc = Vec::new();
    go(&mut acc, symbols);
    acc
}

fn names(symbols: &[Symbol]) -> Vec<String> {
    flatten(symbols).into_iter().map(|(n, _, _)| n).collect()
}

fn find(symbols: &[Symbol], name: &str) -> Option<(SymbolKind, Option<String>)> {
    flatten(symbols)
        .into_iter()
        .find(|(n, _, _)| n == name)
        .map(|(_, k, d)| (k, d))
}

#[test]
fn figure_div_appears_in_outline() {
    let doc = Document::new(
        "test.qmd",
        r#"---
title: demo
---

::: {#fig-one}

![](placeholder.png)

This is the caption.

:::
"#,
    );
    let analysis = analyze_document(&doc);
    let (kind, detail) =
        find(&analysis.symbols, "fig-one").expect("fig-one should appear as an outline entry");
    assert_eq!(kind, SymbolKind::Class);
    let detail = detail.expect("fig-one should carry a detail label");
    assert!(detail.starts_with("Figure 1"), "detail was {detail:?}");
    assert!(
        detail.contains("This is the caption"),
        "detail was {detail:?}"
    );
}

#[test]
fn theorem_div_appears_in_outline_and_hides_inner_header() {
    let doc = Document::new(
        "test.qmd",
        r#"---
title: demo
---

::: {#thm-line}

## Line

The equation of a straight line is $y = mx + b$.

:::
"#,
    );
    let analysis = analyze_document(&doc);

    let (kind, detail) =
        find(&analysis.symbols, "thm-line").expect("thm-line should appear as an outline entry");
    assert_eq!(kind, SymbolKind::Class);
    let detail = detail.expect("thm-line should carry a detail label");
    assert!(detail.contains("Theorem"), "detail was {detail:?}");
    assert!(detail.contains("Line"), "detail was {detail:?}");

    // The `## Line` header was absorbed into the theorem's title slot,
    // so it must NOT appear as a standalone outline entry.
    assert!(
        !names(&analysis.symbols).contains(&"Line".to_string()),
        "inner header `Line` should not appear alongside the theorem; symbols: {:?}",
        names(&analysis.symbols)
    );
}

#[test]
fn real_headers_and_crossrefs_coexist() {
    let doc = Document::new(
        "test.qmd",
        r#"# Section one

Some introductory text.

::: {#fig-a}

![](a.png)

Caption A.

:::

# Section two

::: {#thm-t}

A theorem body.

:::
"#,
    );
    let analysis = analyze_document(&doc);
    let flat_names = names(&analysis.symbols);

    // Real headers still present.
    assert!(flat_names.contains(&"Section one".to_string()));
    assert!(flat_names.contains(&"Section two".to_string()));

    // Crossref targets present.
    assert!(flat_names.contains(&"fig-a".to_string()));
    assert!(flat_names.contains(&"thm-t".to_string()));
}

#[test]
fn two_figures_numbered_sequentially() {
    let doc = Document::new(
        "test.qmd",
        r#"::: {#fig-first}

![](1.png)

First.

:::

::: {#fig-second}

![](2.png)

Second.

:::
"#,
    );
    let analysis = analyze_document(&doc);

    let (_, d1) = find(&analysis.symbols, "fig-first").expect("fig-first missing");
    let (_, d2) = find(&analysis.symbols, "fig-second").expect("fig-second missing");
    let d1 = d1.expect("detail missing");
    let d2 = d2.expect("detail missing");

    assert!(d1.starts_with("Figure 1"), "fig-first detail = {d1:?}");
    assert!(d2.starts_with("Figure 2"), "fig-second detail = {d2:?}");
}

#[test]
fn malformed_id_does_not_panic_or_emit_symbol() {
    // `{#fig-}` has no suffix after the prefix; not a valid crossref target.
    let doc = Document::new(
        "test.qmd",
        r#"::: {#fig-}

![](x.png)

Nothing.

:::
"#,
    );
    let analysis = analyze_document(&doc);
    let flat_names = names(&analysis.symbols);
    assert!(
        !flat_names.contains(&"fig-".to_string()),
        "malformed id should not surface as a symbol; symbols: {flat_names:?}"
    );
}

#[test]
fn labeled_equation_appears_in_outline() {
    let doc = Document::new(
        "test.qmd",
        r#"The quadratic formula:

$$
x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}
$$ {#eq-quadratic}
"#,
    );
    let analysis = analyze_document(&doc);
    let flat_names = names(&analysis.symbols);
    assert!(
        flat_names.contains(&"eq-quadratic".to_string()),
        "eq-quadratic should appear; symbols: {flat_names:?}"
    );
}

#[test]
fn table_div_appears_in_outline() {
    let doc = Document::new(
        "test.qmd",
        r#"::: {#tbl-data}

| A | B |
|---|---|
| 1 | 2 |

Data.

:::
"#,
    );
    let analysis = analyze_document(&doc);
    let flat_names = names(&analysis.symbols);
    assert!(
        flat_names.contains(&"tbl-data".to_string()),
        "tbl-data should appear; symbols: {flat_names:?}"
    );
}

#[test]
fn no_crossref_divs_is_unchanged_outline() {
    // Pre-existing behavior: a doc with only headers should still produce
    // those header symbols after the pipeline rewrite.
    let doc = Document::new(
        "test.qmd",
        r#"# Only

## A header

Text.
"#,
    );
    let analysis = analyze_document(&doc);
    let flat_names = names(&analysis.symbols);
    assert!(flat_names.contains(&"Only".to_string()));
    assert!(flat_names.contains(&"A header".to_string()));
}
