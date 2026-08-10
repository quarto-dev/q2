/*
 * tests/integration/shortcode_config_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for shortcode resolution in website config
 * strings, document metadata, and include files
 * (bd-shortcodes-in-metadata-bp06aub8).
 */

//! Shortcodes must resolve in every context Quarto 1 resolves them:
//!
//! 1. `website.title` → page `<title>` (plain text, raw-HTML tags
//!    stripped) and navbar brand (markup kept, not escaped);
//! 2. `website.sidebar.title`;
//! 3. `website.page-footer` text regions;
//! 4. document metadata (`title`, `subtitle`, arbitrary keys);
//! 5. `include-in-header` / `include-before-body` /
//!    `include-after-body` files and `{text: …}` smart-includes —
//!    text-level substitution, NOT markdown parsing.
//!
//! Q1 ground truth and design decisions:
//! `claude-notes/plans/2026-08-10-shortcodes-website-config-includes.md`.
//!
//! Tests use `{{< meta … >}}` as the primary shortcode so they stay
//! independent of the `_environment`-file work
//! (bd-environment-files-372u9qbs); the single `env` test sets the
//! process variable explicitly (process env wins over `_environment`
//! files in both designs).

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

fn find_html<'a>(outputs: &'a [(String, String)], stem: &str) -> &'a str {
    &outputs
        .iter()
        .find(|(s, _)| s == stem)
        .unwrap_or_else(|| {
            panic!(
                "no output for stem '{}'; got: {:?}",
                stem,
                outputs.iter().map(|(s, _)| s).collect::<Vec<_>>()
            )
        })
        .1
}

/// A website fixture exercising all substitution contexts at once.
/// `version: 9.9.9` is defined in `_quarto.yml` and read back via
/// `{{< meta version >}}` everywhere.
fn full_fixture(project_dir: &std::path::Path) {
    write(
        &project_dir.join("_quarto.yml"),
        r#"project:
  type: website
  output-dir: _site

version: "9.9.9"

website:
  title: "My Site <small>V {{< meta version >}}</small>"
  navbar:
    left:
      - href: index.qmd
        text: Home
  sidebar:
    title: "Side {{< meta version >}}"
    contents:
      - index.qmd
  page-footer:
    center: "Product {{< meta version >}}"

format:
  html:
    include-before-body:
      - !path _banner.html
"#,
    );
    write(
        &project_dir.join("_banner.html"),
        "<div id=\"banner\">Banner {{< meta version >}} **md-test**</div>\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\nsubtitle: \"Sub {{< meta version >}}\"\n---\n\nBody {{< meta version >}}.\n",
    );
}

// === website.title ========================================================

/// `website.title` with a shortcode and raw HTML: the page `<title>`
/// substitutes the shortcode and strips the raw tags (Q1: innerText),
/// keeping the tags' text content.
#[test]
fn website_title_shortcode_substitutes_in_page_title() {
    let (_dir, outputs) = render_project(full_fixture);
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("<title>Home – My Site V 9.9.9</title>"),
        "expected substituted, tag-stripped <title>; got: {}",
        title_line(html)
    );
}

/// The navbar brand substitutes the shortcode and keeps the raw
/// `<small>` markup as markup (Q1: innerHTML) — no double-escaping.
#[test]
fn website_title_shortcode_substitutes_in_navbar_brand() {
    let (_dir, outputs) = render_project(full_fixture);
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("My Site <small>V 9.9.9</small>"),
        "expected navbar brand with live <small> markup and substituted shortcode; got navbar line: {}",
        line_containing(html, "navbar-brand")
    );
    assert!(
        !html.contains("&lt;small&gt;"),
        "raw HTML in website.title must not be escaped; got navbar line: {}",
        line_containing(html, "navbar-brand")
    );
    assert!(
        !html.contains("{{&lt; meta"),
        "shortcode must not appear escaped in output"
    );
}

// === website.sidebar.title ================================================

/// The sidebar title substitutes shortcodes.
#[test]
fn sidebar_title_shortcode_substitutes() {
    let (_dir, outputs) = render_project(full_fixture);
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("Side 9.9.9"),
        "expected substituted sidebar title; got sidebar-title line: {}",
        line_containing(html, "sidebar-title")
    );
}

