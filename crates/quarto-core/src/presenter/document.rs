/*
 * presenter/document.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-6 Phase 1: the document-level presenter driver.
 */

//! [`present_document`]: the one pass that turns an execution record and the document
//! text it was recorded from into the engine's markdown.
//!
//! The driver consumes the stage's unit list (epic decision 38): each unit's byte
//! offsets locate it in `document_text`, and a unit is presented when a record cell
//! matches it, by id first and by content hash as the fallback (epic decision 34). A
//! unit no cell matches (ceded, unowned, or never run) is copied verbatim with the
//! prose around it. Presentation options (`echo`, `output`, `include`, `warning`) are
//! resolved here, from the document and its `execute:` scope, never from the record.
//!
//! The driver owns the splice discipline the pre-flip loops shared: a cell's
//! `::: {.cell}` div cannot interrupt a paragraph, so when the copied gap ends in a
//! single newline a blank line is added; an inline value is inserted exactly as
//! rendered. `begin_cell` runs for every presented cell, hidden ones included, so
//! figure names stay keyed to a cell's position; inline units never advance it.

use std::path::PathBuf;

use quarto_pandoc_types::ConfigValue;
use quarto_source_map::{By, SourceInfo};

use super::cell::render_cell_with;
use super::expansion::{render_expanded_unit, split_expansions};
use super::figures::FigureSink;
use super::inline::{
    InlineOutcome, InlineTarget, classify_inline_result, escape_yaml_double, front_matter_key_at,
    render_inline,
};
use super::knitr_inputs::{knitr_header_options, original_chunk, present_knitr_cell};
use super::options::CellOptions;
use super::output::CellResult;
use super::visibility::{CellVisibility, cell_config, resolve_cell_options};
use crate::cell_options::partition_cell_options;
use crate::engine::units::{InlineUnit, Unit};
use crate::record::EngineEntry;
use crate::record::{
    RecordCell, UnitKind, content_hash, placed_replacement, replacement, scanned_author_text,
    span_content_hash, stored_markdown,
};

/// What presenting a document produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentResult {
    /// The engine markdown: the document with every presented unit replaced.
    pub markdown: String,
    /// The `<stem>_files` directory, when any figure was written (what the engine
    /// reports as its supporting files); empty otherwise.
    pub files_written: Vec<PathBuf>,
    /// What the driver could not present and left in place, for the caller to report.
    pub notes: Vec<PresentNote>,
}

/// A unit the driver left in place although a record cell matched it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentNote {
    /// An inline unit whose result has no text value the target format can use (or no
    /// value at all): the span source stays in the document.
    InlineNoUsableValue {
        /// Index into the `units` the driver was given.
        unit: usize,
    },
    /// A cell's image output could not be used (a malformed payload or a failed write),
    /// so the figure is missing from the page (`Q-18-11`).
    ImageUnusable {
        /// Index into the `units` the driver was given.
        unit: usize,
        /// The output's mime type, e.g. `image/png`.
        mime: String,
        /// Why it could not be used.
        reason: String,
    },
    /// A fallback unit (finished `text/x-quarto-markdown`) stored for another output
    /// format than the one being presented (or for none): the unit stays as written,
    /// as `record::place` does.
    MarkdownFormatMismatch {
        /// Index into the `units` the driver was given.
        unit: usize,
        /// The stored `quarto_format`, if the output recorded one.
        stored: Option<String>,
        /// The format being presented.
        wanted: String,
    },
}

/// Why a document could not be presented.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PresentError {
    /// A cell's `#|` option lines are not valid YAML.
    #[error("cell options are not valid YAML: {message}")]
    InvalidCellOptions { unit: usize, message: String },
    /// An inline unit in a front matter value rendered to text that cannot be written
    /// back into the YAML string.
    #[error("the front matter value `{key}` cannot hold the inline result: {reason}")]
    FrontMatterValue {
        unit: usize,
        key: String,
        reason: String,
    },
}

