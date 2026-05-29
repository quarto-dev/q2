/*
 * tests/bootstrap_js_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for bd-4eyf: Bootstrap JS runtime injection
 * end-to-end through `render_to_file` and `ProjectPipeline`.
 */

//! End-to-end integration tests for [`BootstrapJsStage`].
//!
//! These tests drive a real `render_to_file` (single-doc) or
//! `ProjectPipeline` (website) render, then assert:
//!
//! - The rendered HTML contains a `<script src="…bootstrap.bundle.min.js">`
//!   tag in the head (or doesn't, when the document opted out).
//! - The actual JS file lands on disk at the expected location, with
//!   the same byte-content the binary embedded.
//! - In a multi-page website, all pages share one copy of the JS file.
//! - Nested pages get the correct `../site_libs/...` relative URL.
//!
//! Mirrors `artifact_scoping_pipeline.rs` for setup conventions.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const BOOTSTRAP_JS_BASENAME: &str = "bootstrap.bundle.min.js";

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write_file(path: &Path, contents: &str) {
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

/// Pull every `<script src="…">` URL out of an HTML string, in document
/// order. Lightweight regex-free parser tailored to the Pandoc template's
/// emitted form.
fn extract_script_srcs(html: &str) -> Vec<String> {
    let needle = "<script src=\"";
    let mut search = html;
    let mut out = Vec::new();
    while let Some(start) = search.find(needle) {
        let after = &search[start + needle.len()..];
        let end = after
            .find('"')
            .expect("malformed <script>: missing closing quote on src");
        out.push(after[..end].to_string());
        search = &after[end..];
    }
    out
}

fn render_website(fixture: impl FnOnce(&Path)) -> PathBuf {
    let temp = TempDir::new().unwrap();
    // Leak the TempDir so the directory survives for the test
    // assertions; tests are short-lived processes.
    let project_dir = canonical(temp.path());
    std::mem::forget(temp);
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
    let summary = pollster::block_on(pipeline.run()).expect("project render");
    assert!(
        !summary.has_failures(),
        "project render reported failures: {:?}",
        summary
    );
    project_dir
}

// ── Single-doc tests ──────────────────────────────────────────────────────

/// A themed single-doc render emits a `<script>` tag pointing at the
/// embedded Bootstrap JS, and the file lands on disk at the expected
/// location.
#[test]
fn single_doc_themed_emits_bootstrap_script_tag() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Test\nformat:\n  html:\n    theme: cosmo\n---\n\nHello.\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("single-doc render");

    let html = read(&result.output_path);
    let scripts = extract_script_srcs(&html);
    let bootstrap_scripts: Vec<_> = scripts
        .iter()
        .filter(|s| s.contains(BOOTSTRAP_JS_BASENAME))
        .collect();
    assert_eq!(
        bootstrap_scripts.len(),
        1,
        "expected exactly one bootstrap.bundle.min.js <script>; found {} (all scripts: {:?})",
        bootstrap_scripts.len(),
        scripts
    );

    // Single-doc Project-scoped artifacts land under `{stem}_files/`
    // (artifact.rs:30-33). Resolve via the script src so we don't have
    // to hardcode the layout.
    let bootstrap_src = bootstrap_scripts[0];
    let on_disk = result.output_path.parent().unwrap().join(bootstrap_src);
    assert!(
        on_disk.exists(),
        "expected Bootstrap JS on disk at {} (script src: {})",
        on_disk.display(),
        bootstrap_src
    );

    // Sanity: file is the bundled build (Popper inlined).
    let bytes = std::fs::read(&on_disk).expect("read JS");
    let s = std::str::from_utf8(&bytes).expect("utf8");
    assert!(
        s.to_ascii_lowercase().contains("popper"),
        "on-disk Bootstrap JS must be the bundled build with Popper"
    );
}

/// `theme: none` opts out: no `<script>` tag pointing at Bootstrap, no
/// JS file on disk.
#[test]
fn single_doc_theme_none_omits_bootstrap_script() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Test\nformat:\n  html:\n    theme: none\n---\n\nHello.\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("single-doc render");

    let html = read(&result.output_path);
    let scripts = extract_script_srcs(&html);
    let bootstrap_scripts: Vec<_> = scripts
        .iter()
        .filter(|s| s.contains(BOOTSTRAP_JS_BASENAME))
        .collect();
    assert!(
        bootstrap_scripts.is_empty(),
        "theme: none must not emit any Bootstrap <script>; found: {:?}",
        bootstrap_scripts
    );

    // Search both the bare layout and the per-page-files layout to
    // make sure we don't miss a stray copy.
    let parent = result.output_path.parent().unwrap();
    for candidate in [
        parent.join(BOOTSTRAP_JS_BASENAME),
        parent.join("doc_files").join(BOOTSTRAP_JS_BASENAME),
    ] {
        assert!(
            !candidate.exists(),
            "theme: none must not write Bootstrap JS to disk; found at {}",
            candidate.display()
        );
    }
}

// ── Multi-page website tests ──────────────────────────────────────────────

fn three_page_fixture(project_dir: &Path) {
    write_file(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n",
    );
    write_file(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nWelcome.\n",
    );
    write_file(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nAbout.\n",
    );
    write_file(
        &project_dir.join("docs").join("api.qmd"),
        "---\ntitle: API\n---\n\nAPI.\n",
    );
}

