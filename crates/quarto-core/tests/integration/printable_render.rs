/*
 * tests/integration/printable_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-vhdknrvl (issue #315): the "open printable version" feature renders
 * the current preview document through the HTML pipeline so it can be
 * opened as a self-contained, paginatable top-level tab.
 */

//! Render-level contract for the printable-export feature.
//!
//! The WASM `render_printable(path)` export coerces the document's
//! *preview* format to its HTML-output equivalent (the inverse of
//! `map_format_for_preview`): `q2-preview → html`, `q2-slides →
//! revealjs`. It then renders through the HTML pipeline
//! (`render_qmd_to_html`) with the document's real path.
//!
//! These tests pin the two load-bearing assumptions of that coercion,
//! natively and in-process (the WASM crate itself is `cdylib`-only and
//! not native-testable):
//!
//! 1. A document whose frontmatter literally says `format: q2-preview`
//!    still renders to a **full HTML document** when driven with an
//!    HTML [`Format`], and a relative user image's `src` survives
//!    verbatim (so the JS `makeSelfContainedHtml` inliner can resolve
//!    and inline it).
//! 2. A `format: revealjs` document renders to a **standalone reveal
//!    deck** (the `reveal`/`slides` shell) — i.e. the coercion target
//!    `q2-slides → revealjs` reaches the deck assembler, not a bare
//!    HTML page.

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::pipeline::{HtmlRenderConfig, render_qmd_to_html};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn test_project() -> ProjectContext {
    ProjectContext {
        dir: PathBuf::from("/project"),
        config: quarto_core::project::ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/test.qmd")],
        output_dir: PathBuf::from("/project"),
    }
}

fn fresh_runtime() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

/// Render `content` through the HTML pipeline with `format`, returning
/// the full HTML string — mirroring what `render_printable` does after
/// coercing the preview format to its HTML-output equivalent.
fn render_html(content: &[u8], format: Format) -> String {
    let project = test_project();
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    pollster::block_on(render_qmd_to_html(
        content,
        "test.qmd",
        &mut ctx,
        &HtmlRenderConfig::default(),
        fresh_runtime(),
    ))
    .expect("html pipeline render")
    .html
}

#[test]
fn q2_preview_frontmatter_renders_as_html_with_image_preserved() {
    // A document authored with the preview pseudo-format, referencing a
    // relative image in a subdirectory. `render_printable` coerces
    // `q2-preview → html`; here we drive `render_qmd_to_html` with the
    // coerced HTML format directly.
    let content = br#"---
title: "Printable Doc"
format: q2-preview
---

# Heading

Some text and an image.

![A plot](figures/plot.png)

More text after the image.
"#;

    let html = render_html(content, Format::html());

    // A full HTML document (not preview AST JSON).
    assert!(
        html.contains("<body") && html.contains("</html>"),
        "expected a full HTML document, got:\n{html}"
    );
    // Document content is present.
    assert!(html.contains("Heading"), "heading missing:\n{html}");
    // The relative image src is preserved verbatim so the JS inliner can
    // resolve and inline it against the document directory.
    assert!(
        html.contains(r#"src="figures/plot.png""#),
        "relative image src not preserved:\n{html}"
    );
}

#[test]
fn revealjs_renders_standalone_deck() {
    // The coercion target for slides is `q2-slides → revealjs`, which
    // must reach the reveal deck assembler.
    let content = br#"---
title: "Printable Deck"
format: revealjs
---

## Slide One

Hello.

## Slide Two

- a
- b
"#;

    let html = render_html(
        content,
        Format::from_format_string("revealjs").expect("revealjs format"),
    );

    assert!(
        html.contains(r#"class="reveal"#),
        "expected a reveal deck shell, got:\n{html}"
    );
    assert!(
        html.contains(r#"class="slides"#),
        "expected the reveal slides container, got:\n{html}"
    );
}
