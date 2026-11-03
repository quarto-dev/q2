/*
 * tests/integration/dashboard_format.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `format: dashboard` identity (bd-dashboard-format-k21hd4vy, Phase 1).
 */

//! What a dashboard render looks like to the rest of the system before any
//! dashboard layout exists: it renders through the native HTML pipeline to
//! an `.html` file, and Lua sees it the way Quarto 1's filters do —
//! `FORMAT == "html"` (Q1 writes dashboards with pandoc's `html` writer),
//! with `quarto.doc.is_format("dashboard")` true through the base format.
//! Layout itself is asserted by the corpus harness (`dashboard_goldens`).

use std::path::Path;
use std::sync::Arc;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

fn render(dir: &Path, format: &str) -> (std::path::PathBuf, String) {
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&dir.join("doc.qmd"), format, &options, runtime)
        .unwrap_or_else(|e| panic!("{format} render failed: {e:?}"));
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    (result.output_path, html)
}

#[test]
fn dashboard_renders_to_html_through_the_native_pipeline() {
    let temp = tempfile::TempDir::new().unwrap();
    write(
        &temp.path().join("doc.qmd"),
        "---\ntitle: D\nformat: dashboard\n---\n\n## Row\n\nDASH-BODY text.\n",
    );
    let (path, html) = render(temp.path(), "dashboard");
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("html"));
    assert!(html.contains("DASH-BODY"), "{html}");
}

/// A user filter in a dashboard render sees `FORMAT == "html"`, and
/// `is_format` recognizes both `html` and `dashboard` but not `revealjs`.
#[test]
fn dashboard_lua_sees_html_format_and_dashboard_base() {
    let temp = tempfile::TempDir::new().unwrap();
    write(
        &temp.path().join("probe.lua"),
        r#"
function Pandoc(doc)
  local s = "FORMAT=" .. FORMAT
    .. " html=" .. tostring(quarto.doc.is_format("html"))
    .. " dashboard=" .. tostring(quarto.doc.is_format("dashboard"))
    .. " revealjs=" .. tostring(quarto.doc.is_format("revealjs"))
  doc.blocks:insert(pandoc.Para(pandoc.Str(s)))
  return doc
end
"#,
    );
    write(
        &temp.path().join("doc.qmd"),
        "---\ntitle: D\nformat: dashboard\nfilters:\n  - probe.lua\n---\n\nBody.\n",
    );
    let (_, html) = render(temp.path(), "dashboard");
    assert!(
        html.contains("FORMAT=html html=true dashboard=true revealjs=false"),
        "{html}"
    );

    // The same filter under plain html: no dashboard base format.
    let (_, html) = render(temp.path(), "html");
    assert!(
        html.contains("FORMAT=html html=true dashboard=false revealjs=false"),
        "{html}"
    );
}

/// Options under `format: dashboard:` flatten into the document metadata
/// (`MetadataMergeStage` keys the flattening by base format), so the
/// dashboard transforms will see `orientation`, `scrolling`, `nav-buttons`.
#[test]
fn dashboard_format_scoped_options_reach_metadata() {
    let temp = tempfile::TempDir::new().unwrap();
    write(
        &temp.path().join("probe.lua"),
        r#"
function Pandoc(doc)
  doc.blocks:insert(pandoc.Para(pandoc.Str(
    "ORIENTATION=" .. pandoc.utils.stringify(doc.meta.orientation or "unset")
    .. " SCROLLING=" .. tostring(doc.meta.scrolling))))
  return doc
end
"#,
    );
    write(
        &temp.path().join("doc.qmd"),
        "---\ntitle: D\nformat:\n  dashboard:\n    orientation: columns\n    scrolling: true\nfilters:\n  - probe.lua\n---\n\nBody.\n",
    );
    let (_, html) = render(temp.path(), "dashboard");
    assert!(html.contains("ORIENTATION=columns SCROLLING=true"), "{html}");
}

