/*
 * lua/constructors.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pandoc element constructors for Lua filters.
 *
 * This module provides the `pandoc.*` namespace with element constructors
 * like `pandoc.Str()`, `pandoc.Para()`, etc.
 */

use hashlink::LinkedHashMap;
use mlua::{Error, Lua, Result, Table, Value};

use crate::pandoc::{
    Block, BlockQuote, BulletList, CodeBlock, Div, Emph, Header, HorizontalRule, Image, Inline,
    LineBreak, Link, Math, MathType, Note, OrderedList, Paragraph, Plain, QuoteType, Quoted,
    RawBlock, RawInline, SmallCaps, SoftBreak, Space, Span, Str, Strikeout, Strong, Subscript,
    Superscript, Underline, attr::AttrSourceInfo,
};

use super::types::{
    LuaAttr, LuaBlock, LuaInline, filter_source_info, lua_table_to_blocks, lua_table_to_inlines,
};

/// Register the pandoc namespace with element constructors
pub fn register_pandoc_namespace(lua: &Lua) -> Result<()> {
    let pandoc = lua.create_table()?;

    // Inline constructors
    register_inline_constructors(lua, &pandoc)?;

    // Block constructors
    register_block_constructors(lua, &pandoc)?;

    // Attr constructor
    register_attr_constructor(lua, &pandoc)?;

    // Set as global
    lua.globals().set("pandoc", pandoc)?;

    Ok(())
}

