/*
 * integration/ipynb_stored_replay.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 7c Phase 3: stored-output replay. A `.ipynb` renders its code
 * cells' *stored* outputs by default (Q1 parity — stored outputs are the
 * document), through the same `render_cell`/`format_outputs` machinery the
 * live jupyter engine uses, with **zero kernel launches** — no jupyter
 * binary required. `execute.enabled: true` (the notebook's front-matter
 * cell, merged by MetadataMergeStage) is the only demand route back to
 * live execution; a document that demands execution must NOT silently
 * replay instead.
 *
 * All routing tests run with `ExecutionPolicy::None` so they are
 * machine-independent: jupyter is absent in CI/dev shells, and a policy of
 * `All` would drive a *live* kernel execution on machines that have it.
 * Replay ignoring the policy gate is itself part of the contract (replay
 * is not execution — `preview --static` must still show stored outputs).
 */

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use quarto_core::engine::jupyter::{JupyterEngine, find_jupyter_call_count};
use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError,
    ExecutionPolicy,
};
use quarto_core::extension::types::FileClaim;
use quarto_core::format::Format;
use quarto_core::pipeline::{PreviewAstOutput, render_qmd_to_preview_ast};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_error_reporting::DiagnosticKind;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const STDOUT_NB: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["# Stored notebook\n\nProse before the cell.\n"]},{"cell_type":"code","execution_count":1,"metadata":{},"outputs":[{"output_type":"stream","name":"stdout","text":["hello from the notebook\n"]}],"source":["print('hello from the notebook')\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

const HTML_OUTPUT_NB: &str = r##"{"cells":[{"cell_type":"code","execution_count":2,"metadata":{},"outputs":[{"output_type":"display_data","data":{"text/html":["<b>rich stored output</b>"]},"metadata":{}}],"source":["from IPython.display import HTML\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

const ERROR_OUTPUT_NB: &str = r##"{"cells":[{"cell_type":"code","execution_count":3,"metadata":{},"outputs":[{"output_type":"error","ename":"ZeroDivisionError","evalue":"division by zero","traceback":["traceback frame 1"]}],"source":["1 / 0\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

const LITERAL_FENCE_NB: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["A markdown cell showing a fence:\n\n```{python}\nprint('not a real cell')\n```\n\nDone.\n"]},{"cell_type":"code","execution_count":4,"metadata":{},"outputs":[{"output_type":"stream","name":"stdout","text":["real cell ran\n"]}],"source":["print('real cell ran')\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

const PNG_OUTPUT_NB: &str = r##"{"cells":[{"cell_type":"code","execution_count":5,"metadata":{},"outputs":[{"output_type":"display_data","data":{"image/png":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==","text/plain":["<Figure>"]},"metadata":{}}],"source":["import matplotlib.pyplot as plt\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

const EXECUTE_DEMANDED_NB: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["---\nexecute:\n  enabled: true\n---\n"]},{"cell_type":"code","execution_count":6,"metadata":{},"outputs":[{"output_type":"stream","name":"stdout","text":["stored, must not replay\n"]}],"source":["print('live would say this')\n"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;

const MARKDOWN_ONLY_NB: &str = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["# No code cells at all\n"]}],"metadata":{"kernelspec":{"name":"python3"}},"nbformat":4,"nbformat_minor":5}"##;

const STUB_MARKER: &str = "STUB-JUPYTER-RAN";

/// A stand-in for the jupyter engine: same name (so the `.ipynb` claim,
/// re-declared from `JupyterEngine::static_file_claims()`, resolves to it)
/// and always available, but `execute` appends an unmistakable marker.
/// This makes the execute-vs-replay routing observable without a kernel,
/// a PATH probe, or jupyter being installed at all.
struct StubJupyter;

impl ExecutionEngine for StubJupyter {
    fn name(&self) -> &str {
        "jupyter"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn file_claims(&self) -> Vec<FileClaim> {
        JupyterEngine::static_file_claims()
    }

    fn execute(
        &self,
        input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        Ok(ExecuteResult::new(format!("{input}\n{STUB_MARKER}\n")))
    }
}

/// A registered jupyter that is unavailable and counts every availability
/// check. Replay must never consult it (EXR-1b: no engine-availability probe).
struct UnavailableJupyter {
    availability_checks: Arc<AtomicUsize>,
}

impl ExecutionEngine for UnavailableJupyter {
    fn name(&self) -> &str {
        "jupyter"
    }

    fn is_available(&self) -> bool {
        self.availability_checks.fetch_add(1, Ordering::SeqCst);
        false
    }

    fn file_claims(&self) -> Vec<FileClaim> {
        JupyterEngine::static_file_claims()
    }

    fn execute(
        &self,
        _input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        panic!("replay must not execute the jupyter engine");
    }
}

fn registry_with_stub_jupyter() -> Arc<EngineRegistry> {
    let mut registry = EngineRegistry::empty();
    registry.register(Arc::new(StubJupyter));
    Arc::new(registry)
}

async fn preview_ast(
    notebook: &str,
    policy: ExecutionPolicy,
    registry: Option<Arc<EngineRegistry>>,
) -> PreviewAstOutput {
    let temp = tempfile::TempDir::new().unwrap();
    let nb_path = temp.path().join("notebook.ipynb");
    std::fs::write(&nb_path, notebook).unwrap();
    let nb_path = nb_path.canonicalize().unwrap();

    let project = ProjectContext {
        dir: temp.path().to_path_buf(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&nb_path)],
        output_dir: temp.path().to_path_buf(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&nb_path);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.execution_policy = policy;
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    render_qmd_to_preview_ast(
        notebook.as_bytes(),
        &nb_path.display().to_string(),
        &mut ctx,
        runtime,
        registry,
        Vec::new().into(),
    )
    .await
    .unwrap_or_else(|e| panic!("notebook must render: {e}"))
}

fn problems(out: &PreviewAstOutput) -> Vec<String> {
    out.diagnostics
        .iter()
        .filter(|d| matches!(d.kind, DiagnosticKind::Warning | DiagnosticKind::Error))
        .map(|d| d.title.clone())
        .collect()
}

/// Regression (Phase 3 e2e, penguins.ipynb): real-world notebooks' code
/// cells lack the trailing newline (nbformat strips it), and before the
/// converter fix the closing fence glued onto the last code line
/// (`plt.show()```), making the cell unparseable markdown — replay could
/// not match it and the whole document rendered as one inert code run.
/// The full pipeline must still replay the stored output.
#[tokio::test]
async fn code_cell_without_trailing_newline_still_replays() {
    let notebook = r##"{"cells":[{"cell_type":"code","execution_count":7,"metadata":{},"outputs":[{"output_type":"stream","name":"stdout","text":["no trailing newline\n"]}],"source":["print('no trailing newline')"]}],"metadata":{"kernelspec":{"name":"python3","language":"python"}},"nbformat":4,"nbformat_minor":5}"##;
    let out = preview_ast(notebook, ExecutionPolicy::None, None).await;
    assert!(
        out.ast_json.contains("no trailing newline"),
        "stored output must replay for a newline-stripped cell source: {}",
        out.ast_json
    );
    assert!(
        out.ast_json.contains("cell-output-stdout"),
        "replayed output must carry the stdout class: {}",
        out.ast_json
    );
    assert_eq!(
        out.ast_json.matches("cell-code").count(),
        1,
        "the cell must be echoed exactly once, not as glued unparseable text: {}",
        out.ast_json
    );
    assert_eq!(
        find_jupyter_call_count(),
        1,
        "replay must not probe for jupyter"
    );
}

/// Flagship (plan Phase 3, TDD item): a stored-stdout notebook renders its
/// stored output through the preview pipeline with the canonical cell
/// shape — echoed `{.python .cell-code}` source plus a
/// `.cell-output-stdout` div — and **zero kernel launches** (the jupyter
/// PATH-lookup counter stays at its process cap of 1). Policy `None`
/// proves replay ignores the execution gate (e2e finding 3: static
/// preview must still show stored outputs).
#[tokio::test]
async fn stored_stdout_replays_without_jupyter() {
    let out = preview_ast(STDOUT_NB, ExecutionPolicy::None, None).await;
    assert!(
        out.ast_json.contains("hello from the notebook"),
        "stored stdout must be replayed into the AST, got: {}",
        out.ast_json
    );
    assert!(
        out.ast_json.contains("cell-output-stdout"),
        "output must carry the canonical stdout class, got: {}",
        out.ast_json
    );
    assert!(
        out.ast_json.contains("cell-code"),
        "cell source must be echoed with the canonical class, got: {}",
        out.ast_json
    );
    assert_eq!(
        find_jupyter_call_count(),
        1,
        "replay must not probe for jupyter beyond the registry-construction cap"
    );
}

/// Jupyter registered but unavailable, `.ipynb` input without `execute.enabled`:
/// replay succeeds and the engine's availability check is never called.
#[tokio::test]
async fn replay_does_not_probe_engine_availability() {
    let availability_checks = Arc::new(AtomicUsize::new(0));
    let mut registry = EngineRegistry::empty();
    registry.register(Arc::new(UnavailableJupyter {
        availability_checks: availability_checks.clone(),
    }));

    let out = preview_ast(STDOUT_NB, ExecutionPolicy::None, Some(Arc::new(registry))).await;

    assert!(
        out.ast_json.contains("hello from the notebook"),
        "stored stdout must replay with jupyter unavailable, got: {}",
        out.ast_json
    );
    assert_eq!(
        availability_checks.load(Ordering::SeqCst),
        0,
        "replay must not consult the registered engine's availability"
    );
}

/// EXR-4 Phase 6: a *record* (the engine execution record, written by `write_record`) is an
/// nbformat 4 notebook, and presenting it needs no engine: with Jupyter registered but
/// unavailable, its recorded outputs replay through the same path and the availability
/// check is never called.
#[tokio::test]
async fn a_record_replays_without_probing_engine_availability() {
    use quarto_core::engine::units::scan_units;
    use quarto_core::record::{
        AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, coalesce,
        fill_ids, write_record,
    };

    let doc = "# Report\n\nProse.\n\n```{python}\nprint('recorded output')\n```\n";
    let mut units = scan_units(doc);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    let def = HashDefinition::conservative("jupyter");
    let mut builder = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
    builder
        .push(
            0,
            UnitResult {
                outputs: vec![serde_json::json!({
                    "output_type": "stream", "name": "stdout", "text": ["ONLY-IN-THE-RECORD\n"]
                })],
                execution_count: Some(1),
                kernel: Some("python3".into()),
            },
        )
        .unwrap();
    let entry = builder
        .finish(EngineEntryInput {
            owned_languages: vec!["python".into()],
            ..Default::default()
        })
        .unwrap();
    let record = coalesce(&[entry], &units).unwrap();
    let bytes = write_record(&record).unwrap();
    // The notebook converter takes the fence language from `metadata.kernelspec.language`
    // (Q1 behavior), and a record carries a per-cell `quarto_lang` instead. Supply the
    // kernelspec a consumer would derive from it; without one the cell converts to an empty
    // `{}` fence and nothing replays (a finding for the code that presents records, EXR-13/14).
    let mut notebook: serde_json::Value = serde_json::from_str(&bytes).unwrap();
    notebook["metadata"]["kernelspec"] =
        serde_json::json!({"name": "python3", "language": "python"});
    let bytes = serde_json::to_string(&notebook).unwrap();

    let availability_checks = Arc::new(AtomicUsize::new(0));
    let mut registry = EngineRegistry::empty();
    registry.register(Arc::new(UnavailableJupyter {
        availability_checks: availability_checks.clone(),
    }));
    let out = preview_ast(&bytes, ExecutionPolicy::None, Some(Arc::new(registry))).await;

    assert!(
        out.ast_json.contains("ONLY-IN-THE-RECORD") && out.ast_json.contains("cell-output-stdout"),
        "the record's stored output (absent from its source) must be presented, got: {}",
        out.ast_json
    );
    assert_eq!(
        availability_checks.load(Ordering::SeqCst),
        0,
        "presenting a record must not consult any engine's availability"
    );
}

/// Rich stored output: `text/html` in a `display_data` bundle renders as
/// the `{=html}` raw block inside a `cell-output-display` div — the same
/// mime priority `format_outputs` applies to live kernel results.
#[tokio::test]
async fn stored_html_output_renders_raw_html() {
    let out = preview_ast(HTML_OUTPUT_NB, ExecutionPolicy::None, None).await;
    assert!(
        out.ast_json.contains("<b>rich stored output</b>"),
        "stored html must reach the AST: {}",
        out.ast_json
    );
    assert!(
        out.ast_json.contains("cell-output-display"),
        "display output must carry the display class: {}",
        out.ast_json
    );
    assert_eq!(
        find_jupyter_call_count(),
        1,
        "replay must not probe for jupyter"
    );
}

/// A stored error output is document content: it renders as a
/// `cell-output-error` div. The live path's `error: false` abort policy
/// is execution semantics and does not apply to content that already
/// happened.
#[tokio::test]
async fn stored_error_output_renders_error_div() {
    let out = preview_ast(ERROR_OUTPUT_NB, ExecutionPolicy::None, None).await;
    assert!(
        out.ast_json.contains("ZeroDivisionError"),
        "stored error must reach the AST: {}",
        out.ast_json
    );
    assert!(
        out.ast_json.contains("cell-output-error"),
        "error output must carry the error class: {}",
        out.ast_json
    );
    assert_eq!(
        find_jupyter_call_count(),
        1,
        "replay must not probe for jupyter"
    );
}

/// A literal ```{python} fence inside a *markdown* cell must stay inert:
/// mapping the fence's content through the source map lands in a
/// markdown-cell virtual file, which has no outputs to replay. Only the
/// real code cell gets the cell wrapper — one `cell-code` occurrence.
#[tokio::test]
async fn markdown_cell_literal_fence_stays_inert_while_real_cell_replays() {
    let out = preview_ast(LITERAL_FENCE_NB, ExecutionPolicy::None, None).await;
    assert!(
        out.ast_json.contains("not a real cell"),
        "markdown-cell fence content must survive verbatim: {}",
        out.ast_json
    );
    assert!(
        out.ast_json.contains("real cell ran"),
        "the real code cell's stored output must replay: {}",
        out.ast_json
    );
    assert_eq!(
        out.ast_json.matches("cell-code").count(),
        1,
        "exactly one echoed code cell — the markdown cell's literal fence must not become one: {}",
        out.ast_json
    );
}

/// Stored image output: the png payload is written to
/// `<stem>_files/figure-html/cell-1-output-1.png` next to the notebook
/// (FigureWriter, the same path live execution uses) and referenced from
/// the AST. This test builds its own temp dir so the figure file on disk
/// can be asserted.
#[tokio::test]
async fn stored_png_output_written_as_figure_file() {
    let temp = tempfile::TempDir::new().unwrap();
    let nb_path = temp.path().join("notebook.ipynb");
    std::fs::write(&nb_path, PNG_OUTPUT_NB).unwrap();
    let nb_path = nb_path.canonicalize().unwrap();

    let project = ProjectContext {
        dir: temp.path().to_path_buf(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&nb_path)],
        output_dir: temp.path().to_path_buf(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&nb_path);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.execution_policy = ExecutionPolicy::None;
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    let out = render_qmd_to_preview_ast(
        PNG_OUTPUT_NB.as_bytes(),
        &nb_path.display().to_string(),
        &mut ctx,
        runtime,
        None,
        Vec::new().into(),
    )
    .await
    .expect("png-output notebook must render");

    assert!(
        out.ast_json
            .contains("notebook_files/figure-html/cell-1-output-1.png"),
        "figure reference must reach the AST: {}",
        out.ast_json
    );
    let fig = temp
        .path()
        .join("notebook_files/figure-html/cell-1-output-1.png");
    assert!(
        fig.is_file(),
        "figure file must be written next to the notebook at {}",
        fig.display()
    );
    assert_eq!(
        find_jupyter_call_count(),
        1,
        "replay must not probe for jupyter"
    );
}

/// Route-through pin (plan Phase 3): `execute.enabled: true` from the
/// notebook's front-matter cell demands live execution — the document
/// must go down the resolved jupyter engine path, NOT the replay path.
///
/// The demand route is observed with a stub engine named "jupyter" that
/// declares the real `.ipynb` file claim (so `SourceConversionStage`
/// converts natively) but appends an unmistakable marker when *executed*.
/// A demanded document must carry the marker and none of its stored
/// outputs; machine-independent — no kernel, no PATH probing, works
/// identically whether or not the host has jupyter installed. Policy is
/// `All` (the `q2 render` default): a demanded document must reach the
/// engine, not be policy-skipped.
#[tokio::test]
async fn execute_enabled_routes_to_jupyter_not_replay() {
    let out = preview_ast(
        EXECUTE_DEMANDED_NB,
        ExecutionPolicy::All,
        Some(registry_with_stub_jupyter()),
    )
    .await;
    assert!(
        out.ast_json.contains(STUB_MARKER),
        "execute demand must route through the resolved jupyter engine (stub marker expected): {}",
        out.ast_json
    );
    assert!(
        !out.ast_json.contains("stored, must not replay"),
        "execution-demanded document must not silently replay stored outputs: {}",
        out.ast_json
    );
}

/// The complement, and the Phase 2 e2e finding 1 sub-nuance: with NO
/// execute demand, the resolved jupyter engine must never run — not even
/// for a zero-code-cell notebook, whose `.ipynb` claim short-circuits
/// resolution to jupyter all the same. The stub marker's presence would
/// mean the replay branch failed to claim the document.
#[tokio::test]
async fn no_execute_demand_replays_instead_of_running_jupyter() {
    let out = preview_ast(
        MARKDOWN_ONLY_NB,
        ExecutionPolicy::All,
        Some(registry_with_stub_jupyter()),
    )
    .await;
    assert!(
        !out.ast_json.contains(STUB_MARKER),
        "a notebook with no execute demand must replay, not run the jupyter engine: {}",
        out.ast_json
    );
    assert!(
        problems(&out).is_empty(),
        "replayed markdown-only notebook must render clean, got: {:?}",
        problems(&out)
    );
}
