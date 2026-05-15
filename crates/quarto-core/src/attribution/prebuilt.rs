/*
 * attribution/prebuilt.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Provider that wraps a hub-client-supplied transport JSON string
//! and decodes it on demand.
//!
//! The JSON is parsed lazily inside [`AttributionSourceProvider::build`]
//! rather than at construction time so that:
//! - construction is infallible (no `Result` at the WASM entry point),
//!   and
//! - the parse + intern step lives behind the same provider trait
//!   surface as `GitBlameProvider`, so a future caller cannot
//!   distinguish the two by where errors surface.

use super::builder::AttributionDataBuilder;
use super::source::AttributionSourceProvider;
use super::types::{AttributionData, TransportAttributionData};
use crate::Result;
use crate::error::QuartoError;
use crate::render::RenderContext;

/// Wraps a transport JSON string. Decodes via
/// [`super::types::TransportAttributionData`] then re-interns through
/// [`super::builder::AttributionDataBuilder`] in `build`.
#[derive(Debug, Clone)]
pub struct PreBuiltAttributionProvider {
    json: String,
}

impl PreBuiltAttributionProvider {
    pub fn new(json: String) -> Self {
        Self { json }
    }

    /// For testing: the raw transport JSON payload this provider was
    /// constructed with.
    pub fn json(&self) -> &str {
        &self.json
    }
}

impl AttributionSourceProvider for PreBuiltAttributionProvider {
    fn build(&self, _ctx: &RenderContext) -> Result<AttributionData> {
        let raw: TransportAttributionData = serde_json::from_str(&self.json).map_err(|e| {
            QuartoError::other(format!("attribution: failed to parse transport JSON: {e}"))
        })?;
        let mut b = AttributionDataBuilder::new();
        // The builder interns each actor on first sight, so identity
        // entries and run entries referencing the same actor share an
        // `Arc::ptr_eq` key regardless of insertion order — the
        // writer-side invariant.
        for (k, id) in raw.identities {
            b.set_identity(&k, id);
        }
        for r in raw.runs {
            b.push_run(r.start, r.end, &r.actor, r.time);
        }
        Ok(b.build())
    }
}
