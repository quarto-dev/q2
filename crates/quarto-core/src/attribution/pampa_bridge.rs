/*
 * attribution/pampa_bridge.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Crate-boundary conversion between [`HtmlFormatOptions`] /
//! [`JsonFormatOptions`] (the writer-side bag populated by
//! [`AttributionRenderTransform`]) and the pampa-facing config record
//! types.
//!
//! Three call sites need this translation: the HTML body stage
//! (`stage::stages::render_html`), the q2-preview JSON writer call
//! inside `pipeline::render_qmd_to_preview_ast`, and the q2-debug
//! WASM entry point in `wasm-quarto-hub-client`. Centralising the
//! per-field clone in one module keeps the three writers' attribution
//! plumbing in lockstep — adding a field to one side of the pair
//! becomes a single edit here plus the matching pampa-side record
//! change, rather than three hand-written copies.
//!
//! [`AttributionRenderTransform`]: crate::transforms::AttributionRenderTransform
//! [`HtmlFormatOptions`]: crate::render::HtmlFormatOptions
//! [`JsonFormatOptions`]: crate::render::JsonFormatOptions

use std::collections::HashMap;
use std::sync::Arc;

use pampa::writers::html::HtmlAttributionRecord;
use pampa::writers::json::{JsonAttributionIdentity, JsonAttributionRecord};

use crate::render::{HtmlFormatOptions, JsonFormatOptions};

/// Translate the writer-side HTML attribution field on `opts` into
/// the pampa-facing map consumed by `HtmlConfig`. Off-path (`None`)
/// the output is `None`, preserving the writer's byte-identicality
/// contract.
///
/// Identity (display name, colour) is not bridged: the HTML writer
/// is identity-free since [`crate::transforms::AttributionViewerTransform`]
/// publishes per-actor identity as CSS custom properties in `<head>`.
pub fn html_attribution_fields(
    opts: &HtmlFormatOptions,
) -> Option<Arc<HashMap<usize, HtmlAttributionRecord>>> {
    opts.attribution_by_node.as_ref().map(|map| {
        Arc::new(
            map.iter()
                .map(|(k, v)| {
                    (
                        *k,
                        HtmlAttributionRecord {
                            actor: Arc::clone(&v.actor),
                            time: v.time,
                        },
                    )
                })
                .collect(),
        )
    })
}

/// Translate the writer-side JSON attribution fields on `opts` into
/// the pampa-facing maps consumed by `JsonConfig`. Off-path (both
/// fields `None`) both outputs are `None`, so the writer's
/// byte-identicality contract holds.
pub fn json_attribution_fields(
    opts: &JsonFormatOptions,
) -> (
    Option<Arc<HashMap<usize, JsonAttributionRecord>>>,
    Option<Arc<HashMap<Arc<str>, JsonAttributionIdentity>>>,
) {
    let by_node = opts.attribution_by_node.as_ref().map(|map| {
        Arc::new(
            map.iter()
                .map(|(k, v)| {
                    (
                        *k,
                        JsonAttributionRecord {
                            actor: Arc::clone(&v.actor),
                            time: v.time,
                        },
                    )
                })
                .collect(),
        )
    });
    let actors = opts.attribution_actors.as_ref().map(|map| {
        Arc::new(
            map.iter()
                .map(|(k, v)| {
                    (
                        Arc::clone(k),
                        JsonAttributionIdentity {
                            display_name: v.display_name.clone(),
                            color: v.color.clone(),
                        },
                    )
                })
                .collect(),
        )
    });
    (by_node, actors)
}
