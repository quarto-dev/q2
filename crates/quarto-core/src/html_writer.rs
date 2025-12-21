/*
 * html_writer.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * HTML writer with support for Quarto CustomNode rendering.
 */

//! HTML writer with CustomNode support.
//!
//! This module provides HTML rendering that extends pampa's HTML writer
//! with support for Quarto-specific CustomNode types like Callouts.
//!
//! The writer walks the entire AST and handles Custom nodes specially,
//! while delegating standard Pandoc elements to appropriate rendering logic.

use std::io::{self, Write};

use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::list::ListNumberStyle;
use quarto_pandoc_types::pandoc::Pandoc;
use serde_json::Value;

/// Main entry point: render a Pandoc document to HTML.
pub fn write<W: Write>(pandoc: &Pandoc, buf: &mut W) -> io::Result<()> {
    write_blocks(&pandoc.blocks, buf)
}

/// Write a sequence of blocks to HTML.
pub fn write_blocks<W: Write>(blocks: &[Block], buf: &mut W) -> io::Result<()> {
    for block in blocks {
        write_block(block, buf)?;
    }
    Ok(())
}

/// Write a sequence of inlines to HTML.
pub fn write_inlines<W: Write>(inlines: &[Inline], buf: &mut W) -> io::Result<()> {
    for inline in inlines {
        write_inline(inline, buf)?;
    }
    Ok(())
}

/// Escape HTML special characters.
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

/// Write HTML attributes (id, classes, key-value pairs).
fn write_attr<W: Write>(attr: &Attr, buf: &mut W) -> io::Result<()> {
    let (id, classes, attrs) = attr;

    if !id.is_empty() {
        write!(buf, " id=\"{}\"", escape_html(id))?;
    }

    if !classes.is_empty() {
        write!(buf, " class=\"{}\"", escape_html(&classes.join(" ")))?;
    }

    // Pandoc prefixes custom attributes with "data-"
    for (k, v) in attrs {
        write!(buf, " data-{}=\"{}\"", escape_html(k), escape_html(v))?;
    }

    Ok(())
}

