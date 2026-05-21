/*
 * attribution/types.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Canonical attribution data types.
//!
//! The canonical in-memory shape (`AttributionData`) is held as
//! `Arc<AttributionData>` on `RenderContext.attribution_data` — the
//! sidecar. It is **never** stored in `ast.meta`. The sole serialization
//! path is the WASM transport boundary; see [`prebuilt`] and
//! [`builder`] for the round-trip discipline.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::format::{Format, FormatIdentifier};

/// A contiguous byte-range run attributed to a single author at a
/// single point in time.
///
/// `start` and `end` are UTF-8 **byte** offsets into the source text,
/// deliberately distinct from the UTF-16 code units used on the JS
/// side (`hub-client/src/services/attribution-runs.ts`) and by
/// Automerge text splice positions. Conversion happens once, at the
/// WASM wire, inside `buildAttributionPayload`. See
/// `claude-notes/designs/attribution-encoding-contract.md`.
///
/// `actor` is `Arc<str>` (not `String`) so the same Arc is shared
/// across every run by the same author. For a doc with 5
/// contributors and 1000 runs this is 5 string allocations + 1000
/// cheap pointer clones, not 1000 string allocations. The
/// interning invariant is enforced by [`super::builder::AttributionDataBuilder`];
/// every `AttributionRun.actor` Arc in a built `AttributionData` is
/// `Arc::ptr_eq` to the corresponding key in
/// [`IdentityMap`].
///
/// `time` is Unix epoch **milliseconds**. Automerge uses ms natively;
/// the git provider multiplies its seconds-since-epoch timestamp by
/// 1000 before populating this field.
///
/// **`Serialize` only**, no `Deserialize` derive: deserialization
/// goes through [`TransportAttributionRun`] (a `String`-actor mirror)
/// then through [`super::builder::AttributionDataBuilder`], which
/// restores the interning invariant a plain
/// `Deserialize for Arc<str>` would have destroyed (each
/// `Arc::from(s)` during deserialize would otherwise allocate
/// per-occurrence).
#[derive(Debug, Clone, Serialize)]
pub struct AttributionRun {
    pub start: usize,
    pub end: usize,
    pub actor: Arc<str>,
    pub time: i64,
}

/// Transparent newtype around `Vec<AttributionRun>`.
///
/// Sorted by `start`, non-overlapping, contiguous. The in-memory
/// queryable form for `query_byte_range` — see
/// [`super::source::AttributionSource`].
///
/// **Single-document only in v1.** v2 (multi-file via includes)
/// replaces the field type with a path-keyed map. The transparent
/// newtype is `Serialize`-only for the same reason as
/// [`AttributionRun`].
#[derive(Debug, Clone, Default, Serialize)]
#[serde(transparent)]
pub struct AttributionMap(pub Vec<AttributionRun>);

impl AttributionMap {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn as_slice(&self) -> &[AttributionRun] {
        &self.0
    }
}

/// Resolved identity for an actor: a display name and a CSS-compatible
/// colour string.
///
/// Wire shape (q2-debug JSON, HTML `data-attr-*` attributes) uses
/// `name` not `display_name` — the Rust field follows the in-code
/// convention; the serde rename keeps the wire faithful.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Identity {
    #[serde(rename = "name")]
    pub display_name: String,
    pub color: String,
}

/// `HashMap<Arc<str>, Identity>` keyed by the same `Arc<str>` used in
/// `AttributionRun.actor`. The merged result of
/// `meta.attribution.identities` (user override) ∪ provider-supplied
/// identities. Built by `AttributionGenerateTransform`; consumed by
/// `AttributionRenderTransform`. Empty when no source supplied
/// identities; unmapped actors fall back to the render-side warning
/// path placeholder.
pub type IdentityMap = HashMap<Arc<str>, Identity>;

/// The canonical in-memory shape, held as
/// `Arc<AttributionData>` on `RenderContext.attribution_data` (the
/// sidecar). Not stored in `ast.meta`.
///
/// `Serialize` derive exists *solely* for the WASM transport
/// boundary; both fields use `#[serde(default, skip_serializing_if)]`
/// so runs-only and identities-only transport payloads serialize
/// compactly.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AttributionData {
    #[serde(default, skip_serializing_if = "AttributionMap::is_empty")]
    pub runs: AttributionMap,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub identities: IdentityMap,
}

/// Transport-only mirror of [`AttributionRun`] used at the WASM
/// boundary. Plain `String` actor field so `serde_json::from_str`
/// works without re-interning machinery; the canonical
/// `Arc<str>` shape is restored via
/// [`super::builder::AttributionDataBuilder`] inside the prebuilt
/// provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportAttributionRun {
    pub start: usize,
    pub end: usize,
    pub actor: String,
    pub time: i64,
}

/// Transport-only mirror of [`AttributionData`].
///
/// The wire shape is identical to the canonical type's `Serialize`
/// form (`Arc<str>` and `String` both serialize as JSON strings), so
/// round-tripping `AttributionData → JSON → TransportAttributionData
/// → AttributionDataBuilder → AttributionData` preserves data; the
/// only thing the transport detour buys is a clean place to re-intern.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransportAttributionData {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<TransportAttributionRun>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub identities: HashMap<String, Identity>,
}

/// Per-node attribution record carried on the writer config.
///
/// `Arc<str>` is pointer-equal to the corresponding key in
/// `attribution_actors` / `attribution_identities`, sharing the
/// interning invariant. Default `Serialize` for `Arc<str>` emits a
/// JSON string, so the wire shape `{ "s": ..., "actor": ..., "time": ... }`
/// uses the field name `actor` for this struct's `actor` value.
#[derive(Debug, Clone, Serialize)]
pub struct AttributionRecord {
    pub actor: Arc<str>,
    pub time: i64,
}

/// Hit returned from `AttributionSource::query_byte_range`.
pub type AttributionHit = AttributionRecord;

/// Whether the given format's writer consumes the attribution lookup.
///
/// Used by `AttributionGenerateTransform`'s skip ladder to
/// short-circuit before invoking the provider; opting in to
/// attribution on a non-consuming format would otherwise fire a
/// `git blame` subprocess whose output goes nowhere visible.
///
/// In v1 returns `true` for HTML and q2-debug JSON only. The
/// `q2-debug` pseudo-format parses with `FormatIdentifier::Html` but
/// keeps its original string in `target_format`, so the HTML branch
/// covers both; `revealjs` has its own identifier and is excluded.
pub fn format_supports_attribution(format: &Format) -> bool {
    matches!(format.identifier, FormatIdentifier::Html)
}

/// Read user-authored `meta.attribution.identities` (a small
/// `ConfigValue::Map` from YAML parse) into an [`IdentityMap`] for
/// the Phase 2 merge step.
///
/// This is the *only* attribution-related `ConfigValue` → Rust-struct
/// converter the plan ships; the bulk `runs` path never visits
/// `ConfigValue`. Returns an empty map when the key is absent or
/// when the value is not a map.
///
/// The keys returned here are fresh `Arc<str>` allocations unrelated
/// to any provider's `AttributionRun.actor`. The Phase 2 merge step
/// uses them only as lookup keys and preserves the provider's
/// pointer-equal key on collision, so the writer-side
/// `Arc::ptr_eq` interning invariant is not weakened.
pub fn identity_map_from_meta(meta: &quarto_pandoc_types::ConfigValue) -> IdentityMap {
    let Some(identities) = meta.get("attribution").and_then(|v| v.get("identities")) else {
        return IdentityMap::new();
    };
    let Some(entries) = identities.as_map_entries() else {
        return IdentityMap::new();
    };
    let mut out = IdentityMap::new();
    for entry in entries {
        let display_name = entry.value.get("name").and_then(|v| v.as_plain_text());
        let color = entry.value.get("color").and_then(|v| v.as_plain_text());
        if let (Some(display_name), Some(color)) = (display_name, color) {
            out.insert(
                Arc::from(entry.key.as_str()),
                Identity {
                    display_name,
                    color,
                },
            );
        }
    }
    out
}

/// Read the YAML opt-out
/// `attribution: { source: git, viewer: false }`.
///
/// Returns `true` (viewer on, the default) when the key is absent,
/// when `attribution` is the short form (a string), or when the
/// `viewer` value is anything other than a literal `false`. Returns
/// `false` only on an explicit `viewer: false` (or `viewer: "false"`).
/// The opt-out is the only currently recognized value; richer
/// theming knobs are deferred per the plan's "out of scope".
///
/// Companion to [`identity_map_from_meta`] for the third
/// rich-form attribution key.
pub fn attribution_viewer_enabled_from_meta(meta: &quarto_pandoc_types::ConfigValue) -> bool {
    let Some(viewer) = meta.get("attribution").and_then(|v| v.get("viewer")) else {
        return true;
    };
    if let Some(b) = viewer.as_bool() {
        return b;
    }
    if let Some(s) = viewer.as_plain_text()
        && s.eq_ignore_ascii_case("false")
    {
        return false;
    }
    true
}
