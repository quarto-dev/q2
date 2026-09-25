/*
 * tests/integration/book_appendix_letter_parity.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P3: the compiled-artifact half of P2's cross-artifact
 * invariant (design doc §4's "provably equal by construction, but nothing
 * tests it"). For a fixture with TWO appendix chapters, the appendix
 * letters P1's sidebar path computes (`book_render_items` +
 * `chapter_label_prefix` — the exact functions the sidebar decoration
 * consumes) must match the letters the unpatched `orange-book` extension
 * actually renders in the compiled single-file Typst PDF. The PDF
 * assertion is written in terms of the *computed* letters, so a data-path
 * regression and a render-path regression each break a different half.
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::book::config::chapter_label_prefix;
use quarto_core::project::book::render_item::book_render_items;
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
  title: "Letter Parity Book"
  author: "Test Author"
  chapters:
    - index.qmd
    - ch1.qmd
  appendices:
    - app-a.qmd
    - app-b.qmd
"#;

fn write_fixture(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), BOOK_QUARTO_YML);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(&project_dir.join("ch1.qmd"), "# Chapter One\n\nBody one.\n");
    write(
        &project_dir.join("app-a.qmd"),
        "# Appendix Alpha\n\nAlpha body.\n",
    );
    write(
        &project_dir.join("app-b.qmd"),
        "# Appendix Beta\n\nBeta body.\n",
    );
}

const UNNUM_BOOK_QUARTO_YML: &str = r#"project:
  type: book

book:
  title: "Unnum App Book"
  author: "Test Author"
  chapters:
    - index.qmd
    - ch1.qmd
  appendices:
    - app-a.qmd
    - app-b.qmd
"#;

#[test]
fn sidebar_appendix_letters_match_the_compiled_typst_pdf_letters() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

    // Data half: the letters P1 computes for the sidebar, via the same
    // functions `BookProjectType::config` feeds `book_sidebar_contents`.
    let book = project
        .config
        .metadata
        .as_ref()
        .and_then(|m| m.get("book"))
        .cloned()
        .expect("book config present");
    let items = book_render_items(&project_dir, &book, "Appendices", runtime.as_ref())
        .expect("render items build");
    let letter_for = |file: &str| -> String {
        items
            .iter()
            .find(|i| i.file.as_deref() == Some(Path::new(file)))
            .and_then(chapter_label_prefix)
            .unwrap_or_else(|| panic!("sidebar letter for {file}"))
    };
    let letter_a = letter_for("app-a.qmd");
    let letter_b = letter_for("app-b.qmd");
    assert_eq!(
        (letter_a.as_str(), letter_b.as_str()),
        ("A", "B"),
        "the sidebar data path must letter the two appendices A and B in order"
    );

    // Compiled half: the letters orange-book actually renders.
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
    assert!(
        std::fs::read(output_path).unwrap().starts_with(b"%PDF-"),
        "expected a real compiled PDF"
    );

    // Same pdf-extract normalizations as the other book tests: NBSP ->
    // space, per-line whitespace-run collapse for kerning-induced runs.
    let raw_text = pdf_extract::extract_text(output_path)
        .unwrap_or_else(|e| panic!("failed to extract text: {e}"))
        .replace('\u{a0}', " ");
    let text: String = raw_text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");

    // Cross-artifact assertion in terms of the *computed* letters: if the
    // sidebar path ever letters differently than the extension renders,
    // one of these two fails.
    assert!(
        text.contains(&format!("{letter_a}. Appendix Alpha")),
        "PDF appendix heading must use the sidebar's letter {letter_a}: {text}"
    );
    assert!(
        text.contains(&format!("{letter_b}. Appendix Beta")),
        "PDF appendix heading must use the sidebar's letter {letter_b}: {text}"
    );
}

/// P3's documented-outcome probe: an individually `.unnumbered` appendix
/// chapter (not the synthetic "Appendices" divider) rendered through
/// single-file Typst mode. The sidebar data path gives it no letter and
/// does not consume a slot (`config.rs::
/// unnumbered_appendix_chapter_sidebar_text_has_no_letter`); this test
/// records what the unpatched extension does in the compiled PDF for the
/// same fixture.
#[test]
fn unnumbered_appendix_chapter_in_single_file_typst() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(&project_dir.join("_quarto.yml"), UNNUM_BOOK_QUARTO_YML);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(&project_dir.join("ch1.qmd"), "# Chapter One\n\nBody one.\n");
    write(
        &project_dir.join("app-a.qmd"),
        "# Unnumbered Appendix {.unnumbered}\n\nUnnumbered appendix body.\n",
    );
    write(
        &project_dir.join("app-b.qmd"),
        "# Appendix Beta\n\nNumbered appendix body.\n",
    );

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

    let raw_text = pdf_extract::extract_text(output_path)
        .unwrap_or_else(|e| panic!("failed to extract text: {e}"))
        .replace('\u{a0}', " ");
    let text: String = raw_text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");

    // Observation assertions (probed empirically, 2026-09-24): the
    // unpatched extension skips an individually-unnumbered appendix
    // chapter for lettering (its heading renders bare, like `sections.lua`
    // skips an unnumbered ordinary chapter for numeric numbering), and the
    // *next* numbered appendix chapter receives letter "A" — the extension
    // letters by counting rendered appendix headings.
    //
    // Cross-artifact note (surfaced to Gordon, 2026-09-24): the sidebar
    // data path — a faithful port of Q1's `chapterInfoForInput` numbering —
    // gives that same next-numbered appendix "B" (unnumbered consumes no
    // slot, so numbered appendices stay dense: 1=A, 2=B), while the PDF
    // shows "A. Appendix Beta". Both halves are Q1-faithful, so this
    // sidebar-vs-PDF divergence exists in Q1 too for this corner case; it
    // is the design doc §4 cross-artifact risk with concrete evidence.
    // (`author:` is required by the unpatched extension's own lib.typ
    // title block — an authorless book crashes in `@preview/orange-book`
    // itself, identically in Q1.)
    assert!(
        text.lines().any(|line| line == "Unnumbered Appendix"),
        "the unnumbered appendix's heading must render bare (no letter prefix): {text}"
    );
    assert!(
        !text.contains("A. Unnumbered Appendix"),
        "the extension must not letter the unnumbered appendix: {text}"
    );
    assert!(
        text.contains("A. Appendix Beta"),
        "the extension letters the first *rendered* numbered appendix 'A', \
         skipping the unnumbered one: {text}"
    );
    assert!(
        !text.contains("B. Appendix Beta"),
        "no second letter may appear — the unnumbered appendix consumed no slot: {text}"
    );
}
