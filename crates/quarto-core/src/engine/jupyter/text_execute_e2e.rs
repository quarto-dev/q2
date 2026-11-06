/*
 * engine/jupyter/text_execute_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-18 Phase A: whole-flow tests of the Jupyter record path against the fake
 * kernel (no Python, R or Julia on the machine).
 */

//! Whole-flow tests for the execution-record epic, Jupyter only.
//!
//! The fake kernel is a lib-internal seam: the kernelspec's argv re-runs this crate's
//! unit-test binary (see `fake_kernel.rs`), so these tests live in the lib rather than
//! under `tests/integration/`. They compose the pieces a render runs after the engine
//! stage: execute (record built) → coalesce → write → read → present, and the record
//! staleness check, with no pipeline stage in front of them. Stage-level behavior (the
//! record cache, overlay, writer gate) is pinned by EXR-14 and EXR-15 in
//! `tests/integration/`.
//!
//! The unit-level counterparts these build on are in `span_tests` (IEE values, errors,
//! locations, fences, comments) and `record_tests` (record shape, ids, trace replay);
//! each test here names the rule it adds beyond those.

use std::collections::BTreeMap;

use serde_json::Value;

use super::record_tests::{Keep, cell, py, staged, stdout};
use super::span_tests::{ctx_for, jupyter_meta};
use super::*;
use crate::engine::jupyter::fake_kernel::{FakeKernel, FakeKernelHome, FakeOutput, Script};
use crate::engine::units::{Unit, scan_units};
use crate::presenter::document::present_document;
use crate::presenter::figures::FsFigureSink;
use crate::presenter::inline::InlineTarget;
use crate::record::{
    AuthorTexts, Record, UnitKind, coalesce, fill_ids, read_record, record_is_stale, write_record,
};

/// A cell, a prose paragraph with a span, and a second cell.
const DOC: &str =
    "Intro.\n\n```{python}\na\n```\n\nValue `{python} e` here.\n\n```{python}\nb\n```\n";

fn doc_script() -> Script {
    Script::from([
        (cell("a"), stdout("A\n")),
        (py("e"), vec![FakeOutput::plain_result("E")]),
        (cell("b"), stdout("B\n")),
    ])
}

/// Run `text` against `script` and coalesce the engine's record into the document record.
fn record_run(text: &str, script: &Script) -> (String, Record, Value, Keep) {
    let (keep, ctx) = staged(text, script);
    let result = execute_qmd(text, &ctx).expect("execution succeeds");
    let engine_record = result.record.clone().expect("a record is built");
    let record = coalesce(std::slice::from_ref(&engine_record), &ctx.units).unwrap();
    let metadata = Value::Object(ctx.document_metadata_json().into_iter().collect());
    (result.markdown, record, metadata, keep)
}

/// Present `record` over `text` the way the replay path does, from nothing but the record.
fn present_from(record: &Record, text: &str, dir: &std::path::Path) -> String {
    let mut units = scan_units(text);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    let mut fig = FsFigureSink::new(&dir.join("doc.qmd"));
    present_document(
        &record.cells,
        text,
        &units,
        None,
        InlineTarget::from_format("html"),
        &mut fig,
    )
    .expect("presents")
    .markdown
}

fn metadata_map(v: &Value) -> BTreeMap<String, Value> {
    v.as_object().unwrap().clone().into_iter().collect()
}

// ── the whole flow ───────────────────────────────────────────────────────

/// Execute, record, save the record file, drop the kernel's world, read the file back and
/// present it: the markdown equals the live run's. The record holds a unit for each
/// executed cell and span, in document order, with ids.
#[test]
fn e2e_record_then_present_flow_without_a_kernel() {
    let (live, record, _meta, keep) = record_run(DOC, &doc_script());

    let kinds: Vec<UnitKind> = record
        .cells
        .iter()
        .map(|c| c.metadata.quarto_kind)
        .collect();
    assert_eq!(kinds, [UnitKind::Cell, UnitKind::Inline, UnitKind::Cell]);
    assert!(record.cells.iter().all(|c| !c.id.is_empty()));
    assert_eq!(record.cells[1].source.concat(), "e");

    // The kernelspec directory goes away with the home: nothing below can start a kernel.
    let file = write_record(&record).unwrap();
    drop(keep);
    // `staged` leaves JUPYTER_PATH pointing at the removed directory.
    let restored = read_record(&file).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let presented = present_from(&restored, DOC, dir.path());

    assert_eq!(presented, live);
    assert!(presented.contains("Value E here."), "{presented}");
    assert!(
        presented.contains("A\n") && presented.contains("B\n"),
        "{presented}"
    );
}

