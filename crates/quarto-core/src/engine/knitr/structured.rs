/*
 * engine/knitr/structured.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! KNR-3: turning a cell that `RecordBuilder::push` wrote under the default profile into a
//! **structured** knitr cell, and rehashing the engine entry over the result.
//!
//! `push` hashes every cell under the default profile (`profile: None`), because the builder is
//! shared with engines that have no profile. A structured knitr cell names the `structured` profile
//! (`quarto_hash_rule`), carries the output format as the flat option
//! [`OUTPUT_FORMAT_OPTION`](super::hash_rule::OUTPUT_FORMAT_OPTION), and stores as flat keys only the
//! options its own profile counts (KNR-3 decision 2, settled 2026-11-03). Done after `push`, through
//! EXR-4's public API alone: no parent change (the spike of decision 8).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::record::{
    EngineRecord, HashDefinition, HashError, RecordCell, UnitHashInput, UnitHashRef,
    execution_options, hash_document, hash_unit, partition_author_text,
};

use super::hash_rule::{OUTPUT_FORMAT_OPTION, STRUCTURED_PROFILE};

/// Make `cell` (pushed under the default profile) a structured cell for `format`.
///
/// Recomputes `quarto_exec_hash` under the `structured` profile with the format counted, names the
/// profile in `quarto_hash_rule`, and replaces the flat option keys with the options that profile
/// counts. Internal `quarto_` keys already on the cell are kept. `attr_tail` is the unit\'s brace
/// tail, which `push` hashed from the scanned unit and the cell does not carry.
pub(crate) fn mark_structured(
    def: &HashDefinition,
    cell: &mut RecordCell,
    attr_tail: &str,
    format: &str,
) -> Result<(), HashError> {
    let author_text = cell.source.concat();
    let (mut options, code) = partition_author_text(&cell.metadata.quarto_lang, &author_text)?;
    options.insert(OUTPUT_FORMAT_OPTION.to_string(), json!(format));
    cell.metadata.quarto_exec_hash = hash_unit(
        def,
        &UnitHashInput {
            kind: cell.metadata.quarto_kind,
            lang: &cell.metadata.quarto_lang,
            code: &code,
            options: &options,
            attr_tail,
            profile: Some(STRUCTURED_PROFILE),
        },
    )?;
    cell.metadata.quarto_hash_rule = Some(STRUCTURED_PROFILE.to_string());
    let flat = execution_options(def, Some(STRUCTURED_PROFILE), &options)?;
    cell.metadata.extra.retain(|k, _| k.starts_with("quarto_"));
    cell.metadata.extra.extend(flat);
    Ok(())
}

/// Recompute `record`\'s document hash from its cells\' current hashes and profiles, over the merged
/// document metadata, with the entry\'s own stored declaration.
pub(crate) fn rehash_document(
    record: &mut EngineRecord,
    document_metadata: &BTreeMap<String, Value>,
) -> Result<(), HashError> {
    let refs: Vec<UnitHashRef> = record
        .cells
        .iter()
        .map(|c| UnitHashRef {
            hash: &c.metadata.quarto_exec_hash,
            profile: c.metadata.quarto_hash_rule.as_deref(),
        })
        .collect();
    record.engine.document_hash =
        hash_document(&record.engine.hash_definition, document_metadata, &refs)?;
    Ok(())
}
