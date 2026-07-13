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
use super::mediabag::create_shared_mediabag;
use super::quarto_api::register_quarto_api;
use super::readwrite::{create_reader_options_table, create_writer_options_table};
use super::runtime::SystemRuntime;
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

/// Output from applying a Lua filter.
///
/// Contains the filtered document, context, diagnostics, and any HTML dependencies
/// or text includes registered via the `quarto.doc` API.
pub struct FilterOutput {
    pub pandoc: Pandoc,
    pub context: ASTContext,
    pub diagnostics: Vec<DiagnosticMessage>,
    pub html_dependencies: Vec<super::quarto_doc::HtmlDependency>,
    pub text_includes: Vec<super::quarto_doc::TextInclude>,
    /// Raw paths collected via `quarto.doc.add_resource(path)`
    /// (`bd-o8pr` Phase 3). The orchestrator resolves each path
    /// against the project root and the document's parent dir.
    pub resources: Vec<std::path::PathBuf>,
}

/// Create the Lua environment exactly as user filters see it.
///
/// This is the single place the filter execution environment is
/// assembled: the `pandoc` and `quarto` namespaces, the
/// `FORMAT`/`PANDOC_*` globals, and (on WASM) the synthetic
/// `io`/`os`/`dofile`. `apply_lua_filter` delegates here; the Lua
/// conformance suite (`tests/integration/lua_conformance.rs`) uses it
/// directly so that conformance is measured against the production
/// environment rather than a synthetic registration.
///
/// `script_path` seeds `PANDOC_SCRIPT_FILE` and the script directory
/// used by `quarto.utils.resolve_path`.
pub fn create_filter_environment(
    runtime: Arc<dyn SystemRuntime>,
    target_format: &str,
    script_path: &Path,
    attribution: Option<Arc<dyn crate::attribution::AttributionLookup>>,
) -> FilterResult<Lua> {
    // Create Lua state
    // On WASM, we can't load all libraries (no package/io/os/debug support),
    // so use a restricted set. On native, load everything for full compatibility.
    #[cfg(target_arch = "wasm32")]
    let lua = {
        use mlua::StdLib;
        let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
        let lua = Lua::new_with(libs, mlua::LuaOptions::default())
            .map_err(|e| LuaFilterError::LuaError(e))?;
        super::os_wasm::register_wasm_os(&lua, runtime.clone())?;
        super::io_wasm::register_wasm_io(&lua, runtime.clone())?;
        super::dofile_wasm::register_wasm_dofile(&lua, runtime.clone())?;
        lua
    };
    #[cfg(not(target_arch = "wasm32"))]
    let lua = Lua::new();

    // Create mediabag for storing media items
    // In the future, this could be pre-populated from the document or passed in
    let mediabag = create_shared_mediabag();

    // Register pandoc namespace with constructors (also registers quarto namespace)
    register_pandoc_namespace(&lua, runtime, mediabag)?;

    // Register quarto.json, quarto.log, quarto.utils
    register_quarto_api(&lua)?;

    // Register quarto.attribution.{lookup, lookup_range, identities}.
    // When `attribution` is `None`, registers no-op stubs:
    //   - `lookup` / `lookup_range` return nil
    //   - `identities` returns an empty table
    super::quarto_api::register_quarto_attribution(&lua, attribution)?;

    // Register quarto.doc namespace (is_format, add_html_dependency, etc.)
    super::quarto_doc::register_quarto_doc(&lua)?;

    // Push script dir for quarto.utils.resolve_path
    let script_dir = script_path
        .parent()
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .to_string();
    super::quarto_api::push_script_dir(&lua, &script_dir)?;

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
        script_path.to_string_lossy().to_string(),
    )?;

    // PANDOC_READER_OPTIONS - reader options used for the input
    // We provide default options since we don't track actual reader options yet
    let reader_options = create_reader_options_table(&lua, None)?;
    lua.globals().set("PANDOC_READER_OPTIONS", reader_options)?;

    // PANDOC_WRITER_OPTIONS - writer options to be used for output
    // We provide default options since we don't track actual writer options yet
    let writer_options = create_writer_options_table(&lua, None)?;
    lua.globals().set("PANDOC_WRITER_OPTIONS", writer_options)?;

    Ok(lua)
}

/// Apply a single Lua filter to a document.
///
/// Returns the filtered document, context, diagnostics, and any HTML
/// dependencies or text includes registered via the `quarto.doc` API.
///
/// The `attribution` handle backs the `quarto.attribution.*` Lua host
/// binding. Passing `None` registers no-op stubs (the binding is
/// alive but `lookup` / `lookup_range` return nil and `identities`
/// returns an empty table). Most callers pass `None`; only
/// `quarto-core::UserFiltersStage` passes `Some(handle)`.
pub async fn apply_lua_filter(
    pandoc: &Pandoc,
    context: &ASTContext,
    filter_path: &Path,
    target_format: &str,
    runtime: Arc<dyn SystemRuntime>,
    attribution: Option<Arc<dyn crate::attribution::AttributionLookup>>,
) -> FilterResult<FilterOutput> {
    // Read filter file via runtime (supports VFS on WASM)
    let filter_bytes = runtime.file_read(filter_path).map_err(|e| {
        LuaFilterError::FileReadError(filter_path.to_owned(), std::io::Error::other(e.to_string()))
    })?;
    let filter_source = String::from_utf8(filter_bytes).map_err(|e| {
        LuaFilterError::FileReadError(
            filter_path.to_owned(),
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        )
    })?;

    let lua = create_filter_environment(runtime, target_format, filter_path, attribution)?;

    // Load and execute filter script
    lua.load(&filter_source)
        .set_name(filter_path.to_string_lossy())
        .exec_async()
        .await?;

    // Get filter functions from globals or return value
    let filter_table = get_filter_table(&lua)?;

    // Determine traversal mode
    let walking_order = get_walking_order(&filter_table)?;

    // Apply the filter using the appropriate traversal
    let filtered_blocks = match walking_order {
        WalkingOrder::Typewise => {
            apply_typewise_filter(&lua, &filter_table, &pandoc.blocks).await?
        }
        WalkingOrder::Topdown => apply_topdown_filter(&lua, &filter_table, &pandoc.blocks).await?,
    };

    // Extract diagnostics, HTML dependencies, and text includes from Lua state
    let diagnostics = super::diagnostics::extract_lua_diagnostics(&lua)?;
    let html_dependencies = super::quarto_doc::extract_html_dependencies(&lua)?;
    let text_includes = super::quarto_doc::extract_text_includes(&lua)?;
    let resources = super::quarto_doc::extract_resources(&lua)?;

    // Return filtered document with all extracted data
    let filtered_pandoc = Pandoc {
        meta: pandoc.meta.clone(),
        blocks: filtered_blocks,
    };

    Ok(FilterOutput {
        pandoc: filtered_pandoc,
        context: context.clone(),
        diagnostics,
        html_dependencies,
        text_includes,
        resources,
    })
}