/// Present `cells` (an engine's record cells) over `document_text`, the text the units
/// were located in. `scope` is the document's merged `execute:` scope and `target` the
/// output format's mime preference for inline values; `format` is the full format name
/// a fallback unit's stored markdown must have been produced for; figures go through `fig`.
/// `engine` is the record's entry for the engine that ran (its `known_options` reach a knitr
/// cell\'s presentation); `None` for a record with no such entry.
pub fn present_document(
    cells: &[RecordCell],
    document_text: &str,
    units: &[Unit],
    scope: Option<&ConfigValue>,
    target: InlineTarget,
    format: &str,
    engine: Option<&EngineEntry>,
    fig: &mut dyn FigureSink,
) -> Result<PresentResult, PresentError> {
    let matched = match_cells(units, cells);
    // The author text of each unit: its scanned text without the container prefix and indent,
    // which `replacement` puts back on every line of what it splices.
    let author_texts = crate::record::AuthorTexts::from_scanned(units);
    let mut order: Vec<usize> = (0..units.len()).collect();
    order.sort_by_key(|&i| units[i].start());

    let mut out = String::with_capacity(document_text.len());
    let mut last_end = 0;
    let mut notes = Vec::new();
    for ui in order {
        let Some(cell) = matched[ui] else {
            continue;
        };
        // A fallback unit holds finished markdown: splice it in, never present it.
        if let Some((markdown, stored)) = stored_markdown(cell) {
            if stored.as_deref() != Some(format) {
                notes.push(PresentNote::MarkdownFormatMismatch {
                    unit: ui,
                    stored,
                    wanted: format.to_string(),
                });
                continue;
            }
            let (start, end, text) = placed_replacement(cell, &units[ui], document_text, &markdown);
            if matches!(units[ui], Unit::Cell(_)) {
                // A fallback cell keeps its figure number, as a presented one does.
                fig.begin_cell();
            }
            out.push_str(&document_text[last_end..start]);
            if matches!(units[ui], Unit::Cell(_)) && out.ends_with('\n') && !out.ends_with("\n\n") {
                out.push('\n');
            }
            out.push_str(&text);
            last_end = end;
            continue;
        }
        let result = CellResult::from_nbformat(&cell.outputs, cell.execution_count);
        match &units[ui] {
            Unit::Cell(u) => {
                let author = author_texts.0[ui].text.as_str();
                let parsed =
                    partition_cell_options(&u.lang, author, SourceInfo::generated(By::unknown()))
                        .map_err(|e| PresentError::InvalidCellOptions {
                        unit: ui,
                        message: e.to_string(),
                    })?;
                let parsed_options = parsed.options.clone();
                let cell_only = cell_config(parsed.options.clone());
                let resolved = resolve_cell_options(scope, parsed.options);
                let visibility = CellVisibility::resolve(resolved.as_ref());
                // Every presented cell, visible or not, keeps its figure number.
                fig.begin_cell();
                let options =
                    CellOptions::from_config(resolved.as_ref()).forwarding(cell_only.as_ref());
                let text = match split_expansions(&cell.outputs) {
                    // An expanded cell is one unit presented as one cell per expansion,
                    // each with its own options.
                    Some(expansions) => render_expanded_unit(
                        &u.lang,
                        &parsed.code,
                        &options,
                        visibility,
                        &expansions,
                        scope,
                        fig,
                    )
                    .map_err(|message| PresentError::InvalidCellOptions { unit: ui, message })?,
                    None => {
                        // A structured knitr cell is read by the shared reader; any other
                        // cell presents as it always did.
                        let header = knitr_header_options(parsed_options);
                        let (info, lines) = original_chunk(&u.lang, &u.attr_tail, author);
                        present_knitr_cell(
                            &u.lang,
                            &parsed.code,
                            cell,
                            engine,
                            header.as_ref(),
                            (&info, &lines),
                            fig,
                        )
                        .unwrap_or_else(|| {
                            render_cell_with(
                                &u.lang,
                                &parsed.code,
                                &result,
                                &options,
                                fig,
                                visibility,
                            )
                        })
                    }
                };
                notes.extend(
                    fig.take_failures()
                        .into_iter()
                        .map(|f| PresentNote::ImageUnusable {
                            unit: ui,
                            mime: f.mime,
                            reason: f.reason,
                        }),
                );
                // `replacement` carries the container prefix and indent onto every line; the
                // rendered text keeps its own trailing newlines, as it always did.
                let trailing = &text[text.trim_end_matches('\n').len()..];
                let (start, end, placed) = replacement(&units[ui], document_text, &text);
                let text = format!("{}{trailing}", placed.trim_end_matches('\n'));
                out.push_str(&document_text[last_end..start]);
                if out.ends_with('\n') && !out.ends_with("\n\n") {
                    out.push('\n');
                }
                out.push_str(&text);
                last_end = end;
            }
            Unit::Inline(u) => {
                let Some(text) = inline_text(ui, u, &result, document_text, target, &mut notes)?
                else {
                    continue;
                };
                out.push_str(&document_text[last_end..u.start]);
                out.push_str(&text);
                last_end = u.end;
            }
        }
    }
    out.push_str(&document_text[last_end..]);

    Ok(PresentResult {
        markdown: out,
        files_written: fig
            .wrote_any()
            .then(|| fig.files_dir())
            .into_iter()
            .collect(),
        notes,
    })
}

