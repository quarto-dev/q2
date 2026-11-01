/*
 * tests/integration/knitr_chunk_records.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! EXR-11 (`claude-notes/plans/2026-10-28-exr-11-knitr-k1-chunk-records.md`):
//! knitr chunk records. Phase 11S, S1: an extra `#|` option reaches the
//! chunk hook under its own name, and the hook must not emit it as a
//! `name='value'` attribute on the cell div.
//!
//! Real R and knitr, no mocks; same gate as `knitr_label_reinject.rs`.

#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ProjectContext;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

use crate::engine_stage_driver::{AuxRecorder, run_engine_stage, run_engine_stage_ast};

fn knitr_render_available() -> bool {
    Command::new("Rscript")
        .args([
            "-e",
            "if (!requireNamespace(\"knitr\", quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn skip(name: &str) -> bool {
    if knitr_render_available() {
        return false;
    }
    eprintln!("SKIP: knitr (Rscript + knitr package) not available — {name}");
    true
}

fn render_html(tmp: &TempDir, source: &str) -> String {
    let input = tmp.path().join("doc.qmd");
    std::fs::write(&input, source).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref())
        .expect("project discovery for the chunk-records fixture");
    let result = render_document_to_file(
        &input,
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

/// S1: the id option is not emitted as a div attribute.
#[test]
fn knitr_chunk_id_is_not_emitted_as_an_attribute() {
    if skip("knitr_chunk_id_is_not_emitted_as_an_attribute") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(
        &tmp,
        "---\ntitle: Id\nengine: knitr\n---\n\n\
```{r}\n\
#| quarto-unit-id: abc123\n\
#| echo: false\n\
1 + 1\n\
```\n",
    );
    assert!(
        !html.contains("quarto-unit-id") && !html.contains("abc123"),
        "id option leaked into the HTML:\n{html}"
    );
}

/// Run the engine stage with real knitr and return the coalesced record and the document dir.
fn record_for(tmp: &TempDir, source: &str) -> quarto_core::record::Record {
    let input = tmp.path().join("doc.qmd");
    std::fs::write(&input, source).unwrap();
    let observer = AuxRecorder::arc(true);
    run_engine_stage(&input, observer.clone(), None).expect("the engine stage succeeds");
    observer.record()
}

fn markdown_of(cell: &quarto_core::record::RecordCell) -> &str {
    cell.outputs[0]["data"]["text/x-quarto-markdown"]
        .as_str()
        .expect("rendered markdown")
}

/// One unit per owned chunk, in document order, with the stage's ids.
#[test]
fn knitr_chunk_records_one_per_owned_chunk() {
    if skip("knitr_chunk_records_one_per_owned_chunk") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let record = record_for(
        &tmp,
        "---\nengine: knitr\n---\n\nText.\n\n\
```{r}\n1 + 1\n```\n\n\
```{r}\n#| eval: false\nstop('never')\n```\n\n\
```{r}\n#| include: false\n3 + 3\n```\n\n\
```{r}\n#| label: fig-a\n#| fig-cap: A plot\nplot(1)\n```\n",
    );
    assert_eq!(record.cells.len(), 4, "{:#?}", record.cells);
    let ids: Vec<&str> = record
        .cells
        .iter()
        .map(|c| c.metadata.quarto_cell_id.as_str())
        .collect();
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 4, "distinct stage ids: {ids:?}");
    for c in &record.cells {
        assert_eq!(c.metadata.quarto_engine.as_deref(), Some("knitr"));
        assert_eq!(c.outputs[0]["metadata"]["quarto_format"], "html");
    }
    assert!(markdown_of(&record.cells[0]).contains("[1] 2"));
    assert_eq!(
        markdown_of(&record.cells[2]),
        "",
        "include: false records nothing"
    );
    assert!(markdown_of(&record.cells[3]).contains("fig-a"));
    assert!(!markdown_of(&record.cells[3]).contains("quarto-unit-id"));
}

/// Each figure file is attributed to its chunk, and the bytes arrive through the
/// existing capture-file path.
#[test]
fn knitr_record_files_per_chunk() {
    if skip("knitr_record_files_per_chunk") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let record = record_for(
        &tmp,
        "---\nengine: knitr\n---\n\n\
```{r}\nplot(1)\n```\n\n```{r}\nplot(2)\n```\n",
    );
    let files: Vec<Vec<String>> = record
        .cells
        .iter()
        .map(|c| {
            c.outputs[0]["metadata"]["quarto_files"]
                .as_array()
                .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|f| f.len() == 1), "{files:?}");
    assert_ne!(files[0], files[1]);

    let runtime = NativeRuntime::new();
    for f in files.iter().flatten() {
        let captured = quarto_core::engine::capture_files::collect_capture_files(
            &runtime,
            tmp.path(),
            &[std::path::PathBuf::from(f)],
        );
        assert_eq!(captured.len(), 1, "bytes for {f}");
    }
}

/// Shadow equality at stage level: placing the recorded chunk markdown into the source
/// with the real `place` and parsing it gives the live stage's document. Both sides go
/// through the qmd writer, which normalizes knitr's blank-line spacing after a chunk
/// (not part of a record); the front matter is the source's on both sides.
#[test]
fn knitr_stage_shadow_equality() {
    if skip("knitr_stage_shadow_equality") {
        return;
    }
    let cases = [
        (
            "labeled figure",
            "---\nengine: knitr\n---\n\n```{r}\n#| label: fig-cars\n#| echo: true\n#| fig-cap: \"Cars plot\"\nplot(cars)\n```\n",
        ),
        (
            "plain cell between prose",
            "---\nengine: knitr\n---\n\nBefore.\n\n```{r}\ncat(paste0(\"O\", \"UT\"), \"\\n\")\n```\n\nAfter.\n",
        ),
        (
            "include false",
            "---\nengine: knitr\n---\n\nTop.\n\n```{r}\n#| include: false\n1 + 1\n```\n\nBottom.\n",
        ),
        (
            "several chunks",
            "---\nengine: knitr\n---\n\nA.\n\n```{r}\n1 + 1\n```\n\nB.\n\n```{r}\n#| echo: false\n2 + 2\n```\n\nC.\n",
        ),
    ];
    let qmd = |p: &pampa::pandoc::Pandoc| {
        let mut buf = Vec::new();
        pampa::writers::qmd::write(p, &mut buf).expect("write qmd");
        String::from_utf8(buf).unwrap()
    };
    for (what, source) in cases {
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("doc.qmd");
        std::fs::write(&input, source).unwrap();

        let recording = AuxRecorder::arc(true);
        run_engine_stage(&input, recording.clone(), None).expect("record run");
        let record = recording.record();
        let mut live =
            run_engine_stage_ast(&input, AuxRecorder::arc(false), None).expect("live run");

        // `place` runs on the canonical text the stage handed to the engine (crossref
        // wrapper Divs, `label:` lines lifted off the chunk). Its units get the ids of
        // the author's own text, which is what the stage hashes: the author text keeps
        // the label lines the canonical text lacks. Same chunks, same order.
        let mut authored = quarto_core::engine::units::scan_units(source);
        let texts = quarto_core::record::AuthorTexts::from_scanned(&authored);
        quarto_core::record::fill_ids(&mut authored, &texts).unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&input, runtime.as_ref()).unwrap();
        let canonical = String::from_utf8(
            pollster::block_on(quarto_core::engine::preview_record::compute_input_qmd(
                &input, &project, runtime,
            ))
            .expect("canonical text"),
        )
        .unwrap();
        let mut units = quarto_core::engine::units::scan_units(&canonical);
        assert_eq!(units.len(), authored.len(), "{what}: same chunks");
        for (unit, authored) in units.iter_mut().zip(&authored) {
            match (unit, authored) {
                (
                    quarto_core::engine::units::Unit::Cell(u),
                    quarto_core::engine::units::Unit::Cell(a),
                ) => u.id = a.id.clone(),
                _ => panic!("{what}: cells only"),
            }
        }
        let placed = quarto_core::record::place(&record, &units, &canonical, "html");
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

/// EXR-11 11D: a failing chunk is reported at the author's lines, not the lines of the
/// text handed to R (which carries an id option line per chunk and the re-inserted label).
#[test]
fn knitr_error_lines_point_at_the_authors_file() {
    if skip("knitr_error_lines_point_at_the_authors_file") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("doc.qmd");
    // knitr reports the whole failing chunk: lines 16-19 of the file (R sees ~150 more
    // lines, mostly the serialized default front matter).
    let source = "---\nengine: knitr\ntitle: T\n---\n\nIntro.\n\n\
```{r}\n#| label: fig-a\n#| fig-cap: A\nplot(1)\n```\n\nMid.\n\n\
```{r}\nx <- 1\nstop(\"boom\")\n```\n";
    assert_eq!(source.lines().position(|l| l.contains("boom")), Some(17));
    std::fs::write(&input, source).unwrap();
    for wants_record in [true, false] {
        let err = run_engine_stage(&input, AuxRecorder::arc(wants_record), None)
            .expect_err("the chunk fails");
        let text = format!("{err:?}");
        assert!(
            text.contains("at lines 16-19: boom"),
            "wants_record={wants_record}: {text}"
        );
    }
}
