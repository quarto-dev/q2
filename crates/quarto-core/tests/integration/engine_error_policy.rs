//! bd-ohvl879u: engine error policy — a failing cell aborts the render
//! unless `error: true` allows it (per-cell `#| error: true` or the
//! document's `execute: error: true`), matching knitr and Q1 semantics.
//!
//! Drives the real jupyter engine through `record_capture` (the same
//! producer path `q2 render` / `q2 preview` / `q2 provide-hub` use).
//! Tests skip when the engine isn't installed — same gating as
//! capture_splice_engines.rs, and the same pollster calling convention
//! (the jupyter engine builds its own tokio runtime internally).

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::engine::EngineRegistry;
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn engine_available(name: &str) -> bool {
    EngineRegistry::default()
        .get(name)
        .is_some_and(|e| e.is_available())
}

fn fixture(
    content: &str,
) -> (
    tempfile::TempDir,
    PathBuf,
    ProjectContext,
    Arc<dyn SystemRuntime>,
) {
    let dir = tempfile::tempdir().unwrap();
    let qmd_path = dir.path().join("doc.qmd");
    std::fs::write(&qmd_path, content).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&qmd_path, runtime.as_ref()).unwrap();
    (dir, qmd_path, project, runtime)
}

fn run(content: &str) -> Result<Vec<quarto_trace::EngineCapture>, String> {
    let (_tmp, path, project, runtime) = fixture(content);
    pollster::block_on(record_capture(&path, &project, runtime, None)).map_err(|e| format!("{e:?}"))
}

fn result_markdown(captures: &[quarto_trace::EngineCapture]) -> String {
    captures[0]
        .result
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

#[test]
fn plain_cell_error_fails_the_render() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let result = run(
        "---\ntitle: Error policy\nengine: jupyter\n---\n\nBefore.\n\n```{python}\nraise Exception(\"boom\")\n```\n",
    );
    let err = result.expect_err(
        "bd-ohvl879u: an un-annotated cell error must fail the render (knitr/Q1 parity)",
    );
    assert!(
        err.contains("Exception") && err.contains("boom"),
        "diagnostic should carry the kernel error; got: {err}"
    );
}

#[test]
fn cell_error_true_embeds_error_output_and_strips_directive() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let captures = run(
        "---\ntitle: Error policy\nengine: jupyter\n---\n\nBefore.\n\n```{python}\n#| error: true\nraise Exception(\"boom\")\n```\n",
    )
    .expect("error: true must allow the render to proceed");
    let md = result_markdown(&captures);
    assert!(
        md.contains(".cell-output-error"),
        "error output must be embedded; got:\n{md}"
    );
    assert!(
        md.contains("boom"),
        "error text must be embedded; got:\n{md}"
    );
    // Q1/knitr parity: the option line is stripped from the echoed
    // source (and from what the kernel executed).
    assert!(
        !md.contains("#|"),
        "option lines must not appear in the emitted markdown; got:\n{md}"
    );
}

#[test]
fn document_execute_error_true_allows_plain_cell_error() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let captures = run(
        "---\ntitle: Error policy\nengine: jupyter\nexecute:\n  error: true\n---\n\nBefore.\n\n```{python}\nraise Exception(\"boom\")\n```\n",
    )
    .expect("document-level execute.error: true must allow the render to proceed");
    let md = result_markdown(&captures);
    assert!(
        md.contains(".cell-output-error") && md.contains("boom"),
        "error output must be embedded under doc-level allow; got:\n{md}"
    );
}

#[test]
fn cell_error_false_overrides_document_allow() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let result = run(
        "---\ntitle: Error policy\nengine: jupyter\nexecute:\n  error: true\n---\n\nBefore.\n\n```{python}\n#| error: false\nraise Exception(\"boom\")\n```\n",
    );
    assert!(
        result.is_err(),
        "cell-level error: false must override the document allow (scoped resolution)"
    );
}

#[test]
fn failing_cell_stops_subsequent_cells() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    // If the second cell ran, the engine would have kept executing
    // past a disallowed error. The render must fail on cell 1.
    let result = run(
        "---\ntitle: Error policy\nengine: jupyter\n---\n\n```{python}\nraise Exception(\"first boom\")\n```\n\n```{python}\nprint(\"must not run\")\n```\n",
    );
    let err = result.expect_err("first cell's error must abort");
    assert!(
        err.contains("first boom"),
        "diagnostic should reference the failing cell; got: {err}"
    );
}

#[test]
fn malformed_cell_options_fail_the_render() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let result = run(
        "---\ntitle: Error policy\nengine: jupyter\n---\n\n```{python}\n#| error: [unclosed\nprint(1)\n```\n",
    );
    assert!(
        result.is_err(),
        "malformed cell-option YAML must be a hard error (decision 6)"
    );
}

#[test]
fn healthy_cells_are_unaffected() {
    if !engine_available("jupyter") {
        eprintln!("Skipping test: jupyter not available");
        return;
    }
    let captures = run(
        "---\ntitle: Error policy\nengine: jupyter\n---\n\n```{python}\n#| echo: true\n2 + 3\n```\n",
    )
    .expect("healthy cell renders");
    let md = result_markdown(&captures);
    assert!(md.contains("::: {.cell}"), "cell wrapper intact");
    assert!(md.contains('5'), "output intact");
    assert!(!md.contains("#|"), "directive stripped from echo");
}
