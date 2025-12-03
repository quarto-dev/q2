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

use mlua::{Function, Lua, MultiValue, Result, Table, Value};
use quarto_error_reporting::DiagnosticMessage;
use std::path::Path;
use std::sync::Arc;

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::{Block, Inline, Pandoc};

use super::constructors::register_pandoc_namespace;
use super::runtime::{LuaRuntime, NativeRuntime};
use super::types::{LuaBlock, LuaInline, blocks_to_lua_table, inlines_to_lua_table};

// ============================================================================
// TRAVERSAL CONTROL FOR TOPDOWN MODE
// ============================================================================

/// Control signal for topdown traversal - determines whether to descend into children
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraversalControl {
    /// Continue descent into children
    Continue,
    /// Stop descent - don't process children
    Stop,
}

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

/// How the document should be traversed when applying a filter
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkingOrder {
    /// Process each type separately with four passes (default):
    /// 1. All inline elements (bottom-up)
    /// 2. All inline lists (Inlines filter)
    /// 3. All block elements (bottom-up)
    /// 4. All block lists (Blocks filter)
    Typewise,
    /// Traverse top-down from root to leaves, depth-first
    Topdown,
}

/// Get the walking order from a filter table
pub fn get_walking_order(filter_table: &Table) -> Result<WalkingOrder> {
    match filter_table.get::<Option<String>>("traverse")? {
        Some(s) if s == "topdown" => Ok(WalkingOrder::Topdown),
        _ => Ok(WalkingOrder::Typewise),
    }
}

/// Apply a single Lua filter to a document
///
/// Returns the filtered document, context, and any diagnostics emitted by the filter
/// via `quarto.warn()` or `quarto.error()`.
pub fn apply_lua_filter(
    pandoc: &Pandoc,
    context: &ASTContext,
    filter_path: &Path,
    target_format: &str,
) -> FilterResult<(Pandoc, ASTContext, Vec<DiagnosticMessage>)> {
    // Read filter file
    let filter_source = std::fs::read_to_string(filter_path)
        .map_err(|e| LuaFilterError::FileReadError(filter_path.to_owned(), e))?;

    // Create Lua state
    let lua = Lua::new();

    // Create runtime for system operations
    // For now, we use NativeRuntime. In the future, this could be passed in
    // to allow for sandboxed or WASM runtimes.
    let runtime: Arc<dyn LuaRuntime> = Arc::new(NativeRuntime::new());

    // Register pandoc namespace with constructors (also registers quarto namespace)
    register_pandoc_namespace(&lua, runtime)?;

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

    // Determine traversal mode
    let walking_order = get_walking_order(&filter_table)?;

    // Apply the filter using the appropriate traversal
    let filtered_blocks = match walking_order {
        WalkingOrder::Typewise => apply_typewise_filter(&lua, &filter_table, &pandoc.blocks)?,
        WalkingOrder::Topdown => apply_topdown_filter(&lua, &filter_table, &pandoc.blocks)?,
    };

    // Extract any diagnostics emitted by the filter
    let diagnostics = super::diagnostics::extract_lua_diagnostics(&lua)?;

    // Return filtered document with diagnostics
    let filtered_pandoc = Pandoc {
        meta: pandoc.meta.clone(),
        blocks: filtered_blocks,
    };

    Ok((filtered_pandoc, context.clone(), diagnostics))
}

