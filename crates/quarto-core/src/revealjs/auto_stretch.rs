/*
 * revealjs/auto_stretch.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * RevealAutoStretchTransform: add `.r-stretch` to single-image slides.
 */

//! Auto-stretch single-image slides (Stage D4).
//!
//! Quarto 1 sizes a lone slide image to fill the available vertical space by
//! adding reveal's core `.r-stretch` class (`format-reveal.ts` `applyStretch`).
//! Without it, a large image overflows the slide. This transform ports the
//! **default-on** behavior, gated by `auto-stretch: false` (Q1's schema default
//! is `true`).
//!
//! `.r-stretch` is a **reveal-core** class — no Quarto SCSS is needed. This is
//! therefore a pure AST transform: it adds the class to the image and lets
//! reveal's own layout size it.
//!
//! Scope (conservative, matching this stage's plan; **narrower than Q1**):
//! a slide qualifies only when its content — *ignoring the slide heading* — is
//! exactly **one** block that is a `Paragraph` wrapping a single `Image`, or a
//! `Figure` wrapping a single `Image`. Q1 is more aggressive (it stretches a
//! lone image even on a slide that also has text); we stretch only
//! single-image slides, where overflow is the real problem and there is no text
//! for the stretch to collide with.
//!
//! Opt-outs (ported from Q1 `applyStretch`):
//! - `auto-stretch: false` in metadata — disables the whole transform.
//! - a slide section with class `.nostretch`.
//! - an image with class `.nostretch` (the class is removed and the image
//!   skipped) or `.absolute`, or one already carrying `.stretch`/`.r-stretch`.
//! - an image with **explicit sizing**. Q1 guards only `height`; we also guard
//!   `width` (attribute or inline `style`), so an author who deliberately sized
//!   an image is never overridden — a small, sound divergence in the
//!   conservative direction.
//!
//! Runs after `RevealSlidesTransform` (the `<section>` tree must exist) and
//! after `RevealFootnotesTransform` (so a slide with a coalesced footnote/aside
//! has >1 body block and is naturally skipped). WASM-safe (pure AST).

use quarto_pandoc_types::block::{Block, Div, Figure};
use quarto_pandoc_types::inline::{Image, Inline};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

const STRETCH_CLASS: &str = "r-stretch";

/// Transform that adds `.r-stretch` to single-image reveal slides.
pub struct RevealAutoStretchTransform;

impl RevealAutoStretchTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RevealAutoStretchTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for RevealAutoStretchTransform {
    fn name(&self) -> &str {
        "reveal-auto-stretch"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        let auto_stretch = ast
            .meta
            .get("auto-stretch")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        walk_sections(&mut ast.blocks, auto_stretch);
        Ok(())
    }
}

/// Walk the slide tree, applying stretch to qualifying leaf sections and
/// recursing into stacks (whose direct children are nested sections).
fn walk_sections(blocks: &mut [Block], auto_stretch: bool) {
    for block in blocks {
        if let Block::Div(div) = block {
            if is_section(div) {
                maybe_stretch_section(div, auto_stretch);
            }
            // Recurse so nested (vertical-stack) sections are reached. A stack's
            // own body is nested sections, so `maybe_stretch_section` no-ops on
            // it (not a single image block) before we descend.
            walk_sections(&mut div.content, auto_stretch);
        }
    }
}

fn is_section(div: &Div) -> bool {
    div.attr.1.iter().any(|c| c == "section")
}

/// Add `.r-stretch` to the lone image of a single-image leaf slide, honoring
/// the opt-outs. No-op for stacks, multi-block slides, sized/opted-out images.
fn maybe_stretch_section(div: &mut Div, auto_stretch: bool) {
    // Per-slide opt-out.
    if div.attr.1.iter().any(|c| c == "nostretch") {
        return;
    }

    // The slide body is everything but the heading(s). Exactly one body block
    // qualifies a single-image slide.
    let body: Vec<usize> = div
        .content
        .iter()
        .enumerate()
        .filter(|(_, b)| !matches!(b, Block::Header(_)))
        .map(|(i, _)| i)
        .collect();
    if body.len() != 1 {
        return;
    }

    let Some(image) = body_image_mut(&mut div.content[body[0]]) else {
        return;
    };

    // Per-image opt-outs. `.nostretch` is consumed (removed) per Q1.
    if let Some(pos) = image.attr.1.iter().position(|c| c == "nostretch") {
        image.attr.1.remove(pos);
        return;
    }
    if image.attr.1.iter().any(|c| c == "absolute") {
        return;
    }
    if image
        .attr
        .1
        .iter()
        .any(|c| c == STRETCH_CLASS || c == "stretch")
    {
        return; // already stretched (explicit or idempotent re-run)
    }
    if has_explicit_size(image) {
        return;
    }

    if auto_stretch {
        image.attr.1.push(STRETCH_CLASS.to_string());
    }
}

