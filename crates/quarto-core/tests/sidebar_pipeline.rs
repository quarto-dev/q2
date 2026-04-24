/*
 * tests/sidebar_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for Phase 2 of the website-projects epic: the
 * sidebar feature end-to-end through `ProjectPipeline`.
 */

//! End-to-end integration tests for sidebars.
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-2.md` §Tests
//! 36–39a. Each test writes a small fixture to a temp dir, drives it
//! through `ProjectPipeline`, then inspects the rendered HTML.
//!
//! The goal is to catch wiring bugs that unit tests miss — e.g. the
//! Generate/Render split plays badly with metadata merging, the
//! template slot fails to pick up the rendered HTML, cross-document
//! hrefs resolve to the wrong `output_href`. Every test runs the
//! real pipeline (not a handcrafted `RenderContext`).

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
    assert!(
        !project.is_single_file,
        "test expected a multi-file project"
    );

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

    // Keep the temp dir alive by leaking it — we need the files for
    // inspection after render_project returns. (The temp dir cleans up
    // when the process exits.) This is a deliberate leak for test
    // isolation.
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

// === Test 36 ==============================================================

/// Test 36 — two-page website with a manual sidebar. Both pages
/// render with the sidebar; the current page's link carries `active`.
#[test]
fn pipeline_renders_sidebar_for_two_page_website() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout us.\n",
        );
    });

    let index_html = find_html(&outputs, "index");
    let about_html = find_html(&outputs, "about");

    // Both pages carry a sidebar nav element.
    assert!(
        index_html.contains("<nav id=\"quarto-sidebar\""),
        "index.html missing sidebar; got first 800 chars: {}",
        &index_html[..index_html.len().min(800)]
    );
    assert!(about_html.contains("<nav id=\"quarto-sidebar\""));

    // Source hrefs got rewritten.
    assert!(index_html.contains("href=\"about.html\""));
    assert!(about_html.contains("href=\"index.html\""));
    assert!(!index_html.contains("href=\"about.qmd\""));
    assert!(!about_html.contains("href=\"index.qmd\""));

    // Active highlighting: on index.html, the index link is active
    // and the about link is not; vice-versa on about.html.
    assert!(
        index_html.contains("href=\"index.html\" class=\"sidebar-item-text sidebar-link active\"")
            || index_html
                .contains("href=\"/index.html\" class=\"sidebar-item-text sidebar-link active\""),
        "index page's own link should be active"
    );
    assert!(
        !index_html.contains("href=\"about.html\" class=\"sidebar-item-text sidebar-link active\"")
    );
    assert!(
        about_html.contains("href=\"about.html\" class=\"sidebar-item-text sidebar-link active\"")
    );
    assert!(
        !about_html.contains("href=\"index.html\" class=\"sidebar-item-text sidebar-link active\"")
    );
}

// === Test 37 ==============================================================

/// Test 37 — `auto: true` expands to the full set of non-index pages,
/// visible in every rendered page's sidebar.
#[test]
fn pipeline_auto_sidebar_lists_all_pages() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  sidebar:\n    contents:\n      - auto: true\n",
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
            &project_dir.join("help.qmd"),
            "---\ntitle: Help\n---\n\nDocs.\n",
        );
    });

    let index_html = find_html(&outputs, "index");

    // index.qmd excluded as top-level index.
    assert!(!index_html.contains("href=\"index.html\" class=\"sidebar-item-text sidebar-link\""));

    // about and help both rendered in the sidebar (non-active here
    // since we're on index.html).
    assert!(index_html.contains("href=\"about.html\""));
    assert!(index_html.contains("href=\"help.html\""));

    // Check the same on about.html (same sidebar).
    let about_html = find_html(&outputs, "about");
    assert!(about_html.contains("href=\"help.html\""));
    // about itself is active on about.html.
    assert!(
        about_html.contains("href=\"about.html\" class=\"sidebar-item-text sidebar-link active\"")
    );
}

// === Test 38 ==============================================================

/// Test 38 — two sidebars, pages partitioned by containment.
#[test]
fn pipeline_multiple_sidebars_select_by_containment() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  sidebar:\n    - id: main\n      contents:\n        - a.qmd\n        - b.qmd\n\
             \n    - id: reference\n      contents:\n        - c.qmd\n        - d.qmd\n",
        );
        write(&project_dir.join("a.qmd"), "---\ntitle: A\n---\n\nA.\n");
        write(&project_dir.join("b.qmd"), "---\ntitle: B\n---\n\nB.\n");
        write(&project_dir.join("c.qmd"), "---\ntitle: C\n---\n\nC.\n");
        write(&project_dir.join("d.qmd"), "---\ntitle: D\n---\n\nD.\n");
    });

    let a_html = find_html(&outputs, "a");
    let c_html = find_html(&outputs, "c");

    // a.qmd is in the "main" sidebar → its sidebar has A and B but
    // NOT C or D.
    assert!(a_html.contains("href=\"a.html\""));
    assert!(a_html.contains("href=\"b.html\""));
    assert!(
        !a_html.contains("href=\"c.html\""),
        "a.html's sidebar should not include c (different sidebar)"
    );

    // c.qmd is in "reference" → its sidebar has C and D but not A/B.
    assert!(c_html.contains("href=\"c.html\""));
    assert!(c_html.contains("href=\"d.html\""));
    assert!(
        !c_html.contains("href=\"a.html\""),
        "c.html's sidebar should not include a (different sidebar)"
    );
}

// === Test 39 ==============================================================

/// Test 39 — cross-page sidebar links resolve to `.html` in the
/// rendered HTML (confirms Render-step rewrite runs inside the
/// full pipeline, not just in unit tests).
#[test]
fn pipeline_cross_page_links_are_written_as_html() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  sidebar:\n    contents:\n      - about.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\n[Go](about.qmd)\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout.\n",
        );
    });

    let index_html = find_html(&outputs, "index");
    // The sidebar entry for about.qmd must render as about.html.
    assert!(index_html.contains("href=\"about.html\""));
    // The raw source path must never leak into the rendered sidebar.
    // (Finding the sidebar fragment and checking there specifically.)
    let sidebar_start = index_html.find("<nav id=\"quarto-sidebar\"").unwrap();
    let sidebar_end = index_html[sidebar_start..].find("</nav>").unwrap() + sidebar_start;
    let sidebar_fragment = &index_html[sidebar_start..sidebar_end];
    assert!(
        !sidebar_fragment.contains(".qmd"),
        "sidebar fragment should not contain .qmd; got: {}",
        sidebar_fragment
    );
}

// === Test 39a =============================================================

/// Test 39a — navigation.sidebar (the intermediate structured form)
/// preserves .qmd paths between Generate and Render. This tests the
/// format-agnostic invariant indirectly: if Generate leaked .html
/// into navigation.sidebar, changing the Render step's rewrite would
/// be a no-op, masking bugs.
///
/// The direct way to assert this is hard from an integration test
/// (we don't have an easy post-Generate hook). Instead we verify a
/// closely related claim: if we set up a sidebar entry that Render
/// can't resolve (no matching profile), the raw .qmd stays in the
/// output as a dangling link — which can only happen if the entry
/// carried `.qmd` at the Render boundary.
#[test]
fn pipeline_unresolved_sidebar_entry_keeps_raw_qmd() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  sidebar:\n    contents:\n      - index.qmd\n      - ghost.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        );
        // `ghost.qmd` is referenced in the sidebar but doesn't exist
        // as a project file, so no profile is built for it.
    });

    let index_html = find_html(&outputs, "index");
    // Known file was rewritten.
    assert!(index_html.contains("href=\"index.html\""));
    // Ghost reference survives untouched as a dangling link.
    assert!(
        index_html.contains("href=\"ghost.qmd\""),
        "unresolved .qmd link should pass through verbatim; got sidebar region: {}",
        index_html
            .find("<nav id=\"quarto-sidebar\"")
            .map(|i| &index_html[i..(i + 400).min(index_html.len())])
            .unwrap_or("<no sidebar found>")
    );
}
