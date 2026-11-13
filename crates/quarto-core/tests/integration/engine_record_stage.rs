/*
 * tests/integration/engine_record_stage.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-5 Phases 0c, 0d and 0e: the engine stage's record plumbing, driven with stub
 * engines that build a record from the stage's units exactly as the Jupyter engine
 * does (EXR-4's `RecordBuilder` over `ExecutionContext.units` and `author_texts`).
 *
 * The plan names the echo and behave fixture engines for the side-data test; they
 * are TS engines that return no record until EXR-7 wires the record over the
 * protocol, so a stub engine stands in here and EXR-7 can swap it.
 */

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tempfile::TempDir;

use quarto_core::engine::preview_record::{record_capture, record_capture_run};
use quarto_core::engine::units::Unit;
use quarto_core::engine::{
    EngineRegistry, ExecuteResult, ExecutionContext, ExecutionEngine, ExecutionError, LanguageClaim,
};
use quarto_core::project::ProjectContext;
use quarto_core::record::{
    EngineEntryInput, Record, RecordBuilder, UnitResult, content_hash, read_record,
    read_record_value, write_record,
};
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_core::stage::JsonTraceObserver;
use quarto_core::stage::stages::{
    ENGINE_CAPTURE_KIND, ENGINE_CAPTURE_RECORD_KIND, ENGINE_RECORD_KIND,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use quarto_trace::RenderInfo;

use crate::engine_stage_driver::{AuxRecorder, run_engine_stage, run_engine_stage_presenting};

// ── stub engine ────────────────────────────────────────────────────────────

/// An engine that owns one language and, when asked, records every unit of that
/// language the stage listed, with one stdout line per cell naming the engine.
struct StubRecordEngine {
    name: &'static str,
    lang: &'static str,
    side_data: bool,
    builds: AtomicUsize,
    /// What the stage handed each run: the unit list and the author-text count.
    seen: Mutex<Vec<(Vec<Unit>, usize)>>,
}

impl StubRecordEngine {
    fn arc(name: &'static str, lang: &'static str, side_data: bool) -> Arc<Self> {
        Arc::new(Self {
            name,
            lang,
            side_data,
            builds: AtomicUsize::new(0),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn seen_units(&self) -> Vec<Unit> {
        self.seen.lock().unwrap()[0].0.clone()
    }
}

fn stub_error(e: impl std::fmt::Display) -> ExecutionError {
    ExecutionError::execution_failed("stub", e.to_string())
}

impl ExecutionEngine for StubRecordEngine {
    fn name(&self) -> &str {
        self.name
    }

    fn is_available(&self) -> bool {
        true
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
        self.seen
            .lock()
            .unwrap()
            .push((ctx.units.clone(), ctx.author_texts.len()));
        let mut result = ExecuteResult::new(input);
        if self.side_data {
            result
                .includes
                .header_includes
                .push(format!("<style>{}</style>", self.name));
            result.filters.push(format!("{}.lua", self.name));
            result.preserve.insert("ph".to_string(), "<b/>".to_string());
            result.needs_postprocess = true;
        }
        if ctx.builds_record() {
            self.builds.fetch_add(1, Ordering::SeqCst);
            let def = self.hash_definition();
            let mut builder = RecordBuilder::new(self.name, &def, &ctx.units, &ctx.author_texts)
                .map_err(stub_error)?;
            for (index, unit) in ctx.units.iter().enumerate() {
                if unit.lang() == self.lang {
                    builder
                        .push(
                            index,
                            UnitResult {
                                outputs: vec![json!({
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
            // Side data is the stage's to fold (EXR-5 Phase 2), so only the
            // document-hash input is filled, as the Jupyter engine fills it.
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

// ── fixtures ───────────────────────────────────────────────────────────────

fn write_doc(dir: &Path, content: &str) -> PathBuf {
    let input = dir.join("doc.qmd");
    std::fs::write(&input, content).unwrap();
    input
}

fn unit_id(unit: &Unit) -> Option<String> {
    match unit {
        Unit::Cell(c) => c.id.as_ref().map(|i| i.0.clone()),
        Unit::Inline(i) => i.id.as_ref().map(|i| i.0.clone()),
    }
}

/// Run `content` with a stub engine for `stub-a` and return the coalesced record.
fn record_for(content: &str) -> Record {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), content);
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    let observer = AuxRecorder::arc(true);
    run_engine_stage(&input, observer.clone(), Some(registry(&[engine]))).unwrap();
    observer.record()
}

const LABELED_CELL: &str = "#| label: fig-x\n#| fig-cap: A cap\nx = 1";

fn registry(engines: &[Arc<StubRecordEngine>]) -> Arc<EngineRegistry> {
    let mut registry = EngineRegistry::new();
    for engine in engines {
        registry.register(engine.clone() as Arc<dyn ExecutionEngine>);
    }
    Arc::new(registry)
}

const ONE_ENGINE: &str = "---\nengine: stub-a\n---\n\nText.\n\n```{stub-a}\nx = 1\n```\n\n\
                          More.\n\n```{stub-a}\n#| echo: false\ny = 2\n```\n";

const TWO_ENGINES: &str = "---\nengine: [stub-a, stub-b]\n---\n\n```{stub-a}\na1\n```\n\n\
                           ```{stub-b}\nb1\n```\n\n```{stub-a}\na2\n```\n";

// ── 0c: gating ─────────────────────────────────────────────────────────────

#[test]
fn no_record_is_built_when_observer_does_not_want_one() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    let observer = AuxRecorder::arc(false);

    run_engine_stage(&input, observer.clone(), Some(registry(&[engine.clone()]))).unwrap();

    assert_eq!(engine.builds.load(Ordering::SeqCst), 0, "no builder call");
    assert_eq!(observer.events_of(ENGINE_CAPTURE_KIND).len(), 1);
    assert!(observer.events_of(ENGINE_RECORD_KIND).is_empty());
    assert!(observer.events_of(ENGINE_CAPTURE_RECORD_KIND).is_empty());
}

#[test]
fn legacy_capture_payload_is_byte_identical_with_and_without_record() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);

    let capture_result = |wants: bool| -> String {
        let engine = StubRecordEngine::arc("stub-a", "stub-a", true);
        let observer = AuxRecorder::arc(wants);
        run_engine_stage(&input, observer.clone(), Some(registry(&[engine.clone()]))).unwrap();
        assert_eq!(engine.builds.load(Ordering::SeqCst), usize::from(wants));
        let captures = observer.events_of(ENGINE_CAPTURE_KIND);
        assert_eq!(captures.len(), 1);
        serde_json::to_string(&captures[0].1).unwrap()
    };

    let without = capture_result(false);
    let with = capture_result(true);
    assert_eq!(without, with);
    assert!(!with.contains("\"record\""), "{with}");
}

// ── 0d: trace ──────────────────────────────────────────────────────────────

#[test]
fn trace_engine_capture_carries_per_engine_record_when_tracing() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let trace_path = tmp.path().join("trace.json");
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    let observer = Arc::new(JsonTraceObserver::new(
        trace_path.clone(),
        RenderInfo::default(),
    ));

    run_engine_stage(&input, observer, Some(registry(&[engine.clone()]))).unwrap();

    assert_eq!(
        engine.builds.load(Ordering::SeqCst),
        1,
        "the trace wants a record"
    );
    let trace = quarto_trace::read::read_trace(&trace_path).expect("the trace was written");
    assert_eq!(trace.engine_captures.len(), 1);
    let capture = &trace.engine_captures[0];
    assert_eq!(capture.engine_name, "stub-a");
    let record = read_record_value(capture.record.clone().expect("the per-engine record"))
        .expect("the record field is a record notebook");
    assert_eq!(record.metadata.quarto_engines.len(), 1);
    assert_eq!(record.metadata.quarto_engines[0].name, "stub-a");
    assert_eq!(record.cells.len(), 2);
    // The legacy capture itself did not grow.
    assert!(capture.result.get("record").is_none());
}

#[test]
fn trace_pairs_each_engines_record_with_its_own_capture() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), TWO_ENGINES);
    let trace_path = tmp.path().join("trace.json");
    let a = StubRecordEngine::arc("stub-a", "stub-a", false);
    let b = StubRecordEngine::arc("stub-b", "stub-b", false);
    let observer = Arc::new(JsonTraceObserver::new(
        trace_path.clone(),
        RenderInfo::default(),
    ));

    run_engine_stage(&input, observer, Some(registry(&[a, b]))).unwrap();

    let trace = quarto_trace::read::read_trace(&trace_path).expect("the trace was written");
    assert_eq!(trace.engine_captures.len(), 2);
    for capture in &trace.engine_captures {
        let record = read_record_value(capture.record.clone().expect("per-engine record"))
            .expect("record notebook");
        assert_eq!(record.metadata.quarto_engines[0].name, capture.engine_name);
        for cell in &record.cells {
            assert_eq!(
                cell.metadata.quarto_engine.as_deref(),
                Some(capture.engine_name.as_str())
            );
        }
    }
    assert_eq!(trace.engine_captures[0].engine_name, "stub-a");
    assert_eq!(trace.engine_captures[1].engine_name, "stub-b");
}

// ── 0e: side data and coalescing ───────────────────────────────────────────

#[test]
fn engine_stage_folds_result_side_data_into_notebook_metadata() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let engine = StubRecordEngine::arc("stub-a", "stub-a", true);
    let observer = AuxRecorder::arc(true);

    run_engine_stage(&input, observer.clone(), Some(registry(&[engine]))).unwrap();

    let record = observer.record();
    assert_eq!(record.metadata.quarto_engines.len(), 1);
    let entry = &record.metadata.quarto_engines[0];
    assert_eq!(entry.name, "stub-a");
    assert_eq!(
        entry.includes,
        Some(json!({
            "header_includes": ["<style>stub-a</style>"],
            "include_before": [],
            "include_after": [],
        }))
    );
    assert_eq!(entry.filters, ["stub-a.lua"]);
    assert_eq!(entry.preserve.get("ph").map(String::as_str), Some("<b/>"));
    assert!(entry.needs_postprocess);
}

#[test]
fn two_engines_coalesce_in_source_order_with_quarto_engine() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), TWO_ENGINES);
    let a = StubRecordEngine::arc("stub-a", "stub-a", false);
    let b = StubRecordEngine::arc("stub-b", "stub-b", false);
    let observer = AuxRecorder::arc(true);

    run_engine_stage(&input, observer.clone(), Some(registry(&[a, b]))).unwrap();

    let record = observer.record();
    let engines: Vec<&str> = record
        .metadata
        .quarto_engines
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(engines, ["stub-a", "stub-b"]);
    let cell_engines: Vec<&str> = record
        .cells
        .iter()
        .map(|c| c.metadata.quarto_engine.as_deref().unwrap())
        .collect();
    assert_eq!(cell_engines, ["stub-a", "stub-b", "stub-a"]);
    let indices: Vec<&str> = record
        .cells
        .iter()
        .map(|c| c.metadata.quarto_cell_id.split('-').next().unwrap())
        .collect();
    assert_eq!(indices, ["0", "1", "2"]);
    let sources: Vec<&str> = record.cells.iter().map(|c| c.source[0].as_str()).collect();
    assert_eq!(sources, ["a1", "b1", "a2"]);
}

#[test]
fn coalescing_is_deterministic() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), TWO_ENGINES);

    let bytes = || -> String {
        let a = StubRecordEngine::arc("stub-a", "stub-a", true);
        let b = StubRecordEngine::arc("stub-b", "stub-b", true);
        let observer = AuxRecorder::arc(true);
        run_engine_stage(&input, observer.clone(), Some(registry(&[a, b]))).unwrap();
        write_record(&observer.record()).unwrap()
    };

    let first = bytes();
    let second = bytes();
    assert_eq!(first, second);
    assert!(first.contains("\"quarto_engines\""), "{first}");
}

