/*
 * toc_markup.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-toc-smart-quotes-6nro57ed: TOC entries must carry the heading's
 * inline markup, not a flattened plain-text projection.
 */

//! End-to-end coverage for markup in table-of-contents entries.
//!
//! Before this file there was **no end-to-end TOC test at all**, which is
//! why a visible defect — TOC labels disagreeing with the headings they
//! point at — reached a release. Every test here drives the real CLI
//! render path (`ProjectPipeline` -> `RenderToFileRenderer` ->
//! `render_document_to_file`) against a temp project and inspects the
//! HTML actually written to disk.
//!
//! The expected shapes are Quarto 1's, captured in
//! `claude-notes/plans/toc-smart-quotes-investigation/OBSERVED.md`.
//!
//! Plan: `claude-notes/plans/2026-08-13-toc-smart-quotes.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Build a single-page website project whose `index.qmd` has `toc: true`
/// and the given body, render it through the real pipeline, and return
/// the rendered `index.html`.
///
/// `pub(crate)` so `toc_title_context` can drive the same real render
/// path without cloning ~50 lines of pipeline setup. The two files are
/// sibling modules of the one `integration` binary (see
/// `.claude/rules/integration-tests.md`), so this is an ordinary
/// intra-binary import, not a test depending on another test.
pub(crate) fn render_index_with_toc(
    body: &str,
    extra_frontmatter: &str,
    project_yml: Option<&str>,
) -> String {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());

    write(
        &project_dir.join("_quarto.yml"),
        project_yml.unwrap_or("project:\n  type: website\nwebsite:\n  title: \"TOC test\"\n"),
    );
    write(
        &project_dir.join("index.qmd"),
        &format!("---\ntitle: \"Home\"\ntoc: true\n{extra_frontmatter}---\n\n{body}"),
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).expect("discover");
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );

    let summary = pollster::block_on(pipeline.run()).expect("pipeline run");
    assert!(
        summary.pass1_failures.is_empty(),
        "pass1 failures: {:?}",
        summary
            .pass1_failures
            .iter()
            .map(|f| (&f.input, &f.error))
            .collect::<Vec<_>>()
    );
    assert!(
        summary.pass2_failures.is_empty(),
        "pass2 failures: {:?}",
        summary
            .pass2_failures
            .iter()
            .map(|f| (&f.input, &f.error))
            .collect::<Vec<_>>()
    );

    let out: PathBuf = project.output_dir.clone();
    std::fs::read_to_string(out.join("index.html")).expect("rendered index.html")
}

/// Slice out the `<nav id="TOC">…</nav>` region so assertions cannot
/// accidentally match the document body (which renders the same markup
/// correctly today and would mask a TOC regression).
///
/// `pub(crate)` for the same reason as [`render_index_with_toc`].
pub(crate) fn toc_nav(html: &str) -> String {
    let start = html
        .find("<nav id=\"TOC\"")
        .unwrap_or_else(|| panic!("no <nav id=\"TOC\"> in rendered HTML:\n{html}"));
    let rest = &html[start..];
    let end = rest.find("</nav>").expect("unterminated <nav id=\"TOC\">") + "</nav>".len();
    rest[..end].to_string()
}

/// The inner HTML of each TOC entry anchor, in document order.
fn toc_entry_labels(html: &str) -> Vec<String> {
    let nav = toc_nav(html);
    let mut labels = Vec::new();
    let mut rest = nav.as_str();
    while let Some(open) = rest.find("<a ") {
        let after_open = &rest[open..];
        let Some(gt) = after_open.find('>') else {
            break;
        };
        let body_start = gt + 1;
        let Some(close) = after_open.find("</a>") else {
            break;
        };
        labels.push(after_open[body_start..close].trim().to_string());
        rest = &after_open[close + "</a>".len()..];
    }
    labels
}

// ---------------------------------------------------------------------
// The strand's own case
// ---------------------------------------------------------------------

/// bd-toc-smart-quotes-6nro57ed. A quoted span in a heading renders with
/// curly quotes; its TOC entry must not drop the delimiters.
///
/// Quarto 1: `Using a “raw” volume` in both places.
#[test]
fn toc_entry_keeps_quote_glyphs() {
    let html = render_index_with_toc("## Using a \"raw\" volume\n\nBody.\n", "", None);

    assert!(
        html.contains("<h2>Using a \u{201C}raw\u{201D} volume</h2>"),
        "precondition: the heading itself should already render curly quotes; got:\n{}",
        toc_nav(&html)
    );
    assert_eq!(
        toc_entry_labels(&html),
        vec!["Using a \u{201C}raw\u{201D} volume".to_string()],
        "TOC entry must carry the same quote glyphs as the heading it points at"
    );
}

/// Controls from the strand: an apostrophe and an en dash are
/// `Str`-internal smart-typography rewrites and already survive. If these
/// ever fail, the bug is in the reader, not in the TOC.
#[test]
fn toc_entry_keeps_str_internal_smart_typography() {
    let html = render_index_with_toc(
        "## Finding your repository's identifiers\n\nBody.\n\n## What's in the Gallery -- really\n\nBody.\n",
        "",
        None,
    );

    assert_eq!(
        toc_entry_labels(&html),
        vec![
            "Finding your repository\u{2019}s identifiers".to_string(),
            "What\u{2019}s in the Gallery \u{2013} really".to_string(),
        ]
    );
}

