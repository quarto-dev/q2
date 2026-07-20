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

use mlua::{Error, Function, Lua, MultiValue, Result, Table, Value};
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
use super::types::{LuaBlock, LuaInline, peek_blocks_fuzzy, peek_inlines_fuzzy};

// ============================================================================
// TRAVERSAL CONTROL FOR TOPDOWN MODE
// ============================================================================

/// Control signal for topdown traversal - determines whether to descend into children
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TraversalControl {
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

    // Apply the filter using the appropriate traversal. Doc-level order
    // matches pandoc's applyFully (pandoc-lua-marshal Pandoc.hs):
    //   typewise: element walk -> Meta -> Pandoc
    //   topdown:  Pandoc -> Meta -> element walk
    // (`Pandoc`/`Doc` handler invocation is bd-a9g50za2 Phase 4.)
    let meta_new_source = quarto_source_map::SourceInfo::generated(quarto_source_map::By::filter(
        filter_path.to_string_lossy().to_string(),
        0,
    ));
    let (filtered_blocks, filtered_meta) = match walking_order {
        WalkingOrder::Typewise => {
            let blocks = apply_typewise_filter(&lua, &filter_table, &pandoc.blocks).await?;
            let meta =
                apply_meta_function(&lua, &filter_table, &pandoc.meta, &meta_new_source).await?;
            (blocks, meta)
        }
        WalkingOrder::Topdown => {
            let meta =
                apply_meta_function(&lua, &filter_table, &pandoc.meta, &meta_new_source).await?;
            let blocks = apply_topdown_filter(&lua, &filter_table, &pandoc.blocks).await?;
            (blocks, meta)
        }
    };

    // Extract diagnostics, HTML dependencies, and text includes from Lua state
    let mut diagnostics = super::diagnostics::extract_lua_diagnostics(&lua)?;
    diagnostics.extend(unimplemented_doc_handler_warnings(&lua, filter_path)?);
    let html_dependencies = super::quarto_doc::extract_html_dependencies(&lua)?;
    let text_includes = super::quarto_doc::extract_text_includes(&lua)?;
    let resources = super::quarto_doc::extract_resources(&lua)?;

    // Return filtered document with all extracted data
    let filtered_pandoc = Pandoc {
        meta: filtered_meta.unwrap_or_else(|| pandoc.meta.clone()),
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
        "Meta",
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

/// Invoke the `Meta` doc-level handler, if the filter defines one.
///
/// Pandoc semantics (applyMetaFunction / applyStraight): the handler
/// receives the materialized meta (native shapes, "Meta"-named table);
/// `nil` return keeps the document's meta unchanged; a table return is
/// peeked back as a map, reconciled against the original so untouched
/// entries keep their provenance. Returns `None` when the handler is
/// absent or returned nil. `new_source` attributes changed/new nodes
/// (the filter pass uses `By::filter(path, 0)`; `doc:walk` uses the
/// Lua-stack `filter_source_info`).
pub(crate) async fn apply_meta_function(
    lua: &Lua,
    filter_table: &Table,
    meta: &quarto_pandoc_types::ConfigValue,
    new_source: &quarto_source_map::SourceInfo,
) -> Result<Option<quarto_pandoc_types::ConfigValue>> {
    let func = match filter_table.get::<Function>("Meta") {
        Ok(func) => func,
        Err(_) => return Ok(None),
    };
    let meta_value = super::config_value::push_meta(lua, meta)?;
    let result: Value = func.call_async(meta_value).await?;
    match result {
        Value::Nil => Ok(None),
        Value::Table(table) => Ok(Some(super::config_value::peek_meta(
            lua,
            &table,
            Some(meta),
            new_source,
        )?)),
        other => Err(filter_return_error(
            "Meta",
            lua_facing_type_name(&other),
            Error::runtime("table or nil expected"),
        )),
    }
}

/// Q-11-6: the Pandoc/Doc doc-level filter functions are collected
/// but not yet invoked (bd-a9g50za2 Phase 4). Until then, a filter that
/// defines one gets a loud warning instead of a silent no-op. (`Meta`
/// handlers ARE invoked — see `apply_meta_function`.)
fn unimplemented_doc_handler_warnings(
    lua: &Lua,
    filter_path: &Path,
) -> Result<Vec<DiagnosticMessage>> {
    let globals = lua.globals();
    let mut warnings = Vec::new();
    for name in ["Pandoc", "Doc"] {
        if globals.get::<Function>(name).is_ok() {
            warnings.push(
                quarto_error_reporting::DiagnosticMessageBuilder::warning(format!(
                    "Unimplemented: Lua filter '{}' defines a '{name}' handler, \
                     which q2 does not invoke yet",
                    filter_path.display()
                ))
                .with_code("Q-11-6")
                .problem(
                    "Whole-document filter functions (Pandoc, Doc) are \
                     not yet supported; the handler is ignored",
                )
                .add_hint("Element-level handlers (Str, Para, ...) and Meta run normally")
                .build(),
            );
        }
    }
    Ok(warnings)
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

use super::types::lua_facing_type_name;

/// Wrap a fuzzy-peeker failure on a filter's return value in an error
/// naming the filter function and the Lua type it returned.
///
/// Pandoc errors on non-coercible filter returns (e.g. `return 5` →
/// "Inline, list of Inlines, or string expected, got number"); we match,
/// with the filter function named for actionability and the Q-11-4
/// code of the marshaling error contract (bd-9p2686pc). The inner
/// peeker error already carries its own Q-11-3 "expected, got" detail;
/// strip that code so the user sees one code, one message.
fn filter_return_error(fn_name: &str, got: &'static str, inner: Error) -> Error {
    let detail = match &inner {
        Error::RuntimeError(msg) => msg.clone(),
        other => other.to_string(),
    };
    let detail = detail.strip_prefix("Q-11-3: ").unwrap_or(&detail);
    // A contract-shaped inner error already ends in "…expected, got X";
    // only append the got-type when the detail doesn't state it.
    if detail.contains("expected, got") {
        Error::runtime(format!(
            "Q-11-4: invalid value returned from filter function '{fn_name}': {detail}"
        ))
    } else {
        Error::runtime(format!(
            "Q-11-4: invalid value returned from filter function '{fn_name}': {detail}, got {got}"
        ))
    }
}

/// Handle return value from an inline filter.
///
/// Matches pandoc's semantics: nil keeps the original, everything else
/// is coerced through `peek_inlines_fuzzy` (bare string → word-split;
/// table → element-wise coercion, empty table deletes; single userdata
/// → singleton; non-coercible values are loud errors).
pub(crate) fn handle_inline_return(
    lua: &Lua,
    ret: Value,
    original: &Inline,
    fn_name: &str,
) -> Result<Vec<Inline>> {
    match ret {
        Value::Nil => Ok(vec![original.clone()]),
        other => {
            let got = lua_facing_type_name(&other);
            peek_inlines_fuzzy(lua, other).map_err(|e| filter_return_error(fn_name, got, e))
        }
    }
}

/// Handle return value from a block filter.
///
/// Matches pandoc's semantics: nil keeps the original, everything else
/// is coerced through `peek_blocks_fuzzy` (bare string / Inline →
/// `Plain`-wrapped; table → element-wise coercion, empty table deletes;
/// non-coercible values are loud errors).
pub(crate) fn handle_block_return(
    lua: &Lua,
    ret: Value,
    original: &Block,
    fn_name: &str,
) -> Result<Vec<Block>> {
    match ret {
        Value::Nil => Ok(vec![original.clone()]),
        other => {
            let got = lua_facing_type_name(&other);
            peek_blocks_fuzzy(lua, other).map_err(|e| filter_return_error(fn_name, got, e))
        }
    }
}

/// Handle return value from an Inlines list filter (nil keeps the
/// original list; anything else is coerced like an inline-filter return).
pub(crate) fn handle_inlines_return(
    lua: &Lua,
    ret: Value,
    original: &[Inline],
    fn_name: &str,
) -> Result<Vec<Inline>> {
    match ret {
        Value::Nil => Ok(original.to_vec()),
        other => {
            let got = lua_facing_type_name(&other);
            peek_inlines_fuzzy(lua, other).map_err(|e| filter_return_error(fn_name, got, e))
        }
    }
}

/// Handle return value from a Blocks list filter (nil keeps the
/// original list; anything else is coerced like a block-filter return).
pub(crate) fn handle_blocks_return(
    lua: &Lua,
    ret: Value,
    original: &[Block],
    fn_name: &str,
) -> Result<Vec<Block>> {
    match ret {
        Value::Nil => Ok(original.to_vec()),
        other => {
            let got = lua_facing_type_name(&other);
            peek_blocks_fuzzy(lua, other).map_err(|e| filter_return_error(fn_name, got, e))
        }
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
pub(crate) fn handle_inline_return_with_control(
    lua: &Lua,
    ret: MultiValue,
    original: &Inline,
    fn_name: &str,
) -> Result<(Vec<Inline>, TraversalControl)> {
    let mut iter = ret.into_iter();
    let first = iter.next().unwrap_or(Value::Nil);
    let elements = handle_inline_return(lua, first, original, fn_name)?;

    // Second return value: traversal control (nil/missing = Continue, false = Stop)
    let control = match iter.next() {
        Some(Value::Boolean(false)) => TraversalControl::Stop,
        _ => TraversalControl::Continue,
    };

    Ok((elements, control))
}

/// Handle return value from a block filter with traversal control.
/// Returns (elements, control) where control indicates whether to descend into children.
pub(crate) fn handle_block_return_with_control(
    lua: &Lua,
    ret: MultiValue,
    original: &Block,
    fn_name: &str,
) -> Result<(Vec<Block>, TraversalControl)> {
    let mut iter = ret.into_iter();
    let first = iter.next().unwrap_or(Value::Nil);
    let elements = handle_block_return(lua, first, original, fn_name)?;

    // Second return value: traversal control (nil/missing = Continue, false = Stop)
    let control = match iter.next() {
        Some(Value::Boolean(false)) => TraversalControl::Stop,
        _ => TraversalControl::Continue,
    };

    Ok((elements, control))
}

/// Handle return value from a Blocks list filter with traversal control.
pub(crate) fn handle_blocks_return_with_control(
    lua: &Lua,
    ret: MultiValue,
    original: &[Block],
    fn_name: &str,
) -> Result<(Vec<Block>, TraversalControl)> {
    let mut iter = ret.into_iter();
    let first = iter.next().unwrap_or(Value::Nil);
    let blocks = handle_blocks_return(lua, first, original, fn_name)?;

    // Second return value: traversal control
    let control = match iter.next() {
        Some(Value::Boolean(false)) => TraversalControl::Stop,
        _ => TraversalControl::Continue,
    };

    Ok((blocks, control))
}

/// Handle return value from an Inlines list filter with traversal control.
pub(crate) fn handle_inlines_return_with_control(
    lua: &Lua,
    ret: MultiValue,
    original: &[Inline],
    fn_name: &str,
) -> Result<(Vec<Inline>, TraversalControl)> {
    let mut iter = ret.into_iter();
    let first = iter.next().unwrap_or(Value::Nil);
    let inlines = handle_inlines_return(lua, first, original, fn_name)?;

    // Second return value: traversal control
    let control = match iter.next() {
        Some(Value::Boolean(false)) => TraversalControl::Stop,
        _ => TraversalControl::Continue,
    };

    Ok((inlines, control))
}

/// Get the tag name for a block
pub(crate) fn block_tag(block: &Block) -> &'static str {
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
pub(crate) fn inline_tag(inline: &Inline) -> &'static str {
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
    super::walk::topdown_blocks(lua, filter_table, blocks).await
}

/// Apply typewise filter traversal (four separate passes) to a block
/// list. Delegates to the shared walk engine in `super::walk`.
pub async fn apply_typewise_filter(
    lua: &Lua,
    filter_table: &Table,
    blocks: &[Block],
) -> Result<Vec<Block>> {
    super::walk::typewise_blocks(lua, filter_table, blocks).await
}

/// Apply typewise filter traversal to an inline list. All four passes
/// run: block filter functions reach blocks nested inside Notes.
pub async fn apply_typewise_inlines(
    lua: &Lua,
    filter_table: &Table,
    inlines: &[Inline],
) -> Result<Vec<Inline>> {
    super::walk::typewise_inlines(lua, filter_table, inlines).await
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
        let result = handle_inline_return(&lua, Value::Nil, &original, "Str").unwrap();
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
        let result =
            handle_inline_return(&lua, Value::Table(empty_table), &original, "Str").unwrap();
        assert_eq!(result.len(), 0); // Empty table means delete
    }

    #[test]
    fn test_handle_inline_return_number_errors() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        // Pandoc errors on non-coercible returns; so do we, loudly.
        let err = handle_inline_return(&lua, Value::Integer(42), &original, "Str").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("number"), "should name the got-type: {msg}");
        assert!(
            msg.contains("'Str'"),
            "should name the filter function: {msg}"
        );
    }

    #[test]
    fn test_handle_inline_return_boolean_errors() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        let err = handle_inline_return(&lua, Value::Boolean(true), &original, "Str").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("boolean"), "should name the got-type: {msg}");
        assert!(
            msg.contains("'Str'"),
            "should name the filter function: {msg}"
        );
    }

    #[test]
    fn test_handle_inline_return_string_wordsplit() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        // Bare string return is word-split like peekInlinesFuzzy.
        let ret = Value::String(lua.create_string("two words").unwrap());
        let result = handle_inline_return(&lua, ret, &original, "Str").unwrap();
        assert_eq!(result.len(), 3);
        assert!(matches!(&result[0], Inline::Str(s) if s.text == "two"));
        assert!(matches!(&result[1], Inline::Space(_)));
        assert!(matches!(&result[2], Inline::Str(s) if s.text == "words"));
    }

    #[test]
    fn test_handle_inline_return_table_string_entries() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        // Strings inside a returned table become single Strs (element-wise
        // peekInlineFuzzy: NO word-split inside lists).
        let table = lua.create_table().unwrap();
        table.push("a b").unwrap();
        table.push("c").unwrap();
        let result = handle_inline_return(&lua, Value::Table(table), &original, "Str").unwrap();
        assert_eq!(result.len(), 2);
        assert!(matches!(&result[0], Inline::Str(s) if s.text == "a b"));
        assert!(matches!(&result[1], Inline::Str(s) if s.text == "c"));
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
        let result = handle_block_return(&lua, Value::Nil, &original, "Para").unwrap();
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
        let result =
            handle_block_return(&lua, Value::Table(empty_table), &original, "Para").unwrap();
        assert_eq!(result.len(), 0); // Empty table means delete
    }

    #[test]
    fn test_handle_block_return_number_errors() {
        let lua = Lua::new();
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::SourceInfo;

        let original = Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        });
        // Pandoc errors on non-coercible returns; so do we, loudly.
        let err = handle_block_return(&lua, Value::Integer(42), &original, "Para").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("number"), "should name the got-type: {msg}");
        assert!(
            msg.contains("'Para'"),
            "should name the filter function: {msg}"
        );
    }

    #[test]
    fn test_handle_block_return_string_plain() {
        let lua = Lua::new();
        use crate::pandoc::block::Plain;
        use crate::pandoc::{Block, Inline};
        use quarto_source_map::SourceInfo;

        let original = Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        });
        // Bare string return from a Block filter → Plain(word-split).
        let ret = Value::String(lua.create_string("plain text").unwrap());
        let result = handle_block_return(&lua, ret, &original, "Para").unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Block::Plain(p) => {
                assert_eq!(p.content.len(), 3);
                assert!(matches!(&p.content[0], Inline::Str(s) if s.text == "plain"));
                assert!(matches!(&p.content[1], Inline::Space(_)));
                assert!(matches!(&p.content[2], Inline::Str(s) if s.text == "text"));
            }
            other => panic!("Expected Plain, got {other:?}"),
        }
    }

    #[test]
    fn test_handle_block_return_table_string_entry() {
        let lua = Lua::new();
        use crate::pandoc::block::Plain;
        use crate::pandoc::{Block, Inline};
        use quarto_source_map::SourceInfo;

        let original = Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        });
        // A string entry in a Block-filter table → Plain(word-split),
        // element-wise peekBlockFuzzy.
        let table = lua.create_table().unwrap();
        table.push("two words").unwrap();
        let result = handle_block_return(&lua, Value::Table(table), &original, "Para").unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Block::Plain(p) => {
                assert_eq!(p.content.len(), 3);
                assert!(matches!(&p.content[0], Inline::Str(s) if s.text == "two"));
            }
            other => panic!("Expected Plain, got {other:?}"),
        }
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
            handle_inline_return_with_control(&lua, MultiValue::new(), &original, "Str").unwrap();
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
            handle_inline_return_with_control(&lua, values, &original, "Str").unwrap();
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
            handle_inline_return_with_control(&lua, values, &original, "Str").unwrap();
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
            handle_block_return_with_control(&lua, MultiValue::new(), &original, "Para").unwrap();
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
            handle_block_return_with_control(&lua, values, &original, "Para").unwrap();
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
            handle_blocks_return_with_control(&lua, MultiValue::new(), &original, "Blocks")
                .unwrap();
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
            handle_blocks_return_with_control(&lua, values, &original, "Blocks").unwrap();
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
            handle_inlines_return_with_control(&lua, MultiValue::new(), &original, "Inlines")
                .unwrap();
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
            handle_inlines_return_with_control(&lua, values, &original, "Inlines").unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(control, TraversalControl::Stop);
    }

    #[test]
    fn test_handle_inlines_return_with_control_number_errors() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = vec![Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        })];
        let mut values = MultiValue::new();
        values.push_front(Value::Integer(42)); // Not coercible to Inlines
        let err =
            handle_inlines_return_with_control(&lua, values, &original, "Inlines").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("number"), "should name the got-type: {msg}");
    }

    #[test]
    fn test_handle_blocks_return_with_control_number_errors() {
        let lua = Lua::new();
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::SourceInfo;

        let original = vec![Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        })];
        let mut values = MultiValue::new();
        values.push_front(Value::Integer(42)); // Not coercible to Blocks
        let err = handle_blocks_return_with_control(&lua, values, &original, "Blocks").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("number"), "should name the got-type: {msg}");
        assert!(
            msg.contains("'Blocks'"),
            "should name the filter function: {msg}"
        );
    }

    #[test]
    fn test_handle_inline_return_with_control_string_wordsplit() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        });
        let mut values = MultiValue::new();
        values.push_front(Value::String(lua.create_string("x y").unwrap()));
        values.push_back(Value::Boolean(false));
        let (elements, control) =
            handle_inline_return_with_control(&lua, values, &original, "Str").unwrap();
        assert_eq!(elements.len(), 3); // word-split: Str, Space, Str
        assert!(matches!(&elements[0], Inline::Str(s) if s.text == "x"));
        assert_eq!(control, TraversalControl::Stop);
    }

    #[test]
    fn test_handle_inlines_return_with_control_string_wordsplit() {
        let lua = Lua::new();
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::SourceInfo;

        let original = vec![Inline::Str(Str {
            text: "original".to_string(),
            source_info: SourceInfo::for_test(),
        })];
        let mut values = MultiValue::new();
        values.push_front(Value::String(lua.create_string("x y").unwrap()));
        let (elements, control) =
            handle_inlines_return_with_control(&lua, values, &original, "Inlines").unwrap();
        assert_eq!(elements.len(), 3);
        assert!(matches!(&elements[0], Inline::Str(s) if s.text == "x"));
        assert!(matches!(&elements[1], Inline::Space(_)));
        assert!(matches!(&elements[2], Inline::Str(s) if s.text == "y"));
        assert_eq!(control, TraversalControl::Continue);
    }

    #[test]
    fn test_handle_blocks_return_with_control_string_plain() {
        let lua = Lua::new();
        use crate::pandoc::block::Plain;
        use crate::pandoc::{Block, Inline};
        use quarto_source_map::SourceInfo;

        let original = vec![Block::Plain(Plain {
            content: vec![],
            source_info: SourceInfo::for_test(),
        })];
        let mut values = MultiValue::new();
        values.push_front(Value::String(lua.create_string("block text").unwrap()));
        let (elements, control) =
            handle_blocks_return_with_control(&lua, values, &original, "Blocks").unwrap();
        assert_eq!(elements.len(), 1);
        match &elements[0] {
            Block::Plain(p) => {
                assert_eq!(p.content.len(), 3);
                assert!(matches!(&p.content[0], Inline::Str(s) if s.text == "block"));
            }
            other => panic!("Expected Plain, got {other:?}"),
        }
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
