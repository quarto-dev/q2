/*
 * plaintext.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Pure plain-text writer for Pandoc AST.
//!
//! This writer produces plain text with no HTML escaping or markup.
//! It's used for generating metadata values that will appear in
//! plain-text contexts (HTML `<title>`, meta tags, etc.)
//!
//! # Design decisions
//!
//! - RawInline/RawBlock: echo contents if format is "plaintext", otherwise drop with warning
//! - Unsupported nodes emit diagnostic warnings and are dropped
//! - No HTML escaping (unlike `write_inlines_as_text` in html.rs)
//! - Block structure mimics markdown writer for lists, code blocks, blockquotes, line blocks
//! - Inline structure is stripped (no `*`, `_`, `^`, `~`, `[]`, `![]` markers)

use crate::pandoc::block::{
    Block, BlockQuote, BulletList, CaptionBlock, CodeBlock, DefinitionList, Div, Figure, Header,
    HorizontalRule, LineBlock, MetaBlock, NoteDefinitionFencedBlock, NoteDefinitionPara,
    OrderedList, Paragraph, Plain, RawBlock,
};
use crate::pandoc::inline::{
    Cite, Code, Delete, EditComment, Emph, Highlight, Image, Inline, Inlines, Insert, Link, Math,
    NoteReference, QuoteType, Quoted, RawInline, SmallCaps, Span, Strikeout, Strong, Subscript,
    Superscript, Underline,
};
use crate::pandoc::table::Table;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::SourceInfo;

/// Context for plain-text writing, threading diagnostics through.
pub struct PlainTextWriterContext {
    diagnostics: Vec<DiagnosticMessage>,
}

impl PlainTextWriterContext {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    pub fn into_diagnostics(self) -> Vec<DiagnosticMessage> {
        self.diagnostics
    }

    pub fn diagnostics(&self) -> &[DiagnosticMessage] {
        &self.diagnostics
    }

    fn warn_dropped_node(&mut self, description: &str, source_info: &SourceInfo) {
        let diag = DiagnosticMessageBuilder::warning(format!(
            "Node dropped in plain-text output: {}",
            description
        ))
        .with_location(source_info.clone())
        .build();
        self.diagnostics.push(diag);
    }
}

impl Default for PlainTextWriterContext {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Inline writing
// ============================================================================

/// Write inlines as pure plain text (no escaping, no markup).
pub fn write_inlines<T: std::io::Write>(
    inlines: &Inlines,
    buf: &mut T,
    ctx: &mut PlainTextWriterContext,
) -> std::io::Result<()> {
    for inline in inlines {
        write_inline(inline, buf, ctx)?;
    }
    Ok(())
}

/// Write a single inline element.
fn write_inline<T: std::io::Write>(
    inline: &Inline,
    buf: &mut T,
    ctx: &mut PlainTextWriterContext,
) -> std::io::Result<()> {
    match inline {
        // Direct output
        Inline::Str(s) => write!(buf, "{}", s.text)?,
        Inline::Space(_) => write!(buf, " ")?,
        Inline::SoftBreak(_) => write!(buf, " ")?,
        Inline::LineBreak(_) => writeln!(buf)?,

        // Recurse into content (strip structure markers)
        Inline::Emph(Emph { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::Strong(Strong { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::Underline(Underline { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::Strikeout(Strikeout { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::Superscript(Superscript { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::Subscript(Subscript { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::SmallCaps(SmallCaps { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::Span(Span { content, .. }) => write_inlines(content, buf, ctx)?,

        // Quoted: use actual quote characters
        Inline::Quoted(Quoted {
            quote_type,
            content,
            ..
        }) => {
            let (open, close) = match quote_type {
                QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'), // ' '
                QuoteType::DoubleQuote => ('\u{201C}', '\u{201D}'), // " "
            };
            write!(buf, "{}", open)?;
            write_inlines(content, buf, ctx)?;
            write!(buf, "{}", close)?;
        }

        // Code: mimic markdown with backticks
        Inline::Code(Code { text, .. }) => {
            write!(buf, "`{}`", text)?;
        }

        // Math: output raw TeX
        Inline::Math(Math { text, .. }) => {
            write!(buf, "{}", text)?;
        }

        // Link/Image: recurse into content only (no ![], [] markers)
        Inline::Link(Link { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::Image(Image { content, .. }) => write_inlines(content, buf, ctx)?,

        // Cite: recurse into content
        Inline::Cite(Cite { content, .. }) => write_inlines(content, buf, ctx)?,

        // Note: skip (no warning - expected behavior)
        Inline::Note(_) => {}

        // RawInline: echo if 'plaintext', else drop with warning
        Inline::RawInline(RawInline {
            format,
            text,
            source_info,
        }) => {
            if format == "plaintext" {
                write!(buf, "{}", text)?;
            } else {
                ctx.warn_dropped_node(&format!("RawInline with format '{}'", format), source_info);
            }
        }

        // CriticMarkup extensions: recurse into content
        Inline::Insert(Insert { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::Delete(Delete { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::Highlight(Highlight { content, .. }) => write_inlines(content, buf, ctx)?,
        Inline::EditComment(EditComment { content, .. }) => write_inlines(content, buf, ctx)?,

        // Quarto extensions: drop (Shortcode and Attr don't have source_info)
        Inline::Shortcode(_) => {
            // Shortcode doesn't have source_info, so we drop silently
        }
        Inline::NoteReference(NoteReference { source_info, .. }) => {
            ctx.warn_dropped_node("NoteReference", source_info);
        }
        Inline::Attr(_, _) => {
            // Attr uses AttrSourceInfo, not SourceInfo, so we drop silently
        }
        Inline::Custom(custom) => {
            ctx.warn_dropped_node(
                &format!("Custom inline ({})", custom.type_name),
                &custom.source_info,
            );
        }
    }
    Ok(())
}

// ============================================================================
// Block writing
// ============================================================================

/// Write blocks as plain text.
pub fn write_blocks<T: std::io::Write>(
    blocks: &[Block],
    buf: &mut T,
    ctx: &mut PlainTextWriterContext,
) -> std::io::Result<()> {
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            writeln!(buf)?;
        }
        write_block(block, buf, ctx)?;
    }
    Ok(())
}

/// Write a single block element.
fn write_block<T: std::io::Write>(
    block: &Block,
    buf: &mut T,
    ctx: &mut PlainTextWriterContext,
) -> std::io::Result<()> {
    match block {
        // Plain and Paragraph: write inlines
        Block::Plain(Plain { content, .. }) => {
            write_inlines(content, buf, ctx)?;
        }
        Block::Paragraph(Paragraph { content, .. }) => {
            write_inlines(content, buf, ctx)?;
        }

        // Header: write inlines (no # prefix in plain text)
        Block::Header(Header { content, .. }) => {
            write_inlines(content, buf, ctx)?;
        }

        // CodeBlock: mimic markdown (fenced with backticks)
        Block::CodeBlock(CodeBlock { text, .. }) => {
            writeln!(buf, "```")?;
            write!(buf, "{}", text)?;
            if !text.ends_with('\n') {
                writeln!(buf)?;
            }
            write!(buf, "```")?;
        }

        // BlockQuote: mimic markdown (> prefix)
        Block::BlockQuote(BlockQuote { content, .. }) => {
            write_blocks_prefixed(content, "> ", buf, ctx)?;
        }

        // LineBlock: mimic markdown (| prefix)
        Block::LineBlock(LineBlock { content, .. }) => {
            for (i, line) in content.iter().enumerate() {
                if i > 0 {
                    writeln!(buf)?;
                }
                write!(buf, "| ")?;
                write_inlines(line, buf, ctx)?;
            }
        }

        // OrderedList: mimic markdown (1. 2. 3.)
        Block::OrderedList(OrderedList { content, attr, .. }) => {
            let start = attr.0;
            for (i, item) in content.iter().enumerate() {
                if i > 0 {
                    writeln!(buf)?;
                }
                let num = start + i;
                let prefix = format!("{}. ", num);
                write_list_item(item, &prefix, buf, ctx)?;
            }
        }

        // BulletList: mimic markdown (- prefix)
        Block::BulletList(BulletList { content, .. }) => {
            for (i, item) in content.iter().enumerate() {
                if i > 0 {
                    writeln!(buf)?;
                }
                write_list_item(item, "- ", buf, ctx)?;
            }
        }

        // HorizontalRule: output ---
        Block::HorizontalRule(HorizontalRule { .. }) => {
            write!(buf, "---")?;
        }

        // Div: write contents only (no ::: scaffold)
        Block::Div(Div { content, .. }) => {
            write_blocks(content, buf, ctx)?;
        }

        // Figure: write contents only
        Block::Figure(Figure { content, .. }) => {
            write_blocks(content, buf, ctx)?;
        }

        // RawBlock: echo if 'plaintext', else drop with warning
        Block::RawBlock(RawBlock {
            format,
            text,
            source_info,
        }) => {
            if format == "plaintext" {
                write!(buf, "{}", text)?;
            } else {
                ctx.warn_dropped_node(&format!("RawBlock with format '{}'", format), source_info);
            }
        }

        // DefinitionList: complex structure, drop with warning
        Block::DefinitionList(DefinitionList { source_info, .. }) => {
            ctx.warn_dropped_node("DefinitionList", source_info);
        }

        // Table: complex structure, drop with warning
        Block::Table(Table { source_info, .. }) => {
            ctx.warn_dropped_node("Table", source_info);
        }

        // Quarto extensions: drop with warning
        Block::BlockMetadata(MetaBlock { source_info, .. }) => {
            ctx.warn_dropped_node("BlockMetadata", source_info);
        }
        Block::NoteDefinitionPara(NoteDefinitionPara { source_info, .. }) => {
            ctx.warn_dropped_node("NoteDefinitionPara", source_info);
        }
        Block::NoteDefinitionFencedBlock(NoteDefinitionFencedBlock { source_info, .. }) => {
            ctx.warn_dropped_node("NoteDefinitionFencedBlock", source_info);
        }
        Block::CaptionBlock(CaptionBlock { source_info, .. }) => {
            ctx.warn_dropped_node("CaptionBlock", source_info);
        }
        Block::Custom(custom) => {
            ctx.warn_dropped_node(
                &format!("Custom block ({})", custom.type_name),
                &custom.source_info,
            );
        }
    }
    Ok(())
}

/// Write blocks with a prefix on each line (for blockquotes).
fn write_blocks_prefixed<T: std::io::Write>(
    blocks: &[Block],
    prefix: &str,
    buf: &mut T,
    ctx: &mut PlainTextWriterContext,
) -> std::io::Result<()> {
    // Render blocks to a temporary buffer first
    let mut temp = Vec::new();
    write_blocks(blocks, &mut temp, ctx)?;
    let content = String::from_utf8_lossy(&temp);

    // Apply prefix to each line
    for (i, line) in content.lines().enumerate() {
        if i > 0 {
            writeln!(buf)?;
        }
        write!(buf, "{}{}", prefix, line)?;
    }
    Ok(())
}

/// Write a list item with proper prefix handling.
fn write_list_item<T: std::io::Write>(
    blocks: &[Block],
    prefix: &str,
    buf: &mut T,
    ctx: &mut PlainTextWriterContext,
) -> std::io::Result<()> {
    // For continuation lines, use spaces of the same width as the prefix
    let continuation = " ".repeat(prefix.len());

    // Render blocks to a temporary buffer first
    let mut temp = Vec::new();
    write_blocks(blocks, &mut temp, ctx)?;
    let content = String::from_utf8_lossy(&temp);

    // Apply prefix to first line, continuation indent to subsequent lines
    for (i, line) in content.lines().enumerate() {
        if i > 0 {
            writeln!(buf)?;
        }
        if i == 0 {
            write!(buf, "{}{}", prefix, line)?;
        } else {
            write!(buf, "{}{}", continuation, line)?;
        }
    }
    Ok(())
}

// ============================================================================
// Convenience functions
// ============================================================================

/// Convert inlines to a plain text string, returning diagnostics.
pub fn inlines_to_string(inlines: &Inlines) -> (String, Vec<DiagnosticMessage>) {
    let mut buf = Vec::new();
    let mut ctx = PlainTextWriterContext::new();
    // Ignore io::Result since we're writing to a Vec
    let _ = write_inlines(inlines, &mut buf, &mut ctx);
    (
        String::from_utf8_lossy(&buf).into_owned(),
        ctx.into_diagnostics(),
    )
}

/// Convert blocks to a plain text string, returning diagnostics.
pub fn blocks_to_string(blocks: &[Block]) -> (String, Vec<DiagnosticMessage>) {
    let mut buf = Vec::new();
    let mut ctx = PlainTextWriterContext::new();
    // Ignore io::Result since we're writing to a Vec
    let _ = write_blocks(blocks, &mut buf, &mut ctx);
    (
        String::from_utf8_lossy(&buf).into_owned(),
        ctx.into_diagnostics(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pandoc::inline::{Space, Str};

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::from_range(
            quarto_source_map::FileId(0),
            quarto_source_map::Range {
                start: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    fn make_str(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })
    }

    fn make_space() -> Inline {
        Inline::Space(Space {
            source_info: dummy_source_info(),
        })
    }

    #[test]
    fn test_simple_string() {
        let inlines = vec![make_str("Hello"), make_space(), make_str("world")];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "Hello world");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_emphasis_stripped() {
        let inlines = vec![Inline::Emph(Emph {
            content: vec![make_str("emphasized")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "emphasized");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_code_with_backticks() {
        let inlines = vec![Inline::Code(Code {
            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
            text: "code".to_string(),
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "`code`");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_quoted_with_smart_quotes() {
        let inlines = vec![Inline::Quoted(Quoted {
            quote_type: QuoteType::DoubleQuote,
            content: vec![make_str("quoted")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "\u{201C}quoted\u{201D}");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_raw_inline_plaintext_format() {
        let inlines = vec![Inline::RawInline(RawInline {
            format: "plaintext".to_string(),
            text: "raw text".to_string(),
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "raw text");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_raw_inline_html_dropped_with_warning() {
        let inlines = vec![Inline::RawInline(RawInline {
            format: "html".to_string(),
            text: "<b>bold</b>".to_string(),
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("RawInline"));
    }

    #[test]
    fn test_paragraph_block() {
        let blocks = vec![Block::Paragraph(Paragraph {
            content: vec![make_str("A paragraph.")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "A paragraph.");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_code_block_fenced() {
        let blocks = vec![Block::CodeBlock(CodeBlock {
            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
            text: "let x = 1;".to_string(),
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "```\nlet x = 1;\n```");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_bullet_list() {
        let blocks = vec![Block::BulletList(BulletList {
            content: vec![
                vec![Block::Plain(Plain {
                    content: vec![make_str("Item 1")],
                    source_info: dummy_source_info(),
                })],
                vec![Block::Plain(Plain {
                    content: vec![make_str("Item 2")],
                    source_info: dummy_source_info(),
                })],
            ],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "- Item 1\n- Item 2");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_table_dropped_with_warning() {
        let blocks = vec![Block::Table(Table {
            attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
            caption: crate::pandoc::caption::Caption {
                short: None,
                long: None,
                source_info: dummy_source_info(),
            },
            colspec: vec![],
            head: crate::pandoc::table::TableHead {
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                rows: vec![],
                source_info: dummy_source_info(),
                attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
            },
            bodies: vec![],
            foot: crate::pandoc::table::TableFoot {
                attr: ("".to_string(), vec![], hashlink::LinkedHashMap::new()),
                rows: vec![],
                source_info: dummy_source_info(),
                attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
            },
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("Table"));
    }
}
