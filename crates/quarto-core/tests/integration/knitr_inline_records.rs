/*
 * tests/integration/knitr_inline_records.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! EXR-12 (`claude-notes/plans/2026-10-28-exr-12-knitr-k2-inline.md`): knitr records each
//! brace-spelling inline expression as a unit, matched by an id the rewrite embeds in the
//! span. Real R and knitr, no mocks; a missing R is a skip, a signal about the machine.
//!
//! Counts here never include chunks the stage's engine would not run, and no expectation
//! depends on which other engines are installed: each document names `engine: knitr`.

#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ProjectContext;
use quarto_core::engine::units::{Unit, scan_units};
use quarto_core::record::{AuthorTexts, Record, RecordCell, fill_ids};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

use crate::engine_stage_driver::{AuxRecorder, run_engine_stage, run_engine_stage_ast_as};

/// The shadow-equality test pins the fallback unit (stored markdown, which `place` splices); see
/// `knitr_chunk_records::FALLBACK_FORMAT`.
const FALLBACK_FORMAT: &str = "pdf";

fn skip(name: &str) -> bool {
    let ok = Command::new("Rscript")
        .args([
            "-e",
            "if (!requireNamespace(\"knitr\", quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("SKIP: knitr (Rscript + knitr package) not available — {name}");
    }
    !ok
}

/// A document whose setup chunk defines `version` and `star`, followed by `body`.
fn doc(body: &str) -> String {
    format!(
        "---\ntitle: Inline records\nengine: knitr\n---\n\n\
         ```{{r}}\n#| echo: false\n\
         version <- paste0(\"2026.0\", \"8.1\")\n\
         star <- \"*emph*\"\n```\n\n\
         {body}\n"
    )
}

fn record_for(tmp: &TempDir, source: &str) -> Record {
    let input = tmp.path().join("doc.qmd");
    std::fs::write(&input, source).unwrap();
    let observer = AuxRecorder::arc(true);
    run_engine_stage(&input, observer.clone(), None).expect("the engine stage succeeds");
    observer.record()
}

fn is_span(cell: &RecordCell) -> bool {
    cell.metadata.quarto_cell_id.ends_with("-inline")
}

fn spans(record: &Record) -> Vec<&RecordCell> {
    record.cells.iter().filter(|c| is_span(c)).collect()
}

fn markdown_of(cell: &RecordCell) -> &str {
    cell.outputs[0]["data"]["text/x-quarto-markdown"]
        .as_str()
        .expect("rendered markdown")
}

/// The ids the stage numbers for `source`'s spans, in document order.
fn scanner_span_ids(source: &str) -> Vec<String> {
    let mut units = scan_units(source);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    units
        .iter()
        .filter_map(|u| match u {
            Unit::Inline(i) if i.lang == "r" => Some(i.id.as_ref().unwrap().0.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn inline_records_one_unit_per_span_in_document_order() {
    if skip("inline_records_one_unit_per_span_in_document_order") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let source = doc("One `{r} version`, two `{r} 1 + 1`, three `{r} star`.\n\n\
         ```{r}\n#| echo: false\nx <- 40\n```\n\n\
         Four `{r} x + 2`.");
    let record = record_for(&tmp, &source);
    let spans = spans(&record);
    let texts: Vec<&str> = spans.iter().map(|c| markdown_of(c)).collect();
    assert_eq!(texts, [r"2026\.08\.1", "2", r"\*emph\*", "42"]);
    let ids: Vec<&str> = spans
        .iter()
        .map(|c| c.metadata.quarto_cell_id.as_str())
        .collect();
    assert_eq!(ids, scanner_span_ids(&source), "the stage's ids, in order");
    for c in &spans {
        assert_eq!(c.outputs[0]["output_type"], "display_data");
        assert_eq!(c.outputs[0]["metadata"]["quarto_format"], "html");
        assert_eq!(c.metadata.quarto_engine.as_deref(), Some("knitr"));
    }
    assert_eq!(record.cells.len(), 6, "two chunks and four spans");
}

#[test]
fn inline_records_brace_spans_and_not_classic_spans() {
    if skip("inline_records_brace_spans_and_not_classic_spans") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let source =
        doc("Brace `{r} \"a*b\"`, null `{r} NULL`, classic `r 6 * 7`, classic null `r NULL`.");
    let record = record_for(&tmp, &source);
    let texts: Vec<&str> = spans(&record).iter().map(|c| markdown_of(c)).collect();
    assert_eq!(texts, [r"a\*b", "NULL"], "classic spans are no units");
    assert_eq!(record.cells.len(), 3, "the setup chunk and two brace spans");
}

/// A span with nothing to insert never reaches knitr's `inline` hook; it is recorded
/// empty under its own id and the span after it keeps its id.
#[test]
fn inline_null_and_invisible_spans_record_an_empty_unit_and_do_not_shift_ids() {
    if skip("inline_null_and_invisible_spans_record_an_empty_unit_and_do_not_shift_ids") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let source = doc("A `{r} 1` b `{r} character(0)` c `{r} 3` d `{r} NULL` e `{r} 5`.");
    let record = record_for(&tmp, &source);
    let spans = spans(&record);
    let texts: Vec<&str> = spans.iter().map(|c| markdown_of(c)).collect();
    assert_eq!(texts, ["1", "", "3", "NULL", "5"]);
    let ids: Vec<&str> = spans
        .iter()
        .map(|c| c.metadata.quarto_cell_id.as_str())
        .collect();
    assert_eq!(ids, scanner_span_ids(&source));
}

/// Shadow equality: the live document is the same with and without the record being
/// built, and placing the recorded span text back into the source reproduces it.
#[test]
fn inline_id_embedding_does_not_change_output() {
    if skip("inline_id_embedding_does_not_change_output") {
        return;
    }
    let qmd = |p: &pampa::pandoc::Pandoc| {
        let mut buf = Vec::new();
        pampa::writers::qmd::write(p, &mut buf).expect("write qmd");
        String::from_utf8(buf).unwrap()
    };
    // The middle field says whether to also run `place` on the record. Off for a document
    // with a classic span (no unit, so `place` leaves it literal where the live path
    // evaluates it) and for the masked display fence.
    let cases = [
        (
            "prose",
            true,
            doc("Prose: `{r} version`, `{r} star`, `{r} I(star)`."),
        ),
        (
            "attribute and link title",
            true,
            doc("::: {#hero data-version=\"`{r} version`\"}\nHero.\n:::\n\n\
                 [link](https://example.com \"`{r} version`\")"),
        ),
        (
            "null, empty and classic",
            false,
            doc("`{r} NULL` | `{r} character(0)` | `r NULL` | `r 1 + 1` | `{r} 2 + 2`"),
        ),
        (
            "numbers and vectors",
            false,
            doc("`{r} pi` `{r} c(1.234567891, 2)` `r pi`"),
        ),
        (
            // `place` runs on the masked canonical text here; unmasking is the preview
            // path's job, not this test's.
            "display fence",
            false,
            doc("````markdown\nShown: `{r} version`\n````\n\nLive `{r} version`."),
        ),
    ];
    for (what, place_check, source) in cases {
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("doc.qmd");
        std::fs::write(&input, &source).unwrap();

        let live = run_engine_stage_ast_as(&input, FALLBACK_FORMAT, AuxRecorder::arc(false), None)
            .unwrap_or_else(|e| panic!("{what}: live run: {e:?}"));

        let recording = AuxRecorder::arc(true);
        let with_record = run_engine_stage_ast_as(&input, FALLBACK_FORMAT, recording.clone(), None)
            .unwrap_or_else(|e| panic!("{what}: record run: {e:?}"));
        let record = recording.record();
        assert_eq!(
            qmd(&with_record),
            qmd(&live),
            "{what}: embedding changed the output"
        );

        if !place_check {
            continue;
        }
        // place(record) on the canonical text gives the live document.
        let mut authored = scan_units(&source);
        let texts = AuthorTexts::from_scanned(&authored);
        fill_ids(&mut authored, &texts).unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&input, runtime.as_ref()).unwrap();
        let canonical = String::from_utf8(
            pollster::block_on(quarto_core::engine::preview_record::compute_input_qmd(
                &input, &project, runtime,
            ))
            .expect("canonical text"),
        )
        .unwrap();
        let mut units = scan_units(&canonical);
        assert_eq!(units.len(), authored.len(), "{what}: same units");
        for (unit, authored) in units.iter_mut().zip(&authored) {
            match (unit, authored) {
                (Unit::Cell(u), Unit::Cell(a)) => u.id = a.id.clone(),
                (Unit::Inline(u), Unit::Inline(a)) => u.id = a.id.clone(),
                _ => panic!("{what}: unit kinds differ"),
            }
        }
        let placed = quarto_core::record::place(&record, &units, &canonical, FALLBACK_FORMAT);
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
        let mut live = live;
        live.meta = shadow.meta.clone();
        assert_eq!(
            qmd(&shadow),
            qmd(&live),
            "{what}: place(record) must reproduce the live document\n--- placed text\n{}",
            placed.text
        );
    }
}

#[test]
fn inline_in_attribute_and_link_title_are_units() {
    if skip("inline_in_attribute_and_link_title_are_units") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let source = doc("::: {#hero data-version=\"`{r} version`\"}\nHero.\n:::\n\n\
         [link](https://example.com \"`{r} version`\")\n\nProse `{r} 1 + 1`.");
    let record = record_for(&tmp, &source);
    let texts: Vec<&str> = spans(&record).iter().map(|c| markdown_of(c)).collect();
    assert_eq!(texts, [r"2026\.08\.1", r"2026\.08\.1", "2"]);
}

/// A span inside a display fence is masked upstream: no unit, no recorded value, and the
/// text stays literal.
#[test]
fn inline_in_nested_display_fence_is_not_a_unit() {
    if skip("inline_in_nested_display_fence_is_not_a_unit") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let source = doc("````markdown\nShown `{r} version`\n````\n\nLive `{r} version`.");
    let record = record_for(&tmp, &source);
    let texts: Vec<&str> = spans(&record).iter().map(|c| markdown_of(c)).collect();
    assert_eq!(texts, [r"2026\.08\.1"], "only the live span");
    assert_eq!(scanner_span_ids(&source).len(), 1);
}

/// S3: `execute: enabled: false` does not stop knitr evaluating a brace span (existing
/// behavior, unchanged); the span is recorded like any other.
#[test]
fn inline_with_execution_disabled_is_recorded_like_any_span() {
    if skip("inline_with_execution_disabled_is_recorded_like_any_span") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let record = record_for(
        &tmp,
        "---\nengine: knitr\nexecute:\n  enabled: false\n---\n\nValue `{r} 1+1`.\n\n```{r}\n1+1\n```\n",
    );
    let texts: Vec<&str> = spans(&record).iter().map(|c| markdown_of(c)).collect();
    assert_eq!(texts, ["2"]);
}

/// An inline expression that fails is reported at its line in the author's file, not at a
/// line of the text handed to R, with and without a record being built; `error: true`
/// does not change that (it governs chunks).
#[test]
fn inline_error_names_the_unit_location() {
    if skip("inline_error_names_the_unit_location") {
        return;
    }
    for front in ["engine: knitr", "engine: knitr\nexecute:\n  error: true"] {
        let source = format!(
            "---\n{front}\n---\n\nIntro.\n\n```{{r}}\n#| label: fig-a\nplot(1)\n```\n\nValue `{{r}} stop(\"boom\")` after.\n"
        );
        let line = source.lines().position(|l| l.contains("boom")).unwrap() + 1;
        let tmp = TempDir::new().unwrap();
        let input = tmp.path().join("doc.qmd");
        std::fs::write(&input, &source).unwrap();
        for wants_record in [true, false] {
            let err = run_engine_stage(&input, AuxRecorder::arc(wants_record), None)
                .expect_err("the span fails");
            let text = format!("{err:?}");
            assert!(
                text.contains(&format!("at lines {line}-{line}: ")),
                "{front} wants_record={wants_record}: {text}"
            );
        }
    }
}
