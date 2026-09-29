/*
 * tests/integration/book_project_type.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for P1 of the book-projects epic:
 * `BookProjectType` + `run_with_book_support()` end-to-end through
 * `ProjectPipeline`.
 */

//! Each test writes a small fixture to a temp dir and drives it
//! through `ProjectPipeline`, then inspects the rendered output and
//! the project-level orchestration behavior (format gate, output
//! dir, teardown parity, single-file render set).
//!
//! See `claude-notes/plans/2026-09-21-book-projects-P1-foundations.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::{Format, FormatIdentifier};
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult};
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

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

const BOOK_QUARTO_YML: &str = r#"project:
  type: book

book:
  title: "My Book"
  chapters:
    - index.qmd
    - ch1.qmd
"#;

fn write_book_fixture(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), BOOK_QUARTO_YML);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "---\ntitle: First\n---\n\nChapter one.\n",
    );
}

/// Drive a fixture through `ProjectPipeline::run_with_book_support()`.
/// Returns the project dir and summary; panics on render error.
fn render_book(
    fixture: impl FnOnce(&Path),
    format: Format,
    format_str: &str,
) -> (TempDir, PathBuf, ProjectRenderSummary<RenderToFileResult>) {
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
        format,
        format_str,
        &options,
        runtime,
    );
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("run_with_book_support failed: {e}"));
    (temp, project_dir, summary)
}

/// A minimal book project renders like a website: per-chapter HTML under
/// `_book/`, sidebar with chapter-number decoration, no `_site/` (plan
/// milestone: "a book renders exactly like a website, minus numbering").
#[test]
fn book_renders_per_chapter_html_into_book_dir() {
    let (_temp, project_dir, summary) = render_book(write_book_fixture, Format::html(), "html");
    assert_eq!(
        summary.outputs.len(),
        2,
        "both chapters rendered: {summary:?}"
    );
    let index = project_dir.join("_book/index.html");
    let ch1 = project_dir.join("_book/ch1.html");
    assert!(
        index.is_file(),
        "index.html exists under _book/: {}",
        index.display()
    );
    assert!(
        ch1.is_file(),
        "ch1.html exists under _book/: {}",
        ch1.display()
    );
    assert!(
        !project_dir.join("_site").exists(),
        "book output must not land in website's _site/"
    );

    // Sidebar carries the chapter-number decoration the sidebar translation
    // baked in (config.rs's `[N]{.chapter-number}` span).
    let index_html = read(&index);
    assert!(
        index_html.contains("chapter-number"),
        "rendered index page sidebar carries the chapter-number decoration"
    );
}

/// A book project's render output lands under `_book/`, not `_site/`
/// (Q1's `book.ts` `outputDir: "_book"`).
#[test]
fn book_output_dir_is_book_not_site() {
    let (_temp, project_dir, _summary) = render_book(write_book_fixture, Format::html(), "html");
    assert!(project_dir.join("_book").is_dir());
}

/// Book to an unsupported format (docx) errors with Q-5-33 via the
/// general `is_supported_format` mechanism — P3's docx/pptx
/// diagnostic is one instance of this rule, not a special case.
/// Also asserts teardown parity: `shutdown_all()` ran exactly once
/// even on the failure path.
#[test]
fn unsupported_format_errors_q_5_33_and_still_shuts_down_once() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_book_fixture(&project_dir);

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let registry = project.registry.clone();

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let format = Format::from_format_string("docx").unwrap();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        "docx",
        &options,
        runtime,
    );
    let result = pollster::block_on(pipeline.run_with_book_support());
    match result {
        Ok(_) => panic!("book to docx must be rejected"),
        Err(e) => {
            let msg = format!("{e}");
            assert!(
                msg.contains("Q-5-33"),
                "unsupported-format error carries Q-5-33, got: {msg}"
            );
        }
    }
    assert_eq!(
        registry.shutdown_all_call_count(),
        1,
        "shutdown_all runs exactly once even on the book-branch error path"
    );
}

/// The shared `shutdown_all()` teardown runs exactly once on the
/// book branch's success path (P1 regression test for the
/// eighteenth-pass review finding).
#[test]
fn book_render_calls_shutdown_all_exactly_once() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_book_fixture(&project_dir);

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let registry = project.registry.clone();

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime,
    );
    pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("book render failed: {e}"));
    assert_eq!(registry.shutdown_all_call_count(), 1);
}

/// Rendering a minimal non-book project through `run_with_book_support()`
/// (its fallthrough branch) produces byte-identical output to calling `run()`
/// directly — regression guard that adding the method changed nothing for
/// Default/Website projects.
#[test]
fn non_book_fallthrough_is_byte_identical_to_run() {
    let fixture = |dir: &Path| {
        write(&dir.join("_quarto.yml"), "project:\n  type: default\n");
        write(&dir.join("page.qmd"), "# Hello\n\nWorld.\n");
    };

    let render_once = |use_book_support: bool| -> String {
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
            Format::html(),
            "html",
            &options,
            runtime,
        );
        let summary = if use_book_support {
            pollster::block_on(pipeline.run_with_book_support())
        } else {
            pollster::block_on(pipeline.run())
        }
        .unwrap_or_else(|e| panic!("render failed: {e}"));
        assert_eq!(summary.outputs.len(), 1);
        read(&project_dir.join("page.html"))
    };

    let via_run = render_once(false);
    let via_book_support = render_once(true);
    assert_eq!(
        via_run, via_book_support,
        "run_with_book_support's non-book fallthrough must be byte-identical to run()"
    );
}

