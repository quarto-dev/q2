/*
 * tests/integration/format_css.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for user-declared stylesheets
 * (`format.html.css` / document `css:`) in project renders:
 * copy into the output tree, per-page href rebasing, the
 * `_extensions/` relocation, and the Q-5-29 missing-file
 * diagnostic. bd-format-css-not-copied-crn3bjdz.
 */

//! End-to-end tests for user-declared CSS in project renders.
//!
//! Each test writes a small fixture to a temp dir, drives it through
//! `ProjectPipeline` (the same path `q2 render <project>` uses), then
//! inspects rendered HTML and the on-disk output tree.
//!
//! What we pin (plan:
//! `claude-notes/plans/2026-08-14-format-css-not-copied.md`):
//!
//! - project-root css is mirrored into the output tree
//!   (`_site/styles.css`), Q1-parity;
//! - `<link>` hrefs are depth-correct per page (`styles.css` at the
//!   root, `../../styles.css` two levels down);
//! - css living under `_extensions/` is relocated to
//!   `<lib_dir>/quarto-contrib/quarto-project/…` so the `_extensions/`
//!   tree itself never ships;
//! - document-front-matter `css:` resolves against the document's own
//!   directory;
//! - a missing declared file emits Q-5-29 and the render completes
//!   with the verbatim link still present (favicon-parity posture);
//! - external URLs pass through untouched;
//! - the default project type (which also serves books) gets the same
//!   copy + rebase treatment — the fix must not be website-only;
//! - revealjs decks link user css (previously dropped entirely).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
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

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Drive a fixture through `ProjectPipeline`. Same harness as
/// `website_post_render.rs`: returns the project dir (kept alive past
/// return) and the full render summary.
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
        summary.pass2_failures
    );

    std::mem::forget(temp);
    (project_dir, summary)
}

