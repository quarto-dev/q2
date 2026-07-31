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
use quarto_source_map::{Anchor, By, SourceInfo};
use smallvec::smallvec;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use quarto_analysis::AnalysisContext;

use crate::Result;
use crate::extension::types::Extension;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
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
///
/// The synthesized `Inline::Str` instances reuse the input
/// `ConfigValue`'s own `source_info` — the bytes effectively come
/// from the YAML value being interpolated. Plan 7f Phase 6.5
/// switched these from `SourceInfo::default()`; the canonical
/// `stamp_block`/`stamp_inline` enrichment pass (further down in
/// this file) wraps the result with the shortcode token's
/// `Invocation` anchor.
fn config_value_to_inlines(value: &ConfigValue) -> Vec<Inline> {
    // Use helper methods on ConfigValue for scalar types
    if let Some(s) = value.as_str() {
        return vec![Inline::Str(Str {
            text: s.to_string(),
            source_info: value.source_info.clone(),
        })];
    }

    if let Some(b) = value.as_bool() {
        return vec![Inline::Str(Str {
            text: b.to_string(),
            source_info: value.source_info.clone(),
        })];
    }

    if let Some(n) = value.as_int() {
        return vec![Inline::Str(Str {
            text: n.to_string(),
            source_info: value.source_info.clone(),
        })];
    }

    // Handle specific ConfigValueKind variants
    match &value.value {
        ConfigValueKind::PandocInlines(inlines) => inlines.clone(),
        ConfigValueKind::PandocBlocks(blocks) => {
            // For blocks in inline context, flatten to plain text
            // This matches TS Quarto behavior
            flatten_blocks_to_inlines(blocks, &value.source_info)
        }
        // Scalar that wasn't captured by helpers (e.g., float, null)
        ConfigValueKind::Scalar(_) => {
            if let Some(plain) = value.as_plain_text() {
                vec![Inline::Str(Str {
                    text: plain,
                    source_info: value.source_info.clone(),
                })]
            } else {
                vec![Inline::Str(Str {
                    text: String::new(),
                    source_info: value.source_info.clone(),
                })]
            }
        }
        // Arrays and maps - not suitable for inline context
        ConfigValueKind::Array(_) | ConfigValueKind::Map(_) => vec![Inline::Str(Str {
            text: "?invalid meta type".to_string(),
            source_info: value.source_info.clone(),
        })],
        // Path, Glob, Expr were handled by as_str() above
        ConfigValueKind::Path(_) | ConfigValueKind::Glob(_) | ConfigValueKind::Expr(_) => {
            // This shouldn't be reached since as_str() handles these
            vec![Inline::Str(Str {
                text: "?invalid meta type".to_string(),
                source_info: value.source_info.clone(),
            })]
        }
    }
}

