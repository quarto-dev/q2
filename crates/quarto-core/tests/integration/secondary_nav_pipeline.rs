/*
 * tests/integration/secondary_nav_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for the website mobile secondary-nav bar
 * (bd-26bf3j1y): `nav.quarto-secondary-nav`, its sidebar-collapse
 * plumbing, and the mobile breadcrumb instance — end-to-end through
 * `ProjectPipeline`.
 */

//! Same harness shape as `breadcrumbs_pipeline.rs`: write a small
//! website fixture to a temp dir, drive it through the real
//! `ProjectPipeline`, inspect the rendered HTML.
//!
//! These drive the real project pipeline rather than
//! `render_qmd_to_html`, per `CLAUDE.md`'s end-to-end rule — the
//! secondary nav only exists on website renders with a sidebar, which
//! a bare document render never exercises.
//!
//! Q1 reference behavior was verified against rendered output from the
//! Posit Connect docs site, not just Q1 sources; see the plan's
//! "Findings during implementation".

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

fn render_project(fixture: impl FnOnce(&std::path::Path)) -> Vec<(String, String)> {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
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
    std::mem::forget(temp); // keep files alive for inspection

    let site_root = project_dir.join("_site");
    summary
        .outputs
        .iter()
        .map(|out| {
            let href = out
                .output_path
                .strip_prefix(&site_root)
                .unwrap_or(&out.output_path)
                .to_string_lossy()
                .replace('\\', "/");
            (href, read(&out.output_path))
        })
        .collect()
}

fn find_html<'a>(outputs: &'a [(String, String)], href: &str) -> &'a str {
    &outputs
        .iter()
        .find(|(h, _)| h == href)
        .unwrap_or_else(|| {
            panic!(
                "no output for href '{}'; got: {:?}",
                href,
                outputs.iter().map(|(h, _)| h).collect::<Vec<_>>()
            )
        })
        .1
}

/// Two-level nested sidebar; `guide/advanced/deep.qmd` sits inside
/// `Guide > Advanced`, and `index.qmd` is a top-level sidebar entry
/// (trail length 1).
fn nested_fixture(project_dir: &std::path::Path, quarto_yml_extra: &str) {
    write(
        &project_dir.join("_quarto.yml"),
        &format!(
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: Site\n{}  sidebar:\n    contents:\n      - index.qmd\n      - section: Guide\n        contents:\n          - guide/intro.qmd\n          - section: Advanced\n            contents:\n              - guide/advanced/deep.qmd\n",
            quarto_yml_extra
        ),
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome.\n",
    );
    write(
        &project_dir.join("guide/intro.qmd"),
        "---\ntitle: Intro\n---\n\nIntro.\n",
    );
    write(
        &project_dir.join("guide/advanced/deep.qmd"),
        "---\ntitle: Deep Page\n---\n\nDeep content.\n",
    );
}

/// Return the `nav.quarto-secondary-nav` element, for focused
/// assertions and better failure messages.
fn secondary_nav(html: &str) -> &str {
    let start = html
        .find("<nav class=\"quarto-secondary-nav\">")
        .unwrap_or_else(|| panic!("no nav.quarto-secondary-nav in output:\n{html}"));
    let end = html[start..]
        .find("</nav>\n</header>")
        .or_else(|| html[start..].find("</header>"))
        .map_or(html.len(), |i| start + i);
    &html[start..end]
}

/// The bar renders on a sidebar-bearing page, lives inside
/// `#quarto-header`, and its toggle targets the class the sidebar
/// actually carries. That last pairing is the whole point: a toggle
/// pointed at markup that doesn't exist is the reason this work was
/// split out of bd-breadcrumbs-missing-1vpuqh34.
#[test]
fn secondary_nav_renders_with_toggle_wired_to_the_sidebar() {
    let outputs = render_project(|dir| nested_fixture(dir, ""));
    let html = find_html(&outputs, "guide/advanced/deep.html");

    let header = html
        .find("<header id=\"quarto-header\"")
        .expect("#quarto-header present");
    let bar = html
        .find("<nav class=\"quarto-secondary-nav\">")
        .expect("secondary nav present");
    let header_close = html[header..]
        .find("</header>")
        .map(|i| i + header)
        .expect("#quarto-header closes");
    assert!(
        header < bar && bar < header_close,
        "secondary nav must sit inside #quarto-header"
    );

    let bar_html = secondary_nav(html);
    assert!(
        bar_html.contains("data-bs-target=\".quarto-sidebar-collapse-item\""),
        "toggle must target the sidebar's collapse class; got: {bar_html}"
    );

    // ...and the sidebar must actually carry it.
    let sidebar_open = {
        let i = html
            .find("<nav id=\"quarto-sidebar\"")
            .expect("sidebar present");
        &html[i..i + html[i..].find('>').unwrap()]
    };
    assert!(
        sidebar_open.contains("quarto-sidebar-collapse-item"),
        "sidebar must carry the class the toggle targets; got: {sidebar_open}"
    );
    assert!(
        sidebar_open.contains("collapse"),
        "sidebar must be Bootstrap-collapsible; got: {sidebar_open}"
    );
}

