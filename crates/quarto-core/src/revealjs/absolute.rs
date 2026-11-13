/*
 * revealjs/absolute.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Reveal absolute positioning: position attributes → inline style.
 */

//! Convert the position attributes of an `.absolute` element to an inline
//! style.
//!
//! The author writes `![](a.png){.absolute top=200 left=400 width="100"}` to
//! place an element at a fixed spot on a slide. Quarto 1 rewrites the six
//! attributes `top`, `left`, `bottom`, `right`, `width` and `height` into
//! `style` (`quarto-post/reveal.lua`, `applyPosition`), for a `Div`, a `Span`
//! or an `Image`, and adds `px` to a value with no unit. Its stylesheet then
//! makes every `.absolute` element `position: absolute`.
//!
//! This transform does the rewrite on the AST, so `q2 render` (the HTML writer
//! emits the `style`) and `q2 preview` (which renders from the AST) agree.
//! Without it the HTML writer emits `data-top="200"`, which positions nothing.
//! The matching rule is in `resources/revealjs/quarto-reveal.css`.
//!
//! Differences from Quarto 1, all in output Quarto 1 gets wrong: no trailing
//! space in the style, a `;` between an author `style` and the declarations
//! added here, no empty `style` on an `.absolute` element that has none of
//! the six attributes, and `px` only on plain decimal numbers (Quarto 1 lets
//! Lua decide, so `1e2` becomes `100.0px`).
//!
//! WASM-safe (pure AST manipulation).

use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::ast_walk::{for_each_block_list_mut, for_each_inline_mut};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// The class that marks an absolutely positioned element.
const ABSOLUTE_CLASS: &str = "absolute";

/// The attributes moved into `style`, in the order Quarto 1 writes them.
const POSITION_ATTRS: [&str; 6] = ["top", "left", "bottom", "right", "width", "height"];

/// Transform that rewrites the position attributes of `.absolute` elements.
pub struct RevealAbsoluteTransform;

impl RevealAbsoluteTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RevealAbsoluteTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for RevealAbsoluteTransform {
    fn name(&self) -> &str {
        "reveal-absolute"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        apply_to_blocks(&mut ast.blocks);
        Ok(())
    }
}

/// Rewrite every `.absolute` `Div`, `Span` and `Image` under `blocks`.
fn apply_to_blocks(blocks: &mut Vec<Block>) {
    for_each_block_list_mut(blocks, &mut |list| {
        for block in list.iter_mut() {
            if let Block::Div(div) = block {
                apply_position(&mut div.attr);
            }
        }
    });
    for_each_inline_mut(blocks, &mut |inline| match inline {
        Inline::Span(span) => apply_position(&mut span.attr),
        Inline::Image(image) => apply_position(&mut image.attr),
        _ => {}
    });
}

/// If `attr` has the `absolute` class, move its position attributes into
/// `style`. An element with none of them is left as it is.
fn apply_position(attr: &mut Attr) {
    // Read-only checks first: a mutable borrow of an empty `Attr` allocates.
    if !attr.1.iter().any(|c| c == ABSOLUTE_CLASS)
        || !POSITION_ATTRS.iter().any(|key| attr.2.contains_key(*key))
    {
        return;
    }
    let mut declarations = Vec::new();
    for key in POSITION_ATTRS {
        let Some(value) = attr.2.remove(key) else {
            continue;
        };
        let value = value.trim();
        if !value.is_empty() {
            declarations.push(format!("{key}: {};", css_size(value)));
        }
    }
    if declarations.is_empty() {
        return;
    }
    let existing = attr.2.remove("style").unwrap_or_default();
    let existing = existing.trim();
    let mut style = String::new();
    if !existing.is_empty() {
        style.push_str(existing);
        if !existing.ends_with(';') {
            style.push(';');
        }
        style.push(' ');
    }
    style.push_str(&declarations.join(" "));
    attr.2.insert("style".to_string(), style);
}

/// A plain decimal number means pixels; any other value is already a CSS
/// length (or something else the author meant) and passes through.
fn css_size(value: &str) -> String {
    if is_plain_number(value) {
        format!("{value}px")
    } else {
        value.to_string()
    }
}

