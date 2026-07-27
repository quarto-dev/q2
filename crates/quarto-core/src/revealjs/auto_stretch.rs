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
//! Scope (matches Quarto 1's `applyStretch`): a slide qualifies when it holds
//! exactly **one** image (peripheral `.notes`/`.aside` content aside), carries
//! no `.aside`, and that image sits in a *standalone* top-level block — a
//! `Paragraph` whose only inline is the image, or a `Figure`. Sibling blocks (a
//! heading, an explanatory paragraph) are allowed: reveal sizes the image to
//! the space they leave, so the common "heading + a sentence + a diagram" slide
//! stretches. An image nested in a `.column`/layout/fragment div, an image
//! among inline text, or a multi-image slide is left untouched.
//!
//! **Structure matters: the stretched `<img>` must be a *direct child* of the
//! `<section>`.** reveal sizes `.r-stretch` only via the selector
//! `section > .stretch, section > .r-stretch` (direct children), so a Pandoc
//! `<p>` wrapper makes the class inert and the image renders at natural size
//! (bd-zkstclhl). We therefore **unwrap** the standalone `Paragraph[Image]`
//! into a `Plain[Image]` — the HTML writer renders `Plain` inlines bare (no
//! `<p>`), yielding `section > img.r-stretch`.
//!
//! Quarto 1 achieves the same DOM with a *post-Pandoc DOM postprocessor*
//! (`applyStretch` in `format-reveal.ts`) that detaches the `<img>` and
//! re-inserts it at section level. **We deliberately do not port that pattern.**
//! Q2 emits HTML directly from the AST and has no DOM-mutation stage; the
//! structural fix belongs here, in the AST, not in a new postprocessor. (See
//! the no-DOM-postprocessor rule in `CLAUDE.md` → Architecture Notes.)
//!
//! A captioned `Figure` (markdown `![caption](x)`) is hoisted the same way: the
//! `Figure` is replaced by a `Plain[Image]` (figure `id` transferred onto the
//! img) followed by a caption `Paragraph` carrying a trailing
//! `Inline::Attr{.caption}` (capability bd-itqcfxc3), so the writer emits
//! `section > img.r-stretch` + a sibling `<p class="caption">` (bd-38ioql41).
//! The *cross-referenceable* figure case (`::: {#fig-…}`) is still un-stretched
//! on single-image slides: at auto-stretch time it is a plain `Block::Div`, and
//! the crossref→`Figure` conversion runs later and is excluded from the preview
//! pipeline (no single AST shape common to render and preview) — a documented
//! divergence from Q1 deferred for a future figure-alignment milestone.
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

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Div, Figure, Paragraph, Plain};
use quarto_pandoc_types::inline::{Image, Inline, InlineAttr};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

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

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
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

