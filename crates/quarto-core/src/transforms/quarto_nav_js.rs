/*
 * transforms/quarto_nav_js.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Ship the fixed-header JS (quarto-nav.js + headroom.min.js) when the
 * page renders a `#quarto-header`.
 */

//! Ship `quarto-nav.js` (fixed-header offset management) and
//! `headroom.min.js` (scroll-away behavior) as Project-scoped `js:*`
//! artifacts (bd-ersobfbt).
//!
//! ## Predicate
//!
//! The template emits `<header id="quarto-header" class="headroom
//! fixed-top">` when `rendered.navigation.navbar` or
//! `rendered.navigation.secondary-nav` is non-empty
//! (`template.rs::QUARTO_HEADER_PARTIAL` and its `$if$` gate). This
//! transform ships the JS **iff that header ships** — same signal, read
//! after the navbar / secondary-nav render transforms have run (this
//! transform must be registered after them in the Navigation phase).
//! Without a fixed header the script would be inert, so no
//! `ProjectKind` gate is needed: a standalone document that renders a
//! navbar gets (and needs) the same JS.
//!
//! `headroom.min.js` is omitted — Q1-parity, `websiteHeadroom()` in
//! `website-navigation.ts:1500-1509` — when the navbar or the sidebar
//! is `pinned: true`. `quarto-nav.js` still ships: the header is still
//! `fixed-top`, so the offset machinery is still required. The
//! `quarto-nav.js` init is guarded on `window.Headroom`, so the pinned
//! page simply keeps a permanently-pinned header.
//!
//! Q1 deviation, deliberate: Q1 disables headroom site-wide when *any*
//! sidebar in the (possibly multi-sidebar) config is pinned; q2 reads
//! the page's *resolved* `navigation.sidebar`, so a multi-sidebar site
//! pins per-section. Per-page is the more faithful reading of the
//! option; revisit only if a parity complaint arrives.
//!
//! ## Script ordering
//!
//! [`ApplyTemplateStage`](crate::stage::stages::ApplyTemplateStage)
//! emits `<script>` tags in sorted-key order: `js:bootstrap` <
//! `js:clipboard` < `js:code-copy-init` < `js:quarto-nav:headroom` <
//! `js:quarto-nav:nav` < `js:tabsets`. Both files attach their work to
//! `DOMContentLoaded` (and `quarto-nav.js` guards on `window.Headroom`
//! at that point), so intra-pair order is not load-bearing — but the
//! keys are chosen so headroom still loads first.
//!
//! ## WASM
//!
//! The module is `cfg(not(target_arch = "wasm32"))` (gated at
//! `transforms/mod.rs`): the hub-client preview excludes
//! `ApplyTemplateStage`, so a `js:*` artifact would never become a
//! `<script>` tag there. The preview injects the same two vendored
//! files itself at `ts-packages/preview-renderer/src/q2-preview/
//! entry.tsx` module top (the Phase F.1 Bootstrap pattern). Native and
//! preview must reference the same `resources/js/` bytes.
//!
//! Slated for replacement by the sticky-header redesign (bd-pt1wxeq2);
//! the predicate survives that change, the shipped files do not.

use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::artifact::{Artifact, ArtifactScope, ArtifactStore};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// headroom.js v0.12.0 (MIT), byte-identical to Q1's vendored copy.
/// Version contract: `resources/js/README.md` § `headroom/`.
const HEADROOM_JS: &[u8] = include_bytes!("../../../../resources/js/headroom/headroom.min.js");

/// The q2 port of Q1's header-machinery subset of quarto-nav.js.
const QUARTO_NAV_JS: &[u8] = include_bytes!("../../../../resources/js/quarto-nav/quarto-nav.js");

/// Artifact keys. Sort order is load order (see module docs).
const HEADROOM_KEY: &str = "js:quarto-nav:headroom";
const NAV_KEY: &str = "js:quarto-nav:nav";

/// On-disk layout mirrors Q1's `site_libs/quarto-nav/`.
const HEADROOM_REL_PATH: &str = "quarto-nav/headroom.min.js";
const NAV_REL_PATH: &str = "quarto-nav/quarto-nav.js";

