/*
 * tests/integration/pandoc_typst_resource_copy.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P2c: `ctx.resource_copies` must be flushed to the output
 * directory *before* `TypstCompileStage` shells out to `typst compile` —
 * not just at end-of-render, which is too late for the compile step to
 * see the file. See
 * `claude-notes/plans/2026-09-24-pandoc-hybrid-resource-copy-ordering.md`.
 */

//! Discovered while scoping book-projects P2's item 49 (a compiling
//! book-with-figures fixture): a Typst render of a project whose
//! `output-dir` differs from the project directory, referencing a local
//! image, failed to compile at all — `typst compile` read the intermediate
//! `.typ` file's `image("img/dot.png")` reference off disk, relative to
//! the output directory, *before* the copy that would have placed the
//! image there ever ran (`Error [Q-21-3]`: `file not found (searched at
//! <output-dir>/img/dot.png)`). This is not book-specific: this file
//! exercises a plain, non-book project, exactly the shape that first
//! proved the gap was general.
//!
//! Reproducing this needs a real *project* render (`ProjectPipeline`, the
//! same path `q2 render <project>` uses) — a bare `render_document_to_file`
//! call with no `_quarto.yml` discovers a synthetic single-file "project"
//! that computes a different (climb-back-to-source) relative image path,
//! which does not exhibit the bug. `format_path_keys.rs`'s `render_project`
//! harness (same pattern here) is what actually exercises the code path a
//! real `output-dir:` project uses.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// A minimal valid 1x1 PNG (the smallest well-known valid PNG byte
/// sequence) — same fixture `pandoc_render_to_file.rs`'s docx test uses.
const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

const QUARTO_YML: &str = "project:\n  type: default\n  output-dir: _output\n";

fn write_fixture(project_dir: &Path) {
    write(&project_dir.join("_quarto.yml"), QUARTO_YML);
    write(
        &project_dir.join("doc.qmd"),
        "---\ntitle: Image Compile\n---\n\n![alt](img/dot.png)\n",
    );
    std::fs::create_dir_all(project_dir.join("img")).unwrap();
    std::fs::write(project_dir.join("img/dot.png"), ONE_PIXEL_PNG).unwrap();
}

fn render_project_to_typst(
    fixture: impl FnOnce(&Path),
) -> (TempDir, PathBuf, ProjectRenderSummary) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let format = Format::from_format_string("typst").unwrap();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        "typst",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).unwrap_or_else(|e| panic!("pipeline: {e}"));
    (temp, project_dir, summary)
}

/// The end-to-end regression guard: a plain (non-book) `type: default`
/// project with `output-dir: _output`, one document referencing a local
/// image by a relative path. Before the fix, `typst compile` failed with
/// "file not found" because the image was never copied into the output
/// directory in time. After the fix, the image must actually be present
/// at the output directory *and* the compile must produce a real PDF.
#[test]
fn typst_project_copies_image_to_output_dir_before_compiling() {
    let (_temp, project_dir, summary) = render_project_to_typst(write_fixture);

    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "expected a clean render (the image-copy-ordering bug surfaces as a \
         pass2 failure): pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    assert_eq!(summary.outputs.len(), 1, "{summary:?}");

    let output_path = &summary.outputs[0].output_path;
    assert_eq!(
        output_path.extension().and_then(|e| e.to_str()),
        Some("pdf")
    );
    let bytes = std::fs::read(output_path)
        .unwrap_or_else(|e| panic!("expected {} to exist: {e}", output_path.display()));
    assert!(
        bytes.starts_with(b"%PDF-"),
        "expected a real PDF header at {}, got: {:?}",
        output_path.display(),
        &bytes[..bytes.len().min(20)]
    );

    let copied_image = project_dir.join("_output/img/dot.png");
    assert!(
        copied_image.is_file(),
        "the referenced image must be copied to the output directory \
         (in time for typst compile to find it): {}",
        copied_image.display()
    );
}
