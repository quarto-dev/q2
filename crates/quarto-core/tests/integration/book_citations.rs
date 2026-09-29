/*
 * tests/integration/book_citations.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P2 items 82/83: book-wide citation correctness for the
 * single-file merge. The merge defers citeproc out of each chapter and
 * runs `pampa::citeproc_filter::apply_citeproc_filter` once on the merged
 * document (plan Decision 1), so a book must get one deduplicated
 * bibliography and one document-wide citation-numbering pass — not N
 * per-chapter ones.
 *
 * These tests drove the bd-oqoozmtr fix that landed with them: pampa's
 * citeproc used to resolve `bibliography`/`csl` against the process CWD
 * (no base dir at all), so any relative declaration — the only shape a
 * real book can portably write — failed to load. The fix threads a
 * declaration-site base dir into `apply_citeproc_filter` from both call
 * paths (UserFiltersStage = the document's directory; the book-merge
 * driver = the first file-chapter's directory, where the merged meta's
 * marked `Path` values are anchored).
 *
 * Fixtures use CSL-JSON bibliographies: pampa's citeproc does not parse
 * BibTeX (`.bib`) at all — separate gap, tracked independently.
 *
 * Also home to the non-book face of the same bd-oqoozmtr fix
 * (`single_file_citeproc_resolves_paths_relative_to_document`): the
 * UserFiltersStage call path had the identical process-CWD defect.
 */

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

/// A minimal numeric CSL style: in-text `[<citation-number>]`,
/// bibliography sorted by citation number.
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

fn render_book(project_dir: &Path) -> quarto_core::project::orchestrator::ProjectRenderSummary {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(project_dir, runtime.as_ref()).unwrap();

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
    pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("run_with_book_support failed: {e}"))
}

fn extract_pdf_text(output_path: &Path) -> String {
    // Same normalizations as items 49/77: NBSP -> space, then per-line
    // whitespace-run collapse for pdf-extract's kerning-induced
    // multi-spaces.
    let raw_text = pdf_extract::extract_text(output_path)
        .unwrap_or_else(|e| panic!("failed to extract text from {}: {e}", output_path.display()))
        .replace('\u{a0}', " ");
    raw_text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Item 82: two chapters citing the same source under a numeric CSL style
/// produce book-wide-correct, non-duplicated *in-text* citation numbers —
/// chapter two's citation of chapter one's source must reuse its original
/// number ("[1]"), not a fresh one, and the merged bibliography must list
/// each source exactly once.
#[test]
fn numeric_citation_numbers_are_book_wide() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: book\n\nbibliography: refs.json\ncsl: numeric.csl\n\nbook:\n  title: \"Citation Book\"\n  author: \"Test Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
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

    let summary = render_book(&project_dir);
    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let text = extract_pdf_text(&summary.outputs[0].output_path);

    assert!(
        text.contains("First use [1] and also [2]."),
        "chapter one's citations must number 1 and 2 in first-use order: {text}"
    );
    assert!(
        text.contains("same source [1]."),
        "chapter two's citation of chapter one's source must reuse the book-wide \
         number [1], not a fresh per-chapter number: {text}"
    );
    assert!(
        !text.contains("[3]"),
        "no third number may exist — the shared source must not be renumbered: {text}"
    );
    assert_eq!(
        text.matches("The TeXbook").count(),
        1,
        "the merged bibliography must list the shared source exactly once: {text}"
    );
}

/// Item 83: two references by the same author in the same year, cited
/// from *different* chapters under an author-date style, must disambiguate
/// book-wide — chapter one's "Knuth 1984a" and chapter two's "Knuth
/// 1984b" must agree with each other and with the single merged
/// bibliography. Per-chapter citeproc passes would each see only their own
/// citations and could assign suffixes independently.
#[test]
fn year_suffix_disambiguation_is_book_wide() {
    // Minimal author-date style with year-suffix disambiguation enabled.
    const AUTHOR_DATE_CSL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text" default-locale="en-US">
  <info>
    <title>Test Author Date</title>
    <id>http://www.zotero.org/styles/test-author-date</id>
    <link href="http://www.zotero.org/styles/test-author-date" rel="self"/>
    <author><name>Test</name></author>
    <category citation-format="author-date"/>
    <updated>2026-01-01T00:00:00+00:00</updated>
    <rights license="http://creativecommons.org/licenses/by-sa/3.0/">CC BY-SA</rights>
  </info>
  <citation disambiguate-add-year-suffix="true">
    <sort><key variable="issued"/><key variable="title"/></sort>
    <layout prefix="(" suffix=")" delimiter="; ">
      <group delimiter=" ">
        <names variable="author"><name form="short" and="text" initialize-with=". "/></names>
        <date variable="issued" form="text" date-parts="year"/>
      </group>
    </layout>
  </citation>
  <bibliography>
    <sort><key variable="author"/><key variable="issued"/><key variable="title"/></sort>
    <layout suffix=".">
      <group delimiter=" ">
        <names variable="author"><name and="text" initialize-with=". "/></names>
        <date variable="issued" form="text" date-parts="year"/>
        <text variable="title"/>
      </group>
    </layout>
  </bibliography>
</style>
"#;

    const SAME_AUTHOR_YEAR_REFS: &str = r#"[
  {
    "id": "knuth1984tex",
    "type": "book",
    "author": [{ "family": "Knuth", "given": "Donald E." }],
    "title": "The TeXbook",
    "issued": { "date-parts": [[1984]] },
    "publisher": "Addison-Wesley"
  },
  {
    "id": "knuth1984metafont",
    "type": "book",
    "author": [{ "family": "Knuth", "given": "Donald E." }],
    "title": "The METAFONTbook",
    "issued": { "date-parts": [[1984]] },
    "publisher": "Addison-Wesley"
  }
]
"#;

    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: book\n\nbibliography: refs.json\ncsl: author-date.csl\n\nbook:\n  title: \"Disambiguation Book\"\n  author: \"Test Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    // Chapter one cites the first of the two same-author-year works.
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\nTypesetting [@knuth1984tex].\n",
    );
    // Chapter two cites the second work and re-cites the first.
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nFonts [@knuth1984metafont], and again typesetting [@knuth1984tex].\n",
    );
    write(&project_dir.join("refs.json"), SAME_AUTHOR_YEAR_REFS);
    write(&project_dir.join("author-date.csl"), AUTHOR_DATE_CSL);

    let summary = render_book(&project_dir);
    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let text = extract_pdf_text(&summary.outputs[0].output_path);

    // Both suffixes exist and each chapter's citation matches the merged
    // bibliography's assignment: the bibliography sorts by title
    // ("METAFONTbook" < "TeXbook"), so METAFONTbook is 1984a.
    assert!(
        text.contains("Fonts (Knuth 1984a)"),
        "chapter two's METAFONTbook citation must carry the 1984a suffix: {text}"
    );
    assert!(
        text.contains("Typesetting (Knuth 1984b)"),
        "chapter one's TeXbook citation must carry the 1984b suffix: {text}"
    );
    assert!(
        text.contains("and again typesetting (Knuth 1984b)"),
        "chapter two's re-citation must reuse the same 1984b suffix: {text}"
    );
    assert!(
        !text.contains("(Knuth 1984)"),
        "no un-suffixed citation may survive — disambiguation is book-wide: {text}"
    );
    assert_eq!(
        text.matches("The METAFONTbook").count(),
        1,
        "each same-year work appears exactly once in the merged bibliography: {text}"
    );
    assert_eq!(
        text.matches("The TeXbook").count(),
        1,
        "each same-year work appears exactly once in the merged bibliography: {text}"
    );
}

