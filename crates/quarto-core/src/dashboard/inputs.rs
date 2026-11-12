/*
 * dashboard/inputs.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Card inputs and page panels: Quarto 1's dashboard pass 3
//! (`quarto-post/dashboard.lua`).
//!
//! - [`place_card_inputs`]: card toolbars and card sidebars (the unprocessed
//!   markers [`super::structure`] made) attach to the next card, the
//!   previous one, or the card holding a given id. A card's own id counts
//!   (D5-Q15; Q1 only looked inside the card). Anything that cannot be
//!   placed is a located `Q-27-2` warning and is dropped; Q1 either failed
//!   fatally or, for a "next" toolbar overwritten by a later one, dropped it
//!   silently.
//! - [`make_panels`]: a column container holding a `.sidebar` (row container
//!   holding a `.toolbar`) becomes a `DashboardPanel`; empty containers go.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::custom::Slot;
use quarto_pandoc_types::inline::{Inline, Space};
use quarto_source_map::SourceInfo;

use super::model::{
    CARD_TYPE, CONTAINER_TYPE, Card, CardSidebarData, Container, ContainerData, Orientation, Panel,
    PanelData, PanelKind,
};

fn has_class(attr: &Attr, class: &str) -> bool {
    attr.1.iter().any(|c| c == class)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Toolbar,
    Sidebar,
}

const TOOLBAR_UNPROCESSED: &str = "card-toolbar-unprocessed";
const SIDEBAR_UNPROCESSED: &str = "card-sidebar-unprocessed";

fn marker_kind(block: &Block) -> Option<Kind> {
    let Block::Div(d) = block else { return None };
    if has_class(&d.attr, TOOLBAR_UNPROCESSED) {
        Some(Kind::Toolbar)
    } else if has_class(&d.attr, SIDEBAR_UNPROCESSED) {
        Some(Kind::Sidebar)
    } else {
        None
    }
}

/// The target a marker names: `next`, `previous`, or an id.
fn marker_target(block: &Block, kind: Kind) -> String {
    let Block::Div(d) = block else {
        return "next".to_string();
    };
    let key = match kind {
        Kind::Toolbar => "target-element",
        Kind::Sidebar => "data-target",
    };
    d.attr
        .2
        .get(key)
        .cloned()
        .unwrap_or_else(|| "next".to_string())
}

fn is_id_target(target: &str) -> bool {
    target != "next" && target != "previous"
}

/// Visit every block list of the document in order (Divs and dashboard
/// node slots included), the same way in every pass that numbers cards.
fn for_each_list(blocks: &mut Blocks, f: &mut dyn FnMut(&mut Blocks)) {
    f(blocks);
    for b in blocks.iter_mut() {
        match b {
            Block::Div(d) => for_each_list(&mut d.content, f),
            Block::Custom(n) => {
                for (_, slot) in n.slots.iter_mut() {
                    if let Slot::Blocks(inner) = slot {
                        for_each_list(inner, f);
                    }
                }
            }
            _ => {}
        }
    }
}

/// An attachment to the card with index `card`.
struct Attachment {
    card: usize,
    kind: Kind,
    marker: Block,
}

fn unplaced(marker: &Block, why: &str) -> DiagnosticMessage {
    let what = match marker_kind(marker) {
        Some(Kind::Sidebar) => "card sidebar",
        _ => "card toolbar",
    };
    DiagnosticMessageBuilder::warning(format!("{what} could not be placed"))
        .with_code("Q-27-2")
        .with_location(marker.source_info().clone())
        .problem(format!(
            "This {what} {why}, so it was left out of the dashboard."
        ))
        .add_hint(
            "A card toolbar or sidebar attaches to the next card by default; use \
             `target-element=\"previous\"` (toolbars) or `target=\"previous\"` (sidebars) \
             for the card before it, or `header-for` / `footer-for` / `target` with the \
             id of a card or of something inside one.",
        )
        .build()
}

/// The ids a card carries: its source id and every id inside it (Q1
/// `idsWithinEl`, plus the card's own id, D5-Q15).
fn ids_of(card: &Card) -> Vec<String> {
    let mut ids = Vec::new();
    if !card.data.source_id.is_empty() {
        ids.push(card.data.source_id.clone());
    }
    let mut lists: Vec<Blocks> = vec![card.header.clone(), card.footer.clone()];
    lists.extend(card.bodies.iter().cloned());
    for mut list in lists {
        collect_ids(&mut list, &mut ids);
    }
    ids
}

fn collect_ids(blocks: &mut Blocks, ids: &mut Vec<String>) {
    for_each_list(blocks, &mut |list| {
        for b in list.iter() {
            let id = match b {
                Block::Div(d) => &d.attr.0,
                Block::Header(h) => &h.attr.0,
                Block::CodeBlock(c) => &c.attr.0,
                Block::Figure(f) => &f.attr.0,
                _ => continue,
            };
            if !id.is_empty() {
                ids.push(id.clone());
            }
        }
    });
    crate::ast_walk::for_each_inline_mut(blocks, &mut |i| {
        let id = match i {
            Inline::Span(s) => &s.attr.0,
            Inline::Image(img) => &img.attr.0,
            Inline::Link(l) => &l.attr.0,
            Inline::Code(c) => &c.attr.0,
            _ => return,
        };
        if !id.is_empty() {
            ids.push(id.clone());
        }
    });
}

fn card_of(block: &Block) -> Option<Card> {
    match block {
        Block::Custom(n) if n.type_name == CARD_TYPE => Card::from_custom(n),
        _ => None,
    }
}

/// Q1 pass 3's card-input placement (see the module docs).
pub fn place_card_inputs(blocks: &mut Blocks, diagnostics: &mut Vec<DiagnosticMessage>) {
    // Id-targeted markers leave the flow first (Q1 removes them in pass 1).
    let mut by_id: Vec<(String, Kind, Block)> = Vec::new();
    for_each_list(blocks, &mut |list| {
        let mut i = 0;
        while i < list.len() {
            if let Some(kind) = marker_kind(&list[i]) {
                let target = marker_target(&list[i], kind);
                if is_id_target(&target) {
                    by_id.push((target, kind, list.remove(i)));
                    continue;
                }
            }
            i += 1;
        }
    });

    // Plan, in document order.
    let mut plan: Vec<Attachment> = Vec::new();
    let mut cards = 0usize;
    let mut previous: Option<usize> = None;
    let mut pending: [Option<Block>; 2] = [None, None];
    let slot = |k: Kind| match k {
        Kind::Toolbar => 0,
        Kind::Sidebar => 1,
    };
    for_each_list(blocks, &mut |list| {
        for b in list.iter() {
            if let Some(card) = card_of(b) {
                let index = cards;
                cards += 1;
                for kind in [Kind::Toolbar, Kind::Sidebar] {
                    if let Some(marker) = pending[slot(kind)].take() {
                        plan.push(Attachment {
                            card: index,
                            kind,
                            marker,
                        });
                    }
                }
                for id in ids_of(&card) {
                    let mut j = 0;
                    while j < by_id.len() {
                        if by_id[j].0 == id {
                            let (_, kind, marker) = by_id.remove(j);
                            plan.push(Attachment {
                                card: index,
                                kind,
                                marker,
                            });
                        } else {
                            j += 1;
                        }
                    }
                }
                previous = Some(index);
            } else if let Some(kind) = marker_kind(b) {
                let target = marker_target(b, kind);
                if target == "previous" {
                    match previous {
                        Some(card) => plan.push(Attachment {
                            card,
                            kind,
                            marker: b.clone(),
                        }),
                        None => diagnostics.push(unplaced(
                            b,
                            "targets the previous card, but no card comes before it",
                        )),
                    }
                } else if let Some(dropped) = pending[slot(kind)].replace(b.clone()) {
                    diagnostics.push(unplaced(
                        &dropped,
                        "was waiting for the next card when another one took its place",
                    ));
                }
            }
        }
    });
    for marker in pending.into_iter().flatten() {
        diagnostics.push(unplaced(
            &marker,
            "targets the next card, but no card follows it",
        ));
    }
    for (id, _, marker) in &by_id {
        diagnostics.push(unplaced(
            marker,
            &format!("targets `{id}`, which no card contains"),
        ));
    }

    // Apply: drop every remaining marker, attach the planned ones.
    let mut cards = 0usize;
    for_each_list(blocks, &mut |list| {
        list.retain(|b| marker_kind(b).is_none());
        for b in list.iter_mut() {
            if let Some(mut card) = card_of(b) {
                let index = cards;
                cards += 1;
                let mine: Vec<&Attachment> = plan.iter().filter(|a| a.card == index).collect();
                if mine.is_empty() {
                    continue;
                }
                for a in mine {
                    attach(&mut card, a.kind, a.marker.clone());
                }
                let source = b.source_info().clone();
                *b = card.to_block(source);
            }
        }
    });
}

fn mark_processed(marker: Block) -> Block {
    match marker {
        Block::Div(mut d) => {
            d.attr
                .1
                .retain(|c| c != TOOLBAR_UNPROCESSED && c != SIDEBAR_UNPROCESSED);
            Block::Div(d)
        }
        other => other,
    }
}

/// Q1 `card_toolbar.addToTarget` (`card.addToHeader` / `addToFooter`) and
/// `card_sidebar.addToTarget`.
fn attach(card: &mut Card, kind: Kind, marker: Block) {
    let Block::Div(d) = &marker else { return };
    match kind {
        Kind::Toolbar => {
            let position = d.attr.2.get("target-position").cloned();
            let in_header = match position.as_deref() {
                Some(p) => p == "header",
                None => d.attr.2.get("target-element").map(String::as_str) != Some("previous"),
            };
            if in_header {
                if let Some(title) = d.attr.2.get("title") {
                    // Q1 puts the toolbar's title first in the header, ahead
                    // of the card's own title text.
                    let mut inlines = vec![Inline::Str(quarto_pandoc_types::inline::Str {
                        text: title.clone().into(),
                        source_info: d.source_info.clone(),
                    })];
                    if let Some(existing) = card.title.take() {
                        inlines.push(Inline::Space(Space {
                            source_info: SourceInfo::generated(quarto_source_map::By::unknown()),
                        }));
                        inlines.extend(existing);
                    }
                    card.title = Some(inlines);
                }
                card.header.push(mark_processed(marker));
            } else {
                card.footer.push(mark_processed(marker));
            }
        }
        Kind::Sidebar => {
            card.data.sidebar = Some(CardSidebarData {
                position: d
                    .attr
                    .2
                    .get("position")
                    .cloned()
                    .unwrap_or_else(|| "left".to_string()),
                width: d.attr.2.get("data-width").cloned(),
            });
            card.sidebar = vec![mark_processed(marker)];
        }
    }
}

// ---------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------

fn panel_kind(block: &Block) -> Option<PanelKind> {
    let Block::Div(d) = block else { return None };
    if has_class(&d.attr, "sidebar") {
        Some(PanelKind::Sidebar)
    } else if has_class(&d.attr, "toolbar") {
        Some(PanelKind::Toolbar)
    } else {
        None
    }
}

/// Q1 pass 3's `Div` handler: containers holding a sidebar or toolbar become
/// panels, and empty containers (other than the root) disappear. D5-Q16:
/// Q1 means to drop every empty container, but appends its AST vault (a
/// hidden Div) to the document's last section first, so the last empty
/// container survives there; q2 drops it too.
pub fn make_panels(blocks: &mut Blocks) {
    let mut i = 0;
    while i < blocks.len() {
        match &mut blocks[i] {
            Block::Div(d) => make_panels(&mut d.content),
            Block::Custom(n) if n.type_name == CONTAINER_TYPE => {
                if let Some(mut c) = Container::from_custom(n) {
                    make_panels(&mut c.children);
                    if c.children.is_empty() && !c.data.root {
                        blocks.remove(i);
                        continue;
                    }
                    blocks[i] = match panel_for(c) {
                        Ok(panel) => panel.to_block(),
                        Err(c) => c.to_block(),
                    };
                }
            }
            Block::Custom(n) => {
                for (_, slot) in n.slots.iter_mut() {
                    if let Slot::Blocks(inner) = slot {
                        make_panels(inner);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// A column container with a `.sidebar` child (row container with a
/// `.toolbar`) as a panel (Q1 `sidebar.makeSidebar` / `toolbar.makeToolbar`).
/// The last such child is the panel; the rest go into a fresh container of
/// the same orientation. The panel goes at the end when the container does
/// not start with it.
fn panel_for(c: Container) -> Result<Panel, Container> {
    let wanted = match c.data.orientation {
        Orientation::Columns => PanelKind::Sidebar,
        Orientation::Rows => PanelKind::Toolbar,
    };
    if !c.children.iter().any(|b| panel_kind(b) == Some(wanted)) {
        return Err(c);
    }
    let starts_with_content = c
        .children
        .first()
        .is_some_and(|b| panel_kind(b) != Some(wanted));
    let mut panel_div = None;
    let mut rest = Vec::new();
    for b in c.children {
        if panel_kind(&b) == Some(wanted) {
            panel_div = Some(b);
        } else {
            rest.push(b);
        }
    }
    let Some(Block::Div(panel)) = panel_div else {
        unreachable!("checked above")
    };
    let get = |k: &str| panel.attr.2.get(k).cloned();
    let at_end = starts_with_content
        || match wanted {
            PanelKind::Sidebar => get("position").as_deref() == Some("right"),
            PanelKind::Toolbar => get("position").as_deref() == Some("end"),
        };
    let inner = Container {
        data: ContainerData {
            orientation: c.data.orientation,
            id: String::new(),
            classes: Vec::new(),
            layout: None,
            height: None,
            width: None,
            root: false,
            grid_template: String::new(),
        },
        children: rest,
    };
    Ok(Panel {
        data: PanelData {
            kind: wanted,
            at_end,
            width: get("width").or_else(|| get("data-width")),
            height: get("height").or_else(|| get("data-height")),
        },
        panel: panel.content,
        content: vec![inner.to_block()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::model::{CARD_TYPE, Card, PANEL_TYPE, Panel};
    use crate::dashboard::structure::{DashboardOptions, build_cards, build_layout};

    /// The dashboard pass up to and including card inputs and panels.
    fn run(body: &str) -> (Blocks, Vec<DiagnosticMessage>) {
        let mut sink = Vec::new();
        let (ast, _, _) =
            pampa::readers::qmd::read(body.as_bytes(), false, "t.qmd", &mut sink, true, None)
                .expect("parses");
        let doc = DashboardOptions::default();
        let mut diags = Vec::new();
        let blocks = build_cards(ast.blocks, &doc, &mut diags);
        let (mut blocks, _) = build_layout(pampa::transforms::sectionize_blocks(blocks), &doc);
        place_card_inputs(&mut blocks, &mut diags);
        make_panels(&mut blocks);
        (blocks, diags)
    }

    fn all_cards(blocks: &Blocks) -> Vec<Card> {
        let mut out = Vec::new();
        let mut blocks = blocks.clone();
        for_each_list(&mut blocks, &mut |list| {
            for b in list {
                if let Block::Custom(n) = b
                    && n.type_name == CARD_TYPE
                {
                    out.push(Card::from_custom(n).unwrap());
                }
            }
        });
        out
    }

    fn all_panels(blocks: &Blocks) -> Vec<Panel> {
        let mut out = Vec::new();
        let mut blocks = blocks.clone();
        for_each_list(&mut blocks, &mut |list| {
            for b in list {
                if let Block::Custom(n) = b
                    && n.type_name == PANEL_TYPE
                {
                    out.push(Panel::from_custom(n).unwrap());
                }
            }
        });
        out
    }

    fn text(blocks: &Blocks) -> String {
        format!("{blocks:?}")
    }

    #[test]
    fn toolbar_attaches_to_next_header_and_previous_footer() {
        let (blocks, diags) = run(
            "## Row\n\n::: {.card-toolbar}\nNEXT\n:::\n\n::: {.card title=\"A\"}\na\n:::\n\n::: {.card title=\"B\"}\nb\n:::\n\n::: {.card-toolbar target-element=\"previous\"}\nPREV\n:::\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
        let cards = all_cards(&blocks);
        assert_eq!(cards.len(), 2);
        assert!(text(&cards[0].header).contains("NEXT"));
        // Q1: a `previous` toolbar with no position goes in the footer.
        assert!(text(&cards[1].footer).contains("PREV"));
    }

    #[test]
    fn footer_for_the_cards_own_id() {
        let (blocks, diags) = run(
            "## Row\n\n::: {#c .card title=\"A\"}\na\n:::\n\n::: {.card-toolbar footer-for=\"c\"}\nFOOT\n:::\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
        let cards = all_cards(&blocks);
        assert!(text(&cards[0].footer).contains("FOOT"));
        assert!(!text(&cards[0].header).contains("FOOT"));
    }

    #[test]
    fn card_sidebar_records_position_and_width() {
        let (blocks, _) = run(
            "## Row\n\n::: {.card title=\"A\"}\na\n:::\n\n::: {.card-sidebar target=\"previous\" width=\"150px\"}\nSIDE\n:::\n",
        );
        let card = &all_cards(&blocks)[0];
        let sb = card.data.sidebar.as_ref().expect("a card sidebar");
        assert_eq!(sb.width.as_deref(), Some("150px"));
        assert_eq!(sb.position, "right");
        assert!(text(&card.sidebar).contains("SIDE"));
    }

    #[test]
    fn unplaceable_markers_warn_and_are_dropped() {
        let (blocks, diags) = run(
            "## Row\n\n::: {.card-toolbar target-element=\"previous\"}\nLOST\n:::\n\n::: {.card title=\"A\"}\na\n:::\n",
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-27-2"));
        assert!(!text(&blocks).contains("LOST"));
    }

    #[test]
    fn sidebar_section_makes_a_panel_at_the_start_or_end() {
        let (blocks, _) =
            run("## {.sidebar width=\"200px\"}\n\nS\n\n## Column\n\n::: {.card}\na\n:::\n");
        let panels = all_panels(&blocks);
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].data.kind, PanelKind::Sidebar);
        assert!(!panels[0].data.at_end);
        assert_eq!(panels[0].data.width.as_deref(), Some("200px"));

        let (blocks, _) = run("## Column\n\n::: {.card}\na\n:::\n\n## {.sidebar}\n\nS\n");
        assert!(all_panels(&blocks)[0].data.at_end);
    }

    #[test]
    fn toolbar_section_makes_a_toolbar_panel() {
        let (blocks, _) = run("## {.toolbar}\n\nT\n\n## Row\n\n::: {.card}\na\n:::\n");
        let panels = all_panels(&blocks);
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].data.kind, PanelKind::Toolbar);
    }
}
