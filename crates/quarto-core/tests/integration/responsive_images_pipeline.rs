/*
 * tests/integration/responsive_images_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for bd-images-no-max-width-e5ywgnma: body images
 * carry Bootstrap's `img-fluid` class so an unsized image is capped at
 * the width of its container instead of laying out at intrinsic width.
 */

//! End-to-end tests for the responsive-image pass.
//!
//! These drive a real `render_to_file` and inspect the emitted `<img>`
//! tags, mirroring the shared acceptance fixture at
//! `q2-positron-docs/llms-info/repros/images-no-max-width/`. Its four
//! cases — plain, `width=`, `height=`, and inline — are reproduced here
//! as `four_cases_match_quarto_1`, which pins the exact 3-of-4 split
//! Quarto 1 produces.
//!
//! The per-node rules (which images qualify, opt-outs, idempotence) are
//! unit-tested in `crates/quarto-core/src/transforms/responsive_image.rs`;
//! what these tests add is proof that the transform is actually wired
//! into the pipeline the `q2` binary runs.

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Render `source` as a single document and return the emitted HTML.
fn render_html(source: &str) -> String {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(&qmd_path, source);
    // The tests reference `wide.png` by name only; nothing reads it,
    // but resource collection is happier when the file exists.
    write_file(&temp.path().join("wide.png"), "not-really-a-png");

    let options = RenderToFileOptions::default();
    let result =
        render_to_file(&qmd_path, "html", &options, runtime_arc()).expect("single-doc render");
    std::fs::read_to_string(&result.output_path).expect("read rendered HTML")
}

/// Every `<img …>` tag in `html`, in document order.
fn img_tags(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search = html;
    while let Some(start) = search.find("<img ") {
        let after = &search[start..];
        let end = after.find('>').expect("malformed <img>: no closing '>'");
        out.push(after[..=end].to_string());
        search = &after[end..];
    }
    out
}

/// The `<img>` tags whose `src` is `wide.png` — i.e. body content,
/// excluding any navbar/footer chrome.
fn body_img_tags(html: &str) -> Vec<String> {
    img_tags(html)
        .into_iter()
        .filter(|t| t.contains("src=\"wide.png\""))
        .collect()
}

fn has_img_fluid(tag: &str) -> bool {
    // `class="…"` may hold several classes; match the token, not a
    // substring of some other class.
    tag.split("class=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .any(|classes| classes.split_whitespace().any(|c| c == "img-fluid"))
}

// ── The core regression ───────────────────────────────────────────────────

/// bd-images-no-max-width-e5ywgnma: the bug as reported. A plain image
/// with no author-supplied sizing must be constrained to its container.
#[test]
fn plain_image_gets_img_fluid() {
    let html = render_html("---\ntitle: T\n---\n\n![A wide screenshot](wide.png)\n");
    let tags = body_img_tags(&html);
    assert_eq!(tags.len(), 1, "expected one body <img>; got {tags:?}");
    assert!(
        has_img_fluid(&tags[0]),
        "unsized body image must carry img-fluid; got: {}",
        tags[0]
    );
}

/// The four cases of the shared acceptance fixture, with the exact
/// split Quarto 1 produces: plain, `width=` and inline are tagged;
/// `height=` is deliberately skipped.
#[test]
fn four_cases_match_quarto_1() {
    let html = render_html(
        "---\ntitle: T\n---\n\n\
         ![A wide screenshot](wide.png)\n\n\
         ![A wide screenshot, sized](wide.png){width=450}\n\n\
         ![A wide screenshot, height-sized](wide.png){height=100}\n\n\
         Text before ![inline](wide.png) text after.\n",
    );
    let tags = body_img_tags(&html);
    assert_eq!(tags.len(), 4, "expected four body <img>; got {tags:?}");

    let tagged: Vec<bool> = tags.iter().map(|t| has_img_fluid(t)).collect();
    assert_eq!(
        tagged,
        vec![true, true, false, true],
        "img-fluid must land on the plain, width-sized and inline images \
         but not the height-sized one; got:\n{}",
        tags.join("\n")
    );
}

// ── The two deliberate exclusions ─────────────────────────────────────────

/// An explicit `height` means the author fixed the vertical size;
/// `max-width:100%; height:auto` would fight it. Quarto 1 skips these
/// and so must we.
#[test]
fn image_with_explicit_height_is_skipped() {
    let html = render_html("---\ntitle: T\n---\n\n![cap](wide.png){height=100}\n");
    let tags = body_img_tags(&html);
    assert_eq!(tags.len(), 1, "expected one body <img>; got {tags:?}");
    assert!(
        !has_img_fluid(&tags[0]),
        "an image with an explicit height must not be made fluid; got: {}",
        tags[0]
    );
}

/// `data-no-responsive` is the per-image opt-out.
#[test]
fn data_no_responsive_opts_out() {
    let html = render_html("---\ntitle: T\n---\n\n![cap](wide.png){data-no-responsive=\"true\"}\n");
    let tags = body_img_tags(&html);
    assert_eq!(tags.len(), 1, "expected one body <img>; got {tags:?}");
    assert!(
        !has_img_fluid(&tags[0]),
        "data-no-responsive must suppress img-fluid; got: {}",
        tags[0]
    );
}

// ── The document-level switch ─────────────────────────────────────────────

/// `fig-responsive: false` turns the whole pass off, as in Quarto 1.
#[test]
fn fig_responsive_false_disables_the_pass() {
    let html = render_html(
        "---\ntitle: T\nfig-responsive: false\n---\n\n![cap](wide.png)\n\n\
         Inline ![i](wide.png) here.\n",
    );
    let tags = body_img_tags(&html);
    assert_eq!(tags.len(), 2, "expected two body <img>; got {tags:?}");
    assert!(
        tags.iter().all(|t| !has_img_fluid(t)),
        "fig-responsive: false must leave every image untagged; got:\n{}",
        tags.join("\n")
    );
}

/// `fig-responsive: true` is the default, so saying it explicitly
/// changes nothing.
#[test]
fn fig_responsive_true_is_the_default() {
    let html = render_html("---\ntitle: T\nfig-responsive: true\n---\n\n![cap](wide.png)\n");
    let tags = body_img_tags(&html);
    assert_eq!(tags.len(), 1, "expected one body <img>; got {tags:?}");
    assert!(
        has_img_fluid(&tags[0]),
        "fig-responsive: true must behave like the default; got: {}",
        tags[0]
    );
}

// ── The preview pipeline ──────────────────────────────────────────────────
//
// `q2 preview` and the hub-client don't run the html render pipeline —
// they run `build_q2_preview_transform_pipeline` and hand serialized
// AST to a React renderer. The transform is included there by default
// (that builder delegates to the same builder and filters by
// deny-list), but "included" and "reaches the output" are different
// claims, so these pin the AST the SPA actually receives.

use quarto_core::pipeline::render_qmd_to_preview_ast;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};

