/*
 * tests/website_post_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for Phase 7 of the website-projects epic:
 * post-render orchestration (title prefix, favicon, canonical URL,
 * sitemap, robots.txt) end-to-end through `ProjectPipeline`.
 */

//! End-to-end integration tests for Phase 7's per-page transforms
//! and post-render writes.
//!
//! Each test writes a small fixture to a temp dir, drives it
//! through `ProjectPipeline`, then inspects the rendered HTML and
//! the project-level output files (`_site/sitemap.xml`,
//! `_site/robots.txt`, `_site/<favicon>`).
//!
//! See `claude-notes/plans/2026-04-27-websites-phase-7.md` §Tests
//! 30–39.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
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

fn write_bytes(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn html_format() -> Format {
    Format::html()
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Drive a fixture through `ProjectPipeline`. Returns the project
/// directory and the full render summary (so tests can inspect
/// per-page HTML *and* `project_diagnostics`).
fn render_project(fixture: impl FnOnce(&Path)) -> (PathBuf, ProjectRenderSummary) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

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
        summary.pass2_failures
    );

    // Leak the temp dir so the test can inspect files after this
    // function returns (cleanup happens at process exit).
    std::mem::forget(temp);
    (project_dir, summary)
}

fn html_for_stem(summary: &ProjectRenderSummary, stem: &str) -> String {
    let path = summary
        .outputs
        .iter()
        .find(|out| out.output_path.file_stem().and_then(|s| s.to_str()) == Some(stem))
        .unwrap_or_else(|| {
            panic!(
                "no output for stem '{}'; got: {:?}",
                stem,
                summary
                    .outputs
                    .iter()
                    .map(|o| o.output_path.display().to_string())
                    .collect::<Vec<_>>()
            )
        })
        .output_path
        .clone();
    read(&path)
}

