/*
 * remap.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * FileId remapping for Pandoc ASTs.
 */

//! Remap every [`FileId`] reachable from a [`Pandoc`] AST.
//!
//! The reconciler merges two ASTs that were parsed against different files
//! (original `.qmd` and engine-produced intermediate, e.g. `.rmarkdown`).
//! Each AST carries `SourceInfo` with `FileId(0)` pointing at its own file,
//! so naively merging them would collide. The caller gives each AST its own
//! slot in a combined filename table and then applies a remap on the AST
//! that needs its ids shifted before calling [`reconcile`].
//!
//! This module is independent of reconciliation plans — it just walks the
//! AST and calls the caller-supplied mapping function on every `FileId`.
//! This keeps the reconcile crate free of filename knowledge; callers
//! (pipeline code) decide which file goes to which slot.
//!
//! [`FileId`]: quarto_source_map::FileId
//! [`Pandoc`]: quarto_pandoc_types::Pandoc
//! [`reconcile`]: crate::reconcile

use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
use quarto_pandoc_types::caption::Caption;
use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::table::{Cell, Row, Table, TableBody, TableFoot, TableHead};
use quarto_pandoc_types::{Block, Inline, Pandoc};
use quarto_source_map::{FileId, SourceInfo};

/// Remap every [`FileId`] reachable from a [`Pandoc`] AST using `map`.
///
/// Walks blocks, inlines, attr source info, target source info, captions,
/// tables, custom node slots, and config value meta. Every `FileId` stored
/// inside a `SourceInfo::Original` — including those nested in
/// `SourceInfo::Substring` parents and `SourceInfo::Concat` pieces — is
/// rewritten in place.
pub fn remap_file_ids<F>(pandoc: &mut Pandoc, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    remap_config_value(&mut pandoc.meta, map);
    for block in &mut pandoc.blocks {
        remap_block(block, map);
    }
}

fn remap_source_info<F>(si: &mut SourceInfo, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    si.remap_file_ids(map);
}

fn remap_opt_source_info<F>(si: &mut Option<SourceInfo>, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    if let Some(si) = si.as_mut() {
        remap_source_info(si, map);
    }
}

fn remap_attr_source<F>(attr_source: &mut AttrSourceInfo, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    remap_opt_source_info(&mut attr_source.id, map);
    for class in &mut attr_source.classes {
        remap_opt_source_info(class, map);
    }
    for (key, val) in &mut attr_source.attributes {
        remap_opt_source_info(key, map);
        remap_opt_source_info(val, map);
    }
}

fn remap_target_source<F>(target_source: &mut TargetSourceInfo, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    remap_opt_source_info(&mut target_source.url, map);
    remap_opt_source_info(&mut target_source.title, map);
}

fn remap_caption<F>(caption: &mut Caption, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    if let Some(inlines) = caption.short.as_mut() {
        for inline in inlines {
            remap_inline(inline, map);
        }
    }
    if let Some(blocks) = caption.long.as_mut() {
        for block in blocks {
            remap_block(block, map);
        }
    }
    remap_source_info(&mut caption.source_info, map);
}

