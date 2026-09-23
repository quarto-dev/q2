/*
 * tests/integration/website_post_render_format_gate.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for P7-foundation Task 2: the project-mode
 * containment gate (design doc §13, bd-bgeet2mw).
 */

//! `WebsiteProjectType::post_render`'s hook sequence (sitemap,
//! robots.txt, alias redirects) assumes HTML outputs exist. A website
//! project rendered to a Pandoc target (docx, pptx, ...) has none, so
//! the gate at `orchestrator.rs`'s `post_render` call site must skip
//! the whole sequence when `format.identifier.is_html_based()` is
//! false — matching Q1's own `websiteProjectType.postRender`, which
//! filters `outputFiles` to HTML-only before running this logic.
//!
//! These tests construct a `Format` directly and drive the
//! orchestrator in-process, so — per the plan's Task 2 prerequisite
//! note — they do not need P7-foundation's Task 3 (the `render.rs`
//! CLI gate relaxation) to be exercisable: `Format::docx()` already
//! routes through `render_qmd_to_pandoc` today.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::error::QuartoError;
use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Drive a fixture through `ProjectPipeline` with the given format,
/// returning the project directory and the pipeline's `Result`.
/// Mirrors `website_aliases.rs`'s `try_render_project`, parameterized
/// over the target format.
fn try_render_project_as(
    fixture: impl FnOnce(&Path),
    format: Format,
) -> (PathBuf, Result<ProjectRenderSummary, QuartoError>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let format_str = format.identifier.as_str().to_string();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        &format_str,
        &options,
        runtime.clone(),
    );
    let result = pollster::block_on(pipeline.run());

    // Leak the temp dir so the test can inspect files afterwards
    // (cleanup happens at process exit).
    std::mem::forget(temp);
    (project_dir, result)
}

/// [`try_render_project_as`] plus the assertion that every page
/// rendered and the pipeline itself returned `Ok`.
fn render_project_as(
    fixture: impl FnOnce(&Path),
    format: Format,
) -> (PathBuf, ProjectRenderSummary) {
    let (project_dir, result) = try_render_project_as(fixture, format);
    let summary = result.expect("pipeline should succeed");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    (project_dir, summary)
}

/// A minimal website project with `site-url` set (so `sitemap.xml` /
/// `robots.txt` would fire if the gate did not stop them) and one
/// page carrying an `aliases:` entry (so `write_alias_redirects`
/// would fire too).
const WEBSITE_YML_WITH_SITE_URL: &str = "project:\n  type: website\n  output-dir: _site\n\
     website:\n  site-url: \"https://example.com\"\n";

fn write_minimal_website(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), WEBSITE_YML_WITH_SITE_URL);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\naliases:\n  - /old-name.html\n---\n\nHome body.\n",
    );
}

/// Assert every output in `summary.outputs` has the given extension
/// and is a non-empty file on disk — the positive half the vacuity
/// check requires: an absence assertion alone (no sitemap.xml) is
/// also satisfied by a render that merely panicked before writing
/// anything.
fn assert_all_outputs_nonempty(summary: &ProjectRenderSummary, extension: &str) {
    assert!(!summary.outputs.is_empty(), "expected at least one output");
    for output in &summary.outputs {
        assert_eq!(
            output.output_path.extension().and_then(|e| e.to_str()),
            Some(extension),
            "unexpected output extension: {}",
            output.output_path.display()
        );
        let len = std::fs::metadata(&output.output_path)
            .unwrap_or_else(|e| panic!("read metadata for {}: {}", output.output_path.display(), e))
            .len();
        assert!(
            len > 0,
            "output must be non-empty: {}",
            output.output_path.display()
        );
    }
}

/// T2.1 (real pandoc): a docx render of a website project writes no
/// `sitemap.xml`, no `robots.txt`, and no alias stub — paired with
/// the positive assertion that the render actually produced a
/// non-empty docx (the vacuity check: a docx render that merely
/// panicked in the pipeline would also leave those files absent).
#[test]
fn docx_project_writes_no_sitemap_robots_or_alias_stub() {
    let (project_dir, summary) = render_project_as(write_minimal_website, Format::docx());
    assert_all_outputs_nonempty(&summary, "docx");

    assert!(!project_dir.join("_site/sitemap.xml").exists());
    assert!(!project_dir.join("_site/robots.txt").exists());
    assert!(!project_dir.join("_site/old-name.html").exists());
}

/// T2.2 ("the path was actually exercised" row): the same project
/// rendered to html still writes all three. Without this row, a gate
/// written as `if false` (skipping the hook sequence unconditionally)
/// would also pass T2.1.
#[test]
fn html_project_still_writes_sitemap_robots_and_alias_stub() {
    let (project_dir, summary) = render_project_as(write_minimal_website, Format::html());
    assert_all_outputs_nonempty(&summary, "html");

    assert!(project_dir.join("_site/sitemap.xml").exists());
    assert!(project_dir.join("_site/robots.txt").exists());
    assert!(project_dir.join("_site/old-name.html").exists());
}

/// T2.3 (real pandoc, alias-collision path): an alias collision that
/// would hard-fail an html render (`write_alias_redirects`, Q-5-24)
/// does not fail a docx render, because the whole hook sequence —
/// collision check included — is skipped for a non-HTML target.
#[test]
fn docx_project_survives_alias_collision() {
    let (_project_dir, summary) = render_project_as(
        |dir| {
            write(&dir.join("_quarto.yml"), WEBSITE_YML_WITH_SITE_URL);
            write(
                &dir.join("one/index.qmd"),
                "---\ntitle: One\naliases:\n  - /shared.html\n---\n\n1.\n",
            );
            write(
                &dir.join("two/index.qmd"),
                "---\ntitle: Two\naliases:\n  - /shared.html\n---\n\n2.\n",
            );
        },
        Format::docx(),
    );
    assert_all_outputs_nonempty(&summary, "docx");
}
