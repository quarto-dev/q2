//! Citation resolution in single-document HTML renders (GH #1026), and
//! bibliography placement in the appendix (GH #1221).
//!
//! Quarto 1 runs citeproc whenever a document names a bibliography
//! (`citeMethod()` in `src/command/render/filters.ts`). Q2 used to run it
//! for HTML only when `filters:` listed `citeproc`, so `[@key]` came out as
//! literal `@key` with no reference list and no diagnostic.

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const REFS_BIB: &str = "@article{doe2020,\n  author = {Doe, Jane},\n  title = {A title},\n  journal = {A Journal},\n  year = {2020}\n}\n";

/// Render `body` under `frontmatter` (plus `bibliography: refs.bib`) to
/// HTML and return the output.
fn render_html(frontmatter: &str, body: &str) -> String {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("index.qmd");
    std::fs::write(
        &input_path,
        format!("---\nformat: html\nbibliography: refs.bib\n{frontmatter}---\n\n{body}\n"),
    )
    .unwrap();
    std::fs::write(project_dir.join("refs.bib"), REFS_BIB).unwrap();
    let output_path = project_dir.join("index.html");
    let options = RenderToFileOptions {
        output_path: Some(output_path.clone()),
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    render_document_to_file(
        &input_path,
        "html",
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("html render should succeed");
    read(&output_path)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// The issue's repro: a bibliography alone resolves the citation and
/// builds the reference list.
#[test]
fn bibliography_alone_resolves_html_citations() {
    let html = render_html("", "See [@doe2020].");
    assert!(html.contains("Doe 2020"), "citation not resolved:\n{html}");
    assert!(
        !html.contains(">@doe2020<"),
        "citation left as literal @key:\n{html}"
    );
    assert!(
        html.contains(r#"id="ref-doe2020""#),
        "no reference list entry:\n{html}"
    );
}

/// `citeproc: true` resolves citations on HTML too.
#[test]
fn citeproc_true_resolves_html_citations() {
    let html = render_html("citeproc: true\n", "See [@doe2020].");
    assert!(html.contains("Doe 2020"), "citation not resolved:\n{html}");
    assert!(html.contains(r#"id="ref-doe2020""#), "{html}");
}

/// `citeproc: false` opts out: citations stay unresolved, as in Q1.
#[test]
fn citeproc_false_leaves_html_citations_unresolved() {
    let html = render_html("citeproc: false\n", "See [@doe2020].");
    assert!(html.contains(">@doe2020<"), "{html}");
    assert!(!html.contains(r#"id="ref-doe2020""#), "{html}");
}

/// The default places citeproc after crossref resolution, so a document
/// that mixes `@fig-` references with citations renders both. A
/// pre-crossref citeproc fails with "Reference 'fig-a' not found".
#[test]
fn bibliography_default_runs_after_crossref_resolution() {
    let html = render_html(
        "",
        "::: {#fig-a}\nA\n\nCaption\n:::\n\nSee @fig-a and [@doe2020].",
    );
    assert!(
        html.contains(r##"<a href="#fig-a" class="quarto-xref">"##),
        "crossref not resolved:\n{html}"
    );
    assert!(html.contains("Doe 2020"), "citation not resolved:\n{html}");
}

/// Q1's Bootstrap HTML format defaults `link-citations: true`, so the
/// rendered citation keeps its `citation` span and links to its entry,
/// matching Q1's output for the issue's repro (bd-390vcoox).
#[test]
fn html_citations_link_to_their_entries_by_default() {
    let html = render_html("", "See [@doe2020].");
    assert!(
        html.contains(
            r##"<span class="citation" data-cites="doe2020">(<a href="#ref-doe2020" role="doc-biblioref">Doe 2020</a>)</span>"##
        ),
        "{html}"
    );
}

/// An explicit `link-citations: false` turns the links off; the citation
/// span stays.
#[test]
fn link_citations_false_leaves_html_citations_unlinked() {
    let html = render_html("link-citations: false\n", "See [@doe2020].");
    assert!(
        html.contains(r#"<span class="citation" data-cites="doe2020">(Doe 2020)</span>"#),
        "{html}"
    );
}

/// Minimal HTML (`theme: none`) is not Bootstrap HTML, which is where Q1
/// sets the default, so it keeps Pandoc's `link-citations: false`.
#[test]
fn minimal_html_does_not_link_citations_by_default() {
    let html = render_html("theme: none\n", "See [@doe2020].");
    assert!(!html.contains(r##"href="#ref-doe2020""##), "{html}");
    assert!(html.contains("(Doe 2020)"), "{html}");
}

// ─── Bibliography placement in the appendix (GH #1221) ─────────────────
//
// Quarto 1 moves the bibliography into `#quarto-appendix`, in a
// `#quarto-bibliography` section under a localized "References" heading (or
// `reference-section-title`). Q2's citeproc runs after `appendix-structure`
// for HTML, so `AppendixBibliographyStage` places it.

/// Byte offset of `needle` in `html`, panicking with the document if absent.
fn offset(html: &str, needle: &str) -> usize {
    html.find(needle)
        .unwrap_or_else(|| panic!("`{needle}` not found in:\n{html}"))
}

/// Assert that each needle occurs in `html`, in the given order.
fn assert_in_order(html: &str, needles: &[&str]) {
    let offsets: Vec<usize> = needles.iter().map(|n| offset(html, n)).collect();
    assert!(
        offsets.windows(2).all(|w| w[0] < w[1]),
        "expected {needles:?} in order, found offsets {offsets:?}:\n{html}"
    );
}

/// The appendix's bibliography heading: `#quarto-bibliography`, then an
/// `<h2 class="anchored quarto-appendix-heading" …>` holding `title`, then
/// `#refs`, all inside `#quarto-appendix`.
fn assert_bibliography_in_appendix(html: &str, title: &str) {
    assert_in_order(
        html,
        &[
            r#"id="quarto-appendix""#,
            r#"id="quarto-bibliography""#,
            r#"class="anchored quarto-appendix-heading""#,
            &format!(">{title}</h2>"),
            r#"id="refs""#,
            r#"id="ref-doe2020""#,
        ],
    );
}

/// The issue's English page: the bibliography is in the appendix under
/// "References", not a bare `#refs` after the body.
#[test]
fn bibliography_is_placed_in_appendix_with_references_heading() {
    let html = render_html("", "See [@doe2020].");
    assert_bibliography_in_appendix(&html, "References");
    assert_eq!(html.matches(r#"id="refs""#).count(), 1, "{html}");
}

/// The heading follows the document's `lang`.
#[test]
fn bibliography_heading_is_localized() {
    let html = render_html("lang: fr\n", "Voir [@doe2020].");
    assert_bibliography_in_appendix(&html, "Les références");
}

/// `reference-section-title` overrides the heading.
#[test]
fn reference_section_title_overrides_bibliography_heading() {
    let html = render_html("reference-section-title: Sources\n", "See [@doe2020].");
    assert_bibliography_in_appendix(&html, "Sources");
    assert!(!html.contains(">References</h2>"), "{html}");
}

/// With footnotes, the bibliography goes in the same appendix, before the
/// footnotes, as in Quarto 1. It used to land after the appendix.
#[test]
fn bibliography_precedes_footnotes_in_appendix() {
    let html = render_html("", "See [@doe2020].[^1]\n\n[^1]: A note.");
    assert_eq!(html.matches(r#"id="quarto-appendix""#).count(), 1, "{html}");
    assert_in_order(
        &html,
        &[
            r#"id="quarto-appendix""#,
            r#"id="quarto-bibliography""#,
            r#"id="refs""#,
            r#"id="footnotes""#,
        ],
    );
}

/// `citation-location: margin` keeps the bibliography out of the appendix
/// (Quarto 1's `hasMarginCites`).
#[test]
fn margin_citations_keep_bibliography_out_of_appendix() {
    let html = render_html("citation-location: margin\n", "See [@doe2020].");
    assert!(!html.contains(r#"id="quarto-bibliography""#), "{html}");
}

/// `reference-location: margin` moves footnotes to the margin, not the
/// bibliography: it still goes in the appendix.
#[test]
fn margin_footnotes_still_place_bibliography_in_appendix() {
    let html = render_html("reference-location: margin\n", "See [@doe2020].");
    assert_bibliography_in_appendix(&html, "References");
}

/// `appendix-style: none` turns appendix processing off entirely.
#[test]
fn appendix_style_none_leaves_bibliography_in_place() {
    let html = render_html("appendix-style: none\n", "See [@doe2020].");
    assert!(!html.contains(r#"id="quarto-appendix""#), "{html}");
    assert!(html.contains(r#"id="refs""#), "{html}");
}

/// A `#refs` div the author placed under their own heading stays there,
/// as in Quarto 1 (its `positioned-refs` trait): no appendix copy.
#[test]
fn author_placed_refs_under_heading_stay_in_place() {
    let html = render_html(
        "",
        "See [@doe2020].\n\n## Works cited\n\n::: {#refs}\n:::\n\n## After\n\nMore text.",
    );
    assert!(!html.contains(r#"id="quarto-bibliography""#), "{html}");
    assert_in_order(&html, &[">Works cited</h2>", r#"id="refs""#, ">After</h2>"]);
}