/// A mutable reference to the slide's lone image, when the body block is a
/// `Paragraph` whose only inline is an `Image`, or a `Figure` wrapping exactly
/// one image. Otherwise `None`.
fn body_image_mut(block: &mut Block) -> Option<&mut Image> {
    match block {
        Block::Paragraph(p) if p.content.len() == 1 => match &mut p.content[0] {
            Inline::Image(img) => Some(img),
            _ => None,
        },
        Block::Figure(f) if count_figure_images(f) == 1 => figure_image_mut(f),
        _ => None,
    }
}

/// Count `Image` inlines directly inside a figure's `Paragraph`/`Plain` blocks.
fn count_figure_images(f: &Figure) -> usize {
    f.content
        .iter()
        .map(|b| match b {
            Block::Paragraph(p) => image_count(&p.content),
            Block::Plain(p) => image_count(&p.content),
            _ => 0,
        })
        .sum()
}

fn image_count(inlines: &[Inline]) -> usize {
    inlines
        .iter()
        .filter(|i| matches!(i, Inline::Image(_)))
        .count()
}

/// First `Image` mutable ref inside a figure's `Paragraph`/`Plain` blocks.
fn figure_image_mut(f: &mut Figure) -> Option<&mut Image> {
    for b in &mut f.content {
        let inlines = match b {
            Block::Paragraph(p) => &mut p.content,
            Block::Plain(p) => &mut p.content,
            _ => continue,
        };
        for inline in inlines {
            if let Inline::Image(img) = inline {
                return Some(img);
            }
        }
    }
    None
}