// ═══════════════════════════════════════════════════════════════════
// Test 30 — title prefix combines doc + site titles
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_title_prefix_combines_titles() {
    let (_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: Site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Index\n---\n\nHome.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nA.\n",
        );
    });

    let index_html = html_for_stem(&summary, "index");
    let about_html = html_for_stem(&summary, "about");
    assert!(
        index_html.contains("<title>Index – Site</title>"),
        "index <title> not prefixed: {}",
        index_html
            .lines()
            .find(|l| l.contains("<title"))
            .unwrap_or("")
    );
    assert!(
        about_html.contains("<title>About – Site</title>"),
        "about <title> not prefixed"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 31 — favicon <link> emitted per page with correct relative href
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_favicon_link_emitted_per_page() {
    let (_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  favicon: favicon.ico\n",
        );
        // 1×1 transparent placeholder bytes; content is irrelevant.
        write_bytes(&project_dir.join("favicon.ico"), b"\x00\x00\x01\x00");
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("docs/api.qmd"),
            "---\ntitle: API\n---\n\nA.\n",
        );
    });

    let index_html = html_for_stem(&summary, "index");
    let api_html = html_for_stem(&summary, "api");
    assert!(
        index_html.contains(r#"<link rel="icon" href="favicon.ico" type="image/x-icon">"#),
        "index favicon link missing or wrong: {}",
        index_html
            .lines()
            .find(|l| l.contains("rel=\"icon\""))
            .unwrap_or("")
    );
    assert!(
        api_html.contains(r#"<link rel="icon" href="../favicon.ico" type="image/x-icon">"#),
        "nested-page favicon href should be `../favicon.ico`: {}",
        api_html
            .lines()
            .find(|l| l.contains("rel=\"icon\""))
            .unwrap_or("")
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 32 — favicon source file copied to output dir
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_favicon_file_copied_to_output_dir() {
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  favicon: favicon.ico\n",
        );
        write_bytes(&project_dir.join("favicon.ico"), b"\x00\x00\x01\x00");
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });
    assert!(
        project_dir.join("_site/favicon.ico").exists(),
        "favicon was not copied to _site/"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 33 — canonical URL emitted per page
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_canonical_url_per_page() {
    let (_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  site-url: \"https://example.com\"\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("docs/api.qmd"),
            "---\ntitle: API\n---\n\nA.\n",
        );
    });

    let index_html = html_for_stem(&summary, "index");
    let api_html = html_for_stem(&summary, "api");
    assert!(
        index_html.contains(r#"<link rel="canonical" href="https://example.com/index.html">"#),
        "index canonical link wrong: {}",
        index_html
            .lines()
            .find(|l| l.contains("canonical"))
            .unwrap_or("")
    );
    assert!(
        api_html.contains(r#"<link rel="canonical" href="https://example.com/docs/api.html">"#),
        "api canonical link wrong: {}",
        api_html
            .lines()
            .find(|l| l.contains("canonical"))
            .unwrap_or("")
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 34 — sitemap emitted with site-url
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_sitemap_emitted_with_site_url() {
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  site-url: \"https://example.com\"\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nA.\n",
        );
    });
    let sitemap = read(&project_dir.join("_site/sitemap.xml"));
    assert!(sitemap.starts_with("<?xml"), "missing prologue: {sitemap}");
    assert!(
        sitemap.contains("<loc>https://example.com/index.html</loc>"),
        "missing index loc: {sitemap}"
    );
    assert!(
        sitemap.contains("<loc>https://example.com/about.html</loc>"),
        "missing about loc: {sitemap}"
    );
    assert!(
        sitemap.contains("<lastmod>"),
        "expected lastmod entries from real input mtimes"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 35 — sitemap omitted without site-url
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_sitemap_omitted_without_site_url() {
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: Site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });
    assert!(
        !project_dir.join("_site/sitemap.xml").exists(),
        "sitemap should not be written without site-url"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 36 — robots.txt emitted when site-url is set
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_robots_txt_emitted_when_site_url_set() {
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  site-url: \"https://example.com\"\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });
    let robots = read(&project_dir.join("_site/robots.txt"));
    assert_eq!(robots, "Sitemap: https://example.com/sitemap.xml\n");
}

// ═══════════════════════════════════════════════════════════════════
// Test 37 — user's robots.txt takes precedence over auto-generation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_robots_txt_user_file_takes_precedence() {
    let user_body = "User-agent: *\nDisallow: /private\n";
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  site-url: \"https://example.com\"\n",
        );
        write(&project_dir.join("robots.txt"), user_body);
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });
    let robots = read(&project_dir.join("_site/robots.txt"));
    assert_eq!(
        robots, user_body,
        "user robots.txt should be copied verbatim"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Test 38 — missing favicon source: warning + render still completes
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_favicon_missing_diagnoses_continues() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  favicon: missing.ico\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    // The page-level `<link>` is still emitted (we want a visibly
    // broken icon, not a silently-absent one).
    let index_html = html_for_stem(&summary, "index");
    assert!(
        index_html.contains(r#"<link rel="icon" href="missing.ico" type="image/x-icon">"#),
        "expected the link tag even when source is missing"
    );

    // The favicon file is NOT copied.
    assert!(
        !project_dir.join("_site/missing.ico").exists(),
        "missing favicon should not have been written"
    );

    // A warning diagnostic surfaced through the summary.
    assert!(
        summary
            .project_diagnostics
            .iter()
            .any(|d| d.title.contains("missing.ico")),
        "expected a diagnostic mentioning 'missing.ico'; got: {:?}",
        summary
            .project_diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════
// Case A (bd-root-relative-paths-design-fc5pvkcv) — navbar logo copy
// ═══════════════════════════════════════════════════════════════════

/// Decision 5: favicon is not special — a missing navbar logo gets
/// the same warn-and-continue treatment `copy_favicon` pioneered.
/// The render completes, nothing is copied, and a warning diagnostic
/// names the missing file.
#[test]
fn pipeline_navbar_logo_missing_diagnoses_continues() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  navbar:\n    title: Site\n    logo: images/missing-logo.svg\n    left:\n      - index.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    assert!(
        !project_dir.join("_site/images/missing-logo.svg").exists(),
        "missing logo must not be written"
    );
    assert!(
        summary
            .project_diagnostics
            .iter()
            .any(|d| d.title.contains("missing-logo.svg")),
        "expected a diagnostic mentioning 'missing-logo.svg'; got: {:?}",
        summary
            .project_diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
}

/// Decision 5, footer edition — upgraded by
/// bd-page-footer-image-items-stmpikgo Phase 4: a footer text-region
/// image whose file is missing raises the same **Q-5-6** the identical
/// reference would raise in a document body, located at the reference
/// in `_quarto.yml`, and the render continues.
#[test]
fn pipeline_footer_image_missing_diagnoses_continues() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  page-footer:\n    center: \"![](/images/gone.svg)\"\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    assert!(
        !project_dir.join("_site/images/gone.svg").exists(),
        "missing footer image must not be written"
    );
    assert_footer_q_5_6(&summary, "gone.svg");
}

/// Assert exactly the uniform-diagnostic contract of Phase 4
/// (bd-page-footer-image-items-stmpikgo): a Q-5-6 warning whose
/// rendered text names the missing file and which carries a source
/// location (the reference inside `_quarto.yml`).
fn assert_footer_q_5_6(summary: &ProjectRenderSummary, missing: &str) {
    let diag = summary
        .project_diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-5-6"))
        .unwrap_or_else(|| {
            panic!(
                "expected a Q-5-6 diagnostic; got: {:?}",
                summary
                    .project_diagnostics
                    .iter()
                    .map(|d| (d.code.clone(), d.title.clone()))
                    .collect::<Vec<_>>()
            )
        });
    let text = diag.to_text(None);
    assert!(
        text.contains(missing),
        "Q-5-6 must name the missing file `{missing}`; got: {text}"
    );
    assert!(
        diag.location.is_some(),
        "Q-5-6 must carry the reference's source location"
    );
}

/// Phase 4, items edition: an *item's* `text:` image gets the same
/// treatment — missing file raises Q-5-6, present file is copied into
/// the output tree (previously Items regions were skipped entirely).
#[test]
fn pipeline_footer_item_image_missing_raises_q_5_6() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  page-footer:\n    right:\n      - text: \"![](/images/gone.svg)\"\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    assert!(
        !project_dir.join("_site/images/gone.svg").exists(),
        "missing footer item image must not be written"
    );
    assert_footer_q_5_6(&summary, "gone.svg");
}