/// Single-chapter render of a book project renders without error
/// through the ordinary per-document path (preview smoke test):
/// discovering a chapter file of a book project gives a full project
/// context, and the CLI restricts the render to the named file via
/// [`RenderMode::Subset`] — the book's config/sidebar translation
/// must not collide with that path, and the render set stays the one
/// chapter.
#[test]
fn single_chapter_subset_render_keeps_one_file_render_set() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_book_fixture(&project_dir);
    let ch1 = canonical(&project_dir.join("ch1.qmd"));

    let runtime = runtime_arc();
    // Mirror render.rs's single-doc path: discover from the file
    // (finds the book project's _quarto.yml), then render just the
    // named file as a subset.
    let mut project = ProjectContext::discover(&ch1, runtime.as_ref()).unwrap();

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime,
    )
    .with_mode(quarto_core::project::orchestrator::RenderMode::Subset(
        std::collections::HashSet::from([ch1]),
    ));
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("single-chapter book render failed: {e}"));
    assert_eq!(
        summary.outputs.len(),
        1,
        "subset render keeps its one-file render set: {summary:?}"
    );
}

/// Preview smoke test: the exact code path the WASM
/// `render_page_in_project` entry point drives —
/// `ProjectPipeline<RenderToHtmlRenderer>` with
/// `RenderMode::ActivePage(chapter)` — renders a book chapter without
/// error, confirming `BookProjectType`'s config/sidebar shapes don't
/// collide with the preview renderer (which never calls
/// `run_with_book_support`'s book orchestration — it uses the generic
/// pipeline's `run()`; the translated sidebar config still flows
/// through `pre_render`).
#[test]
fn preview_active_page_render_of_book_chapter_succeeds() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_book_fixture(&project_dir);
    let ch1 = canonical(&project_dir.join("ch1.qmd"));

    let runtime = runtime_arc();
    // Mirror the WASM entry point: discover from the active file,
    // then re-discover from the project root when multi-file so
    // Pass 1 profiles cover every sibling.
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    assert!(!project.is_single_file);

    let project_type = project_type_for(&project);
    let vfs_root = project.dir.join(".quarto/project-artifacts");
    let renderer = quarto_core::project::pass2_renderer::RenderToHtmlRenderer::new(&vfs_root)
        .with_url_root("/.quarto/project-artifacts");

    let mut pipeline = ProjectPipeline::with_renderer(
        &mut project,
        project_type,
        Format::html(),
        "html",
        runtime,
        renderer,
    )
    .with_mode(quarto_core::project::orchestrator::RenderMode::ActivePage(
        ch1,
    ));

    let summary = pollster::block_on(pipeline.run()).expect("preview pipeline run");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "no render failures: {:?} / {:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    assert_eq!(summary.outputs.len(), 1);
    let html = match &summary.outputs[0].payload {
        quarto_core::project::pass2_renderer::Pass2Payload::Html(h) => h.clone(),
        other => panic!("expected HTML payload, got {other:?}"),
    };
    assert!(
        html.contains("chapter-number"),
        "previewed chapter carries the decorated sidebar: {html:.500}"
    );
}

/// A `page-footer` region referencing a file the chapter list doesn't
/// name causes that file to render anyway; same for a `404.qmd`
/// present in the project directory (Q1's footer-file walk + `ext404`
/// check — books render only what the render list names, so these
/// must be added explicitly).
#[test]
fn footer_and_404_files_render_even_when_unlisted() {
    let fixture = |dir: &Path| {
        write(
            &dir.join("_quarto.yml"),
            r#"project:
  type: book

book:
  title: "My Book"
  chapters:
    - index.qmd
    - ch1.qmd
  page-footer:
    left:
      - href: extra.qmd
"#,
        );
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        );
        write(
            &dir.join("ch1.qmd"),
            "---\ntitle: First\n---\n\nChapter one.\n",
        );
        write(&dir.join("extra.qmd"), "# Extra\n\nFooter page.\n");
        write(&dir.join("404.qmd"), "# Not found\n\nMissing.\n");
    };
    let (_temp, project_dir, summary) = render_book(fixture, Format::html(), "html");
    assert_eq!(
        summary.outputs.len(),
        4,
        "chapters + footer file + 404 all render: {summary:?}"
    );
    assert!(project_dir.join("_book/extra.html").is_file());
    assert!(project_dir.join("_book/404.html").is_file());
}

/// `is_supported_format` is the general rule: html/typst/epub
/// supported; docx/pptx/revealjs/pdf/gfm rejected.
#[test]
fn supported_format_matrix() {
    use quarto_core::project::book::is_supported_format;
    for (s, expected) in [
        ("html", true),
        ("typst", true),
        ("epub", true),
        ("docx", false),
        ("pptx", false),
        ("pdf", false),
        ("revealjs", false),
        ("gfm", false),
    ] {
        let id = FormatIdentifier::try_from(s).unwrap();
        assert_eq!(
            is_supported_format(&id),
            expected,
            "is_supported_format({s})"
        );
    }
}