/// Apply multiple Lua filters in sequence
///
/// Returns the filtered document, context, and accumulated diagnostics from all filters.
pub fn apply_lua_filters(
    pandoc: Pandoc,
    context: ASTContext,
    filter_paths: &[std::path::PathBuf],
    target_format: &str,
) -> FilterResult<(Pandoc, ASTContext, Vec<DiagnosticMessage>)> {
    let mut current_pandoc = pandoc;
    let mut current_context = context;
    let mut all_diagnostics = Vec::new();

    for filter_path in filter_paths {
        let (new_pandoc, new_context, diagnostics) = apply_lua_filter(
            &current_pandoc,
            &current_context,
            filter_path,
            target_format,
        )?;
        current_pandoc = new_pandoc;
        current_context = new_context;
        all_diagnostics.extend(diagnostics);
    }

    Ok((current_pandoc, current_context, all_diagnostics))
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

    // Copy traverse setting if present (for topdown mode)
    if let Ok(traverse) = globals.get::<String>("traverse") {
        filter_table.set("traverse", traverse)?;
    }

    Ok(filter_table)
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

// ============================================================================
// TOPDOWN FILTER RETURN HANDLING
// ============================================================================

/// Handle return value from an inline filter with traversal control.
/// Returns (elements, control) where control indicates whether to descend into children.
///
/// Lua filter return semantics:
/// - nil → (original, Continue)
/// - element → (element, Continue)
/// - element, true → (element, Continue)
/// - element, false → (element, Stop)
/// - {elements} → (elements, Continue)
/// - {elements}, false → (elements, Stop)
fn handle_inline_return_with_control(
    ret: MultiValue,
    original: &Inline,
) -> Result<(Vec<Inline>, TraversalControl)> {
    let mut iter = ret.into_iter();

    // First return value: the element(s)
    let first = iter.next().unwrap_or(Value::Nil);
    let elements = match first {
        Value::Nil => vec![original.clone()],
        Value::UserData(ud) => {
            let lua_inline = ud.borrow::<LuaInline>()?;
            vec![lua_inline.0.clone()]
        }
        Value::Table(table) => {
            let len = table.raw_len();
            if len == 0 {
                vec![]
            } else {
                let mut inlines = Vec::new();
                for i in 1..=len {
                    let value: Value = table.get(i)?;
                    if let Value::UserData(ud) = value {
                        let lua_inline = ud.borrow::<LuaInline>()?;
                        inlines.push(lua_inline.0.clone());
                    }
                }
                inlines
            }
        }
        _ => vec![original.clone()],
    };

    // Second return value: traversal control (nil/missing = Continue, false = Stop)
    let control = match iter.next() {
        Some(Value::Boolean(false)) => TraversalControl::Stop,
        _ => TraversalControl::Continue,
    };

    Ok((elements, control))
}

/// Handle return value from a block filter with traversal control.
/// Returns (elements, control) where control indicates whether to descend into children.
fn handle_block_return_with_control(
    ret: MultiValue,
    original: &Block,
) -> Result<(Vec<Block>, TraversalControl)> {
    let mut iter = ret.into_iter();

    // First return value: the element(s)
    let first = iter.next().unwrap_or(Value::Nil);
    let elements = match first {
        Value::Nil => vec![original.clone()],
        Value::UserData(ud) => {
            let lua_block = ud.borrow::<LuaBlock>()?;
            vec![lua_block.0.clone()]
        }
        Value::Table(table) => {
            let len = table.raw_len();
            if len == 0 {
                vec![]
            } else {
                let mut blocks = Vec::new();
                for i in 1..=len {
                    let value: Value = table.get(i)?;
                    if let Value::UserData(ud) = value {
                        let lua_block = ud.borrow::<LuaBlock>()?;
                        blocks.push(lua_block.0.clone());
                    }
                }
                blocks
            }
        }
        _ => vec![original.clone()],
    };

    // Second return value: traversal control (nil/missing = Continue, false = Stop)
    let control = match iter.next() {
        Some(Value::Boolean(false)) => TraversalControl::Stop,
        _ => TraversalControl::Continue,
    };

    Ok((elements, control))
}

/// Handle return value from a Blocks list filter with traversal control.
fn handle_blocks_return_with_control(
    ret: MultiValue,
    original: &[Block],
) -> Result<(Vec<Block>, TraversalControl)> {
    let mut iter = ret.into_iter();

    // First return value: the block list
    let first = iter.next().unwrap_or(Value::Nil);
    let blocks = match first {
        Value::Nil => original.to_vec(),
        Value::Table(table) => {
            let len = table.raw_len();
            if len == 0 {
                vec![]
            } else {
                let mut result = Vec::new();
                for i in 1..=len {
                    let value: Value = table.get(i)?;
                    if let Value::UserData(ud) = value {
                        let lua_block = ud.borrow::<LuaBlock>()?;
                        result.push(lua_block.0.clone());
                    }
                }
                result
            }
        }
        _ => original.to_vec(),
    };

    // Second return value: traversal control
    let control = match iter.next() {
        Some(Value::Boolean(false)) => TraversalControl::Stop,
        _ => TraversalControl::Continue,
    };

    Ok((blocks, control))
}

/// Handle return value from an Inlines list filter with traversal control.
fn handle_inlines_return_with_control(
    ret: MultiValue,
    original: &[Inline],
) -> Result<(Vec<Inline>, TraversalControl)> {
    let mut iter = ret.into_iter();

    // First return value: the inline list
    let first = iter.next().unwrap_or(Value::Nil);
    let inlines = match first {
        Value::Nil => original.to_vec(),
        Value::Table(table) => {
            let len = table.raw_len();
            if len == 0 {
                vec![]
            } else {
                let mut result = Vec::new();
                for i in 1..=len {
                    let value: Value = table.get(i)?;
                    if let Value::UserData(ud) = value {
                        let lua_inline = ud.borrow::<LuaInline>()?;
                        result.push(lua_inline.0.clone());
                    }
                }
                result
            }
        }
        _ => original.to_vec(),
    };

    // Second return value: traversal control
    let control = match iter.next() {
        Some(Value::Boolean(false)) => TraversalControl::Stop,
        _ => TraversalControl::Continue,
    };

    Ok((inlines, control))
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

// ============================================================================
// FOUR-PASS TYPEWISE TRAVERSAL
// ============================================================================
//
// Pandoc's typewise traversal performs four separate passes over the document:
// 1. walkInlineSplicing - Apply inline element filters (Str, Emph, etc.)
// 2. walkInlinesStraight - Apply Inlines list filter
// 3. walkBlockSplicing - Apply block element filters (Para, Div, etc.)
// 4. walkBlocksStraight - Apply Blocks list filter
//
// Each pass traverses the ENTIRE document before the next pass begins.

/// Pass 1: Walk the entire document and apply inline element filters.
/// This visits all Str, Emph, Strong, etc. elements bottom-up.
fn walk_inline_splicing(lua: &Lua, filter_table: &Table, blocks: &[Block]) -> Result<Vec<Block>> {
    blocks
        .iter()
        .map(|block| walk_block_for_inline_splicing(lua, filter_table, block))
        .collect()
}

/// Helper: Walk a single block, applying inline element filters to its inline content
fn walk_block_for_inline_splicing(lua: &Lua, filter_table: &Table, block: &Block) -> Result<Block> {
    match block {
        // Blocks with inline content
        Block::Paragraph(p) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &p.content)?;
            Ok(Block::Paragraph(crate::pandoc::Paragraph {
                content: filtered,
                ..p.clone()
            }))
        }
        Block::Plain(p) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &p.content)?;
            Ok(Block::Plain(crate::pandoc::Plain {
                content: filtered,
                ..p.clone()
            }))
        }
        Block::Header(h) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &h.content)?;
            Ok(Block::Header(crate::pandoc::Header {
                content: filtered,
                ..h.clone()
            }))
        }
        // Blocks with nested block content
        Block::BlockQuote(b) => {
            let filtered = walk_inline_splicing(lua, filter_table, &b.content)?;
            Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                content: filtered,
                ..b.clone()
            }))
        }
        Block::BulletList(l) => {
            let filtered_items: Vec<Vec<Block>> = l
                .content
                .iter()
                .map(|item| walk_inline_splicing(lua, filter_table, item))
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
                .map(|item| walk_inline_splicing(lua, filter_table, item))
                .collect::<Result<_>>()?;
            Ok(Block::OrderedList(crate::pandoc::OrderedList {
                content: filtered_items,
                ..l.clone()
            }))
        }
        Block::Div(d) => {
            let filtered = walk_inline_splicing(lua, filter_table, &d.content)?;
            Ok(Block::Div(crate::pandoc::Div {
                content: filtered,
                ..d.clone()
            }))
        }
        Block::Figure(f) => {
            let filtered = walk_inline_splicing(lua, filter_table, &f.content)?;
            Ok(Block::Figure(crate::pandoc::Figure {
                content: filtered,
                ..f.clone()
            }))
        }
        Block::LineBlock(l) => {
            let filtered_lines: Vec<Vec<Inline>> = l
                .content
                .iter()
                .map(|line| walk_inlines_for_element_filters(lua, filter_table, line))
                .collect::<Result<_>>()?;
            Ok(Block::LineBlock(crate::pandoc::LineBlock {
                content: filtered_lines,
                ..l.clone()
            }))
        }
        // Terminal blocks (no inline/block children to filter)
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

/// Helper: Walk inlines applying element filters (Str, Emph, etc.) but NOT Inlines filter
fn walk_inlines_for_element_filters(
    lua: &Lua,
    filter_table: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    let mut result = Vec::new();
    for inline in inlines {
        // First walk children (bottom-up)
        let walked = walk_inline_children_for_element_filters(lua, filter_table, inline)?;

        // Then apply type-specific or generic Inline filter
        let tag = inline_tag(&walked);
        let filtered = if let Ok(filter_fn) = filter_table.get::<Function>(tag) {
            let inline_ud = lua.create_userdata(LuaInline(walked.clone()))?;
            let ret: Value = filter_fn.call(inline_ud)?;
            handle_inline_return(ret, &walked)?
        } else if let Ok(filter_fn) = filter_table.get::<Function>("Inline") {
            let inline_ud = lua.create_userdata(LuaInline(walked.clone()))?;
            let ret: Value = filter_fn.call(inline_ud)?;
            handle_inline_return(ret, &walked)?
        } else {
            vec![walked]
        };
        result.extend(filtered);
    }
    Ok(result)
}

