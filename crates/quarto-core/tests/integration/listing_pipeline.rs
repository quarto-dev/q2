/*
 * tests/listing_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for L3 listings end-to-end through
 * `ProjectPipeline`. See
 * `claude-notes/plans/2026-05-06-listings-L3-resolve-transform.md`
 * §"TDD phase 6 — Pipeline wiring + e2e".
 */

//! End-to-end integration tests for listings.
//!
//! Each test writes a fixture project to a temp dir, drives it
//! through the full `ProjectPipeline`, then inspects the rendered
//! HTML for the listing host page. Same shape as the navbar /
//! sidebar pipeline tests.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn html_format() -> Format {
    Format::html()
}

/// Drive a fresh project through the full Pass-1/Pass-2 pipeline.
/// Returns the project dir + the rendered HTML for each output
/// keyed by its file stem.
fn render_project(fixture: impl FnOnce(&std::path::Path)) -> (PathBuf, Vec<(String, String)>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    assert!(
        !project.is_single_file,
        "test expected a multi-file project"
    );

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        html_format(),
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

    std::mem::forget(temp);

    let outputs: Vec<(String, String)> = summary
        .outputs
        .iter()
        .map(|out| {
            let stem = out
                .output_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let html = read(&out.output_path);
            (stem, html)
        })
        .collect();
    (project_dir, outputs)
}

fn html_for<'a>(outputs: &'a [(String, String)], stem: &str) -> &'a str {
    outputs.iter().find(|(s, _)| s == stem).map_or_else(
        || panic!("no output for stem `{}`", stem),
        |(_, h)| h.as_str(),
    )
}

