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

/// Render `contents` as a single file to `format`, returning the HTML.
fn render_as(contents: &str, format: &str) -> String {
    let temp = tempfile::TempDir::new().unwrap();
    let qmd_path = temp.path().join("talk.qmd");
    write_file(&qmd_path, contents);
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&qmd_path, format, &options, runtime)
        .unwrap_or_else(|e| panic!("{format} render failed: {e:?}"));
    read(&result.output_path)
}

/// Render `contents` as a single-file revealjs deck, returning the HTML.
fn render_revealjs(contents: &str) -> String {
    render_as(contents, "revealjs")
}

/// Render a minimal deck and return the text of the `quarto-reveal.css` it
/// links (Quarto's layer over reveal.js; written beside the deck, not inlined).
fn rendered_quarto_reveal_css() -> String {
    let temp = tempfile::TempDir::new().unwrap();
    let qmd_path = temp.path().join("talk.qmd");
    write_file(&qmd_path, "---\nformat: revealjs\n---\n\n## S\n\nBody.\n");
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result =
        render_to_file(&qmd_path, "revealjs", &options, runtime).expect("revealjs render failed");
    let html = read(&result.output_path);
    let href_end = html
        .find("quarto-reveal.css\"")
        .expect("the deck links quarto-reveal.css")
        + "quarto-reveal.css".len();
    let href_start = html[..href_end].rfind('"').expect("href opening quote") + 1;
    let css_path = result
        .output_path
        .parent()
        .expect("output dir")
        .join(&html[href_start..href_end]);
    read(&css_path)
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
        while let Some(start) = s.find(open) {
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
fn footnote_inside_callout_coalesces_into_slide_aside() {
    // bd-akkproqy: since #1050 a callout is still a `Callout` custom node
    // while the footnote transforms run, so their walkers must descend
    // into its slots. The note must be resolved, renumbered per slide and
    // land in that slide's aside, like a note outside a callout.
    let html = body_markup(&render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\n::: {.callout-note}\nInside.^[In the callout.]\n:::\n",
    ));
    assert!(
        compact(&html).contains("<sup>1</sup>"),
        "the callout's footnote ref must render as `<sup>1</sup>`; html:\n{}",
        &html[..html.len().min(3500)]
    );
    assert_eq!(
        count(&html, "class=\"aside-footnotes\""),
        1,
        "the callout's footnote must be coalesced into the slide aside; html:\n{}",
        &html[..html.len().min(3500)]
    );
    assert!(html.contains("In the callout."));
    assert!(
        !html.contains("role=\"doc-noteref\""),
        "no uncoalesced doc-noteref link may survive; html:\n{}",
        &html[..html.len().min(3500)]
    );
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

// ── video shortcode auto-stretch on reveal (bd-5b21rbaq Phase 5) ──────────
//
// A `{{< video >}}` on a slide emits a bare `<iframe>` with no size, which
// reveal renders at the browser default (~300×150). reveal sizes
// `section > iframe.r-stretch` to fill the slide, so the video shortcode adds
// `r-stretch` to the reveal iframe — but only when auto-stretch is on AND the
// author gave no explicit width/height (so explicit sizing always wins).

fn video_iframe_line(html: &str) -> String {
    html.lines()
        .find(|l| l.contains("<iframe") && l.contains("youtube.com/embed"))
        .unwrap_or_else(|| panic!("no youtube iframe in reveal output:\n{html}"))
        .to_string()
}

#[test]
fn video_reveal_default_iframe_gets_r_stretch() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## Video\n\n{{< video https://youtu.be/sAWFsP0Bbbk >}}\n",
    );
    let iframe = video_iframe_line(&html);
    assert!(
        iframe.contains("r-stretch"),
        "default reveal video iframe must carry `r-stretch` so reveal sizes it; iframe:\n{iframe}"
    );
}

