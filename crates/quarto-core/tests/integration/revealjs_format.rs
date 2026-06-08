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
fn revealjs_links_theme_stylesheet() {
    let html = render_revealjs(FLAT_DECK);
    // Default theme stylesheet must be referenced (reveal theme CSS).
    assert!(
        html.contains("reveal") && html.to_lowercase().contains("theme"),
        "expected a reveal theme stylesheet reference"
    );
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
