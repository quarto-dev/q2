/*
 * lua/filter.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Lua filter execution engine.
 *
 * This module handles loading and executing Lua filters, supporting:
 * - Typewise traversal (default): call functions for each element type
 * - Filter return semantics: nil=unchanged, element=replace, list=splice, {}=delete
 */

use mlua::{Function, Lua, Result, Table, Value};
use std::path::Path;

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::{Block, Inline, Pandoc};

use super::constructors::register_pandoc_namespace;
use super::types::{LuaBlock, LuaInline, blocks_to_lua_table, inlines_to_lua_table};

/// Errors that can occur during Lua filter execution
#[derive(Debug)]
pub enum LuaFilterError {
    /// Failed to read the filter file
    FileReadError(std::path::PathBuf, std::io::Error),
    /// Lua execution error
    LuaError(mlua::Error),
    /// Filter returned invalid type
    InvalidReturn(String),
}

impl std::fmt::Display for LuaFilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LuaFilterError::FileReadError(path, err) => {
                write!(f, "Failed to read filter '{}': {}", path.display(), err)
            }
            LuaFilterError::LuaError(err) => write!(f, "Lua filter error: {}", err),
            LuaFilterError::InvalidReturn(msg) => write!(f, "Invalid filter return: {}", msg),
        }
    }
}

impl std::error::Error for LuaFilterError {}

impl From<mlua::Error> for LuaFilterError {
    fn from(err: mlua::Error) -> Self {
        LuaFilterError::LuaError(err)
    }
}

/// Result type for filter operations
pub type FilterResult<T> = std::result::Result<T, LuaFilterError>;

/// Represents what a filter function returned
enum FilterReturn<T> {
    /// nil - element unchanged
    Unchanged,
    /// Single element - replace
    Replace(T),
    /// Empty table - delete element
    Delete,
    /// Table of elements - splice
    Splice(Vec<T>),
}

/// Apply a single Lua filter to a document
pub fn apply_lua_filter(
    pandoc: &Pandoc,
    context: &ASTContext,
    filter_path: &Path,
    target_format: &str,
) -> FilterResult<(Pandoc, ASTContext)> {
    // Read filter file
    let filter_source = std::fs::read_to_string(filter_path)
        .map_err(|e| LuaFilterError::FileReadError(filter_path.to_owned(), e))?;

    // Create Lua state
    let lua = Lua::new();

    // Register pandoc namespace with constructors
    register_pandoc_namespace(&lua)?;

    // Set global variables
    // FORMAT - the target output format (html, latex, etc.)
    lua.globals().set("FORMAT", target_format)?;

    // PANDOC_VERSION - version of Pandoc (we emulate 3.x behavior)
    // Set as a table with numeric indices for version components
    let version_table = lua.create_table()?;
    version_table.set(1, 3)?;
    version_table.set(2, 0)?;
    version_table.set(3, 0)?;
    lua.globals().set("PANDOC_VERSION", version_table)?;

    // PANDOC_API_VERSION - version of the pandoc-types API
    let api_version_table = lua.create_table()?;
    api_version_table.set(1, 1)?;
    api_version_table.set(2, 23)?;
    api_version_table.set(3, 1)?;
    lua.globals().set("PANDOC_API_VERSION", api_version_table)?;

    // PANDOC_SCRIPT_FILE - path to the current filter script
    lua.globals().set(
        "PANDOC_SCRIPT_FILE",
        filter_path.to_string_lossy().to_string(),
    )?;

    // Load and execute filter script
    lua.load(&filter_source)
        .set_name(filter_path.to_string_lossy())
        .exec()?;

    // Get filter functions from globals or return value
    let filter_table = get_filter_table(&lua)?;

    // Apply the filter
    let filtered_blocks = apply_filter_to_blocks(&lua, &filter_table, &pandoc.blocks)?;

    // Return filtered document
    let filtered_pandoc = Pandoc {
        meta: pandoc.meta.clone(),
        blocks: filtered_blocks,
    };

    Ok((filtered_pandoc, context.clone()))
}