// === website.page-footer ==================================================

/// Footer text regions substitute shortcodes.
#[test]
fn page_footer_shortcode_substitutes() {
    let (_dir, outputs) = render_project(full_fixture);
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("Product 9.9.9"),
        "expected substituted footer text; got footer line: {}",
        line_containing(html, "nav-footer-center")
    );
    assert!(
        !html.contains("Product {{"),
        "footer must not contain the literal shortcode; got footer line: {}",
        line_containing(html, "nav-footer-center")
    );
}

// === document metadata ====================================================

/// A shortcode in the document `subtitle` substitutes (metadata walk
/// — the title block renders metadata inlines).
#[test]
fn doc_subtitle_shortcode_substitutes() {
    let (_dir, outputs) = render_project(full_fixture);
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("Sub 9.9.9"),
        "expected substituted subtitle; got subtitle line: {}",
        line_containing(html, "subtitle")
    );
    assert!(
        !html.contains("quarto-unresolved-shortcode"),
        "no unresolved-shortcode markers expected anywhere in this fixture"
    );
}

/// A shortcode in the document `title` substitutes in both the `<h1>`
/// title block and the derived `pagetitle`.
#[test]
fn doc_title_shortcode_substitutes_in_h1_and_pagetitle() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nversion: \"9.9.9\"\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: \"Home {{< meta version >}}\"\n---\n\nBody.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout.\n",
        );
    });
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("Home 9.9.9"),
        "expected substituted title text; got h1 area: {}",
        line_containing(html, "<h1")
    );
    assert!(
        html.contains("<title>Home 9.9.9</title>"),
        "expected substituted pagetitle; got: {}",
        title_line(html)
    );
}

// === include files ========================================================

/// Include-file contents get text-level shortcode substitution but are
/// NOT markdown-parsed (Q1 parity: `**md-test**` stays literal).
#[test]
fn include_before_body_file_shortcode_substitutes_textually() {
    let (_dir, outputs) = render_project(full_fixture);
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("Banner 9.9.9 **md-test**"),
        "expected substituted-but-not-markdown-parsed banner; got banner line: {}",
        line_containing(html, "banner")
    );
    assert!(
        !html.contains("Banner {{"),
        "banner must not contain the literal shortcode"
    );
}

/// `{text: …}` smart-includes substitute shortcodes instead of
/// silently dropping them.
#[test]
fn text_smart_include_shortcode_substitutes() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nversion: \"9.9.9\"\n\
             format:\n  html:\n    include-before-body:\n      - text: \"<div id='ti'>T {{< meta version >}}</div>\"\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nBody.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout.\n",
        );
    });
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("<div id='ti'>T 9.9.9</div>"),
        "expected substituted {{text}} smart-include; got: {}",
        line_containing(html, "id='ti'")
    );
}

/// The `env` shortcode works in include files (explicitly-set process
/// env; independent of `_environment`-file loading).
#[test]
fn include_file_env_shortcode_substitutes() {
    // Safety: nextest runs each test in its own process, so mutating
    // this process's environment cannot race another test.
    unsafe { std::env::set_var("Q2_TEST_SHORTCODE_CONFIG_VAR", "env-ok") };
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             format:\n  html:\n    include-before-body:\n      - !path _b.html\n",
        );
        write(
            &project_dir.join("_b.html"),
            "<div id=\"eb\">E {{< env Q2_TEST_SHORTCODE_CONFIG_VAR >}}</div>\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nBody.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout.\n",
        );
    });
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("<div id=\"eb\">E env-ok</div>"),
        "expected env-substituted include; got: {}",
        line_containing(html, "id=\"eb\"")
    );
}