#[test]
fn video_reveal_explicit_width_no_r_stretch() {
    // Explicit author sizing must win — no auto-stretch.
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## Video\n\n{{< video https://youtu.be/sAWFsP0Bbbk width=\"640\" >}}\n",
    );
    let iframe = video_iframe_line(&html);
    assert!(
        !iframe.contains("r-stretch"),
        "reveal video with explicit width must NOT be stretched; iframe:\n{iframe}"
    );
}

#[test]
fn video_reveal_auto_stretch_false_no_r_stretch() {
    // The `auto-stretch: false` global opt-out must suppress the class —
    // requires the Lua meta bridge to forward the boolean to the handler.
    let html = render_revealjs(
        "---\nformat: revealjs\nauto-stretch: false\n---\n\n## Video\n\n{{< video https://youtu.be/sAWFsP0Bbbk >}}\n",
    );
    let iframe = video_iframe_line(&html);
    assert!(
        !iframe.contains("r-stretch"),
        "reveal video must NOT be stretched when `auto-stretch: false`; iframe:\n{iframe}"
    );
}

// ── absolute positioning ─────────────────────────────────────────────────

/// The `<img …>` tag whose `src` names `file`.
fn img_tag(html: &str, file: &str) -> String {
    let needle = format!("src=\"{file}\"");
    let at = html
        .find(&needle)
        .unwrap_or_else(|| panic!("no <img> for {file} in:\n{}", body_markup(html)));
    let start = html[..at].rfind("<img").expect("img tag start");
    let end = at + html[at..].find('>').expect("img tag end");
    html[start..=end].to_string()
}

#[test]
fn absolute_image_position_becomes_style() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\n\
         ![](a.png){.absolute top=200 left=400 width=\"100\"}\n\nBody.\n",
    );
    let img = img_tag(&html, "a.png");
    assert!(
        img.contains("style=\"top: 200px; left: 400px; width: 100px;\""),
        "position attributes of an `.absolute` image must become an inline style \
         with `px` on bare numbers (Quarto 1 `applyPosition`); img: {img}"
    );
    assert!(img.contains("class=\"absolute\""), "class kept; img: {img}");
    assert!(
        !img.contains("data-top") && !img.contains("data-left") && !img.contains(" width=\"100\""),
        "the raw position attributes must be gone; img: {img}"
    );
}

#[test]
fn absolute_div_and_span_position_becomes_style() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\n\
         ::: {.absolute bottom=20 right=\"10%\"}\nBoxed.\n:::\n\n\
         Text [pinned]{.absolute top=0 left=\"2em\"} here.\n",
    );
    let body = body_markup(&html);
    assert!(
        body.contains("<div class=\"absolute\" style=\"bottom: 20px; right: 10%;\">"),
        "an `.absolute` div gets the style, and a value with a unit passes through; body:\n{body}"
    );
    assert!(
        body.contains("<span class=\"absolute\" style=\"top: 0px; left: 2em;\">pinned</span>"),
        "an `.absolute` span gets the style; body:\n{body}"
    );
    assert!(
        !body.contains("data-bottom") && !body.contains("data-right") && !body.contains("data-top"),
        "no position attribute may leak as `data-*`; body:\n{body}"
    );
}

#[test]
fn absolute_rule_ships_with_the_deck() {
    let css = rendered_quarto_reveal_css();
    assert!(
        compact(&css).contains(".reveal.slidessection.absolute{position:absolute;display:block;}"),
        "the deck stylesheet must carry the `.absolute` rule (Quarto 1 `styles.html`); css:\n{css}"
    );
}

/// Auto-stretch must keep skipping an `.absolute` image now that its `width`
/// and `height` have moved into `style`: the image stays in its paragraph and
/// gets no `r-stretch`.
#[test]
fn absolute_image_is_not_stretched() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## S\n\n![](a.png){.absolute top=10 left=10}\n",
    );
    let img = img_tag(&html, "a.png");
    assert!(
        !img.contains("r-stretch"),
        "an `.absolute` image must not be stretched; img: {img}"
    );
    assert!(
        compact(&body_markup(&html)).contains("<p><imgsrc=\"a.png\""),
        "an `.absolute` image stays inside its paragraph"
    );
}

