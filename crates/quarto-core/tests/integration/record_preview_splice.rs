//! EXR-14 Phase D, native end to end: a record recorded by a real `record_capture_run`
//! (a stub engine standing in for Jupyter) is spliced into the preview AST by unit hash.
//!
//! The preview renders with an *empty* engine registry, so every output marker that
//! shows up came from the record, never from running an engine.
#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::sync::Arc;

use serde_json::json;

use quarto_core::engine::preview_record::{compute_source_inputs, record_capture_run};
use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError,
    LanguageClaim, MarkdownEngine,
};
use quarto_core::format::Format;
use quarto_core::pandoc_request::captures::CapturePayload;
use quarto_core::pipeline::render_qmd_to_preview_ast;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::record::{EngineEntryInput, Record, RecordBuilder, UnitResult};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Stands in for an engine that owns one language; records one stdout line per unit,
/// `OUT:<first source line>`. `jupyter`/`python` for most tests; `knitr`/`r` for the
/// label-reinjection one (the engine stage re-injects `#| label:` for an engine named `knitr`).
struct StubEngine {
    name: &'static str,
    lang: &'static str,
}

const STUB_PYTHON: StubEngine = StubEngine {
    name: "jupyter",
    lang: "python",
};

impl ExecutionEngine for StubEngine {
    fn name(&self) -> &str {
        self.name
    }
    /// Like Jupyter's declaration: `echo` is display-only, so it is outside the execution hash.
    fn hash_definition(&self) -> quarto_core::record::HashDefinition {
        let mut def = quarto_core::record::HashDefinition::conservative(self.name);
        def.presentation_cell_options = vec!["echo".into()];
        def.presentation_execute_keys = vec!["echo".into()];
        def
    }
    fn claims_language(&self, language: &str, _first_class: Option<&str>) -> LanguageClaim {
        if language == self.lang {
            LanguageClaim::Primary(1)
        } else {
            LanguageClaim::None
        }
    }
    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        let mut result = ExecuteResult::new(input);
        if ctx.want_record {
            let def = self.hash_definition();
            let stub = |e: &dyn std::fmt::Display| {
                ExecutionError::execution_failed(self.name, e.to_string())
            };
            let mut builder = RecordBuilder::new(self.name, &def, &ctx.units, &ctx.author_texts)
                .map_err(|e| stub(&e))?;
            for (index, unit) in ctx.units.iter().enumerate() {
                if unit.lang() == self.lang {
                    let text = ctx.author_texts.0[index].text.clone();
                    let first = text
                        .lines()
                        .find(|l| !l.starts_with("#|"))
                        .unwrap_or("")
                        .to_string();
                    builder
                        .push(
                            index,
                            UnitResult {
                                outputs: vec![json!({
                                    "output_type": "stream",
                                    "name": "stdout",
                                    "text": format!("OUT:{first}\n"),
                                })],
                                execution_count: Some(1),
                                kernel: None,
                            },
                        )
                        .map_err(|e| stub(&e))?;
                }
            }
            result.record = Some(
                builder
                    .finish(EngineEntryInput {
                        document_metadata: ctx.document_metadata_json(),
                        owned_languages: ctx.owned_languages.clone(),
                        ..Default::default()
                    })
                    .map_err(|e| stub(&e))?,
            );
        }
        Ok(result)
    }
}

fn doc(cells: &[(&str, &str)]) -> String {
    let mut out = String::from("---\nformat: q2-preview\n---\n\n");
    for (opts, code) in cells {
        out.push_str(&format!("```{{python}}\n{opts}{code}\n```\n\n"));
    }
    out
}

async fn record_for(dir: &Path, source: &str) -> Record {
    record_with(dir, source, STUB_PYTHON).await
}