/// Add `.r-stretch` to the lone image of a slide, honoring the opt-outs.
///
/// A slide qualifies when it holds exactly **one** image (counting everything
/// but speaker `.notes`), carries no peripheral `.aside`, and that image lives
/// in a *standalone* top-level block — a `Paragraph` whose only inline is the
/// image, or a `Figure`. Other blocks (a heading, explanatory paragraphs) may
/// coexist: reveal sizes the image to the space they leave. This matches Quarto
/// 1's `applyStretch`; an image nested in a `.column`/layout/fragment div, an
/// image among inline text, or a multi-image slide is left untouched.
fn maybe_stretch_section(div: &mut Div, auto_stretch: bool) {
    // Per-slide opt-out, peripheral aside, and exactly-one-image gates.
    if div.attr.1.iter().any(|c| c == "nostretch") {
        return;
    }
    if contains_aside(&div.content) {
        return;
    }
    if count_images(&div.content) != 1 {
        return;
    }

    // The image must sit in a standalone top-level block (Paragraph[Image] or
    // Figure). If the single image is nested in a custom/layout div, no
    // top-level block is eligible and we leave it alone.
    //
    // We index by position so that, when we stretch, we can replace the holder
    // block with the unwrapped form (a `Figure` expands to *two* blocks — image
    // + caption). The mutable borrow used to decide/build is scoped to end
    // before the structural `splice`.
    for idx in 0..div.content.len() {
        // What to do with the block at `idx`.
        enum Plan {
            /// Not the image holder — keep scanning.
            Skip,
            /// Holder found but left wrapped (an opt-out fired, or auto-stretch
            /// is off) — nothing to do, and there is only one image, so stop.
            LeaveWrapped,
            /// Replace the holder block with these unwrapped blocks.
            Replace(Vec<Block>),
        }

        let plan = match &mut div.content[idx] {
            // Standalone `Paragraph[Image]` — the common case. When the image
            // is (or becomes) stretched we **unwrap** the paragraph into a
            // `Plain` so the writer emits `<img class="r-stretch">` as a
            // *direct child* of the `<section>`. reveal sizes `.r-stretch` only
            // via `section > .r-stretch` (direct child); a `<p>` wrapper makes
            // it inert.
            Block::Paragraph(p) if p.content.len() == 1 => {
                if let Inline::Image(image) = &mut p.content[0] {
                    match decide_stretch(image, auto_stretch) {
                        StretchOutcome::LeaveWrapped => Plan::LeaveWrapped,
                        StretchOutcome::Unwrap => {
                            let source_info = p.source_info.clone();
                            let img = p.content.remove(0);
                            Plan::Replace(vec![Block::Plain(Plain {
                                content: vec![img],
                                source_info,
                            })])
                        }
                    }
                } else {
                    Plan::Skip
                }
            }

            // `Figure` (a captioned `![caption](x)`). When stretched, hoist the
            // image to section level (`Plain[Image]`, figure `id` transferred
            // onto the img) and re-emit the caption as a sibling
            // `Paragraph[…, Attr{.caption}]` → `<p class="caption">`. This is
            // the AST equivalent of Q1's `applyStretch` figure branch — done in
            // the AST, not a DOM postprocessor (bd-38ioql41).
            Block::Figure(f) if count_figure_images(f) == 1 => {
                let outcome = match figure_image_mut(f) {
                    Some(image) => decide_stretch(image, auto_stretch),
                    None => StretchOutcome::LeaveWrapped,
                };
                match outcome {
                    StretchOutcome::LeaveWrapped => Plan::LeaveWrapped,
                    StretchOutcome::Unwrap => Plan::Replace(hoist_figure(f, "")),
                }
            }

            // Float-figure wrapper (bd-hcp8m3ve): the crossref renderer emits
            // `Div(.quarto-figure) > Figure > Div(aria-describedby) [content]`
            // for HTML-family formats, reveal included. Treat it exactly like
            // the bare `Figure` case, transferring the *outer* div's crossref
            // id (that's where it lives in the float shape) onto the hoisted
            // image so `@fig-id` anchors keep resolving.
            Block::Div(d) if is_float_figure_div(d) => {
                let outer_id = d.attr.0.clone();
                let Some(Block::Figure(f)) = d.content.first_mut() else {
                    unreachable!("is_float_figure_div guarantees a Figure child");
                };
                if count_figure_images(f) != 1 {
                    Plan::Skip
                } else {
                    let outcome = match figure_image_mut(f) {
                        Some(image) => decide_stretch(image, auto_stretch),
                        None => StretchOutcome::LeaveWrapped,
                    };
                    match outcome {
                        StretchOutcome::LeaveWrapped => Plan::LeaveWrapped,
                        StretchOutcome::Unwrap => Plan::Replace(hoist_figure(f, &outer_id)),
                    }
                }
            }

            _ => Plan::Skip,
        };

        match plan {
            Plan::Skip => continue,
            Plan::LeaveWrapped => return,
            Plan::Replace(blocks) => {
                div.content.splice(idx..=idx, blocks);
                return;
            }
        }
    }
}