/// L3 phase 6 §"e2e CLI verification" — exact-fixture render on
/// the standard four-file blog setup.
#[test]
fn default_listing_renders_three_posts_in_date_desc_order() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            "---\ntitle: Blog\nlisting: default\nformat: html\n---\n\n# Posts\n",
        );
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: First\ndate: 2026-01-15\nauthor: Alice\ndescription: First desc.\nformat: html\n---\n\nFirst body.\n",
        );
        write(
            &p.join("posts/b.qmd"),
            "---\ntitle: Second\ndate: 2026-02-20\nauthor: Bob\ndescription: Second desc.\nformat: html\n---\n\nSecond body.\n",
        );
        write(
            &p.join("posts/c.qmd"),
            "---\ntitle: Third\ndate: 2026-03-05\nauthor: Carol\ndescription: Third desc.\nformat: html\n---\n\nThird body.\n",
        );
    });

    let host = html_for(&outputs, "index");

    // Listing wrapper Div with the auto-synthesized id and the
    // data-listing-rendered marker.
    assert!(
        host.contains(r#"id="listing-1""#),
        "expected listing wrapper id; got:\n{}",
        host
    );
    assert!(
        host.contains(r#"data-listing-rendered="1""#),
        "expected idempotency marker; got:\n{}",
        host
    );
    // Inner default-listing list.
    assert!(host.contains("quarto-listing-default"));

    // All three post titles appear, with hrefs pointing at the
    // rendered output files (relative to the host page).
    assert!(host.contains(r#"href="a.html""#) || host.contains(r#"href="posts/a.html""#));
    assert!(host.contains(r#"href="b.html""#) || host.contains(r#"href="posts/b.html""#));
    assert!(host.contains(r#"href="c.html""#) || host.contains(r#"href="posts/c.html""#));
    assert!(host.contains("First"));
    assert!(host.contains("Second"));
    assert!(host.contains("Third"));

    // Default sort = date desc → Third (Mar) before Second (Feb)
    // before First (Jan). Find the three title positions and
    // assert the ordering.
    let p_third = host.find("Third").expect("Third missing");
    let p_second = host.find("Second").expect("Second missing");
    let p_first = host.find("First").expect("First missing");
    assert!(
        p_third < p_second && p_second < p_first,
        "expected date-desc ordering (Third, Second, First); positions: third={}, second={}, first={}",
        p_third,
        p_second,
        p_first
    );

    // L7 ran during `WebsiteProjectType::post_render`: the
    // description envelope markers are stripped from the rendered
    // host and the description region is replaced with each post's
    // engine-rendered first paragraph (`"First body."` etc., from
    // the post bodies above).
    assert!(
        !host.contains("desc-begin(5A0113B34292)"),
        "L7 should have stripped description begin markers; got: {host}"
    );
    assert!(
        !host.contains("desc-end(5A0113B34292)"),
        "L7 should have stripped description end markers"
    );
    // Engine-rendered first paragraph from each post's body.
    assert!(
        host.contains("First body.")
            && host.contains("Second body.")
            && host.contains("Third body."),
        "L7 should substitute the engine first paragraphs; got: {host}"
    );

    // Authors and dates flow through.
    assert!(host.contains("Alice"));
    assert!(host.contains("Bob"));
    assert!(host.contains("Carol"));
    // Dates are pre-formatted at record-build with the `medium`
    // default (bd-13f821l5).
    assert!(host.contains("Jan 15, 2026"));
}

/// Listing item links must travel through `LinkRewriteTransform`.
/// The listing render emits `.qmd` source-path links (body-link
/// convention); LinkRewriteTransform then rewrites them to
/// page-relative output URLs via the resolver.
///
/// This is what hub-client's iframe interceptor relies on: in
/// native CLI the page-relative `.html` form lets the browser
/// load the file directly; in the WASM/VFS path the rewrite
/// produces an artifact-rooted URL hub-client reverse-maps back
/// to `.qmd` for in-app navigation. Either way, the listing must
/// not skip the rewrite by emitting raw `output_href` values.
#[test]
fn listing_item_links_are_page_relative_after_link_rewrite() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            "---\ntitle: Blog\nlisting: default\nformat: html\n---\n",
        );
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: First\ndate: 2026-01-15\nformat: html\n---\n\nBody.\n",
        );
        write(
            &p.join("posts/b.qmd"),
            "---\ntitle: Second\ndate: 2026-02-15\nformat: html\n---\n\nBody.\n",
        );
    });

    let host = html_for(&outputs, "index");

    // The host is at posts/index.html and the items are siblings,
    // so the rewriter should produce bare leaf hrefs (e.g.
    // `a.html`), NOT project-relative `posts/a.html`. The latter
    // form is what we get if the listing skipped rewrite by
    // emitting `output_href` directly.
    assert!(
        host.contains(r#"href="a.html""#),
        "expected page-relative `a.html` href (post-LinkRewrite); got:\n{}",
        host
    );
    assert!(
        host.contains(r#"href="b.html""#),
        "expected page-relative `b.html` href (post-LinkRewrite); got:\n{}",
        host
    );

    // The unrewritten form must NOT appear — that would indicate
    // the listing skipped LinkRewriteTransform by emitting raw
    // `output_href` (`posts/a.html`).
    assert!(
        !host.contains(r#"href="posts/a.html""#),
        "found unrewritten `posts/a.html` — listing items skipped LinkRewriteTransform; got:\n{}",
        host
    );
}

#[test]
fn grid_type_emits_grid_classes() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            "---\ntitle: Blog\nlisting:\n  type: grid\nformat: html\n---\n\n# Posts\n",
        );
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: First\ndate: 2026-01-15\nformat: html\n---\n\nBody.\n",
        );
    });

    let host = html_for(&outputs, "index");
    assert!(host.contains("quarto-listing-grid"));
    assert!(host.contains("quarto-grid-item"));
    assert!(host.contains("quarto-listing-cols-3"));
}

#[test]
fn table_type_emits_listing_table_wrapper() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            "---\ntitle: Blog\nlisting:\n  type: table\nformat: html\n---\n",
        );
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: First\ndate: 2026-01-15\nauthor: Alice\nformat: html\n---\n\nBody.\n",
        );
    });

    let host = html_for(&outputs, "index");
    assert!(host.contains("quarto-listing-table-wrapper"));
    // Header row present.
    assert!(host.contains("Title"));
    // Row data present.
    assert!(host.contains("First"));
    assert!(host.contains("Alice"));
    // Dates are pre-formatted at record-build with the `medium`
    // default (bd-13f821l5).
    assert!(host.contains("Jan 15, 2026"));
}

