/*
 * secondary_nav_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for the website mobile secondary-nav bar
//! (bd-26bf3j1y).
//!
//! Reads the resolved `navigation.sidebar` and writes
//! `rendered.navigation.secondary-nav` — Q1's
//! `nav.quarto-secondary-nav`, the whole narrow-viewport navigation
//! bar. SCSS hides it at `lg`+; below that it is the only way to reach
//! the sidebar, which is collapsed into a drawer.
//!
//! ## Relationship to [`BreadcrumbsRenderTransform`]
//!
//! Both derive a trail from the same sidebar, and they deliberately do
//! not share a result, because Q1 gates the two instances differently:
//!
//! | | title-block instance | this (mobile) instance |
//! |---|---|---|
//! | classes | `quarto-title-breadcrumbs d-none d-lg-block` | none |
//! | trail-length gate | `> 1` crumb | **none** — renders at 1, and even at 0 |
//! | `bread-crumbs: false` | nothing rendered | collapsed page title instead |
//!
//! The "renders at 1" rule was confirmed against Q1's rendered output
//! (a single-crumb `nav.quarto-page-breadcrumbs` on a top-level sidebar
//! page), not just its sources.
//!
//! A page that is not in the sidebar at all gets an empty trail, and
//! Q1 still emits the bar with an empty breadcrumb list. That is the
//! right behavior rather than an oversight: the bar carries the sidebar
//! toggle, and a page outside the sidebar still needs a way to open it.
//!
//! ## Rendered on native AND WASM (bd-ersobfbt lifted decision 3)
//!
//! At introduction (bd-26bf3j1y decision 3) this transform was
//! native-only, on the premise that the hub-client preview
//! reinitialized its iframe every render tick and so shipped no
//! Bootstrap JS — an inert toggle being worse than none. That premise
//! went stale: the preview iframe is persistent, and Phase F.1
//! (bd-kw93.14) injects Bootstrap's bundle at `entry.tsx` module top,
//! so the collapse toggle works in preview. bd-ersobfbt lifted the
//! `cfg` in `pipeline.rs`; `PreviewDocument.tsx` renders the bar via
//! `SecondaryNavSlot` inside the `#quarto-header` wrapper.
//! Plans: `claude-notes/plans/2026-08-17-website-secondary-nav-mobile.md`,
//! `claude-notes/plans/2026-08-21-headroom-fixed-top-header.md`.
//!
//! ## Skip conditions
//!
//! - `sidebar` feature disabled, or `navigation.sidebar` absent (no
//!   toggle target → no bar).
//! - `rendered.navigation.secondary-nav` already populated (user
//!   override, same convention as the sibling renderers).

use quarto_navigation::render_html::{
    SecondaryNavContent, breadcrumbs_to_html, secondary_nav_to_html,
};
use quarto_navigation::{Sidebar, breadcrumb_trail};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::language::LanguageTerms;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::navigation_active::page_relative_source;
use crate::transforms::navigation_href::{NavSurface, resolve_href_for_html};
use crate::transforms::{is_feature_disabled, resolve_website_bool};

/// Fallback when the resolved language table has no `toggle-sidebar`
/// term. Matches `resources/language/_language.yml`.
const DEFAULT_TOGGLE_LABEL: &str = "Toggle sidebar navigation";

pub struct SecondaryNavRenderTransform;

impl SecondaryNavRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SecondaryNavRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for SecondaryNavRenderTransform {
    fn name(&self) -> &str {
        "secondary-nav-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "sidebar") {
            return Ok(());
        }
        if ast
            .meta
            .contains_path(&["rendered", "navigation", "secondary-nav"])
        {
            return Ok(());
        }
        let Some(sidebar_cv) = ast.meta.get_path(&["navigation", "sidebar"]) else {
            return Ok(());
        };
        let sidebar = Sidebar::from_config_value(sidebar_cv);

        let toggle_label = LanguageTerms::from_meta(&ast.meta)
            .and_then(|terms| terms.get("toggle-sidebar").map(str::to_string))
            .unwrap_or_else(|| DEFAULT_TOGGLE_LABEL.to_string());

        let show_breadcrumbs = resolve_website_bool(&ast.meta, "bread-crumbs", true);

        let html = if show_breadcrumbs {
            let page_source = page_relative_source(ctx);
            let mut crumbs = breadcrumb_trail(&sidebar, &page_source);
            // Crumb hrefs are sidebar hrefs, already resolved and
            // diagnosed by sidebar rendering; resolve through the same
            // helper but discard the duplicate diagnostics. Same
            // reasoning as `BreadcrumbsRenderTransform`.
            let mut discarded = Vec::new();
            for crumb in &mut crumbs {
                if let Some(href) = crumb.href.as_mut() {
                    *href = resolve_href_for_html(
                        href,
                        ctx.resource_resolver.as_ref(),
                        ctx.project_index.as_deref(),
                        NavSurface::Sidebar {
                            id: sidebar.id.as_deref(),
                        },
                        None,
                        &mut discarded,
                    );
                }
            }
            // No extra classes, and no trail-length gate — see the
            // comparison table in the module docs.
            let breadcrumbs = breadcrumbs_to_html(&crumbs, &[]);
            secondary_nav_to_html(
                SecondaryNavContent::Breadcrumbs(&breadcrumbs),
                &toggle_label,
            )
        } else {
            // Q1 collapses the page title into the bar instead, and
            // hides the document `h1.title` below `lg` so the two do
            // not duplicate. The template consumes the flag set below.
            let Some(title) = ast.meta.get("title") else {
                return Ok(());
            };
            let html =
                secondary_nav_to_html(SecondaryNavContent::CollapsedTitle(title), &toggle_label);
            ast.meta.insert_path(
                &["rendered", "navigation", "secondary-nav-collapsed-title"],
                ConfigValue::new_bool(true, SourceInfo::generated(By::programmatic_config())),
            );
            html
        };

        ast.meta.insert_path(
            &["rendered", "navigation", "secondary-nav"],
            ConfigValue::new_string(&html, SourceInfo::generated(By::programmatic_config())),
        );

        Ok(())
    }
}