/// `200`, `12.5`, `-10`, `+.5`: an optional sign, then digits with at most one
/// decimal point and at least one digit.
fn is_plain_number(value: &str) -> bool {
    let digits = value.strip_prefix(['-', '+']).unwrap_or(value);
    digits.chars().any(|c| c.is_ascii_digit())
        && digits.chars().filter(|c| *c == '.').count() <= 1
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
    use quarto_pandoc_types::block::{Div, Paragraph};
    use quarto_pandoc_types::inline::{Image, Span};
    use quarto_source_map::{By, SourceInfo};

    fn attr(classes: &[&str], kvs: &[(&str, &str)]) -> Attr {
        let mut map = LinkedHashMap::new();
        for (k, v) in kvs {
            map.insert(k.to_string(), v.to_string());
        }
        (
            String::new(),
            classes.iter().map(|c| c.to_string()).collect(),
            map,
        )
            .into()
    }

    fn source() -> SourceInfo {
        SourceInfo::generated(By::revealjs())
    }

    fn style_of(attr: &Attr) -> Option<&str> {
        attr.2.get("style").map(String::as_str)
    }

    fn positioned(kvs: &[(&str, &str)]) -> Attr {
        let mut a = attr(&["absolute"], kvs);
        apply_position(&mut a);
        a
    }

    #[test]
    fn bare_numbers_become_pixels() {
        let a = positioned(&[("top", "200"), ("left", "400"), ("width", "100")]);
        assert_eq!(style_of(&a), Some("top: 200px; left: 400px; width: 100px;"));
    }

    #[test]
    fn position_attributes_are_removed() {
        let a = positioned(&[
            ("top", "1"),
            ("left", "2"),
            ("bottom", "3"),
            ("right", "4"),
            ("width", "5"),
            ("height", "6"),
        ]);
        for key in POSITION_ATTRS {
            assert!(a.2.get(key).is_none(), "`{key}` must be removed");
        }
        assert_eq!(
            style_of(&a),
            Some("top: 1px; left: 2px; bottom: 3px; right: 4px; width: 5px; height: 6px;")
        );
    }

    /// Quarto 1 writes the declarations in a fixed order, whatever order the
    /// author used.
    #[test]
    fn declarations_follow_quarto_1_order() {
        let a = positioned(&[("height", "10"), ("right", "20"), ("top", "30")]);
        assert_eq!(style_of(&a), Some("top: 30px; right: 20px; height: 10px;"));
    }

    #[test]
    fn values_with_units_pass_through() {
        let a = positioned(&[
            ("top", "10%"),
            ("left", "2em"),
            ("width", "calc(50% - 1em)"),
        ]);
        assert_eq!(
            style_of(&a),
            Some("top: 10%; left: 2em; width: calc(50% - 1em);")
        );
    }

    #[test]
    fn decimal_and_negative_numbers_become_pixels() {
        let a = positioned(&[("top", "12.5"), ("left", "-10"), ("width", " 40 ")]);
        assert_eq!(style_of(&a), Some("top: 12.5px; left: -10px; width: 40px;"));
    }

    /// Only plain decimal numbers get `px`. Quarto 1 would turn `1e2` into
    /// `100.0px`; here it passes through unchanged.
    #[test]
    fn non_decimal_numbers_pass_through() {
        let a = positioned(&[("top", "1e2"), ("left", "0x10"), ("width", ".")]);
        assert_eq!(style_of(&a), Some("top: 1e2; left: 0x10; width: .;"));
    }

    #[test]
    fn declarations_follow_an_existing_style() {
        let a = positioned(&[("style", "color: red;"), ("top", "10")]);
        assert_eq!(style_of(&a), Some("color: red; top: 10px;"));
    }

    /// Quarto 1 concatenates with no separator, which breaks both
    /// declarations. The missing `;` is added here.
    #[test]
    fn existing_style_without_semicolon_gets_one() {
        let a = positioned(&[("style", "color: red"), ("top", "10")]);
        assert_eq!(style_of(&a), Some("color: red; top: 10px;"));
    }

    #[test]
    fn other_attributes_are_kept() {
        let a = positioned(&[("top", "10"), ("data-id", "box"), ("fig-alt", "A box")]);
        assert_eq!(a.2.get("data-id"), Some(&"box".to_string()));
        assert_eq!(a.2.get("fig-alt"), Some(&"A box".to_string()));
        assert_eq!(style_of(&a), Some("top: 10px;"));
    }

    #[test]
    fn empty_value_is_dropped() {
        let a = positioned(&[("top", ""), ("left", "5")]);
        assert!(a.2.get("top").is_none());
        assert_eq!(style_of(&a), Some("left: 5px;"));
    }

    #[test]
    fn absolute_without_position_attributes_untouched() {
        let a = positioned(&[("data-id", "box")]);
        assert!(style_of(&a).is_none(), "no empty style is added");
        assert_eq!(a.2.get("data-id"), Some(&"box".to_string()));
    }

    #[test]
    fn non_absolute_element_untouched() {
        let mut a = attr(&["fragment"], &[("top", "200"), ("width", "100")]);
        apply_position(&mut a);
        assert_eq!(a.2.get("top"), Some(&"200".to_string()));
        assert_eq!(a.2.get("width"), Some(&"100".to_string()));
        assert!(style_of(&a).is_none());
    }

    fn absolute_div() -> Block {
        Block::Div(Box::new(Div {
            attr: attr(&["absolute"], &[("bottom", "20")]),
            content: vec![],
            source_info: source(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    fn absolute_span() -> Inline {
        Inline::Span(Box::new(Span {
            attr: attr(&["absolute"], &[("right", "30")]),
            content: vec![],
            source_info: source(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    fn absolute_image() -> Inline {
        Inline::Image(Box::new(Image {
            attr: attr(&["absolute"], &[("top", "40")]),
            content: vec![],
            target: ("a.png".to_string(), String::new()),
            source_info: source(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        }))
    }

    fn para(inlines: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content: inlines,
            source_info: source(),
        })
    }

    #[test]
    fn div_span_and_image_are_all_rewritten() {
        let mut blocks = vec![
            absolute_div(),
            para(vec![absolute_span(), absolute_image()]),
        ];
        apply_to_blocks(&mut blocks);

        let Block::Div(div) = &blocks[0] else {
            panic!("div survives");
        };
        assert_eq!(style_of(&div.attr), Some("bottom: 20px;"));

        let Block::Paragraph(p) = &blocks[1] else {
            panic!("paragraph survives");
        };
        let Inline::Span(span) = &p.content[0] else {
            panic!("span survives");
        };
        assert_eq!(style_of(&span.attr), Some("right: 30px;"));
        let Inline::Image(image) = &p.content[1] else {
            panic!("image survives");
        };
        assert_eq!(style_of(&image.attr), Some("top: 40px;"));
    }

    /// An `.absolute` element nested in another div, and one inside a custom
    /// node's block slot (a callout is still a `Callout` custom node when this
    /// transform runs), are both reached.
    #[test]
    fn nested_and_custom_node_slot_elements_are_rewritten() {
        use quarto_pandoc_types::custom::{CustomNode, Slot};

        let outer = Block::Div(Box::new(Div {
            attr: attr(&["columns"], &[]),
            content: vec![absolute_div(), para(vec![absolute_image()])],
            source_info: source(),
            attr_source: AttrSourceInfo::empty(),
        }));
        let mut callout = CustomNode::new(
            "Callout",
            (String::new(), vec![], LinkedHashMap::new()).into(),
            source(),
        );
        callout.set_slot("content", Slot::Blocks(vec![absolute_div()]));
        let mut blocks = vec![outer, Block::Custom(Box::new(callout))];
        apply_to_blocks(&mut blocks);

        let Block::Div(outer) = &blocks[0] else {
            panic!("outer div survives");
        };
        let Block::Div(inner) = &outer.content[0] else {
            panic!("inner div survives");
        };
        assert_eq!(style_of(&inner.attr), Some("bottom: 20px;"));
        let Block::Paragraph(p) = &outer.content[1] else {
            panic!("paragraph survives");
        };
        let Inline::Image(image) = &p.content[0] else {
            panic!("image survives");
        };
        assert_eq!(style_of(&image.attr), Some("top: 40px;"));

        let Block::Custom(node) = &blocks[1] else {
            panic!("custom node survives");
        };
        let Some(Slot::Blocks(content)) = node.slots.get("content") else {
            panic!("content slot survives");
        };
        let Block::Div(div) = &content[0] else {
            panic!("div in slot survives");
        };
        assert_eq!(style_of(&div.attr), Some("bottom: 20px;"));
    }
}
