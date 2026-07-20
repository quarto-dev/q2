/*
 * walk.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Lua filter traversal engine (bd-2j048yfm), mirroring
 * pandoc-lua-marshal's Walk.hs / SpliceList.hs / Topdown.hs.
 *
 * Structure: the AST children map is written ONCE (`walk_inline_children`
 * / `walk_block_children`, generic over a `LuaWalker`), and two walkers
 * are built on it:
 *
 * - `TypewisePass` — one of four sequential full walks (Inline-splicing,
 *   Inlines-straight, Block-splicing, Blocks-straight), each bottom-up:
 *   children are visited before the element's own filter function.
 * - `TopdownWalk` — a single pre-order traversal: list function first,
 *   then per-element function, then descent into the (possibly
 *   replaced) element's children. An element function's second return
 *   value of `false` skips that element's children but siblings
 *   continue; a list function's `false` halts processing of that list.
 *
 * The subtree rule falls out of the entry points: walking an ELEMENT
 * starts at `walk_*_children`, so the element itself is never offered
 * to the filter and no synthetic singleton list exists. Walking a LIST
 * (document blocks, `Inlines`/`Blocks` values) starts at the list
 * walker, so the top-level list IS offered to the list functions —
 * matching pandoc's `Walkable` instances.
 */

use async_trait::async_trait;
use mlua::{Function, Lua, MultiValue, Result, Table, Value};

use crate::pandoc::{Block, Caption, Citation, Inline, Row};

use super::filter::{
    TraversalControl, block_tag, handle_block_return, handle_block_return_with_control,
    handle_blocks_return, handle_blocks_return_with_control, handle_inline_return,
    handle_inline_return_with_control, handle_inlines_return, handle_inlines_return_with_control,
    inline_tag,
};
use super::types::{LuaBlock, LuaInline, blocks_to_lua_table, inlines_to_lua_table};

// ============================================================================
// The walker abstraction: how to process a list of inlines / blocks.
// ============================================================================

#[async_trait(?Send)]
pub(crate) trait LuaWalker {
    async fn walk_inlines(&self, inlines: Vec<Inline>) -> Result<Vec<Inline>>;
    async fn walk_blocks(&self, blocks: Vec<Block>) -> Result<Vec<Block>>;
}

// ============================================================================
// The children map — the ONE place that knows which fields of each AST
// node contain walkable inline/block lists. Mirrors pandoc's Walkable
// instances (walkInlineM/walkBlockM/walkCitationM/…); the q2-specific
// variants (Shortcode, NoteReference, InlineAttr, BlockMetadata,
// NoteDefinition*, CaptionBlock, Custom, Insert/Delete/Highlight/
// EditComment) follow the native engine in `crate::filters`.
// ============================================================================

async fn walk_citation<W: LuaWalker>(w: &W, citation: Citation) -> Result<Citation> {
    Ok(Citation {
        prefix: w.walk_inlines(citation.prefix).await?,
        suffix: w.walk_inlines(citation.suffix).await?,
        ..citation
    })
}

async fn walk_caption<W: LuaWalker>(w: &W, caption: Caption) -> Result<Caption> {
    let short = match caption.short {
        Some(short) => Some(w.walk_inlines(short).await?),
        None => None,
    };
    let long = match caption.long {
        Some(long) => Some(w.walk_blocks(long).await?),
        None => None,
    };
    Ok(Caption {
        short,
        long,
        source_info: caption.source_info,
    })
}

async fn walk_rows<W: LuaWalker>(w: &W, rows: Vec<Row>) -> Result<Vec<Row>> {
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let mut cells = Vec::with_capacity(row.cells.len());
        for cell in row.cells {
            cells.push(crate::pandoc::Cell {
                content: w.walk_blocks(cell.content).await?,
                ..cell
            });
        }
        result.push(Row { cells, ..row });
    }
    Ok(result)
}

/// Walk the children of a single table Cell (its content blocks) —
/// pandoc's `walkBlocksAndInlines` on a Cell; the cell itself is not
/// offered to any filter function.
async fn walk_cell_children<W: LuaWalker>(
    w: &W,
    cell: crate::pandoc::Cell,
) -> Result<crate::pandoc::Cell> {
    Ok(crate::pandoc::Cell {
        content: w.walk_blocks(cell.content).await?,
        ..cell
    })
}

async fn walk_blocks_vec<W: LuaWalker>(w: &W, items: Vec<Vec<Block>>) -> Result<Vec<Vec<Block>>> {
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        result.push(w.walk_blocks(item).await?);
    }
    Ok(result)
}

