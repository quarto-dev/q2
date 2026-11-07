//! `place`: substitute stored rendered markdown at its units (EXR-4 Phase 5).
//!
//! The shared helper for engines whose record cell holds finished markdown (marimo, knitr):
//! a cell holding exactly one `display_data` output with `text/x-quarto-markdown` replaces
//! its unit in the document text. Anything else stays verbatim, never an error:
//!
//! * a unit with no record cell (new, edited or stale: its id carries the content hash);
//! * a cell that is not of that shape (raw units: Jupyter's outputs are presented, not placed);
//! * a front-matter span (`quarto_location: metadata`, or the scanner's `in_front_matter`);
//! * a stored `quarto_format` that differs from the requested format, which is also reported.

use std::collections::HashMap;

use serde_json::Value;

use crate::engine::units::Unit;

use super::Record;
use super::types::{RecordCell, UnitLocation};

/// The mime type of finished markdown stored in a record cell.
pub const QUARTO_MARKDOWN_MIME: &str = "text/x-quarto-markdown";

/// Something `place` left verbatim that the caller should surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceNote {
    /// The stored markdown was produced for another output format (or recorded none).
    FormatMismatch {
        cell_id: String,
        stored: Option<String>,
        wanted: String,
    },
}

/// The document text with stored markdown substituted, and what was left alone and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub text: String,
    pub notes: Vec<PlaceNote>,
}

/// Substitute each unit's stored markdown for `format` into `document_text`. `units` must be
/// the scan of `document_text` with ids filled (a unit without an id has no cell to match).
pub fn place(record: &Record, units: &[Unit], document_text: &str, format: &str) -> Placed {
    let cells: HashMap<&str, &RecordCell> = record
        .cells
        .iter()
        .map(|c| (c.metadata.quarto_cell_id.as_str(), c))
        .collect();

    let mut notes = Vec::new();
    // (start, end, replacement), in document order.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for unit in units {
        if matches!(unit, Unit::Inline(i) if i.in_front_matter) {
            continue;
        }
        let Some(id) = unit_id(unit) else { continue };
        let Some(cell) = cells.get(id).copied() else {
            continue;
        };
        if cell.metadata.quarto_location == Some(UnitLocation::Metadata) {
            continue;
        }
        let Some((markdown, stored_format)) = stored_markdown(cell) else {
            continue;
        };
        if stored_format.as_deref() != Some(format) {
            notes.push(PlaceNote::FormatMismatch {
                cell_id: id.to_string(),
                stored: stored_format,
                wanted: format.to_string(),
            });
            continue;
        }
        edits.push(placed_replacement(cell, unit, document_text, &markdown));
    }

    let mut text = document_text.to_string();
    // Back to front, so earlier offsets stay valid.
    edits.sort_by_key(|e| e.0);
    for (start, end, replacement) in edits.into_iter().rev() {
        text.replace_range(start..end, &replacement);
    }
    Placed { text, notes }
}

fn unit_id(unit: &Unit) -> Option<&str> {
    match unit {
        Unit::Cell(c) => c.id.as_ref(),
        Unit::Inline(i) => i.id.as_ref(),
    }
    .map(|id| id.0.as_str())
}

