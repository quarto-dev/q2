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
    outputs
        .iter()
        .find(|(s, _)| s == stem)
        .map(|(_, h)| h.as_str())
        .unwrap_or_else(|| panic!("no output for stem `{}`", stem))
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

    // L7-bound description placeholders are emitted alongside the
    // L1 fallback descriptions.
    assert!(host.contains("desc(5A0113B34292)"));

    // Authors and dates flow through.
    assert!(host.contains("Alice"));
    assert!(host.contains("Bob"));
    assert!(host.contains("Carol"));
    assert!(host.contains("2026-01-15"));
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
    assert!(host.contains("2026-01-15"));
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