/// The crossref float wrapper: a `Div` with the `quarto-figure` class whose
/// sole child is a `Figure` (bd-hcp8m3ve float shape). Layout/column divs
/// don't match (no `quarto-figure` class), so nested-image opt-outs behave
/// as before.
fn is_float_figure_div(d: &Div) -> bool {
    d.attr.1.iter().any(|c| c == "quarto-figure")
        && d.content.len() == 1
        && matches!(d.content[0], Block::Figure(_))
}

/// Build the hoisted replacement for a stretched captioned `Figure`: a
/// `Plain[Image]` (the figure's lone image — now carrying `.r-stretch` — with
/// the figure `id` transferred onto it) followed, when the figure has a
/// caption, by a `Paragraph` whose trailing inline is an `Inline::Attr{.caption}`
/// so the HTML writer emits `<p class="caption">` (capability bd-itqcfxc3).
///
/// The `<figure>` element itself is discarded — its image becomes a direct
/// child of the slide `<section>` so reveal's `section > .r-stretch` selector
/// matches it.
fn hoist_figure(f: &mut Figure, fallback_id: &str) -> Vec<Block> {
    let mut image = figure_image_mut(f)
        .expect("figure has exactly one image (gated by count_figure_images)")
        .clone();
    // Q1 parity: move the figure id onto the image so an `@fig-id` anchor
    // still resolves once the `<figure>` wrapper is gone. In the float shape
    // (bd-hcp8m3ve) the id lives on the outer wrapper div — the caller passes
    // it as `fallback_id`.
    if !f.attr.0.is_empty() {
        image.attr.0 = f.attr.0.clone();
    } else if !fallback_id.is_empty() {
        image.attr.0 = fallback_id.to_string();
    }
    let img_source = image.source_info.clone();
    let mut blocks = vec![Block::Plain(Plain {
        content: vec![Inline::Image(image)],
        source_info: img_source,
    })];

    if let Some(caption) = figure_caption_inlines(f) {
        blocks.push(caption_paragraph(caption));
    }
    blocks
}

