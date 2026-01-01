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
use mlua::{Error, IntoLua, Lua, Result, Table as LuaTable, Value};
use std::sync::Arc;

use super::mediabag::SharedMediaBag;
use super::runtime::SystemRuntime;

use crate::pandoc::{
    Block, BlockQuote, BulletList, Caption, Citation, CitationMode, Cite, CodeBlock,
    DefinitionList, Div, Emph, Figure, Header, HorizontalRule, Image, Inline, LineBlock, LineBreak,
    Link, Math, MathType, Note, OrderedList, Paragraph, Plain, QuoteType, Quoted, RawBlock,
    RawInline, SmallCaps, SoftBreak, Space, Span, Str, Strikeout, Strong, Subscript, Superscript,
    Underline,
    attr::AttrSourceInfo,
    list::{ListAttributes, ListNumberDelim, ListNumberStyle},
    table::{
        Alignment, Cell, ColSpec, ColWidth, Row, Table as PandocTable, TableBody, TableFoot,
        TableHead,
    },
};

use super::list::{
    get_or_create_blocks_metatable, get_or_create_inlines_metatable, get_or_create_list_metatable,
};
use super::types::{
    LuaAttr, LuaBlock, LuaInline, filter_source_info, lua_table_to_blocks, lua_table_to_inlines,
};
use mlua::UserData;

// Lua userdata wrappers for table-related types

/// Wrapper for Caption
#[derive(Debug, Clone)]
pub struct LuaCaption(pub Caption);

impl UserData for LuaCaption {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            mlua::MetaMethod::Index,
            |lua, this, key: String| match key.as_str() {
                "short" => match &this.0.short {
                    Some(inlines) => super::types::inlines_to_lua_table(lua, inlines),
                    None => Ok(Value::Nil),
                },
                "long" => match &this.0.long {
                    Some(blocks) => super::types::blocks_to_lua_table(lua, blocks),
                    None => Ok(Value::Nil),
                },
                "t" | "tag" => "Caption".into_lua(lua),
                _ => Ok(Value::Nil),
            },
        );
    }
}

/// Wrapper for Alignment sentinel values
#[derive(Debug, Clone)]
pub struct LuaAlignment(pub Alignment);

impl UserData for LuaAlignment {}

/// Wrapper for ColWidth sentinel values
#[derive(Debug, Clone)]
pub struct LuaColWidth(pub ColWidth);

impl UserData for LuaColWidth {}

/// Wrapper for TableHead
#[derive(Debug, Clone)]
pub struct LuaTableHead(pub TableHead);

impl UserData for LuaTableHead {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            mlua::MetaMethod::Index,
            |lua, this, key: String| match key.as_str() {
                "rows" => {
                    let table = lua.create_table()?;
                    for (i, row) in this.0.rows.iter().enumerate() {
                        table.set(i + 1, lua.create_userdata(LuaRow(row.clone()))?)?;
                    }
                    Ok(Value::Table(table))
                }
                "attr" => super::types::attr_to_lua_userdata(lua, &this.0.attr),
                "t" | "tag" => "TableHead".into_lua(lua),
                _ => Ok(Value::Nil),
            },
        );
    }
}

/// Wrapper for TableFoot
#[derive(Debug, Clone)]
pub struct LuaTableFoot(pub TableFoot);

impl UserData for LuaTableFoot {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            mlua::MetaMethod::Index,
            |lua, this, key: String| match key.as_str() {
                "rows" => {
                    let table = lua.create_table()?;
                    for (i, row) in this.0.rows.iter().enumerate() {
                        table.set(i + 1, lua.create_userdata(LuaRow(row.clone()))?)?;
                    }
                    Ok(Value::Table(table))
                }
                "attr" => super::types::attr_to_lua_userdata(lua, &this.0.attr),
                "t" | "tag" => "TableFoot".into_lua(lua),
                _ => Ok(Value::Nil),
            },
        );
    }
}

/// Wrapper for TableBody
#[derive(Debug, Clone)]
pub struct LuaTableBody(pub TableBody);

impl UserData for LuaTableBody {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            mlua::MetaMethod::Index,
            |lua, this, key: String| match key.as_str() {
                "body" => {
                    let table = lua.create_table()?;
                    for (i, row) in this.0.body.iter().enumerate() {
                        table.set(i + 1, lua.create_userdata(LuaRow(row.clone()))?)?;
                    }
                    Ok(Value::Table(table))
                }
                "head" => {
                    let table = lua.create_table()?;
                    for (i, row) in this.0.head.iter().enumerate() {
                        table.set(i + 1, lua.create_userdata(LuaRow(row.clone()))?)?;
                    }
                    Ok(Value::Table(table))
                }
                "row_head_columns" => (this.0.rowhead_columns as i64).into_lua(lua),
                "attr" => super::types::attr_to_lua_userdata(lua, &this.0.attr),
                "t" | "tag" => "TableBody".into_lua(lua),
                _ => Ok(Value::Nil),
            },
        );
    }
}