/// Item 85's regression half (the audit half is recorded in the plan):
/// P2 defers citeproc out of each chapter, so a chapter's
/// Normalization-phase transforms now run on *raw* `Inline::Cite` nodes —
/// where previously an in-chapter citeproc pass could have replaced them
/// with formatted text first. The audit found every Normalization-phase
/// Cite touch point (`footnotes`, `footnotes_resolve`, `title_block`,
/// `metadata_normalize`, `shortcode_resolve`, `reference_link_diagnostics`)
/// only traverses `cite.content` or treats the node opaquely — none assume
/// formatted citations. This test proves the composite behavior: a
/// citation nested inside a footnote (a structure two Normalization-phase
/// transforms rewrite and relocate) must still be found and formatted by
/// the deferred, book-wide citeproc, in first-use order relative to the
/// other chapter's citations.
#[test]
fn normalization_phase_transforms_run_on_raw_citations_before_deferred_citeproc() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: book\n\nbibliography: refs.json\ncsl: numeric.csl\n\nbook:\n  title: \"Nested Citation Book\"\n  author: \"Test Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome to the book.\n",
    );
    // The footnote citation is the book's first citation in document
    // order, so book-wide numbering must give it [1] even though it sits
    // inside a structure Normalization-phase transforms relocate.
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\nBody with a note.[^1]\n\n[^1]: A cited note [@knuth1984].\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nSecond chapter cites [@doe2020].\n",
    );
    write(&project_dir.join("refs.json"), REFS_JSON);
    write(&project_dir.join("numeric.csl"), NUMERIC_CSL);

    let summary = render_book(&project_dir);
    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let text = extract_pdf_text(&summary.outputs[0].output_path);

    assert!(
        text.contains("A cited note [1]."),
        "the footnote-nested citation must be formatted by the deferred citeproc, \
         as the book-wide first citation: {text}"
    );
    assert!(
        text.contains("Second chapter cites [2]."),
        "chapter two's citation must follow the footnote citation in book-wide order: {text}"
    );
    assert!(
        !text.contains("@knuth1984") && !text.contains("@doe2020"),
        "no raw citation may survive the deferred citeproc: {text}"
    );
}

/// The non-book face of the same bd-oqoozmtr fix: a plain single-file
/// render with an explicit `filters: [citeproc]` and document-relative
/// `bibliography`/`csl` must resolve them against the document's own
/// directory, not the process CWD (nextest's CWD is the crate dir, so a
/// CWD-relative read fails here).
#[test]
fn single_file_citeproc_resolves_paths_relative_to_document() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    let input_path = project_dir.join("f.qmd");
    write(
        &input_path,
        "---\ntitle: Single\nbibliography: refs.json\ncsl: numeric.csl\nfilters:\n  - citeproc\n---\n\nA citation [@knuth1984].\n",
    );
    write(&project_dir.join("refs.json"), REFS_JSON);
    write(&project_dir.join("numeric.csl"), NUMERIC_CSL);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = quarto_core::render_to_file::render_document_to_file(
        &input_path,
        "html",
        &RenderToFileOptions::default(),
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("single-file render with citeproc must succeed with doc-relative paths");

    let html = std::fs::read_to_string(&result.output_path)
        .unwrap_or_else(|e| panic!("expected {}: {e}", result.output_path.display()));
    assert!(
        html.contains("[1]"),
        "citation must be processed by citeproc: {html}"
    );
    assert!(
        !html.contains("@knuth1984"),
        "no raw citation may survive: {html}"
    );
    assert_eq!(
        html.matches("The TeXbook").count(),
        1,
        "bibliography must list the source exactly once: {html}"
    );
}
