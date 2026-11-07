//! Coalescing per-engine records into one [`Record`] (EXR-4 Phase 3).
//!
//! Cells are ordered by the stage's single unit list (epic decision 38): no renumbering and no
//! use of source positions, since every engine's ids already came from that list. Each cell
//! keeps its `quarto_engine`; engines stay in execution order.
//!
//! The returned record has `quarto_source`, `quarto_files` and `quarto_overlay` unset: they
//! come from the pipeline, not from any engine, and the caller sets them on the (public) fields.

use std::collections::{BTreeMap, HashMap};

use crate::engine::units::Unit;

use super::exec_hash::hash_record;
use super::types::{EngineRecord, Record, RecordCell, RecordMetadata};

/// Why engine records could not be coalesced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoalesceError {
    /// An engine record holds a cell for a unit the stage's list does not have, so it cannot
    /// be placed. Reported rather than dropped: it means an engine and the stage disagree.
    #[error("engine `{engine}` has a cell `{cell_id}` that matches no unit in the stage's list")]
    UnknownCell { engine: String, cell_id: String },
    /// Two engines (or one twice) recorded the same unit.
    #[error("unit `{cell_id}` was recorded by both `{first}` and `{second}`")]
    DuplicateCell {
        cell_id: String,
        first: String,
        second: String,
    },
}

fn unit_id(unit: &Unit) -> Option<&str> {
    match unit {
        Unit::Cell(c) => c.id.as_ref().map(|i| i.0.as_str()),
        Unit::Inline(i) => i.id.as_ref().map(|i| i.0.as_str()),
    }
}

