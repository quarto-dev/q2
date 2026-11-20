/*
 * tests/integration/revealjs_website.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * RevealJS decks inside a website project: what the site gives a deck
 * and what it must not.
 */

//! End-to-end tests for `format: revealjs` pages in a `type: website`
//! project, compared with Quarto 1.9.38:
//!
//! - the website `page-footer` is site chrome and never becomes a deck's
//!   slide footer (a deck's own `footer:` still does);
//! - slide background images and videos are copied into the output tree;
//! - the deck `<title>` carries the site title, site first
//!   (`Site – Deck`), as Quarto 1's reveal template writes it.
//!
//! Harness mirrors `tests/integration/config_path_consumers.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
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

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Render a website whose `_quarto.yml` is `project: website` +
/// `output-dir: _site` + `config_tail`, with `index.qmd` and the deck
/// `talks/deck.qmd` (front matter `deck_front_matter`, body `deck_body`).
/// Returns the project dir; the temp dir is leaked so the caller can
/// inspect `_site`.
fn render_site(config_tail: &str, deck_front_matter: &str, deck_body: &str) -> PathBuf {
    render_site_with(config_tail, deck_front_matter, deck_body, |_| {})
}

fn render_site_with(
    config_tail: &str,
    deck_front_matter: &str,
    deck_body: &str,
    extra: impl FnOnce(&Path),
) -> PathBuf {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        &format!("project:\n  type: website\n  output-dir: _site\n{config_tail}"),
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );
    write(
        &project_dir.join("talks/deck.qmd"),
        &format!("---\ntitle: Deck\nformat: revealjs\n{deck_front_matter}---\n\n{deck_body}"),
    );
    extra(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::from_format_string("html").unwrap(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected render failures"
    );
    std::mem::forget(temp);
    project_dir
}

const SITE_FOOTER: &str =
    "website:\n  title: \"My Site\"\n  page-footer:\n    center: \"SITE-FOOTER\"\n";

// ═══════════════════════════════════════════════════════════════════
// page-footer
// ═══════════════════════════════════════════════════════════════════

#[test]
fn website_page_footer_does_not_become_the_deck_footer() {
    let dir = render_site(SITE_FOOTER, "", "## A slide\n\nText.\n");
    let deck = read(&dir.join("_site/talks/deck.html"));
    assert!(
        !deck.contains("SITE-FOOTER"),
        "the website page-footer is site chrome; Quarto 1 never shows it on a deck; got:\n{deck}"
    );
    assert!(
        !deck.contains("footer-default"),
        "a deck with no footer of its own has no slide footer; got:\n{deck}"
    );
    // The HTML page still gets the site footer.
    let page = read(&dir.join("_site/index.html"));
    assert!(page.contains("SITE-FOOTER"), "got:\n{page}");
}

#[test]
fn deck_footer_wins_over_the_website_page_footer() {
    let dir = render_site(
        SITE_FOOTER,
        "footer: \"DECK-FOOTER\"\n",
        "## A slide\n\nText.\n",
    );
    let deck = read(&dir.join("_site/talks/deck.html"));
    assert!(
        deck.contains(r#"<div class="footer footer-default">DECK-FOOTER</div>"#),
        "got:\n{deck}"
    );
    assert!(!deck.contains("SITE-FOOTER"), "got:\n{deck}");
}

#[test]
fn directory_metadata_footer_reaches_the_deck() {
    let dir = render_site_with(SITE_FOOTER, "", "## A slide\n\nText.\n", |p| {
        write(
            &p.join("talks/_metadata.yml"),
            "format:\n  revealjs:\n    footer: \"TALKS-FOOTER\"\n",
        );
    });
    let deck = read(&dir.join("_site/talks/deck.html"));
    assert!(
        deck.contains(r#"<div class="footer footer-default">TALKS-FOOTER</div>"#),
        "got:\n{deck}"
    );
    assert!(!deck.contains("SITE-FOOTER"), "got:\n{deck}");
}

#[test]
fn top_level_page_footer_still_reaches_the_deck() {
    // q2's own spelling (top-level `page-footer:`) is feature-scoped, not
    // website-scoped, and keeps working for decks.
    let dir = render_site(
        "page-footer:\n  center: \"TOP-FOOTER\"\n",
        "",
        "## A slide\n\nText.\n",
    );
    let deck = read(&dir.join("_site/talks/deck.html"));
    assert!(
        deck.contains(r#"<div class="footer footer-default">TOP-FOOTER</div>"#),
        "got:\n{deck}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// slide background resources
// ═══════════════════════════════════════════════════════════════════

#[test]
fn slide_background_image_is_copied_into_the_site() {
    let dir = render_site_with(
        "",
        "",
        "## Background {background-image=\"img/bg.png\"}\n\nText.\n",
        |p| write(&p.join("talks/img/bg.png"), "png"),
    );
    let deck = read(&dir.join("_site/talks/deck.html"));
    assert!(
        deck.contains(r#"data-background-image="img/bg.png""#),
        "got:\n{deck}"
    );
    assert!(
        dir.join("_site/talks/img/bg.png").is_file(),
        "the background image is copied beside the deck, as Quarto 1 does"
    );
}

#[test]
fn slide_background_video_sources_are_copied_into_the_site() {
    let dir = render_site_with(
        "",
        "",
        "## Video {background-video=\"media/a.mp4, media/a.webm\"}\n\nText.\n",
        |p| {
            write(&p.join("talks/media/a.mp4"), "mp4");
            write(&p.join("talks/media/a.webm"), "webm");
        },
    );
    assert!(dir.join("_site/talks/media/a.mp4").is_file());
    assert!(
        dir.join("_site/talks/media/a.webm").is_file(),
        "each comma-separated reveal.js video source is copied"
    );
}

// ═══════════════════════════════════════════════════════════════════
// <title>
// ═══════════════════════════════════════════════════════════════════

#[test]
fn deck_title_carries_the_site_title_site_first() {
    let dir = render_site(SITE_FOOTER, "", "## A slide\n\nText.\n");
    let deck = read(&dir.join("_site/talks/deck.html"));
    assert!(
        deck.contains("<title>My Site – Deck</title>"),
        "Quarto 1's reveal template writes `Site – Deck`; got:\n{deck}"
    );
    // Pages keep the page-first order.
    let page = read(&dir.join("_site/index.html"));
    assert!(
        page.contains("<title>Home – My Site</title>"),
        "got:\n{page}"
    );
}

#[test]
fn deck_pagetitle_override_is_used_verbatim() {
    let dir = render_site(
        SITE_FOOTER,
        "pagetitle: \"Custom Tab\"\n",
        "## A slide\n\nText.\n",
    );
    let deck = read(&dir.join("_site/talks/deck.html"));
    assert!(deck.contains("<title>Custom Tab</title>"), "got:\n{deck}");
}