/// The rewrite is revealjs-only, as in Quarto 1 (`reveal.lua` returns no
/// filter for other formats).
#[test]
fn absolute_attributes_untouched_in_html() {
    let html = render_as(
        "---\nformat: html\n---\n\n## S\n\n::: {.absolute top=200}\nBoxed.\n:::\n",
        "html",
    );
    assert!(
        html.contains("<div class=\"absolute\" data-top=\"200\">"),
        "`format: html` must leave `.absolute` attributes alone; html:\n{}",
        body_markup(&html)
    );
}

// ── aside placement ──────────────────────────────────────────────────────
//
// An aside is `position: absolute; bottom: 20px`, so it reaches the bottom of
// the slide only when the slide's `<section>` is as tall as the deck. These
// rules are CSS-only; where the aside lands was measured in a browser against
// Quarto 1 (claude-notes/plans/2026-11-13-layout-gaps-revealjs-absolute-aside-html-columns.md).

/// Quarto 1: `.reveal .slide:not(.center) { height: 100%; }` (`styles.html`).
/// q2 sections have no `slide` class and stack wrappers are sections too, so
/// leaf slides are picked out with reveal's run-time `stack` class; a centered
/// deck is recognised on the `.reveal` wrapper, where reveal puts `center`.
#[test]
fn leaf_slides_are_full_height() {
    let css = compact(&rendered_quarto_reveal_css());
    assert!(
        css.contains(".reveal:not(.center).slidessection:not(.stack):not(.center){height:100%;}"),
        "leaf slides of a top-aligned deck must be full height so a bottom aside \
         reaches the slide bottom"
    );
}

/// Quarto 1: `.reveal .slide.scrollable { overflow-y: auto; }` and, for its
/// aside, `position: relative; margin-top: 1em` so the aside follows the
/// scrolled content instead of covering it.
#[test]
fn scrollable_slide_scrolls_and_its_aside_follows_the_content() {
    let css = compact(&rendered_quarto_reveal_css());
    assert!(
        css.contains(".reveal.slidessection.scrollable{overflow-y:auto;}"),
        "a `.scrollable` slide must scroll"
    );
    assert!(
        css.contains(
            ".reveal.slidessection.scrollableaside.aside{position:relative;margin-top:1em;}"
        ),
        "the aside of a `.scrollable` slide must be in the flow"
    );
}

/// Quarto 1: `.reveal.center .slide aside { position: initial; }`.
#[test]
fn centered_deck_aside_is_not_pinned() {
    let css = compact(&rendered_quarto_reveal_css());
    assert!(
        css.contains(".reveal.center.slidesaside.aside{position:initial;}"),
        "in a centered deck the aside must stay in the flow"
    );
}

/// The classes the rules above key on reach the slide `<section>`.
#[test]
fn scrollable_and_center_classes_reach_the_section() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## One {.scrollable}\n\nBody.\n\n\
         ## Two {.center}\n\nBody.\n",
    );
    let body = body_markup(&html);
    assert!(
        body.contains("<section id=\"one\" class=\"section scrollable\">"),
        "`.scrollable` on the slide heading must land on its section; body:\n{body}"
    );
    assert!(
        body.contains("<section id=\"two\" class=\"section center\">"),
        "`.center` on the slide heading must land on its section; body:\n{body}"
    );
}

// ── math ─────────────────────────────────────────────────────────────────
//
// `MathJsStage` puts the engine's config and loader markup into `meta.math`;
// the HTML templates print it through `$math$`. The reveal scaffold writes its
// own `<head>`, so it must print the slot itself. Typesetting was measured in a
// browser against Quarto 1 (see the revealjs math plan).

const MATH_DECK_BODY: &str = "## Inline\n\nInline $e^{i\\pi}+1=0$ here.\n\n\
     $$\\int_0^1 x^2\\,dx = \\frac13$$ {#eq-one}\n\n## Later\n\nSee @eq-one.\n";

fn math_deck(method: Option<&str>) -> String {
    let method_line = method
        .map(|m| format!("html-math-method: {m}\n"))
        .unwrap_or_default();
    render_revealjs(&format!(
        "---\nformat: revealjs\n{method_line}---\n\n{MATH_DECK_BODY}"
    ))
}