/// Recording the same document twice with the same scripted kernel gives byte-identical
/// record files (the committable-file requirement).
#[test]
fn e2e_record_files_are_deterministic() {
    let (_, first, _, _k1) = record_run(DOC, &doc_script());
    let (_, second, _, _k2) = record_run(DOC, &doc_script());
    assert_eq!(
        write_record(&first).unwrap(),
        write_record(&second).unwrap()
    );
}

// ── figures ──────────────────────────────────────────────────────────────

/// A 1x1 PNG, base64 without the trailing `=` padding, as Ark emits plots.
const UNPADDED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg";

/// A kernel whose figure arrives as unpadded base64 still yields a figure file and an
/// `<img>` reference (bd-yt2o2ovt: on Ark every plot was silently dropped before).
#[test]
fn e2e_unpadded_base64_figure_is_written_and_referenced() {
    let text = "```{python}\nplot\n```\n";
    let script = Script::from([(
        cell("plot"),
        vec![FakeOutput::Display(
            serde_json::json!({ "image/png": UNPADDED_PNG, "text/plain": "<Figure>" }),
        )],
    )]);
    let (keep, ctx) = staged(text, &script);
    let result = execute_qmd(text, &ctx).expect("execution succeeds");

    let rel = "doc_files/figure-html/cell-1-output-1.png";
    assert!(result.markdown.contains(rel), "{}", result.markdown);
    let file = ctx.source_path.parent().unwrap().join(rel);
    assert!(file.is_file(), "figure file missing: {}", file.display());
    assert_eq!(&std::fs::read(&file).unwrap()[..4], b"\x89PNG");
    drop(keep);
}

// ── IEE policy beyond the unit tests ────────────────────────────────────

/// A statement as a span: a real kernel answers with a `SyntaxError` (the span is
/// evaluated as an expression, as in q1). The render fails, the error carries the code
/// `Q-18-10`, names `file:line:col` and the kernel's own error.
#[test]
fn e2e_a_statement_span_fails_with_q_18_10_and_the_kernels_syntax_error() {
    let script = Script::from([(
        py("y = 5"),
        vec![FakeOutput::Error {
            ename: "SyntaxError".to_string(),
            evalue: "invalid syntax".to_string(),
        }],
    )]);
    let (_keep, ctx) = staged("one\ntwo `{python} y = 5`\n", &script);
    let err = execute_qmd("one\ntwo `{python} y = 5`\n", &ctx)
        .expect_err("a statement span is fatal")
        .to_string();
    assert!(err.contains("SyntaxError"), "{err}");
    assert!(err.contains("doc.qmd:2:"), "{err}");
    assert!(
        err.contains(crate::engine::diagnostics::INLINE_SPAN_CODE),
        "{err}"
    );
}

// ── staleness (epic decision 8) ─────────────────────────────────────────

/// A prose edit leaves the record fresh; a code edit, a `#|` line and a span each make it
/// stale. (The check is the engine-free one the preview and hub run.)
#[test]
fn e2e_staleness_prose_keeps_the_record_fresh_code_options_and_spans_do_not() {
    let (_, record, meta, _keep) = record_run(DOC, &doc_script());
    let meta = metadata_map(&meta);
    let stale = |text: &str| record_is_stale(&record, text, &[], &meta).unwrap();

    assert!(!stale(DOC), "the unedited document is fresh");
    assert!(
        !stale(&DOC.replace("Intro.", "A completely different introduction.")),
        "a prose edit keeps the record fresh"
    );
    assert!(
        stale(&DOC.replace("\na\n", "\na2\n")),
        "a code edit is stale"
    );
    assert!(
        stale(&DOC.replace("\nb\n", "\n#| eval: false\nb\n")),
        "an execution-relevant `#|` line is stale"
    );
    assert!(
        stale(&DOC.replace("`{python} e`", "`{python} e + 1`")),
        "a span edit is stale"
    );
}

// ── where a span is evaluated (epic decision 41) ────────────────────────