/// Flatten a figure's caption (`caption.long`, typically `[Plain|Para[inlines]]`)
/// into one inline sequence. Returns `None` when the figure has no caption.
fn figure_caption_inlines(f: &Figure) -> Option<Vec<Inline>> {
    let blocks = f.caption.long.as_ref()?;
    let mut out = Vec::new();
    for b in blocks {
        match b {
            Block::Plain(p) => out.extend(p.content.iter().cloned()),
            Block::Paragraph(p) => out.extend(p.content.iter().cloned()),
            _ => {}
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Wrap caption inlines in a `Paragraph` carrying a trailing `Inline::Attr`
/// with the `caption` class. The block writers collect that trailing attr and
/// emit `<p class="caption">…</p>` (bd-itqcfxc3 / `block_attr.rs`).
fn caption_paragraph(mut content: Vec<Inline>) -> Block {
    let caption_attr: Attr = (
        String::new(),
        vec!["caption".to_string()],
        LinkedHashMap::new(),
    );
    content.push(Inline::Attr(InlineAttr::new(
        caption_attr,
        AttrSourceInfo::empty(),
        SourceInfo::generated(By::revealjs()),
    )));
    Block::Paragraph(Paragraph {
        content,
        source_info: SourceInfo::generated(By::revealjs()),
    })
}

/// What to do with the standalone block holding the lone image.
enum StretchOutcome {
    /// The image carries (or just gained) a stretch class — its container
    /// should be unwrapped so the image becomes a direct child of the section.
    Unwrap,
    /// An opt-out fired, or auto-stretch is off and the image had no stretch
    /// class: leave the container untouched.
    LeaveWrapped,
}

/// Apply the per-image opt-outs and, when eligible, add `.r-stretch`. Returns
/// whether the container should be unwrapped (i.e. the image ends up stretched).
///
/// Opt-outs mirror Q1's `applyStretch`: `.nostretch` (consumed), `.absolute`,
/// and explicit `width`/`height` sizing leave the image alone. An image that
/// *already* carries `.stretch`/`.r-stretch` (author-supplied, or an idempotent
/// re-run) is still unwrapped — Q1 hoists any stretched image that isn't yet a
/// direct child of the section, independent of who added the class.
fn decide_stretch(image: &mut Image, auto_stretch: bool) -> StretchOutcome {
    if let Some(pos) = image.attr.1.iter().position(|c| c == "nostretch") {
        image.attr.1.remove(pos);
        return StretchOutcome::LeaveWrapped;
    }
    if image.attr.1.iter().any(|c| c == "absolute") {
        return StretchOutcome::LeaveWrapped;
    }
    if has_explicit_size(image) {
        return StretchOutcome::LeaveWrapped;
    }
    let already_stretched = image
        .attr
        .1
        .iter()
        .any(|c| c == STRETCH_CLASS || c == "stretch");
    if !already_stretched {
        if !auto_stretch {
            return StretchOutcome::LeaveWrapped;
        }
        image.attr.1.push(STRETCH_CLASS.to_string());
    }
    StretchOutcome::Unwrap
}

/// Whether the block tree contains a peripheral `.aside` div (a coalesced
/// footnote block or an author `.aside`). Speaker `.notes` do not count.
fn contains_aside(blocks: &[Block]) -> bool {
    blocks.iter().any(|b| match b {
        Block::Div(d) if d.attr.1.iter().any(|c| c == "aside") => true,
        Block::Div(d) => contains_aside(&d.content),
        Block::Figure(f) => contains_aside(&f.content),
        _ => false,
    })
}

/// Count `Image` inlines across the slide, skipping `.notes`/`.aside` divs
/// (their images are peripheral and should not gate auto-stretch).
fn count_images(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(p) => image_count(&p.content),
            Block::Plain(p) => image_count(&p.content),
            Block::Figure(f) => count_images(&f.content),
            Block::Div(d) if d.attr.1.iter().any(|c| c == "notes" || c == "aside") => 0,
            Block::Div(d) => count_images(&d.content),
            _ => 0,
        })
        .sum()
}

/// Count `Image` inlines directly inside a figure's `Paragraph`/`Plain` blocks.
fn count_figure_images(f: &Figure) -> usize {
    fn count(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(p) => image_count(&p.content),
                Block::Plain(p) => image_count(&p.content),
                // The float shape (bd-hcp8m3ve) wraps figure content in an
                // aria-describedby div — descend through Divs.
                Block::Div(d) => count(&d.content),
                _ => 0,
            })
            .sum()
    }
    count(&f.content)
}

fn image_count(inlines: &[Inline]) -> usize {
    inlines
        .iter()
        .filter(|i| matches!(i, Inline::Image(_)))
        .count()
}