/// The record cell for each unit: the cell whose id is the unit's, else (a unit with no
/// id, or an id filled from a different author text) the first unused cell of the same
/// kind with the unit's content hash. Each cell matches at most one unit.
fn match_cells<'a>(units: &[Unit], cells: &'a [RecordCell]) -> Vec<Option<&'a RecordCell>> {
    let mut used = vec![false; cells.len()];
    let mut matched: Vec<Option<usize>> = vec![None; units.len()];
    for (ui, unit) in units.iter().enumerate() {
        let Some(id) = unit_id(unit) else {
            continue;
        };
        if let Some(ci) = (0..cells.len()).find(|&ci| {
            !used[ci]
                && cells[ci].metadata.quarto_kind == unit_kind(unit)
                && cells[ci].metadata.quarto_cell_id == id
        }) {
            used[ci] = true;
            matched[ui] = Some(ci);
        }
    }
    for (ui, unit) in units.iter().enumerate() {
        if matched[ui].is_some() {
            continue;
        }
        let hash = unit_content_hash(unit);
        if let Some(ci) = (0..cells.len()).find(|&ci| {
            !used[ci]
                && cells[ci].metadata.quarto_kind == unit_kind(unit)
                && cells[ci].metadata.quarto_content_hash == hash
        }) {
            used[ci] = true;
            matched[ui] = Some(ci);
        }
    }
    matched
        .into_iter()
        .map(|m| m.map(|ci| &cells[ci]))
        .collect()
}

fn unit_id(unit: &Unit) -> Option<&str> {
    match unit {
        Unit::Cell(c) => c.id.as_ref().map(|i| i.0.as_str()),
        Unit::Inline(i) => i.id.as_ref().map(|i| i.0.as_str()),
    }
}

fn unit_kind(unit: &Unit) -> UnitKind {
    match unit {
        Unit::Cell(_) => UnitKind::Cell,
        Unit::Inline(_) => UnitKind::Inline,
    }
}

/// The content hash of a unit's scanned text (its author text when no sugaring touched it).
fn unit_content_hash(unit: &Unit) -> String {
    let text = scanned_author_text(unit);
    match unit {
        Unit::Cell(_) => content_hash(&text),
        Unit::Inline(i) => span_content_hash(&i.lang, &text),
    }
}

/// The text that replaces an inline unit, or `None` to leave it as written (no usable
/// value, noted; or an empty value, as Q1 leaves it). A front matter value is escaped
/// for the YAML double-quoted string the writer put it in, and a newline in it is an
/// error (it would fold to a space).
fn inline_text(
    ui: usize,
    unit: &InlineUnit,
    result: &CellResult,
    document_text: &str,
    target: InlineTarget,
    notes: &mut Vec<PresentNote>,
) -> Result<Option<String>, PresentError> {
    let text = match classify_inline_result(result) {
        InlineOutcome::Value(bundle) => render_inline(&bundle, target),
        InlineOutcome::NoTextValue | InlineOutcome::Silent | InlineOutcome::Raised { .. } => None,
    };
    let Some(text) = text else {
        notes.push(PresentNote::InlineNoUsableValue { unit: ui });
        return Ok(None);
    };
    if text.is_empty() {
        return Ok(None);
    }
    if !unit.in_front_matter {
        return Ok(Some(text));
    }
    if text.contains(['\n', '\r']) {
        return Err(PresentError::FrontMatterValue {
            unit: ui,
            key: front_matter_key_at(document_text, unit.start).unwrap_or_else(|| "?".to_string()),
            reason: "the value contains a newline, which a double-quoted YAML string would \
                     fold to a space"
                .to_string(),
        });
    }
    Ok(Some(escape_yaml_double(&text)))
}

#[cfg(test)]
mod tests {
    //! Every test builds its record with the real `RecordBuilder` over units the scanner
    //! produced, so the ids and hashes are the ones the engine would write.

    use super::*;
    use crate::engine::units::{UnitId, scan_units};
    use crate::presenter::figures::FsFigureSink;
    use crate::record::{
        AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, fill_ids,
    };
    use quarto_pandoc_types::ConfigMapEntry;
    use serde_json::{Value, json};

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    fn stdout(text: &str) -> Vec<Value> {
        vec![json!({"output_type": "stream", "name": "stdout", "text": text})]
    }

    fn plain(text: &str) -> Vec<Value> {
        vec![json!({
            "output_type": "execute_result",
            "execution_count": 1,
            "data": {"text/plain": text},
            "metadata": {}
        })]
    }

