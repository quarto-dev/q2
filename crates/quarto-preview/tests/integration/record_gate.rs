//! RIU-1 Phase A: with the preview's record gate ON, a document whose engines do not
//! all build a record still shows every engine's output. The record-less rule stores
//! the whole document in the legacy format, so the reader gets one capture per engine.
//!
//! Drives the real capture pipeline (`record_capture_run`) with stub engines, writes
//! the capture doc through `write_capture_doc_gated(.., true)` and reads it back with
//! `read_capture_from_doc`, the reader the page uses. No R or Python runtime needed.

use std::sync::Arc;

use quarto_core::engine::preview_record::record_capture_run;
use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError, LanguageClaim,
};
use quarto_core::pandoc_request::captures::CapturePayload;
use quarto_core::project::ProjectContext;
use quarto_core::record::{EngineEntryInput, RecordBuilder, UnitResult};
use quarto_hub::HubContext;
use quarto_hub::context::HubConfig;
use quarto_hub::storage::StorageManager;
use quarto_preview::capture_driver::{read_capture_from_doc, write_capture_doc_gated};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Owns one language and builds a record for it when asked (as Jupyter does).
struct RecordingEngine {
    name: &'static str,
    lang: &'static str,
}

fn stub_error(e: impl std::fmt::Display) -> ExecutionError {
    ExecutionError::execution_failed("stub", e.to_string())
}

impl ExecutionEngine for RecordingEngine {
    fn name(&self) -> &str {
        self.name
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
            let mut builder = RecordBuilder::new(self.name, &def, &ctx.units, &ctx.author_texts)
                .map_err(stub_error)?;
            for (index, unit) in ctx.units.iter().enumerate() {
                if unit.lang() == self.lang {
                    builder
                        .push(
                            index,
                            UnitResult {
                                outputs: vec![serde_json::json!({
                                    "output_type": "stream",
                                    "name": "stdout",
                                    "text": format!("{}\n", self.name),
                                })],
                                execution_count: Some(1),
                                kernel: None,
                            },
                        )
                        .map_err(stub_error)?;
                }
            }
            result.record = Some(
                builder
                    .finish(EngineEntryInput {
                        document_metadata: ctx.document_metadata_json(),
                        ..Default::default()
                    })
                    .map_err(stub_error)?,
            );
        }
        Ok(result)
    }
}

/// Owns one language and never builds a record (as knitr does today).
struct RecordlessEngine {
    name: &'static str,
    lang: &'static str,
}

impl ExecutionEngine for RecordlessEngine {
    fn name(&self) -> &str {
        self.name
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
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        Ok(ExecuteResult::passthrough(&format!(
            "{input}\n<!-- {} -->\n",
            self.name
        )))
    }
}

/// Run `qmd` through the capture pipeline, write it with the gate on, read it back.
async fn round_trip(qmd: &str, registry: Option<EngineRegistry>) -> CapturePayload {
    let project_dir = tempfile::TempDir::with_prefix("q2-riu1-gate-").unwrap();
    let path = project_dir.path().join("doc.qmd");
    std::fs::write(&path, qmd).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();

    // As the driver does (`re_execute.rs`): a blocking worker and `pollster`, because the
    // Jupyter engine drives its own runtime and cannot start one under a tokio executor.
    let run = tokio::task::spawn_blocking(move || {
        pollster::block_on(record_capture_run(
            &path,
            &project,
            runtime,
            registry.map(Arc::new),
        ))
    })
    .await
    .unwrap()
    .expect("capture pipeline runs");

    let data = tempfile::TempDir::new().unwrap();
    let storage = StorageManager::new(data.path()).unwrap();
    let ctx = Arc::new(
        HubContext::new(storage, HubConfig::default())
            .await
            .unwrap(),
    );
    let doc_id = write_capture_doc_gated(&ctx, "doc.qmd", &run, true)
        .await
        .expect("capture doc written");
    read_capture_from_doc(&ctx, &doc_id)
        .await
        .expect("capture doc reads back")
}

