/*
 * tests/integration/jupyter_markdown_goldens.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-6 Phase 0: the Jupyter markdown goldens, frozen from the pre-flip live path.
 */

//! Goldens of the markdown the Jupyter engine and the stored-notebook replay produce,
//! captured from the **pre-flip** splice loops (EXR-6 Phase 0) and kept as the oracle
//! once those loops are gone. See `tests/fixtures/jupyter-markdown-goldens/README.md`.
//!
//! Two fixture families, one flat directory:
//!
//! - `<case>.md` for every EXR-5 shadow fixture (`tests/fixtures/jupyter-record-shadow/`),
//!   rendered against a real Python kernel through the engine stage; beside it,
//!   `<case>.record.json` is the execution record the same run built, so a kernel-free
//!   test can present the record and compare it with the golden markdown.
//! - `ipynb-<case>.ipynb` plus `ipynb-<case>.md`: the `ipynb_stored_replay.rs` notebooks,
//!   replayed through the engine stage with no kernel.
//!
//! The wrapper-shape tests at the end pin the four places the flip is most likely to
//! drift at: the `::: {.cell}` wrapper of a visible cell, the empty string for a hidden
//! cell, the blank line a cell gets after a paragraph, and figure names keyed to the
//! executed-cell position with hidden cells in between.
//!
//! Regenerating is documented in the fixtures README and is never done in CI:
//! `JUPYTER_MARKDOWN_GOLDENS_BLESS=1`.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use quarto_core::engine::jupyter::list_kernelspecs;
use quarto_core::engine::preview_record::record_capture_run;
use quarto_core::engine::units::{Unit, scan_units, units_for_engine};
use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError, LanguageClaim,
};
use quarto_core::presenter::cell::render_cell;
use quarto_core::presenter::document::present_document;
use quarto_core::presenter::figures::{FigureSink, FsFigureSink};
use quarto_core::presenter::inline::InlineTarget;
use quarto_core::presenter::output::{CellOutput, CellResult, ExecuteStatus};
use quarto_core::presenter::visibility::CellVisibility;
use quarto_core::project::ProjectContext;
use quarto_core::record::{
    AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, fill_ids,
    read_record, write_record,
};
use quarto_core::stage::stages::ENGINE_CAPTURE_KIND;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
use quarto_source_map::SourceInfo;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

use crate::engine_stage_driver::{AuxRecorder, run_engine_stage};

/// The EXR-5 shadow fixtures (`tests/fixtures/jupyter-record-shadow/<case>.qmd`).
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

/// The `ipynb_stored_replay.rs` notebooks that replay (the execute-demanded one is
/// routed to a live engine and has no replay markdown).
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

fn bless() -> bool {
    std::env::var_os("JUPYTER_MARKDOWN_GOLDENS_BLESS").is_some()
}

fn python_kernel_available() -> bool {
    EngineRegistry::default()
        .get("jupyter")
        .is_some_and(|e| e.is_available())
        && tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(list_kernelspecs())
            .iter()
            .any(|s| s.language.eq_ignore_ascii_case("python"))
}

/// The one engine capture the stage emitted for `input`, with the record when
/// `want_record` (the stored replay builds none).
fn capture(input: &Path, want_record: bool) -> (Value, Option<String>) {
    let observer = AuxRecorder::arc(want_record);
    run_engine_stage(input, observer.clone(), None).expect("the engine stage runs");
    let captures = observer.events_of(ENGINE_CAPTURE_KIND);
    assert_eq!(captures.len(), 1, "one engine capture: {captures:?}");
    let record =
        want_record.then(|| write_record(&observer.record()).expect("the record serializes"));
    (captures.into_iter().next().unwrap().1, record)
}

/// Compare `actual` with the golden at `path`, or write it when blessing.
fn check(path: &Path, actual: &str, problems: &mut Vec<String>) {
    if bless() {
        std::fs::write(path, actual).unwrap();
        return;
    }
    match std::fs::read_to_string(path) {
        Ok(expected) if expected == actual => {}
        Ok(expected) => problems.push(format!(
            "{}: differs\n--- expected\n{expected}\n--- actual\n{actual}",
            path.display()
        )),
        Err(e) => problems.push(format!("{}: unreadable ({e})", path.display())),
    }
}

/// The capture's markdown with the one value in it that is not a function of the
/// fixture: the `date-modified` the pipeline's listing metadata takes from the copied
/// file's mtime (today's date) becomes `<date>`.
fn markdown_of(capture: &Value) -> String {
    let markdown = capture["result"]["markdown"]
        .as_str()
        .expect("the capture carries markdown");
    regex::Regex::new(r"date-modified: \d{4}-\d{2}-\d{2}")
        .unwrap()
        .replace_all(markdown, "date-modified: <date>")
        .into_owned()
}