// ── Phase 2: gating by the document ────────────────────────────────────────

#[test]
fn execute_record_true_builds_a_record_for_a_plain_observer() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(
        tmp.path(),
        "---\nengine: stub-a\nexecute:\n  record: true\n---\n\n```{stub-a}\nx = 1\n```\n",
    );
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    let observer = AuxRecorder::arc(false);

    run_engine_stage(&input, observer.clone(), Some(registry(&[engine.clone()]))).unwrap();

    assert_eq!(engine.builds.load(Ordering::SeqCst), 1);
    let record = observer.record();
    assert_eq!(record.cells.len(), 1);
    assert_eq!(record.metadata.quarto_source.as_deref(), Some("doc.qmd"));
}

// ── OPR-7a: present_from_record builds the record without the extra cost ──────

/// Every file under `dir/.quarto`, so a test can say nothing was written.
fn files_under_dot_quarto(dir: &Path) -> Vec<PathBuf> {
    fn walk(p: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&dir.join(".quarto"), &mut out);
    out
}

#[test]
fn present_from_record_builds_the_record_but_serializes_and_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    let observer = AuxRecorder::arc(false);

    run_engine_stage_presenting(&input, observer.clone(), Some(registry(&[engine.clone()])))
        .unwrap();

    assert_eq!(
        engine.builds.load(Ordering::SeqCst),
        1,
        "the engine built a record"
    );
    assert!(
        observer.events_of(ENGINE_RECORD_KIND).is_empty(),
        "no full-record JSON copy goes to an observer that did not ask"
    );
    assert!(files_under_dot_quarto(tmp.path()).is_empty());
}

