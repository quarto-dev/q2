//! The record's types: notebook, cells, engine entries, and the builder's inputs.
//!
//! The schema reference is in the EXR-4 plan. Maps are `BTreeMap` so serialization is
//! deterministic. Unknown `quarto_` keys survive a read and write (forward compatibility): a
//! flattened `extra` map on the notebook metadata, on the engine entry and on each cell's
//! metadata catches them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::hash_definition::HashDefinition;

/// One execution record: an nbformat 4.5 notebook with `quarto_` metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub nbformat: u32,
    pub nbformat_minor: u32,
    pub metadata: RecordMetadata,
    pub cells: Vec<RecordCell>,
}

impl Record {
    pub const NBFORMAT: u32 = 4;
    pub const NBFORMAT_MINOR: u32 = 5;
    /// q2's schema version.
    pub const RECORD_VERSION: u32 = 1;
    /// Emitted only while the schema subset really is Positron-compatible.
    pub const CACHE_VERSION: u32 = 1;
}

/// Notebook `metadata`: flat keys.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RecordMetadata {
    /// q2's schema version (required).
    pub quarto_record_version: u32,
    /// Positron's cache version, kept while the schemas stay compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_cache_version: Option<u32>,
    /// Project-relative document path, forward slashes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_source: Option<String>,
    /// One entry per engine that ran, in execution order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quarto_engines: Vec<EngineEntry>,
    /// Supporting files by doc-relative forward-slash path.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub quarto_files: BTreeMap<String, RecordFile>,
    /// Hash over the ordered per-engine document hashes (what record-level staleness compares).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_execution_hash: Option<String>,
    /// Informational only: the execution-relevant keys an overlay contributed. Never hashed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_overlay: Option<BTreeMap<String, Value>>,
    /// Reserved, parsed and ignored; the writer never emits it (Phase 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_files_ref: Option<Value>,
    /// Unknown keys, so a newer record survives a read and write.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A supporting file in `quarto_files`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordFile {
    /// Base64 (standard alphabet, padded) contents.
    pub data: String,
    /// Decoded size in bytes.
    pub size: u64,
}

/// One engine's entry in `quarto_engines`: identity, its hash declaration and document hash,
/// and its folded `ExecuteResult`. Each engine's result is kept separate and applied in order
/// at consumption, so no cross-engine merge is needed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineEntry {
    pub name: String,
    /// Engine version, kernelspec name and language, executor version, platform.
    /// Informational only; in no hash.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, Value>,
    pub can_freeze: bool,
    /// The engine's full declaration, so a consumer with no engine loaded can recompute.
    pub hash_definition: HashDefinition,
    /// This engine's document-level execution-input hash.
    pub document_hash: String,
    /// The languages the engine owned for this document.
    #[serde(default)]
    pub owned_languages: Vec<String>,

    // The folded `ExecuteResult` (`engine/context.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub includes: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub preserve: BTreeMap<String, String>,
    #[serde(default)]
    pub needs_postprocess: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub html_dependencies: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pandoc: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Value>,

    /// The option names the engine knows, from the engine that ran (knitr:
    /// `names(knitr::opts_chunk$get())`). The presenter subtracts them from a chunk's current
    /// options to find the ones to forward as cell attributes. Absent means no filtering, as for
    /// Jupyter and records written before the field existed; optional, so
    /// `quarto_record_version` stays 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_options: Option<Vec<String>>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// nbformat cell type. A record holds only code cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CellType {
    Code,
}

/// A record cell: one execution unit with its outputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordCell {
    pub cell_type: CellType,
    /// The count the kernel reported; `null` when unknown.
    pub execution_count: Option<u64>,
    /// nbformat 4.5 cell id: the content hash with `-2`, `-3` for later duplicates. Never
    /// the index-bearing `quarto_cell_id`.
    pub id: String,
    pub metadata: CellMetadata,
    /// nbformat outputs as JSON values.
    pub outputs: Vec<Value>,
    /// The author text as a line array (every line but the last keeps its newline).
    pub source: Vec<String>,
}

/// Whether a unit is a fenced cell or an inline span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitKind {
    Cell,
    Inline,
}

/// Where a unit sits when it is not in the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitLocation {
    /// A span in the document's own front matter; never spliceable.
    Metadata,
}

/// A cell's metadata: Positron's three keys, q2's keys, and the execution-relevant options as
/// flat keys under their Quarto names (isomorphic to executable-block metadata, so
/// `fig-width: 7` in a `#|` line is `"fig-width": 7` here).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellMetadata {
    pub quarto_cell_id: String,
    /// Positron's content hash: the key for matching.
    pub quarto_content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_label: Option<String>,
    pub quarto_kind: UnitKind,
    /// The full language token as written (every comparison of it is case-insensitive).
    pub quarto_lang: String,
    /// Absent in a record that predates engine tags; engines then derive from ownership.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_engine: Option<String>,
    /// Provenance only, in no hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_kernel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_location: Option<UnitLocation>,
    /// The unit's execution-input hash.
    pub quarto_exec_hash: String,
    /// The hash profile the unit was hashed under; absent means the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_hash_rule: Option<String>,
    /// The execution-relevant options (flat keys) and any unknown `quarto_` keys.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl CellMetadata {
    /// The execution-relevant options: every flat key that is not a `quarto_` key.
    pub fn options(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.extra.iter().filter(|(k, _)| !k.starts_with("quarto_"))
    }
}