/// Wrapper for Row
#[derive(Debug, Clone)]
pub struct LuaRow(pub Row);

impl UserData for LuaRow {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            mlua::MetaMethod::Index,
            |lua, this, key: String| match key.as_str() {
                "cells" => {
                    let table = lua.create_table()?;
                    for (i, cell) in this.0.cells.iter().enumerate() {
                        table.set(i + 1, lua.create_userdata(LuaCell(cell.clone()))?)?;
                    }
                    Ok(Value::Table(table))
                }
                "attr" => super::types::attr_to_lua_userdata(lua, &this.0.attr),
                "t" | "tag" => "Row".into_lua(lua),
                _ => Ok(Value::Nil),
            },
        );
    }
}

/// Wrapper for Cell
#[derive(Debug, Clone)]
pub struct LuaCell(pub Cell);

impl UserData for LuaCell {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            mlua::MetaMethod::Index,
            |lua, this, key: String| match key.as_str() {
                "content" => super::types::blocks_to_lua_table(lua, &this.0.content),
                "alignment" => {
                    let align_str = match this.0.alignment {
                        Alignment::Default => "AlignDefault",
                        Alignment::Left => "AlignLeft",
                        Alignment::Center => "AlignCenter",
                        Alignment::Right => "AlignRight",
                    };
                    align_str.into_lua(lua)
                }
                "row_span" => (this.0.row_span as i64).into_lua(lua),
                "col_span" => (this.0.col_span as i64).into_lua(lua),
                "attr" => super::types::attr_to_lua_userdata(lua, &this.0.attr),
                "t" | "tag" => "Cell".into_lua(lua),
                _ => Ok(Value::Nil),
            },
        );
    }
}

/// Wrapper for ListAttributes
#[derive(Debug, Clone)]
pub struct LuaListAttributes(pub ListAttributes);

impl UserData for LuaListAttributes {}

/// Register the pandoc namespace with element constructors
pub fn register_pandoc_namespace(
    lua: &Lua,
    runtime: Arc<dyn SystemRuntime>,
    mediabag: SharedMediaBag,
) -> Result<()> {
    let pandoc = lua.create_table()?;

    // Inline constructors
    register_inline_constructors(lua, &pandoc)?;

    // Block constructors
    register_block_constructors(lua, &pandoc)?;

    // Attr constructor
    register_attr_constructor(lua, &pandoc)?;

    // List constructors
    register_list_constructors(lua, &pandoc)?;

    // Utils namespace
    super::utils::register_pandoc_utils(lua, &pandoc)?;

    // Text namespace (UTF-8 aware string functions)
    super::text::register_pandoc_text(lua, &pandoc)?;

    // JSON namespace
    super::json::register_pandoc_json(lua, &pandoc)?;

    // Path namespace (path manipulation functions)
    super::path::register_pandoc_path(lua, &pandoc, runtime.clone())?;

    // System namespace (system operations via SystemRuntime)
    super::system::register_pandoc_system(lua, &pandoc, runtime.clone())?;

    // MediaBag namespace (media storage and manipulation)
    super::mediabag::register_pandoc_mediabag(lua, &pandoc, runtime, mediabag)?;

    // Read/Write functions (pandoc.read, pandoc.write, and option constructors)
    super::readwrite::register_pandoc_readwrite(lua, &pandoc)?;

    // Set as global
    lua.globals().set("pandoc", pandoc)?;

    // Register the quarto namespace (includes quarto.warn, quarto.error)
    super::diagnostics::register_quarto_namespace(lua)?;

    Ok(())
}

