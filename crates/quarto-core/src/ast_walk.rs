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

// ---------------------------------------------------------------------------
// List-level walks
// ---------------------------------------------------------------------------
//
// `for_each_inline_mut` hands out one inline at a time. Transforms that splice (wrap a
// slice of siblings, remove a marker, pair two siblings) need the containing `Vec`, so
// these walks hand out each inline list, and each block list, instead. Both are
// post-order: a list's elements (and everything under them) are visited before the list.
// They cover every place `for_each_inline_mut` does, plus `Cite` content and citation
// prefixes/suffixes, and the metadata walk reaches `PandocInlines` / `PandocBlocks` values.

use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};

/// A visitor of the lists directly under one node. [`block_children_mut`] and
/// [`inline_children_mut`] hand it each child list of a node (one level down) and leave the
/// recursion, and its order, to the visitor: the post-order list walks below recurse first
/// and call back after; a top-down rewrite (P6's editorial-marks export) decides per list.
pub trait ListVisitor {
    fn inlines(&mut self, list: &mut Inlines);
    fn blocks(&mut self, list: &mut Vec<Block>);
}

/// Hand `v` every inline and block list directly under `block`, including those in a custom
/// node's slots. A `Slot::Block` holds one block, with no room for a list: the visitor sees a
/// one-element list, and if it comes back as anything but one block, those blocks go in a
/// classless `Div` (pandoc renders a `Div` as its content).
pub fn block_children_mut(block: &mut Block, v: &mut dyn ListVisitor) {
    match block {
        Block::Plain(p) => v.inlines(&mut p.content),
        Block::Paragraph(p) => v.inlines(&mut p.content),
        Block::LineBlock(lb) => {
            for line in lb.content.iter_mut() {
                v.inlines(line);
            }
        }
        Block::BlockQuote(bq) => v.blocks(&mut bq.content),
        Block::OrderedList(ol) => {
            for item in ol.content.iter_mut() {
                v.blocks(item);
            }
        }
        Block::BulletList(bl) => {
            for item in bl.content.iter_mut() {
                v.blocks(item);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in dl.content.iter_mut() {
                v.inlines(term);
                for def in defs.iter_mut() {
                    v.blocks(def);
                }
            }
        }
        Block::Header(h) => v.inlines(&mut h.content),
        Block::Div(d) => v.blocks(&mut d.content),
        Block::Figure(fig) => {
            caption_children_mut(&mut fig.caption, v);
            v.blocks(&mut fig.content);
        }
        Block::Table(t) => {
            caption_children_mut(&mut t.caption, v);
            for row in t.head.rows.iter_mut().chain(t.foot.rows.iter_mut()) {
                for cell in row.cells.iter_mut() {
                    v.blocks(&mut cell.content);
                }
            }
            for body in t.bodies.iter_mut() {
                for row in body.head.iter_mut().chain(body.body.iter_mut()) {
                    for cell in row.cells.iter_mut() {
                        v.blocks(&mut cell.content);
                    }
                }
            }
        }
        Block::CaptionBlock(cb) => v.inlines(&mut cb.content),
        Block::NoteDefinitionPara(n) => v.inlines(&mut n.content),
        Block::NoteDefinitionFencedBlock(n) => v.blocks(&mut n.content),
        Block::Custom(c) => {
            for (_name, slot) in c.slots.iter_mut() {
                slot_children_mut(slot, v);
            }
        }
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_) => {}
    }
}

fn caption_children_mut(
    caption: &mut quarto_pandoc_types::caption::Caption,
    v: &mut dyn ListVisitor,
) {
    if let Some(short) = caption.short.as_mut() {
        v.inlines(short);
    }
    if let Some(long) = caption.long.as_mut() {
        v.blocks(long);
    }
}

