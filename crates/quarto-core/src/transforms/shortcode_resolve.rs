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

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

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

/// Trait for shortcode handlers.
///
/// Each built-in shortcode (meta, var, env, etc.) implements this trait.
pub trait ShortcodeHandler: Send + Sync {
    /// The shortcode name (e.g., "meta", "var", "env")
    fn name(&self) -> &str;

    /// Resolve the shortcode to content.
    fn resolve(&self, shortcode: &Shortcode, ctx: &ShortcodeContext) -> ShortcodeResult;
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

    fn resolve(&self, shortcode: &Shortcode, ctx: &ShortcodeContext) -> ShortcodeResult {
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

        // Look up value in metadata (supports dot notation)
        match get_nested_metadata(ctx.metadata, &key) {
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

/// Navigate nested metadata using dot notation.
///
/// # Example
///
/// For metadata `{ author: { name: "Alice" } }`:
/// - `get_nested_metadata(meta, "author.name")` returns `Some("Alice")`
/// - `get_nested_metadata(meta, "author.email")` returns `None`
fn get_nested_metadata<'a>(meta: &'a ConfigValue, key: &str) -> Option<&'a ConfigValue> {
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = meta;

    for part in parts {
        match &current.value {
            ConfigValueKind::Map(entries) => {
                // Find the entry with matching key
                let found = entries.iter().find(|e| e.key == part);
                match found {
                    Some(entry) => current = &entry.value,
                    None => return None,
                }
            }
            _ => return None,
        }
    }

    Some(current)
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
pub struct ShortcodeResolveTransform {
    handlers: Vec<Box<dyn ShortcodeHandler>>,
}

impl ShortcodeResolveTransform {
    /// Create a new shortcode resolve transform with default handlers.
    pub fn new() -> Self {
        Self {
            handlers: vec![Box::new(MetaShortcodeHandler)],
        }
    }

    /// Resolve a shortcode using the appropriate handler.
    fn resolve_shortcode(&self, shortcode: &Shortcode, ctx: &ShortcodeContext) -> ShortcodeResult {
        // Handle escaped shortcodes - preserve as literal text
        if shortcode.is_escaped {
            return ShortcodeResult::Preserve;
        }

        // Find and call handler
        for handler in &self.handlers {
            if handler.name() == shortcode.name {
                return handler.resolve(shortcode, ctx);
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

impl AstTransform for ShortcodeResolveTransform {
    fn name(&self) -> &str {
        "shortcode-resolve"
    }

    fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Collect diagnostics during traversal
        let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();

        // Resolve shortcodes in all blocks
        resolve_blocks(&mut ast.blocks, self, &ast.meta, &mut diagnostics);

        // Add any diagnostics to the render context
        for diagnostic in diagnostics {
            ctx.add_warning(diagnostic);
        }

        Ok(())
    }
}

/// Resolve shortcodes in a vector of blocks.
fn resolve_blocks(
    blocks: &mut Vec<Block>,
    transform: &ShortcodeResolveTransform,
    metadata: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    for block in blocks.iter_mut() {
        resolve_block(block, transform, metadata, diagnostics);
    }
}

/// Resolve shortcodes in a single block.
fn resolve_block(
    block: &mut Block,
    transform: &ShortcodeResolveTransform,
    metadata: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    match block {
        Block::Plain(Plain { content, .. }) | Block::Paragraph(Paragraph { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics);
        }
        Block::LineBlock(LineBlock { content, .. }) => {
            for line in content {
                resolve_inlines(line, transform, metadata, diagnostics);
            }
        }
        Block::Header(Header { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics);
        }
        Block::BlockQuote(BlockQuote { content, .. }) => {
            resolve_blocks(content, transform, metadata, diagnostics);
        }
        Block::OrderedList(OrderedList { content, .. }) => {
            for item in content {
                resolve_blocks(item, transform, metadata, diagnostics);
            }
        }
        Block::BulletList(BulletList { content, .. }) => {
            for item in content {
                resolve_blocks(item, transform, metadata, diagnostics);
            }
        }
        Block::DefinitionList(DefinitionList { content, .. }) => {
            for (term, defs) in content {
                resolve_inlines(term, transform, metadata, diagnostics);
                for def in defs {
                    resolve_blocks(def, transform, metadata, diagnostics);
                }
            }
        }
        Block::Figure(Figure {
            content, caption, ..
        }) => {
            resolve_blocks(content, transform, metadata, diagnostics);
            if let Some(short) = &mut caption.short {
                resolve_inlines(short, transform, metadata, diagnostics);
            }
            if let Some(long) = &mut caption.long {
                resolve_blocks(long, transform, metadata, diagnostics);
            }
        }
        Block::Div(Div { content, .. }) => {
            resolve_blocks(content, transform, metadata, diagnostics);
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
                resolve_inlines(short, transform, metadata, diagnostics);
            }
            if let Some(long) = &mut caption.long {
                resolve_blocks(long, transform, metadata, diagnostics);
            }
            // Table head
            for row in &mut head.rows {
                for cell in &mut row.cells {
                    resolve_blocks(&mut cell.content, transform, metadata, diagnostics);
                }
            }
            // Table bodies
            for body in bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        resolve_blocks(&mut cell.content, transform, metadata, diagnostics);
                    }
                }
            }
            // Table foot
            for row in &mut foot.rows {
                for cell in &mut row.cells {
                    resolve_blocks(&mut cell.content, transform, metadata, diagnostics);
                }
            }
        }
        Block::Custom(custom) => {
            // Resolve shortcodes in custom node slots
            for slot in custom.slots.values_mut() {
                match slot {
                    quarto_pandoc_types::custom::Slot::Block(b) => {
                        resolve_block(b, transform, metadata, diagnostics);
                    }
                    quarto_pandoc_types::custom::Slot::Blocks(bs) => {
                        resolve_blocks(bs, transform, metadata, diagnostics);
                    }
                    quarto_pandoc_types::custom::Slot::Inline(i) => {
                        let mut inlines = vec![i.as_ref().clone()];
                        resolve_inlines(&mut inlines, transform, metadata, diagnostics);
                        if inlines.len() == 1 {
                            **i = inlines.pop().unwrap();
                        }
                        // If resolution produced multiple inlines, we can't put them
                        // back into a single Inline slot - keep the original
                    }
                    quarto_pandoc_types::custom::Slot::Inlines(is) => {
                        resolve_inlines(is, transform, metadata, diagnostics);
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
) {
    let mut i = 0;
    while i < inlines.len() {
        if let Inline::Shortcode(shortcode) = &inlines[i] {
            let shortcode_ctx = ShortcodeContext {
                metadata,
                source_info: &shortcode.source_info,
            };

            match transform.resolve_shortcode(shortcode, &shortcode_ctx) {
                ShortcodeResult::Inlines(replacement) => {
                    // Replace shortcode with resolved inlines
                    let replacement_len = replacement.len();
                    inlines.splice(i..=i, replacement);
                    // Advance past the replacement (they shouldn't contain shortcodes,
                    // but even if they do, we don't want infinite loops)
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
                    let literal = shortcode_to_literal(shortcode);
                    inlines[i] = literal;
                    i += 1;
                }
            }
        } else {
            // Recurse into inline containers
            recurse_inline(&mut inlines[i], transform, metadata, diagnostics);
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
            resolve_inlines(content, transform, metadata, diagnostics);
        }
        Inline::Quoted(Quoted { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics);
        }
        Inline::Cite(Cite { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics);
        }
        Inline::Link(Link { content, .. }) | Inline::Image(Image { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics);
        }
        Inline::Note(Note { content, .. }) => {
            resolve_blocks(content, transform, metadata, diagnostics);
        }
        Inline::Span(Span { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics);
        }
        Inline::EditComment(EditComment { content, .. }) => {
            resolve_inlines(content, transform, metadata, diagnostics);
        }
        Inline::Custom(custom) => {
            // Resolve shortcodes in custom inline node slots
            for slot in custom.slots.values_mut() {
                match slot {
                    quarto_pandoc_types::custom::Slot::Inlines(is) => {
                        resolve_inlines(is, transform, metadata, diagnostics);
                    }
                    quarto_pandoc_types::custom::Slot::Inline(i) => {
                        let mut inlines = vec![i.as_ref().clone()];
                        resolve_inlines(&mut inlines, transform, metadata, diagnostics);
                        if inlines.len() == 1 {
                            **i = inlines.pop().unwrap();
                        }
                    }
                    quarto_pandoc_types::custom::Slot::Blocks(bs) => {
                        resolve_blocks(bs, transform, metadata, diagnostics);
                    }
                    quarto_pandoc_types::custom::Slot::Block(b) => {
                        resolve_block(b, transform, metadata, diagnostics);
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
    use crate::project::{DocumentInfo, ProjectContext};
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
            config: None,
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

    #[test]
    fn test_get_nested_metadata_simple() {
        let meta = ConfigValue::new_map(
            vec![make_map_entry(
                "title",
                ConfigValue::new_string("My Doc", dummy_source_info()),
            )],
            dummy_source_info(),
        );

        let result = get_nested_metadata(&meta, "title");
        assert!(result.is_some());
        if let Some(cv) = result {
            assert_eq!(cv.as_str(), Some("My Doc"));
        }
    }

    #[test]
    fn test_get_nested_metadata_dot_notation() {
        let author_map = ConfigValue::new_map(
            vec![make_map_entry(
                "name",
                ConfigValue::new_string("Alice", dummy_source_info()),
            )],
            dummy_source_info(),
        );
        let meta = ConfigValue::new_map(
            vec![make_map_entry("author", author_map)],
            dummy_source_info(),
        );

        let result = get_nested_metadata(&meta, "author.name");
        assert!(result.is_some());
        if let Some(cv) = result {
            assert_eq!(cv.as_str(), Some("Alice"));
        }
    }

    #[test]
    fn test_get_nested_metadata_missing() {
        let meta = ConfigValue::new_map(
            vec![make_map_entry(
                "title",
                ConfigValue::new_string("My Doc", dummy_source_info()),
            )],
            dummy_source_info(),
        );

        let result = get_nested_metadata(&meta, "author");
        assert!(result.is_none());
    }

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

        let result = handler.resolve(&shortcode, &ctx);
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

        let result = handler.resolve(&shortcode, &ctx);
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

        let result = handler.resolve(&shortcode, &ctx);
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

        let result = transform.resolve_shortcode(&shortcode, &ctx);
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

        let result = transform.resolve_shortcode(&shortcode, &ctx);
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
        assert!(ctx.warnings.is_empty());
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
        assert_eq!(ctx.warnings.len(), 1);
    }
}