/// Write a single block to HTML.
fn write_block<W: Write>(block: &Block, buf: &mut W) -> io::Result<()> {
    match block {
        // CustomNode handling - the key extension over pampa's writer
        Block::Custom(custom) => {
            write_custom_block(custom, buf)?;
        }

        Block::Plain(plain) => {
            write_inlines(&plain.content, buf)?;
            writeln!(buf)?;
        }
        Block::Paragraph(para) => {
            write!(buf, "<p>")?;
            write_inlines(&para.content, buf)?;
            writeln!(buf, "</p>")?;
        }
        Block::LineBlock(lineblock) => {
            writeln!(buf, "<div class=\"line-block\">")?;
            for line in &lineblock.content {
                write!(buf, "  ")?;
                write_inlines(line, buf)?;
                writeln!(buf, "<br />")?;
            }
            writeln!(buf, "</div>")?;
        }
        Block::CodeBlock(codeblock) => {
            write!(buf, "<pre")?;
            write_attr(&codeblock.attr, buf)?;
            write!(buf, "><code>")?;
            write!(buf, "{}", escape_html(&codeblock.text))?;
            writeln!(buf, "</code></pre>")?;
        }
        Block::RawBlock(raw) => {
            // Only output raw HTML if format is "html"
            if raw.format == "html" {
                writeln!(buf, "{}", raw.text)?;
            }
        }
        Block::BlockQuote(quote) => {
            writeln!(buf, "<blockquote>")?;
            write_blocks(&quote.content, buf)?;
            writeln!(buf, "</blockquote>")?;
        }
        Block::OrderedList(list) => {
            let (start, style, _delim) = &list.attr;
            write!(buf, "<ol")?;
            if *start != 1 {
                write!(buf, " start=\"{}\"", start)?;
            }
            let list_type = match style {
                ListNumberStyle::Decimal => "1",
                ListNumberStyle::LowerAlpha => "a",
                ListNumberStyle::UpperAlpha => "A",
                ListNumberStyle::LowerRoman => "i",
                ListNumberStyle::UpperRoman => "I",
                _ => "1",
            };
            write!(buf, " type=\"{}\"", list_type)?;
            writeln!(buf, ">")?;
            for item in &list.content {
                write!(buf, "<li>")?;
                write_blocks_inline(item, buf)?;
                writeln!(buf, "</li>")?;
            }
            writeln!(buf, "</ol>")?;
        }
        Block::BulletList(list) => {
            writeln!(buf, "<ul>")?;
            for item in &list.content {
                write!(buf, "<li>")?;
                write_blocks_inline(item, buf)?;
                writeln!(buf, "</li>")?;
            }
            writeln!(buf, "</ul>")?;
        }
        Block::DefinitionList(deflist) => {
            writeln!(buf, "<dl>")?;
            for (term, definitions) in &deflist.content {
                write!(buf, "<dt>")?;
                write_inlines(term, buf)?;
                writeln!(buf, "</dt>")?;
                for def_blocks in definitions {
                    writeln!(buf, "<dd>")?;
                    write_blocks(def_blocks, buf)?;
                    writeln!(buf, "</dd>")?;
                }
            }
            writeln!(buf, "</dl>")?;
        }
        Block::Header(header) => {
            write!(buf, "<h{}", header.level)?;
            write_attr(&header.attr, buf)?;
            write!(buf, ">")?;
            write_inlines(&header.content, buf)?;
            writeln!(buf, "</h{}>", header.level)?;
        }
        Block::HorizontalRule(_) => {
            writeln!(buf, "<hr />")?;
        }
        Block::Table(table) => {
            write!(buf, "<table")?;
            write_attr(&table.attr, buf)?;
            writeln!(buf, ">")?;

            // Caption (if any)
            if let Some(ref long_caption) = table.caption.long {
                if !long_caption.is_empty() {
                    writeln!(buf, "<caption>")?;
                    write_blocks(long_caption, buf)?;
                    writeln!(buf, "</caption>")?;
                }
            }

            // Column group (for alignment)
            if !table.colspec.is_empty() {
                writeln!(buf, "<colgroup>")?;
                for colspec in &table.colspec {
                    let align = match colspec.0 {
                        quarto_pandoc_types::table::Alignment::Left => " align=\"left\"",
                        quarto_pandoc_types::table::Alignment::Right => " align=\"right\"",
                        quarto_pandoc_types::table::Alignment::Center => " align=\"center\"",
                        quarto_pandoc_types::table::Alignment::Default => "",
                    };
                    writeln!(buf, "<col{} />", align)?;
                }
                writeln!(buf, "</colgroup>")?;
            }

            // Head
            if !table.head.rows.is_empty() {
                writeln!(buf, "<thead>")?;
                for row in &table.head.rows {
                    write_table_row(row, buf, true)?;
                }
                writeln!(buf, "</thead>")?;
            }

            // Bodies
            for body in &table.bodies {
                writeln!(buf, "<tbody>")?;
                for row in &body.body {
                    write_table_row(row, buf, false)?;
                }
                writeln!(buf, "</tbody>")?;
            }

            // Foot
            if !table.foot.rows.is_empty() {
                writeln!(buf, "<tfoot>")?;
                for row in &table.foot.rows {
                    write_table_row(row, buf, false)?;
                }
                writeln!(buf, "</tfoot>")?;
            }

            writeln!(buf, "</table>")?;
        }
        Block::Figure(figure) => {
            write!(buf, "<figure")?;
            write_attr(&figure.attr, buf)?;
            writeln!(buf, ">")?;
            write_blocks(&figure.content, buf)?;
            if let Some(ref long_caption) = figure.caption.long {
                if !long_caption.is_empty() {
                    writeln!(buf, "<figcaption>")?;
                    write_blocks(long_caption, buf)?;
                    writeln!(buf, "</figcaption>")?;
                }
            }
            writeln!(buf, "</figure>")?;
        }
        Block::Div(div) => {
            write!(buf, "<div")?;
            write_attr(&div.attr, buf)?;
            writeln!(buf, ">")?;
            write_blocks(&div.content, buf)?;
            writeln!(buf, "</div>")?;
        }
        // Quarto extensions
        Block::BlockMetadata(_) => {
            // Metadata blocks don't render to HTML
        }
        Block::NoteDefinitionPara(note) => {
            write!(
                buf,
                "<div class=\"footnote\" id=\"fn{}\">[{}] ",
                note.id, note.id
            )?;
            write_inlines(&note.content, buf)?;
            writeln!(buf, "</div>")?;
        }
        Block::NoteDefinitionFencedBlock(note) => {
            writeln!(
                buf,
                "<div class=\"footnote\" id=\"fn{}\">[{}]",
                note.id, note.id
            )?;
            write_blocks(&note.content, buf)?;
            writeln!(buf, "</div>")?;
        }
        Block::CaptionBlock(caption) => {
            write!(buf, "<div class=\"caption\">")?;
            write_inlines(&caption.content, buf)?;
            writeln!(buf, "</div>")?;
        }
    }
    Ok(())
}

