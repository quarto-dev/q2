//! OPR-0a: pinning tests for the two presenter loops.
//!
//! Render drives `present_document`; preview, the hub and a supplied record drive the splice
//! (`splice_record`). Both end in the same cell renderers, but the loops around them differ.
//! Each test here runs **one record** through both loops and pins what each does today, so
//! OPR-1a and OPR-1b can show they changed only what the epic decided. A comment tagged with the plan id and a
//! colon (OPR-1a or OPR-1b) marks an expectation that plan will deliberately change; the plan file
//! `claude-notes/plans/2026-11-08-opr-0a-pinning-tests.md` has the table.

use super::*;
use crate::presenter::document::{PresentError, PresentNote, PresentResult, present_document};
use crate::presenter::figures::FsFigureSink;
use crate::presenter::knitr_inputs::{OUTPUT_KINDS_KEY, RESOLVED_OPTIONS_KEY};

/// What the two loops did with one record.
struct Run {
    /// `present_document` (render).
    render: Result<PresentResult, PresentError>,
    /// The splice (preview, hub, supplied record), as the document's text afterwards.
    splice: String,
    splice_report: SpliceReport,
}

/// Build a record over `source` with `results` (unit index to outputs), the way the writer
/// does, owning `python` and `r`.
fn record_for(source: &str, results: &[(usize, Vec<Value>)]) -> Record {
    let doc = parse(source);
    let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
    let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
    let def = HashDefinition::conservative("jupyter");
    let mut builder = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
    for (i, outputs) in results {
        builder
            .push(
                *i,
                UnitResult {
                    outputs: outputs.clone(),
                    execution_count: Some(1),
                    kernel: None,
                },
            )
            .unwrap();
    }
    let entry = builder
        .finish(EngineEntryInput {
            owned_languages: vec!["python".into(), "r".into()],
            ..Default::default()
        })
        .unwrap();
    coalesce(&[entry], &units).unwrap()
}