fn register_inline_constructors(lua: &Lua, pandoc: &LuaTable) -> Result<()> {
    // pandoc.Str(text)
    pandoc.set(
        "Str",
        lua.create_function(|lua, text: String| {
            lua.create_userdata(LuaInline(Inline::Str(Str {
                text,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Space()
    pandoc.set(
        "Space",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaInline(Inline::Space(Space {
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.SoftBreak()
    pandoc.set(
        "SoftBreak",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaInline(Inline::SoftBreak(SoftBreak {
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.LineBreak()
    pandoc.set(
        "LineBreak",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaInline(Inline::LineBreak(LineBreak {
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                    source_info: filter_source_info(lua),
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
                    source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Cite(citations, content)
    pandoc.set(
        "Cite",
        lua.create_function(|lua, (citations, content): (Value, Value)| {
            let citations = parse_citations(lua, citations)?;
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaInline(Inline::Cite(Cite {
                citations,
                content: inlines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    Ok(())
}

fn register_block_constructors(lua: &Lua, pandoc: &LuaTable) -> Result<()> {
    // pandoc.Para(content)
    pandoc.set(
        "Para",
        lua.create_function(|lua, content: Value| {
            let inlines = lua_table_to_inlines(lua, content)?;
            lua.create_userdata(LuaBlock(Block::Paragraph(Paragraph {
                content: inlines,
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
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
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })))
        })?,
    )?;

    // pandoc.HorizontalRule()
    pandoc.set(
        "HorizontalRule",
        lua.create_function(|lua, ()| {
            lua.create_userdata(LuaBlock(Block::HorizontalRule(HorizontalRule {
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.DefinitionList(content)
    // content is a list of {term, definitions} pairs
    // where term is a list of inlines and definitions is a list of list of blocks
    pandoc.set(
        "DefinitionList",
        lua.create_function(|lua, content: Value| {
            let items = parse_definition_list_items(lua, content)?;
            lua.create_userdata(LuaBlock(Block::DefinitionList(DefinitionList {
                content: items,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.LineBlock(content)
    // content is a list of lines, where each line is a list of inlines
    pandoc.set(
        "LineBlock",
        lua.create_function(|lua, content: Value| {
            let lines = parse_line_block_content(lua, content)?;
            lua.create_userdata(LuaBlock(Block::LineBlock(LineBlock {
                content: lines,
                source_info: filter_source_info(lua),
            })))
        })?,
    )?;

    // pandoc.Figure(content, caption?, attr?)
    pandoc.set(
        "Figure",
        lua.create_function(
            |lua, (content, caption, attr): (Value, Option<Value>, Option<Value>)| {
                let blocks = lua_table_to_blocks(lua, content)?;
                let caption = parse_caption(lua, caption)?;
                let attr = parse_attr(lua, attr)?;
                lua.create_userdata(LuaBlock(Block::Figure(Figure {
                    content: blocks,
                    caption,
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                })))
            },
        )?,
    )?;

    // pandoc.Table(caption, colspecs, head, bodies, foot, attr?)
    pandoc.set(
        "Table",
        lua.create_function(
            |lua,
             (caption, colspecs, head, bodies, foot, attr): (
                Value,
                Value,
                Value,
                Value,
                Value,
                Option<Value>,
            )| {
                let caption = parse_caption(lua, Some(caption))?;
                let colspecs = parse_colspecs(lua, colspecs)?;
                let head = parse_table_head(lua, head)?;
                let bodies = parse_table_bodies(lua, bodies)?;
                let foot = parse_table_foot(lua, foot)?;
                let attr = parse_attr(lua, attr)?;
                lua.create_userdata(LuaBlock(Block::Table(PandocTable {
                    caption,
                    colspec: colspecs,
                    head,
                    bodies,
                    foot,
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                })))
            },
        )?,
    )?;

    Ok(())
}

/// Parse optional attr argument into Attr tuple
fn parse_attr(_lua: &Lua, attr: Option<Value>) -> Result<crate::pandoc::Attr> {
    match attr {
        None => Ok((String::new(), vec![], LinkedHashMap::new())),
        Some(Value::UserData(ud)) => {
            // Support LuaAttr userdata
            let lua_attr = ud.borrow::<LuaAttr>()?;
            Ok(lua_attr.0.clone())
        }
        Some(Value::Table(table)) => {
            // Support table format: {identifier, classes, attributes}
            let identifier: String = table.get("identifier").unwrap_or_default();
            let classes: Vec<String> = table
                .get::<Option<LuaTable>>("classes")?
                .map(|t| {
                    t.sequence_values::<String>()
                        .filter_map(|r| r.ok())
                        .collect()
                })
                .unwrap_or_default();
            let attributes: LinkedHashMap<String, String> = table
                .get::<Option<LuaTable>>("attributes")?
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

/// Parse citations from Lua table
fn parse_citations(lua: &Lua, val: Value) -> Result<Vec<Citation>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let citation = parse_single_citation(lua, item)?;
                result.push(citation);
            }
            Ok(result)
        }
        _ => Err(Error::runtime("expected table of citations")),
    }
}

/// Parse a single Citation from a Lua table
fn parse_single_citation(lua: &Lua, val: Value) -> Result<Citation> {
    match val {
        Value::Table(table) => {
            let id: String = table.get("id")?;
            let mode_str: String = table
                .get("mode")
                .unwrap_or_else(|_| "NormalCitation".to_string());
            let mode = match mode_str.as_str() {
                "AuthorInText" => CitationMode::AuthorInText,
                "SuppressAuthor" => CitationMode::SuppressAuthor,
                _ => CitationMode::NormalCitation,
            };
            let prefix: Value = table
                .get("prefix")
                .unwrap_or(Value::Table(lua.create_table()?));
            let prefix = lua_table_to_inlines(lua, prefix).unwrap_or_default();
            let suffix: Value = table
                .get("suffix")
                .unwrap_or(Value::Table(lua.create_table()?));
            let suffix = lua_table_to_inlines(lua, suffix).unwrap_or_default();
            let note_num: i64 = table.get("note_num").unwrap_or(0);
            let hash: i64 = table.get("hash").unwrap_or(0);

            Ok(Citation {
                id,
                mode,
                prefix,
                suffix,
                note_num: note_num as usize,
                hash: hash as usize,
                id_source: None,
            })
        }
        _ => Err(Error::runtime("expected citation table")),
    }
}

/// Parse definition list items: list of {term, definitions}
fn parse_definition_list_items(
    lua: &Lua,
    val: Value,
) -> Result<Vec<(Vec<Inline>, Vec<Vec<Block>>)>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                match item {
                    Value::Table(pair) => {
                        // First element is term (inlines), second is definitions (list of blocks)
                        let term_val: Value = pair.get(1)?;
                        let term = lua_table_to_inlines(lua, term_val)?;
                        let defs_val: Value = pair.get(2)?;
                        let defs = parse_list_items(lua, defs_val)?;
                        result.push((term, defs));
                    }
                    _ => return Err(Error::runtime("expected definition list item as table")),
                }
            }
            Ok(result)
        }
        _ => Err(Error::runtime("expected table for definition list")),
    }
}

/// Parse line block content: list of lines (each line is a list of inlines)
fn parse_line_block_content(lua: &Lua, val: Value) -> Result<Vec<Vec<Inline>>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let inlines = lua_table_to_inlines(lua, item)?;
                result.push(inlines);
            }
            Ok(result)
        }
        _ => Err(Error::runtime("expected table of lines")),
    }
}

/// Parse Caption from Lua value
fn parse_caption(lua: &Lua, val: Option<Value>) -> Result<Caption> {
    match val {
        None | Some(Value::Nil) => Ok(Caption {
            short: None,
            long: None,
            source_info: filter_source_info(lua),
        }),
        Some(Value::Table(table)) => {
            let short_val: Option<Value> = table.get("short").ok();
            let short = match short_val {
                Some(Value::Table(_)) => Some(lua_table_to_inlines(lua, short_val.unwrap())?),
                Some(Value::Nil) | None => None,
                _ => None,
            };
            let long_val: Option<Value> = table.get("long").ok();
            let long = match long_val {
                Some(Value::Table(_)) => Some(lua_table_to_blocks(lua, long_val.unwrap())?),
                Some(Value::Nil) | None => None,
                _ => None,
            };
            Ok(Caption {
                short,
                long,
                source_info: filter_source_info(lua),
            })
        }
        Some(Value::UserData(ud)) => {
            // If it's a LuaCaption userdata
            if let Ok(lua_caption) = ud.borrow::<LuaCaption>() {
                Ok(lua_caption.0.clone())
            } else {
                Err(Error::runtime("expected Caption userdata"))
            }
        }
        _ => Err(Error::runtime("expected caption table or nil")),
    }
}

/// Parse column specifications
fn parse_colspecs(_lua: &Lua, val: Value) -> Result<Vec<ColSpec>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                match item {
                    Value::Table(spec) => {
                        let align_val: Value = spec.get(1)?;
                        let width_val: Value = spec.get(2)?;
                        let alignment = parse_alignment(align_val)?;
                        let width = parse_col_width(width_val)?;
                        result.push((alignment, width));
                    }
                    _ => return Err(Error::runtime("expected colspec as table")),
                }
            }
            Ok(result)
        }
        _ => Err(Error::runtime("expected table of colspecs")),
    }
}

/// Parse alignment value
fn parse_alignment(val: Value) -> Result<Alignment> {
    match val {
        Value::String(s) => {
            let s = s.to_str()?;
            match s.as_ref() {
                "AlignLeft" => Ok(Alignment::Left),
                "AlignCenter" => Ok(Alignment::Center),
                "AlignRight" => Ok(Alignment::Right),
                _ => Ok(Alignment::Default),
            }
        }
        Value::UserData(ud) => {
            // Check if it's a sentinel value like pandoc.AlignDefault
            if let Ok(align) = ud.borrow::<LuaAlignment>() {
                Ok(align.0.clone())
            } else {
                Ok(Alignment::Default)
            }
        }
        _ => Ok(Alignment::Default),
    }
}

/// Parse column width value
fn parse_col_width(val: Value) -> Result<ColWidth> {
    match val {
        Value::Number(n) => Ok(ColWidth::Percentage(n)),
        Value::Integer(i) => Ok(ColWidth::Percentage(i as f64)),
        Value::UserData(ud) => {
            if let Ok(width) = ud.borrow::<LuaColWidth>() {
                Ok(width.0.clone())
            } else {
                Ok(ColWidth::Default)
            }
        }
        _ => Ok(ColWidth::Default),
    }
}

/// Parse TableHead from Lua value
fn parse_table_head(lua: &Lua, val: Value) -> Result<TableHead> {
    match val {
        Value::Table(table) => {
            // Check if it has a 'rows' field (userdata-style) or is just a list of rows
            let rows_val: Value = table
                .get("rows")
                .unwrap_or_else(|_| Value::Table(table.clone()));
            let rows = parse_rows(lua, rows_val)?;
            let attr = match table.get::<Option<Value>>("attr")? {
                Some(v) => parse_attr(lua, Some(v))?,
                None => (String::new(), vec![], LinkedHashMap::new()),
            };
            Ok(TableHead {
                rows,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Value::UserData(ud) => {
            if let Ok(head) = ud.borrow::<LuaTableHead>() {
                Ok(head.0.clone())
            } else {
                Err(Error::runtime("expected TableHead userdata"))
            }
        }
        _ => Err(Error::runtime("expected table or TableHead")),
    }
}

/// Parse TableFoot from Lua value
fn parse_table_foot(lua: &Lua, val: Value) -> Result<TableFoot> {
    match val {
        Value::Table(table) => {
            let rows_val: Value = table
                .get("rows")
                .unwrap_or_else(|_| Value::Table(table.clone()));
            let rows = parse_rows(lua, rows_val)?;
            let attr = match table.get::<Option<Value>>("attr")? {
                Some(v) => parse_attr(lua, Some(v))?,
                None => (String::new(), vec![], LinkedHashMap::new()),
            };
            Ok(TableFoot {
                rows,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Value::UserData(ud) => {
            if let Ok(foot) = ud.borrow::<LuaTableFoot>() {
                Ok(foot.0.clone())
            } else {
                Err(Error::runtime("expected TableFoot userdata"))
            }
        }
        _ => Err(Error::runtime("expected table or TableFoot")),
    }
}

/// Parse list of TableBody from Lua value
fn parse_table_bodies(lua: &Lua, val: Value) -> Result<Vec<TableBody>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let body = parse_single_table_body(lua, item)?;
                result.push(body);
            }
            Ok(result)
        }
        _ => Err(Error::runtime("expected table of TableBody")),
    }
}

/// Parse a single TableBody from Lua value
fn parse_single_table_body(lua: &Lua, val: Value) -> Result<TableBody> {
    match val {
        Value::Table(table) => {
            // Check for body field
            let body_val: Value = table
                .get("body")
                .unwrap_or_else(|_| Value::Table(table.clone()));
            let body = parse_rows(lua, body_val)?;
            let head_val: Value = table
                .get("head")
                .unwrap_or_else(|_| Value::Table(lua.create_table().unwrap()));
            let head = parse_rows(lua, head_val).unwrap_or_default();
            let rowhead_columns: i64 = table.get("row_head_columns").unwrap_or(0);
            let attr = match table.get::<Option<Value>>("attr")? {
                Some(v) => parse_attr(lua, Some(v))?,
                None => (String::new(), vec![], LinkedHashMap::new()),
            };
            Ok(TableBody {
                body,
                head,
                rowhead_columns: rowhead_columns as usize,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Value::UserData(ud) => {
            if let Ok(body) = ud.borrow::<LuaTableBody>() {
                Ok(body.0.clone())
            } else {
                Err(Error::runtime("expected TableBody userdata"))
            }
        }
        _ => Err(Error::runtime("expected table or TableBody")),
    }
}

/// Parse rows from Lua value
fn parse_rows(lua: &Lua, val: Value) -> Result<Vec<Row>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let row = parse_single_row(lua, item)?;
                result.push(row);
            }
            Ok(result)
        }
        _ => Ok(vec![]),
    }
}

/// Parse a single Row from Lua value
fn parse_single_row(lua: &Lua, val: Value) -> Result<Row> {
    match val {
        Value::Table(table) => {
            // Check for cells field
            let cells_val: Value = table
                .get("cells")
                .unwrap_or_else(|_| Value::Table(table.clone()));
            let cells = parse_cells(lua, cells_val)?;
            let attr = match table.get::<Option<Value>>("attr")? {
                Some(v) => parse_attr(lua, Some(v))?,
                None => (String::new(), vec![], LinkedHashMap::new()),
            };
            Ok(Row {
                cells,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Value::UserData(ud) => {
            if let Ok(row) = ud.borrow::<LuaRow>() {
                Ok(row.0.clone())
            } else {
                Err(Error::runtime("expected Row userdata"))
            }
        }
        _ => Err(Error::runtime("expected table or Row")),
    }
}

/// Parse cells from Lua value
fn parse_cells(lua: &Lua, val: Value) -> Result<Vec<Cell>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                let item = item?;
                let cell = parse_single_cell(lua, item)?;
                result.push(cell);
            }
            Ok(result)
        }
        _ => Ok(vec![]),
    }
}

/// Parse a single Cell from Lua value
fn parse_single_cell(lua: &Lua, val: Value) -> Result<Cell> {
    match val {
        Value::Table(table) => {
            // Check if it has a content field or is just a list of blocks
            let content_val: Value = table.get("content").unwrap_or_else(|_| {
                // Try to treat the table itself as blocks content
                Value::Table(table.clone())
            });
            let content = lua_table_to_blocks(lua, content_val)?;
            let align_val: Value = table.get("alignment").unwrap_or(Value::Nil);
            let alignment = parse_alignment(align_val).unwrap_or(Alignment::Default);
            let row_span: i64 = table.get("row_span").unwrap_or(1);
            let col_span: i64 = table.get("col_span").unwrap_or(1);
            let attr = match table.get::<Option<Value>>("attr")? {
                Some(v) => parse_attr(lua, Some(v))?,
                None => (String::new(), vec![], LinkedHashMap::new()),
            };
            Ok(Cell {
                content,
                alignment,
                row_span: row_span as usize,
                col_span: col_span as usize,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            })
        }
        Value::UserData(ud) => {
            if let Ok(cell) = ud.borrow::<LuaCell>() {
                Ok(cell.0.clone())
            } else {
                Err(Error::runtime("expected Cell userdata"))
            }
        }
        _ => Err(Error::runtime("expected table or Cell")),
    }
}

/// Parse ListAttributes from Lua value
fn parse_list_attributes(val: Value) -> Result<ListAttributes> {
    match val {
        Value::Table(table) => {
            let start: i64 = table.get(1).unwrap_or(1);
            let style_str: String = table.get(2).unwrap_or_else(|_| "DefaultStyle".to_string());
            let delim_str: String = table.get(3).unwrap_or_else(|_| "DefaultDelim".to_string());

            let style = match style_str.as_str() {
                "Decimal" => ListNumberStyle::Decimal,
                "LowerAlpha" => ListNumberStyle::LowerAlpha,
                "UpperAlpha" => ListNumberStyle::UpperAlpha,
                "LowerRoman" => ListNumberStyle::LowerRoman,
                "UpperRoman" => ListNumberStyle::UpperRoman,
                "Example" => ListNumberStyle::Example,
                _ => ListNumberStyle::Default,
            };

            let delim = match delim_str.as_str() {
                "Period" => ListNumberDelim::Period,
                "OneParen" => ListNumberDelim::OneParen,
                "TwoParens" => ListNumberDelim::TwoParens,
                _ => ListNumberDelim::Default,
            };

            Ok((start as usize, style, delim))
        }
        Value::UserData(ud) => {
            if let Ok(attr) = ud.borrow::<LuaListAttributes>() {
                Ok(attr.0.clone())
            } else {
                Err(Error::runtime("expected ListAttributes userdata"))
            }
        }
        _ => Ok((1, ListNumberStyle::Default, ListNumberDelim::Default)),
    }
}

/// Register the pandoc.Attr() constructor and other utility constructors
fn register_attr_constructor(lua: &Lua, pandoc: &LuaTable) -> Result<()> {
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

    // pandoc.Citation(id, mode, prefix?, suffix?, note_num?, hash?)
    pandoc.set(
        "Citation",
        lua.create_function(
            |lua,
             (id, mode, prefix, suffix, note_num, hash): (
                String,
                String,
                Option<Value>,
                Option<Value>,
                Option<i64>,
                Option<i64>,
            )| {
                let mode = match mode.as_str() {
                    "AuthorInText" => CitationMode::AuthorInText,
                    "SuppressAuthor" => CitationMode::SuppressAuthor,
                    _ => CitationMode::NormalCitation,
                };
                let prefix = match prefix {
                    Some(v) => lua_table_to_inlines(lua, v).unwrap_or_default(),
                    None => vec![],
                };
                let suffix = match suffix {
                    Some(v) => lua_table_to_inlines(lua, v).unwrap_or_default(),
                    None => vec![],
                };
                let citation = Citation {
                    id,
                    mode,
                    prefix,
                    suffix,
                    note_num: note_num.unwrap_or(0) as usize,
                    hash: hash.unwrap_or(0) as usize,
                    id_source: None,
                };
                // Return as a table so it can be used with Cite constructor
                let table = lua.create_table()?;
                table.set("id", citation.id.clone())?;
                table.set(
                    "mode",
                    match citation.mode {
                        CitationMode::AuthorInText => "AuthorInText",
                        CitationMode::SuppressAuthor => "SuppressAuthor",
                        CitationMode::NormalCitation => "NormalCitation",
                    },
                )?;
                table.set(
                    "prefix",
                    super::types::inlines_to_lua_table(lua, &citation.prefix)?,
                )?;
                table.set(
                    "suffix",
                    super::types::inlines_to_lua_table(lua, &citation.suffix)?,
                )?;
                table.set("note_num", citation.note_num as i64)?;
                table.set("hash", citation.hash as i64)?;
                Ok(table)
            },
        )?,
    )?;

    // pandoc.Caption(short?, long?)
    pandoc.set(
        "Caption",
        lua.create_function(|lua, (short, long): (Option<Value>, Option<Value>)| {
            let short_inlines = match short {
                Some(Value::Nil) | None => None,
                Some(v) => Some(lua_table_to_inlines(lua, v)?),
            };
            let long_blocks = match long {
                Some(Value::Nil) | None => None,
                Some(v) => Some(lua_table_to_blocks(lua, v)?),
            };
            let caption = Caption {
                short: short_inlines,
                long: long_blocks,
                source_info: filter_source_info(lua),
            };
            lua.create_userdata(LuaCaption(caption))
        })?,
    )?;

    // pandoc.ListAttributes(start?, style?, delim?)
    pandoc.set(
        "ListAttributes",
        lua.create_function(
            |lua, (start, style, delim): (Option<i64>, Option<String>, Option<String>)| {
                let start = start.unwrap_or(1) as usize;
                let style = match style.as_deref() {
                    Some("Decimal") => ListNumberStyle::Decimal,
                    Some("LowerAlpha") => ListNumberStyle::LowerAlpha,
                    Some("UpperAlpha") => ListNumberStyle::UpperAlpha,
                    Some("LowerRoman") => ListNumberStyle::LowerRoman,
                    Some("UpperRoman") => ListNumberStyle::UpperRoman,
                    Some("Example") => ListNumberStyle::Example,
                    _ => ListNumberStyle::Default,
                };
                let delim = match delim.as_deref() {
                    Some("Period") => ListNumberDelim::Period,
                    Some("OneParen") => ListNumberDelim::OneParen,
                    Some("TwoParens") => ListNumberDelim::TwoParens,
                    _ => ListNumberDelim::Default,
                };
                // Return as a table with positional access like Pandoc
                let table = lua.create_table()?;
                table.set(1, start as i64)?;
                table.set(
                    2,
                    match style {
                        ListNumberStyle::Decimal => "Decimal",
                        ListNumberStyle::LowerAlpha => "LowerAlpha",
                        ListNumberStyle::UpperAlpha => "UpperAlpha",
                        ListNumberStyle::LowerRoman => "LowerRoman",
                        ListNumberStyle::UpperRoman => "UpperRoman",
                        ListNumberStyle::Example => "Example",
                        ListNumberStyle::Default => "DefaultStyle",
                    },
                )?;
                table.set(
                    3,
                    match delim {
                        ListNumberDelim::Period => "Period",
                        ListNumberDelim::OneParen => "OneParen",
                        ListNumberDelim::TwoParens => "TwoParens",
                        ListNumberDelim::Default => "DefaultDelim",
                    },
                )?;
                Ok(table)
            },
        )?,
    )?;

    // Alignment sentinel values
    pandoc.set(
        "AlignDefault",
        lua.create_userdata(LuaAlignment(Alignment::Default))?,
    )?;
    pandoc.set(
        "AlignLeft",
        lua.create_userdata(LuaAlignment(Alignment::Left))?,
    )?;
    pandoc.set(
        "AlignCenter",
        lua.create_userdata(LuaAlignment(Alignment::Center))?,
    )?;
    pandoc.set(
        "AlignRight",
        lua.create_userdata(LuaAlignment(Alignment::Right))?,
    )?;

    // ColWidth sentinel values
    pandoc.set(
        "ColWidthDefault",
        lua.create_userdata(LuaColWidth(ColWidth::Default))?,
    )?;

    // pandoc.Cell(content, align?, row_span?, col_span?, attr?)
    pandoc.set(
        "Cell",
        lua.create_function(
            |lua,
             (content, align, row_span, col_span, attr): (
                Value,
                Option<Value>,
                Option<i64>,
                Option<i64>,
                Option<Value>,
            )| {
                let blocks = lua_table_to_blocks(lua, content)?;
                let alignment = match align {
                    Some(v) => parse_alignment(v).unwrap_or(Alignment::Default),
                    None => Alignment::Default,
                };
                let row_span = row_span.unwrap_or(1) as usize;
                let col_span = col_span.unwrap_or(1) as usize;
                let attr = parse_attr(lua, attr)?;
                lua.create_userdata(LuaCell(Cell {
                    content: blocks,
                    alignment,
                    row_span,
                    col_span,
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                }))
            },
        )?,
    )?;

    // pandoc.Row(cells, attr?)
    pandoc.set(
        "Row",
        lua.create_function(|lua, (cells, attr): (Value, Option<Value>)| {
            let cells = parse_cells(lua, cells)?;
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaRow(Row {
                cells,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            }))
        })?,
    )?;

    // pandoc.TableHead(rows, attr?)
    pandoc.set(
        "TableHead",
        lua.create_function(|lua, (rows, attr): (Value, Option<Value>)| {
            let rows = parse_rows(lua, rows)?;
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaTableHead(TableHead {
                rows,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            }))
        })?,
    )?;

    // pandoc.TableFoot(rows, attr?)
    pandoc.set(
        "TableFoot",
        lua.create_function(|lua, (rows, attr): (Value, Option<Value>)| {
            let rows = parse_rows(lua, rows)?;
            let attr = parse_attr(lua, attr)?;
            lua.create_userdata(LuaTableFoot(TableFoot {
                rows,
                attr,
                source_info: filter_source_info(lua),
                attr_source: AttrSourceInfo::empty(),
            }))
        })?,
    )?;

    // pandoc.TableBody(body, attr?, row_head_columns?, head?)
    pandoc.set(
        "TableBody",
        lua.create_function(
            |lua,
             (body, attr, row_head_columns, head): (
                Value,
                Option<Value>,
                Option<i64>,
                Option<Value>,
            )| {
                let body_rows = parse_rows(lua, body)?;
                let head_rows = match head {
                    Some(v) => parse_rows(lua, v).unwrap_or_default(),
                    None => vec![],
                };
                let attr = parse_attr(lua, attr)?;
                let rowhead_columns = row_head_columns.unwrap_or(0) as usize;
                lua.create_userdata(LuaTableBody(TableBody {
                    body: body_rows,
                    head: head_rows,
                    rowhead_columns,
                    attr,
                    source_info: filter_source_info(lua),
                    attr_source: AttrSourceInfo::empty(),
                }))
            },
        )?,
    )?;

    Ok(())
}

/// Register pandoc.List, pandoc.Inlines, pandoc.Blocks constructors
fn register_list_constructors(lua: &Lua, pandoc: &LuaTable) -> Result<()> {
    // pandoc.List(table?) - creates a generic List
    let list_mt = get_or_create_list_metatable(lua)?;
    pandoc.set("List", list_mt)?;

    // pandoc.Inlines(content) - creates an Inlines list
    pandoc.set(
        "Inlines",
        lua.create_function(|lua, content: Option<Value>| {
            let mt = get_or_create_inlines_metatable(lua)?;
            let table = match content {
                None | Some(Value::Nil) => lua.create_table()?,
                Some(Value::Table(t)) => {
                    // Convert table contents to proper format if needed
                    let result = lua.create_table()?;
                    let len = t.raw_len();
                    for i in 1..=len {
                        let val: Value = t.raw_get(i)?;
                        // Handle string conversion to Str
                        let inline = match val {
                            Value::String(s) => {
                                let text = s.to_str()?.to_string();
                                Value::UserData(lua.create_userdata(LuaInline(Inline::Str(
                                    Str {
                                        text,
                                        source_info: filter_source_info(lua),
                                    },
                                )))?)
                            }
                            _ => val,
                        };
                        result.raw_set(i, inline)?;
                    }
                    result
                }
                Some(Value::String(s)) => {
                    // Convert string to Inlines containing Str elements
                    let result = lua.create_table()?;
                    let text = s.to_str()?.to_string();
                    result.raw_set(
                        1,
                        lua.create_userdata(LuaInline(Inline::Str(Str {
                            text,
                            source_info: filter_source_info(lua),
                        })))?,
                    )?;
                    result
                }
                Some(Value::UserData(ud)) => {
                    // Single inline element - wrap in list
                    let result = lua.create_table()?;
                    result.raw_set(1, Value::UserData(ud))?;
                    result
                }
                Some(_) => {
                    return Err(Error::runtime(
                        "pandoc.Inlines expects a table, string, or Inline element",
                    ));
                }
            };
            table.set_metatable(Some(mt));
            Ok(table)
        })?,
    )?;

    // pandoc.Blocks(content) - creates a Blocks list
    pandoc.set(
        "Blocks",
        lua.create_function(|lua, content: Option<Value>| {
            let mt = get_or_create_blocks_metatable(lua)?;
            let table = match content {
                None | Some(Value::Nil) => lua.create_table()?,
                Some(Value::Table(t)) => t,
                Some(Value::UserData(ud)) => {
                    // Single block element - wrap in list
                    let result = lua.create_table()?;
                    result.raw_set(1, Value::UserData(ud))?;
                    result
                }
                Some(_) => {
                    return Err(Error::runtime(
                        "pandoc.Blocks expects a table or Block element",
                    ));
                }
            };
            table.set_metatable(Some(mt));
            Ok(table)
        })?,
    )?;

    Ok(())
}
