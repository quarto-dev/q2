//! Writer helpers for trace files.
//!
//! The `quarto-core` crate's `JsonTraceObserver` owns the actual observer
//! implementation and drives the [`TraceDocument`] construction; this
//! module exposes the atomic "write-to-disk" step so the file-system
//! concern lives here rather than duplicated in the observer.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{SCHEMA_VERSION, TraceDocument};

/// Error type for trace write operations.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("I/O error writing {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Write a trace document to disk.
///
/// On-disk traces are written as compact (un-indented) JSON: pretty-print
/// indentation accounted for ~80% of bytes on real traces (bd-5qnj).
/// If `path` ends in `.gz`, the JSON stream is gzipped; otherwise plain
/// JSON is emitted. Humans who want a pretty view use
/// `quarto trace show` (which formats from the parsed [`TraceDocument`])
/// or pipe the file through `jq` (after `gunzip` if applicable).
///
/// As of `schema_version: 2`, the writer also deduplicates AST values
/// across pipeline entries: identical AST sub-values are collected into
/// a top-level `asts` map keyed by content hash, and replaced inside
/// entries by `{ "$ref": "<hash>" }` sentinels. The caller's
/// [`TraceDocument`] is never mutated; the dedup happens on a clone.
///
/// Creates parent directories as needed.
pub fn write_trace(doc: &TraceDocument, path: &Path) -> Result<(), WriteError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| WriteError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let file = File::create(path).map_err(|source| WriteError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let buffered = BufWriter::new(file);

    let to_write = encode_v2(doc);

    if has_gz_extension(path) {
        let gz = flate2::write::GzEncoder::new(buffered, flate2::Compression::default());
        serde_json::to_writer(gz, &to_write)?;
        // GzEncoder finishes its stream on drop; the BufWriter inside is
        // flushed when the GzEncoder drops it.
    } else {
        serde_json::to_writer(buffered, &to_write)?;
    }
    Ok(())
}

/// Build a v2-encoded clone of `doc` ready for serialization: AST
/// sub-values are dedup'd into the top-level `asts` map and replaced
/// inside entries by `{ "$ref": "<hash>" }`. The original `doc` is not
/// modified.
fn encode_v2(doc: &TraceDocument) -> TraceDocument {
    let mut out = doc.clone();
    out.schema_version = SCHEMA_VERSION;
    // Start from an empty asts map; if the caller already had entries
    // there (e.g. round-tripping a previously-read trace without
    // rehydration), they're folded into the new dedup pass below
    // when entries reference them by `$ref`.
    let mut asts: BTreeMap<String, serde_json::Value> = std::mem::take(&mut out.asts);

    for entry in &mut out.pipeline {
        let Some(data) = entry.data.as_mut() else {
            continue;
        };
        let Some(kind) = entry.data_kind.as_deref() else {
            continue;
        };
        match kind {
            "DocumentAst" => dedup_document_ast(data, &mut asts),
            "AtProfile" | "PandocInvocation" => dedup_wrapped_ast(data, &mut asts),
            _ => {}
        }
    }

    out.asts = asts;
    out
}

/// `DocumentAst`-kind entries come in two shapes:
/// - **Wrapped** (from `serialize_pipeline_data`): `{path, ast, warnings_count}`.
/// - **Bare** (from `on_transform_data`): the AST itself
///   (`{pandoc-api-version, meta, blocks, ...}`), no wrapper.
///
/// Disambiguator: wrapped entries have a top-level `ast` key; the bare
/// AST does not.
fn dedup_document_ast(
    data: &mut serde_json::Value,
    asts: &mut BTreeMap<String, serde_json::Value>,
) {
    if let serde_json::Value::Object(map) = data
        && map.contains_key("ast")
    {
        dedup_wrapped_ast(data, asts);
        return;
    }
    // Bare AST: replace the whole `data` value with a $ref.
    replace_with_ref(data, asts);
}

/// Wrapped shape (`AtProfile`, `PandocInvocation`, or wrapped `DocumentAst`): replace
/// `data["ast"]` (if present) with a `$ref` sentinel.
fn dedup_wrapped_ast(data: &mut serde_json::Value, asts: &mut BTreeMap<String, serde_json::Value>) {
    if let serde_json::Value::Object(map) = data
        && let Some(ast_value) = map.get_mut("ast")
    {
        replace_with_ref(ast_value, asts);
    }
}

/// Hash `value`, ensure it's stored in `asts`, and replace it with a
/// `$ref` sentinel pointing at the same content. Idempotent: if `value`
/// is already a `$ref`, leave it alone.
fn replace_with_ref(value: &mut serde_json::Value, asts: &mut BTreeMap<String, serde_json::Value>) {
    if is_dollar_ref(value) {
        return;
    }
    let hash = hash_value(value);
    let owned = std::mem::replace(value, serde_json::Value::Null);
    asts.entry(hash.clone()).or_insert(owned);
    *value = serde_json::json!({ "$ref": hash });
}

fn is_dollar_ref(value: &serde_json::Value) -> bool {
    matches!(value, serde_json::Value::Object(m) if m.len() == 1 && m.contains_key("$ref"))
}

/// Hash a JSON value to a stable 16-hex-char string (truncated SHA-256,
/// 64 bits). Collision probability is ≪10⁻¹² for the ~100 entries that
/// fit in a single trace, so a truncated hash is a fine choice over a
/// 256-bit one (it shaves ~50 bytes per `asts` key).
fn hash_value(value: &serde_json::Value) -> String {
    // `to_vec` is stable for a given `Value` because `serde_json::Map`
    // preserves insertion order.
    let bytes = serde_json::to_vec(value).expect("serde_json::Value serializes infallibly");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(16);
    for b in &digest[..8] {
        use std::fmt::Write as _;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

fn has_gz_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gz"))
}