// ---------------------------------------------------------------------
// The wider defect: all inline markup is flattened
// ---------------------------------------------------------------------

/// Quarto 1 renders `Use <code>code</code> and <em>em</em> and
/// <strong>strong</strong>` in the TOC entry. q2 flattened all of it.
#[test]
fn toc_entry_keeps_inline_markup() {
    let html = render_index_with_toc("## Use `code` and *em* and **strong**\n\nBody.\n", "", None);

    assert_eq!(
        toc_entry_labels(&html),
        vec!["Use <code>code</code> and <em>em</em> and <strong>strong</strong>".to_string()]
    );
}

/// Inline math survives into the TOC entry with the same span shape the
/// heading uses.
#[test]
fn toc_entry_keeps_inline_math() {
    let html = render_index_with_toc("## Math $x+y$ inline\n\nBody.\n", "", None);

    let labels = toc_entry_labels(&html);
    assert_eq!(labels.len(), 1, "one heading");
    assert!(
        labels[0].contains("<span class=\"math inline\">"),
        "TOC entry should carry the math span; got {:?}",
        labels[0]
    );
    assert!(
        labels[0].contains("x+y"),
        "TOC entry should carry the math text; got {:?}",
        labels[0]
    );
}

// ---------------------------------------------------------------------
// Render-time stripping (Phase 2 decision)
// ---------------------------------------------------------------------

/// A link inside a heading must contribute its *text* to the TOC entry
/// but not its anchor: the entry is itself an `<a>`, and anchors cannot
/// nest. Pandoc does this with `deLink`; Quarto 1's output for
/// `## Math $x+y$ and a [link](…)` is `… and a link`.
#[test]
fn toc_entry_strips_links_but_keeps_their_text() {
    let html = render_index_with_toc(
        "## See [the docs](https://example.com) now\n\nBody.\n",
        "",
        None,
    );

    let labels = toc_entry_labels(&html);
    assert_eq!(labels, vec!["See the docs now".to_string()]);

    // Belt and braces: no nested anchor anywhere in the nav.
    let nav = toc_nav(&html);
    assert!(
        !nav.contains("https://example.com"),
        "the heading's link href must not appear inside the TOC nav:\n{nav}"
    );
}

/// A footnote in a heading must not drag the footnote reference into the
/// TOC entry. Pandoc strips these with `deNote`.
#[test]
fn toc_entry_drops_footnotes() {
    let html = render_index_with_toc(
        "## Heading with a note^[the note text]\n\nBody.\n",
        "",
        None,
    );

    let labels = toc_entry_labels(&html);
    assert_eq!(labels.len(), 1, "one heading");
    assert!(
        !labels[0].contains("the note text"),
        "footnote body must not appear in the TOC entry; got {:?}",
        labels[0]
    );
    assert!(
        labels[0].starts_with("Heading with a note"),
        "heading text should survive; got {:?}",
        labels[0]
    );
}

/// Angle brackets typed literally in a heading are `Str` content and must
/// still be escaped once the title stops being a plain `String` that
/// `html_escape` handled wholesale.
#[test]
fn toc_entry_escapes_literal_markup_characters() {
    let html = render_index_with_toc("## A \\<b\\> and an & ampersand\n\nBody.\n", "", None);

    let labels = toc_entry_labels(&html);
    assert_eq!(labels.len(), 1, "one heading");
    assert!(
        labels[0].contains("&lt;b&gt;"),
        "literal angle brackets must be escaped, not emitted as a tag; got {:?}",
        labels[0]
    );
    assert!(
        labels[0].contains("&amp;"),
        "literal ampersand must be escaped; got {:?}",
        labels[0]
    );
}

// ---------------------------------------------------------------------
// Phase 3: toc-title
// ---------------------------------------------------------------------

/// `toc-title` in document front matter is parsed as markdown by the
/// metadata layer (`InterpretationContext::DocumentMetadata`), so its
/// markup must reach the rendered `<h2 id="toc-title">`.
#[test]
fn toc_title_from_frontmatter_keeps_markup() {
    let html = render_index_with_toc(
        "## Section\n\nBody.\n",
        "toc-title: \"On **this** page\"\n",
        None,
    );

    let nav = toc_nav(&html);
    assert!(
        nav.contains("<h2 id=\"toc-title\">On <strong>this</strong> page</h2>"),
        "front-matter toc-title markup should survive; got:\n{nav}"
    );
}

/// `toc-title` in `_quarto.yml` is `Scalar(String)` at load time
/// (`InterpretationContext::ProjectConfig` keeps strings literal), so it
/// only gets markdown semantics once `toc-title` is blessed in
/// `MARKDOWN_CONFIG_PATHS`.
#[test]
fn toc_title_from_project_config_keeps_markup() {
    let html = render_index_with_toc(
        "## Section\n\nBody.\n",
        "",
        Some(
            "project:\n  type: website\nwebsite:\n  title: \"TOC test\"\ntoc-title: \"On **this** page\"\n",
        ),
    );

    let nav = toc_nav(&html);
    assert!(
        nav.contains("<h2 id=\"toc-title\">On <strong>this</strong> page</h2>"),
        "project-config toc-title markup should survive once toc-title is blessed in \
         MARKDOWN_CONFIG_PATHS; got:\n{nav}"
    );
}

/// Guard against the `RenderMode` import going unused if the harness is
/// ever refactored to the active-page form.
#[allow(dead_code)]
fn _render_mode_is_available(_m: RenderMode) {}