fn remap_block<F>(block: &mut Block, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    match block {
        Block::Plain(b) => {
            for inline in &mut b.content {
                remap_inline(inline, map);
            }
            remap_source_info(&mut b.source_info, map);
        }
        Block::Paragraph(b) => {
            for inline in &mut b.content {
                remap_inline(inline, map);
            }
            remap_source_info(&mut b.source_info, map);
        }
        Block::LineBlock(b) => {
            for line in &mut b.content {
                for inline in line {
                    remap_inline(inline, map);
                }
            }
            remap_source_info(&mut b.source_info, map);
        }
        Block::CodeBlock(b) => {
            remap_source_info(&mut b.source_info, map);
            remap_attr_source(&mut b.attr_source, map);
        }
        Block::RawBlock(b) => {
            remap_source_info(&mut b.source_info, map);
        }
        Block::BlockQuote(b) => {
            for block in &mut b.content {
                remap_block(block, map);
            }
            remap_source_info(&mut b.source_info, map);
        }
        Block::OrderedList(b) => {
            for item in &mut b.content {
                for block in item {
                    remap_block(block, map);
                }
            }
            remap_source_info(&mut b.source_info, map);
        }
        Block::BulletList(b) => {
            for item in &mut b.content {
                for block in item {
                    remap_block(block, map);
                }
            }
            remap_source_info(&mut b.source_info, map);
        }
        Block::DefinitionList(b) => {
            for (term, defs) in &mut b.content {
                for inline in term {
                    remap_inline(inline, map);
                }
                for def in defs {
                    for block in def {
                        remap_block(block, map);
                    }
                }
            }
            remap_source_info(&mut b.source_info, map);
        }
        Block::Header(b) => {
            for inline in &mut b.content {
                remap_inline(inline, map);
            }
            remap_source_info(&mut b.source_info, map);
            remap_attr_source(&mut b.attr_source, map);
        }
        Block::HorizontalRule(b) => {
            remap_source_info(&mut b.source_info, map);
        }
        Block::Table(t) => {
            remap_table(t, map);
        }
        Block::Figure(b) => {
            remap_caption(&mut b.caption, map);
            for block in &mut b.content {
                remap_block(block, map);
            }
            remap_source_info(&mut b.source_info, map);
            remap_attr_source(&mut b.attr_source, map);
        }
        Block::Div(b) => {
            for block in &mut b.content {
                remap_block(block, map);
            }
            remap_source_info(&mut b.source_info, map);
            remap_attr_source(&mut b.attr_source, map);
        }
        Block::BlockMetadata(b) => {
            remap_config_value(&mut b.meta, map);
            remap_source_info(&mut b.source_info, map);
        }
        Block::NoteDefinitionPara(b) => {
            for inline in &mut b.content {
                remap_inline(inline, map);
            }
            remap_source_info(&mut b.source_info, map);
        }
        Block::NoteDefinitionFencedBlock(b) => {
            for block in &mut b.content {
                remap_block(block, map);
            }
            remap_source_info(&mut b.source_info, map);
        }
        Block::CaptionBlock(b) => {
            for inline in &mut b.content {
                remap_inline(inline, map);
            }
            remap_source_info(&mut b.source_info, map);
        }
        Block::Custom(node) => {
            remap_custom_node(node, map);
        }
    }
}

fn remap_inline<F>(inline: &mut Inline, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    match inline {
        Inline::Str(i) => remap_source_info(&mut i.source_info, map),
        Inline::Emph(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
        }
        Inline::Underline(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
        }
        Inline::Strong(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
        }
        Inline::Strikeout(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
        }
        Inline::Superscript(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
        }
        Inline::Subscript(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
        }
        Inline::SmallCaps(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
        }
        Inline::Quoted(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
        }
        Inline::Cite(i) => {
            for citation in &mut i.citations {
                for c in &mut citation.prefix {
                    remap_inline(c, map);
                }
                for c in &mut citation.suffix {
                    remap_inline(c, map);
                }
                remap_opt_source_info(&mut citation.id_source, map);
            }
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
        }
        Inline::Code(i) => {
            remap_source_info(&mut i.source_info, map);
            remap_attr_source(&mut i.attr_source, map);
        }
        Inline::Space(i) => remap_source_info(&mut i.source_info, map),
        Inline::SoftBreak(i) => remap_source_info(&mut i.source_info, map),
        Inline::LineBreak(i) => remap_source_info(&mut i.source_info, map),
        Inline::Math(i) => remap_source_info(&mut i.source_info, map),
        Inline::RawInline(i) => remap_source_info(&mut i.source_info, map),
        Inline::Link(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
            remap_attr_source(&mut i.attr_source, map);
            remap_target_source(&mut i.target_source, map);
        }
        Inline::Image(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
            remap_attr_source(&mut i.attr_source, map);
            remap_target_source(&mut i.target_source, map);
        }
        Inline::Note(i) => {
            for block in &mut i.content {
                remap_block(block, map);
            }
            remap_source_info(&mut i.source_info, map);
        }
        Inline::Span(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
            remap_attr_source(&mut i.attr_source, map);
        }
        Inline::Shortcode(i) => {
            remap_source_info(&mut i.source_info, map);
        }
        Inline::NoteReference(i) => remap_source_info(&mut i.source_info, map),
        Inline::Attr(a) => {
            remap_attr_source(&mut a.attr_source, map);
            remap_source_info(&mut a.source_info, map);
        }
        Inline::Insert(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
            remap_attr_source(&mut i.attr_source, map);
        }
        Inline::Delete(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
            remap_attr_source(&mut i.attr_source, map);
        }
        Inline::Highlight(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
            remap_attr_source(&mut i.attr_source, map);
        }
        Inline::EditComment(i) => {
            for c in &mut i.content {
                remap_inline(c, map);
            }
            remap_source_info(&mut i.source_info, map);
            remap_attr_source(&mut i.attr_source, map);
        }
        Inline::Custom(node) => {
            remap_custom_node(node, map);
        }
    }
}

