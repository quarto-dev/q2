//! The record reader (EXR-4 Phase 5).
//!
//! Validates the nbformat envelope before trusting the `quarto_` fields, then reads the
//! typed [`Record`]. Unknown `quarto_` keys survive (forward compatibility), and the reserved
//! `quarto_files_ref` sidecar field is parsed and ignored (the writer never emits it).

use serde_json::Value;

use super::types::{Record, RecordCell};

/// Why a record could not be read. Callers reject, never delete, a file that fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("record is not valid JSON: {0}")]
    Json(String),
    #[error("record is not a JSON object")]
    NotAnObject,
    #[error("record is not nbformat 4 (found {0})")]
    NotNbformat4(String),
    #[error("record has no `cells` array")]
    NoCells,
    #[error("record has no `metadata.quarto_record_version`")]
    NoRecordVersion,
    #[error("record version {found} is newer than this reader understands ({supported})")]
    UnsupportedVersion { found: u64, supported: u32 },
    #[error("record structure is invalid: {0}")]
    Invalid(String),
}

/// Read a record from its bytes.
pub fn read_record(text: &str) -> Result<Record, ReadError> {
    let value: Value = serde_json::from_str(text).map_err(|e| ReadError::Json(e.to_string()))?;
    read_record_value(value)
}

/// Read a record from an already-parsed JSON value.
pub fn read_record_value(value: Value) -> Result<Record, ReadError> {
    let obj = value.as_object().ok_or(ReadError::NotAnObject)?;
    match obj.get("nbformat").and_then(Value::as_u64) {
        Some(4) => {}
        Some(n) => return Err(ReadError::NotNbformat4(n.to_string())),
        None => {
            let found = obj
                .get("nbformat")
                .map_or_else(|| "nothing".to_string(), Value::to_string);
            return Err(ReadError::NotNbformat4(found));
        }
    }
    if !obj.get("cells").is_some_and(Value::is_array) {
        return Err(ReadError::NoCells);
    }
    let version = obj
        .get("metadata")
        .and_then(|m| m.get("quarto_record_version"))
        .ok_or(ReadError::NoRecordVersion)?;
    let version = version
        .as_u64()
        .ok_or_else(|| ReadError::Invalid("`quarto_record_version` is not a number".into()))?;
    if version > u64::from(Record::RECORD_VERSION) {
        return Err(ReadError::UnsupportedVersion {
            found: version,
            supported: Record::RECORD_VERSION,
        });
    }
    serde_json::from_value(value).map_err(|e| ReadError::Invalid(e.to_string()))
}

impl Record {
    /// The engine that ran `cell`: its `quarto_engine` tag, or, for a record that predates
    /// engine tags, the first engine whose `owned_languages` holds the cell's language
    /// (compared case-insensitively on the full token).
    pub fn engine_of<'a>(&'a self, cell: &'a RecordCell) -> Option<&'a str> {
        if let Some(tag) = cell.metadata.quarto_engine.as_deref() {
            return Some(tag);
        }
        let lang = &cell.metadata.quarto_lang;
        self.metadata
            .quarto_engines
            .iter()
            .find(|e| {
                e.owned_languages
                    .iter()
                    .any(|l| l.eq_ignore_ascii_case(lang))
            })
            .map(|e| e.name.as_str())
    }

    /// The cells `engine` ran, in record order (see [`Record::engine_of`]).
    pub fn cells_of<'a>(&'a self, engine: &'a str) -> impl Iterator<Item = &'a RecordCell> + 'a {
        self.cells
            .iter()
            .filter(move |c| self.engine_of(c) == Some(engine))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::record::write_record;

    const GOLDEN: &str = include_str!("../../tests/fixtures/records/golden.ipynb");

    #[test]
    fn the_golden_reads_and_writes_back_to_the_same_bytes() {
        let rec = read_record(GOLDEN).unwrap();
        assert_eq!(write_record(&rec).unwrap(), GOLDEN);
    }

    fn golden_value() -> Value {
        serde_json::from_str(GOLDEN).unwrap()
    }

    #[test]
    fn rejects_non_json_and_non_objects() {
        assert!(matches!(read_record("{"), Err(ReadError::Json(_))));
        assert_eq!(read_record("[]"), Err(ReadError::NotAnObject));
    }

    #[test]
    fn rejects_other_nbformat_versions() {
        let mut v = golden_value();
        v["nbformat"] = json!(3);
        assert_eq!(
            read_record_value(v.clone()),
            Err(ReadError::NotNbformat4("3".into()))
        );
        v.as_object_mut().unwrap().remove("nbformat");
        assert!(matches!(
            read_record_value(v),
            Err(ReadError::NotNbformat4(_))
        ));
    }

    #[test]
    fn rejects_a_missing_or_non_array_cells() {
        let mut v = golden_value();
        v["cells"] = json!({});
        assert_eq!(read_record_value(v.clone()), Err(ReadError::NoCells));
        v.as_object_mut().unwrap().remove("cells");
        assert_eq!(read_record_value(v), Err(ReadError::NoCells));
    }

    #[test]
    fn rejects_a_missing_or_future_record_version() {
        let mut v = golden_value();
        v["metadata"]["quarto_record_version"] = json!(99);
        assert_eq!(
            read_record_value(v.clone()),
            Err(ReadError::UnsupportedVersion {
                found: 99,
                supported: Record::RECORD_VERSION
            })
        );
        v["metadata"]
            .as_object_mut()
            .unwrap()
            .remove("quarto_record_version");
        assert_eq!(read_record_value(v), Err(ReadError::NoRecordVersion));
    }

    #[test]
    fn rejects_a_structurally_invalid_cell() {
        let mut v = golden_value();
        v["cells"][0]["cell_type"] = json!("markdown");
        assert!(matches!(read_record_value(v), Err(ReadError::Invalid(_))));
    }

    #[test]
    fn a_record_without_engine_tags_derives_engines_from_ownership() {
        let mut rec = read_record(GOLDEN).unwrap();
        let tagged: Vec<Option<String>> = rec
            .cells
            .iter()
            .map(|c| rec.engine_of(c).map(str::to_string))
            .collect();
        assert!(tagged.iter().all(Option::is_some));
        for c in &mut rec.cells {
            c.metadata.quarto_engine = None;
        }
        let derived: Vec<Option<String>> = rec
            .cells
            .iter()
            .map(|c| rec.engine_of(c).map(str::to_string))
            .collect();
        assert_eq!(derived, tagged);
        // The names round trip through the file too.
        let reread = read_record(&write_record(&rec).unwrap()).unwrap();
        assert_eq!(reread, rec);
    }

    #[test]
    fn an_unowned_language_has_no_engine() {
        let mut rec = read_record(GOLDEN).unwrap();
        rec.cells[0].metadata.quarto_engine = None;
        rec.cells[0].metadata.quarto_lang = "cobol".into();
        assert_eq!(rec.engine_of(&rec.cells[0]), None);
    }

    #[test]
    fn the_reserved_sidecar_field_is_read_and_ignored() {
        let mut v = golden_value();
        v["metadata"]["quarto_files_ref"] = json!("sidecar.bin");
        let rec = read_record_value(v).unwrap();
        assert!(rec.metadata.quarto_files_ref.is_some());
        // Writing back drops it.
        assert_eq!(write_record(&rec).unwrap(), GOLDEN);
    }
}
