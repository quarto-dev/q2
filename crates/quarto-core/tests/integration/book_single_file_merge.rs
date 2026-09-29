/*
 * tests/integration/book_single_file_merge.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end integration tests for book-projects P2's single-file-merge
 * branch (`run_with_book_support()` → `render_book_single_file`), driven
 * through the real `ProjectPipeline`, a real `pandoc`, and a real
 * `typst` binary — not just the in-process unit tests in
 * `single_file_render.rs`.
 *
 * See `claude-notes/plans/2026-09-21-book-projects-P2-single-file-merge.md`.
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::engine::{EngineRegistry, ExecutionPolicy, FixtureEngine};
use quarto_core::format::Format;
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

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

const BOOK_QUARTO_YML: &str = r#"project:
  type: book

book:
  title: "Merge Test Book"
  author: "Test Author"
  chapters:
    - index.qmd
    - ch1.qmd
    - ch2.qmd
"#;

fn write_two_chapter_fixture(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), BOOK_QUARTO_YML);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\nBody of chapter one.\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nBody of chapter two.\n",
    );
}

fn render_book_to_typst(
    fixture: impl FnOnce(&Path),
) -> (TempDir, PathBuf, ProjectRenderSummary<RenderToFileResult>) {
    render_book_to_typst_with_options(fixture, RenderToFileOptions::default())
}

fn render_book_to_typst_with_options(
    fixture: impl FnOnce(&Path),
    options: RenderToFileOptions,
) -> (TempDir, PathBuf, ProjectRenderSummary<RenderToFileResult>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

    let project_type = project_type_for(&project);
    let format = Format::from_format_string("typst").unwrap();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        "typst",
        &options,
        runtime,
    );
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("run_with_book_support failed: {e}"));
    (temp, project_dir, summary)
}

/// A `fixture-a` registry, mirroring `execution_policy.rs`'s pattern —
/// deterministic "execution" with no real Python/R/Julia engine needed.
fn fixture_engine_registry() -> Arc<EngineRegistry> {
    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(FixtureEngine::with_results(
        "fixture-a",
        vec!["EXECUTED-RESULT-MARKER".to_string()],
    )));
    Arc::new(registry)
}

/// Same two-chapter shape as [`write_two_chapter_fixture`], except chapter
/// one declares `engine: fixture-a` and has one executable cell.
fn write_two_chapter_fixture_with_engine_cell(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), BOOK_QUARTO_YML);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "---\nengine: fixture-a\n---\n\n# Chapter One\n\nBefore.\n\n\
         ```{fixture-a}\nseed-cell-source\n```\n\nAfter.\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nBody of chapter two.\n",
    );
}

/// The primary end-to-end claim of book-projects P2: a multi-chapter
/// Typst book target produces **one** compiled output (the merged
/// book), not one PDF per chapter — the single-file-merge branch, not
/// `run_inner()`'s ordinary per-document Pass 2.
///
/// Invocation this test mirrors: `q2 render <book> --to typst`.
#[test]
fn typst_book_merges_all_chapters_into_one_compiled_pdf() {
    let (_temp, project_dir, summary) = render_book_to_typst(write_two_chapter_fixture);

    assert_eq!(
        summary.outputs.len(),
        1,
        "exactly one merged output, not one per chapter: {summary:?}"
    );
    let output = &summary.outputs[0];
    assert_eq!(
        output.output_path.extension().and_then(|e| e.to_str()),
        Some("pdf"),
        "typst book output must be a compiled PDF: {}",
        output.output_path.display()
    );
    assert!(
        output.output_path.is_file(),
        "compiled PDF must exist on disk: {}",
        output.output_path.display()
    );
    // A real, non-trivial PDF — not a zero-byte placeholder.
    let size = std::fs::metadata(&output.output_path).unwrap().len();
    assert!(
        size > 100,
        "compiled PDF must have real content: {size} bytes"
    );

    // Output filename is the book's own stem (`book_output_stem`), not
    // any individual chapter's — "Merge Test Book" sanitized.
    assert_eq!(
        output.output_path.file_stem().and_then(|s| s.to_str()),
        Some("Merge-Test-Book"),
    );

    // Per-chapter output files must NOT exist — this is the
    // single-file-merge branch, not a per-chapter render.
    assert!(!project_dir.join("_book/ch1.pdf").exists());
    assert!(!project_dir.join("_book/ch2.pdf").exists());
}

/// bd-sl79jjiq / `RenderOutput::execution_skipped`: a book render must
/// report "some chapter's code was excluded" truthfully, not hardcode
/// `false`. Chapter one declares an executable `fixture-a` cell; the
/// render's `ExecutionPolicy::None` excludes every input from execution,
/// so the merged document's `execution_skipped` must come out `true`.
#[test]
fn execution_skipped_is_true_when_a_chapter_engine_cell_is_excluded() {
    let options = RenderToFileOptions {
        engine_registry_override: Some(fixture_engine_registry()),
        execution_policy: ExecutionPolicy::None,
        ..Default::default()
    };
    let (_temp, _project_dir, summary) =
        render_book_to_typst_with_options(write_two_chapter_fixture_with_engine_cell, options);

    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    assert!(
        summary.outputs[0].render_output.execution_skipped,
        "chapter one's fixture-a cell was excluded by ExecutionPolicy::None, \
         so the book-level flag must be true: {:?}",
        summary.outputs[0].render_output
    );
}

/// Negative control for the test above: with the same `ExecutionPolicy::None`
/// but no chapter declaring any engine cell, there is nothing to skip — the
/// flag must not overshoot to "always true" once threaded through the book
/// render.
#[test]
fn execution_skipped_is_false_when_no_chapter_has_an_engine_cell() {
    let options = RenderToFileOptions {
        engine_registry_override: Some(fixture_engine_registry()),
        execution_policy: ExecutionPolicy::None,
        ..Default::default()
    };
    let (_temp, _project_dir, summary) =
        render_book_to_typst_with_options(write_two_chapter_fixture, options);

    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    assert!(
        !summary.outputs[0].render_output.execution_skipped,
        "no chapter had code to execute, so nothing was skipped: {:?}",
        summary.outputs[0].render_output
    );
}

/// book-projects P2 item 81 — *documenting* test, mirroring P6's
/// numeric-citation-style pin: a chapter's front-matter `subtitle`/`date`/
/// `abstract` is silently dropped by the single-file merge (the merged
/// document's meta is seeded from the *first* chapter's meta; every other
/// chapter contributes blocks only). This pins the current
/// (`bd-ock6cyiy`-tracked) behavior explicitly, so a future change to it —
/// fix or regression — is a deliberate, visible diff rather than silent
/// drift either way.
///
/// Empirically probed (real render, kept `.typ` inspected): the first
/// chapter's own `subtitle`/`date` DO survive into orange-book's
/// `book.with(subtitle: …, date: …)` call — the positive control proving
/// the drop is chapter-meta-specific, not template incapability. The
/// `abstract` case is pinned only for the non-first chapter: orange-book's
/// `typst-show.typ` has no `abstract` slot at all, so even a surviving
/// abstract would never reach the compiled output.
#[test]
fn chapter_front_matter_subtitle_date_abstract_are_silently_dropped() {
    fn fixture(project_dir: &Path) {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: book\n\nkeep-typ: true\n\nbook:\n  title: \"Chapter Meta Book\"\n  author: \"Test Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\nsubtitle: Index Subtitle\ndate: 2020-01-01\nabstract: Index abstract text.\n---\n\nWelcome to the book.\n",
        );
        write(
            &project_dir.join("ch1.qmd"),
            "---\ntitle: Chapter One\nsubtitle: Chapter One Subtitle\ndate: 2021-02-02\nabstract: Chapter one abstract text.\n---\n\nBody one.\n",
        );
        write(&project_dir.join("ch2.qmd"), "# Chapter Two\n\nBody two.\n");
    }

    let (_temp, _project_dir, summary) =
        render_book_to_typst_with_options(fixture, RenderToFileOptions::default());

    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let output_path = &summary.outputs[0].output_path;
    let typ_path = output_path.with_extension("typ");
    let typ_text = std::fs::read_to_string(&typ_path)
        .unwrap_or_else(|e| panic!("expected kept intermediate {}: {e}", typ_path.display()));

    // Positive control: the FIRST chapter's meta seeds the merged
    // document, so its subtitle/date reach orange-book's book.with call.
    assert!(
        typ_text.contains("subtitle: [Index Subtitle]"),
        "first chapter's subtitle must survive into the merged meta (it seeds it): {typ_text}"
    );
    assert!(
        typ_text.contains("date: \"2020-01-01\""),
        "first chapter's date must survive into the merged meta: {typ_text}"
    );

    // The pinned silent drop: chapter one's front matter never reaches
    // the merged document at all.
    assert!(
        !typ_text.contains("Chapter One Subtitle"),
        "non-first chapter's subtitle is silently dropped by the merge (bd-ock6cyiy): {typ_text}"
    );
    assert!(
        !typ_text.contains("2021-02-02"),
        "non-first chapter's date is silently dropped by the merge (bd-ock6cyiy): {typ_text}"
    );
    assert!(
        !typ_text.contains("Chapter one abstract text"),
        "non-first chapter's abstract is silently dropped by the merge (bd-ock6cyiy): {typ_text}"
    );
}
