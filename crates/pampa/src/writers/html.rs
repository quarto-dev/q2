/*
 * html.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{Attr, Block, CitationMode, Inline, Inlines, Pandoc};

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
fn write_attr<T: std::io::Write>(attr: &Attr, buf: &mut T) -> std::io::Result<()> {
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

/// Write inline elements
fn write_inline<T: std::io::Write>(inline: &Inline, buf: &mut T) -> std::io::Result<()> {
    match inline {
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
                crate::pandoc::QuoteType::SingleQuote => ("\u{2018}", "\u{2019}"),
                crate::pandoc::QuoteType::DoubleQuote => ("\u{201C}", "\u{201D}"),
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
                crate::pandoc::MathType::InlineMath => "math inline",
                crate::pandoc::MathType::DisplayMath => "math display",
            };
            // Use \(...\) for inline math and \[...\] for display math
            let (open, close) = match m.math_type {
                crate::pandoc::MathType::InlineMath => ("\\(", "\\)"),
                crate::pandoc::MathType::DisplayMath => ("\\[", "\\]"),
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
            // For alt text, we need to extract plain text from inlines
            write_inlines_as_text(&image.content, buf)?;
            write!(buf, "\"")?;
            write_attr(&image.attr, buf)?;
            if !image.target.1.is_empty() {
                write!(buf, " title=\"{}\"", escape_html(&image.target.1))?;
            }
            write!(buf, " />")?;
        }
        Inline::RawInline(raw) => {
            // Only output raw HTML if format is "html"
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
            // Footnotes are rendered as superscript with a link
            write!(
                buf,
                "<sup class=\"footnote-ref\"><a href=\"#fn{}\">",
                note.content.len()
            )?;
            write!(buf, "[{}]", note.content.len())?;
            write!(buf, "</a></sup>")?;
            // Note: Proper footnote handling would require collecting all footnotes
            // and rendering them at the end of the document
        }
        Inline::Cite(cite) => {
            // Collect all citation IDs for data-cites attribute
            let cite_ids: Vec<String> = cite.citations.iter().map(|c| c.id.clone()).collect();
            let data_cites = cite_ids.join(" ");

            write!(
                buf,
                "<span class=\"citation\" data-cites=\"{}\">",
                escape_html(&data_cites)
            )?;

            // Pandoc outputs citation content if present, otherwise builds citation text
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
                        CitationMode::AuthorInText => write!(buf, "{}", escape_html(&citation.id))?,
                        CitationMode::SuppressAuthor => {
                            write!(buf, "-@{}", escape_html(&citation.id))?
                        }
                        CitationMode::NormalCitation => {
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
        // Quarto extensions - render as raw HTML or skip
        Inline::Shortcode(_) | Inline::NoteReference(_) | Inline::Attr(_, _) => {
            // These should not appear in final output
        }
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

/// Write a sequence of inlines
pub fn write_inlines<T: std::io::Write>(inlines: &Inlines, buf: &mut T) -> std::io::Result<()> {
    for inline in inlines {
        write_inline(inline, buf)?;
    }
    Ok(())
}

/// Write inlines as plain text (for alt attributes, etc.)
fn write_inlines_as_text<T: std::io::Write>(inlines: &Inlines, buf: &mut T) -> std::io::Result<()> {
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

/// Write block elements
fn write_block<T: std::io::Write>(block: &Block, buf: &mut T) -> std::io::Result<()> {
    match block {
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
            // Pandoc uses type attribute instead of style
            let list_type = match style {
                crate::pandoc::ListNumberStyle::Decimal => "1",
                crate::pandoc::ListNumberStyle::LowerAlpha => "a",
                crate::pandoc::ListNumberStyle::UpperAlpha => "A",
                crate::pandoc::ListNumberStyle::LowerRoman => "i",
                crate::pandoc::ListNumberStyle::UpperRoman => "I",
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
                        crate::pandoc::table::Alignment::Left => " align=\"left\"",
                        crate::pandoc::table::Alignment::Right => " align=\"right\"",
                        crate::pandoc::table::Alignment::Center => " align=\"center\"",
                        crate::pandoc::table::Alignment::Default => "",
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
            // Note definitions would typically be collected and rendered as footnotes
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
            // Caption blocks are rendered as divs with caption class
            write!(buf, "<div class=\"caption\">")?;
            write_inlines(&caption.content, buf)?;
            writeln!(buf, "</div>")?;
        }
    }
    Ok(())
}

/// Write a table row
fn write_table_row<T: std::io::Write>(
    row: &crate::pandoc::table::Row,
    buf: &mut T,
    is_header: bool,
) -> std::io::Result<()> {
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
            crate::pandoc::table::Alignment::Left => " align=\"left\"",
            crate::pandoc::table::Alignment::Right => " align=\"right\"",
            crate::pandoc::table::Alignment::Center => " align=\"center\"",
            crate::pandoc::table::Alignment::Default => "",
        };
        write!(buf, "{}>", align)?;

        write_blocks(&cell.content, buf)?;
        writeln!(buf, "</{}>", tag)?;
    }
    writeln!(buf, "</tr>")?;
    Ok(())
}

/// Write a sequence of blocks
pub fn write_blocks<T: std::io::Write>(blocks: &[Block], buf: &mut T) -> std::io::Result<()> {
    for block in blocks {
        write_block(block, buf)?;
    }
    Ok(())
}

/// Write blocks inline (for list items) - strips paragraph tags for simple cases
fn write_blocks_inline<T: std::io::Write>(blocks: &[Block], buf: &mut T) -> std::io::Result<()> {
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

/// Main entry point for the HTML writer
pub fn write<T: std::io::Write>(pandoc: &Pandoc, buf: &mut T) -> std::io::Result<()> {
    write_blocks(&pandoc.blocks, buf)?;
    Ok(())
}