async fn record_with(dir: &Path, source: &str, engine: StubEngine) -> Record {
    let path = dir.join("doc.qmd");
    std::fs::write(&path, source).unwrap();
    let path = quarto_system_runtime::canonicalize(&path).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let mut registry = EngineRegistry::new();
    // Never let the real knitr own a `{r}` cell: it needs Rscript, which CI lacks, and the
    // test's point is the cell index, not R. A stub also named `knitr` replaces it.
    if engine.name != "knitr" {
        registry.register(Arc::new(StubEngine {
            name: "knitr",
            lang: "r",
        }));
    }
    registry.register(Arc::new(engine));
    record_capture_run(&path, &project, runtime, Some(Arc::new(registry)))
        .await
        .unwrap()
        .record
        .expect("the stub engine produced a record")
}

/// The registry for the preview side: markdown only. A recorded document's cells are
/// spliced from the record and never run, so no real engine belongs here. `new()` would
/// register the real Jupyter engine, which runs when Jupyter is installed and then
/// panics (`block_on` inside the test's runtime).
fn splice_only_registry() -> Arc<EngineRegistry> {
    let mut registry = EngineRegistry::empty();
    registry.register(Arc::new(MarkdownEngine::new()));
    Arc::new(registry)
}

/// Preview-render `source` with `record` and an empty registry; the AST JSON.
async fn preview(dir: &Path, source: &str, record: &Record) -> String {
    let path = dir.join("doc.qmd");
    std::fs::write(&path, source).unwrap();
    let path = quarto_system_runtime::canonicalize(&path).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let doc_info = DocumentInfo::from_path(&path);
    let format = Format::html();
    let bins = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc_info, &format, &bins);
    render_qmd_to_preview_ast(
        source.as_bytes(),
        &path.display().to_string(),
        &mut ctx,
        runtime,
        Some(splice_only_registry()),
        CapturePayload::Record(Arc::new(record.clone())),
    )
    .await
    .unwrap()
    .ast_json
}

fn outputs(ast: &str) -> usize {
    ast.matches("OUT:").count()
}

const CELLS: [(&str, &str); 3] = [("", "a = 1"), ("", "b = 2"), ("", "c = 3")];

#[tokio::test]
async fn record_splices_every_matching_cell() {
    let dir = tempfile::tempdir().unwrap();
    let source = doc(&CELLS);
    let record = record_for(dir.path(), &source).await;
    let ast = preview(dir.path(), &source, &record).await;
    assert_eq!(outputs(&ast), 3, "all three recorded outputs appear");
}

#[tokio::test]
async fn editing_one_cell_leaves_n_minus_1_outputs_and_one_source_cell() {
    let dir = tempfile::tempdir().unwrap();
    let record = record_for(dir.path(), &doc(&CELLS)).await;
    let edited = doc(&[("", "a = 1"), ("", "b = 22"), ("", "c = 3")]);
    let ast = preview(dir.path(), &edited, &record).await;
    assert_eq!(outputs(&ast), 2, "the edited cell has no recorded output");
    assert!(ast.contains("b = 22"), "the edited cell stays as source");
    assert!(!ast.contains("OUT:b = 2"), "the old output is not shown");
}

#[tokio::test]
async fn a_presentation_only_edit_keeps_all_outputs_with_the_new_presentation() {
    let dir = tempfile::tempdir().unwrap();
    let record = record_for(dir.path(), &doc(&CELLS)).await;
    let edited = doc(&[("", "a = 1"), ("#| echo: false\n", "b = 2"), ("", "c = 3")]);
    let ast = preview(dir.path(), &edited, &record).await;
    assert_eq!(outputs(&ast), 3, "echo does not strand the cell");
    assert!(ast.contains("OUT:b = 2"));
    // `b = 2` appears only as part of the output line: the source block is hidden.
    assert_eq!(
        ast.matches("b = 2").count(),
        1,
        "echo: false hides the b source block"
    );
    assert_eq!(
        ast.matches("a = 1").count(),
        2,
        "other cells keep source + output"
    );
}

// ── cross-site conformance (Phase C) ───────────────────────────────────────

use std::sync::Mutex;