/// Phase 4, items edition, present-file half: the item image is
/// copied to the output tree like a Text region's.
#[test]
fn pipeline_footer_item_image_present_is_copied() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  page-footer:\n    right:\n      - text: \"![](/images/logo.svg)\"\n",
        );
        std::fs::create_dir_all(project_dir.join("images")).unwrap();
        write(&project_dir.join("images/logo.svg"), "<svg></svg>");
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    assert!(
        project_dir.join("_site/images/logo.svg").exists(),
        "footer item image must be copied into the output tree"
    );
    assert!(
        !summary
            .project_diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-5-6")),
        "no Q-5-6 for a present file; got: {:?}",
        summary
            .project_diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
}

/// Phase 4, blocks edition: an `!md` multi-block region's missing
/// image raises Q-5-6 too (the collector walks `PandocBlocks`).
#[test]
fn pipeline_footer_md_blocks_image_missing_raises_q_5_6() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  page-footer:\n    center: !md |\n      ![](/images/gone.svg)\n\n      second paragraph\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    assert!(
        !project_dir.join("_site/images/gone.svg").exists(),
        "missing footer image must not be written"
    );
    assert_footer_q_5_6(&summary, "gone.svg");
}

// ═══════════════════════════════════════════════════════════════════
// Test 39 — default project: no Phase-7 outputs, no metadata churn
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_default_project_no_phase_7_outputs() {
    // A plain default-project (no website.* config) must not
    // produce sitemap / robots.txt / favicon / canonical-url, and
    // its `<title>` must not be prefixed.
    //
    // Use an explicit `output-dir: _out` so file discovery can
    // distinguish the project from its output (default-project
    // emits beside the project root, which collapses with the
    // default discovery rules and renders zero files).
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: default\n  output-dir: _out\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });
    assert!(
        !summary.outputs.is_empty(),
        "default project should still render its files"
    );
    assert!(
        !project_dir.join("_out/sitemap.xml").exists()
            && !project_dir.join("_site/sitemap.xml").exists(),
        "default project should not emit sitemap.xml"
    );
    assert!(
        !project_dir.join("_out/robots.txt").exists()
            && !project_dir.join("_site/robots.txt").exists(),
        "default project should not emit robots.txt"
    );
    let index_html = html_for_stem(&summary, "index");
    assert!(
        index_html.contains("<title>Home</title>"),
        "default project should not prefix the title; got line: {}",
        index_html
            .lines()
            .find(|l| l.contains("<title"))
            .unwrap_or("")
    );
    assert!(
        !index_html.contains("rel=\"icon\""),
        "default project should not emit a favicon link"
    );
    assert!(
        !index_html.contains("rel=\"canonical\""),
        "default project should not emit a canonical link"
    );
    assert!(summary.project_diagnostics.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// L7 (`bd-qf7r`) — `substitute_listing_placeholders` is wired into
// `WebsiteProjectType::post_render` and runs against the project's
// rendered outputs.
// ═══════════════════════════════════════════════════════════════════

// L7 plan §"Tests" Phase 7 #38
#[test]
fn pipeline_website_post_render_substitutes_listing_placeholders() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  contents: \"posts/*.qmd\"\n  type: default\nformat: html\n---\n\nHome page.\n",
        );
        // Explicit `description:` populates the L1 fallback so the
        // listing template's `$if(description)$` block renders. The
        // body content is what L7 will pull as the engine first
        // paragraph from the rendered sibling.
        write(
            &project_dir.join("posts/foo.qmd"),
            "---\ntitle: Foo\ndate: 2026-01-15\ndescription: Foo L1 fallback.\nformat: html\n---\n\nEngine first paragraph from foo.\n",
        );
        write(
            &project_dir.join("posts/bar.qmd"),
            "---\ntitle: Bar\ndate: 2026-01-10\ndescription: Bar L1 fallback.\nformat: html\n---\n\nEngine first paragraph from bar.\n",
        );
    });

    let host = read(&project_dir.join("_site/index.html"));
    assert!(
        host.contains("Engine first paragraph from foo."),
        "expected foo's engine first paragraph in host; got: {host}"
    );
    assert!(
        host.contains("Engine first paragraph from bar."),
        "expected bar's engine first paragraph in host; got: {host}"
    );
    // L7-substituted preview replaces the L1 fallback; the static
    // text should NOT survive.
    assert!(
        !host.contains("Foo L1 fallback."),
        "L1 fallback should be replaced by engine first paragraph; got: {host}"
    );
    assert!(
        !host.contains("Bar L1 fallback."),
        "L1 fallback should be replaced by engine first paragraph"
    );
    // Both halves of the description envelope must be gone.
    assert!(
        !host.contains("desc-begin(5A0113B34292)"),
        "begin marker must be stripped from rendered host; got first 800 chars:\n{}",
        &host.chars().take(800).collect::<String>()
    );
    assert!(
        !host.contains("desc-end(5A0113B34292)"),
        "end marker must be stripped from rendered host"
    );
    // No Q-12-13 — both posts produced rendered output.
    assert!(
        summary
            .project_diagnostics
            .iter()
            .all(|d| d.code.as_deref() != Some("Q-12-13")),
        "expected no Q-12-13; got: {:?}",
        summary.project_diagnostics
    );
}