/// The one `display_data` markdown output of a cell and the format it was produced for.
pub(crate) fn stored_markdown(cell: &RecordCell) -> Option<(String, Option<String>)> {
    let [output] = cell.outputs.as_slice() else {
        return None;
    };
    if output.get("output_type").and_then(Value::as_str) != Some("display_data") {
        return None;
    }
    let markdown = match output.get("data")?.get(QUARTO_MARKDOWN_MIME)? {
        Value::String(s) => s.clone(),
        // nbformat allows a multiline string as an array of lines.
        Value::Array(lines) => lines
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()?
            .concat(),
        _ => return None,
    };
    let format = output
        .get("metadata")
        .and_then(|m| m.get("quarto_format"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some((markdown, format))
}

/// Metadata key on a stored-markdown output: the markdown is inserted exactly as stored, even
/// for an inline unit (whose trailing newlines are otherwise trimmed). knitr sets it on a span,
/// whose live value is inserted verbatim (KNR-4).
pub const VERBATIM_KEY: &str = "quarto_verbatim";

/// [`replacement`], except that an inline unit whose stored output is marked verbatim keeps
/// every character of the stored markdown.
pub(crate) fn placed_replacement(
    cell: &RecordCell,
    unit: &Unit,
    text: &str,
    markdown: &str,
) -> (usize, usize, String) {
    let verbatim = cell
        .outputs
        .first()
        .and_then(|o| o.get("metadata")?.get(VERBATIM_KEY)?.as_bool())
        .unwrap_or(false);
    match unit {
        Unit::Inline(i) if verbatim => (i.start, i.end, markdown.to_string()),
        _ => replacement(unit, text, markdown),
    }
}

/// The edit for one unit. A fenced cell keeps its trailing newline and carries its blockquote
/// prefix and indentation onto the continuation lines, so the substitution stays in its container.
pub(crate) fn replacement(unit: &Unit, text: &str, markdown: &str) -> (usize, usize, String) {
    match unit {
        Unit::Inline(i) => (i.start, i.end, markdown.trim_end_matches('\n').to_string()),
        Unit::Cell(c) => {
            let original = &text[c.start..c.end];
            let mut body = markdown.trim_end_matches('\n').to_string();
            let line_prefix = format!("{}{}", c.container_prefix, " ".repeat(c.indent));
            if !line_prefix.is_empty() {
                let blank_prefix = c.container_prefix.trim_end();
                body = body
                    .split('\n')
                    .enumerate()
                    .map(|(n, line)| match (n, line.is_empty()) {
                        (0, _) => line.to_string(),
                        (_, true) => blank_prefix.to_string(),
                        _ => format!("{line_prefix}{line}"),
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            if original.ends_with('\n') {
                body.push('\n');
            }
            (c.start, c.end, body)
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::engine::units::scan_units;
    use crate::record::{
        AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, coalesce,
        fill_ids,
    };

    fn markdown_output(md: &str, format: &str) -> Value {
        json!({
            "output_type": "display_data",
            "data": {QUARTO_MARKDOWN_MIME: md},
            "metadata": {"quarto_format": format}
        })
    }

    /// Scan `doc` and build one engine's record with `outputs_for(unit_index, unit)` for every
    /// unit that returns `Some`.
    fn setup(
        doc: &str,
        engine: &str,
        outputs_for: impl Fn(usize) -> Option<Vec<Value>>,
    ) -> (Record, Vec<Unit>) {
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        let def = HashDefinition::conservative(engine);
        let mut b = RecordBuilder::new(engine, &def, &units, &texts).unwrap();
        for i in 0..units.len() {
            if let Some(outputs) = outputs_for(i) {
                b.push(
                    i,
                    UnitResult {
                        outputs,
                        ..Default::default()
                    },
                )
                .unwrap();
            }
        }
        let entry = b.finish(EngineEntryInput::default()).unwrap();
        (coalesce(&[entry], &units).unwrap(), units)
    }

    const MARIMO_DOC: &str =
        "# Heading\n\n```{python .marimo}\nimport marimo as mo\n40 + 2\n```\n\nAfter.\n";

    #[test]
    fn a_marimo_shaped_record_places_its_markdown() {
        let island = "```{=html}\n<marimo-island>42</marimo-island>\n```\n";
        let (rec, units) = setup(MARIMO_DOC, "marimo", |_| {
            Some(vec![markdown_output(island, "html")])
        });
        let placed = place(&rec, &units, MARIMO_DOC, "html");
        assert_eq!(
            placed.text,
            format!("# Heading\n\n{island}\nAfter.\n"),
            "the fenced source is replaced and the surrounding prose is untouched"
        );
        assert!(placed.notes.is_empty());
    }

    #[test]
    fn a_unit_with_no_cell_stays_verbatim() {
        let (rec, units) = setup(MARIMO_DOC, "marimo", |_| None);
        let placed = place(&rec, &units, MARIMO_DOC, "html");
        assert_eq!(placed.text, MARIMO_DOC);
        assert!(placed.notes.is_empty());

        // An edited cell has a different content hash, so its id matches nothing.
        let (rec, _) = setup(MARIMO_DOC, "marimo", |_| {
            Some(vec![markdown_output("X\n", "html")])
        });
        let edited = MARIMO_DOC.replace("40 + 2", "41 + 2");
        let mut units = scan_units(&edited);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        assert_eq!(place(&rec, &units, &edited, "html").text, edited);
    }

    #[test]
    fn a_format_mismatch_leaves_the_unit_verbatim_and_is_reported() {
        let (rec, units) = setup(MARIMO_DOC, "marimo", |_| {
            Some(vec![markdown_output("X\n", "typst")])
        });
        let placed = place(&rec, &units, MARIMO_DOC, "html");
        assert_eq!(placed.text, MARIMO_DOC);
        assert_eq!(placed.notes.len(), 1);
        assert!(matches!(
            &placed.notes[0],
            PlaceNote::FormatMismatch { stored: Some(s), wanted, .. } if s == "typst" && wanted == "html"
        ));
        // A rendered output with no recorded format is also not trusted.
        let (rec, units) = setup(MARIMO_DOC, "marimo", |_| {
            Some(vec![json!({
                "output_type": "display_data",
                "data": {QUARTO_MARKDOWN_MIME: "X\n"},
                "metadata": {}
            })])
        });
        let placed = place(&rec, &units, MARIMO_DOC, "html");
        assert_eq!(placed.text, MARIMO_DOC);
        assert!(matches!(
            &placed.notes[0],
            PlaceNote::FormatMismatch { stored: None, .. }
        ));
    }

    #[test]
    fn a_front_matter_span_is_never_placed() {
        let doc = "---\ntitle: \"`{r} 1`\"\n---\n\nBody `{r} 2` here.\n";
        let (rec, units) = setup(doc, "knitr", |_| {
            Some(vec![markdown_output("VALUE\n", "html")])
        });
        let front = rec
            .cells
            .iter()
            .find(|c| c.metadata.quarto_location == Some(UnitLocation::Metadata))
            .expect("the record marks the front-matter span");
        assert_eq!(front.outputs.len(), 1);
        let placed = place(&rec, &units, doc, "html");
        assert_eq!(
            placed.text,
            "---\ntitle: \"`{r} 1`\"\n---\n\nBody VALUE here.\n"
        );
    }

    #[test]
    fn a_raw_cell_is_left_alone() {
        let (rec, units) = setup(MARIMO_DOC, "jupyter", |_| {
            Some(vec![
                json!({"output_type": "stream", "name": "stdout", "text": ["42\n"]}),
            ])
        });
        assert_eq!(place(&rec, &units, MARIMO_DOC, "html").text, MARIMO_DOC);
    }

    #[test]
    fn several_units_place_back_to_front_without_shifting_each_other() {
        let doc = "A `{r} 1` B\n\n```{r}\nx\n```\n\nC `{r} 2` D\n";
        let (rec, units) = setup(doc, "knitr", |i| {
            Some(vec![markdown_output(&format!("<<{i}>>\n"), "html")])
        });
        assert_eq!(
            place(&rec, &units, doc, "html").text,
            "A <<0>> B\n\n<<1>>\n\nC <<2>> D\n"
        );
    }

    #[test]
    fn a_cell_in_a_blockquote_stays_in_its_container() {
        let doc = "> ```{r}\n> x\n> ```\n";
        let (rec, units) = setup(doc, "knitr", |_| {
            Some(vec![markdown_output("one\n\ntwo\n", "html")])
        });
        assert_eq!(place(&rec, &units, doc, "html").text, "> one\n>\n> two\n");
    }
}
