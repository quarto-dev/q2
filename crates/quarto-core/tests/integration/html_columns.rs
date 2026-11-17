/*
 * html_columns.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `::: {.column width="40%"}` in `format: html`.
 */

//! End-to-end tests for column widths in `format: html`.
//!
//! Pandoc's HTML writer turns `width` on a `div.column` into an inline
//! `width` style, which is how Quarto 1 lays columns out side by side at the
//! author's widths. q2 writes HTML itself, so `HtmlColumnsTransform` does the
//! rewrite. These tests drive the real render path and read the output file.
//!
//! Where the columns land (side by side, 40% and 60% of the body) is CSS and
//! was measured in a browser against Quarto 1; see
//! claude-notes/plans/2026-11-13-layout-gaps-revealjs-absolute-aside-html-columns.md.

use std::path::Path;
use std::sync::Arc;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const COLUMNS_DOC: &str = "\
## Columns

:::: {.columns}

::: {.column width=\"40%\"}
COL-LEFT
:::

::: {.column width=\"60%\"}
COL-RIGHT
:::

::::
";

/// Render [`COLUMNS_DOC`] with the given front matter to `format`, returning
/// the output HTML.
fn render(front_matter: &str, format: &str) -> String {
    let temp = tempfile::TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write(
        &qmd_path,
        &format!("---\n{front_matter}\n---\n\n{COLUMNS_DOC}"),
    );
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&qmd_path, format, &options, runtime)
        .unwrap_or_else(|e| panic!("{format} render failed: {e:?}"));
    std::fs::read_to_string(&result.output_path).unwrap()
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

/// Every `<div …>` opening tag whose class list has the word `column`.
fn column_tags(html: &str) -> Vec<String> {
    html.match_indices("<div class=\"column")
        .filter_map(|(at, _)| {
            let tag = &html[at..at + html[at..].find('>')? + 1];
            (tag.contains("class=\"column\"") || tag.contains("class=\"column "))
                .then(|| tag.to_string())
        })
        .collect()
}

#[test]
fn html_column_width_becomes_inline_width() {
    let html = render("format: html", "html");
    assert_eq!(
        column_tags(&html),
        [
            "<div class=\"column\" style=\"width: 40%;\">",
            "<div class=\"column\" style=\"width: 60%;\">",
        ],
        "each column's `width` must become an inline `width` style and the bare \
         attribute must be gone"
    );
    assert!(
        html.contains("<div class=\"columns\">"),
        "the `.columns` container is kept as written"
    );
    assert!(html.contains("COL-LEFT") && html.contains("COL-RIGHT"));
}

/// RevealJS keeps its own rewrite (`flex-basis`); the HTML transform is not
/// registered for decks.
#[test]
fn revealjs_columns_keep_flex_basis() {
    let html = render("format: revealjs", "revealjs");
    assert_eq!(
        column_tags(&html),
        [
            "<div class=\"column\" style=\"flex-basis: 40%;\">",
            "<div class=\"column\" style=\"flex-basis: 60%;\">",
        ],
    );
}

/// In a dashboard a `.columns` div is a layout container and the dashboard
/// transform sizes its children itself, so the HTML rewrite must not run
/// there: no column may pick up an inline `width: 40%`.
#[test]
fn dashboard_is_not_rewritten() {
    let html = render("format: dashboard", "dashboard");
    assert!(
        html.contains("COL-LEFT") && html.contains("COL-RIGHT"),
        "content survives"
    );
    assert!(
        !html.contains("style=\"width: 40%;\"") && !html.contains("style=\"width: 60%;\""),
        "`format: dashboard` must not get the HTML column rewrite"
    );
}
