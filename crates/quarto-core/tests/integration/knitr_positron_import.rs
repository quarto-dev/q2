//! KNR-5 Phase 5A: a Positron cache for `{r}` cells fills knitr-owned units and presents
//! through the shared presenter, display-only. Outputs have no knitr provenance, so the
//! presentation options come from the current document source.

#![cfg(not(target_arch = "wasm32"))]

use serde_json::json;
use tempfile::TempDir;

use quarto_core::engine::units::{Unit, scan_units};
use quarto_core::presenter::document::present_document;
use quarto_core::presenter::figures::FsFigureSink;
use quarto_core::presenter::inline::InlineTarget;
use quarto_core::record::{
    AuthorTexts, EngineEntryInput, HashDefinition, KNITR_ENGINE, RecordBuilder, UnitResult,
    fill_ids, import_positron_cache,
};

/// A 1x1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGD4DwABBAEAwS2OUAAAAABJRU5ErkJggg==";

fn staged(text: &str) -> (Vec<Unit>, AuthorTexts) {
    let mut units = scan_units(text);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    (units, texts)
}

/// A Positron-shaped cache with one `{r}` cell holding every output kind K-S2 saw.
fn positron_cache(hash: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "nbformat": 4, "nbformat_minor": 5,
        "metadata": {"quarto_source": "file:///d/a.qmd", "quarto_cache_version": 1},
        "cells": [{
            "cell_type": "code", "execution_count": null,
            "metadata": {"quarto_cell_id": "0-x-unlabeled", "quarto_content_hash": hash},
            "source": ["x"],
            "outputs": [
                {"output_type": "stream", "name": "stdout", "text": ["hello\n"]},
                {"output_type": "execute_result", "execution_count": null,
                 "data": {"text/plain": "[1] 2"}, "metadata": {}},
                {"output_type": "execute_result", "execution_count": null,
                 "data": {"image/png": PNG, "text/plain": "<Figure>"}, "metadata": {}},
                {"output_type": "error", "ename": "", "evalue": "boom", "traceback": ["Error: boom"]}
            ]
        }]
    }))
    .unwrap()
}

fn present(doc: &str, owner: &str, fallback: bool, tmp: &TempDir) -> (String, bool) {
    let (units, texts) = staged(doc);
    let hash = &texts.0[0].content_hash;
    let import = import_positron_cache(&positron_cache(hash)).unwrap();
    let lang = units[0].lang().to_string();
    let Some(found) = import.outputs_for(hash, 0, owner, &lang, fallback) else {
        return (doc.to_string(), false);
    };
    let def = HashDefinition::conservative(KNITR_ENGINE);
    let mut b = RecordBuilder::new(KNITR_ENGINE, &def, &units, &texts).unwrap();
    b.push(
        0,
        UnitResult {
            outputs: found.outputs().to_vec(),
            execution_count: None,
            kernel: None,
        },
    )
    .unwrap();
    let cells = b.finish(EngineEntryInput::default()).unwrap().cells;
    let mut fig = FsFigureSink::new(&tmp.path().join("doc.qmd"));
    let out = present_document(
        &cells,
        doc,
        &units,
        None,
        InlineTarget::from_format("html"),
        "html",
        &[],
        &mut fig,
    )
    .unwrap();
    (out.markdown, !out.files_written.is_empty())
}

#[test]
fn an_ark_cache_fills_a_knitr_owned_r_unit_and_presents() {
    let tmp = TempDir::new().unwrap();
    let (md, wrote) = present("```{r}\n1 + 1\n```\n", KNITR_ENGINE, false, &tmp);
    assert!(md.contains("hello"), "{md}");
    assert!(md.contains("[1] 2"), "{md}");
    assert!(md.contains("boom"), "{md}");
    assert!(wrote, "the plot goes through the figure sink: {md}");
    assert!(md.contains("cell-1-output-"), "{md}");
    assert!(
        tmp.path()
            .join("doc_files/figure-html")
            .read_dir()
            .unwrap()
            .count()
            >= 1
    );
}

#[test]
fn current_document_options_drive_the_imported_presentation() {
    let tmp = TempDir::new().unwrap();
    let (md, _) = present(
        "```{r}\n#| echo: false\n1 + 1\n```\n",
        KNITR_ENGINE,
        false,
        &tmp,
    );
    assert!(!md.contains("cell-code"), "{md}");
    assert!(md.contains("[1] 2"), "{md}");
}

#[test]
fn a_unit_another_engine_owns_or_a_fallback_unit_is_left_alone() {
    let tmp = TempDir::new().unwrap();
    let doc = "```{r}\n1 + 1\n```\n";
    assert_eq!(present(doc, "other", false, &tmp), (doc.to_string(), false));
    assert_eq!(
        present(doc, KNITR_ENGINE, true, &tmp),
        (doc.to_string(), false)
    );
}