#[test]
fn explicit_slot_div_id_is_filled() {
    // When the host page contains a Div with id matching
    // listing.id, the render transform fills that slot in place
    // rather than appending a fresh wrapper.
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            r#"---
title: Blog
listing:
  id: my-blog
  type: default
format: html
---

# Heading

::: {#my-blog}
:::

# After
"#,
        );
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: First\ndate: 2026-01-15\nformat: html\n---\n\nBody.\n",
        );
    });

    let host = html_for(&outputs, "index");
    // The id should exist exactly once on a Div, with the
    // data-listing-rendered marker, and the listing items live
    // *inside* that Div (between Heading and After in source order).
    assert!(host.contains(r#"id="my-blog""#));
    assert!(host.contains(r#"data-listing-rendered="1""#));
    assert!(host.contains("First"));
    let p_heading = host.find("Heading").expect("Heading missing");
    let p_first = host.find("First").expect("First missing");
    let p_after = host.find("After").expect("After missing");
    assert!(
        p_heading < p_first && p_first < p_after,
        "expected First inside the slot, between Heading and After"
    );
}

#[test]
fn vendored_js_artifacts_emit_script_tags_and_land_under_site_libs() {
    // Phase 7: list.min.js + quarto-listing.js are registered as
    // Project-scoped artifacts when at least one listing is
    // rendered, picked up by `<script>` auto-emission via the
    // `js:` artifact-key prefix, and flushed to
    // `_site/site_libs/listing/<file>.js` by `flush_site_libs`.
    let (project_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            "---\ntitle: Blog\nlisting: default\nformat: html\n---\n",
        );
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: A\ndate: 2026-01-01\nformat: html\n---\n\nBody.\n",
        );
    });

    let host = html_for(&outputs, "index");
    // Both `<script>` tags appear in the rendered HTML, with
    // depth-relative paths (the listing host is at posts/index.html
    // so the path walks up one directory).
    assert!(
        host.contains(r#"src="../site_libs/listing/list.min.js""#)
            || host.contains(r#"src="site_libs/listing/list.min.js""#),
        "expected list.min.js script tag in host HTML; got:\n{}",
        host
    );
    assert!(
        host.contains(r#"src="../site_libs/listing/quarto-listing.js""#)
            || host.contains(r#"src="site_libs/listing/quarto-listing.js""#),
        "expected quarto-listing.js script tag in host HTML; got:\n{}",
        host
    );
    // The flushed bytes land at the resolver-determined location.
    let list_js = project_dir.join("_site/site_libs/listing/list.min.js");
    let quarto_listing_js = project_dir.join("_site/site_libs/listing/quarto-listing.js");
    assert!(
        list_js.exists(),
        "list.min.js missing at {}",
        list_js.display()
    );
    assert!(
        quarto_listing_js.exists(),
        "quarto-listing.js missing at {}",
        quarto_listing_js.display()
    );
    // Sanity: the bytes match what we vendored. list.min.js's
    // first-line marker is the `var List=function...` declaration
    // (or similar) that the third-party file ships with.
    let bytes = std::fs::read(&list_js).unwrap();
    assert!(
        bytes.len() > 1000,
        "list.min.js body unexpectedly small: {} bytes",
        bytes.len()
    );
}

#[test]
fn include_filter_drops_non_matching_items() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            r#"---
title: Blog
listing:
  type: default
  include:
    - author: Alice
format: html
---
"#,
        );
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: First\nauthor: Alice\nformat: html\n---\n\nBody.\n",
        );
        write(
            &p.join("posts/b.qmd"),
            "---\ntitle: Second\nauthor: Bob\nformat: html\n---\n\nBody.\n",
        );
    });

    let host = html_for(&outputs, "index");
    // Alice's post present.
    assert!(host.contains("First"));
    assert!(host.contains("Alice"));
    // Bob's filtered out.
    assert!(
        !host.contains("Second"),
        "Bob's post should be filtered out; got:\n{}",
        host
    );
    assert!(!host.contains("Bob"));
}

/// L5 plan §"Tests" #38 — end-to-end CLI verification that
/// per-item category chips and the right-margin categories sidebar
/// both land in the rendered HTML when the listing has
/// `categories: true`. Drives the same `ProjectPipeline` the CLI's
/// `render` command runs.
#[test]
fn listing_with_categories_renders_chips_and_sidebar_e2e() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            r#"---
title: Blog
toc: true
listing:
  type: default
  categories: true
