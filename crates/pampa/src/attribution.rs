/*
 * attribution.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pampa-side surface for the per-document attribution sidecar.
//!
//! `pampa` cannot depend on `quarto-core` (the dependency direction
//! is `quarto-core → pampa`), but the Lua filter runner inside
//! `pampa::lua` needs to expose attribution data via the
//! `quarto.attribution.*` host binding. This module defines the
//! abstract surface — a small trait plus plain-data return types —
//! that callers in `quarto-core` (the `AttributionLookupHandle`
//! wrapping an `Arc<AttributionData>`) plug into when invoking
//! `apply_filters`.
//!
//! Plain `String` here is deliberate: pampa doesn't need the
//! interning invariant carried by `quarto-core`'s `Arc<str>` actors.
//! Per-call cloning is negligible next to the Lua VM call cost.
//!
//! See `claude-notes/plans/2026-05-15-attribution-lua-binding-plan.md`
//! for the bd-0fd0 design context.

/// Read-only lookup surface for the attribution sidecar.
///
/// Implementations live in `quarto-core::attribution::handle` —
/// callers in `quarto-core::stage::stages::UserFiltersStage`
/// construct an [`AttributionLookupHandle`](../../quarto_core/attribution/handle/struct.AttributionLookupHandle.html)
/// from an `Arc<AttributionData>` and pass `Some(handle.clone())` to
/// [`crate::unified_filter::apply_filters`].
///
/// `Send + Sync` is required so the handle can be cloned into a
/// `Arc<dyn AttributionLookup>` and shared across the
/// `unified_filter` → `lua::filter` boundary, which carries the
/// future across a `tokio::task::block_in_place` bridge on native.
/// The query methods are sync and cheap (binary search + identity
/// clone); no I/O is involved.
pub trait AttributionLookup: Send + Sync {
    /// Most-recent `(actor, time)` hit overlapping the given byte
    /// range in the primary file. Returns `None` when no run covers
    /// the range, when `start >= end`, or when no provider is
    /// installed.
    fn lookup_range(&self, start: usize, end: usize) -> Option<LookupHit>;

    /// Identity map snapshot. Cheap clone — internally a `Vec` of
    /// owned strings sized by distinct-actor count.
    fn identities(&self) -> Vec<IdentityEntry>;
}

/// Raw hit returned by [`AttributionLookup::lookup_range`].
///
/// Does **not** carry display-name / colour — call
/// [`AttributionLookup::identities`] (or the Lua-side
/// `quarto.attribution.lookup(el)` which joins them automatically)
/// for those.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupHit {
    pub actor: String,
    pub time: i64,
}

/// One entry in the identity map. Mirrors `quarto-core`'s
/// `Identity` minus the `Arc<str>` interning concern (irrelevant
/// across the Lua boundary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityEntry {
    pub actor: String,
    pub name: String,
    pub color: String,
}