/// Apply multiple Lua filters in sequence.
///
/// Returns the filtered document, context, and accumulated
/// diagnostics, HTML dependencies, and text includes from all
/// filters. The `attribution` handle is threaded to every per-filter
/// Lua state — see [`apply_lua_filter`] for the contract.
pub async fn apply_lua_filters(
    pandoc: Pandoc,
    context: ASTContext,
    filter_paths: &[std::path::PathBuf],
    target_format: &str,
    runtime: Arc<dyn SystemRuntime>,
    attribution: Option<Arc<dyn crate::attribution::AttributionLookup>>,
) -> FilterResult<FilterOutput> {
    let mut current_pandoc = pandoc;
    let mut current_context = context;
    let mut all_diagnostics = Vec::new();
    let mut all_html_dependencies = Vec::new();
    let mut all_text_includes = Vec::new();
    let mut all_resources = Vec::new();

    for filter_path in filter_paths {
        let output = apply_lua_filter(
            &current_pandoc,
            &current_context,
            filter_path,
            target_format,
            runtime.clone(),
            attribution.clone(),
        )
        .await?;
        current_pandoc = output.pandoc;
        current_context = output.context;
        all_diagnostics.extend(output.diagnostics);
        all_html_dependencies.extend(output.html_dependencies);
        all_text_includes.extend(output.text_includes);
        all_resources.extend(output.resources);
    }

    Ok(FilterOutput {
        pandoc: current_pandoc,
        context: current_context,
        diagnostics: all_diagnostics,
        html_dependencies: all_html_dependencies,
        text_includes: all_text_includes,
        resources: all_resources,
    })
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
        // QMD-specific inline types
        "Insert",
        "Delete",
        "Highlight",
        "EditComment",
        "NoteReference",
        "Shortcode",
        "Custom",
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

/// Extract an Inline from a Lua UserData value (flushing any cached
/// property mutations first — see PropertyCache in types.rs).
pub(crate) fn extract_lua_inline(lua: &Lua, ud: &mlua::AnyUserData) -> Result<Inline> {
    ud.borrow::<LuaInline>()?.extract_flushed(lua)
}

/// Extract a Block from a Lua UserData value (flushing any cached
/// property mutations first).
pub(crate) fn extract_lua_block(lua: &Lua, ud: &mlua::AnyUserData) -> Result<Block> {
    ud.borrow::<LuaBlock>()?.extract_flushed(lua)
}

/// Extract a Vec<Inline> from a Lua table of UserData values.
pub(crate) fn extract_lua_inlines_from_table(
    lua: &Lua,
    table: &mlua::Table,
) -> Result<Vec<Inline>> {
    let len = table.raw_len();
    let mut inlines = Vec::new();
    for i in 1..=len {
        let value: Value = table.get(i)?;
        if let Value::UserData(ud) = value {
            inlines.push(extract_lua_inline(lua, &ud)?);
        }
    }
    Ok(inlines)
}

/// Extract a Vec<Block> from a Lua table of UserData values.
pub(crate) fn extract_lua_blocks_from_table(lua: &Lua, table: &mlua::Table) -> Result<Vec<Block>> {
    let len = table.raw_len();
    let mut blocks = Vec::new();
    for i in 1..=len {
        let value: Value = table.get(i)?;
        if let Value::UserData(ud) = value {
            blocks.push(extract_lua_block(lua, &ud)?);
        }
    }
    Ok(blocks)
}

/// Handle return value from an inline filter
fn handle_inline_return(lua: &Lua, ret: Value, original: &Inline) -> Result<Vec<Inline>> {
    match ret {
        Value::Nil => Ok(vec![original.clone()]),
        Value::UserData(ud) => {
            // Single element return - replace
            let lua_inline = ud.borrow::<LuaInline>()?;
            Ok(vec![lua_inline.extract_flushed(lua)?])
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
                    inlines.push(lua_inline.extract_flushed(lua)?);
                }
            }
            Ok(inlines)
        }
        _ => Ok(vec![original.clone()]),
    }
}

/// Handle return value from a block filter
fn handle_block_return(lua: &Lua, ret: Value, original: &Block) -> Result<Vec<Block>> {
    match ret {
        Value::Nil => Ok(vec![original.clone()]),
        Value::UserData(ud) => {
            // Single element return - replace
            let lua_block = ud.borrow::<LuaBlock>()?;
            Ok(vec![lua_block.extract_flushed(lua)?])
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
                    blocks.push(lua_block.extract_flushed(lua)?);
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
    lua: &Lua,
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
            vec![lua_inline.extract_flushed(lua)?]
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
                        inlines.push(lua_inline.extract_flushed(lua)?);
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
    lua: &Lua,
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
            vec![lua_block.extract_flushed(lua)?]
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
                        blocks.push(lua_block.extract_flushed(lua)?);
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
    lua: &Lua,
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
                        result.push(lua_block.extract_flushed(lua)?);
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
    lua: &Lua,
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
                        result.push(lua_inline.extract_flushed(lua)?);
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
        Block::Custom(_) => "Custom",
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
        Inline::Attr(_) => "Attr",
        Inline::Insert(_) => "Insert",
        Inline::Delete(_) => "Delete",
        Inline::Highlight(_) => "Highlight",
        Inline::EditComment(_) => "EditComment",
        Inline::Custom(_) => "Custom",
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
async fn walk_inline_splicing(
    lua: &Lua,
    filter_table: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    let mut result = Vec::new();
    for block in blocks {
        result.push(walk_block_for_inline_splicing(lua, filter_table, block).await?);
    }
    Ok(result)
}

/// Helper: Walk a single block, applying inline element filters to its inline content
fn walk_block_for_inline_splicing<'a>(
    lua: &'a Lua,
    filter_table: &'a Table,
    block: &'a Block,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Block>> + 'a>> {
    Box::pin(async move {
        match block {
            // Blocks with inline content
            Block::Paragraph(p) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &p.content).await?;
                Ok(Block::Paragraph(crate::pandoc::Paragraph {
                    content: filtered,
                    ..p.clone()
                }))
            }
            Block::Plain(p) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &p.content).await?;
                Ok(Block::Plain(crate::pandoc::Plain {
                    content: filtered,
                    ..p.clone()
                }))
            }
            Block::Header(h) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &h.content).await?;
                Ok(Block::Header(crate::pandoc::Header {
                    content: filtered,
                    ..h.clone()
                }))
            }
            // Blocks with nested block content
            Block::BlockQuote(b) => {
                let filtered = walk_inline_splicing(lua, filter_table, &b.content).await?;
                Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                    content: filtered,
                    ..b.clone()
                }))
            }
            Block::BulletList(l) => {
                let mut filtered_items = Vec::new();
                for item in &l.content {
                    filtered_items.push(walk_inline_splicing(lua, filter_table, item).await?);
                }
                Ok(Block::BulletList(crate::pandoc::BulletList {
                    content: filtered_items,
                    ..l.clone()
                }))
            }
            Block::OrderedList(l) => {
                let mut filtered_items = Vec::new();
                for item in &l.content {
                    filtered_items.push(walk_inline_splicing(lua, filter_table, item).await?);
                }
                Ok(Block::OrderedList(crate::pandoc::OrderedList {
                    content: filtered_items,
                    ..l.clone()
                }))
            }
            Block::Div(d) => {
                let filtered = walk_inline_splicing(lua, filter_table, &d.content).await?;
                Ok(Block::Div(crate::pandoc::Div {
                    content: filtered,
                    ..d.clone()
                }))
            }
            Block::Figure(f) => {
                let filtered = walk_inline_splicing(lua, filter_table, &f.content).await?;
                Ok(Block::Figure(crate::pandoc::Figure {
                    content: filtered,
                    ..f.clone()
                }))
            }
            Block::LineBlock(l) => {
                let mut filtered_lines = Vec::new();
                for line in &l.content {
                    filtered_lines
                        .push(walk_inlines_for_element_filters(lua, filter_table, line).await?);
                }
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
            | Block::CaptionBlock(_)
            | Block::Custom(_) => Ok(block.clone()),
        }
    })
}

