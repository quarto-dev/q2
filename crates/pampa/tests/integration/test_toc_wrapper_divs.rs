/*
 * test_toc_wrapper_divs.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pipeline-level tests for which wrapped headings reach the TOC.
 */

//! What reaches the table of contents when a heading is wrapped in a
//! fenced div, driven from markdown rather than from a hand-built AST.
//!
//! `toc.rs`'s unit tests construct sectionized blocks directly, so they
//! pin `collect_toc_entries` in isolation. This bug was an *interaction*:
//! `sectionize_blocks` absorbs an anonymous wrapper into the section it
//! holds but cannot absorb an id-bearing one (its id would collide with
//! the section's), and the walk then refused to enter what was left.
//! Either half can change without the other's tests noticing, so these
//! run the real pair — `readers::qmd::read` → `sectionize_blocks` →
//! `generate_toc` — over the eight wrapper shapes that pin the contract.
//!
//! Every expectation here was measured against `pandoc 3.8.1
//! --toc --section-divs` and against Quarto 1, which agree with each
//! other on all eight. bd-toc-skips-headings-in-id-div-1jorg679.

use pampa::toc::{TocConfig, TocEntry, generate_toc};
use pampa::transforms::sectionize_blocks;

/// The eight cases in one document, matching the out-of-repo repro
/// fixture case for case so the two cannot drift apart silently.
const FIXTURE: &str = r#"## A top level

body

::: {}
## B no attributes
:::

::: {.someclass}
## C class only
:::

::: {#someid}
## D id only
:::

::: {#someid2 .someclass2}
## E id and class
:::

::: {#someid3}
## [F span heading]{#span-id}
:::

::: {#someid4}
Prose before the heading.

## G not the sole child
:::

::: {#outer}
::: {#inner}
## H doubly nested
:::
:::
"#;

fn toc_ids(markdown: &str) -> Vec<String> {
    let (pandoc, _context, _warnings) = pampa::readers::qmd::read(
        markdown.as_bytes(),
        false,
        "<test>",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("fixture must parse");
    let sectionized = sectionize_blocks(pandoc.blocks);
    let toc = generate_toc(
        &sectionized,
        &TocConfig {
            depth: 6,
            title: None,
        },
    );
    let mut ids = Vec::new();
    collect_ids(&toc.entries, &mut ids);
    ids
}

fn collect_ids(entries: &[TocEntry], out: &mut Vec<String>) {
    for entry in entries {
        out.push(entry.id.clone());
        collect_ids(&entry.children, out);
    }
}

/// The whole contract in one assertion. G is absent and everything else
/// is present, in document order.
#[test]
fn wrapped_headings_reach_the_toc_except_when_the_wrapper_holds_more() {
    assert_eq!(
        toc_ids(FIXTURE),
        vec![
            "a-top-level",
            "b-no-attributes",
            "c-class-only",
            "d-id-only",
            "e-id-and-class",
            "f-span-heading",
            "h-doubly-nested",
        ],
        "G must stay out — pandoc and Quarto 1 both exclude a wrapper \
         whose content is not a single Div — and every other case must \
         come in"
    );
}

/// The case the bug report was filed for, on its own so a failure names
/// it. An id on the wrapper is the entire trigger: C, one line away in
/// the fixture above, differs only in carrying a class instead.
#[test]
fn an_id_on_the_wrapper_does_not_hide_the_heading() {
    assert_eq!(toc_ids("::: {#someid}\n## Wrapped\n:::\n"), vec!["wrapped"]);
}

/// Depth: Quarto 1 descends through arbitrarily many wrappers, so the
/// descent has to iterate rather than unwrap one level.
#[test]
fn stacked_wrappers_are_all_descended_through() {
    assert_eq!(
        toc_ids("::: {#a}\n::: {#b}\n::: {#c}\n## Deep\n:::\n:::\n:::\n"),
        vec!["deep"]
    );
}

/// The negative control, isolated. Prose ahead of the heading means the
/// wrapper holds two blocks, and neither pandoc nor Quarto 1 lists it.
#[test]
fn prose_before_the_heading_keeps_it_out() {
    assert!(
        toc_ids("::: {#someid}\nProse first.\n\n## Wrapped\n:::\n").is_empty(),
        "listing this would diverge from Quarto 1 in the other direction"
    );
}

/// The other way to hold more than one block: two sibling sections. The
/// existing negative control fails the predicate on block *kind*; this
/// one fails it on block *count*. Verified against pandoc 3.8.1.
#[test]
fn two_sibling_sections_in_one_wrapper_stay_out() {
    assert!(
        toc_ids("::: {#someid}\n## One\n\n## Two\n:::\n").is_empty(),
        "a wrapper holding two sections is not a transparent wrapper"
    );
}
