/*
 * tests/integration/jupyter_record_shadow.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-5 Phase 3: the shadow equivalence harness. For every fixture in
 * `tests/fixtures/jupyter-record-shadow/`, the Jupyter engine runs against a real
 * kernel with a record wanted, and `present(record)` (the EXR-1 presenter, driven by
 * the same units and the same `execute_scope` the engine used) must equal the live
 * markdown byte for byte, written figure file names and the `supporting_files`
 * directory included. A negative control perturbs one output and asserts the
 * comparator fails, so the harness cannot pass vacuously.
 *
 * The comparator is test-only: nothing in production presents from the record
 * until EXR-6 (the flip), which takes this harness as its baseline.
 */

#![cfg(not(target_arch = "wasm32"))]

use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use quarto_core::cell_options::partition_cell_options;
use quarto_core::engine::EngineRegistry;
use quarto_core::engine::jupyter::router::{SpliceKind, splice_mixed};
use quarto_core::engine::jupyter::{CellResult, MimeBundle, list_kernelspecs};
use quarto_core::engine::units::{Unit, scan_units};
use quarto_core::format::FormatIdentifier;
use quarto_core::presenter::cell::render_cell;
use quarto_core::presenter::figures::{FigureSink, FsFigureSink};
use quarto_core::presenter::inline::{InlineTarget, render_inline};
use quarto_core::presenter::visibility::{CellVisibility, resolve_cell_options};
use quarto_core::record::{Record, UnitKind, write_record};
use quarto_core::stage::stages::{ENGINE_CAPTURE_KIND, merge_execute_scope};
use quarto_source_map::{By, SourceInfo};

use crate::engine_stage_driver::{AuxRecorder, run_engine_stage};

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

/// Skip the test (return from it) when no Python kernel is installed.
macro_rules! require_kernel {
    ($name:literal) => {
        if !python_kernel_available() {
            eprintln!("SKIP: no jupyter runtime or Python kernelspec — {}", $name);
            return;
        }
    };
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jupyter-record-shadow")
        .join(format!("{name}.qmd"))
}

/// What one live run of a fixture produced: the engine's input, its markdown, the
/// supporting-files directory it reported, and the record built beside it.
struct LiveRun {
    /// Keeps the document and its `<stem>_files` alive.
    dir: TempDir,
    input_qmd: String,
    markdown: String,
    supporting_files: Vec<String>,
    record: Record,
}

/// Copy `fixture_path` into a temp dir and run the engine stage on it with a record
/// wanted, against the real kernel.
fn live_run(fixture_path: &Path) -> LiveRun {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("doc.qmd");
    std::fs::copy(fixture_path, &input).unwrap();
    let observer = AuxRecorder::arc(true);
    run_engine_stage(&input, observer.clone(), None).expect("the engine stage runs");

    let captures = observer.events_of(ENGINE_CAPTURE_KIND);
    assert_eq!(captures.len(), 1, "one jupyter capture: {captures:?}");
    let capture = &captures[0].1;
    assert_eq!(capture["engine_name"], "jupyter");
    let supporting_files = capture["result"]["supporting_files"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| p.as_str())
                .map(|p| {
                    Path::new(p)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    LiveRun {
        input_qmd: capture["input_qmd"].as_str().unwrap().to_string(),
        markdown: capture["result"]["markdown"].as_str().unwrap().to_string(),
        supporting_files,
        record: observer.record(),
        dir,
    }
}

/// The document-wide counter in a record cell's id (`{n}-{hash8}-{label}`).
fn id_index(cell: &quarto_core::record::RecordCell) -> Option<usize> {
    cell.metadata
        .quarto_cell_id
        .split('-')
        .next()
        .and_then(|n| n.parse().ok())
}

/// The record cell for `unit`, by kind and document-wide counter (the rule the engine
/// uses to map a run unit to its stage index).
fn cell_for<'a>(record: &'a Record, unit: &Unit) -> Option<&'a quarto_core::record::RecordCell> {
    record.cells.iter().find(|c| match unit {
        Unit::Cell(u) => c.metadata.quarto_kind == UnitKind::Cell && id_index(c) == Some(u.index),
        Unit::Inline(u) => {
            c.metadata.quarto_kind == UnitKind::Inline && id_index(c) == Some(u.span_index)
        }
    })
}

/// The document's `execute:` scope as the engine stage merges it for `html`.
fn execute_scope(input_qmd: &str) -> Option<quarto_pandoc_types::ConfigValue> {
    let (ast, _, _) = pampa::readers::qmd::read(
        input_qmd.as_bytes(),
        false,
        "doc.qmd",
        &mut std::io::sink(),
        false,
        None,
    )
    .expect("the engine input parses");
    merge_execute_scope(FormatIdentifier::Html, ast.meta.get("execute").cloned())
}

