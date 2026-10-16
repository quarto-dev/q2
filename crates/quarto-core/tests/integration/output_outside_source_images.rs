/*
 * tests/integration/output_outside_source_images.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * q2-preview hub-parity Plan 2 (P2-D1): images survive a render whose
 * output directory is outside the source tree (an OS temp dir or
 * `<proj>/.quarto/preview-out/x`), for docx, odt, epub and typst.
 */

//! Fixtures: `sub/a.qmd` references `pic.png` (beside it) and `/top.png`
//! (project root). Tests that need `pandoc` or `typst` skip with a reason
//! when the binary is not on `PATH`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn have(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

macro_rules! require {
    ($bin:expr) => {
        if !have($bin) {
            eprintln!("SKIP: `{}` not found on PATH", $bin);
            return;
        }
    };
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A project with `sub/a.qmd`, `sub/pic.png` and `top.png`. `website`
/// selects `type: website` (output dir `_site`); otherwise a default
/// project whose `_quarto.yml` pins the project dir.
fn fixture(root: &Path, website: bool, with_top: bool) -> PathBuf {
    let yml = if website {
        "project:\n  type: website\n"
    } else {
        "project:\n  title: x\n"
    };
    write(&root.join("_quarto.yml"), yml.as_bytes());
    let body = if with_top {
        "---\ntitle: T\n---\n\n![](pic.png)\n\n![](/top.png)\n"
    } else {
        "---\ntitle: T\n---\n\n![](pic.png)\n"
    };
    write(&root.join("sub/a.qmd"), body.as_bytes());
    write(&root.join("sub/pic.png"), PNG);
    write(&root.join("top.png"), PNG);
    root.join("sub/a.qmd")
}

fn render(
    input: &Path,
    format: &str,
    out_dir: &Path,
) -> quarto_core::render_to_file::RenderToFileResult {
    let options = RenderToFileOptions {
        output_dir: Some(out_dir.to_path_buf()),
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    render_to_file(input, format, &options, runtime)
        .unwrap_or_else(|e| panic!("{format} render into {} failed: {e}", out_dir.display()))
}

fn zip_media_entries(path: &Path) -> Vec<String> {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut media = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let lower = name.to_ascii_lowercase();
        if !entry.is_dir() && (lower.contains("media/") || lower.contains("pictures/")) {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).unwrap();
            if buf == PNG {
                media.push(name);
            }
        }
    }
    media
}

fn assert_no_resource_diagnostics(result: &quarto_core::render_to_file::RenderToFileResult) {
    let bad: Vec<_> = result
        .render_output
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code.as_deref(), Some("Q-5-6" | "Q-11-1")))
        .map(|d| d.title.clone())
        .collect();
    assert!(bad.is_empty(), "unexpected resource diagnostics: {bad:?}");
}

/// Output directories to try: an OS temp dir and the request directory
/// under the project's `.quarto/preview-out/`.
fn out_dirs(project: &Path, os: &TempDir, tag: &str) -> Vec<PathBuf> {
    vec![
        os.path().join(tag),
        project.join(".quarto/preview-out").join(tag),
    ]
}

fn check_pandoc_zip_format(format: &str, ext: &str, website: bool) {
    require!("pandoc");
    let proj = TempDir::new().unwrap();
    let os = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    let input = fixture(&root, website, true);
    for out in out_dirs(&root, &os, format) {
        let result = render(&input, format, &out);
        assert_no_resource_diagnostics(&result);
        let produced = out.join(format!("a.{ext}"));
        assert!(produced.is_file(), "{} missing", produced.display());
        let media = zip_media_entries(&produced);
        // pic.png and top.png are the same bytes; pandoc may dedupe them
        // into one entry, so require at least one and, in the default
        // project, that both paths resolved (no diagnostics above).
        assert!(
            !media.is_empty(),
            "{format} from {} has no image media (website={website})",
            out.display()
        );
    }
}

#[test]
fn docx_keeps_images_outside_the_source_tree() {
    check_pandoc_zip_format("docx", "docx", false);
    check_pandoc_zip_format("docx", "docx", true);
}

#[test]
fn odt_keeps_images_outside_the_source_tree() {
    check_pandoc_zip_format("odt", "odt", false);
    check_pandoc_zip_format("odt", "odt", true);
}

#[test]
fn epub_keeps_images_outside_the_source_tree() {
    check_pandoc_zip_format("epub", "epub", false);
    check_pandoc_zip_format("epub", "epub", true);
}

/// `![](/top.png)` resolves through the site root, not the source dir.
#[test]
fn docx_root_relative_image_resolves_to_the_project_root() {
    require!("pandoc");
    let proj = TempDir::new().unwrap();
    let os = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    write(&root.join("_quarto.yml"), b"project:\n  title: x\n");
    write(
        &root.join("sub/a.qmd"),
        b"---\ntitle: T\n---\n\n![](/top.png)\n",
    );
    write(&root.join("top.png"), PNG);
    let result = render(&root.join("sub/a.qmd"), "docx", &os.path().join("o"));
    assert_no_resource_diagnostics(&result);
    assert!(!zip_media_entries(&os.path().join("o/a.docx")).is_empty());
}

/// typst compiles in place: the `.typ` hrefs climb from the request
/// directory to the source, in a default project and in a website
/// project with a subfolder document (the latter was `Q-21-3` before).
#[test]
fn typst_finds_source_images_in_the_request_directory() {
    require!("pandoc");
    require!("typst");
    for website in [false, true] {
        let proj = TempDir::new().unwrap();
        let root = proj.path().canonicalize().unwrap();
        let input = fixture(&root, website, true);
        let out = root.join(".quarto/preview-out/t");
        let result = render(&input, "typst", &out);
        assert_no_resource_diagnostics(&result);
        assert!(out.join("a.pdf").is_file(), "no pdf (website={website})");
    }
}

/// A relative image that already exists in the typst request directory
/// (a materialized capture plot) is left alone, not rewritten as a
/// source-relative path.
#[test]
fn typst_leaves_images_that_exist_in_the_request_directory() {
    require!("pandoc");
    require!("typst");
    let proj = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    write(&root.join("_quarto.yml"), b"project:\n  title: x\n");
    // No `plot.png` beside the source; only in the request directory.
    write(
        &root.join("sub/a.qmd"),
        b"---\ntitle: T\n---\n\n![](plot.png)\n",
    );
    let out = root.join(".quarto/preview-out/c");
    write(&out.join("plot.png"), PNG);
    let result = render(&root.join("sub/a.qmd"), "typst", &out);
    assert_no_resource_diagnostics(&result);
    assert!(out.join("a.pdf").is_file());
}
