/*
 * shortcode_resolve.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that resolves shortcodes in the AST.
 */

//! Shortcode resolution transform.
//!
//! This transform processes shortcodes (`{{< name args... >}}`) in the document AST,
//! replacing them with their resolved content.
//!
//! ## Built-in Shortcodes
//!
//! Currently supported:
//! - `meta` - Insert metadata values from document frontmatter
//!
//! ## Error Handling
//!
//! When a shortcode fails to resolve (e.g., missing metadata key), the transform:
//! 1. Emits a diagnostic warning with source location for IDE integration
//! 2. Renders visible error content (e.g., `?meta:keyname`) matching TS Quarto behavior
//!
//! ## Pipeline Order
//!
//! This transform should run early in the pipeline, after `CalloutResolveTransform`
//! and before `MetadataNormalizeTransform`, so that:
//! - Shortcodes in callout content are resolved after callouts are processed
//! - Metadata normalization sees resolved content, not shortcode placeholders

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::block::{
    Block, BlockQuote, BulletList, DefinitionList, Div, Figure, Header, LineBlock, OrderedList,
    Paragraph, Plain,
};
use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};
use quarto_pandoc_types::inline::{
    Cite, Code, Delete, EditComment, Emph, Highlight, Image, Inline, Insert, Link, Note, Quoted,
    SmallCaps, Span, Str, Strikeout, Strong, Subscript, Superscript, Underline,
};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::shortcode::{Shortcode, ShortcodeArg};
use quarto_pandoc_types::table::Table;
use quarto_source_map::SourceInfo;

use std::path::PathBuf;
use std::sync::Arc;

use quarto_analysis::AnalysisContext;

use crate::Result;
use crate::extension::discover::find_extension;
use crate::extension::types::Extension;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use quarto_system_runtime::SystemRuntime;

/// Error information for shortcode resolution failures.
pub struct ShortcodeError {
    /// Error key for visible output (e.g., "meta:title")
    pub key: String,
    /// Full diagnostic message with source location
    pub diagnostic: DiagnosticMessage,
}

/// Result of resolving a shortcode.
pub enum ShortcodeResult {
    /// Resolved to inline content
    Inlines(Vec<Inline>),
    /// Resolved to block content (for block-context shortcodes)
    Blocks(Vec<Block>),
    /// Error - renders visible content AND emits diagnostic
    Error(ShortcodeError),
    /// Shortcode should be preserved as literal text (e.g., escaped shortcodes)
    Preserve,
}

/// Context passed to shortcode handlers.
pub struct ShortcodeContext<'a> {
    /// Document metadata
    pub metadata: &'a ConfigValue,
    /// Source info for the shortcode (for error reporting)
    pub source_info: &'a SourceInfo,
}

/// Whether the shortcode is being resolved in block or inline context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionContext {
    /// Shortcode is the sole content of a Para/Plain — may return Blocks
    Block,
    /// Shortcode is inline among other content — must return Inlines
    Inline,
}

/// Trait for shortcode handlers.
///
/// Each built-in shortcode (meta, var, env, etc.) implements this trait.
pub trait ShortcodeHandler: Send + Sync {
    /// The shortcode name (e.g., "meta", "var", "env")
    fn name(&self) -> &str;

    /// Resolve the shortcode to content.
    fn resolve(
        &self,
        shortcode: &Shortcode,
        ctx: &ShortcodeContext,
        resolution_ctx: ResolutionContext,
    ) -> ShortcodeResult;
}

/// Handler for the `meta` shortcode.
///
/// Usage: `{{< meta key >}}` or `{{< meta key.subkey >}}`
///
/// Reads values from document metadata (YAML frontmatter) and inserts
/// them as inline content. Supports dot notation for nested values.
pub struct MetaShortcodeHandler;

impl ShortcodeHandler for MetaShortcodeHandler {
    fn name(&self) -> &str {
        "meta"
    }

    fn resolve(
        &self,
        shortcode: &Shortcode,
        ctx: &ShortcodeContext,
        _resolution_ctx: ResolutionContext,
    ) -> ShortcodeResult {
        // Get the key from positional args
        let key = match shortcode.positional_args.first() {
            Some(ShortcodeArg::String(s)) => s.clone(),
            _ => {
                let diagnostic = DiagnosticMessageBuilder::warning("Missing shortcode argument")
                    .problem("The `meta` shortcode requires a metadata key")
                    .add_hint("Use `{{< meta key >}}` where `key` is a metadata field name")
                    .with_location(ctx.source_info.clone())
                    .build();
                return ShortcodeResult::Error(ShortcodeError {
                    key: "meta".to_string(),
                    diagnostic,
                });
            }
        };

        // Look up value in metadata (supports dot notation via ConfigValue::get_nested)
        match ctx.metadata.get_nested(&key) {
            Some(value) => ShortcodeResult::Inlines(config_value_to_inlines(value)),
            None => {
                let diagnostic = DiagnosticMessageBuilder::warning("Unknown metadata key")
                    .problem(format!("Metadata key `{}` not found in document", key))
                    .add_hint("Check that the key exists in your YAML frontmatter")
                    .with_location(ctx.source_info.clone())
                    .build();
                ShortcodeResult::Error(ShortcodeError {
                    key: format!("meta:{}", key),
                    diagnostic,
                })
            }
        }
    }
}

/// Convert a ConfigValue to inline content.
fn config_value_to_inlines(value: &ConfigValue) -> Vec<Inline> {
    // Use helper methods on ConfigValue for scalar types
    if let Some(s) = value.as_str() {
        return vec![Inline::Str(Str {
            text: s.to_string(),
            source_info: SourceInfo::default(),
        })];
    }

    if let Some(b) = value.as_bool() {
        return vec![Inline::Str(Str {
            text: b.to_string(),
            source_info: SourceInfo::default(),
        })];
    }

    if let Some(n) = value.as_int() {
        return vec![Inline::Str(Str {
            text: n.to_string(),
            source_info: SourceInfo::default(),
        })];
    }

    // Handle specific ConfigValueKind variants
    match &value.value {
        ConfigValueKind::PandocInlines(inlines) => inlines.clone(),
        ConfigValueKind::PandocBlocks(blocks) => {
            // For blocks in inline context, flatten to plain text
            // This matches TS Quarto behavior
            flatten_blocks_to_inlines(blocks)
        }
        // Scalar that wasn't captured by helpers (e.g., float, null)
        ConfigValueKind::Scalar(_) => {
            if let Some(plain) = value.as_plain_text() {
                vec![Inline::Str(Str {
                    text: plain,
                    source_info: SourceInfo::default(),
                })]
            } else {
                vec![Inline::Str(Str {
                    text: String::new(),
                    source_info: SourceInfo::default(),
                })]
            }
        }
        // Arrays and maps - not suitable for inline context
        ConfigValueKind::Array(_) | ConfigValueKind::Map(_) => vec![Inline::Str(Str {
            text: "?invalid meta type".to_string(),
            source_info: SourceInfo::default(),
        })],
        // Path, Glob, Expr were handled by as_str() above
        ConfigValueKind::Path(_) | ConfigValueKind::Glob(_) | ConfigValueKind::Expr(_) => {
            // This shouldn't be reached since as_str() handles these
            vec![Inline::Str(Str {
                text: "?invalid meta type".to_string(),
                source_info: SourceInfo::default(),
            })]
        }
    }
}

