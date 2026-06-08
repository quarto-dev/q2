/*
 * revealjs/transform.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * RevealSlidesTransform: build the reveal slide tree + title slide.
 */

//! The `RevealSlidesTransform` AST transform.
//!
//! For `format: revealjs` this transform *replaces* the generic
//! `TitleBlockTransform` + `SectionizeTransform` pair (Pandoc keeps reveal
//! slide-construction separate from its `--section-divs` machinery; so do we).
//! It:
//!
//! 1. reads `slide-level` from the merged metadata (default 2),
//! 2. builds the two-level reveal slide tree via [`build_reveal_slides`],
//! 3. prepends a `<section id="title-slide">` synthesized from the document
//!    metadata (title / subtitle / author / date).

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div, Header, Paragraph};
use quarto_pandoc_types::inline::{Inline, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

use super::slides::{DEFAULT_SLIDE_LEVEL, build_reveal_slides};

/// Transform that constructs the reveal.js slide tree for `format: revealjs`.
pub struct RevealSlidesTransform;

impl RevealSlidesTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RevealSlidesTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for RevealSlidesTransform {
    fn name(&self) -> &str {
        "reveal-slides"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        let slide_level = ast
            .meta
            .get("slide-level")
            .and_then(|v| v.as_int())
            .filter(|n| *n >= 1)
            .map_or(DEFAULT_SLIDE_LEVEL, |n| n as usize);

        let title_slide = build_title_slide(&ast.meta);

        let blocks = std::mem::take(&mut ast.blocks);
        let mut slides = build_reveal_slides(blocks, slide_level);

        if let Some(title) = title_slide {
            slides.insert(0, title);
        }
        ast.blocks = slides;
        Ok(())
    }
}

/// A plain-text inline run.
fn str_inline(text: String) -> Inline {
    Inline::Str(Str {
        text,
        source_info: SourceInfo::default(),
    })
}

/// A classed paragraph for the title slide, rendered as
/// `<div class="…"><p>text</p></div>` (Paragraph carries no attr of its own).
fn classed_para(class: &str, text: String) -> Block {
    let para = Block::Paragraph(Paragraph {
        content: vec![str_inline(text)],
        source_info: SourceInfo::default(),
    });
    Block::Div(Div {
        attr: (String::new(), vec![class.to_string()], LinkedHashMap::new()),
        content: vec![para],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Read a metadata field as plain text (joining array items with ", ").
fn meta_text(meta: &quarto_pandoc_types::ConfigValue, key: &str) -> Option<String> {
    let v = meta.get(key)?;
    if let Some(arr) = v.as_array() {
        let parts: Vec<String> = arr.iter().filter_map(|i| i.as_plain_text()).collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    } else {
        v.as_plain_text()
    }
}

/// Build the `<section id="title-slide">` from metadata, or `None` if there is
/// no title and no author to show.
fn build_title_slide(meta: &quarto_pandoc_types::ConfigValue) -> Option<Block> {
    let title = meta_text(meta, "title");
    let subtitle = meta_text(meta, "subtitle");
    let author = meta_text(meta, "author");
    let date = meta_text(meta, "date");

    if title.is_none() && author.is_none() {
        return None;
    }

    let mut content: Vec<Block> = Vec::new();
    if let Some(title) = title {
        content.push(Block::Header(Header {
            level: 1,
            attr: (
                String::new(),
                vec!["title".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![str_inline(title)],
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        }));
    }
    if let Some(subtitle) = subtitle {
        content.push(classed_para("subtitle", subtitle));
    }
    if let Some(author) = author {
        content.push(classed_para("author", author));
    }
    if let Some(date) = date {
        content.push(classed_para("date", date));
    }

    Some(Block::Div(Div {
        attr: (
            "title-slide".to_string(),
            vec!["section".to_string(), "title-slide".to_string()],
            LinkedHashMap::new(),
        ),
        content,
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    }))
}