#[test]
fn present_from_record_with_an_observer_still_emits_the_record() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    let observer = AuxRecorder::arc(true);

    run_engine_stage_presenting(&input, observer.clone(), Some(registry(&[engine]))).unwrap();

    assert_eq!(observer.record().cells.len(), 2);
}

#[test]
fn record_true_still_writes_the_file_and_the_observer_still_receives_the_record() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(
        tmp.path(),
        "---\nengine: stub-a\nexecute:\n  record: true\n---\n\n```{stub-a}\nx = 1\n```\n",
    );
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    let observer = AuxRecorder::arc(true);

    run_engine_stage(&input, observer.clone(), Some(registry(&[engine]))).unwrap();

    assert_eq!(observer.record().cells.len(), 1);
    let files = files_under_dot_quarto(tmp.path());
    assert!(
        files
            .iter()
            .any(|f| f.extension().is_some_and(|e| e == "ipynb")),
        "{files:?}"
    );
}

#[test]
fn a_plain_run_writes_no_record_file() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    run_engine_stage(&input, AuxRecorder::arc(false), Some(registry(&[engine]))).unwrap();
    assert!(files_under_dot_quarto(tmp.path()).is_empty());
}

// ── Phase 2: unit ids and author texts in the stage ────────────────────────

#[test]
fn stage_fills_unit_ids_before_any_engine_runs() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    // No record wanted: the ids are filled all the same, since the wire carries them.
    let observer = AuxRecorder::arc(false);

    run_engine_stage(&input, observer, Some(registry(&[engine.clone()]))).unwrap();

    let units = engine.seen_units();
    assert_eq!(units.len(), 2);
    let ids: Vec<String> = units
        .iter()
        .map(|u| unit_id(u).expect("id filled"))
        .collect();
    assert!(ids[0].starts_with("0-"), "{ids:?}");
    assert!(ids[1].starts_with("1-"), "{ids:?}");
    assert!(ids[0].ends_with("-unlabeled"), "{ids:?}");
}