/// Run `record` over `source` through both loops, for output format `html`.
fn run_both(source: &str, record: &Record) -> Run {
    let doc = parse(source);
    let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
    let (units, _texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
    let scope = merge_execute_scope(
        ctx().format.identifier,
        doc.ast.meta.get("execute").cloned(),
    );
    let dir = tempfile::tempdir().unwrap();
    let mut fig = FsFigureSink::new(&dir.path().join("test.qmd"));
    let render = present_document(
        &record.cells,
        &qmd,
        &units,
        scope.as_ref(),
        InlineTarget::from_format("html"),
        "html",
        &record.metadata.quarto_engines,
        &mut fig,
    );
    let (spliced, splice_report) = splice_record(parse(source), record, &mut ctx(), dir.path());
    Run {
        render,
        splice: text_of(&spliced),
        splice_report,
    }
}

fn stdout_out(text: &str) -> Value {
    json!({"output_type": "stream", "name": "stdout", "text": [text]})
}

fn stderr_out(text: &str) -> Value {
    json!({"output_type": "stream", "name": "stderr", "text": [text]})
}

fn value_out(text: &str) -> Value {
    json!({"output_type": "execute_result", "execution_count": 1,
           "data": {"text/plain": text}, "metadata": {}})
}

fn display_out(text: &str) -> Value {
    json!({"output_type": "display_data", "data": {"text/plain": text}, "metadata": {}})
}

fn png_only() -> Value {
    json!({"output_type": "display_data", "data": {"image/png": PNG_B64}, "metadata": {}})
}

fn error_out(evalue: &str) -> Value {
    json!({"output_type": "error", "ename": "ValueError", "evalue": evalue, "traceback": []})
}

const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

fn markdown_cell(markdown: &str, format: &str) -> Value {
    json!({"output_type": "display_data",
           "data": {"text/x-quarto-markdown": markdown},
           "metadata": {"quarto_format": format}})
}

fn rendered(run: &Run) -> &str {
    &run.render.as_ref().expect("render presents").markdown
}

/// The one-cell Jupyter document used by the scope and option tests.
const ONE_CELL: &str = "```{python}\n1\n```\n";

// ---- Row: unknown `#|` options on a Jupyter cell ----------------------------------------

#[test]
fn unknown_cell_option_is_forwarded_by_render_and_by_the_splice() {
    let src = "```{python}\n#| data-testid: hero-plot\n1\n```\n";
    let run = run_both(src, &record_for(src, &[(0, vec![stdout_out("hi\n")])]));
    assert!(
        rendered(&run).starts_with("::: {.cell data-testid='hero-plot'}\n"),
        "{}",
        rendered(&run)
    );
    // OPR-1a (done): the splice forwards unknown options too (D9), as render does.
    assert!(
        run.splice
            .starts_with("::: {.cell data-testid=\"hero-plot\"}\n"),
        "{}",
        run.splice
    );
    assert_eq!(run.splice_report.units[0].1, UnitStaleness::Fresh);
}

// ---- Row: errors in cell options ---------------------------------------------------------

#[test]
fn invalid_cell_options_fail_render_and_leave_the_source_in_the_splice() {
    // The record builder rejects invalid options, so build the record from a valid twin and
    // re-key its cell to the invalid source's id and content hash (the fallback match).
    let valid = "```{python}\n#| echo: false\n1\n```\n";
    let src = "```{python}\n#| echo: [unclosed\n1\n```\n";
    let mut record = record_for(valid, &[(0, vec![stdout_out("hi\n")])]);
    let (qmd, _) = serialize_ast_to_qmd(&parse(src).ast).unwrap();
    let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
    record.cells[0].metadata.quarto_cell_id = unit_id(&units[0]);
    record.cells[0].metadata.quarto_content_hash = texts.0[0].content_hash.clone();
    let run = run_both(src, &record);
    assert!(
        matches!(
            run.render,
            Err(PresentError::InvalidCellOptions { unit: 0, .. })
        ),
        "{:?}",
        run.render
    );
    // The splice swallows the error and keeps the cell as source.
    assert!(
        run.splice.contains("{python}") && run.splice.contains("unclosed"),
        "{}",
        run.splice
    );
    assert!(!run.splice.contains("cell-output"), "{}", run.splice);
}

// ---- Row: stored markdown for another format ----------------------------------------------

#[test]
fn stored_markdown_for_another_format_is_noted_by_render_and_silent_in_the_splice() {
    let src = "```{r}\n1\n```\n";
    let mismatch = record_for(src, &[(0, vec![markdown_cell("STORED\n", "pdf")])]);
    let run = run_both(src, &mismatch);
    let result = run.render.as_ref().unwrap();
    assert_eq!(result.markdown, "```{r}\n1\n```\n");
    assert_eq!(
        result.notes,
        [PresentNote::MarkdownFormatMismatch {
            unit: 0,
            stored: Some("pdf".to_string()),
            wanted: "html".to_string()
        }]
    );
    // The splice has no notes channel today (no epic decision changes that); it skips silently. The text is the
    // same on both sides: the source stays.
    assert!(
        run.splice.contains("{r}") && !run.splice.contains("STORED"),
        "{}",
        run.splice
    );

    // For the matching format both loops place the stored markdown.
    let matching = record_for(src, &[(0, vec![markdown_cell("STORED\n", "html")])]);
    let run = run_both(src, &matching);
    assert_eq!(rendered(&run), "STORED\n");
    assert!(
        run.splice.contains("STORED") && !run.splice.contains("{r}"),
        "{}",
        run.splice
    );
}

// ---- Rows: span value choice and front-matter spans ---------------------------------------

const SPAN: &str = "Value `{python} x` here.\n";

/// What a loop did with the span: `Some(text)` the value it wrote, `None` the source kept.
fn span_value_render(run: &Run) -> Option<String> {
    let md = rendered(run);
    if md.contains("`{python} x`") {
        return None;
    }
    Some(
        md.strip_prefix("Value ")?
            .strip_suffix(" here.\n")?
            .to_string(),
    )
}

fn span_value_splice(run: &Run) -> Option<String> {
    if run.splice.contains("`{python} x`") {
        return None;
    }
    Some(
        run.splice
            .strip_prefix("Value ")?
            .trim_end()
            .strip_suffix(" here.")?
            .to_string(),
    )
}

#[test]
fn span_cases_through_both_routines() {
    type Row = (
        &'static str,
        Vec<Value>,
        Option<&'static str>,
        Option<&'static str>,
    );
    let rows: Vec<Row> = vec![
        // (case, outputs, render's value, the splice's value); None keeps the span source.
        (
            "one text value",
            vec![value_out("42")],
            Some("42"),
            Some("42"),
        ),
        (
            "stdout then a value",
            vec![stdout_out("noise\n"), value_out("42")],
            Some("42"),
            Some("42"),
        ),
        // OPR-1b (done, D9): both surfaces use `classify_inline_result`: the last text-bearing
        // output wins, an error wins over everything, an image-only bundle is skipped.
        (
            "two text values: last versus first",
            vec![value_out("1"), display_out("2")],
            Some("2"),
            Some("2"),
        ),
        (
            "an error then a value",
            vec![error_out("boom"), value_out("7")],
            None,
            None,
        ),
        (
            "an image-only bundle then a text value",
            vec![png_only(), value_out("5")],
            Some("5"),
            Some("5"),
        ),
        ("an empty result", vec![], None, None),
    ];
    let mut problems = Vec::new();
    for (case, outputs, want_render, want_splice) in rows {
        let run = run_both(SPAN, &record_for(SPAN, &[(0, outputs)]));
        let (got_render, got_splice) = (span_value_render(&run), span_value_splice(&run));
        if got_render.as_deref() != want_render || got_splice.as_deref() != want_splice {
            problems.push(format!(
                "{case}: render {got_render:?} (want {want_render:?}), splice {got_splice:?} (want {want_splice:?})\n  render: {:?}\n  splice: {:?}",
                run.render, run.splice
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn render_notes_a_span_with_no_usable_value_and_the_splice_has_no_channel_for_it() {
    for outputs in [vec![], vec![error_out("boom")], vec![png_only()]] {
        let run = run_both(SPAN, &record_for(SPAN, &[(0, outputs)]));
        assert_eq!(
            run.render.as_ref().unwrap().notes,
            [PresentNote::InlineNoUsableValue { unit: 0 }]
        );
    }
}

const FRONT_MATTER_SPAN: &str = "---\ntitle: \"Report `{python} x`\"\n---\n\nBody.\n";

#[test]
fn a_front_matter_span_is_presented_by_render_and_skipped_by_the_splice() {
    let run = run_both(
        FRONT_MATTER_SPAN,
        &record_for(FRONT_MATTER_SPAN, &[(0, vec![value_out("'say \"hi\"'")])]),
    );
    // Render writes the value, YAML-escaped.
    assert_eq!(
        rendered(&run),
        "---\ntitle: \"Report say \\\"hi\\\"\"\n---\n\nBody.\n"
    );
    // The splice skips a front-matter span (no epic decision changes that).
    assert!(run.splice.contains("`{python} x`"), "{}", run.splice);
}

#[test]
fn a_newline_in_a_front_matter_span_value_fails_render_and_the_splice_skips_it() {
    let run = run_both(
        FRONT_MATTER_SPAN,
        &record_for(FRONT_MATTER_SPAN, &[(0, vec![value_out("two\nlines")])]),
    );
    assert!(
        matches!(&run.render, Err(PresentError::FrontMatterValue { unit: 0, key, .. }) if key == "title"),
        "{:?}",
        run.render
    );
    assert!(run.splice.contains("`{python} x`"), "{}", run.splice);
}

// ---- Row: which engine entry a cell sees ---------------------------------------------------

const MIXED_DOC: &str = "```{python}\n1\n```\n\n```{r}\n#| bar.unknown: 2\n1\n```\n";

/// A record with a Jupyter cell and a structured knitr cell, whose engine entries differ:
/// knitr's `known_options` is `["echo"]`, Jupyter's is none. Returns the record and the
/// two entries.
fn mixed_engine_record() -> Record {
    let doc = parse(MIXED_DOC);
    let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
    let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
    let build = |name: &str, lang: &str, known: Option<Vec<String>>, index: usize| {
        let def = HashDefinition::conservative(name);
        let mut b = RecordBuilder::new(name, &def, &units, &texts).unwrap();
        b.push(
            index,
            UnitResult {
                outputs: vec![stdout_out("[1] 1")],
                execution_count: None,
                kernel: None,
            },
        )
        .unwrap();
        b.finish(EngineEntryInput {
            owned_languages: vec![lang.into()],
            known_options: known,
            ..Default::default()
        })
        .unwrap()
    };
    let py = build("jupyter", "python", None, 0);
    let mut r = build("knitr", "r", Some(vec!["echo".to_string()]), 1);
    for cell in &mut r.cells {
        cell.metadata
            .extra
            .insert(RESOLVED_OPTIONS_KEY.into(), json!({}));
        cell.metadata
            .extra
            .insert(OUTPUT_KINDS_KEY.into(), json!([null]));
    }
    coalesce(&[py, r], &units).unwrap()
}

#[test]
fn the_engine_entry_a_knitr_cell_sees_is_looked_up_per_cell_in_both_loops() {
    let record = mixed_engine_record();
    let jupyter = record.metadata.quarto_engines[0].clone();
    let knitr = record.metadata.quarto_engines[1].clone();
    assert_eq!(
        (jupyter.name.as_str(), knitr.name.as_str()),
        ("jupyter", "knitr")
    );

    // Both loops look the entry up per cell (`quarto_engine`): knitr's `known_options` apply,
    // so `bar.unknown` is not known and is forwarded as a cell attribute.
    let via_knitr = run_both(MIXED_DOC, &record);
    assert!(
        via_knitr.splice.contains("bar.unknown"),
        "{}",
        via_knitr.splice
    );
    // Render gets one entry for the whole document: handed knitr's, it forwards too...
    assert!(
        rendered(&via_knitr).contains("bar.unknown='2'"),
        "{}",
        rendered(&via_knitr)
    );
    // ...and handed another engine's, it forwards too (OPR-1a, done): the entry is looked up
    // per cell, by the cell's `quarto_engine`, so the one handed in does not matter.
    let via_jupyter = run_both(MIXED_DOC, &record);
    assert!(
        rendered(&via_jupyter).contains("bar.unknown"),
        "{}",
        rendered(&via_jupyter)
    );
    assert!(
        via_jupyter.splice.contains("bar.unknown"),
        "{}",
        via_jupyter.splice
    );
}

// ---- Document `execute:` scope (D3) --------------------------------------------------------

fn scoped(key: &str) -> String {
    format!("---\nexecute:\n  {key}: false\n---\n\n{ONE_CELL}")
}

fn scope_outputs() -> Vec<Value> {
    vec![stdout_out("OUT-TEXT\n"), stderr_out("ERR-TEXT\n")]
}

#[test]
fn a_jupyter_cell_with_no_resolved_options_is_layered_with_the_document_scope() {
    // D3: a cell with no resolved options gets the document scope layered at present time.
    // What each loop emits for each scope key, today (the bytes after the front matter).
    for (key, shows_code, shows_stdout, shows_stderr, shows_cell) in [
        ("echo", false, true, true, true),
        ("output", true, false, false, true),
        ("include", false, false, false, false),
        ("warning", true, true, false, true),
    ] {
        let src = scoped(key);
        let run = run_both(&src, &record_for(&src, &[(0, scope_outputs())]));
        for (loop_name, text) in [("render", rendered(&run)), ("splice", run.splice.as_str())] {
            let body = text.split("---\n").last().unwrap_or(text);
            let seen = (
                body.contains("cell-code"),
                body.contains("OUT-TEXT"),
                body.contains("ERR-TEXT"),
                body.contains("::: {.cell"),
            );
            assert_eq!(
                seen,
                (shows_code, shows_stdout, shows_stderr, shows_cell),
                "{loop_name} under execute: {{{key}: false}}:\n{text}"
            );
        }
    }
}
