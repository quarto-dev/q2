//! Record-level staleness (EXR-15, epic decision 5).
//!
//! Does a stored record still describe the document? Answered from the record's own
//! declarations, with no engine loaded, so the same check runs in the preview, on the hub and
//! in WASM: every unit of the current masked, pre-engine document is hashed under the engine
//! whose stored `owned_languages` contains its language, each engine's document hash is
//! recomputed over those hashes and the current merged metadata, and the result is compared
//! with the hash stored in that engine's entry. Prose never enters a hash, so a prose edit
//! leaves a record fresh; editing code, an execution-relevant option or an execution-relevant
//! metadata key does not.
//!
//! This is the whole-document judgment. Per-unit staleness (which cell to dim) is the splice's
//! (EXR-14, [`super::match_units`]).

use std::collections::BTreeMap;

use serde_json::Value;

use super::author_text::{SugaredBlock, author_texts};
use super::exec_hash::{HashError, UnitHashRef, hash_document};
use super::matching::UnitKey;
use super::types::{EngineEntry, Record, UnitKind};
use crate::engine::units::{Unit, scan_units};

/// The engine whose recorded owned-language set contains `lang`.
fn engine_for<'a>(record: &'a Record, lang: &str) -> Option<&'a EngineEntry> {
    record.metadata.quarto_engines.iter().find(|e| {
        e.owned_languages
            .iter()
            .any(|l| l.eq_ignore_ascii_case(lang))
    })
}

