/*
 * json.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::location::{Location, Range};
use crate::pandoc::{
    Alignment, Attr, AttrSourceInfo, Block, BlockQuote, BulletList, Caption, Cell, Citation,
    CitationMode, Cite, Code, CodeBlock, ColSpec, ColWidth, CustomNode, DefinitionList, Div, Emph,
    Figure, Header, HorizontalRule, Image, Inline, Inlines, LineBlock, Link, ListAttributes,
    ListNumberDelim, ListNumberStyle, Math, MathType, MetaBlock, MetaMapEntry,
    MetaValueWithSourceInfo, Note, OrderedList, Pandoc, Paragraph, Plain, QuoteType, Quoted,
    RawBlock, RawInline, Row, Slot, SmallCaps, SoftBreak, Space, Span, Str, Strikeout, Strong,
    Subscript, Superscript, Table, TableBody, TableFoot, TableHead, Underline,
};
use crate::template::meta_to_config_value;
use hashlink::LinkedHashMap;
use quarto_source_map::FileId;
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug)]
pub enum JsonReadError {
    InvalidJson(serde_json::Error),
    MissingField(String),
    InvalidType(String),
    UnsupportedVariant(String),
    InvalidSourceInfoRef(usize),
    ExpectedSourceInfoRef,
    MalformedSourceInfoPool,
    CircularSourceInfoReference(usize),
}

impl std::fmt::Display for JsonReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonReadError::InvalidJson(e) => write!(f, "Invalid JSON: {}", e),
            JsonReadError::MissingField(field) => write!(f, "Missing required field: {}", field),
            JsonReadError::InvalidType(msg) => write!(f, "Invalid type: {}", msg),
            JsonReadError::UnsupportedVariant(variant) => {
                write!(f, "Unsupported variant: {}", variant)
            }
            JsonReadError::InvalidSourceInfoRef(id) => {
                write!(f, "Invalid SourceInfo reference ID: {}", id)
            }
            JsonReadError::ExpectedSourceInfoRef => {
                write!(f, "Expected SourceInfo $ref, got inline SourceInfo")
            }
            JsonReadError::MalformedSourceInfoPool => {
                write!(f, "Malformed sourceInfoPool in astContext")
            }
            JsonReadError::CircularSourceInfoReference(id) => {
                write!(
                    f,
                    "Circular or forward reference in sourceInfoPool: ID {} references a parent that doesn't exist yet",
                    id
                )
            }
        }
    }
}

impl std::error::Error for JsonReadError {}

type Result<T> = std::result::Result<T, JsonReadError>;

/// Deserializer that reconstructs SourceInfo objects from a pool.
///
/// During JSON deserialization, the sourceInfoPool from astContext is parsed
/// into a Vec<SourceInfo>. References in the AST ({"$ref": id}) are resolved
/// by looking up the ID in this pool.
///
/// The pool must be built in topological order (parents before children) so
/// that when we reconstruct a SourceInfo with a parent_id, the parent already
/// exists in the pool.
///
/// **Backward Compatibility**: When reading JSON without source info (legacy format),
/// all SourceInfo::default() instances throughout this file are intentional and provide
/// backward compatibility with JSON that predates source tracking.
#[derive(Debug)]
struct SourceInfoDeserializer {
    pool: Vec<quarto_source_map::SourceInfo>,
}

impl SourceInfoDeserializer {
    /// Create a new empty deserializer (for documents without SourceInfo)
    fn empty() -> Self {
        SourceInfoDeserializer { pool: Vec::new() }
    }

    /// Build the pool from the sourceInfoPool JSON array (compact format)
    ///
    /// New format: {"r": [start_offset, end_offset], "t": type_code, "d": data}
    /// Old format: {"r": [start_off, start_row, start_col, end_off, end_row, end_col], "t": type_code, "d": data}
    /// ID is implicit from array index
    ///
    /// Note: Row/column information from old format is ignored since SourceInfo now stores only offsets.
    fn new(pool_json: &Value) -> Result<Self> {
        let pool_array = pool_json
            .as_array()
            .ok_or(JsonReadError::MalformedSourceInfoPool)?;

        let mut pool: Vec<quarto_source_map::SourceInfo> = Vec::with_capacity(pool_array.len());

        // Build pool in order - parents must come before children
        for (current_index, item) in pool_array.iter().enumerate() {
            // Parse offsets from "r" array
            let range_array = item
                .get("r")
                .and_then(|v| v.as_array())
                .ok_or(JsonReadError::MalformedSourceInfoPool)?;

            let (start_offset, end_offset) = match range_array.len() {
                2 => {
                    // New format: [start_offset, end_offset]
                    let start = range_array[0]
                        .as_u64()
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?
                        as usize;
                    let end = range_array[1]
                        .as_u64()
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?
                        as usize;
                    (start, end)
                }
                6 => {
                    // Old format: [start_offset, start_row, start_col, end_offset, end_row, end_col]
                    // Extract only offsets, ignore row/column
                    let start = range_array[0]
                        .as_u64()
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?
                        as usize;
                    let end = range_array[3]
                        .as_u64()
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?
                        as usize;
                    (start, end)
                }
                _ => return Err(JsonReadError::MalformedSourceInfoPool),
            };

            // Parse type code from "t"
            let type_code =
                item.get("t")
                    .and_then(|v| v.as_u64())
                    .ok_or(JsonReadError::MalformedSourceInfoPool)? as usize;

            // Parse data from "d"
            let data = item
                .get("d")
                .ok_or(JsonReadError::MalformedSourceInfoPool)?;

            let source_info = match type_code {
                0 => {
                    // Original: data is file_id (number)
                    let file_id = data
                        .as_u64()
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?
                        as usize;
                    quarto_source_map::SourceInfo::Original {
                        file_id: FileId(file_id),
                        start_offset,
                        end_offset,
                    }
                }
                1 => {
                    // Substring: data is parent_id (new format) or [parent_id, offset] (old format)
                    // In new format, offsets are already in start_offset/end_offset above
                    let parent_id = if let Some(id) = data.as_u64() {
                        // New format: just parent_id
                        id as usize
                    } else if let Some(data_array) = data.as_array() {
                        // Old format: [parent_id, offset] - ignore offset, use start_offset/end_offset
                        if data_array.len() != 2 {
                            return Err(JsonReadError::MalformedSourceInfoPool);
                        }
                        data_array[0]
                            .as_u64()
                            .ok_or(JsonReadError::MalformedSourceInfoPool)?
                            as usize
                    } else {
                        return Err(JsonReadError::MalformedSourceInfoPool);
                    };

                    // Check for circular/forward references
                    if parent_id >= current_index {
                        return Err(JsonReadError::CircularSourceInfoReference(parent_id));
                    }

                    let parent = pool
                        .get(parent_id)
                        .ok_or(JsonReadError::InvalidSourceInfoRef(parent_id))?
                        .clone();

                    quarto_source_map::SourceInfo::Substring {
                        parent: Arc::new(parent),
                        start_offset,
                        end_offset,
                    }
                }
                2 => {
                    // Concat: data is [[source_info_id, offset_in_concat, length], ...]
                    let pieces_array = data
                        .as_array()
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?;

                    let pieces: Result<Vec<quarto_source_map::SourcePiece>> = pieces_array
                        .iter()
                        .map(|piece_array| {
                            let piece = piece_array
                                .as_array()
                                .ok_or(JsonReadError::MalformedSourceInfoPool)?;
                            if piece.len() != 3 {
                                return Err(JsonReadError::MalformedSourceInfoPool);
                            }
                            let source_info_id = piece[0]
                                .as_u64()
                                .ok_or(JsonReadError::MalformedSourceInfoPool)?
                                as usize;
                            let offset_in_concat = piece[1]
                                .as_u64()
                                .ok_or(JsonReadError::MalformedSourceInfoPool)?
                                as usize;
                            let length = piece[2]
                                .as_u64()
                                .ok_or(JsonReadError::MalformedSourceInfoPool)?
                                as usize;

                            // Check for circular/forward references
                            if source_info_id >= current_index {
                                return Err(JsonReadError::CircularSourceInfoReference(
                                    source_info_id,
                                ));
                            }

                            let source_info = pool
                                .get(source_info_id)
                                .ok_or(JsonReadError::InvalidSourceInfoRef(source_info_id))?
                                .clone();

                            Ok(quarto_source_map::SourcePiece {
                                source_info,
                                offset_in_concat,
                                length,
                            })
                        })
                        .collect();

                    quarto_source_map::SourceInfo::Concat { pieces: pieces? }
                }
                3 => {
                    // Transformed variant no longer exists in SourceInfo
                    // Convert to approximate Substring pointing to parent
                    // This loses the transformation mapping but preserves the parent relationship
                    let data_array = data
                        .as_array()
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?;
                    if data_array.is_empty() {
                        return Err(JsonReadError::MalformedSourceInfoPool);
                    }
                    let parent_id = data_array[0]
                        .as_u64()
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?
                        as usize;

                    // Check for circular/forward references
                    if parent_id >= current_index {
                        return Err(JsonReadError::CircularSourceInfoReference(parent_id));
                    }

                    let parent = pool
                        .get(parent_id)
                        .ok_or(JsonReadError::InvalidSourceInfoRef(parent_id))?
                        .clone();

                    // Approximate with Substring
                    quarto_source_map::SourceInfo::Substring {
                        parent: Arc::new(parent),
                        start_offset,
                        end_offset,
                    }
                }
                _ => {
                    return Err(JsonReadError::MalformedSourceInfoPool);
                }
            };

            pool.push(source_info);
        }

        Ok(SourceInfoDeserializer { pool })
    }

    /// Resolve a numeric reference to a SourceInfo
    fn from_json_ref(&self, value: &Value) -> Result<quarto_source_map::SourceInfo> {
        if let Some(ref_id) = value.as_u64() {
            let id = ref_id as usize;
            self.pool
                .get(id)
                .cloned()
                .ok_or(JsonReadError::InvalidSourceInfoRef(id))
        } else {
            Err(JsonReadError::ExpectedSourceInfoRef)
        }
    }
}

/// Convert from old JSON format (filename_index, range) to new SourceInfo
fn make_source_info(filename_index: Option<usize>, range: Range) -> quarto_source_map::SourceInfo {
    let file_id = FileId(filename_index.unwrap_or(0));
    let qsm_range = quarto_source_map::Range {
        start: quarto_source_map::Location {
            offset: range.start.offset,
            row: range.start.row,
            column: range.start.column,
        },
        end: quarto_source_map::Location {
            offset: range.end.offset,
            row: range.end.row,
            column: range.end.column,
        },
    };
    quarto_source_map::SourceInfo::from_range(file_id, qsm_range)
}

fn empty_range() -> Range {
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
    }
}

fn read_location(value: &Value) -> Option<(Option<usize>, Range)> {
    let obj = value.as_object()?;
    let start_obj = obj.get("start")?.as_object()?;
    let end_obj = obj.get("end")?.as_object()?;

    let start = Location {
        offset: start_obj.get("offset")?.as_u64()? as usize,
        row: start_obj.get("row")?.as_u64()? as usize,
        column: start_obj.get("column")?.as_u64()? as usize,
    };

    let end = Location {
        offset: end_obj.get("offset")?.as_u64()? as usize,
        row: end_obj.get("row")?.as_u64()? as usize,
        column: end_obj.get("column")?.as_u64()? as usize,
    };

    let filename_index = obj
        .get("filenameIndex")
        .and_then(|v| v.as_u64())
        .map(|i| i as usize);

    Some((filename_index, Range { start, end }))
}

fn read_attr(value: &Value) -> Result<Attr> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for Attr".to_string()))?;

    if arr.len() != 3 {
        return Err(JsonReadError::InvalidType(
            "Attr array must have 3 elements".to_string(),
        ));
    }

    let id = arr[0]
        .as_str()
        .ok_or_else(|| JsonReadError::InvalidType("Attr id must be string".to_string()))?
        .to_string();

    let classes = arr[1]
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Attr classes must be array".to_string()))?
        .iter()
        .map(|v| {
            v.as_str()
                .ok_or_else(|| JsonReadError::InvalidType("Class must be string".to_string()))
                .map(|s| s.to_string())
        })
        .collect::<Result<Vec<_>>>()?;

    let kvs = arr[2]
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Attr key-values must be array".to_string()))?
        .iter()
        .map(|v| {
            let kv_arr = v.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Key-value pair must be array".to_string())
            })?;
            if kv_arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "Key-value pair must have 2 elements".to_string(),
                ));
            }
            let key = kv_arr[0]
                .as_str()
                .ok_or_else(|| JsonReadError::InvalidType("Key must be string".to_string()))?
                .to_string();
            let value = kv_arr[1]
                .as_str()
                .ok_or_else(|| JsonReadError::InvalidType("Value must be string".to_string()))?
                .to_string();
            Ok((key, value))
        })
        .collect::<Result<Vec<_>>>()?;

    let kvs_map = kvs.into_iter().collect();
    Ok((id, classes, kvs_map))
}

/// Read AttrSourceInfo from JSON, returning empty if not present or null.
///
/// Format: {
///   "id": <source_info_ref or null>,
///   "classes": [<source_info_ref or null>, ...],
///   "kvs": [[<key_ref or null>, <val_ref or null>], ...]
/// }
fn read_attr_source(
    value: Option<&Value>,
    deserializer: &SourceInfoDeserializer,
) -> Result<AttrSourceInfo> {
    // If attrS field is missing or null, return empty
    let Some(obj) = value.and_then(|v| v.as_object()) else {
        return Ok(AttrSourceInfo::empty());
    };

    // Read id (optional SourceInfo ref or null)
    let id = obj
        .get("id")
        .and_then(|v| {
            if v.is_null() {
                None
            } else {
                Some(deserializer.from_json_ref(v).ok())
            }
        })
        .flatten();

    // Read classes (array of optional SourceInfo refs)
    let classes = obj
        .get("classes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|v| {
                    if v.is_null() {
                        Ok(None)
                    } else {
                        deserializer.from_json_ref(v).map(Some)
                    }
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();

    // Read kvs (array of [key_ref, val_ref] pairs)
    let attributes = obj
        .get("kvs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|v| {
                    let pair = v.as_array().ok_or_else(|| {
                        JsonReadError::InvalidType(
                            "AttrSourceInfo kvs entry must be array".to_string(),
                        )
                    })?;
                    if pair.len() != 2 {
                        return Err(JsonReadError::InvalidType(
                            "AttrSourceInfo kvs entry must have 2 elements".to_string(),
                        ));
                    }
                    let key = if pair[0].is_null() {
                        None
                    } else {
                        Some(deserializer.from_json_ref(&pair[0])?)
                    };
                    let val = if pair[1].is_null() {
                        None
                    } else {
                        Some(deserializer.from_json_ref(&pair[1])?)
                    };
                    Ok((key, val))
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(AttrSourceInfo {
        id,
        classes,
        attributes,
    })
}

fn read_citation_mode(value: &Value) -> Result<CitationMode> {
    let obj = value.as_object().ok_or_else(|| {
        JsonReadError::InvalidType("Expected object for CitationMode".to_string())
    })?;
    let t = obj
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;

    match t {
        "NormalCitation" => Ok(CitationMode::NormalCitation),
        "AuthorInText" => Ok(CitationMode::AuthorInText),
        "SuppressAuthor" => Ok(CitationMode::SuppressAuthor),
        _ => Err(JsonReadError::UnsupportedVariant(format!(
            "CitationMode: {}",
            t
        ))),
    }
}

fn read_inline(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<Inline> {
    let obj = value
        .as_object()
        .ok_or_else(|| JsonReadError::InvalidType("Expected object for Inline".to_string()))?;
    let t = obj
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;

    // Extract source information - try new format ("s" field) first, fall back to old format ("l" field)
    let source_info = if let Some(s_val) = obj.get("s") {
        // New format: source info reference to pool
        deserializer.from_json_ref(s_val)?
    } else {
        // Old format: inline location
        let (filename_index, range) = obj
            .get("l")
            .and_then(read_location)
            .unwrap_or_else(|| (None, empty_range()));
        make_source_info(filename_index, range)
    };

    match t {
        "Str" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let text = c
                .as_str()
                .ok_or_else(|| {
                    JsonReadError::InvalidType("Str content must be string".to_string())
                })?
                .to_string();
            Ok(Inline::Str(Str { text, source_info }))
        }
        "Space" => Ok(Inline::Space(Space { source_info })),
        "LineBreak" => Ok(Inline::LineBreak(crate::pandoc::inline::LineBreak {
            source_info,
        })),
        "SoftBreak" => Ok(Inline::SoftBreak(SoftBreak { source_info })),
        "Emph" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Emph(Emph {
                content,
                source_info,
            }))
        }
        "Strong" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Strong(Strong {
                content,
                source_info,
            }))
        }
        "Code" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Code content must be array".to_string())
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "Code array must have 2 elements".to_string(),
                ));
            }
            let attr = read_attr(&arr[0])?;
            let text = arr[1]
                .as_str()
                .ok_or_else(|| JsonReadError::InvalidType("Code text must be string".to_string()))?
                .to_string();
            let attr_source = read_attr_source(obj.get("attrS"), deserializer)?;
            Ok(Inline::Code(Code {
                attr,
                text,
                source_info,
                attr_source,
            }))
        }
        "Math" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Math content must be array".to_string())
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "Math array must have 2 elements".to_string(),
                ));
            }

            let math_type_obj = arr[0].as_object().ok_or_else(|| {
                JsonReadError::InvalidType("Math type must be object".to_string())
            })?;
            let math_type_t = math_type_obj
                .get("t")
                .and_then(|v| v.as_str())
                .ok_or_else(|| JsonReadError::MissingField("t in math type".to_string()))?;
            let math_type = match math_type_t {
                "InlineMath" => MathType::InlineMath,
                "DisplayMath" => MathType::DisplayMath,
                _ => {
                    return Err(JsonReadError::UnsupportedVariant(format!(
                        "MathType: {}",
                        math_type_t
                    )));
                }
            };

            let text = arr[1]
                .as_str()
                .ok_or_else(|| JsonReadError::InvalidType("Math text must be string".to_string()))?
                .to_string();
            Ok(Inline::Math(Math {
                math_type,
                text,
                source_info,
            }))
        }
        "Underline" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Underline(Underline {
                content,
                source_info,
            }))
        }
        "Strikeout" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Strikeout(Strikeout {
                content,
                source_info,
            }))
        }
        "Superscript" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Superscript(Superscript {
                content,
                source_info,
            }))
        }
        "Subscript" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Subscript(Subscript {
                content,
                source_info,
            }))
        }
        "SmallCaps" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::SmallCaps(SmallCaps {
                content,
                source_info,
            }))
        }
        "Quoted" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Quoted content must be array".to_string())
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "Quoted array must have 2 elements".to_string(),
                ));
            }

            let quote_type_obj = arr[0].as_object().ok_or_else(|| {
                JsonReadError::InvalidType("Quote type must be object".to_string())
            })?;
            let quote_type_t = quote_type_obj
                .get("t")
                .and_then(|v| v.as_str())
                .ok_or_else(|| JsonReadError::MissingField("t in quote type".to_string()))?;
            let quote_type = match quote_type_t {
                "SingleQuote" => QuoteType::SingleQuote,
                "DoubleQuote" => QuoteType::DoubleQuote,
                _ => {
                    return Err(JsonReadError::UnsupportedVariant(format!(
                        "QuoteType: {}",
                        quote_type_t
                    )));
                }
            };

            let content = read_inlines(&arr[1], deserializer)?;
            Ok(Inline::Quoted(Quoted {
                quote_type,
                content,
                source_info,
            }))
        }
        "Link" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Link content must be array".to_string())
            })?;
            if arr.len() != 3 {
                return Err(JsonReadError::InvalidType(
                    "Link array must have 3 elements".to_string(),
                ));
            }

            let attr = read_attr(&arr[0])?;
            let content = read_inlines(&arr[1], deserializer)?;

            let target_arr = arr[2].as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Link target must be array".to_string())
            })?;
            if target_arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "Link target array must have 2 elements".to_string(),
                ));
            }
            let url = target_arr[0]
                .as_str()
                .ok_or_else(|| JsonReadError::InvalidType("Link URL must be string".to_string()))?
                .to_string();
            let title = target_arr[1]
                .as_str()
                .ok_or_else(|| JsonReadError::InvalidType("Link title must be string".to_string()))?
                .to_string();
            let target = (url, title);
            let attr_source = read_attr_source(obj.get("attrS"), deserializer)?;

            Ok(Inline::Link(Link {
                attr,
                content,
                target,
                source_info,
                attr_source,
                target_source: crate::pandoc::attr::TargetSourceInfo::empty(),
            }))
        }
        "RawInline" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("RawInline content must be array".to_string())
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "RawInline array must have 2 elements".to_string(),
                ));
            }
            let format = arr[0]
                .as_str()
                .ok_or_else(|| {
                    JsonReadError::InvalidType("RawInline format must be string".to_string())
                })?
                .to_string();
            let text = arr[1]
                .as_str()
                .ok_or_else(|| {
                    JsonReadError::InvalidType("RawInline text must be string".to_string())
                })?
                .to_string();
            Ok(Inline::RawInline(RawInline {
                format,
                text,
                source_info,
            }))
        }
        "Image" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Image content must be array".to_string())
            })?;
            if arr.len() != 3 {
                return Err(JsonReadError::InvalidType(
                    "Image array must have 3 elements".to_string(),
                ));
            }

            let attr = read_attr(&arr[0])?;
            let content = read_inlines(&arr[1], deserializer)?;

            let target_arr = arr[2].as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Image target must be array".to_string())
            })?;
            if target_arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "Image target array must have 2 elements".to_string(),
                ));
            }
            let url = target_arr[0]
                .as_str()
                .ok_or_else(|| JsonReadError::InvalidType("Image URL must be string".to_string()))?
                .to_string();
            let title = target_arr[1]
                .as_str()
                .ok_or_else(|| {
                    JsonReadError::InvalidType("Image title must be string".to_string())
                })?
                .to_string();
            let target = (url, title);
            let attr_source = read_attr_source(obj.get("attrS"), deserializer)?;

            Ok(Inline::Image(Image {
                attr,
                content,
                target,
                source_info,
                attr_source,
                target_source: crate::pandoc::attr::TargetSourceInfo::empty(),
            }))
        }
        "Span" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Span content must be array".to_string())
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "Span array must have 2 elements".to_string(),
                ));
            }

            let attr = read_attr(&arr[0])?;

            // Check if this is a custom node wrapper
            if attr.1.contains(&"__quarto_custom_node".to_string()) {
                return read_custom_inline_from_span(&attr, &arr[1], source_info, deserializer);
            }

            let content = read_inlines(&arr[1], deserializer)?;
            let attr_source = read_attr_source(obj.get("attrS"), deserializer)?;
            Ok(Inline::Span(Span {
                attr,
                content,
                source_info,
                attr_source,
            }))
        }
        "Note" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_blocks(c, deserializer)?;
            Ok(Inline::Note(Note {
                content,
                source_info,
            }))
        }
        "Cite" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let c_arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Cite content must be array".to_string())
            })?;

            if c_arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "Cite content must have 2 elements".to_string(),
                ));
            }

            // First element is the array of citations
            let citations_arr = c_arr[0]
                .as_array()
                .ok_or_else(|| JsonReadError::InvalidType("Citations must be array".to_string()))?;

            let citations = citations_arr
                .iter()
                .map(|citation_val| {
                    let citation_obj = citation_val.as_object().ok_or_else(|| {
                        JsonReadError::InvalidType("Citation must be object".to_string())
                    })?;

                    let id = citation_obj
                        .get("citationId")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| JsonReadError::MissingField("citationId".to_string()))?
                        .to_string();

                    let prefix = read_inlines(
                        citation_obj.get("citationPrefix").ok_or_else(|| {
                            JsonReadError::MissingField("citationPrefix".to_string())
                        })?,
                        deserializer,
                    )?;
                    let suffix = read_inlines(
                        citation_obj.get("citationSuffix").ok_or_else(|| {
                            JsonReadError::MissingField("citationSuffix".to_string())
                        })?,
                        deserializer,
                    )?;

                    let mode =
                        read_citation_mode(citation_obj.get("citationMode").ok_or_else(|| {
                            JsonReadError::MissingField("citationMode".to_string())
                        })?)?;

                    let hash = citation_obj
                        .get("citationHash")
                        .and_then(|v| v.as_i64())
                        .ok_or_else(|| JsonReadError::MissingField("citationHash".to_string()))?
                        as usize;

                    let note_num = citation_obj
                        .get("citationNoteNum")
                        .and_then(|v| v.as_i64())
                        .ok_or_else(|| JsonReadError::MissingField("citationNoteNum".to_string()))?
                        as usize;

                    Ok(Citation {
                        id,
                        prefix,
                        suffix,
                        mode,
                        hash,
                        note_num,
                        id_source: None,
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            // Second element is the content inlines
            let content = read_inlines(&c_arr[1], deserializer)?;

            Ok(Inline::Cite(Cite {
                citations,
                content,
                source_info,
            }))
        }
        _ => Err(JsonReadError::UnsupportedVariant(format!("Inline: {}", t))),
    }
}

fn read_inlines(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<Inlines> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for Inlines".to_string()))?;
    arr.iter().map(|v| read_inline(v, deserializer)).collect()
}

fn read_ast_context(value: &Value) -> Result<ASTContext> {
    let obj = value
        .as_object()
        .ok_or_else(|| JsonReadError::InvalidType("Expected object for ASTContext".to_string()))?;

    // Read files array - each entry has "name" and optionally "line_breaks"/"total_length"
    let files_val = obj
        .get("files")
        .ok_or_else(|| JsonReadError::MissingField("files".to_string()))?;

    let files_arr = files_val
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("files must be array".to_string()))?;

    let mut filenames = Vec::new();
    let mut source_context = quarto_source_map::SourceContext::new();

    for file_obj in files_arr {
        let file_map = file_obj
            .as_object()
            .ok_or_else(|| JsonReadError::InvalidType("file entry must be object".to_string()))?;

        // Extract filename
        let filename = file_map
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JsonReadError::MissingField("name in file entry".to_string()))?
            .to_string();

        filenames.push(filename.clone());

        // Try to extract FileInformation fields
        let has_line_breaks = file_map.get("line_breaks").is_some();
        let has_total_length = file_map.get("total_length").is_some();

        if has_line_breaks && has_total_length {
            // Deserialize FileInformation from the fields
            let line_breaks: Vec<usize> = serde_json::from_value(
                file_map.get("line_breaks").unwrap().clone(),
            )
            .map_err(|_| {
                JsonReadError::InvalidType("line_breaks must be array of numbers".to_string())
            })?;

            let total_length: usize = serde_json::from_value(
                file_map.get("total_length").unwrap().clone(),
            )
            .map_err(|_| JsonReadError::InvalidType("total_length must be number".to_string()))?;

            let file_info =
                quarto_source_map::FileInformation::from_parts(line_breaks, total_length);
            source_context.add_file_with_info(filename, file_info);
        } else {
            // No FileInformation - try to read from disk
            source_context.add_file(filename, None);
        }
    }

    Ok(ASTContext {
        filenames,
        example_list_counter: std::cell::Cell::new(1),
        source_context,
        parent_source_info: None,
    })
}

pub fn read<R: std::io::Read>(reader: &mut R) -> Result<(Pandoc, ASTContext)> {
    let mut buffer = String::new();
    reader
        .read_to_string(&mut buffer)
        .map_err(|e| JsonReadError::InvalidJson(serde_json::Error::io(e)))?;
    let json: Value = serde_json::from_str(&buffer).map_err(JsonReadError::InvalidJson)?;
    read_pandoc(&json)
}

fn read_pandoc(value: &Value) -> Result<(Pandoc, ASTContext)> {
    let obj = value
        .as_object()
        .ok_or_else(|| JsonReadError::InvalidType("Expected object for Pandoc".to_string()))?;

    // We could validate the API version here if needed
    // let _api_version = obj.get("pandoc-api-version");

    // Read astContext first (we need it for key sources and source info pool)
    let context = if let Some(ast_context_val) = obj.get("astContext") {
        read_ast_context(ast_context_val)?
    } else {
        // If no astContext is present, create an empty one for backward compatibility
        ASTContext::new()
    };

    // Extract sourceInfoPool and create deserializer
    let deserializer = if let Some(ast_context_val) = obj.get("astContext") {
        if let Some(ast_context_obj) = ast_context_val.as_object() {
            if let Some(pool_json) = ast_context_obj.get("sourceInfoPool") {
                SourceInfoDeserializer::new(pool_json)?
            } else {
                SourceInfoDeserializer::empty()
            }
        } else {
            SourceInfoDeserializer::empty()
        }
    } else {
        SourceInfoDeserializer::empty()
    };

    // Extract metaTopLevelKeySources if present
    let key_sources = if let Some(ast_context_val) = obj.get("astContext") {
        if let Some(ast_context_obj) = ast_context_val.as_object() {
            if let Some(key_sources_val) = ast_context_obj.get("metaTopLevelKeySources") {
                Some(key_sources_val)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let meta_value = read_meta_with_key_sources(
        obj.get("meta")
            .ok_or_else(|| JsonReadError::MissingField("meta".to_string()))?,
        key_sources,
        &deserializer,
    )?;
    let blocks = read_blocks(
        obj.get("blocks")
            .ok_or_else(|| JsonReadError::MissingField("blocks".to_string()))?,
        &deserializer,
    )?;

    // Convert MetaValueWithSourceInfo to ConfigValue for Pandoc.meta
    let meta = meta_to_config_value(&meta_value);
    Ok((Pandoc { meta, blocks }, context))
}

fn read_blockss(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<Vec<Vec<Block>>> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for blockss".to_string()))?;
    arr.iter()
        .map(|blocks_val| read_blocks(blocks_val, deserializer))
        .collect()
}

fn read_list_attributes(value: &Value) -> Result<ListAttributes> {
    let arr = value.as_array().ok_or_else(|| {
        JsonReadError::InvalidType("Expected array for ListAttributes".to_string())
    })?;

    if arr.len() != 3 {
        return Err(JsonReadError::InvalidType(
            "ListAttributes array must have 3 elements".to_string(),
        ));
    }

    let start_num = arr[0].as_i64().ok_or_else(|| {
        JsonReadError::InvalidType("ListAttributes start number must be integer".to_string())
    })? as usize;

    let number_style_obj = arr[1].as_object().ok_or_else(|| {
        JsonReadError::InvalidType("ListAttributes number style must be object".to_string())
    })?;
    let number_style_t = number_style_obj
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonReadError::MissingField("t in number style".to_string()))?;
    let number_style = match number_style_t {
        "Decimal" => ListNumberStyle::Decimal,
        "LowerAlpha" => ListNumberStyle::LowerAlpha,
        "UpperAlpha" => ListNumberStyle::UpperAlpha,
        "LowerRoman" => ListNumberStyle::LowerRoman,
        "UpperRoman" => ListNumberStyle::UpperRoman,
        "DefaultStyle" => ListNumberStyle::Default,
        _ => {
            return Err(JsonReadError::UnsupportedVariant(format!(
                "ListNumberStyle: {}",
                number_style_t
            )));
        }
    };

    let number_delimiter_obj = arr[2].as_object().ok_or_else(|| {
        JsonReadError::InvalidType("ListAttributes number delimiter must be object".to_string())
    })?;
    let number_delimiter_t = number_delimiter_obj
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonReadError::MissingField("t in number delimiter".to_string()))?;
    let number_delimiter = match number_delimiter_t {
        "Period" => ListNumberDelim::Period,
        "OneParen" => ListNumberDelim::OneParen,
        "TwoParens" => ListNumberDelim::TwoParens,
        "DefaultDelim" => ListNumberDelim::Default,
        _ => {
            return Err(JsonReadError::UnsupportedVariant(format!(
                "ListNumberDelim: {}",
                number_delimiter_t
            )));
        }
    };

    Ok((start_num, number_style, number_delimiter))
}

// Read caption from Pandoc array format: [short, long]
// Source info is passed separately from parallel fields
fn read_caption(
    value: &Value,
    deserializer: &SourceInfoDeserializer,
    source_val: Option<&Value>,
) -> Result<Caption> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for Caption".to_string()))?;

    if arr.len() != 2 {
        return Err(JsonReadError::InvalidType(
            "Caption array must have 2 elements".to_string(),
        ));
    }

    let short = if arr[0].is_null() {
        None
    } else {
        Some(read_inlines(&arr[0], deserializer)?)
    };

    let long = if arr[1].is_null() {
        None
    } else {
        Some(read_blocks(&arr[1], deserializer)?)
    };

    // Read source info from parallel source value if provided
    let source_info = if let Some(s_val) = source_val {
        if s_val.is_number() {
            deserializer.from_json_ref(s_val)?
        } else {
            quarto_source_map::SourceInfo::default()
        }
    } else {
        quarto_source_map::SourceInfo::default()
    };

    Ok(Caption {
        short,
        long,
        source_info,
    })
}

fn read_blocks(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<Vec<Block>> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for blocks".to_string()))?;
    arr.iter().map(|v| read_block(v, deserializer)).collect()
}

fn read_alignment(value: &Value) -> Result<Alignment> {
    let obj = value
        .as_object()
        .ok_or_else(|| JsonReadError::InvalidType("Expected object for Alignment".to_string()))?;
    let t = obj
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;

    match t {
        "AlignLeft" => Ok(Alignment::Left),
        "AlignCenter" => Ok(Alignment::Center),
        "AlignRight" => Ok(Alignment::Right),
        "AlignDefault" => Ok(Alignment::Default),
        _ => Err(JsonReadError::UnsupportedVariant(format!(
            "Alignment: {}",
            t
        ))),
    }
}

fn read_colwidth(value: &Value) -> Result<ColWidth> {
    let obj = value
        .as_object()
        .ok_or_else(|| JsonReadError::InvalidType("Expected object for ColWidth".to_string()))?;
    let t = obj
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;

    match t {
        "ColWidthDefault" => Ok(ColWidth::Default),
        "ColWidth" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let percentage = c.as_f64().ok_or_else(|| {
                JsonReadError::InvalidType("ColWidth percentage must be number".to_string())
            })?;
            Ok(ColWidth::Percentage(percentage))
        }
        _ => Err(JsonReadError::UnsupportedVariant(format!(
            "ColWidth: {}",
            t
        ))),
    }
}

fn read_colspec(value: &Value) -> Result<ColSpec> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for ColSpec".to_string()))?;

    if arr.len() != 2 {
        return Err(JsonReadError::InvalidType(
            "ColSpec array must have 2 elements".to_string(),
        ));
    }

    let alignment = read_alignment(&arr[0])?;
    let colwidth = read_colwidth(&arr[1])?;
    Ok((alignment, colwidth))
}

// Read cell from Pandoc array format: [attr, alignment, rowSpan, colSpan, content]
// Source info is passed separately from parallel fields
fn read_cell(
    value: &Value,
    deserializer: &SourceInfoDeserializer,
    source_val: Option<&Value>,
) -> Result<Cell> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for Cell".to_string()))?;

    if arr.len() != 5 {
        return Err(JsonReadError::InvalidType(
            "Cell array must have 5 elements".to_string(),
        ));
    }

    let attr = read_attr(&arr[0])?;
    let alignment = read_alignment(&arr[1])?;
    let row_span = arr[2]
        .as_u64()
        .ok_or_else(|| JsonReadError::InvalidType("Cell rowSpan must be number".to_string()))?
        as usize;
    let col_span = arr[3]
        .as_u64()
        .ok_or_else(|| JsonReadError::InvalidType("Cell colSpan must be number".to_string()))?
        as usize;
    let content = read_blocks(&arr[4], deserializer)?;

    // Read source info from parallel source structure if provided
    let (source_info, attr_source) = if let Some(s_obj) = source_val {
        let source_info = if let Some(s_val) = s_obj.get("s") {
            deserializer.from_json_ref(s_val)?
        } else {
            quarto_source_map::SourceInfo::default()
        };
        let attr_source = read_attr_source(s_obj.get("attrS"), deserializer)?;
        (source_info, attr_source)
    } else {
        (
            quarto_source_map::SourceInfo::default(),
            AttrSourceInfo::empty(),
        )
    };

    Ok(Cell {
        attr,
        alignment,
        row_span,
        col_span,
        content,
        source_info,
        attr_source,
    })
}

// Read row from Pandoc array format: [attr, cells]
// Source info is passed separately from parallel fields
fn read_row(
    value: &Value,
    deserializer: &SourceInfoDeserializer,
    source_val: Option<&Value>,
) -> Result<Row> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for Row".to_string()))?;

    if arr.len() != 2 {
        return Err(JsonReadError::InvalidType(
            "Row array must have 2 elements".to_string(),
        ));
    }

    let attr = read_attr(&arr[0])?;
    let cells_arr = arr[1]
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Row cells must be array".to_string()))?;

    // Read source info from parallel source structure if provided
    let (source_info, attr_source, cells_source) = if let Some(s_obj) = source_val {
        let source_info = if let Some(s_val) = s_obj.get("s") {
            deserializer.from_json_ref(s_val)?
        } else {
            quarto_source_map::SourceInfo::default()
        };
        let attr_source = read_attr_source(s_obj.get("attrS"), deserializer)?;
        let cells_source = s_obj.get("cellsS").and_then(|v| v.as_array());
        (source_info, attr_source, cells_source)
    } else {
        (
            quarto_source_map::SourceInfo::default(),
            AttrSourceInfo::empty(),
            None,
        )
    };

    // Read cells with their source info
    let cells = cells_arr
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let cell_source = cells_source.and_then(|cs| cs.get(i));
            read_cell(v, deserializer, cell_source)
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Row {
        attr,
        cells,
        source_info,
        attr_source,
    })
}

// Read table head from Pandoc array format: [attr, rows]
// Source info is passed separately from parallel fields
fn read_table_head(
    value: &Value,
    deserializer: &SourceInfoDeserializer,
    source_val: Option<&Value>,
) -> Result<TableHead> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for TableHead".to_string()))?;

    if arr.len() != 2 {
        return Err(JsonReadError::InvalidType(
            "TableHead array must have 2 elements".to_string(),
        ));
    }

    let attr = read_attr(&arr[0])?;
    let rows_arr = arr[1]
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("TableHead rows must be array".to_string()))?;

    // Read source info from parallel source structure if provided
    let (source_info, attr_source, rows_source) = if let Some(s_obj) = source_val {
        let source_info = if let Some(s_val) = s_obj.get("s") {
            deserializer.from_json_ref(s_val)?
        } else {
            quarto_source_map::SourceInfo::default()
        };
        let attr_source = read_attr_source(s_obj.get("attrS"), deserializer)?;
        let rows_source = s_obj.get("rowsS").and_then(|v| v.as_array());
        (source_info, attr_source, rows_source)
    } else {
        (
            quarto_source_map::SourceInfo::default(),
            AttrSourceInfo::empty(),
            None,
        )
    };

    // Read rows with their source info
    let rows = rows_arr
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let row_source = rows_source.and_then(|rs| rs.get(i));
            read_row(v, deserializer, row_source)
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(TableHead {
        attr,
        rows,
        source_info,
        attr_source,
    })
}

// Read table body from Pandoc array format: [attr, rowHeadColumns, head, body]
// Source info is passed separately from parallel fields
fn read_table_body(
    value: &Value,
    deserializer: &SourceInfoDeserializer,
    source_val: Option<&Value>,
) -> Result<TableBody> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for TableBody".to_string()))?;

    if arr.len() != 4 {
        return Err(JsonReadError::InvalidType(
            "TableBody array must have 4 elements".to_string(),
        ));
    }

    let attr = read_attr(&arr[0])?;
    let rowhead_columns = arr[1].as_u64().ok_or_else(|| {
        JsonReadError::InvalidType("TableBody rowHeadColumns must be number".to_string())
    })? as usize;
    let head_arr = arr[2]
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("TableBody head must be array".to_string()))?;
    let body_arr = arr[3]
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("TableBody body must be array".to_string()))?;

    // Read source info from parallel source structure if provided
    let (source_info, attr_source, head_source, body_source) = if let Some(s_obj) = source_val {
        let source_info = if let Some(s_val) = s_obj.get("s") {
            deserializer.from_json_ref(s_val)?
        } else {
            quarto_source_map::SourceInfo::default()
        };
        let attr_source = read_attr_source(s_obj.get("attrS"), deserializer)?;
        let head_source = s_obj.get("headS").and_then(|v| v.as_array());
        let body_source = s_obj.get("bodyS").and_then(|v| v.as_array());
        (source_info, attr_source, head_source, body_source)
    } else {
        (
            quarto_source_map::SourceInfo::default(),
            AttrSourceInfo::empty(),
            None,
            None,
        )
    };

    // Read head rows with their source info
    let head = head_arr
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let row_source = head_source.and_then(|hs| hs.get(i));
            read_row(v, deserializer, row_source)
        })
        .collect::<Result<Vec<_>>>()?;

    // Read body rows with their source info
    let body = body_arr
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let row_source = body_source.and_then(|bs| bs.get(i));
            read_row(v, deserializer, row_source)
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(TableBody {
        attr,
        rowhead_columns,
        head,
        body,
        source_info,
        attr_source,
    })
}

// Read table foot from Pandoc array format: [attr, rows]
// Source info is passed separately from parallel fields
fn read_table_foot(
    value: &Value,
    deserializer: &SourceInfoDeserializer,
    source_val: Option<&Value>,
) -> Result<TableFoot> {
    let arr = value
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Expected array for TableFoot".to_string()))?;

    if arr.len() != 2 {
        return Err(JsonReadError::InvalidType(
            "TableFoot array must have 2 elements".to_string(),
        ));
    }

    let attr = read_attr(&arr[0])?;
    let rows_arr = arr[1]
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("TableFoot rows must be array".to_string()))?;

    // Read source info from parallel source structure if provided
    let (source_info, attr_source, rows_source) = if let Some(s_obj) = source_val {
        let source_info = if let Some(s_val) = s_obj.get("s") {
            deserializer.from_json_ref(s_val)?
        } else {
            quarto_source_map::SourceInfo::default()
        };
        let attr_source = read_attr_source(s_obj.get("attrS"), deserializer)?;
        let rows_source = s_obj.get("rowsS").and_then(|v| v.as_array());
        (source_info, attr_source, rows_source)
    } else {
        (
            quarto_source_map::SourceInfo::default(),
            AttrSourceInfo::empty(),
            None,
        )
    };

    // Read rows with their source info
    let rows = rows_arr
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let row_source = rows_source.and_then(|rs| rs.get(i));
            read_row(v, deserializer, row_source)
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(TableFoot {
        attr,
        rows,
        source_info,
        attr_source,
    })
}

fn read_block(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<Block> {
    let obj = value
        .as_object()
        .ok_or_else(|| JsonReadError::InvalidType("Expected object for Block".to_string()))?;
    let t = obj
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;

    // Extract source information - try new format ("s" field) first, fall back to old format ("l" field)
    let source_info = if let Some(s_val) = obj.get("s") {
        // New format: source info reference to pool
        deserializer.from_json_ref(s_val)?
    } else {
        // Old format: inline location
        let (filename_index, range) = obj
            .get("l")
            .and_then(read_location)
            .unwrap_or_else(|| (None, empty_range()));
        make_source_info(filename_index, range)
    };

    match t {
        "Para" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Block::Paragraph(Paragraph {
                content,
                source_info,
            }))
        }
        "Plain" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Block::Plain(Plain {
                content,
                source_info,
            }))
        }
        "LineBlock" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("LineBlock content must be array".to_string())
            })?;
            let content = arr
                .iter()
                .map(|v| read_inlines(v, deserializer))
                .collect::<Result<Vec<_>>>()?;
            Ok(Block::LineBlock(LineBlock {
                content,
                source_info,
            }))
        }
        "CodeBlock" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("CodeBlock content must be array".to_string())
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "CodeBlock array must have 2 elements".to_string(),
                ));
            }
            let attr = read_attr(&arr[0])?;
            let text = arr[1]
                .as_str()
                .ok_or_else(|| {
                    JsonReadError::InvalidType("CodeBlock text must be string".to_string())
                })?
                .to_string();
            let attr_source = read_attr_source(obj.get("attrS"), deserializer)?;
            Ok(Block::CodeBlock(CodeBlock {
                attr,
                text,
                source_info,
                attr_source,
            }))
        }
        "RawBlock" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("RawBlock content must be array".to_string())
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "RawBlock array must have 2 elements".to_string(),
                ));
            }
            let format = arr[0]
                .as_str()
                .ok_or_else(|| {
                    JsonReadError::InvalidType("RawBlock format must be string".to_string())
                })?
                .to_string();
            let text = arr[1]
                .as_str()
                .ok_or_else(|| {
                    JsonReadError::InvalidType("RawBlock text must be string".to_string())
                })?
                .to_string();
            Ok(Block::RawBlock(RawBlock {
                format,
                text,
                source_info,
            }))
        }
        "BlockQuote" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_blocks(c, deserializer)?;
            Ok(Block::BlockQuote(BlockQuote {
                content,
                source_info,
            }))
        }
        "OrderedList" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("OrderedList content must be array".to_string())
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "OrderedList array must have 2 elements".to_string(),
                ));
            }
            let attr = read_list_attributes(&arr[0])?;
            let content = read_blockss(&arr[1], deserializer)?;
            Ok(Block::OrderedList(OrderedList {
                attr,
                content,
                source_info,
            }))
        }
        "BulletList" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_blockss(c, deserializer)?;
            Ok(Block::BulletList(BulletList {
                content,
                source_info,
            }))
        }
        "DefinitionList" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("DefinitionList content must be array".to_string())
            })?;
            let content = arr
                .iter()
                .map(|item| {
                    let item_arr = item.as_array().ok_or_else(|| {
                        JsonReadError::InvalidType("DefinitionList item must be array".to_string())
                    })?;
                    if item_arr.len() != 2 {
                        return Err(JsonReadError::InvalidType(
                            "DefinitionList item must have 2 elements".to_string(),
                        ));
                    }
                    let term = read_inlines(&item_arr[0], deserializer)?;
                    let definition = read_blockss(&item_arr[1], deserializer)?;
                    Ok((term, definition))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Block::DefinitionList(DefinitionList {
                content,
                source_info,
            }))
        }
        "Header" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Header content must be array".to_string())
            })?;
            if arr.len() != 3 {
                return Err(JsonReadError::InvalidType(
                    "Header array must have 3 elements".to_string(),
                ));
            }
            let level = arr[0].as_u64().ok_or_else(|| {
                JsonReadError::InvalidType("Header level must be number".to_string())
            })? as usize;
            let attr = read_attr(&arr[1])?;
            let content = read_inlines(&arr[2], deserializer)?;
            let attr_source = read_attr_source(obj.get("attrS"), deserializer)?;
            Ok(Block::Header(Header {
                level,
                attr,
                content,
                source_info,
                attr_source,
            }))
        }
        "HorizontalRule" => Ok(Block::HorizontalRule(HorizontalRule { source_info })),
        "Figure" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Figure content must be array".to_string())
            })?;
            if arr.len() != 3 {
                return Err(JsonReadError::InvalidType(
                    "Figure array must have 3 elements".to_string(),
                ));
            }
            let attr = read_attr(&arr[0])?;
            // Figure caption uses old format (object), not parallel source fields yet
            let caption = read_caption(&arr[1], deserializer, None)?;
            let content = read_blocks(&arr[2], deserializer)?;
            let attr_source = read_attr_source(obj.get("attrS"), deserializer)?;
            Ok(Block::Figure(Figure {
                attr,
                caption,
                content,
                source_info,
                attr_source,
            }))
        }
        "Table" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Table content must be array".to_string())
            })?;
            if arr.len() != 6 {
                return Err(JsonReadError::InvalidType(
                    "Table array must have 6 elements".to_string(),
                ));
            }
            let attr = read_attr(&arr[0])?;

            // Read parallel source fields if present
            let caption_source = obj.get("captionS");
            let head_source = obj.get("headS");
            let bodies_source = obj.get("bodiesS").and_then(|v| v.as_array());
            let foot_source = obj.get("footS");

            let caption = read_caption(&arr[1], deserializer, caption_source)?;
            let colspec_arr = arr[2].as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Table colspec must be array".to_string())
            })?;
            let colspec = colspec_arr
                .iter()
                .map(read_colspec)
                .collect::<Result<Vec<_>>>()?;
            let head = read_table_head(&arr[3], deserializer, head_source)?;
            let bodies_arr = arr[4].as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Table bodies must be array".to_string())
            })?;
            let bodies = bodies_arr
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let body_source = bodies_source.and_then(|bs| bs.get(i));
                    read_table_body(v, deserializer, body_source)
                })
                .collect::<Result<Vec<_>>>()?;
            let foot = read_table_foot(&arr[5], deserializer, foot_source)?;
            let attr_source = read_attr_source(obj.get("attrS"), deserializer)?;
            Ok(Block::Table(Table {
                attr,
                caption,
                colspec,
                head,
                bodies,
                foot,
                source_info,
                attr_source,
            }))
        }
        "Div" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Div content must be array".to_string())
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "Div array must have 2 elements".to_string(),
                ));
            }
            let attr = read_attr(&arr[0])?;

            // Check if this is a custom node wrapper
            if attr.1.contains(&"__quarto_custom_node".to_string()) {
                return read_custom_block_from_div(&attr, &arr[1], source_info, deserializer);
            }

            let content = read_blocks(&arr[1], deserializer)?;
            let attr_source = read_attr_source(obj.get("attrS"), deserializer)?;
            Ok(Block::Div(Div {
                attr,
                content,
                source_info,
                attr_source,
            }))
        }
        "BlockMetadata" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            // BlockMetadata uses MetaValueWithSourceInfo format (not top-level meta)
            // Convert to ConfigValue for the new MetaBlock.meta type
            let meta_value = read_meta_value_with_source_info(c, deserializer)?;
            let meta = meta_to_config_value(&meta_value);
            Ok(Block::BlockMetadata(MetaBlock { meta, source_info }))
        }
        "NoteDefinitionPara" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("NoteDefinitionPara content must be array".to_string())
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "NoteDefinitionPara array must have 2 elements".to_string(),
                ));
            }
            let id = arr[0]
                .as_str()
                .ok_or_else(|| {
                    JsonReadError::InvalidType("NoteDefinitionPara id must be string".to_string())
                })?
                .to_string();
            let content = read_inlines(&arr[1], deserializer)?;
            Ok(Block::NoteDefinitionPara(
                crate::pandoc::block::NoteDefinitionPara {
                    id,
                    content,
                    source_info,
                },
            ))
        }
        "NoteDefinitionFencedBlock" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType(
                    "NoteDefinitionFencedBlock content must be array".to_string(),
                )
            })?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType(
                    "NoteDefinitionFencedBlock array must have 2 elements".to_string(),
                ));
            }
            let id = arr[0]
                .as_str()
                .ok_or_else(|| {
                    JsonReadError::InvalidType(
                        "NoteDefinitionFencedBlock id must be string".to_string(),
                    )
                })?
                .to_string();
            let content = read_blocks(&arr[1], deserializer)?;
            Ok(Block::NoteDefinitionFencedBlock(
                crate::pandoc::block::NoteDefinitionFencedBlock {
                    id,
                    content,
                    source_info,
                },
            ))
        }
        _ => Err(JsonReadError::UnsupportedVariant(format!("Block: {}", t))),
    }
}

fn read_meta_with_key_sources(
    value: &Value,
    key_sources: Option<&Value>,
    deserializer: &SourceInfoDeserializer,
) -> Result<MetaValueWithSourceInfo> {
    // meta is an object with key-value pairs (Pandoc-compatible format)
    let obj = value
        .as_object()
        .ok_or_else(|| JsonReadError::InvalidType("Expected object for Meta".to_string()))?;

    let mut entries = Vec::new();
    for (key, val) in obj {
        // Look up key_source from the provided map using deserializer
        let key_source = if let Some(sources) = key_sources {
            if let Some(sources_obj) = sources.as_object() {
                if let Some(source_val) = sources_obj.get(key) {
                    deserializer.from_json_ref(source_val)?
                } else {
                    // Legitimate default: JSON doesn't have source info for this key (backward compat)
                    quarto_source_map::SourceInfo::default()
                }
            } else {
                // Legitimate default: JSON key_sources is not an object
                quarto_source_map::SourceInfo::default()
            }
        } else {
            // Legitimate default: No key_sources in JSON (backward compatibility)
            quarto_source_map::SourceInfo::default()
        };

        entries.push(MetaMapEntry {
            key: key.clone(),
            key_source,
            value: read_meta_value_with_source_info(val, deserializer)?,
        });
    }

    Ok(MetaValueWithSourceInfo::MetaMap {
        entries,
        // Legitimate default: MetaMap itself doesn't have source tracking in JSON (only entries do)
        source_info: quarto_source_map::SourceInfo::default(),
    })
}

fn read_meta_value_with_source_info(
    value: &Value,
    deserializer: &SourceInfoDeserializer,
) -> Result<MetaValueWithSourceInfo> {
    let obj = value
        .as_object()
        .ok_or_else(|| JsonReadError::InvalidType("Expected object for MetaValue".to_string()))?;
    let t = obj
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;

    // Read source_info using deserializer (new format), or use default (old format for backwards compatibility)
    let source_info = if let Some(s) = obj.get("s") {
        deserializer.from_json_ref(s)?
    } else {
        // Legitimate default: Old JSON format doesn't have "s" field (backward compatibility)
        quarto_source_map::SourceInfo::default()
    };

    match t {
        "MetaString" => {
            let c = obj.get("c").and_then(|v| v.as_str()).ok_or_else(|| {
                JsonReadError::InvalidType("MetaString content must be string".to_string())
            })?;
            Ok(MetaValueWithSourceInfo::MetaString {
                value: c.to_string(),
                source_info,
            })
        }
        "MetaInlines" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let inlines = read_inlines(c, deserializer)?;
            Ok(MetaValueWithSourceInfo::MetaInlines {
                content: inlines,
                source_info,
            })
        }
        "MetaBlocks" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let blocks = read_blocks(c, deserializer)?;
            Ok(MetaValueWithSourceInfo::MetaBlocks {
                content: blocks,
                source_info,
            })
        }
        "MetaBool" => {
            let c = obj.get("c").and_then(|v| v.as_bool()).ok_or_else(|| {
                JsonReadError::InvalidType("MetaBool content must be boolean".to_string())
            })?;
            Ok(MetaValueWithSourceInfo::MetaBool {
                value: c,
                source_info,
            })
        }
        "MetaList" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("MetaList content must be array".to_string())
            })?;
            let list = arr
                .iter()
                .map(|v| read_meta_value_with_source_info(v, deserializer))
                .collect::<Result<Vec<_>>>()?;
            Ok(MetaValueWithSourceInfo::MetaList {
                items: list,
                source_info,
            })
        }
        "MetaMap" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("MetaMap content must be array".to_string())
            })?;
            let mut entries = Vec::new();
            for item in arr {
                // Handle both old format (array) and new format (object)
                let (key, key_source, value) = if let Some(obj) = item.as_object() {
                    // New format: {"key": "...", "key_source": {...}, "value": {...}}
                    let key = obj
                        .get("key")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            JsonReadError::MissingField("MetaMap entry missing 'key'".to_string())
                        })?
                        .to_string();
                    let key_source = if let Some(ks) = obj.get("key_source") {
                        deserializer.from_json_ref(ks)?
                    } else {
                        // Legitimate default: JSON entry doesn't have key_source (backward compat)
                        quarto_source_map::SourceInfo::default()
                    };
                    let value = read_meta_value_with_source_info(
                        obj.get("value").ok_or_else(|| {
                            JsonReadError::MissingField("MetaMap entry missing 'value'".to_string())
                        })?,
                        deserializer,
                    )?;
                    (key, key_source, value)
                } else if let Some(kv_arr) = item.as_array() {
                    // Old format: ["key", {...}]
                    if kv_arr.len() != 2 {
                        return Err(JsonReadError::InvalidType(
                            "MetaMap item must have 2 elements".to_string(),
                        ));
                    }
                    let key = kv_arr[0]
                        .as_str()
                        .ok_or_else(|| {
                            JsonReadError::InvalidType("MetaMap key must be string".to_string())
                        })?
                        .to_string();
                    let value = read_meta_value_with_source_info(&kv_arr[1], deserializer)?;
                    // Legitimate default: Old JSON format [key, value] doesn't have key_source
                    (key, quarto_source_map::SourceInfo::default(), value)
                } else {
                    return Err(JsonReadError::InvalidType(
                        "MetaMap item must be array or object".to_string(),
                    ));
                };

                entries.push(MetaMapEntry {
                    key,
                    key_source,
                    value,
                });
            }
            Ok(MetaValueWithSourceInfo::MetaMap {
                entries,
                source_info,
            })
        }
        _ => Err(JsonReadError::UnsupportedVariant(format!(
            "MetaValue: {}",
            t
        ))),
    }
}

/// Read a CustomNode from a wrapper Div with __quarto_custom_node class.
///
/// The Div has:
/// - Class `__quarto_custom_node` (already confirmed by caller)
/// - Attribute `data-custom-type`: the type_name
/// - Attribute `data-custom-slots`: JSON mapping slot names to types
/// - Attribute `data-custom-data`: JSON-serialized plain_data (optional)
/// - Content: slot contents in order, each wrapped in a Div with `data-slot-name`
fn read_custom_block_from_div(
    wrapper_attr: &Attr,
    content: &Value,
    source_info: quarto_source_map::SourceInfo,
    deserializer: &SourceInfoDeserializer,
) -> Result<Block> {
    // Extract custom node metadata from attributes
    let type_name = wrapper_attr
        .2
        .get("data-custom-type")
        .cloned()
        .unwrap_or_else(|| "Unknown".to_string());

    let slot_types_json = wrapper_attr
        .2
        .get("data-custom-slots")
        .cloned()
        .unwrap_or_else(|| "{}".to_string());

    let slot_types: serde_json::Map<String, Value> =
        serde_json::from_str(&slot_types_json).unwrap_or_default();

    let plain_data = if let Some(data_str) = wrapper_attr.2.get("data-custom-data") {
        serde_json::from_str(data_str).unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    // Build the attr without the custom node class
    let classes: Vec<String> = wrapper_attr
        .1
        .iter()
        .filter(|c| *c != "__quarto_custom_node")
        .cloned()
        .collect();

    // Remove the custom node attributes from the attr
    let mut filtered_kvs: LinkedHashMap<String, String> = wrapper_attr.2.clone();
    filtered_kvs.remove("data-custom-type");
    filtered_kvs.remove("data-custom-slots");
    filtered_kvs.remove("data-custom-data");

    let attr = (wrapper_attr.0.clone(), classes, filtered_kvs);

    // Parse slot contents from the wrapper Div children
    let content_arr = content.as_array().ok_or_else(|| {
        JsonReadError::InvalidType("Custom node content must be array".to_string())
    })?;

    let mut slots: LinkedHashMap<String, Slot> = LinkedHashMap::new();

    for slot_wrapper in content_arr {
        // Each slot is a Div with data-slot-name attribute
        let slot_obj = slot_wrapper
            .as_object()
            .ok_or_else(|| JsonReadError::InvalidType("Slot wrapper must be object".to_string()))?;

        let slot_t = slot_obj.get("t").and_then(|v| v.as_str());
        if slot_t != Some("Div") {
            continue; // Skip non-Div elements
        }

        let slot_c = slot_obj.get("c").and_then(|v| v.as_array());
        if slot_c.is_none() || slot_c.unwrap().len() != 2 {
            continue;
        }
        let slot_c = slot_c.unwrap();

        let slot_attr = read_attr(&slot_c[0])?;
        let slot_name = match slot_attr.2.get("data-slot-name") {
            Some(name) => name.clone(),
            None => continue, // Skip if no slot name
        };

        // Get the slot type from the metadata
        let slot_type = slot_types
            .get(&slot_name)
            .and_then(|v| v.as_str())
            .unwrap_or("Blocks");

        let slot_content = &slot_c[1];
        let slot = match slot_type {
            "Block" => {
                // Single block - take the first element
                let blocks = read_blocks(slot_content, deserializer)?;
                if let Some(block) = blocks.into_iter().next() {
                    Slot::Block(Box::new(block))
                } else {
                    Slot::Blocks(vec![])
                }
            }
            "Inline" => {
                // Single inline - expect a Plain block containing the inline
                let blocks = read_blocks(slot_content, deserializer)?;
                if let Some(Block::Plain(plain)) = blocks.into_iter().next() {
                    if let Some(inline) = plain.content.into_iter().next() {
                        Slot::Inline(Box::new(inline))
                    } else {
                        Slot::Inlines(vec![])
                    }
                } else {
                    Slot::Inlines(vec![])
                }
            }
            "Blocks" => {
                let blocks = read_blocks(slot_content, deserializer)?;
                Slot::Blocks(blocks)
            }
            "Inlines" => {
                // Inlines wrapped in a Plain block
                let blocks = read_blocks(slot_content, deserializer)?;
                if let Some(Block::Plain(plain)) = blocks.into_iter().next() {
                    Slot::Inlines(plain.content)
                } else {
                    Slot::Inlines(vec![])
                }
            }
            _ => {
                let blocks = read_blocks(slot_content, deserializer)?;
                Slot::Blocks(blocks)
            }
        };

        slots.insert(slot_name, slot);
    }

    Ok(Block::Custom(CustomNode {
        type_name,
        slots,
        plain_data,
        attr,
        source_info,
    }))
}

/// Read a CustomNode from a wrapper Span with __quarto_custom_node class.
///
/// Similar to read_custom_block_from_div but for inline custom nodes.
fn read_custom_inline_from_span(
    wrapper_attr: &Attr,
    content: &Value,
    source_info: quarto_source_map::SourceInfo,
    deserializer: &SourceInfoDeserializer,
) -> Result<Inline> {
    // Extract custom node metadata from attributes
    let type_name = wrapper_attr
        .2
        .get("data-custom-type")
        .cloned()
        .unwrap_or_else(|| "Unknown".to_string());

    let slot_types_json = wrapper_attr
        .2
        .get("data-custom-slots")
        .cloned()
        .unwrap_or_else(|| "{}".to_string());

    let slot_types: serde_json::Map<String, Value> =
        serde_json::from_str(&slot_types_json).unwrap_or_default();

    let plain_data = if let Some(data_str) = wrapper_attr.2.get("data-custom-data") {
        serde_json::from_str(data_str).unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    // Build the attr without the custom node class
    let classes: Vec<String> = wrapper_attr
        .1
        .iter()
        .filter(|c| *c != "__quarto_custom_node")
        .cloned()
        .collect();

    // Remove the custom node attributes from the attr
    let mut filtered_kvs: LinkedHashMap<String, String> = wrapper_attr.2.clone();
    filtered_kvs.remove("data-custom-type");
    filtered_kvs.remove("data-custom-slots");
    filtered_kvs.remove("data-custom-data");

    let attr = (wrapper_attr.0.clone(), classes, filtered_kvs);

    // Parse slot contents from the wrapper Span children
    let content_arr = content.as_array().ok_or_else(|| {
        JsonReadError::InvalidType("Custom inline content must be array".to_string())
    })?;

    let mut slots: LinkedHashMap<String, Slot> = LinkedHashMap::new();

    for slot_wrapper in content_arr {
        // Each slot is a Span with data-slot-name attribute
        let slot_obj = slot_wrapper
            .as_object()
            .ok_or_else(|| JsonReadError::InvalidType("Slot wrapper must be object".to_string()))?;

        let slot_t = slot_obj.get("t").and_then(|v| v.as_str());
        if slot_t != Some("Span") {
            continue; // Skip non-Span elements
        }

        let slot_c = slot_obj.get("c").and_then(|v| v.as_array());
        if slot_c.is_none() || slot_c.unwrap().len() != 2 {
            continue;
        }
        let slot_c = slot_c.unwrap();

        let slot_attr = read_attr(&slot_c[0])?;
        let slot_name = match slot_attr.2.get("data-slot-name") {
            Some(name) => name.clone(),
            None => continue, // Skip if no slot name
        };

        // Get the slot type from the metadata
        let slot_type = slot_types
            .get(&slot_name)
            .and_then(|v| v.as_str())
            .unwrap_or("Inlines");

        let slot_content = &slot_c[1];
        let slot = match slot_type {
            "Inline" => {
                // Single inline
                let inlines = read_inlines(slot_content, deserializer)?;
                if let Some(inline) = inlines.into_iter().next() {
                    Slot::Inline(Box::new(inline))
                } else {
                    Slot::Inlines(vec![])
                }
            }
            "Inlines" => {
                let inlines = read_inlines(slot_content, deserializer)?;
                Slot::Inlines(inlines)
            }
            _ => {
                // Block slots in inline custom nodes - just read as inlines
                let inlines = read_inlines(slot_content, deserializer)?;
                Slot::Inlines(inlines)
            }
        };

        slots.insert(slot_name, slot);
    }

    Ok(Inline::Custom(CustomNode {
        type_name,
        slots,
        plain_data,
        attr,
        source_info,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{FileId, SourceInfo};
    use serde_json::json;

    #[test]
    fn test_deserialize_source_info_pool_basic() {
        // Test deserializing a simple pool with an Original source
        let pool_json = json!([
            {
                "r": [0, 10],
                "t": 0,
                "d": 0  // file_id
            }
        ]);

        let deserializer = SourceInfoDeserializer::new(&pool_json).unwrap();
        assert_eq!(deserializer.pool.len(), 1);

        // Verify the reconstructed SourceInfo
        let source_info = &deserializer.pool[0];
        match source_info {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => {
                assert_eq!(*file_id, FileId(0));
                assert_eq!(*start_offset, 0);
                assert_eq!(*end_offset, 10);
            }
            _ => panic!("Expected Original variant"),
        }
    }

    #[test]
    fn test_deserialize_source_info_pool_with_refs() {
        // Test deserializing a pool with parent-child relationships
        let pool_json = json!([
            {
                "r": [0, 100],
                "t": 0,
                "d": 0  // Original with file_id 0
            },
            {
                "r": [10, 20],
                "t": 1,
                "d": 0  // Substring with parent_id 0
            },
            {
                "r": [30, 40],
                "t": 1,
                "d": 0  // Another Substring with parent_id 0
            }
        ]);

        let deserializer = SourceInfoDeserializer::new(&pool_json).unwrap();
        assert_eq!(deserializer.pool.len(), 3);

        // Verify parent (Original)
        match &deserializer.pool[0] {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => {
                assert_eq!(*file_id, FileId(0));
                assert_eq!(*start_offset, 0);
                assert_eq!(*end_offset, 100);
            }
            _ => panic!("Expected Original variant"),
        }

        // Verify first child (Substring)
        match &deserializer.pool[1] {
            SourceInfo::Substring {
                parent,
                start_offset,
                end_offset,
            } => {
                assert_eq!(*start_offset, 10);
                assert_eq!(*end_offset, 20);
                // Verify parent points to the Original
                match &**parent {
                    SourceInfo::Original { file_id, .. } => {
                        assert_eq!(*file_id, FileId(0));
                    }
                    _ => panic!("Expected Original parent"),
                }
            }
            _ => panic!("Expected Substring variant"),
        }

        // Test from_json_ref
        let ref_json = json!(1);
        let resolved = deserializer.from_json_ref(&ref_json).unwrap();
        match resolved {
            SourceInfo::Substring {
                start_offset,
                end_offset,
                ..
            } => {
                assert_eq!(start_offset, 10);
                assert_eq!(end_offset, 20);
            }
            _ => panic!("Expected Substring variant"),
        }
    }

    #[test]
    fn test_deserialize_invalid_ref() {
        // Test error handling for invalid reference ID
        let pool_json = json!([
            {
                "r": [0, 100],
                "t": 0,
                "d": 0
            }
        ]);

        let deserializer = SourceInfoDeserializer::new(&pool_json).unwrap();

        // Try to resolve a reference to non-existent ID
        let ref_json = json!(99);
        let result = deserializer.from_json_ref(&ref_json);

        assert!(result.is_err());
        match result.unwrap_err() {
            JsonReadError::InvalidSourceInfoRef(id) => {
                assert_eq!(id, 99);
            }
            _ => panic!("Expected InvalidSourceInfoRef error"),
        }
    }

    #[test]
    fn test_deserialize_circular_ref() {
        // Test error handling for circular/forward references
        let pool_json = json!([
            {
                "r": [0, 100],
                "t": 0,
                "d": 0
            },
            {
                "r": [10, 20],
                "t": 1,
                "d": 2  // Forward reference to ID 2 (doesn't exist yet)
            }
        ]);

        let result = SourceInfoDeserializer::new(&pool_json);

        assert!(result.is_err());
        match result.unwrap_err() {
            JsonReadError::CircularSourceInfoReference(id) => {
                assert_eq!(id, 2);
            }
            _ => panic!("Expected CircularSourceInfoReference error"),
        }
    }
}