/// What the predicate decided for this page.
#[derive(Debug, PartialEq, Eq)]
struct NavJsDecision {
    /// Ship `quarto-nav.js` (the page has a `#quarto-header`).
    ship_nav: bool,
    /// Additionally ship `headroom.min.js` (nothing is pinned).
    ship_headroom: bool,
}

/// Decide from document metadata. Pure so the predicate is unit-testable
/// without a `RenderContext`.
fn decide(meta: &ConfigValue) -> NavJsDecision {
    // Same signal as the template's header gate: a non-empty rendered
    // navbar or secondary nav means `#quarto-header.fixed-top` ships.
    let rendered_non_empty = |key: &str| {
        meta.get_path(&["rendered", "navigation", key])
            .and_then(|v| v.as_plain_text())
            .is_some_and(|s| !s.is_empty())
    };
    let has_header = rendered_non_empty("navbar") || rendered_non_empty("secondary-nav");
    if !has_header {
        return NavJsDecision {
            ship_nav: false,
            ship_headroom: false,
        };
    }

    // Q1's `websiteHeadroom()`: a pinned navbar or pinned sidebar
    // suppresses the scroll-away script (and only that).
    let pinned = |nav_key: &str| {
        meta.get_path(&["navigation", nav_key, "pinned"])
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    NavJsDecision {
        ship_nav: true,
        ship_headroom: !pinned("navbar") && !pinned("sidebar"),
    }
}

/// Idempotently store the artifacts the decision calls for.
/// `ArtifactStore::store` overwrites by key, so re-running is fine.
fn store_artifacts(artifacts: &mut ArtifactStore, decision: &NavJsDecision) {
    if !decision.ship_nav {
        return;
    }
    artifacts.store(
        NAV_KEY,
        Artifact::from_bytes(QUARTO_NAV_JS.to_vec(), "text/javascript")
            .with_path(NAV_REL_PATH)
            .with_scope(ArtifactScope::Project),
    );
    if decision.ship_headroom {
        artifacts.store(
            HEADROOM_KEY,
            Artifact::from_bytes(HEADROOM_JS.to_vec(), "text/javascript")
                .with_path(HEADROOM_REL_PATH)
                .with_scope(ArtifactScope::Project),
        );
    }
}

pub struct QuartoNavJsTransform;

impl QuartoNavJsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for QuartoNavJsTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for QuartoNavJsTransform {
    fn name(&self) -> &str {
        "quarto-nav-js"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let decision = decide(&ast.meta);
        store_artifacts(&mut ctx.artifacts, &decision);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;
    use std::path::Path;

    fn meta_with(paths: &[(&[&str], ConfigValue)]) -> ConfigValue {
        let mut meta = ConfigValue::null(SourceInfo::for_test());
        for (path, value) in paths {
            meta.insert_path(path, value.clone());
        }
        meta
    }

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, SourceInfo::for_test())
    }

    fn b(v: bool) -> ConfigValue {
        ConfigValue::new_bool(v, SourceInfo::for_test())
    }

    #[test]
    fn ships_both_when_navbar_rendered() {
        let meta = meta_with(&[(
            &["rendered", "navigation", "navbar"],
            s("<nav class=\"navbar\">N</nav>"),
        )]);
        assert_eq!(
            decide(&meta),
            NavJsDecision {
                ship_nav: true,
                ship_headroom: true
            }
        );
    }

    #[test]
    fn ships_both_when_only_secondary_nav_rendered() {
        // Sidebar-only site: the header exists (it holds the
        // narrow-viewport secondary nav), so the offset JS is needed.
        let meta = meta_with(&[(
            &["rendered", "navigation", "secondary-nav"],
            s("<nav class=\"quarto-secondary-nav\">S</nav>"),
        )]);
        assert_eq!(
            decide(&meta),
            NavJsDecision {
                ship_nav: true,
                ship_headroom: true
            }
        );
    }

    #[test]
    fn ships_nothing_without_header() {
        let meta = meta_with(&[]);
        assert_eq!(
            decide(&meta),
            NavJsDecision {
                ship_nav: false,
                ship_headroom: false
            }
        );
    }

    #[test]
    fn empty_rendered_navbar_does_not_ship() {
        // An empty string means "nothing rendered" — the template's
        // `$if$` treats it as falsy and emits no header.
        let meta = meta_with(&[(&["rendered", "navigation", "navbar"], s(""))]);
        assert_eq!(
            decide(&meta),
            NavJsDecision {
                ship_nav: false,
                ship_headroom: false
            }
        );
    }

    #[test]
    fn pinned_navbar_omits_headroom_keeps_nav() {
        let meta = meta_with(&[
            (
                &["rendered", "navigation", "navbar"],
                s("<nav class=\"navbar\">N</nav>"),
            ),
            (&["navigation", "navbar", "pinned"], b(true)),
        ]);
        assert_eq!(
            decide(&meta),
            NavJsDecision {
                ship_nav: true,
                ship_headroom: false
            }
        );
    }

    #[test]
    fn pinned_sidebar_omits_headroom_keeps_nav() {
        let meta = meta_with(&[
            (
                &["rendered", "navigation", "navbar"],
                s("<nav class=\"navbar\">N</nav>"),
            ),
            (&["navigation", "sidebar", "pinned"], b(true)),
        ]);
        assert_eq!(
            decide(&meta),
            NavJsDecision {
                ship_nav: true,
                ship_headroom: false
            }
        );
    }

    #[test]
    fn pinned_false_still_ships_headroom() {
        let meta = meta_with(&[
            (
                &["rendered", "navigation", "navbar"],
                s("<nav class=\"navbar\">N</nav>"),
            ),
            (&["navigation", "navbar", "pinned"], b(false)),
        ]);
        assert_eq!(
            decide(&meta),
            NavJsDecision {
                ship_nav: true,
                ship_headroom: true
            }
        );
    }

    #[test]
    fn store_ships_both_artifacts_with_project_scope() {
        let mut store = ArtifactStore::new();
        store_artifacts(
            &mut store,
            &NavJsDecision {
                ship_nav: true,
                ship_headroom: true,
            },
        );

        let nav = store.get(NAV_KEY).expect("quarto-nav.js stored");
        assert_eq!(nav.scope, ArtifactScope::Project);
        assert_eq!(nav.path.as_deref(), Some(Path::new(NAV_REL_PATH)));
        assert_eq!(nav.content_type, "text/javascript");
        assert!(!nav.content.is_empty());

        let headroom = store.get(HEADROOM_KEY).expect("headroom.min.js stored");
        assert_eq!(headroom.scope, ArtifactScope::Project);
        assert_eq!(headroom.path.as_deref(), Some(Path::new(HEADROOM_REL_PATH)));
        assert_eq!(headroom.content_type, "text/javascript");
        let head_len = headroom.content.len().min(120);
        let head = String::from_utf8_lossy(&headroom.content[..head_len]).replace("\r\n", "\n");
        assert!(
            head.contains("headroom.js v0.12.0"),
            "vendored headroom must be v0.12.0; got header: {head}"
        );
    }

    #[test]
    fn store_pinned_ships_nav_only() {
        let mut store = ArtifactStore::new();
        store_artifacts(
            &mut store,
            &NavJsDecision {
                ship_nav: true,
                ship_headroom: false,
            },
        );
        assert!(store.get(NAV_KEY).is_some());
        assert!(store.get(HEADROOM_KEY).is_none());
    }

    #[test]
    fn store_no_header_ships_nothing() {
        let mut store = ArtifactStore::new();
        store_artifacts(
            &mut store,
            &NavJsDecision {
                ship_nav: false,
                ship_headroom: false,
            },
        );
        assert!(store.get(NAV_KEY).is_none());
        assert!(store.get(HEADROOM_KEY).is_none());
    }

    #[test]
    fn keys_sort_after_bootstrap_and_before_tabsets() {
        // ApplyTemplateStage emits scripts in sorted-key order; pin the
        // relative order this module's docs promise.
        let mut keys = vec![
            "js:tabsets",
            NAV_KEY,
            "js:bootstrap",
            HEADROOM_KEY,
            "js:clipboard",
            "js:code-copy-init",
        ];
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "js:bootstrap",
                "js:clipboard",
                "js:code-copy-init",
                HEADROOM_KEY,
                NAV_KEY,
                "js:tabsets",
            ]
        );
    }
}