/// The theme stylesheet a page links (the first `<link rel="stylesheet">`
/// whose href mentions `bootstrap` or a theme artifact), read from disk.
fn linked_theme_css(output: &Path, html: &str) -> String {
    let doc = scraper::Html::parse_document(html);
    let sel = scraper::Selector::parse(r#"link[rel="stylesheet"]"#).unwrap();
    let dir = output.parent().unwrap();
    let mut all = String::new();
    for link in doc.select(&sel) {
        let href = link.value().attr("href").unwrap();
        if href.starts_with("http") {
            continue;
        }
        if let Ok(css) = std::fs::read_to_string(dir.join(href)) {
            all.push_str(&css);
        }
    }
    all
}

/// Dashboard pages compile bslib's component SCSS, htmltools' fill rules
/// and the dashboard Sass layer into their theme CSS; plain HTML pages do
/// not (dashboard plan D4).
#[test]
fn dashboard_theme_css_carries_bslib_and_dashboard_layers() {
    let temp = tempfile::TempDir::new().unwrap();
    write(&temp.path().join("doc.qmd"), "---\ntitle: D\n---\n\nBody.\n");
    let (path, html) = render(temp.path(), "dashboard");
    let css = linked_theme_css(&path, &html);
    assert!(css.contains(".bslib-value-box"), "bslib value box rules");
    assert!(css.contains(".html-fill-container"), "htmltools fill rules");
    assert!(css.contains(".quarto-dashboard"), "dashboard layer rules");

    let temp = tempfile::TempDir::new().unwrap();
    write(&temp.path().join("doc.qmd"), "---\ntitle: D\n---\n\nBody.\n");
    let (path, html) = render(temp.path(), "html");
    let css = linked_theme_css(&path, &html);
    assert!(css.contains(".btn"), "sanity: the theme CSS was found");
    assert!(!css.contains(".bslib-value-box"));
    assert!(!css.contains(".quarto-dashboard"));
}

// --- Page shell (Phase 2) ---------------------------------------------------

fn select<'a>(doc: &'a scraper::Html, css: &str) -> Vec<scraper::ElementRef<'a>> {
    let sel = scraper::Selector::parse(css).unwrap_or_else(|e| panic!("{css}: {e:?}"));
    doc.select(&sel).collect()
}

fn one<'a>(doc: &'a scraper::Html, css: &str) -> scraper::ElementRef<'a> {
    let found = select(doc, css);
    assert_eq!(found.len(), 1, "expected exactly one `{css}`, found {}", found.len());
    found[0]
}

