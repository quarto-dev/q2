/*
 * tests/integration/marimo_records.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! EXR-9 Phase 9A (`claude-notes/plans/2026-10-28-exr-09-marimo.md`): marimo's engine
//! execution record, one unit per marimo-owned cell.
//!
//! Real node, `uv` and marimo, no mocks; the same gate as `marimo_engine_e2e.rs`. The
//! tests that need the engine's per-cell output (9B) or the core adapter (9C) are
//! `#[ignore]`d with the phase that unblocks them; run them with `--run-ignored all` and
//! remove the attribute when that phase lands.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use quarto_core::engine::ts_process::node_available_for_tests;
use quarto_core::record::{Granularity, Record};

use crate::engine_stage_driver::{
    AuxRecorder, run_engine_stage, run_engine_stage_as, run_engine_stage_ast,
};

fn uv_available() -> bool {
    Command::new("uv")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn skip(name: &str) -> bool {
    if node_available_for_tests() && uv_available() {
        return false;
    }
    eprintln!("SKIP: node + uv not both available: {name}");
    true
}

fn marimo_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extensions/marimo")
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// A temp project holding the marimo engine package and `source` as `doc.qmd`.
fn project_for(source: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    copy_dir(
        &marimo_fixture_root().join("_extensions/marimo"),
        &tmp.path().join("_extensions").join("marimo"),
    );
    let input = tmp.path().join("doc.qmd");
    std::fs::write(&input, source).unwrap();
    (tmp, input)
}

/// As [`project_for`]; returns the coalesced record the engine stage emits for it.
fn record_for(source: &str) -> (TempDir, Record) {
    let (tmp, input) = project_for(source);
    let observer = AuxRecorder::arc(true);
    run_engine_stage(&input, observer.clone(), None).expect("the engine stage succeeds");
    let record = observer.record();
    (tmp, record)
}

/// The class rule, not the language, decides which cells are units: a bare `{python}`
/// cell is not marimo's, even though the resolver marks `python` owned by marimo.
#[test]
#[ignore = "bd-mts59whz: the live render fails today (the Python compiler counts the bare \
cell, the engine's regex does not), and the record needs the per-cell output and adapter"]
fn marimo_mixed_tagged_and_bare_python_cells() {
    if skip("marimo_mixed_tagged_and_bare_python_cells") {
        return;
    }
    let (_tmp, record) = record_for(
        "---\nengine: marimo\n---\n\n\
```{python .marimo}\n1 + 1\n```\n\n\
```{python}\n2 + 2\n```\n",
    );
    let marimo: Vec<_> = record
        .metadata
        .quarto_engines
        .iter()
        .filter(|e| e.hash_definition.id == "marimo")
        .collect();
    assert_eq!(marimo.len(), 1, "one marimo engine entry");
    assert_eq!(
        record.cells.len(),
        1,
        "the bare cell is not a unit: {:#?}",
        record.cells
    );
    assert!(
        record.cells[0].source.concat().contains("1 + 1"),
        "the unit is the tagged cell: {:#?}",
        record.cells[0]
    );
}

/// One unit per owned cell, in document order, none for prose.
#[test]
fn marimo_record_has_one_unit_per_owned_cell() {
    if skip("marimo_record_has_one_unit_per_owned_cell") {
        return;
    }
    let (_tmp, record) = record_for(
        "---\nengine: marimo\n---\n\nText.\n\n\
```{python .marimo}\n1 + 1\n```\n\n\
```{markdown .marimo}\n# Heading\n```\n\n\
```{python .marimo}\n3 + 3\n```\n",
    );
    assert_eq!(record.cells.len(), 3, "{:#?}", record.cells);
    let sources: Vec<String> = record.cells.iter().map(|c| c.source.concat()).collect();
    assert!(sources[0].contains("1 + 1"), "{sources:?}");
    assert!(sources[1].contains("Heading"), "{sources:?}");
    assert!(sources[2].contains("3 + 3"), "{sources:?}");
    // Each unit holds the cell's rendered output, tagged with the format: for html, an
    // island element.
    for cell in &record.cells {
        let out = &cell.outputs[0];
        assert_eq!(out["output_type"], "display_data");
        assert_eq!(out["metadata"]["quarto_format"], "html", "{out}");
        assert!(
            markdown_of(cell).contains("marimo-quarto-island"),
            "{}",
            markdown_of(cell)
        );
    }
    // Marimo's declaration reached the record's engine entry.
    let entry = record
        .metadata
        .quarto_engines
        .iter()
        .find(|e| e.name == "marimo")
        .expect("a marimo engine entry");
    assert_eq!(entry.hash_definition.granularity, Granularity::Document);
    assert_eq!(
        entry.hash_definition.document_keys,
        ["eval", "external-env", "pyproject"]
    );
}

/// A typst/pdf render records static markdown, not islands, and tags each output with its
/// own format id: marimo's mode is per output format (`interactive` only for html-compatible
/// formats), so the same cell carries different content per format.
#[test]
fn marimo_record_tags_static_format_for_pdf() {
    if skip("marimo_record_tags_static_format_for_pdf") {
        return;
    }
    let (_tmp, input) = project_for(
        "---\nengine: marimo\n---\n\nText.\n\n```{python .marimo}\n\"static-out\"\n```\n",
    );
    for format in ["typst", "pdf"] {
        let observer = AuxRecorder::arc(true);
        run_engine_stage_as(&input, format, observer.clone(), None)
            .unwrap_or_else(|e| panic!("{format}: the engine stage succeeds: {e:?}"));
        let record = observer.record();
        assert_eq!(record.cells.len(), 1, "{format}: {:#?}", record.cells);
        let out = &record.cells[0].outputs[0];
        assert_eq!(out["metadata"]["quarto_format"], format, "{out}");
        let md = markdown_of(&record.cells[0]);
        assert!(
            !md.contains("marimo-quarto-island"),
            "{format}: static, not islands: {md}"
        );
        assert!(
            md.contains("static-out"),
            "{format}: the cell's output is in the markdown: {md}"
        );
    }
}

/// Bare `{sql}` is a unit exactly when q2 assigned sql to marimo, which it does when a
/// marimo cell is present (the interop rule).
#[test]
fn marimo_record_includes_bare_sql_that_marimo_owns() {
    if skip("marimo_record_includes_bare_sql_that_marimo_owns") {
        return;
    }
    let (_tmp, record) = record_for(
        "---\nengine: marimo\n---\n\n\
```{python .marimo}\n1 + 1\n```\n\n\
```{sql}\nselect 1 as one\n```\n",
    );
    assert_eq!(record.cells.len(), 2, "{:#?}", record.cells);
    assert!(record.cells[1].source.concat().contains("select 1"));
}

/// The island header is stored as content on the engine entry, not as the temp file's path.
/// Core folds every engine's includes this way; this pins it for marimo.
#[test]
fn marimo_record_folds_header_include() {
    if skip("marimo_record_folds_header_include") {
        return;
    }
    let (_tmp, record) =
        record_for("---\nengine: marimo\n---\n\n```{python .marimo}\n1 + 1\n```\n");
    let entry = record
        .metadata
        .quarto_engines
        .iter()
        .find(|e| e.name == "marimo")
        .expect("a marimo engine entry");
    let includes = entry
        .includes
        .as_ref()
        .expect("the header include is folded")
        .to_string();
    assert!(
        includes.contains("data-marimo-islands"),
        "the header content is stored: {}",
        &includes[..includes.len().min(200)]
    );
    assert!(
        !includes.contains("marimo-header-") && !includes.contains("header.html"),
        "the temp-file path appears nowhere"
    );
}

fn markdown_of(cell: &quarto_core::record::RecordCell) -> &str {
    cell.outputs[0]["data"]["text/x-quarto-markdown"]
        .as_str()
        .expect("rendered markdown")
}

/// EXR-9 9C shadow assertion: placing the recorded cells into the document text reproduces
/// the live engine's document. Same shape as `knitr_chunk_records.rs`; marimo's stage input
/// is the author's text, so the units' ids need no re-mapping.
#[test]
fn marimo_place_of_record_reproduces_the_live_document() {
    if skip("marimo_place_of_record_reproduces_the_live_document") {
        return;
    }
    let cases = [
        (
            "python and markdown cells",
            "---\nengine: marimo\n---\n\nText.\n\n\
```{python .marimo}\n1 + 1\n```\n\n\
```{markdown .marimo}\n# Heading\n```\n\nAfter.\n",
        ),
        (
            "bare sql owned by marimo",
            "---\nengine: marimo\n---\n\n\
```{python .marimo}\n1 + 1\n```\n\n\
```{sql}\nselect 1 as one\n```\n",
        ),
    ];
    let qmd = |p: &pampa::pandoc::Pandoc| {
        let mut buf = Vec::new();
        pampa::writers::qmd::write(p, &mut buf).expect("write qmd");
        String::from_utf8(buf).unwrap()
    };
    for (what, source) in cases {
        let (_tmp, input) = project_for(source);
        let recording = AuxRecorder::arc(true);
        run_engine_stage(&input, recording.clone(), None).expect("record run");
        let record = recording.record();
        let mut live =
            run_engine_stage_ast(&input, AuxRecorder::arc(false), None).expect("live run");

        let mut units = quarto_core::engine::units::scan_units(source);
        let texts = quarto_core::record::AuthorTexts::from_scanned(&units);
        quarto_core::record::fill_ids(&mut units, &texts).unwrap();
        let placed = quarto_core::record::place(&record, &units, source, "html");
        assert!(placed.notes.is_empty(), "{what}: {:?}", placed.notes);
        let (shadow, _, _) = pampa::readers::qmd::read(
            placed.text.as_bytes(),
            false,
            "shadow.qmd",
            &mut std::io::sink(),
            false,
            None,
        )
        .unwrap_or_else(|e| panic!("{what}: placed text must parse: {e:?}\n{}", placed.text));
        live.meta = shadow.meta.clone();
        assert_eq!(
            qmd(&shadow),
            qmd(&live),
            "{what}: place(record) must reproduce the live document\n--- placed text\n{}",
            placed.text
        );
    }
}

/// EXR-9 9C replay: a marimo record replays through the preview pipeline with no engine
/// registered, and carries the island header into `meta.rendered.includes.header`, the
/// channel the preview pane reads (the marimo half of bd-5oyk1xce). Without the header the
/// islands never hydrate.
#[test]
fn marimo_record_replays_header_include_without_engine() {
    use std::sync::Arc;

    use quarto_core::engine::EngineRegistry;
    use quarto_core::format::Format;
    use quarto_core::pandoc_request::captures::CapturePayload;
    use quarto_core::pipeline::render_qmd_to_preview_ast;
    use quarto_core::project::{DocumentInfo, ProjectContext};
    use quarto_core::render::{BinaryDependencies, RenderContext};
    use quarto_system_runtime::{NativeRuntime, SystemRuntime};

    if skip("marimo_record_replays_header_include_without_engine") {
        return;
    }
    let source = "---\nengine: marimo\n---\n\n```{python .marimo}\n1 + 1\n```\n";
    let (_tmp, record) = record_for(source);
    let (tmp, input) = project_for(source);
    let input = input.canonicalize().unwrap();
    let _ = &tmp;

    let ast_json = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
            let project = ProjectContext::discover(&input, runtime.as_ref()).unwrap();
            let doc_info = DocumentInfo::from_path(&input);
            let format = Format::html();
            let bins = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc_info, &format, &bins);
            render_qmd_to_preview_ast(
                source.as_bytes(),
                &input.display().to_string(),
                &mut ctx,
                runtime,
                Some(Arc::new(EngineRegistry::new())),
                CapturePayload::Record(Arc::new(record)),
            )
            .await
            .expect("the record replays with no engine")
            .ast_json
        });
    assert!(
        ast_json.contains("data-marimo-islands"),
        "the island header reaches the preview meta: {}",
        &ast_json[..ast_json.len().min(300)]
    );
}

/// EXR-9 9A: a recorded marimo document renders to the same HTML with no engine as the live
/// render did. The replay registry is empty, so nothing can execute: marimo, node and `uv`
/// play no part in the second render.
#[test]
fn marimo_record_replays_without_engine() {
    use std::sync::Arc;

    use quarto_core::engine::EngineRegistry;
    use quarto_core::format::Format;
    use quarto_core::pipeline::{HtmlRenderConfig, render_qmd_to_html};
    use quarto_core::project::{DocumentInfo, ProjectContext};
    use quarto_core::render::{BinaryDependencies, RenderContext};
    use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
    use quarto_system_runtime::{NativeRuntime, SystemRuntime};

    if skip("marimo_record_replays_without_engine") {
        return;
    }
    let source = "---\nengine: marimo\ntitle: Replay\n---\n\nText.\n\n\
```{python .marimo}\n1 + 1\n```\n\nAfter.\n";
    let (_tmp, record) = record_for(source);
    let (_tmp2, input) = project_for(source);
    let input = input.canonicalize().unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let live_result = render_to_file(
        &input,
        "html",
        &RenderToFileOptions::default(),
        runtime.clone(),
    )
    .expect("live render");
    // The file render links its assets under `doc_files/`; the in-memory render has no output
    // directory and links them bare. That is layout, not engine output.
    let live = std::fs::read_to_string(&live_result.output_path)
        .unwrap()
        .replace("doc_files/", "");

    let replayed = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let project = ProjectContext::discover(&input, runtime.as_ref()).unwrap();
            let doc_info = DocumentInfo::from_path(&input);
            let format = Format::html();
            let bins = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc_info, &format, &bins);
            ctx.engine_registry_override = Some(Arc::new(EngineRegistry::new()));
            render_qmd_to_html(
                source.as_bytes(),
                &input.display().to_string(),
                &mut ctx,
                &HtmlRenderConfig::default().with_record(Arc::new(record)),
                runtime,
            )
            .await
            .expect("replay render")
            .html
        });
    assert!(
        replayed.contains("<marimo-quarto-island "),
        "island markup replayed"
    );
    assert_eq!(replayed, live, "replay must equal the live render");
}
