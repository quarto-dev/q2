/*
 * dashboard/sizing.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Grid templates for dashboard containers: the port of Quarto 1's
//! `format-dashboard-layout.ts` (`computeRowLayouts`,
//! `computeColumnLayouts`, `rowLayoutForColumn`, `computeFillFr`,
//! `toGridSize`). Computed once, in the structure pass, and stored on each
//! container (`ContainerData::grid_template`) so the render lowering and
//! the React preview read the same value.
//!
//! One deliberate change (dashboard plan D5-Q2): a container's own
//! `.fill` / `.flow` (from its heading) sizes it in its parent's grid, as
//! the docs promise; Q1 read only cards' `data-fill`.

use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::custom::Slot;

use super::model::{
    CARD_TYPE, CONTAINER_TYPE, Card, Container, Layout, Orientation, PAGE_TYPE, Page,
};

/// One child's size: `fill`, `flow`, or an explicit size string.
#[derive(Debug, Clone, PartialEq)]
enum Size {
    Fill,
    Flow,
    Explicit(String),
}

const MIN_SIZE: &str = "3em";

/// Fill in `grid_template` on every container under `blocks`.
pub fn assign_grid_templates(blocks: &mut Blocks) {
    for block in blocks.iter_mut() {
        let Block::Custom(node) = block else {
            if let Block::Div(d) = block {
                assign_grid_templates(&mut d.content);
            }
            continue;
        };
        if node.type_name == CONTAINER_TYPE
            && let Some(mut container) = Container::from_custom(node)
        {
            assign_grid_templates(&mut container.children);
            container.data.grid_template = grid_template(&container);
            *block = container.to_block();
        } else if node.type_name == PAGE_TYPE
            && let Some(mut page) = Page::from_custom(node)
        {
            assign_grid_templates(&mut page.content);
            *block = page.to_block();
        } else {
            for (_, slot) in node.slots.iter_mut() {
                if let Slot::Blocks(b) = slot {
                    assign_grid_templates(b);
                }
            }
        }
    }
}

/// The grid template of `container`.
pub fn grid_template(container: &Container) -> String {
    let sizes = match container.data.orientation {
        Orientation::Rows => row_sizes(&container.children),
        Orientation::Columns => column_sizes(&container.children),
    };
    let fill_fr = fill_fr(&sizes);
    sizes
        .iter()
        .map(|s| to_grid_size(s, fill_fr))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Q1 skips `.hidden` children and non-active pages (`grid-skip`).
fn skipped(block: &Block) -> bool {
    match block {
        Block::Div(d) => d.attr.1.iter().any(|c| c == "hidden" || c == "grid-skip"),
        Block::Custom(n) if n.type_name == PAGE_TYPE => {
            Page::from_custom(n).is_some_and(|p| p.data.index > 1)
        }
        _ => false,
    }
}

fn card(block: &Block) -> Option<Card> {
    match block {
        Block::Custom(n) if n.type_name == CARD_TYPE => Card::from_custom(n),
        _ => None,
    }
}

fn container(block: &Block) -> Option<Container> {
    match block {
        Block::Custom(n) if n.type_name == CONTAINER_TYPE => Container::from_custom(n),
        _ => None,
    }
}

/// Q1 `isFlowCard`: every body flows (vacuously true without bodies).
fn is_flow_card(card: &Card) -> bool {
    card.data.bodies.iter().all(|b| b.flow)
}

fn is_valuebox(card: &Card) -> bool {
    card.data.classes.iter().any(|c| c == "valuebox")
}

/// Q1 `computeRowLayouts`.
fn row_sizes(children: &[Block]) -> Vec<Size> {
    let mut sizes = Vec::new();
    for child in children.iter().filter(|c| !skipped(c)) {
        if let Some(c) = card(child) {
            sizes.push(if let Some(h) = c.data.height {
                Size::Explicit(h)
            } else if let Some(fill) = c.data.fill {
                if fill { Size::Fill } else { Size::Flow }
            } else if is_flow_card(&c) {
                Size::Flow
            } else {
                Size::Fill
            });
        } else if let Some(k) = container(child) {
            sizes.push(if let Some(h) = k.data.height {
                Size::Explicit(h)
            } else if let Some(layout) = k.data.layout {
                // D5-Q2: the heading's `.fill` / `.flow` decides (Q1
                // ignored it here).
                layout_size(layout)
            } else {
                match k.data.orientation {
                    Orientation::Rows => {
                        if row_sizes(&k.children).contains(&Size::Fill) {
                            Size::Fill
                        } else {
                            Size::Flow
                        }
                    }
                    Orientation::Columns => row_size_for_columns(&k.children),
                }
            });
        } else {
            sizes.push(Size::Fill);
        }
    }
    sizes
}

fn layout_size(layout: Layout) -> Size {
    match layout {
        Layout::Fill => Size::Fill,
        Layout::Flow => Size::Flow,
    }
}

/// Q1 `rowLayoutForColumn` (the containing row has no `data-fill`, so the
/// default is `fill`).
fn row_size_for_columns(children: &[Block]) -> Size {
    let any_fill = children.iter().any(|child| match card(child) {
        Some(c) => match c.data.fill {
            Some(fill) => fill,
            None => !(is_valuebox(&c) || is_flow_card(&c)),
        },
        None => true,
    });
    if any_fill { Size::Fill } else { Size::Flow }
}

/// Q1 `computeColumnLayouts`: explicit width, else fill.
fn column_sizes(children: &[Block]) -> Vec<Size> {
    children
        .iter()
        .filter(|c| !skipped(c))
        .map(|child| {
            if let Some(k) = container(child) {
                return match (k.data.width, k.data.layout) {
                    (Some(w), _) => Size::Explicit(w),
                    // D5-Q2, as for rows.
                    (None, Some(layout)) => layout_size(layout),
                    (None, None) => Size::Fill,
                };
            }
            card(child)
                .and_then(|c| c.data.width)
                .map_or(Size::Fill, Size::Explicit)
        })
        .collect()
}

/// Q1 `computeFillFr`: the share each fill child gets once explicit
/// percentages are allocated.
fn fill_fr(sizes: &[Size]) -> f64 {
    let mut percents = Vec::new();
    let mut fills = 0usize;
    for s in sizes {
        match s {
            Size::Fill => fills += 1,
            Size::Explicit(e) => {
                if let Some(p) = e.strip_suffix('%')
                    && let Ok(v) = p.trim().parse::<f64>()
                {
                    percents.push(v);
                }
            }
            Size::Flow => {}
        }
    }
    let allocated: f64 = percents.iter().sum();
    if allocated > 0.0 {
        if allocated < 100.0 {
            (100.0 - allocated) / fills as f64
        } else {
            *percents.last().unwrap_or(&1.0)
        }
    } else {
        1.0
    }
}

/// JavaScript's number-to-string for the values `fill_fr` produces.
fn js_number(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 {
        format!("{}", v as i64)
    } else if v.is_infinite() {
        if v > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        }
    } else {
        format!("{v}")
    }
}

/// Q1 `toGridSize`.
fn to_grid_size(size: &Size, fill_fr: f64) -> String {
    match size {
        Size::Fill => format!("minmax({MIN_SIZE}, {}fr)", js_number(fill_fr)),
        Size::Flow => format!("minmax({MIN_SIZE}, max-content)"),
        Size::Explicit(e) => {
            if e.ends_with("px") {
                e.clone()
            } else if e.chars().last().is_some_and(|c| c.is_ascii_digit()) {
                format!("{e}px")
            } else if let Some(p) = e.strip_suffix('%') {
                let v: f64 = p.trim().parse().unwrap_or(0.0);
                format!("minmax({MIN_SIZE}, {}fr)", js_number(v))
            } else {
                format!("minmax({MIN_SIZE}, {e})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(v: &[&str]) -> Vec<Size> {
        v.iter()
            .map(|s| match *s {
                "fill" => Size::Fill,
                "flow" => Size::Flow,
                e => Size::Explicit(e.to_string()),
            })
            .collect()
    }

    fn template(v: &[&str]) -> String {
        let s = sizes(v);
        let fr = fill_fr(&s);
        s.iter()
            .map(|x| to_grid_size(x, fr))
            .collect::<Vec<_>>()
            .join(" ")
    }

    use super::super::model::{CardData, ContainerData};

    fn child(orientation: Orientation, layout: Option<Layout>, height: Option<&str>) -> Block {
        let card = Card {
            data: CardData {
                classes: vec!["card".into()],
                bodies: vec![Default::default()],
                ..Default::default()
            },
            title: None,
            header: vec![],
            bodies: vec![vec![]],
            footer: vec![],
        }
        .to_block(quarto_source_map::SourceInfo::for_test());
        Container {
            data: ContainerData {
                orientation,
                id: String::new(),
                classes: vec![],
                layout,
                height: height.map(str::to_string),
                width: None,
                root: false,
                grid_template: String::new(),
            },
            children: vec![card],
        }
        .to_block()
    }

    fn parent(orientation: Orientation, children: Vec<Block>) -> Container {
        Container {
            data: ContainerData {
                orientation,
                id: String::new(),
                classes: vec![],
                layout: None,
                height: None,
                width: None,
                root: true,
                grid_template: String::new(),
            },
            children,
        }
    }

    /// Dashboard plan D5-Q2: `.flow` / `.fill` on a row heading size the row
    /// (Q1 ignored them: only cards' `data-fill` reached the grid). An
    /// explicit height still wins.
    #[test]
    fn container_fill_and_flow_classes_size_the_grid() {
        let p = parent(
            Orientation::Rows,
            vec![
                child(Orientation::Columns, Some(Layout::Flow), None),
                child(Orientation::Columns, Some(Layout::Fill), None),
                child(Orientation::Columns, Some(Layout::Flow), Some("200px")),
            ],
        );
        assert_eq!(
            grid_template(&p),
            "minmax(3em, max-content) minmax(3em, 1fr) 200px"
        );
        let p = parent(
            Orientation::Columns,
            vec![
                child(Orientation::Rows, Some(Layout::Flow), None),
                child(Orientation::Rows, None, None),
            ],
        );
        assert_eq!(
            grid_template(&p),
            "minmax(3em, max-content) minmax(3em, 1fr)"
        );
    }

    #[test]
    fn plain_fills() {
        assert_eq!(
            template(&["fill", "fill"]),
            "minmax(3em, 1fr) minmax(3em, 1fr)"
        );
    }

    #[test]
    fn percentages_share_the_rest_with_fills() {
        assert_eq!(
            template(&["70%", "30%"]),
            "minmax(3em, 70fr) minmax(3em, 30fr)"
        );
        assert_eq!(
            template(&["25%", "fill"]),
            "minmax(3em, 25fr) minmax(3em, 75fr)"
        );
        assert_eq!(
            template(&["40%", "fill", "fill"]),
            "minmax(3em, 40fr) minmax(3em, 30fr) minmax(3em, 30fr)"
        );
    }

    #[test]
    fn explicit_units() {
        assert_eq!(template(&["300px", "fill"]), "300px minmax(3em, 1fr)");
        assert_eq!(template(&["200"]), "200px");
        assert_eq!(template(&["10em"]), "minmax(3em, 10em)");
        assert_eq!(template(&["flow"]), "minmax(3em, max-content)");
        assert_eq!(template(&[]), "");
    }

    #[test]
    fn fractional_shares_print_like_javascript() {
        assert_eq!(
            template(&["10%", "fill", "fill", "fill", "fill"]),
            "minmax(3em, 10fr) minmax(3em, 22.5fr) minmax(3em, 22.5fr) minmax(3em, 22.5fr) minmax(3em, 22.5fr)"
        );
    }
}