/// Walk the children of a single inline element (the element itself is
/// NOT offered to any filter function — the subtree rule).
pub(crate) async fn walk_inline_children<W: LuaWalker>(w: &W, inline: Inline) -> Result<Inline> {
    use crate::pandoc;
    Ok(match inline {
        Inline::Emph(e) => Inline::Emph(pandoc::Emph {
            content: w.walk_inlines(e.content).await?,
            ..e
        }),
        Inline::Underline(u) => Inline::Underline(pandoc::Underline {
            content: w.walk_inlines(u.content).await?,
            ..u
        }),
        Inline::Strong(s) => Inline::Strong(pandoc::Strong {
            content: w.walk_inlines(s.content).await?,
            ..s
        }),
        Inline::Strikeout(s) => Inline::Strikeout(pandoc::Strikeout {
            content: w.walk_inlines(s.content).await?,
            ..s
        }),
        Inline::Superscript(s) => Inline::Superscript(pandoc::Superscript {
            content: w.walk_inlines(s.content).await?,
            ..s
        }),
        Inline::Subscript(s) => Inline::Subscript(pandoc::Subscript {
            content: w.walk_inlines(s.content).await?,
            ..s
        }),
        Inline::SmallCaps(s) => Inline::SmallCaps(pandoc::SmallCaps {
            content: w.walk_inlines(s.content).await?,
            ..s
        }),
        Inline::Quoted(q) => Inline::Quoted(pandoc::Quoted {
            content: w.walk_inlines(q.content).await?,
            ..q
        }),
        Inline::Cite(c) => {
            let mut citations = Vec::with_capacity(c.citations.len());
            for citation in c.citations {
                citations.push(walk_citation(w, citation).await?);
            }
            Inline::Cite(pandoc::Cite {
                citations,
                content: w.walk_inlines(c.content).await?,
                ..c
            })
        }
        Inline::Link(l) => Inline::Link(pandoc::Link {
            content: w.walk_inlines(l.content).await?,
            ..l
        }),
        Inline::Image(i) => Inline::Image(pandoc::Image {
            content: w.walk_inlines(i.content).await?,
            ..i
        }),
        Inline::Note(n) => Inline::Note(pandoc::Note {
            content: w.walk_blocks(n.content).await?,
            ..n
        }),
        Inline::Span(s) => Inline::Span(pandoc::Span {
            content: w.walk_inlines(s.content).await?,
            ..s
        }),
        Inline::Insert(i) => Inline::Insert(pandoc::Insert {
            content: w.walk_inlines(i.content).await?,
            ..i
        }),
        Inline::Delete(d) => Inline::Delete(pandoc::Delete {
            content: w.walk_inlines(d.content).await?,
            ..d
        }),
        Inline::Highlight(h) => Inline::Highlight(pandoc::Highlight {
            content: w.walk_inlines(h.content).await?,
            ..h
        }),
        Inline::EditComment(e) => Inline::EditComment(pandoc::EditComment {
            content: w.walk_inlines(e.content).await?,
            ..e
        }),
        // Terminal inlines (no walkable children)
        Inline::Str(_)
        | Inline::Code(_)
        | Inline::Space(_)
        | Inline::SoftBreak(_)
        | Inline::LineBreak(_)
        | Inline::Math(_)
        | Inline::RawInline(_)
        | Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_)
        | Inline::Custom(_) => inline,
    })
}

