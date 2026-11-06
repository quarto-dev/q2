//! Convert an imported Positron cache into a [`Record`] with no engines and empty execution
//! hashes, so the ordinary record splice can show it (RIU-3a Phase B; design proven in RIU-0 S4).
//!
//! Positron's outputs stay display-only: the conversion reads [`DisplayOnlyOutputs::outputs`]
//! and builds each cell by hand. There is no `From` or `Into` (the conformance probe forbids
//! one), and it never calls `rehash_record`.
//!
//! Target-neutral: it builds for WASM (checked through `npm run build:wasm`).

use std::collections::BTreeMap;

use serde_json::json;

use crate::engine::units::{Unit, scan_units};

use super::author_text::{SugaredBlock, author_texts};
use super::ids::fill_ids;
use super::matching::{UnitKey, match_units};

use super::ids::author_label;
use super::positron_import::PositronImport;
use super::types::{
    AuthorTexts, CellMetadata, CellType, Record, RecordCell, RecordMetadata, UnitKind,
};

/// The provenance marker on the record and on every imported cell, in their `extra`.
pub const PROVENANCE_KEY: &str = "quarto_provenance";
/// The value of [`PROVENANCE_KEY`] for a record imported from Positron.
pub const PROVENANCE_POSITRON: &str = "positron";
/// The record `extra` key holding how many cached cells no unit paired with (a number).
pub const UNCLAIMED_KEY: &str = "quarto_unclaimed_cache_cells";

/// Build a record from `import`. `units` have their ids filled and `authors` come from
/// `author_texts(&units, sugared_blocks)`, the inputs the splice itself uses (never a raw scan).
/// `owner` maps a unit's language token to the engine that owns it.
///
/// The occurrence of a unit is counted per content hash across **all** units, then the language
/// gate decides, so a gated-out duplicate still shifts the pairing the way Positron's own
/// pairing does. The `fallback` argument of the gate is `false`: the conversion has no recorded
/// output, and the language test already excludes the only units that are fallback units.
///
/// A cached cell no unit pairs with (by hash and occurrence, **whether or not the gate then
/// accepts the unit**) is counted into the record `extra` under [`UNCLAIMED_KEY`], so staleness
/// survives the one-time read of the source list.
pub fn record_from_positron(
    import: &PositronImport,
    units: &[Unit],
    authors: &AuthorTexts,
    source: Option<String>,
    owner: &dyn Fn(&str) -> String,
) -> Record {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut cells = Vec::new();
    let mut claimed = 0usize;
    for (unit, author) in units.iter().zip(&authors.0) {
        let hash = author.content_hash.clone();
        let occurrence = {
            let n = seen.entry(hash.clone()).or_insert(0);
            let o = *n;
            *n += 1;
            o
        };
        if import.has_cell(&hash, occurrence) {
            claimed += 1;
        }
        let lang = unit.lang();
        let Some(found) = import.outputs_for(&hash, occurrence, &owner(lang), lang, false) else {
            continue;
        };
        let (kind, id) = match unit {
            Unit::Cell(c) => (UnitKind::Cell, c.id.as_ref().map(|i| i.0.clone())),
            Unit::Inline(i) => (UnitKind::Inline, i.id.as_ref().map(|i| i.0.clone())),
        };
        let mut extra = BTreeMap::new();
        extra.insert(PROVENANCE_KEY.to_string(), json!(PROVENANCE_POSITRON));
        cells.push(RecordCell {
            cell_type: CellType::Code,
            execution_count: None,
            id: if occurrence == 0 {
                hash.clone()
            } else {
                format!("{hash}-{}", occurrence + 1)
            },
            metadata: CellMetadata {
                quarto_cell_id: id.unwrap_or_default(),
                quarto_content_hash: hash,
                quarto_label: author_label(&author.text, lang),
                quarto_kind: kind,
                quarto_lang: lang.to_string(),
                quarto_engine: None,
                quarto_kernel: None,
                quarto_location: None,
                quarto_exec_hash: String::new(),
                quarto_hash_rule: None,
                extra,
            },
            outputs: found.outputs().to_vec(),
            source: author
                .text
                .split_inclusive('\n')
                .map(str::to_string)
                .collect(),
        });
    }
    let mut extra = BTreeMap::new();
    extra.insert(PROVENANCE_KEY.to_string(), json!(PROVENANCE_POSITRON));
    extra.insert(
        UNCLAIMED_KEY.to_string(),
        json!(import.cell_count().saturating_sub(claimed)),
    );
    Record {
        nbformat: Record::NBFORMAT,
        nbformat_minor: Record::NBFORMAT_MINOR,
        metadata: RecordMetadata {
            quarto_record_version: Record::RECORD_VERSION,
            quarto_cache_version: None,
            quarto_source: source,
            extra,
            ..Default::default()
        },
        cells,
    }
}

