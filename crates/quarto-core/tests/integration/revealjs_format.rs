/*
 * tests/revealjs_format.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for `format: revealjs` (bd-2m4wanyd, Phase 1).
 */

//! End-to-end pipeline tests for revealjs presentation output.
//!
//! These drive a real `render_to_file(path, "revealjs", ...)` — the same
//! entry the CLI uses once a format is resolved — and assert on the
//! generated HTML. Passing `"revealjs"` explicitly tests the *pipeline*'s
//! revealjs support; the CLI's front-matter format resolution is covered
//! separately in the `quarto` crate's `revealjs_cli` integration test.
//!
//! What we pin (Phase 1, Tier-1):
//!
//! - The output is a reveal.js scaffold: `<div class="reveal"><div
//!   class="slides">…</div></div>` plus a `Reveal.initialize({…})` call.
//! - A title slide (`<section id="title-slide">`) carries the metadata
//!   title.
//! - A flat deck (title + N level-2 headings, no sections/verticals)
//!   produces exactly N+1 `<section>` slides.
//! - `Reveal.initialize` carries the configured options
//!   (`transition`, `slideNumber`, …) mapped to reveal config keys.
//! - The configured theme stylesheet is referenced.

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

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
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
    let result = render_to_file(&qmd_path, "revealjs", &options, runtime_arc())
        .expect("revealjs render failed");
    read(&result.output_path)
}

/// Whitespace-insensitive containment, for asserting on serialized
/// config without coupling to exact spacing/quoting.
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// A flat deck: title slide + three level-2 slides, no section
/// headers and no vertical subslides. Deterministic slide count.
const FLAT_DECK: &str = "\
---
title: \"My Talk\"
subtitle: \"A Subtitle\"
author: \"Ada Lovelace\"
date: \"2026-06-08\"
format:
  revealjs:
    transition: fade
    slide-number: true
---

## First Slide

- one
- two

## Second Slide

Some prose.

## Third Slide

More prose.
";

/// A richer deck exercising a section header (level 1) and a vertical
/// subslide (level 3) plus code and inline math. Used for scaffold /
/// structural facts that hold regardless of the exact nesting choice
/// (which is pinned precisely in the `RevealSlidesStage` unit tests).
const RICH_DECK: &str = "\
---
title: \"Rich Talk\"
format: revealjs
---

## Opening

Inline math $E = mc^2$.

```python
print(\"hi\")
```

# A Section

## Under Section

### A Vertical Subslide

Nested content.
";

#[test]
fn revealjs_render_produces_reveal_scaffold() {
    let html = render_revealjs(FLAT_DECK);
    assert!(
        html.contains("class=\"reveal\""),
        "expected reveal scaffold `class=\"reveal\"`; got {} bytes",
        html.len()
    );
    assert!(
        html.contains("class=\"slides\""),
        "expected `class=\"slides\"` container"
    );
    assert!(
        html.contains("Reveal.initialize"),
        "expected a `Reveal.initialize(...)` call"
    );
}

#[test]
fn revealjs_render_emits_title_slide() {
    let html = render_revealjs(FLAT_DECK);
    assert!(
        html.contains("id=\"title-slide\""),
        "expected a `<section id=\"title-slide\">`"
    );
    assert!(
        html.contains("My Talk"),
        "title slide must carry the metadata title"
    );
    // The deck defaults to center:false (top-aligned body slides), so the
    // title slide must carry a per-slide `.center` class to stay vertically
    // centered — matching Quarto 1.
    assert!(
        html.contains("title-slide center"),
        "title slide section must carry the `center` class; got:\n{html}"
    );
}

#[test]
fn revealjs_flat_deck_one_section_per_slide() {
    let html = render_revealjs(FLAT_DECK);
    // Title slide + 3 level-2 slides = 4 `<section` opening tags.
    let n = html.matches("<section").count();
    assert_eq!(
        n, 4,
        "flat deck (title + 3 H2) must yield 4 <section> tags, got {n}"
    );
}

#[test]
fn revealjs_initialize_carries_options() {
    let html = render_revealjs(FLAT_DECK);
    let c = compact(&html);
    assert!(
        c.contains("\"transition\":\"fade\""),
        "Reveal.initialize must carry transition: fade"
    );
    assert!(
        c.contains("\"slideNumber\":true"),
        "slide-number: true must map to reveal `slideNumber: true`"
    );
}