// L7 plan §"Tests" Phase 7 #42 — image substitution end-to-end.
//
// The post's body emits the `<img>` via a raw-HTML block so L1's
// `first_image_src` (which looks for Pandoc Image AST nodes) does
// NOT see it and leaves `listing_item.image: None`. The listing
// host emits the image-placeholder envelope; after Pass-2 the
// sibling output's HTML contains a `<img src="preview-image.png">`
// in `main.content`. L7's preview-image extractor picks it up via
// the named-pattern selector (src contains "preview" + ".png")
// and substitutes the envelope.
//
// This avoids needing a real engine: the raw-HTML pathway lets
// us inject the rendered image without depending on jupyter / R
// being installed. The substitution path it exercises is the
// same one engine-rendered images would hit.
#[test]
fn pipeline_website_post_render_substitutes_image_from_sibling_preview() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\nlisting:\n  contents: \"posts/*.qmd\"\n  type: default\nformat: html\n---\n\nHome page.\n",
        );
        // The raw-HTML `{=html}` block emits `<img>` directly.
        // L1's `first_image_src` walks Block::Image AST nodes and
        // sees nothing here (raw blocks don't contain Image
        // inlines), so the listing item's `image:` field stays
        // unset and the host's image-placeholder envelope fires.
        // Body para is needed so the description envelope can
        // substitute too (otherwise the L1 fallback "shows" but
        // L7 sees no <p> to extract).
        write(
            &project_dir.join("posts/with-engine-image.qmd"),
            "---\ntitle: Engine Post\ndate: 2026-01-20\ndescription: Static fallback.\nformat: html\n---\n\nBody paragraph.\n\n```{=html}\n<img src=\"preview-image.png\" alt=\"engine output\">\n```\n",
        );
    });

    let host = read(&project_dir.join("_site/index.html"));
    // The image envelope was substituted with a `<img>` referring
    // to the sibling's preview image, host-relativized.
    assert!(
        host.contains(r#"src="posts/preview-image.png""#),
        "expected substituted img src; got: {host}"
    );
    assert!(
        host.contains(r#"class="thumbnail-image""#),
        "expected thumbnail-image class on substituted img"
    );
    // No image envelope markers survive in the rendered host.
    assert!(
        !host.contains("img-begin(9CEB782EFEE6)"),
        "image begin marker should be stripped; got: {host}"
    );
    assert!(
        !host.contains("img-end(9CEB782EFEE6)"),
        "image end marker should be stripped"
    );
    // No empty placeholder div either (substitution replaced it).
    assert!(
        !host.contains("listing-item-img-placeholder"),
        "empty placeholder should be replaced by substituted img"
    );
    // The description path also substitutes from the body
    // paragraph, not the static fallback.
    assert!(
        host.contains("Body paragraph."),
        "expected engine first paragraph"
    );
    assert!(
        !host.contains("Static fallback."),
        "L1 fallback should be replaced"
    );
    assert!(
        summary
            .project_diagnostics
            .iter()
            .all(|d| d.code.as_deref() != Some("Q-12-13")),
        "no Q-12-13 expected; got: {:?}",
        summary.project_diagnostics
    );
}

// L7 plan §"Tests" Phase 7 #39
#[test]
fn pipeline_default_project_does_not_substitute_listing_placeholders() {
    // L7 should not run on default projects. A default project that
    // happened to emit placeholder-shaped comments in its output
    // would pass through unchanged.
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: default\n  output-dir: _out\n",
        );
        // A bare html comment that resembles a desc-begin marker
        // (the static content in this fixture has no real listing
        // — the comment should never be touched because L7 is
        // never invoked on default projects).
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\n```{=html}\n<!-- desc-begin(5A0113B34292)[max=100]:nope.html -->\n<p>Static fallback.</p>\n<!-- desc-end(5A0113B34292) -->\n```\n",
        );
    });

    let html = read(&project_dir.join("_out/index.html"));
    // Comment must survive verbatim.
    assert!(
        html.contains("desc-begin(5A0113B34292)"),
        "default project must not invoke L7 substitution; markers should survive. Got first 800 chars:\n{}",
        &html.chars().take(800).collect::<String>()
    );
    assert!(
        html.contains("desc-end(5A0113B34292)"),
        "default project must preserve end marker too"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Tests 40–46 — brand-aware favicon fallback (bd-97yc)
//
// Q1 falls back to the brand's *small* logo when `website.favicon` is
// unset (`getFavicon` in `core/brand/brand.ts`, consumed by
// `project/types/website/website.ts:185-205`). These tests pin the Q2
// port. Plan: claude-notes/plans/2026-07-27-brand-aware-favicon-fallback.md
//
// The paths in `_brand.yml` are relative to the **brand file's own
// directory**, so the fallback has to rebase them to project-relative
// form before the existing favicon machinery (page-relative href +
// post-render copy) can consume them. Test 42 is the one that
// distinguishes a correct implementation from one that only works
// because `_brand.yml` happens to sit at the project root.
// ═══════════════════════════════════════════════════════════════════

/// Minimal PNG signature. Content is irrelevant to every assertion
/// here; only the file's existence and extension matter.
const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Assert that `html` contains no favicon `<link>` at all.
fn assert_no_favicon_link(html: &str, context: &str) {
    assert!(
        !html.contains(r#"rel="icon""#),
        "{}: expected no favicon link, found: {}",
        context,
        html.lines()
            .find(|l| l.contains("rel=\"icon\""))
            .unwrap_or("")
    );
}

/// Extract the single favicon `<link>` line, for failure messages.
fn favicon_line(html: &str) -> &str {
    html.lines()
        .find(|l| l.contains("rel=\"icon\""))
        .unwrap_or("<no rel=\"icon\" line>")
}

// ── Test 40 — brand logo.small becomes the favicon ─────────────────

/// `website.favicon` unset + `_brand.yml` with `logo.small` → the
/// small logo is used as the favicon, with a correctly page-relative
/// href on a nested page (mirrors test 31's assertions).
#[test]
fn pipeline_brand_favicon_fallback_link_emitted_per_page() {
    let (_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             brand: _brand.yml\n",
        );
        write(
            &project_dir.join("_brand.yml"),
            "logo:\n  small: logo.png\n",
        );
        write_bytes(&project_dir.join("logo.png"), PNG_BYTES);
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("docs/api.qmd"),
            "---\ntitle: API\n---\n\nA.\n",
        );
    });

    let index_html = html_for_stem(&summary, "index");
    let api_html = html_for_stem(&summary, "api");
    assert!(
        index_html.contains(r#"<link rel="icon" href="logo.png" type="image/png">"#),
        "index should use the brand's small logo as favicon: {}",
        favicon_line(&index_html)
    );
    assert!(
        api_html.contains(r#"<link rel="icon" href="../logo.png" type="image/png">"#),
        "nested-page brand favicon href should be `../logo.png`: {}",
        favicon_line(&api_html)
    );
}

// ── Test 41 — the brand logo file is copied to the output dir ──────

/// The fallback must feed `copy_favicon` too, not just the `<link>`:
/// a favicon that 404s is worse than none.
#[test]
fn pipeline_brand_favicon_fallback_file_copied_to_output_dir() {
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             brand: _brand.yml\n",
        );
        write(
            &project_dir.join("_brand.yml"),
            "logo:\n  small: logo.png\n",
        );
        write_bytes(&project_dir.join("logo.png"), PNG_BYTES);
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });
    assert!(
        project_dir.join("_site/logo.png").exists(),
        "brand logo was not copied to _site/"
    );
}

