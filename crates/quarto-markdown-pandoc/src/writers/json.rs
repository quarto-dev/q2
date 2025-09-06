/*
 * json.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{Attr, Block, Caption, CitationMode, Inline, Inlines, ListAttributes, Pandoc};
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
        "filename": item.filename(),
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
            "c": s.text
        }),
        Inline::Space(_) => json!({
            "t": "Space"
        }),
        Inline::LineBreak(_) => json!({
            "t": "LineBreak"
        }),
        Inline::SoftBreak(_) => json!({
            "t": "SoftBreak"
        }),
        Inline::Emph(e) => json!({
            "t": "Emph",
            "c": write_inlines(&e.content)
        }),
        Inline::Strong(s) => json!({
            "t": "Strong",
            "c": write_inlines(&s.content)
        }),
        Inline::Code(c) => json!({
            "t": "Code",
            "c": [write_attr(&c.attr), c.text]
        }),
        Inline::Math(m) => {
            let math_type = match m.math_type {
                crate::pandoc::MathType::InlineMath => json!({"t": "InlineMath"}),
                crate::pandoc::MathType::DisplayMath => json!({"t": "DisplayMath"}),
            };
            json!({
                "t": "Math",
                "c": [math_type, m.text]
            })
        }
        Inline::Underline(u) => json!({
            "t": "Underline",
            "c": write_inlines(&u.content)
        }),
        Inline::Strikeout(s) => json!({
            "t": "Strikeout",
            "c": write_inlines(&s.content)
        }),
        Inline::Superscript(s) => json!({
            "t": "Superscript",
            "c": write_inlines(&s.content)
        }),
        Inline::Subscript(s) => json!({
            "t": "Subscript",
            "c": write_inlines(&s.content)
        }),
        Inline::SmallCaps(s) => json!({
            "t": "SmallCaps",
            "c": write_inlines(&s.content)
        }),
        Inline::Quoted(q) => {
            let quote_type = match q.quote_type {
                crate::pandoc::QuoteType::SingleQuote => json!({"t": "SingleQuote"}),
                crate::pandoc::QuoteType::DoubleQuote => json!({"t": "DoubleQuote"}),
            };
            json!({
                "t": "Quoted",
                "c": [quote_type, write_inlines(&q.content)]
            })
        }
        Inline::Link(link) => json!({
            "t": "Link",
            "c": [write_attr(&link.attr), write_inlines(&link.content), [link.target.0, link.target.1]]
        }),
        Inline::RawInline(raw) => json!({
            "t": "RawInline",
            "c": [raw.format.clone(), raw.text.clone()]
        }),
        Inline::Image(image) => json!({
            "t": "Image",
            "c": [write_attr(&image.attr), write_inlines(&image.content), [image.target.0, image.target.1]]
        }),
        Inline::Span(span) => json!({
            "t": "Span",
            "c": [write_attr(&span.attr), write_inlines(&span.content)]
        }),
        Inline::Note(note) => json!({
            "t": "Note",
            "c": write_blocks(&note.content)
        }),
        // we can't test this just yet because
        // our citationNoteNum counter doesn't match Pandoc's
        Inline::Cite(cite) => json!({
            "t": "Cite",
            "c": cite.citations.iter().map(|citation| {
                json!({
                    "citationId": citation.id.clone(),
                    "citationPrefix": write_inlines(&citation.prefix),
                    "citationSuffix": write_inlines(&citation.suffix),
                    "citationMode": write_citation_mode(&citation.mode),
                    "citationHash": citation.hash,
                    "citationNoteNum": citation.note_num
                })
            }).collect::<Vec<_>>()
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
        &caption.long.as_ref().map(|l| write_blocks(&l)),
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
        Block::Table(_) => panic!("unimplemented block: Table"),

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

fn write_pandoc(pandoc: &Pandoc) -> Value {
    json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": write_meta(&pandoc.meta),
        "blocks": write_blocks(&pandoc.blocks),
    })
}

pub fn write<W: std::io::Write>(pandoc: &Pandoc, writer: &mut W) -> std::io::Result<()> {
    let json = write_pandoc(pandoc);
    serde_json::to_writer(writer, &json)?;
    Ok(())
}
