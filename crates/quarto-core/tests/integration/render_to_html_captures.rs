/*
 * tests/integration/render_to_html_captures.rs
 *
 * bd-uy4uygha: the project (website) active-page HTML render must splice
 * server-recorded engine captures, so hub-client's default `format: html`
 * preview shows the output of a document executed by a connected
 * `q2 provide-hub` — not just the `format: q2-preview` AST path.
 *
 * Drives `ProjectPipeline<RenderToHtmlRenderer>` in `ActivePage` mode (exactly
 * like the `render_page_in_project` WASM entry) with a capture attached via
 * `RenderToHtmlRenderer::with_captures`, and asserts the recorded output
 * appears in the active page's HTML while the source-only path (no captures)
 * does not.
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
use quarto_core::project::pass2_renderer::{RenderToHtmlRenderer, WasmPassTwoOutput};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use quarto_trace::EngineCapture;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn render_active_page(active: &Path, captures: Vec<EngineCapture>) -> WasmPassTwoOutput {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(active, runtime.as_ref()).unwrap();
    if !project.is_single_file {
        project = ProjectContext::discover(&project.dir, runtime.as_ref()).unwrap();
    }

    let project_type = project_type_for(&project);
    let vfs_root = project.dir.join(".quarto/project-artifacts");
    let renderer = RenderToHtmlRenderer::new(&vfs_root)
        .with_url_root("/.quarto/project-artifacts")
        .with_captures(captures);

    let mut pipeline = ProjectPipeline::with_renderer(
        &mut project,
        project_type,
        Format::html(),
        "html",
        runtime.clone(),
        renderer,
    )
    .with_mode(RenderMode::ActivePage(active.to_path_buf()));

    let summary = pollster::block_on(pipeline.run()).expect("pipeline run");
    assert!(
        summary.pass1_failures.is_empty(),
        "unexpected pass-1 failures: {:?}",
        summary.pass1_failures,
    );
    assert!(
        summary.pass2_failures.is_empty(),
        "unexpected pass-2 failures: {:?}",
        summary.pass2_failures,
    );
    summary
        .outputs
        .into_iter()
        .next()
        .expect("ActivePage mode should produce one output")
}

/// A capture for the `{markerlang}` cell whose stdout is a marker that only the
/// capture carries. A fictitious engine name keeps EngineExecutionStage on the
/// markdown-fallback branch (no subprocess).
fn marker_capture() -> EngineCapture {
    EngineCapture {
        engine_name: "markerlang".into(),
        input_qmd: "```{markerlang}\n1 + 1\n```\n".into(),
        result: serde_json::json!({
            "markdown": "::: {.cell}\n```{.markerlang .cell-code}\n1 + 1\n```\n\n::: {.cell-output .cell-output-stdout}\n```\nSPLICEMARKER_ZX9\n```\n:::\n:::\n"
        }),
        files: Vec::new(),
    }
}

#[test]
fn active_page_html_splices_captures() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: default\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\nengine: markerlang\n---\n\n```{markerlang}\n1 + 1\n```\n",
    );
    // A sibling page with no cell, so the project has more than one file.
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nJust prose.\n",
    );

    let active = canonical(&project_dir.join("index.qmd"));

    // With the capture, the marker appears in the active page's HTML.
    let out = render_active_page(&active, vec![marker_capture()]);
    assert!(
        out.html().contains("SPLICEMARKER_ZX9"),
        "spliced engine output must appear in the active page's HTML; got:\n{}",
        &out.html()[..out.html().len().min(600)],
    );
    assert!(
        out.html().contains("cell-output"),
        "the spliced cell should render as a .cell-output block",
    );

    // Without captures, the same page renders source-only.
    let out2 = render_active_page(&active, vec![]);
    assert!(
        !out2.html().contains("SPLICEMARKER_ZX9"),
        "no capture => source-only render",
    );
}