/// The shadow fixtures against the real kernel: live markdown and the record.
fn check_shadow_cases(problems: &mut Vec<String>) {
    for name in SHADOW_CASES {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("doc.qmd");
        std::fs::copy(shadow_fixture(name), &input).unwrap();
        let (capture, record) = capture(&input, true);
        assert_eq!(capture["engine_name"], "jupyter", "{name}");
        check(
            &goldens_root().join(format!("{name}.md")),
            &markdown_of(&capture),
            problems,
        );
        check(
            &goldens_root().join(format!("{name}.record.json")),
            &record.unwrap(),
            problems,
        );
    }
}

/// The stored notebooks through the engine stage's replay route, no kernel.
fn check_ipynb_cases(problems: &mut Vec<String>) {
    for name in IPYNB_CASES {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("notebook.ipynb");
        std::fs::copy(goldens_root().join(format!("ipynb-{name}.ipynb")), &input).unwrap();
        let (capture, _) = capture(&input, false);
        assert_eq!(capture["engine_name"], "ipynb", "{name}");
        check(
            &goldens_root().join(format!("ipynb-{name}.md")),
            &markdown_of(&capture),
            problems,
        );
    }
}

/// Every golden equals what the engine stage produces today. The shadow half needs a
/// Python kernel and is skipped (with a message) without one; the notebook half always
/// runs.
#[test]
fn jupyter_markdown_matches_goldens() {
    let mut problems = Vec::new();
    check_ipynb_cases(&mut problems);
    if python_kernel_available() {
        check_shadow_cases(&mut problems);
    } else {
        eprintln!("SKIP: no jupyter runtime or Python kernelspec — shadow goldens not checked");
    }
    assert!(
        problems.is_empty(),
        "{} golden mismatch(es):\n{}",
        problems.len(),
        problems.join("\n\n")
    );
}

/// Every file in the goldens directory is one a test above reads, and every file a test
/// reads is there: no stray golden survives a renamed fixture.
#[test]
fn every_golden_file_is_consumed() {
    let mut expected: BTreeSet<String> = BTreeSet::new();
    expected.insert("README.md".to_string());
    for name in SHADOW_CASES {
        expected.insert(format!("{name}.md"));
        expected.insert(format!("{name}.preview.md"));
        expected.insert(format!("{name}.record.json"));
    }
    for name in IPYNB_CASES {
        expected.insert(format!("ipynb-{name}.ipynb"));
        expected.insert(format!("ipynb-{name}.md"));
    }
    let present: BTreeSet<String> = std::fs::read_dir(goldens_root())
        .expect("goldens dir")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let stray: Vec<_> = present.difference(&expected).collect();
    let missing: Vec<_> = expected.difference(&present).collect();
    assert!(
        stray.is_empty() && missing.is_empty(),
        "stray goldens: {stray:?}; missing goldens: {missing:?}"
    );
    for name in SHADOW_CASES {
        assert!(
            shadow_fixture(name).is_file(),
            "shadow fixture {name} went missing"
        );
    }
}

/// A stand-in for the Jupyter engine that presents the stored record of a golden
/// case over whatever input the stage hands it: the stage's units (ids filled), its
/// merged `execute:` scope and the target format, through `present_document`. Named
/// `jupyter` so the fixtures' `jupyter: python3` resolves to it; owns `python`.
struct StoredRecordJupyter {
    record_json: String,
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
        let record = read_record(&self.record_json).expect("the stored record reads");
        let units = units_for_engine(&ctx.units, input);
        let mut fig = FsFigureSink::new(&ctx.source_path);
        let presented = present_document(
            &record.cells,
            input,
            &units,
            ctx.execute_scope.as_ref(),
            InlineTarget::from_format(&ctx.format),
            &ctx.format,
            None,
            &mut fig,
        )
        .map_err(|e| ExecutionError::execution_failed("jupyter", e.to_string()))?;
        let mut result = ExecuteResult::new(presented.markdown);
        if !presented.files_written.is_empty() {
            result = result.with_supporting_files(presented.files_written);
        }
        Ok(result)
    }
}

