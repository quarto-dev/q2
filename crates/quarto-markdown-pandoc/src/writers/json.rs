/*
 * json.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{
    ASTContext, Attr, Block, Caption, CitationMode, Inline, Inlines, ListAttributes, Pandoc,
};
use serde_json::{Value, json};

fn write_location<T: crate::pandoc::location::SourceLocation>(item: &T) -> Value {
    let range = item.range();
    json!({
        "start": {
            "offset": range.start.offset,
            "row": range.start.row,
            "column": range.start.column,
        },
        "end": {
            "offset": range.end.offset,
            "row": range.end.row,
            "column": range.end.column,
        },
        "filenameIndex": item.filename_index(),
    })
}

fn write_attr(attr: &Attr) -> Value {
    json!([
        attr.0, // id
        attr.1, // classes
        attr.2
            .iter()
            .map(|(k, v)| json!([k, v]))
            .collect::<Vec<_>>()  // key-value pairs
    ])
}

fn write_citation_mode(mode: &CitationMode) -> Value {
    match mode {
        CitationMode::NormalCitation => json!({"t": "NormalCitation"}),
        CitationMode::AuthorInText => json!({"t": "AuthorInText"}),
        CitationMode::SuppressAuthor => json!({"t": "SuppressAuthor"}),
    }
}

fn write_inline(inline: &Inline) -> Value {
    match inline {
        Inline::Str(s) => json!({
            "t": "Str",
            "c": s.text,
            "l": write_location(s)
        }),
        Inline::Space(space) => json!({
            "t": "Space",
            "l": write_location(space)
        }),
        Inline::LineBreak(lb) => json!({
            "t": "LineBreak",
            "l": write_location(lb)
        }),
        Inline::SoftBreak(sb) => json!({
            "t": "SoftBreak",
            "l": write_location(sb)
        }),
        Inline::Emph(e) => json!({
            "t": "Emph",
            "c": write_inlines(&e.content),
            "l": write_location(e)
        }),
        Inline::Strong(s) => json!({
            "t": "Strong",
            "c": write_inlines(&s.content),
            "l": write_location(s)
        }),
        Inline::Code(c) => json!({
            "t": "Code",
            "c": [write_attr(&c.attr), c.text],
            "l": write_location(c)
        }),
        Inline::Math(m) => {
            let math_type = match m.math_type {
                crate::pandoc::MathType::InlineMath => json!({"t": "InlineMath"}),
                crate::pandoc::MathType::DisplayMath => json!({"t": "DisplayMath"}),
            };
            json!({
                "t": "Math",
                "c": [math_type, m.text],
                "l": write_location(m)
            })
        }
        Inline::Underline(u) => json!({
            "t": "Underline",
            "c": write_inlines(&u.content),
            "l": write_location(u)
        }),
        Inline::Strikeout(s) => json!({
            "t": "Strikeout",
            "c": write_inlines(&s.content),
            "l": write_location(s)
        }),
        Inline::Superscript(s) => json!({
            "t": "Superscript",
            "c": write_inlines(&s.content),
            "l": write_location(s)
        }),
        Inline::Subscript(s) => json!({
            "t": "Subscript",
            "c": write_inlines(&s.content),
            "l": write_location(s)
        }),
        Inline::SmallCaps(s) => json!({
            "t": "SmallCaps",
            "c": write_inlines(&s.content),
            "l": write_location(s)
        }),
        Inline::Quoted(q) => {
            let quote_type = match q.quote_type {
                crate::pandoc::QuoteType::SingleQuote => json!({"t": "SingleQuote"}),
                crate::pandoc::QuoteType::DoubleQuote => json!({"t": "DoubleQuote"}),
            };
            json!({
                "t": "Quoted",
                "c": [quote_type, write_inlines(&q.content)],
                "l": write_location(q)
            })
        }
        Inline::Link(link) => json!({
            "t": "Link",
            "c": [write_attr(&link.attr), write_inlines(&link.content), [link.target.0, link.target.1]],
            "l": write_location(link)
        }),
        Inline::RawInline(raw) => json!({
            "t": "RawInline",
            "c": [raw.format.clone(), raw.text.clone()],
            "l": write_location(raw)
        }),
        Inline::Image(image) => json!({
            "t": "Image",
            "c": [write_attr(&image.attr), write_inlines(&image.content), [image.target.0, image.target.1]],
            "l": write_location(image)
        }),
        Inline::Span(span) => json!({
            "t": "Span",
            "c": [write_attr(&span.attr), write_inlines(&span.content)],
            "l": write_location(span)
        }),
        Inline::Note(note) => json!({
            "t": "Note",
            "c": write_blocks(&note.content),
            "l": write_location(note)
        }),
        // we can't test this just yet because
        // our citationNoteNum counter doesn't match Pandoc's
        Inline::Cite(cite) => json!({
            "t": "Cite",
            "c": [
                cite.citations.iter().map(|citation| {
                    json!({
                        "citationId": citation.id.clone(),
                        "citationPrefix": write_inlines(&citation.prefix),
                        "citationSuffix": write_inlines(&citation.suffix),
                        "citationMode": write_citation_mode(&citation.mode),
                        "citationHash": citation.hash,
                        "citationNoteNum": citation.note_num
                    })
                }).collect::<Vec<_>>(),
                write_inlines(&cite.content)
            ],
            "l": write_location(cite)
        }),
        Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_)
        | Inline::Insert(_)
        | Inline::Delete(_)
        | Inline::Highlight(_)
        | Inline::EditComment(_) => {
            panic!("Unsupported inline type: {:?}", inline)
        }
    }
}

fn write_inlines(inlines: &Inlines) -> Value {
    json!(inlines.iter().map(write_inline).collect::<Vec<_>>())
}

fn write_list_attributes(attr: &ListAttributes) -> Value {
    let number_style = match attr.1 {
        crate::pandoc::ListNumberStyle::Decimal => json!({"t": "Decimal"}),
        crate::pandoc::ListNumberStyle::LowerAlpha => json!({"t": "LowerAlpha"}),
        crate::pandoc::ListNumberStyle::UpperAlpha => json!({"t": "UpperAlpha"}),
        crate::pandoc::ListNumberStyle::LowerRoman => json!({"t": "LowerRoman"}),
        crate::pandoc::ListNumberStyle::UpperRoman => json!({"t": "UpperRoman"}),
        crate::pandoc::ListNumberStyle::Example => json!({"t": "Example"}),
        crate::pandoc::ListNumberStyle::Default => json!({"t": "Default"}),
    };
    let number_delimiter = match attr.2 {
        crate::pandoc::ListNumberDelim::Period => json!({"t": "Period"}),
        crate::pandoc::ListNumberDelim::OneParen => json!({"t": "OneParen"}),
        crate::pandoc::ListNumberDelim::TwoParens => json!({"t": "TwoParens"}),
        crate::pandoc::ListNumberDelim::Default => json!({"t": "Default"}),
    };
    json!([attr.0, number_style, number_delimiter])
}

fn write_blockss(blockss: &[Vec<Block>]) -> Value {
    json!(
        blockss
            .iter()
            .map(|blocks| blocks.iter().map(write_block).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    )
}

fn write_caption(caption: &Caption) -> Value {
    json!([
        &caption.short.as_ref().map(|s| write_inlines(&s)),
        &caption
            .long
            .as_ref()
            .map(|l| write_blocks(&l))
            .unwrap_or_else(|| json!([])),
    ])
}

fn write_alignment(alignment: &crate::pandoc::table::Alignment) -> Value {
    match alignment {
        crate::pandoc::table::Alignment::Left => json!({"t": "AlignLeft"}),
        crate::pandoc::table::Alignment::Center => json!({"t": "AlignCenter"}),
        crate::pandoc::table::Alignment::Right => json!({"t": "AlignRight"}),
        crate::pandoc::table::Alignment::Default => json!({"t": "AlignDefault"}),
    }
}

fn write_colwidth(colwidth: &crate::pandoc::table::ColWidth) -> Value {
    match colwidth {
        crate::pandoc::table::ColWidth::Default => json!({"t": "ColWidthDefault"}),
        crate::pandoc::table::ColWidth::Percentage(p) => json!({"t": "ColWidth", "c": p}),
    }
}

fn write_colspec(colspec: &crate::pandoc::table::ColSpec) -> Value {
    json!([write_alignment(&colspec.0), write_colwidth(&colspec.1)])
}

fn write_cell(cell: &crate::pandoc::table::Cell) -> Value {
    json!([
        write_attr(&cell.attr),
        write_alignment(&cell.alignment),
        cell.row_span,
        cell.col_span,
        write_blocks(&cell.content)
    ])
}

fn write_row(row: &crate::pandoc::table::Row) -> Value {
    json!([
        write_attr(&row.attr),
        row.cells.iter().map(write_cell).collect::<Vec<_>>()
    ])
}

fn write_table_head(head: &crate::pandoc::table::TableHead) -> Value {
    json!([
        write_attr(&head.attr),
        head.rows.iter().map(write_row).collect::<Vec<_>>()
    ])
}

fn write_table_body(body: &crate::pandoc::table::TableBody) -> Value {
    json!([
        write_attr(&body.attr),
        body.rowhead_columns,
        body.head.iter().map(write_row).collect::<Vec<_>>(),
        body.body.iter().map(write_row).collect::<Vec<_>>()
    ])
}

fn write_table_foot(foot: &crate::pandoc::table::TableFoot) -> Value {
    json!([
        write_attr(&foot.attr),
        foot.rows.iter().map(write_row).collect::<Vec<_>>()
    ])
}

fn write_block(block: &Block) -> Value {
    match block {
        Block::Figure(figure) => json!({
            "t": "Figure",
            "c": [
                write_attr(&figure.attr),
                write_caption(&figure.caption),
                write_blocks(&figure.content)
            ],
            "l": write_location(figure)
        }),
        Block::DefinitionList(deflist) => json!({
            "t": "DefinitionList",
            "c": deflist.content
                .iter()
                .map(|(term, definition)| {
                    json!([
                        write_inlines(term),
                        write_blockss(&definition),
                    ])
                })
                .collect::<Vec<_>>(),
            "l": write_location(deflist),
        }),
        Block::OrderedList(orderedlist) => json!({
            "t": "OrderedList",
            "c": [
                write_list_attributes(&orderedlist.attr),
                write_blockss(&orderedlist.content),
            ],
            "l": write_location(orderedlist),
        }),
        Block::RawBlock(raw) => json!({
            "t": "RawBlock",
            "c": [raw.format.clone(), raw.text.clone()],
            "l": write_location(raw),
        }),
        Block::HorizontalRule(block) => json!({
            "t": "HorizontalRule",
            "l": write_location(block),
        }),
        Block::Table(table) => json!({
            "t": "Table",
            "c": [
                write_attr(&table.attr),
                write_caption(&table.caption),
                table.colspec.iter().map(write_colspec).collect::<Vec<_>>(),
                write_table_head(&table.head),
                table.bodies.iter().map(write_table_body).collect::<Vec<_>>(),
                write_table_foot(&table.foot)
            ],
            "l": write_location(table),
        }),

        Block::Div(div) => json!({
            "t": "Div",
            "c": [write_attr(&div.attr), write_blocks(&div.content)],
            "l": write_location(div),
        }),
        Block::BlockQuote(quote) => json!({
            "t": "BlockQuote",
            "c": write_blocks(&quote.content),
            "l": write_location(quote),
        }),
        Block::LineBlock(lineblock) => json!({
            "t": "LineBlock",
            "c": lineblock.content.iter().map(write_inlines).collect::<Vec<_>>(),
            "l": write_location(lineblock),
        }),
        Block::Paragraph(para) => json!({
            "t": "Para",
            "c": write_inlines(&para.content),
            "l": write_location(para),
        }),
        Block::Header(header) => {
            json!({
                "t": "Header",
                "c": [header.level, write_attr(&header.attr), write_inlines(&header.content)],
                "l": write_location(header),
            })
        }
        Block::CodeBlock(codeblock) => json!({
            "t": "CodeBlock",
            "c": [write_attr(&codeblock.attr), codeblock.text],
            "l": write_location(codeblock),
        }),
        Block::Plain(plain) => json!({
            "t": "Plain",
            "c": write_inlines(&plain.content),
            "l": write_location(plain),
        }),
        Block::BulletList(bulletlist) => json!({
            "t": "BulletList",
            "c": bulletlist.content.iter().map(|blocks| blocks.iter().map(write_block).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "l": write_location(bulletlist),
        }),
        Block::BlockMetadata(meta) => json!({
            "t": "BlockMetadata",
            "c": write_meta(&meta.meta),
            "l": write_location(meta),
        }),
        Block::NoteDefinitionPara(refdef) => json!({
            "t": "NoteDefinitionPara",
            "c": [refdef.id, write_inlines(&refdef.content)],
            "l": write_location(refdef),
        }),
        Block::NoteDefinitionFencedBlock(refdef) => json!({
            "t": "NoteDefinitionFencedBlock",
            "c": [refdef.id, write_blocks(&refdef.content)],
            "l": write_location(refdef),
        }),
        Block::CaptionBlock(_) => {
            panic!(
                "CaptionBlock found in JSON writer - should have been processed during postprocessing"
            )
        }
    }
}

fn write_meta_value(value: &crate::pandoc::MetaValue) -> Value {
    match value {
        crate::pandoc::MetaValue::MetaString(s) => json!({
            "t": "MetaString",
            "c": s
        }),
        crate::pandoc::MetaValue::MetaInlines(inlines) => json!({
            "t": "MetaInlines",
            "c": write_inlines(inlines)
        }),
        crate::pandoc::MetaValue::MetaBlocks(blocks) => json!({
            "t": "MetaBlocks",
            "c": write_blocks(blocks)
        }),
        crate::pandoc::MetaValue::MetaList(list) => json!({
            "t": "MetaList",
            "c": list.iter().map(write_meta_value).collect::<Vec<_>>()
        }),
        crate::pandoc::MetaValue::MetaMap(map) => json!({
            "t": "MetaMap",
            "c": map.iter().map(|(k, v)| json!([k, write_meta_value(v)])).collect::<Vec<_>>()
        }),
        crate::pandoc::MetaValue::MetaBool(b) => json!({
            "t": "MetaBool",
            "c": b
        }),
    }
}

fn write_meta(meta: &crate::pandoc::Meta) -> Value {
    let map: serde_json::Map<String, Value> = meta
        .iter()
        .map(|(k, v)| (k.clone(), write_meta_value(v)))
        .collect();
    Value::Object(map)
}

fn write_blocks(blocks: &[Block]) -> Value {
    json!(blocks.iter().map(write_block).collect::<Vec<_>>())
}

fn write_pandoc(pandoc: &Pandoc, context: &ASTContext) -> Value {
    json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": write_meta(&pandoc.meta),
        "blocks": write_blocks(&pandoc.blocks),
        "astContext": {
            "filenames": context.filenames,
        },
    })
}

pub fn write<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
) -> std::io::Result<()> {
    let json = write_pandoc(pandoc, context);
    serde_json::to_writer(writer, &json)?;
    Ok(())
}
