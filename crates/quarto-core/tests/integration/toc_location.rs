/*
 * toc_location.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-e2kpwy7n: `toc-location` option (left/right/body).
 */

//! End-to-end coverage for TOC placement (`toc-location`).
//!
//! Every test drives the real render path — `ProjectPipeline` for
//! website fixtures, `render_to_file` for standalone documents — and
//! inspects the HTML written to disk. The expected shapes are the two
//! Q1 layout regimes mapped in
//! `claude-notes/plans/toc-location-investigation/q1-mechanism-notes.md`:
//!
//! - **Standalone `left`**: `#quarto-content` gains the `toc-left` grid
//!   class and a `div#quarto-sidebar-toc-left.sidebar.toc-left` holds
//!   the TOC (`body` does NOT ride the floating/docked grids).
//! - **Website `left`**: the TOC merges into `nav#quarto-sidebar`
//!   (synthesized with the floating style when no sidebar is
//!   configured) and the body rides the `floating` grid.
//! - **`body`**: the TOC renders inside `main#quarto-document-content`,
//!   keeping q2's decorated markup (deliberate deviation from Q1's
//!   plain list — design decision 4).
//! - **`right` / unset**: unchanged — TOC in `#quarto-margin-sidebar`.
//!
//! Deliberate q2 deviation (design decision 5): when the TOC moves out
//! of the right margin, q2 omits `#quarto-margin-sidebar` entirely
//! instead of emitting Q1's empty `zindex-bottom` shell.
//!
//! Plan: `claude-notes/plans/2026-08-14-toc-location.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_error_reporting::DiagnosticMessage;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Render a website project fixture through the real `ProjectPipeline`
/// and return the rendered `index.html`.
fn render_website(fixture: impl FnOnce(&Path)) -> String {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).expect("discover");
    assert!(!project.is_single_file, "expected a website project");
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
    let summary = pollster::block_on(pipeline.run()).expect("pipeline run");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary
            .pass1_failures
            .iter()
            .map(|f| (&f.input, &f.error))
            .collect::<Vec<_>>(),
        summary
            .pass2_failures
            .iter()
            .map(|f| (&f.input, &f.error))
            .collect::<Vec<_>>()
    );

    let out = project.output_dir.clone();
    std::fs::read_to_string(out.join("index.html")).expect("rendered index.html")
}

/// The single-page website fixture: no configured sidebar, `toc: true`,
/// and the given extra front-matter lines.
fn website_single_page(extra_frontmatter: &str) -> impl FnOnce(&Path) + '_ {
    move |project_dir: &Path| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\nwebsite:\n  title: \"TOC location test\"\n",
        );
        write(
            &project_dir.join("index.qmd"),
            &format!(
                "---\ntitle: \"Home\"\ntoc: true\n{extra_frontmatter}---\n\n\
                 ## Alpha\n\nText.\n\n## Beta\n\nText.\n"
            ),
        );
    }
}

/// Render a standalone document (no `_quarto.yml`) through the real
/// single-file CLI path (`render_to_file`). Returns the rendered HTML
/// and the collected diagnostics.
fn render_standalone(frontmatter: &str) -> (String, Vec<DiagnosticMessage>) {
    let temp = TempDir::new().unwrap();
    let doc_dir = canonical(temp.path());
    let input = doc_dir.join("doc.qmd");
    write(
        &input,
        &format!(
            "---\ntitle: \"Doc\"\ntoc: true\n{frontmatter}---\n\n\
             ## Alpha\n\nText.\n\n## Beta\n\nText.\n"
        ),
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();
    let result = render_to_file(&input, "html", &options, runtime).expect("render");
    let html = std::fs::read_to_string(&result.output_path).expect("rendered doc.html");
    (html, result.render_output.diagnostics)
}

// ---------------------------------------------------------------------
// HTML region helpers
// ---------------------------------------------------------------------

/// Extract the element region starting at `open_needle`, balancing
/// nested `<tag`/`</tag>` pairs so containers holding a nested element
/// of the same tag (e.g. `nav#quarto-sidebar` around `nav#TOC`) close
/// at the right spot.
fn balanced_region(html: &str, open_needle: &str, tag: &str) -> String {
    let start = html
        .find(open_needle)
        .unwrap_or_else(|| panic!("no `{open_needle}` in rendered HTML:\n{html}"));
    let open_tag = format!("<{tag}");
    let close_tag = format!("</{tag}>");
    let mut depth = 0usize;
    let mut pos = start;
    loop {
        let next_open = html[pos + 1..].find(&open_tag).map(|i| pos + 1 + i);
        let next_close = html[pos + 1..].find(&close_tag).map(|i| pos + 1 + i);
        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                pos = o;
            }
            (_, Some(c)) => {
                if depth == 0 {
                    return html[start..c + close_tag.len()].to_string();
                }
                depth -= 1;
                pos = c;
            }
            _ => panic!("unterminated `{open_needle}` region:\n{html}"),
        }
    }
}

