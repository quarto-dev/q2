/*
 * transforms/image_walk.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! One mutable walk over every authored `Image` in a block tree.
//!
//! Shared by the transforms that act on body images late in the
//! Finalization Phase — `responsive-image` (tags them `img-fluid`) and
//! `hephaestus-render` (swaps `.hep` targets for rendered SVG) — so the
//! set of containers that get descended into is defined once. The
//! coverage matches `link_rewrite`'s walker: captions on figures and
//! tables, table cells, custom-node slots, footnote bodies and the
//! alt-text inlines of an image itself (a filter pass can leave an image
//! inside another's alt text).
//!
//! Deliberately not walked: `CodeBlock` / `RawBlock` / `HorizontalRule` /
//! `BlockMetadata` are true leaves. The two `NoteDefinition*` block
//! variants carry content, but `FootnotesTransform` has lifted every
//! reachable definition into a trailing `Div#footnotes` by the time the
//! Finalization Phase runs, so anything still inside one is unreferenced
//! and never rendered. `Cite` and `EditComment` inlines carry generated
//! citation text and editorial markup respectively — neither is authored
//! image content.

use quarto_pandoc_types::Slot;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{Image, Inline, Inlines};

/// Call `f` on every `Image` reachable from `blocks`, in document
/// order. The callback runs before the image's own alt-text inlines
/// are descended into.
pub(crate) fn for_each_image_mut(blocks: &mut [Block], f: &mut dyn FnMut(&mut Image)) {
    visit_blocks(blocks, f);
}

fn visit_blocks(blocks: &mut [Block], f: &mut dyn FnMut(&mut Image)) {
    for block in blocks.iter_mut() {
        visit_block(block, f);
    }
}

fn visit_block(block: &mut Block, f: &mut dyn FnMut(&mut Image)) {
    match block {
        Block::Plain(p) => visit_inlines(&mut p.content, f),
        Block::Paragraph(p) => visit_inlines(&mut p.content, f),
        Block::LineBlock(lb) => {
            for line in lb.content.iter_mut() {
                visit_inlines(line, f);
            }
        }
        Block::BlockQuote(bq) => visit_blocks(&mut bq.content, f),
        Block::OrderedList(ol) => {
            for item in ol.content.iter_mut() {
                visit_blocks(item, f);
            }
        }
        Block::BulletList(bl) => {
            for item in bl.content.iter_mut() {
                visit_blocks(item, f);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in dl.content.iter_mut() {
                visit_inlines(term, f);
                for def in defs.iter_mut() {
                    visit_blocks(def, f);
                }
            }
        }
        Block::Header(h) => visit_inlines(&mut h.content, f),
        Block::Div(d) => visit_blocks(&mut d.content, f),
        Block::Figure(fig) => {
            visit_blocks(&mut fig.content, f);
            if let Some(short) = fig.caption.short.as_mut() {
                visit_inlines(short, f);
            }
            if let Some(long) = fig.caption.long.as_mut() {
                visit_blocks(long, f);
            }
        }
        Block::Table(t) => {
            if let Some(short) = t.caption.short.as_mut() {
                visit_inlines(short, f);
            }
            if let Some(long) = t.caption.long.as_mut() {
                visit_blocks(long, f);
            }
            for row in t.head.rows.iter_mut().chain(t.foot.rows.iter_mut()) {
                for cell in row.cells.iter_mut() {
                    visit_blocks(&mut cell.content, f);
                }
            }
            for body in t.bodies.iter_mut() {
                for row in body.head.iter_mut().chain(body.body.iter_mut()) {
                    for cell in row.cells.iter_mut() {
                        visit_blocks(&mut cell.content, f);
                    }
                }
            }
        }
        Block::CaptionBlock(cb) => visit_inlines(&mut cb.content, f),
        Block::Custom(c) => {
            for (_name, slot) in c.slots.iter_mut() {
                visit_slot(slot, f);
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

fn visit_inlines(inlines: &mut Inlines, f: &mut dyn FnMut(&mut Image)) {
    for inline in inlines.iter_mut() {
        visit_inline(inline, f);
    }
}

fn visit_inline(inline: &mut Inline, f: &mut dyn FnMut(&mut Image)) {
    match inline {
        Inline::Image(img) => {
            f(img);
            visit_inlines(&mut img.content, f);
        }
        Inline::Link(l) => visit_inlines(&mut l.content, f),
        Inline::Emph(e) => visit_inlines(&mut e.content, f),
        Inline::Underline(u) => visit_inlines(&mut u.content, f),
        Inline::Strong(s) => visit_inlines(&mut s.content, f),
        Inline::Strikeout(s) => visit_inlines(&mut s.content, f),
        Inline::Superscript(s) => visit_inlines(&mut s.content, f),
        Inline::Subscript(s) => visit_inlines(&mut s.content, f),
        Inline::SmallCaps(s) => visit_inlines(&mut s.content, f),
        Inline::Quoted(q) => visit_inlines(&mut q.content, f),
        Inline::Note(n) => visit_blocks(&mut n.content, f),
        Inline::Span(s) => visit_inlines(&mut s.content, f),
        Inline::Insert(i) => visit_inlines(&mut i.content, f),
        Inline::Delete(d) => visit_inlines(&mut d.content, f),
        Inline::Highlight(h) => visit_inlines(&mut h.content, f),
        Inline::Custom(c) => {
            for (_name, slot) in c.slots.iter_mut() {
                visit_slot(slot, f);
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

fn visit_slot(slot: &mut Slot, f: &mut dyn FnMut(&mut Image)) {
    match slot {
        Slot::Block(b) => visit_block(b, f),
        Slot::Blocks(bs) => visit_blocks(bs, f),
        Slot::Inline(i) => visit_inline(i, f),
        Slot::Inlines(is) => visit_inlines(is, f),
    }
}
