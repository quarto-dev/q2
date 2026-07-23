//! Typed schema and reader/writer library for Quarto pipeline execution traces.
//!
//! This crate defines the on-disk schema for trace files written by
//! `quarto`'s pipeline observers, along with thin reader/writer helpers
//! that are shared by the writer side (`quarto-core`'s `JsonTraceObserver`),
//! the CLI analyzer (`quarto trace list|show`), and the viewer backend
//! (`quarto-trace-server`).
//!
//! # Schema at a glance
//!
//! ```json
//! {
//!   "schema_version": 2,
//!   "render": {
//!     "input_path": "doc.qmd",
//!     "output_path": "doc.html",
//!     "format_target": "html",
//!     "started_at_unix_ms": 1799200496000.0,
//!     "git_hash": "abc1234",
//!     "total_duration_ms": 123.4
//!   },
//!   "asts": {
//!     "<hash>": { ...AST JSON... }
//!   },
//!   "pipeline": [
//!     { "stage": "parse", "index": 0, "data_kind": "DocumentAst",
//!       "data": { "path": "doc.qmd", "ast": { "$ref": "<hash>" }, "warnings_count": 0 },
//!       "duration_ms": 1.2, "status": "ok" },
//!     { "stage": "engine-execution", "index": 1, "status": "error",
//!       "error": {"message": "..."} },
//!     { "stage": "render-html-body", "index": 2, "status": "skipped" }
//!   ]
//! }
//! ```
//!
//! ## v2: AST dedup (bd-5qnj)
//!
//! Real traces carry the same AST in many entries — most pipeline
//! transforms are no-ops on any given document, so 36 of 42 DocumentAst
//! entries on a representative trace are byte-identical to the previous
//! one (see
//! `claude-notes/plans/5qnj-trace-size-investigation/measurements.md`).
//! v2 collapses these to one stored copy.
//!
//! - The on-disk JSON has a top-level `asts` map keyed by content hash.
//! - Inside any pipeline entry's `data`, an inline AST is replaced by
//!   `{ "$ref": "<hash>" }`.
//! - The reader rehydrates `$ref` sentinels into inline AST values, so
//!   downstream consumers see a v1-equivalent in-memory
//!   [`TraceDocument`].
//! - Writers always emit v2; readers handle v1 (legacy traces with no
//!   `asts` map and inline ASTs) and v2 transparently.
//!
//! Unknown `status` values and unknown fields are tolerated by readers
//! for forward compatibility.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod read;
pub mod write;

/// Git short hash + optional `-dirty` suffix, captured at build time.
///
/// Falls back to `"unknown"` when `git` is not available (e.g. tarball
/// builds via `cargo package` without a `.git` directory).
pub const BUILD_GIT_HASH: &str = env!("QUARTO_GIT_HASH");

/// Current trace schema version.
///
/// - `1`: original wire format (inline ASTs, no `asts` map).
/// - `2` (current): on-disk dedup of AST values via top-level `asts` map
///   and `{ "$ref": "<hash>" }` sentinels inside entries' `data`.
///   Reader-rehydrated [`TraceDocument`]s are v1-equivalent in shape;
///   the dedup is a wire-format detail.
///
/// Bumped only when entry-shape changes are introduced. Additive
/// changes (new optional fields) don't bump the version.
pub const SCHEMA_VERSION: u32 = 2;

/// Top-level trace document.
///
/// In memory, the `asts` map is always empty: the writer populates it
/// transiently during serialization and the reader folds it back into
/// the entries during deserialization. Direct serialization of an
/// in-memory `TraceDocument` (without going through `write::write_trace`
/// / `read::read_trace`) bypasses the dedup pass; that's fine for tests
/// and ad-hoc serialization, but the on-disk artifact will not have
/// the v2 size benefits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "TraceDocumentDe")]
pub struct TraceDocument {
    pub schema_version: u32,
    pub render: RenderInfo,
    /// Content-addressed AST values, used as the deduplication target
    /// for `{ "$ref": "<hash>" }` references inside entries' `data`.
    /// Empty in-memory after `read_trace`; populated transiently by
    /// `write_trace`. See module-level docs for the wire format.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub asts: BTreeMap<String, serde_json::Value>,
    pub pipeline: Vec<TraceEntry>,
    /// Ordered engine execution captures for replay (bd-45yw, extended to
    /// a sequence by bd-5yff4).
    ///
    /// When `trace: true` is set, the pipeline records each
    /// `ExecutionEngine`'s output here — **one capture per engine that
    /// ran, in execution order** — so the trace can later drive the
    /// in-Rust replay engine(s) for deterministic regression tests
    /// without R/Python/Jupyter installs. Empty for traces produced
    /// before bd-45yw landed and for renders where only the markdown
    /// engine ran (no execution to record).
    ///
    /// On-disk back-compat: a legacy single `engine_capture` object
    /// (schema written before bd-5yff4) is folded into a one-element
    /// vector on read (see `TraceDocumentDe`). Writers always emit the
    /// `engine_captures` array.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub engine_captures: Vec<EngineCapture>,
}

/// Deserialization mirror for [`TraceDocument`].
///
/// Exists solely to accept the **legacy** single `engine_capture` field
/// (written before bd-5yff4) and fold it into the `engine_captures`
/// vector, so old on-disk traces keep loading. `TraceDocument` itself
/// deserializes through this via `#[serde(from = "TraceDocumentDe")]`;
/// serialization is unaffected (the writer emits only `engine_captures`).
#[derive(Deserialize)]
struct TraceDocumentDe {
    schema_version: u32,
    render: RenderInfo,
    #[serde(default)]
    asts: BTreeMap<String, serde_json::Value>,
    pipeline: Vec<TraceEntry>,
    #[serde(default)]
    engine_captures: Vec<EngineCapture>,
    /// Legacy single-capture field (pre-bd-5yff4). Read-only.
    #[serde(default)]
    engine_capture: Option<EngineCapture>,
}