/// Helper: Walk inlines applying element filters (Str, Emph, etc.) but NOT Inlines filter
async fn walk_inlines_for_element_filters(
    lua: &Lua,
    filter_table: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    let mut result = Vec::new();
    for inline in inlines {
        // First walk children (bottom-up)
        let walked = walk_inline_children_for_element_filters(lua, filter_table, inline).await?;

        // Then apply type-specific or generic Inline filter
        let tag = inline_tag(&walked);
        let filtered = if let Ok(filter_fn) = filter_table.get::<Function>(tag) {
            let inline_ud = lua.create_userdata(LuaInline::new(walked.clone()))?;
            let ret: Value = filter_fn.call_async(inline_ud).await?;
            handle_inline_return(lua, ret, &walked)?
        } else if let Ok(filter_fn) = filter_table.get::<Function>("Inline") {
            let inline_ud = lua.create_userdata(LuaInline::new(walked.clone()))?;
            let ret: Value = filter_fn.call_async(inline_ud).await?;
            handle_inline_return(lua, ret, &walked)?
        } else {
            vec![walked]
        };
        result.extend(filtered);
    }
    Ok(result)
}

/// Helper: Walk children of an inline for element filters
fn walk_inline_children_for_element_filters<'a>(
    lua: &'a Lua,
    filter_table: &'a Table,
    inline: &'a Inline,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Inline>> + 'a>> {
    Box::pin(async move {
        match inline {
            // Inlines with content children
            Inline::Emph(e) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &e.content).await?;
                Ok(Inline::Emph(crate::pandoc::Emph {
                    content: filtered,
                    ..e.clone()
                }))
            }
            Inline::Strong(s) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &s.content).await?;
                Ok(Inline::Strong(crate::pandoc::Strong {
                    content: filtered,
                    ..s.clone()
                }))
            }
            Inline::Underline(u) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &u.content).await?;
                Ok(Inline::Underline(crate::pandoc::Underline {
                    content: filtered,
                    ..u.clone()
                }))
            }
            Inline::Strikeout(s) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &s.content).await?;
                Ok(Inline::Strikeout(crate::pandoc::Strikeout {
                    content: filtered,
                    ..s.clone()
                }))
            }
            Inline::Superscript(s) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &s.content).await?;
                Ok(Inline::Superscript(crate::pandoc::Superscript {
                    content: filtered,
                    ..s.clone()
                }))
            }
            Inline::Subscript(s) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &s.content).await?;
                Ok(Inline::Subscript(crate::pandoc::Subscript {
                    content: filtered,
                    ..s.clone()
                }))
            }
            Inline::SmallCaps(s) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &s.content).await?;
                Ok(Inline::SmallCaps(crate::pandoc::SmallCaps {
                    content: filtered,
                    ..s.clone()
                }))
            }
            Inline::Quoted(q) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &q.content).await?;
                Ok(Inline::Quoted(crate::pandoc::Quoted {
                    content: filtered,
                    ..q.clone()
                }))
            }
            Inline::Link(l) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &l.content).await?;
                Ok(Inline::Link(crate::pandoc::Link {
                    content: filtered,
                    ..l.clone()
                }))
            }
            Inline::Image(i) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &i.content).await?;
                Ok(Inline::Image(crate::pandoc::Image {
                    content: filtered,
                    ..i.clone()
                }))
            }
            Inline::Span(s) => {
                let filtered =
                    walk_inlines_for_element_filters(lua, filter_table, &s.content).await?;
                Ok(Inline::Span(crate::pandoc::Span {
                    content: filtered,
                    ..s.clone()
                }))
            }
            // Note contains blocks - need to recurse into blocks
            Inline::Note(n) => {
                let filtered = walk_inline_splicing(lua, filter_table, &n.content).await?;
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
            | Inline::Attr(_)
            | Inline::Insert(_)
            | Inline::Delete(_)
            | Inline::Highlight(_)
            | Inline::EditComment(_)
            | Inline::Custom(_) => Ok(inline.clone()),
        }
    })
}

/// Pass 2: Walk the entire document and apply Inlines list filter.
/// This visits all inline LISTS (not individual elements).
async fn walk_inlines_straight(
    lua: &Lua,
    filter_table: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    // If no Inlines filter, pass through
    if filter_table.get::<Function>("Inlines").is_err() {
        return Ok(blocks.to_vec());
    }

    let mut result = Vec::new();
    for block in blocks {
        result.push(walk_block_for_inlines_straight(lua, filter_table, block).await?);
    }
    Ok(result)
}

/// Helper: Walk a single block, applying Inlines filter to inline lists
fn walk_block_for_inlines_straight<'a>(
    lua: &'a Lua,
    filter_table: &'a Table,
    block: &'a Block,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Block>> + 'a>> {
    Box::pin(async move {
        match block {
            // Blocks with inline content - apply Inlines filter
            Block::Paragraph(p) => {
                let filtered = apply_inlines_filter(lua, filter_table, &p.content).await?;
                Ok(Block::Paragraph(crate::pandoc::Paragraph {
                    content: filtered,
                    ..p.clone()
                }))
            }
            Block::Plain(p) => {
                let filtered = apply_inlines_filter(lua, filter_table, &p.content).await?;
                Ok(Block::Plain(crate::pandoc::Plain {
                    content: filtered,
                    ..p.clone()
                }))
            }
            Block::Header(h) => {
                let filtered = apply_inlines_filter(lua, filter_table, &h.content).await?;
                Ok(Block::Header(crate::pandoc::Header {
                    content: filtered,
                    ..h.clone()
                }))
            }
            // Blocks with nested block content
            Block::BlockQuote(b) => {
                let filtered = walk_inlines_straight(lua, filter_table, &b.content).await?;
                Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                    content: filtered,
                    ..b.clone()
                }))
            }
            Block::BulletList(l) => {
                let mut filtered_items = Vec::new();
                for item in &l.content {
                    filtered_items.push(walk_inlines_straight(lua, filter_table, item).await?);
                }
                Ok(Block::BulletList(crate::pandoc::BulletList {
                    content: filtered_items,
                    ..l.clone()
                }))
            }
            Block::OrderedList(l) => {
                let mut filtered_items = Vec::new();
                for item in &l.content {
                    filtered_items.push(walk_inlines_straight(lua, filter_table, item).await?);
                }
                Ok(Block::OrderedList(crate::pandoc::OrderedList {
                    content: filtered_items,
                    ..l.clone()
                }))
            }
            Block::Div(d) => {
                let filtered = walk_inlines_straight(lua, filter_table, &d.content).await?;
                Ok(Block::Div(crate::pandoc::Div {
                    content: filtered,
                    ..d.clone()
                }))
            }
            Block::Figure(f) => {
                let filtered = walk_inlines_straight(lua, filter_table, &f.content).await?;
                Ok(Block::Figure(crate::pandoc::Figure {
                    content: filtered,
                    ..f.clone()
                }))
            }
            Block::LineBlock(l) => {
                // Each line is a separate inline list
                let mut filtered_lines = Vec::new();
                for line in &l.content {
                    filtered_lines.push(apply_inlines_filter(lua, filter_table, line).await?);
                }
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
            | Block::CaptionBlock(_)
            | Block::Custom(_) => Ok(block.clone()),
        }
    })
}

