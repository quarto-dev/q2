/*
 * transforms/image_walk.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! One mutable walk over every authored `Image` in a block tree.
//!
//! Shared by the transforms that act on body images — `image-alt-normalize`
//! (folds `fig-alt` into the alt text, end of the Normalization Phase),
//! `responsive-image` (tags them `img-fluid`) and `hephaestus-render`
//! (swaps `.hep` targets for rendered SVG), both late in the Finalization
//! Phase — so the set of containers that get descended into is defined
//! once. The coverage matches `link_rewrite`'s walker: captions on figures
//! and tables, table cells, custom-node slots, footnote bodies and the
//! alt-text inlines of an image itself (a filter pass can leave an image
//! inside another's alt text).
//!
//! Deliberately not walked: `CodeBlock` / `RawBlock` / `HorizontalRule` /
//! `BlockMetadata` are true leaves. The two `NoteDefinition*` block
//! variants carry content, but `FootnotesTransform` (early in the
//! Normalization Phase, ahead of every caller) has lifted every reachable
//! definition into a trailing `Div#footnotes`, so anything still inside one
//! is unreferenced and never rendered. `Cite` and `EditComment` inlines
//! carry generated citation text and editorial markup respectively —
//! neither is authored image content.

use quarto_pandoc_types::Slot;
use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::custom::CustomNode;
use quarto_pandoc_types::inline::{Image, Inline, Inlines};

use crate::crossref::FLOAT_REF_TARGET;

/// Call `f` on every `Image` reachable from `blocks`, in document
/// order. The callback runs before the image's own alt-text inlines
/// are descended into.
pub(crate) fn for_each_image_mut(blocks: &mut [Block], f: &mut dyn FnMut(&mut Image)) {
    let mut on_image = |img: &mut Image, _: ImageContext<'_>| f(img);
    let mut walker = Walker {
        on_image: &mut on_image,
        on_float_exit: None,
        standalone_pending: false,
    };
    walker.visit_blocks(blocks, None);
}

/// Where an image sits, for the passes that care.
#[derive(Clone, Copy)]
pub(crate) struct ImageContext<'a> {
    /// The `FloatRefTarget` the image belongs to, if any.
    pub float: Option<FloatScope<'a>>,
    /// The image is the only inline of a `Para`, directly or as the only
    /// inline of the paragraph's only `Link` — the two shapes Quarto 1's
    /// `discoverFigure` / `discoverLinkedFigure` read as an implicit figure
    /// when the image has alt text.
    pub standalone: bool,
}

/// The `FloatRefTarget` an image belongs to: the nearest enclosing float
/// whose `content` slot holds the image. Images in a float's caption slots
/// have no scope, and a nested float starts a scope of its own rather than
/// inheriting its parent's — a subfigure belongs to the subfigure.
#[derive(Clone, Copy)]
pub(crate) struct FloatScope<'a> {
    pub attr: &'a Attr,
    pub plain_data: &'a serde_json::Value,
}

/// [`for_each_image_mut`], plus context: `on_image` also receives the
/// image's [`ImageContext`], and `on_float_exit` runs on every
/// `FloatRefTarget` after its whole subtree has been visited (so a float
/// may consume attributes its images were just given in scope).
pub(crate) fn for_each_image_with_context_mut(
    blocks: &mut [Block],
    on_image: &mut dyn FnMut(&mut Image, ImageContext<'_>),
    on_float_exit: &mut dyn FnMut(&mut CustomNode),
) {
    let mut walker = Walker {
        on_image,
        on_float_exit: Some(on_float_exit),
        standalone_pending: false,
    };
    walker.visit_blocks(blocks, None);
}

/// `Para [Image]` or `Para [Link [Image]]`.
fn is_standalone_shape(inlines: &[Inline]) -> bool {
    match inlines {
        [Inline::Image(_)] => true,
        [Inline::Link(link)] => matches!(link.content.as_slice(), [Inline::Image(_)]),
        _ => false,
    }
}

struct Walker<'f> {
    on_image: &'f mut dyn FnMut(&mut Image, ImageContext<'_>),
    on_float_exit: Option<&'f mut dyn FnMut(&mut CustomNode)>,
    /// Set while entering a standalone-shaped paragraph; the first image
    /// visited — by the shape, the paragraph's only image — takes it.
    standalone_pending: bool,
}

impl Walker<'_> {
    fn visit_blocks(&mut self, blocks: &mut [Block], scope: Option<FloatScope<'_>>) {
        for block in blocks.iter_mut() {
            self.visit_block(block, scope);
        }
    }

    fn visit_block(&mut self, block: &mut Block, scope: Option<FloatScope<'_>>) {
        match block {
            Block::Plain(p) => self.visit_inlines(&mut p.content, scope),
            Block::Paragraph(p) => {
                self.standalone_pending = is_standalone_shape(&p.content);
                self.visit_inlines(&mut p.content, scope);
                self.standalone_pending = false;
            }
            Block::LineBlock(lb) => {
                for line in lb.content.iter_mut() {
                    self.visit_inlines(line, scope);
                }
            }
            Block::BlockQuote(bq) => self.visit_blocks(&mut bq.content, scope),
            Block::OrderedList(ol) => {
                for item in ol.content.iter_mut() {
                    self.visit_blocks(item, scope);
                }
            }
            Block::BulletList(bl) => {
                for item in bl.content.iter_mut() {
                    self.visit_blocks(item, scope);
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in dl.content.iter_mut() {
                    self.visit_inlines(term, scope);
                    for def in defs.iter_mut() {
                        self.visit_blocks(def, scope);
                    }
                }
            }
            Block::Header(h) => self.visit_inlines(&mut h.content, scope),
            Block::Div(d) => self.visit_blocks(&mut d.content, scope),
            Block::Figure(fig) => {
                self.visit_blocks(&mut fig.content, scope);
                if let Some(short) = fig.caption.short.as_mut() {
                    self.visit_inlines(short, scope);
                }
                if let Some(long) = fig.caption.long.as_mut() {
                    self.visit_blocks(long, scope);
                }
            }
            Block::Table(t) => {
                if let Some(short) = t.caption.short.as_mut() {
                    self.visit_inlines(short, scope);
                }
                if let Some(long) = t.caption.long.as_mut() {
                    self.visit_blocks(long, scope);
                }
                for row in t.head.rows.iter_mut().chain(t.foot.rows.iter_mut()) {
                    for cell in row.cells.iter_mut() {
                        self.visit_blocks(&mut cell.content, scope);
                    }
                }
                for body in t.bodies.iter_mut() {
                    for row in body.head.iter_mut().chain(body.body.iter_mut()) {
                        for cell in row.cells.iter_mut() {
                            self.visit_blocks(&mut cell.content, scope);
                        }
                    }
                }
            }
            Block::CaptionBlock(cb) => self.visit_inlines(&mut cb.content, scope),
            Block::Custom(c) if c.type_name == FLOAT_REF_TARGET => {
                {
                    let CustomNode {
                        slots,
                        attr,
                        plain_data,
                        ..
                    } = &mut **c;
                    let own = FloatScope {
                        attr: &*attr,
                        plain_data: &*plain_data,
                    };
                    for (name, slot) in slots.iter_mut() {
                        let slot_scope = (name == "content").then_some(own);
                        self.visit_slot(slot, slot_scope);
                    }
                }
                if let Some(exit) = self.on_float_exit.as_mut() {
                    exit(c);
                }
            }
            Block::Custom(c) => {
                for (_name, slot) in c.slots.iter_mut() {
                    self.visit_slot(slot, scope);
                }
            }
            Block::CodeBlock(_)
            | Block::RawBlock(_)
            | Block::HorizontalRule(_)
            | Block::BlockMetadata(_)
            | Block::NoteDefinitionPara(_)
            | Block::NoteDefinitionFencedBlock(_) => {}
        }
    }

    fn visit_inlines(&mut self, inlines: &mut Inlines, scope: Option<FloatScope<'_>>) {
        for inline in inlines.iter_mut() {
            self.visit_inline(inline, scope);
        }
    }

    fn visit_inline(&mut self, inline: &mut Inline, scope: Option<FloatScope<'_>>) {
        match inline {
            Inline::Image(img) => {
                let context = ImageContext {
                    float: scope,
                    standalone: std::mem::take(&mut self.standalone_pending),
                };
                (self.on_image)(img, context);
                self.visit_inlines(&mut img.content, scope);
            }
            Inline::Link(l) => self.visit_inlines(&mut l.content, scope),
            Inline::Emph(e) => self.visit_inlines(&mut e.content, scope),
            Inline::Underline(u) => self.visit_inlines(&mut u.content, scope),
            Inline::Strong(s) => self.visit_inlines(&mut s.content, scope),
            Inline::Strikeout(s) => self.visit_inlines(&mut s.content, scope),
            Inline::Superscript(s) => self.visit_inlines(&mut s.content, scope),
            Inline::Subscript(s) => self.visit_inlines(&mut s.content, scope),
            Inline::SmallCaps(s) => self.visit_inlines(&mut s.content, scope),
            Inline::Quoted(q) => self.visit_inlines(&mut q.content, scope),
            Inline::Note(n) => self.visit_blocks(&mut n.content, scope),
            Inline::Span(s) => self.visit_inlines(&mut s.content, scope),
            Inline::Insert(i) => self.visit_inlines(&mut i.content, scope),
            Inline::Delete(d) => self.visit_inlines(&mut d.content, scope),
            Inline::Highlight(h) => self.visit_inlines(&mut h.content, scope),
            Inline::Custom(c) => {
                for (_name, slot) in c.slots.iter_mut() {
                    self.visit_slot(slot, scope);
                }
            }
            Inline::Str(_)
            | Inline::Cite(_)
            | Inline::Code(_)
            | Inline::Space(_)
            | Inline::SoftBreak(_)
            | Inline::LineBreak(_)
            | Inline::Math(_)
            | Inline::RawInline(_)
            | Inline::Shortcode(_)
            | Inline::NoteReference(_)
            | Inline::Attr(_)
            | Inline::EditComment(_) => {}
        }
    }

    fn visit_slot(&mut self, slot: &mut Slot, scope: Option<FloatScope<'_>>) {
        match slot {
            Slot::Block(b) => self.visit_block(b, scope),
            Slot::Blocks(bs) => self.visit_blocks(bs, scope),
            Slot::Inline(i) => self.visit_inline(i, scope),
            Slot::Inlines(is) => self.visit_inlines(is, scope),
        }
    }
}
