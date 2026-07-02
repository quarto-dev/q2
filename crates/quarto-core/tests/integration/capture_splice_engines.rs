//! bd-gthycd33: engine captures must be consumable by the preview
//! capture splice for BOTH engines.
//!
//! Regression pair for the bug where a jupyter capture arrived in the
//! hub-client preview but its output never spliced in (the `{python}`
//! cell kept rendering as raw source), while the identical knitr flow
//! worked. Root cause: the jupyter engine emitted its executed cells
//! *without* the `::: {.cell}` wrapper that
//! `derive_cell_outputs` (crate::engine::capture_splice) requires, so
//! the cell-output map came out empty and the splice was a fail-soft
//! no-op.
//!
//! Each test drives the real engine through `record_capture` — the
//! exact producer path `q2 preview` and `q2 provide-hub` use — then
//! replays what `CaptureSpliceStage` does on the consumer side:
//! parse `capture.input_qmd` / `capture.result.markdown` with the
//! pampa qmd reader, derive the cell-output map, and splice onto the
//! live AST. Tests skip (with a note) when the engine isn't installed.

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::engine::EngineRegistry;
use quarto_core::engine::capture_splice::{derive_cell_outputs, engine_cell_lang, splice_cells};
use quarto_core::engine::preview_record::record_capture;
use quarto_core::project::ProjectContext;
use quarto_pandoc_types::{Block, Div, Pandoc};
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

/// Parse a QMD string the way `CaptureSpliceStage` does (no source
/// tracking; throwaway file name).
fn parse(qmd: &str, name: &str) -> Pandoc {
    let (pandoc, _, _) = pampa::readers::qmd::read(
        qmd.as_bytes(),
        false,
        name,
        &mut std::io::sink(),
        false,
        None,
    )
    .expect("parse");
    pandoc
}

fn is_cell_div(block: &Block) -> bool {
    let Block::Div(Div { attr, .. }) = block else {
        return false;
    };
    attr.1.iter().any(|c| c == "cell")
}

/// Run `content` through the real engine via `record_capture`, then
/// assert the capture splices: the cell-output map is non-empty and
/// the engine cell in the (unedited) live AST gets replaced by the
/// captured `Div.cell` wrapper.
fn assert_capture_splices(content: &str, engine: &str) {
    if !engine_available(engine) {
        eprintln!("Skipping test: engine '{engine}' not available on this machine");
        return;
    }

    let (_tmp, path, project, runtime) = fixture(content);
    // Mirror the provider's calling convention (quarto-hub-provider's
    // spawn_blocking + pollster::block_on): the jupyter engine builds
    // its own current-thread tokio runtime internally, so this must
    // NOT run inside a #[tokio::test] context.
    let captures =
        pollster::block_on(record_capture(&path, &project, runtime, None)).expect("record_capture");
    assert_eq!(captures.len(), 1, "expected exactly one capture");
    let capture = &captures[0];
    assert_eq!(capture.engine_name, engine);

    let result_markdown = capture
        .result
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let a1 = parse(&capture.input_qmd, "capture-input.rmarkdown");
    let b1 = parse(result_markdown, "capture-result.md");

    let map = derive_cell_outputs(&a1, &b1);
    assert_eq!(
        map.len(),
        1,
        "bd-gthycd33: derive_cell_outputs must map the single {engine} cell; \
         capture.result.markdown was:\n{result_markdown}"
    );

    // The live AST for an unedited doc: input_qmd is the canonical
    // serialization of the live pre-engine AST, so re-parsing it is
    // the same AST the browser-side pipeline would hand the splice.
    let a2 = parse(&capture.input_qmd, "live.qmd");
    let out = splice_cells(a2, &map, engine);

    let cell_divs = out.blocks.iter().filter(|b| is_cell_div(b)).count();
    assert_eq!(
        cell_divs,
        1,
        "splice must replace the {engine} cell with the captured Div.cell wrapper; \
         got blocks: {:?}",
        out.blocks.iter().map(block_kind).collect::<Vec<_>>()
    );
    let remaining_engine_cells = out
        .blocks
        .iter()
        .filter(|b| engine_cell_lang(b).is_some())
        .count();
    assert_eq!(
        remaining_engine_cells, 0,
        "no raw engine cell may remain after the splice"
    );
}

fn block_kind(b: &Block) -> &'static str {
    match b {
        Block::Paragraph(_) => "Paragraph",
        Block::Plain(_) => "Plain",
        Block::CodeBlock(_) => "CodeBlock",
        Block::Div(_) => "Div",
        Block::RawBlock(_) => "RawBlock",
        Block::Header(_) => "Header",
        _ => "Other",
    }
}

#[test]
fn jupyter_capture_splices_into_preview_ast() {
    assert_capture_splices(
        "---\ntitle: Splice demo\nengine: jupyter\n---\n\nSome prose.\n\n```{python}\n2 + 3\n```\n",
        "jupyter",
    );
}

#[test]
fn knitr_capture_splices_into_preview_ast() {
    assert_capture_splices(
        "---\ntitle: Splice demo\nengine: knitr\n---\n\nSome prose.\n\n```{r}\n1 + 1\n```\n",
        "knitr",
    );
}