#[test]
fn author_texts_has_one_entry_per_unit() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(
        tmp.path(),
        "---\nengine: stub-a\n---\n\n```{stub-a}\na\n```\n\nSee `{stub-a} 1`.\n\n```{stub-a}\nb\n```\n",
    );
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    let observer = AuxRecorder::arc(true);

    run_engine_stage(&input, observer, Some(registry(&[engine.clone()]))).unwrap();

    let seen = engine.seen.lock().unwrap();
    let (units, author_texts) = &seen[0];
    assert_eq!(units.len(), 3);
    assert_eq!(*author_texts, 3);
}

/// Crossref sugaring strips the label and caption lines from the engine's text;
/// the id and the content hash come from the author's text (epic decision 36).
#[test]
fn labeled_cell_id_uses_author_text() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(
        tmp.path(),
        &format!("---\nengine: stub-a\n---\n\n```{{stub-a}}\n{LABELED_CELL}\n```\n"),
    );
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
    let observer = AuxRecorder::arc(true);

    run_engine_stage(&input, observer.clone(), Some(registry(&[engine.clone()]))).unwrap();

    // The test is meaningful only if sugaring stripped the lines the engine saw.
    let units = engine.seen_units();
    let Unit::Cell(cell) = &units[0] else {
        panic!("a cell")
    };
    assert!(!cell.code.contains("fig-cap"), "{}", cell.code);

    let hash = content_hash(LABELED_CELL);
    assert_eq!(
        unit_id(&units[0]).unwrap(),
        format!("0-{}-fig-x", &hash[..8])
    );
    let record = observer.record();
    let recorded = &record.cells[0];
    assert_eq!(recorded.metadata.quarto_content_hash, hash);
    assert_eq!(recorded.metadata.quarto_label.as_deref(), Some("fig-x"));
    assert_eq!(
        recorded.source,
        ["#| label: fig-x\n", "#| fig-cap: A cap\n", "x = 1"]
    );
}