use quarto_core::pipeline::build_q2_preview_pipeline_stages;
use quarto_core::stage::stages::RECORD_STALENESS_KIND;
use quarto_core::stage::{LoadedSource, Pipeline, PipelineData, PipelineObserver, StageContext};

/// Collects the payload of the splice's `RecordStaleness` auxiliary event.
#[derive(Default)]
struct StalenessObserver(Mutex<Option<serde_json::Value>>);

impl PipelineObserver for StalenessObserver {
    fn on_auxiliary_data(&self, _stage: &str, _index: usize, kind: &str, data: &serde_json::Value) {
        if kind == RECORD_STALENESS_KIND {
            *self.0.lock().unwrap() = Some(data.clone());
        }
    }
}

/// Run the real preview pipeline (splice stage included) over `source` with `record` and
/// return the splice's staleness report.
async fn splice_report(dir: &Path, source: &str, record: &Record) -> serde_json::Value {
    let path = dir.join("doc.qmd");
    std::fs::write(&path, source).unwrap();
    let path = quarto_system_runtime::canonicalize(&path).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let observer = Arc::new(StalenessObserver::default());
    let mut ctx = StageContext::new(
        runtime,
        Format::from_format_string("q2-preview").unwrap(),
        project,
        DocumentInfo::from_path(&path),
    )
    .unwrap()
    .with_observer(observer.clone());
    ctx.registry = splice_only_registry();
    let stages = build_q2_preview_pipeline_stages(CapturePayload::Record(Arc::new(record.clone())));
    let input = PipelineData::LoadedSource(LoadedSource::new(path, source.as_bytes().to_vec()));
    Pipeline::new(stages)
        .unwrap()
        .run(input, &mut ctx)
        .await
        .unwrap();
    let report = observer.0.lock().unwrap().take();
    report.expect("the record splice reports staleness")
}

/// `(document, total units the shared scan finds, python units (cells and spans) the stub records)`.
///
/// Every document is read by the writer (which masks a clone of the AST) and again by the
/// splice (which masks its own clone), so a display block in the corpus puts the mask on the
/// path of both; the plain documents are the mask-off control.
const CONFORMANCE: &[(&str, usize, usize)] = &[
    // Mask off: nothing to mask.
    (
        "```{python}\na = 1\n```\n\nText `{python} a` here.\n\n```{python}\nb = 2\n```\n",
        3,
        3,
    ),
    // Mask on: a four-backtick display block holding a cell and a span (no units), then a real one.
    (
        "````markdown\n```{python}\nshown = 1\n```\n\nSee `{python} shown`.\n````\n\n```{python}\nreal = 1\n```\n",
        1,
        1,
    ),
    // Mask on: a blockquote holding a real fence (a unit) beside a display block.
    (
        "> ```{python}\n> quoted = 1\n> ```\n\n```text\n```{python}\nnot_a_unit\n```\n```\n",
        1,
        1,
    ),
    // A doubled brace is literal text, never claimed.
    (
        "Literal `{{python}} x` text.\n\n```{python}\nafter = 1\n```\n",
        1,
        1,
    ),
    // A ceded braced cell and a dotted token count in the document-wide cell index.
    (
        "```{r}\nceded <- 1\n```\n\n```{python.marimo}\nm = 1\n```\n\n```{python}\nafter_both = 1\n```\n",
        3,
        1,
    ),
];