/// Flatten blocks to inlines (extracts text content).
fn flatten_blocks_to_inlines(blocks: &[Block]) -> Vec<Inline> {
    let mut result = Vec::new();
    for block in blocks {
        match block {
            Block::Plain(plain) => result.extend(plain.content.clone()),
            Block::Paragraph(para) => {
                if !result.is_empty() {
                    // Add space between paragraphs
                    result.push(Inline::Space(quarto_pandoc_types::inline::Space {
                        source_info: SourceInfo::default(),
                    }));
                }
                result.extend(para.content.clone());
            }
            // For other block types, recursively extract inlines
            _ => {
                // Skip complex blocks - they don't make sense in inline context
            }
        }
    }
    result
}

/// Transform that resolves shortcodes in the AST.
///
/// Supports both built-in Rust handlers and Lua shortcode scripts loaded from
/// extensions or user-specified paths.
///
/// The `LuaShortcodeEngine` (which holds a `!Send + !Sync` Lua state) is created
/// inside `transform()` as a stack-local variable, never stored as a field.
pub struct ShortcodeResolveTransform {
    handlers: Vec<Box<dyn ShortcodeHandler>>,
    /// Shortcode script paths from merged metadata (absolute)
    lua_shortcode_paths: Vec<PathBuf>,
    /// Extensions for name-based shortcode lookup
    extensions: Vec<Extension>,
    /// System runtime for reading Lua files
    runtime: Option<Arc<dyn SystemRuntime>>,
    /// Target format string (e.g., "html") for Lua FORMAT global
    target_format: String,
}

impl ShortcodeResolveTransform {
    /// Create a new shortcode resolve transform with default handlers only.
    /// Used in tests that don't need Lua support.
    pub fn new() -> Self {
        Self {
            handlers: vec![Box::new(MetaShortcodeHandler)],
            lua_shortcode_paths: Vec::new(),
            extensions: Vec::new(),
            runtime: None,
            target_format: String::new(),
        }
    }

    /// Create a shortcode resolve transform with Lua support.
    ///
    /// The `LuaShortcodeEngine` is NOT created here (it's `!Send + !Sync`).
    /// It is created on the stack inside `transform()`.
    pub fn with_lua_support(
        lua_shortcode_paths: Vec<PathBuf>,
        extensions: Vec<Extension>,
        runtime: Arc<dyn SystemRuntime>,
        target_format: String,
    ) -> Self {
        Self {
            handlers: vec![Box::new(MetaShortcodeHandler)],
            lua_shortcode_paths,
            extensions,
            runtime: Some(runtime),
            target_format,
        }
    }

    /// Resolve a shortcode using the appropriate handler.
    ///
    /// Priority: built-in Rust handlers > loaded Lua handlers > extension name lookup.
    fn resolve_shortcode(
        &self,
        shortcode: &Shortcode,
        ctx: &ShortcodeContext,
        resolution_ctx: ResolutionContext,
        lua_engine: &mut Option<pampa::lua::LuaShortcodeEngine>,
    ) -> ShortcodeResult {
        // Handle escaped shortcodes - preserve as literal text
        if shortcode.is_escaped {
            return ShortcodeResult::Preserve;
        }

        // 1. Try built-in Rust handlers first
        for handler in &self.handlers {
            if handler.name() == shortcode.name {
                return handler.resolve(shortcode, ctx, resolution_ctx);
            }
        }

        // 2. Try Lua engine (loaded handlers)
        if let Some(engine) = lua_engine.as_mut() {
            // If handler is already loaded, call it
            if engine.has_handler(&shortcode.name) {
                return dispatch_lua_shortcode(engine, shortcode, ctx, resolution_ctx);
            }

            // 3. Try name-based extension lookup (on-demand loading)
            if let Some(ext) = find_extension(&shortcode.name, &self.extensions) {
                if !ext.contributes.shortcodes.is_empty() {
                    for script_path in &ext.contributes.shortcodes {
                        if let Err(e) = engine.load_script(script_path) {
                            let diagnostic =
                                DiagnosticMessageBuilder::warning("Shortcode script error")
                                    .problem(format!(
                                        "Failed to load shortcode script `{}`: {}",
                                        script_path.display(),
                                        e
                                    ))
                                    .with_location(ctx.source_info.clone())
                                    .build();
                            return ShortcodeResult::Error(ShortcodeError {
                                key: shortcode.name.clone(),
                                diagnostic,
                            });
                        }
                    }
                    // Retry after loading extension scripts
                    if engine.has_handler(&shortcode.name) {
                        return dispatch_lua_shortcode(engine, shortcode, ctx, resolution_ctx);
                    }
                }
            }
        }

        // Unknown shortcode - create error with diagnostic
        let diagnostic = DiagnosticMessageBuilder::warning("Unknown shortcode")
            .problem(format!("Shortcode `{}` is not recognized", shortcode.name))
            .add_hint("Check the shortcode name for typos")
            .with_location(ctx.source_info.clone())
            .build();
        ShortcodeResult::Error(ShortcodeError {
            key: shortcode.name.clone(),
            diagnostic,
        })
    }
}

impl Default for ShortcodeResolveTransform {
    fn default() -> Self {
        Self::new()
    }
}

/// Dispatch a shortcode call to the Lua engine and convert the result.
fn dispatch_lua_shortcode(
    engine: &mut pampa::lua::LuaShortcodeEngine,
    shortcode: &Shortcode,
    ctx: &ShortcodeContext,
    resolution_ctx: ResolutionContext,
) -> ShortcodeResult {
    let args = shortcode_to_lua_args(shortcode, ctx.metadata);
    let call_ctx = match resolution_ctx {
        ResolutionContext::Block => pampa::lua::ShortcodeCallContext::Block,
        ResolutionContext::Inline => pampa::lua::ShortcodeCallContext::Inline,
    };
    match engine.call(&shortcode.name, &args, call_ctx) {
        Some(result) => lua_result_to_shortcode_result(result, ctx.source_info),
        None => {
            let diagnostic = DiagnosticMessageBuilder::warning("Shortcode handler not found")
                .problem(format!(
                    "Lua handler for shortcode `{}` was not found",
                    shortcode.name
                ))
                .with_location(ctx.source_info.clone())
                .build();
            ShortcodeResult::Error(ShortcodeError {
                key: shortcode.name.clone(),
                diagnostic,
            })
        }
    }
}