/// The class attribute of the first tag matching `open_needle`.
fn class_attr(html: &str, open_needle: &str) -> String {
    let start = html
        .find(open_needle)
        .unwrap_or_else(|| panic!("no `{open_needle}` in rendered HTML:\n{html}"));
    let tag_end = html[start..].find('>').expect("unterminated tag") + start;
    let tag = &html[start..tag_end];
    let class_start = tag.find("class=\"").map(|i| i + "class=\"".len());
    match class_start {
        Some(cs) => {
            let rest = &tag[cs..];
            let end = rest.find('"').expect("unterminated class attribute");
            rest[..end].to_string()
        }
        None => String::new(),
    }
}

fn has_class(html: &str, open_needle: &str, class: &str) -> bool {
    class_attr(html, open_needle)
        .split_whitespace()
        .any(|c| c == class)
}

fn body_classes(html: &str) -> String {
    class_attr(html, "<body")
}

fn diagnostics_mention(diags: &[DiagnosticMessage], needle: &str) -> bool {
    diags.iter().any(|d| format!("{d:?}").contains(needle))
}

// ---------------------------------------------------------------------
// Website `left`
// ---------------------------------------------------------------------

/// The Connect-docs repro shape: a website page with `toc-location:
/// left` and no configured sidebar gets a synthesized floating
/// `nav#quarto-sidebar` holding the TOC; the right margin sidebar is
/// omitted entirely (decision 5).
#[test]
fn website_left_without_sidebar_synthesizes_floating_sidebar() {
    let html = render_website(website_single_page("toc-location: left\n"));

    let sidebar = balanced_region(&html, "<nav id=\"quarto-sidebar\"", "nav");
    assert!(
        sidebar.contains("<nav id=\"TOC\""),
        "the TOC must live inside nav#quarto-sidebar; sidebar region:\n{sidebar}"
    );
    assert!(
        has_class(&html, "<nav id=\"quarto-sidebar\"", "sidebar-floating"),
        "a synthesized sidebar uses the floating style; got classes: {:?}",
        class_attr(&html, "<nav id=\"quarto-sidebar\"")
    );
    assert!(
        body_classes(&html)
            .split_whitespace()
            .any(|c| c == "floating"),
        "the body must ride the floating grid; got: {:?}",
        body_classes(&html)
    );
    assert!(
        !html.contains("id=\"quarto-margin-sidebar\""),
        "no empty right-margin shell when the TOC moves left (decision 5)"
    );
}

/// A configured website sidebar and `toc-location: left` share one
/// `nav#quarto-sidebar`: nav items first, TOC appended after (Q1's
/// sidebar.ejs merge order).
#[test]
fn website_left_with_sidebar_merges_toc_after_nav_items() {
    let html = render_website(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\nwebsite:\n  title: \"TOC location test\"\n  \
             sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: \"Home\"\ntoc: true\ntoc-location: left\n---\n\n\
             ## Alpha\n\nText.\n\n## Beta\n\nText.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: \"About\"\n---\n\nAbout.\n",
        );
    });

    assert_eq!(
        html.matches("id=\"quarto-sidebar\"").count(),
        1,
        "exactly one sidebar container — the TOC merges, it does not add a second"
    );
    let sidebar = balanced_region(&html, "<nav id=\"quarto-sidebar\"", "nav");
    let nav_item = sidebar
        .find("about.html")
        .expect("sidebar nav items must render before the TOC");
    let toc = sidebar
        .find("<nav id=\"TOC\"")
        .expect("the TOC must live inside nav#quarto-sidebar");
    assert!(
        nav_item < toc,
        "nav items come first, TOC after (Q1 merge order); sidebar region:\n{sidebar}"
    );
    assert!(
        !html.contains("id=\"quarto-margin-sidebar\""),
        "no empty right-margin shell when the TOC moves left (decision 5)"
    );
}