#[test]
fn sugared_cell_inside_a_container_keeps_its_author_text() {
    let record = record_for(&format!(
        "---\nengine: stub-a\n---\n\n```{{stub-a}}\na\n```\n\n::: {{.panel}}\n```{{stub-a}}\n{LABELED_CELL}\n```\n:::\n\n```{{stub-a}}\nc\n```\n"
    ));
    let sources: Vec<String> = record.cells.iter().map(|c| c.source.concat()).collect();
    assert_eq!(sources, ["a", LABELED_CELL, "c"]);
    assert_eq!(
        record.cells[1].metadata.quarto_label.as_deref(),
        Some("fig-x")
    );
}

/// The stage scans once; a second engine in the sequence sees the same snapshot.
#[test]
fn second_engine_reuses_the_one_snapshot() {
    let tmp = TempDir::new().unwrap();
    let labeled_b = LABELED_CELL.replace("x = 1", "b1");
    let input = write_doc(
        tmp.path(),
        &format!(
            "---\nengine: [stub-a, stub-b]\n---\n\n```{{stub-a}}\na1\n```\n\n```{{stub-b}}\n{labeled_b}\n```\n"
        ),
    );
    let a = StubRecordEngine::arc("stub-a", "stub-a", false);
    let b = StubRecordEngine::arc("stub-b", "stub-b", false);
    let observer = AuxRecorder::arc(true);

    run_engine_stage(
        &input,
        observer.clone(),
        Some(registry(&[a.clone(), b.clone()])),
    )
    .unwrap();

    assert_eq!(a.seen_units().len(), 2);
    assert_eq!(b.seen_units().len(), 2);
    assert_eq!(unit_id(&a.seen_units()[1]), unit_id(&b.seen_units()[1]));
    let record = observer.record();
    assert_eq!(
        record.cells[1].metadata.quarto_engine.as_deref(),
        Some("stub-b")
    );
    assert_eq!(
        record.cells[1].metadata.quarto_label.as_deref(),
        Some("fig-x")
    );
    assert_eq!(record.cells[1].source.concat(), labeled_b);
}

// ── Phase 2: source of the hash inputs ─────────────────────────────────────

