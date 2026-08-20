/*
 * tests/integration/include_project_absolute.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Project-absolute (leading-`/`) include paths (bd-w9koo1i2).
 */

//! `{{< include /path/from/root.qmd >}}` resolves against the
//! **project root**, per the Quarto path convention (Q1
//! `resolvePath`, q2 glob decision D2) — never against the
//! filesystem root. For single-file renders the anchor is the
//! document's own directory (Q1 parity: `rootDir = sourceDir`), which
//! is exactly what `ProjectContext::discover` puts in `project.dir`.
//!
//! Fixtures here go through `ProjectContext::discover` on a real
//! temp-dir layout (with or without `_quarto.yml`) so the
//! project-vs-single-file anchor comes from the same discovery branch
//! the CLI uses, not from a hand-built context.
//!
//! Plan: `claude-notes/plans/2026-08-07-include-project-absolute-paths.md`.

use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::pipeline::{HtmlRenderConfig, RenderOutput, render_qmd_to_html};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};

use crate::include_expansion_diagnostics::codes;

const MARKER: &str = "PROJECT-ABSOLUTE-INCLUDE-MARKER";

/// Write `files` (paths may contain subdirectories) into a temp dir,
/// discover the project context from `main` (finding `_quarto.yml` if
/// the fixture provides one), and render `main` through the real HTML
/// pipeline.
async fn render_discovered_fixture(files: &[(&str, &str)], main: &str) -> RenderOutput {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().to_path_buf();

    for (name, content) in files {
        let path = project_dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    let main_path = project_dir.join(main);
    let content = std::fs::read(&main_path).unwrap();

    let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
        Arc::new(quarto_system_runtime::NativeRuntime::new());

    let project =
        ProjectContext::discover(&main_path, runtime.as_ref()).expect("fixture project discovers");
    let doc = DocumentInfo::from_path(&main_path);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    render_qmd_to_html(
        &content,
        &main_path.to_string_lossy(),
        &mut ctx,
        &HtmlRenderConfig::default(),
        runtime,
    )
    .await
    .expect("render completes (include failures are diagnostics, not fatal)")
}

fn assert_include_resolved(output: &RenderOutput) {
    let codes = codes(output);
    assert!(
        !codes.contains(&"Q-17-2"),
        "project-absolute include must resolve, not report Q-17-2; diagnostics: {:?}",
        output.diagnostics
    );
    assert!(
        output.html.contains(MARKER),
        "included content must reach the HTML:\n{}",
        output.html
    );
}

#[tokio::test]
async fn project_absolute_include_resolves_from_project_root() {
    let output = render_discovered_fixture(
        &[
            ("_quarto.yml", "project:\n  type: default\n"),
            (
                "sub/doc.qmd",
                "---\ntitle: Root-relative include\n---\n\n\
                 {{< include /sub/_includes/snippet.qmd >}}\n",
            ),
            (
                "sub/_includes/snippet.qmd",
                "PROJECT-ABSOLUTE-INCLUDE-MARKER\n",
            ),
        ],
        "sub/doc.qmd",
    )
    .await;

    assert_include_resolved(&output);
}

#[tokio::test]
async fn project_absolute_include_reaches_across_directories() {
    // The include target lives in a *different* subtree than the
    // including document — the case a document-relative fallback
    // cannot fake.
    let output = render_discovered_fixture(
        &[
            ("_quarto.yml", "project:\n  type: default\n"),
            (
                "a/b/doc.qmd",
                "---\ntitle: Cross-tree include\n---\n\n\
                 {{< include /shared/_snippet.qmd >}}\n",
            ),
            ("shared/_snippet.qmd", "PROJECT-ABSOLUTE-INCLUDE-MARKER\n"),
        ],
        "a/b/doc.qmd",
    )
    .await;

    assert_include_resolved(&output);
}

#[tokio::test]
async fn single_file_project_absolute_include_anchors_at_document_dir() {
    // No `_quarto.yml`: single-file render. Q1 parity — the
    // leading-`/` anchor falls back to the source file's directory.
    let output = render_discovered_fixture(
        &[
            (
                "sub/doc.qmd",
                "---\ntitle: Single-file root-relative include\n---\n\n\
                 {{< include /deep/_snippet.qmd >}}\n",
            ),
            ("sub/deep/_snippet.qmd", "PROJECT-ABSOLUTE-INCLUDE-MARKER\n"),
        ],
        "sub/doc.qmd",
    )
    .await;

    assert_include_resolved(&output);
}

#[tokio::test]
async fn nested_project_absolute_include_anchors_at_project_root() {
    // A relatively-included child that itself uses a leading-`/`
    // include: the root anchor must stay fixed at the project root at
    // every nesting level (it must NOT drift to the child's own
    // directory, where `other/` does not exist).
    let output = render_discovered_fixture(
        &[
            ("_quarto.yml", "project:\n  type: default\n"),
            (
                "doc.qmd",
                "---\ntitle: Nested root-relative include\n---\n\n\
                 {{< include sub/_a.qmd >}}\n",
            ),
            (
                "sub/_a.qmd",
                "outer child\n\n{{< include /other/_b.qmd >}}\n",
            ),
            ("other/_b.qmd", "PROJECT-ABSOLUTE-INCLUDE-MARKER\n"),
        ],
        "doc.qmd",
    )
    .await;

    assert_include_resolved(&output);
}

#[tokio::test]
async fn relative_include_still_resolves_against_document_dir() {
    // Regression guard: the fix must not disturb plain relative
    // resolution.
    let output = render_discovered_fixture(
        &[
            ("_quarto.yml", "project:\n  type: default\n"),
            (
                "sub/doc.qmd",
                "---\ntitle: Relative include\n---\n\n\
                 {{< include _includes/snippet.qmd >}}\n",
            ),
            (
                "sub/_includes/snippet.qmd",
                "PROJECT-ABSOLUTE-INCLUDE-MARKER\n",
            ),
        ],
        "sub/doc.qmd",
    )
    .await;

    assert_include_resolved(&output);
}
