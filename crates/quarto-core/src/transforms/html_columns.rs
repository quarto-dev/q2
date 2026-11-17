/*
 * transforms/html_columns.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * HTML columns: width attribute → inline width style.
 */

//! Convert `::: {.column width="40%"}` to an inline `width` for `format: html`.
//!
//! Pandoc's column syntax is a `.columns` div holding `.column` divs, each with
//! a `width`. A bare `width` attribute does nothing on a `<div>`. Pandoc's own
//! HTML writer rewrites it to `style="width:40%;"`, which is how Quarto 1 gets
//! side-by-side columns in an HTML page without any code of its own. q2 writes
//! HTML itself, so the rewrite is done here, on the AST: `q2 render` emits the
//! `style` and `q2 preview`, which renders from the AST, sees it too.
//!
//! The matching layout rules (`div.columns`, `div.column`) are in
//! `resources/scss/bootstrap/_bootstrap-rules.scss`.
//!
//! This is the `format: html` sibling of
//! [`RevealColumnsTransform`](crate::revealjs::RevealColumnsTransform), which
//! writes `flex-basis` for decks. It is not registered for:
//!
//! - **revealjs**, which has its own transform;
//! - **dashboard**, where a `.columns` div is a dashboard layout container and
//!   the dashboard transform reads `width` itself;
//! - **Pandoc-writer formats** (docx, pptx, …), whose writers need the raw
//!   `width` attribute.
//!
//! WASM-safe (pure AST manipulation).

use quarto_pandoc_types::block::{Block, Div};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::ast_walk::for_each_block_list_mut;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Transform that rewrites `.column` `width` attributes to an inline `width`.
pub struct HtmlColumnsTransform;

impl HtmlColumnsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HtmlColumnsTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for HtmlColumnsTransform {
    fn name(&self) -> &str {
        "html-columns"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        convert_columns(&mut ast.blocks);
        Ok(())
    }
}

/// Rewrite every `.column` div under `blocks`, at any depth (nested divs,
/// list items, custom-node slots).
fn convert_columns(blocks: &mut Vec<Block>) {
    for_each_block_list_mut(blocks, &mut |list| {
        for block in list.iter_mut() {
            if let Block::Div(div) = block {
                convert_column(div);
            }
        }
    });
}