/// Helper: Walk children of an inline for element filters
fn walk_inline_children_for_element_filters(
    lua: &Lua,
    filter_table: &Table,
    inline: &Inline,
) -> Result<Inline> {
    match inline {
        // Inlines with content children
        Inline::Emph(e) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &e.content)?;
            Ok(Inline::Emph(crate::pandoc::Emph {
                content: filtered,
                ..e.clone()
            }))
        }
        Inline::Strong(s) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &s.content)?;
            Ok(Inline::Strong(crate::pandoc::Strong {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Underline(u) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &u.content)?;
            Ok(Inline::Underline(crate::pandoc::Underline {
                content: filtered,
                ..u.clone()
            }))
        }
        Inline::Strikeout(s) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &s.content)?;
            Ok(Inline::Strikeout(crate::pandoc::Strikeout {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Superscript(s) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &s.content)?;
            Ok(Inline::Superscript(crate::pandoc::Superscript {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Subscript(s) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &s.content)?;
            Ok(Inline::Subscript(crate::pandoc::Subscript {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::SmallCaps(s) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &s.content)?;
            Ok(Inline::SmallCaps(crate::pandoc::SmallCaps {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Quoted(q) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &q.content)?;
            Ok(Inline::Quoted(crate::pandoc::Quoted {
                content: filtered,
                ..q.clone()
            }))
        }
        Inline::Link(l) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &l.content)?;
            Ok(Inline::Link(crate::pandoc::Link {
                content: filtered,
                ..l.clone()
            }))
        }
        Inline::Image(i) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &i.content)?;
            Ok(Inline::Image(crate::pandoc::Image {
                content: filtered,
                ..i.clone()
            }))
        }
        Inline::Span(s) => {
            let filtered = walk_inlines_for_element_filters(lua, filter_table, &s.content)?;
            Ok(Inline::Span(crate::pandoc::Span {
                content: filtered,
                ..s.clone()
            }))
        }
        // Note contains blocks - need to recurse into blocks
        Inline::Note(n) => {
            let filtered = walk_inline_splicing(lua, filter_table, &n.content)?;
            Ok(Inline::Note(crate::pandoc::Note {
                content: filtered,
                ..n.clone()
            }))
        }
        // Terminal inlines
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

/// Pass 2: Walk the entire document and apply Inlines list filter.
/// This visits all inline LISTS (not individual elements).
fn walk_inlines_straight(lua: &Lua, filter_table: &Table, blocks: &[Block]) -> Result<Vec<Block>> {
    // If no Inlines filter, pass through
    if filter_table.get::<Function>("Inlines").is_err() {
        return Ok(blocks.to_vec());
    }

    blocks
        .iter()
        .map(|block| walk_block_for_inlines_straight(lua, filter_table, block))
        .collect()
}

/// Helper: Walk a single block, applying Inlines filter to inline lists
fn walk_block_for_inlines_straight(
    lua: &Lua,
    filter_table: &Table,
    block: &Block,
) -> Result<Block> {
    match block {
        // Blocks with inline content - apply Inlines filter
        Block::Paragraph(p) => {
            let filtered = apply_inlines_filter(lua, filter_table, &p.content)?;
            Ok(Block::Paragraph(crate::pandoc::Paragraph {
                content: filtered,
                ..p.clone()
            }))
        }
        Block::Plain(p) => {
            let filtered = apply_inlines_filter(lua, filter_table, &p.content)?;
            Ok(Block::Plain(crate::pandoc::Plain {
                content: filtered,
                ..p.clone()
            }))
        }
        Block::Header(h) => {
            let filtered = apply_inlines_filter(lua, filter_table, &h.content)?;
            Ok(Block::Header(crate::pandoc::Header {
                content: filtered,
                ..h.clone()
            }))
        }
        // Blocks with nested block content
        Block::BlockQuote(b) => {
            let filtered = walk_inlines_straight(lua, filter_table, &b.content)?;
            Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                content: filtered,
                ..b.clone()
            }))
        }
        Block::BulletList(l) => {
            let filtered_items: Vec<Vec<Block>> = l
                .content
                .iter()
                .map(|item| walk_inlines_straight(lua, filter_table, item))
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
                .map(|item| walk_inlines_straight(lua, filter_table, item))
                .collect::<Result<_>>()?;
            Ok(Block::OrderedList(crate::pandoc::OrderedList {
                content: filtered_items,
                ..l.clone()
            }))
        }
        Block::Div(d) => {
            let filtered = walk_inlines_straight(lua, filter_table, &d.content)?;
            Ok(Block::Div(crate::pandoc::Div {
                content: filtered,
                ..d.clone()
            }))
        }
        Block::Figure(f) => {
            let filtered = walk_inlines_straight(lua, filter_table, &f.content)?;
            Ok(Block::Figure(crate::pandoc::Figure {
                content: filtered,
                ..f.clone()
            }))
        }
        Block::LineBlock(l) => {
            // Each line is a separate inline list
            let filtered_lines: Vec<Vec<Inline>> = l
                .content
                .iter()
                .map(|line| apply_inlines_filter(lua, filter_table, line))
                .collect::<Result<_>>()?;
            Ok(Block::LineBlock(crate::pandoc::LineBlock {
                content: filtered_lines,
                ..l.clone()
            }))
        }
        // Terminal blocks
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

/// Helper: Apply the Inlines filter to a list of inlines
fn apply_inlines_filter(
    lua: &Lua,
    filter_table: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    if let Ok(inlines_fn) = filter_table.get::<Function>("Inlines") {
        let inlines_table = inlines_to_lua_table(lua, inlines)?;
        let ret: Value = inlines_fn.call(inlines_table)?;
        match ret {
            Value::Nil => Ok(inlines.to_vec()),
            Value::Table(table) => {
                let len = table.raw_len();
                if len == 0 {
                    return Ok(vec![]);
                }
                let mut result = Vec::new();
                for i in 1..=len {
                    let value: Value = table.get(i)?;
                    if let Value::UserData(ud) = value {
                        let lua_inline = ud.borrow::<LuaInline>()?;
                        result.push(lua_inline.0.clone());
                    }
                }
                Ok(result)
            }
            _ => Ok(inlines.to_vec()),
        }
    } else {
        Ok(inlines.to_vec())
    }
}

/// Pass 3: Walk the entire document and apply block element filters.
/// This visits all Para, Div, Header, etc. elements bottom-up.
fn walk_block_splicing(lua: &Lua, filter_table: &Table, blocks: &[Block]) -> Result<Vec<Block>> {
    let mut result = Vec::new();
    for block in blocks {
        // First walk children (bottom-up)
        let walked = walk_block_children_for_element_filters(lua, filter_table, block)?;

        // Then apply type-specific or generic Block filter
        let tag = block_tag(&walked);
        let filtered = if let Ok(filter_fn) = filter_table.get::<Function>(tag) {
            let block_ud = lua.create_userdata(LuaBlock(walked.clone()))?;
            let ret: Value = filter_fn.call(block_ud)?;
            handle_block_return(ret, &walked)?
        } else if let Ok(filter_fn) = filter_table.get::<Function>("Block") {
            let block_ud = lua.create_userdata(LuaBlock(walked.clone()))?;
            let ret: Value = filter_fn.call(block_ud)?;
            handle_block_return(ret, &walked)?
        } else {
            vec![walked]
        };
        result.extend(filtered);
    }
    Ok(result)
}

/// Helper: Walk children of a block for element filters (but don't apply filter to block itself)
fn walk_block_children_for_element_filters(
    lua: &Lua,
    filter_table: &Table,
    block: &Block,
) -> Result<Block> {
    match block {
        // Blocks with nested block content
        Block::BlockQuote(b) => {
            let filtered = walk_block_splicing(lua, filter_table, &b.content)?;
            Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                content: filtered,
                ..b.clone()
            }))
        }
        Block::BulletList(l) => {
            let filtered_items: Vec<Vec<Block>> = l
                .content
                .iter()
                .map(|item| walk_block_splicing(lua, filter_table, item))
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
                .map(|item| walk_block_splicing(lua, filter_table, item))
                .collect::<Result<_>>()?;
            Ok(Block::OrderedList(crate::pandoc::OrderedList {
                content: filtered_items,
                ..l.clone()
            }))
        }
        Block::Div(d) => {
            let filtered = walk_block_splicing(lua, filter_table, &d.content)?;
            Ok(Block::Div(crate::pandoc::Div {
                content: filtered,
                ..d.clone()
            }))
        }
        Block::Figure(f) => {
            let filtered = walk_block_splicing(lua, filter_table, &f.content)?;
            Ok(Block::Figure(crate::pandoc::Figure {
                content: filtered,
                ..f.clone()
            }))
        }
        // Other blocks don't have block children (inline content was already handled in pass 1-2)
        Block::Paragraph(_)
        | Block::Plain(_)
        | Block::Header(_)
        | Block::LineBlock(_)
        | Block::CodeBlock(_)
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

/// Pass 4: Walk the entire document and apply Blocks list filter.
/// This visits all block LISTS (not individual elements).
fn walk_blocks_straight(lua: &Lua, filter_table: &Table, blocks: &[Block]) -> Result<Vec<Block>> {
    // First recurse into nested block lists
    let walked: Vec<Block> = blocks
        .iter()
        .map(|block| walk_block_for_blocks_straight(lua, filter_table, block))
        .collect::<Result<_>>()?;

    // Then apply Blocks filter to this list
    if let Ok(blocks_fn) = filter_table.get::<Function>("Blocks") {
        let blocks_table = blocks_to_lua_table(lua, &walked)?;
        let ret: Value = blocks_fn.call(blocks_table)?;
        match ret {
            Value::Nil => Ok(walked),
            Value::Table(table) => {
                let len = table.raw_len();
                if len == 0 {
                    return Ok(vec![]);
                }
                let mut result = Vec::new();
                for i in 1..=len {
                    let value: Value = table.get(i)?;
                    if let Value::UserData(ud) = value {
                        let lua_block = ud.borrow::<LuaBlock>()?;
                        result.push(lua_block.0.clone());
                    }
                }
                Ok(result)
            }
            _ => Ok(walked),
        }
    } else {
        Ok(walked)
    }
}

/// Helper: Walk a single block for Blocks filter (recursing into nested block lists)
fn walk_block_for_blocks_straight(lua: &Lua, filter_table: &Table, block: &Block) -> Result<Block> {
    match block {
        // Blocks with nested block content
        Block::BlockQuote(b) => {
            let filtered = walk_blocks_straight(lua, filter_table, &b.content)?;
            Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                content: filtered,
                ..b.clone()
            }))
        }
        Block::BulletList(l) => {
            let filtered_items: Vec<Vec<Block>> = l
                .content
                .iter()
                .map(|item| walk_blocks_straight(lua, filter_table, item))
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
                .map(|item| walk_blocks_straight(lua, filter_table, item))
                .collect::<Result<_>>()?;
            Ok(Block::OrderedList(crate::pandoc::OrderedList {
                content: filtered_items,
                ..l.clone()
            }))
        }
        Block::Div(d) => {
            let filtered = walk_blocks_straight(lua, filter_table, &d.content)?;
            Ok(Block::Div(crate::pandoc::Div {
                content: filtered,
                ..d.clone()
            }))
        }
        Block::Figure(f) => {
            let filtered = walk_blocks_straight(lua, filter_table, &f.content)?;
            Ok(Block::Figure(crate::pandoc::Figure {
                content: filtered,
                ..f.clone()
            }))
        }
        // Other blocks don't have block children
        Block::Paragraph(_)
        | Block::Plain(_)
        | Block::Header(_)
        | Block::LineBlock(_)
        | Block::CodeBlock(_)
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

