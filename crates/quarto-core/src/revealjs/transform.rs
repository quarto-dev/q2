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
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

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

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        let slide_level = slide_level_from_meta(&ast.meta);

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

/// Read `slide-level:` from the merged metadata. Values below 1 (and
/// unparseable ones) fall back to [`DEFAULT_SLIDE_LEVEL`]. Lenient:
/// accepts quoted and bare-scalar forms too (bd-yjsz6hdu).
fn slide_level_from_meta(meta: &quarto_pandoc_types::ConfigValue) -> usize {
    meta.get("slide-level")
        .and_then(|v| v.as_int_lenient())
        .filter(|n| *n >= 1)
        .map_or(DEFAULT_SLIDE_LEVEL, |n| n as usize)
}

/// A plain-text inline run.
fn str_inline(text: String) -> Inline {
    Inline::Str(Str {
        text,
        source_info: SourceInfo::generated(By::revealjs()),
    })
}

/// A classed paragraph for the title slide, rendered as
/// `<div class="…"><p>text</p></div>` (Paragraph carries no attr of its own).
fn classed_para(class: &str, text: String) -> Block {
    let para = Block::Paragraph(Paragraph {
        content: vec![str_inline(text)],
        source_info: SourceInfo::generated(By::revealjs()),
    });
    Block::Div(Div {
        attr: (String::new(), vec![class.to_string()], LinkedHashMap::new()),
        content: vec![para],
        source_info: SourceInfo::generated(By::revealjs()),
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
            source_info: SourceInfo::generated(By::revealjs()),
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
            // `center` keeps the title slide vertically centered even though
            // the deck defaults to `center: false` (top-aligned body slides).
            // reveal honors the per-slide `.center` class when the global
            // `center` config is off — this mirrors Quarto 1's approach
            // (format-reveal.ts: add `.center` to the title slide).
            vec![
                "section".to_string(),
                "title-slide".to_string(),
                "center".to_string(),
            ],
            LinkedHashMap::new(),
        ),
        content,
        source_info: SourceInfo::generated(By::revealjs()),
        attr_source: AttrSourceInfo::empty(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
    use quarto_source_map::{By, SourceInfo};

    fn meta_with_slide_level(value: ConfigValue) -> ConfigValue {
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "slide-level".to_string(),
                key_source: SourceInfo::generated(By::revealjs()),
                value,
            }],
            SourceInfo::generated(By::revealjs()),
        )
    }

    #[test]
    fn slide_level_defaults_and_reads_unquoted_integer() {
        let empty = ConfigValue::new_map(vec![], SourceInfo::generated(By::revealjs()));
        assert_eq!(slide_level_from_meta(&empty), DEFAULT_SLIDE_LEVEL);

        let int = ConfigValue::new_scalar(
            yaml_rust2::Yaml::Integer(3),
            SourceInfo::generated(By::revealjs()),
        );
        assert_eq!(slide_level_from_meta(&meta_with_slide_level(int)), 3);
    }

    // bd-yjsz6hdu Phase 3: a quoted `slide-level: "3"` (or the
    // PandocInlines a bare front-matter scalar becomes) was missed by
    // the strict `as_int()` and silently fell back to the default.
    #[test]
    fn slide_level_accepts_quoted_integer() {
        let quoted = ConfigValue::new_string("3", SourceInfo::generated(By::revealjs()));
        assert_eq!(slide_level_from_meta(&meta_with_slide_level(quoted)), 3);
    }

    #[test]
    fn slide_level_below_one_falls_back_to_default() {
        let zero = ConfigValue::new_scalar(
            yaml_rust2::Yaml::Integer(0),
            SourceInfo::generated(By::revealjs()),
        );
        assert_eq!(
            slide_level_from_meta(&meta_with_slide_level(zero)),
            DEFAULT_SLIDE_LEVEL
        );
    }
}
