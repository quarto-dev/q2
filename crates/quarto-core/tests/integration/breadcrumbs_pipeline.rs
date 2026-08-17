/*
 * tests/breadcrumbs_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for website breadcrumbs
 * (bd-breadcrumbs-missing-1vpuqh34): the title-block
 * `.quarto-title-breadcrumbs` trail, end-to-end through
 * `ProjectPipeline`.
 */

//! Every test writes a small website fixture to a temp dir, drives it
//! through the real `ProjectPipeline`, then inspects the rendered
//! HTML — same harness shape as `sidebar_pipeline.rs`.

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

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Build and render a website project fixture. Returns the map of
/// `project-relative output href → rendered HTML` (href, not stem —
/// breadcrumb fixtures have several `index.html` at different depths).
fn render_project(fixture: impl FnOnce(&std::path::Path)) -> Vec<(String, String)> {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
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
        Format::html(),
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
    std::mem::forget(temp); // keep files alive for inspection

    let site_root = project_dir.join("_site");
    summary
        .outputs
        .iter()
        .map(|out| {
            let href = out
                .output_path
                .strip_prefix(&site_root)
                .unwrap_or(&out.output_path)
                .to_string_lossy()
                .replace('\\', "/");
            (href, read(&out.output_path))
        })
        .collect()
}

fn find_html<'a>(outputs: &'a [(String, String)], href: &str) -> &'a str {
    &outputs
        .iter()
        .find(|(h, _)| h == href)
        .unwrap_or_else(|| {
            panic!(
                "no output for href '{}'; got: {:?}",
                href,
                outputs.iter().map(|(h, _)| h).collect::<Vec<_>>()
            )
        })
        .1
}

/// The standard fixture: a two-level nested sidebar,
/// `guide/advanced/deep.qmd` sitting inside `Guide > Advanced`.
fn nested_fixture(project_dir: &std::path::Path, quarto_yml_extra: &str) {
    write(
        &project_dir.join("_quarto.yml"),
        &format!(
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: Site\n{}  sidebar:\n    contents:\n      - index.qmd\n      - section: Guide\n        contents:\n          - guide/intro.qmd\n          - section: Advanced\n            contents:\n              - guide/advanced/deep.qmd\n",
            quarto_yml_extra
        ),
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome.\n",
    );
    write(
        &project_dir.join("guide/intro.qmd"),
        "---\ntitle: Intro\n---\n\nIntro.\n",
    );
    write(
        &project_dir.join("guide/advanced/deep.qmd"),
        "---\ntitle: Deep Page\n---\n\nDeep content.\n",
    );
}

/// A deep page renders the title-block breadcrumb trail: sections
/// (borrowing their first child's href, page-relativized) then the
/// current page as the final linked crumb.
#[test]
fn breadcrumbs_render_on_deep_page_with_resolved_hrefs() {
    let outputs = render_project(|dir| nested_fixture(dir, ""));
    let html = find_html(&outputs, "guide/advanced/deep.html");

    assert!(
        html.contains("quarto-title-breadcrumbs"),
        "deep page should carry title-block breadcrumbs; got:\n{}",
        html
    );
    assert!(
        html.contains("aria-label=\"breadcrumb\""),
        "breadcrumb nav needs the aria label"
    );
    // Section crumb "Guide" borrows guide/intro.qmd → ../intro.html
    // relative to guide/advanced/deep.html.
    assert!(
        html.contains("<li class=\"breadcrumb-item\"><a href=\"../intro.html\">Guide</a></li>"),
        "section crumb should borrow first child's href, page-relative; got:\n{}",
        breadcrumb_region(html)
    );
    // Section crumb "Advanced" borrows deep.qmd → deep.html (sibling).
    assert!(
        html.contains("<li class=\"breadcrumb-item\"><a href=\"deep.html\">Advanced</a></li>"),
        "inner section crumb borrows its first child; got:\n{}",
        breadcrumb_region(html)
    );
    // The current page is the final, linked crumb (Q1 parity).
    assert!(
        html.contains("<li class=\"breadcrumb-item\"><a href=\"deep.html\">Deep Page</a></li>"),
        "current page should be the final linked crumb; got:\n{}",
        breadcrumb_region(html)
    );
}

fn breadcrumb_region(html: &str) -> &str {
    html.split("quarto-page-breadcrumbs").nth(1).unwrap_or(html)
}

/// `bread-crumbs: false` at site level suppresses the trail.
#[test]
fn breadcrumbs_site_config_false_suppresses() {
    let outputs = render_project(|dir| nested_fixture(dir, "  bread-crumbs: false\n"));
    let html = find_html(&outputs, "guide/advanced/deep.html");
    assert!(
        !html.contains("quarto-page-breadcrumbs"),
        "bread-crumbs: false must suppress the trail"
    );
}

/// Page-level `bread-crumbs: false` suppresses just that page.
#[test]
fn breadcrumbs_page_level_false_suppresses() {
    let outputs = render_project(|dir| {
        nested_fixture(dir, "");
        write(
            &dir.join("guide/advanced/deep.qmd"),
            "---\ntitle: Deep Page\nbread-crumbs: false\n---\n\nDeep content.\n",
        );
    });
    let html = find_html(&outputs, "guide/advanced/deep.html");
    assert!(
        !html.contains("quarto-page-breadcrumbs"),
        "page-level bread-crumbs: false must suppress the trail"
    );
    // Other pages keep theirs.
    let intro = find_html(&outputs, "guide/intro.html");
    assert!(
        intro.contains("quarto-page-breadcrumbs"),
        "other pages keep their trail"
    );
}

/// A top-level page (trail length 1 — just itself) gets no
/// title-block trail (Q1 renders that instance only when the trail
/// has more than one crumb).
#[test]
fn breadcrumbs_absent_on_top_level_page() {
    let outputs = render_project(|dir| nested_fixture(dir, ""));
    let html = find_html(&outputs, "index.html");
    assert!(
        !html.contains("quarto-page-breadcrumbs"),
        "length-1 trail must not render"
    );
}

/// No sidebar → no breadcrumbs (nothing to derive a trail from).
#[test]
fn breadcrumbs_absent_without_sidebar() {
    let outputs = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: Site\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(&dir.join("about.qmd"), "---\ntitle: About\n---\n\nHi.\n");
    });
    for (_, html) in &outputs {
        assert!(
            !html.contains("quarto-page-breadcrumbs"),
            "no sidebar, no breadcrumbs"
        );
    }
}

/// A section with its own explicit href links the crumb to that href
/// (no borrowing).
#[test]
fn breadcrumbs_section_with_own_href_links_it() {
    let outputs = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: Site\n  sidebar:\n    contents:\n      - index.qmd\n      - section: Guide\n        href: guide/intro.qmd\n        contents:\n          - guide/deep.qmd\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
        write(
            &dir.join("guide/intro.qmd"),
            "---\ntitle: Intro\n---\n\nIntro.\n",
        );
        write(
            &dir.join("guide/deep.qmd"),
            "---\ntitle: Deep\n---\n\nDeep.\n",
        );
    });
    let html = find_html(&outputs, "guide/deep.html");
    assert!(
        html.contains("<li class=\"breadcrumb-item\"><a href=\"intro.html\">Guide</a></li>"),
        "section crumb uses its own href when present; got:\n{}",
        breadcrumb_region(html)
    );
}
