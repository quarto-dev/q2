/*
 * lua/utils.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pandoc utility functions for Lua filters.
 *
 * This module provides the `pandoc.utils` namespace with utility functions
 * like `pandoc.utils.stringify()`.
 */

use mlua::{Lua, Result, Table, Value};

use crate::pandoc::{Block, Inline};

use super::types::{LuaBlock, LuaInline};

/// Register the pandoc.utils namespace
pub fn register_pandoc_utils(lua: &Lua, pandoc: &Table) -> Result<()> {
    let utils = lua.create_table()?;

    // pandoc.utils.stringify(element)
    utils.set(
        "stringify",
        lua.create_function(|_lua, value: Value| {
            let result = stringify_value(&value)?;
            Ok(result)
        })?,
    )?;

    pandoc.set("utils", utils)?;

    Ok(())
}

/// Convert a Lua value (block, inline, list of elements) to plain text
fn stringify_value(value: &Value) -> Result<String> {
    match value {
        Value::UserData(ud) => {
            // Try to extract as LuaInline
            if let Ok(inline) = ud.borrow::<LuaInline>() {
                return Ok(stringify_inline(&inline.0));
            }
            // Try to extract as LuaBlock
            if let Ok(block) = ud.borrow::<LuaBlock>() {
                return Ok(stringify_block(&block.0));
            }
            Ok(String::new())
        }
        Value::Table(table) => {
            // Handle table of elements
            let mut result = String::new();
            for item in table.clone().sequence_values::<Value>() {
                let item = item?;
                result.push_str(&stringify_value(&item)?);
            }
            Ok(result)
        }
        Value::String(s) => Ok(s.to_str()?.to_string()),
        _ => Ok(String::new()),
    }
}

/// Convert a single inline element to plain text
fn stringify_inline(inline: &Inline) -> String {
    match inline {
        Inline::Str(s) => s.text.clone(),
        Inline::Space(_) => " ".to_string(),
        Inline::SoftBreak(_) => "\n".to_string(),
        Inline::LineBreak(_) => "\n".to_string(),
        Inline::Emph(e) => stringify_inlines(&e.content),
        Inline::Strong(s) => stringify_inlines(&s.content),
        Inline::Underline(u) => stringify_inlines(&u.content),
        Inline::Strikeout(s) => stringify_inlines(&s.content),
        Inline::Superscript(s) => stringify_inlines(&s.content),
        Inline::Subscript(s) => stringify_inlines(&s.content),
        Inline::SmallCaps(s) => stringify_inlines(&s.content),
        Inline::Quoted(q) => {
            let content = stringify_inlines(&q.content);
            format!("\"{}\"", content)
        }
        Inline::Code(c) => c.text.clone(),
        Inline::Math(m) => m.text.clone(),
        Inline::RawInline(_) => String::new(), // Raw content is dropped
        Inline::Link(l) => stringify_inlines(&l.content),
        Inline::Image(i) => stringify_inlines(&i.content),
        Inline::Span(s) => stringify_inlines(&s.content),
        Inline::Note(n) => stringify_blocks(&n.content),
        Inline::Cite(c) => stringify_inlines(&c.content),
        // Additional inline types
        Inline::Shortcode(_) => String::new(),
        Inline::NoteReference(_) => String::new(),
        Inline::Attr(_, _) => String::new(),
        Inline::Insert(i) => stringify_inlines(&i.content),
        Inline::Delete(d) => stringify_inlines(&d.content),
        Inline::Highlight(h) => stringify_inlines(&h.content),
        Inline::EditComment(_) => String::new(),
    }
}

/// Convert a list of inline elements to plain text
fn stringify_inlines(inlines: &[Inline]) -> String {
    inlines.iter().map(stringify_inline).collect()
}

/// Convert a single block element to plain text
fn stringify_block(block: &Block) -> String {
    match block {
        Block::Paragraph(p) => stringify_inlines(&p.content),
        Block::Plain(p) => stringify_inlines(&p.content),
        Block::Header(h) => stringify_inlines(&h.content),
        Block::CodeBlock(c) => c.text.clone(),
        Block::RawBlock(_) => String::new(), // Raw content is dropped
        Block::BlockQuote(b) => stringify_blocks(&b.content),
        Block::BulletList(l) => l
            .content
            .iter()
            .map(|items| stringify_blocks(items))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::OrderedList(l) => l
            .content
            .iter()
            .map(|items| stringify_blocks(items))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::DefinitionList(d) => d
            .content
            .iter()
            .map(|(term, defs)| {
                let term_str = stringify_inlines(term);
                let defs_str = defs
                    .iter()
                    .map(|def| stringify_blocks(def))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{}: {}", term_str, defs_str)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Div(d) => stringify_blocks(&d.content),
        Block::LineBlock(l) => l
            .content
            .iter()
            .map(|line| stringify_inlines(line))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Table(t) => {
            // Stringify table caption
            let mut result = String::new();
            if let Some(ref long) = t.caption.long {
                result.push_str(&stringify_blocks(long));
            }
            result
        }
        Block::Figure(f) => {
            let mut result = stringify_blocks(&f.content);
            if let Some(ref long) = f.caption.long {
                result.push_str(&stringify_blocks(long));
            }
            result
        }
        Block::HorizontalRule(_) => String::new(),
        Block::CaptionBlock(c) => stringify_inlines(&c.content),
        // Additional block types
        Block::BlockMetadata(_) => String::new(),
        Block::NoteDefinitionPara(n) => stringify_inlines(&n.content),
        Block::NoteDefinitionFencedBlock(n) => stringify_blocks(&n.content),
    }
}

/// Convert a list of block elements to plain text
fn stringify_blocks(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(stringify_block)
        .collect::<Vec<_>>()
        .join("\n")
}
