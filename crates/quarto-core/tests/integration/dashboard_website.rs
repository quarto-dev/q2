//! A dashboard page in a website (dashboard plan
//! `claude-notes/plans/2026-11-03-dashboard-format-port.md`, Phase 9): the
//! site header wraps the dashboard header (Q1 `processNavigation`), the
//! content area is the site's `#quarto-content`, the site footer follows,
//! and the page takes the site's `format.html` theme unless it sets one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use tempfile::TempDir;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn render_website(files: &[(&str, &str)]) -> PathBuf {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    std::mem::forget(temp);
    for (path, contents) in files {
        write_file(&dir.join(path), contents);
    }
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&dir, runtime.as_ref()).unwrap();
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
    let summary = pollster::block_on(pipeline.run()).expect("project render");
    assert!(!summary.has_failures(), "{summary:?}");
    dir
}

const SITE: &str = "project:\n  type: website\nwebsite:\n  title: Site\n  navbar:\n    left:\n      - href: index.qmd\n        text: Home\n      - href: dash.qmd\n        text: Dash\n  page-footer: FOOTER-TEXT\nformat:\n  html:\n    theme: cosmo\n";
const INDEX: &str = "---\ntitle: Home\n---\n\nHome.\n";
const DASH: &str =
    "---\ntitle: Dash\nformat: dashboard\n---\n\n## Row\n\n::: {.card title=\"A\"}\nA.\n:::\n";

/// The text of the theme stylesheet a page links (`quarto-theme-*.css`).
fn theme_css(site: &Path, page: &str) -> String {
    let html = std::fs::read_to_string(site.join("_site").join(page)).unwrap();
    let start = html
        .find("site_libs/quarto/quarto-theme-")
        .expect("theme link");
    let end = start + html[start..].find('"').unwrap();
    std::fs::read_to_string(site.join("_site").join(&html[start..end])).unwrap()
}

#[test]
fn dashboard_page_sits_in_the_site_chrome() {
    let site = render_website(&[
        ("_quarto.yml", SITE),
        ("index.qmd", INDEX),
        ("dash.qmd", DASH),
    ]);
    let html = std::fs::read_to_string(site.join("_site/dash.html")).unwrap();
    let doc = scraper::Html::parse_document(&html);
    let sel = |s: &str| scraper::Selector::parse(s).unwrap();
    assert_eq!(
        doc.select(&sel(
            "header#quarto-header > header#quarto-dashboard-header"
        ))
        .count(),
        1,
        "the dashboard header sits inside the site header"
    );
    assert_eq!(
        doc.select(&sel("header#quarto-header nav.navbar a.nav-link"))
            .count(),
        2
    );
    assert_eq!(
        doc.select(&sel(
            "div#quarto-content.quarto-container.page-navbar.quarto-dashboard-content[role=main]"
        ))
        .count(),
        1
    );
    assert_eq!(doc.select(&sel("#quarto-document-content")).count(), 0);
    let skip = doc.select(&sel("a#quarto-skip-link")).next().unwrap();
    assert_eq!(skip.value().attr("href"), Some("#quarto-content"));
    assert!(html.contains("FOOTER-TEXT"), "site footer");
}

#[test]
fn dashboard_page_takes_the_site_html_theme_unless_it_sets_one() {
    let site = render_website(&[
        ("_quarto.yml", SITE),
        ("index.qmd", INDEX),
        ("dash.qmd", DASH),
        (
            "own.qmd",
            "---\ntitle: Own\nformat:\n  dashboard:\n    theme: darkly\n---\n\n## Row\n\nx\n",
        ),
    ]);
    // cosmo's font stack and darkly's body background mark the themes.
    let dash = theme_css(&site, "dash.html");
    assert!(
        dash.contains("Source Sans Pro"),
        "dash.html should use the site's cosmo theme"
    );
    assert!(
        dash.contains("bslib-sidebar-layout"),
        "with the dashboard layers"
    );
    let own = theme_css(&site, "own.html");
    assert!(!own.contains("Source Sans Pro"), "own.html sets darkly");
    assert!(own.contains("#222"), "darkly");
}

/// A project-level `logo:` serves dashboard pages at any depth (Q1
/// `resolveInputRelativeLogo`): the src is page-relative and the file is
/// copied into the site.
#[test]
fn project_logo_resolves_for_pages_at_any_depth() {
    let site = render_website(&[
        (
            "_quarto.yml",
            "project:\n  type: website\nwebsite:\n  title: S\n  navbar:\n    left:\n      - index.qmd\nformat:\n  dashboard:\n    logo: images/logo.svg\n",
        ),
        ("index.qmd", INDEX),
        (
            "images/logo.svg",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"/>",
        ),
        ("top.qmd", DASH),
        ("sub/dash.qmd", DASH),
    ]);
    let src = |page: &str| {
        let html = std::fs::read_to_string(site.join("_site").join(page)).unwrap();
        let doc = scraper::Html::parse_document(&html);
        let sel = scraper::Selector::parse("img.navbar-logo.light-content").unwrap();
        doc.select(&sel)
            .next()
            .and_then(|e| e.value().attr("src").map(str::to_string))
            .expect("logo img")
    };
    assert_eq!(src("top.html"), "images/logo.svg");
    assert_eq!(src("sub/dash.html"), "../images/logo.svg");
    assert!(site.join("_site/images/logo.svg").is_file(), "logo copied");
}
