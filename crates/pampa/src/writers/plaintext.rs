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
    use crate::pandoc::block::{
        BlockQuote, CaptionBlock, DefinitionList, Div, Figure, Header, HorizontalRule, LineBlock,
        MetaBlock, NoteDefinitionFencedBlock, NoteDefinitionPara, OrderedList, Plain, RawBlock,
    };
    use crate::pandoc::inline::{
        Cite, Delete, EditComment, Highlight, Image, Insert, LineBreak, Link, Math, MathType, Note,
        NoteReference, SmallCaps, SoftBreak, Space, Span, Str, Strikeout, Strong, Subscript,
        Superscript, Underline,
    };

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
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
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
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
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
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            caption: crate::pandoc::caption::Caption {
                short: None,
                long: None,
                source_info: dummy_source_info(),
            },
            colspec: vec![],
            head: crate::pandoc::table::TableHead {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                rows: vec![],
                source_info: dummy_source_info(),
                attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
            },
            bodies: vec![],
            foot: crate::pandoc::table::TableFoot {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
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

    // ============================================================================
    // Additional inline tests for coverage
    // ============================================================================

    #[test]
    fn test_soft_break() {
        let inlines = vec![
            make_str("first"),
            Inline::SoftBreak(SoftBreak {
                source_info: dummy_source_info(),
            }),
            make_str("second"),
        ];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "first second");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_line_break() {
        let inlines = vec![
            make_str("first"),
            Inline::LineBreak(LineBreak {
                source_info: dummy_source_info(),
            }),
            make_str("second"),
        ];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "first\nsecond");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_strong_stripped() {
        let inlines = vec![Inline::Strong(Strong {
            content: vec![make_str("bold")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "bold");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_underline_stripped() {
        let inlines = vec![Inline::Underline(Underline {
            content: vec![make_str("underlined")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "underlined");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_strikeout_stripped() {
        let inlines = vec![Inline::Strikeout(Strikeout {
            content: vec![make_str("struck")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "struck");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_superscript_stripped() {
        let inlines = vec![Inline::Superscript(Superscript {
            content: vec![make_str("sup")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "sup");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_subscript_stripped() {
        let inlines = vec![Inline::Subscript(Subscript {
            content: vec![make_str("sub")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "sub");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_smallcaps_stripped() {
        let inlines = vec![Inline::SmallCaps(SmallCaps {
            content: vec![make_str("smallcaps")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "smallcaps");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_span_stripped() {
        let inlines = vec![Inline::Span(Span {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![make_str("span content")],
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "span content");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_quoted_single_quotes() {
        let inlines = vec![Inline::Quoted(Quoted {
            quote_type: QuoteType::SingleQuote,
            content: vec![make_str("quoted")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        // Single quotes: ' (U+2018) and ' (U+2019)
        assert_eq!(result, "\u{2018}quoted\u{2019}");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_math_inline() {
        let inlines = vec![Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "E = mc^2".to_string(),
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "E = mc^2");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_link_content_only() {
        let inlines = vec![Inline::Link(Link {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![make_str("link text")],
            target: ("https://example.com".to_string(), "".to_string()),
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
            target_source: crate::pandoc::attr::TargetSourceInfo::empty(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "link text");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_image_content_only() {
        let inlines = vec![Inline::Image(Image {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![make_str("alt text")],
            target: ("image.png".to_string(), "".to_string()),
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
            target_source: crate::pandoc::attr::TargetSourceInfo::empty(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "alt text");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_cite_content() {
        let inlines = vec![Inline::Cite(Cite {
            citations: vec![],
            content: vec![make_str("citation content")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "citation content");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_note_silently_dropped() {
        let inlines = vec![
            make_str("text"),
            Inline::Note(Note {
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![make_str("note content")],
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            }),
            make_str(" more"),
        ];
        let (result, diags) = inlines_to_string(&inlines);
        // Note is dropped silently (expected behavior)
        assert_eq!(result, "text more");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_insert_stripped() {
        let inlines = vec![Inline::Insert(Insert {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![make_str("inserted")],
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "inserted");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_delete_stripped() {
        let inlines = vec![Inline::Delete(Delete {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![make_str("deleted")],
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "deleted");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_highlight_stripped() {
        let inlines = vec![Inline::Highlight(Highlight {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![make_str("highlighted")],
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "highlighted");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_edit_comment_stripped() {
        let inlines = vec![Inline::EditComment(EditComment {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![make_str("comment")],
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "comment");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_shortcode_silently_dropped() {
        use quarto_pandoc_types::Shortcode;
        use std::collections::HashMap;

        let inlines = vec![
            make_str("before"),
            Inline::Shortcode(Shortcode {
                is_escaped: false,
                name: "include".to_string(),
                positional_args: vec![],
                keyword_args: HashMap::new(),
            }),
            make_str("after"),
        ];
        let (result, diags) = inlines_to_string(&inlines);
        // Shortcode is dropped silently (no source_info)
        assert_eq!(result, "beforeafter");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_note_reference_with_warning() {
        let inlines = vec![Inline::NoteReference(NoteReference {
            id: "fn1".to_string(),
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("NoteReference"));
    }

    #[test]
    fn test_attr_inline_silently_dropped() {
        let inlines = vec![
            make_str("text"),
            Inline::Attr(
                (
                    "id".to_string(),
                    vec!["class".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                crate::pandoc::attr::AttrSourceInfo::empty(),
            ),
        ];
        let (result, diags) = inlines_to_string(&inlines);
        // Attr is dropped silently (uses AttrSourceInfo, not SourceInfo)
        assert_eq!(result, "text");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_custom_inline_with_warning() {
        use crate::pandoc::custom::CustomNode;

        let inlines = vec![Inline::Custom(CustomNode {
            type_name: "TestCustom".to_string(),
            slots: hashlink::LinkedHashMap::new(),
            plain_data: serde_json::Value::Null,
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("Custom inline"));
        assert!(diags[0].title.contains("TestCustom"));
    }

    // ============================================================================
    // Additional block tests for coverage
    // ============================================================================

    #[test]
    fn test_plain_block() {
        let blocks = vec![Block::Plain(Plain {
            content: vec![make_str("Plain text.")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "Plain text.");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_header_block() {
        let blocks = vec![Block::Header(Header {
            level: 2,
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![make_str("Heading")],
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        // No # prefix in plain text
        assert_eq!(result, "Heading");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_blockquote() {
        let blocks = vec![Block::BlockQuote(BlockQuote {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("Quoted text")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "> Quoted text");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_blockquote_multiline() {
        let blocks = vec![Block::BlockQuote(BlockQuote {
            content: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("First para")],
                    source_info: dummy_source_info(),
                }),
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Second para")],
                    source_info: dummy_source_info(),
                }),
            ],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        // The newline between paragraphs is prefixed with ">"
        assert_eq!(result, "> First para\n> Second para");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_line_block() {
        let blocks = vec![Block::LineBlock(LineBlock {
            content: vec![
                vec![make_str("Line one")],
                vec![make_str("Line two")],
                vec![make_str("Line three")],
            ],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "| Line one\n| Line two\n| Line three");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_ordered_list() {
        use crate::pandoc::list::{ListNumberDelim, ListNumberStyle};

        let blocks = vec![Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![
                vec![Block::Plain(Plain {
                    content: vec![make_str("First")],
                    source_info: dummy_source_info(),
                })],
                vec![Block::Plain(Plain {
                    content: vec![make_str("Second")],
                    source_info: dummy_source_info(),
                })],
            ],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "1. First\n2. Second");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_ordered_list_start_number() {
        use crate::pandoc::list::{ListNumberDelim, ListNumberStyle};

        let blocks = vec![Block::OrderedList(OrderedList {
            attr: (5, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![
                vec![Block::Plain(Plain {
                    content: vec![make_str("Fifth")],
                    source_info: dummy_source_info(),
                })],
                vec![Block::Plain(Plain {
                    content: vec![make_str("Sixth")],
                    source_info: dummy_source_info(),
                })],
            ],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "5. Fifth\n6. Sixth");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_horizontal_rule() {
        let blocks = vec![Block::HorizontalRule(HorizontalRule {
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "---");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_div_contents_only() {
        let blocks = vec![Block::Div(Div {
            attr: (
                "myid".to_string(),
                vec!["myclass".to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("Div content")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        // No ::: scaffold
        assert_eq!(result, "Div content");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_figure_contents_only() {
        let blocks = vec![Block::Figure(Figure {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            caption: crate::pandoc::caption::Caption {
                short: None,
                long: None,
                source_info: dummy_source_info(),
            },
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("Figure content")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "Figure content");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_raw_block_plaintext() {
        let blocks = vec![Block::RawBlock(RawBlock {
            format: "plaintext".to_string(),
            text: "Raw plaintext content".to_string(),
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "Raw plaintext content");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_raw_block_html_dropped_with_warning() {
        let blocks = vec![Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text: "<div>HTML content</div>".to_string(),
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("RawBlock"));
        assert!(diags[0].title.contains("html"));
    }

    #[test]
    fn test_definition_list_dropped_with_warning() {
        let blocks = vec![Block::DefinitionList(DefinitionList {
            content: vec![(
                vec![make_str("Term")],
                vec![vec![Block::Paragraph(Paragraph {
                    content: vec![make_str("Definition")],
                    source_info: dummy_source_info(),
                })]],
            )],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("DefinitionList"));
    }

    #[test]
    fn test_block_metadata_dropped_with_warning() {
        use quarto_pandoc_types::config_value::ConfigValue;

        let blocks = vec![Block::BlockMetadata(MetaBlock {
            meta: ConfigValue::null(dummy_source_info()),
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("BlockMetadata"));
    }

    #[test]
    fn test_note_definition_para_dropped_with_warning() {
        let blocks = vec![Block::NoteDefinitionPara(NoteDefinitionPara {
            id: "fn1".to_string(),
            content: vec![make_str("Note content")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("NoteDefinitionPara"));
    }

    #[test]
    fn test_note_definition_fenced_block_dropped_with_warning() {
        let blocks = vec![Block::NoteDefinitionFencedBlock(
            NoteDefinitionFencedBlock {
                id: "fn2".to_string(),
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![make_str("Fenced note content")],
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            },
        )];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("NoteDefinitionFencedBlock"));
    }

    #[test]
    fn test_caption_block_dropped_with_warning() {
        let blocks = vec![Block::CaptionBlock(CaptionBlock {
            content: vec![make_str("Caption text")],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("CaptionBlock"));
    }

    #[test]
    fn test_custom_block_with_warning() {
        use crate::pandoc::custom::CustomNode;

        let blocks = vec![Block::Custom(CustomNode {
            type_name: "TestCallout".to_string(),
            slots: hashlink::LinkedHashMap::new(),
            plain_data: serde_json::Value::Null,
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.contains("Custom block"));
        assert!(diags[0].title.contains("TestCallout"));
    }

    // ============================================================================
    // Multiple blocks and edge cases
    // ============================================================================

    #[test]
    fn test_multiple_blocks_separated_by_newlines() {
        let blocks = vec![
            Block::Paragraph(Paragraph {
                content: vec![make_str("First paragraph.")],
                source_info: dummy_source_info(),
            }),
            Block::Paragraph(Paragraph {
                content: vec![make_str("Second paragraph.")],
                source_info: dummy_source_info(),
            }),
            Block::Paragraph(Paragraph {
                content: vec![make_str("Third paragraph.")],
                source_info: dummy_source_info(),
            }),
        ];
        let (result, diags) = blocks_to_string(&blocks);
        // Single newline between blocks (not blank line)
        assert_eq!(
            result,
            "First paragraph.\nSecond paragraph.\nThird paragraph."
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn test_code_block_with_trailing_newline() {
        let blocks = vec![Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            text: "let x = 1;\n".to_string(),
            source_info: dummy_source_info(),
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        // Text already has trailing newline, so no extra added
        assert_eq!(result, "```\nlet x = 1;\n```");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_list_item_multiline() {
        let blocks = vec![Block::BulletList(BulletList {
            content: vec![vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("First line")],
                    source_info: dummy_source_info(),
                }),
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Second line")],
                    source_info: dummy_source_info(),
                }),
            ]],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = blocks_to_string(&blocks);
        // Continuation lines use spaces (same width as "- ") instead of bullet
        assert_eq!(result, "- First line\n  Second line");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_diagnostics_accessor() {
        let ctx = PlainTextWriterContext::new();
        assert!(ctx.diagnostics().is_empty());
    }

    #[test]
    fn test_nested_formatting() {
        // Test deeply nested inline formatting
        let inlines = vec![Inline::Strong(Strong {
            content: vec![Inline::Emph(Emph {
                content: vec![Inline::Underline(Underline {
                    content: vec![make_str("nested")],
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "nested");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_empty_inlines() {
        let inlines: Vec<Inline> = vec![];
        let (result, diags) = inlines_to_string(&inlines);
        assert_eq!(result, "");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_empty_blocks() {
        let blocks: Vec<Block> = vec![];
        let (result, diags) = blocks_to_string(&blocks);
        assert_eq!(result, "");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_plain_text_writer_context_default() {
        let ctx = PlainTextWriterContext::default();
        assert!(ctx.diagnostics.is_empty());
    }
}