fn remap_custom_node<F>(node: &mut CustomNode, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    for slot in node.slots.values_mut() {
        match slot {
            Slot::Block(b) => remap_block(b, map),
            Slot::Inline(i) => remap_inline(i, map),
            Slot::Blocks(blocks) => {
                for block in blocks {
                    remap_block(block, map);
                }
            }
            Slot::Inlines(inlines) => {
                for inline in inlines {
                    remap_inline(inline, map);
                }
            }
        }
    }
    remap_source_info(&mut node.source_info, map);
}

fn remap_table<F>(table: &mut Table, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    remap_caption(&mut table.caption, map);
    remap_table_head(&mut table.head, map);
    for body in &mut table.bodies {
        remap_table_body(body, map);
    }
    remap_table_foot(&mut table.foot, map);
    remap_source_info(&mut table.source_info, map);
    remap_attr_source(&mut table.attr_source, map);
}

fn remap_table_head<F>(head: &mut TableHead, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    for row in &mut head.rows {
        remap_row(row, map);
    }
    remap_source_info(&mut head.source_info, map);
    remap_attr_source(&mut head.attr_source, map);
}

fn remap_table_body<F>(body: &mut TableBody, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    for row in &mut body.head {
        remap_row(row, map);
    }
    for row in &mut body.body {
        remap_row(row, map);
    }
    remap_source_info(&mut body.source_info, map);
    remap_attr_source(&mut body.attr_source, map);
}

fn remap_table_foot<F>(foot: &mut TableFoot, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    for row in &mut foot.rows {
        remap_row(row, map);
    }
    remap_source_info(&mut foot.source_info, map);
    remap_attr_source(&mut foot.attr_source, map);
}

fn remap_row<F>(row: &mut Row, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    for cell in &mut row.cells {
        remap_cell(cell, map);
    }
    remap_source_info(&mut row.source_info, map);
    remap_attr_source(&mut row.attr_source, map);
}

fn remap_cell<F>(cell: &mut Cell, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    for block in &mut cell.content {
        remap_block(block, map);
    }
    remap_source_info(&mut cell.source_info, map);
    remap_attr_source(&mut cell.attr_source, map);
}