// ============================================================================
// TOPDOWN TRAVERSAL
// ============================================================================
//
// Topdown traversal processes parents before children, depth-first.
// This is the opposite of typewise traversal which processes children first.
//
// Algorithm:
// 1. Apply Blocks filter to the list first (if present)
// 2. If Stop, return without descending
// 3. For each block, apply block filter then recurse if Continue
// 4. Inside each block, apply the same to inlines

/// Apply topdown filter traversal
fn apply_topdown_filter(lua: &Lua, filter_table: &Table, blocks: &[Block]) -> Result<Vec<Block>> {
    walk_blocks_topdown(lua, filter_table, blocks)
}

/// Walk blocks in topdown order: apply Blocks filter, then each block, then children
pub fn walk_blocks_topdown(
    lua: &Lua,
    filter_table: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    // Step 1: Apply Blocks filter first (operates on whole list)
    let (blocks, ctrl) = if let Ok(blocks_fn) = filter_table.get::<Function>("Blocks") {
        let blocks_table = blocks_to_lua_table(lua, blocks)?;
        let ret: MultiValue = blocks_fn.call(blocks_table)?;
        handle_blocks_return_with_control(ret, blocks)?
    } else {
        (blocks.to_vec(), TraversalControl::Continue)
    };

    // If Stop, return without descending into individual blocks
    if ctrl == TraversalControl::Stop {
        return Ok(blocks);
    }

    // Step 2: For each block, apply block filter then recurse
    let mut result = Vec::new();
    for block in &blocks {
        let (filtered, ctrl) = apply_block_filter_topdown(lua, filter_table, block)?;
        for b in filtered {
            if ctrl == TraversalControl::Stop {
                // Don't descend into children
                result.push(b);
            } else {
                // Recurse into children
                let walked = walk_block_children_topdown(lua, filter_table, &b)?;
                result.push(walked);
            }
        }
    }
    Ok(result)
}

