/*
 * tests/revealjs_features.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for revealjs authoring features (Phase 2).
 */

//! Render-side tests for revealjs authoring constructs (fragments, notes,
//! columns, …). These drive `render_to_file(_, "revealjs", _)` and assert on
//! the generated HTML markup that reveal.js interprets.
//!
//! Preview parity: for **pure pass-through** features (class 1 — the AST class
//! survives to the DOM unchanged), the `q2 preview` `previewRegistry` emits the
//! same class-bearing element, so render-side coverage implies preview parity
//! (the class-passthrough behavior is exercised live). Features that change the
//! element or add CSS (class 2 — notes/columns) get explicit preview-side
//! assertions when implemented.

use std::path::Path;
use std::sync::Arc;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Render `contents` as a single-file revealjs deck, returning the HTML.
fn render_revealjs(contents: &str) -> String {
    let temp = tempfile::TempDir::new().unwrap();
    let qmd_path = temp.path().join("talk.qmd");
    write_file(&qmd_path, contents);
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result =
        render_to_file(&qmd_path, "revealjs", &options, runtime).expect("revealjs render failed");
    read(&result.output_path)
}

/// Whitespace-insensitive containment.
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

// ── 2a: fragments ────────────────────────────────────────────────────────

#[test]
fn fragment_div_passes_through() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\n::: {.fragment}\nReveal on click.\n:::\n",
    );
    assert!(
        html.contains("class=\"fragment\""),
        "a `.fragment` Div must render as `<div class=\"fragment\">`"
    );
    assert!(html.contains("Reveal on click."));
}

#[test]
fn fragment_variant_classes_pass_through() {
    // A representative spread of reveal fragment variants.
    let variants = [
        "fade-out",
        "fade-up",
        "grow",
        "shrink",
        "highlight-red",
        "highlight-blue",
        "semi-fade-out",
        "current-visible",
    ];
    let body: String = variants
        .iter()
        .map(|v| format!("::: {{.fragment .{v}}}\n{v}\n:::\n\n"))
        .collect();
    let html = render_revealjs(&format!("---\nformat: revealjs\n---\n\n## S\n\n{body}"));
    for v in variants {
        assert!(
            html.contains(&format!("fragment {v}")) || html.contains(&format!("{v} fragment")),
            "fragment variant `.{v}` must survive to the slide HTML"
        );
    }
}

// ── 2d: incremental lists ────────────────────────────────────────────────

/// Count `<li class="fragment">` openings.
fn fragment_li_count(html: &str) -> usize {
    html.matches("<li class=\"fragment\">").count()
}

#[test]
fn incremental_div_makes_list_items_fragments() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\n::: {.incremental}\n\n- a\n- b\n- c\n\n:::\n",
    );
    assert_eq!(
        fragment_li_count(&html),
        3,
        "each item of an `.incremental` list must be `<li class=\"fragment\">`; html:\n{}",
        &html[..html.len().min(2500)]
    );
}

#[test]
fn incremental_slide_heading_makes_lists_fragments() {
    // `.incremental` on the slide heading (hoisted to the section) applies to
    // all lists on the slide.
    let html =
        render_revealjs("---\nformat: revealjs\n---\n\n## Slide {.incremental}\n\n- one\n- two\n");
    assert_eq!(fragment_li_count(&html), 2);
}

#[test]
fn plain_list_is_not_incremental() {
    let html = render_revealjs("---\nformat: revealjs\n---\n\n## S\n\n- a\n- b\n");
    assert_eq!(
        fragment_li_count(&html),
        0,
        "non-incremental list must stay plain `<li>`"
    );
    assert!(html.contains("<li>"));
}

#[test]
fn global_incremental_makes_all_lists_fragments() {
    let html =
        render_revealjs("---\nformat: revealjs\nincremental: true\n---\n\n## S\n\n- a\n- b\n");
    assert_eq!(
        fragment_li_count(&html),
        2,
        "global `incremental: true` applies to every list"
    );
}

#[test]
fn nonincremental_opts_out_under_global_incremental() {
    let html = render_revealjs(
        "---\nformat: revealjs\nincremental: true\n---\n\n## S\n\n::: {.nonincremental}\n\n- a\n- b\n\n:::\n",
    );
    assert_eq!(
        fragment_li_count(&html),
        0,
        "`.nonincremental` must opt out even under global `incremental: true`"
    );
}

#[test]
fn ordered_incremental_list_items_are_fragments() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\n::: {.incremental}\n\n1. first\n2. second\n\n:::\n",
    );
    assert_eq!(
        fragment_li_count(&html),
        2,
        "ordered lists honor `.incremental` too"
    );
}

// ── 2c: columns ──────────────────────────────────────────────────────────

const COLUMNS_DECK: &str = "\
---
format: revealjs
---

## Columns

:::: {.columns}

::: {.column width=\"40%\"}
Left column.
:::

::: {.column width=\"60%\"}
Right column.
:::

::::
";

#[test]
fn column_width_becomes_flex_basis_style() {
    let html = render_revealjs(COLUMNS_DECK);
    let c = compact(&html);
    // Each column's `width=X%` becomes an inline `flex-basis` style…
    assert!(
        c.contains("flex-basis:40%"),
        "column width=40% must become `flex-basis: 40%`; html:\n{}",
        &html[..html.len().min(2500)]
    );
    assert!(
        c.contains("flex-basis:60%"),
        "column width=60% → flex-basis"
    );
    // …and the bare `width=` attribute must be gone (invalid on a div).
    assert!(
        !c.contains("class=\"column\"width=") && !html.contains("<div class=\"column\" width="),
        "the raw `width` attribute must be removed from columns"
    );
}

#[test]
fn columns_container_present() {
    let html = render_revealjs(COLUMNS_DECK);
    assert!(
        html.contains("class=\"columns\""),
        "columns container present"
    );
    assert!(html.contains("Left column.") && html.contains("Right column."));
}

// ── 2b: speaker notes ────────────────────────────────────────────────────

#[test]
fn notes_div_renders_as_aside() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\nVisible.\n\n::: {.notes}\nSpeaker note.\n:::\n",
    );
    assert!(
        html.contains("<aside class=\"notes\">"),
        "a `.notes` Div must render as `<aside class=\"notes\">` (reveal speaker \
         notes; hidden on the slide by reveal.css); html:\n{}",
        &html[..html.len().min(2000)]
    );
    assert!(html.contains("Speaker note."));
    // The visible slide content must remain.
    assert!(html.contains("Visible."));
}

#[test]
fn fragment_data_index_passes_through() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\n::: {.fragment fragment-index=\"2\"}\nSecond.\n:::\n",
    );
    let c = compact(&html);
    assert!(
        c.contains("data-fragment-index=\"2\""),
        "`fragment-index` must render as the reveal `data-fragment-index` attribute; html:\n{}",
        &html[..html.len().min(1500)]
    );
}