format: html
---

# Posts
"#,
        );
        // Three posts with overlapping categories; total
        // resolved item count = 3.
        // - rust appears on a, b → count 2
        // - design appears on a → count 1
        // - elm appears on c → count 1
        write(
            &p.join("posts/a.qmd"),
            r#"---
title: First
date: 2026-01-15
categories: [rust, design]
format: html
---

First body.
"#,
        );
        write(
            &p.join("posts/b.qmd"),
            r#"---
title: Second
date: 2026-02-20
categories: [rust]
format: html
---

Second body.
"#,
        );
        write(
            &p.join("posts/c.qmd"),
            r#"---
title: Third
date: 2026-03-05
categories: [elm]
format: html
---

Third body.
"#,
        );
    });

    let host = html_for(&outputs, "index");

    // Per-item category chips: each post's listing entry carries a
    // `<div class="listing-categories">` block with one
    // `<div class="listing-category">` per category.
    let chip_count = host.matches(r#"<div class="listing-category""#).count();
    // a: 2 chips (rust, design), b: 1 (rust), c: 1 (elm) = 4 total.
    assert_eq!(
        chip_count, 4,
        "expected 4 per-item chips across the three posts; got {chip_count}.\nHTML:\n{host}"
    );

    // The sidebar's distinct pills: one per unique category name
    // plus the leading "All" pill in default mode = 4 pills.
    // Locate the sidebar wrapper first to scope the count.
    let sidebar_open = host
        .find(r#"<div class="quarto-listing-category category-default">"#)
        .expect("expected sidebar container in rendered HTML");
    let sidebar_close = host[sidebar_open..]
        .find("</div>\n</div>")
        .map_or(host.len(), |i| sidebar_open + i);
    let sidebar_html = &host[sidebar_open..sidebar_close];
    // 1 All pill + 3 distinct categories = 4 sidebar pills with
    // the `<div class="category"` shape (note: per-item chips use
    // `<div class="listing-category"` so this scope-discriminant
    // is reliable).
    let sidebar_pills = sidebar_html.matches(r#"<div class="category""#).count();
    assert_eq!(
        sidebar_pills, 4,
        "expected 4 sidebar pills (All + 3 categories); got {sidebar_pills}.\nSidebar:\n{sidebar_html}"
    );
    assert!(
        sidebar_html.contains(">All "),
        "expected leading All pill in default mode; got: {sidebar_html}"
    );
    // Counts: rust=2, design=1, elm=1, All=3.
    assert!(sidebar_html.contains(r#"<span class="quarto-category-count">(3)</span>"#));
    assert!(sidebar_html.contains(r#"<span class="quarto-category-count">(2)</span>"#));
    assert_eq!(
        sidebar_html
            .matches(r#"<span class="quarto-category-count">(1)</span>"#)
            .count(),
        2,
        "expected two count(1) pills (design + elm)"
    );

    // Sidebar wrapper id (#quarto-margin-sidebar) is present —
    // confirms the FULL_HTML_TEMPLATE branch is opening the
    // sidebar via the new margin_categories path.
    assert!(
        host.contains(r#"<div id="quarto-margin-sidebar""#),
        "expected #quarto-margin-sidebar wrapper; got:\n{host}"
    );
    // Categories heading present.
    assert!(host.contains(r#"<h5 class="quarto-listing-category-title">Categories</h5>"#));

    // L5 must not perturb L3's artifact-store wiring: the
    // vendored quarto-listing.js script reference still appears
    // in the rendered HTML (the click handler that consumes the
    // markup we emit).
    assert!(
        host.contains("quarto-listing.js"),
        "L5 must not perturb L3's quarto-listing.js artifact wiring; got:\n{host}"
    );
}

// ─────────── L5 snapshot tests #30–33 ──────────────────────────────
//
// Snapshot only the L5-owned subsets of the rendered HTML — the
// per-item chip blocks and the right-margin categories sidebar
// block — not the whole 30+ KB rendered page. Locks the canonical
// emit byte-for-byte while staying robust against unrelated changes
// (theme tweaks, navbar updates, code-highlight CSS, etc.).
//
// Snapshots live at `crates/quarto-core/tests/integration/snapshots/`
// per insta's default convention (the binary name `integration`
// becomes part of the snapshot filename prefix).

/// Extract every `<div class="listing-categories">…</div>` block
/// from the rendered HTML in document order, joined by blank lines.
/// Returns "(no chip blocks)" if none are present.
fn extract_chip_blocks(html: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let needle_open = r#"<div class="listing-categories">"#;
    let mut cursor = 0usize;
    while let Some(rel) = html[cursor..].find(needle_open) {
        let start = cursor + rel;
        // Locate the matching `</div>` closer for the chip block.
        // The block's *internal* `<div class="listing-category">` chips
        // each have their own `</div>`; the wrapper closer is the LAST
        // `</div>` before the next `<div` at the same indentation. Q1's
        // chip block is one line per chip + outer `</div>`, but the
        // doctemplate output reflows whitespace. We match the wrapper
        // by counting `<div` opens and `</div>` closes from the start
        // of the wrapper.
        let mut depth = 0i32;
        let mut pos = start;
        let block_end = loop {
            let next_open = html[pos..].find("<div").map(|i| pos + i);
            let next_close = html[pos..].find("</div>").map(|i| pos + i);
            match (next_open, next_close) {
                (Some(o), Some(c)) if o < c => {
                    depth += 1;
                    pos = o + 4;
                }
                (_, Some(c)) => {
                    depth -= 1;
                    pos = c + 6;
                    if depth == 0 {
                        break c + 6;
                    }
                }
                _ => break html.len(),
            }
        };
        out.push(html[start..block_end].trim_end());
        cursor = block_end;
    }
    if out.is_empty() {
        return "(no chip blocks)".to_string();
    }
    out.join("\n\n")
}

/// Extract the right-margin categories sidebar block — the heading
/// plus the container div. Returns "(no sidebar)" if absent.
///
/// We slice by locating the heading and walking forward through
/// the container's matching closer. Robust to whitespace because
/// we depth-count `<div` / `</div>` tokens.
fn extract_sidebar_block(html: &str) -> String {
    let heading_marker = r#"<h5 class="quarto-listing-category-title">"#;
    let Some(h_start) = html.find(heading_marker) else {
        return "(no sidebar)".to_string();
    };
    // Find the container open right after the heading closer.
    let after_heading = h_start + heading_marker.len();
    let h_close = html[after_heading..]
        .find("</h5>")
        .map_or(html.len(), |i| after_heading + i + "</h5>".len());
    let container_marker = r#"<div class="quarto-listing-category"#;
    let Some(rel) = html[h_close..].find(container_marker) else {
        return html[h_start..h_close].to_string();
    };
    let container_start = h_close + rel;
    // Depth-walk to find the matching `</div>`.
    let mut depth = 0i32;
    let mut pos = container_start;
    let container_end = loop {
        let next_open = html[pos..].find("<div").map(|i| pos + i);
        let next_close = html[pos..].find("</div>").map(|i| pos + i);
        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                pos = o + 4;
            }
            (_, Some(c)) => {
                depth -= 1;
                pos = c + 6;
                if depth == 0 {
                    break c + 6;
                }
            }
            _ => break html.len(),
        }
    };
    html[h_start..container_end].to_string()
}

/// Compose the L5-owned slice of the rendered output: chip blocks,
/// then a separator, then the sidebar. Used as the snapshot input
/// for tests #30–32.
fn l5_owned_slice(html: &str) -> String {
    format!(
        "=== chip blocks ===\n{}\n\n=== sidebar ===\n{}\n",
        extract_chip_blocks(html),
        extract_sidebar_block(html),
    )
}

// L5 plan §"Tests" #30 — default mode, three posts, snapshot the
// chip blocks + sidebar.
#[test]
fn snapshot_builtin_default_with_categories_default_mode() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            r#"---
title: Blog
listing:
  type: default
  categories: true
format: html
---
"#,
        );
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: First\ndate: 2026-01-15\ncategories: [rust, design]\nformat: html\n---\nFirst body.\n",
        );
        write(
            &p.join("posts/b.qmd"),
            "---\ntitle: Second\ndate: 2026-02-20\ncategories: [rust]\nformat: html\n---\nSecond body.\n",
        );
        write(
            &p.join("posts/c.qmd"),
            "---\ntitle: Third\ndate: 2026-03-05\ncategories: [elm]\nformat: html\n---\nThird body.\n",
        );
    });
    let host = html_for(&outputs, "index");
    insta::assert_snapshot!(l5_owned_slice(host));
}

// L5 plan §"Tests" #31 — cloud mode.
#[test]
fn snapshot_builtin_default_with_categories_cloud_mode() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            r#"---
title: Blog
listing:
  type: default
  categories: cloud
format: html
---
"#,
        );
        // Counts skewed so the cloud sizing is visibly different
        // across categories: rust=3 (largest), design=1, elm=1.
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: First\ndate: 2026-01-15\ncategories: [rust, design]\nformat: html\n---\nFirst body.\n",
        );
        write(
            &p.join("posts/b.qmd"),
            "---\ntitle: Second\ndate: 2026-02-20\ncategories: [rust]\nformat: html\n---\nSecond body.\n",
        );
        write(
            &p.join("posts/c.qmd"),
            "---\ntitle: Third\ndate: 2026-03-05\ncategories: [elm, rust]\nformat: html\n---\nThird body.\n",
        );
    });
    let host = html_for(&outputs, "index");
    insta::assert_snapshot!(l5_owned_slice(host));
}

