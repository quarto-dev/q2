/*
 * ast_walk.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Mutable traversal of every inline in a document.
//!
//! Several post-filter stages need to visit each `Inline` wherever it
//! sits — paragraph, header, list item, table cell, caption, footnote, or
//! a custom node's slots — and either edit it in place or replace it.
//! This is that walk, written once. It is pre-order: `f` sees a node
//! before the walk descends into the node's own content, so a
//! replacement's children are visited too.

use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::custom::Slot;
use quarto_pandoc_types::inline::{Inline, Inlines};

/// Call `f` on every inline under `blocks`, in document order.
pub fn for_each_inline_mut(blocks: &mut [Block], f: &mut dyn FnMut(&mut Inline)) {
    for block in blocks.iter_mut() {
        visit_block(block, f);
    }
}

fn visit_block(block: &mut Block, f: &mut dyn FnMut(&mut Inline)) {
    match block {
        Block::Plain(p) => visit_inlines(&mut p.content, f),
        Block::Paragraph(p) => visit_inlines(&mut p.content, f),
        Block::LineBlock(lb) => {
            for line in lb.content.iter_mut() {
                visit_inlines(line, f);
            }
        }
        Block::BlockQuote(bq) => for_each_inline_mut(&mut bq.content, f),
        Block::OrderedList(ol) => {
            for item in ol.content.iter_mut() {
                for_each_inline_mut(item, f);
            }
        }
        Block::BulletList(bl) => {
            for item in bl.content.iter_mut() {
                for_each_inline_mut(item, f);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in dl.content.iter_mut() {
                visit_inlines(term, f);
                for def in defs.iter_mut() {
                    for_each_inline_mut(def, f);
                }
            }
        }
        Block::Header(h) => visit_inlines(&mut h.content, f),
        Block::Div(d) => for_each_inline_mut(&mut d.content, f),
        Block::Figure(fig) => {
            if let Some(short) = fig.caption.short.as_mut() {
                visit_inlines(short, f);
            }
            if let Some(long) = fig.caption.long.as_mut() {
                for_each_inline_mut(long, f);
            }
            for_each_inline_mut(&mut fig.content, f);
        }
        Block::Table(t) => {
            if let Some(short) = t.caption.short.as_mut() {
                visit_inlines(short, f);
            }
            if let Some(long) = t.caption.long.as_mut() {
                for_each_inline_mut(long, f);
            }
            for row in t.head.rows.iter_mut().chain(t.foot.rows.iter_mut()) {
                for cell in row.cells.iter_mut() {
                    for_each_inline_mut(&mut cell.content, f);
                }
            }
            for body in t.bodies.iter_mut() {
                for row in body.head.iter_mut().chain(body.body.iter_mut()) {
                    for cell in row.cells.iter_mut() {
                        for_each_inline_mut(&mut cell.content, f);
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

fn visit_inlines(inlines: &mut Inlines, f: &mut dyn FnMut(&mut Inline)) {
    for inline in inlines.iter_mut() {
        visit_inline(inline, f);
    }
}

fn visit_inline(inline: &mut Inline, f: &mut dyn FnMut(&mut Inline)) {
    f(inline);
    match inline {
        Inline::Emph(e) => visit_inlines(&mut e.content, f),
        Inline::Underline(u) => visit_inlines(&mut u.content, f),
        Inline::Strong(s) => visit_inlines(&mut s.content, f),
        Inline::Strikeout(s) => visit_inlines(&mut s.content, f),
        Inline::Superscript(s) => visit_inlines(&mut s.content, f),
        Inline::Subscript(s) => visit_inlines(&mut s.content, f),
        Inline::SmallCaps(s) => visit_inlines(&mut s.content, f),
        Inline::Quoted(q) => visit_inlines(&mut q.content, f),
        Inline::Link(l) => visit_inlines(&mut l.content, f),
        Inline::Image(i) => visit_inlines(&mut i.content, f),
        Inline::Span(s) => visit_inlines(&mut s.content, f),
        Inline::Note(n) => for_each_inline_mut(&mut n.content, f),
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

fn visit_slot(slot: &mut Slot, f: &mut dyn FnMut(&mut Inline)) {
    match slot {
        Slot::Block(b) => visit_block(b, f),
        Slot::Blocks(bs) => for_each_inline_mut(bs, f),
        Slot::Inline(i) => visit_inline(i, f),
        Slot::Inlines(is) => visit_inlines(is, f),
    }
}