// ── Test 42 — brand in a subdirectory: paths are brand-relative ────

/// **The load-bearing test.** `_brand.yml` lives in `_brand/`, so its
/// `logo.small: logo.png` names `_brand/logo.png` in project terms.
/// An implementation that forwards the raw YAML path unchanged emits
/// `href="logo.png"` and copies nothing — and passes tests 40/41,
/// where the two forms coincide.
#[test]
fn pipeline_brand_favicon_fallback_rebases_subdirectory_brand() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             brand: _brand/_brand.yml\n",
        );
        write(
            &project_dir.join("_brand/_brand.yml"),
            "logo:\n  small: logo.png\n",
        );
        write_bytes(&project_dir.join("_brand/logo.png"), PNG_BYTES);
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("docs/api.qmd"),
            "---\ntitle: API\n---\n\nA.\n",
        );
    });

    let index_html = html_for_stem(&summary, "index");
    let api_html = html_for_stem(&summary, "api");
    assert!(
        index_html.contains(r#"<link rel="icon" href="_brand/logo.png" type="image/png">"#),
        "brand-relative logo path must be rebased to project-relative: {}",
        favicon_line(&index_html)
    );
    assert!(
        api_html.contains(r#"<link rel="icon" href="../_brand/logo.png" type="image/png">"#),
        "nested-page href for a subdirectory brand should be `../_brand/logo.png`: {}",
        favicon_line(&api_html)
    );
    assert!(
        project_dir.join("_site/_brand/logo.png").exists(),
        "subdirectory brand logo was not copied to _site/_brand/"
    );
}

// ── Test 43 — an explicit website.favicon still wins ───────────────

/// The brand is a *fallback*. When the user names a favicon
/// explicitly it must be used verbatim, and the brand logo must not
/// be copied into the output.
#[test]
fn pipeline_explicit_website_favicon_wins_over_brand_logo() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  favicon: favicon.ico\n\
             brand: _brand.yml\n",
        );
        write(
            &project_dir.join("_brand.yml"),
            "logo:\n  small: logo.png\n",
        );
        write_bytes(&project_dir.join("logo.png"), PNG_BYTES);
        write_bytes(&project_dir.join("favicon.ico"), b"\x00\x00\x01\x00");
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    let index_html = html_for_stem(&summary, "index");
    assert!(
        index_html.contains(r#"<link rel="icon" href="favicon.ico" type="image/x-icon">"#),
        "explicit website.favicon must win over the brand logo: {}",
        favicon_line(&index_html)
    );
    assert!(
        !index_html.contains("logo.png"),
        "brand logo must not appear when website.favicon is set: {}",
        favicon_line(&index_html)
    );
    assert!(
        project_dir.join("_site/favicon.ico").exists(),
        "explicit favicon should still be copied"
    );
    assert!(
        !project_dir.join("_site/logo.png").exists(),
        "brand logo must not be copied when website.favicon is set"
    );
}

