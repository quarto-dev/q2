/*
 * json.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::attr::{AttrSourceInfo, TargetSourceInfo};
use crate::pandoc::{
    ASTContext, Attr, Block, Caption, CitationMode, Inline, Inlines, ListAttributes, Pandoc,
};
use quarto_source_map::{FileId, SourceInfo};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

/// Configuration for JSON output format.
#[derive(Debug, Clone)]
pub struct JsonConfig {
    /// If true, include resolved source locations ('l' field) in each node.
    /// The 'l' field contains an object with:
    /// - 'f': file_id (usize)
    /// - 'b': begin position {o: offset, l: line (1-based), c: column (1-based)}
    /// - 'e': end position {o: offset, l: line (1-based), c: column (1-based)}
    pub include_inline_locations: bool,
}

impl Default for JsonConfig {
    fn default() -> Self {
        Self {
            include_inline_locations: false,
        }
    }
}

/// Serializable version of SourceInfo that uses ID references instead of Rc pointers.
///
/// This structure is used during JSON serialization to avoid duplicating parent chains.
/// Each unique SourceInfo is assigned an ID and stored in a pool. References to parent
/// SourceInfo objects are replaced with parent_id integers.
///
/// Serializes in compact format: {"r": [2 offset values], "t": type_code, "d": type_data}
/// The ID is implicit from the array index in the pool.
///
/// Note: Row/column information is not stored in the serialized format.
/// To get row/column, the reader must map offsets through the SourceContext.
struct SerializableSourceInfo {
    id: usize,
    start_offset: usize,
    end_offset: usize,
    mapping: SerializableSourceMapping,
}

impl Serialize for SerializableSourceInfo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(3))?;

        // Serialize offsets as array [start_offset, end_offset]
        let offset_array = [self.start_offset, self.end_offset];
        map.serialize_entry("r", &offset_array)?;

        // Serialize type code and data based on mapping variant
        match &self.mapping {
            SerializableSourceMapping::Original { file_id } => {
                map.serialize_entry("t", &0)?;
                map.serialize_entry("d", &file_id.0)?;
            }
            SerializableSourceMapping::Substring { parent_id } => {
                map.serialize_entry("t", &1)?;
                map.serialize_entry("d", parent_id)?;
            }
            SerializableSourceMapping::Concat { pieces } => {
                map.serialize_entry("t", &2)?;
                let piece_arrays: Vec<[usize; 3]> = pieces
                    .iter()
                    .map(|p| [p.source_info_id, p.offset_in_concat, p.length])
                    .collect();
                map.serialize_entry("d", &piece_arrays)?;
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
    },
    Concat {
        pieces: Vec<SerializableSourcePiece>,
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
struct SourceInfoSerializer<'a> {
    pool: Vec<SerializableSourceInfo>,
    id_map: HashMap<*const SourceInfo, usize>,
    context: &'a ASTContext,
    config: &'a JsonConfig,
}

impl<'a> SourceInfoSerializer<'a> {
    fn new(context: &'a ASTContext, config: &'a JsonConfig) -> Self {
        SourceInfoSerializer {
            pool: Vec::new(),
            id_map: HashMap::new(),
            context,
            config,
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

        // Extract offsets and recursively intern parents to build the serializable mapping
        let (start_offset, end_offset, mapping) = match source_info {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => (
                *start_offset,
                *end_offset,
                SerializableSourceMapping::Original { file_id: *file_id },
            ),
            SourceInfo::Substring {
                parent,
                start_offset,
                end_offset,
            } => {
                let parent_id = self.intern(parent);
                (
                    *start_offset,
                    *end_offset,
                    SerializableSourceMapping::Substring { parent_id },
                )
            }
            SourceInfo::Concat { pieces } => {
                let serializable_pieces = pieces
                    .iter()
                    .map(|piece| SerializableSourcePiece {
                        source_info_id: self.intern(&piece.source_info),
                        offset_in_concat: piece.offset_in_concat,
                        length: piece.length,
                    })
                    .collect();
                (
                    0,
                    pieces.iter().map(|p| p.length).sum(),
                    SerializableSourceMapping::Concat {
                        pieces: serializable_pieces,
                    },
                )
            }
        };

        // Calculate ID after recursion completes
        let id = self.pool.len();

        // Add to pool
        self.pool.push(SerializableSourceInfo {
            id,
            start_offset,
            end_offset,
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

    /// Add source info fields to a JSON object.
    /// Always adds 's' field (source info ID).
    /// If config.include_inline_locations is true, also adds 'l' field with resolved location.
    fn add_source_info(
        &mut self,
        obj: &mut serde_json::Map<String, Value>,
        source_info: &SourceInfo,
    ) {
        let id = self.intern(source_info);
        obj.insert("s".to_string(), json!(id));

        if self.config.include_inline_locations {
            if let Some(location) = resolve_location(source_info, self.context) {
                obj.insert("l".to_string(), location);
            }
        }
    }
}

/// Resolve source info to fully resolved location with file_id, line, column, and offset.
///
/// Returns None if the source info cannot be mapped (e.g., synthetic nodes).
///
/// The returned JSON has the structure:
/// ```json
/// {
///   "f": file_id,
///   "b": {"o": offset, "l": line (1-based), "c": column (1-based)},
///   "e": {"o": offset, "l": line (1-based), "c": column (1-based)}
/// }
/// ```
fn resolve_location(source_info: &SourceInfo, context: &ASTContext) -> Option<Value> {
    // Map both start and end offsets through the transformation chain
    let (start_mapped, end_mapped) =
        source_info.map_range(0, source_info.length(), &context.source_context)?;

    // Convert from 0-indexed (internal) to 1-based (output) for line and column
    Some(json!({
        "f": start_mapped.file_id.0,
        "b": {
            "o": start_mapped.location.offset,
            "l": start_mapped.location.row + 1,
            "c": start_mapped.location.column + 1
        },
        "e": {
            "o": end_mapped.location.offset,
            "l": end_mapped.location.row + 1,
            "c": end_mapped.location.column + 1
        }
    }))
}

/// Helper to build a node JSON object with type, optional content, and source info.
///
/// This centralizes the pattern of creating nodes with 't', 'c', 's', and optionally 'l' fields.
fn node_with_source(
    t: &str,
    c: Option<Value>,
    source_info: &SourceInfo,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("t".to_string(), json!(t));
    if let Some(content) = c {
        obj.insert("c".to_string(), content);
    }
    serializer.add_source_info(&mut obj, source_info);
    Value::Object(obj)
}

// NOTE: This function is currently unused and would need a SourceContext parameter
// to map offsets to row/column positions. Commenting out for now.
// fn write_location(source_info: &quarto_source_map::SourceInfo, ctx: &SourceContext) -> Value {
//     // Extract filename index by walking to the Original mapping
//     let filename_index = crate::pandoc::location::extract_filename_index(source_info);
//
//     // Map start and end offsets to locations with row/column
//     let start_mapped = source_info.map_offset(0, ctx).unwrap();
//     let end_mapped = source_info.map_offset(source_info.length(), ctx).unwrap();
//
//     json!({
//         "start": {
//             "offset": source_info.start_offset(),
//             "row": start_mapped.location.row,
//             "column": start_mapped.location.column,
//         },
//         "end": {
//             "offset": source_info.end_offset(),
//             "row": end_mapped.location.row,
//             "column": end_mapped.location.column,
//         },
//         "filenameIndex": filename_index,
//     })
// }

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

/// Serialize AttrSourceInfo as JSON.
///
/// Format: {
///   "id": <source_info_ref or null>,
///   "classes": [<source_info_ref or null>, ...],
///   "kvs": [[<key_ref or null>, <value_ref or null>], ...]
/// }
fn write_attr_source(attr_source: &AttrSourceInfo, serializer: &mut SourceInfoSerializer) -> Value {
    json!({
        "id": attr_source.id.as_ref().map(|s| serializer.to_json_ref(s)),
        "classes": attr_source.classes.iter().map(|cls|
            cls.as_ref().map(|s| serializer.to_json_ref(s))
        ).collect::<Vec<_>>(),
        "kvs": attr_source.attributes.iter().map(|(k, v)|
            json!([
                k.as_ref().map(|s| serializer.to_json_ref(s)),
                v.as_ref().map(|s| serializer.to_json_ref(s))
            ])
        ).collect::<Vec<_>>()
    })
}

fn write_target_source(
    target_source: &TargetSourceInfo,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    json!([
        target_source
            .url
            .as_ref()
            .map(|s| serializer.to_json_ref(s)),
        target_source
            .title
            .as_ref()
            .map(|s| serializer.to_json_ref(s))
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
        Inline::Str(s) => node_with_source(
            "Str",
            Some(json!(s.text)),
            &s.source_info,
            serializer,
        ),
        Inline::Space(space) => node_with_source(
            "Space",
            None,
            &space.source_info,
            serializer,
        ),
        Inline::LineBreak(lb) => node_with_source(
            "LineBreak",
            None,
            &lb.source_info,
            serializer,
        ),
        Inline::SoftBreak(sb) => node_with_source(
            "SoftBreak",
            None,
            &sb.source_info,
            serializer,
        ),
        Inline::Emph(e) => node_with_source(
            "Emph",
            Some(write_inlines(&e.content, serializer)),
            &e.source_info,
            serializer,
        ),
        Inline::Strong(s) => node_with_source(
            "Strong",
            Some(write_inlines(&s.content, serializer)),
            &s.source_info,
            serializer,
        ),
        Inline::Code(c) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Code"));
            obj.insert("c".to_string(), json!([write_attr(&c.attr), c.text]));
            serializer.add_source_info(&mut obj, &c.source_info);
            obj.insert("attrS".to_string(), write_attr_source(&c.attr_source, serializer));
            Value::Object(obj)
        }
        Inline::Math(m) => {
            let math_type = match m.math_type {
                crate::pandoc::MathType::InlineMath => json!({"t": "InlineMath"}),
                crate::pandoc::MathType::DisplayMath => json!({"t": "DisplayMath"}),
            };
            node_with_source(
                "Math",
                Some(json!([math_type, m.text])),
                &m.source_info,
                serializer,
            )
        }
        Inline::Underline(u) => node_with_source(
            "Underline",
            Some(write_inlines(&u.content, serializer)),
            &u.source_info,
            serializer,
        ),
        Inline::Strikeout(s) => node_with_source(
            "Strikeout",
            Some(write_inlines(&s.content, serializer)),
            &s.source_info,
            serializer,
        ),
        Inline::Superscript(s) => node_with_source(
            "Superscript",
            Some(write_inlines(&s.content, serializer)),
            &s.source_info,
            serializer,
        ),
        Inline::Subscript(s) => node_with_source(
            "Subscript",
            Some(write_inlines(&s.content, serializer)),
            &s.source_info,
            serializer,
        ),
        Inline::SmallCaps(s) => node_with_source(
            "SmallCaps",
            Some(write_inlines(&s.content, serializer)),
            &s.source_info,
            serializer,
        ),
        Inline::Quoted(q) => {
            let quote_type = match q.quote_type {
                crate::pandoc::QuoteType::SingleQuote => json!({"t": "SingleQuote"}),
                crate::pandoc::QuoteType::DoubleQuote => json!({"t": "DoubleQuote"}),
            };
            node_with_source(
                "Quoted",
                Some(json!([quote_type, write_inlines(&q.content, serializer)])),
                &q.source_info,
                serializer,
            )
        }
        Inline::Link(link) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Link"));
            obj.insert("c".to_string(), json!([
                write_attr(&link.attr),
                write_inlines(&link.content, serializer),
                [link.target.0, link.target.1]
            ]));
            serializer.add_source_info(&mut obj, &link.source_info);
            obj.insert("attrS".to_string(), write_attr_source(&link.attr_source, serializer));
            obj.insert("targetS".to_string(), write_target_source(&link.target_source, serializer));
            Value::Object(obj)
        }
        Inline::RawInline(raw) => node_with_source(
            "RawInline",
            Some(json!([raw.format.clone(), raw.text.clone()])),
            &raw.source_info,
            serializer,
        ),
        Inline::Image(image) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Image"));
            obj.insert("c".to_string(), json!([
                write_attr(&image.attr),
                write_inlines(&image.content, serializer),
                [image.target.0, image.target.1]
            ]));
            serializer.add_source_info(&mut obj, &image.source_info);
            obj.insert("attrS".to_string(), write_attr_source(&image.attr_source, serializer));
            obj.insert("targetS".to_string(), write_target_source(&image.target_source, serializer));
            Value::Object(obj)
        }
        Inline::Span(span) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Span"));
            obj.insert("c".to_string(), json!([
                write_attr(&span.attr),
                write_inlines(&span.content, serializer)
            ]));
            serializer.add_source_info(&mut obj, &span.source_info);
            obj.insert("attrS".to_string(), write_attr_source(&span.attr_source, serializer));
            Value::Object(obj)
        }
        Inline::Note(note) => node_with_source(
            "Note",
            Some(write_blocks(&note.content, serializer)),
            &note.source_info,
            serializer,
        ),
        // we can't test this just yet because
        // our citationNoteNum counter doesn't match Pandoc's
        Inline::Cite(cite) => node_with_source(
            "Cite",
            Some(json!([
                cite.citations.iter().map(|citation| {
                    json!({
                        "citationId": citation.id.clone(),
                        "citationPrefix": write_inlines(&citation.prefix, serializer),
                        "citationSuffix": write_inlines(&citation.suffix, serializer),
                        "citationMode": write_citation_mode(&citation.mode),
                        "citationHash": citation.hash,
                        "citationNoteNum": citation.note_num,
                        "citationIdS": citation.id_source.as_ref().map(|s| serializer.to_json_ref(s))
                    })
                }).collect::<Vec<_>>(),
                write_inlines(&cite.content, serializer)
            ])),
            &cite.source_info,
            serializer,
        ),
        Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_, _)
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

// Write caption as Pandoc array format: [short, long]
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

// Write caption source info separately
fn write_caption_source(caption: &Caption, serializer: &mut SourceInfoSerializer) -> Value {
    json!(serializer.to_json_ref(&caption.source_info))
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

// Write cell as Pandoc array format: [attr, alignment, rowSpan, colSpan, content]
fn write_cell(cell: &crate::pandoc::table::Cell, serializer: &mut SourceInfoSerializer) -> Value {
    json!([
        write_attr(&cell.attr),
        write_alignment(&cell.alignment),
        cell.row_span,
        cell.col_span,
        write_blocks(&cell.content, serializer)
    ])
}

// Write cell source info separately
fn write_cell_source(
    cell: &crate::pandoc::table::Cell,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    json!({
        "s": serializer.to_json_ref(&cell.source_info),
        "attrS": write_attr_source(&cell.attr_source, serializer)
    })
}

// Write row as Pandoc array format: [attr, cells]
fn write_row(row: &crate::pandoc::table::Row, serializer: &mut SourceInfoSerializer) -> Value {
    json!([
        write_attr(&row.attr),
        row.cells
            .iter()
            .map(|cell| write_cell(cell, serializer))
            .collect::<Vec<_>>()
    ])
}

// Write row source info separately
fn write_row_source(
    row: &crate::pandoc::table::Row,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    json!({
        "s": serializer.to_json_ref(&row.source_info),
        "attrS": write_attr_source(&row.attr_source, serializer),
        "cellsS": row.cells
            .iter()
            .map(|cell| write_cell_source(cell, serializer))
            .collect::<Vec<_>>()
    })
}

// Write table head as Pandoc array format: [attr, rows]
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

// Write table head source info separately
fn write_table_head_source(
    head: &crate::pandoc::table::TableHead,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    json!({
        "s": serializer.to_json_ref(&head.source_info),
        "attrS": write_attr_source(&head.attr_source, serializer),
        "rowsS": head.rows
            .iter()
            .map(|row| write_row_source(row, serializer))
            .collect::<Vec<_>>()
    })
}

// Write table body as Pandoc array format: [attr, rowHeadColumns, head, body]
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

// Write table body source info separately
fn write_table_body_source(
    body: &crate::pandoc::table::TableBody,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    json!({
        "s": serializer.to_json_ref(&body.source_info),
        "attrS": write_attr_source(&body.attr_source, serializer),
        "headS": body.head
            .iter()
            .map(|row| write_row_source(row, serializer))
            .collect::<Vec<_>>(),
        "bodyS": body.body
            .iter()
            .map(|row| write_row_source(row, serializer))
            .collect::<Vec<_>>()
    })
}

// Write table foot as Pandoc array format: [attr, rows]
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

// Write table foot source info separately
fn write_table_foot_source(
    foot: &crate::pandoc::table::TableFoot,
    serializer: &mut SourceInfoSerializer,
) -> Value {
    json!({
        "s": serializer.to_json_ref(&foot.source_info),
        "attrS": write_attr_source(&foot.attr_source, serializer),
        "rowsS": foot.rows
            .iter()
            .map(|row| write_row_source(row, serializer))
            .collect::<Vec<_>>()
    })
}

fn write_block(block: &Block, serializer: &mut SourceInfoSerializer) -> Value {
    match block {
        Block::Figure(figure) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Figure"));
            obj.insert(
                "c".to_string(),
                json!([
                    write_attr(&figure.attr),
                    write_caption(&figure.caption, serializer),
                    write_blocks(&figure.content, serializer)
                ]),
            );
            serializer.add_source_info(&mut obj, &figure.source_info);
            obj.insert(
                "attrS".to_string(),
                write_attr_source(&figure.attr_source, serializer),
            );
            Value::Object(obj)
        }
        Block::DefinitionList(deflist) => node_with_source(
            "DefinitionList",
            Some(json!(
                deflist
                    .content
                    .iter()
                    .map(|(term, definition)| {
                        json!([
                            write_inlines(term, serializer),
                            write_blockss(&definition, serializer),
                        ])
                    })
                    .collect::<Vec<_>>()
            )),
            &deflist.source_info,
            serializer,
        ),
        Block::OrderedList(orderedlist) => node_with_source(
            "OrderedList",
            Some(json!([
                write_list_attributes(&orderedlist.attr),
                write_blockss(&orderedlist.content, serializer),
            ])),
            &orderedlist.source_info,
            serializer,
        ),
        Block::RawBlock(raw) => node_with_source(
            "RawBlock",
            Some(json!([raw.format.clone(), raw.text.clone()])),
            &raw.source_info,
            serializer,
        ),
        Block::HorizontalRule(block) => {
            node_with_source("HorizontalRule", None, &block.source_info, serializer)
        }
        Block::Table(table) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Table"));
            obj.insert(
                "c".to_string(),
                json!([
                    write_attr(&table.attr),
                    write_caption(&table.caption, serializer),
                    table.colspec.iter().map(write_colspec).collect::<Vec<_>>(),
                    write_table_head(&table.head, serializer),
                    table
                        .bodies
                        .iter()
                        .map(|body| write_table_body(body, serializer))
                        .collect::<Vec<_>>(),
                    write_table_foot(&table.foot, serializer)
                ]),
            );
            serializer.add_source_info(&mut obj, &table.source_info);
            obj.insert(
                "attrS".to_string(),
                write_attr_source(&table.attr_source, serializer),
            );
            obj.insert(
                "captionS".to_string(),
                write_caption_source(&table.caption, serializer),
            );
            obj.insert(
                "headS".to_string(),
                write_table_head_source(&table.head, serializer),
            );
            obj.insert(
                "bodiesS".to_string(),
                json!(
                    table
                        .bodies
                        .iter()
                        .map(|body| write_table_body_source(body, serializer))
                        .collect::<Vec<_>>()
                ),
            );
            obj.insert(
                "footS".to_string(),
                write_table_foot_source(&table.foot, serializer),
            );
            Value::Object(obj)
        }

        Block::Div(div) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Div"));
            obj.insert(
                "c".to_string(),
                json!([
                    write_attr(&div.attr),
                    write_blocks(&div.content, serializer)
                ]),
            );
            serializer.add_source_info(&mut obj, &div.source_info);
            obj.insert(
                "attrS".to_string(),
                write_attr_source(&div.attr_source, serializer),
            );
            Value::Object(obj)
        }
        Block::BlockQuote(quote) => node_with_source(
            "BlockQuote",
            Some(write_blocks(&quote.content, serializer)),
            &quote.source_info,
            serializer,
        ),
        Block::LineBlock(lineblock) => node_with_source(
            "LineBlock",
            Some(json!(
                lineblock
                    .content
                    .iter()
                    .map(|inlines| write_inlines(inlines, serializer))
                    .collect::<Vec<_>>()
            )),
            &lineblock.source_info,
            serializer,
        ),
        Block::Paragraph(para) => node_with_source(
            "Para",
            Some(write_inlines(&para.content, serializer)),
            &para.source_info,
            serializer,
        ),
        Block::Header(header) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("Header"));
            obj.insert(
                "c".to_string(),
                json!([
                    header.level,
                    write_attr(&header.attr),
                    write_inlines(&header.content, serializer)
                ]),
            );
            serializer.add_source_info(&mut obj, &header.source_info);
            obj.insert(
                "attrS".to_string(),
                write_attr_source(&header.attr_source, serializer),
            );
            Value::Object(obj)
        }
        Block::CodeBlock(codeblock) => {
            let mut obj = serde_json::Map::new();
            obj.insert("t".to_string(), json!("CodeBlock"));
            obj.insert(
                "c".to_string(),
                json!([write_attr(&codeblock.attr), codeblock.text]),
            );
            serializer.add_source_info(&mut obj, &codeblock.source_info);
            obj.insert(
                "attrS".to_string(),
                write_attr_source(&codeblock.attr_source, serializer),
            );
            Value::Object(obj)
        }
        Block::Plain(plain) => node_with_source(
            "Plain",
            Some(write_inlines(&plain.content, serializer)),
            &plain.source_info,
            serializer,
        ),
        Block::BulletList(bulletlist) => node_with_source(
            "BulletList",
            Some(json!(
                bulletlist
                    .content
                    .iter()
                    .map(|blocks| blocks
                        .iter()
                        .map(|block| write_block(block, serializer))
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            )),
            &bulletlist.source_info,
            serializer,
        ),
        Block::BlockMetadata(meta) => node_with_source(
            "BlockMetadata",
            Some(write_meta_value_with_source_info(&meta.meta, serializer)),
            &meta.source_info,
            serializer,
        ),
        Block::NoteDefinitionPara(refdef) => node_with_source(
            "NoteDefinitionPara",
            Some(json!([
                refdef.id,
                write_inlines(&refdef.content, serializer)
            ])),
            &refdef.source_info,
            serializer,
        ),
        Block::NoteDefinitionFencedBlock(refdef) => node_with_source(
            "NoteDefinitionFencedBlock",
            Some(json!([
                refdef.id,
                write_blocks(&refdef.content, serializer)
            ])),
            &refdef.source_info,
            serializer,
        ),
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

fn write_pandoc(pandoc: &Pandoc, context: &ASTContext, config: &JsonConfig) -> Value {
    // Create the SourceInfo serializer
    let mut serializer = SourceInfoSerializer::new(context, config);

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

    // Serialize files array combining filenames and FileInformation
    // Each file entry has: "name", "line_breaks", "total_length"
    let files_array: Vec<Value> = (0..context.filenames.len())
        .map(|idx| {
            let filename = &context.filenames[idx];
            let file_info = context
                .source_context
                .get_file(quarto_source_map::FileId(idx))
                .and_then(|file| file.file_info.as_ref());

            if let Some(info) = file_info {
                // File with FileInformation - serialize everything
                json!({
                    "name": filename,
                    "line_breaks": info.line_breaks(),
                    "total_length": info.total_length()
                })
            } else {
                // File without FileInformation - just the name
                json!({
                    "name": filename
                })
            }
        })
        .collect();

    ast_context_obj.insert("files".to_string(), json!(files_array));

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

/// Write Pandoc AST to JSON with custom configuration.
pub fn write_with_config<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
    config: &JsonConfig,
) -> std::io::Result<()> {
    let json = write_pandoc(pandoc, context, config);
    serde_json::to_writer(writer, &json)?;
    Ok(())
}

/// Write Pandoc AST to JSON with default configuration.
pub fn write<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
) -> std::io::Result<()> {
    write_with_config(pandoc, context, writer, &JsonConfig::default())
}