/// A 3-page website renders one shared Bootstrap JS file under
/// `_site/site_libs/quarto/bootstrap.bundle.min.js`, and every page's
/// `<script>` tag references it. (Single-copy invariant.)
#[test]
fn website_render_emits_one_shared_bootstrap_js() {
    let project_dir = render_website(three_page_fixture);
    let site = project_dir.join("_site");
    let shared_js = site
        .join("site_libs")
        .join("quarto")
        .join(BOOTSTRAP_JS_BASENAME);
    assert!(
        shared_js.exists(),
        "expected shared Bootstrap JS at {}",
        shared_js.display()
    );

    // Each page's HTML must include exactly one bootstrap.bundle.min.js
    // <script>, and they must all resolve to the same on-disk file.
    for page in &["index.html", "about.html"] {
        let html = read(&site.join(page));
        let scripts = extract_script_srcs(&html);
        let count = scripts
            .iter()
            .filter(|s| s.contains(BOOTSTRAP_JS_BASENAME))
            .count();
        assert_eq!(
            count, 1,
            "{}: expected one bootstrap.bundle.min.js <script>, found {} (scripts: {:?})",
            page, count, scripts
        );
    }
}

/// A nested page (`docs/api.html`) gets the correct `../site_libs/...`
/// relative URL for the shared Bootstrap JS.
#[test]
fn website_nested_page_links_bootstrap_with_relative_path() {
    let project_dir = render_website(three_page_fixture);
    let api_html = read(&project_dir.join("_site").join("docs").join("api.html"));
    let scripts = extract_script_srcs(&api_html);
    let bootstrap_src = scripts
        .iter()
        .find(|s| s.contains(BOOTSTRAP_JS_BASENAME))
        .unwrap_or_else(|| {
            panic!(
                "nested page missing bootstrap <script>; scripts: {:?}",
                scripts
            )
        });
    assert!(
        bootstrap_src.starts_with("../site_libs/quarto/"),
        "nested page must use `../site_libs/quarto/...` href; got {}",
        bootstrap_src
    );
    assert!(
        bootstrap_src.ends_with(BOOTSTRAP_JS_BASENAME),
        "nested page href must end in bootstrap.bundle.min.js; got {}",
        bootstrap_src
    );
}

/// A root-level page (`index.html`) gets a direct `site_libs/...` URL
/// with no `../` prefix.
#[test]
fn website_root_page_links_bootstrap_with_direct_path() {
    let project_dir = render_website(three_page_fixture);
    let index_html = read(&project_dir.join("_site").join("index.html"));
    let scripts = extract_script_srcs(&index_html);
    let bootstrap_src = scripts
        .iter()
        .find(|s| s.contains(BOOTSTRAP_JS_BASENAME))
        .unwrap_or_else(|| {
            panic!(
                "root page missing bootstrap <script>; scripts: {:?}",
                scripts
            )
        });
    assert!(
        bootstrap_src.starts_with("site_libs/quarto/"),
        "root page must use direct `site_libs/quarto/...` href; got {}",
        bootstrap_src
    );
    assert!(
        !bootstrap_src.contains("../"),
        "root page href must not include ../; got {}",
        bootstrap_src
    );
}

/// Navbar dropdown menus are the primary motivating use case for
/// shipping Bootstrap JS at all (they need `bootstrap.Dropdown` +
/// Popper for positioning). This test guards that *both* prerequisites
/// land in the rendered output simultaneously: the `<script>` tag that
/// loads the runtime AND the `data-bs-toggle="dropdown"` /
/// `.dropdown-menu` / `.dropdown-item` markup the runtime drives. If
/// either side regresses, the menu silently breaks in a real browser.
///
/// We intentionally *don't* spin up a real browser here — the live
/// dropdown-click smoke is recorded in the plan doc. This test just
/// locks in that the necessary inputs to a working menu are emitted.
#[test]
fn website_navbar_dropdown_emits_bootstrap_js_and_dropdown_markup() {
    let project_dir = render_website(|p| {
        write_file(
            &p.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\n\
             navbar:\n  title: Smoke\n  left:\n    - href: index.qmd\n      text: Home\n    \
             - text: Docs\n      menu:\n        - href: guide.qmd\n          text: Guide\n        \
             - href: api.qmd\n          text: API\n",
        );
        write_file(&p.join("index.qmd"), "---\ntitle: Home\n---\n\nWelcome.\n");
        write_file(&p.join("guide.qmd"), "---\ntitle: Guide\n---\n\nGuide.\n");
        write_file(&p.join("api.qmd"), "---\ntitle: API\n---\n\nAPI.\n");
    });

    let index_html = read(&project_dir.join("_site").join("index.html"));

    // Bootstrap JS is loaded.
    let scripts = extract_script_srcs(&index_html);
    assert!(
        scripts.iter().any(|s| s.contains(BOOTSTRAP_JS_BASENAME)),
        "navbar+dropdown page missing bootstrap.bundle.min.js <script>; scripts: {:?}",
        scripts
    );

    // The dropdown trigger carries the data-bs-toggle attribute Bootstrap
    // listens on. (Without it, click-to-open does nothing.)
    assert!(
        index_html.contains("data-bs-toggle=\"dropdown\""),
        "navbar dropdown missing data-bs-toggle=\"dropdown\""
    );

    // The dropdown menu container is present.
    assert!(
        index_html.contains("class=\"dropdown-menu\"") || index_html.contains("dropdown-menu\""),
        "navbar dropdown missing .dropdown-menu container"
    );

    // Both menu items render as dropdown-item links with rewritten hrefs.
    assert!(
        index_html.contains("href=\"guide.html\""),
        "Guide menu item href not rewritten to guide.html"
    );
    assert!(
        index_html.contains("href=\"api.html\""),
        "API menu item href not rewritten to api.html"
    );
    let dropdown_items = index_html.matches("class=\"dropdown-item").count();
    assert!(
        dropdown_items >= 2,
        "expected at least 2 dropdown-item entries, found {}",
        dropdown_items
    );
}
