/*
 * tests/integration/book_multifile_bibliography.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P6: unified bibliography for multi-file HTML books. Each
 * chapter keeps its own per-chapter citeproc pass (in-text citations,
 * chapter-local citation numbering), but only the designated
 * `book.references` chapter keeps a rendered bibliography div — every
 * other citing chapter is suppressed (`suppress-bibliography: true`) so
 * it does not also grow its own duplicate, unmerged local bibliography.
 * The references chapter's bibliography div is replaced with one merged,
 * deduplicated, book-wide-numbered/disambiguated list, built via
 * `quarto-citeproc`'s in-process `Processor` over the union of every
 * chapter's cited references.
 *
 * Fixtures mirror `book_citations.rs` (P2's single-file-merge sibling):
 * CSL-JSON bibliographies, the same minimal numeric/author-date CSL
 * styles it uses for its own book-wide-numbering/disambiguation tests.
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

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

/// A minimal numeric CSL style: in-text `[<citation-number>]`,
/// bibliography sorted by citation number. Identical to
/// `book_citations.rs`'s own `NUMERIC_CSL` fixture.
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

/// Minimal author-date style with year-suffix disambiguation enabled.
/// Identical to `book_citations.rs`'s own `AUTHOR_DATE_CSL` fixture.
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

fn render_book(project_dir: &Path) -> ProjectRenderSummary<RenderToFileResult> {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(project_dir, runtime.as_ref()).unwrap();
    let project_type = project_type_for(&project);
    let format = Format::from_format_string("html").unwrap();
    let options = RenderToFileOptions::default();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        "html",
        &options,
        runtime,
    );
    pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("run_with_book_support failed: {e}"))
}

fn read_output(outputs: &[RenderToFileResult], stem: &str) -> String {
    outputs
        .iter()
        .find(|r| r.output_path.file_stem().is_some_and(|s| s == stem))
        .unwrap_or_else(|| panic!("no html output with stem {stem} in {outputs:?}"))
        .render_output
        .html
        .clone()
}

/// Items 28/30/31/32/33 (merge core + the nineteenth-pass regression
/// guard): two chapters citing the same source produce one deduplicated
/// entry in the references chapter, a citation used in only one chapter
/// still appears exactly once, each citing chapter's own in-text citation
/// still resolves normally, and — the regression this phase's whole
/// `suppress-bibliography` fix exists for — an ordinary (non-references)
/// chapter that cites a source renders with **no** local bibliography
/// block at all.
#[test]
fn merged_bibliography_dedupes_and_ordinary_chapters_have_no_local_bibliography() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: book\n\nbibliography: refs.json\nfilters:\n  - citeproc\n\nbook:\n  title: \"Bib Book\"\n  author: \"Test Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n  references: references.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "# Welcome {.unnumbered}\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\nSee @knuth1984 and @doe2020.\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nAlso see @knuth1984.\n",
    );
    write(&project_dir.join("references.qmd"), "# References\n");
    write(&project_dir.join("refs.json"), REFS_JSON);

    let summary = render_book(&project_dir);
    assert!(
        summary.pass2_failures.is_empty(),
        "{:?}",
        summary.pass2_failures
    );

    let ch1 = read_output(&summary.outputs, "ch1");
    let ch2 = read_output(&summary.outputs, "ch2");
    let references = read_output(&summary.outputs, "references");

    // In-text citations still render normally in every citing chapter.
    assert!(
        ch1.contains("Knuth") && ch1.contains("Doe"),
        "chapter one's in-text citations must still render: {ch1}"
    );
    assert!(
        ch2.contains("Knuth"),
        "chapter two's in-text citation must still render: {ch2}"
    );

    // Neither ordinary chapter grows its own local bibliography div.
    assert!(
        !ch1.contains(r#"id="refs""#),
        "chapter one must have no local bibliography div: {ch1}"
    );
    assert!(
        !ch2.contains(r#"id="refs""#),
        "chapter two must have no local bibliography div: {ch2}"
    );

    // The references chapter carries exactly one merged, deduplicated
    // bibliography: each cited source appears exactly once.
    assert_eq!(
        references.matches(r#"id="ref-knuth1984""#).count(),
        1,
        "knuth1984 must appear exactly once in the merged bibliography: {references}"
    );
    assert_eq!(
        references.matches(r#"id="ref-doe2020""#).count(),
        1,
        "doe2020 (cited only by chapter one) must still appear exactly once: {references}"
    );
    assert!(
        references.contains(r#"id="refs""#),
        "the references chapter must carry the merged bibliography div: {references}"
    );
}

/// Items 8/9's documenting + regression pair: under a numeric CSL style,
/// the same source renumbers per chapter (Q1's own confirmed, inherited
/// limitation — pinned here so a future change is deliberate, not silent
/// drift) — but the *merged bibliography itself* is correctly numbered in
/// book-wide first-cited order, not blank/unnumbered.
#[test]
fn numeric_style_in_text_differs_per_chapter_but_merged_bibliography_is_book_numbered() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: book\n\nbibliography: refs.json\ncsl: numeric.csl\nfilters:\n  - citeproc\n\nbook:\n  title: \"Numeric Bib Book\"\n  author: \"Test Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n  references: references.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "# Welcome {.unnumbered}\n\nWelcome to the book.\n",
    );
    // Chapter one cites doe2020 first, then knuth1984 — its own processor
    // numbers them [1] and [2].
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\nFirst @doe2020, then @knuth1984.\n",
    );
    // Chapter two cites only knuth1984 — its own (separate) processor
    // numbers it [1], not [2]: the known, Q1-identical per-chapter
    // renumbering limitation.
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nAgain @knuth1984.\n",
    );
    write(&project_dir.join("references.qmd"), "# References\n");
    write(&project_dir.join("refs.json"), REFS_JSON);
    write(&project_dir.join("numeric.csl"), NUMERIC_CSL);

    let summary = render_book(&project_dir);
    assert!(
        summary.pass2_failures.is_empty(),
        "{:?}",
        summary.pass2_failures
    );

    let ch1 = read_output(&summary.outputs, "ch1");
    let ch2 = read_output(&summary.outputs, "ch2");
    let references = read_output(&summary.outputs, "references");

    assert!(
        ch1.contains("First [1], then [2]."),
        "chapter one's own citation order must number 1 then 2: {ch1}"
    );
    // Documenting test: chapter two's own processor starts fresh, so the
    // same source gets [1] here too — not the book-wide [2]. This is the
    // inherited, Q1-identical limitation this phase deliberately does not
    // fix (see plan Decisions).
    assert!(
        ch2.contains("Again [1]."),
        "chapter two's per-chapter renumbering must reproduce Q1's own \
         behavior (not book-wide [2]): {ch2}"
    );

    // The merged bibliography itself IS book-wide numbered: doe2020 was
    // first-cited across the book (chapter one, before knuth1984), so it
    // gets [1] and knuth1984 gets [2] — not blank/unnumbered.
    let doe_pos = references
        .find("A Paper")
        .unwrap_or_else(|| panic!("doe2020 entry missing from merged bibliography: {references}"));
    let knuth_pos = references.find("The TeXbook").unwrap_or_else(|| {
        panic!("knuth1984 entry missing from merged bibliography: {references}")
    });
    assert!(
        references.contains("[1]") && references.contains("[2]"),
        "the merged bibliography must be numbered, not blank: {references}"
    );
    assert!(
        doe_pos < knuth_pos,
        "book-wide first-cited order (doe2020 before knuth1984) must drive the \
         merged bibliography's citation-number order: {references}"
    );
}

/// Item 37: two references sharing the same author+year, cited from
/// *different* chapters under an author-date style, must disambiguate
/// book-wide in the merged bibliography — chapter one's citation and
/// chapter two's citation must each carry a year suffix, and the two
/// distinct works must not collide in the merged list.
#[test]
fn year_suffix_disambiguation_is_book_wide_in_merged_bibliography() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: book\n\nbibliography: refs.json\ncsl: author-date.csl\nfilters:\n  - citeproc\n\nbook:\n  title: \"Disambiguation Bib Book\"\n  author: \"Test Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n  references: references.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "# Welcome {.unnumbered}\n\nWelcome to the book.\n",
    );
    write(
        &project_dir.join("ch1.qmd"),
        "# Chapter One\n\nTypesetting [@knuth1984tex].\n",
    );
    write(
        &project_dir.join("ch2.qmd"),
        "# Chapter Two\n\nFonts [@knuth1984metafont].\n",
    );
    write(&project_dir.join("references.qmd"), "# References\n");
    write(&project_dir.join("refs.json"), SAME_AUTHOR_YEAR_REFS);
    write(&project_dir.join("author-date.csl"), AUTHOR_DATE_CSL);

    let summary = render_book(&project_dir);
    assert!(
        summary.pass2_failures.is_empty(),
        "{:?}",
        summary.pass2_failures
    );

    let references = read_output(&summary.outputs, "references");

    assert_eq!(
        references.matches("The METAFONTbook").count(),
        1,
        "each same-author-year work appears exactly once in the merged bibliography: {references}"
    );
    assert_eq!(
        references.matches("The TeXbook").count(),
        1,
        "each same-author-year work appears exactly once in the merged bibliography: {references}"
    );
    assert!(
        references.contains("1984a") || references.contains("1984b"),
        "the merged bibliography must carry year-suffix disambiguation \
         (seeded via process_citations_with_disambiguation): {references}"
    );
}

/// Item resolved with the user (fifteenth pass): a book chapter whose
/// `filters:` config explicitly orders `citeproc` into `.post` (after the
/// `quarto` sentinel) produces a `Q-23-1` diagnostic naming the chapter,
/// rather than silently dropping its references from the merged
/// bibliography.
#[test]
fn citeproc_in_post_produces_named_diagnostic_not_silent_drop() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: book\n\nbibliography: refs.json\n\nbook:\n  title: \"Post Ordering Book\"\n  author: \"Test Author\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n  references: references.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "# Welcome {.unnumbered}\n\nWelcome to the book.\n",
    );
    // `quarto` sentinel first, so `citeproc` resolves in `.post`.
    write(
        &project_dir.join("ch1.qmd"),
        "---\nfilters:\n  - quarto\n  - citeproc\n---\n\n# Chapter One\n\nSee @knuth1984.\n",
    );
    write(&project_dir.join("references.qmd"), "# References\n");
    write(&project_dir.join("refs.json"), REFS_JSON);

    let summary = render_book(&project_dir);
    assert!(
        summary.pass2_failures.is_empty(),
        "{:?}",
        summary.pass2_failures
    );

    let ch1 = summary
        .outputs
        .iter()
        .find(|r| r.output_path.file_stem().is_some_and(|s| s == "ch1"))
        .expect("chapter one must still render");
    assert!(
        ch1.render_output
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-23-1")),
        "a citeproc-in-.post chapter must produce a Q-23-1 diagnostic naming it: {:?}",
        ch1.render_output.diagnostics
    );
}