#[tokio::test]
async fn splice_ids_equal_the_writers_with_and_without_the_mask() {
    for (i, (source, total, cells)) in CONFORMANCE.iter().enumerate() {
        let source = format!("---\nformat: q2-preview\n---\n\n{source}");
        let dir = tempfile::tempdir().unwrap();
        let record = record_for(dir.path(), &source).await;
        // The Python stub's cells only: the `knitr` stub also records a `{r}` chunk, which this
        // fixture does not count.
        let written: Vec<String> = record
            .cells
            .iter()
            .filter(|c| c.metadata.quarto_engine.as_deref() == Some(STUB_PYTHON.name))
            .map(|c| c.metadata.quarto_cell_id.clone())
            .collect();
        assert_eq!(
            written.len(),
            *cells,
            "fixture {i}: the writer's units {written:?}"
        );

        let report = splice_report(dir.path(), &source, &record).await;
        let units = report["units"].as_array().unwrap();
        assert_eq!(
            units.len(),
            *total,
            "fixture {i}: no phantom or missing units: {report}"
        );
        let read: Vec<&str> = units.iter().map(|u| u["id"].as_str().unwrap()).collect();
        // Every id the writer recorded is an id the splice computed, in the same order.
        let mut cursor = 0;
        for id in &written {
            let at = read[cursor..]
                .iter()
                .position(|r| r == id)
                .unwrap_or_else(|| panic!("fixture {i}: writer id {id} not among {read:?}"));
            cursor += at + 1;
        }
        assert!(
            report["orphaned"].as_array().unwrap().is_empty(),
            "fixture {i}: nothing orphaned: {report}"
        );
        for id in &written {
            let unit = units.iter().find(|u| u["id"] == *id).unwrap();
            assert_eq!(unit["staleness"], "fresh", "fixture {i}: {id}");
        }
    }
}

/// Canonical recorded input (EXR-10 landed): a knitr labeled cell hashes the same when the
/// writer records it and when the splice reads it. The engine stage re-injects the
/// `#| label:` line for an engine named `knitr` *after* hashing, and crossref sugaring
/// strips it from the AST the splice sees, so neither may leak into the id or the hash.
#[tokio::test]
async fn a_knitr_labeled_cell_hashes_the_same_at_record_and_splice_time() {
    let source = "---\nformat: q2-preview\nengine: knitr\n---\n\n```{r}\n#| label: fig-two-sines\n#| fig-cap: Two sines\nplot(1)\n```\n";
    let dir = tempfile::tempdir().unwrap();
    let engine = StubEngine {
        name: "knitr",
        lang: "r",
    };
    let record = record_with(dir.path(), source, engine).await;
    assert_eq!(record.cells.len(), 1, "the labeled cell is recorded");

    let report = splice_report(dir.path(), source, &record).await;
    let unit = &report["units"].as_array().unwrap()[0];
    assert_eq!(
        unit["id"], record.cells[0].metadata.quarto_cell_id,
        "same id at both ends"
    );
    assert_eq!(unit["staleness"], "fresh", "{report}");
    assert!(report["orphaned"].as_array().unwrap().is_empty());
}

// ── record-level staleness (EXR-15) ───────────────────────────────────────────

/// Whether `record` is stale against the document now on disk in `dir`, judged the way the
/// preview watcher does: the masked, overlay-free pre-engine inputs, no engine loaded.
async fn stale_now(dir: &Path, record: &Record) -> bool {
    let path = quarto_system_runtime::canonicalize(&dir.join("doc.qmd")).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let source = compute_source_inputs(&path, &project, runtime)
        .await
        .unwrap();
    quarto_core::record::record_is_stale(
        record,
        &String::from_utf8_lossy(&source.input_qmd),
        &source.sugared_blocks,
        &source.metadata,
    )
    .unwrap()
}

/// The viewer computes its units from the masked pre-engine AST, so a display block holding a
/// cell or a span adds no phantom unit and the recorded document is fresh (shares the splice
/// conformance corpus above: the same documents, judged by hash instead of by id).
#[tokio::test]
async fn a_recorded_document_is_fresh_with_and_without_the_mask() {
    for (i, (source, _, _)) in CONFORMANCE.iter().enumerate() {
        let source = format!("---\nformat: q2-preview\n---\n\n{source}");
        let dir = tempfile::tempdir().unwrap();
        let record = record_for(dir.path(), &source).await;
        assert!(
            !stale_now(dir.path(), &record).await,
            "fixture {i}: a freshly recorded document must not be stale"
        );
    }
}

