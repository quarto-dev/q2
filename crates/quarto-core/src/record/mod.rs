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
mod location;
mod matching;
mod overlay;
mod place;
mod positron_export;
mod positron_import;
mod positron_record;
mod reader;
#[cfg(not(target_arch = "wasm32"))]
mod render_sources;
mod source_key;
mod sources;
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
pub use location::{execution_record_path, project_relative_source};
pub use matching::{
    HEADER_KEYS_KEY, Matching, OUTPUT_FORMAT_OPTION, UnitKey, UnitStaleness, lost_header_keys,
    match_units,
};
pub use overlay::{overlay_difference, rehash_record};
pub use place::{PlaceNote, Placed, QUARTO_MARKDOWN_MIME, VERBATIM_KEY, place};
pub(crate) use place::{placed_replacement, replacement, stored_markdown};
pub use positron_export::{
    DropReason, DroppedRepresentation, ExportError, ExportOptions, ExportedCache, SkipReason,
    SkippedUnit, export_for_positron,
};
pub use positron_import::{
    DisplayOnlyOutputs, ImportError, POSITRON_CACHE_VERSION, PositronImport, Provenance,
    import_positron_cache,
};
pub use positron_record::{
    PROVENANCE_KEY, PROVENANCE_POSITRON, UNCLAIMED_KEY, import_record_source,
    imported_record_is_stale, is_imported_record, record_from_positron,
};
pub use reader::{ReadError, read_record, read_record_value};
#[cfg(not(target_arch = "wasm32"))]
pub use render_sources::{
    RenderRecordSources, RenderSelection, SelectInputs, select_render_record,
};
pub use source_key::{
    CommandSources, RECORD_SOURCES_KEY, ResolvedSources, SourceKeyError, SourceNote, Trust,
    describe as describe_sources, host_record_sources, resolve_record_sources,
};
pub use sources::{
    Chosen, EngineError, EngineResult, ImportContext, ImportedOnly, LookupError, OutcomeKind,
    ParseSourceError, RecordLookup, RecordSource, RecordSources, Resolution, SourceOutcome,
    WalkContext, resolve_records,
};
pub use staleness::{record_format_mismatch, record_is_stale, record_staleness, source_staleness};
pub use types::{
    AuthorText, AuthorTexts, CellMetadata, CellType, EngineEntry, EngineRecord, Record, RecordCell,
    RecordFile, RecordMetadata, UnitKind, UnitLocation, UnitResult,
};
pub use writer::write_record;
