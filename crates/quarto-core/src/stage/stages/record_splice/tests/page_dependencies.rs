//! OPR-7i: the page dependencies a labeled record derives, through both presenter loops.
//!
//! Render (`present_document`) and the splice (`splice_record`) must give the same head text and
//! the same body for one record, and the derived text must never be stored in the record.

use super::*;
use crate::presenter::document::{PresentNote, present_document};
use crate::presenter::figures::FsFigureSink;

fn html_out(html: &str) -> Value {
    json!({"output_type": "display_data", "data": {"text/html": html, "text/plain": "x"}, "metadata": {}})
}

const PLOTLY: &str = "<script type=\"text/javascript\">\n  require.undef(\"plotly\");\n</script>";
const SOURCE: &str = "```{python}\n1\n```\n";

/// A one-cell record whose engine entry carries the label (or not).
fn labeled(outputs: Vec<Value>, label: Option<&str>) -> Record {
    let doc = parse(SOURCE);
    let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
    let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
    let def = HashDefinition::conservative("jupyter");
    let mut builder = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
    builder
        .push(
            0,
            UnitResult {
                outputs,
                execution_count: Some(1),
                kernel: None,
            },
        )
        .unwrap();
    let entry = builder
        .finish(EngineEntryInput {
            owned_languages: vec!["python".into()],
            ..Default::default()
        })
        .unwrap();
    let mut record = coalesce(&[entry], &units).unwrap();
    record.metadata.quarto_engines[0].dependency_data = label.map(str::to_string);
    record
}

fn render_of(record: &Record, format: &str) -> crate::presenter::document::PresentResult {
    let doc = parse(SOURCE);
    let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
    let (units, _): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
    let dir = tempfile::tempdir().unwrap();
    let mut fig = FsFigureSink::new(&dir.path().join("test.qmd"));
    present_document(
        &record.cells,
        &qmd,
        &units,
        None,
        InlineTarget::from_format(format),
        format,
        &record.metadata.quarto_engines,
        &mut fig,
    )
    .unwrap()
}

/// `text` without its leading front matter block (the head text lives there after a splice).
fn body_of(text: &str) -> String {
    match text
        .strip_prefix("---\n")
        .and_then(|t| t.split_once("\n---\n"))
    {
        Some((_, body)) => body.to_string(),
        None => text.to_string(),
    }
}