/// The one scan, with ids filled, of the document `inputs` describes: what the splice's own
/// `scan_and_fill_ids` does, from the same masked text and sugared snapshot.
pub(crate) fn scan_inputs(
    inputs: &crate::engine::preview_record::SourceInputs,
) -> (Vec<Unit>, AuthorTexts) {
    let mut units = scan_units(&String::from_utf8_lossy(&inputs.input_qmd));
    let authors = author_texts(&units, &inputs.sugared_blocks);
    fill_ids(&mut units, &authors).expect("author_texts is total over the units it was built from");
    (units, authors)
}

/// Convert an imported cache for the document described by `inputs`: scan the same masked
/// document the splice scans, take the author texts from the same sugared snapshot, and the
/// owner of each language from `inputs.ownership` (computed on request, see
/// `SourceInputsOptions::ownership`; without it no unit is owned and none takes outputs).
pub fn import_record_source(
    import: &PositronImport,
    inputs: &crate::engine::preview_record::SourceInputs,
    source: Option<String>,
) -> Record {
    let (units, authors) = scan_inputs(inputs);
    record_from_positron(import, &units, &authors, source, &|lang| {
        inputs.owner_of(lang)
    })
}

/// Whether `record` came from the Positron importer: it carries the provenance marker **and**
/// has no engines. A hand-edited `.quarto` or directory file that carries the marker with
/// engines is not imported and takes the ordinary `record_is_stale` path.
pub fn is_imported_record(record: &Record) -> bool {
    record.metadata.extra.contains_key(PROVENANCE_KEY) && record.metadata.quarto_engines.is_empty()
}

