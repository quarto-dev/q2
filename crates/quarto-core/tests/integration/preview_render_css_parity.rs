/*
 * tests/integration/preview_render_css_parity.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-4b7f1hr7: theme-CSS identity between the html and q2-preview
 * pipelines.
 */

//! Theme-CSS parity tests between `q2 render` and `q2 preview`.
//!
//! Both pipelines run the same [`CompileThemeCssStage`], but nothing
//! else guarantees they hand it the same *inputs* (format-flattened
//! metadata, theme resolution, doc-derived SCSS variables). These
//! tests render the same document through both pipelines — natively,
//! in-process — and assert the stored `css:theme:<fingerprint>`
//! artifact is byte-identical, fingerprint key included.
//!
//! Each render gets its own fresh cache directory so both sides
//! actually compile (a shared cache would let the second render
//! parrot the first's output and hide input divergence).
//!
//! Known limitation, deliberately out of scope here: in the browser,
//! the q2-preview pipeline compiles SCSS with dart-sass (JS bridge)
//! while native render uses grass, so the *bytes shipped to the
//! browser* can still differ from render's even when these tests
//! pass. These tests pin down everything upstream of the compiler:
//! if they pass, both pipelines assemble identical SCSS. The
//! compiler split is tracked separately (see the bd-4b7f1hr7 plan,
//! `claude-notes/plans/2026-06-10-html-preview-css-drift-audit.md`).

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::pipeline::{HtmlRenderConfig, render_qmd_to_html, render_qmd_to_preview_ast};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn test_project() -> ProjectContext {
    ProjectContext {
        dir: PathBuf::from("/project"),
        config: quarto_core::project::ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/test.qmd")],
        output_dir: PathBuf::from("/project"),

        ..Default::default()
    }
}

/// A runtime with a private, throwaway sass cache. Every call sites a
/// fresh cache so no render in this file can serve another's output.
fn fresh_runtime() -> Arc<dyn SystemRuntime> {
    let temp = TempDir::new().unwrap();
    let cache_dir = temp.path().to_path_buf();
    // Leak the TempDir so the directory survives for the render;
    // tests are short-lived processes.
    std::mem::forget(temp);
    Arc::new(NativeRuntime::with_cache_dir(cache_dir))
}

/// Extract the single `css:theme:<fingerprint>` artifact a pipeline
/// run stored: returns `(key, css)`.
fn theme_css_artifact(ctx: &RenderContext, pipeline_name: &str) -> (String, String) {
    let entries: Vec<_> = ctx.artifacts.get_by_prefix("css:theme:");
    assert_eq!(
        entries.len(),
        1,
        "{pipeline_name}: expected exactly one css:theme:* artifact, found {}",
        entries.len()
    );
    let (key, artifact) = &entries[0];
    let css = String::from_utf8(artifact.content.clone()).expect("theme CSS should be valid UTF-8");
    (key.to_string(), css)
}

/// Render `content` through the html pipeline and return the stored
/// theme-CSS artifact `(key, css)`.
fn html_theme_css(content: &[u8]) -> (String, String) {
    let project = test_project();
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    pollster::block_on(render_qmd_to_html(
        content,
        "test.qmd",
        &mut ctx,
        &HtmlRenderConfig::default(),
        fresh_runtime(),
    ))
    .expect("html pipeline render");
    theme_css_artifact(&ctx, "html pipeline")
}

/// Render `content` through the q2-preview pipeline and return the
/// stored theme-CSS artifact `(key, css)`.
fn preview_theme_css(content: &[u8]) -> (String, String) {
    let project = test_project();
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = Format::from_format_string("q2-preview").expect("q2-preview format");
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    pollster::block_on(render_qmd_to_preview_ast(
        content,
        "test.qmd",
        &mut ctx,
        fresh_runtime(),
        None,
        vec![],
    ))
    .expect("q2-preview pipeline render");
    theme_css_artifact(&ctx, "q2-preview pipeline")
}

/// Assert both pipelines stored a byte-identical theme artifact for
/// `content`, and return the shared CSS for cross-case assertions.
fn assert_css_parity(content: &[u8], case: &str) -> String {
    let (html_key, html_css) = html_theme_css(content);
    let (preview_key, preview_css) = preview_theme_css(content);
    assert_eq!(
        html_key, preview_key,
        "{case}: html and q2-preview pipelines produced different theme \
         fingerprint keys — their CompileThemeCssStage inputs have drifted"
    );
    assert_eq!(
        html_css, preview_css,
        "{case}: html and q2-preview pipelines produced different theme \
         CSS bytes for the same document"
    );
    html_css
}

/// No `theme:` key at all — the default Bootstrap + Quarto layer.
#[test]
fn default_theme_css_identical_between_html_and_preview_pipelines() {
    assert_css_parity(b"---\ntitle: Test\n---\n\nHello.\n", "default theme");
}

/// Top-level named theme.
#[test]
fn named_theme_css_identical_between_html_and_preview_pipelines() {
    let themed = assert_css_parity(
        b"---\ntitle: Test\ntheme: cosmo\n---\n\nHello.\n",
        "theme: cosmo",
    );
    let default = assert_css_parity(b"---\ntitle: Test\n---\n\nHello.\n", "default theme");
    // Guard against a vacuous pass where BOTH pipelines ignore the
    // theme and agree on default CSS.
    assert_ne!(
        themed, default,
        "theme: cosmo must change the compiled CSS relative to the default theme"
    );
}

/// Format-scoped theme (`format: html: theme: ...`) — the place drift
/// is most likely to hide: the q2-preview pseudo-format must flatten
/// `format.html.*` metadata exactly like the html pipeline does.
#[test]
fn format_scoped_theme_css_identical_between_html_and_preview_pipelines() {
    let themed = assert_css_parity(
        b"---\ntitle: Test\nformat:\n  html:\n    theme: darkly\n---\n\nHello.\n",
        "format: html: theme: darkly",
    );
    let default = assert_css_parity(b"---\ntitle: Test\n---\n\nHello.\n", "default theme");
    assert_ne!(
        themed, default,
        "format-scoped darkly must change the compiled CSS relative to the \
         default theme (if equal, both pipelines dropped the format-scoped key)"
    );
}

/// `theme: none` opt-out must also agree (ships the static DEFAULT_CSS).
#[test]
fn theme_none_css_identical_between_html_and_preview_pipelines() {
    assert_css_parity(
        b"---\ntitle: Test\ntheme: none\n---\n\nHello.\n",
        "theme: none",
    );
}