/// The stylesheet `<link>` lines of an HTML document, in order.
fn stylesheet_links(html: &str) -> Vec<&str> {
    html.lines()
        .filter(|l| l.contains(r#"rel="stylesheet""#))
        .map(str::trim)
        .collect()
}

/// Assert `html` links `href` as a stylesheet.
fn assert_links_css(html: &str, href: &str, context: &str) {
    let expected = format!(r#"<link rel="stylesheet" href="{href}">"#);
    assert!(
        html.contains(&expected),
        "{context}: expected {expected}; stylesheet links were: {:#?}",
        stylesheet_links(html)
    );
}

const PROJECT_CSS: &str = "body { --test-project-css: 1; }\n";
const EXTENSION_CSS: &str = "body { --test-extension-css: 1; }\n";

/// Standard two-page website fixture declaring `styles.css` at the
/// project root. `deep/deeper/page.qmd` sits two levels down so the
/// rebased href is unambiguous.
fn website_with_root_css(project_dir: &Path) {
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n\
         format:\n  html:\n    css:\n      - styles.css\n",
    );
    write(&project_dir.join("styles.css"), PROJECT_CSS);
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nH.\n",
    );
    write(
        &project_dir.join("deep/deeper/page.qmd"),
        "---\ntitle: Deep\n---\n\nD.\n",
    );
}

// ═══════════════════════════════════════════════════════════════════
// Copy: project-root css is mirrored into the output tree
// ═══════════════════════════════════════════════════════════════════

#[test]
fn website_format_css_copied_to_output_dir() {
    let (project_dir, _summary) = render_project(website_with_root_css);
    let copied = project_dir.join("_site/styles.css");
    assert!(copied.exists(), "styles.css was not copied to _site/");
    assert_eq!(
        read(&copied),
        PROJECT_CSS,
        "copied stylesheet content must match the source"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Rebase: hrefs are depth-correct per page
// ═══════════════════════════════════════════════════════════════════

#[test]
fn website_format_css_href_rebased_per_page() {
    let (project_dir, _summary) = render_project(website_with_root_css);

    let index_html = read(&project_dir.join("_site/index.html"));
    let deep_html = read(&project_dir.join("_site/deep/deeper/page.html"));
    assert_links_css(&index_html, "styles.css", "root page");
    assert_links_css(&deep_html, "../../styles.css", "page two levels down");
    assert!(
        !deep_html.contains(r#"href="styles.css""#),
        "deep page must not keep the verbatim project-relative href; links: {:#?}",
        stylesheet_links(&deep_html)
    );
}

/// User css must come after the theme stylesheet so user rules win the
/// cascade (Q1 ordering).
#[test]
fn website_format_css_linked_after_theme() {
    let (project_dir, _summary) = render_project(website_with_root_css);
    let index_html = read(&project_dir.join("_site/index.html"));
    let links = stylesheet_links(&index_html);
    let theme_pos = links
        .iter()
        .position(|l| l.contains("quarto-theme"))
        .unwrap_or_else(|| panic!("no theme link found: {links:#?}"));
    let user_pos = links
        .iter()
        .position(|l| l.contains("styles.css"))
        .unwrap_or_else(|| panic!("no user css link found: {links:#?}"));
    assert!(
        user_pos > theme_pos,
        "user css must be linked after the theme stylesheet; links: {links:#?}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Relocation: css under _extensions/ ships via quarto-contrib
// ═══════════════════════════════════════════════════════════════════

#[test]
fn website_extension_css_relocated_to_quarto_contrib() {
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             format:\n  html:\n    css:\n      - _extensions/acme/widget/widget.css\n",
        );
        write(
            &project_dir.join("_extensions/acme/widget/widget.css"),
            EXTENSION_CSS,
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("deep/deeper/page.qmd"),
            "---\ntitle: Deep\n---\n\nD.\n",
        );
    });

    let relocated =
        project_dir.join("_site/site_libs/quarto-contrib/quarto-project/acme/widget/widget.css");
    assert!(
        relocated.exists(),
        "extension css was not relocated into site_libs/quarto-contrib/quarto-project/"
    );
    assert_eq!(read(&relocated), EXTENSION_CSS);
    assert!(
        !project_dir.join("_site/_extensions").exists(),
        "the _extensions/ tree must never ship in the output"
    );

    let index_html = read(&project_dir.join("_site/index.html"));
    let deep_html = read(&project_dir.join("_site/deep/deeper/page.html"));
    assert_links_css(
        &index_html,
        "site_libs/quarto-contrib/quarto-project/acme/widget/widget.css",
        "root page",
    );
    assert_links_css(
        &deep_html,
        "../../site_libs/quarto-contrib/quarto-project/acme/widget/widget.css",
        "page two levels down",
    );
}

// ═══════════════════════════════════════════════════════════════════
// Document front matter: css resolves against the document's dir
// ═══════════════════════════════════════════════════════════════════

#[test]
fn document_front_matter_css_resolves_against_document_dir() {
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("deep/deeper/page.qmd"),
            "---\ntitle: Deep\ncss: local.css\n---\n\nD.\n",
        );
        write(&project_dir.join("deep/deeper/local.css"), PROJECT_CSS);
    });

    assert!(
        project_dir.join("_site/deep/deeper/local.css").exists(),
        "document-relative css was not copied to _site/deep/deeper/"
    );
    let deep_html = read(&project_dir.join("_site/deep/deeper/page.html"));
    assert_links_css(&deep_html, "local.css", "declaring page");
    // The stylesheet belongs to that one document only.
    let index_html = read(&project_dir.join("_site/index.html"));
    assert!(
        !index_html.contains("local.css"),
        "front-matter css must not leak onto other pages"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Missing file: Q-5-29, link still emitted, render completes
// ═══════════════════════════════════════════════════════════════════

#[test]
fn missing_format_css_emits_q_5_29_and_continues() {
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             format:\n  html:\n    css:\n      - nope.css\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
    });

    // Favicon-parity posture: emit the (visibly broken) link rather
    // than silently dropping the declaration.
    let index_html = read(&project_dir.join("_site/index.html"));
    assert_links_css(&index_html, "nope.css", "page with missing css");
    assert!(
        !project_dir.join("_site/nope.css").exists(),
        "missing css must not be conjured into the output"
    );

    let diags: Vec<(Option<String>, String)> = summary
        .project_diagnostics
        .iter()
        .map(|d| (d.code.clone(), d.title.clone()))
        .collect();
    assert!(
        diags
            .iter()
            .any(|(code, title)| code.as_deref() == Some("Q-5-29") && title.contains("nope.css")),
        "expected Q-5-29 naming 'nope.css'; got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// External URLs: verbatim on every page, no copy, no diagnostic
// ═══════════════════════════════════════════════════════════════════

#[test]
fn external_url_css_passes_through_verbatim() {
    let url = "https://example.com/x.css";
    let (project_dir, summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            &format!(
                "project:\n  type: website\n  output-dir: _site\n\
                 format:\n  html:\n    css:\n      - \"{url}\"\n"
            ),
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("deep/deeper/page.qmd"),
            "---\ntitle: Deep\n---\n\nD.\n",
        );
    });

    let index_html = read(&project_dir.join("_site/index.html"));
    let deep_html = read(&project_dir.join("_site/deep/deeper/page.html"));
    assert_links_css(&index_html, url, "root page");
    assert_links_css(
        &deep_html,
        url,
        "deep page (must not be made page-relative)",
    );
    assert!(
        summary.project_diagnostics.is_empty(),
        "an external css URL is valid config; got: {:?}",
        summary
            .project_diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════
// Default project type (books' dispatch): same copy + rebase
// ═══════════════════════════════════════════════════════════════════

#[test]
fn default_project_format_css_copied_and_rebased() {
    let (project_dir, _summary) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: default\n  output-dir: _out\n\
             format:\n  html:\n    css:\n      - styles.css\n",
        );
        write(&project_dir.join("styles.css"), PROJECT_CSS);
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nH.\n",
        );
        write(
            &project_dir.join("docs/api.qmd"),
            "---\ntitle: API\n---\n\nA.\n",
        );
    });

    let copied = project_dir.join("_out/styles.css");
    assert!(
        copied.exists(),
        "default project must copy declared css too (books ride this path)"
    );
    assert_eq!(read(&copied), PROJECT_CSS);

    let index_html = read(&project_dir.join("_out/index.html"));
    let api_html = read(&project_dir.join("_out/docs/api.html"));
    assert_links_css(&index_html, "styles.css", "root page (default project)");
    assert_links_css(&api_html, "../styles.css", "nested page (default project)");
}

// ═══════════════════════════════════════════════════════════════════
// Revealjs: user css is linked (previously dropped entirely)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn revealjs_links_user_css() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("talk.qmd");
    write(
        &qmd_path,
        "---\ntitle: Talk\nformat: revealjs\ncss: custom.css\n---\n\n## One\n\nSlide.\n",
    );
    write(&temp.path().join("custom.css"), PROJECT_CSS);

    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let result = render_to_file(&qmd_path, "revealjs", &options, runtime_arc())
        .expect("revealjs render failed");
    let html = read(&result.output_path);
    assert_links_css(&html, "custom.css", "revealjs deck");
}
