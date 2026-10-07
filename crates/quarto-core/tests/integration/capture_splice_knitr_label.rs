//! Regression for bd-mu9i0bct: a labeled `fig-*` cell whose engine input had
//! its `#| label:` re-injected (knitr only, `label_reinject::inject`) must
//! still splice from its capture.
//!
//! The recorded `input_qmd` is the text the engine received, so its cell
//! carries the re-injected label line; the live pre-engine AST (label lifted
//! onto the wrapper Div) does not. `CaptureSpliceStage` strips the line from
//! the capture's AST before keying cells. A fake `knitr`-named engine run
//! through the real `record_capture` exercises the real re-injection with no
//! R dependency.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use quarto_core::engine::preview_record::record_capture;
use quarto_core::engine::{EngineRegistry, ExecutionEngine, LanguageClaim};
use quarto_core::engine::{ExecuteResult, ExecutionContext, ExecutionError};
use quarto_core::format::Format;
use quarto_core::pipeline::render_qmd_to_preview_ast;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

struct FakeKnitr;

impl ExecutionEngine for FakeKnitr {
    fn name(&self) -> &str {
        "knitr"
    }
    fn execute(&self, input: &str, _c: &ExecutionContext) -> Result<ExecuteResult, ExecutionError> {
        let mut out = String::new();
        let mut in_cell = false;
        for line in input.lines() {
            if line.starts_with("```{r}") {
                in_cell = true;
                out.push_str("::: {.cell}\n```{.r .cell-code}\n");
            } else if in_cell && line == "```" {
                in_cell = false;
                out.push_str("```\n\n::: {.cell-output .cell-output-stdout}\n```\nOUT_MARKER\n```\n:::\n:::\n");
            } else if in_cell && line.starts_with("#|") {
                // knitr consumes option lines
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        Ok(ExecuteResult::new(out))
    }
    fn claims_language(&self, l: &str, _f: Option<&str>) -> LanguageClaim {
        if l == "r" {
            LanguageClaim::Primary(1)
        } else {
            LanguageClaim::None
        }
    }
}

async fn run(doc: &str) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.qmd");
    std::fs::write(&path, doc).unwrap();
    let path = path.canonicalize().unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let mut reg = EngineRegistry::new();
    reg.register(Arc::new(FakeKnitr));
    let caps = record_capture(&path, &project, runtime.clone(), Some(Arc::new(reg)))
        .await
        .unwrap();
    let input_qmd = caps[0].input_qmd.clone();

    let doc_info = DocumentInfo::from_path(&path);
    let format = Format::html();
    let bins = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc_info, &format, &bins);
    let out = render_qmd_to_preview_ast(
        doc.as_bytes(),
        &path.display().to_string(),
        &mut ctx,
        runtime,
        Some(Arc::new(EngineRegistry::new())),
        caps,
    )
    .await
    .unwrap();
    (input_qmd, out.ast_json)
}

#[tokio::test]
async fn labeled_figure_cell_splices() {
    let (input_qmd, ast) = run("---\nengine: knitr\n---\n\n```{r}\n#| label: fig-two-sines\n#| fig-cap: Two sines\nplot(1)\n```\n").await;
    assert!(
        input_qmd.contains("#| label: fig-two-sines"),
        "premise: the recorded input carries the re-injected label line"
    );
    assert!(ast.contains("OUT_MARKER"), "labeled cell did not splice");
    assert!(ast.contains("sines"), "figure caption must survive");
}

#[tokio::test]
async fn unlabeled_cell_splices() {
    let (input_qmd, ast) = run("---\nengine: knitr\n---\n\n```{r}\nplot(1)\n```\n").await;
    let _ = input_qmd;
    assert!(ast.contains("OUT_MARKER"), "unlabeled cell did not splice");
}

/// The strip is engine-agnostic: a capture from any engine whose input still
/// carries the line (as every engine's will once bd-6sb1z0i4 drops the knitr
/// gate) splices too. Hand-built capture; the engine name is irrelevant.
#[tokio::test]
async fn labeled_figure_splices_for_any_engine_name() {
    let doc = "---\nengine: knitr\n---\n\n```{r}\n#| label: fig-two-sines\n#| fig-cap: Two sines\nplot(1)\n```\n";
    let capture = quarto_trace::EngineCapture {
        engine_name: "someengine".into(),
        input_qmd: "::: {#fig-two-sines}\n\n```{r}\n#| label: fig-two-sines\nplot(1)\n```\n\nTwo sines\n\n:::\n".into(),
        result: serde_json::json!({
            "markdown": "::: {#fig-two-sines}\n\n::: {.cell}\n```{.r .cell-code}\nplot(1)\n```\n\n::: {.cell-output .cell-output-stdout}\n```\nOUT_MARKER\n```\n:::\n:::\n\nTwo sines\n\n:::\n"
        }),
        files: Vec::new(),
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.qmd");
    std::fs::write(&path, doc).unwrap();
    let path = path.canonicalize().unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&path, runtime.as_ref()).unwrap();
    let doc_info = DocumentInfo::from_path(&path);
    let format = Format::html();
    let bins = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc_info, &format, &bins);
    let out = render_qmd_to_preview_ast(
        doc.as_bytes(),
        &path.display().to_string(),
        &mut ctx,
        runtime,
        Some(Arc::new(EngineRegistry::new())),
        vec![capture],
    )
    .await
    .unwrap();
    assert!(
        out.ast_json.contains("OUT_MARKER"),
        "labeled cell did not splice"
    );
}
