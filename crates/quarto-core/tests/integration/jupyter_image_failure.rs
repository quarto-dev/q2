//! bd-yt2o2ovt: an image output Quarto cannot use is reported (`Q-18-11`), once, at
//! the cell, and the placeholder that stands in for it is literal text.
//!
//! Before, the failure was a log line only; the placeholder `[Image output]` was
//! parsed as a bracketed span, which cost its brackets and raised an unrelated
//! `Q-2-49` warning with no cell location.
//!
//! Skips when no Python Jupyter kernel is available (CI installs ipykernel).

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ProjectContext;
use quarto_core::engine::EngineRegistry;
use quarto_core::render_to_file::{
    RenderToFileOptions, RenderToFileResult, render_document_to_file,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn jupyter_available() -> bool {
    EngineRegistry::default()
        .get("jupyter")
        .is_some_and(|e| e.is_available())
}

/// A cell that displays a PNG whose payload is not base64.
const DOC: &str = "---\ntitle: Bad image\nengine: jupyter\n---\n\n\
    ```{python}\n\
    from IPython.display import display\n\
    display({'image/png': '!!!not base64!!!'}, raw=True)\n\
    ```\n";

fn render(tmp: &TempDir) -> (RenderToFileResult, String) {
    let input = tmp.path().join("bad.qmd");
    std::fs::write(&input, DOC).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref()).expect("project discovery");
    let result = render_document_to_file(
        &input,
        "html",
        &RenderToFileOptions::default(),
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    )
    .unwrap_or_else(|e| panic!("render must succeed: {e}"));
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    (result, html)
}

fn codes(result: &RenderToFileResult) -> Vec<String> {
    result
        .render_output
        .diagnostics
        .iter()
        .filter_map(|d| d.code.clone())
        .collect()
}

#[test]
fn unusable_image_warns_q_18_11_at_the_cell_and_not_q_2_49() {
    if !jupyter_available() {
        eprintln!(
            "SKIP: no Jupyter runtime — unusable_image_warns_q_18_11_at_the_cell_and_not_q_2_49"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (result, html) = render(&tmp);

    let codes = codes(&result);
    assert_eq!(
        codes.iter().filter(|c| *c == "Q-18-11").count(),
        1,
        "expected exactly one Q-18-11, got {codes:?}"
    );
    assert!(
        !codes.iter().any(|c| c == "Q-2-49"),
        "the placeholder must not parse as a span: {codes:?}"
    );

    let warning = result
        .render_output
        .diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-18-11"))
        .unwrap()
        .to_text(None);
    assert!(warning.contains("bad.qmd:"), "no cell location: {warning}");
    assert!(warning.contains("image/png"), "{warning}");

    assert!(
        html.contains("[Image output]"),
        "the placeholder is literal text:\n{html}"
    );
}
