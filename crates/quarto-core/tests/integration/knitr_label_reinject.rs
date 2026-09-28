/*
 * tests/integration/knitr_label_reinject.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! bd-2lxj10z0 — empirical tests for plan §D2/§D3
//! (`claude-notes/plans/2026-09-28-knitr-label-preengine-sugaring-fix.md`).
//!
//! `PreEngineSugaringStage` lifts a crossref-classified `label:` out of a
//! code cell's body and onto the wrapping Div, so knitr never sees it and
//! falls back to a positional filename (`unnamed-chunk-N`) instead of the
//! label-derived one (`fig-cars-1.<ext>`).
//! `crates/quarto-core/src/crossref/label_reinject.rs` re-injects the
//! label into the text serialized for knitr, without touching the stored
//! AST. These tests drive the real per-document render path
//! (`render_document_to_file`, the same entry `q2 render` uses), with
//! real knitr — no mocks — and check two things a reasoning-only review
//! cannot: that the label actually reaches knitr's filename logic, and
//! that it does not leak into the `echo: true` source the reader sees
//! (§D3's stated risk against `quarto_ast_reconcile`'s structural-equality
//! check).
//!
//! Gate copied from `nested_cell_mask_render.rs` (Rscript binary + the R
//! `knitr` package) — a skip is a signal about the machine, not a pass.

#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ProjectContext;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn rscript_available() -> bool {
    Command::new("Rscript")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn knitr_r_package_available() -> bool {
    Command::new("Rscript")
        .args([
            "-e",
            "if (!requireNamespace(\"knitr\", quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn knitr_render_available() -> bool {
    rscript_available() && knitr_r_package_available()
}

fn skip(name: &str) -> bool {
    if knitr_render_available() {
        return false;
    }
    eprintln!("SKIP: knitr (Rscript + knitr package) not available — {name}");
    true
}

/// A labelled, captioned, `echo: true` R plot chunk written with the
/// authoring shorthand `codeblock_shorthand.rs` desugars — the exact
/// shape that stripped `label:` before this fix (and the same shape as
/// `crates/quarto/tests/smoke-all/typst/orange-book/chapter1.qmd`'s
/// `fig-cars` chunk, plan §D6).
const FIXTURE: &str = "---\ntitle: Label\nengine: knitr\n---\n\n\
```{r}\n\
#| label: fig-cars\n\
#| echo: true\n\
#| fig-cap: \"Cars plot\"\n\
plot(cars)\n\
```\n";

/// Render the fixture to HTML through the real per-document path and
/// return the rendered HTML plus the output path (the `_files` figure
/// directory sits beside it).
fn render(tmp: &TempDir) -> (String, std::path::PathBuf) {
    let input = tmp.path().join("label.qmd");
    std::fs::write(&input, FIXTURE).unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref())
        .expect("project discovery for the label-reinject fixture");

    let result = render_document_to_file(
        &input,
        "html",
        &RenderToFileOptions::default(),
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    )
    .unwrap_or_else(|e| panic!("render must succeed: {e}"));

    let html = std::fs::read_to_string(&result.output_path).expect("read rendered HTML");
    (html, result.output_path)
}

/// Remove HTML tags, keeping text-node content and ordering intact —
/// enough to reconstruct syntax-highlighted source into plain text
/// without needing an HTML parser. Entities pandoc's highlighter
/// commonly emits in code are decoded too, so a literal `#|` line
/// survives the round trip readably.
fn visible_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// The core fix (plan §D2): knitr must see the label again and use it
/// for the output filename, not the positional `unnamed-chunk-N`
/// fallback it produces when `label:` never reaches it.
#[test]
fn labelled_figure_uses_label_derived_filename() {
    if skip("labelled_figure_uses_label_derived_filename") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (_html, output_path) = render(&tmp);
    let figure_dir = output_path
        .parent()
        .unwrap()
        .join("label_files")
        .join("figure-html");
    assert!(
        figure_dir.is_dir(),
        "expected a figure-html output dir at {}",
        figure_dir.display()
    );
    let names: Vec<String> = std::fs::read_dir(&figure_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().any(|n| n.starts_with("fig-cars-1.")),
        "expected a fig-cars-1.* artifact in {figure_dir:?}; got {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.starts_with("unnamed-chunk")),
        "must not fall back to the positional unnamed-chunk name; got {names:?}"
    );
}

/// Plan §D3: the injected `#| label: fig-cars` line must not leak into
/// the *echoed* source `echo: true` shows the reader. This is the
/// reconciliation-equality risk the plan calls out as "an expectation,
/// not a verified fact" — knitr's `hooks.R` `quarto_opts` filter is
/// relied on (see the module doc in
/// `crates/quarto-core/src/crossref/label_reinject.rs`), and this test
/// is what actually verifies it rather than trusting that reasoning.
#[test]
fn label_option_is_not_echoed_in_rendered_source() {
    if skip("label_option_is_not_echoed_in_rendered_source") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (html, _output_path) = render(&tmp);
    let text = visible_text(&html);
    assert!(
        !text.contains("label: fig-cars") && !text.contains("label:fig-cars"),
        "rendered source must not literally echo the label option; visible text:\n{text}"
    );
    // Sanity: the code that *is* supposed to be visible must still be there,
    // so a failure above is a real leak, not visible_text swallowing everything.
    assert!(
        text.contains("plot(cars)"),
        "the actual chunk body must still be echoed; visible text:\n{text}"
    );
}

/// Plan §D5: knitr's own `cache` option keys its on-disk cache files by
/// the chunk's real (knitr-internal) label. This is a related,
/// out-of-scope side effect the plan says should self-resolve once the
/// label reaches knitr again — this test is the "worth a regression
/// test... if convenient" the plan asks for, not a separate fix.
#[test]
fn labelled_cached_chunk_uses_label_derived_cache_key() {
    if skip("labelled_cached_chunk_uses_label_derived_cache_key") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("cached.qmd");
    std::fs::write(
        &input,
        "---\ntitle: Cached\nengine: knitr\n---\n\n\
```{r}\n\
#| label: fig-cars\n\
#| cache: true\n\
plot(cars)\n\
```\n",
    )
    .unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref())
        .expect("project discovery for the cached-chunk fixture");
    let result = render_document_to_file(
        &input,
        "html",
        &RenderToFileOptions::default(),
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    )
    .unwrap_or_else(|e| panic!("render must succeed: {e}"));

    let cache_dir = result
        .output_path
        .parent()
        .unwrap()
        .join("cached_cache")
        .join("html");
    assert!(
        cache_dir.is_dir(),
        "expected a knitr cache dir at {}",
        cache_dir.display()
    );
    let names: Vec<String> = std::fs::read_dir(&cache_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().any(|n| n.starts_with("fig-cars_")),
        "cache key must be derived from the label, not position; got {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.starts_with("unnamed-chunk")),
        "cache key must not fall back to a positional name; got {names:?}"
    );
}
