/*
 * panel_tabset.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Transform that converts panel-tabset Divs to CustomNodes.
 */

//! Panel-tabset conversion transform.
//!
//! This transform finds Div blocks with the `panel-tabset` class and
//! converts them to CustomNode blocks with type "Tabset". Mirrors the
//! callout pair ([`super::CalloutTransform`] /
//! [`super::CalloutResolveTransform`]); the resolve half lives in
//! [`super::PanelTabsetResolveTransform`].
//!
//! ## Input Structure
//!
//! A tabset in the source document looks like:
//!
//! ```markdown
//! ::: {.panel-tabset}
//! ## Tab Alpha
//!
//! Alpha content.
//!
//! ## Tab Beta
//!
//! Beta content.
//! :::
//! ```
//!
//! The first Header inside the Div fixes the tab level; every Header of
//! that level starts a new tab (title = the header's inlines, content =
//! the blocks up to the next same-level header). Deeper headers stay
//! inside their tab's content. This reproduces Q1's
//! `parse_tabset_contents` (quarto-cli
//! `src/resources/filters/customnodes/panel-tabset.lua`).
//!
//! **Consuming the Headers here is the strand's TOC fix**
//! (bd-toc-tabset-titles-zq93gjvf): this transform runs before
//! `SectionizeTransform` and `TocGenerateTransform`, so tab titles never
//! reach the TOC — same mechanism as Q1, where the tabset filter eats
//! the headers before any TOC pass.
//!
//! ## Output Structure
//!
//! A CustomNode with:
//! - `type_name`: "Tabset"
//! - `slots`: `"title-<i>"` (Inlines) and `"content-<i>"` (Blocks) per
//!   tab, `i` zero-based
//! - `plain_data`: `{"level": N, "tab_count": N, "actives": [bool, …],
//!   "group": "…"?}` — `actives` has exactly one `true` (an explicit
//!   `.active` header class wins; otherwise the first tab)
//! - `attr`: the original Div attributes (class `panel-tabset` kept; a
//!   `group="…"` attribute rides along and renders as `data-group`)
//!
//! ## Scope (design decision 2, plan
//! `claude-notes/plans/2026-08-17-tabset-panel-tabset.md`)
//!
//! Bootstrap HTML only: the transform self-gates to HTML-based,
//! non-reveal formats that are not minimal HTML. Everywhere else the
//! Div passes through untouched (stacked headings, as today).

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::block::{Block, Div};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;
use serde_json::json;

use crate::Result;
use crate::format::{is_minimal_html, is_revealjs_target};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// The class that marks a Div as a tabset.
const TABSET_CLASS: &str = "panel-tabset";

/// Transform that converts panel-tabset Divs to CustomNodes.
pub struct PanelTabsetTransform;

impl PanelTabsetTransform {
    /// Create a new panel-tabset transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PanelTabsetTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for PanelTabsetTransform {
    fn name(&self) -> &str {
        "panel-tabset"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Bootstrap-HTML-only scope: reveal has its own (future) tabset
        // story (bd-y5j0m776) and minimal HTML ships no Bootstrap CSS/JS
        // for the nav-tabs markup to work with.
        if !ctx.format.identifier.is_html_based()
            || is_revealjs_target(&ctx.format.target_format)
            || is_minimal_html(&ast.meta)
        {
            return Ok(());
        }
        let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();
        transform_blocks(&mut ast.blocks, &mut diagnostics);
        ctx.diagnostics.extend(diagnostics);
        Ok(())
    }
}

/// Transform a vector of blocks, converting panel-tabset Divs to CustomNodes.
fn transform_blocks(blocks: &mut Vec<Block>, diagnostics: &mut Vec<DiagnosticMessage>) {
    for block in blocks.iter_mut() {
        transform_block(block, diagnostics);
    }
}

/// Transform a single block, potentially converting it to a CustomNode.
fn transform_block(block: &mut Block, diagnostics: &mut Vec<DiagnosticMessage>) {
    // First, recursively transform any nested blocks (a tabset inside a
    // tab pane converts before its parent buckets the pane's blocks).
    match block {
        Block::BlockQuote(bq) => {
            transform_blocks(&mut bq.content, diagnostics);
        }
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                transform_blocks(item, diagnostics);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                transform_blocks(item, diagnostics);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    transform_blocks(def, diagnostics);
                }
            }
        }
        Block::Figure(fig) => {
            transform_blocks(&mut fig.content, diagnostics);
        }
        Block::Div(div) => {
            transform_blocks(&mut div.content, diagnostics);

            if div.attr.1.iter().any(|c| c == TABSET_CLASS)
                && let Some(custom) = convert_div_to_tabset(div, diagnostics)
            {
                *block = Block::Custom(custom);
            }
        }
        Block::Table(table) => {
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        transform_blocks(&mut cell.content, diagnostics);
                    }
                }
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    transform_blocks(&mut cell.content, diagnostics);
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    transform_blocks(&mut cell.content, diagnostics);
                }
            }
        }
        Block::Custom(custom) => {
            for (_name, slot) in &mut custom.slots {
                match slot {
                    Slot::Block(b) => transform_block(b, diagnostics),
                    Slot::Blocks(bs) => transform_blocks(bs, diagnostics),
                    _ => {}
                }
            }
        }
        // Other block types don't contain nested blocks
        _ => {}
    }
}