    fn png() -> Vec<Value> {
        vec![json!({
            "output_type": "display_data",
            "data": {"image/png": PNG, "text/plain": "<Figure>"},
            "metadata": {}
        })]
    }

    /// The stage's units for `text`: scanned once, ids filled.
    fn staged_units(text: &str) -> (Vec<Unit>, AuthorTexts) {
        let mut units = scan_units(text);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        (units, texts)
    }

    /// A record over `units` with `outputs` for each `(unit index, outputs)`.
    fn record(
        units: &[Unit],
        texts: &AuthorTexts,
        results: &[(usize, Vec<Value>)],
    ) -> Vec<RecordCell> {
        let def = HashDefinition::conservative("test");
        let mut builder = RecordBuilder::new("jupyter", &def, units, texts).unwrap();
        for (index, outputs) in results {
            builder
                .push(
                    *index,
                    UnitResult {
                        outputs: outputs.clone(),
                        execution_count: Some(1),
                        kernel: Some("python3".to_string()),
                    },
                )
                .unwrap();
        }
        builder.finish(EngineEntryInput::default()).unwrap().cells
    }

    fn scope(key: &str, value: bool) -> ConfigValue {
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_bool(value, SourceInfo::for_test()),
            }],
            SourceInfo::for_test(),
        )
    }

    /// Present `text` with `results`, figures under a temp dir; returns the result and
    /// the temp dir (so written files can be inspected).
    fn present(
        text: &str,
        results: &[(usize, Vec<Value>)],
        scope: Option<&ConfigValue>,
    ) -> (PresentResult, tempfile::TempDir) {
        let (units, texts) = staged_units(text);
        let cells = record(&units, &texts, results);
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        let out = present_document(
            &cells,
            text,
            &units,
            scope,
            InlineTarget::from_format("html"),
            "html",
            None,
            &mut fig,
        )
        .unwrap();
        (out, dir)
    }

    fn cell_md(code: &str, text: &str) -> String {
        format!(
            "::: {{.cell}}\n\n```{{.python .cell-code}}\n{code}\n```\n\n\
             ::: {{.cell-output .cell-output-stdout}}\n\n```\n{text}\n```\n\n:::\n\n:::\n"
        )
    }

    // ── knitr cells (KNR-3 Phase 3B) ──

    /// An engine entry (no cells) carrying `known_options`.
    fn entry_with(units: &[Unit], texts: &AuthorTexts, known: Option<Vec<String>>) -> EngineEntry {
        let def = HashDefinition::conservative("knitr");
        RecordBuilder::new("knitr", &def, units, texts)
            .unwrap()
            .finish(EngineEntryInput {
                known_options: known,
                ..Default::default()
            })
            .unwrap()
            .engine
    }

    #[test]
    fn a_structured_knitr_cell_reaches_the_shared_reader() {
        use crate::presenter::knitr_inputs::fixture;
        let (units, texts) = staged_units(fixture::SOURCE);
        let mut cells = record(&units, &texts, &[(0, fixture::outputs())]);
        fixture::apply(&mut cells[0]);
        let engine = entry_with(
            &units,
            &texts,
            Some(fixture::KNOWN.iter().map(|s| s.to_string()).collect()),
        );
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        let out = present_document(
            &cells,
            fixture::SOURCE,
            &units,
            None,
            InlineTarget::from_format("html"),
            "html",
            Some(&engine),
            &mut fig,
        )
        .unwrap();
        fixture::assert_rich(&out.markdown);
    }

    #[test]
    fn a_list_nested_structured_cell_keeps_its_indent_on_every_line() {
        // The unit's `code` keeps the container indent; the presenter partitions the author
        // text instead and splices through `replacement`, so the fences stay in the list item.
        use crate::presenter::knitr_inputs::fixture;
        let body = fixture::SOURCE
            .lines()
            .map(|l| format!("  {l}\n"))
            .collect::<String>();
        let text = format!("- item\n\n{body}\nafter\n");
        let (units, texts) = staged_units(&text);
        let mut cells = record(&units, &texts, &[(0, fixture::outputs())]);
        fixture::apply(&mut cells[0]);
        let engine = entry_with(
            &units,
            &texts,
            Some(fixture::KNOWN.iter().map(|s| s.to_string()).collect()),
        );
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        let out = present_document(
            &cells,
            &text,
            &units,
            None,
            InlineTarget::from_format("html"),
            "html",
            Some(&engine),
            &mut fig,
        )
        .unwrap()
        .markdown;
        let cell_part = out.strip_prefix("- item\n\n").unwrap();
        let before_after = cell_part.split("\nafter").next().unwrap();
        assert!(
            before_after
                .lines()
                .all(|l| l.is_empty() || l.starts_with("  ")),
            "{out}"
        );
    }

    #[test]
    fn a_cell_with_no_knitr_keys_presents_as_before_even_with_an_engine_entry() {
        let (units, texts) = staged_units("```{python}\nprint(1)\n```\n");
        let cells = record(&units, &texts, &[(0, stdout("1\n"))]);
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        let with = |engine: Option<&EngineEntry>, fig: &mut FsFigureSink| {
            present_document(
                &cells,
                "```{python}\nprint(1)\n```\n",
                &units,
                None,
                InlineTarget::from_format("html"),
                "html",
                engine,
                fig,
            )
            .unwrap()
            .markdown
        };
        let entry = entry_with(&units, &texts, Some(vec![]));
        assert_eq!(with(None, &mut fig), with(Some(&entry), &mut fig));
    }

    // ── expanded cells ──────────────────────────────────────────────────

    #[test]
    fn an_expanded_unit_presents_one_cell_per_expansion_with_its_own_options() {
        let text = "```{python}\nexpand()\n```\n";
        let outputs = vec![
            json!({"output_type": "display_data", "data": {}, "metadata": {},
                   "quarto_expansion": {"index": 1, "source": "#| echo: false\na()"}}),
            json!({"output_type": "stream", "name": "stdout", "text": "A",
                   "quarto_expansion": {"index": 1}}),
            json!({"output_type": "display_data", "data": {}, "metadata": {},
                   "quarto_expansion": {"index": 2, "source": "b()"}}),
            json!({"output_type": "stream", "name": "stdout", "text": "B",
                   "quarto_expansion": {"index": 2}}),
        ];
        let (out, _dir) = present(text, &[(0, outputs)], None);
        // The unit's own source is a cell of its own (echoed under the usual rules, as
        // the engine's own markdown does), then one cell per expansion.
        assert_eq!(
            out.markdown.matches("::: {.cell}").count(),
            3,
            "{}",
            out.markdown
        );
        assert!(!out.markdown.contains("a()"), "{}", out.markdown);
        assert!(out.markdown.contains("b()"), "{}", out.markdown);
        let own = out
            .markdown
            .find("expand()")
            .expect("the unit's source is echoed");
        assert!(own < out.markdown.find("b()").unwrap(), "{}", out.markdown);
    }

    #[test]
    fn an_expanded_units_own_echo_false_hides_its_source() {
        let text = "```{python}\n#| echo: false\nexpand()\n```\n";
        let outputs = vec![
            json!({"output_type": "display_data", "data": {}, "metadata": {},
                   "quarto_expansion": {"index": 1, "source": "b()"}}),
            json!({"output_type": "stream", "name": "stdout", "text": "B",
                   "quarto_expansion": {"index": 1}}),
        ];
        let (out, _dir) = present(text, &[(0, outputs)], None);
        assert!(!out.markdown.contains("expand()"), "{}", out.markdown);
        assert!(out.markdown.contains("b()"), "{}", out.markdown);
    }

    // ── the cell-option layer (KNR-2 Phase 2C, decision 12) ─────────────

    #[test]
    fn a_cell_option_shapes_the_jupyter_cell() {
        let text = "```{python}
#| class.source: k
#| column: margin
print(1)
```\n";
        let (out, _) = present(text, &[(0, stdout("1\n"))], None);
        assert!(
            out.markdown
                .starts_with("::: {.cell .column-margin}\n\n```{.python .k .cell-code}\n"),
            "{}",
            out.markdown
        );
    }

    /// Provisional decision 1: a Jupyter cell that sets `panel` or `layout-ncol` now gets
    /// them on its cell div (Quarto 1 does the same for knitr), so its output changes.
    #[test]
    fn panel_and_layout_ncol_shape_the_jupyter_cell_div() {
        let text = "```{python}
#| panel: tabset
#| layout-ncol: 2
print(1)
```\n";
        let (out, _) = present(text, &[(0, stdout("1\n"))], None);
        assert!(
            out.markdown
                .starts_with("::: {.cell .panel-tabset layout-ncol=\"2\"}\n\n"),
            "{}",
            out.markdown
        );
    }

    #[test]
    fn a_cell_without_layer_options_is_unchanged() {
        let text = "```{python}
#| echo: true
print(1)
```\n";
        let (out, _) = present(text, &[(0, stdout("1\n"))], None);
        assert_eq!(out.markdown, format!("{}", cell_md("print(1)", "1")));
    }

    // ── gaps and adjacent cells ─────────────────────────────────────────

    #[test]
    fn gaps_are_copied_and_each_cell_is_substituted_by_byte_range() {
        let text =
            "# Title\n\nProse one.\n\n```{python}\na\n```\n\n```{python}\nb\n```\n\nProse two.\n";
        let (out, _) = present(text, &[(0, stdout("A\n")), (1, stdout("B\n"))], None);
        assert_eq!(
            out.markdown,
            format!(
                "# Title\n\nProse one.\n\n{}\n{}\nProse two.\n",
                cell_md("a", "A"),
                cell_md("b", "B")
            )
        );
        assert!(out.files_written.is_empty());
        assert!(out.notes.is_empty());
    }

    #[test]
    fn cells_at_document_start_and_end_and_a_ceded_fence_left_verbatim() {
        let text = "```{python}\na\n```\n\n```{ojs}\nviewof x = 1\n```\n\n```{python}\nb\n```";
        let (out, _) = present(text, &[(0, stdout("A\n")), (2, stdout("B\n"))], None);
        assert_eq!(
            out.markdown,
            format!(
                "{}\n```{{ojs}}\nviewof x = 1\n```\n\n{}",
                cell_md("a", "A"),
                cell_md("b", "B")
            )
        );
    }

    // ── the newline rule ────────────────────────────────────────────────

    #[test]
    fn a_cell_after_a_paragraph_ending_in_one_newline_gets_a_blank_line() {
        let text = "Para.\n```{python}\na\n```\n";
        let (out, _) = present(text, &[(0, stdout("A\n"))], None);
        assert_eq!(out.markdown, format!("Para.\n\n{}", cell_md("a", "A")));
        // An existing blank line is not doubled.
        let text = "Para.\n\n```{python}\na\n```\n";
        let (out, _) = present(text, &[(0, stdout("A\n"))], None);
        assert_eq!(out.markdown, format!("Para.\n\n{}", cell_md("a", "A")));
    }

    #[test]
    fn a_hidden_cell_emits_nothing_and_keeps_the_newline_rule() {
        let text = "Before.\n\n```{python}\n#| include: false\na\n```\n\nAfter.\n";
        let (out, _) = present(text, &[(0, stdout("A\n"))], None);
        // The cell's bytes (through the newline after its closing fence) vanish; the
        // gap before it already ends in a blank line, so nothing is added.
        assert_eq!(out.markdown, "Before.\n\n\nAfter.\n");
    }

    // ── begin_cell for every executed unit ───────────────────────────────

    #[test]
    fn begin_cell_runs_for_hidden_cells_so_figure_names_keep_their_position() {
        let text = "```{python}\n#| include: false\nsetup()\n```\n\n```{python}\nplot()\n```\n";
        let (out, dir) = present(text, &[(0, png()), (1, png())], None);
        assert!(
            out.markdown
                .contains("![](doc_files/figure-html/cell-2-output-1.png)"),
            "{}",
            out.markdown
        );
        assert!(!out.markdown.contains("cell-1-output"), "{}", out.markdown);
        let figs = dir.path().join("doc_files/figure-html");
        assert!(figs.join("cell-2-output-1.png").is_file());
        assert!(!figs.join("cell-1-output-1.png").exists());
        // The document scope hides a cell the same way.
        let text = "```{python}\nsetup()\n```\n\n```{python}\nplot()\n```\n";
        let (out, _) = present(
            text,
            &[(0, png()), (1, png())],
            Some(&scope("include", false)),
        );
        assert_eq!(out.markdown, "\n\n");
    }

    // ── inline units ────────────────────────────────────────────────────

    #[test]
    fn an_inline_unit_is_replaced_in_place_and_consumes_no_cell_number() {
        let text =
            "```{python}\nplot()\n```\n\nValue `{python} x` here.\n\n```{python}\nplot()\n```\n";
        let (out, _) = present(text, &[(0, png()), (1, plain("42")), (2, png())], None);
        assert!(
            out.markdown.contains("Value 42 here.\n"),
            "{}",
            out.markdown
        );
        assert!(
            out.markdown.contains("cell-1-output-1.png"),
            "{}",
            out.markdown
        );
        assert!(
            out.markdown.contains("cell-2-output-1.png"),
            "the span between the cells must not advance the cell number:\n{}",
            out.markdown
        );
    }

    #[test]
    fn an_inline_unit_with_no_usable_value_is_left_in_place_and_noted() {
        let text = "Value `{python} x` here.\n";
        let only_png = vec![json!({
            "output_type": "display_data",
            "data": {"image/png": PNG},
            "metadata": {}
        })];
        let (out, _) = present(text, &[(0, only_png)], None);
        assert_eq!(out.markdown, text);
        assert_eq!(out.notes, [PresentNote::InlineNoUsableValue { unit: 0 }]);
        // An empty value leaves the span as written too, silently (Q1 parity).
        let (out, _) = present(text, &[(0, plain("''"))], None);
        assert_eq!(out.markdown, text);
        assert!(out.notes.is_empty());
    }

    #[test]
    fn a_front_matter_inline_value_is_escaped_and_a_newline_in_it_is_an_error() {
        let text = "---\ntitle: \"Report `{python} name`\"\n---\n\nBody.\n";
        let (out, _) = present(text, &[(0, plain("'say \"hi\"'"))], None);
        assert_eq!(
            out.markdown,
            "---\ntitle: \"Report say \\\"hi\\\"\"\n---\n\nBody.\n"
        );

        let (units, texts) = staged_units(text);
        let cells = record(&units, &texts, &[(0, plain("two\nlines"))]);
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        let err = present_document(
            &cells,
            text,
            &units,
            None,
            InlineTarget::from_format("html"),
            "html",
            None,
            &mut fig,
        )
        .unwrap_err();
        assert!(
            matches!(&err, PresentError::FrontMatterValue { unit: 0, key, .. } if key == "title"),
            "{err:?}"
        );
    }

    // ── unmatched units ─────────────────────────────────────────────────

    #[test]
    fn a_unit_with_no_matching_record_cell_is_left_verbatim() {
        let text = "```{python}\na\n```\n\n```{python}\nb\n```\n";
        let (out, _) = present(text, &[(1, stdout("B\n"))], None);
        assert_eq!(
            out.markdown,
            format!("```{{python}}\na\n```\n\n{}", cell_md("b", "B"))
        );
    }

    #[test]
    fn an_id_mismatch_falls_back_to_the_content_hash_once_per_cell() {
        let text = "```{python}\na\n```\n\n```{python}\na\n```\n";
        let (units, texts) = staged_units(text);
        let cells = record(
            &units,
            &texts,
            &[(0, stdout("first\n")), (1, stdout("second\n"))],
        );
        // Replay-shaped units: a fresh scan with ids filled from different author texts.
        let mut replay_units = scan_units(text);
        for (n, unit) in replay_units.iter_mut().enumerate() {
            if let Unit::Cell(c) = unit {
                c.id = Some(UnitId(format!("{n}-deadbeef-unlabeled")));
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        let out = present_document(
            &cells,
            text,
            &replay_units,
            None,
            InlineTarget::from_format("html"),
            "html",
            None,
            &mut fig,
        )
        .unwrap();
        // Identical cells: each record cell is used once, in order.
        assert_eq!(
            out.markdown,
            format!("{}\n{}", cell_md("a", "first"), cell_md("a", "second"))
        );
    }

    #[test]
    fn a_unit_without_an_id_matches_by_content_hash() {
        let text = "```{python}\na\n```\n";
        let (units, texts) = staged_units(text);
        let cells = record(&units, &texts, &[(0, stdout("A\n"))]);
        let bare = scan_units(text);
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        let out = present_document(
            &cells,
            text,
            &bare,
            None,
            InlineTarget::from_format("html"),
            "html",
            None,
            &mut fig,
        )
        .unwrap();
        assert_eq!(out.markdown, cell_md("a", "A"));
    }

    // ── fallback (markdown) units (KNR-2 Phase 2F) ──────────────────────

    fn markdown_unit(md: &str, format: &str) -> Vec<Value> {
        vec![json!({
            "output_type": "display_data",
            "data": {"text/x-quarto-markdown": md},
            "metadata": {"quarto_format": format}
        })]
    }

    fn present_in(text: &str, results: &[(usize, Vec<Value>)], format: &str) -> PresentResult {
        let (units, texts) = staged_units(text);
        let cells = record(&units, &texts, results);
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        present_document(
            &cells,
            text,
            &units,
            None,
            InlineTarget::from_format(format),
            format,
            None,
            &mut fig,
        )
        .unwrap()
    }

    #[test]
    fn a_fallback_cell_is_replaced_by_its_stored_markdown_verbatim() {
        let text = "Before.\n\n```{r}\n1 + 1\n```\n\nAfter.\n";
        let md = "::: {.cell}\n\n``` r\n1 + 1\n```\n\n:::\n";
        let out = present_in(text, &[(0, markdown_unit(md, "html"))], "html");
        assert_eq!(out.markdown, format!("Before.\n\n{md}\nAfter.\n"));
        assert!(out.notes.is_empty());
    }

    #[test]
    fn a_fallback_inline_unit_is_replaced_without_its_trailing_newline() {
        let text = "Value `{r} 1 + 1` here.\n";
        let out = present_in(text, &[(0, markdown_unit("2\n", "html"))], "html");
        assert_eq!(out.markdown, "Value 2 here.\n");
    }

    #[test]
    fn a_verbatim_inline_unit_keeps_its_trailing_newlines() {
        // knitr\'s `include_graphics` in a span inserts a block with its own blank lines; the
        // record marks such a value verbatim, so the fence is not glued to the text after it.
        let text = "Icon `{r} 1 + 1` here.\n";
        let unit = vec![json!({
            "output_type": "display_data",
            "data": {"text/x-quarto-markdown": "::: {.x}\nA\n:::\n\n"},
            "metadata": {"quarto_format": "html", "quarto_verbatim": true}
        })];
        let out = present_in(text, &[(0, unit)], "html");
        assert_eq!(out.markdown, "Icon ::: {.x}\nA\n:::\n\n here.\n");
    }

    #[test]
    fn a_mixed_record_keeps_unit_order() {
        let text = "```{python}\na\n```\n\n```{r}\nb\n```\n\n```{python}\nc\n```\n";
        let out = present_in(
            text,
            &[
                (0, stdout("A\n")),
                (1, markdown_unit("FALLBACK\n", "html")),
                (2, stdout("C\n")),
            ],
            "html",
        );
        assert_eq!(
            out.markdown,
            format!(
                "{}\n{}\n{}",
                cell_md("a", "A"),
                "FALLBACK\n",
                cell_md("c", "C")
            )
        );
    }

    #[test]
    fn a_fallback_unit_stored_for_another_format_stays_as_written_and_is_noted() {
        let text = "```{r}\n1\n```\n";
        let out = present_in(text, &[(0, markdown_unit("X\n", "html"))], "typst");
        assert_eq!(out.markdown, text);
        assert_eq!(
            out.notes,
            [PresentNote::MarkdownFormatMismatch {
                unit: 0,
                stored: Some("html".to_string()),
                wanted: "typst".to_string()
            }]
        );
    }

    #[test]
    fn a_fallback_cell_in_a_blockquote_keeps_its_container_prefix() {
        let text = "> ```{r}\n> 1\n> ```\n";
        let out = present_in(text, &[(0, markdown_unit("a\n\nb\n", "html"))], "html");
        assert_eq!(out.markdown, "> a\n>\n> b\n");
    }

    // ── files_written ───────────────────────────────────────────────────

    #[test]
    fn a_figure_writing_record_reports_the_files_dir_and_a_text_only_one_does_not() {
        let text = "```{python}\nplot()\n```\n";
        let (out, dir) = present(text, &[(0, png())], None);
        assert_eq!(out.files_written, [dir.path().join("doc_files")]);
        let (out, _) = present(text, &[(0, stdout("A\n"))], None);
        assert!(out.files_written.is_empty());
        // A hidden figure cell writes nothing and reports nothing.
        let (out, _) = present(text, &[(0, png())], Some(&scope("output", false)));
        assert!(out.files_written.is_empty());
    }

    // ── unusable images are noted per unit (Q-18-11) ────────────────────

    /// A display whose only payload is an image that is not base64.
    fn bad_png() -> Vec<Value> {
        vec![json!({
            "output_type": "display_data",
            "data": {"image/png": "!!!not base64!!!"},
            "metadata": {}
        })]
    }

    #[test]
    fn an_unusable_image_is_noted_against_its_cell_and_the_placeholder_is_escaped() {
        let text = "Intro.\n\n```{python}\na()\n```\n\n```{python}\nplot()\n```\n";
        let (out, _) = present(text, &[(0, stdout("A\n")), (1, bad_png())], None);
        let [PresentNote::ImageUnusable { unit, mime, reason }] = out.notes.as_slice() else {
            panic!("expected one ImageUnusable note, got {:?}", out.notes);
        };
        assert_eq!((*unit, mime.as_str()), (1, "image/png"));
        assert!(reason.contains("base64"), "{reason}");
        assert!(
            out.markdown.contains("\\[Image output\\]"),
            "{}",
            out.markdown
        );
        assert!(out.files_written.is_empty());
    }

    #[test]
    fn a_good_image_and_a_failed_one_note_only_the_failure() {
        let text = "```{python}\nplot()\n```\n\n```{python}\nplot()\n```\n";
        let (out, _) = present(text, &[(0, png()), (1, bad_png())], None);
        assert_eq!(out.notes.len(), 1, "{:?}", out.notes);
        assert!(matches!(
            out.notes[0],
            PresentNote::ImageUnusable { unit: 1, .. }
        ));
    }
}