/// The hashes are built from the stage's pre-engine units and merged metadata: a
/// code edit inside an included file changes them, a prose-only edit does not.
#[test]
fn code_edit_in_an_included_file_changes_the_hashes_and_prose_does_not() {
    let tmp = TempDir::new().unwrap();
    let part = tmp.path().join("_part.qmd");
    let doc = "---\nengine: stub-a\n---\n\nIntro.\n\n{{< include _part.qmd >}}\n";
    std::fs::write(&part, "```{stub-a}\nx = 1\n```\n").unwrap();

    let hashes = |content: &str| {
        let input = write_doc(tmp.path(), content);
        let engine = StubRecordEngine::arc("stub-a", "stub-a", false);
        let observer = AuxRecorder::arc(true);
        run_engine_stage(&input, observer.clone(), Some(registry(&[engine]))).unwrap();
        let record = observer.record();
        assert_eq!(record.cells.len(), 1, "the included cell is a unit");
        (
            record.cells[0].metadata.quarto_exec_hash.clone(),
            record.metadata.quarto_execution_hash.clone().unwrap(),
        )
    };

    let first = hashes(doc);
    let prose_edit = hashes(&doc.replace("Intro.", "A different intro."));
    assert_eq!(first, prose_edit);

    std::fs::write(&part, "```{stub-a}\nx = 2\n```\n").unwrap();
    let code_edit = hashes(doc);
    assert_ne!(first.0, code_edit.0);
    assert_ne!(first.1, code_edit.1);
}

#[test]
fn equivalent_metadata_spellings_hash_alike() {
    let document_hash = |front_matter: &str| {
        let record = record_for(&format!(
            "---\nengine: stub-a\n{front_matter}---\n\n```{{stub-a}}\nx = 1\n```\n"
        ));
        record.metadata.quarto_engines[0].document_hash.clone()
    };
    let a = document_hash("jupyter: python3\nexecute:\n  echo: false\n  warning: true\n");
    let b = document_hash("execute: {warning: true, echo: false}\njupyter: \"python3\"\n");
    assert_eq!(a, b);
    let c = document_hash("jupyter: python3\nexecute:\n  echo: true\n  warning: true\n");
    assert_ne!(a, c);
}

// ── Phase 2: the record handoff to the capture writers ─────────────────────

fn capture_project(input: &Path) -> (ProjectContext, Arc<dyn SystemRuntime>) {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(input, runtime.as_ref()).expect("discover");
    (project, runtime)
}

#[test]
fn capture_run_carries_the_record() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let (project, runtime) = capture_project(&input);
    let engine = StubRecordEngine::arc("stub-a", "stub-a", false);

    let run = pollster::block_on(record_capture_run(
        &input,
        &project,
        runtime,
        Some(registry(&[engine])),
    ))
    .expect("capture run");

    assert_eq!(run.captures.len(), 1);
    let record = run.record.expect("the run carries the record");
    assert_eq!(record.cells.len(), 2);
    assert_eq!(record.metadata.quarto_engines[0].name, "stub-a");
}

#[test]
fn record_capture_wrapper_returns_the_same_captures() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let (project, runtime) = capture_project(&input);

    let run = pollster::block_on(record_capture_run(
        &input,
        &project,
        runtime.clone(),
        Some(registry(&[StubRecordEngine::arc("stub-a", "stub-a", true)])),
    ))
    .unwrap();
    let wrapped = pollster::block_on(record_capture(
        &input,
        &project,
        runtime,
        Some(registry(&[StubRecordEngine::arc("stub-a", "stub-a", true)])),
    ))
    .unwrap();

    assert_eq!(
        serde_json::to_string(&run.captures).unwrap(),
        serde_json::to_string(&wrapped).unwrap()
    );
}

#[test]
fn legacy_capture_bytes_unchanged_when_record_collected() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let (project, runtime) = capture_project(&input);

    // The collector always wants the record; the capture it stores must not
    // carry it (the legacy payload is what preview and hub documents hold).
    let run = pollster::block_on(record_capture_run(
        &input,
        &project,
        runtime,
        Some(registry(&[StubRecordEngine::arc("stub-a", "stub-a", true)])),
    ))
    .unwrap();
    assert!(run.record.is_some());
    let bytes = serde_json::to_string(&run.captures[0]).unwrap();
    assert!(!bytes.contains("\"record\""), "{bytes}");
    assert!(run.captures[0].record.is_none());
    assert!(run.captures[0].result.get("record").is_none());
}

// ── Phase 2: the record file for a plain render ────────────────────────────

