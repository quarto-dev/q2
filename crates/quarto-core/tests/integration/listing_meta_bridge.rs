/*
 * tests/integration/listing_meta_bridge.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! End-to-end pins for the `meta.listings.<id>` bridge (bd-2oqen84t,
//! M1 + M2): a host page's resolved listings are published into its
//! metadata *before* the lmd cells and the user pre-filters run, so
//! both kinds of Lua code can build a listing from the items.
//!
//! Same harness shape as `listing_pipeline.rs`: write a fixture
//! project to a temp dir, drive the full `ProjectPipeline`, read the
//! rendered host HTML.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Render a fresh website project; return the host page's HTML
/// (`posts/index.html`).
fn render_blog(host_page: &str, extra_files: &[(&str, &str)]) -> String {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
    );
    write(&project_dir.join("posts/index.qmd"), host_page);
    write(
        &project_dir.join("posts/a.qmd"),
        "---\ntitle: First\ndate: 2026-01-15\ncategories: [news]\nformat: html\n---\n\nFirst body.\n",
    );
    write(
        &project_dir.join("posts/b.qmd"),
        "---\ntitle: Second *post*\ndate: 2026-03-05\nformat: html\n---\n\nSecond body.\n",
    );
    for (rel, contents) in extra_files {
        write(&project_dir.join(rel), contents);
    }

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
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
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures,
    );
    let host: PathBuf = project_dir.join("_site/posts/index.html");
    let html =
        std::fs::read_to_string(&host).unwrap_or_else(|e| panic!("read {}: {e}", host.display()));
    std::mem::forget(temp);
    html
}

/// The text between a marker and the end of the document, for
/// ordering assertions inside one element.
fn after<'a>(html: &'a str, marker: &str) -> &'a str {
    let at = html
        .find(marker)
        .unwrap_or_else(|| panic!("`{marker}` missing in:\n{html}"));
    &html[at..]
}

/// A plain Lua filter at `user-filters-pre` reads `doc.meta.listings`:
/// the resolved items are there, in resolved (sorted) order, with
/// `title` as inlines and `path` as a string.
#[test]
fn lua_filter_reads_meta_listings_before_the_transforms() {
    let html = render_blog(
        "---\ntitle: Blog\nlisting:\n  id: posts\n  contents: \"*.qmd\"\n  sort: \"date desc\"\nfilters: [bridge.lua]\nformat: html\n---\n\n# Posts\n",
        &[(
            "posts/bridge.lua",
            r#"
function Pandoc(doc)
  local items = doc.meta.listings.posts.items
  local parts = {}
  for _, it in ipairs(items) do
    parts[#parts + 1] = pandoc.utils.stringify(it.title) .. "=" .. it.path
  end
  doc.blocks:insert(pandoc.Div(
    pandoc.Para(pandoc.Str(table.concat(parts, ";"))),
    pandoc.Attr("", {"bridge"})))
  return doc
end
"#,
        )],
    );
    assert!(
        html.contains("Second post=b.qmd;First=a.qmd"),
        "filter output missing; got:\n{html}"
    );
}

/// The motivating example: a `type: metadata` listing resolves the
/// items and renders nothing; an lmd cell iterates `meta.listings` and
/// builds the markup itself. Item links go through `pandoc.Link` with
/// the host-relative `.qmd` path, which the link rewriter later turns
/// into the output URL, exactly as a doctemplate listing's links are.
#[test]
fn lmd_cell_builds_a_listing_from_meta_listings() {
    let html = render_blog(
        concat!(
            "---\ntitle: Blog\nlisting:\n  id: posts\n  type: metadata\n  contents: \"*.qmd\"\n  sort: \"date desc\"\nformat: html\n---\n\n",
            "```{lmd}\n",
            "local cards = meta.listings.posts.items:map(function(it)\n",
            "  return {=:\n",
            "::: {.card}\n",
            "### `{lmd} pandoc.Link(it.title, it.path)`\n",
            "\n",
            "`{lmd} it.date` · `{lmd} tostring(#it.categories)` categories\n",
            ":::\n",
            ":=}\n",
            "end)\n",
            "return pandoc.Div(cards, pandoc.Attr(\"lmd-posts\"))\n",
            "```\n",
        ),
        &[],
    );
    let div = after(&html, r#"id="lmd-posts""#);
    // Both items, as cards, newest first, linked to the rendered pages.
    let b = div
        .find(r#"href="b.html""#)
        .unwrap_or_else(|| panic!("no link to b.html in:\n{div}"));
    let a = div
        .find(r#"href="a.html""#)
        .unwrap_or_else(|| panic!("no link to a.html in:\n{div}"));
    assert!(b < a, "date desc: Second before First; got:\n{div}");
    // Title markup survives (inlines, not a stringified title).
    assert!(
        div.contains("Second <em>post</em>"),
        "title inlines expected; got:\n{div}"
    );
    // `date` is the display string, categories a real list.
    assert!(div.contains("Mar 5, 2026"), "display date; got:\n{div}");
    assert!(
        div.contains("1 categories"),
        "First has one category; got:\n{div}"
    );
    assert!(div.contains("0 categories"), "Second has none; got:\n{div}");
    // `type: metadata` rendered no listing of its own: no container,
    // no "No matching items" placeholder, no List.js bootstrap.
    assert!(
        !html.contains("quarto-listing"),
        "no built-in container; got:\n{html}"
    );
    assert!(!html.contains("listing-no-matching"));
    assert!(
        !html.contains("list.min.js"),
        "no List.js script; got:\n{html}"
    );
}
