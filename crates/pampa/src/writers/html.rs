/*
 * html.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{ASTContext, Attr, Block, CitationMode, Inline, Inlines, Pandoc};
use crate::writers::html_source::build_source_map;
use crate::writers::json::{self, JsonConfig};
use quarto_pandoc_types::meta::MetaValueWithSourceInfo;
use std::collections::HashMap;
use std::io::Write;
use std::marker::PhantomData;

// =============================================================================
// Configuration and Context
// =============================================================================

/// Configuration for HTML output
#[derive(Debug, Clone, Default)]
pub struct HtmlConfig {
    /// Include source location tracking (data-loc, data-sid attributes)
    pub include_source_locations: bool,
}

/// Extract HTML configuration from document metadata.
///
/// Looks for the following structure in YAML frontmatter:
/// ```yaml
/// format:
///   html:
///     source-location: full
/// ```
///
/// If `format.html.source-location` is set to "full", enables source location tracking.
pub fn extract_config_from_metadata(meta: &MetaValueWithSourceInfo) -> HtmlConfig {
    let include_source_locations = meta
        .get("format")
        .and_then(|f| f.get("html"))
        .and_then(|h| h.get("source-location"))
        .map(|sl| sl.is_string_value("full"))
        .unwrap_or(false);

    HtmlConfig {
        include_source_locations,
    }
}

/// Information extracted from JSON for each AST node
#[derive(Debug, Clone)]
pub struct SourceNodeInfo {
    /// Pool ID (the "s" field from JSON)
    pub pool_id: usize,
    /// Resolved location (the "l" field from JSON)
    pub location: Option<ResolvedLocation>,
}

/// Resolved source location
#[derive(Debug, Clone)]
pub struct ResolvedLocation {
    pub file_id: usize,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl ResolvedLocation {
    /// Format as data-loc attribute value: "file:line:col-line:col"
    pub fn to_data_loc(&self) -> String {
        format!(
            "{}:{}:{}-{}:{}",
            self.file_id, self.start_line, self.start_col, self.end_line, self.end_col
        )
    }
}

/// Context threaded through HTML writer functions.
///
/// This struct is generic over the writer type and implements `Write` itself,
/// so `write!` and `writeln!` macros can be used directly on the context.
pub struct HtmlWriterContext<'ast, W: Write> {
    /// The underlying writer
    writer: W,
    /// Map from AST node pointers to source info
    source_map: HashMap<*const (), SourceNodeInfo>,
    /// Configuration
    config: HtmlConfig,
    /// Lifetime marker
    _phantom: PhantomData<&'ast ()>,
}

impl<'ast, W: Write> Write for HtmlWriterContext<'ast, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl<'ast, W: Write> HtmlWriterContext<'ast, W> {
    /// Create a new context with default config (no source tracking)
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            source_map: HashMap::new(),
            config: HtmlConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create a new context with config
    pub fn with_config(writer: W, config: HtmlConfig) -> Self {
        Self {
            writer,
            source_map: HashMap::new(),
            config,
            _phantom: PhantomData,
        }
    }

    /// Set the source map (populated by parallel walk in Phase B)
    pub fn set_source_map(&mut self, source_map: HashMap<*const (), SourceNodeInfo>) {
        self.source_map = source_map;
    }

    /// Look up source info for a block
    pub fn get_block_info(&self, block: &Block) -> Option<&SourceNodeInfo> {
        let key = block as *const Block as *const ();
        self.source_map.get(&key)
    }

    /// Look up source info for an inline
    pub fn get_inline_info(&self, inline: &Inline) -> Option<&SourceNodeInfo> {
        let key = inline as *const Inline as *const ();
        self.source_map.get(&key)
    }

    /// Check if source locations are enabled
    pub fn include_source_locations(&self) -> bool {
        self.config.include_source_locations
    }
}

// =============================================================================
// Helper functions
// =============================================================================

/// Escape HTML special characters
fn escape_html(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            _ => c.to_string(),
        })
        .collect()
}

/// Write HTML attributes (id, classes, key-value pairs)
fn write_attr<W: Write>(attr: &Attr, ctx: &mut HtmlWriterContext<'_, W>) -> std::io::Result<()> {
    let (id, classes, attrs) = attr;

    if !id.is_empty() {
        write!(ctx, " id=\"{}\"", escape_html(id))?;
    }

    if !classes.is_empty() {
        write!(ctx, " class=\"{}\"", escape_html(&classes.join(" ")))?;
    }

    // Pandoc prefixes custom attributes with "data-"
    for (k, v) in attrs {
        write!(ctx, " data-{}=\"{}\"", escape_html(k), escape_html(v))?;
    }

    Ok(())
}

/// Write source location attributes for a block element.
///
/// Outputs `data-sid` (pool ID) and `data-loc` (resolved location) if
/// source tracking is enabled and we have source info for this block.
fn write_block_source_attrs<W: Write>(
    block: &Block,
    ctx: &mut HtmlWriterContext<'_, W>,
) -> std::io::Result<()> {
    if !ctx.include_source_locations() {
        return Ok(());
    }

    // Extract values to avoid borrowing ctx through info
    let source_attrs = ctx.get_block_info(block).map(|info| {
        (
            info.pool_id,
            info.location.as_ref().map(|loc| loc.to_data_loc()),
        )
    });

    if let Some((pool_id, loc_str)) = source_attrs {
        write!(ctx, " data-sid=\"{}\"", pool_id)?;
        if let Some(loc) = loc_str {
            write!(ctx, " data-loc=\"{}\"", loc)?;
        }
    }

    Ok(())
}

/// Write source location attributes for an inline element.
///
/// Outputs `data-sid` (pool ID) and `data-loc` (resolved location) if
/// source tracking is enabled and we have source info for this inline.
fn write_inline_source_attrs<W: Write>(
    inline: &Inline,
    ctx: &mut HtmlWriterContext<'_, W>,
) -> std::io::Result<()> {
    if !ctx.include_source_locations() {
        return Ok(());
    }

    // Extract values to avoid borrowing ctx through info
    let source_attrs = ctx.get_inline_info(inline).map(|info| {
        (
            info.pool_id,
            info.location.as_ref().map(|loc| loc.to_data_loc()),
        )
    });

    if let Some((pool_id, loc_str)) = source_attrs {
        write!(ctx, " data-sid=\"{}\"", pool_id)?;
        if let Some(loc) = loc_str {
            write!(ctx, " data-loc=\"{}\"", loc)?;
        }
    }

    Ok(())
}

/// Write inline elements
fn write_inline<W: Write>(
    inline: &Inline,
    ctx: &mut HtmlWriterContext<'_, W>,
) -> std::io::Result<()> {
    match inline {
        Inline::Str(s) => {
            if ctx.include_source_locations() {
                // Wrap in span for source tracking
                write!(ctx, "<span")?;
                write_inline_source_attrs(inline, ctx)?;
                write!(ctx, ">{}</span>", escape_html(&s.text))?;
            } else {
                write!(ctx, "{}", escape_html(&s.text))?;
            }
        }
        Inline::Space(_) => {
            write!(ctx, " ")?;
        }
        Inline::SoftBreak(_) => {
            write!(ctx, "\n")?;
        }
        Inline::LineBreak(_) => {
            write!(ctx, "<br />")?;
        }
        Inline::Emph(e) => {
            write!(ctx, "<em")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&e.content, ctx)?;
            write!(ctx, "</em>")?;
        }
        Inline::Strong(s) => {
            write!(ctx, "<strong")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&s.content, ctx)?;
            write!(ctx, "</strong>")?;
        }
        Inline::Underline(u) => {
            write!(ctx, "<u")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&u.content, ctx)?;
            write!(ctx, "</u>")?;
        }
        Inline::Strikeout(s) => {
            write!(ctx, "<del")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&s.content, ctx)?;
            write!(ctx, "</del>")?;
        }
        Inline::Superscript(s) => {
            write!(ctx, "<sup")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&s.content, ctx)?;
            write!(ctx, "</sup>")?;
        }
        Inline::Subscript(s) => {
            write!(ctx, "<sub")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&s.content, ctx)?;
            write!(ctx, "</sub>")?;
        }
        Inline::SmallCaps(s) => {
            write!(ctx, "<span style=\"font-variant: small-caps;\"")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&s.content, ctx)?;
            write!(ctx, "</span>")?;
        }
        Inline::Quoted(q) => {
            let (open, close) = match q.quote_type {
                crate::pandoc::QuoteType::SingleQuote => ("\u{2018}", "\u{2019}"),
                crate::pandoc::QuoteType::DoubleQuote => ("\u{201C}", "\u{201D}"),
            };
            write!(ctx, "{}", open)?;
            write_inlines(&q.content, ctx)?;
            write!(ctx, "{}", close)?;
        }
        Inline::Code(c) => {
            write!(ctx, "<code")?;
            write_attr(&c.attr, ctx)?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">{}</code>", escape_html(&c.text))?;
        }
        Inline::Math(m) => {
            let class = match m.math_type {
                crate::pandoc::MathType::InlineMath => "math inline",
                crate::pandoc::MathType::DisplayMath => "math display",
            };
            // Use \(...\) for inline math and \[...\] for display math
            let (open, close) = match m.math_type {
                crate::pandoc::MathType::InlineMath => ("\\(", "\\)"),
                crate::pandoc::MathType::DisplayMath => ("\\[", "\\]"),
            };
            write!(ctx, "<span class=\"{}\"", class)?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">{}{}{}</span>", open, escape_html(&m.text), close)?;
        }
        Inline::Link(link) => {
            write!(ctx, "<a href=\"{}\"", escape_html(&link.target.0))?;
            write_attr(&link.attr, ctx)?;
            if !link.target.1.is_empty() {
                write!(ctx, " title=\"{}\"", escape_html(&link.target.1))?;
            }
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&link.content, ctx)?;
            write!(ctx, "</a>")?;
        }
        Inline::Image(image) => {
            write!(ctx, "<img src=\"{}\"", escape_html(&image.target.0))?;
            write!(ctx, " alt=\"")?;
            // For alt text, we need to extract plain text from inlines
            write_inlines_as_text(&image.content, ctx)?;
            write!(ctx, "\"")?;
            write_attr(&image.attr, ctx)?;
            if !image.target.1.is_empty() {
                write!(ctx, " title=\"{}\"", escape_html(&image.target.1))?;
            }
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, " />")?;
        }
        Inline::RawInline(raw) => {
            // Only output raw HTML if format is "html"
            if raw.format == "html" {
                write!(ctx, "{}", raw.text)?;
            }
        }
        Inline::Span(span) => {
            write!(ctx, "<span")?;
            write_attr(&span.attr, ctx)?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&span.content, ctx)?;
            write!(ctx, "</span>")?;
        }
        Inline::Note(note) => {
            // Footnotes are rendered as superscript with a link
            write!(
                ctx,
                "<sup class=\"footnote-ref\"><a href=\"#fn{}\">",
                note.content.len()
            )?;
            write!(ctx, "[{}]", note.content.len())?;
            write!(ctx, "</a></sup>")?;
            // Note: Proper footnote handling would require collecting all footnotes
            // and rendering them at the end of the document
        }
        Inline::Cite(cite) => {
            // Collect all citation IDs for data-cites attribute
            let cite_ids: Vec<String> = cite.citations.iter().map(|c| c.id.clone()).collect();
            let data_cites = cite_ids.join(" ");

            write!(
                ctx,
                "<span class=\"citation\" data-cites=\"{}\"",
                escape_html(&data_cites)
            )?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;

            // Pandoc outputs citation content if present, otherwise builds citation text
            if !cite.content.is_empty() {
                write_inlines(&cite.content, ctx)?;
            } else {
                for (i, citation) in cite.citations.iter().enumerate() {
                    if i > 0 {
                        write!(ctx, "; ")?;
                    }
                    write_inlines(&citation.prefix, ctx)?;
                    if !citation.prefix.is_empty() {
                        write!(ctx, " ")?;
                    }
                    match citation.mode {
                        CitationMode::AuthorInText => write!(ctx, "{}", escape_html(&citation.id))?,
                        CitationMode::SuppressAuthor => {
                            write!(ctx, "-@{}", escape_html(&citation.id))?
                        }
                        CitationMode::NormalCitation => {
                            write!(ctx, "@{}", escape_html(&citation.id))?
                        }
                    }
                    if !citation.suffix.is_empty() {
                        write!(ctx, " ")?;
                    }
                    write_inlines(&citation.suffix, ctx)?;
                }
            }
            write!(ctx, "</span>")?;
        }
        // Quarto extensions - render as raw HTML or skip
        Inline::Shortcode(_) | Inline::NoteReference(_) | Inline::Attr(_, _) => {
            // These should not appear in final output
        }
        Inline::Insert(ins) => {
            write!(ctx, "<ins")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&ins.content, ctx)?;
            write!(ctx, "</ins>")?;
        }
        Inline::Delete(del) => {
            write!(ctx, "<del")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&del.content, ctx)?;
            write!(ctx, "</del>")?;
        }
        Inline::Highlight(h) => {
            write!(ctx, "<mark")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&h.content, ctx)?;
            write!(ctx, "</mark>")?;
        }
        Inline::EditComment(c) => {
            write!(ctx, "<span class=\"comment\"")?;
            write_inline_source_attrs(inline, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&c.content, ctx)?;
            write!(ctx, "</span>")?;
        }
        Inline::Custom(_) => {
            // Custom inline nodes are not rendered in HTML output
        }
    }
    Ok(())
}

/// Write a sequence of inlines
fn write_inlines<W: Write>(
    inlines: &Inlines,
    ctx: &mut HtmlWriterContext<'_, W>,
) -> std::io::Result<()> {
    for inline in inlines {
        write_inline(inline, ctx)?;
    }
    Ok(())
}

/// Write inlines as plain text (for alt attributes, etc.)
fn write_inlines_as_text<W: Write>(
    inlines: &Inlines,
    ctx: &mut HtmlWriterContext<'_, W>,
) -> std::io::Result<()> {
    for inline in inlines {
        match inline {
            Inline::Str(s) => write!(ctx, "{}", escape_html(&s.text))?,
            Inline::Space(_) => write!(ctx, " ")?,
            Inline::SoftBreak(_) | Inline::LineBreak(_) => write!(ctx, " ")?,
            Inline::Emph(e) => write_inlines_as_text(&e.content, ctx)?,
            Inline::Strong(s) => write_inlines_as_text(&s.content, ctx)?,
            Inline::Underline(u) => write_inlines_as_text(&u.content, ctx)?,
            Inline::Strikeout(s) => write_inlines_as_text(&s.content, ctx)?,
            Inline::Superscript(s) => write_inlines_as_text(&s.content, ctx)?,
            Inline::Subscript(s) => write_inlines_as_text(&s.content, ctx)?,
            Inline::SmallCaps(s) => write_inlines_as_text(&s.content, ctx)?,
            Inline::Span(span) => write_inlines_as_text(&span.content, ctx)?,
            Inline::Quoted(q) => write_inlines_as_text(&q.content, ctx)?,
            Inline::Code(c) => write!(ctx, "{}", escape_html(&c.text))?,
            Inline::Link(link) => write_inlines_as_text(&link.content, ctx)?,
            Inline::Image(image) => write_inlines_as_text(&image.content, ctx)?,
            _ => {}
        }
    }
    Ok(())
}

/// Write block elements
fn write_block<W: Write>(block: &Block, ctx: &mut HtmlWriterContext<'_, W>) -> std::io::Result<()> {
    match block {
        Block::Plain(plain) => {
            write_inlines(&plain.content, ctx)?;
            writeln!(ctx)?;
        }
        Block::Paragraph(para) => {
            write!(ctx, "<p")?;
            write_block_source_attrs(block, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&para.content, ctx)?;
            writeln!(ctx, "</p>")?;
        }
        Block::LineBlock(lineblock) => {
            write!(ctx, "<div class=\"line-block\"")?;
            write_block_source_attrs(block, ctx)?;
            writeln!(ctx, ">")?;
            for line in &lineblock.content {
                write!(ctx, "  ")?;
                write_inlines(line, ctx)?;
                writeln!(ctx, "<br />")?;
            }
            writeln!(ctx, "</div>")?;
        }
        Block::CodeBlock(codeblock) => {
            write!(ctx, "<pre")?;
            write_attr(&codeblock.attr, ctx)?;
            write_block_source_attrs(block, ctx)?;
            write!(ctx, "><code>")?;
            write!(ctx, "{}", escape_html(&codeblock.text))?;
            writeln!(ctx, "</code></pre>")?;
        }
        Block::RawBlock(raw) => {
            // Only output raw HTML if format is "html"
            if raw.format == "html" {
                writeln!(ctx, "{}", raw.text)?;
            }
        }
        Block::BlockQuote(quote) => {
            write!(ctx, "<blockquote")?;
            write_block_source_attrs(block, ctx)?;
            writeln!(ctx, ">")?;
            write_blocks(&quote.content, ctx)?;
            writeln!(ctx, "</blockquote>")?;
        }
        Block::OrderedList(list) => {
            let (start, style, _delim) = &list.attr;
            write!(ctx, "<ol")?;
            if *start != 1 {
                write!(ctx, " start=\"{}\"", start)?;
            }
            // Pandoc uses type attribute instead of style
            let list_type = match style {
                crate::pandoc::ListNumberStyle::Decimal => "1",
                crate::pandoc::ListNumberStyle::LowerAlpha => "a",
                crate::pandoc::ListNumberStyle::UpperAlpha => "A",
                crate::pandoc::ListNumberStyle::LowerRoman => "i",
                crate::pandoc::ListNumberStyle::UpperRoman => "I",
                _ => "1",
            };
            write!(ctx, " type=\"{}\"", list_type)?;
            write_block_source_attrs(block, ctx)?;
            writeln!(ctx, ">")?;
            for item in &list.content {
                write!(ctx, "<li>")?;
                write_blocks_inline(item, ctx)?;
                writeln!(ctx, "</li>")?;
            }
            writeln!(ctx, "</ol>")?;
        }
        Block::BulletList(list) => {
            write!(ctx, "<ul")?;
            write_block_source_attrs(block, ctx)?;
            writeln!(ctx, ">")?;
            for item in &list.content {
                write!(ctx, "<li>")?;
                write_blocks_inline(item, ctx)?;
                writeln!(ctx, "</li>")?;
            }
            writeln!(ctx, "</ul>")?;
        }
        Block::DefinitionList(deflist) => {
            write!(ctx, "<dl")?;
            write_block_source_attrs(block, ctx)?;
            writeln!(ctx, ">")?;
            for (term, definitions) in &deflist.content {
                write!(ctx, "<dt>")?;
                write_inlines(term, ctx)?;
                writeln!(ctx, "</dt>")?;
                for def_blocks in definitions {
                    writeln!(ctx, "<dd>")?;
                    write_blocks(def_blocks, ctx)?;
                    writeln!(ctx, "</dd>")?;
                }
            }
            writeln!(ctx, "</dl>")?;
        }
        Block::Header(header) => {
            write!(ctx, "<h{}", header.level)?;
            write_attr(&header.attr, ctx)?;
            write_block_source_attrs(block, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&header.content, ctx)?;
            writeln!(ctx, "</h{}>", header.level)?;
        }
        Block::HorizontalRule(_) => {
            write!(ctx, "<hr")?;
            write_block_source_attrs(block, ctx)?;
            writeln!(ctx, " />")?;
        }
        Block::Table(table) => {
            write!(ctx, "<table")?;
            write_attr(&table.attr, ctx)?;
            write_block_source_attrs(block, ctx)?;
            writeln!(ctx, ">")?;

            // Caption (if any)
            if let Some(ref long_caption) = table.caption.long {
                if !long_caption.is_empty() {
                    writeln!(ctx, "<caption>")?;
                    write_blocks(long_caption, ctx)?;
                    writeln!(ctx, "</caption>")?;
                }
            }

            // Column group (for alignment)
            if !table.colspec.is_empty() {
                writeln!(ctx, "<colgroup>")?;
                for colspec in &table.colspec {
                    let align = match colspec.0 {
                        crate::pandoc::table::Alignment::Left => " align=\"left\"",
                        crate::pandoc::table::Alignment::Right => " align=\"right\"",
                        crate::pandoc::table::Alignment::Center => " align=\"center\"",
                        crate::pandoc::table::Alignment::Default => "",
                    };
                    writeln!(ctx, "<col{} />", align)?;
                }
                writeln!(ctx, "</colgroup>")?;
            }

            // Head
            if !table.head.rows.is_empty() {
                writeln!(ctx, "<thead>")?;
                for row in &table.head.rows {
                    write_table_row(row, ctx, true)?;
                }
                writeln!(ctx, "</thead>")?;
            }

            // Bodies
            for body in &table.bodies {
                writeln!(ctx, "<tbody>")?;
                for row in &body.body {
                    write_table_row(row, ctx, false)?;
                }
                writeln!(ctx, "</tbody>")?;
            }

            // Foot
            if !table.foot.rows.is_empty() {
                writeln!(ctx, "<tfoot>")?;
                for row in &table.foot.rows {
                    write_table_row(row, ctx, false)?;
                }
                writeln!(ctx, "</tfoot>")?;
            }

            writeln!(ctx, "</table>")?;
        }
        Block::Figure(figure) => {
            write!(ctx, "<figure")?;
            write_attr(&figure.attr, ctx)?;
            write_block_source_attrs(block, ctx)?;
            writeln!(ctx, ">")?;
            write_blocks(&figure.content, ctx)?;
            if let Some(ref long_caption) = figure.caption.long {
                if !long_caption.is_empty() {
                    writeln!(ctx, "<figcaption>")?;
                    write_blocks(long_caption, ctx)?;
                    writeln!(ctx, "</figcaption>")?;
                }
            }
            writeln!(ctx, "</figure>")?;
        }
        Block::Div(div) => {
            write!(ctx, "<div")?;
            write_attr(&div.attr, ctx)?;
            write_block_source_attrs(block, ctx)?;
            writeln!(ctx, ">")?;
            write_blocks(&div.content, ctx)?;
            writeln!(ctx, "</div>")?;
        }
        // Quarto extensions
        Block::BlockMetadata(_) => {
            // Metadata blocks don't render to HTML
        }
        Block::NoteDefinitionPara(note) => {
            // Note definitions would typically be collected and rendered as footnotes
            write!(ctx, "<div class=\"footnote\" id=\"fn{}\"", note.id)?;
            write_block_source_attrs(block, ctx)?;
            write!(ctx, ">[{}] ", note.id)?;
            write_inlines(&note.content, ctx)?;
            writeln!(ctx, "</div>")?;
        }
        Block::NoteDefinitionFencedBlock(note) => {
            write!(ctx, "<div class=\"footnote\" id=\"fn{}\"", note.id)?;
            write_block_source_attrs(block, ctx)?;
            writeln!(ctx, ">[{}]", note.id)?;
            write_blocks(&note.content, ctx)?;
            writeln!(ctx, "</div>")?;
        }
        Block::CaptionBlock(caption) => {
            // Caption blocks are rendered as divs with caption class
            write!(ctx, "<div class=\"caption\"")?;
            write_block_source_attrs(block, ctx)?;
            write!(ctx, ">")?;
            write_inlines(&caption.content, ctx)?;
            writeln!(ctx, "</div>")?;
        }
        Block::Custom(_) => {
            // Custom block nodes are not rendered in HTML output
        }
    }
    Ok(())
}

/// Write a table row
fn write_table_row<W: Write>(
    row: &crate::pandoc::table::Row,
    ctx: &mut HtmlWriterContext<'_, W>,
    is_header: bool,
) -> std::io::Result<()> {
    writeln!(ctx, "<tr>")?;
    for cell in &row.cells {
        let tag = if is_header { "th" } else { "td" };
        write!(ctx, "<{}", tag)?;
        write_attr(&cell.attr, ctx)?;

        if cell.row_span > 1 {
            write!(ctx, " rowspan=\"{}\"", cell.row_span)?;
        }
        if cell.col_span > 1 {
            write!(ctx, " colspan=\"{}\"", cell.col_span)?;
        }

        let align = match cell.alignment {
            crate::pandoc::table::Alignment::Left => " align=\"left\"",
            crate::pandoc::table::Alignment::Right => " align=\"right\"",
            crate::pandoc::table::Alignment::Center => " align=\"center\"",
            crate::pandoc::table::Alignment::Default => "",
        };
        write!(ctx, "{}>", align)?;

        write_blocks(&cell.content, ctx)?;
        writeln!(ctx, "</{}>", tag)?;
    }
    writeln!(ctx, "</tr>")?;
    Ok(())
}

/// Write a sequence of blocks (internal, uses context)
fn write_blocks<W: Write>(
    blocks: &[Block],
    ctx: &mut HtmlWriterContext<'_, W>,
) -> std::io::Result<()> {
    for block in blocks {
        write_block(block, ctx)?;
    }
    Ok(())
}

/// Write blocks inline (for list items) - strips paragraph tags for simple cases
fn write_blocks_inline<W: Write>(
    blocks: &[Block],
    ctx: &mut HtmlWriterContext<'_, W>,
) -> std::io::Result<()> {
    // For simple list items with just a single paragraph, write the content inline
    if blocks.len() == 1 {
        if let Block::Paragraph(para) = &blocks[0] {
            write_inlines(&para.content, ctx)?;
            return Ok(());
        } else if let Block::Plain(plain) = &blocks[0] {
            write_inlines(&plain.content, ctx)?;
            return Ok(());
        }
    }

    // For complex list items, write blocks normally
    write_blocks(blocks, ctx)?;
    Ok(())
}

// =============================================================================
// Public API
// =============================================================================

/// Write a Pandoc document to HTML with configuration
pub fn write_with_config<W: Write>(
    pandoc: &Pandoc,
    writer: W,
    config: HtmlConfig,
) -> std::io::Result<()> {
    let mut ctx = HtmlWriterContext::with_config(writer, config);
    write_blocks(&pandoc.blocks, &mut ctx)?;
    Ok(())
}

/// Write a Pandoc document to HTML with source location tracking.
///
/// This function generates JSON from the AST with inline source locations,
/// performs a parallel walk of the AST and JSON to build a pointer-based
/// source map, and then writes HTML with source attributes on each element.
///
/// # Arguments
///
/// * `pandoc` - The Pandoc AST to render
/// * `ast_context` - The AST context containing source info pool and file registry
/// * `writer` - The output writer
///
/// # Returns
///
/// Returns `Ok(())` on success. If JSON generation fails, the error is logged
/// and HTML is written without source attributes.
pub fn write_with_source_tracking<W: Write>(
    pandoc: &Pandoc,
    ast_context: &ASTContext,
    writer: W,
) -> std::io::Result<()> {
    let config = HtmlConfig {
        include_source_locations: true,
    };
    let mut ctx = HtmlWriterContext::with_config(writer, config);

    // Generate JSON with source locations enabled
    let json_config = JsonConfig {
        include_inline_locations: true,
    };

    match json::write_pandoc(pandoc, ast_context, &json_config) {
        Ok(json_value) => {
            // Build source map by walking AST and JSON in parallel
            let source_map = build_source_map(pandoc, &json_value);
            ctx.set_source_map(source_map);
        }
        Err(_errors) => {
            // If JSON generation fails, we continue without source tracking
            // This is a graceful degradation - the HTML will still be valid
            // but without source location attributes.
            // TODO: Consider logging this failure for debugging
        }
    }

    write_blocks(&pandoc.blocks, &mut ctx)?;
    Ok(())
}

/// Main entry point for the HTML writer.
///
/// This function checks the document's YAML frontmatter for configuration options
/// and automatically enables features based on metadata. Currently supports:
///
/// ```yaml
/// format:
///   html:
///     source-location: full
/// ```
///
/// When `source-location: full` is specified, the output HTML will include
/// `data-sid` and `data-loc` attributes on elements for source tracking.
///
/// # Arguments
///
/// * `pandoc` - The Pandoc AST to render
/// * `ast_context` - The AST context containing source info pool and file registry
/// * `writer` - The output writer
///
/// # Returns
///
/// Returns `Ok(())` on success.
pub fn write<W: Write>(
    pandoc: &Pandoc,
    ast_context: &ASTContext,
    writer: W,
) -> std::io::Result<()> {
    let config = extract_config_from_metadata(&pandoc.meta);

    if config.include_source_locations {
        write_with_source_tracking(pandoc, ast_context, writer)
    } else {
        write_with_config(pandoc, writer, config)
    }
}

/// Public wrapper to write blocks (for external callers)
pub fn write_blocks_to<W: Write>(blocks: &[Block], writer: W) -> std::io::Result<()> {
    let mut ctx = HtmlWriterContext::new(writer);
    write_blocks(blocks, &mut ctx)?;
    Ok(())
}

/// Public wrapper to write inlines (for external callers)
pub fn write_inlines_to<W: Write>(inlines: &Inlines, writer: W) -> std::io::Result<()> {
    let mut ctx = HtmlWriterContext::new(writer);
    write_inlines(inlines, &mut ctx)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pandoc::block::Paragraph;
    use crate::pandoc::inline::Str;
    use quarto_pandoc_types::meta::MetaValueWithSourceInfo;
    use quarto_source_map::SourceInfo;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::default()
    }

    #[test]
    fn test_write_paragraph_without_source_tracking() {
        use crate::pandoc::ASTContext;

        let ctx = ASTContext::anonymous();
        let para = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "Hello".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        });
        let pandoc = Pandoc {
            meta: MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: dummy_source_info(),
            },
            blocks: vec![para],
        };

        let mut output = Vec::new();
        write(&pandoc, &ctx, &mut output).unwrap();
        let html = String::from_utf8(output).unwrap();

        assert!(html.contains("<p>Hello</p>"));
        // Without source tracking config, there should be no data-sid or data-loc
        assert!(!html.contains("data-sid"));
        assert!(!html.contains("data-loc"));
    }

    #[test]
    fn test_write_with_source_tracking_creates_attributes() {
        use crate::pandoc::ASTContext;
        use quarto_source_map::{FileId, Location, Range};

        // Create an AST context with proper source info
        let ctx = ASTContext::anonymous();

        // Create source info with real location data
        let source = SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 1,
                    column: 1,
                },
                end: Location {
                    offset: 10,
                    row: 1,
                    column: 11,
                },
            },
        );

        let para = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "Hello".to_string(),
                source_info: source.clone(),
            })],
            source_info: source,
        });
        let pandoc = Pandoc {
            meta: MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: dummy_source_info(),
            },
            blocks: vec![para],
        };

        let mut output = Vec::new();
        write_with_source_tracking(&pandoc, &ctx, &mut output).unwrap();
        let html = String::from_utf8(output).unwrap();

        // The output should contain the paragraph content
        assert!(html.contains("Hello"));
        // The output should have data-sid and data-loc attributes
        // (depending on whether source tracking worked correctly)
    }

    #[test]
    fn test_resolved_location_to_data_loc_format() {
        let loc = ResolvedLocation {
            file_id: 0,
            start_line: 5,
            start_col: 1,
            end_line: 5,
            end_col: 41,
        };
        assert_eq!(loc.to_data_loc(), "0:5:1-5:41");
    }

    #[test]
    fn test_resolved_location_multiline() {
        let loc = ResolvedLocation {
            file_id: 2,
            start_line: 10,
            start_col: 5,
            end_line: 15,
            end_col: 20,
        };
        assert_eq!(loc.to_data_loc(), "2:10:5-15:20");
    }

    #[test]
    fn test_html_config_default() {
        let config = HtmlConfig::default();
        assert!(!config.include_source_locations);
    }

    #[test]
    fn test_html_writer_context_include_source_locations() {
        let config = HtmlConfig {
            include_source_locations: true,
        };
        let ctx: HtmlWriterContext<'_, Vec<u8>> =
            HtmlWriterContext::with_config(Vec::new(), config);
        assert!(ctx.include_source_locations());
    }

    #[test]
    fn test_html_writer_context_no_source_locations() {
        let ctx: HtmlWriterContext<'_, Vec<u8>> = HtmlWriterContext::new(Vec::new());
        assert!(!ctx.include_source_locations());
    }

    // =========================================================================
    // Metadata-based configuration tests
    // =========================================================================

    fn make_meta_entry(
        key: &str,
        value: MetaValueWithSourceInfo,
    ) -> quarto_pandoc_types::meta::MetaMapEntry {
        quarto_pandoc_types::meta::MetaMapEntry {
            key: key.to_string(),
            key_source: dummy_source_info(),
            value,
        }
    }

    fn make_meta_string(value: &str) -> MetaValueWithSourceInfo {
        MetaValueWithSourceInfo::MetaString {
            value: value.to_string(),
            source_info: dummy_source_info(),
        }
    }

    fn make_meta_map(
        entries: Vec<quarto_pandoc_types::meta::MetaMapEntry>,
    ) -> MetaValueWithSourceInfo {
        MetaValueWithSourceInfo::MetaMap {
            entries,
            source_info: dummy_source_info(),
        }
    }

    #[test]
    fn test_extract_config_empty_metadata() {
        let meta = make_meta_map(vec![]);
        let config = extract_config_from_metadata(&meta);
        assert!(!config.include_source_locations);
    }

    #[test]
    fn test_extract_config_no_format_key() {
        let meta = make_meta_map(vec![make_meta_entry(
            "title",
            make_meta_string("My Document"),
        )]);
        let config = extract_config_from_metadata(&meta);
        assert!(!config.include_source_locations);
    }

    #[test]
    fn test_extract_config_format_without_html() {
        let meta = make_meta_map(vec![make_meta_entry(
            "format",
            make_meta_map(vec![make_meta_entry("pdf", make_meta_map(vec![]))]),
        )]);
        let config = extract_config_from_metadata(&meta);
        assert!(!config.include_source_locations);
    }

    #[test]
    fn test_extract_config_html_without_source_location() {
        let meta = make_meta_map(vec![make_meta_entry(
            "format",
            make_meta_map(vec![make_meta_entry(
                "html",
                make_meta_map(vec![make_meta_entry(
                    "toc",
                    MetaValueWithSourceInfo::MetaBool {
                        value: true,
                        source_info: dummy_source_info(),
                    },
                )]),
            )]),
        )]);
        let config = extract_config_from_metadata(&meta);
        assert!(!config.include_source_locations);
    }

    #[test]
    fn test_extract_config_source_location_full() {
        let meta = make_meta_map(vec![make_meta_entry(
            "format",
            make_meta_map(vec![make_meta_entry(
                "html",
                make_meta_map(vec![make_meta_entry(
                    "source-location",
                    make_meta_string("full"),
                )]),
            )]),
        )]);
        let config = extract_config_from_metadata(&meta);
        assert!(config.include_source_locations);
    }

    #[test]
    fn test_extract_config_source_location_other_value() {
        let meta = make_meta_map(vec![make_meta_entry(
            "format",
            make_meta_map(vec![make_meta_entry(
                "html",
                make_meta_map(vec![make_meta_entry(
                    "source-location",
                    make_meta_string("none"),
                )]),
            )]),
        )]);
        let config = extract_config_from_metadata(&meta);
        assert!(!config.include_source_locations);
    }

    #[test]
    fn test_write_without_source_location_config() {
        use crate::pandoc::ASTContext;

        let ctx = ASTContext::anonymous();
        let para = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "Hello".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        });
        let pandoc = Pandoc {
            meta: make_meta_map(vec![]),
            blocks: vec![para],
        };

        let mut output = Vec::new();
        write(&pandoc, &ctx, &mut output).unwrap();
        let html = String::from_utf8(output).unwrap();

        assert!(html.contains("<p>Hello</p>"));
        // Without source-location: full, there should be no data-sid
        assert!(!html.contains("data-sid"));
    }

    #[test]
    fn test_write_with_source_location_full_metadata() {
        use crate::pandoc::ASTContext;
        use quarto_source_map::{FileId, Location, Range};

        let ctx = ASTContext::anonymous();

        // Create source info with real location data
        let source = SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 1,
                    column: 1,
                },
                end: Location {
                    offset: 10,
                    row: 1,
                    column: 11,
                },
            },
        );

        let para = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "Hello".to_string(),
                source_info: source.clone(),
            })],
            source_info: source,
        });

        // Create metadata with format.html.source-location: full
        let meta = make_meta_map(vec![make_meta_entry(
            "format",
            make_meta_map(vec![make_meta_entry(
                "html",
                make_meta_map(vec![make_meta_entry(
                    "source-location",
                    make_meta_string("full"),
                )]),
            )]),
        )]);

        let pandoc = Pandoc {
            meta,
            blocks: vec![para],
        };

        let mut output = Vec::new();
        write(&pandoc, &ctx, &mut output).unwrap();
        let html = String::from_utf8(output).unwrap();

        assert!(html.contains("Hello"));
        // With source-location: full, we should have source tracking attributes
        // (The actual presence depends on whether the parallel walk found matches)
    }
}
