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
use super::matching::{OUTPUT_FORMAT_OPTION, UnitKey, lost_header_keys, match_units};
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

    // For each unit of an owned language: its engine entry and its hash under the default
    // profile. A cell that names a hash profile hashes differently, which is settled once the
    // units are matched to cells below.
    let format = recorded_format(record);
    let mut owned: Vec<Option<(usize, String)>> = Vec::with_capacity(units.len());
    // The unit keys the splice would build; they also drive the header-key check.
    let mut keys: Vec<UnitKey> = Vec::with_capacity(units.len());
    for (unit, text) in units.iter().zip(&texts.0) {
        let kind = match unit {
            Unit::Cell(_) => UnitKind::Cell,
            Unit::Inline(_) => UnitKind::Inline,
        };
        let content_only = || UnitKey {
            kind,
            exec_hash: String::new(),
            content_hash: text.content_hash.clone(),
            alt_exec_hashes: Vec::new(),
            header_keys: None,
        };
        let Some(entry) = engine_for(record, unit.lang()) else {
            owned.push(None);
            keys.push(content_only());
            continue;
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
        owned.push(Some((index, key.exec_hash.clone())));
        // A cell naming a hash profile matches by a profile hash that counts the format.
        keys.push(match &format {
            Some(format) => UnitKey::from_author_text_with_profiles(
                &entry.hash_definition,
                kind,
                unit.lang(),
                &text.text,
                tail,
                format,
            )
            .unwrap_or_else(|_| content_only()),
            None => key,
        });
    }

    // The writer hashed a profiled cell under its profile (KNR-3 slice 9). A unit matched to
    // such a cell carries that cell's profile and hash into the document hash: the matching
    // already established that the unit's own hash under the profile equals it. Any other unit
    // (default profile, or matched to nothing) keeps its default-profile hash, so an edit that
    // matches no cell still changes the document hash.
    let matching = match_units(&keys, &record.cells);
    let mut per_engine: Vec<Vec<(String, Option<String>)>> =
        vec![Vec::new(); record.metadata.quarto_engines.len()];
    for (u, slot) in owned.iter().enumerate() {
        let Some((engine, default_hash)) = slot else {
            continue;
        };
        let profiled = matching.assignment[u]
            .map(|c| &record.cells[c].metadata)
            .filter(|m| m.quarto_hash_rule.is_some());
        per_engine[*engine].push(match profiled {
            Some(m) => (m.quarto_exec_hash.clone(), m.quarto_hash_rule.clone()),
            None => (default_hash.clone(), None),
        });
    }

    for (entry, hashes) in record.metadata.quarto_engines.iter().zip(&per_engine) {
        let refs: Vec<UnitHashRef> = hashes
            .iter()
            .map(|(hash, profile)| UnitHashRef {
                hash,
                profile: profile.as_deref(),
            })
            .collect();
        let current = hash_document(&entry.hash_definition, metadata, &refs)?;
        if current != entry.document_hash {
            return Ok(true);
        }
    }

    // The hashes say fresh. A unit that lost a header key its cell recorded is still stale
    // (KNR-3 slice 8); only the matched pairs count, so an unmatched unit (a prose edit never
    // makes one; a code edit already failed the hash check) does not.
    Ok(!lost_header_keys(&matching, &keys, &record.cells).is_empty())
}

