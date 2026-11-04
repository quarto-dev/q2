/*
 * dashboard/mod.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! `format: dashboard` (bd-dashboard-format-k21hd4vy; plan
//! `claude-notes/plans/2026-11-03-dashboard-format-port.md`).
//!
//! Two transforms, run last and adjacent in the transform list:
//!
//! - [`DashboardTransform`] builds the typed structure — pages, row/column
//!   containers, cards — as `CustomNode`s ([`model`]) from the sectioned
//!   document ([`structure`]), computes grid templates ([`sizing`]), and
//!   records the page list for the navbar in `rendered.dashboard.pages`.
//! - [`DashboardResolveTransform`] lowers the nodes to bslib markup
//!   ([`resolve`]). The q2 preview excludes it so the React preview can
//!   render the nodes itself.

pub mod inputs;
pub mod model;
pub mod resolve;
pub mod sizing;
pub mod structure;
pub mod valuebox;

use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

use model::Orientation;
use structure::DashboardOptions;

fn generated() -> SourceInfo {
    SourceInfo::generated(quarto_source_map::By::unknown())
}

/// Read the document-level dashboard options from the merged metadata.
pub fn dashboard_options(meta: &ConfigValue) -> DashboardOptions {
    let mut o = DashboardOptions::default();
    if let Some(v) = meta.get("orientation").and_then(|v| v.as_plain_text())
        && v == "columns"
    {
        o.orientation = Orientation::Columns;
    }
    if let Some(b) = meta.get("scrolling").and_then(|v| v.as_bool()) {
        o.scrolling = b;
    }
    if let Some(b) = meta.get("expandable").and_then(|v| v.as_bool()) {
        o.expandable = b;
    }
    o
}

/// Builds the dashboard structure (see the module docs).
pub struct DashboardTransform;

impl DashboardTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DashboardTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for DashboardTransform {
    fn name(&self) -> &str {
        "dashboard"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if ctx.format.base_format() != "dashboard" {
            return Ok(());
        }
        let options = dashboard_options(&ast.meta);
        let mut diagnostics = Vec::new();
        let blocks =
            structure::build_cards(std::mem::take(&mut ast.blocks), &options, &mut diagnostics);
        ctx.diagnostics.extend(diagnostics);
        // Sections are made here, after the cards (Q1 pass 2 calls
        // `make_sections` itself); `SectionizeTransform` is skipped for
        // dashboards.
        let blocks = pampa::transforms::sectionize_blocks(blocks);
        let (mut blocks, info) = structure::build_layout(blocks, &options);
        let mut diagnostics = Vec::new();
        inputs::place_card_inputs(&mut blocks, &mut diagnostics);
        ctx.diagnostics.extend(diagnostics);
        inputs::make_panels(&mut blocks);
        sizing::assign_grid_templates(&mut blocks);
        ast.blocks = blocks;

        if !info.pages.is_empty() {
            let pages = info
                .pages
                .iter()
                .map(|p| {
                    let scrolling = match p.scrolling.as_deref() {
                        Some(s) => s == "true",
                        None => options.scrolling,
                    };
                    let entry = |k: &str, v: ConfigValue| ConfigMapEntry {
                        key: k.to_string(),
                        key_source: generated(),
                        value: v,
                    };
                    ConfigValue::new_map(
                        vec![
                            entry("id", ConfigValue::new_string(p.id.clone(), generated())),
                            entry(
                                "text",
                                ConfigValue::new_string(p.title.clone(), generated()),
                            ),
                            entry("active", ConfigValue::new_bool(p.index == 1, generated())),
                            entry(
                                "scrolling",
                                ConfigValue::new_string(scrolling.to_string(), generated()),
                            ),
                        ],
                        generated(),
                    )
                })
                .collect();
            ast.meta.insert_path(
                &["rendered", "dashboard", "pages"],
                ConfigValue::new_array(pages, generated()),
            );
        }
        Ok(())
    }
}

/// Lowers the dashboard nodes to bslib markup (see the module docs).
pub struct DashboardResolveTransform;

impl DashboardResolveTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DashboardResolveTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for DashboardResolveTransform {
    fn name(&self) -> &str {
        "dashboard-resolve"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if ctx.format.base_format() != "dashboard" {
            return Ok(());
        }
        let (blocks, top) = resolve::lower_document(std::mem::take(&mut ast.blocks));
        ast.blocks = blocks;
        for (key, classes) in [
            ("body-classes", top.body_classes),
            ("content-classes", top.content_classes),
        ] {
            if !classes.is_empty() {
                ast.meta.insert_path(
                    &["rendered", "dashboard", key],
                    ConfigValue::new_string(classes.join(" "), generated()),
                );
            }
        }
        Ok(())
    }
}
