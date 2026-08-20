/*
 * tests/integration/video_shortcode_preview.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-5b21rbaq: the {{< video >}} shortcode must embed an <iframe> in the
 * q2-preview pipeline, exactly as it does in the html render pipeline.
 */

//! End-to-end regression: the built-in `video` Lua shortcode in the
//! **preview** pipeline.
//!
//! The video shortcode (`resources/extensions/quarto/video/video.lua`)
//! gates on `quarto.doc.is_format("html:js")`: when true it emits an
//! `<iframe>` player, otherwise it falls through to a plain
//! `pandoc.Link`. The Lua `FORMAT` global is set from the pipeline's
//! `target_format`. The html render pipeline passes `"html"`, so the
//! check passes and an iframe is emitted. The q2-preview pipeline passes
//! the pseudo-format `"q2-preview"`, which `is_html_output` did not
//! recognize — so the shortcode degraded to a bare link in preview and
//! the hub-client.
//!
//! These tests render the same video document through both pipelines
//! in-process and assert both produce an embedded iframe. The
//! `q2-slides` (revealjs preview) case is covered too.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::pipeline::{HtmlRenderConfig, render_qmd_to_html, render_qmd_to_preview_ast};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const VIDEO_DOC: &[u8] = b"---\ntitle: Test\n---\n\n{{< video https://youtu.be/sAWFsP0Bbbk >}}\n";

const VIDEO_DOC_REVEAL: &[u8] =
    b"---\ntitle: Test\nformat: revealjs\n---\n\n## Slide\n\n{{< video https://youtu.be/sAWFsP0Bbbk >}}\n";

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

/// A runtime with a private, throwaway cache so renders never share
/// state across tests.
fn fresh_runtime() -> Arc<dyn SystemRuntime> {
    let temp = TempDir::new().unwrap();
    let cache_dir = temp.path().to_path_buf();
    std::mem::forget(temp);
    Arc::new(NativeRuntime::with_cache_dir(cache_dir))
}

/// Render `content` through the html render pipeline, returning the HTML.
fn render_html(content: &[u8]) -> String {
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
    .expect("html pipeline render")
    .html
}

/// Render `content` through the q2-preview pipeline for `pseudo_format`
/// (`"q2-preview"` or `"q2-slides"`), returning the serialized AST JSON.
fn render_preview_ast(content: &[u8], pseudo_format: &str) -> String {
    let project = test_project();
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = Format::from_format_string(pseudo_format).expect("pseudo-format");
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
    .expect("q2-preview pipeline render")
    .ast_json
}

/// Sanity baseline: the html render pipeline embeds an iframe. This is
/// the behavior the preview pipeline must match; if this fails, the test
/// harness isn't loading the built-in video extension at all.
#[test]
fn video_shortcode_html_render_embeds_iframe() {
    let html = render_html(VIDEO_DOC);
    assert!(
        html.contains("<iframe") && html.contains("youtube.com/embed"),
        "html render should embed a YouTube iframe, got:\n{html}"
    );
}

/// Guard: the html (non-reveal) render keeps the responsive `quarto-video`
/// wrapper and must NOT carry reveal's `r-stretch` class (that's reveal-only —
/// bd-5b21rbaq Phase 5).
#[test]
fn video_shortcode_html_render_has_no_r_stretch() {
    let html = render_html(VIDEO_DOC);
    assert!(
        html.contains("quarto-video") && !html.contains("r-stretch"),
        "html video must use the quarto-video wrapper and not r-stretch, got:\n{html}"
    );
}

/// The actual bug: the q2-preview pipeline must embed the iframe too,
/// not degrade to a plain link.
#[test]
fn video_shortcode_preview_embeds_iframe() {
    let ast_json = render_preview_ast(VIDEO_DOC, "q2-preview");
    assert!(
        ast_json.contains("youtube.com/embed") && ast_json.contains("iframe"),
        "q2-preview pipeline should embed a YouTube iframe (RawBlock), \
         but it appears to have degraded to a plain link. AST JSON:\n{ast_json}"
    );
}

/// The revealjs preview pseudo-format (`q2-slides`) must also embed the
/// iframe rather than degrade to a link.
#[test]
fn video_shortcode_slides_preview_embeds_iframe() {
    let ast_json = render_preview_ast(VIDEO_DOC_REVEAL, "q2-slides");
    assert!(
        ast_json.contains("youtube.com/embed") && ast_json.contains("iframe"),
        "q2-slides preview pipeline should embed a YouTube iframe, but it \
         appears to have degraded to a plain link. AST JSON:\n{ast_json}"
    );
    // Preview parity for the auto-stretch fix: because the Lua FORMAT resolves
    // to `revealjs` in slides preview, `is_format("revealjs")` fires and the
    // video iframe carries `r-stretch`, just like native reveal render.
    assert!(
        ast_json.contains("r-stretch"),
        "q2-slides preview video iframe should be auto-stretched (r-stretch), \
         matching native reveal render. AST JSON:\n{ast_json}"
    );
}
