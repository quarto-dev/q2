/*
 * native.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{
    Attr, Block, Citation, CitationMode, Inline, ListNumberDelim, MathType, Pandoc, QuoteType,
};

fn write_safe_string<T: std::io::Write>(text: &str, buf: &mut T) -> std::io::Result<()> {
    write!(buf, "\"")?;
    for ch in text.chars() {
        match ch {
            '\\' => write!(buf, "\\\\"),
            '"' => write!(buf, "\\\""),
            '\n' => write!(buf, "\\n"),
            _ => write!(buf, "{}", ch),
        }?
    }
    write!(buf, "\"")?;
    Ok(())
}

fn write_native_attr<T: std::io::Write>(attr: &Attr, buf: &mut T) -> std::io::Result<()> {
    let (id, classes, attrs) = attr;
    write!(buf, "( ")?;
    write_safe_string(&id, buf)?;
    write!(buf, " , [")?;

    for (i, class) in classes.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_safe_string(&class, buf)?;
    }

    write!(buf, "] , [")?;

    for (i, (k, v)) in attrs.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write!(buf, "(")?;
        write_safe_string(k, buf)?;
        write!(buf, ", ")?;
        write_safe_string(v, buf)?;
        write!(buf, ")")?;
    }

    write!(buf, "] )")?;
    Ok(())
}

fn write_inline_math_type<T: std::io::Write>(
    math_type: &MathType,
    buf: &mut T,
) -> std::io::Result<()> {
    match math_type {
        MathType::InlineMath => write!(buf, "InlineMath"),
        MathType::DisplayMath => write!(buf, "DisplayMath"),
    }
}

fn write_native_quote_type<T: std::io::Write>(
    quote_type: &QuoteType,
    buf: &mut T,
) -> std::io::Result<()> {
    match quote_type {
        QuoteType::SingleQuote => write!(buf, "SingleQuote"),
        QuoteType::DoubleQuote => write!(buf, "DoubleQuote"),
    }
}

fn write_native_alignment<T: std::io::Write>(
    alignment: &crate::pandoc::Alignment,
    buf: &mut T,
) -> std::io::Result<()> {
    match alignment {
        crate::pandoc::Alignment::Left => write!(buf, "AlignLeft"),
        crate::pandoc::Alignment::Right => write!(buf, "AlignRight"),
        crate::pandoc::Alignment::Center => write!(buf, "AlignCenter"),
        crate::pandoc::Alignment::Default => write!(buf, "AlignDefault"),
    }
}

fn write_native_colwidth<T: std::io::Write>(
    colwidth: &crate::pandoc::ColWidth,
    buf: &mut T,
) -> std::io::Result<()> {
    match colwidth {
        crate::pandoc::ColWidth::Default => write!(buf, "ColWidthDefault"),
        crate::pandoc::ColWidth::Percentage(percentage) => {
            // FIXME
            panic!("ColWidthPercentage is not implemented yet: {}", percentage);
        }
    }
}

fn write_native_table_body<T: std::io::Write>(
    table_body: &crate::pandoc::TableBody,
    buf: &mut T,
) -> std::io::Result<()> {
    write!(buf, "TableBody ")?;
    write_native_attr(&table_body.attr, buf)?;
    write!(buf, " (RowHeadColumns {}) ", table_body.rowhead_columns)?;
    write_native_rows(&table_body.head, buf)?;
    write!(buf, " ")?;
    write_native_rows(&table_body.body, buf)?;
    Ok(())
}

fn write_inlines<T: std::io::Write>(inlines: &[Inline], buf: &mut T) -> std::io::Result<()> {
    write!(buf, "[")?;
    for (i, inline) in inlines.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_inline(inline, buf)?;
    }
    write!(buf, "]")?;
    Ok(())
}

fn write_citation_mode<T: std::io::Write>(mode: &CitationMode, buf: &mut T) -> std::io::Result<()> {
    match mode {
        CitationMode::NormalCitation => write!(buf, "NormalCitation"),
        CitationMode::SuppressAuthor => write!(buf, "SuppressAuthor"),
        CitationMode::AuthorInText => write!(buf, "AuthorInText"),
    }
}
fn write_native_cell<T: std::io::Write>(
    cell: &crate::pandoc::Cell,
    buf: &mut T,
) -> std::io::Result<()> {
    write!(buf, "Cell ")?;
    write_native_attr(&cell.attr, buf)?;
    write!(buf, " ")?;
    write_native_alignment(&cell.alignment, buf)?;
    write!(
        buf,
        " (RowSpan {}) (ColSpan {})",
        cell.row_span, cell.col_span
    )?;
    write!(buf, " [")?;
    for (i, block) in cell.content.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_block(block, buf)?;
    }
    write!(buf, "] ")?;
    Ok(())
}

fn write_native_row<T: std::io::Write>(
    row: &crate::pandoc::Row,
    buf: &mut T,
) -> std::io::Result<()> {
    write!(buf, "Row ")?;
    write_native_attr(&row.attr, buf)?;
    write!(buf, " [")?;
    for (i, cell) in row.cells.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_native_cell(cell, buf)?;
    }
    write!(buf, "] ")?;
    Ok(())
}

fn write_native_rows<T: std::io::Write>(
    rows: &Vec<crate::pandoc::Row>,
    buf: &mut T,
) -> std::io::Result<()> {
    write!(buf, "[")?;
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_native_row(row, buf)?;
    }
    write!(buf, "]")?;
    Ok(())
}

fn write_native_table_foot<T: std::io::Write>(
    foot: &crate::pandoc::TableFoot,
    buf: &mut T,
) -> std::io::Result<()> {
    write!(buf, "(TableFoot ")?;
    write_native_attr(&foot.attr, buf)?;
    write!(buf, " ")?;
    write_native_rows(&foot.rows, buf)?;
    write!(buf, " )")?;
    Ok(())
}

fn write_inline<T: std::io::Write>(text: &Inline, buf: &mut T) -> std::io::Result<()> {
    match text {
        Inline::Math(math_struct) => {
            write!(buf, "Math ")?;
            write_inline_math_type(&math_struct.math_type, buf)?;
            write!(buf, " ")?;
            write_safe_string(&math_struct.text, buf)?;
        }
        Inline::Space(_) => write!(buf, "Space")?,
        Inline::SoftBreak(_) => write!(buf, "SoftBreak")?,
        Inline::LineBreak(_) => write!(buf, "LineBreak")?,
        Inline::Str(str_struct) => {
            write!(buf, "Str ")?;
            write_safe_string(&str_struct.text, buf)?;
        }
        Inline::Emph(emph_struct) => {
            write!(buf, "Emph ")?;
            write_inlines(&emph_struct.content, buf)?;
        }
        Inline::Underline(underline_struct) => {
            write!(buf, "Underline ")?;
            write_inlines(&underline_struct.content, buf)?;
        }
        Inline::SmallCaps(smallcaps_struct) => {
            write!(buf, "SmallCaps ")?;
            write_inlines(&smallcaps_struct.content, buf)?;
        }
        Inline::Superscript(superscript_struct) => {
            write!(buf, "Superscript ")?;
            write_inlines(&superscript_struct.content, buf)?;
        }
        Inline::Strong(strong_struct) => {
            write!(buf, "Strong ")?;
            write_inlines(&strong_struct.content, buf)?;
        }
        Inline::Span(span_struct) => {
            write!(buf, "Span ")?;
            write_native_attr(&span_struct.attr, buf)?;
            write!(buf, " ")?;
            write_inlines(&span_struct.content, buf)?;
        }
        Inline::Link(link_struct) => {
            let (url, title) = &link_struct.target;
            write!(buf, "Link ")?;
            write_native_attr(&link_struct.attr, buf)?;
            write!(buf, " ")?;
            write_inlines(&link_struct.content, buf)?;
            write!(buf, " (")?;
            write_safe_string(url, buf)?;
            write!(buf, " , ")?;
            write_safe_string(title, buf)?;
            write!(buf, ")")?;
        }
        Inline::Code(code_struct) => {
            write!(buf, "Code ")?;
            write_native_attr(&code_struct.attr, buf)?;
            write!(buf, " ")?;
            write_safe_string(&code_struct.text, buf)?;
        }
        Inline::RawInline(raw_struct) => {
            write!(buf, "RawInline (Format ")?;
            write_safe_string(&raw_struct.format, buf)?;
            write!(buf, ") ")?;
            write_safe_string(&raw_struct.text, buf)?;
        }
        Inline::Quoted(quoted_struct) => {
            write!(buf, "Quoted ")?;
            write_native_quote_type(&quoted_struct.quote_type, buf)?;
            write!(buf, " ")?;
            write_inlines(&quoted_struct.content, buf)?;
        }
        Inline::Note(note_struct) => {
            write!(buf, "Note [")?;
            for (i, block) in note_struct.content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_block(block, buf)?;
            }
            write!(buf, "]")?;
        }
        Inline::Image(image_struct) => {
            let (url, title) = &image_struct.target;
            write!(buf, "Image ")?;
            write_native_attr(&image_struct.attr, buf)?;
            write!(buf, " ")?;
            write_inlines(&image_struct.content, buf)?;
            write!(buf, " (")?;
            write_safe_string(url, buf)?;
            write!(buf, " , ")?;
            write_safe_string(title, buf)?;
            write!(buf, ")")?;
        }
        Inline::Subscript(subscript_struct) => {
            write!(buf, "Subscript ")?;
            write_inlines(&subscript_struct.content, buf)?;
        }
        Inline::Strikeout(strikeout_struct) => {
            write!(buf, "Strikeout ")?;
            write_inlines(&strikeout_struct.content, buf)?;
        }
        Inline::Cite(cite_struct) => {
            write!(buf, "Cite [")?;
            for (
                i,
                Citation {
                    mode,
                    note_num,
                    hash,
                    id,
                    prefix,
                    suffix,
                },
            ) in cite_struct.citations.iter().enumerate()
            {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write!(buf, "Citation {{ citationId = ")?;
                write_safe_string(id, buf)?;
                write!(buf, ", citationPrefix = ")?;
                write_inlines(prefix, buf)?;
                write!(buf, ", citationSuffix = ")?;
                write_inlines(suffix, buf)?;
                write!(buf, ", citationMode = ")?;
                write_citation_mode(mode, buf)?;
                write!(
                    buf,
                    ", citationNoteNum = {}, citationHash = {} }}",
                    note_num, hash
                )?;
            }
            write!(buf, "] ")?;
            write_inlines(&cite_struct.content, buf)?;
        }
        _ => panic!("Unsupported inline type: {:?}", text),
    }
    Ok(())
}

fn write_list_number_delim<T: std::io::Write>(
    delim: &crate::pandoc::ListNumberDelim,
    buf: &mut T,
) -> std::io::Result<()> {
    match delim {
        ListNumberDelim::Period => write!(buf, "Period"),
        ListNumberDelim::OneParen => write!(buf, "OneParen"),
        ListNumberDelim::TwoParens => write!(buf, "TwoParens"),
        ListNumberDelim::Default => write!(buf, "Period"), // Is this supposed to be the default?
    }
}

fn write_list_number_style<T: std::io::Write>(
    style: &crate::pandoc::ListNumberStyle,
    buf: &mut T,
) -> std::io::Result<()> {
    match style {
        crate::pandoc::ListNumberStyle::Decimal => write!(buf, "Decimal"),
        crate::pandoc::ListNumberStyle::LowerAlpha => write!(buf, "LowerAlpha"),
        crate::pandoc::ListNumberStyle::UpperAlpha => write!(buf, "UpperAlpha"),
        crate::pandoc::ListNumberStyle::LowerRoman => write!(buf, "LowerRoman"),
        crate::pandoc::ListNumberStyle::UpperRoman => write!(buf, "UpperRoman"),
        crate::pandoc::ListNumberStyle::Example => write!(buf, "Example"),
        crate::pandoc::ListNumberStyle::Default => write!(buf, "Decimal"), // Is this supposed to be the default?
    }
}

fn write_short_caption<T: std::io::Write>(
    caption: &Option<Vec<Inline>>,
    buf: &mut T,
) -> std::io::Result<()> {
    match caption {
        Some(text) => write_inlines(text, buf),
        None => write!(buf, "Nothing"),
    }
}

fn write_long_caption<T: std::io::Write>(
    caption: &Option<Vec<Block>>,
    buf: &mut T,
) -> std::io::Result<()> {
    match caption {
        Some(blocks) => {
            write!(buf, "[ ")?;
            for (i, block) in blocks.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_block(block, buf)?;
            }
            write!(buf, " ]")?;
        }
        None => write!(buf, "[]")?,
    }
    Ok(())
}

fn write_caption<T: std::io::Write>(
    caption: &crate::pandoc::Caption,
    buf: &mut T,
) -> std::io::Result<()> {
    write!(buf, "(Caption ")?;
    write_short_caption(&caption.short, buf)?;
    write!(buf, " ")?;
    write_long_caption(&caption.long, buf)?;
    write!(buf, ")")?;
    Ok(())
}

fn write_block<T: std::io::Write>(block: &Block, buf: &mut T) -> std::io::Result<()> {
    match block {
        Block::Plain(crate::pandoc::Plain { content, .. }) => {
            write!(buf, "Plain ")?;
            write_inlines(content, buf)?;
        }
        Block::Paragraph(crate::pandoc::Paragraph { content, .. }) => {
            write!(buf, "Para ")?;
            write_inlines(content, buf)?;
        }
        Block::CodeBlock(crate::pandoc::CodeBlock {
            attr,
            text,
            source_info: _,
            source_info_qsm: _,
        }) => {
            write!(buf, "CodeBlock ")?;
            write_native_attr(attr, buf)?;
            write!(buf, " ")?;
            write_safe_string(text, buf)?;
        }
        Block::RawBlock(crate::pandoc::RawBlock { format, text, .. }) => {
            write!(buf, "RawBlock (Format ")?;
            write_safe_string(format, buf)?;
            write!(buf, ") ")?;
            write_safe_string(text, buf)?;
        }
        Block::BulletList(crate::pandoc::BulletList { content, .. }) => {
            write!(buf, "BulletList [")?;
            for (i, item) in content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write!(buf, "[")?;
                for (j, block) in item.iter().enumerate() {
                    if j > 0 {
                        write!(buf, ", ")?;
                    }
                    write_block(block, buf)?;
                }
                write!(buf, "]")?;
            }
            write!(buf, "]")?;
        }
        Block::OrderedList(crate::pandoc::OrderedList { content, attr, .. }) => {
            let (number, style, delim) = attr;
            write!(buf, "OrderedList ({}, ", number)?;
            write_list_number_style(style, buf)?;
            write!(buf, ", ")?;
            write_list_number_delim(delim, buf)?;
            write!(buf, ") [")?;
            for (i, item) in content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write!(buf, "[")?;
                for (j, block) in item.iter().enumerate() {
                    if j > 0 {
                        write!(buf, ", ")?;
                    }
                    write_block(block, buf)?;
                }
                write!(buf, "]")?;
            }
            write!(buf, "]")?;
        }
        Block::BlockQuote(crate::pandoc::BlockQuote { content, .. }) => {
            write!(buf, "BlockQuote [")?;
            for (i, block) in content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_block(block, buf)?;
            }
            write!(buf, "]")?;
        }
        Block::Div(crate::pandoc::Div { attr, content, .. }) => {
            write!(buf, "Div ")?;
            write_native_attr(attr, buf)?;
            write!(buf, " [")?;
            for (i, block) in content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_block(block, buf)?;
            }
            write!(buf, "]")?;
        }
        Block::Figure(crate::pandoc::Figure {
            attr,
            caption,
            content,
            ..
        }) => {
            write!(buf, "Figure ")?;
            write_native_attr(attr, buf)?;
            write!(buf, " ")?;
            write_caption(caption, buf)?;
            write!(buf, " [")?;
            for (i, block) in content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_block(block, buf)?;
            }
            write!(buf, "]")?;
        }
        Block::Header(crate::pandoc::Header {
            level,
            attr,
            content,
            ..
        }) => {
            write!(buf, "Header {} ", level)?;
            write_native_attr(attr, buf)?;
            write!(buf, " ")?;
            write_inlines(content, buf)?;
        }
        Block::HorizontalRule(crate::pandoc::HorizontalRule { .. }) => {
            write!(buf, "HorizontalRule")?
        }
        Block::Table(crate::pandoc::Table {
            attr,
            caption,
            colspec,
            head,
            bodies,
            foot,
            ..
        }) => {
            write!(buf, "Table ")?;
            write_native_attr(attr, buf)?;
            write!(buf, " ")?;
            write_caption(caption, buf)?;
            write!(buf, " [")?;
            for (i, spec) in colspec.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write!(buf, "(")?;
                write_native_alignment(&spec.0, buf)?;
                write!(buf, ", ")?;
                write_native_colwidth(&spec.1, buf)?;
                write!(buf, ")")?;
            }
            write!(buf, "] (TableHead ")?;
            write_native_attr(&head.attr, buf)?;
            write!(buf, " ")?;
            write_native_rows(&head.rows, buf)?;
            write!(buf, ") [")?;
            for (i, table_body) in bodies.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_native_table_body(table_body, buf)?;
            }
            write!(buf, "] ")?;
            write_native_table_foot(foot, buf)?;
        }
        Block::DefinitionList(crate::pandoc::DefinitionList { content, .. }) => {
            write!(buf, "DefinitionList [")?;
            for (i, (term, definitions)) in content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write!(buf, "(")?;
                write_inlines(term, buf)?;
                write!(buf, ", [")?;
                for (j, def_blocks) in definitions.iter().enumerate() {
                    if j > 0 {
                        write!(buf, ", ")?;
                    }
                    write!(buf, "[")?;
                    for (k, block) in def_blocks.iter().enumerate() {
                        if k > 0 {
                            write!(buf, ", ")?;
                        }
                        write_block(block, buf)?;
                    }
                    write!(buf, "]")?;
                }
                write!(buf, "])")?;
            }
            write!(buf, "]")?;
        }
        _ => panic!("Unsupported block type in native writer: {:?}", block),
    }
    Ok(())
}

pub fn write<T: std::io::Write>(pandoc: &Pandoc, mut buf: &mut T) -> std::io::Result<()> {
    write!(buf, "[ ")?;
    for (i, block) in pandoc.blocks.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_block(block, &mut buf)?;
    }
    write!(buf, " ]")?;
    Ok(())
}