/// One parsed tab, before it becomes CustomNode slots.
struct Tab {
    title: Vec<Inline>,
    active: bool,
    content: Vec<Block>,
}

/// Convert a panel-tabset Div to a CustomNode with type "Tabset".
///
/// Returns `None` (leaving the Div untouched) when the Div contains no
/// Header to define tabs — mirroring Q1, which warns and renders
/// nothing tab-like in that case.
fn convert_div_to_tabset(
    div: &mut Div,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<CustomNode> {
    // The first Header's level defines the tab boundaries (Q1
    // `parse_tabset_contents`).
    let level = div.content.iter().find_map(|b| match b {
        Block::Header(h) => Some(h.level),
        _ => None,
    });
    let Some(level) = level else {
        diagnostics.push(
            DiagnosticMessageBuilder::warning("No tabs found in tabset")
                .problem(
                    "A `panel-tabset` div contains no headings to define tabs. \
                     Each tab starts with a heading; the div is left unchanged.",
                )
                .with_location(div.source_info.clone())
                .build(),
        );
        return None;
    };

    // Bucket blocks into tabs at each level-N header. Content before
    // the first tab header has nowhere to go; Q1 silently drops it —
    // we drop it too, but say so.
    let mut tabs: Vec<Tab> = Vec::new();
    let mut dropped_leading_blocks = 0usize;
    for b in div.content.drain(..) {
        match b {
            Block::Header(h) if h.level == level => {
                tabs.push(Tab {
                    active: h.attr.1.iter().any(|c| c == "active"),
                    title: h.content,
                    content: Vec::new(),
                });
            }
            other => match tabs.last_mut() {
                Some(tab) => tab.content.push(other),
                None => dropped_leading_blocks += 1,
            },
        }
    }
    if dropped_leading_blocks > 0 {
        diagnostics.push(
            DiagnosticMessageBuilder::warning("Content before first tab heading is dropped")
                .problem(format!(
                    "{dropped_leading_blocks} block(s) appear before the first tab heading \
                     in this `panel-tabset` and are not part of any tab."
                ))
                .with_location(div.source_info.clone())
                .build(),
        );
    }

    // Exactly one active tab: an explicit `.active` header class wins
    // (first such tab, matching Q1's find_if); otherwise the first tab.
    let explicit_active = tabs.iter().position(|t| t.active);
    let active_index = explicit_active.unwrap_or(0);
    let actives: Vec<bool> = (0..tabs.len()).map(|i| i == active_index).collect();

    let group = div.attr.2.get("group").cloned();

    let mut plain_data = json!({
        "level": level,
        "tab_count": tabs.len(),
        "actives": actives,
    });
    if let Some(group) = &group
        && let Some(obj) = plain_data.as_object_mut()
    {
        obj.insert("group".into(), json!(group));
    }

    let mut custom = CustomNode::new("Tabset", div.attr.clone(), div.source_info.clone());
    custom.plain_data = plain_data;
    for (i, tab) in tabs.into_iter().enumerate() {
        custom.set_slot(format!("title-{i}"), Slot::Inlines(tab.title));
        custom.set_slot(format!("content-{i}"), Slot::Blocks(tab.content));
    }

    Some(custom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::{Header, Paragraph};
    use quarto_pandoc_types::inline::Str;
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/test.qmd")],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        }
    }

    fn header(level: usize, text: &str, classes: &[&str]) -> Block {
        Block::Header(Header {
            level,
            attr: (
                String::new(),
                classes.iter().map(|s| s.to_string()).collect(),
                hashlink::LinkedHashMap::new(),
            ),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: si(),
            })],
            source_info: si(),
        })
    }

    fn tabset_div(content: Vec<Block>) -> Block {
        Block::Div(Div {
            attr: (
                String::new(),
                vec![TABSET_CLASS.to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            content,
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    async fn run_transform(blocks: Vec<Block>, format: Format) -> (Pandoc, Vec<String>) {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks,
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        PanelTabsetTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let warnings = ctx.diagnostics.iter().map(|d| d.title.clone()).collect();
        (ast, warnings)
    }

    #[tokio::test]
    async fn two_tab_div_converts_to_custom_node() {
        let (ast, warnings) = run_transform(
            vec![tabset_div(vec![
                header(2, "Tab Alpha", &[]),
                para("Alpha content."),
                header(2, "Tab Beta", &[]),
                para("Beta content."),
            ])],
            Format::html(),
        )
        .await;

        assert!(warnings.is_empty(), "no warnings expected: {warnings:?}");
        let Block::Custom(custom) = &ast.blocks[0] else {
            panic!("expected Custom block, got {:?}", ast.blocks[0]);
        };
        assert_eq!(custom.type_name, "Tabset");
        assert_eq!(custom.plain_data["tab_count"], 2);
        assert_eq!(custom.plain_data["level"], 2);
        // First tab active by default.
        assert_eq!(custom.plain_data["actives"][0], true);
        assert_eq!(custom.plain_data["actives"][1], false);
        assert!(matches!(
            custom.get_slot("title-0"),
            Some(Slot::Inlines(t)) if matches!(&t[0], Inline::Str(s) if s.text == "Tab Alpha")
        ));
        assert!(matches!(
            custom.get_slot("content-1"),
            Some(Slot::Blocks(b)) if b.len() == 1
        ));
    }

    #[tokio::test]
    async fn explicit_active_header_wins() {
        let (ast, _) = run_transform(
            vec![tabset_div(vec![
                header(2, "A", &[]),
                header(2, "B", &["active"]),
            ])],
            Format::html(),
        )
        .await;

        let Block::Custom(custom) = &ast.blocks[0] else {
            panic!("expected Custom block");
        };
        assert_eq!(custom.plain_data["actives"][0], false);
        assert_eq!(custom.plain_data["actives"][1], true);
    }

    #[tokio::test]
    async fn deeper_headers_stay_inside_tab_content() {
        let (ast, _) = run_transform(
            vec![tabset_div(vec![
                header(2, "Tab", &[]),
                header(3, "Subsection", &[]),
                para("text"),
            ])],
            Format::html(),
        )
        .await;

        let Block::Custom(custom) = &ast.blocks[0] else {
            panic!("expected Custom block");
        };
        assert_eq!(custom.plain_data["tab_count"], 1);
        let Some(Slot::Blocks(content)) = custom.get_slot("content-0") else {
            panic!("content-0 slot");
        };
        assert_eq!(content.len(), 2, "### header + paragraph stay in the tab");
        assert!(matches!(&content[0], Block::Header(h) if h.level == 3));
    }

    #[tokio::test]
    async fn no_header_div_warns_and_passes_through() {
        let (ast, warnings) =
            run_transform(vec![tabset_div(vec![para("just text")])], Format::html()).await;

        assert!(
            matches!(&ast.blocks[0], Block::Div(d) if d.attr.1.contains(&TABSET_CLASS.to_string())),
            "div must pass through untouched"
        );
        assert!(
            warnings.iter().any(|w| w.contains("No tabs found")),
            "expected 'No tabs found in tabset' warning; got {warnings:?}"
        );
    }

    #[tokio::test]
    async fn leading_content_before_first_tab_warns() {
        let (ast, warnings) = run_transform(
            vec![tabset_div(vec![
                para("orphan"),
                header(2, "Tab", &[]),
                para("content"),
            ])],
            Format::html(),
        )
        .await;

        assert!(matches!(&ast.blocks[0], Block::Custom(_)));
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("before first tab heading")),
            "expected dropped-leading-content warning; got {warnings:?}"
        );
    }

    #[tokio::test]
    async fn group_attribute_lands_in_plain_data() {
        let mut attrs = hashlink::LinkedHashMap::new();
        attrs.insert("group".to_string(), "language".to_string());
        let div = Block::Div(Div {
            attr: (String::new(), vec![TABSET_CLASS.to_string()], attrs),
            content: vec![header(2, "Python", &[]), para("py")],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let (ast, _) = run_transform(vec![div], Format::html()).await;

        let Block::Custom(custom) = &ast.blocks[0] else {
            panic!("expected Custom block");
        };
        assert_eq!(custom.plain_data["group"], "language");
    }

    #[tokio::test]
    async fn nested_tabset_inside_tab_converts_too() {
        let inner = tabset_div(vec![header(3, "Inner", &[]), para("inner content")]);
        let (ast, _) = run_transform(
            vec![tabset_div(vec![header(2, "Outer", &[]), inner])],
            Format::html(),
        )
        .await;

        let Block::Custom(outer) = &ast.blocks[0] else {
            panic!("expected outer Custom block");
        };
        let Some(Slot::Blocks(content)) = outer.get_slot("content-0") else {
            panic!("content-0 slot");
        };
        assert!(
            matches!(&content[0], Block::Custom(c) if c.type_name == "Tabset"),
            "nested tabset must convert as well"
        );
    }

    #[tokio::test]
    async fn revealjs_format_is_passthrough() {
        let (ast, warnings) = run_transform(
            vec![tabset_div(vec![header(2, "Tab", &[]), para("content")])],
            Format::from_format_string("revealjs").expect("revealjs format"),
        )
        .await;

        assert!(
            matches!(&ast.blocks[0], Block::Div(_)),
            "reveal keeps the passthrough div (bd-y5j0m776 owns reveal tabsets)"
        );
        assert!(warnings.is_empty());
    }
}