/// Convert a q2 `Shortcode` to pampa's `ShortcodeArgs` for Lua dispatch.
fn shortcode_to_lua_args(
    shortcode: &Shortcode,
    metadata: &ConfigValue,
) -> pampa::lua::ShortcodeArgs {
    let positional: Vec<String> = shortcode
        .positional_args
        .iter()
        .filter_map(|arg| match arg {
            ShortcodeArg::String(s) => Some(s.clone()),
            ShortcodeArg::Number(n) => Some(n.to_string()),
            ShortcodeArg::Boolean(b) => Some(b.to_string()),
            _ => None,
        })
        .collect();

    let keyword: Vec<(String, String)> = shortcode
        .keyword_args
        .iter()
        .filter_map(|(key, value)| match value {
            ShortcodeArg::String(s) => Some((key.clone(), s.clone())),
            ShortcodeArg::Number(n) => Some((key.clone(), n.to_string())),
            ShortcodeArg::Boolean(b) => Some((key.clone(), b.to_string())),
            _ => None,
        })
        .collect();

    // Extract top-level metadata as string key-value pairs for Lua
    let meta_entries: Vec<(String, String)> = if let Some(entries) = metadata.as_map_entries() {
        entries
            .iter()
            .filter_map(|entry| {
                entry
                    .value
                    .as_str()
                    .map(|v| (entry.key.clone(), v.to_string()))
            })
            .collect()
    } else {
        Vec::new()
    };

    pampa::lua::ShortcodeArgs {
        positional,
        keyword,
        metadata: meta_entries,
    }
}

/// Convert a pampa `LuaShortcodeResult` back to a q2 `ShortcodeResult`.
fn lua_result_to_shortcode_result(
    result: pampa::lua::LuaShortcodeResult,
    source_info: &SourceInfo,
) -> ShortcodeResult {
    match result {
        pampa::lua::LuaShortcodeResult::Inlines(inlines) => ShortcodeResult::Inlines(inlines),
        pampa::lua::LuaShortcodeResult::Blocks(blocks) => ShortcodeResult::Blocks(blocks),
        pampa::lua::LuaShortcodeResult::Text(text) => {
            ShortcodeResult::Inlines(vec![Inline::Str(Str {
                text,
                source_info: SourceInfo::default(),
            })])
        }
        pampa::lua::LuaShortcodeResult::Error(msg) => {
            let diagnostic = DiagnosticMessageBuilder::warning("Shortcode error")
                .problem(msg)
                .with_location(source_info.clone())
                .build();
            ShortcodeResult::Error(ShortcodeError {
                key: "lua-shortcode".to_string(),
                diagnostic,
            })
        }
    }
}

/// Extract shortcode paths from merged metadata.
///
/// After metadata merge, `meta["shortcodes"]` contains an array of paths
/// (either `ConfigValueKind::Path` from extensions or `Scalar` from user frontmatter).
pub fn extract_shortcode_paths(meta: &ConfigValue, document_dir: &std::path::Path) -> Vec<PathBuf> {
    let Some(sc_val) = meta.get("shortcodes") else {
        return vec![];
    };
    let Some(items) = sc_val.as_array() else {
        return vec![];
    };
    items
        .iter()
        .filter_map(|item| match &item.value {
            ConfigValueKind::Path(s) => Some(document_dir.join(s)),
            ConfigValueKind::Scalar(_) => item.as_str().map(|s| document_dir.join(s)),
            _ => None,
        })
        .collect()
}

impl AstTransform for ShortcodeResolveTransform {
    fn name(&self) -> &str {
        "shortcode-resolve"
    }

    fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Collect diagnostics during traversal
        let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();

        // Create Lua engine on the stack if we have paths, extensions, or a runtime.
        // The engine is !Send + !Sync so it cannot be stored as a field.
        let mut lua_engine = if (!self.lua_shortcode_paths.is_empty()
            || !self.extensions.is_empty())
            && self.runtime.is_some()
        {
            let runtime = self.runtime.as_ref().unwrap().clone();
            match pampa::lua::LuaShortcodeEngine::new(&self.target_format, runtime) {
                Ok(mut engine) => {
                    // Load scripts from metadata-specified paths
                    for path in &self.lua_shortcode_paths {
                        if let Err(e) = engine.load_script(path) {
                            diagnostics.push(
                                DiagnosticMessageBuilder::warning("Shortcode script error")
                                    .problem(format!(
                                        "Failed to load shortcode script `{}`: {}",
                                        path.display(),
                                        e
                                    ))
                                    .build(),
                            );
                        }
                    }
                    Some(engine)
                }
                Err(e) => {
                    diagnostics.push(
                        DiagnosticMessageBuilder::warning("Lua shortcode engine error")
                            .problem(format!("Failed to create Lua shortcode engine: {}", e))
                            .build(),
                    );
                    None
                }
            }
        } else {
            None
        };

        // Resolve shortcodes in all blocks
        resolve_blocks(
            &mut ast.blocks,
            self,
            &ast.meta,
            &mut diagnostics,
            &mut lua_engine,
        );

        // Add any diagnostics to the render context
        for diagnostic in diagnostics {
            ctx.add_diagnostic(diagnostic);
        }

        Ok(())
    }
}

/// Resolve shortcodes in a vector of blocks.
///
/// Uses index-based iteration because block-context shortcodes can splice
/// multiple blocks in place of a single Para/Plain.
fn resolve_blocks(
    blocks: &mut Vec<Block>,
    transform: &ShortcodeResolveTransform,
    metadata: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
    lua_engine: &mut Option<pampa::lua::LuaShortcodeEngine>,
) {
    let mut i = 0;
    while i < blocks.len() {
        // Check for block-context shortcode: Para/Plain with exactly one non-escaped Shortcode
        if let Some(shortcode) = single_shortcode_in_para_or_plain(&blocks[i]) {
            let shortcode_owned = shortcode.clone();
            let ctx = ShortcodeContext {
                metadata,
                source_info: &shortcode_owned.source_info,
            };
            match transform.resolve_shortcode(
                &shortcode_owned,
                &ctx,
                ResolutionContext::Block,
                lua_engine,
            ) {
                ShortcodeResult::Blocks(new_blocks) => {
                    let n = new_blocks.len();
                    blocks.splice(i..=i, new_blocks);
                    i += n.max(1);
                    continue;
                }
                ShortcodeResult::Inlines(inlines) => {
                    replace_shortcode_in_block(&mut blocks[i], inlines);
                    i += 1;
                    continue;
                }
                ShortcodeResult::Error(error) => {
                    diagnostics.push(error.diagnostic);
                    let error_inline = make_error_inline(&error.key);
                    replace_shortcode_in_block(&mut blocks[i], vec![error_inline]);
                    i += 1;
                    continue;
                }
                ShortcodeResult::Preserve => {
                    let literal = shortcode_to_literal(&shortcode_owned);
                    replace_shortcode_in_block(&mut blocks[i], vec![literal]);
                    i += 1;
                    continue;
                }
            }
        }
        // General case: recurse into block
        resolve_block(&mut blocks[i], transform, metadata, diagnostics, lua_engine);
        i += 1;
    }
}

