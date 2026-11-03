//! Citation resolution in single-document HTML renders (GH #1026).
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