/// Whether `record` no longer describes the document.
///
/// `input_qmd` is the masked, serialized pre-engine document (`SourceInputs::input_qmd`),
/// `sugared` the pre-sugaring snapshot of the same run (the author's text for each braced
/// block), and `metadata` the merged, overlay-free document metadata
/// (`SourceInputs::metadata`).
///
/// A unit whose options no longer parse counts as an edit: the record was written from valid
/// ones. An error is returned only when a stored declaration cannot be applied (for example
/// an unknown hash profile), so the caller can decline to judge.
pub fn record_is_stale(
    record: &Record,
    input_qmd: &str,
    sugared: &[SugaredBlock],
    metadata: &BTreeMap<String, Value>,
) -> Result<bool, HashError> {
    let units = scan_units(input_qmd);
    let texts = author_texts(&units, sugared);

    // Unit hashes per engine entry, in document order.
    let mut per_engine: Vec<Vec<String>> = vec![Vec::new(); record.metadata.quarto_engines.len()];
    for (unit, text) in units.iter().zip(&texts.0) {
        let Some(entry) = engine_for(record, unit.lang()) else {
            continue;
        };
        let kind = match unit {
            Unit::Cell(_) => UnitKind::Cell,
            Unit::Inline(_) => UnitKind::Inline,
        };
        // The brace tail is part of a cell's hash on the writer and splice sides (EXR-11).
        let tail = match unit {
            Unit::Cell(c) => c.attr_tail.as_str(),
            Unit::Inline(_) => "",
        };
        let key = match UnitKey::from_author_text(
            &entry.hash_definition,
            kind,
            unit.lang(),
            &text.text,
            tail,
        ) {
            Ok(key) => key,
            Err(HashError::InvalidOptions(_)) => return Ok(true),
            Err(e) => return Err(e),
        };
        let index = record
            .metadata
            .quarto_engines
            .iter()
            .position(|e| e.name == entry.name)
            .expect("engine_for returns an entry of the record");
        per_engine[index].push(key.exec_hash);
    }

    for (entry, hashes) in record.metadata.quarto_engines.iter().zip(&per_engine) {
        let refs: Vec<UnitHashRef> = hashes
            .iter()
            .map(|h| UnitHashRef {
                hash: h,
                profile: None,
            })
            .collect();
        let current = hash_document(&entry.hash_definition, metadata, &refs)?;
        if current != entry.document_hash {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{
        AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, coalesce,
        fill_ids,
    };
    use serde_json::json;

    /// Jupyter's declaration as the staleness check sees it: presentation options and
    /// `execute:` keys are excluded.
    fn jupyter_def() -> HashDefinition {
        let mut def = HashDefinition::conservative("jupyter");
        def.presentation_cell_options = vec!["echo".into(), "warning".into()];
        def.presentation_execute_keys = vec!["echo".into(), "warning".into()];
        def
    }

    /// Knitr declares empty exclusion lists: its presentation keys are in its hash.
    fn knitr_def() -> HashDefinition {
        HashDefinition::conservative("knitr")
    }

    fn meta(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    /// A record of `doc` written by `def` (owning `langs`) under `metadata`; every unit the
    /// engine owns gets a cell, as the engine stage pushes them.
    fn record_of(
        doc: &str,
        def: &HashDefinition,
        langs: &[&str],
        metadata: &BTreeMap<String, Value>,
    ) -> Record {
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        let mut b = RecordBuilder::new(def.id.clone(), def, &units, &texts).unwrap();
        for (i, u) in units.iter().enumerate() {
            if langs.contains(&u.lang()) {
                b.push(i, UnitResult::default()).unwrap();
            }
        }
        let engine = b
            .finish(EngineEntryInput {
                owned_languages: langs.iter().map(|l| l.to_string()).collect(),
                document_metadata: metadata.clone(),
                ..Default::default()
            })
            .unwrap();
        coalesce(&[engine], &units).unwrap()
    }

    fn stale(record: &Record, doc: &str, metadata: &BTreeMap<String, Value>) -> bool {
        record_is_stale(record, doc, &[], metadata).unwrap()
    }

    const DOC: &str = "# Title\n\nSome prose.\n\n```{python}\nx = 1\n```\n\nMore prose.\n";

    #[test]
    fn recorded_document_is_fresh() {
        let m = BTreeMap::new();
        let rec = record_of(DOC, &jupyter_def(), &["python"], &m);
        assert!(!stale(&rec, DOC, &m));
    }

    #[test]
    fn prose_only_edit_is_fresh() {
        let m = BTreeMap::new();
        let rec = record_of(DOC, &jupyter_def(), &["python"], &m);
        let edited = DOC.replace("Some prose.", "Entirely different prose, much longer.");
        assert!(!stale(&rec, &edited, &m));
    }

    #[test]
    fn code_edit_is_stale() {
        let m = BTreeMap::new();
        let rec = record_of(DOC, &jupyter_def(), &["python"], &m);
        assert!(stale(&rec, &DOC.replace("x = 1", "x = 2"), &m));
    }

    #[test]
    fn execution_relevant_cell_option_edit_is_stale() {
        let m = BTreeMap::new();
        let doc = "```{python}\n#| fig-width: 5\nx = 1\n```\n";
        let rec = record_of(doc, &jupyter_def(), &["python"], &m);
        assert!(stale(
            &rec,
            &doc.replace("fig-width: 5", "fig-width: 6"),
            &m
        ));
    }

    #[test]
    fn execution_relevant_front_matter_key_edit_is_stale() {
        let before = meta(&[("params", json!({"n": 1}))]);
        let after = meta(&[("params", json!({"n": 2}))]);
        let rec = record_of(DOC, &jupyter_def(), &["python"], &before);
        assert!(!stale(&rec, DOC, &before));
        assert!(stale(&rec, DOC, &after));
    }

    /// Epic decision 34: an `echo` edit on a Jupyter cell leaves the record fresh, so the
    /// preview shows the recorded output with the new presentation.
    #[test]
    fn jupyter_echo_option_edit_is_fresh() {
        let m = BTreeMap::new();
        let doc = "```{python}\n#| echo: true\nx = 1\n```\n";
        let rec = record_of(doc, &jupyter_def(), &["python"], &m);
        assert!(!stale(&rec, &doc.replace("echo: true", "echo: false"), &m));
    }

    /// Epic decision 35: the exclusion lists are per engine, applied to the stored
    /// declaration. `execute: echo` is in knitr's hash and not in Jupyter's.
    #[test]
    fn execute_echo_toggle_is_stale_for_knitr_and_fresh_for_jupyter() {
        let before = meta(&[("execute", json!({"echo": true}))]);
        let after = meta(&[("execute", json!({"echo": false}))]);
        let doc = "```{r}\nx <- 1\n```\n";
        let knitr = record_of(doc, &knitr_def(), &["r"], &before);
        assert!(stale(&knitr, doc, &after), "knitr: echo is in the hash");

        let jupyter = record_of(DOC, &jupyter_def(), &["python"], &before);
        assert!(!stale(&jupyter, DOC, &after), "jupyter: echo is excluded");
    }

    #[test]
    fn toggling_the_record_key_changes_neither_engine() {
        let before = meta(&[("record", json!(true))]);
        let after = meta(&[("record", json!(false))]);
        let knitr = record_of("```{r}\nx <- 1\n```\n", &knitr_def(), &["r"], &before);
        let jupyter = record_of(DOC, &jupyter_def(), &["python"], &before);
        assert!(!stale(&knitr, "```{r}\nx <- 1\n```\n", &after));
        assert!(!stale(&jupyter, DOC, &after));
    }

    #[test]
    fn a_unit_with_unparseable_options_counts_as_an_edit() {
        let m = BTreeMap::new();
        let doc = "```{python}\n#| fig-width: 5\nx = 1\n```\n";
        let rec = record_of(doc, &jupyter_def(), &["python"], &m);
        assert!(stale(
            &rec,
            &doc.replace("fig-width: 5", "fig-width: [5"),
            &m
        ));
    }

    #[test]
    fn an_added_unit_of_an_owned_language_is_stale() {
        let m = BTreeMap::new();
        let rec = record_of(DOC, &jupyter_def(), &["python"], &m);
        let more = format!("{DOC}\n```{{python}}\ny = 2\n```\n");
        assert!(stale(&rec, &more, &m));
    }

    #[test]
    fn a_unit_of_an_unowned_language_is_ignored() {
        let m = BTreeMap::new();
        let rec = record_of(DOC, &jupyter_def(), &["python"], &m);
        let more = format!("{DOC}\n```{{r}}\ny <- 2\n```\n");
        assert!(!stale(&rec, &more, &m));
    }
}
