//! Versioned scan-manifest types (`hub admin scan --output`,
//! bd-eiku4ymo).
//!
//! The manifest is the **only** input `hub admin collect` accepts —
//! never ad-hoc doc ids — so its shape is a contract: versioned,
//! self-describing (tool, timestamp, data dir), and carrying enough
//! per-doc evidence that a human can audit *why* each candidate was
//! deemed removable. Field names are camelCase on the wire so a
//! future hub-client admin page can consume the same JSON.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Current manifest schema version. Bump on any incompatible change;
/// `collect` refuses manifests with a version it doesn't know.
pub const MANIFEST_VERSION: u32 = 1;

/// Top-level scan output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanManifest {
    /// Schema version of this manifest ([`MANIFEST_VERSION`]).
    pub manifest_version: u32,
    /// Tool + crate version that produced this manifest.
    pub tool: String,
    /// RFC 3339 UTC timestamp of the scan.
    pub scanned_at: String,
    /// The `--data-dir` the scan ran against (canonicalized).
    /// `collect` refuses a manifest whose dataDir doesn't match its
    /// own — pointing yesterday's manifest at a different server must
    /// fail loudly.
    pub data_dir: String,
    /// Age gate that was in effect, in days (candidates are older).
    pub older_than_days: i64,
    /// Whether unstamped (pre-envelope) captures were eligible.
    pub include_unstamped: bool,
    /// Counts of every doc kind seen, keyed by [`DocKind::as_str`]
    /// (BTreeMap for stable JSON output).
    pub inventory: BTreeMap<String, KindStats>,
    /// Docs the scan determined are safely removable.
    pub candidates: Vec<CandidateDoc>,
    /// Unreferenced docs that are NOT collectible in v1 (non-capture
    /// kinds, unknown shapes, too-young or unstamped captures).
    /// Informational: these are the follow-up-strand material.
    pub not_collectible: Vec<ReportedDoc>,
}

/// Per-kind inventory counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindStats {
    pub count: usize,
    pub bytes: u64,
}

/// A doc the scan deems safely removable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateDoc {
    pub doc_id: String,
    /// Always `"engine-capture"` in v1 (the collector re-checks).
    pub kind: String,
    /// Total bytes of this doc's storage chunks.
    pub size_bytes: u64,
    /// The uncompressed audit meta (absent on legacy captures — which
    /// are only candidates under `--include-unstamped`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<CandidateMeta>,
    /// Human-readable evidence for why this doc is removable.
    pub reason: String,
}

/// The capture-doc `meta` envelope as carried in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub engines: Vec<String>,
}

/// An unreferenced-but-protected doc, reported for visibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportedDoc {
    pub doc_id: String,
    pub kind: String,
    pub size_bytes: u64,
    pub reason: String,
}
