//! The engine execution record: nbformat 4.5 with `quarto_` metadata (EXR-4).
//!
//! Target-neutral (no engine, no pipeline, no I/O): it defines, hashes, coalesces,
//! writes and reads the record every engine produces. See
//! `claude-notes/plans/2026-10-28-exr-04-record-format.md`.
//!
//! # Figures
//!
//! Figure bytes are stored inline today, as base64 strings inside the outputs, so a record is
//! one self-contained file. The notebook-level `quarto_files_ref` field is reserved for a
//! future sidecar: the reader parses and ignores it, and the writer never emits it.

mod author_text;
mod builder;
mod coalesce;
mod exec_hash;
#[cfg(test)]
mod freeze_readiness;
mod hash_definition;
mod ids;
mod matching;
mod overlay;
mod place;
mod positron_import;
mod reader;
mod staleness;
mod types;
mod writer;

pub use author_text::{SugaredBlock, author_texts};
pub use builder::{BuildError, EngineEntryInput, RecordBuilder};
pub use coalesce::{CoalesceError, coalesce};
pub use exec_hash::{
    HashError, UnitHashInput, UnitHashRef, canonical_json, execution_options, hash_document,
    hash_record, hash_unit, partition_author_text, recompute_cell_hash, yaml_to_json,
};
pub use hash_definition::{Granularity, HashDefinition, HashProfile};
pub use ids::{
    FillIdsError, author_label, cell_id, content_hash, fill_ids, scanned_author_text,
    span_content_hash, span_id,
};
pub use matching::{
    HEADER_KEYS_KEY, Matching, OUTPUT_FORMAT_OPTION, UnitKey, UnitStaleness, lost_header_keys,
    match_units,
};
pub use overlay::{overlay_difference, rehash_record};
pub use place::{PlaceNote, Placed, QUARTO_MARKDOWN_MIME, place};
pub(crate) use place::{replacement, stored_markdown};
pub use positron_import::{
    DisplayOnlyOutputs, ImportError, JUPYTER_ENGINE, KNITR_ENGINE, POSITRON_CACHE_VERSION,
    PositronImport, Provenance, import_positron_cache,
};
pub use reader::{ReadError, read_record, read_record_value};
pub use staleness::record_is_stale;
pub use types::{
    AuthorText, AuthorTexts, CellMetadata, CellType, EngineEntry, EngineRecord, Record, RecordCell,
    RecordFile, RecordMetadata, UnitKind, UnitLocation, UnitResult,
};
pub use writer::write_record;