/// Helper: Apply the Inlines filter to a list of inlines
async fn apply_inlines_filter(
    lua: &Lua,
    filter_table: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    if let Ok(inlines_fn) = filter_table.get::<Function>("Inlines") {
        let inlines_table = inlines_to_lua_table(lua, inlines)?;
        let ret: Value = inlines_fn.call_async(inlines_table).await?;
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
                        result.push(lua_inline.extract_flushed(lua)?);
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
async fn walk_block_splicing(
    lua: &Lua,
    filter_table: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    let mut result = Vec::new();
    for block in blocks {
        // First walk children (bottom-up)
        let walked = walk_block_children_for_element_filters(lua, filter_table, block).await?;

        // Then apply type-specific or generic Block filter
        let tag = block_tag(&walked);
        let filtered = if let Ok(filter_fn) = filter_table.get::<Function>(tag) {
            let block_ud = lua.create_userdata(LuaBlock::new(walked.clone()))?;
            let ret: Value = filter_fn.call_async(block_ud).await?;
            handle_block_return(lua, ret, &walked)?
        } else if let Ok(filter_fn) = filter_table.get::<Function>("Block") {
            let block_ud = lua.create_userdata(LuaBlock::new(walked.clone()))?;
            let ret: Value = filter_fn.call_async(block_ud).await?;
            handle_block_return(lua, ret, &walked)?
        } else {
            vec![walked]
        };
        result.extend(filtered);
    }
    Ok(result)
}

/// Helper: Walk children of a block for element filters (but don't apply filter to block itself)
fn walk_block_children_for_element_filters<'a>(
    lua: &'a Lua,
    filter_table: &'a Table,
    block: &'a Block,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Block>> + 'a>> {
    Box::pin(async move {
        match block {
            // Blocks with nested block content
            Block::BlockQuote(b) => {
                let filtered = walk_block_splicing(lua, filter_table, &b.content).await?;
                Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                    content: filtered,
                    ..b.clone()
                }))
            }
            Block::BulletList(l) => {
                let mut filtered_items = Vec::new();
                for item in &l.content {
                    filtered_items.push(walk_block_splicing(lua, filter_table, item).await?);
                }
                Ok(Block::BulletList(crate::pandoc::BulletList {
                    content: filtered_items,
                    ..l.clone()
                }))
            }
            Block::OrderedList(l) => {
                let mut filtered_items = Vec::new();
                for item in &l.content {
                    filtered_items.push(walk_block_splicing(lua, filter_table, item).await?);
                }
                Ok(Block::OrderedList(crate::pandoc::OrderedList {
                    content: filtered_items,
                    ..l.clone()
                }))
            }
            Block::Div(d) => {
                let filtered = walk_block_splicing(lua, filter_table, &d.content).await?;
                Ok(Block::Div(crate::pandoc::Div {
                    content: filtered,
                    ..d.clone()
                }))
            }
            Block::Figure(f) => {
                let filtered = walk_block_splicing(lua, filter_table, &f.content).await?;
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
            | Block::CaptionBlock(_)
            | Block::Custom(_) => Ok(block.clone()),
        }
    })
}

/// Pass 4: Walk the entire document and apply Blocks list filter.
/// This visits all block LISTS (not individual elements).
async fn walk_blocks_straight(
    lua: &Lua,
    filter_table: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    // First recurse into nested block lists
    let mut walked = Vec::new();
    for block in blocks {
        walked.push(walk_block_for_blocks_straight(lua, filter_table, block).await?);
    }

    // Then apply Blocks filter to this list
    if let Ok(blocks_fn) = filter_table.get::<Function>("Blocks") {
        let blocks_table = blocks_to_lua_table(lua, &walked)?;
        let ret: Value = blocks_fn.call_async(blocks_table).await?;
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
                        result.push(lua_block.extract_flushed(lua)?);
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
fn walk_block_for_blocks_straight<'a>(
    lua: &'a Lua,
    filter_table: &'a Table,
    block: &'a Block,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Block>> + 'a>> {
    Box::pin(async move {
        match block {
            // Blocks with nested block content
            Block::BlockQuote(b) => {
                let filtered = walk_blocks_straight(lua, filter_table, &b.content).await?;
                Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                    content: filtered,
                    ..b.clone()
                }))
            }
            Block::BulletList(l) => {
                let mut filtered_items = Vec::new();
                for item in &l.content {
                    filtered_items.push(walk_blocks_straight(lua, filter_table, item).await?);
                }
                Ok(Block::BulletList(crate::pandoc::BulletList {
                    content: filtered_items,
                    ..l.clone()
                }))
            }
            Block::OrderedList(l) => {
                let mut filtered_items = Vec::new();
                for item in &l.content {
                    filtered_items.push(walk_blocks_straight(lua, filter_table, item).await?);
                }
                Ok(Block::OrderedList(crate::pandoc::OrderedList {
                    content: filtered_items,
                    ..l.clone()
                }))
            }
            Block::Div(d) => {
                let filtered = walk_blocks_straight(lua, filter_table, &d.content).await?;
                Ok(Block::Div(crate::pandoc::Div {
                    content: filtered,
                    ..d.clone()
                }))
            }
            Block::Figure(f) => {
                let filtered = walk_blocks_straight(lua, filter_table, &f.content).await?;
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
            | Block::CaptionBlock(_)
            | Block::Custom(_) => Ok(block.clone()),
        }
    })
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
async fn apply_topdown_filter(
    lua: &Lua,
    filter_table: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    walk_blocks_topdown(lua, filter_table, blocks).await
}

/// Walk blocks in topdown order: apply Blocks filter, then each block, then children
pub async fn walk_blocks_topdown(
    lua: &Lua,
    filter_table: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    // Step 1: Apply Blocks filter first (operates on whole list)
    let (blocks, ctrl) = if let Ok(blocks_fn) = filter_table.get::<Function>("Blocks") {
        let blocks_table = blocks_to_lua_table(lua, blocks)?;
        let ret: MultiValue = blocks_fn.call_async(blocks_table).await?;
        handle_blocks_return_with_control(lua, ret, blocks)?
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
        let (filtered, ctrl) = apply_block_filter_topdown(lua, filter_table, block).await?;
        for b in filtered {
            if ctrl == TraversalControl::Stop {
                // Don't descend into children
                result.push(b);
            } else {
                // Recurse into children
                let walked = walk_block_children_topdown(lua, filter_table, &b).await?;
                result.push(walked);
            }
        }
    }
    Ok(result)
}

/// Apply filter to a single block and return (result, control)
async fn apply_block_filter_topdown(
    lua: &Lua,
    filter_table: &Table,
    block: &Block,
) -> Result<(Vec<Block>, TraversalControl)> {
    let tag = block_tag(block);

    // Try type-specific filter first, then generic Block filter
    if let Ok(filter_fn) = filter_table.get::<Function>(tag) {
        let block_ud = lua.create_userdata(LuaBlock::new(block.clone()))?;
        let ret: MultiValue = filter_fn.call_async(block_ud).await?;
        handle_block_return_with_control(lua, ret, block)
    } else if let Ok(filter_fn) = filter_table.get::<Function>("Block") {
        let block_ud = lua.create_userdata(LuaBlock::new(block.clone()))?;
        let ret: MultiValue = filter_fn.call_async(block_ud).await?;
        handle_block_return_with_control(lua, ret, block)
    } else {
        Ok((vec![block.clone()], TraversalControl::Continue))
    }
}

/// Walk children of a block in topdown order
fn walk_block_children_topdown<'a>(
    lua: &'a Lua,
    filter_table: &'a Table,
    block: &'a Block,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Block>> + 'a>> {
    Box::pin(async move {
        match block {
            // Blocks with inline content
            Block::Paragraph(p) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &p.content).await?;
                Ok(Block::Paragraph(crate::pandoc::Paragraph {
                    content: filtered,
                    ..p.clone()
                }))
            }
            Block::Plain(p) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &p.content).await?;
                Ok(Block::Plain(crate::pandoc::Plain {
                    content: filtered,
                    ..p.clone()
                }))
            }
            Block::Header(h) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &h.content).await?;
                Ok(Block::Header(crate::pandoc::Header {
                    content: filtered,
                    ..h.clone()
                }))
            }
            // Blocks with nested block content
            Block::BlockQuote(b) => {
                let filtered = walk_blocks_topdown(lua, filter_table, &b.content).await?;
                Ok(Block::BlockQuote(crate::pandoc::BlockQuote {
                    content: filtered,
                    ..b.clone()
                }))
            }
            Block::BulletList(l) => {
                let mut filtered_items = Vec::new();
                for item in &l.content {
                    filtered_items.push(walk_blocks_topdown(lua, filter_table, item).await?);
                }
                Ok(Block::BulletList(crate::pandoc::BulletList {
                    content: filtered_items,
                    ..l.clone()
                }))
            }
            Block::OrderedList(l) => {
                let mut filtered_items = Vec::new();
                for item in &l.content {
                    filtered_items.push(walk_blocks_topdown(lua, filter_table, item).await?);
                }
                Ok(Block::OrderedList(crate::pandoc::OrderedList {
                    content: filtered_items,
                    ..l.clone()
                }))
            }
            Block::Div(d) => {
                let filtered = walk_blocks_topdown(lua, filter_table, &d.content).await?;
                Ok(Block::Div(crate::pandoc::Div {
                    content: filtered,
                    ..d.clone()
                }))
            }
            Block::Figure(f) => {
                let filtered = walk_blocks_topdown(lua, filter_table, &f.content).await?;
                Ok(Block::Figure(crate::pandoc::Figure {
                    content: filtered,
                    ..f.clone()
                }))
            }
            Block::LineBlock(l) => {
                let mut filtered_lines = Vec::new();
                for line in &l.content {
                    filtered_lines.push(walk_inlines_topdown(lua, filter_table, line).await?);
                }
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
            | Block::CaptionBlock(_)
            | Block::Custom(_) => Ok(block.clone()),
        }
    })
}