// L5 plan §"Tests" #32 — unnumbered mode.
#[test]
fn snapshot_builtin_default_with_categories_unnumbered_mode() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        write(
            &p.join("posts/index.qmd"),
            r#"---
title: Blog
listing:
  type: default
  categories: unnumbered
format: html
---
"#,
        );
        write(
            &p.join("posts/a.qmd"),
            "---\ntitle: First\ndate: 2026-01-15\ncategories: [rust, design]\nformat: html\n---\nFirst body.\n",
        );
        write(
            &p.join("posts/b.qmd"),
            "---\ntitle: Second\ndate: 2026-02-20\ncategories: [rust]\nformat: html\n---\nSecond body.\n",
        );
        write(
            &p.join("posts/c.qmd"),
            "---\ntitle: Third\ndate: 2026-03-05\ncategories: [elm]\nformat: html\n---\nThird body.\n",
        );
    });
    let host = html_for(&outputs, "index");
    insta::assert_snapshot!(l5_owned_slice(host));
}

// L5 plan §"Tests" #33 — two listings on one page, both with
// `categories: default`, aggregate sidebar should union both
// listings' categories. Snapshots the sidebar only since chips
// are scattered across two listing blocks (each chip block
// itself is covered by tests #30–32 + #38).
#[test]
fn snapshot_page_with_two_listings_aggregates_sidebar() {
    let (_dir, outputs) = render_project(|p| {
        write(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: \"My Site\"\n",
        );
        // Single host page with two listings. Each listing's
        // `contents:` glob points at a different sub-dir so we
        // get a clean partition.
        write(
            &p.join("hub.qmd"),
            r#"---
title: Hub
listing:
  - id: posts
    type: default
    contents: "posts/*.qmd"
    categories: true
  - id: notes
    type: default
    contents: "notes/*.qmd"
    categories: true
format: html
---
"#,
        );
        // Posts: rust + design.
        write(
            &p.join("posts/p1.qmd"),
            "---\ntitle: P1\ndate: 2026-01-01\ncategories: [rust, design]\nformat: html\n---\nBody.\n",
        );
        write(
            &p.join("posts/p2.qmd"),
            "---\ntitle: P2\ndate: 2026-01-02\ncategories: [rust]\nformat: html\n---\nBody.\n",
        );
        // Notes: design + elm. design overlaps with posts; elm is
        // unique to notes; the sidebar should union all three.
        write(
            &p.join("notes/n1.qmd"),
            "---\ntitle: N1\ndate: 2026-02-01\ncategories: [design, elm]\nformat: html\n---\nBody.\n",
        );
        write(
            &p.join("notes/n2.qmd"),
            "---\ntitle: N2\ndate: 2026-02-02\ncategories: [elm]\nformat: html\n---\nBody.\n",
        );
    });
    let host = html_for(&outputs, "hub");
    insta::assert_snapshot!(extract_sidebar_block(host));
}