/// Check if a block is a Para/Plain with exactly one non-escaped Shortcode inline.
fn single_shortcode_in_para_or_plain(block: &Block) -> Option<&Shortcode> {
    let content = match block {
        Block::Paragraph(Paragraph { content, .. }) | Block::Plain(Plain { content, .. }) => {
            content
        }
        _ => return None,
    };
    if content.len() != 1 {
        return None;
    }
    match &content[0] {
        Inline::Shortcode(sc) if !sc.is_escaped => Some(sc),
        _ => None,
    }
}

/// Replace the single Shortcode inline in a Para/Plain with the given inlines.
fn replace_shortcode_in_block(block: &mut Block, inlines: Vec<Inline>) {
    match block {
        Block::Paragraph(Paragraph { content, .. }) | Block::Plain(Plain { content, .. }) => {
            *content = inlines;
        }
        _ => {}
    }
}

/// Resolve shortcodes in a single block.
fn resolve_block(
    block: &mut Block,
    transform: &ShortcodeResolveTransform,
    metadata: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
    lua_engine: &mut Option<pampa::lua::LuaShortcodeEngine>,
) {
    match block {
        Block::Plain(Plain { content, .. }) | Block::Paragraph(Paragraph { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics, lua_engine);
        }
        Block::LineBlock(LineBlock { content, .. }) => {
            for line in content {
                resolve_inlines(line, transform, metadata, diagnostics, lua_engine);
            }
        }
        Block::Header(Header { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics, lua_engine);
        }
        Block::BlockQuote(BlockQuote { content, .. }) => {
            resolve_blocks(content, transform, metadata, diagnostics, lua_engine);
        }
        Block::OrderedList(OrderedList { content, .. }) => {
            for item in content {
                resolve_blocks(item, transform, metadata, diagnostics, lua_engine);
            }
        }
        Block::BulletList(BulletList { content, .. }) => {
            for item in content {
                resolve_blocks(item, transform, metadata, diagnostics, lua_engine);
            }
        }
        Block::DefinitionList(DefinitionList { content, .. }) => {
            for (term, defs) in content {
                resolve_inlines(term, transform, metadata, diagnostics, lua_engine);
                for def in defs {
                    resolve_blocks(def, transform, metadata, diagnostics, lua_engine);
                }
            }
        }
        Block::Figure(Figure {
            content, caption, ..
        }) => {
            resolve_blocks(content, transform, metadata, diagnostics, lua_engine);
            if let Some(short) = &mut caption.short {
                resolve_inlines(short, transform, metadata, diagnostics, lua_engine);
            }
            if let Some(long) = &mut caption.long {
                resolve_blocks(long, transform, metadata, diagnostics, lua_engine);
            }
        }
        Block::Div(Div { content, .. }) => {
            resolve_blocks(content, transform, metadata, diagnostics, lua_engine);
        }
        Block::Table(Table {
            caption,
            head,
            bodies,
            foot,
            ..
        }) => {
            // Table caption
            if let Some(short) = &mut caption.short {
                resolve_inlines(short, transform, metadata, diagnostics, lua_engine);
            }
            if let Some(long) = &mut caption.long {
                resolve_blocks(long, transform, metadata, diagnostics, lua_engine);
            }
            // Table head
            for row in &mut head.rows {
                for cell in &mut row.cells {
                    resolve_blocks(
                        &mut cell.content,
                        transform,
                        metadata,
                        diagnostics,
                        lua_engine,
                    );
                }
            }
            // Table bodies
            for body in bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        resolve_blocks(
                            &mut cell.content,
                            transform,
                            metadata,
                            diagnostics,
                            lua_engine,
                        );
                    }
                }
            }
            // Table foot
            for row in &mut foot.rows {
                for cell in &mut row.cells {
                    resolve_blocks(
                        &mut cell.content,
                        transform,
                        metadata,
                        diagnostics,
                        lua_engine,
                    );
                }
            }
        }
        Block::Custom(custom) => {
            // Resolve shortcodes in custom node slots
            for slot in custom.slots.values_mut() {
                match slot {
                    quarto_pandoc_types::custom::Slot::Block(b) => {
                        resolve_block(b, transform, metadata, diagnostics, lua_engine);
                    }
                    quarto_pandoc_types::custom::Slot::Blocks(bs) => {
                        resolve_blocks(bs, transform, metadata, diagnostics, lua_engine);
                    }
                    quarto_pandoc_types::custom::Slot::Inline(i) => {
                        let mut inlines = vec![i.as_ref().clone()];
                        resolve_inlines(&mut inlines, transform, metadata, diagnostics, lua_engine);
                        if inlines.len() == 1 {
                            **i = inlines.pop().unwrap();
                        }
                        // If resolution produced multiple inlines, we can't put them
                        // back into a single Inline slot - keep the original
                    }
                    quarto_pandoc_types::custom::Slot::Inlines(is) => {
                        resolve_inlines(is, transform, metadata, diagnostics, lua_engine);
                    }
                }
            }
        }
        // These blocks don't contain inlines that could have shortcodes
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_)
        | Block::CaptionBlock(_) => {}
    }
}

/// Resolve shortcodes in a vector of inlines.
fn resolve_inlines(
    inlines: &mut Vec<Inline>,
    transform: &ShortcodeResolveTransform,
    metadata: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
    lua_engine: &mut Option<pampa::lua::LuaShortcodeEngine>,
) {
    let mut i = 0;
    while i < inlines.len() {
        if let Inline::Shortcode(shortcode) = &inlines[i] {
            let shortcode_owned = shortcode.clone();
            let shortcode_ctx = ShortcodeContext {
                metadata,
                source_info: &shortcode_owned.source_info,
            };

            match transform.resolve_shortcode(
                &shortcode_owned,
                &shortcode_ctx,
                ResolutionContext::Inline,
                lua_engine,
            ) {
                ShortcodeResult::Inlines(replacement) => {
                    // Replace shortcode with resolved inlines
                    let replacement_len = replacement.len();
                    inlines.splice(i..=i, replacement);
                    // Advance past the replacement (they shouldn't contain shortcodes,
                    // but even if they do, we don't want infinite loops)
                    i += replacement_len.max(1);
                }
                ShortcodeResult::Blocks(blocks) => {
                    // Graceful degradation: flatten blocks to inlines
                    let replacement = flatten_blocks_to_inlines(&blocks);
                    let replacement_len = replacement.len();
                    inlines.splice(i..=i, replacement);
                    i += replacement_len.max(1);
                }
                ShortcodeResult::Error(error) => {
                    // Emit diagnostic
                    diagnostics.push(error.diagnostic);
                    // Replace with visible error (TS Quarto style)
                    let error_inline = make_error_inline(&error.key);
                    inlines[i] = error_inline;
                    i += 1;
                }
                ShortcodeResult::Preserve => {
                    // Convert escaped shortcode to literal text
                    let literal = shortcode_to_literal(&shortcode_owned);
                    inlines[i] = literal;
                    i += 1;
                }
            }
        } else {
            // Recurse into inline containers
            recurse_inline(
                &mut inlines[i],
                transform,
                metadata,
                diagnostics,
                lua_engine,
            );
            i += 1;
        }
    }
}

