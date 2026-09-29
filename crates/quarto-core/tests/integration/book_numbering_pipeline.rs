/*
 * tests/integration/book_numbering_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P2 item 49: a real, compiled 3-chapter Typst book with a
 * figure in each chapter — inspected as actual rendered text, not
 * asserted from reading `book-numbering.lua`'s show-rule. See
 * `claude-notes/plans/2026-09-21-book-projects-P2-single-file-merge.md`.
 */

//! Drives a 3-chapter book (bare `--to typst`, resolving to the vendored
//! `orange-book` default per book-projects P2 item 82) through the real,
//! **unmodified** (except the tracked `book-numbering.lua` patch, item 79)
//! vendored Lua chain, compiles to a real PDF, and extracts its literal
//! text with `pdf-extract` (a pure-Rust PDF text extractor — no external
//! binary; CI provisions `typst` but not `pdftotext`, `typst query`
//! resolves numbering *patterns* not the resolved counter value at a given
//! figure, and Typst's SVG output renders text as vector paths, not
//! `<text>` nodes — none of those are viable here).
//!
//! Empirically observed behavior (not assumed): `book-numbering.lua`'s
//! `typst_book_counter_reset_rule` resets `counter(figure.where(kind:
//! image))` to 0 at every chapter (level-1 heading), so each chapter's
//! first figure renders as plain "Figure 1" — **not** a combined
//! "chapter.figure" numbering scheme. This is the "top-level-division
//! alone is not sufficient" finding's permanent regression guard: without
//! the counter-reset show rule, the three figures would render "Figure
//! 1"/"Figure 2"/"Figure 3" continuously across chapter boundaries.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
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

/// A minimal valid 1x1 PNG (the smallest well-known valid PNG byte
/// sequence) — same fixture `pandoc_render_to_file.rs`'s docx test and
/// `pandoc_typst_resource_copy.rs` use.
const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

const BOOK_QUARTO_YML: &str = r#"project:
  type: book

book:
  title: "Figure Numbering Book"
  author: "Test Author"
  chapters:
    - index.qmd
    - ch1.qmd
    - ch2.qmd
    - ch3.qmd
"#;

fn write_three_chapter_figure_fixture(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), BOOK_QUARTO_YML);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\n![First figure](img.png)\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\n![Second figure](img.png)\n",
    );
    write(
        &project_dir.join("ch3.qmd"),
        "# Chapter Three\n\n![Third figure](img.png)\n",
    );
    std::fs::write(project_dir.join("img.png"), ONE_PIXEL_PNG).unwrap();
}

/// The primary claim of book-projects P2 item 49: a real, compiled Typst
/// book resets each chapter's figure numbering to 1 — via
/// `book-numbering.lua`'s dynamically-generated show-rule, run
/// **unmodified** through pandoc's real `main.lua` chain (book-projects
/// P2b/P2c prerequisites: the dispatch fix and the resource-copy-ordering
/// fix, both of which this test also exercises end-to-end).
///
/// Numbering shape under orange-book's *real* `typst-show.typ` (staged via
/// `template-partials` after the PandocWriteStage fix; earlier revisions of
/// this test ran against the vendored default template because the partials
/// never reached the Typst leg): figures are chapter-prefixed, so the
/// per-chapter reset manifests as "Figure 1.1"/"Figure 2.1"/"Figure 3.1" —
/// without the reset, the running counter would read "Figure 2.2" and
/// "Figure 3.3" instead.
#[test]
fn typst_book_resets_figure_numbering_at_each_chapter() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_three_chapter_figure_fixture(&project_dir);

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

    // `pdf-extract` renders the supplement/number separator as a real
    // U+00A0 NO-BREAK SPACE glyph (confirmed empirically — Typst's own
    // `figure` show rule uses one there, not a plain ASCII space), so
    // normalize before doing any plain-ASCII substring matching below.
    let text = pdf_extract::extract_text(output_path)
        .unwrap_or_else(|e| panic!("failed to extract text from {}: {e}", output_path.display()))
        .replace('\u{a0}', " ");

    // Each chapter's own figure must read "Figure <chapter>.1" —
    // chapter-scoped reset under orange-book's chapter-prefixed
    // numbering, not a running count ("Figure 2.2", "Figure 3.3") across
    // chapter boundaries. Global contains-checks, not per-chapter text
    // slices: orange-book renders its own outline page, so the first
    // occurrence of each chapter title is the TOC entry, not the chapter
    // heading.
    assert!(
        text.contains("Figure 1.1"),
        "chapter one's own figure must be numbered 'Figure 1.1': {text}"
    );
    assert!(
        text.contains("Figure 2.1") && !text.contains("Figure 2.2"),
        "chapter two's figure must reset to 'Figure 2.1', not continue as 'Figure 2.2': {text}"
    );
    assert!(
        text.contains("Figure 3.1") && !text.contains("Figure 3.2") && !text.contains("Figure 3.3"),
        "chapter three's figure must reset to 'Figure 3.1', not continue as 'Figure 3.3': {text}"
    );
}