/// Walk the children of a single block element (the element itself is
/// NOT offered to any filter function — the subtree rule).
pub(crate) async fn walk_block_children<W: LuaWalker>(w: &W, block: Block) -> Result<Block> {
    use crate::pandoc;
    Ok(match block {
        Block::Plain(p) => Block::Plain(pandoc::Plain {
            content: w.walk_inlines(p.content).await?,
            ..p
        }),
        Block::Paragraph(p) => Block::Paragraph(pandoc::Paragraph {
            content: w.walk_inlines(p.content).await?,
            ..p
        }),
        Block::LineBlock(l) => {
            let mut lines = Vec::with_capacity(l.content.len());
            for line in l.content {
                lines.push(w.walk_inlines(line).await?);
            }
            Block::LineBlock(pandoc::LineBlock {
                content: lines,
                ..l
            })
        }
        Block::BlockQuote(b) => Block::BlockQuote(pandoc::BlockQuote {
            content: w.walk_blocks(b.content).await?,
            ..b
        }),
        Block::OrderedList(l) => Block::OrderedList(pandoc::OrderedList {
            content: walk_blocks_vec(w, l.content).await?,
            ..l
        }),
        Block::BulletList(l) => Block::BulletList(pandoc::BulletList {
            content: walk_blocks_vec(w, l.content).await?,
            ..l
        }),
        Block::DefinitionList(l) => {
            let mut content = Vec::with_capacity(l.content.len());
            for (term, definitions) in l.content {
                content.push((
                    w.walk_inlines(term).await?,
                    walk_blocks_vec(w, definitions).await?,
                ));
            }
            Block::DefinitionList(pandoc::DefinitionList { content, ..l })
        }
        Block::Header(h) => Block::Header(pandoc::Header {
            content: w.walk_inlines(h.content).await?,
            ..h
        }),
        Block::Table(t) => {
            let caption = walk_caption(w, t.caption).await?;
            let head = pandoc::TableHead {
                rows: walk_rows(w, t.head.rows).await?,
                ..t.head
            };
            let mut bodies = Vec::with_capacity(t.bodies.len());
            for body in t.bodies {
                bodies.push(pandoc::TableBody {
                    head: walk_rows(w, body.head).await?,
                    body: walk_rows(w, body.body).await?,
                    ..body
                });
            }
            let foot = pandoc::TableFoot {
                rows: walk_rows(w, t.foot.rows).await?,
                ..t.foot
            };
            Block::Table(pandoc::Table {
                caption,
                head,
                bodies,
                foot,
                ..t
            })
        }
        Block::Figure(f) => Block::Figure(pandoc::Figure {
            caption: walk_caption(w, f.caption).await?,
            content: w.walk_blocks(f.content).await?,
            ..f
        }),
        Block::Div(d) => Block::Div(pandoc::Div {
            content: w.walk_blocks(d.content).await?,
            ..d
        }),
        // Terminal blocks (no walkable children)
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_)
        | Block::CaptionBlock(_)
        | Block::Custom(_) => block,
    })
}

// ============================================================================
// Typewise walker (WalkForEachType): four sequential full walks.
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PassMode {
    InlineSplicing,
    InlinesStraight,
    BlockSplicing,
    BlocksStraight,
}

const ALL_PASSES: [PassMode; 4] = [
    PassMode::InlineSplicing,
    PassMode::InlinesStraight,
    PassMode::BlockSplicing,
    PassMode::BlocksStraight,
];

pub(crate) struct TypewisePass<'a> {
    lua: &'a Lua,
    filter: &'a Table,
    mode: PassMode,
}

/// Look up the filter function for an inline element: type-specific
/// name first, then the generic "Inline".
fn inline_fn<'a>(filter: &Table, tag: &'a str) -> Option<(Function, &'a str)> {
    if let Ok(f) = filter.get::<Function>(tag) {
        return Some((f, tag));
    }
    if let Ok(f) = filter.get::<Function>("Inline") {
        return Some((f, "Inline"));
    }
    None
}

fn block_fn<'a>(filter: &Table, tag: &'a str) -> Option<(Function, &'a str)> {
    if let Ok(f) = filter.get::<Function>(tag) {
        return Some((f, tag));
    }
    if let Ok(f) = filter.get::<Function>("Block") {
        return Some((f, "Block"));
    }
    None
}

#[async_trait(?Send)]
impl LuaWalker for TypewisePass<'_> {
    async fn walk_inlines(&self, inlines: Vec<Inline>) -> Result<Vec<Inline>> {
        // Children before element function (bottom-up), elements before
        // the list function.
        let mut walked = Vec::with_capacity(inlines.len());
        for inline in inlines {
            let child_done = walk_inline_children(self, inline).await?;
            if self.mode == PassMode::InlineSplicing {
                let tag = inline_tag(&child_done);
                if let Some((f, fn_name)) = inline_fn(self.filter, tag) {
                    let ud = self
                        .lua
                        .create_userdata(LuaInline::new(child_done.clone()))?;
                    let ret: Value = f.call_async(ud).await?;
                    walked.extend(handle_inline_return(self.lua, ret, &child_done, fn_name)?);
                    continue;
                }
            }
            walked.push(child_done);
        }
        if self.mode == PassMode::InlinesStraight
            && let Ok(f) = self.filter.get::<Function>("Inlines")
        {
            let table = inlines_to_lua_table(self.lua, &walked)?;
            let ret: Value = f.call_async(table).await?;
            return handle_inlines_return(self.lua, ret, &walked, "Inlines");
        }
        Ok(walked)
    }

    async fn walk_blocks(&self, blocks: Vec<Block>) -> Result<Vec<Block>> {
        let mut walked = Vec::with_capacity(blocks.len());
        for block in blocks {
            let child_done = walk_block_children(self, block).await?;
            if self.mode == PassMode::BlockSplicing {
                let tag = block_tag(&child_done);
                if let Some((f, fn_name)) = block_fn(self.filter, tag) {
                    let ud = self
                        .lua
                        .create_userdata(LuaBlock::new(child_done.clone()))?;
                    let ret: Value = f.call_async(ud).await?;
                    walked.extend(handle_block_return(self.lua, ret, &child_done, fn_name)?);
                    continue;
                }
            }
            walked.push(child_done);
        }
        if self.mode == PassMode::BlocksStraight
            && let Ok(f) = self.filter.get::<Function>("Blocks")
        {
            let table = blocks_to_lua_table(self.lua, &walked)?;
            let ret: Value = f.call_async(table).await?;
            return handle_blocks_return(self.lua, ret, &walked, "Blocks");
        }
        Ok(walked)
    }
}