fn engine_names(payload: CapturePayload) -> Vec<String> {
    payload
        .into_captures()
        .into_iter()
        .map(|c| c.engine_name)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recordless_engine_beside_a_recording_one_shows_both_outputs() {
    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(RecordingEngine {
        name: "stub-rec",
        lang: "stub-rec",
    }));
    registry.register(Arc::new(RecordlessEngine {
        name: "stub-plain",
        lang: "stub-plain",
    }));
    let qmd = "---\ntitle: mixed\n---\n\n```{stub-rec}\na\n```\n\n```{stub-plain}\nb\n```\n";

    let payload = round_trip(qmd, Some(registry)).await;

    assert!(
        matches!(payload, CapturePayload::Legacy(_)),
        "the record-less engine keeps the whole document in the legacy format"
    );
    let mut names = engine_names(payload);
    names.sort();
    assert_eq!(names, ["stub-plain", "stub-rec"], "both engines' outputs");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_engine_in_the_record_gives_a_record() {
    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(RecordingEngine {
        name: "stub-rec",
        lang: "stub-rec",
    }));
    let qmd = "---\ntitle: recorded\n---\n\n```{stub-rec}\na\n```\n";

    match round_trip(qmd, Some(registry)).await {
        CapturePayload::Record(r) => assert_eq!(r.cells.len(), 1),
        other => panic!("expected a record, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recordless_engine_alone_shows_its_output() {
    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(RecordlessEngine {
        name: "stub-plain",
        lang: "stub-plain",
    }));
    let qmd = "---\ntitle: plain\n---\n\n```{stub-plain}\nb\n```\n";

    let payload = round_trip(qmd, Some(registry)).await;
    assert_eq!(engine_names(payload), ["stub-plain"]);
}

// ── real engines (the S1 fixtures' shapes) ─────────────────────────────────

fn r_available() -> bool {
    std::process::Command::new("Rscript")
        .args([
            "-e",
            "if (!requireNamespace(\"knitr\", quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn python_available() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import ipykernel, jupyter_core"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Every output text the payload carries, whether it is legacy captures or a record.
fn captured_text(payload: &CapturePayload) -> String {
    match payload {
        CapturePayload::Legacy(captures) => captures
            .iter()
            .map(|c| c.result.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        CapturePayload::Record(r) => r
            .cells
            .iter()
            .flat_map(|c| c.outputs.iter().map(|o| o.to_string()))
            .collect::<Vec<_>>()
            .join("\n"),
        CapturePayload::Empty => String::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn knitr_asis_chunk_shows_its_output_with_the_gate_on() {
    if !r_available() {
        eprintln!("SKIP: knitr (Rscript + knitr package) not available");
        return;
    }
    let qmd = "---\ntitle: asis\nengine: knitr\n---\n\n```{r}\n#| results: asis\ncat(\"ASIS-SENTINEL\")\n```\n";
    let payload = round_trip(qmd, None).await;
    assert!(
        !matches!(payload, CapturePayload::Empty),
        "the document must have a payload"
    );
    assert!(
        captured_text(&payload).contains("ASIS-SENTINEL"),
        "the asis output must survive: {}",
        captured_text(&payload)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_python_cell_and_a_knitr_chunk_show_both_outputs_with_the_gate_on() {
    if !r_available() || !python_available() {
        eprintln!("SKIP: needs Rscript+knitr and python3 with ipykernel");
        return;
    }
    let qmd = "---\ntitle: mixed\njupyter: python3\n---\n\n```{python}\nprint(\"PY-SENTINEL\")\n```\n\n```{r}\ncat(\"R-SENTINEL\")\n```\n";
    let payload = round_trip(qmd, None).await;
    let text = captured_text(&payload);
    assert!(text.contains("PY-SENTINEL"), "python output lost: {text}");
    assert!(text.contains("R-SENTINEL"), "knitr output lost: {text}");
}