/// The `env` shortcode composes with `_environment` file loading
/// (bd-environment-files-372u9qbs): a variable defined only in the
/// project's `_environment` file resolves in include files and website
/// config strings through the same handler plumbing.
#[test]
fn environment_file_var_resolves_in_include_and_title() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_environment"),
            "Q2_TEST_COMPOSE_VAR=from-env-file\n",
        );
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"T {{< env Q2_TEST_COMPOSE_VAR >}}\"\n\
             format:\n  html:\n    include-before-body:\n      - !path _c.html\n",
        );
        write(
            &project_dir.join("_c.html"),
            "<div id=\"cb\">C {{< env Q2_TEST_COMPOSE_VAR >}}</div>\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nBody.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout.\n",
        );
    });
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("<div id=\"cb\">C from-env-file</div>"),
        "expected _environment-sourced env var substituted in include; got: {}",
        line_containing(html, "id=\"cb\"")
    );
    assert!(
        html.contains("<title>Home – T from-env-file</title>"),
        "expected _environment-sourced env var substituted in <title>; got: {}",
        title_line(html)
    );
}

// === unresolved shortcodes ================================================

/// An unresolvable shortcode in `website.title` renders the visible
/// body-text-policy marker (`?meta:nope`) rather than the literal
/// shortcode or nothing.
#[test]
fn unresolved_shortcode_in_website_title_renders_marker() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: \"T {{< meta nope >}}\"\n  navbar:\n    left:\n      - href: index.qmd\n        text: Home\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nBody.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout.\n",
        );
    });
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("?meta:nope"),
        "expected visible unresolved-shortcode marker in title contexts; <title>: {} navbar: {}",
        title_line(html),
        line_containing(html, "navbar-brand")
    );
    assert!(
        !html.contains("{{&lt; meta nope"),
        "literal escaped shortcode must not leak into output"
    );
}

/// An unresolvable shortcode in an include file renders the plain
/// `?key` marker at text level.
#[test]
fn unresolved_shortcode_in_include_file_renders_marker() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             format:\n  html:\n    include-before-body:\n      - !path _u.html\n",
        );
        write(
            &project_dir.join("_u.html"),
            "<div id=\"ub\">U {{< meta nope >}}</div>\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nBody.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout.\n",
        );
    });
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("<div id=\"ub\">U ?meta:nope</div>"),
        "expected plain-text unresolved marker in include; got: {}",
        line_containing(html, "id=\"ub\"")
    );
}

// === regressions ==========================================================

/// Escaped shortcodes in include files stay literal (unescaped once):
/// `{{{< meta v >}}}` → `{{< meta v >}}`.
#[test]
fn escaped_shortcode_in_include_file_stays_literal() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nversion: \"9.9.9\"\n\
             format:\n  html:\n    include-before-body:\n      - !path _esc.html\n",
        );
        write(
            &project_dir.join("_esc.html"),
            "<div id=\"esc\">{{{< meta version >}}}</div>\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nBody.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout.\n",
        );
    });
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("<div id=\"esc\">{{< meta version >}}</div>"),
        "expected escaped shortcode to render as its literal form; got: {}",
        line_containing(html, "id=\"esc\"")
    );
}

/// Config strings without shortcodes or markup keep rendering exactly
/// as before (no behavior change for plain titles/footers).
#[test]
fn plain_config_strings_unchanged() {
    let (_dir, outputs) = render_project(|project_dir| {
        write(
            &project_dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             website:\n  title: Plain Site\n  navbar:\n    left:\n      - href: index.qmd\n        text: Home\n  page-footer:\n    center: Plain Footer\n",
        );
        write(
            &project_dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nBody.\n",
        );
        write(
            &project_dir.join("about.qmd"),
            "---\ntitle: About\n---\n\nAbout.\n",
        );
    });
    let html = find_html(&outputs, "index");

    assert!(
        html.contains("<title>Home – Plain Site</title>"),
        "plain site title in <title>; got: {}",
        title_line(html)
    );
    assert!(
        html.contains(">Plain Site</a>"),
        "plain navbar brand unchanged; got: {}",
        line_containing(html, "navbar-brand")
    );
    assert!(html.contains("Plain Footer"), "plain footer unchanged");
}

// === helpers ==============================================================

fn title_line(html: &str) -> &str {
    line_containing(html, "<title>")
}

fn line_containing<'a>(html: &'a str, needle: &str) -> &'a str {
    html.lines()
        .find(|l| l.contains(needle))
        .unwrap_or("<line not found>")
}