/// Recurse into an inline element to resolve nested shortcodes.
fn recurse_inline(
    inline: &mut Inline,
    transform: &ShortcodeResolveTransform,
    metadata: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
    lua_engine: &mut Option<pampa::lua::LuaShortcodeEngine>,
) {
    match inline {
        Inline::Emph(Emph { content, .. })
        | Inline::Underline(Underline { content, .. })
        | Inline::Strong(Strong { content, .. })
        | Inline::Strikeout(Strikeout { content, .. })
        | Inline::Superscript(Superscript { content, .. })
        | Inline::Subscript(Subscript { content, .. })
        | Inline::SmallCaps(SmallCaps { content, .. })
        | Inline::Insert(Insert { content, .. })
        | Inline::Delete(Delete { content, .. })
        | Inline::Highlight(Highlight { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics, lua_engine);
        }
        Inline::Quoted(Quoted { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics, lua_engine);
        }
        Inline::Cite(Cite { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics, lua_engine);
        }
        Inline::Link(Link { content, .. }) | Inline::Image(Image { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics, lua_engine);
        }
        Inline::Note(Note { content, .. }) => {
            resolve_blocks(content, transform, metadata, diagnostics, lua_engine);
        }
        Inline::Span(Span { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics, lua_engine);
        }
        Inline::EditComment(EditComment { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics, lua_engine);
        }
        Inline::Custom(custom) => {
            // Resolve shortcodes in custom inline node slots
            for slot in custom.slots.values_mut() {
                match slot {
                    quarto_pandoc_types::custom::Slot::Inlines(is) => {
                        resolve_inlines(is, transform, metadata, diagnostics, lua_engine);
                    }
                    quarto_pandoc_types::custom::Slot::Inline(i) => {
                        let mut inlines = vec![i.as_ref().clone()];
                        resolve_inlines(&mut inlines, transform, metadata, diagnostics, lua_engine);
                        if inlines.len() == 1 {
                            **i = inlines.pop().unwrap();
                        }
                    }
                    quarto_pandoc_types::custom::Slot::Blocks(bs) => {
                        resolve_blocks(bs, transform, metadata, diagnostics, lua_engine);
                    }
                    quarto_pandoc_types::custom::Slot::Block(b) => {
                        resolve_block(b, transform, metadata, diagnostics, lua_engine);
                    }
                }
            }
        }
        // These inlines don't contain nested content
        Inline::Str(_)
        | Inline::Code(Code { .. })
        | Inline::Space(_)
        | Inline::SoftBreak(_)
        | Inline::LineBreak(_)
        | Inline::Math(_)
        | Inline::RawInline(_)
        | Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_, _) => {}
    }
}

/// Create visible error inline: Strong("?key")
fn make_error_inline(key: &str) -> Inline {
    Inline::Strong(Strong {
        content: vec![Inline::Str(Str {
            text: format!("?{}", key),
            source_info: SourceInfo::default(),
        })],
        source_info: SourceInfo::default(),
    })
}

