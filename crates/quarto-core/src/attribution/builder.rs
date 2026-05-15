/*
 * attribution/builder.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The single canonical-form constructor for [`AttributionData`].
//!
//! All producer call-sites (the two providers and test fixtures) go
//! through this builder; no producer should construct `AttributionRun`
//! literals with ad-hoc `Arc::from(s)` calls. The invariant the
//! builder enforces by construction is:
//!
//! > Every `AttributionRun.actor` in the built `AttributionData` is
//! > `Arc::ptr_eq` to the corresponding key in [`IdentityMap`].
//!
//! Callers pass `&str` actors throughout; the builder interns each
//! distinct string exactly once and reuses the resulting `Arc<str>`
//! for every `push_run` / `set_identity` referencing the same actor.
//! A previous revision exposed `intern_actor` and required callers to
//! thread the returned `Arc<str>` through `push_run` / `set_identity`
//! manually — that was a doc-only contract that a misuse would silently
//! break the writer-side `Arc::ptr_eq` invariant. The `&str` API makes
//! the invariant unforgeable.

use std::collections::HashMap;
use std::sync::Arc;

use super::types::{AttributionData, AttributionMap, AttributionRun, Identity, IdentityMap};

/// Build an [`AttributionData`] while preserving the `Arc<str>`
/// interning invariant — every actor string allocates exactly once,
/// and every reference to that actor across runs and the identity
/// map shares the same `Arc`.
#[derive(Debug, Default)]
pub struct AttributionDataBuilder {
    runs: Vec<AttributionRun>,
    identities: IdentityMap,
    intern: HashMap<String, Arc<str>>,
}

impl AttributionDataBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern `actor`, allocating an `Arc<str>` on first sight and
    /// `Arc::clone`-ing thereafter.
    fn intern(&mut self, actor: &str) -> Arc<str> {
        if let Some(existing) = self.intern.get(actor) {
            return Arc::clone(existing);
        }
        let arc: Arc<str> = Arc::from(actor);
        self.intern.insert(actor.to_string(), Arc::clone(&arc));
        arc
    }

    /// Append a run attributed to `actor`.
    pub fn push_run(&mut self, start: usize, end: usize, actor: &str, time: i64) {
        let actor = self.intern(actor);
        self.runs.push(AttributionRun {
            start,
            end,
            actor,
            time,
        });
    }

    /// Record (or overwrite) an identity for `actor`.
    pub fn set_identity(&mut self, actor: &str, id: Identity) {
        let actor = self.intern(actor);
        self.identities.insert(actor, id);
    }

    /// Record an identity for `actor` only if no identity has been
    /// set yet. Returns `true` iff the identity was inserted. Used
    /// by providers (e.g. `attribution_from_porcelain`) that walk a
    /// run list and want to fix the synthesised identity on first
    /// sight without paying for a per-run overwrite.
    pub fn set_identity_if_absent(&mut self, actor: &str, id: Identity) -> bool {
        let actor = self.intern(actor);
        if self.identities.contains_key(&actor) {
            return false;
        }
        self.identities.insert(actor, id);
        true
    }

    pub fn build(self) -> AttributionData {
        AttributionData {
            runs: AttributionMap(self.runs),
            identities: self.identities,
        }
    }
}