impl From<TraceDocumentDe> for TraceDocument {
    fn from(de: TraceDocumentDe) -> Self {
        let mut engine_captures = de.engine_captures;
        // Fold the legacy single capture in when the new field is absent.
        // (If both are somehow present, the new field wins.)
        if engine_captures.is_empty()
            && let Some(capture) = de.engine_capture
        {
            engine_captures.push(capture);
        }
        TraceDocument {
            schema_version: de.schema_version,
            render: de.render,
            asts: de.asts,
            pipeline: de.pipeline,
            engine_captures,
        }
    }
}

impl TraceDocument {
    /// Construct a new empty trace document stamped with the current schema
    /// version.
    pub fn new(render: RenderInfo) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            render,
            asts: BTreeMap::new(),
            pipeline: Vec::new(),
            engine_captures: Vec::new(),
        }
    }
}

/// Captured `ExecuteResult` from an engine run, attached to a
/// [`TraceDocument`] for later replay (bd-45yw).
///
/// Stores the engine name (matching what `ExecutionEngine::name()`
/// returned), the QMD input that was handed to `execute()`, and the
/// full `ExecuteResult` as opaque JSON. The exact `ExecuteResult`
/// shape lives in `quarto-core`; we keep it as a `serde_json::Value`
/// here so this crate stays leaf-level and doesn't need to depend on
/// `quarto-core`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineCapture {
    /// Name of the engine that produced this result (e.g. `"knitr"`,
    /// `"jupyter"`). The replay engine registers under this name so
    /// it can stand in for the original engine without document
    /// metadata changes.
    pub engine_name: String,

    /// Verbatim QMD text handed to `ExecutionEngine::execute()`.
    /// Replay validates the document under investigation matches this
    /// (string equality) and hard-fails on mismatch.
    pub input_qmd: String,

    /// Serialized `ExecuteResult` (markdown, supporting_files,
    /// filters, includes, needs_postprocess). Treated as opaque here;
    /// `quarto-core::engine::replay` deserializes via
    /// `serde_json::from_value`.
    pub result: serde_json::Value,

    /// Contents of engine-generated supporting files (bd-qbhp2cvv).
    ///
    /// `result.supporting_files` records *paths* only; those files
    /// exist only on the machine (often only in the temp dir) where
    /// the engine ran. For preview replay on another machine — or in
    /// the browser WASM VFS — the bytes must travel with the capture.
    /// Recording embeds them here; `CaptureSpliceStage` materializes
    /// them next to the document before splicing.
    ///
    /// Empty for captures that predate this field (`serde(default)`)
    /// and for engines that produced no supporting files; such
    /// captures serialize without the key, byte-identical to the
    /// pre-field wire format.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<CaptureFile>,
}

/// One engine-generated supporting file embedded in an
/// [`EngineCapture`] (bd-qbhp2cvv).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureFile {
    /// Path relative to the source document's directory, always with
    /// forward-slash separators (e.g.
    /// `"doc_files/figure-html/cell-1.png"`) so captures recorded on
    /// one platform replay on any other.
    pub path: String,

    /// Base64-encoded (standard alphabet, padded) file contents.
    pub contents_base64: String,
}

/// Top-level metadata about a render invocation.
///
/// Captured once per trace, populated progressively as the pipeline runs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RenderInfo {
    /// Path to the input document, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_path: Option<String>,
    /// Path to the final output, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    /// Target format identifier (e.g. `"html"`, `"pdf"`), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_target: Option<String>,
    /// Milliseconds since the Unix epoch when the pipeline started.
    /// A number rather than a formatted string so no date library is
    /// required to produce it; viewers can format via
    /// `new Date(ms).toISOString()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_unix_ms: Option<f64>,
    /// Git short hash of the `quarto` build that produced this trace, with
    /// `-dirty` suffix if the working tree was dirty at build time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_hash: Option<String>,
    /// Total pipeline wall-clock duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_duration_ms: Option<f64>,
}

/// One entry in the pipeline array.
///
/// Entries with `status == Ok` carry `data` and `data_kind`; entries with
/// `status == Error` carry `error`; entries with `status == Skipped`
/// carry neither.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    /// Human-readable stage name (e.g. `"parse"`, `"metadata-merge"`).
    ///
    /// Synthetic names are also used: `"__input"` for the pipeline input,
    /// `"transform:<name>"` for individual AST transforms within
    /// `AstTransformsStage`.
    pub stage: String,

    /// Zero-based index of the stage in the outer pipeline.
    pub index: usize,

    /// Kind tag for the data payload. Present whenever `data` is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_kind: Option<String>,

    /// Data payload — serialized pipeline data (AST JSON, markdown, HTML, etc.).
    /// Absent on errored and skipped stages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,

    /// Wall-clock duration for this stage in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<f64>,

    /// Status marker. Defaults to `Ok` on older traces that pre-date the
    /// field.
    #[serde(default)]
    pub status: StageStatus,

    /// Error payload, present when `status == Error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<StageErrorInfo>,
}

/// Status of a stage within a trace.
///
/// `Unknown` is used by readers when deserializing a newer trace that
/// adds a status variant we don't know about yet — this keeps readers
/// forward-compatible.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StageStatus {
    #[default]
    Ok,
    Error,
    Skipped,
    /// Unknown status value produced by a newer writer.
    #[serde(other)]
    Unknown,
}

/// Error information attached to an errored stage entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageErrorInfo {
    /// Human-readable error message.
    pub message: String,
}
