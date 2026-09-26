/*
 * tests/integration/book_numbering_torture.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P3: reduction of Q1's `orange-book` numbering torture test
 * (`tests/docs/smoke-all/typst/orange-book` in quarto-cli — the deepest
 * numbering-correctness fixture in Q1's own suite), rebuilt as a Q2 fixture
 * with image-based figures (Q2 cannot execute the original's knitr cells)
 * and no bibliography/citations (P3 asserts those separately), rendered
 * through the real single-file-merge path against the *unpatched*,
 * zero-config-defaulted `orange-book` extension, with every expected string
 * taken from Q1's own compiled PDF (`Test-Typst-Book.pdf`, extracted with
 * pdftotext) and verified against a q2 probe render before being written
 * down.
 *
 * Constructs covered (one per construct type Q1's fixture exercises):
 * figures, tables, display equations, theorems/lemmas/definitions,
 * code listings, labeled callouts, sub-figure panels, and a custom
 * `crossref.custom` kind — each asserted chapter-scoped ("Figure 1.1"
 * resetting to "Figure 2.1", "2.1a"/"2.1b" panel sub-numbers, "Theorem
 * A.1" appendix lettering) against the compiled PDF, plus cross-chapter
 * references resolving to the *original* chapter's number.
 *
 * See claude-notes/plans/2026-09-21-book-projects-P3-typst-epub.md.
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

/// A minimal valid 1x1 PNG (same fixture `book_numbering_pipeline.rs`
/// uses) — stands in for the knitr-generated SVGs Q1's original embeds.
const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

const BOOK_QUARTO_YML: &str = r#"project:
  type: book

keep-typ: true

crossref:
  custom:
    - kind: float
      key: dino
      reference-prefix: Dinosaur

book:
  title: "Numbering Torture Book"
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
    write(
        &project_dir.join("ch1.qmd"),
        r#"# Chapter One

See @fig-cars for an example figure.

![A plot of the cars dataset](img.png){#fig-cars}

::: {#wrn-tea .callout-warning}
## Whose Tea Is This?

On no account allow anyone to make you a conditions-affected beverage. See @wrn-tea to reference this warning.
:::

::: {#dino-steg}
**Stegosaurus**

This is the first dinosaur. See @dino-steg to self-reference.
:::

The fundamental equation:

$$
E = mc^2
$$ {#eq-einstein}

As shown in @eq-einstein, energy and mass are equivalent.

::: {#thm-pythagorean}
## Pythagorean Theorem

For a right triangle: $a^2 + b^2 = c^2$
:::

See @thm-pythagorean for the classic result.

::: {#lem-triangle}
## Triangle Inequality

For any triangle: $a + b > c$
:::

See @lem-triangle for the inequality.

```{#lst-hello .python lst-cap="Hello World in Python"}
def hello():
    print("Hello, World!")
```

See @lst-hello for a simple example.
"#,
    );
    write(
        &project_dir.join("ch2.qmd"),
        r#"# Chapter Two

See @tbl-data for sample data.

| Column A | Column B |
|----------|----------|
| 1        | 2        |

: Sample data table {#tbl-data}

See @fig-panel for a panel, or @fig-panel-a and @fig-panel-b individually.

::: {#fig-panel layout-ncol=2}

![First panel](img.png){#fig-panel-a}

![Second panel](img.png){#fig-panel-b}

A panel with two sub-figures.
:::

Refer back to @fig-cars and @eq-einstein from Chapter One.

The quadratic formula:

$$
x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}
$$ {#eq-quadratic}

Use @eq-quadratic to solve quadratics.

::: {#def-continuous}
## Continuous Function

A function $f$ is continuous at $c$ if $\lim_{x \to c} f(x) = f(c)$.
:::

See @def-continuous for the definition.

```{#lst-quicksort .python lst-cap="Quicksort Algorithm"}
def quicksort(arr):
    return arr
```

See @lst-quicksort for sorting.
"#,
    );
    write(
        &project_dir.join("app-a.qmd"),
        r#"# Appendix Alpha

::: {#thm-app .theorem name="Appendix Theorem"}
An appendix theorem.
:::

See @thm-app.

![An appendix figure](img.png){#fig-app}

::: {#wrn-app .callout-warning}
## Appendix Warning

An appendix warning. See @wrn-app.
:::

$$
e^{i\pi} + 1 = 0
$$ {#eq-app}

As shown in @eq-app.
"#,
    );
    write(&project_dir.join("img.png"), ""); // replaced below with real bytes
    std::fs::write(project_dir.join("img.png"), ONE_PIXEL_PNG).unwrap();
}

/// Whitespace-collapse the extracted PDF text into a single space-separated
/// string: NBSP → space first (Typst emits U+00A0 between a label kind and
/// its number), then every whitespace run (including pdf_extract's line
/// breaks, which fall at different points than pdftotext's) → single space.
/// Assertions are long, distinctive strings, so cross-line joins are safe.
fn collapsed_pdf_text(output_path: &Path) -> String {
    let raw = pdf_extract::extract_text(output_path)
        .unwrap_or_else(|e| panic!("failed to extract text from {}: {e}", output_path.display()))
        .replace('\u{a0}', " ");
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The reduction, end to end: every construct type Q1's orange-book
/// fixture exercises comes out chapter-scoped in the compiled PDF, with
/// cross-chapter references resolving to the original chapter's number
/// and appendix constructs lettered.
#[test]
fn orange_book_numbering_torture_reduces_q1_smoke_fixture() {
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
    // what reached the compiler (same channel as book_part_appendix's
    // #part[...] check).
    let typ_path = output_path.with_extension("typ");
    let typ_text = std::fs::read_to_string(&typ_path)
        .unwrap_or_else(|e| panic!("expected kept intermediate {}: {e}", typ_path.display()));
    assert!(
        typ_text.contains("#part[Part One]"),
        "expected the unpatched extension's own #part[...] emission in the .typ: {typ_text}"
    );

    let text = collapsed_pdf_text(output_path);

    // --- Chapter one: one of every construct, numbered "1.1". ---
    assert!(
        text.contains("Figure 1.1: A plot of the cars dataset"),
        "chapter one's figure caption: {text}"
    );
    assert!(
        text.contains("Warning 1.1: Whose Tea Is This?"),
        "chapter one's labeled callout: {text}"
    );
    assert!(
        text.contains("Dinosaur 1.1: This is the first dinosaur."),
        "chapter one's custom-kind float: {text}"
    );
    assert!(
        text.contains("Theorem 1.1 (Pythagorean Theorem)"),
        "chapter one's theorem: {text}"
    );
    assert!(
        text.contains("Lemma 1.1 (Triangle Inequality)"),
        "chapter one's lemma: {text}"
    );
    assert!(
        text.contains("Listing 1.1: Hello World in Python"),
        "chapter one's listing caption: {text}"
    );
    assert!(
        text.contains("See Listing 1.1 for a simple example."),
        "chapter one's in-text listing reference must resolve: {text}"
    );
    assert!(
        text.contains("Equation (1.1)"),
        "chapter one's equation reference: {text}"
    );

    // --- Chapter two: counters reset; new construct types. ---
    assert!(
        text.contains("Table 2.1: Sample data table"),
        "chapter two's table resets to 2.1: {text}"
    );
    assert!(
        text.contains("(a) First panel") && text.contains("(b) Second panel"),
        "sub-figure panel sub-captions: {text}"
    );
    assert!(
        text.contains("Figure 2.1a") && text.contains("Figure 2.1b"),
        "individual sub-figure references: {text}"
    );
    assert!(
        text.contains("Figure 2.1: A panel with two sub-figures."),
        "the panel's own caption resets to 2.1: {text}"
    );
    assert!(
        !text.contains("Figure 1.2"),
        "no figure may continue chapter one's sequence: {text}"
    );
    assert!(
        text.contains("Equation (2.1)"),
        "chapter two's equation resets to 2.1: {text}"
    );
    assert!(
        text.contains("Definition 2.1 (Continuous Function)"),
        "chapter two's definition: {text}"
    );
    assert!(
        text.contains("Listing 2.1: Quicksort Algorithm"),
        "chapter two's listing caption resets to 2.1: {text}"
    );
    assert!(
        text.contains("See Listing 2.1 for sorting."),
        "chapter two's in-text listing reference must resolve: {text}"
    );

    // --- Cross-chapter references resolve to the ORIGINAL chapter's
    // number (P3 item 2: "free" per design §6, but asserted). ---
    assert!(
        text.contains("Refer back to Figure 1.1 and Equation (1.1) from Chapter One."),
        "chapter two's references back into chapter one keep their original numbers: {text}"
    );

    // --- Appendix: letter-scoped numbering. ---
    assert!(
        text.contains("A. Appendix Alpha"),
        "the appendix chapter is lettered 'A.': {text}"
    );
    assert!(
        text.contains("Theorem A.1 (Appendix Theorem)"),
        "appendix theorem lettered: {text}"
    );
    assert!(
        !text.contains("Theorem 3.1") && !text.contains("3. Appendix Alpha"),
        "the appendix must not be numbered as a continuing chapter 3: {text}"
    );
    assert!(
        text.contains("Figure A.1: An appendix figure"),
        "appendix figure lettered: {text}"
    );
    assert!(
        text.contains("Warning A.1: Appendix Warning"),
        "appendix callout lettered: {text}"
    );
    assert!(
        text.contains("Equation (A.1)"),
        "appendix equation lettered: {text}"
    );
}
