/*
 * dashboard/model.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The dashboard's typed model and its `CustomNode` encoding.
//!
//! [`super::DashboardTransform`] builds three node types and
//! [`super::DashboardResolveTransform`] lowers them to bslib markup:
//!
//! - **`DashboardPage`** — a level-1 heading's page. `plain_data`:
//!   [`PageData`]; slot `content` (one `DashboardContainer`).
//! - **`DashboardContainer`** — a row or column grid. `plain_data`:
//!   [`ContainerData`] (including the computed `grid_template`); slot
//!   `children`.
//! - **`DashboardCard`** — a card. `plain_data`: [`CardData`]; slots
//!   `title` (inlines, when titled), `header` (extra header blocks),
//!   `body-<i>` (one per body, see [`CardData::bodies`]) and `footer`.
//!
//! Card bodies use indexed slots, as the `Tabset` node does for its tabs
//! (dashboard plan D1). Typed values round-trip through
//! [`to_block`](Card::to_block) / [`from_custom`](Card::from_custom).

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::Inlines;
use quarto_source_map::SourceInfo;
use serde::{Deserialize, Serialize};

pub const PAGE_TYPE: &str = "DashboardPage";
pub const CONTAINER_TYPE: &str = "DashboardContainer";
pub const CARD_TYPE: &str = "DashboardCard";

/// How a container lays out its children. `Rows` stacks them vertically
/// (`grid-template-rows`), `Columns` side by side. Q1's Lua classes
/// (`.rows` / `.columns`) name the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Rows,
    Columns,
}

impl Orientation {
    pub fn flip(self) -> Self {
        match self {
            Orientation::Rows => Orientation::Columns,
            Orientation::Columns => Orientation::Rows,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rows" => Some(Orientation::Rows),
            "columns" => Some(Orientation::Columns),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Orientation::Rows => "rows",
            Orientation::Columns => "columns",
        }
    }
}

/// Q1's `fill` / `flow` layout values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Fill,
    Flow,
}

impl Layout {
    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Fill => "fill",
            Layout::Flow => "flow",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerData {
    pub orientation: Orientation,
    /// The id of an explicit `.rows` / `.columns` div (heading containers
    /// have none, as in Q1).
    #[serde(default)]
    pub id: String,
    /// User classes from the heading (`rows` / `columns` removed),
    /// including `fill` / `flow`, as Q1 keeps them.
    #[serde(default)]
    pub classes: Vec<String>,
    /// `fill` / `flow` from the heading's classes, or from `scrolling` for
    /// the root (Q1 `data-layout`).
    #[serde(default)]
    pub layout: Option<Layout>,
    /// Explicit `height=` / `width=` from the heading.
    #[serde(default)]
    pub height: Option<String>,
    #[serde(default)]
    pub width: Option<String>,
    /// The page-level root container (a direct child of the content area).
    #[serde(default)]
    pub root: bool,
    /// Computed grid template (`grid-template-rows` or `-columns`).
    #[serde(default)]
    pub grid_template: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageData {
    pub id: String,
    pub title: String,
    pub orientation: Orientation,
    /// The heading's `scrolling=` attribute, verbatim (Q1 `data-scrolling`).
    #[serde(default)]
    pub scrolling: Option<String>,
    /// 1-based position; the first page is the active one.
    pub index: usize,
}

/// A card body: the attributes of its Div plus Q1's body options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BodyData {
    #[serde(default)]
    pub id: String,
    /// Classes of the body Div, `card-body` included.
    #[serde(default)]
    pub classes: Vec<String>,
    /// Other attributes of the body Div.
    #[serde(default)]
    pub attributes: Vec<(String, String)>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub height: Option<String>,
    #[serde(default)]
    pub min_height: Option<String>,
    #[serde(default)]
    pub max_height: Option<String>,
    /// Q1 `data-fill` on the body.
    #[serde(default)]
    pub fill: Option<bool>,
    /// The body lays out at its natural height (Q1 `cardBodyLayout`):
    /// `fill=false`, or Q1's Shiny-input test, which also holds for a body
    /// with no element children (text only).
    #[serde(default)]
    pub flow: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CardData {
    /// Card classes (`card` first), deduplicated.
    #[serde(default)]
    pub classes: Vec<String>,
    #[serde(default)]
    pub padding: Option<String>,
    #[serde(default)]
    pub height: Option<String>,
    #[serde(default)]
    pub width: Option<String>,
    #[serde(default)]
    pub min_height: Option<String>,
    #[serde(default)]
    pub max_height: Option<String>,
    /// Explicit fill (`fill=` attribute, `.fill` / `.flow` classes, or a
    /// default the builder set, e.g. `false` for loose content).
    #[serde(default)]
    pub fill: Option<bool>,
    /// Whether the card gets the full-screen button (resolved default).
    #[serde(default)]
    pub expandable: bool,
    #[serde(default)]
    pub has_title: bool,
    #[serde(default)]
    pub bodies: Vec<BodyData>,
    #[serde(default)]
    pub has_footer: bool,
    #[serde(default)]
    pub has_header: bool,
}

/// A dashboard container (row or column grid).
#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub data: ContainerData,
    pub children: Blocks,
}

/// A dashboard page.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub data: PageData,
    pub content: Blocks,
}

/// A dashboard card.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub data: CardData,
    pub title: Option<Inlines>,
    /// Extra header content (Q1 `.card-header` children; card toolbars).
    pub header: Blocks,
    /// One block list per entry of `data.bodies`.
    pub bodies: Vec<Blocks>,
    pub footer: Blocks,
}

fn generated() -> SourceInfo {
    SourceInfo::generated(quarto_source_map::By::unknown())
}

fn empty_attr() -> Attr {
    (String::new(), Vec::new(), LinkedHashMap::new())
}

fn data_of<T: for<'de> Deserialize<'de>>(node: &CustomNode) -> Option<T> {
    serde_json::from_value(node.plain_data.clone()).ok()
}

fn blocks_slot(node: &CustomNode, name: &str) -> Blocks {
    match node.get_slot(name) {
        Some(Slot::Blocks(b)) => b.clone(),
        Some(Slot::Block(b)) => vec![(**b).clone()],
        _ => Vec::new(),
    }
}

fn custom(type_name: &str, data: impl Serialize, source_info: SourceInfo) -> CustomNode {
    CustomNode::new(type_name, empty_attr(), source_info)
        .with_data(serde_json::to_value(data).expect("dashboard data serializes"))
}

impl Container {
    pub fn to_block(self) -> Block {
        Block::Custom(
            custom(CONTAINER_TYPE, &self.data, generated())
                .with_slot("children", Slot::Blocks(self.children)),
        )
    }

    pub fn from_custom(node: &CustomNode) -> Option<Self> {
        (node.type_name == CONTAINER_TYPE).then_some(())?;
        Some(Container {
            data: data_of(node)?,
            children: blocks_slot(node, "children"),
        })
    }
}

impl Page {
    pub fn to_block(self) -> Block {
        Block::Custom(
            custom(PAGE_TYPE, &self.data, generated())
                .with_slot("content", Slot::Blocks(self.content)),
        )
    }

    pub fn from_custom(node: &CustomNode) -> Option<Self> {
        (node.type_name == PAGE_TYPE).then_some(())?;
        Some(Page {
            data: data_of(node)?,
            content: blocks_slot(node, "content"),
        })
    }
}

impl Card {
    pub fn to_block(self, source_info: SourceInfo) -> Block {
        let mut data = self.data;
        data.has_title = self.title.is_some();
        data.has_header = !self.header.is_empty();
        data.has_footer = !self.footer.is_empty();
        let mut node = custom(CARD_TYPE, &data, source_info);
        if let Some(title) = self.title {
            node.set_slot("title", Slot::Inlines(title));
        }
        if !self.header.is_empty() {
            node.set_slot("header", Slot::Blocks(self.header));
        }
        for (i, body) in self.bodies.into_iter().enumerate() {
            node.set_slot(format!("body-{i}"), Slot::Blocks(body));
        }
        if !self.footer.is_empty() {
            node.set_slot("footer", Slot::Blocks(self.footer));
        }
        Block::Custom(node)
    }

    pub fn from_custom(node: &CustomNode) -> Option<Self> {
        (node.type_name == CARD_TYPE).then_some(())?;
        let data: CardData = data_of(node)?;
        let title = match node.get_slot("title") {
            Some(Slot::Inlines(i)) => Some(i.clone()),
            _ => None,
        };
        let bodies = (0..data.bodies.len())
            .map(|i| blocks_slot(node, &format!("body-{i}")))
            .collect();
        Some(Card {
            header: blocks_slot(node, "header"),
            footer: blocks_slot(node, "footer"),
            title,
            bodies,
            data,
        })
    }
}

/// The custom node of `block` when it is a dashboard node of `type_name`.
pub fn as_dashboard_node<'a>(block: &'a Block, type_name: &str) -> Option<&'a CustomNode> {
    match block {
        Block::Custom(c) if c.type_name == type_name => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::Plain;
    use quarto_pandoc_types::inline::{Inline, Str};

    fn para(text: &str) -> Block {
        Block::Plain(Plain {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: generated(),
            })],
            source_info: generated(),
        })
    }

    #[test]
    fn card_round_trips_through_custom_node() {
        let card = Card {
            data: CardData {
                classes: vec!["card".into(), "cell".into()],
                expandable: true,
                bodies: vec![
                    BodyData {
                        classes: vec!["card-body".into()],
                        title: Some("A".into()),
                        ..Default::default()
                    },
                    BodyData {
                        classes: vec!["card-body".into()],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            title: Some(vec![Inline::Str(Str {
                text: "T".into(),
                source_info: generated(),
            })]),
            header: Vec::new(),
            bodies: vec![vec![para("one")], vec![para("two")]],
            footer: vec![para("foot")],
        };
        let block = card.clone().to_block(generated());
        let Block::Custom(node) = &block else {
            panic!("not custom")
        };
        let back = Card::from_custom(node).unwrap();
        assert_eq!(back.bodies, card.bodies);
        assert_eq!(back.footer, card.footer);
        assert_eq!(back.title, card.title);
        assert!(back.data.has_title && back.data.has_footer && !back.data.has_header);
        assert_eq!(back.data.bodies[0].title.as_deref(), Some("A"));
    }

    #[test]
    fn container_and_page_round_trip() {
        let c = Container {
            data: ContainerData {
                orientation: Orientation::Columns,
                id: String::new(),
                classes: vec!["flow".into()],
                layout: Some(Layout::Flow),
                height: Some("30%".into()),
                width: None,
                root: false,
                grid_template: "minmax(3em, 1fr)".into(),
            },
            children: vec![para("x")],
        };
        let Block::Custom(node) = c.clone().to_block() else {
            panic!()
        };
        assert_eq!(Container::from_custom(&node).unwrap(), c);
        assert!(Page::from_custom(&node).is_none(), "type checked");

        let p = Page {
            data: PageData {
                id: "page-1".into(),
                title: "P".into(),
                orientation: Orientation::Rows,
                scrolling: Some("true".into()),
                index: 1,
            },
            content: vec![para("y")],
        };
        let Block::Custom(node) = p.clone().to_block() else {
            panic!()
        };
        assert_eq!(Page::from_custom(&node).unwrap(), p);
    }
}
