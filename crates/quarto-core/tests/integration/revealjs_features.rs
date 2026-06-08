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