/// Staleness of an imported record (epic decision 5 of 2026-11-05). An imported record never
/// asserts freshness, but it reports staleness when a cached cell is orphaned: its output
/// belongs to code that is no longer in the document.
///
/// True when the conversion counted unclaimed cache cells ([`UNCLAIMED_KEY`], the cache older
/// than the document at boot), or when matching the document's units to the record's cells by
/// content hash leaves a record cell with no unit (an edit or a deletion since). A unit with no
/// cell (never run in Positron, or gated out) is **not** stale, so this reads only
/// `Matching::orphaned` and never `Matching::staleness()`, which reports exactly that case.
///
/// `input_qmd` and `sugared` are `SourceInputs::input_qmd` and `sugared_blocks`, as for
/// [`record_is_stale`](super::record_is_stale).
pub fn imported_record_is_stale(
    record: &Record,
    input_qmd: &str,
    sugared: &[SugaredBlock],
) -> bool {
    if record
        .metadata
        .extra
        .get(UNCLAIMED_KEY)
        .and_then(|v| v.as_u64())
        .is_some_and(|n| n > 0)
    {
        return true;
    }
    let units = scan_units(input_qmd);
    let texts = author_texts(&units, sugared);
    let keys: Vec<UnitKey> = units
        .iter()
        .zip(&texts.0)
        .map(|(u, t)| {
            let kind = match u {
                Unit::Cell(_) => UnitKind::Cell,
                Unit::Inline(_) => UnitKind::Inline,
            };
            UnitKey::content_only(kind, &t.content_hash)
        })
        .collect();
    !match_units(&keys, &record.cells).orphaned.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::units::scan_units;
    use crate::record::{
        SugaredBlock, author_texts, content_hash, fill_ids, import_positron_cache, read_record,
        write_record,
    };
    use serde_json::{Value, json};

    fn scanned(text: &str) -> (Vec<Unit>, AuthorTexts) {
        let mut units = scan_units(text);
        let authors = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &authors).unwrap();
        (units, authors)
    }

    /// A Positron cache whose cells are `(content hash, stdout text)`, in order.
    fn cache(cells: &[(&str, &str)]) -> PositronImport {
        let cells: Vec<Value> = cells
            .iter()
            .map(|(h, t)| {
                json!({"cell_type": "code", "execution_count": null, "id": null,
                    "metadata": {"quarto_cell_id": "x", "quarto_content_hash": h},
                    "outputs": [{"output_type": "stream", "name": "stdout", "text": [t]}],
                    "source": []})
            })
            .collect();
        let file = json!({"nbformat": 4, "nbformat_minor": 5,
            "metadata": {"quarto_cache_version": 1}, "cells": cells});
        import_positron_cache(file.to_string().as_bytes()).unwrap()
    }

    /// The owner a real registry would give: marimo for a dotted token, `knitr` for `r`,
    /// `julia` for `julia`, jupyter for the rest.
    fn owner(lang: &str) -> String {
        match lang {
            l if l.contains('.') => "marimo",
            "r" => "knitr",
            "julia" => "julia",
            _ => "jupyter",
        }
        .to_string()
    }

    fn stdout(cell: &crate::record::RecordCell) -> String {
        cell.outputs[0]["text"][0].as_str().unwrap().to_string()
    }

    const DOC: &str =
        "```{python}\nx = 1\n```\n\n```{python}\nx = 1\n```\n\n```{python.marimo}\ny = 2\n```\n";

    // S4 scratch tests, ported ---------------------------------------------------------

    #[test]
    fn duplicates_pair_by_occurrence_and_marimo_gets_nothing() {
        let (units, authors) = scanned(DOC);
        let h = |i: usize| authors.0[i].content_hash.clone();
        let import = cache(&[(&h(0), "first\n"), (&h(1), "second\n"), (&h(2), "marimo\n")]);
        let record = record_from_positron(&import, &units, &authors, None, &owner);
        assert_eq!(record.cells.len(), 2, "the python.marimo unit is gated out");
        assert_eq!(stdout(&record.cells[0]), "first\n");
        assert_eq!(stdout(&record.cells[1]), "second\n");
        assert_eq!(record.cells[0].id, h(0));
        assert_eq!(record.cells[1].id, format!("{}-2", h(0)));
    }

    #[test]
    fn a_gated_out_duplicate_still_counts_toward_the_occurrence() {
        // marimo `x = 1` first, python `x = 1` second: Positron reads both as python, so the
        // python unit is the second cached cell of that hash and must not take the first's.
        let doc = "```{python.marimo}\nx = 1\n```\n\n```{python}\nx = 1\n```\n";
        let (units, authors) = scanned(doc);
        let h = authors.0[0].content_hash.clone();
        assert_eq!(h, authors.0[1].content_hash);
        let import = cache(&[(&h, "marimo's\n"), (&h, "python's\n")]);
        let record = record_from_positron(&import, &units, &authors, None, &owner);
        assert_eq!(record.cells.len(), 1);
        assert_eq!(stdout(&record.cells[0]), "python's\n");
    }

    #[test]
    fn the_record_has_no_engines_and_empty_exec_hashes() {
        let (units, authors) = scanned(DOC);
        let h = |i: usize| authors.0[i].content_hash.clone();
        let import = cache(&[(&h(0), "first\n"), (&h(1), "second\n")]);
        let record = record_from_positron(&import, &units, &authors, Some("d.qmd".into()), &owner);
        assert!(record.metadata.quarto_engines.is_empty());
        assert!(
            record
                .cells
                .iter()
                .all(|c| c.metadata.quarto_exec_hash.is_empty()
                    && c.metadata.quarto_engine.is_none())
        );
        assert_eq!(record.metadata.quarto_cache_version, None);
        assert_eq!(
            record.metadata.quarto_record_version,
            Record::RECORD_VERSION
        );
        assert_eq!(record.metadata.quarto_source.as_deref(), Some("d.qmd"));
        assert!(
            record.describes_something(),
            "cells and no engines still describe something"
        );
    }

    #[test]
    fn provenance_survives_write_and_read_and_is_not_an_option() {
        let (units, authors) = scanned(DOC);
        let h = |i: usize| authors.0[i].content_hash.clone();
        let import = cache(&[(&h(0), "first\n"), (&h(1), "second\n")]);
        let record = record_from_positron(&import, &units, &authors, Some("d.qmd".into()), &owner);
        let back = read_record(&write_record(&record).unwrap()).unwrap();
        assert_eq!(back, record);
        assert_eq!(
            back.metadata.extra.get(PROVENANCE_KEY),
            Some(&json!(PROVENANCE_POSITRON))
        );
        for cell in &back.cells {
            assert_eq!(
                cell.metadata.extra.get(PROVENANCE_KEY),
                Some(&json!(PROVENANCE_POSITRON))
            );
            assert_eq!(
                cell.metadata.options().count(),
                0,
                "`quarto_` keys are not options"
            );
        }
    }

    // Hashes and source come from the splice's inputs (the spike took a shortcut) ----------

    #[test]
    fn a_labeled_cell_converts_from_the_sugared_snapshot_not_the_raw_scan() {
        // Crossref sugaring strips the label line from the text the scanner sees. The hash and
        // the source must be the author's text, as `author_texts` returns it.
        let author = "#| label: fig-a\nx = 1";
        let sugared = "x = 1";
        let doc = format!("```{{python}}\n{sugared}\n```\n");
        let mut units = scan_units(&doc);
        let snapshot = [SugaredBlock {
            author_text: author.into(),
            sugared_text: sugared.into(),
        }];
        let authors = author_texts(&units, &snapshot);
        fill_ids(&mut units, &authors).unwrap();
        assert_eq!(authors.0[0].content_hash, content_hash(author));
        assert_ne!(
            authors.0[0].content_hash,
            AuthorTexts::from_scanned(&units).0[0].content_hash,
            "the two differ for a labeled cell, which is why the raw scan is wrong"
        );

        let import = cache(&[(&content_hash(author), "labeled\n")]);
        let record = record_from_positron(&import, &units, &authors, None, &owner);
        assert_eq!(record.cells.len(), 1);
        let cell = &record.cells[0];
        assert_eq!(cell.metadata.quarto_content_hash, content_hash(author));
        assert_eq!(
            cell.source.concat(),
            author,
            "the source is the author text"
        );
        assert_eq!(cell.metadata.quarto_label.as_deref(), Some("fig-a"));
        assert_eq!(cell.metadata.quarto_lang, "python");
        assert!(
            !cell.metadata.quarto_cell_id.is_empty(),
            "the unit id is carried"
        );
    }

    // The gate at conversion -------------------------------------------------------------

    #[test]
    fn a_unit_whose_language_the_owner_does_not_hold_gets_no_outputs() {
        let doc = "```{python}\na = 1\n```\n\n```{ojs}\nb = 2\n```\n";
        let (units, authors) = scanned(doc);
        let import = cache(&[
            (&authors.0[0].content_hash, "py\n"),
            (&authors.0[1].content_hash, "ojs\n"),
        ]);
        let only_python = |lang: &str| {
            if lang == "python" {
                "jupyter"
            } else {
                "observable"
            }
            .to_string()
        };
        let record = record_from_positron(&import, &units, &authors, None, &only_python);
        assert_eq!(record.cells.len(), 1);
        assert_eq!(record.cells[0].metadata.quarto_lang, "python");
    }

    #[test]
    fn python_julia_and_knitr_r_units_take_outputs_with_the_fallback_flag_false() {
        let doc = "```{python}\na = 1\n```\n\n```{julia}\nb = 2\n```\n\n```{r}\nc <- 3\n```\n";
        let (units, authors) = scanned(doc);
        let import = cache(&[
            (&authors.0[0].content_hash, "py\n"),
            (&authors.0[1].content_hash, "jl\n"),
            (&authors.0[2].content_hash, "r\n"),
        ]);
        let record = record_from_positron(&import, &units, &authors, None, &owner);
        let got: Vec<_> = record.cells.iter().map(stdout).collect();
        assert_eq!(got, ["py\n", "jl\n", "r\n"]);
    }

    #[test]
    fn an_r_chunk_the_gate_refuses_is_left_out() {
        // With `r` owned by an engine the gate does not admit, the chunk gets nothing.
        let doc = "```{r}\nc <- 3\n```\n";
        let (units, authors) = scanned(doc);
        let import = cache(&[(&authors.0[0].content_hash, "r\n")]);
        let record = record_from_positron(&import, &units, &authors, None, &|_| "other".into());
        assert!(record.cells.is_empty());
    }

    // Unclaimed cache cells (decision 5 of 2026-11-05) -----------------------------------

    fn unclaimed(record: &Record) -> u64 {
        record.metadata.extra[UNCLAIMED_KEY].as_u64().unwrap()
    }

    #[test]
    fn a_cache_cell_no_unit_pairs_with_is_counted() {
        let (units, authors) = scanned("```{python}\nx = 1\n```\n");
        let import = cache(&[
            (&authors.0[0].content_hash, "kept\n"),
            ("deadbeefdeadbeef", "the old code's output\n"),
        ]);
        let record = record_from_positron(&import, &units, &authors, None, &owner);
        assert_eq!(unclaimed(&record), 1);
    }

    #[test]
    fn an_all_claimed_cache_stores_zero() {
        let (units, authors) = scanned("```{python}\nx = 1\n```\n");
        let import = cache(&[(&authors.0[0].content_hash, "kept\n")]);
        let record = record_from_positron(&import, &units, &authors, None, &owner);
        assert_eq!(unclaimed(&record), 0, "the key is always present");
    }

    #[test]
    fn a_cache_cell_whose_unit_the_gate_refuses_is_not_counted() {
        // An `{r}` chunk under an owner the gate refuses, and a python.marimo unit: both pair
        // with a cached cell by hash and occurrence, so both cells are claimed.
        let doc = "```{r}\nc <- 3\n```\n\n```{python.marimo}\ny = 2\n```\n";
        let (units, authors) = scanned(doc);
        let import = cache(&[
            (&authors.0[0].content_hash, "r\n"),
            (&authors.0[1].content_hash, "marimo\n"),
        ]);
        let record = record_from_positron(&import, &units, &authors, None, &|_| "other".into());
        assert!(record.cells.is_empty());
        assert_eq!(unclaimed(&record), 0);
    }

    #[test]
    fn a_duplicate_content_pair_counts_per_occurrence() {
        // Three cached cells with one hash, two units with it: the third is unclaimed.
        let doc = "```{python}\nx = 1\n```\n\n```{python}\nx = 1\n```\n";
        let (units, authors) = scanned(doc);
        let h = authors.0[0].content_hash.clone();
        let import = cache(&[(&h, "1\n"), (&h, "2\n"), (&h, "3\n")]);
        let record = record_from_positron(&import, &units, &authors, None, &owner);
        assert_eq!(record.cells.len(), 2);
        assert_eq!(unclaimed(&record), 1);
    }

    #[test]
    fn a_unit_the_cache_does_not_cover_is_not_an_unclaimed_cell() {
        let doc = "```{python}\nx = 1\n```\n\n```{python}\ny = 2\n```\n";
        let (units, authors) = scanned(doc);
        let import = cache(&[(&authors.0[0].content_hash, "only the first ran\n")]);
        let record = record_from_positron(&import, &units, &authors, None, &owner);
        assert_eq!(record.cells.len(), 1);
        assert_eq!(unclaimed(&record), 0);
    }

    // Staleness (Phase B) ----------------------------------------------------------------

    fn record_for(doc: &str, cached: &[(&str, &str)]) -> Record {
        let (units, authors) = scanned(doc);
        // `cached` names the text of each cached cell; hash it as Positron does.
        let pairs: Vec<(String, &str)> = cached
            .iter()
            .map(|(code, out)| (content_hash(code), *out))
            .collect();
        let refs: Vec<(&str, &str)> = pairs.iter().map(|(h, o)| (h.as_str(), *o)).collect();
        record_from_positron(&cache(&refs), &units, &authors, None, &owner)
    }

    fn stale(record: &Record, doc: &str) -> bool {
        imported_record_is_stale(record, doc, &[])
    }

    const TWO: &str = "```{python}\nx = 1\n```\n\n```{python}\ny = 2\n```\n";

    #[test]
    fn an_unchanged_imported_page_is_not_stale() {
        let record = record_for(TWO, &[("x = 1", "a\n"), ("y = 2", "b\n")]);
        assert!(!stale(&record, TWO));
    }

    #[test]
    fn an_edited_unit_leaves_its_old_cell_orphaned_and_flags_the_page() {
        let record = record_for(TWO, &[("x = 1", "a\n"), ("y = 2", "b\n")]);
        assert!(stale(&record, &TWO.replace("y = 2", "y = 3")));
    }

    #[test]
    fn a_deleted_unit_flags_the_page() {
        let record = record_for(TWO, &[("x = 1", "a\n"), ("y = 2", "b\n")]);
        assert!(stale(&record, "```{python}\nx = 1\n```\n"));
    }

    #[test]
    fn a_cache_older_than_the_document_is_stale_at_publish_through_the_unclaimed_count() {
        // The cache holds an output for code the document no longer has; nothing is orphaned
        // in the record (the conversion never built that cell), so only the count says so.
        let record = record_for(TWO, &[("x = 1", "a\n"), ("old = 0", "gone\n")]);
        assert!(stale(&record, TWO));
    }

    #[test]
    fn a_unit_the_cache_does_not_cover_does_not_flag_the_page() {
        // `y = 2` has no cell and nothing is orphaned: the page lacks that output and stays
        // quiet. `Matching::staleness()` would call that unit Stale; this rule ignores it.
        let record = record_for(TWO, &[("x = 1", "a\n")]);
        assert!(!stale(&record, TWO));
    }

    #[test]
    fn a_chunk_the_gate_refuses_does_not_flag_the_page() {
        let doc = "```{python}\nx = 1\n```\n\n```{ojs}\nz = 3\n```\n";
        let (units, authors) = scanned(doc);
        let import = cache(&[
            (&content_hash("x = 1"), "a\n"),
            (&content_hash("z = 3"), "ojs\n"),
        ]);
        let only_python = |l: &str| {
            if l == "python" {
                "jupyter"
            } else {
                "observable"
            }
            .to_string()
        };
        let record = record_from_positron(&import, &units, &authors, None, &only_python);
        assert_eq!(record.cells.len(), 1);
        assert!(
            !stale(&record, doc),
            "the refused chunk's cache cell is claimed"
        );
    }

    #[test]
    fn imported_means_the_marker_and_no_engines() {
        let mut record = record_for(TWO, &[("x = 1", "a\n")]);
        assert!(is_imported_record(&record));
        record.metadata.quarto_engines.push(
            serde_json::from_value(json!({
                "name": "jupyter", "can_freeze": true, "document_hash": "h",
                "hash_definition": crate::record::HashDefinition::conservative("jupyter")
            }))
            .unwrap(),
        );
        assert!(
            !is_imported_record(&record),
            "a hand-edited copy with engines takes the normal path"
        );
        record.metadata.quarto_engines.clear();
        record.metadata.extra.remove(PROVENANCE_KEY);
        assert!(!is_imported_record(&record), "no marker, not imported");
    }

    #[test]
    fn an_imported_cell_carries_no_header_keys_so_the_demotion_is_a_no_op() {
        let record = record_for(TWO, &[("x = 1", "a\n")]);
        assert!(record.cells.iter().all(|c| {
            !c.metadata
                .extra
                .contains_key(crate::record::HEADER_KEYS_KEY)
        }));
    }
}
