/*
 * attribution/handle.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Bridge between `quarto-core`'s canonical [`AttributionData`] and
//! the [`pampa::attribution::AttributionLookup`] trait that the Lua
//! filter runner consumes.
//!
//! [`AttributionLookupHandle`] wraps an `Arc<AttributionData>` and
//! implements the trait. `quarto-core::stage::stages::UserFiltersStage`
//! constructs it from `ctx.attribution_data` and threads it into
//! [`pampa::unified_filter::apply_filters`] alongside the runtime.
//!
//! Plain `String` translations (vs the canonical `Arc<str>`) happen
//! per call: the Lua boundary doesn't care about pampa's interning
//! invariant, and per-call cloning is dominated by the Lua VM call
//! cost. See `claude-notes/plans/2026-05-15-attribution-lua-binding-plan.md`
//! § "Phase 4a (Option β)".

use std::sync::Arc;

use pampa::attribution::{AttributionLookup, IdentityEntry, LookupHit};

use super::source::AttributionSource;
use super::types::AttributionData;

/// `pampa` adapter for [`AttributionData`].
///
/// Cheap to clone (it's an `Arc`); designed to be passed by value
/// into `apply_filters` once per filter pass.
#[derive(Debug, Clone)]
pub struct AttributionLookupHandle(pub Arc<AttributionData>);

impl AttributionLookupHandle {
    pub fn new(data: Arc<AttributionData>) -> Self {
        Self(data)
    }
}

impl AttributionLookup for AttributionLookupHandle {
    fn lookup_range(&self, start: usize, end: usize) -> Option<LookupHit> {
        let hit = self.0.runs.query_byte_range(start, end)?;
        Some(LookupHit {
            actor: hit.actor.to_string(),
            time: hit.time,
        })
    }

    fn blamed_file_id(&self) -> usize {
        self.0.file_id.0
    }

    fn identities(&self) -> Vec<IdentityEntry> {
        self.0
            .identities
            .iter()
            .map(|(actor, identity)| IdentityEntry {
                actor: actor.to_string(),
                name: identity.display_name.clone(),
                color: identity.color.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribution::AttributionDataBuilder;
    use crate::attribution::Identity;

    fn make_data() -> AttributionData {
        let mut b = AttributionDataBuilder::new();
        b.set_identity(
            "alice@example.com",
            Identity {
                display_name: "Alice".to_string(),
                color: "#ff0000".to_string(),
            },
        );
        b.set_identity(
            "bob@example.com",
            Identity {
                display_name: "Bob".to_string(),
                color: "#00ff00".to_string(),
            },
        );
        b.push_run(0, 5, "alice@example.com", 1);
        b.push_run(5, 10, "bob@example.com", 2);
        b.build()
    }

    #[test]
    fn lookup_range_returns_most_recent_run_overlapping() {
        let handle = AttributionLookupHandle::new(Arc::new(make_data()));
        // Range [2, 8) overlaps both runs; bob has higher time.
        let hit = handle.lookup_range(2, 8).expect("hit");
        assert_eq!(hit.actor, "bob@example.com");
        assert_eq!(hit.time, 2);
    }

    #[test]
    fn lookup_range_returns_none_on_no_overlap() {
        let handle = AttributionLookupHandle::new(Arc::new(make_data()));
        assert!(handle.lookup_range(100, 200).is_none());
    }

    #[test]
    fn lookup_range_returns_none_on_empty_data() {
        let handle = AttributionLookupHandle::new(Arc::new(AttributionData::default()));
        assert!(handle.lookup_range(0, 10).is_none());
    }

    #[test]
    fn identities_passthrough_matches_input() {
        let handle = AttributionLookupHandle::new(Arc::new(make_data()));
        let mut ids = handle.identities();
        ids.sort_by(|a, b| a.actor.cmp(&b.actor));
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0].actor, "alice@example.com");
        assert_eq!(ids[0].name, "Alice");
        assert_eq!(ids[0].color, "#ff0000");
        assert_eq!(ids[1].actor, "bob@example.com");
        assert_eq!(ids[1].name, "Bob");
        assert_eq!(ids[1].color, "#00ff00");
    }
}