/// First `Image` mutable ref inside a figure's `Paragraph`/`Plain` blocks,
/// descending through Divs (the float shape's aria wrapper — bd-hcp8m3ve).
fn figure_image_mut(f: &mut Figure) -> Option<&mut Image> {
    // Two-phase find: locate the image's block path immutably, then descend
    // mutably along it (sidesteps the recursive-&mut-return borrowck limit).
    fn find_path(blocks: &[Block], path: &mut Vec<usize>) -> Option<usize> {
        for (i, b) in blocks.iter().enumerate() {
            match b {
                Block::Paragraph(p) => {
                    if let Some(j) = p.content.iter().position(|x| matches!(x, Inline::Image(_))) {
                        path.push(i);
                        return Some(j);
                    }
                }
                Block::Plain(p) => {
                    if let Some(j) = p.content.iter().position(|x| matches!(x, Inline::Image(_))) {
                        path.push(i);
                        return Some(j);
                    }
                }
                Block::Div(d) => {
                    path.push(i);
                    if let Some(j) = find_path(&d.content, path) {
                        return Some(j);
                    }
                    path.pop();
                }
                _ => {}
            }
        }
        None
    }
    let mut path = Vec::new();
    let inline_idx = find_path(&f.content, &mut path)?;
    let mut blocks: &mut Vec<Block> = &mut f.content;
    let (last, dirs) = path.split_last().expect("non-empty path");
    for &i in dirs {
        let Block::Div(d) = &mut blocks[i] else {
            return None;
        };
        blocks = &mut d.content;
    }
    let inlines = match &mut blocks[*last] {
        Block::Paragraph(p) => &mut p.content,
        Block::Plain(p) => &mut p.content,
        _ => return None,
    };
    match &mut inlines[inline_idx] {
        Inline::Image(img) => Some(img),
        _ => None,
    }
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

    /// A `Figure` with an `id` and a `caption.long` of `[Plain[caption]]`,
    /// mirroring what `![caption](x)` parses to.
    fn figure_with_caption(blocks: Vec<Block>, caption: Vec<Inline>, id: &str) -> Block {
        Block::Figure(Figure {
            attr: (id.to_string(), vec![], LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: Some(vec![Block::Plain(Plain {
                    content: caption,
                    source_info: si(),
                })]),
                source_info: si(),
            },
            content: blocks,
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn caption_inlines() -> Vec<Inline> {
        vec![Inline::Str(Str {
            text: "Cap".to_string(),
            source_info: si(),
        })]
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

    /// The kind ("Plain"/"Paragraph"/"Figure"/"Div") of the first section's
    /// top-level block that directly holds an `Image` inline, after the walk.
    /// Used to assert the container is unwrapped to `Plain` when stretched so
    /// the writer emits `section > img` (not `section > p > img`).
    fn container_kind(mut blocks: Vec<Block>, auto_stretch: bool) -> String {
        walk_sections(&mut blocks, auto_stretch);
        let Block::Div(div) = &blocks[0] else {
            panic!("expected section");
        };
        fn holds_image(inlines: &[Inline]) -> bool {
            inlines.iter().any(|i| matches!(i, Inline::Image(_)))
        }
        for b in &div.content {
            let kind = match b {
                Block::Plain(p) if holds_image(&p.content) => "Plain",
                Block::Paragraph(p) if holds_image(&p.content) => "Paragraph",
                Block::Figure(_) => "Figure",
                _ => continue,
            };
            return kind.to_string();
        }
        panic!("no top-level block holds an image");
    }

    #[test]
    fn lone_paragraph_image_becomes_plain() {
        // When the lone image is stretched, its `Paragraph` container is
        // unwrapped to a `Plain` so the writer emits `section > img.r-stretch`
        // (reveal's `section > .r-stretch` selector needs a direct child).
        let blocks = vec![section(&[], vec![header(), para(vec![image(&[], &[])])])];
        assert_eq!(container_kind(blocks, true), "Plain");
    }

    #[test]
    fn non_stretched_image_keeps_paragraph() {
        // With auto-stretch disabled the image is not stretched, so we must not
        // disturb its container — it stays a `Paragraph` (renders as `<p>`).
        let blocks = vec![section(&[], vec![header(), para(vec![image(&[], &[])])])];
        assert_eq!(container_kind(blocks, false), "Paragraph");
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
    fn lone_figure_becomes_plain_image_plus_caption() {
        // A captioned single-image Figure is hoisted: the Figure is replaced
        // by a `Plain[Image]` (with `.r-stretch`) followed by a caption
        // `Paragraph` whose trailing inline is an `Inline::Attr{.caption}` —
        // so the writer emits `section > img.r-stretch` + `<p class="caption">`.
        let mut blocks = vec![section(
            &[],
            vec![
                header(),
                figure_with_caption(vec![para(vec![image(&[], &[])])], caption_inlines(), ""),
            ],
        )];
        walk_sections(&mut blocks, true);
        let Block::Div(div) = &blocks[0] else {
            panic!("expected section");
        };
        // The Figure is gone.
        assert!(
            !div.content.iter().any(|b| matches!(b, Block::Figure(_))),
            "figure should be unwrapped, got: {:?}",
            div.content
        );
        // A `Plain` holds the stretched image.
        let img = div
            .content
            .iter()
            .find_map(|b| match b {
                Block::Plain(p) => p.content.iter().find_map(|i| match i {
                    Inline::Image(img) => Some(img),
                    _ => None,
                }),
                _ => None,
            })
            .expect("Plain[Image] present");
        assert!(
            img.attr.1.iter().any(|c| c == "r-stretch"),
            "hoisted image must be stretched"
        );
        // A caption `Paragraph` ends in `Inline::Attr{.caption}`.
        let cap_para = div
            .content
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => Some(p),
                _ => None,
            })
            .expect("caption Paragraph present");
        match cap_para.content.last() {
            Some(Inline::Attr(a)) => assert!(
                a.attr.1.iter().any(|c| c == "caption"),
                "trailing Inline::Attr must carry the `caption` class"
            ),
            other => panic!("expected trailing Inline::Attr, got {other:?}"),
        }
    }

    #[test]
    fn figure_id_transferred_onto_image() {
        // Q1 parity: the figure `id` moves onto the hoisted `<img>` so an
        // `@fig-id` anchor still resolves.
        let mut blocks = vec![section(
            &[],
            vec![figure_with_caption(
                vec![para(vec![image(&[], &[])])],
                caption_inlines(),
                "fig-x",
            )],
        )];
        walk_sections(&mut blocks, true);
        let Block::Div(div) = &blocks[0] else {
            panic!("expected section");
        };
        let img = div
            .content
            .iter()
            .find_map(|b| match b {
                Block::Plain(p) => p.content.iter().find_map(|i| match i {
                    Inline::Image(img) => Some(img),
                    _ => None,
                }),
                _ => None,
            })
            .expect("image present");
        assert_eq!(img.attr.0, "fig-x", "figure id must move onto the image");
    }

    #[test]
    fn nostretch_figure_left_intact() {
        // A `.nostretch` figure is not stretched, so it must not be unwrapped.
        let mut blocks = vec![section(
            &[],
            vec![figure_with_caption(
                vec![para(vec![image(&["nostretch"], &[])])],
                caption_inlines(),
                "",
            )],
        )];
        walk_sections(&mut blocks, true);
        let Block::Div(div) = &blocks[0] else {
            panic!("expected section");
        };
        assert!(
            div.content.iter().any(|b| matches!(b, Block::Figure(_))),
            "a nostretch figure must stay a Figure"
        );
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
    fn lone_image_with_sibling_text_stretches() {
        // heading + explanatory paragraph + a standalone image → the lone image
        // still stretches (matches Q1: one image, in its own block, amid text).
        let blocks = vec![section(
            &[],
            vec![
                header(),
                para(vec![Inline::Str(Str {
                    text: "Here is our architecture:".to_string(),
                    source_info: si(),
                })]),
                para(vec![image(&[], &[])]),
            ],
        )];
        assert!(stretch_classes(blocks, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn two_images_skipped() {
        let blocks = vec![section(
            &[],
            vec![
                header(),
                para(vec![image(&[], &[])]),
                para(vec![image(&[], &[])]),
            ],
        )];
        assert!(!stretch_classes(blocks, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn slide_with_aside_skipped() {
        // A peripheral aside (e.g. a coalesced footnote) suppresses auto-stretch,
        // matching Q1 (`aside:not(.notes)`).
        let aside = Block::Div(Div {
            attr: (
                String::new(),
                vec!["aside".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![para(vec![Inline::Str(Str {
                text: "note".to_string(),
                source_info: si(),
            })])],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let blocks = vec![section(
            &[],
            vec![header(), para(vec![image(&[], &[])]), aside],
        )];
        assert!(!stretch_classes(blocks, true).contains(&"r-stretch".to_string()));
    }

    #[test]
    fn image_nested_in_column_div_skipped() {
        // A single image inside a `.column` is not a top-level standalone block,
        // so it is left alone (Q1 screens out layout/column/fragment parents).
        let column = Block::Div(Div {
            attr: (
                String::new(),
                vec!["column".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![para(vec![image(&[], &[])])],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let blocks = vec![section(&[], vec![header(), column])];
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