/// The `<head>` of a rendered deck.
fn head_markup(html: &str) -> &str {
    &html[..html.find("</head>").expect("deck has </head>")]
}

/// Default engine (MathJax 3): the config block and the loader land in
/// `<head>` once each, config first, and the math markup is intact.
#[test]
fn mathjax_loader_reaches_deck_head() {
    let html = math_deck(None);
    let head = head_markup(&html);
    assert_eq!(
        count(&html, "window.MathJax = {"),
        1,
        "one MathJax config block; head:\n{head}"
    );
    let config = head
        .find("window.MathJax = {")
        .expect("MathJax config must be in <head>");
    let loader = head
        .find(r#"<script defer src="https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-chtml-full.js""#)
        .expect("MathJax loader must be in <head>");
    assert!(config < loader, "config must precede the loader");
    assert!(
        head.find("quarto-reveal.css").expect("stylesheet") < config,
        "math must follow the deck stylesheets"
    );
    assert!(html.contains(r#"<span class="math inline">\(e^{i\pi}+1=0\)</span>"#));
    assert!(
        html.contains(r"\tag{1}"),
        "the labelled equation keeps its tag"
    );
}

/// `html-math-method: katex`: stylesheet, KaTeX, and the auto-render call.
#[test]
fn katex_loader_reaches_deck_head() {
    let html = math_deck(Some("katex"));
    let head = head_markup(&html);
    assert!(
        head.contains(
            r#"<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.19.1/dist/katex.min.css">"#
        ),
        "KaTeX stylesheet in <head>; head:\n{head}"
    );
    assert!(head.contains(r#"<script defer src="https://cdn.jsdelivr.net/npm/katex@0.19.1/dist/katex.min.js"></script>"#));
    assert!(head.contains("renderMathInElement(document.body"));
    assert!(!html.contains("MathJax"), "KaTeX deck loads no MathJax");
}

/// A deck without math loads no engine.
#[test]
fn deck_without_math_loads_no_engine() {
    let html = render_revealjs("---\nformat: revealjs\n---\n\n## S\n\nNo math.\n");
    assert!(!html.contains("MathJax"), "no MathJax without math");
    assert!(!html.contains("katex"), "no KaTeX without math");
}

/// `plain`, `webtex` and `gladtex` leave `meta.math` unset, as in the HTML
/// format: the deck keeps the math markup and loads no engine.
#[test]
fn unsupported_math_methods_load_no_engine() {
    for method in ["plain", "webtex", "gladtex"] {
        let html = math_deck(Some(method));
        assert!(
            !html.contains("MathJax") && !html.contains("katex"),
            "`html-math-method: {method}` must load no engine"
        );
    }
}

/// `html-math-method: mathml` converts every expression in the AST, so no
/// loader is needed and none is added.
#[test]
fn mathml_deck_needs_no_loader() {
    let html = math_deck(Some("mathml"));
    assert!(html.contains("<math"), "math converted to MathML");
    assert!(
        !html.contains("MathJax"),
        "no loader for a fully converted deck"
    );
}

/// Math in a callout, a table, and a speaker note is found and the loader is
/// added (the deck has no other math).
#[test]
fn math_in_callout_table_and_notes_loads_engine() {
    for body in [
        "## S\n\n::: {.callout-note}\nIn a callout $x^2$.\n:::\n",
        "## S\n\n| a | b |\n|---|---|\n| $y^2$ | 1 |\n",
        "## S\n\nText.\n\n::: {.notes}\nNote $z^2$.\n:::\n",
    ] {
        let html = render_revealjs(&format!("---\nformat: revealjs\n---\n\n{body}"));
        assert!(
            head_markup(&html).contains("window.MathJax = {"),
            "math must load the engine for body:\n{body}"
        );
    }
}

// ── table of contents (`toc: true`) ──────────────────────────────────────
//
// Quarto 1 (`share/formats/revealjs/pandoc/toc-slide.html`) adds a
// horizontal `<section id="TOC">` slide right after the title slide. Its list
// links use reveal's hash routing: `href="#/<id>"`, `id="/toc-<id>"`.

const TOC_DECK: &str = "---\ntitle: Deck\nformat: revealjs\ntoc: true\n---\n\n\
# Section A\n\n## Slide one\n\nText.\n\n## Slide two {#custom-id}\n\nText.\n\n\
### Sub heading\n\nNot a slide.\n\n# Section B {.unnumbered}\n\n\
## Slide three {.unlisted}\n\nText.\n\n## Slide four\n\nText.\n";

/// The `<section id="TOC">…</section>` markup of a rendered deck.
fn toc_slide(html: &str) -> &str {
    let start = html
        .find("<section id=\"TOC\"")
        .unwrap_or_else(|| panic!("deck has no TOC slide:\n{html}"));
    let end = start + html[start..].find("</section>").expect("TOC slide closes");
    &html[start..end]
}

#[test]
fn toc_slide_follows_the_title_slide() {
    let html = render_revealjs(TOC_DECK);
    let title = html.find("id=\"title-slide\"").expect("title slide");
    let toc = html.find("<section id=\"TOC\"").expect("TOC slide");
    let first_body = html.find("id=\"section-a\"").expect("first body slide");
    assert!(
        title < toc && toc < first_body,
        "TOC sits between title and body"
    );
    // A horizontal slide: no section is open around it.
    let before = &html[..toc];
    assert!(
        before.matches("<section").count() == before.matches("</section>").count(),
        "TOC slide must be top-level (horizontal)"
    );
}

#[test]
fn toc_slide_lists_sections_and_slides_like_quarto_1() {
    let html = render_revealjs(TOC_DECK);
    let toc = compact(toc_slide(&html));
    let expected = compact(
        r##"<section id="TOC" class="section"><nav role="doc-toc">
<h2 id="toc-title">Table of contents</h2>
<ul>
<li><a href="#/section-a" id="/toc-section-a">Section A</a>
<ul>
<li><a href="#/slide-one" id="/toc-slide-one">Slide one</a></li>
<li><a href="#/custom-id" id="/toc-custom-id">Slide two</a>
<ul>
<li><a href="#/sub-heading" id="/toc-sub-heading">Sub heading</a></li>
</ul></li>
</ul></li>
<li><a href="#/section-b" id="/toc-section-b">Section B</a>
<ul>
<li><a href="#/slide-four" id="/toc-slide-four">Slide four</a></li>
</ul></li>
</ul>
</nav>"##,
    );
    assert_eq!(toc, expected);
}

#[test]
fn toc_false_or_unset_adds_no_slide() {
    for toc in ["toc: false\n", ""] {
        let html = render_revealjs(&format!(
            "---\ntitle: D\nformat: revealjs\n{toc}---\n\n## A\n\n## B\n"
        ));
        assert!(!html.contains("id=\"TOC\""), "no TOC slide for `{toc}`");
    }
}

#[test]
fn toc_without_title_slide_comes_first() {
    let html = render_revealjs("---\nformat: revealjs\ntoc: true\n---\n\n## Eins\n\n## Zwei\n");
    let toc = html.find("<section id=\"TOC\"").expect("TOC slide");
    let first = html.find("<section").expect("a slide");
    assert_eq!(
        toc, first,
        "the TOC is the first slide without a title slide"
    );
}

#[test]
fn toc_options_depth_title_numbers_and_lang() {
    let html = render_revealjs(
        "---\ntitle: D\nformat: revealjs\ntoc: true\ntoc-depth: 1\n\
         toc-title: \"Agenda *today*\"\n---\n\n# Section A\n\n## Slide one\n\n# Section B\n\n## Slide two\n",
    );
    let toc = toc_slide(&html);
    assert!(
        toc.contains("<h2 id=\"toc-title\">Agenda <em>today</em></h2>"),
        "{toc}"
    );
    assert!(
        toc.contains("#/section-a") && toc.contains("#/section-b"),
        "{toc}"
    );
    assert!(
        !toc.contains("slide-one"),
        "toc-depth: 1 lists only `#` sections: {toc}"
    );

    let html = render_revealjs(
        "---\ntitle: D\nlang: de\nformat: revealjs\ntoc: true\n---\n\n## Eins\n\n## Zwei\n",
    );
    assert!(toc_slide(&html).contains(">Inhaltsverzeichnis</h2>"));

    let html = render_revealjs(
        "---\nformat: revealjs\ntoc: true\nnumber-sections: true\n---\n\n# Section A\n\n## Slide one\n",
    );
    let toc = toc_slide(&html);
    assert!(
        toc.contains("<span class=\"header-section-number\">1</span> Section A"),
        "{toc}"
    );
}

/// Quarto 1 does not number the title slide's `<h1 class="title">`: with
/// `number-sections`, the first `#` section is 1, on its slide and in the
/// TOC. q2 counted the generated title heading as section 1.
#[test]
fn number_sections_skip_the_title_slide_heading() {
    let html = render_revealjs(
        "---\ntitle: Deck\nformat: revealjs\ntoc: true\nnumber-sections: true\n---\n\n\
         # Section A\n\n## Slide one\n\n# Section B\n\n## Slide two\n",
    );
    let title_start = html.find("id=\"title-slide\"").expect("title slide");
    let title_end = title_start + html[title_start..].find("</section>").unwrap();
    assert!(
        !html[title_start..title_end].contains("header-section-number"),
        "the title heading must not be numbered"
    );
    let toc = toc_slide(&html);
    assert!(
        toc.contains("<span class=\"header-section-number\">1</span> Section A")
            && toc.contains("<span class=\"header-section-number\">2</span> Section B"),
        "{toc}"
    );
    assert!(
        html.contains("<span class=\"header-section-number\">1.1</span> Slide one"),
        "slide numbers start at 1.1"
    );
}

// ── tabsets (bd-y5j0m776) ────────────────────────────────────────────────

const TABSET_DECK: &str = "---\nformat: revealjs\n---\n\n## Slide\n\n\
::: {.panel-tabset}\n## TabA\n\nTAB-A-BODY\n\n## TabB\n\nTAB-B-BODY\n:::\n";

/// Render a deck and return its HTML with the text of every file it links
/// whose URL contains `needle` (a stylesheet `href` or a script `src`).
fn render_revealjs_with_linked(contents: &str, needle: &str) -> (String, Vec<String>) {
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
    let html = read(&result.output_path);
    let dir = result.output_path.parent().expect("output dir");
    let mut linked = Vec::new();
    for attr in ["href=\"", "src=\""] {
        let mut rest = html.as_str();
        while let Some(start) = rest.find(attr) {
            let after = &rest[start + attr.len()..];
            let end = after.find('"').expect("closing quote");
            let url = &after[..end];
            if url.contains(needle) {
                linked.push(read(&dir.join(url)));
            }
            rest = &after[end..];
        }
    }
    (html, linked)
}

/// The `<script src>` URLs of a deck, in document order.
fn script_srcs(html: &str) -> Vec<String> {
    let needle = "<script src=\"";
    let mut rest = html;
    let mut out = Vec::new();
    while let Some(start) = rest.find(needle) {
        let after = &rest[start + needle.len()..];
        let end = after.find('"').expect("closing quote");
        out.push(after[..end].to_string());
        rest = &after[end..];
    }
    out
}

/// Quarto 1 renders a deck's tabset as Tabby markup (`tabbyTabs()` in
/// `panel-tabset.lua`): a list of links, then the panes. q2 left the tab
/// titles as headings, one pane under the other.
#[test]
fn tabset_becomes_tabby_markup() {
    let html = render_revealjs(TABSET_DECK);
    let body = compact(&body_markup(&html));
    assert!(
        body.contains(
            "<divclass=\"panel-tabset\"><ulid=\"tabset-1\"class=\"panel-tabset-tabby\">\
             <li><adata-tabby-defaulthref=\"#tabset-1-1\">TabA</a></li>\
             <li><ahref=\"#tabset-1-2\">TabB</a></li></ul>"
        ),
        "tab list; body:\n{body}"
    );
    assert!(
        body.contains(
            "<divclass=\"tab-content\"><divid=\"tabset-1-1\"><p>TAB-A-BODY</p></div>\
             <divid=\"tabset-1-2\"><p>TAB-B-BODY</p></div></div>"
        ),
        "panes; body:\n{body}"
    );
    assert!(
        !body.contains(">TabA</h2>") && !body.contains("nav-tabs"),
        "tab headings are consumed and no Bootstrap markup is written; body:\n{body}"
    );
}

/// An `.active` heading picks the default tab, and a nested tabset takes the
/// next id, as in the HTML format.
#[test]
fn tabset_active_tab_and_nesting() {
    let html = render_revealjs(
        "---\nformat: revealjs\n---\n\n## Slide\n\n\
         ::: {.panel-tabset}\n### One\n\n::: {.panel-tabset}\n#### Inner\n\nI.\n:::\n\n\
         ### Two {.active}\n\nT.\n:::\n",
    );
    let body = compact(&body_markup(&html));
    assert!(
        body.contains("<li><ahref=\"#tabset-1-1\">One</a></li>")
            && body.contains("<li><adata-tabby-defaulthref=\"#tabset-1-2\">Two</a></li>"),
        "the `.active` tab is the default; body:\n{body}"
    );
    assert!(
        body.contains("<divid=\"tabset-1-1\"><divclass=\"panel-tabset\"><ulid=\"tabset-2\""),
        "the nested tabset is tabset-2, inside the first pane; body:\n{body}"
    );
}

/// Tabby and its init script load after reveal.js, and only in a deck that
/// has a tabset.
#[test]
fn tabby_ships_only_with_a_tabset() {
    let (html, tabby) = render_revealjs_with_linked(TABSET_DECK, "tabby");
    let srcs = script_srcs(&html);
    let pos = |leaf: &str| {
        srcs.iter()
            .position(|s| s.ends_with(leaf))
            .unwrap_or_else(|| panic!("the deck must load {leaf}; scripts: {srcs:?}"))
    };
    assert!(pos("reveal.js") < pos("tabby.js"));
    assert!(pos("tabby.js") < pos("tabby-init.js"));
    assert_eq!(tabby.len(), 2, "both files are written beside the deck");
    assert!(tabby[0].contains("root.Tabby = factory(root)"));
    assert!(tabby[1].contains("new window.Tabby("));

    let plain = render_revealjs("---\nformat: revealjs\n---\n\n## S\n\nBody.\n");
    assert!(
        !plain.contains("tabby"),
        "a deck with no tabset loads no Tabby"
    );
}

/// The Tabby rules of Quarto 1's `_quarto-rules-tabby.scss` and the reveal
/// tab padding of its `quarto.scss` compile into the deck theme.
#[test]
fn tabset_rules_ship_with_the_theme() {
    let (_, themes) = render_revealjs_with_linked(TABSET_DECK, "theme-");
    assert_eq!(themes.len(), 1, "one theme stylesheet");
    let css = compact(&themes[0]);
    for rule in [
        ".panel-tabset[role=tablist]{border-bottom:1pxsolid#bbb;list-style:none;margin:0;padding:0;width:100%",
        ".panel-tabset[role=tab][aria-selected=true]{background-color:rgba(0,0,0,0);border:1pxsolid#bbb;border-bottom-color:#fff",
        ".reveal.panel-tabset[role=tab]{padding:.25em.7em",
    ] {
        assert!(css.contains(rule), "missing `{rule}` in the theme");
    }
}

/// The HTML format keeps its Bootstrap tabs and loads no Tabby.
#[test]
fn html_tabset_is_still_bootstrap() {
    let html = render_as(&TABSET_DECK.replace("revealjs", "html"), "html");
    assert!(html.contains("class=\"nav nav-tabs\""));
    assert!(!html.contains("panel-tabset-tabby") && !html.contains("tabby"));
}
