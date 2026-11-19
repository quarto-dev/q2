//! Registry operations over the flat sidecar (G6, G7, G8, G11, G12).
//!
//! This module is plain data: a [`RegistryView`] in, a list of [`RegistryOp`] out, with no Automerge. The
//! [`doc`] submodule reads a view out of a sidecar document and applies ops to it, exactly as the contract's
//! "Sidecar doc" section says. See the contract, "Operations".

pub mod doc;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub blob: String,
    pub doc: String,
    /// The doc's generation; stored as an Automerge `int`. Fixture rows spell it `gen`.
    #[serde(rename = "gen")]
    pub generation: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryView {
    pub entries: Vec<Entry>,
    pub retired: Vec<Entry>,
}

/// A write to the sidecar. Applying a plan's ops is one Automerge change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryOp {
    /// Set a key of `entries`.
    Put { key: String, generation: i64 },
    /// Remove a key of `entries` (only a move forward does this).
    Delete { key: String },
    /// Add a key to the grow-only `retired` map.
    Retire { key: String, generation: i64 },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("registry path must use forward slashes: {0}")]
    Backslash(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyParts {
    pub path: String,
    pub blob: String,
    pub doc: String,
}

const NUL: char = '\0';

/// `path NUL blob NUL doc`. Paths always use `/` (a native Windows path is a caller bug).
pub fn registry_key(path: &str, blob: &str, doc: &str) -> Result<String, RegistryError> {
    if path.contains('\\') {
        return Err(RegistryError::Backslash(path.to_string()));
    }
    Ok(format!("{path}{NUL}{blob}{NUL}{doc}"))
}

pub fn parse_key(key: &str) -> Option<KeyParts> {
    let mut it = key.split(NUL);
    let (path, blob, doc) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() {
        return None;
    }
    Some(KeyParts {
        path: path.into(),
        blob: blob.into(),
        doc: doc.into(),
    })
}

fn key_of(e: &Entry) -> String {
    // Entries read from a doc already passed `registry_key` on the way in, or are built by tests with `/` paths.
    format!("{}{NUL}{}{NUL}{}", e.path, e.blob, e.doc)
}

fn is_retired(v: &RegistryView, path: &str, blob: &str, doc: &str) -> bool {
    v.retired
        .iter()
        .any(|e| e.path == path && e.blob == blob && e.doc == doc)
}

/// Lowest `(generation, docId)` among the non-retired entries of `(path, blob)`; the doc id compares bytewise
/// on its string form (Rust `str` ordering, identical to the TypeScript comparison). `None` when there is none.
pub fn canonical<'a>(v: &'a RegistryView, path: &str, blob: &str) -> Option<&'a Entry> {
    v.entries
        .iter()
        .filter(|e| e.path == path && e.blob == blob && !is_retired(v, &e.path, &e.blob, &e.doc))
        .min_by(|a, b| {
            a.generation
                .cmp(&b.generation)
                .then_with(|| a.doc.cmp(&b.doc))
        })
}

pub fn entries_for<'a>(v: &'a RegistryView, path: &str) -> Vec<&'a Entry> {
    v.entries.iter().filter(|e| e.path == path).collect()
}

/// `max + 1` over every entry of the path on any blob; the first doc of a path gets 0.
pub fn next_generation(v: &RegistryView, path: &str) -> i64 {
    entries_for(v, path)
        .iter()
        .map(|e| e.generation)
        .max()
        .unwrap_or(-1)
        + 1
}

fn has_entry(v: &RegistryView, key: &str) -> Option<i64> {
    v.entries
        .iter()
        .find(|e| key_of(e) == key)
        .map(|e| e.generation)
}

/// Put-if-absent.
pub fn plan_register(v: &RegistryView, e: &Entry) -> Vec<RegistryOp> {
    let key = key_of(e);
    if has_entry(v, &key).is_some() {
        return vec![];
    }
    vec![RegistryOp::Put {
        key,
        generation: e.generation,
    }]
}

/// One change: add `(path, newBlob, doc)` with the doc's generation and remove the old key (G8). When the doc
/// has no entry under the old blob (a step 5 fallback doc), the new entry gets `max + 1`.
pub fn plan_move_forward(
    v: &RegistryView,
    path: &str,
    old_blob: &str,
    new_blob: &str,
    doc: &str,
) -> Vec<RegistryOp> {
    let old_key = format!("{path}{NUL}{old_blob}{NUL}{doc}");
    let new_key = format!("{path}{NUL}{new_blob}{NUL}{doc}");
    match has_entry(v, &old_key) {
        Some(generation) => vec![
            RegistryOp::Put {
                key: new_key,
                generation,
            },
            RegistryOp::Delete { key: old_key },
        ],
        None => vec![RegistryOp::Put {
            key: new_key,
            generation: next_generation(v, path),
        }],
    }
}

/// The `reset --hard` step: grow-only `retired`; the entry stays in `entries`. The retired value copies the
/// entry's generation (0 when the entry is absent).
pub fn plan_retire(v: &RegistryView, path: &str, blob: &str, doc: &str) -> Vec<RegistryOp> {
    if is_retired(v, path, blob, doc) {
        return vec![];
    }
    let key = format!("{path}{NUL}{blob}{NUL}{doc}");
    let generation = has_entry(v, &key).unwrap_or(0);
    vec![RegistryOp::Retire { key, generation }]
}

/// After a sidecar sync: an attached doc with no entry on any blob, and not retired, is registered again under
/// its HEAD blob with its persisted generation. Never revives a retired entry, never re-registers a doc that
/// moved forward.
pub fn plan_repair(
    v: &RegistryView,
    path: &str,
    doc: &str,
    generation: i64,
    head_blob: &str,
) -> Vec<RegistryOp> {
    let any_entry = v.entries.iter().any(|e| e.path == path && e.doc == doc);
    let any_retired = v.retired.iter().any(|e| e.path == path && e.doc == doc);
    if any_entry || any_retired {
        return vec![];
    }
    plan_register(
        v,
        &Entry {
            path: path.into(),
            blob: head_blob.into(),
            doc: doc.into(),
            generation,
        },
    )
}

/// Paths whose lowercased form collides with another path's: they fall back to ordinary sync (G12).
pub fn case_collisions(v: &RegistryView) -> BTreeSet<String> {
    let mut by_lower: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for e in &v.entries {
        by_lower
            .entry(e.path.to_lowercase())
            .or_default()
            .insert(&e.path);
    }
    by_lower
        .into_values()
        .filter(|s| s.len() > 1)
        .flatten()
        .map(str::to_string)
        .collect()
}

/// True when the doc is registered under a path other than `path` (a doc is live under at most one path); the
/// fork rule (G12) then applies.
pub fn live_elsewhere(v: &RegistryView, doc: &str, path: &str) -> bool {
    v.entries.iter().any(|e| e.doc == doc && e.path != path)
}