fn preview_test_project() -> ProjectContext {
    ProjectContext {
        dir: std::path::PathBuf::from("/project"),
        config: quarto_core::project::ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/test.qmd")],
        output_dir: std::path::PathBuf::from("/project"),
        ..Default::default()
    }
}

/// Render `content` through the preview pipeline for `pseudo_format`,
/// returning the serialized AST JSON the SPA consumes.
fn render_preview_ast(content: &[u8], pseudo_format: &str) -> String {
    let project = preview_test_project();
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = quarto_core::format::Format::from_format_string(pseudo_format)
        .expect("pseudo-format resolves");
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    pollster::block_on(render_qmd_to_preview_ast(
        content,
        "test.qmd",
        &mut ctx,
        runtime_arc(),
        None,
        vec![],
    ))
    .expect("preview pipeline render")
    .ast_json
}

/// `q2-preview` is `format: html` under preview. It must tag images
/// exactly as the render pipeline does, or the preview shows an
/// overflowing image for a page that renders correctly.
#[test]
fn preview_pipeline_tags_images() {
    let ast = render_preview_ast(b"---\ntitle: T\n---\n\n![cap](wide.png)\n", "q2-preview");
    assert!(
        ast.contains("img-fluid"),
        "the preview AST must carry img-fluid so preview matches render; got:\n{ast}"
    );
}

/// The `height` exclusion holds in preview too — the per-image rules
/// are pipeline-independent, and a divergence here would be invisible
/// to every render-path test.
#[test]
fn preview_pipeline_honors_the_height_exclusion() {
    let ast = render_preview_ast(
        b"---\ntitle: T\n---\n\n![cap](wide.png){height=100}\n",
        "q2-preview",
    );
    assert!(
        !ast.contains("img-fluid"),
        "an explicit height must be honored in preview as in render; got:\n{ast}"
    );
}

/// `q2-slides` is how every live reveal deck reaches the pipeline.
/// It is legacy as a user-facing format, but since the bd-vwp4y5ku
/// convergence `format: revealjs` in hub-client and `q2 preview` is
/// rewritten to it by `map_format_for_preview` in the WASM entry — so
/// this is the case that covers the React deck people actually use.
///
/// Quarto 1 does not tag deck images, and neither must q2, in either
/// pipeline. This is also the case an identifier-based format gate
/// would get wrong, since `q2-slides` resolves to
/// `FormatIdentifier::Html`.
#[test]
fn slides_preview_pipeline_does_not_tag_images() {
    let ast = render_preview_ast(
        b"---\ntitle: T\nformat: revealjs\n---\n\n## S\n\nText ![cap](wide.png) here.\n",
        "q2-slides",
    );
    assert!(
        !ast.contains("img-fluid"),
        "reveal decks are not tagged in either engine; got:\n{ast}"
    );
}
