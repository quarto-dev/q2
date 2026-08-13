/*
 * tests/navbar_footer_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for Phase 3 of the website-projects epic: navbar
 * and page-footer end-to-end through `ProjectPipeline`.
 */

//! End-to-end integration tests for navbar + page-footer project
//! integration.
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-3.md` §Tests
//! 45–50. Each test writes a fixture to a temp dir, drives it through
//! `ProjectPipeline`, then inspects the rendered HTML — the same
//! shape as `sidebar_pipeline.rs`.
//!
//! Phase 3 keeps the YAML surface at the top level (navbar / page-
//! footer — see Decision 1), so these fixtures set `navbar:` /
//! `page-footer:` in `_quarto.yml` without a `website:` wrapper.

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

// === Test 45 ==============================================================

/// Test 45 — two-page website with a navbar. Both pages render a
/// `<nav class="navbar ...">`; the current page's nav-link carries
/// `active`.
#[test]
fn pipeline_renders_navbar_for_two_page_website() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             navbar:\n  title: Site\n  left:\n    - index.qmd\n    - about.qmd\n",
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

    // Both pages carry a navbar.
    assert!(
        index_html.contains("<nav class=\"navbar"),
        "index.html missing navbar; got first 800 chars: {}",
        &index_html[..index_html.len().min(800)]
    );
    assert!(about_html.contains("<nav class=\"navbar"));

    // Hrefs rewritten to .html.
    assert!(index_html.contains("href=\"index.html\""));
    assert!(index_html.contains("href=\"about.html\""));
    assert!(!index_html.contains("href=\"index.qmd\""));
    assert!(!index_html.contains("href=\"about.qmd\""));

    // Active-item highlighting per-page.
    assert!(
        index_html.contains("href=\"index.html\" class=\"nav-link active\""),
        "index's own link should be active on index.html; got: {}",
        &index_html[..index_html.len().min(2000)]
    );
    assert!(
        !index_html.contains("href=\"about.html\" class=\"nav-link active\""),
        "about link should not be active on index.html"
    );
    assert!(
        about_html.contains("href=\"about.html\" class=\"nav-link active\""),
        "about's own link should be active on about.html"
    );
    assert!(
        !about_html.contains("href=\"index.html\" class=\"nav-link active\""),
        "index link should not be active on about.html"
    );
}

// === Test 46 ==============================================================

/// Test 46 — navbar dropdown with a `.qmd` entry gets rewritten and
/// appears as a `dropdown-item` in the rendered HTML.
#[test]
fn pipeline_navbar_dropdown_href_rewriting() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             navbar:\n  left:\n    - text: Docs\n      menu:\n        - guide.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        );
        write(
            &project_dir.join("guide.qmd"),
            "---\ntitle: Guide\n---\n\nThe guide.\n",
        );
    });

    for stem in ["index", "guide"] {
        let html = find_html(&outputs, stem);
        assert!(
            html.contains("<li class=\"nav-item dropdown\">"),
            "{}.html missing dropdown; got first 1200 chars: {}",
            stem,
            &html[..html.len().min(1200)]
        );
        assert!(
            html.contains("class=\"dropdown-item"),
            "{}.html missing dropdown-item class",
            stem
        );
        assert!(
            html.contains("href=\"guide.html\""),
            "{}.html missing rewritten href",
            stem
        );
        assert!(
            !html.contains("href=\"guide.qmd\""),
            "{}.html should not have .qmd href",
            stem
        );
    }

    // On guide.html, the dropdown leaf should be active.
    let guide_html = find_html(&outputs, "guide");
    assert!(
        guide_html.contains("class=\"dropdown-item active\""),
        "guide.html should have the dropdown leaf marked active"
    );
}

// === Test 47 ==============================================================

/// Test 47 — page-footer renders, and footer items rewrite their
/// `.qmd` hrefs via ProjectIndex.
#[test]
fn pipeline_renders_page_footer_for_two_page_website() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             page-footer:\n  left: \"\\u00a9 2026\"\n  right:\n    - about.qmd\n",
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

    for stem in ["index", "about"] {
        let html = find_html(&outputs, stem);
        assert!(
            html.contains("<footer class=\"footer"),
            "{}.html missing page-footer; got first 800 chars: {}",
            stem,
            &html[..html.len().min(800)]
        );
        assert!(
            html.contains("© 2026"),
            "{}.html missing copyright text",
            stem
        );
        // Right region's about.qmd → about.html.
        assert!(
            html.contains("href=\"about.html\""),
            "{}.html missing rewritten footer href",
            stem
        );
        assert!(
            !html.contains("href=\"about.qmd\""),
            "{}.html should not have .qmd href",
            stem
        );
    }
}

// === Test 48 ==============================================================

/// Test 48 — active-state never cross-contaminates between pages.
/// Rendering `index.qmd` does NOT mark `about.qmd`'s link active in
/// `index.html` (regression guard against transforms sharing state).
#[test]
fn pipeline_navbar_active_never_cross_contaminates() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             navbar:\n  left:\n    - index.qmd\n    - about.qmd\n",
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
    // Only one anchor carries `nav-link active` on index.html.
    let active_count = index_html.matches("class=\"nav-link active\"").count();
    assert_eq!(
        active_count, 1,
        "expected exactly one active nav-link on index.html; got: {}",
        active_count
    );
}

// === Test 49 ==============================================================

/// Test 49 — format-agnostic invariant check. Body-level spot-check:
/// if the nav stored `.qmd` hrefs through Generate, Render rewrote
/// them cleanly without stripping the `active` class. Proves the
/// Generate/Render split stays coherent end-to-end through the full
/// pipeline.
#[test]
fn pipeline_navigation_subtree_is_format_agnostic() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             navbar:\n  left:\n    - index.qmd\n    - about.qmd\n",
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

    // Rendered HTML carries .html hrefs (Render rewrote) AND the
    // active class (Generate's active marking survived through the
    // ConfigValue roundtrip and into Render).
    let about_html = find_html(&outputs, "about");
    assert!(about_html.contains("href=\"about.html\""));
    assert!(about_html.contains("class=\"nav-link active\""));
    // Sanity: no raw .qmd hrefs leaked into the rendered HTML.
    assert!(
        !about_html.contains("href=\"about.qmd\""),
        ".qmd href should not appear in the rendered HTML"
    );
}

// === Test 50 ==============================================================

/// Test 50 — single-file project with a top-level `navbar:` still
/// works. Regression guard for the UX story Phase 3 is built around:
/// a single-doc render that uses top-level `navbar:` must not
/// regress. We set up a project with just one file so discovery
/// treats it as multi-file (with one file), but the doc uses the
/// same YAML shape a standalone revealjs deck would.
#[test]
fn pipeline_single_doc_navbar_works_with_top_level_config() {
    let (_dir, outputs) = render_project(|project_dir| {
        // Two files so `ProjectContext::discover` yields a multi-file
        // project and we go through the project pipeline end-to-end.
        // The navbar itself only references one of them, to simulate
        // the "navbar in a project but targeting only one page" case.
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\nnavbar:\n  left:\n    - index.qmd\n---\n\nWelcome.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout us.\n",
        );
    });

    let index_html = find_html(&outputs, "index");
    // Navbar rendered from document-level frontmatter.
    assert!(
        index_html.contains("<nav class=\"navbar"),
        "doc-level navbar should still render; got first 1200 chars: {}",
        &index_html[..index_html.len().min(1200)]
    );
    // The self-link is rewritten AND active (because index.qmd is
    // the rendering page and the project index is available).
    assert!(index_html.contains("href=\"index.html\""));
    assert!(index_html.contains("class=\"nav-link active\""));

    // about.html has no navbar (doc-level frontmatter doesn't spill).
    let about_html = find_html(&outputs, "about");
    assert!(
        !about_html.contains("<nav class=\"navbar"),
        "about.html should not inherit index.html's doc-level navbar"
    );
}

// === Case A (bd-root-relative-paths-design-fc5pvkcv): navbar logo =========

/// The navbar logo is a config-declared static asset shared by pages
/// at every depth, so it must be emitted page-relative per page —
/// `images/logo.svg` on the root page, `../../images/logo.svg` two
/// levels down — and the file must be copied into the output tree
/// (decision 5) without any `project.resources` declaration.
#[test]
fn pipeline_navbar_logo_rebased_per_page_and_copied() {
    let (project_dir, _outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  navbar:\n    title: Site\n    logo: images/logo.svg\n    left:\n      - index.qmd\n",
        );
        write(
            &project_dir.join("images/logo.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("deep/deeper/page.qmd"),
            "---\ntitle: Deep\n---\n\nD.\n",
        );
    });

    let root_html = read(&project_dir.join("_site/index.html"));
    assert!(
        root_html.contains("<img src=\"images/logo.svg\""),
        "root page logo should be depth-0 relative; got: {}",
        root_html
            .lines()
            .find(|l| l.contains("navbar-logo"))
            .unwrap_or("<no navbar-logo line>")
    );

    let deep_html = read(&project_dir.join("_site/deep/deeper/page.html"));
    assert!(
        deep_html.contains("<img src=\"../../images/logo.svg\""),
        "depth-2 page logo must climb to the site root; got: {}",
        deep_html
            .lines()
            .find(|l| l.contains("navbar-logo"))
            .unwrap_or("<no navbar-logo line>")
    );

    assert!(
        project_dir.join("_site/images/logo.svg").exists(),
        "logo file must be copied to the output tree (decision 5)"
    );
}

/// A leading `/` on the logo path means site-root-relative
/// (decision 4) and produces identical output to the bare form.
#[test]
fn pipeline_navbar_root_slash_logo_rebased_per_page() {
    let (project_dir, _outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  navbar:\n    title: Site\n    logo: /images/logo.svg\n    left:\n      - index.qmd\n",
        );
        write(
            &project_dir.join("images/logo.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("deep/deeper/page.qmd"),
            "---\ntitle: Deep\n---\n\nD.\n",
        );
    });

    let deep_html = read(&project_dir.join("_site/deep/deeper/page.html"));
    assert!(
        deep_html.contains("<img src=\"../../images/logo.svg\""),
        "leading-/ logo must rebase identically to the bare form; got: {}",
        deep_html
            .lines()
            .find(|l| l.contains("navbar-logo"))
            .unwrap_or("<no navbar-logo line>")
    );
    assert!(
        project_dir.join("_site/images/logo.svg").exists(),
        "leading-/ logo file must be copied to the output tree"
    );
}
