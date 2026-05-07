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

fn html_for_stem<'a>(summary: &'a ProjectRenderSummary, stem: &str) -> String {
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
