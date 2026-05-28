/*
 * tests/mermaid_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end integration tests for the mermaid engine (bd-gwfdo).
 *
 * Plan: claude-notes/plans/2026-05-28-mermaidjs-engine-design.md
 *
 * These tests drive the real `render_to_file` path — the same code path
 * `q2 render` uses — and assert on the rendered HTML. They complement
 * the unit tests in `crates/quarto-core/src/engine/mermaid.rs` (which
 * exercise the engine's text transform in isolation) by confirming the
 * engine's output round-trips correctly through pampa's QMD reader and
 * downstream pipeline stages.
 *
 * Why this matters: the engine emits raw HTML wrapped in a
 * ```` ```{=html} ```` fence. A bare `<pre>` / `<script>` at block
 * position is converted to RawInline and pampa tries to parse the
 * interior as Markdown, which fails noisily on the script's
 * `startOnLoad: true`. The unit tests can assert the fence is present
 * but only a real render confirms downstream stages accept it.
 */

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

/// A single-doc render with `engine: mermaidjs` and one `{mermaid}` cell
/// produces a `<pre class="mermaid">` wrapper around the diagram source
/// plus the jsdelivr `<script type="module">` include. End-to-end via
/// the real `render_to_file` entry point that `q2 render` uses.
#[test]
fn single_doc_mermaid_emits_pre_and_script() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\n\
         title: Mermaid test\n\
         engine: mermaidjs\n\
         ---\n\
         \n\
         ```{mermaid}\n\
         graph TD\n\
         A[Client] --> B[Load Balancer]\n\
         ```\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render");

    let html = read(&result.output_path);
    assert!(
        html.contains("<pre class=\"mermaid\">"),
        "expected <pre class=\"mermaid\"> wrapper in rendered HTML. \
         html:\n{html}"
    );
    assert!(
        html.contains("mermaid.esm.min.mjs"),
        "expected jsdelivr mermaid.esm.min.mjs import in rendered HTML. \
         html:\n{html}"
    );
    assert!(
        html.contains("startOnLoad: true"),
        "expected initialize call in rendered HTML. html:\n{html}"
    );
    // The diagram source survives HTML-escaped: `-->` becomes `--&gt;`.
    assert!(
        html.contains("A[Client] --&gt; B[Load Balancer]"),
        "expected HTML-escaped diagram source in rendered HTML. \
         html:\n{html}"
    );
}

/// `engine: mermaidjs` with no `{mermaid}` cells in the document must
/// not emit the script include — the script is per-document gated on
/// at least one matched cell, so a document that mentions the engine
/// but uses no diagrams stays lean.
#[test]
fn single_doc_mermaid_engine_without_cells_omits_script() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\n\
         title: No-cells test\n\
         engine: mermaidjs\n\
         ---\n\
         \n\
         Just prose, no diagrams.\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render");

    let html = read(&result.output_path);
    assert!(
        !html.contains("mermaid.esm.min.mjs"),
        "expected NO script include when no mermaid cells matched. \
         html contains the import; html:\n{html}"
    );
    assert!(
        !html.contains("<pre class=\"mermaid\">"),
        "expected NO pre wrapper when no mermaid cells matched. \
         html:\n{html}"
    );
}

/// A document containing multiple `{mermaid}` cells gets one wrapper
/// per cell and exactly one script include (the per-document
/// once-per-doc invariant). Mirrors the unit test
/// `multiple_cells_share_one_script` but at the rendered-HTML layer.
#[test]
fn single_doc_multiple_mermaid_cells_share_one_script() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\n\
         title: Multi-cell\n\
         engine: mermaidjs\n\
         ---\n\
         \n\
         ```{mermaid}\n\
         graph TD\n\
         A --> B\n\
         ```\n\
         \n\
         Some prose.\n\
         \n\
         ```{mermaid}\n\
         graph LR\n\
         X --> Y\n\
         ```\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render");

    let html = read(&result.output_path);
    assert_eq!(
        html.matches("<pre class=\"mermaid\">").count(),
        2,
        "expected exactly 2 pre wrappers (one per cell). html:\n{html}"
    );
    assert_eq!(
        html.matches("mermaid.esm.min.mjs").count(),
        1,
        "expected exactly 1 script include (once-per-doc invariant). \
         html:\n{html}"
    );
}

/// `engine: [mermaidjs]` — the array form — works the same as the
/// scalar form. This is the PR #238 multi-engine syntax with a single
/// element, and is the canonical form a user writing
/// `engine: [knitr, mermaidjs]` would encounter.
#[test]
fn array_engine_form_works() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\n\
         title: Array engine form\n\
         engine: [mermaidjs]\n\
         ---\n\
         \n\
         ```{mermaid}\n\
         graph TD\n\
         A --> B\n\
         ```\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render");

    let html = read(&result.output_path);
    assert!(
        html.contains("<pre class=\"mermaid\">"),
        "engine: [mermaidjs] array form did not produce pre wrapper. \
         html:\n{html}"
    );
    assert!(
        html.contains("mermaid.esm.min.mjs"),
        "engine: [mermaidjs] array form did not produce script include. \
         html:\n{html}"
    );
}
