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

/// End-to-end: `footer:`/`logo:` metadata produce a single deck-level footer +
/// logo placed OUTSIDE `.slides` (a direct child of `.reveal`). They must live
/// outside `.slides` because reveal applies CSS transforms to `.slides`/section
/// under which `position: fixed` breaks; see `revealjs::assemble`.
#[test]
fn revealjs_footer_and_logo_render_outside_slides() {
    let deck = "\
---
title: \"Footed Talk\"
logo: logo.png
footer: \"© 2026 [Quarto](https://quarto.org)\"
format: revealjs
---

## A slide

- one
";
    let html = render_revealjs(deck);

    // Logo image + footer container both present.
    assert!(
        html.contains(r#"<img class="slide-logo" src="logo.png">"#),
        "expected a .slide-logo img; got:\n{html}"
    );
    assert!(
        html.contains(r#"<div class="footer footer-default">"#),
        "expected a deck-level .footer; got:\n{html}"
    );
    // Footer inline markdown renders (link preserved, not flattened).
    assert!(
        html.contains(r#"<a href="https://quarto.org">Quarto</a>"#),
        "footer link should render as an anchor"
    );
    // `.reveal` carries `has-logo`.
    assert!(
        html.contains(r#"class="reveal has-logo""#),
        "reveal element should carry `has-logo`"
    );
    // Placement: footer/logo come AFTER the `.slides` container closes, so they
    // are direct children of `.reveal`, not nested inside transformed slides.
    let slides_open = html.find(r#"<div class="slides">"#).unwrap();
    let slides_close = slides_open + html[slides_open..].find("</div>").unwrap();
    assert!(
        html.find(r#"class="slide-logo""#).unwrap() > slides_close
            && html.find(r#"class="footer footer-default""#).unwrap() > slides_close,
        "footer/logo must be placed after `.slides` closes (outside it)"
    );
}

/// The canonical `page-footer:` key (shared with `format: html`) also drives the
/// reveal footer — the reveal render reuses the format-agnostic
/// `FooterGenerateTransform`, so no reveal-specific `footer:` alias is required.
#[test]
fn revealjs_page_footer_key_drives_footer() {
    let deck = "\
---
title: \"Canonical Footer\"
page-footer: \"Shared footer key\"
format: revealjs
---

## A slide

- one
";
    let html = render_revealjs(deck);
    assert!(
        html.contains(r#"<div class="footer footer-default">"#)
            && html.contains("Shared footer key"),
        "page-footer: should drive the reveal footer; got:\n{html}"
    );
}

/// End-to-end: a single-image slide gets reveal's `.r-stretch` (default-on);
/// a sized image, an inline-among-text image, and `auto-stretch: false` do not.
#[test]
fn revealjs_auto_stretch_single_image_slides() {
    // 1x1 transparent PNG so the resource collector's copy succeeds.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    let render = |deck: &str| -> String {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("pic.png"), PNG).unwrap();
        let qmd_path = temp.path().join("talk.qmd");
        write_file(&qmd_path, deck);
        let options = RenderToFileOptions {
            quiet: true,
            ..Default::default()
        };
        let result = render_to_file(&qmd_path, "revealjs", &options, runtime_arc())
            .expect("revealjs render failed");
        read(&result.output_path)
    };

    // Lone image slide → image gains r-stretch.
    let html = render("---\ntitle: T\nformat: revealjs\n---\n\n## Slide\n\n![](pic.png)\n");
    assert!(
        html.contains("r-stretch"),
        "lone-image slide should stretch; got:\n{html}"
    );

    // A standalone image amid explanatory text still stretches (matches Q1:
    // one image, in its own block, with sibling prose).
    let amid_text = render(
        "---\ntitle: T\nformat: revealjs\n---\n\n## Slide\n\nHere is the diagram:\n\n![](pic.png)\n",
    );
    assert!(
        amid_text.contains("r-stretch"),
        "a standalone image beside text should stretch; got:\n{amid_text}"
    );

    // Sized image → no stretch.
    let sized =
        render("---\ntitle: T\nformat: revealjs\n---\n\n## Slide\n\n![](pic.png){width=\"300\"}\n");
    assert!(
        !sized.contains("r-stretch"),
        "sized image must not stretch; got:\n{sized}"
    );

    // Inline image among text → no stretch.
    let inline = render(
        "---\ntitle: T\nformat: revealjs\n---\n\n## Slide\n\nHere is ![](pic.png) inline.\n",
    );
    assert!(
        !inline.contains("r-stretch"),
        "inline image among text must not stretch; got:\n{inline}"
    );

    // auto-stretch: false → opt-out.
    let off = render(
        "---\ntitle: T\nformat:\n  revealjs:\n    auto-stretch: false\n---\n\n## Slide\n\n![](pic.png)\n",
    );
    assert!(
        !off.contains("r-stretch"),
        "auto-stretch: false must disable stretching; got:\n{off}"
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
        "Reveal.initialize must carry the front-matter transition: fade"
    );
    // slide-number: true → Quarto's "c/t" format (linear navigation default).
    assert!(
        c.contains("\"slideNumber\":\"c/t\""),
        "slide-number: true must map to reveal `slideNumber: \"c/t\"`"
    );
}

/// Extract the fingerprinted theme href (`…/revealjs/theme-<hash>.css`) from a
/// rendered deck. The theme filename is content-fingerprinted (like
/// `format: html`'s theme), so tests can't hardcode it.
fn theme_href(html: &str) -> String {
    let marker = "revealjs/theme-";
    let start = html.find(marker).expect("theme css link present in deck");
    let rest = &html[start..];
    let end = rest.find(".css").expect("theme css href ends in .css") + ".css".len();
    rest[..end].to_string()
}

/// Find the flushed `theme-<hash>.css` file in a revealjs lib dir.
fn find_theme_css(libs: &Path) -> std::path::PathBuf {
    std::fs::read_dir(libs)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("theme-") && n.ends_with(".css"))
        })
        .expect("a theme-<hash>.css file should be flushed")
}

#[test]
fn revealjs_links_assets_instead_of_inlining() {
    // bd-jij5gge2: vendored reveal assets are LINKED (shared lib dir), not
    // inlined into every deck. Pin the <link>/<script src> tags + cascade
    // order, and assert the ~700KB core does NOT appear inline.
    let html = render_revealjs(FLAT_DECK);

    let theme = theme_href(&html);
    let mut hrefs = vec![
        "talk_files/revealjs/reset.css".to_string(),
        "talk_files/revealjs/reveal.css".to_string(),
        "talk_files/revealjs/quarto-reveal.css".to_string(),
    ];
    hrefs.push(format!("talk_files/{theme}"));
    for href in &hrefs {
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
    assert!(at("revealjs/reveal.css") < at(&theme));
    assert!(at(&theme) < at("quarto-reveal.css"));
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
    // The fixed-name assets, plus the fingerprinted theme file.
    let mut paths: Vec<std::path::PathBuf> =
        ["reset.css", "reveal.css", "quarto-reveal.css", "reveal.js"]
            .iter()
            .map(|f| libs.join(f))
            .collect();
    paths.push(find_theme_css(&libs));
    for p in paths {
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
    let libs = out_dir.join("talk_files").join("revealjs");
    let theme_css = read(&find_theme_css(&libs));
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

/// bd-j8qoyc0s Stage D2: a `_brand.yml` flows into the compiled reveal theme —
/// brand colors and typography reach the `--r-*` custom properties.
#[test]
fn revealjs_brand_yml_flows_into_theme() {
    let temp = tempfile::TempDir::new().unwrap();
    write_file(
        &temp.path().join("_brand.yml"),
        "color:\n  palette:\n    brandred: \"#cc0000\"\n  primary: brandred\ntypography:\n  base:\n    family: Georgia\n",
    );
    let qmd_path = temp.path().join("talk.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Branded\nbrand: _brand.yml\nformat: revealjs\n---\n\n## A slide\n\n- x\n",
    );
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let result = render_to_file(&qmd_path, "revealjs", &options, runtime_arc())
        .expect("revealjs render failed");

    let libs = result
        .output_path
        .parent()
        .unwrap()
        .join("talk_files")
        .join("revealjs");
    let css = compact(&read(&find_theme_css(&libs)));

    // Brand primary → $link-color → --r-link-color (#cc0000 minifies to #c00).
    assert!(
        css.contains("--r-link-color:#c00") || css.contains("--r-link-color:#cc0000"),
        "brand primary should drive --r-link-color\n{css}"
    );
    // Brand base font → --r-main-font.
    assert!(
        css.contains("--r-main-font:Georgia"),
        "brand base font should drive --r-main-font"
    );
}

/// bd-r9mkybwl Stage B: selecting a built-in theme (`theme: dark`) compiles
/// THAT theme into the deck — end-to-end through `render_to_file`. Guards the
/// theme-resolution + per-theme-compile path beyond the white default.
#[test]
fn revealjs_named_theme_is_compiled_and_selected() {
    let deck = "\
---
title: \"Dark Talk\"
format:
  revealjs:
    theme: dark
---

## A slide

- one
";
    let temp = tempfile::TempDir::new().unwrap();
    let qmd_path = temp.path().join("talk.qmd");
    write_file(&qmd_path, deck);
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let result = render_to_file(&qmd_path, "revealjs", &options, runtime_arc())
        .expect("revealjs render failed");

    let out_dir = result.output_path.parent().unwrap();
    let libs = out_dir.join("talk_files").join("revealjs");
    let css = compact(&read(&find_theme_css(&libs)));

    // The `dark` theme's dark background + light text flowed into --r-*.
    assert!(
        css.contains("--r-background-color:#191919"),
        "theme: dark should compile the dark background\n{css}"
    );
    assert!(
        css.contains("--r-main-color:#fff"),
        "theme: dark should use light text"
    );
}