fn text(el: scraper::ElementRef<'_>) -> String {
    el.text().collect::<Vec<_>>().join("").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Q1's dashboard page shell (`formats/dashboard/template.html`,
/// `title-block.html`, `_nav-container.html`, plus the postprocessor's
/// container classes, `format-dashboard.ts:322-341`).
#[test]
fn dashboard_page_shell() {
    let temp = tempfile::TempDir::new().unwrap();
    write(
        &temp.path().join("doc.qmd"),
        "---\ntitle: SHELL-TITLE\nsubtitle: SHELL-SUB\nauthor:\n  - Ada One\n  - Bob Two\n---\n\nSHELL-BODY text.\n",
    );
    let (_, html) = render(temp.path(), "dashboard");
    let doc = scraper::Html::parse_document(&html);

    let body = one(&doc, "body");
    let classes: Vec<&str> = body.value().classes().collect();
    assert!(classes.contains(&"quarto-dashboard"), "{classes:?}");
    assert!(classes.contains(&"dashboard-fill"), "{classes:?}");

    one(&doc, "a#quarto-skip-link[href=\"#quarto-document-content\"]");
    one(&doc, "header#quarto-dashboard-header > nav.navbar.navbar-expand-md.slim[data-bs-theme=\"dark\"] > div.navbar-container.container-fluid");
    one(&doc, "div.navbar-container > button.navbar-toggler.hidden[data-bs-target=\"#dashboard-collapse\"]");
    assert_eq!(text(one(&doc, "div.navbar-brand-container div.navbar-title-text a")), "SHELL-TITLE");
    assert_eq!(text(one(&doc, "div.navbar-title div.navbar-subtitle")), "SHELL-SUB");
    // Q1 #10334: several authors, comma-separated.
    assert_eq!(text(one(&doc, "div.navbar-title div.navbar-author")), "Ada One, Bob Two");

    let content = one(
        &doc,
        "div#quarto-document-content.page-layout-custom.quarto-dashboard-content.bslib-gap-spacing.html-fill-container.bslib-page-fill[role=\"main\"][tabindex=\"-1\"]",
    );
    assert!(text(content).contains("SHELL-BODY"));

    // None of the article chrome.
    assert!(select(&doc, "main").is_empty(), "no <main>");
    assert!(select(&doc, "#quarto-content").is_empty());
    assert!(select(&doc, "#title-block-header").is_empty());
}

/// `scrolling: true` lays content out at natural height: no `dashboard-fill`
/// on the body, `dashboard-scrolling` instead of `bslib-page-fill`.
#[test]
fn dashboard_page_shell_scrolling() {
    let temp = tempfile::TempDir::new().unwrap();
    write(
        &temp.path().join("doc.qmd"),
        "---\ntitle: S\nformat:\n  dashboard:\n    scrolling: true\n---\n\nBody.\n",
    );
    let (_, html) = render(temp.path(), "dashboard");
    let doc = scraper::Html::parse_document(&html);
    let body = one(&doc, "body");
    assert!(!body.value().classes().any(|c| c == "dashboard-fill"));
    one(&doc, "div#quarto-document-content.dashboard-scrolling");
    assert!(select(&doc, "div#quarto-document-content.bslib-page-fill").is_empty());
}

/// No title, subtitle, author or logo: the navbar keeps its container but
/// has no brand block (and its markup stays balanced, unlike Q1's
/// `_nav-container.html`, which closes the container itself).
#[test]
fn dashboard_page_shell_without_title() {
    let temp = tempfile::TempDir::new().unwrap();
    write(&temp.path().join("doc.qmd"), "Body only.\n");
    let (_, html) = render(temp.path(), "dashboard");
    let doc = scraper::Html::parse_document(&html);
    one(&doc, "header#quarto-dashboard-header nav.navbar > div.navbar-container");
    assert!(select(&doc, "div.navbar-brand-container").is_empty());
    // The content container is a sibling of the header, not inside it.
    one(&doc, "body > div#quarto-document-content");
}

/// As in Q1, a dashboard's header is the `title-block.html` partial, so a
/// document can replace it with `template-partials`.
#[test]
fn dashboard_title_block_partial_is_overridable() {
    let temp = tempfile::TempDir::new().unwrap();
    write(
        &temp.path().join("title-block.html"),
        "<header id=\"custom-header\">CUSTOM-$title$</header>\n",
    );
    write(
        &temp.path().join("doc.qmd"),
        "---\ntitle: T\ntemplate-partials:\n  - title-block.html\n---\n\nBody.\n",
    );
    let (_, html) = render(temp.path(), "dashboard");
    let doc = scraper::Html::parse_document(&html);
    assert_eq!(text(one(&doc, "header#custom-header")), "CUSTOM-T");
    assert!(select(&doc, "#quarto-dashboard-header").is_empty());
    one(&doc, "div#quarto-document-content.page-layout-custom");
}

/// The dashboard runtime scripts, after the page's other scripts and in
/// Q1's order (`format-dashboard.ts:200-283`): quarto-dashboard.js,
/// stickythead.js, bslib's web components (an ES module) and components.
/// Plain HTML pages ship none of them.
#[test]
fn dashboard_ships_runtime_scripts_in_order() {
    let temp = tempfile::TempDir::new().unwrap();
    write(&temp.path().join("doc.qmd"), "---\ntitle: D\n---\n\nBody.\n");
    let (path, html) = render(temp.path(), "dashboard");
    let doc = scraper::Html::parse_document(&html);
    let srcs: Vec<String> = select(&doc, "script[src]")
        .iter()
        .map(|s| s.value().attr("src").unwrap().to_string())
        .collect();
    let pos = |name: &str| {
        srcs.iter()
            .position(|s| s.ends_with(name))
            .unwrap_or_else(|| panic!("no script {name} in {srcs:?}"))
    };
    let (dash, sticky, web, comp) = (
        pos("/quarto-dashboard.js"),
        pos("/stickythead.js"),
        pos("/web-components.js"),
        pos("/components.js"),
    );
    assert!(dash < sticky && sticky < web && web < comp, "{srcs:?}");
    assert!(pos("bootstrap.bundle.min.js") < dash, "after Bootstrap: {srcs:?}");
    let web_tag = select(&doc, "script[src]")[web];
    assert_eq!(web_tag.value().attr("type"), Some("module"));
    // The files exist where the page points.
    let dir = path.parent().unwrap();
    for s in [&srcs[dash], &srcs[sticky], &srcs[web], &srcs[comp]] {
        assert!(dir.join(s).exists(), "{s} not written next to the page");
    }

    let temp = tempfile::TempDir::new().unwrap();
    write(&temp.path().join("doc.qmd"), "---\ntitle: D\n---\n\nBody.\n");
    let (_, html) = render(temp.path(), "html");
    assert!(!html.contains("quarto-dashboard.js"));
    assert!(!html.contains("web-components.js"));
}