/// Run `text` with `e` answering `E` and return the markdown and the record's unit sources.
fn spans_run(text: &str) -> (String, Vec<String>) {
    let script = Script::from([(py("e"), vec![FakeOutput::plain_result("E")])]);
    let (_keep, ctx) = staged(text, &script);
    let result = execute_qmd(text, &ctx).expect("execution succeeds");
    let record = result.record.expect("a record");
    let sources = record.cells.iter().map(|c| c.source.concat()).collect();
    (result.markdown, sources)
}

/// A span in the document's own front matter is evaluated, and the record holds it.
#[test]
fn e2e_span_in_front_matter_is_evaluated() {
    let (out, units) = spans_run("---\ntitle: \"T `{python} e`\"\n---\n\nbody\n");
    assert!(out.contains("title: \"T E\""), "{out}");
    assert_eq!(units, ["e"]);
}

/// A span in a value the document does not own (`_quarto.yml` reaches the document as a
/// masked `q2-nested-executable` span) is left as written, takes no unit, and does not
/// take a span number: the next span keeps number 0.
#[test]
fn e2e_span_in_project_metadata_stays_literal() {
    let masked = "`q2-nested-executable {python} e`";
    let text = format!("---\ntitle: \"T {masked}\"\n---\n\nbody `{{python}} e`\n");
    let (out, units) = spans_run(&text);
    assert!(out.contains(masked), "{out}");
    assert!(out.contains("body E"), "{out}");
    assert_eq!(units, ["e"], "only the document's own span is a unit");
    let numbers: Vec<usize> = scan_units(&text)
        .iter()
        .filter_map(|u| match u {
            Unit::Inline(i) => Some(i.span_index),
            _ => None,
        })
        .collect();
    assert_eq!(numbers, [0], "the masked span takes no number");
}

/// A span in an HTML comment is left as written, takes no unit, and does not shift the
/// numbering of the next span.
#[test]
fn e2e_span_in_html_comment_is_literal() {
    let (out, units) = spans_run("<!-- `{python} e` -->\n\nafter `{python} e`\n");
    assert!(out.contains("<!-- `{python} e` -->"), "{out}");
    assert!(out.contains("after E"), "{out}");
    assert_eq!(units, ["e"], "only the span outside the comment is a unit");
}

/// A `{python}` span inside another engine's executed fence, and inside a display fence,
/// is left as written and takes no unit.
#[test]
fn e2e_span_in_braced_fence_is_literal() {
    let text =
        "```{r}\nx <- \"`{python} e`\"\n```\n\n```text\n`{python} e`\n```\n\nout `{python} e`\n";
    let (out, units) = spans_run(text);
    assert!(out.contains("x <- \"`{python} e`\""), "{out}");
    assert!(out.contains("```text\n`{python} e`\n```"), "{out}");
    assert!(out.contains("out E"), "{out}");
    assert_eq!(units, ["e"]);
}

/// A span in a language no engine owns stays as written, the document renders, and the
/// record has no unit for it.
#[test]
fn e2e_span_no_engine_owns_stays_literal() {
    let (out, units) = spans_run("keep `{zzz} q` and run `{python} e`\n");
    assert!(out.contains("keep `{zzz} q`"), "{out}");
    assert!(out.contains("run E"), "{out}");
    assert_eq!(units, ["e"]);
}

// ── span-only documents (epic decisions 11 and 42) ──────────────────────

/// With the kernel named by the document's own front matter, a cell-less document's spans
/// run (the kernel's language owns them).
#[test]
fn e2e_span_only_document_with_jupyter_key_runs() {
    let text = "---\njupyter: fake-py\n---\n\nValue `{python} e`.\n";
    let script = Script::from([(py("e"), vec![FakeOutput::plain_result("E")])]);
    let home = FakeKernelHome::new();
    let FakeKernel { name, env } = home.install("fake-py", "python", &script);
    let mut units = scan_units(text);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    let ctx = ctx_for(text)
        .with_metadata(jupyter_meta(&name))
        .with_project_env(env)
        .with_units(units)
        .with_author_texts(texts)
        .with_want_record(true);
    let result = execute_qmd(text, &ctx).unwrap();
    assert!(result.markdown.contains("Value E."), "{}", result.markdown);
    assert_eq!(result.record.unwrap().cells.len(), 1);
}