fn remap_config_value<F>(cv: &mut ConfigValue, map: &F)
where
    F: Fn(FileId) -> FileId,
{
    remap_source_info(&mut cv.source_info, map);
    match &mut cv.value {
        ConfigValueKind::Scalar { .. }
        | ConfigValueKind::Path(_)
        | ConfigValueKind::Glob(_)
        | ConfigValueKind::Expr(_) => {}
        ConfigValueKind::PandocInlines(inlines) => {
            for inline in inlines {
                remap_inline(inline, map);
            }
        }
        ConfigValueKind::PandocBlocks(blocks) => {
            for block in blocks {
                remap_block(block, map);
            }
        }
        ConfigValueKind::Array(items) => {
            for item in items {
                remap_config_value(item, map);
            }
        }
        ConfigValueKind::Map(entries) => {
            for entry in entries {
                remap_source_info(&mut entry.key_source, map);
                remap_config_value(&mut entry.value, map);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::{Div, Header, Paragraph, Str};
    use quarto_source_map::FileId;

    fn src(file: usize) -> SourceInfo {
        SourceInfo::original(FileId(file), 0, 10)
    }

    fn make_str(file: usize) -> Inline {
        Inline::Str(Str {
            text: "hello".to_string(),
            source_info: src(file),
        })
    }

    fn file_id_of(si: &SourceInfo) -> FileId {
        match si {
            SourceInfo::Original { file_id, .. } => *file_id,
            _ => panic!("expected Original"),
        }
    }

    #[test]
    fn remap_shifts_block_and_inline_source_info() {
        let mut pandoc = Pandoc {
            meta: ConfigValue::null(src(0)),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![make_str(0)],
                source_info: src(0),
            })],
        };

        remap_file_ids(&mut pandoc, &|id| FileId(id.0 + 1));

        assert_eq!(file_id_of(&pandoc.meta.source_info), FileId(1));
        let Block::Paragraph(p) = &pandoc.blocks[0] else {
            panic!("expected Paragraph");
        };
        assert_eq!(file_id_of(&p.source_info), FileId(1));
        let Inline::Str(s) = &p.content[0] else {
            panic!("expected Str");
        };
        assert_eq!(file_id_of(&s.source_info), FileId(1));
    }

    #[test]
    fn remap_descends_into_nested_containers() {
        let inner_para = Block::Paragraph(Paragraph {
            content: vec![make_str(0)],
            source_info: src(0),
        });
        let div = Block::Div(Div {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![inner_para],
            source_info: src(0),
            attr_source: AttrSourceInfo::empty(),
        });
        let mut pandoc = Pandoc {
            meta: ConfigValue::null(src(0)),
            blocks: vec![div],
        };

        remap_file_ids(&mut pandoc, &|id| FileId(id.0 + 3));

        let Block::Div(d) = &pandoc.blocks[0] else {
            panic!("expected Div");
        };
        assert_eq!(file_id_of(&d.source_info), FileId(3));
        let Block::Paragraph(p) = &d.content[0] else {
            panic!("expected Paragraph");
        };
        assert_eq!(file_id_of(&p.source_info), FileId(3));
        let Inline::Str(s) = &p.content[0] else {
            panic!("expected Str");
        };
        assert_eq!(file_id_of(&s.source_info), FileId(3));
    }

    #[test]
    fn remap_touches_attr_source() {
        let mut attr_source = AttrSourceInfo {
            id: Some(src(0)),
            classes: vec![Some(src(0))],
            attributes: vec![(Some(src(0)), Some(src(0)))],
        };
        let header = Block::Header(Header {
            level: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: src(0),
            attr_source: std::mem::replace(&mut attr_source, AttrSourceInfo::empty()),
        });
        let mut pandoc = Pandoc {
            meta: ConfigValue::null(src(0)),
            blocks: vec![header],
        };

        remap_file_ids(&mut pandoc, &|id| FileId(id.0 + 5));

        let Block::Header(h) = &pandoc.blocks[0] else {
            panic!("expected Header");
        };
        assert_eq!(file_id_of(h.attr_source.id.as_ref().unwrap()), FileId(5));
        assert_eq!(
            file_id_of(h.attr_source.classes[0].as_ref().unwrap()),
            FileId(5)
        );
        assert_eq!(
            file_id_of(h.attr_source.attributes[0].0.as_ref().unwrap()),
            FileId(5)
        );
        assert_eq!(
            file_id_of(h.attr_source.attributes[0].1.as_ref().unwrap()),
            FileId(5)
        );
    }
}