/// Present `record` over `input_qmd` the way the live loop presents kernel results:
/// the same units (relocated by counter), the same resolved visibility, the same
/// `render_cell` and `render_inline`, figures written beside `doc_path`.
fn present(record: &Record, input_qmd: &str, doc_path: &Path) -> (String, Option<String>) {
    let scope = execute_scope(input_qmd);
    let mut fig = FsFigureSink::new(doc_path);
    let mut replacements: Vec<(Range<usize>, String, SpliceKind)> = Vec::new();
    for unit in scan_units(input_qmd) {
        let Some(cell) = cell_for(record, &unit) else {
            continue; // ceded, unowned, or left literal: not executed
        };
        let result = CellResult::from_nbformat(&cell.outputs, cell.execution_count);
        match &unit {
            Unit::Cell(u) => {
                let parsed =
                    partition_cell_options(&u.lang, &u.code, SourceInfo::generated(By::unknown()))
                        .expect("cell options parse");
                let resolved = resolve_cell_options(scope.as_ref(), parsed.options);
                let visibility = CellVisibility::resolve(resolved.as_ref());
                fig.begin_cell();
                let text = render_cell(&u.lang, &parsed.code, &result, &mut fig, visibility);
                replacements.push((u.start..u.end, text, SpliceKind::Block));
            }
            Unit::Inline(u) => {
                assert!(!u.in_front_matter, "no fixture has a front matter span");
                let bundle: Option<MimeBundle> = cell.outputs.iter().find_map(|o| {
                    matches!(
                        o["output_type"].as_str(),
                        Some("execute_result" | "display_data")
                    )
                    .then(|| {
                        o["data"]
                            .as_object()
                            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                            .unwrap_or_default()
                    })
                });
                if let Some(text) = bundle
                    .and_then(|b| render_inline(&b, InlineTarget::from_format("html")))
                    .filter(|t| !t.is_empty())
                {
                    replacements.push((u.start..u.end, text, SpliceKind::Inline));
                }
            }
        }
    }
    let files_dir = fig.wrote_any().then(|| {
        fig.files_dir()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    (splice_mixed(input_qmd, &replacements), files_dir)
}

/// Every file under `dir`, relative, with its bytes, sorted.
fn files_under(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, std::fs::read(entry.path()).unwrap()));
        }
    }
    out.sort();
    out
}

/// Compare `present(record)` with the live markdown and figures. The mismatch is
/// returned as text, so the negative control can assert on it.
fn compare(live: &LiveRun, record: &Record) -> Result<(), String> {
    let presented_dir = TempDir::new().unwrap();
    let presented_doc = presented_dir.path().join("doc.qmd");
    let (presented, presented_files_dir) = present(record, &live.input_qmd, &presented_doc);
    if presented != live.markdown {
        return Err(format!(
            "markdown differs.\n--- live ---\n{}\n--- presented ---\n{presented}",
            live.markdown
        ));
    }
    let live_files_dir = live.supporting_files.first().cloned();
    if presented_files_dir != live_files_dir {
        return Err(format!(
            "supporting files differ: live {live_files_dir:?}, presented {presented_files_dir:?}"
        ));
    }
    if let Some(name) = live_files_dir {
        let live_files = files_under(&live.dir.path().join(&name));
        let presented_files = files_under(&presented_dir.path().join(&name));
        if live_files != presented_files {
            return Err(format!(
                "figure files differ: live {:?}, presented {:?}",
                live_files.iter().map(|(p, _)| p).collect::<Vec<_>>(),
                presented_files.iter().map(|(p, _)| p).collect::<Vec<_>>()
            ));
        }
    }
    Ok(())
}

fn shadow(name: &str) -> LiveRun {
    let live = live_run(&fixture(name));
    if let Err(mismatch) = compare(&live, &live.record) {
        panic!("present(record) != live for {name}:\n{mismatch}");
    }
    live
}

macro_rules! shadow_case {
    ($test:ident, $name:literal) => {
        #[test]
        fn $test() {
            require_kernel!($name);
            shadow($name);
        }
    };
}

shadow_case!(shadow_stdout, "stdout");
shadow_case!(shadow_plain_value, "plain-value");
shadow_case!(shadow_html, "html");
shadow_case!(shadow_error_allowed, "error-allowed");
shadow_case!(shadow_echo_false, "echo-false");
shadow_case!(shadow_output_false, "output-false");
shadow_case!(shadow_include_false, "include-false");
shadow_case!(shadow_warning_false_stderr, "warning-false-stderr");
shadow_case!(shadow_markdown_only, "markdown-only");
shadow_case!(shadow_ceded_ojs, "ceded-ojs");
shadow_case!(shadow_twenty_cells, "twenty-cells");

/// The figure cases also assert the written file names.
#[test]
fn shadow_png_plus_text() {
    require_kernel!("png-plus-text");
    let live = shadow("png-plus-text");
    assert_eq!(live.supporting_files, ["doc_files"]);
    let names: Vec<String> = files_under(&live.dir.path().join("doc_files"))
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert_eq!(names, ["figure-html/cell-1-output-1.png"]);
}

#[test]
fn shadow_hidden_then_plot() {
    require_kernel!("hidden-then-plot");
    let live = shadow("hidden-then-plot");
    let names: Vec<String> = files_under(&live.dir.path().join("doc_files"))
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    // The hidden first cell keeps its number: the figure is cell 2's.
    assert_eq!(names, ["figure-html/cell-2-output-1.png"]);
    assert_eq!(live.record.cells.len(), 2);
}

/// The negative control: a perturbed output must fail the comparator.
#[test]
fn shadow_negative_control_detects_a_perturbed_output() {
    require_kernel!("negative control");
    let live = shadow("stdout");
    let mut perturbed = live.record.clone();
    perturbed.cells[0].outputs[0]["text"] = Value::String("not what the kernel said\n".into());
    let err = compare(&live, &perturbed).expect_err("the comparator must notice");
    assert!(err.contains("markdown differs"), "{err}");

    let mut dropped = live.record.clone();
    dropped.cells.clear();
    assert!(compare(&live, &dropped).is_err());
}

/// Phase 3's measurement for the plan (an input to bd-sr73 and the EXR-4 figure
/// decision): the serialized record size of the figure fixture and of a twenty-cell
/// document. Printed, not asserted.
#[test]
fn shadow_record_sizes_for_the_plan() {
    require_kernel!("record sizes");
    for name in ["png-plus-text", "twenty-cells"] {
        let live = live_run(&fixture(name));
        let bytes = write_record(&live.record).unwrap();
        eprintln!(
            "RECORD_SIZE {name}: {} bytes, {} cells",
            bytes.len(),
            live.record.cells.len()
        );
    }
}
