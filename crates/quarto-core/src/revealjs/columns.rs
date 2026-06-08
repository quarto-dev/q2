/*
 * revealjs/columns.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Reveal columns: width attribute → flex-basis style (Phase 2c).
 */

//! Convert `::: {.column width="X"}` to flexbox sizing.
//!
//! Quarto's columns are a flex row (`.columns { display:flex }`) of `.column`
//! children. The author writes `width="40%"`, but a bare `width` attribute is
//! invalid on a `<div>`; Quarto 1 rewrites it to an inline
//! `style="flex-basis: 40%;"` (`layout/html.lua`). This transform does the
//! same in Rust so **both** `q2 render` (HTML writer emits the `style`) and
//! `q2 preview` (`previewRegistry` passes `style` through) lay columns out
//! identically. The base `.columns`/`.column` rules live in
//! `resources/revealjs/quarto-reveal.css` (inlined for render, imported for
//! preview).
//!
//! WASM-safe (pure AST manipulation).

use quarto_pandoc_types::block::{Block, Div};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that rewrites `.column` `width` attributes to `flex-basis`.
pub struct RevealColumnsTransform;

impl RevealColumnsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RevealColumnsTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for RevealColumnsTransform {
    fn name(&self) -> &str {
        "reveal-columns"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        walk_blocks(&mut ast.blocks);
        Ok(())
    }
}

fn walk_blocks(blocks: &mut [Block]) {
    for block in blocks {
        if let Block::Div(div) = block {
            convert_column(div);
            walk_blocks(&mut div.content);
        }
    }
}

/// If `div` is a `.column` carrying a `width`, move it to an inline
/// `flex-basis` style and drop the raw `width` attribute.
fn convert_column(div: &mut Div) {
    let is_column = div.attr.1.iter().any(|c| c == "column");
    if !is_column {
        return;
    }
    let Some(width) = div.attr.2.remove("width") else {
        return;
    };
    let existing = div.attr.2.remove("style").unwrap_or_default();
    let style = if existing.trim().is_empty() {
        format!("flex-basis: {width};")
    } else {
        format!("flex-basis: {width}; {}", existing.trim())
    };
    div.attr.2.insert("style".to_string(), style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_source_map::SourceInfo;

    fn column_div(width: Option<&str>, style: Option<&str>) -> Div {
        let mut kvs = LinkedHashMap::new();
        if let Some(w) = width {
            kvs.insert("width".to_string(), w.to_string());
        }
        if let Some(s) = style {
            kvs.insert("style".to_string(), s.to_string());
        }
        Div {
            attr: ("".to_string(), vec!["column".to_string()], kvs),
            content: vec![],
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        }
    }

    #[test]
    fn width_becomes_flex_basis() {
        let mut div = column_div(Some("40%"), None);
        convert_column(&mut div);
        assert_eq!(
            div.attr.2.get("style"),
            Some(&"flex-basis: 40%;".to_string())
        );
        assert!(div.attr.2.get("width").is_none());
    }

    #[test]
    fn width_prepended_to_existing_style() {
        let mut div = column_div(Some("50%"), Some("color: red"));
        convert_column(&mut div);
        assert_eq!(
            div.attr.2.get("style"),
            Some(&"flex-basis: 50%; color: red".to_string())
        );
    }

    #[test]
    fn non_column_div_untouched() {
        let mut kvs = LinkedHashMap::new();
        kvs.insert("width".to_string(), "40%".to_string());
        let mut div = Div {
            attr: ("".to_string(), vec!["notcolumn".to_string()], kvs),
            content: vec![],
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        };
        convert_column(&mut div);
        assert_eq!(div.attr.2.get("width"), Some(&"40%".to_string()));
        assert!(div.attr.2.get("style").is_none());
    }

    #[test]
    fn column_without_width_untouched() {
        let mut div = column_div(None, None);
        convert_column(&mut div);
        assert!(div.attr.2.get("style").is_none());
    }
}