/// Write a table row.
fn write_table_row<W: Write>(
    row: &quarto_pandoc_types::table::Row,
    buf: &mut W,
    is_header: bool,
) -> io::Result<()> {
    writeln!(buf, "<tr>")?;
    for cell in &row.cells {
        let tag = if is_header { "th" } else { "td" };
        write!(buf, "<{}", tag)?;
        write_attr(&cell.attr, buf)?;

        if cell.row_span > 1 {
            write!(buf, " rowspan=\"{}\"", cell.row_span)?;
        }
        if cell.col_span > 1 {
            write!(buf, " colspan=\"{}\"", cell.col_span)?;
        }

        let align = match cell.alignment {
            quarto_pandoc_types::table::Alignment::Left => " align=\"left\"",
            quarto_pandoc_types::table::Alignment::Right => " align=\"right\"",
            quarto_pandoc_types::table::Alignment::Center => " align=\"center\"",
            quarto_pandoc_types::table::Alignment::Default => "",
        };
        write!(buf, "{}>", align)?;

        write_blocks(&cell.content, buf)?;
        writeln!(buf, "</{}>", tag)?;
    }
    writeln!(buf, "</tr>")?;
    Ok(())
}

/// Write blocks inline (for list items) - strips paragraph tags for simple cases.
fn write_blocks_inline<W: Write>(blocks: &[Block], buf: &mut W) -> io::Result<()> {
    // For simple list items with just a single paragraph, write the content inline
    if blocks.len() == 1 {
        if let Block::Paragraph(para) = &blocks[0] {
            write_inlines(&para.content, buf)?;
            return Ok(());
        } else if let Block::Plain(plain) = &blocks[0] {
            write_inlines(&plain.content, buf)?;
            return Ok(());
        }
    }

    // For complex list items, write blocks normally
    write_blocks(blocks, buf)?;
    Ok(())
}