/// Apply multiple Lua filters in sequence
pub fn apply_lua_filters(
    pandoc: Pandoc,
    context: ASTContext,
    filter_paths: &[std::path::PathBuf],
    target_format: &str,
) -> FilterResult<(Pandoc, ASTContext)> {
    let mut current_pandoc = pandoc;
    let mut current_context = context;

    for filter_path in filter_paths {
        let (new_pandoc, new_context) = apply_lua_filter(
            &current_pandoc,
            &current_context,
            filter_path,
            target_format,
        )?;
        current_pandoc = new_pandoc;
        current_context = new_context;
    }

    Ok((current_pandoc, current_context))
}

/// Get the filter table from Lua (either from return value or globals)
fn get_filter_table(lua: &Lua) -> Result<Table> {
    // Pandoc filters can either:
    // 1. Return a table with filter functions
    // 2. Define filter functions as globals
    // We'll support both by creating a table that checks globals
    let globals = lua.globals();

    // Create a filter table that wraps globals
    let filter_table = lua.create_table()?;

    // Copy relevant filter functions from globals
    let filter_names = [
        // Inline types
        "Str",
        "Emph",
        "Strong",
        "Underline",
        "Strikeout",
        "Superscript",
        "Subscript",
        "SmallCaps",
        "Quoted",
        "Cite",
        "Code",
        "Space",
        "SoftBreak",
        "LineBreak",
        "Math",
        "RawInline",
        "Link",
        "Image",
        "Note",
        "Span",
        "Inline",
        "Inlines",
        // Block types
        "Para",
        "Plain",
        "CodeBlock",
        "RawBlock",
        "BlockQuote",
        "OrderedList",
        "BulletList",
        "DefinitionList",
        "Header",
        "HorizontalRule",
        "Table",
        "Figure",
        "Div",
        "LineBlock",
        "Block",
        "Blocks",
        // Document-level
        "Pandoc",
        "Doc",
    ];

    for name in &filter_names {
        if let Ok(func) = globals.get::<Function>(*name) {
            filter_table.set(*name, func)?;
        }
    }

    Ok(filter_table)
}

/// Apply filter to a list of blocks
fn apply_filter_to_blocks(lua: &Lua, filter_table: &Table, blocks: &[Block]) -> Result<Vec<Block>> {
    let mut result = Vec::new();

    // Check for Blocks filter
    if let Ok(blocks_fn) = filter_table.get::<Function>("Blocks") {
        let blocks_table = blocks_to_lua_table(lua, blocks)?;
        let ret: Value = blocks_fn.call(blocks_table)?;
        match handle_blocks_return(ret)? {
            FilterReturn::Unchanged => {
                // Continue with individual block processing
            }
            FilterReturn::Replace(block) => {
                // Single block returned - process it
                result.extend(apply_filter_to_block(lua, filter_table, &block)?);
                return Ok(result);
            }
            FilterReturn::Splice(blocks) => {
                // Table of blocks returned - process each
                for block in blocks {
                    result.extend(apply_filter_to_block(lua, filter_table, &block)?);
                }
                return Ok(result);
            }
            FilterReturn::Delete => return Ok(vec![]),
        }
    }

    // Process each block
    for block in blocks {
        result.extend(apply_filter_to_block(lua, filter_table, block)?);
    }

    Ok(result)
}

/// Apply filter to a single block
fn apply_filter_to_block(lua: &Lua, filter_table: &Table, block: &Block) -> Result<Vec<Block>> {
    // First, recursively process children
    let block_with_filtered_children = filter_block_children(lua, filter_table, block)?;

    // Then apply type-specific filter
    let tag = block_tag(&block_with_filtered_children);

    // Try type-specific function first
    if let Ok(filter_fn) = filter_table.get::<Function>(tag) {
        let block_ud = lua.create_userdata(LuaBlock(block_with_filtered_children.clone()))?;
        let ret: Value = filter_fn.call(block_ud)?;
        return handle_block_return(ret, &block_with_filtered_children);
    }

    // Try generic Block function
    if let Ok(filter_fn) = filter_table.get::<Function>("Block") {
        let block_ud = lua.create_userdata(LuaBlock(block_with_filtered_children.clone()))?;
        let ret: Value = filter_fn.call(block_ud)?;
        return handle_block_return(ret, &block_with_filtered_children);
    }

    // No filter, return unchanged
    Ok(vec![block_with_filtered_children])
}