/// Walk inlines in topdown order: apply Inlines filter, then each inline, then children
pub async fn walk_inlines_topdown(
    lua: &Lua,
    filter_table: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    // Step 1: Apply Inlines filter first (operates on whole list)
    let (inlines, ctrl) = if let Ok(inlines_fn) = filter_table.get::<Function>("Inlines") {
        let inlines_table = inlines_to_lua_table(lua, inlines)?;
        let ret: MultiValue = inlines_fn.call_async(inlines_table).await?;
        handle_inlines_return_with_control(lua, ret, inlines)?
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
        let (filtered, ctrl) = apply_inline_filter_topdown(lua, filter_table, inline).await?;
        for i in filtered {
            if ctrl == TraversalControl::Stop {
                // Don't descend into children
                result.push(i);
            } else {
                // Recurse into children
                let walked = walk_inline_children_topdown(lua, filter_table, &i).await?;
                result.push(walked);
            }
        }
    }
    Ok(result)
}

/// Apply filter to a single inline and return (result, control)
async fn apply_inline_filter_topdown(
    lua: &Lua,
    filter_table: &Table,
    inline: &Inline,
) -> Result<(Vec<Inline>, TraversalControl)> {
    let tag = inline_tag(inline);

    // Try type-specific filter first, then generic Inline filter
    if let Ok(filter_fn) = filter_table.get::<Function>(tag) {
        let inline_ud = lua.create_userdata(LuaInline::new(inline.clone()))?;
        let ret: MultiValue = filter_fn.call_async(inline_ud).await?;
        handle_inline_return_with_control(lua, ret, inline)
    } else if let Ok(filter_fn) = filter_table.get::<Function>("Inline") {
        let inline_ud = lua.create_userdata(LuaInline::new(inline.clone()))?;
        let ret: MultiValue = filter_fn.call_async(inline_ud).await?;
        handle_inline_return_with_control(lua, ret, inline)
    } else {
        Ok((vec![inline.clone()], TraversalControl::Continue))
    }
}

/// Walk children of an inline in topdown order
fn walk_inline_children_topdown<'a>(
    lua: &'a Lua,
    filter_table: &'a Table,
    inline: &'a Inline,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Inline>> + 'a>> {
    Box::pin(async move {
        match inline {
            // Inlines with content children
            Inline::Emph(e) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &e.content).await?;
                Ok(Inline::Emph(crate::pandoc::Emph {
                    content: filtered,
                    ..e.clone()
                }))
            }
            Inline::Strong(s) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &s.content).await?;
                Ok(Inline::Strong(crate::pandoc::Strong {
                    content: filtered,
                    ..s.clone()
                }))
            }
            Inline::Underline(u) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &u.content).await?;
                Ok(Inline::Underline(crate::pandoc::Underline {
                    content: filtered,
                    ..u.clone()
                }))
            }
            Inline::Strikeout(s) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &s.content).await?;
                Ok(Inline::Strikeout(crate::pandoc::Strikeout {
                    content: filtered,
                    ..s.clone()
                }))
            }
            Inline::Superscript(s) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &s.content).await?;
                Ok(Inline::Superscript(crate::pandoc::Superscript {
                    content: filtered,
                    ..s.clone()
                }))
            }
            Inline::Subscript(s) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &s.content).await?;
                Ok(Inline::Subscript(crate::pandoc::Subscript {
                    content: filtered,
                    ..s.clone()
                }))
            }
            Inline::SmallCaps(s) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &s.content).await?;
                Ok(Inline::SmallCaps(crate::pandoc::SmallCaps {
                    content: filtered,
                    ..s.clone()
                }))
            }
            Inline::Quoted(q) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &q.content).await?;
                Ok(Inline::Quoted(crate::pandoc::Quoted {
                    content: filtered,
                    ..q.clone()
                }))
            }
            Inline::Link(l) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &l.content).await?;
                Ok(Inline::Link(crate::pandoc::Link {
                    content: filtered,
                    ..l.clone()
                }))
            }
            Inline::Image(i) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &i.content).await?;
                Ok(Inline::Image(crate::pandoc::Image {
                    content: filtered,
                    ..i.clone()
                }))
            }
            Inline::Span(s) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &s.content).await?;
                Ok(Inline::Span(crate::pandoc::Span {
                    content: filtered,
                    ..s.clone()
                }))
            }
            // Note contains blocks
            Inline::Note(n) => {
                let filtered = walk_blocks_topdown(lua, filter_table, &n.content).await?;
                Ok(Inline::Note(crate::pandoc::Note {
                    content: filtered,
                    ..n.clone()
                }))
            }
            // CriticMarkup types
            Inline::Insert(i) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &i.content).await?;
                Ok(Inline::Insert(crate::pandoc::Insert {
                    content: filtered,
                    ..i.clone()
                }))
            }
            Inline::Delete(d) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &d.content).await?;
                Ok(Inline::Delete(crate::pandoc::Delete {
                    content: filtered,
                    ..d.clone()
                }))
            }
            Inline::Highlight(h) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &h.content).await?;
                Ok(Inline::Highlight(crate::pandoc::Highlight {
                    content: filtered,
                    ..h.clone()
                }))
            }
            Inline::EditComment(ec) => {
                let filtered = walk_inlines_topdown(lua, filter_table, &ec.content).await?;
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
            | Inline::Attr(_)
            | Inline::Custom(_) => Ok(inline.clone()),
        }
    })
}