// ---------------------------------------------------------------------
// Standalone `left`
// ---------------------------------------------------------------------

/// A standalone document with `toc-location: left` uses the toc-left
/// grid: `toc-left` on `#quarto-content`, the TOC inside
/// `div#quarto-sidebar-toc-left.sidebar.toc-left`, and a body that does
/// NOT ride the floating/docked grids (the `.page-columns.toc-left`
/// SCSS is gated on `body:not(.floating):not(.docked)`).
#[test]
fn standalone_left_uses_toc_left_grid() {
    let (html, _) = render_standalone("toc-location: left\n");

    assert!(
        has_class(&html, "<div id=\"quarto-content\"", "toc-left"),
        "#quarto-content must carry the toc-left grid class; got: {:?}",
        class_attr(&html, "<div id=\"quarto-content\"")
    );
    let container = balanced_region(&html, "<div id=\"quarto-sidebar-toc-left\"", "div");
    assert!(
        container.contains("<nav id=\"TOC\""),
        "the TOC must live inside #quarto-sidebar-toc-left; region:\n{container}"
    );
    assert!(
        has_class(&html, "<div id=\"quarto-sidebar-toc-left\"", "sidebar")
            && has_class(&html, "<div id=\"quarto-sidebar-toc-left\"", "toc-left"),
        "the container needs .sidebar.toc-left for the grid placement rules; got: {:?}",
        class_attr(&html, "<div id=\"quarto-sidebar-toc-left\"")
    );
    let body = body_classes(&html);
    assert!(
        !body
            .split_whitespace()
            .any(|c| c == "floating" || c == "docked"),
        "standalone left must not ride the website sidebar grids; body classes: {body:?}"
    );
    assert!(
        !html.contains("id=\"quarto-margin-sidebar\""),
        "no empty right-margin shell when the TOC moves left (decision 5)"
    );
}

// ---------------------------------------------------------------------
// `body`
// ---------------------------------------------------------------------

/// `toc-location: body` renders the TOC inside the main content
/// column. q2 keeps the decorated markup (`data-scroll-target`) —
/// deliberate deviation from Q1's plain list (decision 4: scroll-spy
/// support is coming and the attributes are inert without it).
#[test]
fn body_location_renders_toc_in_main() {
    let (html, _) = render_standalone("toc-location: body\n");

    let main = balanced_region(&html, "<main class=", "main");
    assert!(
        main.contains("<nav id=\"TOC\""),
        "the TOC must render inside <main>; main region:\n{main}"
    );
    assert!(
        main.contains("data-scroll-target="),
        "body TOC keeps q2's decorated markup (decision 4)"
    );
    assert!(
        !html.contains("id=\"quarto-margin-sidebar\""),
        "no right-margin TOC when location is body"
    );
    assert!(
        !html.contains("id=\"quarto-sidebar-toc-left\"") && !html.contains("id=\"quarto-sidebar\""),
        "no sidebar containers when location is body"
    );
}

// ---------------------------------------------------------------------
// `right` / default
// ---------------------------------------------------------------------

/// Explicit `toc-location: right` must be exactly the default
/// placement — the option landing must not perturb the existing shape.
#[test]
fn right_explicit_matches_default_placement() {
    let (default_html, _) = render_standalone("");
    let (right_html, _) = render_standalone("toc-location: right\n");

    let default_margin = balanced_region(&default_html, "<div id=\"quarto-margin-sidebar\"", "div");
    let right_margin = balanced_region(&right_html, "<div id=\"quarto-margin-sidebar\"", "div");
    assert!(
        default_margin.contains("<nav id=\"TOC\""),
        "default: TOC in the right margin sidebar"
    );
    assert_eq!(
        default_margin, right_margin,
        "explicit `right` must be byte-identical to the default margin region"
    );
}

