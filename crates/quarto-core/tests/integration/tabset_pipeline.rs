/*
 * tabset_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-toc-tabset-titles-zq93gjvf: panel-tabset Divs must become
 * Bootstrap tab navigation, consuming their tab-title Headers
 * before sectionize/TOC ever see them.
 */

//! End-to-end integration tests for panel-tabset support.
//!
//! Every test drives the real render path (`ProjectPipeline` /
//! `render_to_file`) against a temp project and inspects the HTML
//! written to disk. The expected markup shape is Quarto 1's committed
//! render of the minimal repro, captured verbatim at
//! `claude-notes/plans/tabset-panel-tabset-investigation/q1-target-markup.html`.
//!
//! Plan: `claude-notes/plans/2026-08-17-tabset-panel-tabset.md`.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};

use crate::toc_markup::{render_index_with_toc, toc_nav};

/// The strand's minimal repro body: two real headings wrapping a
/// two-tab tabset. Q1's TOC has exactly the 2 real entries.
const TABSET_BODY: &str = "\
## Real heading

::: {.panel-tabset}

## Tab Alpha

Alpha content.

## Tab Beta

Beta content.

:::

## Another real heading

Text.
";

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Pull every `<script src="…">` URL out of an HTML string, in document
/// order. Same tailored parser as `bootstrap_js_pipeline.rs`.
fn extract_script_srcs(html: &str) -> Vec<String> {
    let needle = "<script src=\"";
    let mut search = html;
    let mut out = Vec::new();
    while let Some(start) = search.find(needle) {
        let after = &search[start + needle.len()..];
        let end = after
            .find('"')
            .expect("malformed <script>: missing closing quote on src");
        out.push(after[..end].to_string());
        search = &after[end..];
    }
    out
}

// ── The strand's own symptom: TOC pollution ─────────────────────────────

/// Tab-title Headers must be consumed by the tabset transform before
/// TOC generation: the TOC has exactly the two real headings, never
/// the tab names. (Q1 TOC for this document: `Real heading`,
/// `Another real heading` — nothing else.)
#[test]
fn tab_titles_do_not_leak_into_toc() {
    let html = render_index_with_toc(TABSET_BODY, "", None);
    let nav = toc_nav(&html);

    assert!(
        nav.contains("Real heading"),
        "TOC should contain 'Real heading'; got:\n{nav}"
    );
    assert!(
        nav.contains("Another real heading"),
        "TOC should contain 'Another real heading'; got:\n{nav}"
    );
    assert!(
        !nav.contains("Tab Alpha"),
        "tab title 'Tab Alpha' leaked into the TOC:\n{nav}"
    );
    assert!(
        !nav.contains("Tab Beta"),
        "tab title 'Tab Beta' leaked into the TOC:\n{nav}"
    );
}

/// The tab-title Headers must not survive as heading elements in the
/// body either — Q1 renders no `<h2>` for `Tab Alpha`/`Tab Beta`; the
/// titles live only inside the nav-link anchors.
#[test]
fn tab_title_headers_are_consumed() {
    let html = render_index_with_toc(TABSET_BODY, "", None);

    assert!(
        !html.contains("id=\"tab-alpha\""),
        "'Tab Alpha' still renders as a heading/section with its own id:\n{html}"
    );
    assert!(
        !html.contains("id=\"tab-beta\""),
        "'Tab Beta' still renders as a heading/section with its own id:\n{html}"
    );
}

// ── Markup shape (contract: Q1's committed render) ──────────────────────

/// The tabset renders as Bootstrap nav-tabs + tab-content/tab-pane,
/// with the ids, data-bs wiring, and aria attributes Q1 emits.
#[test]
fn tabset_renders_bootstrap_nav_markup() {
    let html = render_index_with_toc(TABSET_BODY, "", None);

    // Navigation list.
    assert!(
        html.contains("class=\"nav nav-tabs\"") && html.contains("role=\"tablist\""),
        "expected <ul class=\"nav nav-tabs\" role=\"tablist\">; got:\n{html}"
    );
    // Nav links carry the Bootstrap tab wiring and aria attributes.
    for needle in [
        "id=\"tabset-1-1-tab\"",
        "data-bs-toggle=\"tab\"",
        "data-bs-target=\"#tabset-1-1\"",
        "role=\"tab\"",
        "aria-controls=\"tabset-1-1\"",
        "id=\"tabset-1-2-tab\"",
        "data-bs-target=\"#tabset-1-2\"",
    ] {
        assert!(
            html.contains(needle),
            "expected {needle} in rendered nav; got:\n{html}"
        );
    }
    // Titles render inside the nav anchors.
    assert!(
        html.contains(">Tab Alpha</a>"),
        "expected 'Tab Alpha' inside a nav-link anchor; got:\n{html}"
    );
    assert!(
        html.contains(">Tab Beta</a>"),
        "expected 'Tab Beta' inside a nav-link anchor; got:\n{html}"
    );

    // Panes.
    assert!(
        html.contains("class=\"tab-content\""),
        "expected a <div class=\"tab-content\"> wrapper; got:\n{html}"
    );
    for needle in [
        "id=\"tabset-1-1\"",
        "id=\"tabset-1-2\"",
        "role=\"tabpanel\"",
        "aria-labelledby=\"tabset-1-1-tab\"",
        "aria-labelledby=\"tabset-1-2-tab\"",
    ] {
        assert!(
            html.contains(needle),
            "expected {needle} in tab panes; got:\n{html}"
        );
    }
    // Pane content survives.
    assert!(
        html.contains("Alpha content.") && html.contains("Beta content."),
        "tab pane content must survive the transform; got:\n{html}"
    );
}