fn register_inline_constructors(lua: &Lua, pandoc: &Table) -> Result<()> {
    // pandoc.Str(text)
    pandoc.set(
        "Str",
        lua.create_function(|lua, text: String| {
            lua.create_userdata(LuaInline(Inline::Str(Str {
                text,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Space()
    pandoc.set(
        "Space",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaInline(Inline::Space(Space {
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.SoftBreak()
    pandoc.set(
        "SoftBreak",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaInline(Inline::SoftBreak(SoftBreak {
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.LineBreak()
    pandoc.set(
        "LineBreak",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaInline(Inline::LineBreak(LineBreak {
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Emph(content)
    pandoc.set(
        "Emph",
        lua.create_function(|lua, content: Value| {
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaInline(Inline::Emph(Emph {
                content: inlines,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Strong(content)
    pandoc.set(
        "Strong",
        lua.create_function(|lua, content: Value| {
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaInline(Inline::Strong(Strong {
                content: inlines,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Underline(content)
    pandoc.set(
        "Underline",
        lua.create_function(|lua, content: Value| {
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaInline(Inline::Underline(Underline {
                content: inlines,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Strikeout(content)
    pandoc.set(
        "Strikeout",
        lua.create_function(|lua, content: Value| {
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaInline(Inline::Strikeout(Strikeout {
                content: inlines,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Superscript(content)
    pandoc.set(
        "Superscript",
        lua.create_function(|lua, content: Value| {
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaInline(Inline::Superscript(Superscript {
                content: inlines,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Subscript(content)
    pandoc.set(
        "Subscript",
        lua.create_function(|lua, content: Value| {
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaInline(Inline::Subscript(Subscript {
                content: inlines,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.SmallCaps(content)
    pandoc.set(
        "SmallCaps",
        lua.create_function(|lua, content: Value| {
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaInline(Inline::SmallCaps(SmallCaps {
                content: inlines,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Quoted(quote_type, content)
    pandoc.set(
        "Quoted",
        lua.create_function(|lua, (quote_type, content): (String, Value)| {
            let qt = match quote_type.as_str() {
                "SingleQuote" => QuoteType::SingleQuote,
                "DoubleQuote" => QuoteType::DoubleQuote,
                _ => {
                    return Err(Error::runtime(format!(
                        "invalid quote type: {}",
                        quote_type
                    )));
                }
            };
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaInline(Inline::Quoted(Quoted {
                quote_type: qt,
                content: inlines,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Code(text, attr?) - attr is optional
    pandoc.set(
        "Code",
        lua.create_function(|lua, (text, attr): (String, Option<Value>)| {
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaInline(Inline::Code(crate::pandoc::Code {
                text,
                attr,
                source_info: filter_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.Math(math_type, text)
    pandoc.set(
        "Math",
        lua.create_function(|lua, (math_type, text): (String, String)| {
            let mt = match math_type.as_str() {
                "InlineMath" => MathType::InlineMath,
                "DisplayMath" => MathType::DisplayMath,
                _ => return Err(Error::runtime(format!("invalid math type: {}", math_type))),
            };
            lua.create_userdata(LuaInline(Inline::Math(Math {
                math_type: mt,
                text,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.RawInline(format, text)
    pandoc.set(
        "RawInline",
        lua.create_function(|lua, (format, text): (String, String)| {
            lua.create_userdata(LuaInline(Inline::RawInline(RawInline {
                format,
                text,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Link(content, target, title?, attr?)
    pandoc.set(
        "Link",
        lua.create_function(
            |lua, (content, target, title, attr): (Value, String, Option<String>, Option<Value>)| {
                let inlines = lua_table_to_inlines(lua, content)?;
                let title = title.unwrap_or_default();
                let attr = parse_attr(lua, attr)?;
                lua.create_userdata(LuaInline(Inline::Link(Link {
                    content: inlines,
                    target: (target, title),
                    attr,
                    source_info: filter_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: crate::pandoc::attr::TargetSourceInfo::empty(),
                })))
            },
        )?,
    )?;

    // pandoc.Image(content, src, title?, attr?)
    pandoc.set(
        "Image",
        lua.create_function(
            |lua, (content, src, title, attr): (Value, String, Option<String>, Option<Value>)| {
                let inlines = lua_table_to_inlines(lua, content)?;
                let title = title.unwrap_or_default();
                let attr = parse_attr(lua, attr)?;
                lua.create_userdata(LuaInline(Inline::Image(Image {
                    content: inlines,
                    target: (src, title),
                    attr,
                    source_info: filter_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: crate::pandoc::attr::TargetSourceInfo::empty(),
                })))
            },
        )?,
    )?;

    // pandoc.Span(content, attr?)
    pandoc.set(
        "Span",
        lua.create_function(|lua, (content, attr): (Value, Option<Value>)| {
            let inlines = lua_table_to_inlines(lua, content)?;
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaInline(Inline::Span(Span {
                content: inlines,
                attr,
                source_info: filter_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.Note(content)
    pandoc.set(
        "Note",
        lua.create_function(|lua, content: Value| {
            let blocks = lua_table_to_blocks(lua, content)?;
            lua.create_userdata(LuaInline(Inline::Note(Note {
                content: blocks,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    Ok(())
}

fn register_block_constructors(lua: &Lua, pandoc: &Table) -> Result<()> {
    // pandoc.Para(content)
    pandoc.set(
        "Para",
        lua.create_function(|lua, content: Value| {
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaBlock(Block::Paragraph(Paragraph {
                content: inlines,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Plain(content)
    pandoc.set(
        "Plain",
        lua.create_function(|lua, content: Value| {
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaBlock(Block::Plain(Plain {
                content: inlines,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Header(level, content, attr?)
    pandoc.set(
        "Header",
        lua.create_function(|lua, (level, content, attr): (i64, Value, Option<Value>)| {
            let inlines = lua_table_to_inlines(lua, content)?;
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaBlock(Block::Header(Header {
                level: level as usize,
                content: inlines,
                attr,
                source_info: filter_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.CodeBlock(text, attr?)
    pandoc.set(
        "CodeBlock",
        lua.create_function(|lua, (text, attr): (String, Option<Value>)| {
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaBlock(Block::CodeBlock(CodeBlock {
                text,
                attr,
                source_info: filter_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.RawBlock(format, text)
    pandoc.set(
        "RawBlock",
        lua.create_function(|lua, (format, text): (String, String)| {
            lua.create_userdata(LuaBlock(Block::RawBlock(RawBlock {
                format,
                text,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.BlockQuote(content)
    pandoc.set(
        "BlockQuote",
        lua.create_function(|lua, content: Value| {
            let blocks = lua_table_to_blocks(lua, content)?;
            lua.create_userdata(LuaBlock(Block::BlockQuote(BlockQuote {
                content: blocks,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.BulletList(items)
    pandoc.set(
        "BulletList",
        lua.create_function(|lua, items: Value| {
            let content = parse_list_items(lua, items)?;
            lua.create_userdata(LuaBlock(Block::BulletList(BulletList {
                content,
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.OrderedList(items, listattributes?)
    pandoc.set(
        "OrderedList",
        lua.create_function(|lua, (items, _list_attr): (Value, Option<Value>)| {
            let content = parse_list_items(lua, items)?;
            lua.create_userdata(LuaBlock(Block::OrderedList(OrderedList {
                content,
                attr: (
                    1, // start
                    crate::pandoc::ListNumberStyle::Default,
                    crate::pandoc::ListNumberDelim::Default,
                ),
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    // pandoc.Div(content, attr?)
    pandoc.set(
        "Div",
        lua.create_function(|lua, (content, attr): (Value, Option<Value>)| {
            let blocks = lua_table_to_blocks(lua, content)?;
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaBlock(Block::Div(Div {
                content: blocks,
                attr,
                source_info: filter_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.HorizontalRule()
    pandoc.set(
        "HorizontalRule",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaBlock(Block::HorizontalRule(HorizontalRule {
                source_info: filter_source_info(),
            })))
        })?,
    )?;

    Ok(())
}

/// Parse optional attr argument into Attr tuple
fn parse_attr(_lua: &Lua, attr: Option<Value>) -> Result<crate::pandoc::Attr> {
    match attr {
        None => Ok(("".to_string(), vec![], LinkedHashMap::new())),
        Some(Value::UserData(ud)) => {
            // Support LuaAttr userdata
            let lua_attr = ud.borrow::<LuaAttr>()?;
            Ok(lua_attr.0.clone())
        }
        Some(Value::Table(table)) => {
            // Support table format: {identifier, classes, attributes}
            let identifier: String = table.get("identifier").unwrap_or_default();
            let classes: Vec<String> = table
                .get::<Option<Table>>("classes")?
                .map(|t| {
                    t.sequence_values::<String>()
                        .filter_map(|r| r.ok())
                        .collect()
                })
                .unwrap_or_default();
            let attributes: LinkedHashMap<String, String> = table
                .get::<Option<Table>>("attributes")?
                .map(|t| t.pairs::<String, String>().filter_map(|r| r.ok()).collect())
                .unwrap_or_default();
            Ok((identifier, classes, attributes))
        }
        Some(Value::String(s)) => {
            // Support simple string format for identifier
            Ok((s.to_str()?.to_string(), vec![], LinkedHashMap::new()))
        }
        Some(_) => Err(Error::runtime("invalid attr format")),
    }
}

/// Parse list items (each item is a list of blocks)
fn parse_list_items(lua: &Lua, items: Value) -> Result<Vec<Vec<Block>>> {
    match items {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let blocks = lua_table_to_blocks(lua, item)?;
                result.push(blocks);
            }
            Ok(result)
        }
        _ => Err(Error::runtime("expected table of items")),
    }
}

/// Register the pandoc.Attr() constructor
fn register_attr_constructor(lua: &Lua, pandoc: &Table) -> Result<()> {
    // pandoc.Attr(identifier, classes, attributes)
    // All parameters are optional with default empty values
    pandoc.set(
        "Attr",
        lua.create_function(
            |lua, (identifier, classes, attributes): (Option<String>, Option<Value>, Option<Value>)| {
                let id = identifier.unwrap_or_default();
                let cls = match classes {
                    Some(Value::Table(table)) => {
                        let mut result = Vec::new();
                        for item in table.sequence_values::<String>() {
                            result.push(item?);
                        }
                        result
                    }
                    Some(_) => return Err(Error::runtime("classes must be a table of strings")),
                    None => Vec::new(),
                };
                let attrs = match attributes {
                    Some(Value::Table(table)) => {
                        let mut result = LinkedHashMap::new();
                        for pair in table.pairs::<String, String>() {
                            let (k, v) = pair?;
                            result.insert(k, v);
                        }
                        result
                    }
                    Some(_) => return Err(Error::runtime("attributes must be a table")),
                    None => LinkedHashMap::new(),
                };
                lua.create_userdata(LuaAttr::new((id, cls, attrs)))
            },
        )?,
    )?;

    Ok(())
}