#[tokio::test]
async fn a_prose_edit_leaves_the_record_fresh_and_a_code_edit_does_not() {
    let source = doc(&[("", "a = 1")]);
    let dir = tempfile::tempdir().unwrap();
    let record = record_for(dir.path(), &source).await;

    std::fs::write(
        dir.path().join("doc.qmd"),
        source.replace(
            "format: q2-preview\n---\n",
            "format: q2-preview\n---\n\nNew prose.\n",
        ),
    )
    .unwrap();
    assert!(!stale_now(dir.path(), &record).await, "prose edit");

    std::fs::write(dir.path().join("doc.qmd"), source.replace("a = 1", "a = 2")).unwrap();
    assert!(stale_now(dir.path(), &record).await, "code edit");
}

/// bd-n1vmlalw: the engine stage re-injects the `#| label:` line for an engine named `knitr`
/// after hashing, so a freshly recorded labeled knitr document used to look stale at once.
/// The hash is taken from the sugared snapshot, never the re-injected text.
#[tokio::test]
async fn a_freshly_recorded_labeled_knitr_document_is_not_stale() {
    let source = "---\nformat: q2-preview\nengine: knitr\n---\n\n```{r}\n#| label: fig-two-sines\n#| fig-cap: Two sines\nplot(1)\n```\n";
    let dir = tempfile::tempdir().unwrap();
    let engine = StubEngine {
        name: "knitr",
        lang: "r",
    };
    let record = record_with(dir.path(), source, engine).await;
    assert!(!stale_now(dir.path(), &record).await);
}

// ── legacy-versus-record parity (plan Risks: presenter output re-parsed to blocks) ──

use quarto_core::engine::preview_record::compute_input_qmd;
use quarto_core::pipeline::{HtmlRenderConfig, render_qmd_to_html};
use quarto_core::record::read_record;

/// Shadow cases whose output needs no supporting file (figures need materialized files,
/// and `ceded-ojs` is another engine's), so both payloads render from bytes alone.
const PARITY_CASES: &[&str] = &[
    "echo-false",
    "error-allowed",
    "html",
    "include-false",
    "markdown-only",
    "output-false",
    "plain-value",
    "stdout",
    "twenty-cells",
    "warning-false-stderr",
];

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Render `source` to HTML with an empty engine registry and the given config.
async fn html_with(path: &Path, source: &str, config: HtmlRenderConfig) -> String {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    let doc_info = DocumentInfo::from_path(path);
    let format = Format::html();
    let bins = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc_info, &format, &bins);
    // Through the config: `render_qmd_to_html` overwrites the context's override with
    // `config.engine_registry`, so setting it on `ctx` would silently do nothing.
    let config = config.with_engine_registry(splice_only_registry());
    render_qmd_to_html(
        source.as_bytes(),
        &path.display().to_string(),
        &mut ctx,
        &config,
        runtime,
    )
    .await
    .unwrap()
    .html
}