/// Recursively filter children of a block
fn filter_block_children(lua: &Lua, filter_table: &Table, block: &Block) -> Result<Block> {
    match block {
        Block::Paragraph(p) => {
            let filtered_content = apply_filter_to_inlines(lua, filter_table, &p.content)?;
            Ok(Block::Paragraph(crate::pandoc::Paragraph {
                content: filtered_content,
                ..p.clone()
            }))
        }
        Block::Plain(p) => {
            let filtered_content = apply_filter_to_inlines(lua, filter_table, &p.content)?;
            Ok(Block::Plain(crate::pandoc::Plain {
                content: filtered_content,
                ..p.clone()
            }))
        }
        Block::Header(h) => {
            let filtered_content = apply_filter_to_inlines(lua, filter_table, &h.content)?;
            Ok(Block::Header(crate::pandoc::Header {
                content: filtered_content,
                ..h.clone()
            }))
        }
        Block::BlockQuote(b) => {
            let filtered_content = apply_filter_to_blocks(lua, filter_table, &b.content)?;
            Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                content: filtered_content,
                ..b.clone()
            }))
        }
        Block::BulletList(l) => {
            let filtered_items: Vec<Vec<Block>> = l
                .content
                .iter()
                .map(|item| apply_filter_to_blocks(lua, filter_table, item))
                .collect::<Result<_>>()?;
            Ok(Block::BulletList(crate::pandoc::BulletList {
                content: filtered_items,
                ..l.clone()
            }))
        }
        Block::OrderedList(l) => {
            let filtered_items: Vec<Vec<Block>> = l
                .content
                .iter()
                .map(|item| apply_filter_to_blocks(lua, filter_table, item))
                .collect::<Result<_>>()?;
            Ok(Block::OrderedList(crate::pandoc::OrderedList {
                content: filtered_items,
                ..l.clone()
            }))
        }
        Block::Div(d) => {
            let filtered_content = apply_filter_to_blocks(lua, filter_table, &d.content)?;
            Ok(Block::Div(crate::pandoc::Div {
                content: filtered_content,
                ..d.clone()
            }))
        }
        Block::Figure(f) => {
            let filtered_content = apply_filter_to_blocks(lua, filter_table, &f.content)?;
            Ok(Block::Figure(crate::pandoc::Figure {
                content: filtered_content,
                ..f.clone()
            }))
        }
        Block::LineBlock(l) => {
            let filtered_lines: Vec<Vec<Inline>> = l
                .content
                .iter()
                .map(|line| apply_filter_to_inlines(lua, filter_table, line))
                .collect::<Result<_>>()?;
            Ok(Block::LineBlock(crate::pandoc::LineBlock {
                content: filtered_lines,
                ..l.clone()
            }))
        }
        // Terminal blocks (no children to filter)
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::Table(_)
        | Block::DefinitionList(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_)
        | Block::CaptionBlock(_) => Ok(block.clone()),
    }
}

/// Apply filter to a list of inlines
fn apply_filter_to_inlines(
    lua: &Lua,
    filter_table: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    let mut result = Vec::new();

    // Check for Inlines filter
    if let Ok(inlines_fn) = filter_table.get::<Function>("Inlines") {
        let inlines_table = inlines_to_lua_table(lua, inlines)?;
        let ret: Value = inlines_fn.call(inlines_table)?;
        match handle_inlines_return(ret)? {
            FilterReturn::Unchanged => {
                // Continue with individual inline processing
            }
            FilterReturn::Replace(inline) => {
                // Single inline returned - process it
                result.extend(apply_filter_to_inline(lua, filter_table, &inline)?);
                return Ok(result);
            }
            FilterReturn::Splice(inlines) => {
                // Table of inlines returned - process each
                for inline in inlines {
                    result.extend(apply_filter_to_inline(lua, filter_table, &inline)?);
                }
                return Ok(result);
            }
            FilterReturn::Delete => return Ok(vec![]),
        }
    }

    // Process each inline
    for inline in inlines {
        result.extend(apply_filter_to_inline(lua, filter_table, inline)?);
    }

    Ok(result)
}

