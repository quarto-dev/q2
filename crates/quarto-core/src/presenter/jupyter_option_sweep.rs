//! KNR-3 slice 7, Phase 7.1 (D10): the Jupyter option sweep.
//!
//! The knitr sweep (`record_splice/tests/option_sweep.rs`) covers knitr only. The Jupyter engine
//! declares about 30 cell options display-only (`DISPLAY_ONLY_KEYS`): the hash leaves them out,
//! so editing one re-runs nothing. That is only safe if the presenter reads each one. This sweep
//! presents a stored Jupyter cell with and without each declared option and classifies it:
//!
//! - [`Outcome::Presents`]: the markdown changes;
//! - [`Outcome::Inert`]: it does not, and `INERT_REASONS` says why that is correct;
//!
//! There is no gap outcome: an option declared display-only that changes nothing and has no
//! `INERT_REASONS` entry is a bug (an edit would show nothing until something else re-ran the
//! cell), so the sweep fails. Phase 7g removed the last of those rows.
//!
//! The reverse check lives in `options.rs`: every key the presenter reads is declared
//! display-only for Jupyter, or listed there with a reason.

use super::document::present_document;
use super::figures::FsFigureSink;
use super::inline::InlineTarget;
use crate::engine::units::scan_units;
use crate::record::{
    AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, fill_ids,
};
use serde_json::{Value, json};

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Presents,
    Inert,
}
use Outcome::*;

/// A cell with printed text, a stderr message and a figure, then a second, table-shaped output.
fn outputs() -> Vec<Value> {
    vec![
        json!({"output_type": "stream", "name": "stdout", "text": "hello"}),
        json!({"output_type": "stream", "name": "stderr", "text": "careful"}),
        json!({"output_type": "display_data", "data": {"image/png": PNG, "text/plain": "<Figure>"}, "metadata": {}}),
        json!({"output_type": "display_data", "data": {"text/markdown": "| a | b |\n|---|---|\n| 1 | 2 |\n"}, "metadata": {}}),
    ]
}

fn source_with(option: Option<(&str, &str)>) -> String {
    let header = option
        .map(|(k, v)| format!("#| {k}: {v}\n"))
        .unwrap_or_default();
    format!("```{{python}}\n{header}1 + 1\n```\n")
}

/// Present the cell `source` over a record built from the *base* source (no option), the way a
/// stored notebook replays: the record is the same, only the header differs.
fn present(source: &str) -> String {
    let base = source_with(None);
    let mut units = scan_units(&base);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    let def = HashDefinition::conservative("test");
    let mut builder = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
    builder
        .push(
            0,
            UnitResult {
                outputs: outputs(),
                execution_count: Some(1),
                kernel: Some("python3".to_string()),
            },
        )
        .unwrap();
    let cells = builder.finish(EngineEntryInput::default()).unwrap().cells;
    // The units of the edited source, carrying the base cell's id so they match its record.
    let mut edited = scan_units(source);
    edited[0] = match (&edited[0], &units[0]) {
        (crate::engine::units::Unit::Cell(e), crate::engine::units::Unit::Cell(b)) => {
            let mut e = e.clone();
            e.id = b.id.clone();
            crate::engine::units::Unit::Cell(e)
        }
        _ => unreachable!("one cell"),
    };
    let dir = tempfile::tempdir().unwrap();
    let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
    present_document(
        &cells,
        source,
        &edited,
        None,
        InlineTarget::from_format("html"),
        "html",
        &[],
        &mut fig,
    )
    .unwrap()
    .markdown
}

/// (declared display-only key, a value that differs from the default, the outcome it must have).
const SWEEP: &[(&str, &str, Outcome)] = &[
    ("echo", "false", Presents),
    ("output", "false", Presents),
    ("warning", "false", Presents),
    ("include", "false", Presents),
    ("code-fold", "true", Presents),
    ("code-summary", "\"'Show'\"", Presents),
    ("code-overflow", "wrap", Presents),
    ("code-line-numbers", "true", Presents),
    ("code-copy", "false", Presents),
    ("fig-cap", "\"'A caption'\"", Presents),
    ("fig-subcap", "\"'sub'\"", Presents),
    ("fig-scap", "\"'short'\"", Presents),
    ("fig-alt", "\"'alt text'\"", Presents),
    ("fig-align", "center", Presents),
    ("fig-link", "\"'http://x'\"", Presents),
    ("fig-cap-location", "top", Presents),
    ("tbl-cap", "\"'A table'\"", Presents),
    ("tbl-subcap", "\"'sub'\"", Presents),
    ("tbl-scap", "\"'short'\"", Presents),
    ("tbl-colwidths", "\"[30, 70]\"", Presents),
    ("tbl-cap-location", "top", Presents),
    ("lst-cap", "\"'Listing'\"", Presents),
    ("lst-label", "lst-a", Presents),
    ("cap-location", "top", Presents),
    ("layout", "\"[[1]]\"", Presents),
    ("layout-ncol", "2", Presents),
    ("layout-nrow", "2", Presents),
    ("layout-align", "center", Presents),
    ("layout-valign", "top", Presents),
    ("panel", "tabset", Presents),
    ("column", "margin", Presents),
    ("keep-hidden", "true", Presents),
    ("fig-env", "\"'figure*'\"", Presents),
    ("fig-pos", "\"'h'\"", Presents),
    ("out-width", "\"'50%'\"", Presents),
    ("out-height", "\"'50%'\"", Presents),
    ("classes", "plain", Presents),
    ("fig-column", "margin", Presents),
    ("tbl-column", "margin", Presents),
    ("out-extra", "\"'style=\\\"x\\\"'\"", Presents),
];

/// Options the presenter does not render, and why that is correct for Jupyter.
const INERT_REASONS: &[(&str, &str)] = &[];

#[test]
fn every_declared_display_only_key_is_swept() {
    let swept: Vec<&str> = SWEEP.iter().map(|(k, _, _)| *k).collect();
    let def = crate::engine::jupyter::jupyter_hash_definition();
    let missing: Vec<&String> = def
        .presentation_cell_options
        .iter()
        .filter(|k| !swept.contains(&k.as_str()))
        .collect();
    assert!(missing.is_empty(), "declared but not swept: {missing:?}");
}

#[test]
fn every_declared_display_only_key_changes_the_markdown() {
    let base = present(&source_with(None));
    let mut wrong = Vec::new();
    for (name, value, want) in SWEEP {
        let got = if present(&source_with(Some((name, value)))) == base {
            Inert
        } else {
            Presents
        };
        if got != *want {
            wrong.push(format!("{name}: {got:?}, table says {want:?}"));
        }
        if *want == Inert {
            assert!(
                INERT_REASONS.iter().any(|(n, _)| n == name),
                "{name} is inert with no reason in INERT_REASONS"
            );
        }
    }
    assert!(wrong.is_empty(), "outcomes changed:\n{}", wrong.join("\n"));
}