/// Does the filter define any function relevant to the given pass?
/// (Mirrors walkSplicing's `acceptedNames` check — skipping a pass with
/// no functions avoids a full AST rebuild.)
fn pass_is_active(filter: &Table, mode: PassMode) -> bool {
    let has = |name: &str| filter.get::<Function>(name).is_ok();
    match mode {
        PassMode::InlinesStraight => has("Inlines"),
        PassMode::BlocksStraight => has("Blocks"),
        // Type-specific names are numerous; checking the generic name
        // is not enough, so conservatively run the pass if the table
        // has ANY function-valued key besides the list functions.
        PassMode::InlineSplicing | PassMode::BlockSplicing => {
            let mut found = false;
            if let Ok(()) = filter.for_each(|k: Value, v: Value| {
                if let (Value::String(s), Value::Function(_)) = (&k, &v) {
                    let name = s.to_str().map(|s| s.to_string()).unwrap_or_default();
                    if name != "Inlines" && name != "Blocks" {
                        found = true;
                    }
                }
                Ok(())
            }) {}
            found
        }
    }
}

// ============================================================================
// Metadata traversal (Pandoc's Walkable instance covers Meta: element
// filter functions visit MetaInlines/MetaBlocks payloads, and Pandoc's
// field order puts meta before blocks in every pass)
// ============================================================================

/// Walk the Inlines/Blocks payloads embedded in a metadata ConfigValue
/// with the given walker. Containers and scalars keep their nodes
/// (source_info, merge_op, key order, key_source) untouched — only the
/// inline/block payload vectors are replaced.
async fn walk_meta_config_value<W: LuaWalker>(
    w: &W,
    cv: quarto_pandoc_types::ConfigValue,
) -> Result<quarto_pandoc_types::ConfigValue> {
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind};
    let ConfigValue {
        value,
        source_info,
        merge_op,
    } = cv;
    let value = match value {
        ConfigValueKind::PandocInlines(inlines) => {
            ConfigValueKind::PandocInlines(w.walk_inlines(inlines).await?)
        }
        ConfigValueKind::PandocBlocks(blocks) => {
            ConfigValueKind::PandocBlocks(w.walk_blocks(blocks).await?)
        }
        ConfigValueKind::Array(items) => {
            let mut walked = Vec::with_capacity(items.len());
            for item in items {
                walked.push(Box::pin(walk_meta_config_value(w, item)).await?);
            }
            ConfigValueKind::Array(walked)
        }
        ConfigValueKind::Map(entries) => {
            let mut walked = Vec::with_capacity(entries.len());
            for entry in entries {
                walked.push(ConfigMapEntry {
                    key: entry.key,
                    key_source: entry.key_source,
                    value: Box::pin(walk_meta_config_value(w, entry.value)).await?,
                });
            }
            ConfigValueKind::Map(walked)
        }
        other => other,
    };
    Ok(ConfigValue {
        value,
        source_info,
        merge_op,
    })
}

/// Typewise element walk over a whole document: per pass, meta payloads
/// first (Pandoc field order), then the block tree.
pub(crate) async fn typewise_pandoc(
    lua: &Lua,
    filter: &Table,
    meta: &quarto_pandoc_types::ConfigValue,
    blocks: &[Block],
) -> Result<(quarto_pandoc_types::ConfigValue, Vec<Block>)> {
    let mut meta = meta.clone();
    let mut blocks = blocks.to_vec();
    for mode in ALL_PASSES {
        if !pass_is_active(filter, mode) {
            continue;
        }
        let pass = TypewisePass { lua, filter, mode };
        meta = walk_meta_config_value(&pass, meta).await?;
        blocks = pass.walk_blocks(blocks).await?;
    }
    Ok((meta, blocks))
}