/// Kernel-free, every run: presenting each case's stored record through the engine
/// stage reproduces its golden byte for byte, figure files included. This is the
/// oracle the flip rests on when no kernel is present.
#[test]
fn stored_records_present_to_the_goldens() {
    let mut problems = Vec::new();
    for name in SHADOW_CASES {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("doc.qmd");
        std::fs::copy(shadow_fixture(name), &input).unwrap();
        let record_json =
            std::fs::read_to_string(goldens_root().join(format!("{name}.record.json"))).unwrap();
        let mut registry = EngineRegistry::new();
        registry.register(std::sync::Arc::new(StoredRecordJupyter { record_json }));
        let observer = AuxRecorder::arc(false);
        run_engine_stage(
            &input,
            observer.clone(),
            Some(std::sync::Arc::new(registry)),
        )
        .expect("the engine stage runs");
        let captures = observer.events_of(ENGINE_CAPTURE_KIND);
        assert_eq!(captures.len(), 1, "{name}: one capture: {captures:?}");
        let golden = std::fs::read_to_string(goldens_root().join(format!("{name}.md"))).unwrap();
        let actual = markdown_of(&captures[0].1);
        if actual != golden {
            problems.push(format!(
                "{name}: differs\n--- golden\n{golden}\n--- presented\n{actual}"
            ));
        }
        if *name == "png-plus-text" {
            assert!(
                dir.path()
                    .join("doc_files/figure-html/cell-1-output-1.png")
                    .is_file(),
                "the figure is written beside the document"
            );
        }
        if *name == "hidden-then-plot" {
            assert!(
                dir.path()
                    .join("doc_files/figure-html/cell-2-output-1.png")
                    .is_file(),
                "the hidden first cell keeps its number"
            );
        }
    }
    assert!(
        problems.is_empty(),
        "{} golden mismatch(es):\n{}",
        problems.len(),
        problems.join("\n\n")
    );
}

/// The preview capture (`record_capture_run`, the legacy `EngineCapture` payload the
/// preview splices) carries the same markdown after the flip as before it. Its input
/// differs from the stage's (`format: q2-preview` in the front matter, a trailing
/// newline), so it has its own goldens, `<case>.preview.md`, blessed from the
/// pre-flip tree. Needs the kernel.
#[test]
fn legacy_capture_payload_is_byte_identical_after_flip() {
    if !python_kernel_available() {
        eprintln!("SKIP: no jupyter runtime or Python kernelspec");
        return;
    }
    let runtime: std::sync::Arc<dyn SystemRuntime> = std::sync::Arc::new(NativeRuntime::new());
    let mut problems = Vec::new();
    for name in SHADOW_CASES {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("doc.qmd");
        std::fs::copy(shadow_fixture(name), &input).unwrap();
        let project = ProjectContext::discover(&input, runtime.as_ref()).expect("project");
        let run = pollster::block_on(record_capture_run(&input, &project, runtime.clone(), None))
            .expect("the capture run succeeds");
        assert_eq!(run.captures.len(), 1, "{name}: one capture");
        let capture = serde_json::to_value(&run.captures[0]).unwrap();
        check(
            &goldens_root().join(format!("{name}.preview.md")),
            &markdown_of(&capture),
            &mut problems,
        );
    }
    assert!(
        problems.is_empty(),
        "{} golden mismatch(es):\n{}",
        problems.len(),
        problems.join("\n\n")
    );
}

// ── Wrapper-shape tests ─────────────────────────────────────────────────────
//
// Written before the flip against `render_cell` and `splice_mixed` (the pre-flip live
// loop's composition) and re-pointed at `present_document` with the flip, with the
// expectations unchanged, so the shape is pinned across it.

fn stdout(text: &str) -> CellResult {
    CellResult {
        status: ExecuteStatus::Ok,
        outputs: vec![CellOutput::Stream {
            name: "stdout".to_string(),
            text: text.to_string(),
        }],
        execution_count: Some(1),
    }
}

/// A one-pixel PNG as a `display_data` output.
fn png() -> CellResult {
    let mut data = std::collections::HashMap::new();
    data.insert(
        "image/png".to_string(),
        Value::String(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
                .to_string(),
        ),
    );
    data.insert(
        "text/plain".to_string(),
        Value::String("<Figure>".to_string()),
    );
    CellResult {
        status: ExecuteStatus::Ok,
        outputs: vec![CellOutput::DisplayData {
            data,
            metadata: Value::Object(Default::default()),
        }],
        execution_count: Some(1),
    }
}

/// A document `execute:` scope of one boolean.
fn scope(key: &str, value: bool) -> ConfigValue {
    ConfigValue::new_map(
        vec![ConfigMapEntry {
            key: key.to_string(),
            key_source: SourceInfo::for_test(),
            value: ConfigValue::new_bool(value, SourceInfo::for_test()),
        }],
        SourceInfo::for_test(),
    )
}

fn visible() -> CellVisibility {
    CellVisibility::resolve(None)
}