// ── Test 44 — light/dark logo pair yields no favicon ───────────────

/// `Brand::favicon()` returns `None` for a `logo.small` light/dark
/// pair by design — picking a side is bd-v5z8w's job. Until then the
/// fallback must decline quietly: no favicon, and no diagnostic
/// (the user has not misconfigured anything).
#[test]
fn pipeline_brand_favicon_light_dark_pair_declines_quietly() {
    let (_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             brand: _brand.yml\n",
        );
        write(
            &project_dir.join("_brand.yml"),
            "logo:\n  small:\n    light: light.png\n    dark: dark.png\n",
        );
        write_bytes(&project_dir.join("light.png"), PNG_BYTES);
        write_bytes(&project_dir.join("dark.png"), PNG_BYTES);
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    let index_html = html_for_stem(&summary, "index");
    assert_no_favicon_link(&index_html, "light/dark logo pair");
    assert!(
        !summary
            .project_diagnostics
            .iter()
            .any(|d| d.title.contains("favicon") || d.title.contains("logo")),
        "a light/dark logo pair is valid config; expected no favicon diagnostic, got: {:?}",
        summary
            .project_diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
}

// ── Test 45 — external logo URL: link only, no copy ────────────────

/// An absolute URL is not a project file. Q1 emits the `<link>` and
/// copies nothing (`isExternalPath` in `core/brand/brand.ts`). The
/// href must survive verbatim — in particular it must *not* be run
/// through the page-relative resolver, which would turn
/// `https://…/logo.png` into `../https:/…/logo.png`.
#[test]
fn pipeline_brand_favicon_external_url_emits_link_without_copy() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             brand: _brand.yml\n",
        );
        write(
            &project_dir.join("_brand.yml"),
            "logo:\n  small: https://example.com/logo.png\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("docs/api.qmd"),
            "---\ntitle: API\n---\n\nA.\n",
        );
    });

    let index_html = html_for_stem(&summary, "index");
    let api_html = html_for_stem(&summary, "api");
    let expected = r#"<link rel="icon" href="https://example.com/logo.png" type="image/png">"#;
    assert!(
        index_html.contains(expected),
        "external brand logo URL must be emitted verbatim: {}",
        favicon_line(&index_html)
    );
    assert!(
        api_html.contains(expected),
        "external URL must not be made page-relative on nested pages: {}",
        favicon_line(&api_html)
    );
    // Nothing resembling the URL should have been written to disk.
    assert!(
        !project_dir.join("_site/https:").exists(),
        "an external favicon URL must not produce a copied file"
    );
    assert!(
        !summary
            .project_diagnostics
            .iter()
            .any(|d| d.title.contains("example.com")),
        "an external favicon URL is valid; expected no diagnostic, got: {:?}",
        summary
            .project_diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
}