/// Convert an escaped shortcode to literal text.
///
/// For `{{{< meta title >}}}`, this produces `{{< meta title >}}`
fn shortcode_to_literal(shortcode: &Shortcode) -> Inline {
    let mut text = String::from("{{< ");
    text.push_str(&shortcode.name);

    for arg in &shortcode.positional_args {
        text.push(' ');
        match arg {
            ShortcodeArg::String(s) => {
                // Quote strings that contain spaces
                if s.contains(' ') {
                    text.push('"');
                    text.push_str(s);
                    text.push('"');
                } else {
                    text.push_str(s);
                }
            }
            ShortcodeArg::Number(n) => {
                text.push_str(&n.to_string());
            }
            ShortcodeArg::Boolean(b) => {
                text.push_str(&b.to_string());
            }
            ShortcodeArg::Shortcode(sc) => {
                // Nested shortcode - just use name for now
                text.push_str("{{< ");
                text.push_str(&sc.name);
                text.push_str(" >}}");
            }
            ShortcodeArg::KeyValue(_) => {
                text.push_str("{...}");
            }
        }
    }

    for (key, value) in &shortcode.keyword_args {
        text.push(' ');
        text.push_str(key);
        text.push('=');
        match value {
            ShortcodeArg::String(s) => {
                text.push('"');
                text.push_str(s);
                text.push('"');
            }
            ShortcodeArg::Number(n) => {
                text.push_str(&n.to_string());
            }
            ShortcodeArg::Boolean(b) => {
                text.push_str(&b.to_string());
            }
            ShortcodeArg::Shortcode(sc) => {
                text.push_str("{{< ");
                text.push_str(&sc.name);
                text.push_str(" >}}");
            }
            ShortcodeArg::KeyValue(_) => {
                text.push_str("{...}");
            }
        }
    }

    text.push_str(" >}}");

    Inline::Str(Str {
        text,
        source_info: SourceInfo::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_source_map::{FileId, Location, Range};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    fn make_shortcode(name: &str, args: Vec<&str>) -> Shortcode {
        Shortcode {
            is_escaped: false,
            name: name.to_string(),
            positional_args: args
                .into_iter()
                .map(|s| ShortcodeArg::String(s.to_string()))
                .collect(),
            keyword_args: HashMap::new(),
            source_info: dummy_source_info(),
        }
    }

    fn make_map_entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: dummy_source_info(),
            value,
        }
    }

    #[test]
    fn test_transform_name() {
        let transform = ShortcodeResolveTransform::new();
        assert_eq!(transform.name(), "shortcode-resolve");
    }

    // Note: Tests for get_nested metadata lookup are in quarto-pandoc-types
    // (ConfigValue::get_nested), so we don't duplicate them here.

    #[test]
    fn test_config_value_to_inlines_string() {
        let value = ConfigValue::new_string("Hello", dummy_source_info());
        let inlines = config_value_to_inlines(&value);
        assert_eq!(inlines.len(), 1);
        if let Inline::Str(s) = &inlines[0] {
            assert_eq!(s.text, "Hello");
        } else {
            panic!("Expected Str inline");
        }
    }

    #[test]
    fn test_config_value_to_inlines_bool() {
        let value = ConfigValue::new_bool(true, dummy_source_info());
        let inlines = config_value_to_inlines(&value);
        assert_eq!(inlines.len(), 1);
        if let Inline::Str(s) = &inlines[0] {
            assert_eq!(s.text, "true");
        } else {
            panic!("Expected Str inline");
        }
    }

    #[test]
    fn test_meta_shortcode_handler_success() {
        let handler = MetaShortcodeHandler;
        let shortcode = make_shortcode("meta", vec!["title"]);

        let meta = ConfigValue::new_map(
            vec![make_map_entry(
                "title",
                ConfigValue::new_string("My Title", dummy_source_info()),
            )],
            dummy_source_info(),
        );

        let ctx = ShortcodeContext {
            metadata: &meta,
            source_info: &shortcode.source_info,
        };

        let result = handler.resolve(&shortcode, &ctx, ResolutionContext::Inline);
        match result {
            ShortcodeResult::Inlines(inlines) => {
                assert_eq!(inlines.len(), 1);
                if let Inline::Str(s) = &inlines[0] {
                    assert_eq!(s.text, "My Title");
                } else {
                    panic!("Expected Str inline");
                }
            }
            _ => panic!("Expected Inlines result"),
        }
    }

    #[test]
    fn test_meta_shortcode_handler_missing_key() {
        let handler = MetaShortcodeHandler;
        let shortcode = make_shortcode("meta", vec!["nonexistent"]);

        let meta = ConfigValue::new_map(
            vec![make_map_entry(
                "title",
                ConfigValue::new_string("My Title", dummy_source_info()),
            )],
            dummy_source_info(),
        );

        let ctx = ShortcodeContext {
            metadata: &meta,
            source_info: &shortcode.source_info,
        };

        let result = handler.resolve(&shortcode, &ctx, ResolutionContext::Inline);
        match result {
            ShortcodeResult::Error(err) => {
                assert_eq!(err.key, "meta:nonexistent");
            }
            _ => panic!("Expected Error result"),
        }
    }

    #[test]
    fn test_meta_shortcode_handler_missing_arg() {
        let handler = MetaShortcodeHandler;
        let shortcode = make_shortcode("meta", vec![]);

        let meta = ConfigValue::default();

        let ctx = ShortcodeContext {
            metadata: &meta,
            source_info: &shortcode.source_info,
        };

        let result = handler.resolve(&shortcode, &ctx, ResolutionContext::Inline);
        match result {
            ShortcodeResult::Error(err) => {
                assert_eq!(err.key, "meta");
            }
            _ => panic!("Expected Error result"),
        }
    }

    #[test]
    fn test_resolve_escaped_shortcode() {
        let transform = ShortcodeResolveTransform::new();

        let shortcode = Shortcode {
            is_escaped: true,
            name: "meta".to_string(),
            positional_args: vec![ShortcodeArg::String("title".to_string())],
            keyword_args: HashMap::new(),
            source_info: dummy_source_info(),
        };

        let ctx = ShortcodeContext {
            metadata: &ConfigValue::default(),
            source_info: &shortcode.source_info,
        };

        let result =
            transform.resolve_shortcode(&shortcode, &ctx, ResolutionContext::Inline, &mut None);
        assert!(matches!(result, ShortcodeResult::Preserve));
    }

    #[test]
    fn test_resolve_unknown_shortcode() {
        let transform = ShortcodeResolveTransform::new();

        let shortcode = make_shortcode("unknown", vec![]);

        let ctx = ShortcodeContext {
            metadata: &ConfigValue::default(),
            source_info: &shortcode.source_info,
        };

        let result =
            transform.resolve_shortcode(&shortcode, &ctx, ResolutionContext::Inline, &mut None);
        match result {
            ShortcodeResult::Error(err) => {
                assert_eq!(err.key, "unknown");
            }
            _ => panic!("Expected Error result"),
        }
    }

    #[test]
    fn test_make_error_inline() {
        let inline = make_error_inline("meta:title");
        match inline {
            Inline::Strong(strong) => {
                assert_eq!(strong.content.len(), 1);
                if let Inline::Str(s) = &strong.content[0] {
                    assert_eq!(s.text, "?meta:title");
                } else {
                    panic!("Expected Str inline");
                }
            }
            _ => panic!("Expected Strong inline"),
        }
    }

    #[test]
    fn test_shortcode_to_literal() {
        let shortcode = make_shortcode("meta", vec!["title"]);
        let literal = shortcode_to_literal(&shortcode);
        if let Inline::Str(s) = literal {
            assert_eq!(s.text, "{{< meta title >}}");
        } else {
            panic!("Expected Str inline");
        }
    }

    #[test]
    fn test_full_transform() {
        let transform = ShortcodeResolveTransform::new();

        // Create AST with a shortcode
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![make_map_entry(
                    "title",
                    ConfigValue::new_string("Test Title", dummy_source_info()),
                )],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![
                    Inline::Str(Str {
                        text: "Title: ".to_string(),
                        source_info: dummy_source_info(),
                    }),
                    Inline::Shortcode(make_shortcode("meta", vec!["title"])),
                ],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        transform.transform(&mut ast, &mut ctx).unwrap();

        // Verify shortcode was resolved
        if let Block::Paragraph(para) = &ast.blocks[0] {
            assert_eq!(para.content.len(), 2);
            if let Inline::Str(s) = &para.content[1] {
                assert_eq!(s.text, "Test Title");
            } else {
                panic!("Expected Str inline, got {:?}", para.content[1]);
            }
        } else {
            panic!("Expected Paragraph");
        }

        // Verify no warnings were emitted
        assert!(ctx.diagnostics.is_empty());
    }

    #[test]
    fn test_full_transform_with_error() {
        let transform = ShortcodeResolveTransform::new();

        // Create AST with a shortcode referencing missing key
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![make_map_entry(
                    "title",
                    ConfigValue::new_string("Test Title", dummy_source_info()),
                )],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Shortcode(make_shortcode("meta", vec!["missing"]))],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        transform.transform(&mut ast, &mut ctx).unwrap();

        // Verify error inline was inserted
        if let Block::Paragraph(para) = &ast.blocks[0] {
            assert_eq!(para.content.len(), 1);
            if let Inline::Strong(strong) = &para.content[0] {
                if let Inline::Str(s) = &strong.content[0] {
                    assert_eq!(s.text, "?meta:missing");
                } else {
                    panic!("Expected Str inline");
                }
            } else {
                panic!("Expected Strong inline");
            }
        } else {
            panic!("Expected Paragraph");
        }

        // Verify warning was emitted
        assert_eq!(ctx.diagnostics.len(), 1);
    }

    /// A test handler that returns Blocks when in block context.
    struct BlockTestHandler;
    impl ShortcodeHandler for BlockTestHandler {
        fn name(&self) -> &str {
            "block-test"
        }
        fn resolve(
            &self,
            _shortcode: &Shortcode,
            _ctx: &ShortcodeContext,
            resolution_ctx: ResolutionContext,
        ) -> ShortcodeResult {
            match resolution_ctx {
                ResolutionContext::Block => ShortcodeResult::Blocks(vec![Block::HorizontalRule(
                    quarto_pandoc_types::block::HorizontalRule {
                        source_info: SourceInfo::default(),
                    },
                )]),
                ResolutionContext::Inline => ShortcodeResult::Inlines(vec![Inline::Str(Str {
                    text: "inline-fallback".to_string(),
                    source_info: SourceInfo::default(),
                })]),
            }
        }
    }

    #[test]
    fn test_block_shortcode_replaces_para() {
        let transform = ShortcodeResolveTransform {
            handlers: vec![Box::new(BlockTestHandler)],
            lua_shortcode_paths: Vec::new(),
            extensions: Vec::new(),
            runtime: None,
            target_format: String::new(),
        };

        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Shortcode(make_shortcode("block-test", vec![]))],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        transform.transform(&mut ast, &mut ctx).unwrap();

        // The Para should be replaced by a HorizontalRule
        assert_eq!(ast.blocks.len(), 1);
        assert!(
            matches!(&ast.blocks[0], Block::HorizontalRule(_)),
            "Expected HorizontalRule, got {:?}",
            ast.blocks[0]
        );
    }

    #[test]
    fn test_inline_shortcode_in_para_stays_inline() {
        let transform = ShortcodeResolveTransform {
            handlers: vec![Box::new(BlockTestHandler)],
            lua_shortcode_paths: Vec::new(),
            extensions: Vec::new(),
            runtime: None,
            target_format: String::new(),
        };

        // Para with text + shortcode — not a block-context shortcode
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![
                    Inline::Str(Str {
                        text: "Before: ".to_string(),
                        source_info: dummy_source_info(),
                    }),
                    Inline::Shortcode(make_shortcode("block-test", vec![])),
                ],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should remain a Paragraph with resolved inline content
        assert_eq!(ast.blocks.len(), 1);
        if let Block::Paragraph(para) = &ast.blocks[0] {
            assert_eq!(para.content.len(), 2);
            if let Inline::Str(s) = &para.content[1] {
                assert_eq!(s.text, "inline-fallback");
            } else {
                panic!("Expected Str inline, got {:?}", para.content[1]);
            }
        } else {
            panic!("Expected Paragraph");
        }
    }

    /// Handler that always returns Blocks regardless of context.
    struct AlwaysBlockHandler;
    impl ShortcodeHandler for AlwaysBlockHandler {
        fn name(&self) -> &str {
            "always-block"
        }
        fn resolve(
            &self,
            _shortcode: &Shortcode,
            _ctx: &ShortcodeContext,
            _resolution_ctx: ResolutionContext,
        ) -> ShortcodeResult {
            ShortcodeResult::Blocks(vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "from-block".to_string(),
                    source_info: SourceInfo::default(),
                })],
                source_info: SourceInfo::default(),
            })])
        }
    }

    #[test]
    fn test_block_result_in_inline_context() {
        let transform = ShortcodeResolveTransform {
            handlers: vec![Box::new(AlwaysBlockHandler)],
            lua_shortcode_paths: Vec::new(),
            extensions: Vec::new(),
            runtime: None,
            target_format: String::new(),
        };

        // Para with text + shortcode — inline context, handler returns Blocks
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![
                    Inline::Str(Str {
                        text: "Before: ".to_string(),
                        source_info: dummy_source_info(),
                    }),
                    Inline::Shortcode(make_shortcode("always-block", vec![])),
                ],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        transform.transform(&mut ast, &mut ctx).unwrap();

        // Blocks should be flattened to inlines
        if let Block::Paragraph(para) = &ast.blocks[0] {
            assert_eq!(para.content.len(), 2);
            if let Inline::Str(s) = &para.content[1] {
                assert_eq!(s.text, "from-block");
            } else {
                panic!("Expected Str inline, got {:?}", para.content[1]);
            }
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_escaped_shortcode_block_context() {
        let transform = ShortcodeResolveTransform {
            handlers: vec![Box::new(MetaShortcodeHandler)],
            lua_shortcode_paths: Vec::new(),
            extensions: Vec::new(),
            runtime: None,
            target_format: String::new(),
        };

        // Escaped shortcode alone in Para — should preserve as literal
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Shortcode(Shortcode {
                    is_escaped: true,
                    name: "meta".to_string(),
                    positional_args: vec![ShortcodeArg::String("title".to_string())],
                    keyword_args: HashMap::new(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should be converted to literal text in the Para
        if let Block::Paragraph(para) = &ast.blocks[0] {
            assert_eq!(para.content.len(), 1);
            if let Inline::Str(s) = &para.content[0] {
                assert_eq!(s.text, "{{< meta title >}}");
            } else {
                panic!("Expected Str inline, got {:?}", para.content[0]);
            }
        } else {
            panic!("Expected Paragraph");
        }
    }

    // === Lua integration tests (3.4.6) ===
    // These tests require the native Lua runtime
    #[cfg(not(target_arch = "wasm32"))]
    mod lua_integration {
        use super::*;
        use crate::extension::types::{Contributes, Extension, ExtensionId};
        use std::io::Write;
        use tempfile::TempDir;

        fn make_runtime() -> Arc<dyn quarto_system_runtime::SystemRuntime> {
            Arc::new(quarto_system_runtime::NativeRuntime::new())
        }

        fn write_lua_script(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
            let path = dir.join(name);
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(content.as_bytes()).unwrap();
            path
        }

        fn make_extension(name: &str, shortcode_paths: Vec<PathBuf>) -> Extension {
            Extension {
                id: ExtensionId::new(name),
                title: name.to_string(),
                author: String::new(),
                version: None,
                quarto_required: None,
                path: PathBuf::from("/extensions").join(name),
                contributes: Contributes {
                    shortcodes: shortcode_paths,
                    ..Default::default()
                },
            }
        }

        #[test]
        fn test_lua_shortcode_from_metadata_paths() {
            let tmp = TempDir::new().unwrap();
            let script_path = write_lua_script(
                tmp.path(),
                "hello.lua",
                r#"return { hello = function(args) return "Hello from Lua" end }"#,
            );

            let runtime = make_runtime();
            let transform = ShortcodeResolveTransform::with_lua_support(
                vec![script_path],
                Vec::new(),
                runtime,
                "html".to_string(),
            );

            let mut ast = Pandoc {
                meta: ConfigValue::default(),
                blocks: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Shortcode(make_shortcode("hello", vec![]))],
                    source_info: dummy_source_info(),
                })],
            };

            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

            transform.transform(&mut ast, &mut ctx).unwrap();

            if let Block::Paragraph(para) = &ast.blocks[0] {
                if let Inline::Str(s) = &para.content[0] {
                    assert_eq!(s.text, "Hello from Lua");
                } else {
                    panic!("Expected Str inline, got {:?}", para.content[0]);
                }
            } else {
                panic!("Expected Paragraph");
            }
            assert!(ctx.diagnostics.is_empty());
        }

        #[test]
        fn test_lua_shortcode_by_extension_name() {
            let tmp = TempDir::new().unwrap();
            let script_path = write_lua_script(
                tmp.path(),
                "greet.lua",
                r#"return { greet = function(args) return "Extension greeting" end }"#,
            );

            let ext = make_extension("greet", vec![script_path]);
            let runtime = make_runtime();

            // No metadata paths — extension discovered by name
            let transform = ShortcodeResolveTransform::with_lua_support(
                Vec::new(),
                vec![ext],
                runtime,
                "html".to_string(),
            );

            let mut ast = Pandoc {
                meta: ConfigValue::default(),
                blocks: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Shortcode(make_shortcode("greet", vec![]))],
                    source_info: dummy_source_info(),
                })],
            };

            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

            transform.transform(&mut ast, &mut ctx).unwrap();

            if let Block::Paragraph(para) = &ast.blocks[0] {
                if let Inline::Str(s) = &para.content[0] {
                    assert_eq!(s.text, "Extension greeting");
                } else {
                    panic!("Expected Str inline, got {:?}", para.content[0]);
                }
            } else {
                panic!("Expected Paragraph");
            }
            assert!(ctx.diagnostics.is_empty());
        }

        #[test]
        fn test_rust_handler_overrides_lua() {
            let tmp = TempDir::new().unwrap();
            // Lua script defines a "meta" handler that should be ignored
            let script_path = write_lua_script(
                tmp.path(),
                "meta.lua",
                r#"return { meta = function(args) return "FROM LUA" end }"#,
            );

            let runtime = make_runtime();
            let transform = ShortcodeResolveTransform::with_lua_support(
                vec![script_path],
                Vec::new(),
                runtime,
                "html".to_string(),
            );

            let meta = ConfigValue::new_map(
                vec![make_map_entry(
                    "title",
                    ConfigValue::new_string("Rust Title", dummy_source_info()),
                )],
                dummy_source_info(),
            );

            let mut ast = Pandoc {
                meta,
                blocks: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Shortcode(make_shortcode("meta", vec!["title"]))],
                    source_info: dummy_source_info(),
                })],
            };

            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

            transform.transform(&mut ast, &mut ctx).unwrap();

            // Built-in Rust handler should win over Lua
            if let Block::Paragraph(para) = &ast.blocks[0] {
                if let Inline::Str(s) = &para.content[0] {
                    assert_eq!(s.text, "Rust Title");
                } else {
                    panic!("Expected Str inline, got {:?}", para.content[0]);
                }
            } else {
                panic!("Expected Paragraph");
            }
        }

        #[test]
        fn test_unknown_shortcode_error() {
            let runtime = make_runtime();
            let transform = ShortcodeResolveTransform::with_lua_support(
                Vec::new(),
                Vec::new(),
                runtime,
                "html".to_string(),
            );

            let mut ast = Pandoc {
                meta: ConfigValue::default(),
                blocks: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Shortcode(make_shortcode("nonexistent", vec![]))],
                    source_info: dummy_source_info(),
                })],
            };

            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

            transform.transform(&mut ast, &mut ctx).unwrap();

            // Should produce error inline
            if let Block::Paragraph(para) = &ast.blocks[0] {
                if let Inline::Strong(strong) = &para.content[0] {
                    if let Inline::Str(s) = &strong.content[0] {
                        assert_eq!(s.text, "?nonexistent");
                    } else {
                        panic!("Expected Str in Strong");
                    }
                } else {
                    panic!("Expected Strong inline, got {:?}", para.content[0]);
                }
            } else {
                panic!("Expected Paragraph");
            }
            assert_eq!(ctx.diagnostics.len(), 1);
        }

        #[test]
        fn test_extension_shortcode_block_context() {
            let tmp = TempDir::new().unwrap();
            let script_path = write_lua_script(
                tmp.path(),
                "break.lua",
                r#"return { ["break"] = function(args) return pandoc.HorizontalRule() end }"#,
            );

            let ext = make_extension("break", vec![script_path]);
            let runtime = make_runtime();

            let transform = ShortcodeResolveTransform::with_lua_support(
                Vec::new(),
                vec![ext],
                runtime,
                "html".to_string(),
            );

            // Shortcode alone in Para → block context
            let mut ast = Pandoc {
                meta: ConfigValue::default(),
                blocks: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Shortcode(make_shortcode("break", vec![]))],
                    source_info: dummy_source_info(),
                })],
            };

            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

            transform.transform(&mut ast, &mut ctx).unwrap();

            // Para should be replaced by HorizontalRule
            assert_eq!(ast.blocks.len(), 1);
            assert!(
                matches!(&ast.blocks[0], Block::HorizontalRule(_)),
                "Expected HorizontalRule, got {:?}",
                ast.blocks[0]
            );
            assert!(ctx.diagnostics.is_empty());
        }

        #[test]
        fn test_full_transform_block_shortcode_rawblock() {
            let tmp = TempDir::new().unwrap();
            let script_path = write_lua_script(
                tmp.path(),
                "pagebreak.lua",
                r#"return { pagebreak = function(args) return pandoc.RawBlock("html", "<hr class=\"page-break\">") end }"#,
            );

            let runtime = make_runtime();
            let transform = ShortcodeResolveTransform::with_lua_support(
                vec![script_path],
                Vec::new(),
                runtime,
                "html".to_string(),
            );

            let mut ast = Pandoc {
                meta: ConfigValue::default(),
                blocks: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Shortcode(make_shortcode("pagebreak", vec![]))],
                    source_info: dummy_source_info(),
                })],
            };

            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

            transform.transform(&mut ast, &mut ctx).unwrap();

            // Para should be replaced by RawBlock
            assert_eq!(ast.blocks.len(), 1);
            match &ast.blocks[0] {
                Block::RawBlock(rb) => {
                    assert_eq!(rb.format, "html");
                    assert!(rb.text.contains("page-break"));
                }
                other => panic!("Expected RawBlock, got {:?}", other),
            }
            assert!(ctx.diagnostics.is_empty());
        }
    }
}