// ---- q2 knitr records are valid to present in Positron (KNR-5, direction 1) ----

use std::process::Command;
use std::sync::Arc;

use quarto_core::record::{Record, write_record};

use crate::engine_stage_driver::{AuxRecorder, run_engine_stage, run_engine_stage_as};

fn knitr_available() -> bool {
    Command::new("Rscript")
        .args([
            "-e",
            "if (!requireNamespace(\"knitr\", quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The record the engine stage writes for `source`, for the structured target (`html`) or the
/// fallback one (`pdf`).
fn knitr_record(tmp: &TempDir, source: &str, format: &str) -> Record {
    let input = tmp.path().join("doc.qmd");
    std::fs::write(&input, source).unwrap();
    let observer = AuxRecorder::arc(true);
    let observer: Arc<AuxRecorder> = observer;
    if format == "html" {
        run_engine_stage(&input, observer.clone(), None).expect("the engine stage succeeds");
    } else {
        run_engine_stage_as(&input, format, observer.clone(), None)
            .expect("the engine stage succeeds");
    }
    observer.record()
}

/// The checks Positron's loader makes (see `positron_conformance`), on the written file.
fn assert_positron_shape(record: &Record) {
    let file: serde_json::Value = serde_json::from_str(&write_record(record).unwrap()).unwrap();
    assert!(file["nbformat"].is_number());
    assert!(file["cells"].is_array());
    assert_eq!(file["metadata"]["quarto_cache_version"], 1);
    for cell in file["cells"].as_array().unwrap() {
        if cell["cell_type"] == "code" {
            assert!(cell["outputs"].is_array(), "{cell}");
            assert!(cell["metadata"]["quarto_content_hash"].is_string());
        }
    }
}

const KNITR_DOC: &str = "---\nengine: knitr\n---\n\n```{r}\ncat('hi\\n')\n1 + 1\n```\n\n\
```{r}\n#| label: fig-a\n#| fig-cap: A plot\nplot(1)\n```\n";

#[test]
fn a_structured_knitr_record_is_valid_to_present_in_positron() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let record = knitr_record(&tmp, KNITR_DOC, "html");
    assert_positron_shape(&record);
    // The KNR-3 keys are there and Positron reads the file without them mattering (K-S1).
    let c = &record.cells[0];
    assert!(c.metadata.extra.contains_key("quarto_resolved_options"));
    assert!(c.metadata.extra.contains_key("quarto_output_kinds"));
    assert!(c.metadata.extra.contains_key("quarto_output_anchors"));
    // q2's mapper emits strings, never nbformat line arrays, for html and plain text
    // (a line array is mangled on load).
    for o in &record.cells[1].outputs {
        if let Some(data) = o.get("data").and_then(|d| d.as_object()) {
            for mime in ["text/html", "text/plain"] {
                if let Some(v) = data.get(mime) {
                    assert!(v.is_string(), "{mime}: {v}");
                }
            }
        }
    }
    if let Some(dir) = std::env::var_os("KNR5_DUMP_RECORDS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("structured.ipynb"),
            write_record(&record).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn a_fallback_knitr_record_is_valid_to_present_in_positron() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let record = knitr_record(&tmp, KNITR_DOC, "pdf");
    assert_positron_shape(&record);
    assert!(
        record.cells[0].outputs[0]["data"]
            .get("text/x-quarto-markdown")
            .is_some()
    );
    if let Some(dir) = std::env::var_os("KNR5_DUMP_RECORDS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("fallback.ipynb"),
            write_record(&record).unwrap(),
        )
        .unwrap();
    }
}

/// The record's `quarto_content_hash` is over the author's text, as the scanner (which
/// reproduces Positron's hash, `positron_conformance`) computes it, not over the text knitr's
/// label re-injection rewrote (parent decision 18).
fn assert_hashes_match_author_text(source: &str) {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let (_, texts) = staged(source);
    let want: Vec<&str> = texts.0.iter().map(|a| a.content_hash.as_str()).collect();
    let record = knitr_record(&tmp, source, "html");
    let got: Vec<&str> = record
        .cells
        .iter()
        .map(|c| c.metadata.quarto_content_hash.as_str())
        .collect();
    assert_eq!(got, want, "{source:?}");
}

#[test]
fn a_labeled_chunk_hashes_the_authors_text() {
    assert_hashes_match_author_text(
        "---\nengine: knitr\n---\n\n```{r}\n#| label: fig-a\n#| fig-cap: A plot\nplot(1)\n```\n",
    );
}

#[test]
fn a_label_option_line_does_not_change_the_hash_rule() {
    assert_hashes_match_author_text(
        "---\nengine: knitr\n---\n\n```{r}\n#| label: a-chunk\n1 + 1\n```\n",
    );
}

#[test]
fn crlf_input_hashes_like_the_scanner() {
    assert_hashes_match_author_text(
        "---\r\nengine: knitr\r\n---\r\n\r\n```{r}\r\n1 + 1\r\n```\r\n",
    );
}

#[test]
fn an_imported_output_with_only_an_unknown_mime_presents_nothing_for_it() {
    let tmp = TempDir::new().unwrap();
    let doc = "```{r}\n1 + 1\n```\n";
    let (units, texts) = staged(doc);
    let hash = &texts.0[0].content_hash;
    let file = serde_json::to_vec(&json!({
        "nbformat": 4, "nbformat_minor": 5,
        "metadata": {"quarto_cache_version": 1},
        "cells": [{
            "cell_type": "code", "execution_count": null,
            "metadata": {"quarto_content_hash": hash}, "source": ["x"],
            "outputs": [
                {"output_type": "execute_result", "execution_count": null,
                 "data": {"application/x-q2-unknown": "zzz-unknown"}, "metadata": {}},
                {"output_type": "stream", "name": "stdout", "text": ["kept\n"]}
            ]
        }]
    }))
    .unwrap();
    let import = import_positron_cache(&file).unwrap();
    let found = import
        .outputs_for(hash, 0, KNITR_ENGINE, "r", false)
        .unwrap();
    let def = HashDefinition::conservative(KNITR_ENGINE);
    let mut b = RecordBuilder::new(KNITR_ENGINE, &def, &units, &texts).unwrap();
    b.push(
        0,
        UnitResult {
            outputs: found.outputs().to_vec(),
            execution_count: None,
            kernel: None,
        },
    )
    .unwrap();
    let cells = b.finish(EngineEntryInput::default()).unwrap().cells;
    let mut fig = FsFigureSink::new(&tmp.path().join("doc.qmd"));
    let md = present_document(
        &cells,
        doc,
        &units,
        None,
        InlineTarget::from_format("html"),
        "html",
        &[],
        &mut fig,
    )
    .unwrap()
    .markdown;
    assert!(!md.contains("zzz-unknown"), "{md}");
    assert!(md.contains("kept"), "{md}");
}

/// KNR-5 5C K-S2 check, off unless `KS2_ARK` (a Positron cache file) and `KS2_QMD` (its document)
/// are set: imports a real Ark-written cache into every knitr-owned `{r}` unit of the document and
/// presents the result as HTML, printing the markdown and the unit-by-unit import outcome.
#[test]
fn a_real_ark_cache_presents_through_the_knitr_layer() {
    let (Some(ark), Some(qmd)) = (std::env::var_os("KS2_ARK"), std::env::var_os("KS2_QMD")) else {
        return;
    };
    let doc = std::fs::read_to_string(qmd).unwrap();
    let (units, texts) = staged(&doc);
    let import = import_positron_cache(&std::fs::read(ark).unwrap()).unwrap();
    let def = HashDefinition::conservative(KNITR_ENGINE);
    let mut b = RecordBuilder::new(KNITR_ENGINE, &def, &units, &texts).unwrap();
    for (i, u) in units.iter().enumerate() {
        let hash = &texts.0[i].content_hash;
        let found = import.outputs_for(hash, 0, KNITR_ENGINE, u.lang(), false);
        eprintln!(
            "KS2 unit {i} lang={} hash={hash} imported={}",
            u.lang(),
            found.is_some()
        );
        let outputs = found.map(|f| f.outputs().to_vec()).unwrap_or_default();
        b.push(
            i,
            UnitResult {
                outputs,
                execution_count: None,
                kernel: None,
            },
        )
        .unwrap();
    }
    let cells = b.finish(EngineEntryInput::default()).unwrap().cells;
    let tmp = TempDir::new().unwrap();
    let mut fig = FsFigureSink::new(&tmp.path().join("doc.qmd"));
    let out = present_document(
        &cells,
        &doc,
        &units,
        None,
        InlineTarget::from_format("html"),
        "html",
        &[],
        &mut fig,
    )
    .unwrap();
    for e in std::fs::read_dir(tmp.path().join("doc_files/figure-html")).unwrap() {
        let e = e.unwrap();
        eprintln!(
            "KS2 wrote {:?} ({} bytes)",
            e.file_name(),
            e.metadata().unwrap().len()
        );
    }
    eprintln!(
        "KS2 files_written={:?}\n----- markdown -----\n{}",
        out.files_written, out.markdown
    );
}