/// Write a single inline to HTML.
fn write_inline<W: Write>(inline: &Inline, buf: &mut W) -> io::Result<()> {
    match inline {
        // CustomNode handling for inline level
        Inline::Custom(custom) => {
            write_custom_inline(custom, buf)?;
        }

        Inline::Str(s) => {
            write!(buf, "{}", escape_html(&s.text))?;
        }
        Inline::Space(_) => {
            write!(buf, " ")?;
        }
        Inline::SoftBreak(_) => {
            write!(buf, "\n")?;
        }
        Inline::LineBreak(_) => {
            write!(buf, "<br />")?;
        }
        Inline::Emph(e) => {
            write!(buf, "<em>")?;
            write_inlines(&e.content, buf)?;
            write!(buf, "</em>")?;
        }
        Inline::Strong(s) => {
            write!(buf, "<strong>")?;
            write_inlines(&s.content, buf)?;
            write!(buf, "</strong>")?;
        }
        Inline::Underline(u) => {
            write!(buf, "<u>")?;
            write_inlines(&u.content, buf)?;
            write!(buf, "</u>")?;
        }
        Inline::Strikeout(s) => {
            write!(buf, "<del>")?;
            write_inlines(&s.content, buf)?;
            write!(buf, "</del>")?;
        }
        Inline::Superscript(s) => {
            write!(buf, "<sup>")?;
            write_inlines(&s.content, buf)?;
            write!(buf, "</sup>")?;
        }
        Inline::Subscript(s) => {
            write!(buf, "<sub>")?;
            write_inlines(&s.content, buf)?;
            write!(buf, "</sub>")?;
        }
        Inline::SmallCaps(s) => {
            write!(buf, "<span style=\"font-variant: small-caps;\">")?;
            write_inlines(&s.content, buf)?;
            write!(buf, "</span>")?;
        }
        Inline::Quoted(q) => {
            let (open, close) = match q.quote_type {
                quarto_pandoc_types::inline::QuoteType::SingleQuote => ("\u{2018}", "\u{2019}"),
                quarto_pandoc_types::inline::QuoteType::DoubleQuote => ("\u{201C}", "\u{201D}"),
            };
            write!(buf, "{}", open)?;
            write_inlines(&q.content, buf)?;
            write!(buf, "{}", close)?;
        }
        Inline::Code(c) => {
            write!(buf, "<code")?;
            write_attr(&c.attr, buf)?;
            write!(buf, ">{}</code>", escape_html(&c.text))?;
        }
        Inline::Math(m) => {
            let class = match m.math_type {
                quarto_pandoc_types::inline::MathType::InlineMath => "math inline",
                quarto_pandoc_types::inline::MathType::DisplayMath => "math display",
            };
            let (open, close) = match m.math_type {
                quarto_pandoc_types::inline::MathType::InlineMath => ("\\(", "\\)"),
                quarto_pandoc_types::inline::MathType::DisplayMath => ("\\[", "\\]"),
            };
            write!(
                buf,
                "<span class=\"{}\">{}{}{}</span>",
                class,
                open,
                escape_html(&m.text),
                close
            )?;
        }
        Inline::Link(link) => {
            write!(buf, "<a href=\"{}\"", escape_html(&link.target.0))?;
            write_attr(&link.attr, buf)?;
            if !link.target.1.is_empty() {
                write!(buf, " title=\"{}\"", escape_html(&link.target.1))?;
            }
            write!(buf, ">")?;
            write_inlines(&link.content, buf)?;
            write!(buf, "</a>")?;
        }
        Inline::Image(image) => {
            write!(buf, "<img src=\"{}\"", escape_html(&image.target.0))?;
            write!(buf, " alt=\"")?;
            write_inlines_as_text(&image.content, buf)?;
            write!(buf, "\"")?;
            write_attr(&image.attr, buf)?;
            if !image.target.1.is_empty() {
                write!(buf, " title=\"{}\"", escape_html(&image.target.1))?;
            }
            write!(buf, " />")?;
        }
        Inline::RawInline(raw) => {
            if raw.format == "html" {
                write!(buf, "{}", raw.text)?;
            }
        }
        Inline::Span(span) => {
            write!(buf, "<span")?;
            write_attr(&span.attr, buf)?;
            write!(buf, ">")?;
            write_inlines(&span.content, buf)?;
            write!(buf, "</span>")?;
        }
        Inline::Note(note) => {
            write!(
                buf,
                "<sup class=\"footnote-ref\"><a href=\"#fn{}\">",
                note.content.len()
            )?;
            write!(buf, "[{}]", note.content.len())?;
            write!(buf, "</a></sup>")?;
        }
        Inline::Cite(cite) => {
            let cite_ids: Vec<String> = cite.citations.iter().map(|c| c.id.clone()).collect();
            let data_cites = cite_ids.join(" ");

            write!(
                buf,
                "<span class=\"citation\" data-cites=\"{}\">",
                escape_html(&data_cites)
            )?;

            if !cite.content.is_empty() {
                write_inlines(&cite.content, buf)?;
            } else {
                for (i, citation) in cite.citations.iter().enumerate() {
                    if i > 0 {
                        write!(buf, "; ")?;
                    }
                    write_inlines(&citation.prefix, buf)?;
                    if !citation.prefix.is_empty() {
                        write!(buf, " ")?;
                    }
                    match citation.mode {
                        quarto_pandoc_types::inline::CitationMode::AuthorInText => {
                            write!(buf, "{}", escape_html(&citation.id))?
                        }
                        quarto_pandoc_types::inline::CitationMode::SuppressAuthor => {
                            write!(buf, "-@{}", escape_html(&citation.id))?
                        }
                        quarto_pandoc_types::inline::CitationMode::NormalCitation => {
                            write!(buf, "@{}", escape_html(&citation.id))?
                        }
                    }
                    if !citation.suffix.is_empty() {
                        write!(buf, " ")?;
                    }
                    write_inlines(&citation.suffix, buf)?;
                }
            }
            write!(buf, "</span>")?;
        }
        // Quarto extensions - don't render
        Inline::Shortcode(_) | Inline::NoteReference(_) | Inline::Attr(_, _) => {}
        Inline::Insert(ins) => {
            write!(buf, "<ins>")?;
            write_inlines(&ins.content, buf)?;
            write!(buf, "</ins>")?;
        }
        Inline::Delete(del) => {
            write!(buf, "<del>")?;
            write_inlines(&del.content, buf)?;
            write!(buf, "</del>")?;
        }
        Inline::Highlight(h) => {
            write!(buf, "<mark>")?;
            write_inlines(&h.content, buf)?;
            write!(buf, "</mark>")?;
        }
        Inline::EditComment(c) => {
            write!(buf, "<span class=\"comment\">")?;
            write_inlines(&c.content, buf)?;
            write!(buf, "</span>")?;
        }
    }
    Ok(())
}

