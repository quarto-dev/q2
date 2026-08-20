/*
 * panel_tabset_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Transform that resolves Tabset CustomNodes to Bootstrap tab markup.
 */

//! Panel-tabset resolution transform.
//!
//! Converts Tabset CustomNodes (built by [`super::PanelTabsetTransform`])
//! into the Bootstrap nav-tabs structure. Runs immediately after the
//! parse half, mirroring the callout pair's ordering.
//!
//! ## Output Structure
//!
//! The markup contract is Q1's committed render of the strand's repro,
//! captured at
//! `claude-notes/plans/tabset-panel-tabset-investigation/q1-target-markup.html`:
//!
//! ```text
//! Div (original attr: .panel-tabset [+ group= → data-group=])
//!   Plain
//!     RawInline(html, "<ul class=\"nav nav-tabs\" role=\"tablist\">")
//!     per tab (N = tabset counter, M = 1-based tab index):
//!       RawInline(html, "<li class=\"nav-item\" role=\"presentation\">")
//!       RawInline(html, "<a class=\"nav-link[ active]\" id=\"tabset-N-M-tab\"
//!         data-bs-toggle=\"tab\" data-bs-target=\"#tabset-N-M\" role=\"tab\"
//!         aria-controls=\"tabset-N-M\" aria-selected=\"true|false\" href=\"\">")
//!       [title inlines…]
//!       RawInline(html, "</a></li>")
//!     RawInline(html, "</ul>")
//!   Div ("", ["tab-content"], {})
//!     per tab:
//!       Div ("tabset-N-M", ["tab-pane"[, "active"]],
//!            {role: tabpanel, aria-labelledby: tabset-N-M-tab})
//!         [content blocks…]
//! ```
//!
//! `bootstrap.bundle.min.js` (shipped by `BootstrapJsStage` on every
//! non-minimal HTML page) picks up `data-bs-toggle="tab"` for switching;
//! the grouped-sync module (`TabsetsJsStage`) syncs same-`data-group`
//! tabsets and persists the choice in localStorage.
//!
//! The tabset counter is document-scoped and starts at 1, matching Q1's
//! `tabsetidx` — re-renders of the same document produce identical ids.

use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Div, Plain};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::{Inline, RawInline};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Transform that resolves Tabset CustomNodes to Bootstrap tab markup.
pub struct PanelTabsetResolveTransform;

impl PanelTabsetResolveTransform {
    /// Create a new panel-tabset resolve transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PanelTabsetResolveTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for PanelTabsetResolveTransform {
    fn name(&self) -> &str {
        "panel-tabset-resolve"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        // Document-scoped tabset counter, 1-based like Q1's `tabsetidx`.
        // No format gate needed: Tabset CustomNodes only exist when the
        // (gated) parse half ran.
        let mut counter = 1u32;
        resolve_blocks(&mut ast.blocks, &mut counter);
        Ok(())
    }
}

/// Resolve Tabset CustomNodes in a vector of blocks.
fn resolve_blocks(blocks: &mut Vec<Block>, counter: &mut u32) {
    for block in blocks.iter_mut() {
        resolve_block(block, counter);
    }
}

/// Resolve a single block, potentially converting CustomNode to Div.
///
/// The walk is **pre-order for the replacement**: a tabset's own id is
/// assigned before its nested tabsets', so document order matches Q1's
/// numbering.
fn resolve_block(block: &mut Block, counter: &mut u32) {
    match block {
        Block::BlockQuote(bq) => {
            resolve_blocks(&mut bq.content, counter);
        }
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                resolve_blocks(item, counter);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                resolve_blocks(item, counter);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    resolve_blocks(def, counter);
                }
            }
        }
        Block::Figure(fig) => {
            resolve_blocks(&mut fig.content, counter);
        }
        Block::Div(div) => {
            resolve_blocks(&mut div.content, counter);
        }
        Block::Table(table) => {
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        resolve_blocks(&mut cell.content, counter);
                    }
                }
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    resolve_blocks(&mut cell.content, counter);
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    resolve_blocks(&mut cell.content, counter);
                }
            }
        }
        Block::Custom(custom) => {
            if custom.type_name == "Tabset" {
                let mut resolved_div = resolve_tabset(custom, counter);
                // Nested tabsets inside the panes resolve after the
                // parent claimed its id (document-order numbering).
                resolve_blocks(&mut resolved_div.content, counter);
                *block = Block::Div(resolved_div);
            } else {
                for (_name, slot) in &mut custom.slots {
                    match slot {
                        Slot::Block(b) => resolve_block(b, counter),
                        Slot::Blocks(bs) => resolve_blocks(bs, counter),
                        _ => {}
                    }
                }
            }
        }
        // Other block types don't contain nested blocks
        _ => {}
    }
}

/// Generated-source marker for tabset scaffolding.
fn tabset_source() -> SourceInfo {
    SourceInfo::generated(By::raw("tabset", serde_json::Value::Null))
}

