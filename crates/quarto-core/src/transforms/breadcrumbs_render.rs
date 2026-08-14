/*
 * breadcrumbs_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for website breadcrumbs
//! (bd-breadcrumbs-missing-1vpuqh34).
//!
//! Reads the resolved `navigation.sidebar` (populated by
//! [`SidebarGenerateTransform`](super::SidebarGenerateTransform)),
//! derives the current page's breadcrumb trail from it, and writes the
//! rendered markup to `rendered.navigation.breadcrumbs` for the
//! title-block partial to pick up.
//!
//! Q1 parity notes (`website-shared.ts::breadCrumbs` +
//! `website-navigation.ts`):
//!
//! - The trail includes the current page as its own final, linked
//!   crumb.
//! - A section crumb without an href borrows its first direct child's
//!   href; when that's absent too the crumb renders unlinked.
//! - The title-block instance renders only when the trail has more
//!   than one crumb.
//! - `website.bread-crumbs` defaults to true; a page-level
//!   `bread-crumbs: false` disables per page
//!   ([`resolve_website_bool`] precedence).
//!
//! Only the title-block instance (`quarto-title-breadcrumbs d-none
//! d-lg-block`) exists in q2 today. Q1's second instance lives inside
//! `.quarto-secondary-nav`, the narrow-viewport bar that also owns the
//! mobile sidebar toggle — a subsystem q2 doesn't have yet; it is
//! tracked separately (discovered-from bd-breadcrumbs-missing-1vpuqh34).
//!
//! ## Skip conditions
//!
//! - `sidebar` feature disabled, or `navigation.sidebar` absent.
//! - `bread-crumbs: false` (site- or page-level).
//! - Trail length ≤ 1 (page at top level, or page not in the sidebar).
//! - `rendered.navigation.breadcrumbs` already populated (user
//!   override).

use quarto_navigation::{Sidebar, breadcrumb_trail, render_html::breadcrumbs_to_html};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::navigation_active::page_relative_source;
use crate::transforms::navigation_href::{NavSurface, resolve_href_for_html};
use crate::transforms::{is_feature_disabled, resolve_website_bool};

pub struct BreadcrumbsRenderTransform;

impl BreadcrumbsRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BreadcrumbsRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for BreadcrumbsRenderTransform {
    fn name(&self) -> &str {
        "breadcrumbs-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "sidebar") {
            return Ok(());
        }
        if !resolve_website_bool(&ast.meta, "bread-crumbs", true) {
            return Ok(());
        }
        if ast
            .meta
            .contains_path(&["rendered", "navigation", "breadcrumbs"])
        {
            return Ok(());
        }
        let Some(sidebar_cv) = ast.meta.get_path(&["navigation", "sidebar"]) else {
            return Ok(());
        };

        let sidebar = Sidebar::from_config_value(sidebar_cv);
        let page_source = page_relative_source(ctx);
        let mut crumbs = breadcrumb_trail(&sidebar, &page_source);
        // Q1 renders the title-block instance only for trails longer
        // than one crumb; an empty trail means the page isn't in the
        // sidebar at all.
        if crumbs.len() <= 1 {
            return Ok(());
        }

        // Crumb hrefs are sidebar hrefs — already resolved and
        // diagnosed once by sidebar rendering. Resolve through the
        // same helper (source-path → output href, page-relative,
        // index-miss statics relativized) but discard the duplicate
        // diagnostics.
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

        let html = breadcrumbs_to_html(
            &crumbs,
            &["quarto-title-breadcrumbs", "d-none", "d-lg-block"],
        );
        ast.meta.insert_path(
            &["rendered", "navigation", "breadcrumbs"],
            ConfigValue::new_string(&html, SourceInfo::generated(By::programmatic_config())),
        );

        Ok(())
    }
}
