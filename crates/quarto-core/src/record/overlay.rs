//! Overlay-free hashes for a record executed under a `-M` overlay (EXR-5; F8-D9).
//!
//! `provide-hub` executes with a runtime overlay (`RenderOverlay`) whose metadata the
//! metadata merge folds over the document's, so the hashes an engine computed describe the
//! overlaid run, not the source a viewer recomputes from. The engine stage cannot undo that
//! merge (an overlay key that overrode a document key loses the original value), so the
//! provider recomputes from the source project, reusing the pre-engine run it already does
//! for the capture's `input_qmd` (`preview_record::compute_source_inputs`). The unit hashes
//! are untouched: the overlay changes metadata, never cell text or options.

use std::collections::BTreeMap;

use serde_json::Value;

use super::exec_hash::{HashError, UnitHashRef, hash_document, hash_record};
use super::types::Record;
use crate::engine::preview_record::{CaptureRun, SourceInputs};
use crate::overlay_runtime::RenderOverlay;

/// Recompute every engine entry's `document_hash`, and the record's
/// `quarto_execution_hash`, over `metadata` (the overlay-free merged metadata) with each
/// entry's own stored declaration. Cells keep their unit hashes.
pub fn rehash_record(
    record: &mut Record,
    metadata: &BTreeMap<String, Value>,
) -> Result<(), HashError> {
    for entry in &mut record.metadata.quarto_engines {
        let refs: Vec<UnitHashRef> = record
            .cells
            .iter()
            .filter(|c| c.metadata.quarto_engine.as_deref() == Some(entry.name.as_str()))
            .map(|c| UnitHashRef {
                hash: &c.metadata.quarto_exec_hash,
                profile: c.metadata.quarto_hash_rule.as_deref(),
            })
            .collect();
        entry.document_hash = hash_document(&entry.hash_definition, metadata, &refs)?;
    }
    let document_hashes: Vec<&str> = record
        .metadata
        .quarto_engines
        .iter()
        .map(|e| e.document_hash.as_str())
        .collect();
    record.metadata.quarto_execution_hash = Some(hash_record(&document_hashes));
    Ok(())
}

/// The execution-relevant part of an overlay's metadata, for `quarto_overlay`
/// (informational, never hashed): the keys the document hash reads (`engine`,
/// `execute-dir`, `params`, `execute`) and every key an engine's declaration names in
/// `document_keys`.
pub fn overlay_difference(overlay: &Value, record: &Record) -> BTreeMap<String, Value> {
    let Value::Object(overlay) = overlay else {
        return BTreeMap::new();
    };
    let mut keys: Vec<&str> = vec!["engine", "execute-dir", "params", "execute"];
    for entry in &record.metadata.quarto_engines {
        keys.extend(
            entry
                .hash_definition
                .document_keys
                .iter()
                .map(String::as_str),
        );
    }
    overlay
        .iter()
        .filter(|(k, _)| keys.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Make a capture run executed under a `-M` overlay describe the source (EXR-5;
/// F8-D9): the first capture's `input_qmd` becomes the overlay-free bytes, and
/// the record's unit hashes, document hashes and `quarto_execution_hash` become
/// the overlay-free ones, with the overlay's execution-relevant keys noted in
/// `quarto_overlay`. The outputs are untouched: they were produced under the
/// overlay, which is the point of `-M`.
pub fn overlay_free_run(
    mut run: CaptureRun,
    source: &SourceInputs,
    overlay: &RenderOverlay,
) -> Result<CaptureRun, String> {
    if let Some(first) = run.captures.first_mut() {
        first.input_qmd = String::from_utf8(source.input_qmd.clone())
            .map_err(|e| format!("source input_qmd is not UTF-8: {e}"))?;
    }
    if let Some(record) = run.record.as_mut() {
        rehash_record(record, &source.metadata)
            .map_err(|e| format!("recomputing the record's overlay-free hashes failed: {e}"))?;
        let noted = overlay
            .metadata
            .as_ref()
            .map(|m| overlay_difference(m, record))
            .unwrap_or_default();
        record.metadata.quarto_overlay = (!noted.is_empty()).then_some(noted);
    }
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::units::scan_units;
    use crate::record::{
        AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, coalesce,
        fill_ids,
    };
    use serde_json::json;

    fn record_with(metadata: BTreeMap<String, Value>) -> Record {
        let doc = "```{python}\nx = 1\n```\n";
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        let mut def = HashDefinition::conservative("jupyter");
        def.document_keys = vec!["jupyter".into()];
        let mut b = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
        b.push(0, UnitResult::default()).unwrap();
        let engine = b
            .finish(EngineEntryInput {
                document_metadata: metadata,
                ..Default::default()
            })
            .unwrap();
        coalesce(&[engine], &units).unwrap()
    }

    fn metadata(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn rehash_restores_the_overlay_free_hashes() {
        let source = metadata(&[("jupyter", json!("python3"))]);
        let overlaid = metadata(&[("jupyter", json!("python3")), ("params", json!({"n": 2}))]);
        let plain = record_with(source.clone());
        let mut under_overlay = record_with(overlaid);
        assert_ne!(
            plain.metadata.quarto_engines[0].document_hash,
            under_overlay.metadata.quarto_engines[0].document_hash
        );

        rehash_record(&mut under_overlay, &source).unwrap();

        assert_eq!(
            under_overlay.metadata.quarto_engines[0].document_hash,
            plain.metadata.quarto_engines[0].document_hash
        );
        assert_eq!(
            under_overlay.metadata.quarto_execution_hash,
            plain.metadata.quarto_execution_hash
        );
        assert_eq!(
            under_overlay.cells, plain.cells,
            "unit hashes are untouched"
        );
    }

    #[test]
    fn overlay_difference_keeps_only_execution_relevant_keys() {
        let record = record_with(BTreeMap::new());
        let overlay = json!({
            "subtitle": "ZZ",
            "params": { "n": 2 },
            "execute": { "echo": false },
            "jupyter": "other",
        });
        let diff = overlay_difference(&overlay, &record);
        assert_eq!(
            diff,
            metadata(&[
                ("execute", json!({ "echo": false })),
                ("jupyter", json!("other")),
                ("params", json!({ "n": 2 })),
            ])
        );
        assert!(overlay_difference(&json!("not a map"), &record).is_empty());
    }
}
