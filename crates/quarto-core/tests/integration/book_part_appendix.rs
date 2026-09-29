/*
 * tests/integration/book_part_appendix.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P2 item 79: a merged book with a `part:` divider and an
 * appendix chapter, rendered through the real, *unpatched* `orange-book`
 * extension (the zero-config book default — no `format:`/extension key
 * anywhere in the fixture), produces a real `#part[...]` block and
 * appendix-letter theorem numbering ("Theorem A.1") in the compiled Typst
 * output. This is the test that proves the comment-marker emission (not a
 * Pandoc-attribute patch) is what makes an unpatched extension work; it
 * would have caught the "patch-and-drop breaks orange-book" finding if it
 * had existed earlier. See
 * claude-notes/plans/2026-09-21-book-projects-P2-single-file-merge.md.
 */

//! Two historical bugs would each have failed this test, and did fail its
//! probe: P2b's dispatch fix (`post-quarto` filters no longer crash), and
//! the `template-partials` staging gap (`PandocWriteStage` staged no
//! extension partials for the Typst leg, so orange-book's `typst-show.typ`
//! — the only place Typst's `part`/`chapter` functions are imported —
//! never reached the output and `#part[Part One]` died with `unknown
//! variable: part`). Fixture shape probed empirically in
//! /tmp/part-app-probe before assertions were written, per this plan's
//! empirical-first pattern.

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

keep-typ: true

book:
  title: "Part Appendix Book"
  author: "Test Author"
  chapters:
    - index.qmd
    - part: "Part One"
      chapters:
        - ch1.qmd
    - ch2.qmd
  appendices:
    - app-a.qmd
"#;

fn write_fixture(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), BOOK_QUARTO_YML);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(&project_dir.join("ch1.qmd"), "# Chapter One\n\nBody one.\n");
    write(&project_dir.join("ch2.qmd"), "# Chapter Two\n\nBody two.\n");
    write(
        &project_dir.join("app-a.qmd"),
        "# Appendix Alpha\n\n::: {#thm-app .theorem name=\"Appendix Theorem\"}\nAn appendix theorem.\n:::\n",
    );
}

/// The item-79 claim, end to end: the part divider and the appendix both
/// survive the merge into the compiled PDF, the unpatched extension's own
/// `#part[...]` emission compiles, and the appendix's theorem is numbered
/// by appendix *letter* ("Theorem A.1"), not by a continuing chapter
/// number ("Theorem 3.1").
#[test]
fn part_divider_and_appendix_compile_through_unpatched_orange_book() {
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

    // The kept intermediate proves the extension's own filter emission is
    // what reached the compiler: a raw `#part[...]` block, unpatched.
    let typ_path = output_path.with_extension("typ");
    let typ_text = std::fs::read_to_string(&typ_path)
        .unwrap_or_else(|e| panic!("expected kept intermediate {}: {e}", typ_path.display()));
    assert!(
        typ_text.contains("#part[Part One]"),
        "expected the unpatched extension's own #part[...] emission in the .typ: {typ_text}"
    );

    // Same pdf-extract normalizations as items 49/77: NBSP -> space, and
    // per-line whitespace-run collapse for kerning-induced multi-spaces.
    let raw_text = pdf_extract::extract_text(output_path)
        .unwrap_or_else(|e| panic!("failed to extract text from {}: {e}", output_path.display()))
        .replace('\u{a0}', " ");
    let text: String = raw_text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        text.contains("Part One"),
        "expected the part divider rendered in the compiled output: {text}"
    );
    assert!(
        text.contains("A. Appendix Alpha"),
        "expected the appendix chapter lettered 'A.': {text}"
    );
    assert!(
        text.contains("Theorem A.1"),
        "expected the appendix's theorem numbered by appendix letter ('Theorem A.1'): {text}"
    );
    assert!(
        !text.contains("Theorem 3.1") && !text.contains("3. Appendix Alpha"),
        "the appendix must not be numbered as a continuing chapter 3: {text}"
    );
}