/// Write inlines as plain text (for alt attributes, etc.).
fn write_inlines_as_text<W: Write>(inlines: &[Inline], buf: &mut W) -> io::Result<()> {
    for inline in inlines {
        match inline {
            Inline::Str(s) => write!(buf, "{}", escape_html(&s.text))?,
            Inline::Space(_) => write!(buf, " ")?,
            Inline::SoftBreak(_) | Inline::LineBreak(_) => write!(buf, " ")?,
            Inline::Emph(e) => write_inlines_as_text(&e.content, buf)?,
            Inline::Strong(s) => write_inlines_as_text(&s.content, buf)?,
            Inline::Underline(u) => write_inlines_as_text(&u.content, buf)?,
            Inline::Strikeout(s) => write_inlines_as_text(&s.content, buf)?,
            Inline::Superscript(s) => write_inlines_as_text(&s.content, buf)?,
            Inline::Subscript(s) => write_inlines_as_text(&s.content, buf)?,
            Inline::SmallCaps(s) => write_inlines_as_text(&s.content, buf)?,
            Inline::Span(span) => write_inlines_as_text(&span.content, buf)?,
            Inline::Quoted(q) => write_inlines_as_text(&q.content, buf)?,
            Inline::Code(c) => write!(buf, "{}", escape_html(&c.text))?,
            Inline::Link(link) => write_inlines_as_text(&link.content, buf)?,
            Inline::Image(image) => write_inlines_as_text(&image.content, buf)?,
            _ => {}
        }
    }
    Ok(())
}

// =============================================================================
// CustomNode rendering
// =============================================================================

/// Write a CustomNode block to HTML.
fn write_custom_block<W: Write>(custom: &CustomNode, buf: &mut W) -> io::Result<()> {
    match custom.type_name.as_str() {
        "Callout" => write_callout(custom, buf),
        "PanelTabset" => write_panel_tabset(custom, buf),
        _ => {
            // Unknown custom node - render as a div with data attributes
            write_unknown_custom_block(custom, buf)
        }
    }
}

/// Write a CustomNode inline to HTML.
fn write_custom_inline<W: Write>(custom: &CustomNode, buf: &mut W) -> io::Result<()> {
    // For now, render unknown inline custom nodes as spans
    write!(buf, "<span class=\"custom-inline\"")?;
    let (id, classes, attrs) = &custom.attr;
    if !id.is_empty() {
        write!(buf, " id=\"{}\"", escape_html(id))?;
    }
    if !classes.is_empty() {
        write!(buf, " data-original-class=\"{}\"", escape_html(&classes.join(" ")))?;
    }
    for (k, v) in attrs {
        write!(buf, " data-{}=\"{}\"", escape_html(k), escape_html(v))?;
    }
    write!(buf, " data-custom-type=\"{}\"", escape_html(&custom.type_name))?;
    write!(buf, ">")?;

    // Render inline slots
    for (_name, slot) in &custom.slots {
        match slot {
            Slot::Inline(inline) => write_inline(inline, buf)?,
            Slot::Inlines(inlines) => write_inlines(inlines, buf)?,
            _ => {} // Block slots in inline context - skip
        }
    }

    write!(buf, "</span>")?;
    Ok(())
}