/// Whether the image already carries explicit `width`/`height` sizing (an attr
/// key or an inline `style` declaration). Such images are left untouched.
fn has_explicit_size(image: &Image) -> bool {
    let kvs = &image.attr.2;
    if kvs.contains_key("width") || kvs.contains_key("height") {
        return true;
    }
    if let Some(style) = kvs.get("style") {
        let s = style.to_ascii_lowercase();
        if s.contains("height:") || s.contains("width:") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::Caption;
    use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
    use quarto_pandoc_types::block::{Figure, Header, Paragraph};
    use quarto_pandoc_types::inline::Str;
    use quarto_source_map::{By, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::generated(By::revealjs())
    }

    fn image(classes: &[&str], kvs: &[(&str, &str)]) -> Inline {
        let mut m = LinkedHashMap::new();
        for (k, v) in kvs {
            m.insert(k.to_string(), v.to_string());
        }
        Inline::Image(Image {
            attr: (
                String::new(),
                classes.iter().map(|s| s.to_string()).collect(),
                m,
            ),
            content: vec![],
            target: ("pic.png".to_string(), String::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    fn para(inlines: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content: inlines,
            source_info: si(),
        })
    }

    fn header() -> Block {
        Block::Header(Header {
            level: 2,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: "Slide".to_string(),
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn section(classes: &[&str], content: Vec<Block>) -> Block {
        let mut cls = vec!["section".to_string()];
        cls.extend(classes.iter().map(|s| s.to_string()));
        Block::Div(Div {
            attr: (String::new(), cls, LinkedHashMap::new()),
            content,
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn figure(blocks: Vec<Block>) -> Block {
        Block::Figure(Figure {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: None,
                source_info: si(),
            },
            content: blocks,
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Classes on the lone image of the first section, after running the walk.
    fn stretch_classes(mut blocks: Vec<Block>, auto_stretch: bool) -> Vec<String> {
        walk_sections(&mut blocks, auto_stretch);
        let Block::Div(div) = &blocks[0] else {
            panic!("expected section");
        };
        // find the image anywhere in the section
        fn find(blocks: &[Block]) -> Option<&Image> {
            for b in blocks {
                match b {
                    Block::Paragraph(p) => {
                        for i in &p.content {
                            if let Inline::Image(img) = i {
                                return Some(img);
                            }
                        }
                    }
                    Block::Plain(p) => {
                        for i in &p.content {
                            if let Inline::Image(img) = i {
                                return Some(img);
                            }
                        }
                    }
                    Block::Figure(f) => {
                        if let Some(img) = find(&f.content) {
                            return Some(img);
                        }
                    }
                    Block::Div(d) => {
                        if let Some(img) = find(&d.content) {
                            return Some(img);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        find(&div.content).expect("image present").attr.1.clone()
    }

    #[test]
    fn lone_paragraph_image_gets_stretch() {
        let blocks = vec![section(&[], vec![header(), para(vec![image(&[], &[])])])];
        assert!(stretch_classes(blocks, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn figure_image_gets_stretch() {
        let blocks = vec![section(
            &[],
            vec![header(), figure(vec![para(vec![image(&[], &[])])])],
        )];
        assert!(stretch_classes(blocks, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn image_without_heading_gets_stretch() {
        let blocks = vec![section(&[], vec![para(vec![image(&[], &[])])])];
        assert!(stretch_classes(blocks, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn auto_stretch_false_disables() {
        let blocks = vec![section(&[], vec![header(), para(vec![image(&[], &[])])])];
        assert!(!stretch_classes(blocks, false).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn multi_block_slide_skipped() {
        // heading + image + a second paragraph → not single-image.
        let blocks = vec![section(
            &[],
            vec![
                header(),
                para(vec![image(&[], &[])]),
                para(vec![Inline::Str(Str {
                    text: "text".to_string(),
                    source_info: si(),
                })]),
            ],
        )];
        assert!(!stretch_classes(blocks, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn inline_image_among_text_skipped() {
        let blocks = vec![section(
            &[],
            vec![
                header(),
                para(vec![
                    Inline::Str(Str {
                        text: "Here".to_string(),
                        source_info: si(),
                    }),
                    image(&[], &[]),
                ]),
            ],
        )];
        assert!(!stretch_classes(blocks, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn sized_image_skipped() {
        let width = vec![section(
            &[],
            vec![para(vec![image(&[], &[("width", "300")])])],
        )];
        assert!(!stretch_classes(width, true).contains(&"r-stretch".to_string()));
        let height = vec![section(
            &[],
            vec![para(vec![image(&[], &[("height", "200")])])],
        )];
        assert!(!stretch_classes(height, true).contains(&"r-stretch".to_string()));
        let styled = vec![section(
            &[],
            vec![para(vec![image(&[], &[("style", "height: 4em;")])])],
        )];
        assert!(!stretch_classes(styled, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn slide_nostretch_opts_out() {
        let blocks = vec![section(
            &["nostretch"],
            vec![header(), para(vec![image(&[], &[])])],
        )];
        assert!(!stretch_classes(blocks, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn image_nostretch_opts_out_and_class_removed() {
        let blocks = vec![section(
            &[],
            vec![header(), para(vec![image(&["nostretch"], &[])])],
        )];
        let classes = stretch_classes(blocks, true);
        assert!(!classes.contains(&"r-stretch".to_string()));
        assert!(
            !classes.contains(&"nostretch".to_string()),
            "nostretch should be consumed"
        );
    }

    #[test]
    fn image_absolute_skipped() {
        let blocks = vec![section(&[], vec![para(vec![image(&["absolute"], &[])])])];
        assert!(!stretch_classes(blocks, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn already_stretched_is_idempotent() {
        let blocks = vec![section(&[], vec![para(vec![image(&["r-stretch"], &[])])])];
        let classes = stretch_classes(blocks, true);
        assert_eq!(
            classes.iter().filter(|c| *c == "r-stretch").count(),
            1,
            "no duplicate r-stretch on re-run"
        );
    }
}