/// Topdown element walk over a whole document: meta payloads first,
/// then the block tree (truncation control stays subtree-local).
pub(crate) async fn topdown_pandoc(
    lua: &Lua,
    filter: &Table,
    meta: &quarto_pandoc_types::ConfigValue,
    blocks: &[Block],
) -> Result<(quarto_pandoc_types::ConfigValue, Vec<Block>)> {
    let walk = TopdownWalk { lua, filter };
    let meta = walk_meta_config_value(&walk, meta.clone()).await?;
    let blocks = walk.walk_blocks(blocks.to_vec()).await?;
    Ok((meta, blocks))
}

// ============================================================================
// Typewise entry points
// ============================================================================

/// Typewise walk rooted at a block LIST (document blocks, `Blocks`
/// values): the top-level list is offered to the Blocks function.
pub(crate) async fn typewise_blocks(
    lua: &Lua,
    filter: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    let mut current = blocks.to_vec();
    for mode in ALL_PASSES {
        if !pass_is_active(filter, mode) {
            continue;
        }
        let pass = TypewisePass { lua, filter, mode };
        current = pass.walk_blocks(current).await?;
    }
    Ok(current)
}

/// Typewise walk rooted at an inline LIST (`Inlines` values): all four
/// passes run (block functions reach blocks nested in Notes).
pub(crate) async fn typewise_inlines(
    lua: &Lua,
    filter: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    let mut current = inlines.to_vec();
    for mode in ALL_PASSES {
        if !pass_is_active(filter, mode) {
            continue;
        }
        let pass = TypewisePass { lua, filter, mode };
        current = pass.walk_inlines(current).await?;
    }
    Ok(current)
}

/// Typewise walk rooted at a block ELEMENT: children only.
pub(crate) async fn typewise_block_element(
    lua: &Lua,
    filter: &Table,
    block: &Block,
) -> Result<Block> {
    let mut current = block.clone();
    for mode in ALL_PASSES {
        if !pass_is_active(filter, mode) {
            continue;
        }
        let pass = TypewisePass { lua, filter, mode };
        current = walk_block_children(&pass, current).await?;
    }
    Ok(current)
}

/// Typewise walk of a table Cell's contents (`Cell:walk`).
pub(crate) async fn typewise_cell(
    lua: &Lua,
    filter: &Table,
    cell: &crate::pandoc::Cell,
) -> Result<crate::pandoc::Cell> {
    let mut current = cell.clone();
    for mode in ALL_PASSES {
        if !pass_is_active(filter, mode) {
            continue;
        }
        let pass = TypewisePass { lua, filter, mode };
        current = walk_cell_children(&pass, current).await?;
    }
    Ok(current)
}

/// Typewise walk of a table Row's cells (`Row:walk`).
pub(crate) async fn typewise_row(lua: &Lua, filter: &Table, row: &Row) -> Result<Row> {
    let mut current = row.clone();
    for mode in ALL_PASSES {
        if !pass_is_active(filter, mode) {
            continue;
        }
        let pass = TypewisePass { lua, filter, mode };
        current = walk_rows(&pass, vec![current])
            .await?
            .pop()
            .expect("walk_rows preserves row count");
    }
    Ok(current)
}

/// Typewise walk rooted at an inline ELEMENT: children only.
pub(crate) async fn typewise_inline_element(
    lua: &Lua,
    filter: &Table,
    inline: &Inline,
) -> Result<Inline> {
    let mut current = inline.clone();
    for mode in ALL_PASSES {
        if !pass_is_active(filter, mode) {
            continue;
        }
        let pass = TypewisePass { lua, filter, mode };
        current = walk_inline_children(&pass, current).await?;
    }
    Ok(current)
}

// ============================================================================
// Topdown walker (WalkTopdown): single pre-order traversal with
// traversal control, mirroring Topdown.hs `walkTopdownM`.
// ============================================================================

pub(crate) struct TopdownWalk<'a> {
    lua: &'a Lua,
    filter: &'a Table,
}

