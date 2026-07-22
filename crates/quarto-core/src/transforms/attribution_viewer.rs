/*
 * attribution_viewer.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Auto-inject the attribution viewer CSS + JS pair into
//! `rendered.includes.{header,after-body}`.
//!
//! Runs only when the upstream [`AttributionRenderTransform`] populated
//! `format_options.html.attribution_by_node` (i.e. attribution is
//! active for this HTML render) and the YAML opt-out
//! `attribution: { source: git, viewer: false }` was not set.
//!
//! The injected CSS is the concatenation of two payloads, kept in one
//! `<style>` block so the dedup sentinel covers both:
//!
//! 1. The static `viewer.css` resource — base paint rule, dotted
//!    underline, badge layout. Single source of truth shared with the
//!    hub-client.
//! 2. One `[data-attr-actor="<actor>"] { --attr-color: …;
//!    --attr-name: …; }` rule per distinct actor referenced by
//!    `attribution_identities`. The base paint rule consumes
//!    `--attr-color` via the cascade so identity is render-time CSS;
//!    `viewer.js` reads `--attr-name` from computed style when building
//!    the hover badge.
//!
//! Both payloads carry an HTML-comment sentinel so re-running the
//! transform on the same `ast.meta` does not double-inject.
//!
//! Mirrors the shape of [`WebsiteFaviconTransform`](super::WebsiteFaviconTransform):
//! append HTML literals to the canonical
//! `meta.rendered.includes.{header,after-body}` lists; the
//! `quarto-core` HTML template wires those slots into `<head>` and
//! before-`</body>` respectively.
//!
//! CLI-only by design: hub-client renders React components and binds
//! events on props, so it shares only the CSS asset (imported via
//! Vite's `?raw`) and ignores `rendered.includes.*` entirely. The
//! `"attribution-viewer"` name is on
//! [`Q2_PREVIEW_TRANSFORM_EXCLUDED`](super::super::pipeline::Q2_PREVIEW_TRANSFORM_EXCLUDED)
//! to enforce the design statement rather than rely on surface-level
//! no-op.

use std::fmt::Write as _;

use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::attribution::{IdentityMap, VIEWER_CSS, VIEWER_JS};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::append_with_sentinel;

/// HTML-comment sentinel embedded in the injected `<style>` block.
/// Used by the dedup scan so a transform re-run is idempotent.
const CSS_SENTINEL: &str = "<!-- quarto-attribution-viewer-css -->";

/// HTML-comment sentinel embedded in the injected `<script>` block.
const JS_SENTINEL: &str = "<!-- quarto-attribution-viewer-js -->";

pub struct AttributionViewerTransform;

impl AttributionViewerTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AttributionViewerTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for AttributionViewerTransform {
    fn name(&self) -> &str {
        "attribution-viewer"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // First gating signal: `AttributionRenderTransform` populated
        // the per-node lookup. Without it there are no wrappers in
        // the body, so the CSS/JS would have nothing to act on.
        if ctx.format_options.html.attribution_by_node.is_none() {
            return Ok(());
        }
        // Second gating signal: YAML opt-out. Default `true` so the
        // feature is discoverable; `viewer: false` flips it.
        if !ctx.format_options.html.attribution_viewer_enabled {
            return Ok(());
        }

        let identities_css = ctx
            .format_options
            .html
            .attribution_identities
            .as_deref()
            .map(render_per_actor_rules)
            .unwrap_or_default();
        let css_payload = format!(
            "{sentinel}\n<style>\n{base}{identities}</style>",
            sentinel = CSS_SENTINEL,
            base = VIEWER_CSS,
            identities = identities_css,
        );
        let js_payload = format!("{}\n<script>\n{}</script>", JS_SENTINEL, VIEWER_JS);

        append_with_sentinel(&mut ast.meta, "header", CSS_SENTINEL, css_payload);
        append_with_sentinel(&mut ast.meta, "after-body", JS_SENTINEL, js_payload);

        Ok(())
    }
}

/// Render one CSS rule per actor in `identities`. Each rule publishes
/// `--attr-color` and `--attr-name` on `[data-attr-actor="<actor>"]`;
/// the base paint rule in `viewer.css` then consumes `--attr-color`
/// via the cascade. Iteration order is sorted by actor key so the
/// emitted CSS is deterministic across renders.
///
/// Returns an empty string when there are no identities to publish.
/// Non-empty output always starts with a newline and ends with one so
/// concatenation with the static `viewer.css` stays tidy.
fn render_per_actor_rules(identities: &IdentityMap) -> String {
    if identities.is_empty() {
        return String::new();
    }
    let mut entries: Vec<(&str, &str, &str)> = identities
        .iter()
        .map(|(actor, id)| (actor.as_ref(), id.display_name.as_str(), id.color.as_str()))
        .collect();
    entries.sort_unstable_by_key(|(actor, _, _)| *actor);

    let mut out = String::new();
    out.push('\n');
    for (actor, name, color) in entries {
        let _ = writeln!(
            out,
            "[data-attr-actor=\"{actor}\"] {{ --attr-color: {color}; --attr-name: \"{name}\"; }}",
            actor = escape_css_string(actor),
            color = color,
            name = escape_css_string(name),
        );
    }
    out
}

/// Escape a Rust `&str` for safe inclusion inside a double-quoted CSS
/// string. Per the CSS Syntax Level 3 spec, only `"`, `\`, and raw
/// newlines are forbidden in a `"…"` token — escape those three. Other
/// characters (including `@`, `.`, `+`, non-ASCII) round-trip
/// unchanged.
fn escape_css_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\A "),
            '\r' => out.push_str("\\D "),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{escape_css_string, render_per_actor_rules};
    use crate::attribution::{Identity, IdentityMap};

    #[test]
    fn renders_one_rule_per_actor_sorted() {
        let mut m = IdentityMap::new();
        m.insert(
            Arc::from("bob@example.com"),
            Identity {
                display_name: "Bob".into(),
                color: "#88CCEE".into(),
            },
        );
        m.insert(
            Arc::from("alice@example.com"),
            Identity {
                display_name: "Alice".into(),
                color: "#CC6677".into(),
            },
        );
        let css = render_per_actor_rules(&m);
        // Alphabetical order — alice's rule appears before bob's.
        let alice_at = css
            .find("[data-attr-actor=\"alice@example.com\"]")
            .expect("alice rule present");
        let bob_at = css
            .find("[data-attr-actor=\"bob@example.com\"]")
            .expect("bob rule present");
        assert!(alice_at < bob_at, "actors emitted sorted ascending");
        assert!(css.contains("--attr-color: #CC6677"));
        assert!(css.contains("--attr-name: \"Alice\""));
    }

    #[test]
    fn empty_identities_emit_empty_string() {
        let m = IdentityMap::new();
        assert!(render_per_actor_rules(&m).is_empty());
    }

    #[test]
    fn escape_css_string_passes_safe_chars_through() {
        assert_eq!(escape_css_string("alice@example.com"), "alice@example.com");
        assert_eq!(escape_css_string("Alice O'Hara"), "Alice O'Hara");
        // Newlines and quotes get escaped. Backslash too.
        assert_eq!(escape_css_string("a\"b\\c\nd"), "a\\\"b\\\\c\\A d");
    }
}