/// Apply typewise filter traversal (four separate passes)
pub async fn apply_typewise_filter(
    lua: &Lua,
    filter_table: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    // Pass 1: Walk all inlines (splicing)
    let blocks = walk_inline_splicing(lua, filter_table, blocks).await?;
    // Pass 2: Walk all inline lists (Inlines filter)
    let blocks = walk_inlines_straight(lua, filter_table, &blocks).await?;
    // Pass 3: Walk all blocks (splicing)
    let blocks = walk_block_splicing(lua, filter_table, &blocks).await?;
    // Pass 4: Walk all block lists (Blocks filter)
    walk_blocks_straight(lua, filter_table, &blocks).await
}

/// Apply typewise filter traversal to inlines only (two passes)
/// Used for elem:walk{} on inline elements and inline lists
pub async fn apply_typewise_inlines(
    lua: &Lua,
    filter_table: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    // Pass 1: Walk all inline elements (splicing)
    let inlines = walk_inlines_for_element_filters(lua, filter_table, inlines).await?;
    // Pass 2: Apply Inlines list filter
    apply_inlines_filter(lua, filter_table, &inlines).await
}

#[cfg(test)]
#[path = "filter_tests.rs"]
mod integration_tests;

#[cfg(test)]
mod unit_tests {
    use super::*;

    // =========================================================================
    // LuaFilterError tests
    // =========================================================================

    #[test]
    fn test_lua_filter_error_file_read_display() {
        let path = std::path::PathBuf::from("/path/to/filter.lua");
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = LuaFilterError::FileReadError(path, io_err);
        let display = format!("{}", err);
        assert!(display.contains("Failed to read filter"));
        assert!(display.contains("/path/to/filter.lua"));
        assert!(display.contains("file not found"));
    }

    #[test]
    fn test_lua_filter_error_lua_error_display() {
        let lua_err = mlua::Error::RuntimeError("test error".to_string());
        let err = LuaFilterError::LuaError(lua_err);
        let display = format!("{}", err);
        assert!(display.contains("Lua filter error"));
        assert!(display.contains("test error"));
    }

