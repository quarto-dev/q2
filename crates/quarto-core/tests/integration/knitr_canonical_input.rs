//! EXR-10: the recorded engine input is the canonical serialized document, and
//! the engine receives the cell options the crossref pass consumed as a map.
//!
//! A fake engine (named `knitr`, or `jupyter` for the enabling test) runs
//! through the real `record_capture`, so no R is needed.
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use quarto_core::engine::preview_record::{compute_input_qmd, record_capture};
use quarto_core::engine::{
    CellOption, CellOptionsMap, EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine,
    ExecutionError, LanguageClaim,
};
use quarto_core::project::ProjectContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

struct Fake {
    name: &'static str,
    seen: Arc<Mutex<Vec<(String, CellOptionsMap)>>>,
}

impl ExecutionEngine for Fake {
    fn name(&self) -> &str {
        self.name
    }
    fn execute(&self, input: &str, c: &ExecutionContext) -> Result<ExecuteResult, ExecutionError> {
        self.seen
            .lock()
            .unwrap()
            .push((input.to_string(), c.cell_options.clone()));
        Ok(ExecuteResult::new(input.to_string()))
    }
    fn claims_language(&self, l: &str, _f: Option<&str>) -> LanguageClaim {
        if l == "r" {
            LanguageClaim::Primary(1)
        } else {
            LanguageClaim::None
        }
    }
}

struct Recorded {
    path: PathBuf,
    project: ProjectContext,
    runtime: Arc<dyn SystemRuntime>,
    input_qmd: String,
    seen: Vec<(String, CellOptionsMap)>,
    _dir: tempfile::TempDir,
}

async fn record(engine_name: &'static str, doc: &str) -> Recorded {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.qmd");
    std::fs::write(&path, doc).unwrap();
    let path = path.canonicalize().unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut reg = EngineRegistry::new();
    reg.register(Arc::new(Fake {
        name: engine_name,
        seen: seen.clone(),
    }));
    let caps = record_capture(&path, &project, runtime.clone(), Some(Arc::new(reg)))
        .await
        .unwrap();
    let seen = seen.lock().unwrap().clone();
    Recorded {
        input_qmd: caps[0].input_qmd.clone(),
        path,
        project,
        runtime,
        seen,
        _dir: dir,
    }
}

const LABELED: &str =
    "---\nengine: knitr\n---\n\n```{r}\n#| label: fig-x\n#| fig-cap: A plot\nplot(1)\n```\n";

/// The strand's repro (bd-n1vmlalw): the recorded input must equal what the
/// staleness check recomputes.
#[tokio::test]
async fn fresh_labeled_knitr_capture_is_not_stale() {
    let r = record("knitr", LABELED).await;
    let live = compute_input_qmd(&r.path, &r.project, r.runtime.clone())
        .await
        .unwrap();
    assert_eq!(String::from_utf8(live).unwrap(), r.input_qmd);
}

#[tokio::test]
async fn canonical_input_has_no_reinjected_label() {
    let r = record("knitr", LABELED).await;
    assert!(
        !r.input_qmd.contains("#| label:"),
        "recorded input has a label line the author's canonical form lacks:\n{}",
        r.input_qmd
    );
}

#[tokio::test]
async fn cell_options_map_carries_consumed_labels() {
    let doc = "---\nengine: knitr\n---\n\n```{r}\n#| label: fig-a\n#| fig-cap: A\nplot(1)\n```\n\n> ```{r}\n> #| label: fig-b\n> #| fig-cap: B\n> plot(2)\n> ```\n";
    let r = record("knitr", doc).await;
    assert_eq!(r.seen.len(), 1);
    let entry = |id: &str| CellOption {
        div_id: id.to_string(),
        label: id.to_string(),
    };
    assert_eq!(
        r.seen[0].1,
        CellOptionsMap::new(vec![entry("fig-a"), entry("fig-b")])
    );
}

/// Enabling test for bd-6sb1z0i4 (extend label naming to Jupyter and drop the
/// `engine.name() == "knitr"` gate): a labeled document recorded under a
/// non-knitr engine is not stale either. With the gate gone for every engine,
/// this stays true by construction; it is written now so that issue lands on a
/// proven base.
#[tokio::test]
async fn labeled_jupyter_doc_is_not_stale_after_record() {
    let doc = LABELED.replace("engine: knitr", "engine: jupyter");
    let r = record("jupyter", &doc).await;
    let live = compute_input_qmd(&r.path, &r.project, r.runtime.clone())
        .await
        .unwrap();
    assert_eq!(String::from_utf8(live).unwrap(), r.input_qmd);
}