fn splice_of(record: &Record) -> (String, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let (doc, _) = splice_record(parse(SOURCE), record, &mut ctx(), dir.path());
    let header = doc
        .ast
        .meta
        .get_path(&["rendered", "includes", "header"])
        .and_then(|v| v.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    (body_of(&text_of(&doc)), header)
}

#[test]
fn render_and_the_splice_derive_the_same_head_and_body() {
    let record = labeled(vec![html_out("<b>hi</b>")], Some("jupyter-outputs"));
    let render = render_of(&record, "html");
    let (body, header) = splice_of(&record);
    assert_eq!(
        render.header_includes.len(),
        1,
        "{:?}",
        render.header_includes
    );
    assert!(render.header_includes[0].contains("requirejs@2.3.6"));
    assert_eq!(header, render.header_includes);
    // The body has no head text in it, and both loops present the cell's output.
    assert!(!body.contains("requirejs"), "{body}");
    assert!(
        !render.markdown.contains("requirejs"),
        "{}",
        render.markdown
    );
    assert!(
        body.contains("hi") && render.markdown.contains("hi"),
        "{body}"
    );
}

#[test]
fn a_hoisted_library_is_in_the_head_and_not_in_either_body() {
    let record = labeled(
        vec![
            html_out(PLOTLY),
            json!({"output_type": "stream", "name": "stdout", "text": ["after\n"]}),
        ],
        Some("jupyter-outputs"),
    );
    let render = render_of(&record, "html");
    let (body, header) = splice_of(&record);
    assert!(render.header_includes[0].contains("require.undef(\"plotly\")"));
    assert_eq!(header, render.header_includes);
    for md in [render.markdown.as_str(), body.as_str()] {
        assert!(!md.contains("plotly"), "{md}");
        assert!(md.contains("after"), "{md}");
    }
}

#[test]
fn the_other_outputs_keep_their_kinds_when_an_output_is_hoisted() {
    // `quarto_output_kinds` is read by position: a hoisted first output must take its entry
    // with it, or the stderr output below is read as the hoisted one's kind.
    let mut record = labeled(
        vec![
            html_out(PLOTLY),
            json!({"output_type": "stream", "name": "stderr", "text": ["careful\n"]}),
        ],
        Some("jupyter-outputs"),
    );
    record.cells[0]
        .metadata
        .extra
        .insert("quarto_output_kinds".into(), json!([null, "warning"]));
    record.cells[0]
        .metadata
        .extra
        .insert("quarto_output_anchors".into(), json!([1, 1]));
    let hoisted = render_of(&record, "html").markdown;
    // Presented as it would be if the library output had never been there.
    let mut plain = record.clone();
    plain.cells[0].outputs.remove(0);
    plain.cells[0]
        .metadata
        .extra
        .insert("quarto_output_kinds".into(), json!(["warning"]));
    plain.cells[0]
        .metadata
        .extra
        .insert("quarto_output_anchors".into(), json!([1]));
    plain.metadata.quarto_engines[0].dependency_data = None;
    assert_eq!(hoisted, render_of(&plain, "html").markdown);
}

#[test]
fn no_label_means_nothing_derived_and_nothing_hoisted() {
    let record = labeled(vec![html_out(PLOTLY)], None);
    let render = render_of(&record, "html");
    assert!(render.header_includes.is_empty());
    assert!(render.markdown.contains("plotly"), "{}", render.markdown);
    let (body, header) = splice_of(&record);
    assert!(header.is_empty(), "{header:?}");
    assert!(body.contains("plotly"), "{body}");
}

#[test]
fn a_non_html_format_derives_nothing() {
    let record = labeled(vec![html_out("<b>hi</b>")], Some("jupyter-outputs"));
    for format in ["pdf", "typst", "docx"] {
        assert!(
            render_of(&record, format).header_includes.is_empty(),
            "{format}"
        );
    }
}

#[test]
fn derived_text_is_never_stored_in_the_record() {
    let record = labeled(vec![html_out("<b>hi</b>")], Some("jupyter-outputs"));
    let before = record.clone();
    let _ = render_of(&record, "html");
    let _ = splice_of(&record);
    assert_eq!(record, before);
    assert!(record.metadata.quarto_engines[0].includes.is_none());
}

#[test]
fn a_widget_view_warns_once_per_cell_with_no_engine_entry_at_all() {
    // Native Jupyter passes no engines; the note does not depend on the label.
    let view = json!({"output_type": "display_data",
        "data": {"application/vnd.jupyter.widget-view+json": {"model_id": "a"}, "text/plain": "w"},
        "metadata": {}});
    let mut record = labeled(vec![view.clone(), view], None);
    record.metadata.quarto_engines.clear();
    let doc = parse(SOURCE);
    let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
    let (units, _): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
    let dir = tempfile::tempdir().unwrap();
    let mut fig = FsFigureSink::new(&dir.path().join("test.qmd"));
    let out = present_document(
        &record.cells,
        &qmd,
        &units,
        None,
        InlineTarget::from_format("html"),
        "html",
        &[],
        &mut fig,
    )
    .unwrap();
    let widget: Vec<_> = out
        .notes
        .iter()
        .filter(|n| matches!(n, PresentNote::WidgetViewUnsupported { .. }))
        .collect();
    assert_eq!(widget.len(), 1, "{:?}", out.notes);
    // The text representation is what the page shows.
    assert!(out.markdown.contains('w'), "{}", out.markdown);
}

#[test]
fn plain_html_output_raises_no_widget_warning() {
    let record = labeled(vec![html_out("<b>hi</b>")], Some("jupyter-outputs"));
    let out = render_of(&record, "html");
    assert!(
        !out.notes
            .iter()
            .any(|n| matches!(n, PresentNote::WidgetViewUnsupported { .. })),
        "{:?}",
        out.notes
    );
}