/// With no explicit `.active` header, the first tab is active — on
/// both the nav-link and its pane — and the second is not.
#[test]
fn first_tab_active_by_default() {
    let html = render_index_with_toc(TABSET_BODY, "", None);

    assert!(
        html.contains("class=\"nav-link active\"") || html.contains("nav-link active\""),
        "first nav-link should carry `active`; got:\n{html}"
    );
    assert!(
        html.contains("aria-selected=\"true\"") && html.contains("aria-selected=\"false\""),
        "expected one selected and one unselected nav-link; got:\n{html}"
    );
    assert!(
        html.contains("tab-pane active"),
        "first pane should carry `active`; got:\n{html}"
    );
}

/// An explicit `.active` class on a tab's Header wins over the
/// first-tab default (Q1: `is_active` honors the class when any
/// header carries it).
#[test]
fn explicit_active_class_wins_over_first_tab() {
    let body = "\
::: {.panel-tabset}

## Tab Alpha

Alpha content.

## Tab Beta {.active}

Beta content.

:::
";
    let html = render_index_with_toc(body, "", None);

    // The second tab (and only it) is selected.
    let selected_true = html.matches("aria-selected=\"true\"").count();
    assert_eq!(
        selected_true, 1,
        "exactly one nav-link should be selected; got:\n{html}"
    );
    let second_link_pos = html
        .find("id=\"tabset-1-2-tab\"")
        .expect("second nav-link present");
    let selected_pos = html.find("aria-selected=\"true\"").unwrap();
    let first_link_pos = html
        .find("id=\"tabset-1-1-tab\"")
        .expect("first nav-link present");
    assert!(
        selected_pos > first_link_pos && (selected_pos as i64 - second_link_pos as i64).abs() < 200,
        "aria-selected=\"true\" should sit on the second nav-link; got:\n{html}"
    );
}

/// Two tabsets in one document get distinct id families
/// (`tabset-1-*`, `tabset-2-*`).
#[test]
fn multiple_tabsets_get_distinct_ids() {
    let body = "\
::: {.panel-tabset}

## A1

one

## A2

two

:::

::: {.panel-tabset}

## B1

three

## B2

four

:::
";
    let html = render_index_with_toc(body, "", None);
    for needle in ["id=\"tabset-1-1\"", "id=\"tabset-2-1\""] {
        assert!(
            html.contains(needle),
            "expected {needle} (per-document tabset counter); got:\n{html}"
        );
    }
}

// ── Grouped tabsets ─────────────────────────────────────────────────────

/// `group="language"` lands as `data-group="language"` on the outer
/// panel div (Q1's grouped-tabset contract; the sync JS selects on
/// `div[data-group]`).
#[test]
fn grouped_tabset_emits_data_group() {
    let body = "\
::: {.panel-tabset group=\"language\"}

## Python

py

## R

r

:::
";
    let html = render_index_with_toc(body, "", None);
    assert!(
        html.contains("data-group=\"language\""),
        "expected data-group=\"language\" on the panel div; got:\n{html}"
    );
    // The attribute alone is vacuous (the HTML writer data-prefixes
    // unknown attrs even on a passthrough div) — the grouped tabset
    // must ALSO have been resolved into tab navigation, since the
    // sync JS selects `div[data-group] a[id^='tabset-']`.
    assert!(
        html.contains("data-bs-toggle=\"tab\"") && html.contains("id=\"tabset-1-1-tab\""),
        "grouped tabset must resolve to nav-tabs markup; got:\n{html}"
    );
}

// ── Sync JS ships with bootstrap JS ─────────────────────────────────────

/// A themed single-doc render ships the tabsets sync module alongside
/// bootstrap JS (design decision 4: same gate, unconditional). The
/// script tag resolves to a real file whose content is the localStorage
/// sync module.
#[test]
fn tabsets_js_ships_alongside_bootstrap_js() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Test\nformat:\n  html:\n    theme: cosmo\n---\n\nHello.\n",
    );

    let runtime = std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("single-doc render");

    let html = std::fs::read_to_string(&result.output_path).expect("read rendered html");
    let scripts = extract_script_srcs(&html);
    let tabset_scripts: Vec<_> = scripts
        .iter()
        .filter(|s| s.contains("tabsets.js"))
        .collect();
    assert_eq!(
        tabset_scripts.len(),
        1,
        "expected exactly one tabsets.js <script>; all scripts: {scripts:?}"
    );

    let on_disk: PathBuf = result.output_path.parent().unwrap().join(tabset_scripts[0]);
    assert!(
        on_disk.exists(),
        "expected tabsets JS on disk at {}",
        on_disk.display()
    );
    let js = std::fs::read_to_string(&on_disk).expect("read tabsets js");
    assert!(
        js.contains("quarto-persistent-tabsets-data"),
        "tabsets.js should carry the localStorage sync module"
    );
}

/// `theme: none` (minimal HTML) opts out of the whole Bootstrap JS
/// family, tabsets module included — same gate as `BootstrapJsStage`.
#[test]
fn theme_none_omits_tabsets_js() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Test\nformat:\n  html:\n    theme: none\n---\n\nHello.\n",
    );

    let runtime = std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("single-doc render");

    let html = std::fs::read_to_string(&result.output_path).expect("read rendered html");
    let scripts = extract_script_srcs(&html);
    assert!(
        !scripts.iter().any(|s| s.contains("tabsets.js")),
        "theme: none must not ship tabsets.js; scripts: {scripts:?}"
    );
}
