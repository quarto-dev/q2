/*
 * native.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::shortcode::shortcode_to_span;
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
    write_safe_string(id, buf)?;
    write!(buf, " , [")?;

    for (i, class) in classes.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_safe_string(class, buf)?;
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
            write!(buf, "(ColWidth {})", percentage)
        }
    }
}

fn write_native_table_body<T: std::io::Write>(
    table_body: &crate::pandoc::TableBody,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    write!(buf, "TableBody ")?;
    write_native_attr(&table_body.attr, buf)?;
    write!(buf, " (RowHeadColumns {}) ", table_body.rowhead_columns)?;
    write_native_rows(&table_body.head, context, buf, errors)?;
    write!(buf, " ")?;
    write_native_rows(&table_body.body, context, buf, errors)?;
    Ok(())
}

fn write_inlines<T: std::io::Write>(
    inlines: &[Inline],
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    write!(buf, "[")?;
    for (i, inline) in inlines.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_inline(inline, context, buf, errors)?;
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
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
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
        write_block(block, context, buf, errors)?;
    }
    write!(buf, "] ")?;
    Ok(())
}

fn write_native_row<T: std::io::Write>(
    row: &crate::pandoc::Row,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    write!(buf, "Row ")?;
    write_native_attr(&row.attr, buf)?;
    write!(buf, " [")?;
    for (i, cell) in row.cells.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_native_cell(cell, context, buf, errors)?;
    }
    write!(buf, "] ")?;
    Ok(())
}

fn write_native_rows<T: std::io::Write>(
    rows: &Vec<crate::pandoc::Row>,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    write!(buf, "[")?;
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_native_row(row, context, buf, errors)?;
    }
    write!(buf, "]")?;
    Ok(())
}

fn write_native_table_foot<T: std::io::Write>(
    foot: &crate::pandoc::TableFoot,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    write!(buf, "(TableFoot ")?;
    write_native_attr(&foot.attr, buf)?;
    write!(buf, " ")?;
    write_native_rows(&foot.rows, context, buf, errors)?;
    write!(buf, " )")?;
    Ok(())
}

fn write_inline<T: std::io::Write>(
    text: &Inline,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
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
            write_inlines(&emph_struct.content, context, buf, errors)?;
        }
        Inline::Underline(underline_struct) => {
            write!(buf, "Underline ")?;
            write_inlines(&underline_struct.content, context, buf, errors)?;
        }
        Inline::SmallCaps(smallcaps_struct) => {
            write!(buf, "SmallCaps ")?;
            write_inlines(&smallcaps_struct.content, context, buf, errors)?;
        }
        Inline::Superscript(superscript_struct) => {
            write!(buf, "Superscript ")?;
            write_inlines(&superscript_struct.content, context, buf, errors)?;
        }
        Inline::Strong(strong_struct) => {
            write!(buf, "Strong ")?;
            write_inlines(&strong_struct.content, context, buf, errors)?;
        }
        Inline::Span(span_struct) => {
            write!(buf, "Span ")?;
            write_native_attr(&span_struct.attr, buf)?;
            write!(buf, " ")?;
            write_inlines(&span_struct.content, context, buf, errors)?;
        }
        Inline::Link(link_struct) => {
            let (url, title) = &link_struct.target;
            write!(buf, "Link ")?;
            write_native_attr(&link_struct.attr, buf)?;
            write!(buf, " ")?;
            write_inlines(&link_struct.content, context, buf, errors)?;
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
            write_inlines(&quoted_struct.content, context, buf, errors)?;
        }
        Inline::Note(note_struct) => {
            write!(buf, "Note [")?;
            for (i, block) in note_struct.content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_block(block, context, buf, errors)?;
            }
            write!(buf, "]")?;
        }
        Inline::Image(image_struct) => {
            let (url, title) = &image_struct.target;
            write!(buf, "Image ")?;
            write_native_attr(&image_struct.attr, buf)?;
            write!(buf, " ")?;
            write_inlines(&image_struct.content, context, buf, errors)?;
            write!(buf, " (")?;
            write_safe_string(url, buf)?;
            write!(buf, " , ")?;
            write_safe_string(title, buf)?;
            write!(buf, ")")?;
        }
        Inline::Subscript(subscript_struct) => {
            write!(buf, "Subscript ")?;
            write_inlines(&subscript_struct.content, context, buf, errors)?;
        }
        Inline::Strikeout(strikeout_struct) => {
            write!(buf, "Strikeout ")?;
            write_inlines(&strikeout_struct.content, context, buf, errors)?;
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
                    id_source: _,
                },
            ) in cite_struct.citations.iter().enumerate()
            {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write!(buf, "Citation {{ citationId = ")?;
                write_safe_string(id, buf)?;
                write!(buf, ", citationPrefix = ")?;
                write_inlines(prefix, context, buf, errors)?;
                write!(buf, ", citationSuffix = ")?;
                write_inlines(suffix, context, buf, errors)?;
                write!(buf, ", citationMode = ")?;
                write_citation_mode(mode, buf)?;
                write!(
                    buf,
                    ", citationNoteNum = {}, citationHash = {} }}",
                    note_num, hash
                )?;
            }
            write!(buf, "] ")?;
            write_inlines(&cite_struct.content, context, buf, errors)?;
        }
        Inline::Shortcode(shortcode) => {
            // Convert shortcode to span representation for native format output
            let span = shortcode_to_span(shortcode.clone());
            write!(buf, "Span ")?;
            write_native_attr(&span.attr, buf)?;
            write!(buf, " ")?;
            write_inlines(&span.content, context, buf, errors)?;
        }
        Inline::NoteReference(note_ref) => {
            // Defensive error - should be converted to Span in postprocess
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Unprocessed note reference",
                )
                .with_code("Q-3-31")
                .problem(format!(
                    "Note reference `[^{}]` was not converted during postprocessing",
                    note_ref.id
                ))
                .with_location(note_ref.source_info.clone())
                .add_detail(
                    "Note references should be converted to Span nodes during postprocessing. \
                     This may indicate a bug in the postprocessor or a filter that bypassed it.",
                )
                .add_hint("Please report this as a bug with a minimal reproducible example")
                .build(),
            );
            // Skip this inline
        }
        Inline::Attr(_attr, attr_source) => {
            // Extension error - standalone attributes not supported in native format
            let mut builder = quarto_error_reporting::DiagnosticMessageBuilder::error(
                "Standalone attributes not supported in native format",
            )
            .with_code("Q-3-32")
            .problem("Cannot render standalone attribute in native format");

            // Add location if available from attr id
            if let Some(ref source_info) = attr_source.id {
                builder = builder.with_location(source_info.clone());
            }

            errors.push(
                builder
                    .add_detail(
                        "Standalone attributes (e.g., in table cells or headings) are not \
                         representable in Pandoc's native format",
                    )
                    .add_hint("Use JSON output format to see attribute details")
                    .build(),
            );
            // Skip this inline
        }
        Inline::Insert(ins) => {
            // Defensive error - editorial marks should be desugared to Span
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Unprocessed Insert markup",
                )
                .with_code("Q-3-33")
                .problem("Insert markup `{++...++}` was not desugared during postprocessing")
                .with_location(ins.source_info.clone())
                .add_detail(
                    "Editorial marks should be converted to Span nodes during postprocessing. \
                     This may indicate a bug or a filter that bypassed postprocessing.",
                )
                .add_hint("Ensure postprocessing is enabled or use a Lua filter to handle editorial marks")
                .build(),
            );
            // Skip this inline
        }
        Inline::Delete(del) => {
            // Defensive error - editorial marks should be desugared to Span
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Unprocessed Delete markup",
                )
                .with_code("Q-3-34")
                .problem("Delete markup `{--...--}` was not desugared during postprocessing")
                .with_location(del.source_info.clone())
                .add_detail(
                    "Editorial marks should be converted to Span nodes during postprocessing. \
                     This may indicate a bug or a filter that bypassed postprocessing.",
                )
                .add_hint("Ensure postprocessing is enabled or use a Lua filter to handle editorial marks")
                .build(),
            );
            // Skip this inline
        }
        Inline::Highlight(hl) => {
            // Defensive error - editorial marks should be desugared to Span
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Unprocessed Highlight markup",
                )
                .with_code("Q-3-35")
                .problem("Highlight markup `{==...==}` was not desugared during postprocessing")
                .with_location(hl.source_info.clone())
                .add_detail(
                    "Editorial marks should be converted to Span nodes during postprocessing. \
                     This may indicate a bug or a filter that bypassed postprocessing.",
                )
                .add_hint("Ensure postprocessing is enabled or use a Lua filter to handle editorial marks")
                .build(),
            );
            // Skip this inline
        }
        Inline::EditComment(ec) => {
            // Defensive error - editorial marks should be desugared to Span
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Unprocessed EditComment markup",
                )
                .with_code("Q-3-36")
                .problem("EditComment markup `{>>...<<}` was not desugared during postprocessing")
                .with_location(ec.source_info.clone())
                .add_detail(
                    "Editorial marks should be converted to Span nodes during postprocessing. \
                     This may indicate a bug or a filter that bypassed postprocessing.",
                )
                .add_hint("Ensure postprocessing is enabled or use a Lua filter to handle editorial marks")
                .build(),
            );
            // Skip this inline
        }
        Inline::Custom(custom) => {
            // Custom nodes are not supported in native format
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Custom inline node in native writer",
                )
                .with_code("Q-3-37")
                .problem(format!(
                    "Custom node type `{}` not supported in native format",
                    custom.type_name
                ))
                .with_location(custom.source_info.clone())
                .add_detail("Custom nodes are internal Quarto extensions")
                .add_hint("Use JSON output format to see custom node details")
                .build(),
            );
            // Skip this inline
        }
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
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    match caption {
        Some(text) => write_inlines(text, context, buf, errors),
        None => write!(buf, "Nothing"),
    }
}

fn write_long_caption<T: std::io::Write>(
    caption: &Option<Vec<Block>>,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    match caption {
        Some(blocks) => {
            write!(buf, "[ ")?;
            for (i, block) in blocks.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_block(block, context, buf, errors)?;
            }
            write!(buf, " ]")?;
        }
        None => write!(buf, "[]")?,
    }
    Ok(())
}

fn write_caption<T: std::io::Write>(
    caption: &crate::pandoc::Caption,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    write!(buf, "(Caption ")?;
    write_short_caption(&caption.short, context, buf, errors)?;
    write!(buf, " ")?;
    write_long_caption(&caption.long, context, buf, errors)?;
    write!(buf, ")")?;
    Ok(())
}

fn write_block<T: std::io::Write>(
    block: &Block,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    match block {
        Block::Plain(crate::pandoc::Plain { content, .. }) => {
            write!(buf, "Plain ")?;
            write_inlines(content, context, buf, errors)?;
        }
        Block::Paragraph(crate::pandoc::Paragraph { content, .. }) => {
            write!(buf, "Para ")?;
            write_inlines(content, context, buf, errors)?;
        }
        Block::CodeBlock(crate::pandoc::CodeBlock {
            attr,
            text,
            source_info: _,
            attr_source: _,
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
                    write_block(block, context, buf, errors)?;
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
                    write_block(block, context, buf, errors)?;
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
                write_block(block, context, buf, errors)?;
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
                write_block(block, context, buf, errors)?;
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
            write_caption(caption, context, buf, errors)?;
            write!(buf, " [")?;
            for (i, block) in content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_block(block, context, buf, errors)?;
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
            write_inlines(content, context, buf, errors)?;
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
            write_caption(caption, context, buf, errors)?;
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
            write_native_rows(&head.rows, context, buf, errors)?;
            write!(buf, ") [")?;
            for (i, table_body) in bodies.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_native_table_body(table_body, context, buf, errors)?;
            }
            write!(buf, "] ")?;
            write_native_table_foot(foot, context, buf, errors)?;
        }
        Block::DefinitionList(crate::pandoc::DefinitionList { content, .. }) => {
            write!(buf, "DefinitionList [")?;
            for (i, (term, definitions)) in content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write!(buf, "(")?;
                write_inlines(term, context, buf, errors)?;
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
                        write_block(block, context, buf, errors)?;
                    }
                    write!(buf, "]")?;
                }
                write!(buf, "])")?;
            }
            write!(buf, "]")?;
        }
        Block::LineBlock(crate::pandoc::LineBlock { content, .. }) => {
            write!(buf, "LineBlock [")?;
            for (i, line) in content.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ")?;
                }
                write_inlines(line, context, buf, errors)?;
            }
            write!(buf, "]")?;
        }
        Block::NoteDefinitionPara(note_def) => {
            // Feature error - accumulate and continue
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Inline note definitions not supported",
                )
                .with_code("Q-3-10")
                .problem(format!(
                    "Cannot render inline note definition `[^{}]` in native format",
                    note_def.id
                ))
                .with_location(note_def.source_info.clone())
                .add_detail(
                    "Inline note definitions require the note content to be coalesced \
                         into the reference location, which is not yet implemented",
                )
                .add_hint("Use inline footnote syntax instead: `^[your note content here]`")
                .build(),
            );
            // Skip this block - don't write anything
        }
        Block::NoteDefinitionFencedBlock(note_def) => {
            // Feature error - accumulate and continue
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Fenced note definitions not supported",
                )
                .with_code("Q-3-11")
                .problem(format!(
                    "Cannot render fenced note definition `[^{}]` in native format",
                    note_def.id
                ))
                .with_location(note_def.source_info.clone())
                .add_detail(
                    "Fenced note definitions require the note content to be coalesced \
                         into the reference location, which is not yet implemented",
                )
                .add_hint("Use inline footnote syntax instead: `^[your note content here]`")
                .build(),
            );
            // Skip this block
        }
        Block::BlockMetadata(meta) => {
            // Defensive error - should not reach writer but might via filters/library usage
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Block metadata not supported in native format",
                )
                .with_code("Q-3-20")
                .problem("Cannot render YAML metadata block in native format")
                .with_location(meta.source_info.clone())
                .add_detail(
                    "Metadata blocks are internal AST nodes that should be processed \
                     before reaching the writer",
                )
                .add_hint("Use JSON output format to see full AST including metadata")
                .build(),
            );
            // Skip this block
        }
        Block::CaptionBlock(caption) => {
            // Defensive error - should be processed in postprocess but might reach via filters
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Caption block not supported in native format",
                )
                .with_code("Q-3-21")
                .problem("Cannot render standalone caption block in native format")
                .with_location(caption.source_info.clone())
                .add_detail(
                    "Caption blocks should be attached to figures or tables during \
                     postprocessing",
                )
                .add_hint(
                    "This may indicate a bug in postprocessing or a filter that \
                     produces orphaned captions",
                )
                .build(),
            );
            // Skip this block
        }
        Block::Custom(custom) => {
            // Custom nodes are not supported in native format
            errors.push(
                quarto_error_reporting::DiagnosticMessageBuilder::error(
                    "Custom block node in native writer",
                )
                .with_code("Q-3-38")
                .problem(format!(
                    "Custom node type `{}` not supported in native format",
                    custom.type_name
                ))
                .with_location(custom.source_info.clone())
                .add_detail("Custom nodes are internal Quarto extensions")
                .add_hint("Use JSON output format to see custom node details")
                .build(),
            );
            // Skip this block
        }
    }
    Ok(())
}

pub fn write<T: std::io::Write>(
    pandoc: &Pandoc,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {
    let mut errors = Vec::new();

    // Try to write - IO errors are fatal
    if let Err(e) = write_impl(pandoc, context, buf, &mut errors) {
        // IO error - wrap and return
        return Err(vec![
            quarto_error_reporting::DiagnosticMessageBuilder::error("IO error during write")
                .with_code("Q-3-1")
                .problem(format!("Failed to write output: {}", e))
                .build(),
        ]);
    }

    // Check for accumulated feature errors
    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(())
}

fn write_impl<T: std::io::Write>(
    pandoc: &Pandoc,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    write!(buf, "[ ")?;
    for (i, block) in pandoc.blocks.iter().enumerate() {
        if i > 0 {
            write!(buf, ", ")?;
        }
        write_block(block, context, buf, errors)?;
    }
    write!(buf, " ]")?;
    Ok(())
}