/// book-projects P2 item 74 — negative control. `book-numbering.lua`'s
/// `typst_book_counter_reset_rule` is gated on **both** `_quarto.format.
/// isTypstOutput()` *and* `param("single-file-book", false)`
/// (`resources/pandoc-filters/filters/quarto-pre/book-numbering.lua`); the
/// book-merge path (item 49, above) always sets both `top-level-division:
/// chapter` and `single-file-book: true` together, so item 49 alone cannot
/// tell you which lever actually causes the reset.
///
/// This test isolates `top-level-division` alone: a **plain, non-book**
/// document (rendered via `render_document_to_file`, not a book project —
/// so `BookSingleFileContributor` is never registered and the
/// `single-file-book` filter param is genuinely absent) with `top-level-
/// division: chapter` set directly in its own front matter and three H1
/// sections, each with a figure. If `top-level-division` alone caused the
/// reset, the figures would read 1/1/1 here too; since it does not, they
/// must read 1/2/3 — proving the two mechanisms are independent, not
/// redundant, exactly as this checklist item requires.
#[test]
fn typst_top_level_division_alone_does_not_reset_figure_numbering() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\n\
         title: Continuous Numbering\n\
         top-level-division: chapter\n\
         ---\n\n\
         # Chapter One\n\n![First figure](image.svg){#fig-one}\n\n\
         # Chapter Two\n\n![Second figure](image.svg){#fig-two}\n\n\
         # Chapter Three\n\n![Third figure](image.svg){#fig-three}\n",
    );
    // A trivial, genuinely valid SVG (same fixture
    // `pandoc_typst_compile.rs`'s crossref test uses) — no PNG CRC32
    // framing to get wrong by hand.
    write(
        &project_dir.join("image.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\">\
         <rect width=\"10\" height=\"10\" fill=\"black\"/></svg>",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    let result = render_document_to_file(
        &input_path,
        "typst",
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("plain typst render with top-level-division: chapter should compile to a real PDF");

    let bytes = std::fs::read(&result.output_path).unwrap();
    assert!(
        bytes.starts_with(b"%PDF-"),
        "expected a real PDF header, got: {:?}",
        &bytes[..bytes.len().min(20)]
    );

    let text = pdf_extract::extract_text(&result.output_path)
        .unwrap_or_else(|e| panic!("failed to extract text: {e}"))
        .replace('\u{a0}', " ");

    assert!(
        text.contains("Figure 1") && text.contains("Figure 2") && text.contains("Figure 3"),
        "expected continuous, unreset numbering (Figure 1/2/3) since `single-file-book` \
         is genuinely absent from this plain (non-book) render — top-level-division alone \
         must not trigger the chapter counter-reset show rule: {text}"
    );
}

/// book-projects P2 item 76 — an unnumbered chapter (`.unnumbered` on its
/// H1), merged into a single-file document.
///
/// **Corrected during implementation, empirically** (same pattern as item
/// 49, above): this item's original text guessed an unnumbered chapter's
/// own floats would number as a *continuation* of the preceding chapter's
/// sequence (no counter reset at its own boundary). A real 4-chapter probe
/// (index + Chapter One + an `{.unnumbered}` "Interlude" + Chapter Two,
/// each with one figure) shows that is not what happens:
/// `book-numbering.lua`'s `typst_book_counter_reset_rule` show rule
/// targets `heading.where(level: 1)` with **no unnumbered exclusion** in
/// its own source, so every level-1 heading — unnumbered chapters
/// included — still gets its own independent figure-counter reset. What
/// an unnumbered chapter actually contributes no boundary to is Typst's
/// **native chapter-number** counter specifically: the probe compiles to
/// "1. Chapter One" / "Interlude" (no number) / "2. Chapter Two" — the
/// unnumbered chapter prints no chapter number of its own, and the
/// following numbered chapter continues 1→2, not 1→3. (Numbers as
/// "1."/"2." with chapter-prefixed figures like "Figure 1.1": that is
/// orange-book's real `typst-show.typ`, staged via `template-partials`
/// after the PandocWriteStage fix; earlier revisions of this test ran
/// against the vendored default template.) This test asserts
/// the actually-observed split: figure counters reset at *every*
/// level-1 heading; only the *chapter*-number counter skips unnumbered
/// ones.
#[test]
fn typst_book_unnumbered_chapter_skips_chapter_number_but_still_resets_figure_counter() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: book\n\nbook:\n  title: \"Unnumbered Chapter Book\"\n  author: \"Test Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - interlude.qmd\n    - ch2.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\n![First figure](img.png)\n",
    );
    write(
        &project_dir.join("interlude.qmd"),
        "# Interlude {.unnumbered}\n\n![Interlude figure](img.png)\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\n![Second figure](img.png)\n",
    );
    std::fs::write(project_dir.join("img.png"), ONE_PIXEL_PNG).unwrap();

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

    let raw_text = pdf_extract::extract_text(output_path)
        .unwrap_or_else(|e| panic!("failed to extract text from {}: {e}", output_path.display()))
        .replace('\u{a0}', " ");
    let text: String = raw_text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");

    // Chapter-number counter: "1. Chapter One", no number of its own on
    // the unnumbered "Interlude", then "2. Chapter Two" — the unnumbered
    // chapter consumes no chapter number. (Global contains-checks rather
    // than text slices: orange-book's outline page puts a TOC occurrence
    // of each chapter title before the heading itself. No negative
    // "N. Interlude" check: the running header prints "Chapter 1.
    // Interlude" — the *counter* is still 1 there, which is exactly the
    // point.)
    assert!(
        text.contains("1. Chapter One"),
        "expected chapter one to be numbered '1.': {text}"
    );
    assert!(
        text.contains("2. Chapter Two") && !text.contains("3. Chapter Two"),
        "expected chapter two to continue the chapter-number sequence as '2.', not '3.' \
         (the unnumbered interlude must not consume a chapter number): {text}"
    );

    // Figure-number counter: every level-1 heading (numbered or not)
    // still gets its own independent reset. Under orange-book's
    // chapter-prefixed numbering, chapter one *and* the unnumbered
    // interlude both print "Figure 1.1" (the interlude resets the counter
    // while the chapter counter is still 1), and chapter two prints
    // "Figure 2.1".
    assert_eq!(
        text.matches("Figure 1.1").count(),
        2,
        "chapter one's and the interlude's figures must both reset to 'Figure 1.1': {text}"
    );
    assert!(
        text.contains("Figure 2.1") && !text.contains("Figure 2.2"),
        "chapter two's figure must reset to 'Figure 2.1', not continue the interlude's \
         sequence: {text}"
    );
    assert!(
        !text.contains("Figure 1.2"),
        "the interlude's figure must not continue chapter one's sequence as 'Figure 1.2': {text}"
    );
}

