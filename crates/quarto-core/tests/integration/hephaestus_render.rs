/*
 * tests/integration/hephaestus_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for hephaestus plot-document (`.hep`) rendering
 * (bd-3qych45b, phase 1).
 */

//! `![](plot.hep)` in an HTML-family render becomes an SVG the page can
//! display: the transform reads the document, renders it with
//! hephaestus's renderer-free SVG backend, stores the markup as a
//! page-scoped artifact under `figure-html/`, and points the image at
//! it. These tests drive the real `render_to_file` path so they cover
//! the artifact flush and the URL the resolver hands back, not just the
//! AST rewrite.
//!
//! The fixture `tests/fixtures/hephaestus/basic.hep` is the document
//! hephaestus's own `document_save` example writes (two panels, a
//! scatter and a line, 10,553 bytes). Regenerate it from
//! `external-sources/hephaestus` with
//! `cargo run --example document_save --features document-write`.
//!
//! Plan: `claude-notes/plans/2026-09-17-hephaestus-hep-integration.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const FIXTURE_HEP: &[u8] = include_bytes!("../fixtures/hephaestus/basic.hep");

/// Q-codes of the `image` subsystem this phase introduces.
const CODE_NOT_FOUND: &str = "Q-18-1";
const CODE_INVALID: &str = "Q-18-2";
const CODE_BRAND_COLOR: &str = "Q-18-4";
/// The resource collector's own "referenced resource not found".
const CODE_RESOURCE_NOT_FOUND: &str = "Q-5-6";

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn write_text(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn write_bytes(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Render `doc.qmd` (with `body` as its markdown) next to the given
/// files, for `format`. Returns the temp dir (kept alive) and the
/// result.
fn render_doc(format: &str, body: &str, files: &[(&str, &[u8])]) -> (TempDir, RenderToFileResult) {
    render_doc_with_meta(format, "", body, files)
}

/// Like [`render_doc`], with extra front-matter lines.
fn render_doc_with_meta(
    format: &str,
    extra_meta: &str,
    body: &str,
    files: &[(&str, &[u8])],
) -> (TempDir, RenderToFileResult) {
    let temp = TempDir::new().unwrap();
    for (name, bytes) in files {
        write_bytes(&temp.path().join(name), bytes);
    }
    let qmd_path = temp.path().join("doc.qmd");
    write_text(
        &qmd_path,
        &format!("---\ntitle: Plot\n{extra_meta}\n---\n\n{body}\n"),
    );
    let options = RenderToFileOptions::default();
    let result =
        render_to_file(&qmd_path, format, &options, runtime_arc()).expect("single-doc render");
    (temp, result)
}

/// Every `<img … src="…">` URL in document order.
fn img_srcs(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search = html;
    while let Some(start) = search.find("<img") {
        let tag_end = search[start..]
            .find('>')
            .map_or(search.len(), |e| start + e);
        let tag = &search[start..tag_end];
        if let Some(s) = tag.find("src=\"") {
            let after = &tag[s + 5..];
            let end = after.find('"').expect("unterminated src attribute");
            out.push(after[..end].to_string());
        }
        search = &search[tag_end..];
    }
    out
}

fn diagnostic_codes(result: &RenderToFileResult) -> Vec<String> {
    result
        .render_output
        .diagnostics
        .iter()
        .filter_map(|d| d.code.clone())
        .collect()
}

/// The single converted `<img>` URL and the SVG file it points at.
fn converted_svg(result: &RenderToFileResult) -> (String, PathBuf) {
    let html = read(&result.output_path);
    let srcs = img_srcs(&html);
    assert_eq!(srcs.len(), 1, "expected exactly one <img>, got {srcs:?}");
    let src = srcs[0].clone();
    let on_disk = result.output_path.parent().unwrap().join(&src);
    (src, on_disk)
}

// ── The core behavior ────────────────────────────────────────────────────

#[test]
fn hep_image_becomes_svg_artifact_under_figure_html() {
    let (_temp, result) = render_doc(
        "html",
        "![A plot](basic.hep)",
        &[("basic.hep", FIXTURE_HEP)],
    );
    let (src, on_disk) = converted_svg(&result);

    assert!(
        src.starts_with("doc_files/figure-html/basic-") && src.ends_with(".svg"),
        "img src must point at a page-scoped SVG artifact, got {src}"
    );
    assert!(
        on_disk.is_file(),
        "SVG artifact must be flushed to {}",
        on_disk.display()
    );
    let svg = read(&on_disk);
    assert!(
        svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""),
        "not an SVG: {svg:.80}"
    );
    assert!(
        svg.contains("<text"),
        "hephaestus SVG keeps text as <text> elements"
    );
    assert_eq!(
        diagnostic_codes(&result),
        Vec::<String>::new(),
        "clean render emits no diagnostics"
    );
}

/// The rendered `<img>` keeps the author's alt text and gains
/// `img-fluid` from the responsive-image pass that runs after us, so
/// the SVG scales with its container.
#[test]
fn converted_image_keeps_alt_and_is_responsive() {
    let (_temp, result) = render_doc(
        "html",
        "![A plot](basic.hep)",
        &[("basic.hep", FIXTURE_HEP)],
    );
    let html = read(&result.output_path);
    let img_tag = {
        let start = html.find("<img").expect("an <img> tag");
        let end = html[start..].find('>').unwrap() + start;
        &html[start..=end]
    };
    assert!(
        img_tag.contains("alt=\"A plot\""),
        "alt text lost: {img_tag}"
    );
    assert!(
        img_tag.contains("img-fluid"),
        "img-fluid missing: {img_tag}"
    );
}

/// Same document, same bytes: the bundled Roboto faces make shaping
/// machine-independent, so two renders agree byte for byte.
#[test]
fn hep_render_is_deterministic() {
    let (_t1, r1) = render_doc("html", "![](basic.hep)", &[("basic.hep", FIXTURE_HEP)]);
    let (_t2, r2) = render_doc("html", "![](basic.hep)", &[("basic.hep", FIXTURE_HEP)]);
    let (src1, svg1) = converted_svg(&r1);
    let (src2, svg2) = converted_svg(&r2);
    assert_eq!(src1, src2, "artifact name is content-derived");
    assert_eq!(
        std::fs::read(&svg1).unwrap(),
        std::fs::read(&svg2).unwrap(),
        "SVG bytes must not depend on the render"
    );
}

/// The SVG names the bundled family, never a system font: that is what
/// keeps the markup identical across machines and matching what the
/// browser client draws.
#[test]
fn svg_text_uses_bundled_roboto() {
    let (_temp, result) = render_doc("html", "![](basic.hep)", &[("basic.hep", FIXTURE_HEP)]);
    let (_, on_disk) = converted_svg(&result);
    let svg = read(&on_disk);
    assert!(
        svg.contains("font-family=\"Roboto\"") || svg.contains("font-family=\"sans-serif\""),
        "expected the bundled family or the generic it maps to; got {:?}",
        svg.split("font-family=").nth(1).map(|s| &s[..30])
    );
    for system in ["Helvetica", "Arial", "DejaVu", "Segoe"] {
        assert!(
            !svg.contains(system),
            "system font {system} leaked into the SVG"
        );
    }
}

/// Explicit `width` / `height` attributes size the rendered scene.
#[test]
fn explicit_width_and_height_size_the_svg() {
    let (_temp, result) = render_doc(
        "html",
        "![](basic.hep){width=300 height=200}",
        &[("basic.hep", FIXTURE_HEP)],
    );
    let (_, on_disk) = converted_svg(&result);
    let svg = read(&on_disk);
    assert!(
        svg.contains("width=\"300\" height=\"200\" viewBox=\"0 0 300 200\""),
        "SVG root must carry the requested size; got {:.200}",
        svg
    );
}

/// Without attributes the writer's size hint (900 × 420 for the
/// fixture) is used.
#[test]
fn size_defaults_to_the_documents_hint() {
    let (_temp, result) = render_doc("html", "![](basic.hep)", &[("basic.hep", FIXTURE_HEP)]);
    let (_, on_disk) = converted_svg(&result);
    let svg = read(&on_disk);
    assert!(
        svg.contains("width=\"900\" height=\"420\""),
        "expected the fixture's 900x420 hint; got {:.200}",
        svg
    );
}

/// Two images of the same document on one page get distinct id
/// prefixes, so their gradient / clip-path ids cannot collide when both
/// SVGs are inlined.
#[test]
fn two_hep_images_get_distinct_svg_id_prefixes() {
    let (_temp, result) = render_doc(
        "html",
        "![](basic.hep)\n\n![](basic.hep){width=300 height=200}",
        &[("basic.hep", FIXTURE_HEP)],
    );
    let html = read(&result.output_path);
    let srcs = img_srcs(&html);
    assert_eq!(srcs.len(), 2, "{srcs:?}");
    assert_ne!(srcs[0], srcs[1], "different sizes are different artifacts");
    let page_dir = result.output_path.parent().unwrap();
    let id_prefix = |src: &str| {
        let svg = read(&page_dir.join(src));
        let at = svg.find("clipPath id=\"").expect("a clipPath id");
        svg[at + 13..].split('"').next().unwrap().to_string()
    };
    assert_ne!(id_prefix(&srcs[0]), id_prefix(&srcs[1]));
}

// ── Formats ──────────────────────────────────────────────────────────────

#[test]
fn revealjs_target_also_converts() {
    let (_temp, result) = render_doc("revealjs", "![](basic.hep)", &[("basic.hep", FIXTURE_HEP)]);
    let (src, on_disk) = converted_svg(&result);
    assert!(src.ends_with(".svg"), "{src}");
    assert!(on_disk.is_file());
}

// ── Left alone ───────────────────────────────────────────────────────────

#[test]
fn non_hep_images_are_untouched() {
    let (_temp, result) = render_doc(
        "html",
        "![](photo.png)\n\n![](https://example.com/remote.hep)",
        &[("photo.png", b"\x89PNG\r\n\x1a\n")],
    );
    let html = read(&result.output_path);
    assert_eq!(
        img_srcs(&html),
        vec![
            "photo.png".to_string(),
            "https://example.com/remote.hep".to_string()
        ]
    );
    assert_eq!(diagnostic_codes(&result), Vec::<String>::new());
}

// ── Failure modes: warn, leave the image, keep rendering ─────────────────

/// A missing `.hep` is reported twice, on purpose: `Q-18-1` says the
/// plot was not rendered, and the resource collector's `Q-5-6` says the
/// file could not be copied beside the page — the same warning any
/// missing image gets.
#[test]
fn missing_hep_file_warns_and_leaves_the_image() {
    let (_temp, result) = render_doc("html", "![](missing.hep)", &[]);
    let html = read(&result.output_path);
    assert_eq!(img_srcs(&html), vec!["missing.hep".to_string()]);
    assert_eq!(
        diagnostic_codes(&result),
        vec![
            CODE_NOT_FOUND.to_string(),
            CODE_RESOURCE_NOT_FOUND.to_string()
        ]
    );
}

#[test]
fn non_hephaestus_bytes_warn_and_leave_the_image() {
    let (_temp, result) = render_doc("html", "![](bogus.hep)", &[("bogus.hep", b"not a plot")]);
    let html = read(&result.output_path);
    assert_eq!(img_srcs(&html), vec!["bogus.hep".to_string()]);
    assert_eq!(diagnostic_codes(&result), vec![CODE_INVALID.to_string()]);
}

// ── brand.yml colors → the plot's palette ────────────────────────────────

/// A brand declared by the document (`brand: brand.yml`) recolors the
/// plot: `background` becomes hephaestus's `paper`, `foreground` its
/// `ink`, `primary` its `accent`. The page-clearing rect and the
/// ink-colored chrome both show it.
#[test]
fn brand_colors_map_onto_the_plot_palette() {
    let brand =
        b"color:\n  background: \"#101820\"\n  foreground: \"#f2f2f2\"\n  primary: \"#ff6f61\"\n";
    let (_temp, result) = render_doc_with_meta(
        "html",
        "brand: brand.yml",
        "![](basic.hep)",
        &[("basic.hep", FIXTURE_HEP), ("brand.yml", brand)],
    );
    let (_, on_disk) = converted_svg(&result);
    let svg = read(&on_disk);
    assert!(
        svg.contains("<rect width=\"900\" height=\"420\" fill=\"#101820\"/>"),
        "page-clearing rect must use the brand background; got {:.400}",
        svg
    );
    assert!(
        svg.contains("fill=\"#f2f2f2\""),
        "ink-colored chrome must use the brand foreground"
    );
    assert!(
        !svg.contains("fill=\"#ffffff\""),
        "the document's own white paper must be gone"
    );
    assert_eq!(diagnostic_codes(&result), Vec::<String>::new());
}

/// A brand color that is not a hex value cannot become a palette
/// anchor: warn once (`Q-18-4`) and keep the document's own color for
/// that slot. The other slots still apply.
#[test]
fn non_hex_brand_color_warns_and_keeps_the_documents_color() {
    let brand = b"color:\n  background: rebeccapurple\n  foreground: \"#f2f2f2\"\n";
    let (_temp, result) = render_doc_with_meta(
        "html",
        "brand: brand.yml",
        "![](basic.hep)\n\n![](basic.hep){width=300 height=200}",
        &[("basic.hep", FIXTURE_HEP), ("brand.yml", brand)],
    );
    let html = read(&result.output_path);
    let srcs = img_srcs(&html);
    assert_eq!(srcs.len(), 2);
    let svg = read(&result.output_path.parent().unwrap().join(&srcs[0]));
    assert!(
        svg.contains("<rect width=\"900\" height=\"420\" fill=\"#ffffff\"/>"),
        "paper stays the document's own white"
    );
    assert!(svg.contains("fill=\"#f2f2f2\""), "ink still applies");
    assert_eq!(
        diagnostic_codes(&result),
        vec![CODE_BRAND_COLOR.to_string()],
        "one warning per document, not per image"
    );
}

/// Without a brand the document's own palette is used unchanged.
#[test]
fn no_brand_keeps_the_documents_palette() {
    let (_temp, result) = render_doc("html", "![](basic.hep)", &[("basic.hep", FIXTURE_HEP)]);
    let (_, on_disk) = converted_svg(&result);
    let svg = read(&on_disk);
    assert!(svg.contains("<rect width=\"900\" height=\"420\" fill=\"#ffffff\"/>"));
}
