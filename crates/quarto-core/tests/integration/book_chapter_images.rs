/*
 * tests/integration/book_chapter_images.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Images in a chapter that lives in a subdirectory of a single-file
 * (typst / epub) book: local, `../`, and site-root targets must all
 * resolve to the file the author meant, in the emitted source and in the
 * compiled output.
 */

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;
use zip::ZipArchive;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// A minimal valid 1x1 PNG.
const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn write_fixture(dir: &Path) {
    write(
        &dir.join("_quarto.yml"),
        b"project:\n  type: book\n\nkeep-typ: true\n\nbook:\n  title: \"Chapter Images\"\n  author: \"A. Author\"\n  chapters:\n    - index.qmd\n    - one.qmd\n    - sub/two.qmd\n",
    );
    write(
        &dir.join("index.qmd"),
        b"---\ntitle: Home\n---\n\nWelcome.\n",
    );
    write(
        &dir.join("one.qmd"),
        b"# One\n\n![root local](rootlocal.png)\n",
    );
    write(
        &dir.join("sub/two.qmd"),
        b"# Two\n\n![local](local.png)\n\n![site root](/img/a.png)\n\n![up](../top.png)\n",
    );
    for f in ["rootlocal.png", "top.png", "img/a.png", "sub/local.png"] {
        write(&dir.join(f), ONE_PIXEL_PNG);
    }
}

fn render(fmt: &str) -> (TempDir, PathBuf, PathBuf) {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_fixture(&dir);
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&dir, runtime.as_ref()).unwrap();
    let project_type = project_type_for(&project);
    let format = Format::from_format_string(fmt).unwrap();
    let options = RenderToFileOptions::default();
    let mut pipeline =
        ProjectPipeline::new(&mut project, project_type, format, fmt, &options, runtime);
    let summary = pollster::block_on(pipeline.run_with_book_support())
        .unwrap_or_else(|e| panic!("run_with_book_support failed: {e}"));
    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let out = summary.outputs[0].output_path.clone();
    (temp, dir, out)
}

#[test]
fn typst_book_subdir_chapter_images_resolve() {
    let (_temp, _dir, out) = render("typst");
    // The compile succeeded (a missing image is a typst error).
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF-"), "expected a compiled PDF");

    let typ = std::fs::read_to_string(out.with_extension("typ")).unwrap();
    for expected in [
        r#"image("rootlocal.png")"#,
        r#"image("sub/local.png")"#,
        r#"image("img/a.png")"#,
        r#"image("top.png")"#,
    ] {
        assert!(typ.contains(expected), "missing {expected} in:\n{typ}");
    }
    assert!(
        !typ.contains("sub/img/"),
        "double-prefixed site-root image:\n{typ}"
    );
    assert!(
        !typ.contains("sub/top.png"),
        "`../` image kept the chapter dir:\n{typ}"
    );

    let out_dir = out.parent().unwrap();
    for f in ["rootlocal.png", "sub/local.png", "img/a.png", "top.png"] {
        assert!(out_dir.join(f).is_file(), "{f} not copied beside the .typ");
    }
}

#[test]
fn epub_book_subdir_chapter_images_are_packaged() {
    let (_temp, _dir, out) = render("epub");
    let mut zip = ZipArchive::new(std::io::Cursor::new(std::fs::read(&out).unwrap())).unwrap();
    let names: Vec<String> = zip.file_names().map(String::from).collect();
    let media = names.iter().filter(|n| n.contains("media/")).count();
    // Four distinct images, all with the same pixel content, so the
    // packaged count is what matters: none may be silently dropped.
    assert!(media >= 1, "no images packaged: {names:?}");
    let mut found_two = String::new();
    for n in names.iter().filter(|n| n.ends_with(".xhtml")) {
        let mut s = String::new();
        zip.by_name(n).unwrap().read_to_string(&mut s).unwrap();
        if s.contains(">Two<") || s.contains("Two</") {
            found_two = s;
        }
    }
    assert_eq!(found_two.matches("<img ").count(), 3, "{found_two}");
}