// ---------------------------------------------------------------------
// Banner interaction
// ---------------------------------------------------------------------

/// Banner mode + `toc-location: left` fires the (previously inert)
/// `banner-header-class` hook: `#title-block-header` carries `toc-left`
/// so the relocated banner header lines up with the shifted body
/// column.
#[test]
fn banner_left_adds_toc_left_header_class() {
    let (html, _) = render_standalone("title-block-banner: true\ntoc-location: left\n");

    assert!(
        has_class(&html, "<header id=\"title-block-header\"", "toc-left"),
        "banner header must carry toc-left; got classes: {:?}",
        class_attr(&html, "<header id=\"title-block-header\"")
    );
}

/// Control: banner mode without `toc-location: left` must NOT get the
/// class.
#[test]
fn banner_without_left_has_no_toc_left_header_class() {
    let (html, _) = render_standalone("title-block-banner: true\n");

    assert!(
        !has_class(&html, "<header id=\"title-block-header\"", "toc-left"),
        "banner header must not carry toc-left when the TOC is on the right; got: {:?}",
        class_attr(&html, "<header id=\"title-block-header\"")
    );
}

// ---------------------------------------------------------------------
// `*-body` fallback and unknown values
// ---------------------------------------------------------------------

/// `left-body` warns and falls back to `left` (the body clone is
/// bd-jclcm0in).
#[test]
fn left_body_warns_and_falls_back_to_left() {
    let (html, diags) = render_standalone("toc-location: left-body\n");

    assert!(
        diagnostics_mention(&diags, "toc-location"),
        "a fallback warning naming toc-location must be emitted; got: {diags:?}"
    );
    let container = balanced_region(&html, "<div id=\"quarto-sidebar-toc-left\"", "div");
    assert!(
        container.contains("<nav id=\"TOC\""),
        "left-body places like left until bd-jclcm0in lands"
    );
}

/// `right-body` warns and falls back to `right`.
#[test]
fn right_body_warns_and_falls_back_to_right() {
    let (html, diags) = render_standalone("toc-location: right-body\n");

    assert!(
        diagnostics_mention(&diags, "toc-location"),
        "a fallback warning naming toc-location must be emitted; got: {diags:?}"
    );
    let margin = balanced_region(&html, "<div id=\"quarto-margin-sidebar\"", "div");
    assert!(
        margin.contains("<nav id=\"TOC\""),
        "right-body places like right until bd-jclcm0in lands"
    );
}

/// An unknown value warns and defaults to `right`.
#[test]
fn unknown_location_warns_and_defaults_to_right() {
    let (html, diags) = render_standalone("toc-location: sideways\n");

    assert!(
        diagnostics_mention(&diags, "toc-location"),
        "an unknown-value warning naming toc-location must be emitted; got: {diags:?}"
    );
    let margin = balanced_region(&html, "<div id=\"quarto-margin-sidebar\"", "div");
    assert!(
        margin.contains("<nav id=\"TOC\""),
        "unknown values keep the default right placement"
    );
}

// ---------------------------------------------------------------------
// Website `left` in banner mode keeps working end-to-end
// ---------------------------------------------------------------------

/// The website regime and the banner producer compose: a website page
/// with a banner and `toc-location: left` gets both the sidebar merge
/// and the header class.
#[test]
fn website_banner_left_composes() {
    let html = render_website(website_single_page(
        "toc-location: left\ntitle-block-banner: true\n",
    ));

    let sidebar = balanced_region(&html, "<nav id=\"quarto-sidebar\"", "nav");
    assert!(
        sidebar.contains("<nav id=\"TOC\""),
        "TOC inside the sidebar in banner mode too; region:\n{sidebar}"
    );
    assert!(
        has_class(&html, "<header id=\"title-block-header\"", "toc-left"),
        "banner header carries toc-left; got: {:?}",
        class_attr(&html, "<header id=\"title-block-header\"")
    );
}
