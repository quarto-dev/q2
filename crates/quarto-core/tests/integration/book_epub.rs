/*
 * tests/integration/book_epub.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P3: EPUB book output, inspected through the zip archive
 * (mirroring `crates/quarto/tests/integration/render_pandoc_formats_e2e.rs`'s
 * `e2e_render_epub_chapter_level` / `e2e_render_epub_cover_image`
 * methodology). Covers:
 *
 * - chapters split at `book.chapters` boundaries (one `.xhtml` per chapter,
 *   each chapter's body in its own file),
 * - the `book.cover-image` → `epub-cover-image` derivation (P3's deliberate
 *   adaptation to Q2's architecture — Q1 writes a generic `cover-image`
 *   metadata key that Pandoc's EPUB writer reads natively; Q2 forwards the
 *   literally-named `epub-cover-image` key instead), asserted via the OPF
 *   manifest's `properties="cover-image"` marker.
 *
 * See claude-notes/plans/2026-09-21-book-projects-P3-typst-epub.md.
 */

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use zip::ZipArchive;

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// A minimal valid 1x1 PNG (same fixture the other book tests use).
const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

fn book_quarto_yml(extra_book_keys: &str) -> String {
    format!(
        r#"project:
  type: book

book:
  title: "Epub Book"
  output-file: epub-book
{extra_book_keys}  chapters:
    - index.qmd
    - ch1.qmd
    - ch2.qmd
"#
    )
}

fn write_fixture(project_dir: &Path, extra_book_keys: &str, with_cover: bool) {
    write(
        &project_dir.join("_quarto.yml"),
        &book_quarto_yml(extra_book_keys),
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\nBody one is distinctively first.\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nBody two is distinctively second.\n",
    );
    if with_cover {
        std::fs::write(project_dir.join("cover.png"), ONE_PIXEL_PNG).unwrap();
    }
}

/// Render the fixture to EPUB through `run_with_book_support()` and return
/// (temp dir, epub bytes).
fn render_epub(extra_book_keys: &str, with_cover: bool) -> (TempDir, Vec<u8>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_fixture(&project_dir, extra_book_keys, with_cover);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

    let project_type = project_type_for(&project);
    let format = Format::from_format_string("epub").unwrap();
    let options = RenderToFileOptions::default();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        "epub",
        &options,
        runtime,
    );
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("run_with_book_support failed: {e}"));

    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let output_path = &summary.outputs[0].output_path;
    let bytes = std::fs::read(output_path)
        .unwrap_or_else(|e| panic!("expected {} to exist: {e}", output_path.display()));
    (temp, bytes)
}

/// The chapter names inside the archive that hold rendered chapter bodies —
/// pandoc's epub writer puts them under `EPUB/text/`, and the title page is
/// not a chapter.
fn chapter_xhtml_names(zip: &mut ZipArchive<impl Read + Seek>) -> Vec<String> {
    zip.file_names()
        .filter(|n| n.ends_with(".xhtml") && n.contains("text") && !n.contains("title_page"))
        .map(|n| n.to_string())
        .collect()
}

fn read_entry(zip: &mut ZipArchive<impl Read + Seek>, name: &str) -> String {
    let mut entry = zip
        .by_name(name)
        .unwrap_or_else(|e| panic!("entry {name} in epub: {e}"));
    let mut text = String::new();
    entry.read_to_string(&mut text).unwrap();
    text
}

/// A book renders to a single `.epub` whose chapters split at
/// `book.chapters` boundaries: one body `.xhtml` per chapter, and each
/// chapter's body text lives in a *different* file (a merged-no-split
/// regression would put all three bodies in one).
#[test]
fn epub_book_splits_chapters_at_book_chapter_boundaries() {
    let (_temp, bytes) = render_epub("", false);
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = ZipArchive::new(cursor).expect("epub should be a valid zip archive");

    let names = chapter_xhtml_names(&mut zip);
    assert_eq!(
        names.len(),
        3,
        "one chapter file per book chapter (index, ch1, ch2): {names:?}"
    );

    let bodies: Vec<String> = names
        .iter()
        .map(|name| read_entry(&mut zip, name))
        .collect();

    let find_body =
        |needle: &str| -> Option<usize> { bodies.iter().position(|b| b.contains(needle)) };
    let home = find_body("Welcome to the book.").expect("index body present");
    let one = find_body("Body one is distinctively first.").expect("ch1 body present");
    let two = find_body("Body two is distinctively second.").expect("ch2 body present");
    assert_ne!(
        home, one,
        "index and chapter one must be in different files"
    );
    assert_ne!(
        one, two,
        "chapter one and chapter two must be in different files"
    );
}