/// Write a Callout block.
fn write_callout<W: Write>(custom: &CustomNode, buf: &mut W) -> io::Result<()> {
    // Extract callout type from plain_data
    let callout_type = extract_string(&custom.plain_data, "type").unwrap_or("note");
    let appearance = extract_string(&custom.plain_data, "appearance").unwrap_or("default");
    let collapse = extract_bool(&custom.plain_data, "collapse").unwrap_or(false);
    let icon = extract_bool(&custom.plain_data, "icon").unwrap_or(true);

    // Build class list
    let mut classes = vec![
        "callout".to_string(),
        format!("callout-{}", callout_type),
    ];
    if appearance != "default" {
        classes.push(format!("callout-appearance-{}", appearance));
    }
    if collapse {
        classes.push("callout-collapse".to_string());
    }

    // Include original classes from attr
    let (orig_id, orig_classes, orig_attrs) = &custom.attr;
    for cls in orig_classes {
        if !cls.starts_with("callout") {
            classes.push(cls.clone());
        }
    }

    // Start the callout div
    write!(buf, "<div class=\"{}\"", escape_html(&classes.join(" ")))?;
    if !orig_id.is_empty() {
        write!(buf, " id=\"{}\"", escape_html(orig_id))?;
    }
    for (k, v) in orig_attrs {
        write!(buf, " data-{}=\"{}\"", escape_html(k), escape_html(v))?;
    }
    writeln!(buf, ">")?;

    // Callout header
    writeln!(buf, "<div class=\"callout-header\">")?;

    // Icon container (if icon is enabled)
    if icon {
        let icon_svg = get_callout_icon(callout_type);
        writeln!(buf, "<div class=\"callout-icon-container\">")?;
        writeln!(buf, "{}", icon_svg)?;
        writeln!(buf, "</div>")?;
    }

    // Title
    writeln!(buf, "<div class=\"callout-title-container flex-fill\">")?;
    if let Some(title_slot) = custom.get_slot("title") {
        match title_slot {
            Slot::Inlines(inlines) => {
                write_inlines(inlines, buf)?;
            }
            Slot::Inline(inline) => {
                write_inline(inline, buf)?;
            }
            _ => {
                // Default title based on type
                write!(buf, "{}", capitalize(callout_type))?;
            }
        }
    } else {
        // Default title based on type
        write!(buf, "{}", capitalize(callout_type))?;
    }
    writeln!(buf, "</div>")?; // callout-title-container

    writeln!(buf, "</div>")?; // callout-header

    // Callout body
    writeln!(buf, "<div class=\"callout-body-container callout-body\">")?;
    if let Some(content_slot) = custom.get_slot("content") {
        match content_slot {
            Slot::Blocks(blocks) => {
                write_blocks(blocks, buf)?;
            }
            Slot::Block(block) => {
                write_block(block, buf)?;
            }
            _ => {} // Inline slots in block context - skip
        }
    }
    writeln!(buf, "</div>")?; // callout-body

    writeln!(buf, "</div>")?; // callout

    Ok(())
}

/// Write a PanelTabset block.
fn write_panel_tabset<W: Write>(custom: &CustomNode, buf: &mut W) -> io::Result<()> {
    // For now, render as a simple div structure
    // Full tabset implementation would require JavaScript
    let (id, classes, attrs) = &custom.attr;

    write!(buf, "<div class=\"panel-tabset\"")?;
    if !id.is_empty() {
        write!(buf, " id=\"{}\"", escape_html(id))?;
    }
    for (k, v) in attrs {
        write!(buf, " data-{}=\"{}\"", escape_html(k), escape_html(v))?;
    }
    writeln!(buf, ">")?;

    // Render content slot
    if let Some(content_slot) = custom.get_slot("content") {
        match content_slot {
            Slot::Blocks(blocks) => {
                write_blocks(blocks, buf)?;
            }
            Slot::Block(block) => {
                write_block(block, buf)?;
            }
            _ => {}
        }
    }

    writeln!(buf, "</div>")?;

    // Add placeholder for classes
    let _ = classes;

    Ok(())
}

