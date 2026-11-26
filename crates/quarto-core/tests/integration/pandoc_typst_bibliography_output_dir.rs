/*
 * tests/integration/pandoc_typst_bibliography_output_dir.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * GH #1011: a Typst render of a project with an `output-dir` must find
 * the `bibliography:` file that `typst compile` reads.
 */

//! With `citeproc: false` the `.typ` carries `#bibliography(...)`, which
//! `typst compile` resolves relative to the `.typ` file, that is, in the
//! output directory. The resource collector queues image copies from the
//! AST; the `bibliography:` value sits in the metadata, so it needs its
//! own copy. Like `pandoc_typst_resource_copy.rs`, this needs a real
//! `ProjectPipeline` render: a bare single-file render writes beside the
//! source, where the file already is.

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

const BIB: &str = "@book{a, title={A}, author={B}, year={2000}}\n";

fn render_project(fixture: impl FnOnce(&Path)) -> (TempDir, PathBuf, ProjectRenderSummary) {
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

fn assert_compiled_pdf(summary: &ProjectRenderSummary) {
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "expected a clean render: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let output_path = &summary.outputs[0].output_path;
    let bytes = std::fs::read(output_path)
        .unwrap_or_else(|e| panic!("expected {} to exist: {e}", output_path.display()));
    assert!(
        bytes.starts_with(b"%PDF-"),
        "expected a PDF at {}",
        output_path.display()
    );
}

/// The issue's reproduction: `output-dir: _site`, front-matter
/// `bibliography:` beside the document, `citeproc: false`.
#[test]
fn typst_output_dir_render_finds_front_matter_bibliography() {
    let (_temp, project_dir, summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), "project:\n  output-dir: _site\n");
        write(
            &dir.join("index.qmd"),
            "---\nformat: typst\nbibliography: references.bib\nciteproc: false\n---\n\n[@a]\n",
        );
        write(&dir.join("references.bib"), BIB);
    });
    assert_compiled_pdf(&summary);
    assert_eq!(
        summary.outputs[0].output_path,
        project_dir.join("_site/index.pdf")
    );
    assert!(project_dir.join("_site/references.bib").is_file());
}

/// A document in a subdirectory, with the bibliography declared in
/// `_quarto.yml` (so it resolves against the project root, not the
/// document's directory).
#[test]
fn typst_output_dir_render_finds_project_bibliography_from_subdir() {
    let (_temp, project_dir, summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  output-dir: _site\nbibliography: refs/references.bib\n",
        );
        write(
            &dir.join("chapters/doc.qmd"),
            "---\nformat: typst\nciteproc: false\n---\n\n[@a]\n",
        );
        write(&dir.join("refs/references.bib"), BIB);
    });
    assert_compiled_pdf(&summary);
    assert_eq!(
        summary.outputs[0].output_path,
        project_dir.join("_site/chapters/doc.pdf")
    );
    // The `.typ` spells the path as written, relative to itself.
    assert!(
        project_dir
            .join("_site/chapters/refs/references.bib")
            .is_file()
    );
}