/// Merge `engine_records` into one record, ordering cells by `unit_order`.
pub fn coalesce(
    engine_records: &[EngineRecord],
    unit_order: &[Unit],
) -> Result<Record, CoalesceError> {
    let position: HashMap<&str, usize> = unit_order
        .iter()
        .enumerate()
        .filter_map(|(i, u)| unit_id(u).map(|id| (id, i)))
        .collect();

    let mut placed: BTreeMap<usize, (&str, &RecordCell)> = BTreeMap::new();
    for record in engine_records {
        for cell in &record.cells {
            let id = cell.metadata.quarto_cell_id.as_str();
            let Some(&pos) = position.get(id) else {
                return Err(CoalesceError::UnknownCell {
                    engine: record.engine.name.clone(),
                    cell_id: id.to_string(),
                });
            };
            if let Some((first, _)) = placed.insert(pos, (&record.engine.name, cell)) {
                return Err(CoalesceError::DuplicateCell {
                    cell_id: id.to_string(),
                    first: first.to_string(),
                    second: record.engine.name.clone(),
                });
            }
        }
    }

    // nbformat cell ids are unique across the record: the content hash, with `-2`, `-3` for
    // later duplicates. Based on the content hash, never on a stored id, so coalescing again
    // gives the same ids.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let cells: Vec<RecordCell> = placed
        .into_values()
        .map(|(_, cell)| {
            let hash = cell.metadata.quarto_content_hash.as_str();
            let count = seen.entry(hash).or_insert(0);
            *count += 1;
            let mut cell = cell.clone();
            cell.id = if *count == 1 {
                hash.to_string()
            } else {
                format!("{hash}-{count}")
            };
            cell
        })
        .collect();

    let document_hashes: Vec<&str> = engine_records
        .iter()
        .map(|r| r.engine.document_hash.as_str())
        .collect();
    Ok(Record {
        nbformat: Record::NBFORMAT,
        nbformat_minor: Record::NBFORMAT_MINOR,
        metadata: RecordMetadata {
            quarto_record_version: Record::RECORD_VERSION,
            quarto_cache_version: Some(Record::CACHE_VERSION),
            quarto_engines: engine_records.iter().map(|r| r.engine.clone()).collect(),
            quarto_execution_hash: Some(hash_record(&document_hashes)),
            ..Default::default()
        },
        cells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::units::scan_units;
    use crate::record::{
        AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, fill_ids,
    };

    fn staged(doc: &str) -> (Vec<Unit>, AuthorTexts) {
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        (units, texts)
    }

    /// One engine record: the engine runs every unit whose language is in `langs`.
    fn engine_record(
        name: &str,
        langs: &[&str],
        units: &[Unit],
        texts: &AuthorTexts,
    ) -> EngineRecord {
        let def = HashDefinition::conservative(name);
        let mut b = RecordBuilder::new(name, &def, units, texts).unwrap();
        for (i, u) in units.iter().enumerate() {
            if langs.contains(&u.lang()) {
                b.push(i, UnitResult::default()).unwrap();
            }
        }
        b.finish(EngineEntryInput::default()).unwrap()
    }

    const INTERLEAVED: &str = "\
```{python}
a = 1
```

```{r}
b <- 2
```

```{python}
c = 3
```

```{r}
d <- 4
```
";

    fn langs(rec: &Record) -> Vec<&str> {
        rec.cells
            .iter()
            .map(|c| c.metadata.quarto_lang.as_str())
            .collect()
    }

    #[test]
    fn two_interleaved_engines_come_out_in_unit_order_each_keeping_its_tag() {
        let (units, texts) = staged(INTERLEAVED);
        let py = engine_record("jupyter", &["python"], &units, &texts);
        let r = engine_record("knitr", &["r"], &units, &texts);
        let rec = coalesce(&[py, r], &units).unwrap();

        assert_eq!(langs(&rec), ["python", "r", "python", "r"]);
        let engines: Vec<_> = rec
            .cells
            .iter()
            .map(|c| c.metadata.quarto_engine.as_deref().unwrap())
            .collect();
        assert_eq!(engines, ["jupyter", "knitr", "jupyter", "knitr"]);
        // Ids come straight from the stage's list, with no renumbering.
        let ids: Vec<_> = rec
            .cells
            .iter()
            .map(|c| c.metadata.quarto_cell_id.split('-').next().unwrap())
            .collect();
        assert_eq!(ids, ["0", "1", "2", "3"]);
        // Engines stay in execution order, whatever the unit order.
        let names: Vec<_> = rec
            .metadata
            .quarto_engines
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["jupyter", "knitr"]);
    }

    #[test]
    fn engine_order_is_execution_order_and_changes_the_record_hash() {
        let (units, texts) = staged(INTERLEAVED);
        let py = engine_record("jupyter", &["python"], &units, &texts);
        let r = engine_record("knitr", &["r"], &units, &texts);
        let a = coalesce(&[py.clone(), r.clone()], &units).unwrap();
        let b = coalesce(&[r, py], &units).unwrap();
        assert_eq!(a.cells, b.cells, "cell order never depends on engine order");
        assert_ne!(
            a.metadata.quarto_execution_hash, b.metadata.quarto_execution_hash,
            "the record hash covers the ordered per-engine document hashes"
        );
        assert_eq!(a.metadata.quarto_execution_hash.as_ref().unwrap().len(), 64);
    }

    #[test]
    fn an_engine_with_no_units_keeps_its_entry() {
        let (units, texts) = staged(INTERLEAVED);
        let py = engine_record("jupyter", &["python"], &units, &texts);
        let idle = engine_record("julia", &["julia"], &units, &texts);
        let rec = coalesce(&[py, idle], &units).unwrap();
        assert_eq!(rec.cells.len(), 2);
        assert_eq!(rec.metadata.quarto_engines.len(), 2);
        assert_eq!(rec.metadata.quarto_engines[1].name, "julia");
    }

    #[test]
    fn duplicate_identical_cells_keep_an_occurrence_index() {
        let doc = "```{python}\nx = 1\n```\n\n```{python}\nx = 1\n```\n\n```{python}\nx = 1\n```\n";
        let (units, texts) = staged(doc);
        let rec = coalesce(
            &[engine_record("jupyter", &["python"], &units, &texts)],
            &units,
        )
        .unwrap();
        let hash = &rec.cells[0].metadata.quarto_content_hash;
        let ids: Vec<&str> = rec.cells.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            [hash.clone(), format!("{hash}-2"), format!("{hash}-3")]
        );
        // The index-bearing id stays distinct, and the nbformat ids are valid ones.
        let cell_ids: Vec<&str> = rec
            .cells
            .iter()
            .map(|c| c.metadata.quarto_cell_id.as_str())
            .collect();
        assert_eq!(
            cell_ids
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            3
        );
        assert!(ids.iter().all(|i| i.len() <= 64));
    }

    #[test]
    fn coalescing_is_idempotent_on_the_nbformat_ids() {
        let doc = "```{python}\nx = 1\n```\n\n```{python}\nx = 1\n```\n";
        let (units, texts) = staged(doc);
        let first = coalesce(
            &[engine_record("jupyter", &["python"], &units, &texts)],
            &units,
        )
        .unwrap();
        let again = EngineRecord {
            engine: first.metadata.quarto_engines[0].clone(),
            cells: first.cells.clone(),
        };
        assert_eq!(coalesce(&[again], &units).unwrap().cells, first.cells);
    }

    #[test]
    fn a_cell_with_no_unit_is_reported_not_dropped() {
        let (units, texts) = staged(INTERLEAVED);
        let py = engine_record("jupyter", &["python"], &units, &texts);
        // The stage's list lost its last unit, so the engine's cell for it cannot be placed.
        let fewer = &units[..units.len() - 1];
        let r = engine_record("knitr", &["r"], &units, &texts);
        let err = coalesce(&[py, r], fewer).unwrap_err();
        assert!(
            matches!(&err, CoalesceError::UnknownCell { engine, cell_id }
                if engine == "knitr" && cell_id.starts_with("3-")),
            "{err:?}"
        );
    }

    #[test]
    fn a_unit_recorded_twice_is_reported() {
        let (units, texts) = staged(INTERLEAVED);
        let a = engine_record("jupyter", &["python"], &units, &texts);
        let b = engine_record("other", &["python"], &units, &texts);
        let err = coalesce(&[a, b], &units).unwrap_err();
        assert!(
            matches!(&err, CoalesceError::DuplicateCell { first, second, .. }
                if first == "jupyter" && second == "other"),
            "{err:?}"
        );
    }

    /// Mixed engines, each with its own declaration (Phase 2 items that needed the builder).
    fn mixed() -> (
        Vec<Unit>,
        Vec<EngineRecord>,
        BTreeMap<String, serde_json::Value>,
    ) {
        let (units, texts) = staged(INTERLEAVED);
        let md: BTreeMap<String, serde_json::Value> = BTreeMap::from([
            (
                "execute".into(),
                serde_json::json!({"echo": false, "error": true}),
            ),
            ("knitr".into(), serde_json::json!({"opts_chunk": {"a": 1}})),
        ]);
        let mut jupyter = HashDefinition::conservative("jupyter");
        jupyter.presentation_execute_keys = vec!["echo".into()];
        let mut knitr = HashDefinition::conservative("knitr");
        knitr.document_keys = vec!["knitr".into()];
        let records = [("jupyter", "python", jupyter), ("knitr", "r", knitr)]
            .into_iter()
            .map(|(name, lang, def)| {
                let mut b = RecordBuilder::new(name, &def, &units, &texts).unwrap();
                for (i, u) in units.iter().enumerate() {
                    if u.lang() == lang {
                        b.push(i, UnitResult::default()).unwrap();
                    }
                }
                b.finish(EngineEntryInput {
                    document_metadata: md.clone(),
                    ..Default::default()
                })
                .unwrap()
            })
            .collect();
        (units, records, md)
    }

    #[test]
    fn each_engines_document_hash_uses_its_own_definition() {
        let (units, records, _) = mixed();
        let rec = coalesce(&records, &units).unwrap();
        let hashes: Vec<&str> = rec
            .metadata
            .quarto_engines
            .iter()
            .map(|e| e.document_hash.as_str())
            .collect();
        assert_ne!(hashes[0], hashes[1]);
        assert_eq!(
            rec.metadata.quarto_execution_hash.as_deref(),
            Some(hash_record(&hashes).as_str())
        );
    }

    #[test]
    fn a_record_reproduces_its_hashes_from_its_stored_declarations_alone() {
        use crate::record::{UnitHashRef, hash_document, recompute_cell_hash};
        let (units, records, md) = mixed();
        let rec = coalesce(&records, &units).unwrap();
        let mut document_hashes = Vec::new();
        for entry in &rec.metadata.quarto_engines {
            // Unit hashes, from each cell and its engine's stored declaration.
            let mine: Vec<&RecordCell> = rec
                .cells
                .iter()
                .filter(|c| c.metadata.quarto_engine.as_deref() == Some(entry.name.as_str()))
                .collect();
            for c in &mine {
                assert_eq!(
                    recompute_cell_hash(&entry.hash_definition, c).unwrap(),
                    c.metadata.quarto_exec_hash
                );
            }
            let refs: Vec<UnitHashRef> = mine
                .iter()
                .map(|c| UnitHashRef {
                    hash: &c.metadata.quarto_exec_hash,
                    profile: c.metadata.quarto_hash_rule.as_deref(),
                })
                .collect();
            let doc = hash_document(&entry.hash_definition, &md, &refs).unwrap();
            assert_eq!(doc, entry.document_hash, "{}", entry.name);
            document_hashes.push(doc);
        }
        let refs: Vec<&str> = document_hashes.iter().map(String::as_str).collect();
        assert_eq!(
            rec.metadata.quarto_execution_hash.as_deref(),
            Some(hash_record(&refs).as_str())
        );
    }

    #[test]
    fn the_header_is_the_current_schema_with_caller_fields_unset() {
        let (units, texts) = staged(INTERLEAVED);
        let rec = coalesce(
            &[engine_record("jupyter", &["python"], &units, &texts)],
            &units,
        )
        .unwrap();
        assert_eq!(rec.nbformat, 4);
        assert_eq!(rec.nbformat_minor, 5);
        assert_eq!(rec.metadata.quarto_record_version, Record::RECORD_VERSION);
        assert_eq!(
            rec.metadata.quarto_cache_version,
            Some(Record::CACHE_VERSION)
        );
        assert!(rec.metadata.quarto_source.is_none());
        assert!(rec.metadata.quarto_files.is_empty());
        assert!(rec.metadata.quarto_overlay.is_none());
    }
}
