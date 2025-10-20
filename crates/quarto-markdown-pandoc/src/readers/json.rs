/*
 * json.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::block::MetaBlock;
use crate::pandoc::location::{Location, Range, SourceInfo};
use crate::pandoc::meta::MetaMapEntry;
use crate::pandoc::table::{
    Alignment, Cell, ColSpec, ColWidth, Row, Table, TableBody, TableFoot, TableHead,
};
use crate::pandoc::{
    Attr, Block, BlockQuote, BulletList, Caption, Citation, CitationMode, Cite, Code, CodeBlock,
    DefinitionList, Div, Emph, Figure, Header, HorizontalRule, Image, Inline, Inlines, LineBlock,
    Link, ListAttributes, ListNumberDelim, ListNumberStyle, Math, MathType,
    MetaValueWithSourceInfo, Note, OrderedList, Pandoc, Paragraph, Plain, QuoteType, Quoted,
    RawBlock, RawInline, SmallCaps, SoftBreak, Space, Span, Str, Strikeout, Strong, Subscript,
    Superscript, Underline,
};
use quarto_source_map::{FileId, RangeMapping, SourceMapping};
use serde_json::Value;
use std::rc::Rc;

#[derive(Debug)]
pub enum JsonReadError {
    InvalidJson(serde_json::Error),
    MissingField(String),
    InvalidType(String),
    UnsupportedVariant(String),
    InvalidSourceInfoRef(usize),
    ExpectedSourceInfoRef,
    MalformedSourceInfoPool,
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
struct SourceInfoDeserializer {
    pool: Vec<quarto_source_map::SourceInfo>,
}

impl SourceInfoDeserializer {
    /// Create a new empty deserializer (for documents without SourceInfo)
    fn empty() -> Self {
        SourceInfoDeserializer { pool: Vec::new() }
    }

    /// Build the pool from the sourceInfoPool JSON array
    fn new(pool_json: &Value) -> Result<Self> {
        let pool_array = pool_json
            .as_array()
            .ok_or(JsonReadError::MalformedSourceInfoPool)?;

        let mut pool: Vec<quarto_source_map::SourceInfo> = Vec::with_capacity(pool_array.len());

        // Build pool in order - parents must come before children
        for item in pool_array {
            let id = item
                .get("id")
                .and_then(|v| v.as_u64())
                .ok_or(JsonReadError::MalformedSourceInfoPool)? as usize;

            // Verify IDs are sequential
            if id != pool.len() {
                return Err(JsonReadError::MalformedSourceInfoPool);
            }

            let range: quarto_source_map::Range = serde_json::from_value(
                item.get("range")
                    .ok_or(JsonReadError::MalformedSourceInfoPool)?
                    .clone(),
            )
            .map_err(JsonReadError::InvalidJson)?;

            let mapping_obj = item
                .get("mapping")
                .and_then(|v| v.as_object())
                .ok_or(JsonReadError::MalformedSourceInfoPool)?;

            // Get the variant type from "t" field and content from "c" field
            let variant_type = mapping_obj
                .get("t")
                .and_then(|v| v.as_str())
                .ok_or(JsonReadError::MalformedSourceInfoPool)?;

            let content = mapping_obj
                .get("c")
                .ok_or(JsonReadError::MalformedSourceInfoPool)?;

            let mapping = match variant_type {
                "Original" => {
                    let file_id: FileId = serde_json::from_value(
                        content
                            .get("file_id")
                            .ok_or(JsonReadError::MalformedSourceInfoPool)?
                            .clone(),
                    )
                    .map_err(JsonReadError::InvalidJson)?;
                    SourceMapping::Original { file_id }
                }
                "Substring" => {
                    let parent_id = content
                        .get("parent_id")
                        .and_then(|v| v.as_u64())
                        .ok_or(JsonReadError::MalformedSourceInfoPool)? as usize;
                    let offset = content
                        .get("offset")
                        .and_then(|v| v.as_u64())
                        .ok_or(JsonReadError::MalformedSourceInfoPool)? as usize;

                    // Parent must already be in pool
                    let parent = pool
                        .get(parent_id)
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?
                        .clone();

                    SourceMapping::Substring {
                        parent: Rc::new(parent),
                        offset,
                    }
                }
                "Transformed" => {
                    let parent_id = content
                        .get("parent_id")
                        .and_then(|v| v.as_u64())
                        .ok_or(JsonReadError::MalformedSourceInfoPool)? as usize;
                    let mapping_array: Vec<RangeMapping> = serde_json::from_value(
                        content
                            .get("mapping")
                            .ok_or(JsonReadError::MalformedSourceInfoPool)?
                            .clone(),
                    )
                    .map_err(JsonReadError::InvalidJson)?;

                    let parent = pool
                        .get(parent_id)
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?
                        .clone();

                    SourceMapping::Transformed {
                        parent: Rc::new(parent),
                        mapping: mapping_array,
                    }
                }
                "Concat" => {
                    let pieces_array = content
                        .get("pieces")
                        .and_then(|v| v.as_array())
                        .ok_or(JsonReadError::MalformedSourceInfoPool)?;

                    let pieces: Result<Vec<quarto_source_map::SourcePiece>> = pieces_array
                        .iter()
                        .map(|piece| {
                            let source_info_id = piece
                                .get("source_info_id")
                                .and_then(|v| v.as_u64())
                                .ok_or(JsonReadError::MalformedSourceInfoPool)? as usize;
                            let offset_in_concat = piece
                                .get("offset_in_concat")
                                .and_then(|v| v.as_u64())
                                .ok_or(JsonReadError::MalformedSourceInfoPool)? as usize;
                            let length = piece
                                .get("length")
                                .and_then(|v| v.as_u64())
                                .ok_or(JsonReadError::MalformedSourceInfoPool)? as usize;

                            let source_info = pool
                                .get(source_info_id)
                                .ok_or(JsonReadError::MalformedSourceInfoPool)?
                                .clone();

                            Ok(quarto_source_map::SourcePiece {
                                source_info,
                                offset_in_concat,
                                length,
                            })
                        })
                        .collect();

                    SourceMapping::Concat {
                        pieces: pieces?,
                    }
                }
                _ => {
                    return Err(JsonReadError::MalformedSourceInfoPool);
                }
            };

            pool.push(quarto_source_map::SourceInfo { range, mapping });
        }

        Ok(SourceInfoDeserializer { pool })
    }

    /// Resolve a $ref reference to a SourceInfo
    fn from_json_ref(&self, value: &Value) -> Result<quarto_source_map::SourceInfo> {
        if let Some(ref_id) = value.get("$ref").and_then(|v| v.as_u64()) {
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
            Ok(Inline::Str(Str {
                text,
                source_info: SourceInfo::new(None, empty_range()),
                source_info_qsm: None,
            }))
        }
        "Space" => {
            let (filename_index, range) = obj
                .get("l")
                .and_then(read_location)
                .unwrap_or_else(|| (None, empty_range()));
            Ok(Inline::Space(Space {
                source_info: SourceInfo::new(filename_index, range),
            }))
        }
        "LineBreak" => {
            let (filename_index, range) = obj
                .get("l")
                .and_then(read_location)
                .unwrap_or_else(|| (None, empty_range()));
            Ok(Inline::LineBreak(crate::pandoc::inline::LineBreak {
                source_info: SourceInfo::new(filename_index, range),
            }))
        }
        "SoftBreak" => {
            let (filename_index, range) = obj
                .get("l")
                .and_then(read_location)
                .unwrap_or_else(|| (None, empty_range()));
            Ok(Inline::SoftBreak(SoftBreak {
                source_info: SourceInfo::new(filename_index, range),
            }))
        }
        "Emph" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Emph(Emph {
                content,
                source_info: SourceInfo::new(None, empty_range()),
            }))
        }
        "Strong" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Strong(Strong {
                content,
                source_info: SourceInfo::new(None, empty_range()),
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
            Ok(Inline::Code(Code {
                attr,
                text,
                source_info: SourceInfo::new(None, empty_range()),
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
                source_info: SourceInfo::new(None, empty_range()),
            }))
        }
        "Underline" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Underline(Underline {
                content,
                source_info: SourceInfo::new(None, empty_range()),
            }))
        }
        "Strikeout" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Strikeout(Strikeout {
                content,
                source_info: SourceInfo::new(None, empty_range()),
            }))
        }
        "Superscript" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Superscript(Superscript {
                content,
                source_info: SourceInfo::new(None, empty_range()),
            }))
        }
        "Subscript" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::Subscript(Subscript {
                content,
                source_info: SourceInfo::new(None, empty_range()),
            }))
        }
        "SmallCaps" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Inline::SmallCaps(SmallCaps {
                content,
                source_info: SourceInfo::new(None, empty_range()),
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
                source_info: SourceInfo::new(None, empty_range()),
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

            Ok(Inline::Link(Link {
                attr,
                content,
                target,
                source_info: SourceInfo::new(None, empty_range()),
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
                source_info: SourceInfo::new(None, empty_range()),
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

            Ok(Inline::Image(Image {
                attr,
                content,
                target,
                source_info: SourceInfo::new(None, empty_range()),
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
            let content = read_inlines(&arr[1], deserializer)?;
            Ok(Inline::Span(Span {
                attr,
                content,
                source_info: SourceInfo::new(None, empty_range()),
            }))
        }
        "Note" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_blocks(c, deserializer)?;
            Ok(Inline::Note(Note {
                content,
                source_info: SourceInfo::new(None, empty_range()),
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

                    let prefix =
                        read_inlines(citation_obj.get("citationPrefix").ok_or_else(|| {
                            JsonReadError::MissingField("citationPrefix".to_string())
                        })?, deserializer)?;
                    let suffix =
                        read_inlines(citation_obj.get("citationSuffix").ok_or_else(|| {
                            JsonReadError::MissingField("citationSuffix".to_string())
                        })?, deserializer)?;

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
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            // Second element is the content inlines
            let content = read_inlines(&c_arr[1], deserializer)?;

            Ok(Inline::Cite(Cite {
                citations,
                content,
                source_info: SourceInfo::new(None, empty_range()),
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

    let filenames_val = obj
        .get("filenames")
        .ok_or_else(|| JsonReadError::MissingField("filenames".to_string()))?;

    let filenames_arr = filenames_val
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("filenames must be array".to_string()))?;

    let filenames = filenames_arr
        .iter()
        .map(|v| {
            v.as_str()
                .ok_or_else(|| JsonReadError::InvalidType("filename must be string".to_string()))
                .map(|s| s.to_string())
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ASTContext {
        filenames,
        example_list_counter: std::cell::Cell::new(1),
        source_context: quarto_source_map::SourceContext::new(),
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

    let meta = read_meta_with_key_sources(
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

fn read_caption(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<Caption> {
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

    Ok(Caption { short, long })
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

fn read_cell(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<Cell> {
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
        .ok_or_else(|| JsonReadError::InvalidType("Cell row_span must be number".to_string()))?
        as usize;
    let col_span = arr[3]
        .as_u64()
        .ok_or_else(|| JsonReadError::InvalidType("Cell col_span must be number".to_string()))?
        as usize;
    let content = read_blocks(&arr[4], deserializer)?;

    Ok(Cell {
        attr,
        alignment,
        row_span,
        col_span,
        content,
    })
}

fn read_row(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<Row> {
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
    let cells = cells_arr
        .iter()
        .map(|v| read_cell(v, deserializer))
        .collect::<Result<Vec<_>>>()?;

    Ok(Row { attr, cells })
}

fn read_table_head(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<TableHead> {
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
    let rows = rows_arr.iter().map(|v| read_row(v, deserializer)).collect::<Result<Vec<_>>>()?;

    Ok(TableHead { attr, rows })
}

fn read_table_body(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<TableBody> {
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
        JsonReadError::InvalidType("TableBody rowhead_columns must be number".to_string())
    })? as usize;
    let head_arr = arr[2]
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("TableBody head must be array".to_string()))?;
    let head = head_arr.iter().map(|v| read_row(v, deserializer)).collect::<Result<Vec<_>>>()?;
    let body_arr = arr[3]
        .as_array()
        .ok_or_else(|| JsonReadError::InvalidType("TableBody body must be array".to_string()))?;
    let body = body_arr.iter().map(|v| read_row(v, deserializer)).collect::<Result<Vec<_>>>()?;

    Ok(TableBody {
        attr,
        rowhead_columns,
        head,
        body,
    })
}

fn read_table_foot(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<TableFoot> {
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
    let rows = rows_arr.iter().map(|v| read_row(v, deserializer)).collect::<Result<Vec<_>>>()?;

    Ok(TableFoot { attr, rows })
}

fn read_block(value: &Value, deserializer: &SourceInfoDeserializer) -> Result<Block> {
    let obj = value
        .as_object()
        .ok_or_else(|| JsonReadError::InvalidType("Expected object for Block".to_string()))?;
    let t = obj
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;

    // Extract location information if present
    let (filename_index, range) = obj
        .get("l")
        .and_then(read_location)
        .unwrap_or_else(|| (None, empty_range()));

    match t {
        "Para" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Block::Paragraph(Paragraph {
                content,
                source_info: SourceInfo::new(filename_index, range),
            }))
        }
        "Plain" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c, deserializer)?;
            Ok(Block::Plain(Plain {
                content,
                source_info: SourceInfo::new(filename_index, range),
            }))
        }
        "LineBlock" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| {
                JsonReadError::InvalidType("LineBlock content must be array".to_string())
            })?;
            let content = arr.iter().map(|v| read_inlines(v, deserializer)).collect::<Result<Vec<_>>>()?;
            Ok(Block::LineBlock(LineBlock {
                content,
                source_info: SourceInfo::new(filename_index, range),
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
            Ok(Block::CodeBlock(CodeBlock {
                attr,
                text,
                source_info: SourceInfo::new(filename_index, range),
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
                source_info: SourceInfo::new(filename_index, range),
            }))
        }
        "BlockQuote" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_blocks(c, deserializer)?;
            Ok(Block::BlockQuote(BlockQuote {
                content,
                source_info: SourceInfo::new(filename_index, range),
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
                source_info: SourceInfo::new(filename_index, range),
            }))
        }
        "BulletList" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_blockss(c, deserializer)?;
            Ok(Block::BulletList(BulletList {
                content,
                source_info: SourceInfo::new(filename_index, range),
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
                source_info: SourceInfo::new(filename_index, range),
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
            Ok(Block::Header(Header {
                level,
                attr,
                content,
                source_info: SourceInfo::new(filename_index, range),
            }))
        }
        "HorizontalRule" => Ok(Block::HorizontalRule(HorizontalRule {
            source_info: SourceInfo::new(filename_index, range),
            source_info_qsm: None,
        })),
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
            let caption = read_caption(&arr[1], deserializer)?;
            let content = read_blocks(&arr[2], deserializer)?;
            Ok(Block::Figure(Figure {
                attr,
                caption,
                content,
                source_info: SourceInfo::new(filename_index, range),
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
            let caption = read_caption(&arr[1], deserializer)?;
            let colspec_arr = arr[2].as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Table colspec must be array".to_string())
            })?;
            let colspec = colspec_arr
                .iter()
                .map(read_colspec)
                .collect::<Result<Vec<_>>>()?;
            let head = read_table_head(&arr[3], deserializer)?;
            let bodies_arr = arr[4].as_array().ok_or_else(|| {
                JsonReadError::InvalidType("Table bodies must be array".to_string())
            })?;
            let bodies = bodies_arr
                .iter()
                .map(|v| read_table_body(v, deserializer))
                .collect::<Result<Vec<_>>>()?;
            let foot = read_table_foot(&arr[5], deserializer)?;
            Ok(Block::Table(Table {
                attr,
                caption,
                colspec,
                head,
                bodies,
                foot,
                source_info: SourceInfo::new(filename_index, range),
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
            let content = read_blocks(&arr[1], deserializer)?;
            Ok(Block::Div(Div {
                attr,
                content,
                source_info: SourceInfo::new(filename_index, range),
            }))
        }
        "BlockMetadata" => {
            let c = obj
                .get("c")
                .ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            // BlockMetadata uses MetaValueWithSourceInfo format (not top-level meta)
            let meta = read_meta_value_with_source_info(c, deserializer)?;
            Ok(Block::BlockMetadata(MetaBlock {
                meta,
                source_info: SourceInfo::new(filename_index, range),
            }))
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
                    source_info: SourceInfo::new(filename_index, range),
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
                    source_info: SourceInfo::new(filename_index, range),
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
                    quarto_source_map::SourceInfo::default()
                }
            } else {
                quarto_source_map::SourceInfo::default()
            }
        } else {
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
                        quarto_source_map::SourceInfo::default()
                    };
                    let value =
                        read_meta_value_with_source_info(obj.get("value").ok_or_else(|| {
                            JsonReadError::MissingField("MetaMap entry missing 'value'".to_string())
                        })?, deserializer)?;
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