    #[test]
    fn test_lua_filter_error_invalid_return_display() {
        let err = LuaFilterError::InvalidReturn("unexpected value".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Invalid filter return"));
        assert!(display.contains("unexpected value"));
    }

    #[test]
    fn test_lua_filter_error_from_mlua_error() {
        let lua_err = mlua::Error::RuntimeError("conversion test".to_string());
        let filter_err: LuaFilterError = lua_err.into();
        match filter_err {
            LuaFilterError::LuaError(e) => {
                assert!(e.to_string().contains("conversion test"));
            }
            _ => panic!("Expected LuaError variant"),
        }
    }

    #[test]
    fn test_lua_filter_error_is_std_error() {
        let err = LuaFilterError::InvalidReturn("test".to_string());
        // Verify it implements std::error::Error (compile-time check)
        let _: &dyn std::error::Error = &err;
    }

    // =========================================================================
    // WalkingOrder and get_walking_order tests
    // =========================================================================

    #[test]
    fn test_walking_order_debug() {
        assert_eq!(format!("{:?}", WalkingOrder::Typewise), "Typewise");
        assert_eq!(format!("{:?}", WalkingOrder::Topdown), "Topdown");
    }

    #[test]
    fn test_get_walking_order_default() {
        let lua = Lua::new();
        let filter_table = lua.create_table().unwrap();
        let order = get_walking_order(&filter_table).unwrap();
        assert_eq!(order, WalkingOrder::Typewise);
    }

    #[test]
    fn test_get_walking_order_typewise_explicit() {
        let lua = Lua::new();
        let filter_table = lua.create_table().unwrap();
        filter_table.set("traverse", "typewise").unwrap();
        let order = get_walking_order(&filter_table).unwrap();
        assert_eq!(order, WalkingOrder::Typewise);
    }

    #[test]
    fn test_get_walking_order_topdown() {
        let lua = Lua::new();
        let filter_table = lua.create_table().unwrap();
        filter_table.set("traverse", "topdown").unwrap();
        let order = get_walking_order(&filter_table).unwrap();
        assert_eq!(order, WalkingOrder::Topdown);
    }

    // =========================================================================
    // block_tag tests
    // =========================================================================

    #[test]
    fn test_block_tag_all_variants() {
        use crate::pandoc::block::*;
        use crate::pandoc::caption::Caption;
        use crate::pandoc::custom::CustomNode;
        use crate::pandoc::table::{Table, TableFoot, TableHead};
        use crate::pandoc::{AttrSourceInfo, Block};
        use hashlink::LinkedHashMap;
        use quarto_source_map::SourceInfo;

        let source_info = SourceInfo::for_test();

        assert_eq!(
            block_tag(&Block::Plain(Plain {
                content: vec![],
                source_info: source_info.clone()
            })),
            "Plain"
        );
        assert_eq!(
            block_tag(&Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![],
                source_info: source_info.clone()
            })),
            "Para"
        );
        assert_eq!(
            block_tag(&Block::LineBlock(LineBlock {
                content: vec![],
                source_info: source_info.clone()
            })),
            "LineBlock"
        );
        assert_eq!(
            block_tag(&Block::CodeBlock(CodeBlock {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                text: String::new(),
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "CodeBlock"
        );
        assert_eq!(
            block_tag(&Block::RawBlock(RawBlock {
                format: String::new(),
                text: String::new(),
                source_info: source_info.clone()
            })),
            "RawBlock"
        );
        assert_eq!(
            block_tag(&Block::BlockQuote(BlockQuote {
                content: vec![],
                source_info: source_info.clone()
            })),
            "BlockQuote"
        );
        assert_eq!(
            block_tag(&Block::OrderedList(OrderedList {
                attr: (
                    1,
                    crate::pandoc::list::ListNumberStyle::Default,
                    crate::pandoc::list::ListNumberDelim::Default
                ),
                content: vec![],
                source_info: source_info.clone()
            })),
            "OrderedList"
        );
        assert_eq!(
            block_tag(&Block::BulletList(BulletList {
                content: vec![],
                source_info: source_info.clone()
            })),
            "BulletList"
        );
        assert_eq!(
            block_tag(&Block::DefinitionList(DefinitionList {
                content: vec![],
                source_info: source_info.clone()
            })),
            "DefinitionList"
        );
        assert_eq!(
            block_tag(&Block::Header(Header {
                level: 1,
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "Header"
        );
        assert_eq!(
            block_tag(&Block::HorizontalRule(HorizontalRule {
                source_info: source_info.clone()
            })),
            "HorizontalRule"
        );
        assert_eq!(
            block_tag(&Block::Table(Table {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                caption: Caption {
                    short: None,
                    long: None,
                    source_info: source_info.clone()
                },
                colspec: vec![],
                head: TableHead {
                    attr: (String::new(), vec![], LinkedHashMap::new()),
                    rows: vec![],
                    source_info: source_info.clone(),
                    attr_source: AttrSourceInfo::empty()
                },
                bodies: vec![],
                foot: TableFoot {
                    attr: (String::new(), vec![], LinkedHashMap::new()),
                    rows: vec![],
                    source_info: source_info.clone(),
                    attr_source: AttrSourceInfo::empty()
                },
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "Table"
        );
        assert_eq!(
            block_tag(&Block::Figure(Figure {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                caption: Caption {
                    short: None,
                    long: None,
                    source_info: source_info.clone()
                },
                content: vec![],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "Figure"
        );
        assert_eq!(
            block_tag(&Block::Div(Div {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "Div"
        );
        assert_eq!(
            block_tag(&Block::BlockMetadata(MetaBlock {
                meta: crate::pandoc::config_value::ConfigValue::null(source_info.clone()),
                source_info: source_info.clone()
            })),
            "BlockMetadata"
        );
        assert_eq!(
            block_tag(&Block::NoteDefinitionPara(NoteDefinitionPara {
                id: String::new(),
                content: vec![],
                source_info: source_info.clone()
            })),
            "NoteDefinitionPara"
        );
        assert_eq!(
            block_tag(&Block::NoteDefinitionFencedBlock(
                NoteDefinitionFencedBlock {
                    id: String::new(),
                    content: vec![],
                    source_info: source_info.clone()
                }
            )),
            "NoteDefinitionFencedBlock"
        );
        assert_eq!(
            block_tag(&Block::CaptionBlock(CaptionBlock {
                content: vec![],
                source_info: source_info.clone()
            })),
            "CaptionBlock"
        );
        assert_eq!(
            block_tag(&Block::Custom(CustomNode {
                type_name: String::new(),
                slots: LinkedHashMap::new(),
                plain_data: serde_json::Value::Null,
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: source_info.clone()
            })),
            "Custom"
        );
    }

    // =========================================================================
    // inline_tag tests
    // =========================================================================

    #[test]
    fn test_inline_tag_all_variants() {
        use crate::pandoc::custom::CustomNode;
        use crate::pandoc::inline::*;
        use crate::pandoc::{AttrSourceInfo, Inline, InlineAttr, TargetSourceInfo};
        use hashlink::LinkedHashMap;
        use quarto_source_map::SourceInfo;

        let source_info = SourceInfo::for_test();

        assert_eq!(
            inline_tag(&Inline::Str(Str {
                text: String::new(),
                source_info: source_info.clone()
            })),
            "Str"
        );
        assert_eq!(
            inline_tag(&Inline::Emph(Emph {
                content: vec![],
                source_info: source_info.clone()
            })),
            "Emph"
        );
        assert_eq!(
            inline_tag(&Inline::Underline(Underline {
                content: vec![],
                source_info: source_info.clone()
            })),
            "Underline"
        );
        assert_eq!(
            inline_tag(&Inline::Strong(Strong {
                content: vec![],
                source_info: source_info.clone()
            })),
            "Strong"
        );
        assert_eq!(
            inline_tag(&Inline::Strikeout(Strikeout {
                content: vec![],
                source_info: source_info.clone()
            })),
            "Strikeout"
        );
        assert_eq!(
            inline_tag(&Inline::Superscript(Superscript {
                content: vec![],
                source_info: source_info.clone()
            })),
            "Superscript"
        );
        assert_eq!(
            inline_tag(&Inline::Subscript(Subscript {
                content: vec![],
                source_info: source_info.clone()
            })),
            "Subscript"
        );
        assert_eq!(
            inline_tag(&Inline::SmallCaps(SmallCaps {
                content: vec![],
                source_info: source_info.clone()
            })),
            "SmallCaps"
        );
        assert_eq!(
            inline_tag(&Inline::Quoted(Quoted {
                quote_type: QuoteType::DoubleQuote,
                content: vec![],
                source_info: source_info.clone()
            })),
            "Quoted"
        );
        assert_eq!(
            inline_tag(&Inline::Cite(Cite {
                citations: vec![],
                content: vec![],
                source_info: source_info.clone()
            })),
            "Cite"
        );
        assert_eq!(
            inline_tag(&Inline::Code(Code {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                text: String::new(),
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "Code"
        );
        assert_eq!(
            inline_tag(&Inline::Space(Space {
                source_info: source_info.clone()
            })),
            "Space"
        );
        assert_eq!(
            inline_tag(&Inline::SoftBreak(SoftBreak {
                source_info: source_info.clone()
            })),
            "SoftBreak"
        );
        assert_eq!(
            inline_tag(&Inline::LineBreak(LineBreak {
                source_info: source_info.clone()
            })),
            "LineBreak"
        );
        assert_eq!(
            inline_tag(&Inline::Math(Math {
                math_type: MathType::InlineMath,
                text: String::new(),
                source_info: source_info.clone()
            })),
            "Math"
        );
        assert_eq!(
            inline_tag(&Inline::RawInline(RawInline {
                format: String::new(),
                text: String::new(),
                source_info: source_info.clone()
            })),
            "RawInline"
        );
        assert_eq!(
            inline_tag(&Inline::Link(Link {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![],
                target: (String::new(), String::new()),
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
                target_source: TargetSourceInfo::empty()
            })),
            "Link"
        );
        assert_eq!(
            inline_tag(&Inline::Image(Image {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![],
                target: (String::new(), String::new()),
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
                target_source: TargetSourceInfo::empty()
            })),
            "Image"
        );
        assert_eq!(
            inline_tag(&Inline::Note(Note {
                content: vec![],
                source_info: source_info.clone()
            })),
            "Note"
        );
        assert_eq!(
            inline_tag(&Inline::Span(Span {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "Span"
        );
        assert_eq!(
            inline_tag(&Inline::Shortcode(
                quarto_pandoc_types::shortcode::Shortcode {
                    is_escaped: false,
                    name: String::new(),
                    positional_args: vec![],
                    keyword_args: LinkedHashMap::new(),
                    source_info: source_info.clone()
                }
            )),
            "Shortcode"
        );
        assert_eq!(
            inline_tag(&Inline::NoteReference(NoteReference {
                id: String::new(),
                source_info: source_info.clone()
            })),
            "NoteReference"
        );
        assert_eq!(
            inline_tag(&Inline::Attr(InlineAttr::new(
                (String::new(), vec![], LinkedHashMap::new()),
                AttrSourceInfo::empty(),
                quarto_source_map::SourceInfo::for_test(),
            ))),
            "Attr"
        );
        assert_eq!(
            inline_tag(&Inline::Insert(Insert {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "Insert"
        );
        assert_eq!(
            inline_tag(&Inline::Delete(Delete {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "Delete"
        );
        assert_eq!(
            inline_tag(&Inline::Highlight(Highlight {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "Highlight"
        );
        assert_eq!(
            inline_tag(&Inline::EditComment(EditComment {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty()
            })),
            "EditComment"
        );
        assert_eq!(
            inline_tag(&Inline::Custom(CustomNode {
                type_name: String::new(),
                slots: LinkedHashMap::new(),
                plain_data: serde_json::Value::Null,
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: source_info.clone()
            })),
            "Custom"
        );
    }

    // =========================================================================
    // handle_inline_return tests
    // =========================================================================

    #[test]
    fn test_handle_inline_return_nil() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        let result = handle_inline_return(&lua, Value::Nil, &original).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Inline::Str(s) => assert_eq!(s.text, "original"),
            _ => panic!("Expected Str"),
        }
    }

    #[test]
    fn test_handle_inline_return_empty_table() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let lua = Lua::new();
        let empty_table = lua.create_table().unwrap();
        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        let result = handle_inline_return(&lua, Value::Table(empty_table), &original).unwrap();
        assert_eq!(result.len(), 0); // Empty table means delete
    }

    #[test]
    fn test_handle_inline_return_other_value() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        // Non-nil, non-table, non-userdata returns original unchanged
        let result = handle_inline_return(&lua, Value::Integer(42), &original).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Inline::Str(s) => assert_eq!(s.text, "original"),
            _ => panic!("Expected Str"),
        }
    }

    // =========================================================================
    // handle_block_return tests
    // =========================================================================

    #[test]
    fn test_handle_block_return_nil() {
        let lua = Lua::new();
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::SourceInfo;

        let original = Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        });
        let result = handle_block_return(&lua, Value::Nil, &original).unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], Block::Plain(_)));
    }

    #[test]
    fn test_handle_block_return_empty_table() {
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::SourceInfo;

        let lua = Lua::new();
        let empty_table = lua.create_table().unwrap();
        let original = Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        });
        let result = handle_block_return(&lua, Value::Table(empty_table), &original).unwrap();
        assert_eq!(result.len(), 0); // Empty table means delete
    }

    #[test]
    fn test_handle_block_return_other_value() {
        let lua = Lua::new();
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::SourceInfo;

        let original = Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        });
        // Non-nil, non-table, non-userdata returns original unchanged
        let result = handle_block_return(&lua, Value::Integer(42), &original).unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], Block::Plain(_)));
    }

    // =========================================================================
    // handle_*_return_with_control tests
    // =========================================================================

    #[test]
    fn test_handle_inline_return_with_control_nil() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        let (elements, control) =
            handle_inline_return_with_control(&lua, MultiValue::new(), &original).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(control, TraversalControl::Continue);
    }

    #[test]
    fn test_handle_inline_return_with_control_stop() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        let mut values = MultiValue::new();
        values.push_front(Value::Nil);
        values.push_back(Value::Boolean(false));
        let (elements, control) =
            handle_inline_return_with_control(&lua, values, &original).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(control, TraversalControl::Stop);
    }

    #[test]
    fn test_handle_inline_return_with_control_continue_explicit() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        let mut values = MultiValue::new();
        values.push_front(Value::Nil);
        values.push_back(Value::Boolean(true));
        let (elements, control) =
            handle_inline_return_with_control(&lua, values, &original).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(control, TraversalControl::Continue);
    }

    #[test]
    fn test_handle_block_return_with_control_nil() {
        let lua = Lua::new();
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::SourceInfo;

        let original = Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        });
        let (elements, control) =
            handle_block_return_with_control(&lua, MultiValue::new(), &original).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(control, TraversalControl::Continue);
    }

    #[test]
    fn test_handle_block_return_with_control_stop() {
        let lua = Lua::new();
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::SourceInfo;

        let original = Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        });
        let mut values = MultiValue::new();
        values.push_front(Value::Nil);
        values.push_back(Value::Boolean(false));
        let (elements, control) =
            handle_block_return_with_control(&lua, values, &original).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(control, TraversalControl::Stop);
    }

    #[test]
    fn test_handle_blocks_return_with_control_nil() {
        let lua = Lua::new();
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::SourceInfo;

        let original = vec![Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        })];
        let (elements, control) =
            handle_blocks_return_with_control(&lua, MultiValue::new(), &original).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(control, TraversalControl::Continue);
    }

    #[test]
    fn test_handle_blocks_return_with_control_stop() {
        let lua = Lua::new();
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::SourceInfo;

        let original = vec![Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        })];
        let mut values = MultiValue::new();
        values.push_front(Value::Nil);
        values.push_back(Value::Boolean(false));
        let (elements, control) =
            handle_blocks_return_with_control(&lua, values, &original).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(control, TraversalControl::Stop);
    }

    #[test]
    fn test_handle_inlines_return_with_control_nil() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = vec![Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        })];
        let (elements, control) =
            handle_inlines_return_with_control(&lua, MultiValue::new(), &original).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(control, TraversalControl::Continue);
    }

    #[test]
    fn test_handle_inlines_return_with_control_stop() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = vec![Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        })];
        let mut values = MultiValue::new();
        values.push_front(Value::Nil);
        values.push_back(Value::Boolean(false));
        let (elements, control) =
            handle_inlines_return_with_control(&lua, values, &original).unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(control, TraversalControl::Stop);
    }

    #[test]
    fn test_handle_inlines_return_with_control_other_value() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = vec![Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        })];
        let mut values = MultiValue::new();
        values.push_front(Value::Integer(42)); // Not a table or nil
        let (elements, control) =
            handle_inlines_return_with_control(&lua, values, &original).unwrap();
        assert_eq!(elements.len(), 1); // Falls back to original
        assert_eq!(control, TraversalControl::Continue);
    }

    #[test]
    fn test_handle_blocks_return_with_control_other_value() {
        let lua = Lua::new();
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::SourceInfo;

        let original = vec![Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        })];
        let mut values = MultiValue::new();
        values.push_front(Value::Integer(42)); // Not a table or nil
        let (elements, control) =
            handle_blocks_return_with_control(&lua, values, &original).unwrap();
        assert_eq!(elements.len(), 1); // Falls back to original
        assert_eq!(control, TraversalControl::Continue);
    }

    // =========================================================================
    // TraversalControl tests
    // =========================================================================

    #[test]
    fn test_traversal_control_debug() {
        assert_eq!(format!("{:?}", TraversalControl::Continue), "Continue");
        assert_eq!(format!("{:?}", TraversalControl::Stop), "Stop");
    }

    #[test]
    fn test_traversal_control_clone() {
        let ctrl = TraversalControl::Continue;
        let cloned = ctrl;
        assert_eq!(ctrl, cloned);
    }

    #[test]
    fn test_traversal_control_copy() {
        let ctrl = TraversalControl::Stop;
        let copied: TraversalControl = ctrl;
        assert_eq!(copied, TraversalControl::Stop);
    }
}
