/*
 * tests/integration/about_page_diagnostics.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-7hzkh4m6: warn (Q-5-57) that `about:` pages are not supported yet.
 */

//! Quarto 1 lays out a website page with `about:` front matter using one
//! of its about templates (jolla, trestles, …). Quarto 2 does not
//! implement them (bd-5xmy5lle), so the page renders as a plain document;
//! these tests pin the warning that says so. Every test drives the real
//! render path: `ProjectPipeline` for websites, `render_to_file` for a
//! standalone document.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult, render_to_file};
use quarto_error_reporting::DiagnosticMessage;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const CODE: &str = "Q-5-57";

const ABOUT_PAGE: &str = "---\ntitle: About\nabout:\n  template: jolla\n  links:\n    \
                          - icon: github\n      text: GitHub\n      href: https://github.com\n\
                          ---\n\nSome text.\n";

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Render a website project (`index.qmd` + `about.qmd`) through the full
/// Pass-1/Pass-2 pipeline and return the per-file results.
fn render_website(about_qmd: &str) -> Vec<RenderToFileResult> {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  title: About test\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHi.\n",
    );
    write(&project_dir.join("about.qmd"), about_qmd);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).expect("discover");
    assert!(!project.is_single_file, "expected a website project");
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
    let summary = pollster::block_on(pipeline.run()).expect("pipeline run");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures"
    );
    summary.outputs
}

fn output<'a>(outputs: &'a [RenderToFileResult], file: &str) -> &'a RenderToFileResult {
    outputs
        .iter()
        .find(|o| o.output_path.file_name().and_then(|f| f.to_str()) == Some(file))
        .unwrap_or_else(|| panic!("no output {file}"))
}

fn with_code(diagnostics: &[DiagnosticMessage]) -> Vec<&DiagnosticMessage> {
    diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some(CODE))
        .collect()
}

/// `(file name, 0-based row)` of a diagnostic's primary location.
fn resolved_location(
    d: &DiagnosticMessage,
    source_context: &quarto_source_map::SourceContext,
) -> Option<(String, usize)> {
    let mapped = d.location.as_ref()?.map_offset(0, source_context)?;
    let file = source_context.get_file(mapped.file_id)?;
    let name = Path::new(&file.path)
        .file_name()?
        .to_string_lossy()
        .into_owned();
    Some((name, mapped.location.row))
}

/// A website page with `about:` warns exactly once, at the `about:` value
/// in its front matter.
#[test]
fn website_about_page_warns_once_at_front_matter() {
    let outputs = render_website(ABOUT_PAGE);
    let about = output(&outputs, "about.html");
    let hits = with_code(&about.render_output.diagnostics);
    assert_eq!(hits.len(), 1, "{:?}", about.render_output.diagnostics);
    let d = hits[0];
    assert_eq!(d.kind, quarto_error_reporting::DiagnosticKind::Warning);
    // Row 3 (0-based) is `template: jolla`, the first line of the `about`
    // map value (row 0 is the opening `---`).
    assert_eq!(
        resolved_location(d, &about.render_output.source_context),
        Some(("about.qmd".to_string(), 3)),
        "{d:?}"
    );
}

/// Pages without `about:` stay quiet.
#[test]
fn website_page_without_about_does_not_warn() {
    let outputs = render_website(ABOUT_PAGE);
    let index = output(&outputs, "index.html");
    assert!(
        with_code(&index.render_output.diagnostics).is_empty(),
        "{:?}",
        index.render_output.diagnostics
    );
}

/// Quarto 1 only builds about pages in website (and book) projects; a
/// standalone document ignores the key there, so q2 does not warn either.
#[test]
fn standalone_document_with_about_does_not_warn() {
    let temp = TempDir::new().unwrap();
    let input = canonical(temp.path()).join("doc.qmd");
    write(&input, ABOUT_PAGE);
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result =
        render_to_file(&input, "html", &RenderToFileOptions::default(), runtime).expect("render");
    assert!(
        with_code(&result.render_output.diagnostics).is_empty(),
        "{:?}",
        result.render_output.diagnostics
    );
}
