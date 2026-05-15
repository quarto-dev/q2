/*
 * attribution/mod.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Per-node authorship attribution.
//!
//! See `claude-notes/plans/CURRENT.md` for the full design. The
//! single-paragraph summary:
//!
//! - The canonical in-memory shape is [`AttributionData`], held as
//!   `Arc<AttributionData>` on [`RenderContext::attribution_data`] —
//!   the sidecar. It is NEVER stored in `ast.meta`.
//! - [`AttributionGenerateTransform`] (registered at the tail of the
//!   Navigation Phase) calls the installed
//!   [`AttributionSourceProvider`] and merges the result with any
//!   user-authored `meta.attribution.identities`, then stores the
//!   sidecar on `RenderContext`.
//! - [`AttributionRenderTransform`] (registered last in the
//!   Finalization Phase) walks the AST once, builds a writer-side
//!   pre-baked lookup table (`Arc<[Option<AttributionRecord>]>`),
//!   and resolves identity exactly once per distinct actor.
//! - The format-specialised writer (HTML body, q2-debug JSON) emits
//!   the lookup output.
//!
//! Both Generate and Render are no-ops when no provider has been
//! installed on `RenderContext` — same code path as the unflagged
//! default.
//!
//! [`AttributionGenerateTransform`]: crate::transforms::AttributionGenerateTransform
//! [`AttributionRenderTransform`]: crate::transforms::AttributionRenderTransform
//! [`RenderContext::attribution_data`]: crate::render::RenderContext::attribution_data

pub mod builder;
pub mod git_blame;
pub mod mode;
pub mod palette;
pub mod pampa_bridge;
pub mod prebuilt;
pub mod source;
pub mod types;

pub use builder::AttributionDataBuilder;
pub use git_blame::{
    BlameLine, BlameRun, GitBlameProvider, attribution_from_porcelain, build_blame_runs,
    parse_blame_porcelain,
};
pub use mode::{AttributionMode, resolve_attribution_mode};
pub use palette::{actor_color, fnv1a_hex8};
pub use pampa_bridge::{html_attribution_fields, json_attribution_fields};
pub use prebuilt::PreBuiltAttributionProvider;
pub use source::{AttributionSource, AttributionSourceProvider};
pub use types::{
    AttributionData, AttributionHit, AttributionMap, AttributionRecord, AttributionRun, Identity,
    IdentityMap, TransportAttributionData, TransportAttributionRun,
    attribution_viewer_enabled_from_meta, format_supports_attribution, identity_map_from_meta,
};

/// Inline CSS auto-injected by `AttributionViewerTransform`. Single
/// source of truth lives at repo-root `resources/attribution/viewer.css`;
/// the hub-client imports the same file via Vite's `?raw` mechanism so
/// both surfaces share badge class names. See the path comment on the
/// `include_str!` count.
pub(crate) const VIEWER_CSS: &str = include_str!("../../../../resources/attribution/viewer.css");

/// Inline JS auto-injected by `AttributionViewerTransform`. CLI-only:
/// the hub-client binds hover via React props, not DOM listeners.
pub(crate) const VIEWER_JS: &str = include_str!("../../../../resources/attribution/viewer.js");

#[cfg(test)]
mod viewer_asset_tests {
    use super::{VIEWER_CSS, VIEWER_JS};

    /// Phase C invariant: the embedded CSS carries the shared badge
    /// class names that form the contract with the hub-client's
    /// `framework/attribution.tsx`. Drift either direction would
    /// silently break visual presentation on one surface.
    #[test]
    fn viewer_css_mentions_hub_client_classes() {
        for class in ["q2-attr-badge", "q2-attr-badge-dot", "q2-attr-badge-time"] {
            assert!(
                VIEWER_CSS.contains(class),
                "viewer.css must mention .{} (shared with hub-client)",
                class
            );
        }
    }

    /// Pin the wrapper-recolour mechanism. Colour assignment is
    /// render-time CSS now: each `[data-attr-actor]` element inherits
    /// its colour from a per-actor rule (`--attr-color`) emitted by
    /// `AttributionViewerTransform`. The static stylesheet carries the
    /// base paint rule so descendants inherit via the cascade. The
    /// hub-client's React `AttributionWrap` sets the same colour
    /// inline on the wrap element directly; both paths converge on
    /// `.q2-attr-badge`'s `var(--attr-color)` for the badge frame.
    #[test]
    fn viewer_css_paints_wrappers_via_attr_color_var() {
        assert!(
            VIEWER_CSS.contains("var(--attr-color"),
            "viewer.css must paint wrapped elements via var(--attr-color); \
             render-time CSS replaces the previous JS paint pass"
        );
    }

    /// The hover-badge JS must read identity via the CSS custom
    /// properties published by `AttributionViewerTransform`'s per-actor
    /// rules. Pins the boundary between the renderer (CSS) and the
    /// interaction (JS).
    #[test]
    fn viewer_js_reads_identity_from_css_vars() {
        assert!(
            VIEWER_JS.contains("--attr-color") && VIEWER_JS.contains("--attr-name"),
            "viewer.js must read `--attr-color` / `--attr-name` from each \
             wrapper's computed style (identity is no longer inlined as \
             `data-attr-*`)"
        );
    }
}