/// The record splice and the legacy splice give the same page for the same Jupyter run.
/// The legacy side is the frozen preview capture markdown (`<case>.preview.md`); the record
/// side is the record the same run built (`<case>.record.json`). Kernel-free. Every case is
/// byte-identical except `include-false`, where the record is the correct one (see below).
#[tokio::test]
async fn record_splice_matches_legacy_splice_on_the_jupyter_fixtures() {
    let mut problems = Vec::new();
    for name in PARITY_CASES {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.qmd");
        let source =
            std::fs::read_to_string(fixtures().join(format!("jupyter-record-shadow/{name}.qmd")))
                .unwrap();
        std::fs::write(&path, &source).unwrap();
        let path = quarto_system_runtime::canonicalize(&path).unwrap();

        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
        let input_qmd =
            String::from_utf8(compute_input_qmd(&path, &project, runtime).await.unwrap()).unwrap();
        let markdown = std::fs::read_to_string(
            fixtures().join(format!("jupyter-markdown-goldens/{name}.preview.md")),
        )
        .unwrap();
        let legacy = quarto_trace::EngineCapture {
            engine_name: "jupyter".into(),
            input_qmd,
            result: json!({ "markdown": markdown }),
            record: None,
            files: Vec::new(),
        };
        let record_json = std::fs::read_to_string(
            fixtures().join(format!("jupyter-markdown-goldens/{name}.record.json")),
        )
        .unwrap();
        let record = read_record(&record_json).unwrap();

        let from_legacy = html_with(
            &path,
            &source,
            HtmlRenderConfig::default().with_captures(vec![legacy]),
        )
        .await;
        let from_record = html_with(
            &path,
            &source,
            HtmlRenderConfig::default().with_record(Arc::new(record)),
        )
        .await;
        if *name == "include-false" {
            // The one deliberate divergence. A hidden cell has an empty output run, which the
            // legacy structural splice cannot tell from "no match", so it leaves the cell
            // showing as source. The record matches the cell by hash and presents it as
            // hidden, which is what `include: false` means.
            assert!(
                from_legacy.contains("nothing at all"),
                "legacy leaves the include: false cell as source"
            );
            assert!(
                !from_record.contains("nothing at all"),
                "the record hides the include: false cell"
            );
            continue;
        }
        if from_legacy != from_record {
            problems.push(format!(
                "{name}: pages differ\n--- legacy\n{from_legacy}\n--- record\n{from_record}"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}

// ── guests (EXR-15 Phase C) ────────────────────────────────────────────────

/// An engine that claims `python` and counts every `execute`. A guest must never reach it.
struct CountingEngine(Arc<std::sync::atomic::AtomicUsize>);

impl ExecutionEngine for CountingEngine {
    fn name(&self) -> &str {
        "jupyter"
    }
    fn claims_language(&self, language: &str, _first_class: Option<&str>) -> LanguageClaim {
        if language == "python" {
            LanguageClaim::Primary(1)
        } else {
            LanguageClaim::None
        }
    }
    fn execute(
        &self,
        input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(ExecuteResult::new(input))
    }
}

/// Preview-render as a guest: execution forbidden (`ExecutionPolicy::None`) and an engine
/// that would count any attempt to run. Returns the AST JSON and the attempt count.
async fn guest_preview(dir: &Path, source: &str, record: &Record) -> (String, usize) {
    let path = dir.join("doc.qmd");
    std::fs::write(&path, source).unwrap();
    let path = quarto_system_runtime::canonicalize(&path).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let doc_info = DocumentInfo::from_path(&path);
    let format = Format::html();
    let bins = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc_info, &format, &bins);
    ctx.execution_policy = quarto_core::engine::ExecutionPolicy::None;
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(CountingEngine(runs.clone())));
    let ast = render_qmd_to_preview_ast(
        source.as_bytes(),
        &path.display().to_string(),
        &mut ctx,
        runtime,
        Some(Arc::new(registry)),
        CapturePayload::Record(Arc::new(record.clone())),
    )
    .await
    .unwrap()
    .ast_json;
    (ast, runs.load(std::sync::atomic::Ordering::SeqCst))
}

/// A guest with a record in the sidecar shows the host's output, and, even with a capable
/// engine in its registry, never executes: a fresh cell shows its recorded output, an
/// edited cell stays as source.
#[tokio::test]
async fn a_guest_shows_the_hosts_recorded_output_and_never_executes() {
    let dir = tempfile::tempdir().unwrap();
    let source = doc(&CELLS);
    let record = record_for(dir.path(), &source).await;

    let host = preview(dir.path(), &source, &record).await;
    let (guest, runs) = guest_preview(dir.path(), &source, &record).await;
    assert_eq!(guest, host, "the guest renders what the host renders");
    assert_eq!(outputs(&guest), 3);
    assert_eq!(runs, 0, "a guest cannot trigger execution");

    let edited = doc(&[("", "a = 1"), ("", "b = 22"), ("", "c = 3")]);
    let (guest, runs) = guest_preview(dir.path(), &edited, &record).await;
    assert_eq!(outputs(&guest), 2, "the edited cell has no recorded output");
    assert!(guest.contains("b = 22"), "the edited cell stays as source");
    assert_eq!(runs, 0, "a stale cell is not re-run on a guest");
}
