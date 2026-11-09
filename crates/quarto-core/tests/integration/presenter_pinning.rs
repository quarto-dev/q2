/*
 * tests/integration/presenter_pinning.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! OPR-0a: pinning tests for the entry paths into the one presenter, written before OPR-1a
//! merges the per-cell step. Companion to the loop tests in
//! `src/stage/stages/record_splice/tests/opr_pinning.rs` and the option-reader test in
//! `src/presenter/options.rs` (`opr_pinning`).
//!
//! - The EXR-6 goldens through the **knitr entry path**: every record cell gets the marker
//!   `quarto_resolved_options: {}`, which is what sends a cell through `present_knitr_cell`
//!   instead of `render_cell_with`. The shadow goldens and the six `ipynb-*` goldens say what
//!   the two paths share today.
//! - A real knitr document under `execute: {output: false}` (R and knitr required; skips
//!   cleanly without them): R bakes the document scope into the record's resolved options.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::engine::content_processors::ORIGINAL_FILE_ID;
use quarto_core::engine::units::{Unit, runs_in_legacy_executor, units_for_engine};
use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError, LanguageClaim,
};
use quarto_core::presenter::document::present_document;
use quarto_core::presenter::figures::FsFigureSink;
use quarto_core::presenter::inline::InlineTarget;
use quarto_core::record::{
    AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, fill_ids, read_record,
};
use quarto_core::stage::stages::ENGINE_CAPTURE_KIND;
use serde_json::{Value, json};

use crate::engine_stage_driver::{AuxRecorder, run_engine_stage};

const MARKER: &str = "quarto_resolved_options";

const SHADOW_CASES: &[&str] = &[
    "ceded-ojs",
    "echo-false",
    "error-allowed",
    "hidden-then-plot",
    "html",
    "include-false",
    "markdown-only",
    "output-false",
    "plain-value",
    "png-plus-text",
    "stdout",
    "twenty-cells",
    "warning-false-stderr",
];

const IPYNB_CASES: &[&str] = &[
    "error-output",
    "html-output",
    "literal-fence",
    "markdown-only",
    "png-output",
    "stdout",
];

fn goldens_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jupyter-markdown-goldens")
}

fn shadow_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jupyter-record-shadow")
        .join(format!("{name}.qmd"))
}

/// The capture's markdown with the one value that is not a function of the fixture (the
/// copied file's mtime) masked, as `jupyter_markdown_goldens` does.
fn markdown_of(capture: &Value) -> String {
    let markdown = capture["result"]["markdown"]
        .as_str()
        .expect("the capture carries markdown");
    regex::Regex::new(r"date-modified: \d{4}-\d{2}-\d{2}")
        .unwrap()
        .replace_all(markdown, "date-modified: <date>")
        .into_owned()
}

/// Run `engine` as the only engine through the engine stage over `input` and return the
/// markdown it produced.
fn run_with(engine: Arc<dyn ExecutionEngine>, input: &Path) -> String {
    let mut registry = EngineRegistry::new();
    registry.register(engine);
    let observer = AuxRecorder::arc(false);
    run_engine_stage(input, observer.clone(), Some(Arc::new(registry)))
        .expect("the engine stage runs");
    let captures = observer.events_of(ENGINE_CAPTURE_KIND);
    assert_eq!(captures.len(), 1, "one capture: {captures:?}");
    markdown_of(&captures[0].1)
}

/// Presents the stored record of a shadow golden case, with or without the knitr marker on
/// every record cell.
struct StoredRecordJupyter {
    record_json: String,
    marker: bool,
}

impl ExecutionEngine for StoredRecordJupyter {
    fn name(&self) -> &str {
        "jupyter"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn claims_language(&self, language: &str, _first_class: Option<&str>) -> LanguageClaim {
        if language.eq_ignore_ascii_case("python") {
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
        let mut record = read_record(&self.record_json).expect("the stored record reads");
        if self.marker {
            for cell in &mut record.cells {
                cell.metadata.extra.insert(MARKER.into(), json!({}));
            }
        }
        let units = units_for_engine(&ctx.units, input);
        let mut fig = FsFigureSink::new(&ctx.source_path);
        let presented = present_document(
            &record.cells,
            input,
            &units,
            ctx.execute_scope.as_ref(),
            InlineTarget::from_format(&ctx.format),
            &ctx.format,
            &[],
            &mut fig,
        )
        .map_err(|e| ExecutionError::execution_failed("jupyter", e.to_string()))?;
        Ok(ExecuteResult::new(presented.markdown))
    }
}

fn shadow_markdown(name: &str, marker: bool) -> (String, String) {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("doc.qmd");
    std::fs::copy(shadow_fixture(name), &input).unwrap();
    let record_json =
        std::fs::read_to_string(goldens_root().join(format!("{name}.record.json"))).unwrap();
    let markdown = run_with(
        Arc::new(StoredRecordJupyter {
            record_json,
            marker,
        }),
        &input,
    );
    let golden = std::fs::read_to_string(goldens_root().join(format!("{name}.md"))).unwrap();
    (golden, markdown)
}

/// S2: with the marker on every record cell, 12 of the 13 shadow goldens are byte-identical.
/// The one that is not, `warning-false-stderr`, differs because `knitr_visibility` does not
/// read `warning` (OPR-2 moves that enforcement; D2).
#[test]
fn shadow_goldens_through_the_knitr_entry_path() {
    let mut differing = Vec::new();
    for name in SHADOW_CASES {
        // The control: with no marker the stand-in reproduces the golden (the oracle test).
        let (golden, unmarked) = shadow_markdown(name, false);
        assert_eq!(unmarked, golden, "{name}: control without the marker");
        let (_, marked) = shadow_markdown(name, true);
        if marked != golden {
            differing.push((*name, golden, marked));
        }
    }
    // OPR-1a: keep this equivalence: the only difference is the `warning` row below.
    let names: Vec<_> = differing.iter().map(|d| d.0).collect();
    assert_eq!(names, ["warning-false-stderr"]);
    let (_, golden, marked) = &differing[0];
    assert!(!golden.contains("cell-output-stderr"), "{golden}");
    assert!(marked.contains("cell-output-stderr"), "{marked}");
}

/// Replays a stored notebook the way `IpynbReplayEngine` does (one record cell per fence that
/// maps into a code cell), optionally marking every record cell as a knitr one.
struct MarkedIpynbReplay {
    marker: bool,
}

impl ExecutionEngine for MarkedIpynbReplay {
    fn name(&self) -> &str {
        "ipynb"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        let mut units = units_for_engine(&ctx.units, input);
        if !units
            .iter()
            .any(|u| matches!(u, Unit::Cell(c) if runs_in_legacy_executor(c)))
        {
            return Ok(ExecuteResult::new(input));
        }
        let bytes = ctx
            .source_context
            .get_file(ORIGINAL_FILE_ID)
            .and_then(|f| f.content.as_deref())
            .expect("the notebook is registered");
        let notebook: Value = serde_json::from_str(bytes).unwrap();
        let cells = notebook["cells"].as_array().unwrap();
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        let def = HashDefinition::conservative("ipynb");
        let mut builder = RecordBuilder::new("ipynb", &def, &units, &texts).unwrap();
        for (index, unit) in units.iter().enumerate() {
            let Unit::Cell(block) = unit else { continue };
            if !runs_in_legacy_executor(block) {
                continue;
            }
            let Some(mapped) = ctx
                .source_info
                .map_offset(block.code_start, &ctx.source_context)
            else {
                continue;
            };
            let Some(i) = mapped.file_id.0.checked_sub(ORIGINAL_FILE_ID.0 + 1) else {
                continue;
            };
            let Some(stored) = cells.get(i) else {
                continue;
            };
            if stored["cell_type"] != "code" {
                continue;
            }
            builder
                .push(
                    index,
                    UnitResult {
                        outputs: stored["outputs"].as_array().cloned().unwrap_or_default(),
                        execution_count: stored["execution_count"].as_u64(),
                        kernel: None,
                    },
                )
                .unwrap();
        }
        let mut record_cells = builder.finish(EngineEntryInput::default()).unwrap().cells;
        if self.marker {
            for cell in &mut record_cells {
                cell.metadata.extra.insert(MARKER.into(), json!({}));
            }
        }
        let mut fig = FsFigureSink::new(&ctx.source_path);
        let presented = present_document(
            &record_cells,
            input,
            &units,
            ctx.execute_scope.as_ref(),
            InlineTarget::from_format(&ctx.format),
            &ctx.format,
            &[],
            &mut fig,
        )
        .map_err(|e| ExecutionError::execution_failed("ipynb", e.to_string()))?;
        Ok(ExecuteResult::new(presented.markdown))
    }
}

fn ipynb_markdown(name: &str, marker: bool) -> (String, String) {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("notebook.ipynb");
    std::fs::copy(goldens_root().join(format!("ipynb-{name}.ipynb")), &input).unwrap();
    let markdown = run_with(Arc::new(MarkedIpynbReplay { marker }), &input);
    let golden = std::fs::read_to_string(goldens_root().join(format!("ipynb-{name}.md"))).unwrap();
    (golden, markdown)
}

/// The six `ipynb-*` goldens, which the S2 spike did not run, through the knitr entry path.
#[test]
fn ipynb_goldens_through_the_knitr_entry_path() {
    let mut differing = Vec::new();
    for name in IPYNB_CASES {
        let (golden, unmarked) = ipynb_markdown(name, false);
        assert_eq!(unmarked, golden, "{name}: control without the marker");
        let (_, marked) = ipynb_markdown(name, true);
        if marked != golden {
            differing.push(format!(
                "{name}\n--- golden\n{golden}\n--- marked\n{marked}"
            ));
        }
    }
    // OPR-1a: keep this equivalence (all six identical, as the shadow goldens are).
    assert!(differing.is_empty(), "{}", differing.join("\n\n"));
}

// ---- Knitr document scope (D3), R required ------------------------------------------------

fn knitr_available() -> bool {
    Command::new("Rscript")
        .args([
            "-e",
            "if (!requireNamespace(\"knitr\", quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

const KNITR_SCOPED: &str = "---\nengine: knitr\nexecute:\n  record: true\n  output: false\n---\n\n\
```{r}\ncat('VISIBLE-TEXT\\n')\n1 + 1\n```\n";

/// D3: R translates `execute: {output: false}` into `results: hide` and `fig.show: hide` in
/// the record's resolved options, keeps the outputs, and the rendered HTML shows none.
#[test]
fn knitr_bakes_the_document_output_scope_into_the_record() {
    if !knitr_available() {
        eprintln!("SKIP: knitr (Rscript + knitr package) not available");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("doc.qmd");
    std::fs::write(&input, KNITR_SCOPED).unwrap();
    let observer = AuxRecorder::arc(true);
    run_engine_stage(&input, observer.clone(), None).expect("the engine stage succeeds");
    let record = observer.record();
    assert_eq!(record.cells.len(), 1, "{:#?}", record.cells);
    let cell = &record.cells[0];
    let resolved = &cell.metadata.extra[MARKER];
    assert_eq!(resolved["results"], "hide", "{resolved}");
    assert_eq!(resolved["fig.show"], "hide", "{resolved}");
    // The cell keeps its outputs; hiding them is the presenter's job.
    let kept = serde_json::to_string(&cell.outputs).unwrap();
    assert!(kept.contains("VISIBLE-TEXT"), "{kept}");

    // And the render path shows none of it.
    let html = render_html(&input);
    assert!(!html.contains("cell-output"), "{html}");
    // The echoed code still mentions VISIBLE-TEXT; the output `[1] 2` is what is hidden.
    assert!(!html.contains("[1] 2"), "{html}");
}

fn render_html(input: &Path) -> String {
    use quarto_core::ProjectContext;
    use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
    use quarto_system_runtime::{NativeRuntime, SystemRuntime};
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(input, runtime.as_ref()).expect("project discovery");
    let result = render_document_to_file(
        input,
        "html",
        &RenderToFileOptions::default(),
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    )
    .unwrap_or_else(|e| panic!("render must succeed: {e}"));
    std::fs::read_to_string(&result.output_path).expect("read rendered HTML")
}
