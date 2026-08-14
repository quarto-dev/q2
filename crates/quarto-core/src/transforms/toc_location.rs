/*
 * toc_location.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Transform that normalizes `toc-location` and publishes placement
 * directives for the template and the sidebar renderer.
 */

//! `toc-location` normalization (bd-e2kpwy7n).
//!
//! Quarto 1 implements `toc-location` with a DOM postprocessor that
//! moves `nav#TOC` into a placeholder emitted by one of two template
//! paths (see
//! `claude-notes/plans/toc-location-investigation/q1-mechanism-notes.md`).
//! Q2 has no DOM postprocessor stage, so this transform decides the
//! placement up front and publishes it as metadata; the template and
//! [`SidebarRenderTransform`](super::SidebarRenderTransform) then emit
//! the right markup the first time.
//!
//! ## Outputs
//!
//! All under `rendered.navigation` unless noted; written only when a
//! rendered TOC exists (`rendered.navigation.toc`, produced by
//! [`TocRenderTransform`](super::TocRenderTransform) — this transform
//! must run after it and before `SidebarRenderTransform`):
//!
//! - **`toc-location`** — the normalized value (`"left"` | `"right"` |
//!   `"body"`). The single source of truth for downstream consumers
//!   (the preview follow-up bd-tqijrhsu reads this).
//! - **`toc-relocated`** (bool) — set when the location is not
//!   `right`; the template suppresses the right-margin TOC region on
//!   it (margin categories keep their margin shell).
//! - **`toc-left`** (bool) — standalone regime only: the template
//!   emits `div#quarto-sidebar-toc-left.sidebar.toc-left` and puts the
//!   `toc-left` grid class on `#quarto-content`
//!   (`.page-columns.toc-left` SCSS, gated on
//!   `body:not(.floating):not(.docked)`).
//! - **`toc-in-sidebar`** (bool) — website regime only: consumed by
//!   `SidebarRenderTransform`, which merges the TOC into
//!   `nav#quarto-sidebar` (appending after nav items, or synthesizing
//!   a floating sidebar when none is configured). Not read by the
//!   template.
//! - **`toc-body`** (bool) — the template emits the TOC inside
//!   `<main>`, between the title block and the body.
//! - **`quarto-template-params.banner-header-class`** = `"toc-left"`
//!   when the location is `left` and a banner title block is active
//!   (`rendered.title-block-banner`, written by the
//!   Normalization-phase
//!   [`TitleBannerTransform`](super::TitleBannerTransform)) — the
//!   previously-inert hook in
//!   [`TITLE_BLOCK_PARTIAL`](crate::template::TITLE_BLOCK_PARTIAL).
//!
//! ## Regimes
//!
//! Mirroring Q1's two layouts (design decision 2): pages of a
//! [`ProjectKind::Website`] project use the website regime (TOC inside
//! `nav#quarto-sidebar`, `body.floating` grid); everything else —
//! standalone documents, default projects, and for now book/manuscript
//! projects — uses the standalone regime (`toc-left` grid).
//!
//! ## Values
//!
//! `left`, `right` (default), `body` are implemented. `left-body` /
//! `right-body` warn (Q-13-8) and fall back to their sidebar half
//! until bd-jclcm0in lands; unknown values warn and default to
//! `right`. Warnings are only emitted when a TOC actually rendered —
//! without one the option is inert, and Q1 ignores it entirely.
//!
//! Plan: `claude-notes/plans/2026-08-14-toc-location.md`.

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::project::ProjectKind;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// The normalized TOC placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TocLocation {
    Left,
    Right,
    Body,
}

impl TocLocation {
    fn as_str(self) -> &'static str {
        match self {
            TocLocation::Left => "left",
            TocLocation::Right => "right",
            TocLocation::Body => "body",
        }
    }
}

pub struct TocLocationTransform;

impl TocLocationTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TocLocationTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for TocLocationTransform {
    fn name(&self) -> &str {
        "toc-location"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Without a rendered TOC the option is inert (Q1 parity: the
        // placeholder is only emitted when `hasTableOfContents`).
        if !ast.meta.contains_path(&["rendered", "navigation", "toc"]) {
            return Ok(());
        }

        // A pre-populated `rendered.navigation.toc-location` is a user
        // override (the `rendered.*` convention shared with toc/sidebar
        // HTML): normalize from it, and leave the key itself alone.
        let (raw, preset) = match ast
            .meta
            .get_path(&["rendered", "navigation", "toc-location"])
        {
            Some(v) => (Some((v.as_plain_text(), v.source_info.clone())), true),
            None => (
                ast.meta
                    .get("toc-location")
                    .map(|v| (v.as_plain_text(), v.source_info.clone())),
                false,
            ),
        };

        let location = match raw {
            None => TocLocation::Right,
            Some((text, source_info)) => {
                let text = text.unwrap_or_default();
                match text.as_str() {
                    "left" => TocLocation::Left,
                    "right" => TocLocation::Right,
                    "body" => TocLocation::Body,
                    "left-body" | "right-body" => {
                        let fallback = if text == "left-body" {
                            TocLocation::Left
                        } else {
                            TocLocation::Right
                        };
                        ctx.diagnostics.push(
                            DiagnosticMessageBuilder::warning(format!(
                                "`toc-location: {text}` is not supported yet"
                            ))
                            .with_code("Q-13-8")
                            .problem(format!(
                                "The `{text}` value (a sidebar TOC plus a copy in the body) \
                                 is not implemented yet. Falling back to `toc-location: {}`.",
                                fallback.as_str()
                            ))
                            .with_location(source_info)
                            .build(),
                        );
                        fallback
                    }
                    other => {
                        ctx.diagnostics.push(
                            DiagnosticMessageBuilder::warning(format!(
                                "Unknown `toc-location` value `{other}`"
                            ))
                            .with_code("Q-13-8")
                            .problem(
                                "Supported values are `left`, `right`, and `body`. \
                                 Using the default `toc-location: right`.",
                            )
                            .with_location(source_info)
                            .build(),
                        );
                        TocLocation::Right
                    }
                }
            }
        };

        if !preset {
            ast.meta.insert_path(
                &["rendered", "navigation", "toc-location"],
                ConfigValue::new_string(location.as_str(), gen_si()),
            );
        }

        let website = ctx.project.project_kind() == ProjectKind::Website;
        match location {
            TocLocation::Right => {}
            TocLocation::Left => {
                set_flag(ast, "toc-relocated");
                if website {
                    set_flag(ast, "toc-in-sidebar");
                } else {
                    set_flag(ast, "toc-left");
                }
                // Fire the banner header-class hook (Q1:
                // `format-html-title.ts:169-175`) — the banner header
                // sits outside `#quarto-content`, so it must carry the
                // grid class itself to line up with the shifted body
                // column.
                if ast.meta.contains_path(&["rendered", "title-block-banner"])
                    && !ast
                        .meta
                        .contains_path(&["quarto-template-params", "banner-header-class"])
                {
                    ast.meta.insert_path(
                        &["quarto-template-params", "banner-header-class"],
                        ConfigValue::new_string("toc-left", gen_si()),
                    );
                }
            }
            TocLocation::Body => {
                set_flag(ast, "toc-relocated");
                set_flag(ast, "toc-body");
            }
        }

        Ok(())
    }
}

fn set_flag(ast: &mut Pandoc, key: &str) {
    ast.meta.insert_path(
        &["rendered", "navigation", key],
        ConfigValue::new_bool(true, gen_si()),
    );
}

fn gen_si() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}
