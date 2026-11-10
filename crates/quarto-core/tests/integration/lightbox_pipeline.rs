/*
 * tests/integration/lightbox_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for lightbox images (bd-kcpkgftw): the wrapping link,
 * the GLightbox runtime and its init, and when nothing ships — through the
 * real `ProjectPipeline`.
 */

//! Same harness shape as `anchor_sections_pipeline.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// The shipped files must be byte-identical to the resources.
const GLIGHTBOX_JS: &[u8] = include_bytes!("../../../../resources/js/glightbox/glightbox.min.js");
const GLIGHTBOX_CSS: &[u8] = include_bytes!("../../../../resources/js/glightbox/glightbox.min.css");

/// A real 1×1 PNG, so the resource copier sees a valid image.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn canonical(path: &Path) -> PathBuf {
    quarto_system_runtime::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Render the project `fixture` writes; return the output root and each
/// output's (root-relative href, html).
fn render_project(
    output_dir: &str,
    fixture: impl FnOnce(&Path),
) -> (PathBuf, Vec<(String, String)>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
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
    let summary = pollster::block_on(pipeline.run_with_book_support()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    std::mem::forget(temp);

    let root = project_dir.join(output_dir);
    let outputs = summary
        .outputs
        .iter()
        .map(|out| {
            let href = out
                .output_path
                .strip_prefix(&root)
                .unwrap_or(&out.output_path)
                .to_string_lossy()
                .replace('\\', "/");
            (href, std::fs::read_to_string(&out.output_path).unwrap())
        })
        .collect();
    (root, outputs)
}

fn find_html<'a>(outputs: &'a [(String, String)], href: &str) -> &'a str {
    &outputs
        .iter()
        .find(|(h, _)| h == href)
        .unwrap_or_else(|| panic!("no output for {href}"))
        .1
}

fn site(dir: &Path) {
    write(
        &dir.join("_quarto.yml"),
        b"project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: Site\n",
    );
    write(&dir.join("img.png"), PNG);
    write(
        &dir.join("index.qmd"),
        b"---\ntitle: Home\nlightbox: true\n---\n\n![One](img.png)\n\n![Two](img.png){group=g}\n",
    );
    write(
        &dir.join("plain.qmd"),
        b"---\ntitle: Plain\n---\n\n![Not lightboxed](img.png)\n",
    );
    write(
        &dir.join("sub/deep.qmd"),
        b"---\ntitle: Deep\n---\n\n![Explicit](../img.png){.lightbox}\n",
    );
}

#[test]
fn lightboxed_page_ships_glightbox_once() {
    let (site_root, outputs) = render_project("_site", site);
    let index = find_html(&outputs, "index.html");

    assert!(
        index.contains("<a href=\"img.png\" class=\"lightbox\" data-gallery=\"quarto-lightbox-gallery-1\" title=\"One\"><img"),
        "first image not wrapped:\n{index}"
    );
    assert!(
        index.contains("data-gallery=\"g\""),
        "group gallery missing:\n{index}"
    );
    assert!(
        index.contains("src=\"site_libs/glightbox/glightbox.min.js\""),
        "script tag missing:\n{index}"
    );
    assert!(
        index.contains("href=\"site_libs/glightbox/glightbox.min.css\""),
        "stylesheet link missing:\n{index}"
    );
    assert_eq!(
        index.matches("GLightbox(").count(),
        1,
        "init must appear exactly once:\n{index}"
    );

    let js = std::fs::read(site_root.join("site_libs/glightbox/glightbox.min.js")).unwrap();
    assert_eq!(js, GLIGHTBOX_JS, "shipped JS must be the resource");
    let css = std::fs::read(site_root.join("site_libs/glightbox/glightbox.min.css")).unwrap();
    assert_eq!(css, GLIGHTBOX_CSS, "shipped CSS must be the resource");
}

#[test]
fn nested_page_links_the_project_runtime() {
    let (_, outputs) = render_project("_site", site);
    let deep = find_html(&outputs, "sub/deep.html");
    assert!(
        deep.contains("<a href=\"../img.png\" class=\"lightbox\""),
        "explicit image not wrapped:\n{deep}"
    );
    assert!(
        deep.contains("src=\"../site_libs/glightbox/glightbox.min.js\""),
        "script must be page-relative:\n{deep}"
    );
}

#[test]
fn page_without_lightbox_images_ships_nothing() {
    let (_, outputs) = render_project("_site", site);
    let plain = find_html(&outputs, "plain.html");
    assert!(!plain.contains("class=\"lightbox\""), "no link:\n{plain}");
    assert!(!plain.contains("glightbox"), "no runtime:\n{plain}");
    assert!(!plain.contains("GLightbox"), "no init:\n{plain}");
}

/// Q1 #11520: with `lightbox: true` on a book, the cover image (which the
/// book transform marks `.nolightbox`) stays unwrapped, while an ordinary
/// chapter figure is lightboxed.
#[test]
fn book_cover_image_escapes_lightbox() {
    let (_, outputs) = render_project("_book", |dir| {
        write(
            &dir.join("_quarto.yml"),
            b"project:\n  type: book\nbook:\n  title: Minibook\n  cover-image: cover.png\n  chapters:\n    - index.qmd\n    - ch1.qmd\nlightbox: true\n",
        );
        write(&dir.join("cover.png"), PNG);
        write(&dir.join("fig.png"), PNG);
        write(
            &dir.join("index.qmd"),
            b"# Preface {.unnumbered}\n\nWelcome.\n",
        );
        write(&dir.join("ch1.qmd"), b"# One\n\n![A figure](fig.png)\n");
    });
    let index = find_html(&outputs, "index.html");
    let cover = index.find("quarto-cover-image").expect("cover image");
    let tag_start = index[..cover].rfind('<').unwrap();
    assert!(
        index[tag_start..].starts_with("<img"),
        "cover must be a bare <img>:\n{index}"
    );
    let before = &index[..tag_start];
    assert!(
        !before.trim_end().ends_with("class=\"lightbox\">")
            && !before[before.rfind('<').unwrap()..].contains("lightbox"),
        "cover must not sit inside a lightbox link:\n{index}"
    );

    let ch1 = find_html(&outputs, "ch1.html");
    assert!(
        ch1.contains("class=\"lightbox\"") && ch1.contains("glightbox.min.js"),
        "chapter figure must be lightboxed:\n{ch1}"
    );
}
