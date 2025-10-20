/*
 * json.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{
    ASTContext, Attr, Block, Caption, CitationMode, Inline, Inlines, ListAttributes, Pandoc,
};
use quarto_source_map::{FileId, Range, RangeMapping, SourceInfo, SourceMapping};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

/// Serializable version of SourceInfo that uses ID references instead of Rc pointers.
///
/// This structure is used during JSON serialization to avoid duplicating parent chains.
/// Each unique SourceInfo is assigned an ID and stored in a pool. References to parent
/// SourceInfo objects are replaced with parent_id integers.
///
/// Serializes in compact format: {"r": [6 range values], "t": type_code, "d": type_data}
/// The ID is implicit from the array index in the pool.
struct SerializableSourceInfo {
    id: usize,
    range: Range,
    mapping: SerializableSourceMapping,
}

impl Serialize for SerializableSourceInfo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(3))?;

        // Serialize range as array [start_offset, start_row, start_col, end_offset, end_row, end_col]
        let range_array = [
            self.range.start.offset,
            self.range.start.row,
            self.range.start.column,
            self.range.end.offset,
            self.range.end.row,
            self.range.end.column,
        ];
        map.serialize_entry("r", &range_array)?;

        // Serialize type code and data based on mapping variant
        match &self.mapping {
            SerializableSourceMapping::Original { file_id } => {
                map.serialize_entry("t", &0)?;
                map.serialize_entry("d", &file_id.0)?;
            }
            SerializableSourceMapping::Substring { parent_id, offset } => {
                map.serialize_entry("t", &1)?;
                map.serialize_entry("d", &[parent_id, offset])?;
            }
            SerializableSourceMapping::Concat { pieces } => {
                map.serialize_entry("t", &2)?;
                let piece_arrays: Vec<[usize; 3]> = pieces
                    .iter()
                    .map(|p| [p.source_info_id, p.offset_in_concat, p.length])
                    .collect();
                map.serialize_entry("d", &piece_arrays)?;
            }
            SerializableSourceMapping::Transformed { parent_id, mapping } => {
                map.serialize_entry("t", &3)?;
                let mapping_arrays: Vec<[usize; 4]> = mapping
                    .iter()
                    .map(|m| [m.from_start, m.from_end, m.to_start, m.to_end])
                    .collect();
                map.serialize_entry("d", &[json!(*parent_id), json!(mapping_arrays)])?;
            }
        }

        map.end()
    }
}

/// Serializable version of SourceMapping that uses parent_id instead of Rc<SourceInfo>.
enum SerializableSourceMapping {
    Original {
        file_id: FileId,
    },
    Substring {
        parent_id: usize,
        offset: usize,
    },
    Concat {
        pieces: Vec<SerializableSourcePiece>,
    },
    Transformed {
        parent_id: usize,
        mapping: Vec<RangeMapping>,
    },
}

/// Serializable version of SourcePiece that uses source_info_id instead of SourceInfo.
struct SerializableSourcePiece {
    source_info_id: usize,
    offset_in_concat: usize,
    length: usize,
}

/// Serializer that builds a pool of unique SourceInfo objects and assigns IDs.
///
/// During AST traversal, each SourceInfo is interned into the pool. Rc-shared
/// SourceInfo objects get the same ID (using pointer equality). Parent references
/// are serialized as parent_id integers instead of full nested objects.
///
/// This approach reduces JSON size by ~93% for documents with many nodes sharing
/// the same parent chains (e.g., YAML metadata with siblings).
struct SourceInfoSerializer {
    pool: Vec<SerializableSourceInfo>,
    id_map: HashMap<*const SourceInfo, usize>,
}

impl SourceInfoSerializer {
    fn new() -> Self {
        SourceInfoSerializer {
            pool: Vec::new(),
            id_map: HashMap::new(),
        }
    }

    /// Intern a SourceInfo into the pool, returning its ID.
    ///
    /// If this SourceInfo (or an Rc-equivalent) has already been interned,
    /// returns the existing ID. Otherwise, recursively interns parents and
    /// adds this SourceInfo to the pool with a new ID.
    fn intern(&mut self, source_info: &SourceInfo) -> usize {
        // For Rc-shared SourceInfo objects, we need to detect if they point to the same
        // underlying data. We use the data pointer address for this.
        let ptr = source_info as *const SourceInfo;

        // Check if already interned
        if let Some(&id) = self.id_map.get(&ptr) {
            return id;
        }

        // Recursively intern parents and build the serializable mapping
        let mapping = match &source_info.mapping {
            SourceMapping::Original { file_id } => {
                SerializableSourceMapping::Original { file_id: *file_id }
            }
            SourceMapping::Substring { parent, offset } => {
                let parent_id = self.intern(parent);
                SerializableSourceMapping::Substring {
                    parent_id,
                    offset: *offset,
                }
            }
            SourceMapping::Transformed { parent, mapping } => {
                let parent_id = self.intern(parent);
                SerializableSourceMapping::Transformed {
                    parent_id,
                    mapping: mapping.clone(),
                }
            }
            SourceMapping::Concat { pieces } => {
                let serializable_pieces = pieces
                    .iter()
                    .map(|piece| SerializableSourcePiece {
                        source_info_id: self.intern(&piece.source_info),
                        offset_in_concat: piece.offset_in_concat,
                        length: piece.length,
                    })
                    .collect();
                SerializableSourceMapping::Concat {
                    pieces: serializable_pieces,
                }
            }
        };

        // Calculate ID after recursion completes
        let id = self.pool.len();

        // Add to pool
        self.pool.push(SerializableSourceInfo {
            id,
            range: source_info.range.clone(),
            mapping,
        });

        // Record this pointer's ID for future lookups
        self.id_map.insert(ptr, id);

        id
    }

    /// Serialize a SourceInfo as a JSON reference: just the id number
    fn to_json_ref(&mut self, source_info: &SourceInfo) -> Value {
        let id = self.intern(source_info);
        json!(id)
    }
}

fn write_location(source_info: &quarto_source_map::SourceInfo) -> Value {
    // Extract filename index by walking to the Original mapping
    let filename_index = crate::pandoc::location::extract_filename_index(source_info);

    json!({
        "start": {
            "offset": source_info.range.start.offset,
            "row": source_info.range.start.row,
            "column": source_info.range.start.column,
        },
        "end": {
            "offset": source_info.range.end.offset,
            "row": source_info.range.end.row,
            "column": source_info.range.end.column,
        },
        "filenameIndex": filename_index,
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

fn write_inline(inline: &Inline, serializer: &mut SourceInfoSerializer) -> Value {
    match inline {
        Inline::Str(s) => json!({
            "t": "Str",
            "c": s.text,
            "s": serializer.to_json_ref(&s.source_info)
        }),
        Inline::Space(space) => json!({
            "t": "Space",
            "s": serializer.to_json_ref(&space.source_info)
        }),
        Inline::LineBreak(lb) => json!({
            "t": "LineBreak",
            "s": serializer.to_json_ref(&lb.source_info)
        }),
        Inline::SoftBreak(sb) => json!({
            "t": "SoftBreak",
            "s": serializer.to_json_ref(&sb.source_info)
        }),
        Inline::Emph(e) => json!({
            "t": "Emph",
            "c": write_inlines(&e.content, serializer),
            "s": serializer.to_json_ref(&e.source_info)
        }),
        Inline::Strong(s) => json!({
            "t": "Strong",
            "c": write_inlines(&s.content, serializer),
            "s": serializer.to_json_ref(&s.source_info)
        }),
        Inline::Code(c) => json!({
            "t": "Code",
            "c": [write_attr(&c.attr), c.text],
            "s": serializer.to_json_ref(&c.source_info)
        }),
        Inline::Math(m) => {
            let math_type = match m.math_type {
                crate::pandoc::MathType::InlineMath => json!({"t": "InlineMath"}),
                crate::pandoc::MathType::DisplayMath => json!({"t": "DisplayMath"}),
            };
            json!({
                "t": "Math",
                "c": [math_type, m.text],
                "s": serializer.to_json_ref(&m.source_info)
            })
        }
        Inline::Underline(u) => json!({
            "t": "Underline",
            "c": write_inlines(&u.content, serializer),
            "s": serializer.to_json_ref(&u.source_info)
        }),
        Inline::Strikeout(s) => json!({
            "t": "Strikeout",
            "c": write_inlines(&s.content, serializer),
            "s": serializer.to_json_ref(&s.source_info)
        }),
        Inline::Superscript(s) => json!({
            "t": "Superscript",
            "c": write_inlines(&s.content, serializer),
            "s": serializer.to_json_ref(&s.source_info)
        }),
        Inline::Subscript(s) => json!({
            "t": "Subscript",
            "c": write_inlines(&s.content, serializer),
            "s": serializer.to_json_ref(&s.source_info)
        }),
        Inline::SmallCaps(s) => json!({
            "t": "SmallCaps",
            "c": write_inlines(&s.content, serializer),
            "s": serializer.to_json_ref(&s.source_info)
        }),
        Inline::Quoted(q) => {
            let quote_type = match q.quote_type {
                crate::pandoc::QuoteType::SingleQuote => json!({"t": "SingleQuote"}),
                crate::pandoc::QuoteType::DoubleQuote => json!({"t": "DoubleQuote"}),
            };
            json!({
                "t": "Quoted",
                "c": [quote_type, write_inlines(&q.content, serializer)],
                "s": serializer.to_json_ref(&q.source_info)
            })
        }
        Inline::Link(link) => json!({
            "t": "Link",
            "c": [write_attr(&link.attr), write_inlines(&link.content, serializer), [link.target.0, link.target.1]],
            "s": serializer.to_json_ref(&link.source_info)
        }),
        Inline::RawInline(raw) => json!({
            "t": "RawInline",
            "c": [raw.format.clone(), raw.text.clone()],
            "s": serializer.to_json_ref(&raw.source_info)
        }),
        Inline::Image(image) => json!({
            "t": "Image",
            "c": [write_attr(&image.attr), write_inlines(&image.content, serializer), [image.target.0, image.target.1]],
            "s": serializer.to_json_ref(&image.source_info)
        }),
        Inline::Span(span) => json!({
            "t": "Span",
            "c": [write_attr(&span.attr), write_inlines(&span.content, serializer)],
            "s": serializer.to_json_ref(&span.source_info)
        }),
        Inline::Note(note) => json!({
            "t": "Note",
            "c": write_blocks(&note.content, serializer),
            "s": serializer.to_json_ref(&note.source_info)
        }),
        // we can't test this just yet because
        // our citationNoteNum counter doesn't match Pandoc's
        Inline::Cite(cite) => json!({
            "t": "Cite",
            "c": [
                cite.citations.iter().map(|citation| {
                    json!({
                        "citationId": citation.id.clone(),
                        "citationPrefix": write_inlines(&citation.prefix, serializer),
                        "citationSuffix": write_inlines(&citation.suffix, serializer),
                        "citationMode": write_citation_mode(&citation.mode),
                        "citationHash": citation.hash,
                        "citationNoteNum": citation.note_num
                    })
                }).collect::<Vec<_>>(),
                write_inlines(&cite.content, serializer)
            ],
            "s": serializer.to_json_ref(&cite.source_info)
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

fn write_inlines(inlines: &Inlines, serializer: &mut SourceInfoSerializer) -> Value {
    json!(
        inlines
            .iter()
            .map(|inline| write_inline(inline, serializer))
            .collect::<Vec<_>>()
    )
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

fn write_blockss(blockss: &[Vec<Block>], serializer: &mut SourceInfoSerializer) -> Value {
    json!(
        blockss
            .iter()
            .map(|blocks| blocks
                .iter()
                .map(|block| write_block(block, serializer))
                .collect::<Vec<_>>())
            .collect::<Vec<_>>()
    )
}

fn write_caption(caption: &Caption, serializer: &mut SourceInfoSerializer) -> Value {
    json!([
        &caption
            .short
            .as_ref()
            .map(|s| write_inlines(&s, serializer)),
        &caption
            .long
            .as_ref()
            .map(|l| write_blocks(&l, serializer))
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

fn write_cell(cell: &crate::pandoc::table::Cell, serializer: &mut SourceInfoSerializer) -> Value {
    json!([
        write_attr(&cell.attr),
        write_alignment(&cell.alignment),
        cell.row_span,
        cell.col_span,
        write_blocks(&cell.content, serializer)
    ])
}

fn write_row(row: &crate::pandoc::table::Row, serializer: &mut SourceInfoSerializer) -> Value {
    json!([
        write_attr(&row.attr),
        row.cells
            .iter()
            .map(|cell| write_cell(cell, serializer))
            .collect::<Vec<_>>()
    ])
}

fn write_table_head(
    head: &crate::pandoc::table::TableHead,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    json!([
        write_attr(&head.attr),
        head.rows
            .iter()
            .map(|row| write_row(row, serializer))
            .collect::<Vec<_>>()
    ])
}

fn write_table_body(
    body: &crate::pandoc::table::TableBody,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    json!([
        write_attr(&body.attr),
        body.rowhead_columns,
        body.head
            .iter()
            .map(|row| write_row(row, serializer))
            .collect::<Vec<_>>(),
        body.body
            .iter()
            .map(|row| write_row(row, serializer))
            .collect::<Vec<_>>()
    ])
}

fn write_table_foot(
    foot: &crate::pandoc::table::TableFoot,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    json!([
        write_attr(&foot.attr),
        foot.rows
            .iter()
            .map(|row| write_row(row, serializer))
            .collect::<Vec<_>>()
    ])
}

fn write_block(block: &Block, serializer: &mut SourceInfoSerializer) -> Value {
    match block {
        Block::Figure(figure) => json!({
            "t": "Figure",
            "c": [
                write_attr(&figure.attr),
                write_caption(&figure.caption, serializer),
                write_blocks(&figure.content, serializer)
            ],
            "s": serializer.to_json_ref(&figure.source_info)
        }),
        Block::DefinitionList(deflist) => json!({
            "t": "DefinitionList",
            "c": deflist.content
                .iter()
                .map(|(term, definition)| {
                    json!([
                        write_inlines(term, serializer),
                        write_blockss(&definition, serializer),
                    ])
                })
                .collect::<Vec<_>>(),
            "s": serializer.to_json_ref(&deflist.source_info),
        }),
        Block::OrderedList(orderedlist) => json!({
            "t": "OrderedList",
            "c": [
                write_list_attributes(&orderedlist.attr),
                write_blockss(&orderedlist.content, serializer),
            ],
            "s": serializer.to_json_ref(&orderedlist.source_info),
        }),
        Block::RawBlock(raw) => json!({
            "t": "RawBlock",
            "c": [raw.format.clone(), raw.text.clone()],
            "s": serializer.to_json_ref(&raw.source_info),
        }),
        Block::HorizontalRule(block) => json!({
            "t": "HorizontalRule",
            "s": serializer.to_json_ref(&block.source_info),
        }),
        Block::Table(table) => json!({
            "t": "Table",
            "c": [
                write_attr(&table.attr),
                write_caption(&table.caption, serializer),
                table.colspec.iter().map(write_colspec).collect::<Vec<_>>(),
                write_table_head(&table.head, serializer),
                table.bodies.iter().map(|body| write_table_body(body, serializer)).collect::<Vec<_>>(),
                write_table_foot(&table.foot, serializer)
            ],
            "s": serializer.to_json_ref(&table.source_info),
        }),

        Block::Div(div) => json!({
            "t": "Div",
            "c": [write_attr(&div.attr), write_blocks(&div.content, serializer)],
            "s": serializer.to_json_ref(&div.source_info),
        }),
        Block::BlockQuote(quote) => json!({
            "t": "BlockQuote",
            "c": write_blocks(&quote.content, serializer),
            "s": serializer.to_json_ref(&quote.source_info),
        }),
        Block::LineBlock(lineblock) => json!({
            "t": "LineBlock",
            "c": lineblock.content.iter().map(|inlines| write_inlines(inlines, serializer)).collect::<Vec<_>>(),
            "s": serializer.to_json_ref(&lineblock.source_info),
        }),
        Block::Paragraph(para) => json!({
            "t": "Para",
            "c": write_inlines(&para.content, serializer),
            "s": serializer.to_json_ref(&para.source_info),
        }),
        Block::Header(header) => {
            json!({
                "t": "Header",
                "c": [header.level, write_attr(&header.attr), write_inlines(&header.content, serializer)],
                "s": serializer.to_json_ref(&header.source_info),
            })
        }
        Block::CodeBlock(codeblock) => json!({
            "t": "CodeBlock",
            "c": [write_attr(&codeblock.attr), codeblock.text],
            "s": serializer.to_json_ref(&codeblock.source_info),
        }),
        Block::Plain(plain) => json!({
            "t": "Plain",
            "c": write_inlines(&plain.content, serializer),
            "s": serializer.to_json_ref(&plain.source_info),
        }),
        Block::BulletList(bulletlist) => json!({
            "t": "BulletList",
            "c": bulletlist.content.iter().map(|blocks| blocks.iter().map(|block| write_block(block, serializer)).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "s": serializer.to_json_ref(&bulletlist.source_info),
        }),
        Block::BlockMetadata(meta) => json!({
            "t": "BlockMetadata",
            "c": write_meta_value_with_source_info(&meta.meta, serializer),
            "s": serializer.to_json_ref(&meta.source_info),
        }),
        Block::NoteDefinitionPara(refdef) => json!({
            "t": "NoteDefinitionPara",
            "c": [refdef.id, write_inlines(&refdef.content, serializer)],
            "s": serializer.to_json_ref(&refdef.source_info),
        }),
        Block::NoteDefinitionFencedBlock(refdef) => json!({
            "t": "NoteDefinitionFencedBlock",
            "c": [refdef.id, write_blocks(&refdef.content, serializer)],
            "s": serializer.to_json_ref(&refdef.source_info),
        }),
        Block::CaptionBlock(_) => {
            panic!(
                "CaptionBlock found in JSON writer - should have been processed during postprocessing"
            )
        }
    }
}

fn write_meta_value_with_source_info(
    value: &crate::pandoc::MetaValueWithSourceInfo,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    match value {
        crate::pandoc::MetaValueWithSourceInfo::MetaString { value, source_info } => json!({
            "t": "MetaString",
            "c": value,
            "s": serializer.to_json_ref(source_info)
        }),
        crate::pandoc::MetaValueWithSourceInfo::MetaBool { value, source_info } => json!({
            "t": "MetaBool",
            "c": value,
            "s": serializer.to_json_ref(source_info)
        }),
        crate::pandoc::MetaValueWithSourceInfo::MetaInlines {
            content,
            source_info,
        } => json!({
            "t": "MetaInlines",
            "c": write_inlines(content, serializer),
            "s": serializer.to_json_ref(source_info)
        }),
        crate::pandoc::MetaValueWithSourceInfo::MetaBlocks {
            content,
            source_info,
        } => json!({
            "t": "MetaBlocks",
            "c": write_blocks(content, serializer),
            "s": serializer.to_json_ref(source_info)
        }),
        crate::pandoc::MetaValueWithSourceInfo::MetaList { items, source_info } => json!({
            "t": "MetaList",
            "c": items.iter().map(|item| write_meta_value_with_source_info(item, serializer)).collect::<Vec<_>>(),
            "s": serializer.to_json_ref(source_info)
        }),
        crate::pandoc::MetaValueWithSourceInfo::MetaMap {
            entries,
            source_info,
        } => json!({
            "t": "MetaMap",
            "c": entries.iter().map(|entry| json!({
                "key": entry.key,
                "key_source": serializer.to_json_ref(&entry.key_source),
                "value": write_meta_value_with_source_info(&entry.value, serializer)
            })).collect::<Vec<_>>(),
            "s": serializer.to_json_ref(source_info)
        }),
    }
}

fn write_meta(
    meta: &crate::pandoc::MetaValueWithSourceInfo,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    // meta should be a MetaMap variant
    // Write as Pandoc-compatible object format
    match meta {
        crate::pandoc::MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            let map: serde_json::Map<String, Value> = entries
                .iter()
                .map(|entry| {
                    (
                        entry.key.clone(),
                        write_meta_value_with_source_info(&entry.value, serializer),
                    )
                })
                .collect();
            Value::Object(map)
        }
        _ => panic!("Expected MetaMap for Pandoc.meta"),
    }
}

fn write_blocks(blocks: &[Block], serializer: &mut SourceInfoSerializer) -> Value {
    json!(
        blocks
            .iter()
            .map(|block| write_block(block, serializer))
            .collect::<Vec<_>>()
    )
}

fn write_pandoc(pandoc: &Pandoc, context: &ASTContext) -> Value {
    // Create the SourceInfo serializer
    let mut serializer = SourceInfoSerializer::new();

    // Serialize AST, which will build the pool
    let meta_json = write_meta(&pandoc.meta, &mut serializer);
    let blocks_json = write_blocks(&pandoc.blocks, &mut serializer);

    // Extract top-level key sources from metadata using the serializer
    let meta_top_level_key_sources: serde_json::Map<String, Value> =
        if let crate::pandoc::MetaValueWithSourceInfo::MetaMap { entries, .. } = &pandoc.meta {
            entries
                .iter()
                .map(|entry| (entry.key.clone(), serializer.to_json_ref(&entry.key_source)))
                .collect()
        } else {
            serde_json::Map::new()
        };

    // Build astContext with pool and metaTopLevelKeySources
    let mut ast_context_obj = serde_json::Map::new();
    ast_context_obj.insert("filenames".to_string(), json!(context.filenames));

    // Only include sourceInfoPool if non-empty
    if !serializer.pool.is_empty() {
        ast_context_obj.insert("sourceInfoPool".to_string(), json!(serializer.pool));
    }

    // Only include metaTopLevelKeySources if non-empty
    if !meta_top_level_key_sources.is_empty() {
        ast_context_obj.insert(
            "metaTopLevelKeySources".to_string(),
            Value::Object(meta_top_level_key_sources),
        );
    }

    json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": meta_json,
        "blocks": blocks_json,
        "astContext": ast_context_obj,
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