/// Hand `v` every inline and block list directly under `inline` (a `Cite`'s citation
/// prefixes and suffixes included, and a custom node's slots). A `Slot::Inline` holds one
/// inline: if the visitor turns it into anything but one, those go in a classless `Span`.
pub fn inline_children_mut(inline: &mut Inline, v: &mut dyn ListVisitor) {
    match inline {
        Inline::Emph(e) => v.inlines(&mut e.content),
        Inline::Underline(u) => v.inlines(&mut u.content),
        Inline::Strong(s) => v.inlines(&mut s.content),
        Inline::Strikeout(s) => v.inlines(&mut s.content),
        Inline::Superscript(s) => v.inlines(&mut s.content),
        Inline::Subscript(s) => v.inlines(&mut s.content),
        Inline::SmallCaps(s) => v.inlines(&mut s.content),
        Inline::Quoted(q) => v.inlines(&mut q.content),
        Inline::Cite(c) => {
            for citation in c.citations.iter_mut() {
                v.inlines(&mut citation.prefix);
                v.inlines(&mut citation.suffix);
            }
            v.inlines(&mut c.content);
        }
        Inline::Link(l) => v.inlines(&mut l.content),
        Inline::Image(i) => v.inlines(&mut i.content),
        Inline::Span(s) => v.inlines(&mut s.content),
        Inline::Note(n) => v.blocks(&mut n.content),
        Inline::Insert(i) => v.inlines(&mut i.content),
        Inline::Delete(d) => v.inlines(&mut d.content),
        Inline::Highlight(h) => v.inlines(&mut h.content),
        Inline::EditComment(e) => v.inlines(&mut e.content),
        Inline::Custom(c) => {
            for (_name, slot) in c.slots.iter_mut() {
                slot_children_mut(slot, v);
            }
        }
        Inline::Str(_)
        | Inline::Code(_)
        | Inline::Space(_)
        | Inline::SoftBreak(_)
        | Inline::LineBreak(_)
        | Inline::Math(_)
        | Inline::RawInline(_)
        | Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_) => {}
    }
}

fn slot_children_mut(slot: &mut Slot, v: &mut dyn ListVisitor) {
    match slot {
        Slot::Blocks(bs) => v.blocks(bs),
        Slot::Inlines(is) => v.inlines(is),
        Slot::Block(b) => {
            let placeholder = Block::HorizontalRule(quarto_pandoc_types::block::HorizontalRule {
                source_info: b.source_info().clone(),
            });
            let mut list = vec![std::mem::replace(&mut **b, placeholder)];
            v.blocks(&mut list);
            **b = if list.len() == 1 {
                list.pop().expect("one block")
            } else {
                Block::Div(quarto_pandoc_types::block::Div {
                    attr: Default::default(),
                    content: list,
                    source_info: b.source_info().clone(),
                    attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
                })
            };
        }
        Slot::Inline(i) => {
            let placeholder = Inline::Space(quarto_pandoc_types::inline::Space {
                source_info: i.source_info().clone(),
            });
            let mut list = vec![std::mem::replace(&mut **i, placeholder)];
            v.inlines(&mut list);
            **i = if list.len() == 1 {
                list.pop().expect("one inline")
            } else {
                Inline::Span(quarto_pandoc_types::inline::Span {
                    attr: Default::default(),
                    content: list,
                    source_info: i.source_info().clone(),
                    attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
                })
            };
        }
    }
}

/// Map each `ConfigValue` `PandocInlines` / `PandocBlocks` list under `value` (maps and arrays
/// at any depth) to `v`.
pub fn config_value_lists_mut(value: &mut ConfigValue, v: &mut dyn ListVisitor) {
    match &mut value.value {
        ConfigValueKind::PandocInlines(inlines) => v.inlines(inlines),
        ConfigValueKind::PandocBlocks(blocks) => v.blocks(blocks),
        ConfigValueKind::Array(items) => {
            for item in items.iter_mut() {
                config_value_lists_mut(item, v);
            }
        }
        ConfigValueKind::Map(entries) => {
            for entry in entries.iter_mut() {
                config_value_lists_mut(&mut entry.value, v);
            }
        }
        ConfigValueKind::Scalar { .. }
        | ConfigValueKind::Path(_)
        | ConfigValueKind::Glob(_)
        | ConfigValueKind::Expr(_) => {}
    }
}