const RECORD_TRUE: &str =
    "---\nengine: stub-a\nexecute:\n  record: true\n---\n\n```{stub-a}\nx = 1\n```\n";

fn record_file(dir: &Path) -> PathBuf {
    dir.join(".quarto")
        .join("execution-records")
        .join("doc.ipynb")
}

#[test]
fn execute_record_true_writes_the_record_file() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), RECORD_TRUE);
    let render = || {
        let engine = StubRecordEngine::arc("stub-a", "stub-a", true);
        run_engine_stage(&input, AuxRecorder::arc(false), Some(registry(&[engine]))).unwrap();
        std::fs::read_to_string(record_file(tmp.path())).expect("the record file is written")
    };

    let first = render();
    let record = read_record(&first).expect("the file is a record notebook");
    assert_eq!(record.cells.len(), 1);
    assert_eq!(record.metadata.quarto_source.as_deref(), Some("doc.qmd"));

    let second = render();
    assert_eq!(first, second, "two renders write identical bytes");
}

#[test]
fn plain_render_writes_no_record_file() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let engine = StubRecordEngine::arc("stub-a", "stub-a", true);

    // Even an observer that wants the record (preview, hub) does not write the
    // file: only the document's own `execute: {record: true}` does.
    run_engine_stage(&input, AuxRecorder::arc(true), Some(registry(&[engine]))).unwrap();

    assert!(!tmp.path().join(".quarto").exists());
}

/// Render `input` to HTML through the real entry point `q2 render` uses, with `engine` as
/// the only engine. Returns the result so a test can read the diagnostics.
fn render_through_render_document_to_file(
    input: &Path,
    engine: Arc<StubRecordEngine>,
) -> quarto_core::error::Result<quarto_core::render_to_file::RenderToFileResult> {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(input, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions {
        engine_registry_override: Some(registry(&[engine])),
        ..Default::default()
    };
    render_document_to_file(
        input,
        "html",
        &options,
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    )
}

/// RIU-1: a plain `q2 render` of a document with no `record: true` creates no records
/// directory, even though the writer gate now defaults on for the preview.
#[test]
fn plain_render_to_file_creates_no_execution_records_directory() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), ONE_ENGINE);
    let engine = StubRecordEngine::arc("stub-a", "stub-a", true);

    render_through_render_document_to_file(&input, engine).expect("the render succeeds");

    assert!(
        !tmp.path()
            .join(".quarto")
            .join("execution-records")
            .exists(),
        "a plain render must not create .quarto/execution-records"
    );
}

/// RIU-1 (epic decision 10): the copy under `.quarto/execution-records/` is optional, so a
/// location that cannot be written warns and the render still succeeds.
#[test]
fn an_unwritable_record_location_warns_and_the_render_succeeds() {
    let tmp = TempDir::new().unwrap();
    let input = write_doc(tmp.path(), RECORD_TRUE);
    // `.quarto/execution-records` is a regular file, which fails the write on every
    // platform with no permission bits. (A file at `.quarto` itself fails earlier, in
    // "Failed to create .quarto directory", with or without `record: true`.)
    std::fs::create_dir_all(tmp.path().join(".quarto")).unwrap();
    std::fs::write(
        tmp.path().join(".quarto").join("execution-records"),
        b"in the way",
    )
    .unwrap();
    let engine = StubRecordEngine::arc("stub-a", "stub-a", true);

    let result = render_through_render_document_to_file(&input, engine)
        .expect("a failed optional record copy must not fail the render");

    assert!(result.output_path.exists(), "the HTML is written");
    let warnings: Vec<String> = result
        .render_output
        .diagnostics
        .iter()
        .filter(|d| d.kind == quarto_error_reporting::DiagnosticKind::Warning)
        .map(|d| d.to_text(None))
        .collect();
    assert!(
        warnings.iter().any(|w| w.contains("execution record")
            && w.contains(
                &tmp.path()
                    .join(".quarto")
                    .join("execution-records")
                    .display()
                    .to_string()
            )),
        "a warning names the record location: {warnings:?}"
    );
}