/// Flatten blocks to inlines (extracts text content). The inter-paragraph
/// `Space` reuses the surrounding `ConfigValue.source_info` so the
/// synthesized separator is still attributable.
fn flatten_blocks_to_inlines(blocks: &[Block], value_source: &SourceInfo) -> Vec<Inline> {
    let mut result = Vec::new();
    for block in blocks {
        match block {
            Block::Plain(plain) => result.extend(plain.content.clone()),
            Block::Paragraph(para) => {
                if !result.is_empty() {
                    // Add space between paragraphs
                    result.push(Inline::Space(quarto_pandoc_types::inline::Space {
                        source_info: value_source.clone(),
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

/// Lua engine plus one-shot extension-activation state.
///
/// The `extensions_loaded` flag lives with the engine (not the transform) so a
/// freshly created engine can never observe a stale "already loaded" marker
/// from a previous document.
pub struct LuaEngineState {
    engine: pampa::lua::LuaShortcodeEngine,
    /// Whether every discovered extension's `contributes.shortcodes` scripts
    /// have been loaded into this engine. Set on the first dispatch that
    /// reaches the Lua stage.
    extensions_loaded: bool,
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
    ///
    /// All `ShortcodeResult::Inlines`/`Blocks` outcomes flow through this single
    /// funnel and are post-walked by `stamp_shortcode_anchors`, which stamps each
    /// returned node with `Generated { by: shortcode(name), from: [Invocation -> ctx.source_info] }`
    /// (and enriches any Lua filter-attached source_info). `Preserve` and `Error`
    /// outcomes do not need stamping — `Preserve` becomes a literal Str via
    /// `shortcode_to_literal` and `Error` becomes a visible error via
    /// `make_error_inline`; both sites carry the token's `Original` source_info
    /// directly.
    async fn resolve_shortcode(
        &self,
        shortcode: &Shortcode,
        ctx: &ShortcodeContext<'_>,
        resolution_ctx: ResolutionContext,
        lua_engine: &mut Option<LuaEngineState>,
        diagnostics: &mut Vec<DiagnosticMessage>,
    ) -> ShortcodeResult {
        let mut result = self
            .dispatch_shortcode(shortcode, ctx, resolution_ctx, lua_engine, diagnostics)
            .await;
        stamp_shortcode_anchors(&mut result, &shortcode.name, ctx.source_info);
        result
    }

    /// Inner dispatch — picks the handler and returns the raw result. Wrapped by
    /// [`resolve_shortcode`], which post-walks the result to stamp Invocation
    /// anchors.
    async fn dispatch_shortcode(
        &self,
        shortcode: &Shortcode,
        ctx: &ShortcodeContext<'_>,
        resolution_ctx: ResolutionContext,
        lua_engine: &mut Option<LuaEngineState>,
        diagnostics: &mut Vec<DiagnosticMessage>,
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

        // 2. Lua handlers. On the first dispatch that reaches the Lua stage,
        //    eagerly load every discovered extension's shortcode scripts.
        //    Handler names come from the Lua table keys (or harvested globals),
        //    decoupled from the extension id — an extension named
        //    `quarto-tiers` may contribute a shortcode named `tier`, so
        //    lookup-by-extension-name cannot work. Load order determines
        //    same-name precedence (later registration wins): document
        //    `shortcodes:` scripts (loaded at engine creation), then
        //    extensions in discovery order (built-ins first, more-local
        //    last). Rust built-in handlers (step 1) always win.
        if let Some(state) = lua_engine.as_mut() {
            if !state.extensions_loaded {
                state.extensions_loaded = true;
                for ext in &self.extensions {
                    for script_path in &ext.contributes.shortcodes {
                        if let Err(e) = state.engine.load_script(script_path).await {
                            // A broken script must not hijack the triggering
                            // shortcode's result (it may resolve from another
                            // extension); warn with the extension and script
                            // named as the cause, and keep loading the rest.
                            diagnostics.push(
                                DiagnosticMessageBuilder::warning("Shortcode script error")
                                    .problem(format!(
                                        "Failed to load shortcode script `{}` from extension `{}`: {}",
                                        script_path.display(),
                                        ext.id,
                                        e
                                    ))
                                    .with_location(ctx.source_info.clone())
                                    .build(),
                            );
                        }
                    }
                }
            }
            if state.engine.has_handler(&shortcode.name) {
                return dispatch_lua_shortcode(&mut state.engine, shortcode, ctx, resolution_ctx)
                    .await;
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
async fn dispatch_lua_shortcode(
    engine: &mut pampa::lua::LuaShortcodeEngine,
    shortcode: &Shortcode,
    ctx: &ShortcodeContext<'_>,
    resolution_ctx: ResolutionContext,
) -> ShortcodeResult {
    let args = shortcode_to_lua_args(shortcode, ctx.metadata);
    let call_ctx = match resolution_ctx {
        ResolutionContext::Block => pampa::lua::ShortcodeCallContext::Block,
        ResolutionContext::Inline => pampa::lua::ShortcodeCallContext::Inline,
    };
    match engine.call(&shortcode.name, &args, call_ctx).await {
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

    // Extract top-level metadata as string key-value pairs for Lua.
    //
    // Forward scalars of every stringifiable kind, not just strings: a handler
    // that gates on a boolean/numeric flag (e.g. the `video` shortcode reading
    // `auto-stretch: false` to decide reveal stretching — bd-5b21rbaq) needs to
    // see it. Booleans/ints are stringified ("false", "16"); string and
    // PandocInlines scalars come through `as_plain_text()`. Map/array values
    // remain dropped (no flat string form).
    let meta_entries: Vec<(String, String)> = if let Some(entries) = metadata.as_map_entries() {
        entries
            .iter()
            .filter_map(|entry| {
                let v = &entry.value;
                let s = v
                    .as_bool()
                    .map(|b| b.to_string())
                    .or_else(|| v.as_int().map(|n| n.to_string()))
                    .or_else(|| v.as_plain_text());
                s.map(|s| (entry.key.clone(), s))
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
            // Reuse the shortcode token's range; the stamper pass that
            // wraps this result then attaches the `Invocation` anchor.
            ShortcodeResult::Inlines(vec![Inline::Str(Str {
                text,
                source_info: source_info.clone(),
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

/// After every shortcode handler dispatch, stamp Invocation provenance on the
/// returned nodes. Recurses into nested AST so every block and inline gets the
/// anchor.
///
/// Enrichment rules (per Plan 6 §"Lua-shortcode enrichment"):
/// - If the existing source_info is `Generated { by: filter, ... }` (Lua's
///   `filter_source_info` auto-attach), promote `by.kind` to `"shortcode"` and
///   move the `filter_path`/`line` data fields into `lua_path`/`lua_line`,
///   then append the Invocation anchor.
/// - Otherwise, replace with a fresh `Generated { by: shortcode(name),
///   from: [Invocation] }`.
fn stamp_shortcode_anchors(
    result: &mut ShortcodeResult,
    shortcode_name: &str,
    token_si: &SourceInfo,
) {
    let token_arc = Arc::new(token_si.clone());
    match result {
        ShortcodeResult::Inlines(inlines) => {
            for inline in inlines.iter_mut() {
                stamp_inline(inline, shortcode_name, &token_arc);
            }
        }
        ShortcodeResult::Blocks(blocks) => {
            for block in blocks.iter_mut() {
                stamp_block(block, shortcode_name, &token_arc);
            }
        }
        ShortcodeResult::Preserve | ShortcodeResult::Error(_) => {}
    }
}

/// Stamp the Invocation anchor on a single inline and recurse into its children.
fn stamp_inline(inline: &mut Inline, name: &str, token_arc: &Arc<SourceInfo>) {
    let new_si = enrich_or_create(inline.source_info(), name, token_arc);
    *inline.source_info_mut() = new_si;
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
        | Inline::Highlight(Highlight { content, .. })
        | Inline::Quoted(Quoted { content, .. })
        | Inline::Cite(Cite { content, .. })
        | Inline::Link(Link { content, .. })
        | Inline::Image(Image { content, .. })
        | Inline::Span(Span { content, .. })
        | Inline::EditComment(EditComment { content, .. }) => {
            for child in content.iter_mut() {
                stamp_inline(child, name, token_arc);
            }
        }
        Inline::Note(Note { content, .. }) => {
            for child in content.iter_mut() {
                stamp_block(child, name, token_arc);
            }
        }
        Inline::Custom(custom) => {
            for slot in custom.slots.values_mut() {
                match slot {
                    quarto_pandoc_types::custom::Slot::Inline(i) => {
                        stamp_inline(i, name, token_arc);
                    }
                    quarto_pandoc_types::custom::Slot::Inlines(is) => {
                        for child in is.iter_mut() {
                            stamp_inline(child, name, token_arc);
                        }
                    }
                    quarto_pandoc_types::custom::Slot::Block(b) => {
                        stamp_block(b, name, token_arc);
                    }
                    quarto_pandoc_types::custom::Slot::Blocks(bs) => {
                        for child in bs.iter_mut() {
                            stamp_block(child, name, token_arc);
                        }
                    }
                }
            }
        }
        // Leaves — no nested AST to walk.
        Inline::Str(_)
        | Inline::Code(_)
        | Inline::Space(_)
        | Inline::SoftBreak(_)
        | Inline::LineBreak(_)
        | Inline::Math(_)
        | Inline::RawInline(_)
        | Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_) => {}
    }
}

/// Stamp the Invocation anchor on a single block and recurse into its children.
fn stamp_block(block: &mut Block, name: &str, token_arc: &Arc<SourceInfo>) {
    let new_si = enrich_or_create(block.source_info(), name, token_arc);
    *block.source_info_mut() = new_si;
    match block {
        Block::Plain(Plain { content, .. }) | Block::Paragraph(Paragraph { content, .. }) => {
            for child in content.iter_mut() {
                stamp_inline(child, name, token_arc);
            }
        }
        Block::LineBlock(LineBlock { content, .. }) => {
            for line in content.iter_mut() {
                for child in line.iter_mut() {
                    stamp_inline(child, name, token_arc);
                }
            }
        }
        Block::Header(Header { content, .. }) => {
            for child in content.iter_mut() {
                stamp_inline(child, name, token_arc);
            }
        }
        Block::BlockQuote(BlockQuote { content, .. }) => {
            for child in content.iter_mut() {
                stamp_block(child, name, token_arc);
            }
        }
        Block::OrderedList(OrderedList { content, .. })
        | Block::BulletList(BulletList { content, .. }) => {
            for item in content.iter_mut() {
                for child in item.iter_mut() {
                    stamp_block(child, name, token_arc);
                }
            }
        }
        Block::DefinitionList(DefinitionList { content, .. }) => {
            for (term, defs) in content.iter_mut() {
                for child in term.iter_mut() {
                    stamp_inline(child, name, token_arc);
                }
                for def in defs.iter_mut() {
                    for child in def.iter_mut() {
                        stamp_block(child, name, token_arc);
                    }
                }
            }
        }
        Block::Figure(Figure {
            content, caption, ..
        }) => {
            for child in content.iter_mut() {
                stamp_block(child, name, token_arc);
            }
            if let Some(short) = caption.short.as_mut() {
                for child in short.iter_mut() {
                    stamp_inline(child, name, token_arc);
                }
            }
            if let Some(long) = caption.long.as_mut() {
                for child in long.iter_mut() {
                    stamp_block(child, name, token_arc);
                }
            }
        }
        Block::Div(Div { content, .. }) => {
            for child in content.iter_mut() {
                stamp_block(child, name, token_arc);
            }
        }
        Block::Table(Table {
            caption,
            head,
            bodies,
            foot,
            ..
        }) => {
            if let Some(short) = caption.short.as_mut() {
                for child in short.iter_mut() {
                    stamp_inline(child, name, token_arc);
                }
            }
            if let Some(long) = caption.long.as_mut() {
                for child in long.iter_mut() {
                    stamp_block(child, name, token_arc);
                }
            }
            for row in head.rows.iter_mut() {
                for cell in row.cells.iter_mut() {
                    for child in cell.content.iter_mut() {
                        stamp_block(child, name, token_arc);
                    }
                }
            }
            for body in bodies.iter_mut() {
                for row in body.body.iter_mut() {
                    for cell in row.cells.iter_mut() {
                        for child in cell.content.iter_mut() {
                            stamp_block(child, name, token_arc);
                        }
                    }
                }
            }
            for row in foot.rows.iter_mut() {
                for cell in row.cells.iter_mut() {
                    for child in cell.content.iter_mut() {
                        stamp_block(child, name, token_arc);
                    }
                }
            }
        }
        Block::Custom(custom) => {
            for slot in custom.slots.values_mut() {
                match slot {
                    quarto_pandoc_types::custom::Slot::Inline(i) => {
                        stamp_inline(i, name, token_arc);
                    }
                    quarto_pandoc_types::custom::Slot::Inlines(is) => {
                        for child in is.iter_mut() {
                            stamp_inline(child, name, token_arc);
                        }
                    }
                    quarto_pandoc_types::custom::Slot::Block(b) => {
                        stamp_block(b, name, token_arc);
                    }
                    quarto_pandoc_types::custom::Slot::Blocks(bs) => {
                        for child in bs.iter_mut() {
                            stamp_block(child, name, token_arc);
                        }
                    }
                }
            }
        }
        // Leaves — no nested AST to walk.
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_)
        | Block::CaptionBlock(_) => {}
    }
}

/// Build the `SourceInfo` for a freshly-resolved shortcode node.
///
/// If the existing source_info is `Generated { by: filter, ... }` (a Lua
/// auto-attach from `filter_source_info`), promote the kind to `"shortcode"`
/// and migrate the `filter_path`/`line` data fields into `lua_path`/`lua_line`,
/// preserving the Lua-side dispatch precision alongside the new shortcode
/// context. Otherwise, mint a fresh `Generated { by: shortcode(name), ... }`.
///
/// In both branches, append an Invocation anchor pointing at the shortcode
/// token's source range (`token_arc`).
///
/// NOTE: the `filter_path`/`line` reads below are temporary. When
/// **bd-36fr9** (Lua-file registration in `SourceContext`) lands, those
/// fields move out of `by.data` and into a typed `Dispatch` anchor inside
/// `from`. This branch will then read the existing Dispatch anchor and copy
/// it alongside the Invocation.
///
/// NOTE: **bd-129m3** (ValueSource anchor stamping for `meta` / `var`
/// shortcodes) is the integration point for appending a second anchor
/// when the metadata loader threads per-key source-info through.
fn enrich_or_create(existing: &SourceInfo, name: &str, token_arc: &Arc<SourceInfo>) -> SourceInfo {
    let by = match existing {
        SourceInfo::Generated { by, .. } if by.kind == "filter" => {
            let lua_path = by.data.get("filter_path").cloned();
            let lua_line = by.data.get("line").cloned();
            let mut data = serde_json::json!({ "name": name });
            if let Some(p) = lua_path {
                data["lua_path"] = p;
            }
            if let Some(l) = lua_line {
                data["lua_line"] = l;
            }
            By {
                kind: "shortcode".to_string(),
                data,
            }
        }
        _ => By::shortcode(name),
    };
    SourceInfo::Generated {
        by,
        from: smallvec![Anchor::invocation(Arc::clone(token_arc))],
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
        .filter_map(|item| item.as_plain_text().map(|s| document_dir.join(s)))
        .collect()
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ShortcodeResolveTransform {
    fn name(&self) -> &str {
        "shortcode-resolve"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Collect diagnostics during traversal
        let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();

        // Create Lua engine on the stack if we have paths, extensions, or a runtime.
        // The engine is !Send + !Sync so it cannot be stored as a field.
        let mut lua_engine = if (!self.lua_shortcode_paths.is_empty()
            || !self.extensions.is_empty())
            && let Some(runtime) = self.runtime.as_ref()
        {
            let runtime = runtime.clone();
            match pampa::lua::LuaShortcodeEngine::new(&self.target_format, runtime) {
                Ok(mut engine) => {
                    // Load scripts from metadata-specified paths. These load
                    // before extension scripts, so a same-named extension
                    // handler overrides a document-level one (Q1 precedence).
                    for path in &self.lua_shortcode_paths {
                        if let Err(e) = engine.load_script(path).await {
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
                    Some(LuaEngineState {
                        engine,
                        extensions_loaded: false,
                    })
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
        )
        .await;

        // Extract Lua-registered data before the engine is dropped
        if let Some(state) = lua_engine.as_mut() {
            let engine = &mut state.engine;
            // Extract diagnostics from quarto.warn()/quarto.error()
            match engine.extract_diagnostics() {
                Ok(lua_diags) => diagnostics.extend(lua_diags),
                Err(e) => diagnostics.push(
                    DiagnosticMessageBuilder::warning("Lua extraction error")
                        .problem(format!("Failed to extract Lua diagnostics: {}", e))
                        .build(),
                ),
            }

            // Extract HTML dependencies and store as artifacts
            match engine.extract_html_dependencies() {
                Ok(deps) => {
                    if let Some(ref runtime) = self.runtime {
                        crate::dependency::store_html_dependencies(
                            deps,
                            &mut ctx.artifacts,
                            runtime.as_ref(),
                            &mut diagnostics,
                        );
                    }
                }
                Err(e) => diagnostics.push(
                    DiagnosticMessageBuilder::warning("Lua extraction error")
                        .problem(format!("Failed to extract HTML dependencies: {}", e))
                        .build(),
                ),
            }

            // Extract text includes and push onto context
            match engine.extract_text_includes() {
                Ok(includes) => {
                    crate::dependency::push_text_includes(includes, &mut ctx.includes);
                }
                Err(e) => diagnostics.push(
                    DiagnosticMessageBuilder::warning("Lua extraction error")
                        .problem(format!("Failed to extract text includes: {}", e))
                        .build(),
                ),
            }
        }

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
///
/// Returns a boxed future because this function is mutually recursive with
/// `resolve_block`, `resolve_inlines`, and `recurse_inline`.
fn resolve_blocks<'a>(
    blocks: &'a mut Vec<Block>,
    transform: &'a ShortcodeResolveTransform,
    metadata: &'a ConfigValue,
    diagnostics: &'a mut Vec<DiagnosticMessage>,
    lua_engine: &'a mut Option<LuaEngineState>,
) -> Pin<Box<dyn Future<Output = ()> + 'a>> {
    Box::pin(async move {
        let mut i = 0;
        while i < blocks.len() {
            // Check for block-context shortcode: Para/Plain with exactly one non-escaped Shortcode
            if let Some(shortcode) = single_shortcode_in_para_or_plain(&blocks[i]) {
                let shortcode_owned = shortcode.clone();
                let ctx = ShortcodeContext {
                    metadata,
                    source_info: &shortcode_owned.source_info,
                };
                match transform
                    .resolve_shortcode(
                        &shortcode_owned,
                        &ctx,
                        ResolutionContext::Block,
                        lua_engine,
                        diagnostics,
                    )
                    .await
                {
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
                        let error_inline =
                            make_error_inline(&error.key, &shortcode_owned.source_info);
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
            resolve_block(&mut blocks[i], transform, metadata, diagnostics, lua_engine).await;
            i += 1;
        }
    })
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
///
/// Returns a boxed future because this function is mutually recursive with
/// `resolve_blocks`, `resolve_inlines`, and `recurse_inline`.
fn resolve_block<'a>(
    block: &'a mut Block,
    transform: &'a ShortcodeResolveTransform,
    metadata: &'a ConfigValue,
    diagnostics: &'a mut Vec<DiagnosticMessage>,
    lua_engine: &'a mut Option<LuaEngineState>,
) -> Pin<Box<dyn Future<Output = ()> + 'a>> {
    Box::pin(async move {
        match block {
            Block::Plain(Plain { content, .. }) | Block::Paragraph(Paragraph { content, .. }) => {
                resolve_inlines(content, transform, metadata, diagnostics, lua_engine).await;
            }
            Block::LineBlock(LineBlock { content, .. }) => {
                for line in content {
                    resolve_inlines(line, transform, metadata, diagnostics, lua_engine).await;
                }
            }
            Block::Header(Header { content, .. }) => {
                resolve_inlines(content, transform, metadata, diagnostics, lua_engine).await;
            }
            Block::BlockQuote(BlockQuote { content, .. }) => {
                resolve_blocks(content, transform, metadata, diagnostics, lua_engine).await;
            }
            Block::OrderedList(OrderedList { content, .. }) => {
                for item in content {
                    resolve_blocks(item, transform, metadata, diagnostics, lua_engine).await;
                }
            }
            Block::BulletList(BulletList { content, .. }) => {
                for item in content {
                    resolve_blocks(item, transform, metadata, diagnostics, lua_engine).await;
                }
            }
            Block::DefinitionList(DefinitionList { content, .. }) => {
                for (term, defs) in content {
                    resolve_inlines(term, transform, metadata, diagnostics, lua_engine).await;
                    for def in defs {
                        resolve_blocks(def, transform, metadata, diagnostics, lua_engine).await;
                    }
                }
            }
            Block::Figure(Figure {
                content, caption, ..
            }) => {
                resolve_blocks(content, transform, metadata, diagnostics, lua_engine).await;
                if let Some(short) = &mut caption.short {
                    resolve_inlines(short, transform, metadata, diagnostics, lua_engine).await;
                }
                if let Some(long) = &mut caption.long {
                    resolve_blocks(long, transform, metadata, diagnostics, lua_engine).await;
                }
            }
            Block::Div(Div { content, .. }) => {
                resolve_blocks(content, transform, metadata, diagnostics, lua_engine).await;
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
                    resolve_inlines(short, transform, metadata, diagnostics, lua_engine).await;
                }
                if let Some(long) = &mut caption.long {
                    resolve_blocks(long, transform, metadata, diagnostics, lua_engine).await;
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
                        )
                        .await;
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
                            )
                            .await;
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
                        )
                        .await;
                    }
                }
            }
            Block::Custom(custom) => {
                // Resolve shortcodes in custom node slots
                for slot in custom.slots.values_mut() {
                    match slot {
                        quarto_pandoc_types::custom::Slot::Block(b) => {
                            resolve_block(b, transform, metadata, diagnostics, lua_engine).await;
                        }
                        quarto_pandoc_types::custom::Slot::Blocks(bs) => {
                            resolve_blocks(bs, transform, metadata, diagnostics, lua_engine).await;
                        }
                        quarto_pandoc_types::custom::Slot::Inline(i) => {
                            let mut inlines = vec![i.as_ref().clone()];
                            resolve_inlines(
                                &mut inlines,
                                transform,
                                metadata,
                                diagnostics,
                                lua_engine,
                            )
                            .await;
                            if inlines.len() == 1 {
                                **i = inlines.pop().unwrap();
                            }
                            // If resolution produced multiple inlines, we can't put them
                            // back into a single Inline slot - keep the original
                        }
                        quarto_pandoc_types::custom::Slot::Inlines(is) => {
                            resolve_inlines(is, transform, metadata, diagnostics, lua_engine).await;
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
    })
}

/// Resolve shortcodes in a vector of inlines.
///
/// Returns a boxed future because this function is mutually recursive with
/// `resolve_blocks`, `resolve_block`, and `recurse_inline`.
fn resolve_inlines<'a>(
    inlines: &'a mut Vec<Inline>,
    transform: &'a ShortcodeResolveTransform,
    metadata: &'a ConfigValue,
    diagnostics: &'a mut Vec<DiagnosticMessage>,
    lua_engine: &'a mut Option<LuaEngineState>,
) -> Pin<Box<dyn Future<Output = ()> + 'a>> {
    Box::pin(async move {
        let mut i = 0;
        while i < inlines.len() {
            if let Inline::Shortcode(shortcode) = &inlines[i] {
                let shortcode_owned = shortcode.clone();
                let shortcode_ctx = ShortcodeContext {
                    metadata,
                    source_info: &shortcode_owned.source_info,
                };

                match transform
                    .resolve_shortcode(
                        &shortcode_owned,
                        &shortcode_ctx,
                        ResolutionContext::Inline,
                        lua_engine,
                        diagnostics,
                    )
                    .await
                {
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
                        let replacement =
                            flatten_blocks_to_inlines(&blocks, &shortcode_owned.source_info);
                        let replacement_len = replacement.len();
                        inlines.splice(i..=i, replacement);
                        i += replacement_len.max(1);
                    }
                    ShortcodeResult::Error(error) => {
                        // Emit diagnostic
                        diagnostics.push(error.diagnostic);
                        // Replace with visible error (TS Quarto style)
                        let error_inline =
                            make_error_inline(&error.key, &shortcode_owned.source_info);
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
                )
                .await;
                i += 1;
            }
        }
    })
}

/// Recurse into an inline element to resolve nested shortcodes.
///
/// Returns a boxed future because this function is mutually recursive with
/// `resolve_blocks`, `resolve_block`, and `resolve_inlines`.
fn recurse_inline<'a>(
    inline: &'a mut Inline,
    transform: &'a ShortcodeResolveTransform,
    metadata: &'a ConfigValue,
    diagnostics: &'a mut Vec<DiagnosticMessage>,
    lua_engine: &'a mut Option<LuaEngineState>,
) -> Pin<Box<dyn Future<Output = ()> + 'a>> {
    Box::pin(async move {
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
                resolve_inlines(content, transform, metadata, diagnostics, lua_engine).await;
            }
            Inline::Quoted(Quoted { content, .. }) => {
                resolve_inlines(content, transform, metadata, diagnostics, lua_engine).await;
            }
            Inline::Cite(Cite { content, .. }) => {
                resolve_inlines(content, transform, metadata, diagnostics, lua_engine).await;
            }
            Inline::Link(Link { content, .. }) | Inline::Image(Image { content, .. }) => {
                resolve_inlines(content, transform, metadata, diagnostics, lua_engine).await;
            }
            Inline::Note(Note { content, .. }) => {
                resolve_blocks(content, transform, metadata, diagnostics, lua_engine).await;
            }
            Inline::Span(Span { content, .. }) => {
                resolve_inlines(content, transform, metadata, diagnostics, lua_engine).await;
            }
            Inline::EditComment(EditComment { content, .. }) => {
                resolve_inlines(content, transform, metadata, diagnostics, lua_engine).await;
            }
            Inline::Custom(custom) => {
                // Resolve shortcodes in custom inline node slots
                for slot in custom.slots.values_mut() {
                    match slot {
                        quarto_pandoc_types::custom::Slot::Inlines(is) => {
                            resolve_inlines(is, transform, metadata, diagnostics, lua_engine).await;
                        }
                        quarto_pandoc_types::custom::Slot::Inline(i) => {
                            let mut inlines = vec![i.as_ref().clone()];
                            resolve_inlines(
                                &mut inlines,
                                transform,
                                metadata,
                                diagnostics,
                                lua_engine,
                            )
                            .await;
                            if inlines.len() == 1 {
                                **i = inlines.pop().unwrap();
                            }
                        }
                        quarto_pandoc_types::custom::Slot::Blocks(bs) => {
                            resolve_blocks(bs, transform, metadata, diagnostics, lua_engine).await;
                        }
                        quarto_pandoc_types::custom::Slot::Block(b) => {
                            resolve_block(b, transform, metadata, diagnostics, lua_engine).await;
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
            | Inline::Attr(_) => {}
        }
    })
}

/// Create visible error inline: Strong("?key")
///
/// Both the inner Str and outer Strong carry the shortcode token's original
/// `source_info` (not `Generated`). The error region is treated as normal
/// editable user-source content — Plan 7's `is_atomic_kind()` does not fire on
/// Original, so the incremental writer Verbatim-copies the original token
/// bytes on round-trip. The Strong-wraps-Str overlap is structurally parallel
/// to the footnote `<sup>` case (Plan 7 §footnotes).
fn make_error_inline(key: &str, token_source_info: &SourceInfo) -> Inline {
    Inline::Strong(Strong {
        content: vec![Inline::Str(Str {
            text: format!("?{}", key),
            source_info: token_source_info.clone(),
        })],
        source_info: token_source_info.clone(),
    })
}

/// Convert an escaped shortcode to literal text.
///
/// For `{{{< meta title >}}}`, this produces `{{< meta title >}}`. The
/// resulting `Str` carries the shortcode token's original `source_info`
/// (an Original), so Plan 7's `is_atomic_kind()` does not fire — round-trip
/// through the incremental writer verbatim-copies the source bytes.
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
        source_info: shortcode.source_info.clone(),
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
            keyword_args: hashlink::LinkedHashMap::new(),
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

    #[tokio::test]
    async fn test_resolve_escaped_shortcode() {
        let transform = ShortcodeResolveTransform::new();

        let shortcode = Shortcode {
            is_escaped: true,
            name: "meta".to_string(),
            positional_args: vec![ShortcodeArg::String("title".to_string())],
            keyword_args: hashlink::LinkedHashMap::new(),
            source_info: dummy_source_info(),
        };

        let ctx = ShortcodeContext {
            metadata: &ConfigValue::default(),
            source_info: &shortcode.source_info,
        };

        let result = transform
            .resolve_shortcode(
                &shortcode,
                &ctx,
                ResolutionContext::Inline,
                &mut None,
                &mut Vec::new(),
            )
            .await;
        assert!(matches!(result, ShortcodeResult::Preserve));
    }

    #[tokio::test]
    async fn test_resolve_unknown_shortcode() {
        let transform = ShortcodeResolveTransform::new();

        let shortcode = make_shortcode("unknown", vec![]);

        let ctx = ShortcodeContext {
            metadata: &ConfigValue::default(),
            source_info: &shortcode.source_info,
        };

        let result = transform
            .resolve_shortcode(
                &shortcode,
                &ctx,
                ResolutionContext::Inline,
                &mut None,
                &mut Vec::new(),
            )
            .await;
        match result {
            ShortcodeResult::Error(err) => {
                assert_eq!(err.key, "unknown");
            }
            _ => panic!("Expected Error result"),
        }
    }

    #[test]
    fn test_make_error_inline() {
        let token_si = dummy_source_info();
        let inline = make_error_inline("meta:title", &token_si);
        match inline {
            Inline::Strong(strong) => {
                assert_eq!(strong.content.len(), 1);
                // Both layers carry the token's source_info (not Default, not Generated).
                assert_eq!(&strong.source_info, &token_si);
                if let Inline::Str(s) = &strong.content[0] {
                    assert_eq!(s.text, "?meta:title");
                    assert_eq!(&s.source_info, &token_si);
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

    #[tokio::test]
    async fn test_full_transform() {
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

        transform.transform(&mut ast, &mut ctx).await.unwrap();

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

    #[tokio::test]
    async fn test_full_transform_with_error() {
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

        transform.transform(&mut ast, &mut ctx).await.unwrap();

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
                        source_info: SourceInfo::for_test(),
                    },
                )]),
                ResolutionContext::Inline => ShortcodeResult::Inlines(vec![Inline::Str(Str {
                    text: "inline-fallback".to_string(),
                    source_info: SourceInfo::for_test(),
                })]),
            }
        }
    }

    #[tokio::test]
    async fn test_block_shortcode_replaces_para() {
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

        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // The Para should be replaced by a HorizontalRule
        assert_eq!(ast.blocks.len(), 1);
        assert!(
            matches!(&ast.blocks[0], Block::HorizontalRule(_)),
            "Expected HorizontalRule, got {:?}",
            ast.blocks[0]
        );
    }

    #[tokio::test]
    async fn test_inline_shortcode_in_para_stays_inline() {
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

        transform.transform(&mut ast, &mut ctx).await.unwrap();

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
                    source_info: SourceInfo::for_test(),
                })],
                source_info: SourceInfo::for_test(),
            })])
        }
    }

    #[tokio::test]
    async fn test_block_result_in_inline_context() {
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

        transform.transform(&mut ast, &mut ctx).await.unwrap();

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

    #[tokio::test]
    async fn test_escaped_shortcode_block_context() {
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
                    keyword_args: hashlink::LinkedHashMap::new(),
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

        transform.transform(&mut ast, &mut ctx).await.unwrap();

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

        #[tokio::test]
        async fn test_lua_shortcode_from_metadata_paths() {
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

            transform.transform(&mut ast, &mut ctx).await.unwrap();

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

        #[tokio::test]
        async fn test_lua_shortcode_by_extension_name() {
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

            transform.transform(&mut ast, &mut ctx).await.unwrap();

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

        #[tokio::test]
        async fn test_rust_handler_overrides_lua() {
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

            transform.transform(&mut ast, &mut ctx).await.unwrap();

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

        #[tokio::test]
        async fn test_unknown_shortcode_error() {
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

            transform.transform(&mut ast, &mut ctx).await.unwrap();

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

        #[tokio::test]
        async fn test_extension_shortcode_block_context() {
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

            transform.transform(&mut ast, &mut ctx).await.unwrap();

            // Para should be replaced by HorizontalRule
            assert_eq!(ast.blocks.len(), 1);
            assert!(
                matches!(&ast.blocks[0], Block::HorizontalRule(_)),
                "Expected HorizontalRule, got {:?}",
                ast.blocks[0]
            );
            assert!(ctx.diagnostics.is_empty());
        }

        #[tokio::test]
        async fn test_full_transform_block_shortcode_rawblock() {
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

            transform.transform(&mut ast, &mut ctx).await.unwrap();

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

        /// Plan 6 §"Lua-shortcode enrichment": when a Lua handler returns a
        /// *typed* Inline (e.g. `pandoc.Str(...)`), the filter_source_info
        /// auto-attach gives it `Generated { by: filter, data: { filter_path,
        /// line } }`. The resolver's post-walk should then promote this to
        /// `Generated { by: shortcode, data: { name, lua_path, lua_line },
        /// from: [Invocation] }` — kind promoted, fields renamed, anchor
        /// appended.
        #[tokio::test]
        async fn lua_shortcode_typed_return_enriched_to_shortcode_kind() {
            let tmp = TempDir::new().unwrap();
            // Note: pandoc.Str(...) returns a typed Lua userdata that the
            // Lua engine's filter_source_info auto-attach picks up.
            let script_path = write_lua_script(
                tmp.path(),
                "typed.lua",
                r#"return { typed = function(args) return pandoc.Str("Hello typed") end }"#,
            );

            let runtime = make_runtime();
            let transform = ShortcodeResolveTransform::with_lua_support(
                vec![script_path.clone()],
                Vec::new(),
                runtime,
                "html".to_string(),
            );

            let tok = token_si();
            let mut ast = Pandoc {
                meta: ConfigValue::default(),
                blocks: vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Shortcode(make_shortcode_with_si(
                        "typed",
                        vec![],
                        tok.clone(),
                    ))],
                    source_info: dummy_source_info(),
                })],
            };

            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
            transform.transform(&mut ast, &mut ctx).await.unwrap();

            let Block::Paragraph(para) = &ast.blocks[0] else {
                panic!("Expected Paragraph");
            };
            let Inline::Str(s) = &para.content[0] else {
                panic!("Expected resolved Str, got {:?}", &para.content[0]);
            };
            assert_eq!(s.text, "Hello typed");
            match &s.source_info {
                SourceInfo::Generated { by, from } => {
                    // Kind promoted to "shortcode", NOT "filter".
                    assert_eq!(
                        by.kind, "shortcode",
                        "kind should be promoted from filter to shortcode"
                    );
                    // Name is the shortcode name.
                    assert_eq!(by.data.get("name").and_then(|v| v.as_str()), Some("typed"));
                    // filter_path → lua_path
                    let lua_path = by
                        .data
                        .get("lua_path")
                        .and_then(|v| v.as_str())
                        .expect("lua_path should be preserved from filter_path");
                    assert!(
                        lua_path.contains("typed.lua"),
                        "lua_path {:?} should reference the script",
                        lua_path
                    );
                    // line → lua_line
                    let lua_line = by
                        .data
                        .get("lua_line")
                        .and_then(|v| v.as_u64())
                        .expect("lua_line should be preserved from line");
                    assert!(lua_line >= 1, "lua_line should be positive");
                    // Invocation anchor points at the token.
                    assert_eq!(from.len(), 1);
                    assert_eq!(from[0].role, quarto_source_map::AnchorRole::Invocation);
                    assert_eq!(&*from[0].source_info, &tok);
                }
                other => panic!("Expected Generated, got {:?}", other),
            }
        }
    }

    // === Plan 6: shortcode-resolution provenance shape tests ===

    /// A test handler that returns a Strong wrapping a Str — exercises
    /// the multi-inline / nested-container stamping path.
    struct MultiInlineTestHandler;
    impl ShortcodeHandler for MultiInlineTestHandler {
        fn name(&self) -> &str {
            "multi"
        }
        fn resolve(
            &self,
            _shortcode: &Shortcode,
            _ctx: &ShortcodeContext,
            _resolution_ctx: ResolutionContext,
        ) -> ShortcodeResult {
            ShortcodeResult::Inlines(vec![
                Inline::Strong(Strong {
                    content: vec![Inline::Str(Str {
                        text: "Bold".into(),
                        source_info: SourceInfo::for_test(),
                    })],
                    source_info: SourceInfo::for_test(),
                }),
                Inline::Space(quarto_pandoc_types::inline::Space {
                    source_info: SourceInfo::for_test(),
                }),
                Inline::Str(Str {
                    text: "Title".into(),
                    source_info: SourceInfo::for_test(),
                }),
            ])
        }
    }

    /// Distinct token source_info so we can check Invocation anchors
    /// point at the *shortcode token*, not at the default.
    fn token_si() -> SourceInfo {
        SourceInfo::original(FileId(0), 100, 130)
    }

    fn make_shortcode_with_si(name: &str, args: Vec<&str>, si: SourceInfo) -> Shortcode {
        Shortcode {
            is_escaped: false,
            name: name.to_string(),
            positional_args: args
                .into_iter()
                .map(|s| ShortcodeArg::String(s.to_string()))
                .collect(),
            keyword_args: hashlink::LinkedHashMap::new(),
            source_info: si,
        }
    }

    fn make_escaped_shortcode_with_si(name: &str, si: SourceInfo) -> Shortcode {
        Shortcode {
            is_escaped: true,
            name: name.to_string(),
            positional_args: vec![],
            keyword_args: hashlink::LinkedHashMap::new(),
            source_info: si,
        }
    }

    /// Resolved Str from a meta shortcode carries
    /// Generated { by: shortcode("meta"), from: [Invocation -> token_si] }.
    #[tokio::test]
    async fn shortcode_resolution_has_generated_with_invocation_anchor() {
        let transform = ShortcodeResolveTransform::new();
        let tok = token_si();
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![make_map_entry(
                    "title",
                    ConfigValue::new_string("Test Title", dummy_source_info()),
                )],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Shortcode(make_shortcode_with_si(
                    "meta",
                    vec!["title"],
                    tok.clone(),
                ))],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        let Block::Paragraph(para) = &ast.blocks[0] else {
            panic!("Expected Paragraph");
        };
        let Inline::Str(s) = &para.content[0] else {
            panic!("Expected resolved Str");
        };
        assert_eq!(s.text, "Test Title");
        match &s.source_info {
            SourceInfo::Generated { by, from } => {
                assert_eq!(by.kind, "shortcode");
                assert_eq!(by.data.get("name").and_then(|v| v.as_str()), Some("meta"));
                assert_eq!(from.len(), 1);
                assert_eq!(from[0].role, quarto_source_map::AnchorRole::Invocation);
                assert_eq!(&*from[0].source_info, &tok);
            }
            other => panic!("Expected Generated, got {:?}", other),
        }
    }

    /// Multi-inline resolution (Strong[Str], Space, Str) — every node gets
    /// stamped with the same Invocation anchor source_info.
    #[tokio::test]
    async fn multi_inline_shortcode_resolution_shares_invocation_source() {
        let mut transform = ShortcodeResolveTransform::new();
        transform.handlers.push(Box::new(MultiInlineTestHandler));
        let tok = token_si();
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Shortcode(make_shortcode_with_si(
                    "multi",
                    vec![],
                    tok.clone(),
                ))],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        let Block::Paragraph(para) = &ast.blocks[0] else {
            panic!("Expected Paragraph");
        };
        assert_eq!(para.content.len(), 3);

        // Helper: extract the Invocation source_info from an inline.
        fn invocation_si(inline: &Inline) -> &SourceInfo {
            match inline.source_info() {
                SourceInfo::Generated { by, from } => {
                    assert_eq!(by.kind, "shortcode", "Got by.kind = {:?}", by.kind);
                    assert_eq!(from.len(), 1);
                    assert_eq!(from[0].role, quarto_source_map::AnchorRole::Invocation);
                    &from[0].source_info
                }
                other => panic!("Expected Generated, got {:?}", other),
            }
        }

        let strong_si = invocation_si(&para.content[0]);
        let space_si = invocation_si(&para.content[1]);
        let str_si = invocation_si(&para.content[2]);
        assert_eq!(strong_si, &tok);
        assert_eq!(space_si, &tok);
        assert_eq!(str_si, &tok);
        // The Strong's inner Str must also be stamped.
        let Inline::Strong(strong) = &para.content[0] else {
            panic!("Expected Strong");
        };
        let inner_si = invocation_si(&strong.content[0]);
        assert_eq!(inner_si, &tok);
    }

    /// Escaped shortcode resolves to a literal Str whose source_info is
    /// the token's Original (NOT Generated) — Plan 7's is_atomic_kind()
    /// does not fire on round-trip.
    #[tokio::test]
    async fn escaped_shortcode_keeps_original_source_info() {
        let transform = ShortcodeResolveTransform::new();
        let tok = token_si();
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Shortcode(make_escaped_shortcode_with_si(
                    "meta",
                    tok.clone(),
                ))],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        let Block::Paragraph(para) = &ast.blocks[0] else {
            panic!("Expected Paragraph");
        };
        let Inline::Str(s) = &para.content[0] else {
            panic!("Expected literal Str");
        };
        // Source_info is Original (the token's bytes), not Generated.
        match &s.source_info {
            SourceInfo::Original { .. } => {}
            other => panic!("Expected Original, got {:?}", other),
        }
        assert_eq!(&s.source_info, &tok);
    }

    /// Unknown shortcode resolves to Strong[Str("?name")] with both
    /// layers carrying the token's Original source_info (NOT Generated,
    /// NOT Default).
    #[tokio::test]
    async fn unknown_shortcode_error_uses_token_source_info() {
        let transform = ShortcodeResolveTransform::new();
        let tok = token_si();
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Shortcode(make_shortcode_with_si(
                    "bogus",
                    vec![],
                    tok.clone(),
                ))],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        let Block::Paragraph(para) = &ast.blocks[0] else {
            panic!("Expected Paragraph");
        };
        let Inline::Strong(strong) = &para.content[0] else {
            panic!("Expected Strong");
        };
        assert!(matches!(strong.source_info, SourceInfo::Original { .. }));
        assert_eq!(&strong.source_info, &tok);
        let Inline::Str(inner) = &strong.content[0] else {
            panic!("Expected inner Str");
        };
        assert!(matches!(inner.source_info, SourceInfo::Original { .. }));
        assert_eq!(&inner.source_info, &tok);
        assert_eq!(inner.text, "?bogus");
    }

    /// Plan 6 source_info-determinism: running the transform twice on
    /// the same input produces structurally-identical ASTs (every
    /// Generated.by, every Generated.from[], and every Original
    /// SourceInfo is ==-equal across runs).
    #[tokio::test]
    async fn shortcode_resolution_is_deterministic() {
        async fn run_once() -> Pandoc {
            let mut transform = ShortcodeResolveTransform::new();
            transform.handlers.push(Box::new(MultiInlineTestHandler));
            let tok = token_si();
            let mut ast = Pandoc {
                meta: ConfigValue::new_map(
                    vec![make_map_entry(
                        "title",
                        ConfigValue::new_string("Title", dummy_source_info()),
                    )],
                    dummy_source_info(),
                ),
                blocks: vec![Block::Paragraph(Paragraph {
                    content: vec![
                        Inline::Shortcode(make_shortcode_with_si(
                            "meta",
                            vec!["title"],
                            tok.clone(),
                        )),
                        Inline::Shortcode(make_shortcode_with_si("multi", vec![], tok)),
                    ],
                    source_info: dummy_source_info(),
                })],
            };
            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
            transform.transform(&mut ast, &mut ctx).await.unwrap();
            ast
        }

        let a = run_once().await;
        let b = run_once().await;
        // Pandoc, Block, Inline, and SourceInfo all derive PartialEq —
        // == compares structurally, including every Generated.by /
        // Generated.from[] and every Original byte range.
        assert_eq!(a, b, "Plan-6 stamper must be deterministic across runs");
    }

    /// Audit-completion test: after Plan 6's stamping pass, the AST
    /// should contain no `Generated { by: shortcode, from: [] }` nodes
    /// (the required-anchor invariant: every shortcode-resolved node
    /// carries an Invocation anchor).
    #[tokio::test]
    async fn shortcode_resolution_required_anchor_invariant() {
        let mut transform = ShortcodeResolveTransform::new();
        transform.handlers.push(Box::new(MultiInlineTestHandler));
        let tok = token_si();
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![make_map_entry(
                    "title",
                    ConfigValue::new_string("Title", dummy_source_info()),
                )],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![
                    Inline::Shortcode(make_shortcode_with_si("meta", vec!["title"], tok.clone())),
                    Inline::Shortcode(make_shortcode_with_si("multi", vec![], tok.clone())),
                ],
                source_info: dummy_source_info(),
            })],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Walk every inline in the AST and assert: any
        // Generated{by.kind=="shortcode"} carries at least one Invocation.
        fn check_inline(inline: &Inline) {
            if let SourceInfo::Generated { by, from } = inline.source_info()
                && by.kind == "shortcode"
            {
                assert!(
                    from.iter()
                        .any(|a| a.role == quarto_source_map::AnchorRole::Invocation),
                    "Generated{{by:shortcode}} missing Invocation anchor"
                );
            }
            // Recurse into children for the common containers exercised here.
            if let Inline::Strong(s) = inline {
                for c in &s.content {
                    check_inline(c);
                }
            }
        }

        for block in &ast.blocks {
            if let Block::Paragraph(p) = block {
                for inline in &p.content {
                    check_inline(inline);
                }
            }
        }
    }
}