/// book-projects P2 item 78: a document using a custom `crossref.custom`
/// kind (mirroring `orange-book`'s own "Dinosaur"-style example category)
/// gets the same chapter-scoped, compile-time-resolved numbering as a
/// built-in figure — proving `book-numbering.lua`'s dynamic
/// `crossref.categories.all` walk (not a hardcoded `kind:image`-only
/// list) is what makes the counter-reset show rule correct for
/// user-declared kinds too.
#[test]
fn typst_book_resets_custom_crossref_kind_numbering_at_each_chapter() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: book\n\n\
         crossref:\n  custom:\n    - key: dino\n      kind: float\n      reference-prefix: Dinosaur\n\n\
         book:\n  title: \"Custom Crossref Book\"\n  author: \"Test Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\n![A dinosaur](img.png){#dino-1}\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\n![Another dinosaur](img.png){#dino-2}\n",
    );
    std::fs::write(project_dir.join("img.png"), ONE_PIXEL_PNG).unwrap();

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

    let raw_text = pdf_extract::extract_text(output_path)
        .unwrap_or_else(|e| panic!("failed to extract text from {}: {e}", output_path.display()))
        .replace('\u{a0}', " ");
    let text: String = raw_text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");

    // Global contains-checks, not per-chapter text slices: orange-book's
    // outline page puts a TOC occurrence of each chapter title before the
    // heading itself. Under orange-book's chapter-prefixed numbering the
    // reset reads as "Dinosaur 1.1" / "Dinosaur 2.1" — without it the
    // running counter would give chapter two "Dinosaur 2.2".
    assert!(
        text.contains("Dinosaur 1.1"),
        "chapter one's custom-kind float must be numbered 'Dinosaur 1.1': {text}"
    );
    assert!(
        text.contains("Dinosaur 2.1") && !text.contains("Dinosaur 2.2"),
        "chapter two's custom-kind float must reset to 'Dinosaur 2.1', not continue as \
         'Dinosaur 2.2': {text}"
    );
}
