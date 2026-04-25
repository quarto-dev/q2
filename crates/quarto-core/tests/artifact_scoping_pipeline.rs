/*
 * tests/artifact_scoping_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for Phase 5 of the website-projects epic: the
 * scope-aware artifact store + `site_libs/` end-to-end through
 * `ProjectPipeline` + `render_to_file`.
 */

//! End-to-end integration tests for Phase 5 artifact scoping.
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-5.md` §Tests
//! 17–23. Each test sets up a fixture, drives it through the
//! render pipeline (single-doc or `ProjectPipeline`), then
//! inspects the resulting on-disk layout and the `<link>` /
//! `<script>` URLs in the rendered HTML.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

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

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

// === Test 17 ================================================================

/// **Plan test 17 (the most important test in Phase 5):** the
/// pre-Phase-5 baseline fixture renders byte-identically through
/// the post-refactor pipeline. Hashes captured at baseline-time
/// (commit `7881178e`, file
/// `tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`)
/// must still match.
#[test]
fn single_doc_render_unchanged_under_scope_refactor() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    let baseline_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/phase5-single-doc-baseline");
    let baseline_qmd = read(&baseline_dir.join("doc.qmd"));
    write_file(&qmd_path, &baseline_qmd);

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("single-doc render");

    // Read expected hashes (one "<rel-path> <sha256>" per line, # lines ignored).
    let expected = read(&baseline_dir.join("expected_hashes.txt"));
    let mut checked = 0usize;
    for line in expected.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let rel = parts.next().expect("path field");
        let want = parts.next().expect("sha256 field");

        let on_disk = result.output_path.parent().unwrap().join(rel);
        let got_bytes = std::fs::read(&on_disk)
            .unwrap_or_else(|e| panic!("missing post-refactor file {}: {}", on_disk.display(), e));
        let got = sha256_hex(&got_bytes);
        assert_eq!(
            got, want,
            "byte-identity broken for {}: post-refactor hash differs from baseline",
            rel
        );
        checked += 1;
    }
    assert!(checked > 0, "no expected hashes parsed from baseline");
}

// === Helper for project-pipeline tests ======================================

fn render_website(fixture: impl FnOnce(&Path)) -> PathBuf {
    let temp = TempDir::new().unwrap();
    // Leak the TempDir so the directory survives for the test
    // assertions; tests are short-lived processes so cleanup at
    // process exit is fine.
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

// === Test 18 ================================================================

/// Plan test 18: a 3-page website renders one shared theme CSS
/// file under `_site/site_libs/quarto/quarto-theme-*.css`.
#[test]
fn website_render_emits_site_libs_dir() {
    let project_dir = render_website(three_page_fixture);
    let site_libs = project_dir.join("_site").join("site_libs").join("quarto");
    assert!(
        site_libs.exists(),
        "expected _site/site_libs/quarto/ to exist"
    );

    let entries: Vec<_> = std::fs::read_dir(&site_libs)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("quarto-theme-") && s.ends_with(".css"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "expected exactly one quarto-theme-*.css under site_libs/quarto/, found {}",
        entries.len()
    );
}

// === Test 19b ===============================================================

/// Plan test 19b: a 3-page website with two distinct themes
/// produces two distinct CSS files under
/// `site_libs/quarto/`. Each page's `<link>` references the file
/// matching its theme. Direct test of the fingerprint-based dedup.
#[test]
fn website_render_emits_two_themes_when_docs_differ() {
    let project_dir = render_website(|p| {
        write_file(&p.join("_quarto.yml"), "project:\n  type: website\n");
        // Two pages share theme: cosmo
        write_file(
            &p.join("index.qmd"),
            "---\ntitle: Home\nformat:\n  html:\n    theme: cosmo\n---\n\nIntro.\n",
        );
        write_file(
            &p.join("methods.qmd"),
            "---\ntitle: Methods\nformat:\n  html:\n    theme: cosmo\n---\n\nMethods.\n",
        );
        // One page uses theme: darkly
        write_file(
            &p.join("appendix.qmd"),
            "---\ntitle: Appendix\nformat:\n  html:\n    theme: darkly\n---\n\nAppendix.\n",
        );
    });

    let site_libs = project_dir.join("_site").join("site_libs").join("quarto");
    let entries: Vec<_> = std::fs::read_dir(&site_libs)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("quarto-theme-") && s.ends_with(".css"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(
        entries.len(),
        2,
        "expected two distinct theme files (cosmo + darkly), found {}",
        entries.len()
    );

    // Each page's <link> references one of the two files; cosmo
    // pages link to the same file (dedup).
    let intro_html = read(&project_dir.join("_site").join("index.html"));
    let methods_html = read(&project_dir.join("_site").join("methods.html"));
    let appendix_html = read(&project_dir.join("_site").join("appendix.html"));

    let intro_href = extract_first_stylesheet_href(&intro_html);
    let methods_href = extract_first_stylesheet_href(&methods_html);
    let appendix_href = extract_first_stylesheet_href(&appendix_html);

    assert_eq!(
        intro_href, methods_href,
        "two cosmo pages should link to the same theme file"
    );
    assert_ne!(
        intro_href, appendix_href,
        "cosmo and darkly pages must link to different theme files"
    );
}

// === Test 20 ================================================================

/// Plan test 20: a nested page (`docs/api.html`) gets the
/// correct `../site_libs/...` relative URL.
#[test]
fn website_nested_page_links_css_with_relative_path() {
    let project_dir = render_website(three_page_fixture);
    let api_html = read(&project_dir.join("_site").join("docs").join("api.html"));
    let href = extract_first_stylesheet_href(&api_html);
    assert!(
        href.starts_with("../site_libs/quarto/quarto-theme-"),
        "nested page must use `../site_libs/...` href; got {}",
        href
    );
    assert!(
        href.ends_with(".css"),
        "href must end in .css; got {}",
        href
    );
}

// === Test 21 ================================================================

/// Plan test 21: a root-level page (`index.html`) gets a direct
/// `site_libs/...` URL with no `../` prefix.
#[test]
fn website_root_page_links_css_with_direct_path() {
    let project_dir = render_website(three_page_fixture);
    let index_html = read(&project_dir.join("_site").join("index.html"));
    let href = extract_first_stylesheet_href(&index_html);
    assert!(
        href.starts_with("site_libs/quarto/quarto-theme-"),
        "root page must use direct `site_libs/...` href; got {}",
        href
    );
    assert!(
        !href.contains("../"),
        "root page href must not include ../; got {}",
        href
    );
}

// === Helper: extract first <link rel="stylesheet"> href =====================

fn extract_first_stylesheet_href(html: &str) -> String {
    let needle = "<link rel=\"stylesheet\" href=\"";
    let start = html
        .find(needle)
        .unwrap_or_else(|| panic!("no <link rel=\"stylesheet\"> in HTML"));
    let after = &html[start + needle.len()..];
    let end = after
        .find('"')
        .expect("malformed <link>: missing closing quote on href");
    after[..end].to_string()
}