#[test]
fn revealjs_links_assets_instead_of_inlining() {
    // bd-jij5gge2: vendored reveal assets are LINKED (shared lib dir), not
    // inlined into every deck. Pin the <link>/<script src> tags + cascade
    // order, and assert the ~700KB core does NOT appear inline.
    let html = render_revealjs(FLAT_DECK);

    for href in [
        "talk_files/revealjs/reset.css",
        "talk_files/revealjs/reveal.css",
        "talk_files/revealjs/theme-white.css",
        "talk_files/revealjs/quarto-reveal.css",
    ] {
        assert!(
            html.contains(&format!(r#"<link rel="stylesheet" href="{href}">"#)),
            "expected a <link> to {href}; head was:\n{}",
            &html[..html.len().min(1200)]
        );
    }
    assert!(
        html.contains(r#"<script src="talk_files/revealjs/reveal.js"></script>"#),
        "expected a <script src> for reveal.js"
    );

    // Cascade order: reset → reveal → theme → quarto overrides.
    let at = |s: &str| html.find(s).unwrap_or_else(|| panic!("missing {s}"));
    assert!(at("reset.css") < at("revealjs/reveal.css"));
    assert!(at("revealjs/reveal.css") < at("theme-white.css"));
    assert!(at("theme-white.css") < at("quarto-reveal.css"));
    // reveal.js loads before the per-doc initialize().
    assert!(at("revealjs/reveal.js") < at("Reveal.initialize"));

    // The inlined-core markers from the OLD self-contained scaffold must
    // be gone, and the document is tiny (no ~700KB of inlined assets).
    assert!(
        !html.contains(r#"<style id="quarto-reveal">"#),
        "reveal CSS must not be inlined as <style>"
    );
    assert!(
        !html.contains(r#"<style id="theme">"#),
        "theme CSS must not be inlined as <style>"
    );
    assert!(
        html.len() < 50_000,
        "linked deck should be small; got {} bytes (core inlined?)",
        html.len()
    );
}

#[test]
fn revealjs_flushes_linked_assets_to_disk() {
    // The linked assets must actually be written to the page's `_files`
    // dir, or the <link>/<script> tags 404.
    let temp = tempfile::TempDir::new().unwrap();
    let qmd_path = temp.path().join("talk.qmd");
    write_file(&qmd_path, FLAT_DECK);
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let result = render_to_file(&qmd_path, "revealjs", &options, runtime_arc())
        .expect("revealjs render failed");

    let out_dir = result.output_path.parent().unwrap();
    let libs = out_dir.join("talk_files").join("revealjs");
    for f in [
        "reset.css",
        "reveal.css",
        "theme-white.css",
        "quarto-reveal.css",
        "reveal.js",
    ] {
        let p = libs.join(f);
        assert!(p.is_file(), "expected flushed asset {}", p.display());
        assert!(
            std::fs::metadata(&p).unwrap().len() > 0,
            "flushed asset {} is empty",
            p.display()
        );
    }
}

#[test]
fn revealjs_rich_deck_has_scaffold_and_section_divider() {
    let html = render_revealjs(RICH_DECK);
    assert!(html.contains("class=\"reveal\""), "scaffold present");
    // The H1 "A Section" becomes a section-divider slide; assert the
    // text survives into a section. Exact nesting is pinned in the
    // RevealSlidesStage unit tests, not here.
    assert!(
        html.contains("A Section"),
        "section header text must appear in the deck"
    );
    assert!(
        html.contains("A Vertical Subslide"),
        "level-3 subslide content must appear in the deck"
    );
}

/// bd-r9mkybwl Stage A: the theme slot now carries a *compiled* Quarto reveal
/// theme (not the stock reveal `white.css`). End-to-end through `render_to_file`,
/// the flushed `theme-white.css` must contain Quarto's look-fixing output:
/// left-aligned slides, non-uppercase headings, Quarto title-slide layout, and
/// the `--r-*` custom properties carrying Quarto's values.
#[test]
fn revealjs_theme_slot_is_compiled_quarto_theme() {
    let temp = tempfile::TempDir::new().unwrap();
    let qmd_path = temp.path().join("talk.qmd");
    write_file(&qmd_path, FLAT_DECK);
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let result = render_to_file(&qmd_path, "revealjs", &options, runtime_arc())
        .expect("revealjs render failed");

    let out_dir = result.output_path.parent().unwrap();
    let theme_css = read(
        &out_dir
            .join("talk_files")
            .join("revealjs")
            .join("theme-white.css"),
    );
    // The shipped theme is minified, so match whitespace-insensitively.
    let css = compact(&theme_css);

    // Quarto values flowed into the reveal custom properties.
    assert!(
        css.contains("--r-main-color:#222"),
        "compiled theme should set --r-main-color to Quarto's body color"
    );
    // Quarto's look-fixing rules are present.
    assert!(
        css.contains("text-align:left"),
        "compiled theme should left-align slides"
    );
    assert!(
        css.contains("#title-slide"),
        "compiled theme should style the Quarto title slide"
    );
    // reveal's default uppercase headings are turned off.
    assert!(
        !css.contains("uppercase"),
        "compiled Quarto theme must not uppercase headings"
    );
}