/// The output format the record\'s hash-profile cells were recorded for: the flat option such a
/// cell carries. `None` when no cell names a profile.
fn recorded_format(record: &Record) -> Option<String> {
    record.cells.iter().find_map(|c| {
        c.metadata.quarto_hash_rule.as_ref()?;
        c.metadata
            .options()
            .find(|(k, _)| k.as_str() == OUTPUT_FORMAT_OPTION)
            .and_then(|(_, v)| v.as_str().map(str::to_string))
    })
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

    // ---- KNR-3 slice 8: a header key deleted from a unit whose cell recorded its keys.

    const HEADED: &str = "# T\n\nProse.\n\n```{python}\n#| echo: true\nx = 1\n```\n";

    /// `jupyter_def` excludes `echo` from the hash, so only the key predicate can see its deletion.
    fn with_recorded_keys(mut rec: Record, keys: &[&str]) -> Record {
        for cell in &mut rec.cells {
            cell.metadata
                .extra
                .insert("quarto_header_keys".into(), json!(keys));
        }
        rec
    }

    #[test]
    fn a_deleted_header_key_makes_the_record_stale() {
        let m = BTreeMap::new();
        let rec = with_recorded_keys(
            record_of(HEADED, &jupyter_def(), &["python"], &m),
            &["echo"],
        );
        assert!(!stale(&rec, HEADED, &m));
        assert!(stale(&rec, &HEADED.replace("#| echo: true\n", ""), &m));
    }

    #[test]
    fn an_added_key_a_changed_value_and_a_prose_edit_leave_the_record_fresh() {
        let m = BTreeMap::new();
        let rec = with_recorded_keys(
            record_of(HEADED, &jupyter_def(), &["python"], &m),
            &["echo"],
        );
        assert!(!stale(
            &rec,
            &HEADED.replace("#| echo: true\n", "#| echo: true\n#| warning: false\n"),
            &m
        ));
        assert!(!stale(
            &rec,
            &HEADED.replace("echo: true", "echo: false"),
            &m
        ));
        assert!(!stale(&rec, &HEADED.replace("Prose.", "Other prose."), &m));
    }

    /// The Jupyter guard (D2): its cells carry no key list, so a deleted presentation key
    /// leaves the record fresh and the preview does not re-run.
    #[test]
    fn a_jupyter_record_with_a_deleted_presentation_key_stays_fresh() {
        let m = BTreeMap::new();
        let rec = record_of(HEADED, &jupyter_def(), &["python"], &m);
        assert!(!stale(&rec, &HEADED.replace("#| echo: true\n", ""), &m));
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

    // ---- KNR-3 slice 9: a record whose cells name a hash profile.

    /// A knitr-shaped declaration: the default profile counts every option, the `structured`
    /// profile leaves `fig.cap` out.
    fn profiled_def() -> HashDefinition {
        let mut def = HashDefinition::conservative("knitr");
        def.profiles.insert(
            "structured".into(),
            crate::record::HashProfile {
                presentation_cell_options: vec!["fig.cap".into()],
                presentation_execute_keys: vec!["echo".into()],
            },
        );
        def
    }

    /// `record_of` with every cell moved to the `structured` profile the way the knitr adapter
    /// writes it: the format as a flat option, the presentation option gone, the unit hash
    /// recomputed under the profile and the document hash rehashed over those.
    fn profiled_record(doc: &str, metadata: &BTreeMap<String, Value>) -> Record {
        let def = profiled_def();
        let mut rec = record_of(doc, &def, &["r"], metadata);
        for cell in &mut rec.cells {
            cell.metadata.quarto_hash_rule = Some("structured".into());
            cell.metadata.extra.remove("fig.cap");
            cell.metadata
                .extra
                .insert(OUTPUT_FORMAT_OPTION.into(), json!("html"));
            cell.metadata.quarto_exec_hash =
                crate::record::recompute_cell_hash(&def, cell).unwrap();
        }
        crate::record::rehash_record(&mut rec, metadata).unwrap();
        rec
    }

    const PROFILED: &str =
        "# T\n\nProse.\n\n```{r}\n#| fig.cap: A\n#| warning: true\nplot(1)\n```\n";

    #[test]
    fn an_unchanged_profiled_record_is_fresh() {
        let m = BTreeMap::new();
        let rec = profiled_record(PROFILED, &m);
        assert!(!stale(&rec, PROFILED, &m));
    }

    #[test]
    fn a_profiled_record_goes_stale_on_code_an_execution_option_and_an_execute_key() {
        let m = BTreeMap::new();
        let rec = profiled_record(PROFILED, &m);
        assert!(stale(&rec, &PROFILED.replace("plot(1)", "plot(2)"), &m));
        assert!(stale(
            &rec,
            &PROFILED.replace("warning: true", "warning: false"),
            &m
        ));
        let with_execute = meta(&[("execute", json!({"cache": true}))]);
        assert!(stale(&rec, PROFILED, &with_execute));
    }

    #[test]
    fn a_presentation_option_edit_and_a_prose_edit_leave_a_profiled_record_fresh() {
        let m = BTreeMap::new();
        let rec = profiled_record(PROFILED, &m);
        assert!(!stale(
            &rec,
            &PROFILED.replace("fig.cap: A", "fig.cap: B"),
            &m
        ));
        assert!(!stale(
            &rec,
            &PROFILED.replace("Prose.", "Other prose."),
            &m
        ));
        // `echo` is a presentation `execute:` key under the profile, so it is not in the hash.
        let echo_off = meta(&[("execute", json!({"echo": false}))]);
        let rec = profiled_record(PROFILED, &m);
        assert!(!stale(&rec, PROFILED, &echo_off));
    }

    /// The whole-document leg of slice 8, which a real knitr record could not reach before.
    #[test]
    fn a_deleted_header_key_makes_a_profiled_record_stale() {
        let m = BTreeMap::new();
        let rec = with_recorded_keys(profiled_record(PROFILED, &m), &["fig.cap", "warning"]);
        assert!(!stale(&rec, PROFILED, &m));
        assert!(stale(&rec, &PROFILED.replace("#| fig.cap: A\n", ""), &m));
    }

    #[test]
    fn an_added_unit_makes_a_profiled_record_stale() {
        let m = BTreeMap::new();
        let rec = profiled_record(PROFILED, &m);
        let more = format!("{PROFILED}\n```{{r}}\ny <- 2\n```\n");
        assert!(stale(&rec, &more, &m));
    }

    /// Fallback cells name no profile and are hashed under the default one, beside profiled
    /// cells of the same engine.
    #[test]
    fn a_record_mixing_profiled_and_default_cells_is_fresh_when_unchanged() {
        let m = BTreeMap::new();
        let doc = format!("{PROFILED}\n```{{r}}\n#| warning: true\ny <- 2\n```\n");
        let def = profiled_def();
        let mut rec = record_of(&doc, &def, &["r"], &m);
        // Only the first cell moves to the profile; the second stays a default-profile cell.
        let cell = &mut rec.cells[0];
        cell.metadata.quarto_hash_rule = Some("structured".into());
        cell.metadata.extra.remove("fig.cap");
        cell.metadata
            .extra
            .insert(OUTPUT_FORMAT_OPTION.into(), json!("html"));
        cell.metadata.quarto_exec_hash = crate::record::recompute_cell_hash(&def, cell).unwrap();
        crate::record::rehash_record(&mut rec, &m).unwrap();
        assert!(!stale(&rec, &doc, &m));
        assert!(stale(&rec, &doc.replace("y <- 2", "y <- 3"), &m));
    }
}