fn raw_html(text: String) -> Inline {
    Inline::RawInline(RawInline {
        format: "html".to_string(),
        text,
        source_info: tabset_source(),
    })
}

/// Resolve one Tabset CustomNode to the Bootstrap Div structure,
/// consuming one value from the document tabset `counter`.
fn resolve_tabset(custom: &mut CustomNode, counter: &mut u32) -> Div {
    let tabset_index = *counter;
    *counter += 1;
    let tabsetid = format!("tabset-{tabset_index}");

    let tab_count = custom
        .plain_data
        .get("tab_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let actives: Vec<bool> = custom
        .plain_data
        .get("actives")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(|v| v.as_bool().unwrap_or(false)).collect())
        .unwrap_or_default();
    let is_active = |i: usize| actives.get(i).copied().unwrap_or(i == 0);

    // ── Navigation <ul>, built from RawInlines with the title inlines
    // spliced in (Q1 `render_tabset`, verbatim attribute shape). ──
    let mut nav: Vec<Inline> = Vec::new();
    nav.push(raw_html(
        "<ul class=\"nav nav-tabs\" role=\"tablist\">".to_string(),
    ));
    for i in 0..tab_count {
        let tabid = format!("{}-{}", tabsetid, i + 1);
        let active_class = if is_active(i) { " active" } else { "" };
        let selected = if is_active(i) { "true" } else { "false" };
        nav.push(raw_html(
            "<li class=\"nav-item\" role=\"presentation\">".to_string(),
        ));
        nav.push(raw_html(format!(
            "<a class=\"nav-link{active_class}\" id=\"{tabid}-tab\" \
             data-bs-toggle=\"tab\" data-bs-target=\"#{tabid}\" role=\"tab\" \
             aria-controls=\"{tabid}\" aria-selected=\"{selected}\" href=\"\">"
        )));
        if let Some(Slot::Inlines(title)) = custom.slots.remove(&format!("title-{i}")) {
            nav.extend(title);
        }
        nav.push(raw_html("</a></li>".to_string()));
    }
    nav.push(raw_html("</ul>".to_string()));

    // ── Panes: <div class="tab-content"> of tab-pane Divs. ──
    let mut panes: Vec<Block> = Vec::new();
    for i in 0..tab_count {
        let tabid = format!("{}-{}", tabsetid, i + 1);
        let mut classes = vec!["tab-pane".to_string()];
        if is_active(i) {
            classes.push("active".to_string());
        }
        let mut attrs = hashlink::LinkedHashMap::new();
        attrs.insert("role".to_string(), "tabpanel".to_string());
        attrs.insert("aria-labelledby".to_string(), format!("{tabid}-tab"));
        let pane_attr: Attr = (tabid, classes, attrs);
        let content = match custom.slots.remove(&format!("content-{i}")) {
            Some(Slot::Blocks(blocks)) => blocks,
            _ => Vec::new(),
        };
        panes.push(Block::Div(Div {
            attr: pane_attr,
            content,
            source_info: tabset_source(),
            attr_source: AttrSourceInfo::empty(),
        }));
    }
    let tab_content = Block::Div(Div {
        attr: (
            String::new(),
            vec!["tab-content".to_string()],
            hashlink::LinkedHashMap::new(),
        ),
        content: panes,
        source_info: tabset_source(),
        attr_source: AttrSourceInfo::empty(),
    });

    // ── Outer div: the original attr (panel-tabset class and any
    // group= attribute, which the HTML writer emits as data-group=). ──
    Div {
        attr: custom.attr.clone(),
        content: vec![
            Block::Plain(Plain {
                content: nav,
                source_info: tabset_source(),
            }),
            tab_content,
        ],
        source_info: custom.source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::Str;
    use serde_json::json;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn str_inline(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: si(),
        })
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![str_inline(text)],
            source_info: si(),
        })
    }

    /// Build a Tabset CustomNode the way `PanelTabsetTransform` does.
    fn make_tabset(titles: &[&str], active_index: usize) -> CustomNode {
        let mut custom = CustomNode::new(
            "Tabset",
            (
                String::new(),
                vec!["panel-tabset".to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            si(),
        );
        custom.plain_data = json!({
            "level": 2,
            "tab_count": titles.len(),
            "actives": (0..titles.len()).map(|i| i == active_index).collect::<Vec<_>>(),
        });
        for (i, title) in titles.iter().enumerate() {
            custom.set_slot(format!("title-{i}"), Slot::Inlines(vec![str_inline(title)]));
            custom.set_slot(
                format!("content-{i}"),
                Slot::Blocks(vec![para(&format!("{title} content"))]),
            );
        }
        custom
    }

    fn raw_text(inlines: &[Inline]) -> String {
        inlines
            .iter()
            .map(|i| match i {
                Inline::RawInline(r) => r.text.clone(),
                Inline::Str(s) => s.text.clone(),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn resolves_to_nav_and_panes() {
        let mut blocks = vec![Block::Custom(make_tabset(&["Alpha", "Beta"], 0))];
        let mut counter = 1u32;
        resolve_blocks(&mut blocks, &mut counter);

        let Block::Div(outer) = &blocks[0] else {
            panic!("expected resolved Div");
        };
        assert!(outer.attr.1.contains(&"panel-tabset".to_string()));
        assert_eq!(outer.content.len(), 2, "nav Plain + tab-content Div");

        let Block::Plain(nav) = &outer.content[0] else {
            panic!("expected nav Plain");
        };
        let nav_html = raw_text(&nav.content);
        assert!(nav_html.starts_with("<ul class=\"nav nav-tabs\" role=\"tablist\">"));
        assert!(nav_html.contains("id=\"tabset-1-1-tab\""));
        assert!(nav_html.contains("data-bs-target=\"#tabset-1-2\""));
        assert!(nav_html.contains("aria-selected=\"true\""));
        assert!(nav_html.contains("Alpha"));
        assert!(nav_html.ends_with("</ul>"));

        let Block::Div(tab_content) = &outer.content[1] else {
            panic!("expected tab-content Div");
        };
        assert!(tab_content.attr.1.contains(&"tab-content".to_string()));
        assert_eq!(tab_content.content.len(), 2);
        let Block::Div(pane1) = &tab_content.content[0] else {
            panic!("expected first pane Div");
        };
        assert_eq!(pane1.attr.0, "tabset-1-1");
        assert!(pane1.attr.1.contains(&"active".to_string()));
        assert_eq!(
            pane1.attr.2.get("role").map(String::as_str),
            Some("tabpanel")
        );
        assert_eq!(
            pane1.attr.2.get("aria-labelledby").map(String::as_str),
            Some("tabset-1-1-tab")
        );
        let Block::Div(pane2) = &tab_content.content[1] else {
            panic!("expected second pane Div");
        };
        assert!(!pane2.attr.1.contains(&"active".to_string()));
    }

    #[test]
    fn active_index_places_active_on_selected_tab_only() {
        let mut blocks = vec![Block::Custom(make_tabset(&["A", "B"], 1))];
        let mut counter = 1u32;
        resolve_blocks(&mut blocks, &mut counter);

        let Block::Div(outer) = &blocks[0] else {
            panic!("expected Div");
        };
        let Block::Plain(nav) = &outer.content[0] else {
            panic!("expected nav");
        };
        let nav_html = raw_text(&nav.content);
        // Exactly one selected link, and it's the second.
        assert_eq!(nav_html.matches("aria-selected=\"true\"").count(), 1);
        assert_eq!(nav_html.matches("nav-link active").count(), 1);
        let selected_pos = nav_html.find("aria-selected=\"true\"").unwrap();
        let second_tab_pos = nav_html.find("id=\"tabset-1-2-tab\"").unwrap();
        assert!(selected_pos > second_tab_pos);
    }

    #[test]
    fn counter_increments_in_document_order() {
        let mut blocks = vec![
            Block::Custom(make_tabset(&["A"], 0)),
            Block::Custom(make_tabset(&["B"], 0)),
        ];
        let mut counter = 1u32;
        resolve_blocks(&mut blocks, &mut counter);

        let ids: Vec<String> = blocks
            .iter()
            .map(|b| {
                let Block::Div(outer) = b else { panic!("Div") };
                let Block::Div(tc) = &outer.content[1] else {
                    panic!("tab-content")
                };
                let Block::Div(pane) = &tc.content[0] else {
                    panic!("pane")
                };
                pane.attr.0.clone()
            })
            .collect();
        assert_eq!(ids, vec!["tabset-1-1", "tabset-2-1"]);
        assert_eq!(counter, 3);
    }

    #[test]
    fn nested_tabset_resolves_with_later_id() {
        let mut inner = make_tabset(&["Inner"], 0);
        inner.plain_data["level"] = json!(3);
        let mut outer = make_tabset(&["Outer"], 0);
        outer.set_slot("content-0", Slot::Blocks(vec![Block::Custom(inner)]));

        let mut blocks = vec![Block::Custom(outer)];
        let mut counter = 1u32;
        resolve_blocks(&mut blocks, &mut counter);

        let Block::Div(outer_div) = &blocks[0] else {
            panic!("outer Div");
        };
        let Block::Div(tc) = &outer_div.content[1] else {
            panic!("tab-content");
        };
        let Block::Div(pane) = &tc.content[0] else {
            panic!("pane");
        };
        assert_eq!(pane.attr.0, "tabset-1-1");
        // The nested tabset resolved inside the pane with the next id.
        let Block::Div(inner_div) = &pane.content[0] else {
            panic!("inner resolved Div, got {:?}", pane.content[0]);
        };
        let Block::Div(inner_tc) = &inner_div.content[1] else {
            panic!("inner tab-content");
        };
        let Block::Div(inner_pane) = &inner_tc.content[0] else {
            panic!("inner pane");
        };
        assert_eq!(inner_pane.attr.0, "tabset-2-1");
    }
}