/// Apply filter to a single block and return (result, control)
fn apply_block_filter_topdown(
    lua: &Lua,
    filter_table: &Table,
    block: &Block,
) -> Result<(Vec<Block>, TraversalControl)> {
    let tag = block_tag(block);

    // Try type-specific filter first, then generic Block filter
    if let Ok(filter_fn) = filter_table.get::<Function>(tag) {
        let block_ud = lua.create_userdata(LuaBlock(block.clone()))?;
        let ret: MultiValue = filter_fn.call(block_ud)?;
        handle_block_return_with_control(ret, block)
    } else if let Ok(filter_fn) = filter_table.get::<Function>("Block") {
        let block_ud = lua.create_userdata(LuaBlock(block.clone()))?;
        let ret: MultiValue = filter_fn.call(block_ud)?;
        handle_block_return_with_control(ret, block)
    } else {
        Ok((vec![block.clone()], TraversalControl::Continue))
    }
}

/// Walk children of a block in topdown order
fn walk_block_children_topdown(lua: &Lua, filter_table: &Table, block: &Block) -> Result<Block> {
    match block {
        // Blocks with inline content
        Block::Paragraph(p) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &p.content)?;
            Ok(Block::Paragraph(crate::pandoc::Paragraph {
                content: filtered,
                ..p.clone()
            }))
        }
        Block::Plain(p) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &p.content)?;
            Ok(Block::Plain(crate::pandoc::Plain {
                content: filtered,
                ..p.clone()
            }))
        }
        Block::Header(h) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &h.content)?;
            Ok(Block::Header(crate::pandoc::Header {
                content: filtered,
                ..h.clone()
            }))
        }
        // Blocks with nested block content
        Block::BlockQuote(b) => {
            let filtered = walk_blocks_topdown(lua, filter_table, &b.content)?;
            Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                content: filtered,
                ..b.clone()
            }))
        }
        Block::BulletList(l) => {
            let filtered_items: Vec<Vec<Block>> = l
                .content
                .iter()
                .map(|item| walk_blocks_topdown(lua, filter_table, item))
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
                .map(|item| walk_blocks_topdown(lua, filter_table, item))
                .collect::<Result<_>>()?;
            Ok(Block::OrderedList(crate::pandoc::OrderedList {
                content: filtered_items,
                ..l.clone()
            }))
        }
        Block::Div(d) => {
            let filtered = walk_blocks_topdown(lua, filter_table, &d.content)?;
            Ok(Block::Div(crate::pandoc::Div {
                content: filtered,
                ..d.clone()
            }))
        }
        Block::Figure(f) => {
            let filtered = walk_blocks_topdown(lua, filter_table, &f.content)?;
            Ok(Block::Figure(crate::pandoc::Figure {
                content: filtered,
                ..f.clone()
            }))
        }
        Block::LineBlock(l) => {
            let filtered_lines: Vec<Vec<Inline>> = l
                .content
                .iter()
                .map(|line| walk_inlines_topdown(lua, filter_table, line))
                .collect::<Result<_>>()?;
            Ok(Block::LineBlock(crate::pandoc::LineBlock {
                content: filtered_lines,
                ..l.clone()
            }))
        }
        // Terminal blocks (no children to walk)
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

/// Walk inlines in topdown order: apply Inlines filter, then each inline, then children
pub fn walk_inlines_topdown(
    lua: &Lua,
    filter_table: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    // Step 1: Apply Inlines filter first (operates on whole list)
    let (inlines, ctrl) = if let Ok(inlines_fn) = filter_table.get::<Function>("Inlines") {
        let inlines_table = inlines_to_lua_table(lua, inlines)?;
        let ret: MultiValue = inlines_fn.call(inlines_table)?;
        handle_inlines_return_with_control(ret, inlines)?
    } else {
        (inlines.to_vec(), TraversalControl::Continue)
    };

    // If Stop, return without descending into individual inlines
    if ctrl == TraversalControl::Stop {
        return Ok(inlines);
    }

    // Step 2: For each inline, apply inline filter then recurse
    let mut result = Vec::new();
    for inline in &inlines {
        let (filtered, ctrl) = apply_inline_filter_topdown(lua, filter_table, inline)?;
        for i in filtered {
            if ctrl == TraversalControl::Stop {
                // Don't descend into children
                result.push(i);
            } else {
                // Recurse into children
                let walked = walk_inline_children_topdown(lua, filter_table, &i)?;
                result.push(walked);
            }
        }
    }
    Ok(result)
}

/// Apply filter to a single inline and return (result, control)
fn apply_inline_filter_topdown(
    lua: &Lua,
    filter_table: &Table,
    inline: &Inline,
) -> Result<(Vec<Inline>, TraversalControl)> {
    let tag = inline_tag(inline);

    // Try type-specific filter first, then generic Inline filter
    if let Ok(filter_fn) = filter_table.get::<Function>(tag) {
        let inline_ud = lua.create_userdata(LuaInline(inline.clone()))?;
        let ret: MultiValue = filter_fn.call(inline_ud)?;
        handle_inline_return_with_control(ret, inline)
    } else if let Ok(filter_fn) = filter_table.get::<Function>("Inline") {
        let inline_ud = lua.create_userdata(LuaInline(inline.clone()))?;
        let ret: MultiValue = filter_fn.call(inline_ud)?;
        handle_inline_return_with_control(ret, inline)
    } else {
        Ok((vec![inline.clone()], TraversalControl::Continue))
    }
}

/// Walk children of an inline in topdown order
fn walk_inline_children_topdown(
    lua: &Lua,
    filter_table: &Table,
    inline: &Inline,
) -> Result<Inline> {
    match inline {
        // Inlines with content children
        Inline::Emph(e) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &e.content)?;
            Ok(Inline::Emph(crate::pandoc::Emph {
                content: filtered,
                ..e.clone()
            }))
        }
        Inline::Strong(s) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &s.content)?;
            Ok(Inline::Strong(crate::pandoc::Strong {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Underline(u) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &u.content)?;
            Ok(Inline::Underline(crate::pandoc::Underline {
                content: filtered,
                ..u.clone()
            }))
        }
        Inline::Strikeout(s) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &s.content)?;
            Ok(Inline::Strikeout(crate::pandoc::Strikeout {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Superscript(s) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &s.content)?;
            Ok(Inline::Superscript(crate::pandoc::Superscript {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Subscript(s) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &s.content)?;
            Ok(Inline::Subscript(crate::pandoc::Subscript {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::SmallCaps(s) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &s.content)?;
            Ok(Inline::SmallCaps(crate::pandoc::SmallCaps {
                content: filtered,
                ..s.clone()
            }))
        }
        Inline::Quoted(q) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &q.content)?;
            Ok(Inline::Quoted(crate::pandoc::Quoted {
                content: filtered,
                ..q.clone()
            }))
        }
        Inline::Link(l) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &l.content)?;
            Ok(Inline::Link(crate::pandoc::Link {
                content: filtered,
                ..l.clone()
            }))
        }
        Inline::Image(i) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &i.content)?;
            Ok(Inline::Image(crate::pandoc::Image {
                content: filtered,
                ..i.clone()
            }))
        }
        Inline::Span(s) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &s.content)?;
            Ok(Inline::Span(crate::pandoc::Span {
                content: filtered,
                ..s.clone()
            }))
        }
        // Note contains blocks
        Inline::Note(n) => {
            let filtered = walk_blocks_topdown(lua, filter_table, &n.content)?;
            Ok(Inline::Note(crate::pandoc::Note {
                content: filtered,
                ..n.clone()
            }))
        }
        // CriticMarkup types
        Inline::Insert(i) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &i.content)?;
            Ok(Inline::Insert(crate::pandoc::Insert {
                content: filtered,
                ..i.clone()
            }))
        }
        Inline::Delete(d) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &d.content)?;
            Ok(Inline::Delete(crate::pandoc::Delete {
                content: filtered,
                ..d.clone()
            }))
        }
        Inline::Highlight(h) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &h.content)?;
            Ok(Inline::Highlight(crate::pandoc::Highlight {
                content: filtered,
                ..h.clone()
            }))
        }
        Inline::EditComment(ec) => {
            let filtered = walk_inlines_topdown(lua, filter_table, &ec.content)?;
            Ok(Inline::EditComment(crate::pandoc::EditComment {
                content: filtered,
                ..ec.clone()
            }))
        }
        // Terminal inlines - no children to walk
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
        | Inline::Attr(_, _) => Ok(inline.clone()),
    }
}