#[async_trait(?Send)]
impl LuaWalker for TopdownWalk<'_> {
    async fn walk_inlines(&self, inlines: Vec<Inline>) -> Result<Vec<Inline>> {
        // 1. List function first; Stop halts processing of this list.
        let inlines = if let Ok(f) = self.filter.get::<Function>("Inlines") {
            let table = inlines_to_lua_table(self.lua, &inlines)?;
            let ret: MultiValue = f.call_async(table).await?;
            let (result, ctrl) =
                handle_inlines_return_with_control(self.lua, ret, &inlines, "Inlines")?;
            if ctrl == TraversalControl::Stop {
                return Ok(result);
            }
            result
        } else {
            inlines
        };

        // 2. Element function, then descent into the (replaced) children.
        let mut result = Vec::with_capacity(inlines.len());
        for inline in inlines {
            let tag = inline_tag(&inline);
            let (spliced, ctrl) = if let Some((f, fn_name)) = inline_fn(self.filter, tag) {
                let ud = self.lua.create_userdata(LuaInline::new(inline.clone()))?;
                let ret: MultiValue = f.call_async(ud).await?;
                handle_inline_return_with_control(self.lua, ret, &inline, fn_name)?
            } else {
                (vec![inline], TraversalControl::Continue)
            };
            match ctrl {
                TraversalControl::Stop => result.extend(spliced),
                TraversalControl::Continue => {
                    for elem in spliced {
                        result.push(walk_inline_children(self, elem).await?);
                    }
                }
            }
        }
        Ok(result)
    }

    async fn walk_blocks(&self, blocks: Vec<Block>) -> Result<Vec<Block>> {
        let blocks = if let Ok(f) = self.filter.get::<Function>("Blocks") {
            let table = blocks_to_lua_table(self.lua, &blocks)?;
            let ret: MultiValue = f.call_async(table).await?;
            let (result, ctrl) =
                handle_blocks_return_with_control(self.lua, ret, &blocks, "Blocks")?;
            if ctrl == TraversalControl::Stop {
                return Ok(result);
            }
            result
        } else {
            blocks
        };

        let mut result = Vec::with_capacity(blocks.len());
        for block in blocks {
            let tag = block_tag(&block);
            let (spliced, ctrl) = if let Some((f, fn_name)) = block_fn(self.filter, tag) {
                let ud = self.lua.create_userdata(LuaBlock::new(block.clone()))?;
                let ret: MultiValue = f.call_async(ud).await?;
                handle_block_return_with_control(self.lua, ret, &block, fn_name)?
            } else {
                (vec![block], TraversalControl::Continue)
            };
            match ctrl {
                TraversalControl::Stop => result.extend(spliced),
                TraversalControl::Continue => {
                    for elem in spliced {
                        result.push(walk_block_children(self, elem).await?);
                    }
                }
            }
        }
        Ok(result)
    }
}

// ============================================================================
// Topdown entry points
// ============================================================================

pub(crate) async fn topdown_blocks(
    lua: &Lua,
    filter: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    let walk = TopdownWalk { lua, filter };
    walk.walk_blocks(blocks.to_vec()).await
}

pub(crate) async fn topdown_inlines(
    lua: &Lua,
    filter: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    let walk = TopdownWalk { lua, filter };
    walk.walk_inlines(inlines.to_vec()).await
}

pub(crate) async fn topdown_block_element(
    lua: &Lua,
    filter: &Table,
    block: &Block,
) -> Result<Block> {
    let walk = TopdownWalk { lua, filter };
    walk_block_children(&walk, block.clone()).await
}

/// Topdown walk of a table Cell's contents (`Cell:walk`).
pub(crate) async fn topdown_cell(
    lua: &Lua,
    filter: &Table,
    cell: &crate::pandoc::Cell,
) -> Result<crate::pandoc::Cell> {
    let walk = TopdownWalk { lua, filter };
    walk_cell_children(&walk, cell.clone()).await
}

/// Topdown walk of a table Row's cells (`Row:walk`).
pub(crate) async fn topdown_row(lua: &Lua, filter: &Table, row: &Row) -> Result<Row> {
    let walk = TopdownWalk { lua, filter };
    walk_rows(&walk, vec![row.clone()])
        .await?
        .pop()
        .ok_or_else(|| mlua::Error::runtime("walk_rows dropped a row"))
}

pub(crate) async fn topdown_inline_element(
    lua: &Lua,
    filter: &Table,
    inline: &Inline,
) -> Result<Inline> {
    let walk = TopdownWalk { lua, filter };
    walk_inline_children(&walk, inline.clone()).await
}
