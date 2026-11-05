/*
 * dashboard/view.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Preview view nodes (plan Phase 8).
//!
//! The q2 preview runs the same lowering as the render ([`super::resolve`]),
//! so the bslib markup has one source of truth. [`to_views`] then rewrites
//! the few elements whose behavior Q1 left to bslib's and Bootstrap's
//! JavaScript into `CustomNode`s the React preview renders with that
//! behavior in React state:
//!
//! | node                       | from the lowered markup                      | React adds          |
//! |----------------------------|----------------------------------------------|---------------------|
//! | [`PAGE_VIEW`]              | `div.dashboard-page.tab-pane`                | the active page     |
//! | [`SIDEBAR_LAYOUT`]         | `div.bslib-sidebar-layout` + its `<aside>`   | collapse            |
//! | [`CARD_VIEW`]              | `div.bslib-card`                             | full screen         |
//! | [`TAB_NAV`] / [`TAB_PANE`] | a tabset card\'s nav list and `div.tab-pane`  | the active tab      |
//!
//! Each node keeps the element\'s attributes, exactly as the render writes
//! them, in its `attr`, and its lowered children in slots, so the React
//! components only add behavior. Scripts and the bslib expand / collapse
//! buttons are dropped (React renders its own), and the split raw tags the
//! render uses (`<aside>…</aside>`, `<p class>…</p>`) become structure:
//! the aside is part of [`SIDEBAR_LAYOUT`], and a value box\'s title or
//! value paragraph is a [`PARAGRAPH`] node (a `<p>` with a class, which a
//! Pandoc `Para` cannot carry).
//!
//! These nodes exist only in the preview, after every Rust transform; they
//! never reach a Pandoc writer, so (like `ExampleEmbed`) they are not in
//! `custom-node-schema.json`.

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Blocks, Div};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_source_map::SourceInfo;
use serde_json::json;

pub const PAGE_VIEW: &str = "DashboardPageView";
pub const SIDEBAR_LAYOUT: &str = "DashboardSidebarLayout";
pub const CARD_VIEW: &str = "DashboardCardView";
pub const TAB_NAV: &str = "DashboardTabNav";
pub const TAB_PANE: &str = "DashboardTabPane";
pub const PARAGRAPH: &str = "DashboardParagraph";

fn has_class(attr: &Attr, class: &str) -> bool {
    attr.1.iter().any(|c| c == class)
}

fn raw_text(block: &Block) -> Option<&str> {
    match block {
        Block::RawBlock(r) if r.format == "html" => Some(r.text.as_str()),
        _ => None,
    }
}

fn node(type_name: &str, attr: Attr, source_info: SourceInfo) -> CustomNode {
    CustomNode::new(type_name, attr, source_info)
}

/// A tabset card\'s nav list (in place of the render\'s raw `<ul>`): the
/// tabset id and each tab\'s pane id and title, in order.
pub fn tab_nav(tabset_id: &str, titles: &[String]) -> Block {
    let tabs: Vec<_> = titles
        .iter()
        .enumerate()
        .map(|(i, t)| json!({ "id": format!("{tabset_id}-{}", i + 1), "title": t }))
        .collect();
    Block::Custom(
        node(
            TAB_NAV,
            (String::new(), Vec::new(), LinkedHashMap::new()),
            SourceInfo::generated(quarto_source_map::By::unknown()),
        )
        .with_data(json!({ "tabset_id": tabset_id, "tabs": tabs })),
    )
}

/// Rewrite the lowered document for the React preview (see the module
/// docs). Children are rewritten before their parents.
pub fn to_views(blocks: Blocks) -> Blocks {
    let blocks = join_value_box_paragraphs(blocks);
    blocks.into_iter().map(to_view).collect()
}

fn to_view(block: Block) -> Block {
    let Block::Div(mut d) = block else {
        return match block {
            Block::Figure(mut f) => {
                f.content = to_views(f.content);
                Block::Figure(f)
            }
            Block::BlockQuote(mut q) => {
                q.content = to_views(q.content);
                Block::BlockQuote(q)
            }
            other => other,
        };
    };
    if has_class(&d.attr, "bslib-sidebar-layout")
        && let Some(layout) = sidebar_layout(&mut d)
    {
        return layout;
    }
    d.content = to_views(d.content);
    let Div {
        attr,
        content,
        source_info,
        ..
    } = d;
    if has_class(&attr, "dashboard-page") {
        let index = attr.0.clone();
        return Block::Custom(
            node(PAGE_VIEW, attr, source_info)
                .with_data(json!({ "id": index }))
                .with_slot("content", Slot::Blocks(content)),
        );
    }
    if has_class(&attr, "tab-pane") && attr.2.get("role").is_some_and(|r| r == "tabpanel") {
        let id = attr.0.clone();
        return Block::Custom(
            node(TAB_PANE, attr, source_info)
                .with_data(json!({ "id": id }))
                .with_slot("content", Slot::Blocks(content)),
        );
    }
    if has_class(&attr, "bslib-card") {
        let expandable = attr.2.contains_key("data-full-screen");
        let content: Blocks = content
            .into_iter()
            .filter(|b| !raw_text(b).is_some_and(is_card_chrome))
            .collect();
        return Block::Custom(
            node(CARD_VIEW, attr, source_info)
                .with_data(json!({ "expandable": expandable }))
                .with_slot("content", Slot::Blocks(content)),
        );
    }
    Block::Div(Div {
        attr,
        content,
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// The card\'s bslib expand button and init script (React renders its own
/// expand control).
fn is_card_chrome(text: &str) -> bool {
    text.starts_with("<bslib-tooltip") || text.starts_with("<script data-bslib-card-init")
}

/// `div.bslib-sidebar-layout` holds `[main, <aside …>, sidebar, </aside>,
/// toggle + script]` (`resolve::sidebar_layout`). `None` (and `d` left as
/// it was) for any other shape.
fn sidebar_layout(d: &mut Div) -> Option<Block> {
    let [
        Block::Div(_),
        open @ Block::RawBlock(_),
        Block::Div(_),
        close @ Block::RawBlock(_),
        toggle @ Block::RawBlock(_),
    ] = d.content.as_slice()
    else {
        return None;
    };
    let open = raw_text(open)?;
    if raw_text(close)? != "</aside>" || !raw_text(toggle)?.contains("collapse-toggle") {
        return None;
    }
    let sidebar_id = open
        .strip_prefix(r#"<aside id=""#)?
        .split('"')
        .next()?
        .to_string();
    let mut content = std::mem::take(&mut d.content).into_iter();
    let main = content.next()?;
    let sidebar = content.nth(1)?;
    let attr = std::mem::replace(
        &mut d.attr,
        (String::new(), Vec::new(), LinkedHashMap::new()),
    );
    Some(Block::Custom(
        node(SIDEBAR_LAYOUT, attr, d.source_info.clone())
            .with_data(json!({ "sidebar_id": sidebar_id }))
            .with_slot("main", Slot::Blocks(to_views(vec![main])))
            .with_slot("sidebar", Slot::Blocks(to_views(vec![sidebar]))),
    ))
}

/// `<p class="X">`, `Plain`, `</p>` (a value-box title or value,
/// `resolve::paragraph_for_value_part`) becomes a [`PARAGRAPH`] node with
/// class `X` around the `Plain`.
fn join_value_box_paragraphs(blocks: Blocks) -> Blocks {
    let mut out = Vec::with_capacity(blocks.len());
    let mut it = blocks.into_iter().peekable();
    while let Some(b) = it.next() {
        let class = raw_text(&b)
            .and_then(|t| t.strip_prefix(r#"<p class=""#))
            .and_then(|t| t.strip_suffix(r#"">"#))
            .map(str::to_string);
        if let Some(class) = class
            && matches!(it.peek(), Some(Block::Plain(_)))
        {
            let plain = it.next().expect("peeked");
            if it.peek().and_then(raw_text) == Some("</p>") {
                it.next();
                out.push(Block::Custom(
                    node(
                        PARAGRAPH,
                        (String::new(), vec![class], LinkedHashMap::new()),
                        SourceInfo::generated(quarto_source_map::By::unknown()),
                    )
                    .with_slot("content", Slot::Blocks(vec![plain])),
                ));
                continue;
            }
            out.push(b);
            out.push(plain);
            continue;
        }
        out.push(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::resolve;
    use crate::dashboard::structure::{DashboardOptions, build_cards, build_layout};

    fn lowered(body: &str, preview: bool) -> Blocks {
        let mut sink = Vec::new();
        let (ast, _, _) =
            pampa::readers::qmd::read(body.as_bytes(), false, "t.qmd", &mut sink, true, None)
                .expect("parses");
        let doc = DashboardOptions::default();
        let mut diags = Vec::new();
        let blocks = build_cards(ast.blocks, &doc, &mut diags);
        let (mut blocks, _) = build_layout(pampa::transforms::sectionize_blocks(blocks), &doc);
        crate::dashboard::inputs::place_card_inputs(&mut blocks, &mut diags);
        crate::dashboard::inputs::make_panels(&mut blocks);
        crate::dashboard::sizing::assign_grid_templates(&mut blocks);
        resolve::lower_document(blocks, preview).0
    }

    fn find<'a>(blocks: &'a [Block], type_name: &str, out: &mut Vec<&'a CustomNode>) {
        for b in blocks {
            match b {
                Block::Custom(n) => {
                    if n.type_name == type_name {
                        out.push(n);
                    }
                    for slot in n.slots.values() {
                        if let Slot::Blocks(inner) = slot {
                            find(inner, type_name, out);
                        }
                    }
                }
                Block::Div(d) => find(&d.content, type_name, out),
                _ => {}
            }
        }
    }

    fn all<'a>(blocks: &'a [Block], type_name: &str) -> Vec<&'a CustomNode> {
        let mut out = Vec::new();
        find(blocks, type_name, &mut out);
        out
    }

    fn has_raw(blocks: &[Block], needle: &str) -> bool {
        format!("{blocks:?}").contains(needle)
    }

    #[test]
    fn pages_cards_and_sidebars_become_view_nodes() {
        let body = "# One\n\n## {.sidebar}\n\nS\n\n## Column\n\n::: {.card title=\"A\"}\na\n:::\n\n# Two\n\n::: {.card title=\"B\"}\nb\n:::\n";
        let blocks = lowered(body, true);
        let pages = all(&blocks, PAGE_VIEW);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].attr.0, "one");
        assert!(has_class(&pages[0].attr, "tab-pane"));
        assert_eq!(all(&blocks, CARD_VIEW).len(), 2);
        let layouts = all(&blocks, SIDEBAR_LAYOUT);
        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].plain_data["sidebar_id"], "bslib-sidebar-1");
        assert!(has_class(&layouts[0].attr, "bslib-sidebar-layout"));
        // No scripts, bslib buttons or split tags survive.
        for needle in [
            "<script",
            "<aside",
            "</aside>",
            "bslib-tooltip",
            "collapse-toggle",
        ] {
            assert!(!has_raw(&blocks, needle), "{needle} left in the preview");
        }
        // The render keeps them.
        let render = lowered(body, false);
        assert!(has_raw(&render, "<aside") && has_raw(&render, "<script"));
        assert!(all(&render, CARD_VIEW).is_empty());
    }

    #[test]
    fn tabset_cards_get_a_nav_node_and_panes() {
        let body = "## Row {.tabset}\n\n::: {.card title=\"T1\"}\none\n:::\n\n::: {.card title=\"T2\"}\ntwo\n:::\n";
        let blocks = lowered(body, true);
        let navs = all(&blocks, TAB_NAV);
        assert_eq!(navs.len(), 1, "{blocks:#?}");
        let tabs = navs[0].plain_data["tabs"].as_array().unwrap();
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[1]["title"], "T2");
        let panes = all(&blocks, TAB_PANE);
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].attr.0, tabs[0]["id"]);
        assert!(!has_raw(&blocks, "nav-tabs"));
    }

    #[test]
    fn value_box_paragraphs_become_paragraph_nodes() {
        let blocks = lowered("## Row\n\n::: {.valuebox title=\"Hits\"}\n42\n:::\n", true);
        assert!(!has_raw(&blocks, "<p class="));
        let paragraphs = all(&blocks, PARAGRAPH);
        let classes: Vec<&str> = paragraphs.iter().map(|n| n.attr.1[0].as_str()).collect();
        assert_eq!(classes, ["value-box-title", "value-box-value"]);
    }
}