// ── Test 46 — no brand key: behavior unchanged ─────────────────────

/// Q2 requires an explicit `brand:` key (there is deliberately no
/// `_brand.yml` auto-discovery — see the plan's Obstacle 3). A
/// `_brand.yml` sitting in the project unreferenced must therefore
/// change nothing.
#[test]
fn pipeline_no_brand_key_emits_no_favicon() {
    let (_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        // Present on disk but never referenced.
        write(
            &project_dir.join("_brand.yml"),
            "logo:\n  small: logo.png\n",
        );
        write_bytes(&project_dir.join("logo.png"), PNG_BYTES);
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    let index_html = html_for_stem(&summary, "index");
    assert_no_favicon_link(&index_html, "unreferenced _brand.yml");
}

// ── Test 48 — a missing brand logo blames the brand, not the key ───

/// The missing-file warning must name the thing the *user wrote*. When
/// the favicon came from the brand fallback there is no
/// `website.favicon` key anywhere in the project, so reporting
/// "website.favicon refers to missing file" would send the reader
/// hunting for a key that doesn't exist.
#[test]
fn pipeline_missing_brand_logo_diagnoses_against_the_brand() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             brand: _brand.yml\n",
        );
        // Names a logo that was never created.
        write(
            &project_dir.join("_brand.yml"),
            "logo:\n  small: gone.png\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    // Same shape as test 38: the link is still emitted, the file is
    // not copied, and the render completes.
    let index_html = html_for_stem(&summary, "index");
    assert!(
        index_html.contains(r#"<link rel="icon" href="gone.png" type="image/png">"#),
        "expected the link tag even when the brand logo is missing: {}",
        favicon_line(&index_html)
    );
    assert!(
        !project_dir.join("_site/gone.png").exists(),
        "missing brand logo should not have been written"
    );

    let titles: Vec<String> = summary
        .project_diagnostics
        .iter()
        .map(|d| d.title.clone())
        .collect();
    assert!(
        titles.iter().any(|t| t.contains("gone.png")),
        "expected a diagnostic naming the missing file; got: {:?}",
        titles
    );
    assert!(
        !titles.iter().any(|t| t.contains("website.favicon")),
        "the project sets no `website.favicon`; blaming that key sends the \
         reader after a key that isn't there. Got: {:?}",
        titles
    );
}

// ── Test 47 — the fallback is website-only ─────────────────────────

/// Every other Phase-7 per-page transform gates itself implicitly, by
/// reading a `website.*` key that a default project simply doesn't
/// have. The brand fallback has no such key to key off — it fires when
/// `website.favicon` is *absent* — so without an explicit project-kind
/// check it would start emitting favicons for default projects that
/// merely use `_brand.yml` for theming. Q1's fallback lives inside the
/// website project type (`website.ts:185-205`) and has the same scope.
///
/// Test 39 covers the default-project case *without* a brand, so it
/// cannot catch this.
#[test]
fn pipeline_default_project_with_brand_emits_no_favicon() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: default\n  output-dir: _out\n\
             brand: _brand.yml\n",
        );
        write(
            &project_dir.join("_brand.yml"),
            "logo:\n  small: logo.png\n",
        );
        write_bytes(&project_dir.join("logo.png"), PNG_BYTES);
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    let index_html = html_for_stem(&summary, "index");
    assert_no_favicon_link(&index_html, "default project with a brand");
    assert!(
        !project_dir.join("_out/logo.png").exists(),
        "default project must not copy a brand logo as a favicon"
    );
}
