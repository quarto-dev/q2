/*
 * tests/integration/headroom_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for the fixed scroll-away website header
 * (bd-ersobfbt): `#quarto-header.headroom.fixed-top`, the
 * `body.nav-fixed` compensation class, and the quarto-nav / headroom
 * JS shipping (incl. the `pinned:` opt-out) — through the real
 * `ProjectPipeline`.
 */

//! Same harness shape as `secondary_nav_pipeline.rs`: write a small
//! website fixture to a temp dir, drive it through the real
//! `ProjectPipeline`, inspect the rendered HTML and the `site_libs/`
//! payloads on disk. Per `CLAUDE.md`'s end-to-end rule, these tests
//! exist because the unit tests (template partial, transform
//! predicate) each check one seam — only a real render proves the
//! seams meet: header classes in the HTML, body class composed, both
//! `<script>` tags emitted, files actually written.

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

/// Render a fixture project; return (site_root, [(href, html)]).
fn render_project(fixture: impl FnOnce(&std::path::Path)) -> (PathBuf, Vec<(String, String)>) {
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
    let outputs = summary
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
        .collect();
    (site_root, outputs)
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

/// Minimal navbar website; `navbar_extra` is spliced into the navbar
/// block (e.g. `"    pinned: true\n"`).
fn navbar_fixture(project_dir: &std::path::Path, navbar_extra: &str) {
    write(
        &project_dir.join("_quarto.yml"),
        &format!(
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: Site\n  navbar:\n{}    left:\n      - href: index.qmd\n        text: Home\n      - href: about.qmd\n        text: About\n",
            navbar_extra
        ),
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome.\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nAbout.\n",
    );
}

/// Sidebar-only website (no navbar): header exists for the secondary
/// nav, but there is no `nav.navbar` inside it.
fn sidebar_only_fixture(project_dir: &std::path::Path) {
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n\
         website:\n  title: Site\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome.\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nAbout.\n",
    );
}

/// The core wiring: header classes, body compensation class, script
/// tags, and the payloads on disk — all from one real render.
#[test]
fn navbar_site_ships_fixed_header_and_both_scripts() {
    let (site_root, outputs) = render_project(|dir| navbar_fixture(dir, ""));
    let html = find_html(&outputs, "index.html");

    // Header markup: fixed + scroll-away classes.
    assert!(
        html.contains("<header id=\"quarto-header\" class=\"headroom fixed-top\">"),
        "header must carry headroom fixed-top; got:\n{html}"
    );

    // Body compensation class, composed with the color-mode class.
    assert!(
        html.contains("<body class=\"") && html.contains("nav-fixed"),
        "body must carry nav-fixed; got:\n{html}"
    );

    // Script tags, in load order (bootstrap before quarto-nav pair,
    // headroom before nav within the pair).
    let bootstrap = html
        .find("src=\"site_libs/quarto/bootstrap.bundle.min.js\"")
        .expect("bootstrap script tag");
    let headroom = html
        .find("src=\"site_libs/quarto-nav/headroom.min.js\"")
        .expect("headroom script tag");
    let nav = html
        .find("src=\"site_libs/quarto-nav/quarto-nav.js\"")
        .expect("quarto-nav script tag");
    assert!(
        bootstrap < headroom && headroom < nav,
        "script order must be bootstrap < headroom < quarto-nav; \
         got {bootstrap}, {headroom}, {nav}"
    );

    // Payloads written under site_libs-equivalent root.
    let headroom_file = site_root.join("site_libs/quarto-nav/headroom.min.js");
    let nav_file = site_root.join("site_libs/quarto-nav/quarto-nav.js");
    assert!(
        headroom_file.exists(),
        "headroom.min.js must be written; missing {}",
        headroom_file.display()
    );
    assert!(
        read(&headroom_file).contains("headroom.js v0.12.0"),
        "vendored headroom must be v0.12.0"
    );
    assert!(
        read(&nav_file).contains("quartoToggleHeadroom"),
        "quarto-nav.js must define the toggle hook"
    );

    // The navbar toggler carries the guarded freeze hook.
    assert!(
        html.contains(
            "onclick=\"if (window.quartoToggleHeadroom) { window.quartoToggleHeadroom(); }\""
        ),
        "navbar toggler must freeze headroom while open; got:\n{html}"
    );
}

/// `navbar: pinned: true` — Q1's opt-out: the header stays fixed
/// (classes and quarto-nav.js unchanged) but the scroll-away script is
/// not shipped.
#[test]
fn pinned_navbar_omits_headroom_script_only() {
    let (site_root, outputs) = render_project(|dir| navbar_fixture(dir, "    pinned: true\n"));
    let html = find_html(&outputs, "index.html");

    assert!(
        html.contains("<header id=\"quarto-header\" class=\"headroom fixed-top\">"),
        "pinned keeps the fixed header markup (Q1 parity); got:\n{html}"
    );
    assert!(
        html.contains("nav-fixed"),
        "pinned still needs the body compensation; got:\n{html}"
    );
    assert!(
        html.contains("src=\"site_libs/quarto-nav/quarto-nav.js\""),
        "pinned still ships the offset machinery; got:\n{html}"
    );
    assert!(
        !html.contains("headroom.min.js"),
        "pinned must NOT ship the scroll-away script; got:\n{html}"
    );
    assert!(
        !site_root.join("site_libs/quarto-nav/headroom.min.js").exists(),
        "pinned must not write headroom.min.js to disk"
    );
}

/// Sidebar-only site: the header exists (secondary nav) so the offset
/// JS ships, but Q1's `nav-fixed` condition requires a navbar — no
/// navbar, no `nav-fixed`.
#[test]
fn sidebar_only_site_ships_js_but_no_nav_fixed() {
    let (_site_root, outputs) = render_project(sidebar_only_fixture);
    let html = find_html(&outputs, "index.html");

    assert!(
        html.contains("<header id=\"quarto-header\" class=\"headroom fixed-top\">"),
        "sidebar site still has the fixed header (secondary nav); got:\n{html}"
    );
    assert!(
        !html.contains("nav-fixed"),
        "no navbar → no nav-fixed body class (Q1's postprocessor \
         requires `#quarto-header.fixed-top nav.navbar`); got:\n{html}"
    );
    assert!(
        html.contains("src=\"site_libs/quarto-nav/quarto-nav.js\""),
        "the fixed header still needs the offset machinery; got:\n{html}"
    );
}