/// Q1 emits a click-catching glass pane as a sibling of the sidebar so
/// tapping outside the open drawer closes it (`sidebar.ejs:100`).
#[test]
fn secondary_nav_sidebar_glass_pane_renders() {
    let outputs = render_project(|dir| nested_fixture(dir, ""));
    let html = find_html(&outputs, "guide/advanced/deep.html");

    assert!(
        html.contains("id=\"quarto-sidebar-glass\""),
        "expected the glass pane in rendered output"
    );
}

/// The mobile breadcrumb instance renders even for a ONE-crumb trail —
/// unlike the title-block instance, which Q1 gates at >1. Verified
/// against Q1's rendered `admin/index.html` on the Connect site, which
/// carries a single-crumb `nav.quarto-page-breadcrumbs` in its
/// secondary nav.
#[test]
fn secondary_nav_breadcrumbs_render_for_single_crumb_trail() {
    let outputs = render_project(|dir| nested_fixture(dir, ""));
    let html = find_html(&outputs, "index.html");

    let bar_html = secondary_nav(html);
    assert!(
        bar_html.contains("<nav class=\"quarto-page-breadcrumbs\" aria-label=\"breadcrumb\">"),
        "single-crumb page still gets the mobile trail; got: {bar_html}"
    );
    assert!(
        bar_html.contains(">Home</a>") || bar_html.contains(">Home<"),
        "the one crumb is the page itself; got: {bar_html}"
    );
    // The title-block instance keeps its >1 gate.
    assert!(
        !html.contains("quarto-title-breadcrumbs"),
        "title-block instance must stay gated at >1 crumb"
    );
}

/// The mobile instance takes no extra classes; the title-block one
/// keeps `quarto-title-breadcrumbs d-none d-lg-block`. Both appear on a
/// deep page, and they must not be confused.
#[test]
fn secondary_nav_and_title_block_instances_are_distinct() {
    let outputs = render_project(|dir| nested_fixture(dir, ""));
    let html = find_html(&outputs, "guide/advanced/deep.html");

    assert!(
        html.contains("<nav class=\"quarto-page-breadcrumbs\" aria-label=\"breadcrumb\">"),
        "mobile instance (bare classes) missing"
    );
    assert!(
        html.contains(
            "<nav class=\"quarto-page-breadcrumbs quarto-title-breadcrumbs d-none d-lg-block\" \
             aria-label=\"breadcrumb\">"
        ),
        "title-block instance (extra classes) missing"
    );

    let bar_html = secondary_nav(html);
    assert!(
        !bar_html.contains("d-lg-block"),
        "the mobile instance must not be desktop-only; got: {bar_html}"
    );
}

/// `bread-crumbs: false` swaps the trail for Q1's collapsed page title,
/// and hides the document `h1.title` below `lg` so the two don't
/// duplicate (`website-navigation.ts:483-493`).
#[test]
fn secondary_nav_collapsed_title_when_breadcrumbs_disabled() {
    let outputs = render_project(|dir| nested_fixture(dir, "  bread-crumbs: false\n"));
    let html = find_html(&outputs, "guide/advanced/deep.html");

    let bar_html = secondary_nav(html);
    assert!(
        bar_html.contains("<h1 class=\"quarto-secondary-nav-title\">Deep Page</h1>"),
        "expected the collapsed title; got: {bar_html}"
    );
    assert!(
        !bar_html.contains("quarto-page-breadcrumbs"),
        "breadcrumbs are disabled; got: {bar_html}"
    );
    assert!(
        html.contains("<h1 class=\"title d-none d-lg-block\">"),
        "the document title must hide below lg when the bar shows it"
    );
}

/// No sidebar means no toggle target, so no bar. Verified against Q1's
/// rendered site root, which has a navbar but no sidebar and emits no
/// secondary nav.
#[test]
fn secondary_nav_absent_without_sidebar() {
    let outputs = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: Site\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHi.\n");
    });
    let html = find_html(&outputs, "index.html");

    assert!(
        !html.contains("quarto-secondary-nav"),
        "no sidebar, no secondary nav"
    );
}

/// Finding F1: Q1's `header > .quarto-title-block` selector never
/// matches, so Q1 never hides the title block — 0 of 350 Connect pages
/// do. Parity means leaving it visible. This pin stops a future parity
/// pass from "fixing" the absence by porting the dead branch.
#[test]
fn secondary_nav_does_not_hide_the_title_block() {
    let outputs = render_project(|dir| nested_fixture(dir, ""));
    let html = find_html(&outputs, "guide/advanced/deep.html");

    let idx = html
        .find("quarto-title-block")
        .expect("title block rendered");
    let tag_start = html[..idx].rfind('<').unwrap();
    let tag_end = idx + html[idx..].find('>').unwrap();
    let tag = &html[tag_start..=tag_end];
    assert!(
        !tag.contains("d-none"),
        "Q1 does not hide the title block (plan finding F1); got: {tag}"
    );
}
