/*
 * attribution_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Attribution generate transform.
//!
//! Reads `ctx.attribution_provider`, calls `build(ctx)?` to obtain an
//! [`AttributionData`], merges it with any user-authored
//! `meta.attribution.identities` (provider wins on
//! `AttributionRun.actor` Arc identity; user wins on identity value
//! on key collision; non-colliding user keys are dropped), and stores
//! the result on `ctx.attribution_data`.
//!
//! Registered at the **tail of the Navigation Phase**, immediately
//! after `FooterRenderTransform`. The entire Finalization Phase runs
//! between this stage and [`AttributionRenderTransform`].
//!
//! ## Two invocation paths
//!
//! - HTML CLI path: registered in `build_transform_pipeline`; runs as
//!   part of the full transform pipeline.
//! - q2-debug WASM path: invoked **directly** by
//!   `parse_qmd_to_ast_with_attribution` after the existing 3-stage
//!   parse.
//!
//! **Both paths must produce identical results.** This transform
//! reads and writes only [`RenderContext`] fields; it must never
//! reach for `StageContext`.
//!
//! [`AttributionData`]: crate::attribution::AttributionData
//! [`AttributionRenderTransform`]: super::AttributionRenderTransform

use std::sync::Arc;

use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::attribution::{AttributionData, format_supports_attribution, identity_map_from_meta};
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;

/// See module docs.
pub struct AttributionGenerateTransform;

impl AttributionGenerateTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AttributionGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for AttributionGenerateTransform {
    fn name(&self) -> &str {
        "attribution-generate"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Skip ladder, in order:
        //
        // 1. Format must consume the lookup. Bail first so opting in
        //    on a non-HTML target doesn't spawn the provider's
        //    subprocess for nothing.
        if !format_supports_attribution(ctx.format) {
            return Ok(());
        }

        // 2. Affirmative `attribution: false` opt-out wins over any
        //    provider installed by the CLI / WASM entry point.
        if is_feature_disabled(&ast.meta, "attribution") {
            return Ok(());
        }

        // 3. No provider installed → nothing to do; sidecar stays None.
        let Some(provider) = ctx.attribution_provider.clone() else {
            return Ok(());
        };

        // 4. Build provider data, merge identities with any
        //    user-authored `meta.attribution.identities`, store sidecar.
        let AttributionData {
            runs,
            mut identities,
        } = provider.build(ctx)?;

        // Preserve provider Arc<str> keys on collision (the
        // interning invariant in `IdentityMap` and `AttributionRun`
        // depends on `Arc::ptr_eq` between them) — `HashMap::get_mut`
        // returns a `&mut Identity` without touching the key. Drop
        // non-colliding user keys (an actor named in YAML but with
        // no runs in this document is invisible at the writer and
        // would be dead weight in the map).
        for (user_key, user_id) in identity_map_from_meta(&ast.meta) {
            if let Some(slot) = identities.get_mut(&user_key) {
                *slot = user_id;
            }
        }

        ctx.attribution_data = Some(Arc::new(AttributionData { runs, identities }));
        Ok(())
    }
}