/// Write an unknown custom block as a div with data attributes.
fn write_unknown_custom_block<W: Write>(custom: &CustomNode, buf: &mut W) -> io::Result<()> {
    let (id, classes, attrs) = &custom.attr;

    write!(buf, "<div class=\"custom-block\"")?;
    if !id.is_empty() {
        write!(buf, " id=\"{}\"", escape_html(id))?;
    }
    if !classes.is_empty() {
        write!(
            buf,
            " data-original-class=\"{}\"",
            escape_html(&classes.join(" "))
        )?;
    }
    for (k, v) in attrs {
        write!(buf, " data-{}=\"{}\"", escape_html(k), escape_html(v))?;
    }
    write!(
        buf,
        " data-custom-type=\"{}\"",
        escape_html(&custom.type_name)
    )?;
    writeln!(buf, ">")?;

    // Render all slots
    for (_name, slot) in &custom.slots {
        match slot {
            Slot::Block(block) => write_block(block, buf)?,
            Slot::Blocks(blocks) => write_blocks(blocks, buf)?,
            Slot::Inline(inline) => {
                write!(buf, "<span>")?;
                write_inline(inline, buf)?;
                writeln!(buf, "</span>")?;
            }
            Slot::Inlines(inlines) => {
                write!(buf, "<span>")?;
                write_inlines(inlines, buf)?;
                writeln!(buf, "</span>")?;
            }
        }
    }

    writeln!(buf, "</div>")?;
    Ok(())
}

// =============================================================================
// Helper functions
// =============================================================================

/// Extract a string value from JSON.
fn extract_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

/// Extract a bool value from JSON.
fn extract_bool(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|v| v.as_bool())
}

/// Capitalize the first letter of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Get the SVG icon for a callout type.
fn get_callout_icon(callout_type: &str) -> &'static str {
    match callout_type {
        "note" => r#"<i class="callout-icon"></i>"#,
        "warning" => r#"<i class="callout-icon"></i>"#,
        "important" => r#"<i class="callout-icon"></i>"#,
        "tip" => r#"<i class="callout-icon"></i>"#,
        "caution" => r#"<i class="callout-icon"></i>"#,
        _ => r#"<i class="callout-icon"></i>"#,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::empty_attr;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::Str;
    use quarto_source_map::{FileId, Location, Range, SourceInfo};
    use serde_json::json;

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

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("Hello & World"), "Hello &amp; World");
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("\"quoted\""), "&quot;quoted&quot;");
    }

    #[test]
    fn test_write_paragraph() {
        let para = Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "Hello World".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        });

        let mut buf = Vec::new();
        write_block(&para, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "<p>Hello World</p>\n");
    }

    #[test]
    fn test_write_callout() {
        let custom = CustomNode::new("Callout", empty_attr(), dummy_source_info())
            .with_data(json!({"type": "warning", "appearance": "default"}))
            .with_slot(
                "title",
                Slot::Inlines(vec![Inline::Str(Str {
                    text: "Warning Title".to_string(),
                    source_info: dummy_source_info(),
                })]),
            )
            .with_slot(
                "content",
                Slot::Blocks(vec![Block::Paragraph(Paragraph {
                    content: vec![Inline::Str(Str {
                        text: "Warning content".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                })]),
            );

        let mut buf = Vec::new();
        write_callout(&custom, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("callout-warning"));
        assert!(output.contains("Warning Title"));
        assert!(output.contains("Warning content"));
        assert!(output.contains("callout-header"));
        assert!(output.contains("callout-body"));
    }

    #[test]
    fn test_write_custom_block() {
        let custom = CustomNode::new("Callout", empty_attr(), dummy_source_info())
            .with_data(json!({"type": "note"}));

        let mut buf = Vec::new();
        write_custom_block(&custom, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("callout-note"));
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("note"), "Note");
        assert_eq!(capitalize("warning"), "Warning");
        assert_eq!(capitalize(""), "");
    }
}