/// A minimal numeric CSL style: in-text `[<citation-number>]`,
/// bibliography sorted by citation number (same fixture
/// `book_citations.rs` uses for its Typst-leg assertions).
const NUMERIC_CSL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text" default-locale="en-US">
  <info>
    <title>Test Numeric</title>
    <id>http://www.zotero.org/styles/test-numeric</id>
    <link href="http://www.zotero.org/styles/test-numeric" rel="self"/>
    <author><name>Test</name></author>
    <category citation-format="numeric"/>
    <updated>2026-01-01T00:00:00+00:00</updated>
    <rights license="http://creativecommons.org/licenses/by-sa/3.0/">CC BY-SA</rights>
  </info>
  <citation>
    <sort><key variable="citation-number"/></sort>
    <layout delimiter=", "><text variable="citation-number" prefix="[" suffix="]"/></layout>
  </citation>
  <bibliography>
    <sort><key variable="citation-number"/></sort>
    <layout><text variable="citation-number" prefix="[" suffix="] "/><group delimiter=" "><names variable="author"><name/></names><date variable="issued" form="numeric" date-parts="year"/><text variable="title"/></group></layout>
  </bibliography>
</style>
"#;

const REFS_JSON: &str = r#"[
  {
    "id": "knuth1984",
    "type": "book",
    "author": [{ "family": "Knuth", "given": "Donald E." }],
    "title": "The TeXbook",
    "issued": { "date-parts": [[1984]] },
    "publisher": "Addison-Wesley"
  },
  {
    "id": "doe2020",
    "type": "article-journal",
    "author": [{ "family": "Doe", "given": "Jane" }],
    "title": "A Paper",
    "issued": { "date-parts": [[2020]] }
  }
]
"#;

/// The EPUB leg of the unified-bibliography invariant (the Typst leg is
/// `book_citations.rs`'s own suite): two chapters citing shared sources
/// under a numeric CSL produce book-wide in-text numbers and ONE merged
/// bibliography across the split chapter files — not a per-chapter
/// reference list with per-chapter numbering.
#[test]
fn epub_book_bibliography_is_unified_across_chapters() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_fixture(&project_dir, "", false);
    write(
        &project_dir.join("_quarto.yml"),
        r#"project:
  type: book

bibliography: refs.json
csl: numeric.csl

book:
  title: "Epub Citations"
  output-file: epub-book
  chapters:
    - index.qmd
    - ch1.qmd
    - ch2.qmd
"#,
    );
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\nFirst use [@knuth1984] and also [@doe2020].\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nSecond use of the same source [@knuth1984].\n",
    );
    write(&project_dir.join("refs.json"), REFS_JSON);
    write(&project_dir.join("numeric.csl"), NUMERIC_CSL);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let project_type = project_type_for(&project);
    let format = Format::from_format_string("epub").unwrap();
    let options = RenderToFileOptions::default();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        "epub",
        &options,
        runtime,
    );
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("run_with_book_support failed: {e}"));
    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let bytes = std::fs::read(&summary.outputs[0].output_path).unwrap();

    let cursor = std::io::Cursor::new(bytes);
    let mut zip = ZipArchive::new(cursor).expect("epub should be a valid zip archive");
    let names = chapter_xhtml_names(&mut zip);
    let bodies: Vec<String> = names
        .iter()
        .map(|name| read_entry(&mut zip, name))
        .collect();
    let all: String = bodies.join("\n");

    assert!(
        bodies
            .iter()
            .any(|b| b.contains("First use [1] and also [2].")),
        "chapter one's citations must number 1 and 2 in first-use order: {all}"
    );
    assert!(
        bodies.iter().any(|b| b.contains("same source [1].")),
        "chapter two's citation of chapter one's source must reuse the book-wide \
         number [1], not a fresh per-chapter number: {all}"
    );
    assert!(
        !all.contains("[3]"),
        "no third number may exist — the shared source must not be renumbered: {all}"
    );
    assert_eq!(
        all.matches("The TeXbook").count(),
        1,
        "the merged bibliography must list the shared source exactly once across \
         the split chapter files: {all}"
    );
}

/// `book.cover-image` set, no explicit `epub-cover-image`: the rendered
/// `.epub`'s OPF manifest marks the derived cover with
/// `properties="cover-image"` (the discriminating signal a cover was wired,
/// not just present as an unrelated media file).
#[test]
fn book_cover_image_derives_epub_cover_image_opf_property() {
    let (_temp, bytes) = render_epub("  cover-image: cover.png\n", true);
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = ZipArchive::new(cursor).expect("epub should be a valid zip archive");

    let opf_name = zip
        .file_names()
        .find(|n| n.ends_with("content.opf"))
        .map(|n| n.to_string())
        .expect("epub should contain an OPF package document");
    let opf = read_entry(&mut zip, &opf_name);
    assert!(
        opf.contains("cover-image"),
        "OPF manifest should mark the derived cover image item: {opf}"
    );
}