/// Apply filter to a single inline
fn apply_filter_to_inline(lua: &Lua, filter_table: &Table, inline: &Inline) -> Result<Vec<Inline>> {
    // First, recursively process children
    let inline_with_filtered_children = filter_inline_children(lua, filter_table, inline)?;

    // Then apply type-specific filter
    let tag = inline_tag(&inline_with_filtered_children);

    // Try type-specific function first
    if let Ok(filter_fn) = filter_table.get::<Function>(tag) {
        let inline_ud = lua.create_userdata(LuaInline(inline_with_filtered_children.clone()))?;
        let ret: Value = filter_fn.call(inline_ud)?;
        return handle_inline_return(ret, &inline_with_filtered_children);
    }

    // Try generic Inline function
    if let Ok(filter_fn) = filter_table.get::<Function>("Inline") {
        let inline_ud = lua.create_userdata(LuaInline(inline_with_filtered_children.clone()))?;
        let ret: Value = filter_fn.call(inline_ud)?;
        return handle_inline_return(ret, &inline_with_filtered_children);
    }

    // No filter, return unchanged
    Ok(vec![inline_with_filtered_children])
}

/// Recursively filter children of an inline
fn filter_inline_children(lua: &Lua, filter_table: &Table, inline: &Inline) -> Result<Inline> {
    match inline {
        Inline::Emph(e) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &e.content)?;
            Ok(Inline::Emph(crate::pandoc::Emph {
                content: filtered,
                ..e.clone()
            }))
        }
        Inline::Strong(s) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &s.content)?;
            Ok(Inline::Strong(crate::pandoc::Strong {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Underline(u) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &u.content)?;
            Ok(Inline::Underline(crate::pandoc::Underline {
                content: filtered,
                ..u.clone()
            }))
        }
        Inline::Strikeout(s) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &s.content)?;
            Ok(Inline::Strikeout(crate::pandoc::Strikeout {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Superscript(s) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &s.content)?;
            Ok(Inline::Superscript(crate::pandoc::Superscript {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Subscript(s) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &s.content)?;
            Ok(Inline::Subscript(crate::pandoc::Subscript {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::SmallCaps(s) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &s.content)?;
            Ok(Inline::SmallCaps(crate::pandoc::SmallCaps {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Quoted(q) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &q.content)?;
            Ok(Inline::Quoted(crate::pandoc::Quoted {
                content: filtered,
                ..q.clone()
            }))
        }
        Inline::Link(l) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &l.content)?;
            Ok(Inline::Link(crate::pandoc::Link {
                content: filtered,
                ..l.clone()
            }))
        }
        Inline::Image(i) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &i.content)?;
            Ok(Inline::Image(crate::pandoc::Image {
                content: filtered,
                ..i.clone()
            }))
        }
        Inline::Span(s) => {
            let filtered = apply_filter_to_inlines(lua, filter_table, &s.content)?;
            Ok(Inline::Span(crate::pandoc::Span {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Note(n) => {
            let filtered = apply_filter_to_blocks(lua, filter_table, &n.content)?;
            Ok(Inline::Note(crate::pandoc::Note {
                content: filtered,
                ..n.clone()
            }))
        }
        // Terminal inlines (no children to filter)
        Inline::Str(_)
        | Inline::Code(_)
        | Inline::Space(_)
        | Inline::SoftBreak(_)
        | Inline::LineBreak(_)
        | Inline::Math(_)
        | Inline::RawInline(_)
        | Inline::Cite(_)
        | Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_, _)
        | Inline::Insert(_)
        | Inline::Delete(_)
        | Inline::Highlight(_)
        | Inline::EditComment(_) => Ok(inline.clone()),
    }
}

/// Handle return value from an inline filter
fn handle_inline_return(ret: Value, original: &Inline) -> Result<Vec<Inline>> {
    match ret {
        Value::Nil => Ok(vec![original.clone()]),
        Value::UserData(ud) => {
            // Single element return - replace
            let lua_inline = ud.borrow::<LuaInline>()?;
            Ok(vec![lua_inline.0.clone()])
        }
        Value::Table(table) => {
            let len = table.raw_len();
            if len == 0 {
                // Empty table - delete
                return Ok(vec![]);
            }
            // Table of elements - splice
            let mut inlines = Vec::new();
            for i in 1..=len {
                let value: Value = table.get(i)?;
                if let Value::UserData(ud) = value {
                    let lua_inline = ud.borrow::<LuaInline>()?;
                    inlines.push(lua_inline.0.clone());
                }
            }
            Ok(inlines)
        }
        _ => Ok(vec![original.clone()]),
    }
}

/// Handle return value from a block filter
fn handle_block_return(ret: Value, original: &Block) -> Result<Vec<Block>> {
    match ret {
        Value::Nil => Ok(vec![original.clone()]),
        Value::UserData(ud) => {
            // Single element return - replace
            let lua_block = ud.borrow::<LuaBlock>()?;
            Ok(vec![lua_block.0.clone()])
        }
        Value::Table(table) => {
            let len = table.raw_len();
            if len == 0 {
                // Empty table - delete
                return Ok(vec![]);
            }
            // Table of elements - splice
            let mut blocks = Vec::new();
            for i in 1..=len {
                let value: Value = table.get(i)?;
                if let Value::UserData(ud) = value {
                    let lua_block = ud.borrow::<LuaBlock>()?;
                    blocks.push(lua_block.0.clone());
                }
            }
            Ok(blocks)
        }
        _ => Ok(vec![original.clone()]),
    }
}

/// Handle return value from an Inlines filter
fn handle_inlines_return(ret: Value) -> Result<FilterReturn<Inline>> {
    match ret {
        Value::Nil => Ok(FilterReturn::Unchanged),
        Value::Table(table) => {
            let len = table.raw_len();
            if len == 0 {
                return Ok(FilterReturn::Delete);
            }
            // Table of inlines
            let mut inlines = Vec::new();
            for i in 1..=len {
                let value: Value = table.get(i)?;
                if let Value::UserData(ud) = value {
                    let lua_inline = ud.borrow::<LuaInline>()?;
                    inlines.push(lua_inline.0.clone());
                }
            }
            Ok(FilterReturn::Splice(inlines))
        }
        _ => Ok(FilterReturn::Unchanged),
    }
}

/// Handle return value from a Blocks filter
fn handle_blocks_return(ret: Value) -> Result<FilterReturn<Block>> {
    match ret {
        Value::Nil => Ok(FilterReturn::Unchanged),
        Value::Table(table) => {
            let len = table.raw_len();
            if len == 0 {
                return Ok(FilterReturn::Delete);
            }
            // Table of blocks
            let mut blocks = Vec::new();
            for i in 1..=len {
                let value: Value = table.get(i)?;
                if let Value::UserData(ud) = value {
                    let lua_block = ud.borrow::<LuaBlock>()?;
                    blocks.push(lua_block.0.clone());
                }
            }
            Ok(FilterReturn::Splice(blocks))
        }
        _ => Ok(FilterReturn::Unchanged),
    }
}

/// Get the tag name for a block
fn block_tag(block: &Block) -> &'static str {
    match block {
        Block::Plain(_) => "Plain",
        Block::Paragraph(_) => "Para",
        Block::LineBlock(_) => "LineBlock",
        Block::CodeBlock(_) => "CodeBlock",
        Block::RawBlock(_) => "RawBlock",
        Block::BlockQuote(_) => "BlockQuote",
        Block::OrderedList(_) => "OrderedList",
        Block::BulletList(_) => "BulletList",
        Block::DefinitionList(_) => "DefinitionList",
        Block::Header(_) => "Header",
        Block::HorizontalRule(_) => "HorizontalRule",
        Block::Table(_) => "Table",
        Block::Figure(_) => "Figure",
        Block::Div(_) => "Div",
        Block::BlockMetadata(_) => "BlockMetadata",
        Block::NoteDefinitionPara(_) => "NoteDefinitionPara",
        Block::NoteDefinitionFencedBlock(_) => "NoteDefinitionFencedBlock",
        Block::CaptionBlock(_) => "CaptionBlock",
    }
}

/// Get the tag name for an inline
fn inline_tag(inline: &Inline) -> &'static str {
    match inline {
        Inline::Str(_) => "Str",
        Inline::Emph(_) => "Emph",
        Inline::Underline(_) => "Underline",
        Inline::Strong(_) => "Strong",
        Inline::Strikeout(_) => "Strikeout",
        Inline::Superscript(_) => "Superscript",
        Inline::Subscript(_) => "Subscript",
        Inline::SmallCaps(_) => "SmallCaps",
        Inline::Quoted(_) => "Quoted",
        Inline::Cite(_) => "Cite",
        Inline::Code(_) => "Code",
        Inline::Space(_) => "Space",
        Inline::SoftBreak(_) => "SoftBreak",
        Inline::LineBreak(_) => "LineBreak",
        Inline::Math(_) => "Math",
        Inline::RawInline(_) => "RawInline",
        Inline::Link(_) => "Link",
        Inline::Image(_) => "Image",
        Inline::Note(_) => "Note",
        Inline::Span(_) => "Span",
        Inline::Shortcode(_) => "Shortcode",
        Inline::NoteReference(_) => "NoteReference",
        Inline::Attr(_, _) => "Attr",
        Inline::Insert(_) => "Insert",
        Inline::Delete(_) => "Delete",
        Inline::Highlight(_) => "Highlight",
        Inline::EditComment(_) => "EditComment",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_uppercase_filter(dir: &TempDir) -> std::path::PathBuf {
        let filter_path = dir.path().join("uppercase.lua");
        fs::write(
            &filter_path,
            r#"
function Str(elem)
    return pandoc.Str(elem.text:upper())
end
"#,
        )
        .unwrap();
        filter_path
    }

    #[test]
    fn test_attr_field_access() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("attr_test.lua");
        fs::write(
            &filter_path,
            r#"
function Span(elem)
    -- Test named field access
    local id = elem.attr.identifier
    -- Test positional access (Lua 1-indexed)
    local id2 = elem.attr[1]
    local classes = elem.attr[2]
    local attrs = elem.attr[3]

    -- Create new span with modified attr using pandoc.Attr constructor
    local new_attr = pandoc.Attr("new-id", {"new-class"}, {key = "value"})
    return pandoc.Span(elem.content, new_attr)
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Span(crate::pandoc::Span {
                    attr: (
                        "old-id".to_string(),
                        vec!["old-class".to_string()],
                        hashlink::LinkedHashMap::new(),
                    ),
                    attr_source: crate::pandoc::AttrSourceInfo::empty(),
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "test".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Span(s) => {
                    assert_eq!(s.attr.0, "new-id");
                    assert_eq!(s.attr.1, vec!["new-class".to_string()]);
                    assert_eq!(s.attr.2.get("key"), Some(&"value".to_string()));
                }
                _ => panic!("Expected Span inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_attr_constructor_defaults() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("attr_constructor.lua");
        fs::write(
            &filter_path,
            r#"
function Span(elem)
    -- Test pandoc.Attr() with defaults (no arguments)
    local attr1 = pandoc.Attr()
    -- Test with just identifier
    local attr2 = pandoc.Attr("my-id")
    -- Test with identifier and classes
    local attr3 = pandoc.Attr("my-id", {"class1", "class2"})

    return pandoc.Span(elem.content, attr3)
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Span(crate::pandoc::Span {
                    attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                    attr_source: crate::pandoc::AttrSourceInfo::empty(),
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "test".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Span(s) => {
                    assert_eq!(s.attr.0, "my-id");
                    assert_eq!(s.attr.1, vec!["class1".to_string(), "class2".to_string()]);
                }
                _ => panic!("Expected Span inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    fn create_identity_filter(dir: &TempDir) -> std::path::PathBuf {
        let filter_path = dir.path().join("identity.lua");
        fs::write(&filter_path, "-- identity filter\n").unwrap();
        filter_path
    }

    #[test]
    fn test_uppercase_filter() {
        let dir = TempDir::new().unwrap();
        let filter_path = create_uppercase_filter(&dir);

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "hello world".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => {
                    assert_eq!(s.text, "HELLO WORLD");
                }
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_identity_filter() {
        let dir = TempDir::new().unwrap();
        let filter_path = create_identity_filter(&dir);

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        // Identity filter should preserve the document
        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => {
                    assert_eq!(s.text, "hello");
                }
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_delete_filter() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("delete.lua");
        fs::write(
            &filter_path,
            r#"
function Str(elem)
    if elem.text == "delete" then
        return {}
    end
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![
                    Inline::Str(crate::pandoc::Str {
                        text: "keep".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Space(crate::pandoc::Space {
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Str(crate::pandoc::Str {
                        text: "delete".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Space(crate::pandoc::Space {
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Str(crate::pandoc::Str {
                        text: "also_keep".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                ],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => {
                // Should have: "keep", Space, Space, "also_keep"
                // The "delete" Str should be removed
                assert_eq!(p.content.len(), 4);
                match &p.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "keep"),
                    _ => panic!("Expected Str"),
                }
                match &p.content[3] {
                    Inline::Str(s) => assert_eq!(s.text, "also_keep"),
                    _ => panic!("Expected Str"),
                }
            }
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_splice_filter() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("splice.lua");
        fs::write(
            &filter_path,
            r#"
function Str(elem)
    if elem.text == "expand" then
        return {pandoc.Str("one"), pandoc.Space(), pandoc.Str("two")}
    end
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "expand".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => {
                // Should have: "one", Space, "two"
                assert_eq!(p.content.len(), 3);
                match &p.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "one"),
                    _ => panic!("Expected Str"),
                }
                match &p.content[1] {
                    Inline::Space(_) => {}
                    _ => panic!("Expected Space"),
                }
                match &p.content[2] {
                    Inline::Str(s) => assert_eq!(s.text, "two"),
                    _ => panic!("Expected Str"),
                }
            }
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_pairs_iteration() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("pairs_test.lua");
        fs::write(
            &filter_path,
            r#"
-- Test pairs() iteration on Str element
function Str(elem)
    local keys = {}
    for k, v in pairs(elem) do
        table.insert(keys, k)
    end
    -- Str should have: tag, text, clone, walk
    -- Return a Str with all keys joined
    return pandoc.Str(table.concat(keys, ","))
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => {
                    // Should contain tag, text, clone, walk
                    assert!(s.text.contains("tag"), "Expected 'tag' in keys: {}", s.text);
                    assert!(
                        s.text.contains("text"),
                        "Expected 'text' in keys: {}",
                        s.text
                    );
                    assert!(
                        s.text.contains("clone"),
                        "Expected 'clone' in keys: {}",
                        s.text
                    );
                    assert!(
                        s.text.contains("walk"),
                        "Expected 'walk' in keys: {}",
                        s.text
                    );
                }
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_walk_method() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("walk_test.lua");
        fs::write(
            &filter_path,
            r#"
-- Test walk() method on Header element
function Header(elem)
    -- Use walk to uppercase all Str elements inside the header
    return elem:walk {
        Str = function(s)
            return pandoc.Str(s.text:upper())
        end
    }
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Header(crate::pandoc::Header {
                level: 1,
                content: vec![
                    Inline::Str(crate::pandoc::Str {
                        text: "hello".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Space(crate::pandoc::Space {
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Str(crate::pandoc::Str {
                        text: "world".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                ],
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                source_info: quarto_source_map::SourceInfo::default(),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Header(h) => {
                assert_eq!(h.content.len(), 3);
                match &h.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "HELLO"),
                    _ => panic!("Expected Str"),
                }
                match &h.content[2] {
                    Inline::Str(s) => assert_eq!(s.text, "WORLD"),
                    _ => panic!("Expected Str"),
                }
            }
            _ => panic!("Expected Header block"),
        }
    }

    #[test]
    fn test_clone_via_field() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("clone_test.lua");
        fs::write(
            &filter_path,
            r#"
-- Test that clone is accessible as a field
function Str(elem)
    local clone_fn = elem.clone
    if type(clone_fn) == "function" then
        local cloned = clone_fn()
        return pandoc.Str(cloned.text .. "_cloned")
    else
        return pandoc.Str("ERROR: clone was not a function")
    end
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "test".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "test_cloned"),
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_walk_nested_elements() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("walk_nested.lua");
        fs::write(
            &filter_path,
            r#"
-- Test walk on Emph to uppercase nested Str
function Emph(elem)
    return elem:walk {
        Str = function(s)
            return pandoc.Str(s.text:upper())
        end
    }
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Emph(crate::pandoc::Emph {
                    content: vec![
                        Inline::Str(crate::pandoc::Str {
                            text: "emphasized".to_string(),
                            source_info: quarto_source_map::SourceInfo::default(),
                        }),
                        Inline::Space(crate::pandoc::Space {
                            source_info: quarto_source_map::SourceInfo::default(),
                        }),
                        Inline::Strong(crate::pandoc::Strong {
                            content: vec![Inline::Str(crate::pandoc::Str {
                                text: "bold".to_string(),
                                source_info: quarto_source_map::SourceInfo::default(),
                            })],
                            source_info: quarto_source_map::SourceInfo::default(),
                        }),
                    ],
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Emph(e) => {
                    // First Str should be uppercased
                    match &e.content[0] {
                        Inline::Str(s) => assert_eq!(s.text, "EMPHASIZED"),
                        _ => panic!("Expected Str"),
                    }
                    // Strong's content should also be walked
                    match &e.content[2] {
                        Inline::Strong(strong) => match &strong.content[0] {
                            Inline::Str(s) => assert_eq!(s.text, "BOLD"),
                            _ => panic!("Expected Str in Strong"),
                        },
                        _ => panic!("Expected Strong"),
                    }
                }
                _ => panic!("Expected Emph inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_topdown_traversal() {
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("topdown_test.lua");
        fs::write(
            &filter_path,
            r#"
-- Test topdown traversal mode
-- In topdown mode, Emph is visited before its children (Str)
-- So we can intercept and replace the entire Emph without ever seeing the Str

local visited_types = {}

function Emph(elem)
    table.insert(visited_types, "Emph")
    -- Replace entire Emph with a Span
    return pandoc.Span({pandoc.Str("replaced")})
end

function Str(elem)
    table.insert(visited_types, "Str:" .. elem.text)
    return elem
end

function Pandoc(doc)
    -- Use topdown traversal
    return doc:walk {
        traverse = "topdown",
        Emph = Emph,
        Str = Str
    }
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Emph(crate::pandoc::Emph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "should_not_see_this".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        // In topdown mode, Emph is replaced with Span before we visit the Str inside
        // So we should see Span(Str("replaced")), not the original "should_not_see_this"
        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Span(s) => match &s.content[0] {
                    Inline::Str(str_elem) => assert_eq!(str_elem.text, "replaced"),
                    _ => panic!("Expected Str in Span"),
                },
                other => panic!("Expected Span, got: {:?}", other),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_filter_provenance_tracking() {
        // Test that elements created by filters capture their source location
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("provenance_test.lua");
        fs::write(
            &filter_path,
            r#"
-- This filter creates a new Str element
-- The source_info should capture this file and line
function Str(elem)
    return pandoc.Str("created-by-filter")
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Str(crate::pandoc::Str {
                    text: "original".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        // The filtered Str should have FilterProvenance source info
        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => {
                    assert_eq!(s.text, "created-by-filter");
                    // Check that the source_info is FilterProvenance
                    match &s.source_info {
                        quarto_source_map::SourceInfo::FilterProvenance {
                            filter_path: path,
                            line,
                        } => {
                            // The filter_path should contain our filter file name
                            assert!(
                                path.contains("provenance_test.lua"),
                                "Expected filter path to contain 'provenance_test.lua', got: {}",
                                path
                            );
                            // The line should be around line 5 where pandoc.Str is called
                            assert!(
                                *line >= 4 && *line <= 7,
                                "Expected line to be between 4-7, got: {}",
                                line
                            );
                        }
                        other => {
                            panic!("Expected FilterProvenance source info, got: {:?}", other)
                        }
                    }
                }
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_pandoc_utils_stringify_basic() {
        // Test pandoc.utils.stringify with basic elements
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("stringify_test.lua");
        fs::write(
            &filter_path,
            r#"
-- Test stringify with various element types
function Para(elem)
    -- Stringify the paragraph content and return a new paragraph
    local text = pandoc.utils.stringify(elem)
    return pandoc.Para({pandoc.Str("result:" .. text)})
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![
                    Inline::Str(crate::pandoc::Str {
                        text: "hello".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Space(crate::pandoc::Space {
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Str(crate::pandoc::Str {
                        text: "world".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                ],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => {
                    assert_eq!(s.text, "result:hello world");
                }
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_pandoc_utils_stringify_nested() {
        // Test stringify with nested elements (Emph containing Strong)
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("stringify_nested.lua");
        fs::write(
            &filter_path,
            r#"
function Emph(elem)
    local text = pandoc.utils.stringify(elem)
    return pandoc.Str("stringified:" .. text)
end
"#,
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![Inline::Emph(crate::pandoc::Emph {
                    content: vec![
                        Inline::Str(crate::pandoc::Str {
                            text: "emphasized".to_string(),
                            source_info: quarto_source_map::SourceInfo::default(),
                        }),
                        Inline::Space(crate::pandoc::Space {
                            source_info: quarto_source_map::SourceInfo::default(),
                        }),
                        Inline::Strong(crate::pandoc::Strong {
                            content: vec![Inline::Str(crate::pandoc::Str {
                                text: "bold".to_string(),
                                source_info: quarto_source_map::SourceInfo::default(),
                            })],
                            source_info: quarto_source_map::SourceInfo::default(),
                        }),
                    ],
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => {
                    assert_eq!(s.text, "stringified:emphasized bold");
                }
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }
}
