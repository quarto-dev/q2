/*
 * tests/integration/book_theorem_crossref.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P2 item 77: a chapter references an *earlier* chapter's
 * labeled theorem after merge+compile — the reference must resolve to
 * the theorem's actual number, not an unresolved or wrong one.
 * `orange-book`'s own test suite guards this exact case with "Critical"
 * severity (a resolution-timing bug class in the Typst theorem package
 * it uses — `theorion`); Q2 inherits this risk by construction. See
 * `claude-notes/plans/2026-09-21-book-projects-P2-single-file-merge.md`.
 */

//! Renders a real 2-chapter Typst book through the vendored `orange-book`
//! extension: chapter one declares a labeled theorem, chapter two both
//! references it (`@thm-ch1`) *and* declares its own theorem. Compiled
//! and inspected as real PDF text (`pdf-extract`), not asserted from
//! reading the Lua/Typst theorem package.
//!
//! **Empirically observed** (not assumed): under orange-book's real
//! `typst-show.typ` (staged via `template-partials` — the PandocWriteStage
//! fix this plan added; earlier revisions of this test ran against the
//! vendored *default* template because the partials were never staged),
//! theorem numbers are chapter-prefixed ("Theorem 1.1" in chapter one,
//! "Theorem 2.1" in chapter two) — the theorem counter itself is
//! document-wide, and the chapter prefix is presentation. The
//! cross-chapter reference in chapter two must therefore read "Theorem
//! 1.1" (the earlier theorem's real number), not "Theorem 2.1" (a
//! same-chapter/most-recent-theorem confusion) and not an unresolved
//! placeholder — the exact resolution-timing failure mode `orange-book`
//! itself guards against.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
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

const BOOK_QUARTO_YML: &str = r#"project:
  type: book

book:
  title: "Theorem Crossref Book"
  author: "Test Author"
  chapters:
    - index.qmd
    - ch1.qmd
    - ch2.qmd
"#;

fn write_fixture(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), BOOK_QUARTO_YML);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\n::: {#thm-ch1 .theorem name=\"First Theorem\"}\nThis is the first theorem.\n:::\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nSee @thm-ch1 for the earlier result.\n\n::: {#thm-ch2 .theorem name=\"Second Theorem\"}\nThis is the second theorem.\n:::\n",
    );
}

#[test]
fn cross_chapter_theorem_reference_resolves_to_original_number() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

    let project_type = project_type_for(&project);
    let format = Format::from_format_string("typst").unwrap();
    let options = RenderToFileOptions::default();
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

    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let output_path = &summary.outputs[0].output_path;
    let bytes = std::fs::read(output_path)
        .unwrap_or_else(|e| panic!("expected {} to exist: {e}", output_path.display()));
    assert!(bytes.starts_with(b"%PDF-"), "expected a real compiled PDF");

    // `pdf-extract` (unlike `pdftotext -layout`) sometimes emits runs of
    // multiple ASCII spaces between glyphs based on their layout kerning
    // (e.g. "Theorem   1" for "Theorem 1") — collapse whitespace runs
    // within each line before substring matching, same spirit as item
    // 49's own NBSP normalization above.
    let raw_text = pdf_extract::extract_text(output_path)
        .unwrap_or_else(|e| panic!("failed to extract text from {}: {e}", output_path.display()))
        .replace('\u{a0}', " ");
    let text: String = raw_text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        text.contains("Theorem 1.1") && text.contains("First Theorem"),
        "expected chapter one's own theorem to be numbered 'Theorem 1.1': {text}"
    );
    assert!(
        text.contains("Theorem 2.1") && text.contains("Second Theorem"),
        "expected chapter two's own theorem to be numbered 'Theorem 2.1' (orange-book \
         prefixes theorem numbers with the chapter): {text}"
    );

    // pdf-extract may merge the reference sentence and the following
    // theorem into one "line", so isolate the reference by a fixed
    // window before the sentence's tail instead of splitting on lines.
    let pos = text
        .find("for the earlier result")
        .unwrap_or_else(|| panic!("expected the cross-chapter reference sentence: {text}"));
    let window = &text[pos.saturating_sub(40)..pos];
    assert!(
        window.contains("Theorem 1.1"),
        "the cross-chapter reference must resolve to the earlier theorem's real \
         number ('Theorem 1.1'), not an unresolved placeholder or the wrong theorem: \
         {window:?}\nfull text: {text}"
    );
    assert!(
        !window.contains("Theorem 2.1"),
        "the cross-chapter reference must not resolve to chapter two's own theorem: \
         {window:?}\nfull text: {text}"
    );
}
