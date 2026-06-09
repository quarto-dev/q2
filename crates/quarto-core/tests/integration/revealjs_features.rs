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

// ── 2e: asides ───────────────────────────────────────────────────────────

#[test]
fn aside_div_renders_as_aside_element() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\nBody.\n\n::: {.aside}\nAn aside note.\n:::\n",
    );
    assert!(
        html.contains("<aside class=\"aside\">"),
        "a `.aside` Div must render as `<aside class=\"aside\">`; html:\n{}",
        &html[..html.len().min(2000)]
    );
    assert!(html.contains("An aside note."));
}

// ── 2e-ii: per-slide footnote coalescing ─────────────────────────────────

/// Strip `<style>…</style>` and `<script>…</script>` blocks so assertions see
/// only the slide *markup*, not the inlined reveal.js library or the
/// `quarto-reveal.css` text (whose comments/selectors mention `aside`,
/// `aside-footnotes`, etc. and would otherwise pollute substring counts).
fn body_markup(html: &str) -> String {
    fn drop_blocks(mut s: String, open: &str, close: &str) -> String {
        loop {
            let Some(start) = s.find(open) else { break };
            let Some(rel_end) = s[start..].find(close) else {
                break;
            };
            let end = start + rel_end + close.len();
            s.replace_range(start..end, "");
        }
        s
    }
    let s = drop_blocks(html.to_string(), "<style", "</style>");
    drop_blocks(s, "<script", "</script>")
}

/// Count occurrences of a (whitespace-insensitive) needle in the slide markup.
fn count(haystack: &str, needle: &str) -> usize {
    compact(haystack).matches(&compact(needle)).count()
}

#[test]
fn footnotes_coalesce_into_per_slide_aside() {
    // A single slide with an inline footnote: the footnote must land in an
    // `<aside>` carrying an `<ol class="aside-footnotes">` on *that* slide, and
    // there must be NO trailing `role="doc-endnotes"` footnotes slide.
    let html = body_markup(&render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\nBody text.^[A footnote.]\n",
    ));
    assert!(
        count(&html, "class=\"aside-footnotes\"") >= 1,
        "footnote must be coalesced into an `<ol class=\"aside-footnotes\">`; html:\n{}",
        &html[..html.len().min(3000)]
    );
    assert!(html.contains("A footnote."), "footnote content preserved");
    assert!(
        !html.contains("role=\"doc-endnotes\""),
        "the trailing footnotes slide must be suppressed when coalescing; html:\n{}",
        &html[..html.len().min(3000)]
    );
    assert!(
        !html.contains("id=\"footnotes\""),
        "no document-level footnotes section should remain"
    );
}

#[test]
fn coalesced_footnote_ref_is_plain_superscript() {
    // The in-text reference becomes a plain `<sup>1</sup>` (per-slide number),
    // not a link to the (now-removed) footnotes section.
    let html = body_markup(&render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\nText.^[note]\n",
    ));
    assert!(
        compact(&html).contains("<sup>1</sup>"),
        "first footnote ref on a slide must render as `<sup>1</sup>`; html:\n{}",
        &html[..html.len().min(3000)]
    );
    // No dangling doc-noteref link should survive coalescing.
    assert!(
        !html.contains("role=\"doc-noteref\""),
        "coalesced refs must not keep the doc-noteref link"
    );
    // The per-slide footnote list must drop the backlink.
    assert!(
        !html.contains("footnote-back"),
        "per-slide footnote items must drop the backlink"
    );
}

#[test]
fn footnotes_renumber_per_slide() {
    // Two slides, each with its own footnote: each slide numbers from 1.
    let html = body_markup(&render_revealjs(
        "---\nformat: revealjs\n---\n\n## One\n\nA.^[first]\n\n## Two\n\nB.^[second]\n",
    ));
    let c = compact(&html);
    assert_eq!(
        c.matches("<sup>1</sup>").count(),
        2,
        "each slide's footnote is numbered 1 (per-slide numbering); html:\n{}",
        &html[..html.len().min(3500)]
    );
    assert_eq!(
        count(&html, "class=\"aside-footnotes\""),
        2,
        "one per-slide footnotes list per slide"
    );
    assert!(html.contains("first") && html.contains("second"));
}

#[test]
fn reference_location_document_keeps_trailing_slide() {
    // Opt out of coalescing: `reference-location: document` restores the
    // trailing footnotes section and does NOT coalesce onto the slide.
    let html = body_markup(&render_revealjs(
        "---\nformat: revealjs\nreference-location: document\n---\n\n## S\n\nText.^[note]\n",
    ));
    assert!(
        html.contains("role=\"doc-endnotes\"") || html.contains("id=\"footnotes\""),
        "reference-location: document must keep the trailing footnotes section; html:\n{}",
        &html[..html.len().min(3000)]
    );
    assert!(
        !html.contains("class=\"aside-footnotes\""),
        "no per-slide coalescing under reference-location: document"
    );
}

#[test]
fn aside_and_footnote_share_one_coalesced_aside() {
    // A slide with BOTH an authored `.aside` and a footnote must end up with a
    // single coalesced `<aside>` (so the two don't overlap as separately
    // absolutely-positioned elements). The footnotes list lives inside it.
    let html = body_markup(&render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\nText.^[fn]\n\n::: {.aside}\nAn aside.\n:::\n",
    ));
    assert_eq!(
        count(&html, "<aside"),
        1,
        "asides + footnotes coalesce into exactly one `<aside>`; html:\n{}",
        &html[..html.len().min(3500)]
    );
    assert!(
        html.contains("An aside."),
        "authored aside content preserved"
    );
    assert!(
        count(&html, "class=\"aside-footnotes\"") >= 1,
        "the coalesced aside still carries the footnotes list"
    );
}

#[test]
fn slide_without_footnotes_or_asides_gets_no_aside() {
    let html = body_markup(&render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\nJust text.\n",
    ));
    assert!(
        !html.contains("<aside"),
        "a slide with no asides/footnotes must not gain an empty aside"
    );
}

#[test]
fn footnotes_coalesce_inside_vertical_stack_slides() {
    // A `#` section divider builds a horizontal *stack* whose `##` children are
    // vertical leaf slides. Footnotes must coalesce onto the inner *leaf*
    // slide, not the enclosing stack section.
    let html = body_markup(&render_revealjs(
        "---\nformat: revealjs\n---\n\n# Part\n\n## Sub A\n\nA.^[note a]\n\n## Sub B\n\nB.^[note b]\n",
    ));
    let c = compact(&html);
    // Two leaf slides, each with its own coalesced footnotes list, each
    // numbered from 1.
    assert_eq!(
        count(&html, "class=\"aside-footnotes\""),
        2,
        "each vertical leaf slide gets its own footnotes list; html:\n{}",
        &html[..html.len().min(3500)]
    );
    assert_eq!(
        c.matches("<sup>1</sup>").count(),
        2,
        "per-slide numbering resets inside the stack"
    );
    assert!(html.contains("note a") && html.contains("note b"));
    assert!(
        !html.contains("role=\"doc-endnotes\""),
        "no trailing footnotes slide"
    );
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