/// The author's text for one unit, as Positron hashes it (epic decision 36).
///
/// `text` is the lines strictly between the fences, split on `\r?\n` and rejoined with `\n`,
/// with the `#|` lines (label and caption lines included) exactly as written; for a span it
/// is the expression. `content_hash` is the Positron hash of `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorText {
    pub text: String,
    pub content_hash: String,
}

/// One [`AuthorText`] per unit, aligned index-for-index with the stage's unit list, so it is
/// total: there is never a gap.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuthorTexts(pub Vec<AuthorText>);

impl AuthorTexts {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, unit_index: usize) -> Option<&AuthorText> {
        self.0.get(unit_index)
    }
}

/// What an engine reports for one unit it ran: the only inputs the builder takes from the
/// engine, so code an engine trimmed or rewrote cannot change matching.
/// `KernelExecuteResult`-shaped: Jupyter converts from it and a TS engine fills it from its
/// notebook's cells.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnitResult {
    /// nbformat outputs as JSON values.
    pub outputs: Vec<Value>,
    /// The execution count the kernel reported.
    pub execution_count: Option<u64>,
    /// The kernel that ran the unit.
    pub kernel: Option<String>,
}

/// One engine's cells, in that engine's unit order, with its entry.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineRecord {
    pub engine: EngineEntry,
    pub cells: Vec<RecordCell>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Granularity, HashProfile};
    use serde_json::json;

    fn entry() -> EngineEntry {
        let mut def = HashDefinition::conservative("jupyter");
        def.granularity = Granularity::Unit;
        def.presentation_cell_options = vec!["echo".into()];
        def.document_keys = vec!["jupyter".into()];
        def.profiles.insert(
            "p".into(),
            HashProfile {
                presentation_cell_options: vec!["echo".into()],
                presentation_execute_keys: vec![],
            },
        );
        EngineEntry {
            name: "jupyter".into(),
            provenance: BTreeMap::from([("kernelspec".into(), json!("python3"))]),
            can_freeze: true,
            hash_definition: def,
            document_hash: "d".repeat(64),
            owned_languages: vec!["python".into()],
            includes: Some(json!({"in_header": ["<style/>"]})),
            filters: vec!["f.lua".into()],
            preserve: BTreeMap::from([("ph".into(), "<b/>".into())]),
            needs_postprocess: true,
            html_dependencies: vec![json!({"name": "d", "version": "1"})],
            pandoc: Some(BTreeMap::from([("to".into(), json!("html"))])),
            metadata: Some(BTreeMap::from([("title".into(), json!("t"))])),
            resource_files: vec!["a.png".into()],
            warnings: vec![json!({"title": "w"})],
            known_options: None,
            extra: BTreeMap::new(),
        }
    }

    fn cell() -> RecordCell {
        RecordCell {
            cell_type: CellType::Code,
            execution_count: Some(3),
            id: "4390f67858f3f1c7".into(),
            metadata: CellMetadata {
                quarto_cell_id: "0-4390f678-unlabeled".into(),
                quarto_content_hash: "4390f67858f3f1c7".into(),
                quarto_label: Some("fig-x".into()),
                quarto_kind: UnitKind::Cell,
                quarto_lang: "python.marimo".into(),
                quarto_engine: Some("jupyter".into()),
                quarto_kernel: Some("python3".into()),
                quarto_location: None,
                quarto_exec_hash: "e".repeat(64),
                quarto_hash_rule: None,
                extra: BTreeMap::from([("fig-width".into(), json!(7))]),
            },
            outputs: vec![json!({
                "output_type": "execute_result",
                "data": {"text/plain": "1"},
                "execution_count": 3,
                "metadata": {}
            })],
            source: vec!["#| label: fig-x\n".into(), "x = 1".into()],
        }
    }

    fn record() -> Record {
        Record {
            nbformat: Record::NBFORMAT,
            nbformat_minor: Record::NBFORMAT_MINOR,
            metadata: RecordMetadata {
                quarto_record_version: Record::RECORD_VERSION,
                quarto_cache_version: Some(Record::CACHE_VERSION),
                quarto_source: Some("docs/a.qmd".into()),
                quarto_engines: vec![entry()],
                quarto_files: BTreeMap::from([(
                    "a_files/x.png".into(),
                    RecordFile {
                        data: "AAAA".into(),
                        size: 3,
                    },
                )]),
                quarto_execution_hash: Some("f".repeat(64)),
                quarto_overlay: Some(BTreeMap::from([("params".into(), json!({"a": 1}))])),
                quarto_files_ref: None,
                extra: BTreeMap::new(),
            },
            cells: vec![cell()],
        }
    }

    #[test]
    fn known_options_round_trips_and_is_absent_when_unset() {
        // Absent means no filtering: the key is not written, and a record without it reads.
        let none = serde_json::to_value(entry()).unwrap();
        assert!(none.get("known_options").is_none());
        assert_eq!(
            serde_json::from_value::<EngineEntry>(none)
                .unwrap()
                .known_options,
            None
        );
        let mut with = entry();
        with.known_options = Some(vec!["echo".into(), "fig.cap".into()]);
        let json = serde_json::to_value(&with).unwrap();
        assert_eq!(json["known_options"], json!(["echo", "fig.cap"]));
        assert_eq!(serde_json::from_value::<EngineEntry>(json).unwrap(), with);
        // The schema version does not move for an optional field.
        assert_eq!(Record::RECORD_VERSION, 1);
    }

    #[test]
    fn every_schema_field_round_trips() {
        let rec = record();
        let json = serde_json::to_string(&rec).unwrap();
        assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), rec);
    }

    #[test]
    fn options_are_flat_keys_on_the_cell() {
        let json = serde_json::to_value(cell()).unwrap();
        assert_eq!(json["metadata"]["fig-width"], 7);
        assert!(json["metadata"].get("quarto_options").is_none());
        let c = cell();
        let opts: Vec<_> = c.metadata.options().map(|(k, _)| k.as_str()).collect();
        assert_eq!(opts, ["fig-width"]);
    }

    #[test]
    fn unknown_quarto_keys_survive_a_read_and_write() {
        let mut json = serde_json::to_value(record()).unwrap();
        json["metadata"]["quarto_future_notebook"] = json!({"k": 1});
        json["metadata"]["quarto_engines"][0]["quarto_future_engine"] = json!("e");
        json["cells"][0]["metadata"]["quarto_future_cell"] = json!([1, 2]);
        let rec: Record = serde_json::from_value(json.clone()).unwrap();
        assert!(rec.metadata.extra.contains_key("quarto_future_notebook"));
        assert_eq!(serde_json::to_value(&rec).unwrap(), json);
        // And an unknown key is not mistaken for an execution option.
        assert_eq!(rec.cells[0].metadata.options().count(), 1);
    }

    #[test]
    fn hash_definition_round_trips_inside_the_engine_entry() {
        let json = serde_json::to_value(entry()).unwrap();
        assert_eq!(
            json["hash_definition"]["presentationCellOptions"],
            json!(["echo"])
        );
        assert_eq!(json["hash_definition"]["granularity"], "unit");
        let back: EngineEntry = serde_json::from_value(json).unwrap();
        assert_eq!(back.hash_definition, entry().hash_definition);
        assert_eq!(back.hash_definition.profiles.len(), 1);
    }

    #[test]
    fn hash_rule_round_trips_and_is_absent_for_the_default_profile() {
        let default = serde_json::to_value(cell()).unwrap();
        assert!(default["metadata"].get("quarto_hash_rule").is_none());
        let mut named = cell();
        named.metadata.quarto_hash_rule = Some("fallback".into());
        let json = serde_json::to_value(&named).unwrap();
        assert_eq!(json["metadata"]["quarto_hash_rule"], "fallback");
        assert_eq!(serde_json::from_value::<RecordCell>(json).unwrap(), named);
    }

    #[test]
    fn a_front_matter_span_records_its_location() {
        let mut c = cell();
        c.metadata.quarto_kind = UnitKind::Inline;
        c.metadata.quarto_location = Some(UnitLocation::Metadata);
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["metadata"]["quarto_kind"], "inline");
        assert_eq!(json["metadata"]["quarto_location"], "metadata");
        assert_eq!(serde_json::from_value::<RecordCell>(json).unwrap(), c);
    }

    #[test]
    fn execution_count_is_always_present_and_null_when_unknown() {
        let mut c = cell();
        c.execution_count = None;
        let json = serde_json::to_value(&c).unwrap();
        assert!(json.get("execution_count").is_some_and(Value::is_null));
    }

    #[test]
    fn a_cell_without_an_engine_tag_still_reads() {
        let mut json = serde_json::to_value(cell()).unwrap();
        json["metadata"]
            .as_object_mut()
            .unwrap()
            .remove("quarto_engine");
        let c: RecordCell = serde_json::from_value(json).unwrap();
        assert_eq!(c.metadata.quarto_engine, None);
    }

    #[test]
    fn the_reserved_sidecar_field_is_parsed_and_never_emitted() {
        let mut json = serde_json::to_value(record()).unwrap();
        json["metadata"]["quarto_files_ref"] = json!("sidecar.bin");
        let rec: Record = serde_json::from_value(json).unwrap();
        assert!(rec.metadata.quarto_files_ref.is_some());
        let mut out = rec.clone();
        out.metadata.quarto_files_ref = None;
        let emitted = serde_json::to_value(&out).unwrap();
        assert!(emitted["metadata"].get("quarto_files_ref").is_none());
    }

    #[test]
    fn author_texts_are_total_by_index() {
        let texts = AuthorTexts(vec![AuthorText {
            text: "x <- 1".into(),
            content_hash: "4390f67858f3f1c7".into(),
        }]);
        assert_eq!(texts.len(), 1);
        assert!(texts.get(1).is_none());
    }
}