/// Apply typewise filter traversal (four separate passes)
pub fn apply_typewise_filter(
    lua: &Lua,
    filter_table: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    // Pass 1: Walk all inlines (splicing)
    let blocks = walk_inline_splicing(lua, filter_table, blocks)?;
    // Pass 2: Walk all inline lists (Inlines filter)
    let blocks = walk_inlines_straight(lua, filter_table, &blocks)?;
    // Pass 3: Walk all blocks (splicing)
    let blocks = walk_block_splicing(lua, filter_table, &blocks)?;
    // Pass 4: Walk all block lists (Blocks filter)
    walk_blocks_straight(lua, filter_table, &blocks)
}

/// Apply typewise filter traversal to inlines only (two passes)
/// Used for elem:walk{} on inline elements and inline lists
pub fn apply_typewise_inlines(
    lua: &Lua,
    filter_table: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    // Pass 1: Walk all inline elements (splicing)
    let inlines = walk_inlines_for_element_filters(lua, filter_table, inlines)?;
    // Pass 2: Apply Inlines list filter
    apply_inlines_filter(lua, filter_table, &inlines)
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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

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

    #[test]
    fn test_typewise_traversal_order() {
        // Test that typewise traversal processes ALL inlines before ANY blocks
        // Pandoc's typewise traversal does four separate passes:
        // 1. walkInlineSplicing - all inline elements
        // 2. walkInlinesStraight - all Inlines lists
        // 3. walkBlockSplicing - all block elements
        // 4. walkBlocksStraight - all Blocks lists
        let dir = TempDir::new().unwrap();
        let order_file = dir.path().join("order.txt");
        let filter_path = dir.path().join("order_test.lua");

        // Create a filter that writes the order of calls to a file
        fs::write(
            &filter_path,
            format!(
                r#"
local order_file = io.open("{}", "w")

function Str(elem)
    order_file:write("Str:" .. elem.text .. "\n")
    order_file:flush()
    return elem
end

function Inlines(inlines)
    order_file:write("Inlines\n")
    order_file:flush()
    return inlines
end

function Para(elem)
    order_file:write("Para\n")
    order_file:flush()
    return elem
end

function Blocks(blocks)
    order_file:write("Blocks\n")
    order_file:flush()
    return blocks
end
"#,
                order_file.display()
            ),
        )
        .unwrap();

        // Document with two paragraphs, each with a Str
        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![
                Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "a".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                }),
                Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "b".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                }),
            ],
        };
        let context = ASTContext::new();

        let _ = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        // Read the order file
        let order = fs::read_to_string(&order_file).unwrap();
        let lines: Vec<&str> = order.lines().collect();

        // Expected order for Pandoc-compatible typewise traversal:
        // Pass 1: All inlines (Str:a, Str:b)
        // Pass 2: All inline lists (Inlines, Inlines)
        // Pass 3: All blocks (Para, Para)
        // Pass 4: All block lists (Blocks)
        let expected = vec![
            "Str:a", "Str:b", // Pass 1: all inline elements
            "Inlines", "Inlines", // Pass 2: all inline lists
            "Para", "Para",   // Pass 3: all block elements
            "Blocks", // Pass 4: all block lists
        ];

        assert_eq!(
            lines, expected,
            "Traversal order mismatch.\nExpected: {:?}\nActual: {:?}",
            expected, lines
        );
    }

    #[test]
    fn test_generic_inline_fallback() {
        // Test that generic `Inline` filter is called when no type-specific filter exists
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("inline_fallback.lua");
        fs::write(
            &filter_path,
            r#"
function Inline(elem)
    if elem.tag == "Str" then
        return pandoc.Str(elem.text:upper())
    end
    return elem
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

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "HELLO"),
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_generic_block_fallback() {
        // Test that generic `Block` filter is called when no type-specific filter exists
        let dir = TempDir::new().unwrap();
        let order_file = dir.path().join("order.txt");
        let filter_path = dir.path().join("block_fallback.lua");

        fs::write(
            &filter_path,
            format!(
                r#"
local order_file = io.open("{}", "w")

function Block(elem)
    order_file:write("Block:" .. elem.tag .. "\n")
    order_file:flush()
    return elem
end
"#,
                order_file.display()
            ),
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![
                Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "a".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                }),
                Block::CodeBlock(crate::pandoc::CodeBlock {
                    attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                    attr_source: crate::pandoc::AttrSourceInfo::empty(),
                    text: "code".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                }),
            ],
        };
        let context = ASTContext::new();

        let _ = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        let order = fs::read_to_string(&order_file).unwrap();
        let lines: Vec<&str> = order.lines().collect();

        // Generic Block filter should be called for both Para and CodeBlock
        assert_eq!(lines, vec!["Block:Para", "Block:CodeBlock"]);
    }

    #[test]
    fn test_type_specific_overrides_generic() {
        // Test that type-specific filter (Str) takes precedence over generic (Inline)
        let dir = TempDir::new().unwrap();
        let order_file = dir.path().join("order.txt");
        let filter_path = dir.path().join("override.lua");

        fs::write(
            &filter_path,
            format!(
                r#"
local order_file = io.open("{}", "w")

function Str(elem)
    order_file:write("Str\n")
    order_file:flush()
    return elem
end

function Inline(elem)
    order_file:write("Inline:" .. elem.tag .. "\n")
    order_file:flush()
    return elem
end
"#,
                order_file.display()
            ),
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
                ],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let _ = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        let order = fs::read_to_string(&order_file).unwrap();
        let lines: Vec<&str> = order.lines().collect();

        // Str uses type-specific filter, Space uses generic Inline filter
        assert_eq!(lines, vec!["Str", "Inline:Space"]);
    }

    #[test]
    fn test_topdown_document_level_traversal_order() {
        // Test that document-level topdown traversal processes parents before children
        // In topdown mode: Para should be visited BEFORE its Str children
        // In typewise mode: Str children are visited BEFORE Para
        let dir = TempDir::new().unwrap();
        let order_file = dir.path().join("order.txt");
        let filter_path = dir.path().join("topdown_order.lua");

        // Note: We set traverse as a global variable since get_filter_table copies from globals
        fs::write(
            &filter_path,
            format!(
                r#"
local order_file = io.open("{}", "w")

traverse = "topdown"

function Para(elem)
    order_file:write("Para\n")
    order_file:flush()
    return elem
end

function Str(elem)
    order_file:write("Str:" .. elem.text .. "\n")
    order_file:flush()
    return elem
end
"#,
                order_file.display()
            ),
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
                        text: "a".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Space(crate::pandoc::Space {
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Str(crate::pandoc::Str {
                        text: "b".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                ],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let _ = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        let order = fs::read_to_string(&order_file).unwrap();
        let lines: Vec<&str> = order.lines().collect();

        // In topdown mode, Para is visited before its Str children
        assert_eq!(
            lines,
            vec!["Para", "Str:a", "Str:b"],
            "Expected topdown order: Para first, then Str children"
        );
    }

    #[test]
    fn test_topdown_stop_signal_prevents_descent() {
        // Test that returning (element, false) in topdown mode stops descent into children
        // In this test, Div returns (elem, false) which should prevent its children from being visited
        let dir = TempDir::new().unwrap();
        let order_file = dir.path().join("order.txt");
        let filter_path = dir.path().join("topdown_stop.lua");

        fs::write(
            &filter_path,
            format!(
                r#"
local order_file = io.open("{}", "w")

traverse = "topdown"

function Div(elem)
    order_file:write("Div\n")
    order_file:flush()
    -- Return element with false to stop descent into children
    return elem, false
end

function Para(elem)
    order_file:write("Para\n")
    order_file:flush()
    return elem
end

function Str(elem)
    order_file:write("Str:" .. elem.text .. "\n")
    order_file:flush()
    return elem
end
"#,
                order_file.display()
            ),
        )
        .unwrap();

        // Create: [Div([Para([Str("inside")])]), Para([Str("outside")])]
        // The Div should be visited, but its children should NOT be visited due to stop signal
        // The second Para and its Str should be visited
        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![
                Block::Div(crate::pandoc::Div {
                    content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                        content: vec![Inline::Str(crate::pandoc::Str {
                            text: "inside".to_string(),
                            source_info: quarto_source_map::SourceInfo::default(),
                        })],
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                    attr_source: crate::pandoc::AttrSourceInfo::empty(),
                    source_info: quarto_source_map::SourceInfo::default(),
                }),
                Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "outside".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                }),
            ],
        };
        let context = ASTContext::new();

        let _ = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        let order = fs::read_to_string(&order_file).unwrap();
        let lines: Vec<&str> = order.lines().collect();

        // Expected: Div is visited, but "inside" Para and Str are NOT visited (stop signal)
        // Then "outside" Para and its Str are visited normally
        assert_eq!(
            lines,
            vec!["Div", "Para", "Str:outside"],
            "Expected Div to stop descent, so 'inside' Para/Str should not be visited"
        );
    }

    #[test]
    fn test_topdown_blocks_filter_order() {
        // Test that in topdown mode, the Blocks filter is called BEFORE individual block filters
        // This is the opposite of typewise mode where Blocks is called AFTER
        let dir = TempDir::new().unwrap();
        let order_file = dir.path().join("order.txt");
        let filter_path = dir.path().join("topdown_blocks.lua");

        fs::write(
            &filter_path,
            format!(
                r#"
local order_file = io.open("{}", "w")

traverse = "topdown"

function Blocks(blocks)
    order_file:write("Blocks\n")
    order_file:flush()
    return blocks
end

function Para(elem)
    order_file:write("Para\n")
    order_file:flush()
    return elem
end
"#,
                order_file.display()
            ),
        )
        .unwrap();

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![
                Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "a".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                }),
                Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "b".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                }),
            ],
        };
        let context = ASTContext::new();

        let _ = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        let order = fs::read_to_string(&order_file).unwrap();
        let lines: Vec<&str> = order.lines().collect();

        // In topdown mode, Blocks is called FIRST, then individual Para elements
        assert_eq!(
            lines,
            vec!["Blocks", "Para", "Para"],
            "Expected topdown: Blocks first, then individual elements"
        );
    }

    #[test]
    fn test_elem_walk_typewise_traversal_order() {
        // Test that elem:walk{} uses correct four-pass traversal order
        // When walking a Div containing two paragraphs, all Str elements should be
        // processed before any Para elements.
        let dir = TempDir::new().unwrap();
        let order_file = dir.path().join("order.txt");
        let filter_path = dir.path().join("elem_walk_order.lua");

        fs::write(
            &filter_path,
            format!(
                r#"
local order_file = io.open("{}", "w")

-- Filter that walks a Div using elem:walk
function Div(elem)
    return elem:walk {{
        Str = function(s)
            order_file:write("Str:" .. s.text .. "\n")
            order_file:flush()
            return s
        end,
        Inlines = function(inlines)
            order_file:write("Inlines\n")
            order_file:flush()
            return inlines
        end,
        Para = function(p)
            order_file:write("Para\n")
            order_file:flush()
            return p
        end,
        Blocks = function(blocks)
            order_file:write("Blocks\n")
            order_file:flush()
            return blocks
        end
    }}
end
"#,
                order_file.display()
            ),
        )
        .unwrap();

        // Document: Div containing two paragraphs, each with one Str
        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Div(crate::pandoc::Div {
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![
                    Block::Paragraph(crate::pandoc::Paragraph {
                        content: vec![Inline::Str(crate::pandoc::Str {
                            text: "a".to_string(),
                            source_info: quarto_source_map::SourceInfo::default(),
                        })],
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Block::Paragraph(crate::pandoc::Paragraph {
                        content: vec![Inline::Str(crate::pandoc::Str {
                            text: "b".to_string(),
                            source_info: quarto_source_map::SourceInfo::default(),
                        })],
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                ],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let _ = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        let order = fs::read_to_string(&order_file).unwrap();
        let lines: Vec<&str> = order.lines().collect();

        // Expected four-pass order:
        // Pass 1: All inline elements (Str:a, Str:b)
        // Pass 2: All inline lists (Inlines, Inlines) - one per Para
        // Pass 3: All block elements (Para, Para)
        // Pass 4: All block lists - there are TWO:
        //         - Div.content (the inner [Para, Para] list)
        //         - The wrapper list [Div] from wrapping the single element
        // Note: The Div filter itself is NOT called because we're inside elem:walk
        assert_eq!(
            lines,
            vec![
                "Str:a", "Str:b", "Inlines", "Inlines", "Para", "Para", "Blocks", "Blocks"
            ],
            "Expected four-pass order: all inlines first, then Inlines lists, then blocks, then Blocks lists"
        );
    }

    #[test]
    fn test_elem_walk_topdown_stop_signal() {
        // Test that elem:walk{} with topdown correctly handles the stop signal.
        // When a filter returns (elem, false), it should NOT descend into children.
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("topdown_stop.lua");

        fs::write(
            &filter_path,
            r#"
-- Filter that uses elem:walk with topdown traversal and stop signal
function Div(elem)
    return elem:walk {
        traverse = "topdown",
        -- Stop descent at Para elements
        Para = function(p)
            return p, false
        end,
        -- This should NOT be called for Str inside Para
        Str = function(s)
            return pandoc.Str(s.text:upper())
        end
    }
end
"#,
        )
        .unwrap();

        // Document: Div containing Para with Str "hello"
        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Div(crate::pandoc::Div {
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                attr_source: crate::pandoc::AttrSourceInfo::empty(),
                content: vec![Block::Paragraph(crate::pandoc::Paragraph {
                    content: vec![Inline::Str(crate::pandoc::Str {
                        text: "hello".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        // The Str should NOT be uppercased because the Para returned false to stop descent
        match &filtered.blocks[0] {
            Block::Div(d) => match &d.content[0] {
                Block::Paragraph(p) => match &p.content[0] {
                    Inline::Str(s) => {
                        assert_eq!(
                            s.text, "hello",
                            "Str should NOT be uppercased because descent was stopped at Para"
                        );
                    }
                    _ => panic!("Expected Str inline"),
                },
                _ => panic!("Expected Paragraph block"),
            },
            _ => panic!("Expected Div block"),
        }
    }

    #[test]
    fn test_inlines_walk_typewise_order() {
        // Test that Inlines:walk{} uses correct two-pass traversal order
        // All inline element filters should be applied before the Inlines filter
        let dir = TempDir::new().unwrap();
        let order_file = dir.path().join("order.txt");
        let filter_path = dir.path().join("inlines_walk_order.lua");

        fs::write(
            &filter_path,
            format!(
                r#"
local order_file = io.open("{}", "w")

-- Filter that walks inlines inside a Para
function Para(elem)
    local walked = elem.content:walk {{
        Str = function(s)
            order_file:write("Str:" .. s.text .. "\n")
            order_file:flush()
            return s
        end,
        Inlines = function(inlines)
            order_file:write("Inlines\n")
            order_file:flush()
            return inlines
        end
    }}
    return pandoc.Para(walked)
end
"#,
                order_file.display()
            ),
        )
        .unwrap();

        // Document: Para with two Str elements
        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![
                    Inline::Str(crate::pandoc::Str {
                        text: "a".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Space(crate::pandoc::Space {
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                    Inline::Str(crate::pandoc::Str {
                        text: "b".to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    }),
                ],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let _ = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        let order = fs::read_to_string(&order_file).unwrap();
        let lines: Vec<&str> = order.lines().collect();

        // Expected two-pass order:
        // Pass 1: All inline elements (Str:a, Str:b)
        // Pass 2: Inlines list filter
        assert_eq!(
            lines,
            vec!["Str:a", "Str:b", "Inlines"],
            "Expected two-pass order: all inline elements first, then Inlines list filter"
        );
    }

    // ============================================================================
    // DIAGNOSTICS TESTS
    // ============================================================================

    #[test]
    fn test_quarto_warn_in_filter() {
        // Test that quarto.warn() emits diagnostics during filter execution
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("warn_test.lua");
        fs::write(
            &filter_path,
            r#"
function Str(elem)
    quarto.warn("This is a warning about: " .. elem.text)
    return elem
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

        let (filtered, _, diagnostics) =
            apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        // Document should be unchanged
        match &filtered.blocks[0] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "hello"),
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }

        // Should have one warning diagnostic
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
        assert_eq!(
            diagnostics[0].kind,
            quarto_error_reporting::DiagnosticKind::Warning
        );
        assert!(
            diagnostics[0]
                .title
                .contains("This is a warning about: hello"),
            "Expected warning message, got: {}",
            diagnostics[0].title
        );

        // Check source location
        if let Some(quarto_source_map::SourceInfo::FilterProvenance { filter_path, line }) =
            &diagnostics[0].location
        {
            assert!(filter_path.contains("warn_test.lua"));
            assert!(*line > 0, "Line should be positive");
        } else {
            panic!("Expected FilterProvenance source info");
        }
    }

    #[test]
    fn test_quarto_error_in_filter() {
        // Test that quarto.error() emits error diagnostics during filter execution
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("error_test.lua");
        fs::write(
            &filter_path,
            r#"
function Para(elem)
    quarto.error("Something went wrong in paragraph processing")
    return elem
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

        let (_, _, diagnostics) =
            apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        // Should have one error diagnostic
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
        assert_eq!(
            diagnostics[0].kind,
            quarto_error_reporting::DiagnosticKind::Error
        );
        assert!(
            diagnostics[0]
                .title
                .contains("Something went wrong in paragraph processing")
        );
    }

    #[test]
    fn test_multiple_diagnostics_from_filter() {
        // Test that multiple warn/error calls accumulate diagnostics
        let dir = TempDir::new().unwrap();
        let filter_path = dir.path().join("multi_diag.lua");
        fs::write(
            &filter_path,
            r#"
function Str(elem)
    quarto.warn("Warning 1")
    quarto.error("Error 1")
    quarto.warn("Warning 2")
    return elem
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

        let (_, _, diagnostics) =
            apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

        // Should have 3 diagnostics
        assert_eq!(diagnostics.len(), 3, "Expected 3 diagnostics");
        assert_eq!(
            diagnostics[0].kind,
            quarto_error_reporting::DiagnosticKind::Warning
        );
        assert_eq!(
            diagnostics[1].kind,
            quarto_error_reporting::DiagnosticKind::Error
        );
        assert_eq!(
            diagnostics[2].kind,
            quarto_error_reporting::DiagnosticKind::Warning
        );
    }

    #[test]
    fn test_diagnostics_accumulated_across_filters() {
        // Test that diagnostics are accumulated when running multiple filters
        let dir = TempDir::new().unwrap();
        let filter1_path = dir.path().join("filter1.lua");
        let filter2_path = dir.path().join("filter2.lua");

        fs::write(
            &filter1_path,
            r#"
function Str(elem)
    quarto.warn("Warning from filter 1")
    return elem
end
"#,
        )
        .unwrap();

        fs::write(
            &filter2_path,
            r#"
function Str(elem)
    quarto.error("Error from filter 2")
    return elem
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

        let (_, _, diagnostics) =
            apply_lua_filters(pandoc, context, &[filter1_path, filter2_path], "html").unwrap();

        // Should have 2 diagnostics from both filters
        assert_eq!(
            diagnostics.len(),
            2,
            "Expected 2 diagnostics from 2 filters"
        );
        assert!(diagnostics[0].title.contains("Warning from filter 1"));
        assert!(diagnostics[1].title.contains("Error from filter 2"));
    }
}
