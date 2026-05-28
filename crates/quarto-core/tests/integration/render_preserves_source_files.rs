/*
 * tests/render_preserves_source_files.rs
 *
 * Regression tests for bd-cfl67: `q2 render` truncates source images
 * referenced in qmd documents.
 *
 * The core invariant: rendering a document must NEVER modify any file
 * the user authored in the source tree. The original bug truncated
 * referenced images to 0 bytes because `ResourceCollectorTransform`
 * stored the source path as the artifact destination with empty
 * content, and neither `on_disk_path_for` nor `write_artifacts`
 * refused to write the empty bytes back over the source.
 *
 * Plan: claude-notes/plans/2026-05-20-render-truncates-source-images.md
 */

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

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

/// A small but real PNG byte sequence. We use a real header (rather
/// than arbitrary bytes) so we exercise the same code paths a real
/// user-authored image would.
const PNG_BYTES: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG signature
    0x00, 0x00, 0x00, 0x0d, // IHDR length
    0x49, 0x48, 0x44, 0x52, // IHDR
    0x00, 0x00, 0x00, 0x01, // width = 1
    0x00, 0x00, 0x00, 0x01, // height = 1
    0x08, 0x06, 0x00, 0x00, 0x00, // bit depth, color type, compression, filter, interlace
    0x1f, 0x15, 0xc4, 0x89, // CRC
];

/// R1 (Phase 0, bd-cfl67): rendering a single-doc qmd that references
/// a binary image in its own directory must not modify the image.
///
/// Before the fix: `q2 render` opens the source image with truncating
/// write semantics and zeros it. The qmd reference is the only thing
/// needed to trigger it — no project config is required.
#[test]
fn render_does_not_truncate_referenced_image() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path();

    let qmd_path = project_dir.join("index.qmd");
    let image_path = project_dir.join("elephant.png");

    write_text(
        &qmd_path,
        "---\ntitle: Test\n---\n\n![Caption](elephant.png)\n",
    );
    write_bytes(&image_path, PNG_BYTES);

    let before = std::fs::read(&image_path).expect("read image before render");
    assert_eq!(
        before, PNG_BYTES,
        "fixture sanity: source image bytes match what we wrote"
    );

    let options = RenderToFileOptions::default();
    let runtime = runtime_arc();
    render_to_file(&qmd_path, "html", &options, runtime).expect("render succeeds");

    let after = std::fs::read(&image_path).expect("read image after render");
    assert_eq!(
        after,
        before,
        "source image must be byte-identical after render (was {} bytes, became {} bytes)",
        before.len(),
        after.len(),
    );
}

/// F4 (Phase 2, bd-cfl67): in a website project where the output
/// dir is distinct from the input dir (`_site/`), a referenced
/// image is **copied** into the output tree at the same relative
/// position the rendered HTML's `<img src="...">` URL points to.
/// The source image is byte-unchanged.
#[test]
fn website_render_copies_image_to_output_and_preserves_source() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path();

    write_text(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n",
    );
    write_text(
        &project_dir.join("index.qmd"),
        "---\ntitle: Test\n---\n\n![Caption](elephant.png)\n",
    );
    write_bytes(&project_dir.join("elephant.png"), PNG_BYTES);

    let options = RenderToFileOptions::default();
    let runtime = runtime_arc();
    let result = render_to_file(&project_dir.join("index.qmd"), "html", &options, runtime)
        .expect("website render succeeds");

    // Source image bytes unchanged.
    let source_after = std::fs::read(project_dir.join("elephant.png")).unwrap();
    assert_eq!(
        source_after, PNG_BYTES,
        "source image must be byte-identical after render"
    );

    // Image was copied into `_site/elephant.png`.
    let copied = std::fs::read(project_dir.join("_site/elephant.png"))
        .expect("expected elephant.png copied into _site/");
    assert_eq!(copied, PNG_BYTES, "copied image bytes must match source");

    // Rendered HTML references the image. We don't pin the exact
    // src serialization (Pandoc + HTML writer choices), but the
    // word `elephant.png` must appear in the output.
    let html = std::fs::read_to_string(&result.output_path).expect("read rendered html");
    assert!(
        html.contains("elephant.png"),
        "rendered HTML must reference elephant.png; full html:\n{}",
        html
    );
}