fn hidden() -> CellVisibility {
    CellVisibility::resolve(Some(&include_false()))
}

fn include_false() -> ConfigValue {
    scope("include", false)
}

/// `present_document` over the one cell in `input` with `result` as its record cell
/// (the record built by the real `RecordBuilder`), so the shape tests pin the driver.
fn splice_one(
    input: &str,
    result: &CellResult,
    scope: Option<&ConfigValue>,
    fig: &mut dyn FigureSink,
) -> String {
    let mut units = scan_units(input);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    assert!(
        matches!(units.as_slice(), [Unit::Cell(_)]),
        "one cell: {units:?}"
    );
    let def = HashDefinition::conservative("test");
    let mut builder = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
    builder
        .push(
            0,
            UnitResult {
                outputs: result.nbformat_outputs(),
                execution_count: None,
                kernel: None,
            },
        )
        .unwrap();
    let cells = builder.finish(EngineEntryInput::default()).unwrap().cells;
    present_document(
        &cells,
        input,
        &units,
        scope,
        InlineTarget::from_format("html"),
        "html",
        None,
        fig,
    )
    .unwrap()
    .markdown
}

#[test]
fn cell_wrapper_shape_unchanged_for_visible_cell() {
    let tmp = TempDir::new().unwrap();
    let mut fig = FsFigureSink::new(&tmp.path().join("doc.qmd"));
    fig.begin_cell();
    let md = render_cell(
        "python",
        "print('hi')\n",
        &stdout("hi\n"),
        &mut fig,
        visible(),
    );
    assert_eq!(
        md,
        "::: {.cell}\n\n```{.python .cell-code}\nprint('hi')\n```\n\n\
         ::: {.cell-output .cell-output-stdout}\n\n```\nhi\n```\n\n:::\n\n:::\n"
    );
}

#[test]
fn hidden_cell_emits_empty_string() {
    let tmp = TempDir::new().unwrap();
    let mut fig = FsFigureSink::new(&tmp.path().join("doc.qmd"));
    fig.begin_cell();
    let md = render_cell(
        "python",
        "print('hi')\n",
        &stdout("hi\n"),
        &mut fig,
        hidden(),
    );
    assert_eq!(md, "");

    // And in a document, the cell's bytes vanish and the prose keeps its newline.
    let input = "Before.\n\n```{python}\nprint('hi')\n```\n\nAfter.\n";
    let out = splice_one(input, &stdout("hi\n"), Some(&include_false()), &mut fig);
    assert_eq!(out, "Before.\n\n\nAfter.\n");
}

#[test]
fn blank_line_before_cell_div_after_paragraph() {
    let tmp = TempDir::new().unwrap();
    let mut fig = FsFigureSink::new(&tmp.path().join("doc.qmd"));
    // No blank line in the source: the div cannot interrupt the paragraph, so one is
    // added.
    let out = splice_one(
        "Para.\n```{python}\nprint('hi')\n```\n",
        &stdout("hi\n"),
        None,
        &mut fig,
    );
    assert!(
        out.starts_with("Para.\n\n::: {.cell}\n"),
        "a blank line separates the paragraph from the cell div:\n{out}"
    );
    // A blank line already there is not doubled.
    let out = splice_one(
        "Para.\n\n```{python}\nprint('hi')\n```\n",
        &stdout("hi\n"),
        None,
        &mut fig,
    );
    assert!(out.starts_with("Para.\n\n::: {.cell}\n"), "{out}");
    assert!(!out.contains("Para.\n\n\n"), "{out}");
}

#[test]
fn figure_names_keyed_to_executed_cell_position_with_hidden_cells() {
    let tmp = TempDir::new().unwrap();
    let doc = tmp.path().join("doc.qmd");
    let mut fig = FsFigureSink::new(&doc);
    // Cell 1 is hidden: executed (so `begin_cell` runs), nothing emitted, no file.
    fig.begin_cell();
    let first = render_cell("python", "plot()\n", &png(), &mut fig, hidden());
    assert_eq!(first, "");
    // Cell 2 is visible: its figure is cell 2's, not cell 1's.
    fig.begin_cell();
    let second = render_cell("python", "plot()\n", &png(), &mut fig, visible());
    assert!(
        second.contains("![](doc_files/figure-html/cell-2-output-1.png)"),
        "{second}"
    );
    let files_dir = tmp.path().join("doc_files/figure-html");
    assert!(files_dir.join("cell-2-output-1.png").is_file());
    assert!(!files_dir.join("cell-1-output-1.png").exists());
    assert!(fig.wrote_any());
}