/// The post-order list walks: recurse into every child list, then call back.
struct ListWalker<'a> {
    inlines: &'a mut dyn FnMut(&mut Inlines),
    blocks: &'a mut dyn FnMut(&mut Vec<Block>),
}

impl ListVisitor for ListWalker<'_> {
    fn blocks(&mut self, blocks: &mut Vec<Block>) {
        for block in blocks.iter_mut() {
            block_children_mut(block, self);
        }
        (self.blocks)(blocks);
    }

    fn inlines(&mut self, inlines: &mut Inlines) {
        for inline in inlines.iter_mut() {
            inline_children_mut(inline, self);
        }
        (self.inlines)(inlines);
    }
}

/// Call `f` on every inline list under `blocks` (post-order: a list after everything in it).
pub fn for_each_inline_list_mut(blocks: &mut Vec<Block>, f: &mut dyn FnMut(&mut Inlines)) {
    ListWalker {
        inlines: f,
        blocks: &mut |_| {},
    }
    .blocks(blocks);
}

/// Call `f` on every block list under `blocks`, `blocks` itself included (post-order).
pub fn for_each_block_list_mut(blocks: &mut Vec<Block>, f: &mut dyn FnMut(&mut Vec<Block>)) {
    ListWalker {
        inlines: &mut |_| {},
        blocks: f,
    }
    .blocks(blocks);
}

/// Call `f` on every inline list in `meta`'s `PandocInlines` and `PandocBlocks` values, at
/// any depth of maps and arrays (post-order).
pub fn for_each_meta_inline_list_mut(meta: &mut ConfigValue, f: &mut dyn FnMut(&mut Inlines)) {
    config_value_lists_mut(
        meta,
        &mut ListWalker {
            inlines: f,
            blocks: &mut |_| {},
        },
    );
}

#[cfg(test)]
mod list_walk_tests {
    use super::*;
    use quarto_pandoc_types::block::{BlockQuote, Paragraph};
    use quarto_pandoc_types::inline::{Emph, Note, Str};
    use quarto_source_map::SourceInfo;

    fn str_(t: &str) -> Inline {
        Inline::Str(Str {
            text: t.into(),
            source_info: SourceInfo::for_test(),
        })
    }

    fn para(content: Inlines) -> Block {
        Block::Paragraph(Paragraph {
            content,
            source_info: SourceInfo::for_test(),
        })
    }

    #[test]
    fn visits_nested_lists_post_order_including_notes_and_cites() {
        let note = Inline::Note(Note {
            content: vec![para(vec![str_("in-note")])],
            source_info: SourceInfo::for_test(),
        });
        let emph = Inline::Emph(Emph {
            content: vec![str_("e")],
            source_info: SourceInfo::for_test(),
        });
        let mut blocks = vec![Block::BlockQuote(BlockQuote {
            content: vec![para(vec![str_("a"), emph, note])],
            source_info: SourceInfo::for_test(),
        })];
        let mut seen: Vec<String> = Vec::new();
        for_each_inline_list_mut(&mut blocks, &mut |list| {
            let text: Vec<String> = list
                .iter()
                .map(|i| match i {
                    Inline::Str(s) => s.text.clone(),
                    _ => "<x>".into(),
                })
                .collect();
            seen.push(text.join(""));
        });
        // The emphasis's list, then the note's paragraph, then the outer paragraph.
        assert_eq!(seen, ["e", "in-note", "a<x><x>"]);
    }

    #[test]
    fn block_lists_include_the_root() {
        let mut blocks = vec![para(vec![str_("a")])];
        let mut n = 0;
        for_each_block_list_mut(&mut blocks, &mut |_| n += 1);
        assert_eq!(n, 1);
    }
}
