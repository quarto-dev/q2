/*
 * tests/integration/include_filter_metadata.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * GH #1257: Lua filters writing the inline include keys.
 */

//! A Lua filter that sets `header-includes`, `include-before`, or
//! `include-after` from `Meta` must reach the HTML output, as it does in
//! Pandoc and Quarto 1 (GH #1257).
//!
//! Pandoc builds the writer's template context from the *final*, post-filter
//! metadata. q2 once folded these keys into `rendered.includes.*` before
//! user filters ran, so anything a filter wrote afterward was never read.
//!
//! Every test renders through `render_document_to_file`, the entry point
//! `q2 render` uses, with a real filter file on disk.

use std::path::Path;
use std::sync::Arc;

use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use tempfile::TempDir;

/// Filter that appends one raw-HTML block to the metadata list at `key`,
/// keeping whatever the front matter already put there.
fn appending_filter(key: &str, html: &str) -> String {
    format!(
        r#"function Meta(meta)
  local items = pandoc.List({{}})
  local cur = meta['{key}']
  if cur then
    if pandoc.utils.type(cur) == 'List' then
      items:extend(cur)
    else
      items:insert(cur)
    end
  end
  items:insert(pandoc.MetaBlocks({{ pandoc.RawBlock('html', '{html}') }}))
  meta['{key}'] = items
  return meta
end
"#
    )
}

/// Render `qmd` with the given extra files and return the HTML.
fn render(qmd: &str, files: &[(&str, &str)]) -> String {
    render_to(qmd, files, "html")
}

fn render_to(qmd: &str, files: &[(&str, &str)], format: &str) -> String {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    for (name, content) in files {
        std::fs::write(dir.join(name), content).unwrap();
    }
    let input = dir.join("test.qmd");
    std::fs::write(&input, qmd).unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_document_to_file(
        &input,
        format,
        &RenderToFileOptions::default(),
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("render must succeed");
    read(&result.output_path)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("expected {}: {e}", path.display()))
}

fn head(html: &str) -> &str {
    let start = html.find("<head").expect("<head>");
    let end = html.find("</head>").expect("</head>");
    &html[start..end]
}

fn pos(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?} in:\n{haystack}"))
}

const FM: &str = "---\ntitle: Test\nfilters: [inc.lua]\n---\n\nHi\n";

/// The issue's repro: the filter sets `header-includes` and front matter sets none.
#[test]
fn filter_set_header_includes_reaches_head() {
    let html = render(
        FM,
        &[(
            "inc.lua",
            &appending_filter(
                "header-includes",
                r#"<meta property="og:title" content="t">"#,
            ),
        )],
    );
    assert!(
        head(&html).contains(r#"<meta property="og:title" content="t">"#),
        "filter-set header-includes must reach <head>:\n{html}"
    );
}

/// The filter appends to front matter; both entries must appear, each once,
/// in order.
#[test]
fn filter_append_keeps_front_matter_header_includes() {
    let qmd = "---\n\
        title: Test\n\
        filters: [inc.lua]\n\
        header-includes:\n  - '<meta name=\"from-fm\" content=\"1\">'\n\
        ---\n\nHi\n";
    let html = render(
        qmd,
        &[(
            "inc.lua",
            &appending_filter(
                "header-includes",
                r#"<meta name="from-filter" content="2">"#,
            ),
        )],
    );
    let h = head(&html);
    assert_eq!(h.matches(r#"name="from-fm""#).count(), 1, "{h}");
    assert_eq!(h.matches(r#"name="from-filter""#).count(), 1, "{h}");
    assert!(pos(h, "from-fm") < pos(h, "from-filter"), "{h}");
}

/// A filter that deletes the key removes the authored entry: the final
/// metadata is authoritative.
#[test]
fn filter_clearing_header_includes_removes_authored_entry() {
    let qmd = "---\n\
        title: Test\n\
        filters: [inc.lua]\n\
        header-includes:\n  - '<meta name=\"from-fm\" content=\"1\">'\n\
        ---\n\nHi\n";
    let filter = "function Meta(meta)\n  meta['header-includes'] = nil\n  return meta\nend\n";
    let html = render(qmd, &[("inc.lua", filter)]);
    assert!(!html.contains("from-fm"), "{html}");
}

#[test]
fn filter_set_include_before_and_after_reach_body() {
    let filter = r#"function Meta(meta)
  meta['include-before'] = pandoc.MetaList({
    pandoc.MetaBlocks({ pandoc.RawBlock('html', '<p id="from-before">b</p>') })
  })
  meta['include-after'] = pandoc.MetaList({
    pandoc.MetaBlocks({ pandoc.RawBlock('html', '<p id="from-after">a</p>') })
  })
  return meta
end
"#;
    let html = render(FM, &[("inc.lua", filter)]);
    let body = pos(&html, "<body");
    let before = pos(&html, r#"id="from-before""#);
    let after = pos(&html, r#"id="from-after""#);
    assert!(body < before, "include-before must follow <body>:\n{html}");
    assert!(
        before < after && after < pos(&html, "</body>"),
        "include-after must precede </body>:\n{html}"
    );
}

/// Q1/Pandoc order: `include-in-header` files (variables) come before the
/// inline `header-includes` metadata value.
#[test]
fn file_slot_precedes_inline_header_includes() {
    let qmd = "---\n\
        title: Test\n\
        include-in-header: extra.html\n\
        header-includes:\n  - '<meta name=\"inline\" content=\"1\">'\n\
        ---\n\nHi\n";
    let html = render(
        qmd,
        &[("extra.html", r#"<meta name="from-file" content="1">"#)],
    );
    let h = head(&html);
    assert!(pos(h, "from-file") < pos(h, r#"name="inline""#), "{h}");
}

/// The reveal scaffold bypasses doctemplate and reads `rendered.includes.*`
/// itself, so it is a separate consumer from the HTML template: a filter's
/// `header-includes` and `include-after` must reach a deck too.
#[test]
fn filter_set_includes_reach_revealjs_deck() {
    let qmd = "---\ntitle: Deck\nformat: revealjs\nfilters: [inc.lua]\n---\n\n## S1\n\nx\n";
    let filter = r#"function Meta(meta)
  meta['header-includes'] = pandoc.MetaList({
    pandoc.MetaBlocks({ pandoc.RawBlock('html', '<meta name="deck-head" content="1">') })
  })
  meta['include-after'] = pandoc.MetaList({
    pandoc.MetaBlocks({ pandoc.RawBlock('html', '<p id="deck-after">a</p>') })
  })
  return meta
end
"#;
    let html = render_to(qmd, &[("inc.lua", filter)], "revealjs");
    assert!(
        head(&html).contains(r#"name="deck-head""#),
        "filter-set header-includes must reach the deck <head>:\n{html}"
    );
    assert!(html.contains(r#"id="deck-after""#), "{html}");
}
