/*
 * tests/page_navigation_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for Phase 4 of the website-projects epic: the
 * prev/next page-navigation strip end-to-end through `ProjectPipeline`.
 */

//! End-to-end integration tests for page-navigation (prev/next).
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-4.md` §Tests
//! 39–44. Each test writes a small fixture to a temp dir, drives it
//! through `ProjectPipeline`, then inspects the rendered HTML for the
//! `<nav class="page-navigation">` strip.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn html_format() -> Format {
    Format::html()
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Build and render a website project fixture. Returns the project
/// directory plus the map of `output-stem → rendered HTML`.
fn render_project(fixture: impl FnOnce(&std::path::Path)) -> (PathBuf, Vec<(String, String)>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        html_format(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );

    std::mem::forget(temp);

    let outputs: Vec<(String, String)> = summary
        .outputs
        .iter()
        .map(|out| {
            let stem = out
                .output_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let html = read(&out.output_path);
            (stem, html)
        })
        .collect();
    (project_dir, outputs)
}

fn find_html<'a>(outputs: &'a [(String, String)], stem: &str) -> &'a str {
    &outputs
        .iter()
        .find(|(s, _)| s == stem)
        .unwrap_or_else(|| {
            panic!(
                "no output for stem '{}'; got: {:?}",
                stem,
                outputs.iter().map(|(s, _)| s).collect::<Vec<_>>()
            )
        })
        .1
}

/// Extract the slice of HTML between `<nav class="page-navigation">`
/// and `</nav>` for assertions about its internal structure.
fn page_nav_block<'a>(html: &'a str) -> &'a str {
    let start = html
        .find("<nav class=\"page-navigation\">")
        .unwrap_or_else(|| panic!("no <nav class=\"page-navigation\"> in:\n{}", html));
    let after_start = &html[start..];
    let end = after_start
        .find("</nav>")
        .unwrap_or_else(|| panic!("page-nav <nav> never closes"));
    &after_start[..end + "</nav>".len()]
}

// === Test 39 ==============================================================

/// Test 39 — three-page website with a linear sidebar. Page 1 has
/// only `next`, page 2 has both, page 3 has only `prev`.
#[test]
fn pipeline_page_nav_three_page_website() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n      - docs.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout us.\n",
        );
        write(
            &project_dir.join("docs.qmd"),
            "---\ntitle: Docs\n---\n\nDocumentation.\n",
        );
    });

    let index_html = find_html(&outputs, "index");
    let about_html = find_html(&outputs, "about");
    let docs_html = find_html(&outputs, "docs");

    // Every page has a page-navigation block.
    let index_pn = page_nav_block(index_html);
    let about_pn = page_nav_block(about_html);
    let docs_pn = page_nav_block(docs_html);

    // index.html: prev wrapper empty, next points at about.html.
    assert!(
        index_pn.contains("href=\"about.html\""),
        "index page-nav should link forward to about; got:\n{}",
        index_pn
    );
    assert_eq!(
        index_pn.matches("class=\"pagination-link\"").count(),
        1,
        "index page-nav: only the next anchor should exist; got:\n{}",
        index_pn
    );

    // about.html: prev → index, next → docs.
    assert!(about_pn.contains("href=\"index.html\""));
    assert!(about_pn.contains("href=\"docs.html\""));
    assert_eq!(
        about_pn.matches("class=\"pagination-link\"").count(),
        2,
        "about page-nav: both prev + next anchors should exist; got:\n{}",
        about_pn
    );

    // docs.html: prev → about, next wrapper empty.
    assert!(docs_pn.contains("href=\"about.html\""));
    assert_eq!(
        docs_pn.matches("class=\"pagination-link\"").count(),
        1,
        "docs page-nav: only the prev anchor should exist; got:\n{}",
        docs_pn
    );
}

// === Test 40 ==============================================================

/// Test 40 — `page-navigation: false` at doc level disables on that
/// page only; sibling pages keep their page-nav.
#[test]
fn pipeline_page_nav_disabled_at_doc_level() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n      - docs.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\npage-navigation: false\n---\n\nAbout us.\n",
        );
        write(
            &project_dir.join("docs.qmd"),
            "---\ntitle: Docs\n---\n\nDocumentation.\n",
        );
    });

    let index_html = find_html(&outputs, "index");
    let about_html = find_html(&outputs, "about");
    let docs_html = find_html(&outputs, "docs");

    assert!(index_html.contains("<nav class=\"page-navigation\">"));
    assert!(
        !about_html.contains("<nav class=\"page-navigation\">"),
        "about.html should not have page-nav (page-navigation: false); got:\n{}",
        &about_html[..about_html.len().min(1500)]
    );
    assert!(docs_html.contains("<nav class=\"page-navigation\">"));
}

// === Test 41 ==============================================================

/// Test 41 — `page-navigation: false` at the top of `_quarto.yml`
/// disables on every page.
#[test]
fn pipeline_page_nav_disabled_at_project_level() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             page-navigation: false\n\
             website:\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n      - docs.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout us.\n",
        );
        write(
            &project_dir.join("docs.qmd"),
            "---\ntitle: Docs\n---\n\nDocumentation.\n",
        );
    });

    for (stem, html) in &outputs {
        assert!(
            !html.contains("<nav class=\"page-navigation\">"),
            "{}.html should not have page-nav; project-level disable failed",
            stem
        );
    }
}

// === Test 42 ==============================================================

/// Test 42 — separators in the sidebar break adjacency. With
/// `[a, ---, b]`, neither page gets a non-empty page-nav (each side's
/// only neighbor is a separator).
#[test]
fn pipeline_page_nav_honors_separator_boundary() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  sidebar:\n    contents:\n      - a.qmd\n      - \"---\"\n      - b.qmd\n",
        );
        write(&project_dir.join("a.qmd"), "---\ntitle: A\n---\n\nA.\n");
        write(&project_dir.join("b.qmd"), "---\ntitle: B\n---\n\nB.\n");
    });

    let a_html = find_html(&outputs, "a");
    let b_html = find_html(&outputs, "b");

    // Each page sits adjacent only to a separator on the relevant
    // side; with no neighbor on the other side either, the strip is
    // skipped entirely.
    assert!(
        !a_html.contains("<nav class=\"page-navigation\">"),
        "a.html: separator-as-next + no prev → no page-nav; got:\n{}",
        &a_html[..a_html.len().min(1500)]
    );
    assert!(
        !b_html.contains("<nav class=\"page-navigation\">"),
        "b.html: separator-as-prev + no next → no page-nav; got:\n{}",
        &b_html[..b_html.len().min(1500)]
    );
}

// === Test 43 ==============================================================

/// Test 43 — rendering one page does not leak active/neighbor state
/// into a sibling. Regression guard against stateful-transform bugs.
#[test]
fn pipeline_page_nav_cross_contamination_guard() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n      - docs.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout us.\n",
        );
        write(
            &project_dir.join("docs.qmd"),
            "---\ntitle: Docs\n---\n\nDocumentation.\n",
        );
    });

    let index_html = find_html(&outputs, "index");
    let docs_html = find_html(&outputs, "docs");

    // index.html should not reference docs.html anywhere in its
    // page-nav block (docs is two steps away from index in the flat
    // list, with about between them).
    let index_pn = page_nav_block(index_html);
    assert!(
        !index_pn.contains("href=\"docs.html\""),
        "index page-nav must not reference docs (it's not the immediate neighbor); got:\n{}",
        index_pn
    );
    let docs_pn = page_nav_block(docs_html);
    assert!(
        !docs_pn.contains("href=\"index.html\""),
        "docs page-nav must not reference index (it's not the immediate neighbor); got:\n{}",
        docs_pn
    );
}

// === Test 44 ==============================================================

/// Test 44 — a single-doc render with no sidebar produces no page-nav
/// even when `page-navigation: true` is set explicitly.
#[test]
fn pipeline_single_doc_no_page_nav() {
    // We still drive this through the project pipeline (the website
    // story doesn't change for a 1-file directory; what we're
    // exercising is the no-sidebar case).
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             page-navigation: true\n",
        );
        write(
            &project_dir.join("doc.qmd"),
            "---\ntitle: Doc\n---\n\nLone document.\n",
        );
    });

    let doc_html = find_html(&outputs, "doc");
    assert!(
        !doc_html.contains("<nav class=\"page-navigation\">"),
        "no sidebar → no page-nav (default-on requires a sidebar); got:\n{}",
        &doc_html[..doc_html.len().min(1500)]
    );
}