/// If `div` is a `.column` carrying a `width`, move it to an inline `width`
/// style and drop the raw attribute.
fn convert_column(div: &mut Div) {
    // Read-only checks first: a mutable borrow of an empty `Attr` allocates.
    if !div.attr.1.iter().any(|c| c == "column") || !div.attr.2.contains_key("width") {
        return;
    }
    let Some(width) = div.attr.2.remove("width") else {
        return;
    };
    let width = width.trim();
    if width.is_empty() {
        return;
    }
    let existing = div.attr.2.remove("style").unwrap_or_default();
    let style = if existing.trim().is_empty() {
        format!("width: {width};")
    } else {
        format!("width: {width}; {}", existing.trim())
    };
    div.attr.2.insert("style".to_string(), style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_source_map::{By, SourceInfo};

    fn div(classes: &[&str], kvs: &[(&str, &str)], content: Vec<Block>) -> Div {
        let mut map = LinkedHashMap::new();
        for (k, v) in kvs {
            map.insert(k.to_string(), v.to_string());
        }
        Div {
            attr: (
                String::new(),
                classes.iter().map(|c| c.to_string()).collect(),
                map,
            )
                .into(),
            content,
            source_info: SourceInfo::generated(By::unknown()),
            attr_source: AttrSourceInfo::empty(),
        }
    }

    fn column(kvs: &[(&str, &str)]) -> Div {
        div(&["column"], kvs, vec![])
    }

    fn style_of(div: &Div) -> Option<&str> {
        div.attr.2.get("style").map(String::as_str)
    }

    #[test]
    fn width_becomes_inline_width() {
        let mut d = column(&[("width", "40%")]);
        convert_column(&mut d);
        assert_eq!(style_of(&d), Some("width: 40%;"));
        assert!(d.attr.2.get("width").is_none(), "raw `width` is removed");
    }

    #[test]
    fn width_is_trimmed() {
        let mut d = column(&[("width", " 30% ")]);
        convert_column(&mut d);
        assert_eq!(style_of(&d), Some("width: 30%;"));
    }

    /// The width goes first so a `width` the author put in `style` by hand
    /// still wins, as it does for the reveal transform's `flex-basis`.
    #[test]
    fn width_is_prepended_to_an_existing_style() {
        let mut d = column(&[("width", "50%"), ("style", "color: red;")]);
        convert_column(&mut d);
        assert_eq!(style_of(&d), Some("width: 50%; color: red;"));
    }

    #[test]
    fn other_attributes_and_classes_are_kept() {
        let mut d = div(
            &["column", "fragment"],
            &[("width", "40%"), ("data-id", "left")],
            vec![],
        );
        convert_column(&mut d);
        assert_eq!(d.attr.1, vec!["column".to_string(), "fragment".to_string()]);
        assert_eq!(d.attr.2.get("data-id"), Some(&"left".to_string()));
        assert_eq!(style_of(&d), Some("width: 40%;"));
    }

    #[test]
    fn column_without_width_untouched() {
        let mut d = column(&[]);
        convert_column(&mut d);
        assert!(style_of(&d).is_none());
    }

    #[test]
    fn empty_width_is_dropped_without_a_style() {
        let mut d = column(&[("width", "  ")]);
        convert_column(&mut d);
        assert!(d.attr.2.get("width").is_none());
        assert!(style_of(&d).is_none());
    }

    /// `width` means something else on other divs (a figure div, a dashboard
    /// card), so only the `column` class is rewritten. `columns` is not
    /// `column`.
    #[test]
    fn non_column_div_untouched() {
        for class in ["columns", "notcolumn", "cell"] {
            let mut d = div(&[class], &[("width", "40%")], vec![]);
            convert_column(&mut d);
            assert_eq!(d.attr.2.get("width"), Some(&"40%".to_string()), "{class}");
            assert!(style_of(&d).is_none(), "{class}");
        }
    }

    /// Columns are found at any depth: inside their `.columns` parent, and
    /// inside a custom node's block slot (a callout is still a `Callout`
    /// custom node when this transform runs).
    #[test]
    fn nested_columns_are_converted() {
        use quarto_pandoc_types::custom::{CustomNode, Slot};

        let columns = |widths: [&str; 2]| {
            Block::Div(Box::new(div(
                &["columns"],
                &[],
                widths
                    .iter()
                    .map(|w| Block::Div(Box::new(column(&[("width", w)]))))
                    .collect(),
            )))
        };
        let mut callout = CustomNode::new(
            "Callout",
            (String::new(), vec![], LinkedHashMap::new()).into(),
            SourceInfo::generated(By::unknown()),
        );
        callout.set_slot("content", Slot::Blocks(vec![columns(["25%", "75%"])]));
        let mut blocks = vec![columns(["40%", "60%"]), Block::Custom(Box::new(callout))];
        convert_columns(&mut blocks);

        let styles = |block: &Block| -> Vec<String> {
            let Block::Div(outer) = block else {
                panic!("columns div survives");
            };
            outer
                .content
                .iter()
                .map(|b| match b {
                    Block::Div(d) => style_of(d).unwrap_or_default().to_string(),
                    _ => panic!("column div survives"),
                })
                .collect()
        };
        assert_eq!(styles(&blocks[0]), ["width: 40%;", "width: 60%;"]);
        let Block::Custom(node) = &blocks[1] else {
            panic!("custom node survives");
        };
        let Some(Slot::Blocks(content)) = node.slots.get("content") else {
            panic!("content slot survives");
        };
        assert_eq!(styles(&content[0]), ["width: 25%;", "width: 75%;"]);
    }
}
