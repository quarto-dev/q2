/*
 * json.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{
    Attr, Block, BlockQuote, BulletList, Caption, Citation, CitationMode, Cite, Code, CodeBlock, 
    DefinitionList, Div, Emph, Figure, Header, HorizontalRule, Image, Inline, Inlines, 
    LineBlock, Link, ListAttributes, ListNumberDelim, ListNumberStyle, Math, MathType, 
    Meta, MetaValue, Note, OrderedList, Pandoc, Paragraph, Plain, Quoted, QuoteType, 
    RawBlock, RawInline, SmallCaps, SoftBreak, Space, Span, Str, Strikeout, Strong, 
    Subscript, Superscript, Underline
};
use crate::pandoc::block::MetaBlock;
use crate::pandoc::location::{Range, Location};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug)]
pub enum JsonReadError {
    InvalidJson(serde_json::Error),
    MissingField(String),
    InvalidType(String),
    UnsupportedVariant(String),
}

impl std::fmt::Display for JsonReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonReadError::InvalidJson(e) => write!(f, "Invalid JSON: {}", e),
            JsonReadError::MissingField(field) => write!(f, "Missing required field: {}", field),
            JsonReadError::InvalidType(msg) => write!(f, "Invalid type: {}", msg),
            JsonReadError::UnsupportedVariant(variant) => write!(f, "Unsupported variant: {}", variant),
        }
    }
}

impl std::error::Error for JsonReadError {}

type Result<T> = std::result::Result<T, JsonReadError>;

fn read_location(value: &Value) -> Option<Range> {
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
    
    Some(Range { start, end })
}

fn read_attr(value: &Value) -> Result<Attr> {
    let arr = value.as_array().ok_or_else(|| JsonReadError::InvalidType("Expected array for Attr".to_string()))?;
    
    if arr.len() != 3 {
        return Err(JsonReadError::InvalidType("Attr array must have 3 elements".to_string()));
    }
    
    let id = arr[0].as_str().ok_or_else(|| JsonReadError::InvalidType("Attr id must be string".to_string()))?.to_string();
    
    let classes = arr[1].as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Attr classes must be array".to_string()))?
        .iter()
        .map(|v| v.as_str().ok_or_else(|| JsonReadError::InvalidType("Class must be string".to_string())).map(|s| s.to_string()))
        .collect::<Result<Vec<_>>>()?;
    
    let kvs = arr[2].as_array()
        .ok_or_else(|| JsonReadError::InvalidType("Attr key-values must be array".to_string()))?
        .iter()
        .map(|v| {
            let kv_arr = v.as_array().ok_or_else(|| JsonReadError::InvalidType("Key-value pair must be array".to_string()))?;
            if kv_arr.len() != 2 {
                return Err(JsonReadError::InvalidType("Key-value pair must have 2 elements".to_string()));
            }
            let key = kv_arr[0].as_str().ok_or_else(|| JsonReadError::InvalidType("Key must be string".to_string()))?.to_string();
            let value = kv_arr[1].as_str().ok_or_else(|| JsonReadError::InvalidType("Value must be string".to_string()))?.to_string();
            Ok((key, value))
        })
        .collect::<Result<Vec<_>>>()?;
    
    let kvs_map = kvs.into_iter().collect();
    Ok((id, classes, kvs_map))
}

fn read_citation_mode(value: &Value) -> Result<CitationMode> {
    let obj = value.as_object().ok_or_else(|| JsonReadError::InvalidType("Expected object for CitationMode".to_string()))?;
    let t = obj.get("t").and_then(|v| v.as_str()).ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;
    
    match t {
        "NormalCitation" => Ok(CitationMode::NormalCitation),
        "AuthorInText" => Ok(CitationMode::AuthorInText),
        "SuppressAuthor" => Ok(CitationMode::SuppressAuthor),
        _ => Err(JsonReadError::UnsupportedVariant(format!("CitationMode: {}", t))),
    }
}

fn read_inline(value: &Value) -> Result<Inline> {
    let obj = value.as_object().ok_or_else(|| JsonReadError::InvalidType("Expected object for Inline".to_string()))?;
    let t = obj.get("t").and_then(|v| v.as_str()).ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;
    
    match t {
        "Str" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let text = c.as_str().ok_or_else(|| JsonReadError::InvalidType("Str content must be string".to_string()))?.to_string();
            Ok(Inline::Str(Str { text }))
        }
        "Space" => {
            let location = obj.get("l").and_then(read_location);
            let range = location.unwrap_or_else(|| Range {
                start: Location { offset: 0, row: 0, column: 0 },
                end: Location { offset: 0, row: 0, column: 0 },
            });
            let filename = obj.get("l")
                .and_then(|l| l.as_object())
                .and_then(|lo| lo.get("filename"))
                .and_then(|f| f.as_str())
                .map(|s| s.to_string());
            Ok(Inline::Space(Space { filename, range }))
        }
        "LineBreak" => {
            let location = obj.get("l").and_then(read_location);
            let range = location.unwrap_or_else(|| Range {
                start: Location { offset: 0, row: 0, column: 0 },
                end: Location { offset: 0, row: 0, column: 0 },
            });
            let filename = obj.get("l")
                .and_then(|l| l.as_object())
                .and_then(|lo| lo.get("filename"))
                .and_then(|f| f.as_str())
                .map(|s| s.to_string());
            Ok(Inline::LineBreak(crate::pandoc::inline::LineBreak { filename, range }))
        }
        "SoftBreak" => {
            let location = obj.get("l").and_then(read_location);
            let range = location.unwrap_or_else(|| Range {
                start: Location { offset: 0, row: 0, column: 0 },
                end: Location { offset: 0, row: 0, column: 0 },
            });
            let filename = obj.get("l")
                .and_then(|l| l.as_object())
                .and_then(|lo| lo.get("filename"))
                .and_then(|f| f.as_str())
                .map(|s| s.to_string());
            Ok(Inline::SoftBreak(SoftBreak { filename, range }))
        }
        "Emph" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c)?;
            Ok(Inline::Emph(Emph { content }))
        }
        "Strong" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c)?;
            Ok(Inline::Strong(Strong { content }))
        }
        "Code" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("Code content must be array".to_string()))?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType("Code array must have 2 elements".to_string()));
            }
            let attr = read_attr(&arr[0])?;
            let text = arr[1].as_str().ok_or_else(|| JsonReadError::InvalidType("Code text must be string".to_string()))?.to_string();
            Ok(Inline::Code(Code { attr, text }))
        }
        "Math" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("Math content must be array".to_string()))?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType("Math array must have 2 elements".to_string()));
            }
            
            let math_type_obj = arr[0].as_object().ok_or_else(|| JsonReadError::InvalidType("Math type must be object".to_string()))?;
            let math_type_t = math_type_obj.get("t").and_then(|v| v.as_str()).ok_or_else(|| JsonReadError::MissingField("t in math type".to_string()))?;
            let math_type = match math_type_t {
                "InlineMath" => MathType::InlineMath,
                "DisplayMath" => MathType::DisplayMath,
                _ => return Err(JsonReadError::UnsupportedVariant(format!("MathType: {}", math_type_t))),
            };
            
            let text = arr[1].as_str().ok_or_else(|| JsonReadError::InvalidType("Math text must be string".to_string()))?.to_string();
            Ok(Inline::Math(Math { math_type, text }))
        }
        "Underline" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c)?;
            Ok(Inline::Underline(Underline { content }))
        }
        "Strikeout" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c)?;
            Ok(Inline::Strikeout(Strikeout { content }))
        }
        "Superscript" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c)?;
            Ok(Inline::Superscript(Superscript { content }))
        }
        "Subscript" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c)?;
            Ok(Inline::Subscript(Subscript { content }))
        }
        "SmallCaps" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c)?;
            Ok(Inline::SmallCaps(SmallCaps { content }))
        }
        "Quoted" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("Quoted content must be array".to_string()))?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType("Quoted array must have 2 elements".to_string()));
            }
            
            let quote_type_obj = arr[0].as_object().ok_or_else(|| JsonReadError::InvalidType("Quote type must be object".to_string()))?;
            let quote_type_t = quote_type_obj.get("t").and_then(|v| v.as_str()).ok_or_else(|| JsonReadError::MissingField("t in quote type".to_string()))?;
            let quote_type = match quote_type_t {
                "SingleQuote" => QuoteType::SingleQuote,
                "DoubleQuote" => QuoteType::DoubleQuote,
                _ => return Err(JsonReadError::UnsupportedVariant(format!("QuoteType: {}", quote_type_t))),
            };
            
            let content = read_inlines(&arr[1])?;
            Ok(Inline::Quoted(Quoted { quote_type, content }))
        }
        "Link" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("Link content must be array".to_string()))?;
            if arr.len() != 3 {
                return Err(JsonReadError::InvalidType("Link array must have 3 elements".to_string()));
            }
            
            let attr = read_attr(&arr[0])?;
            let content = read_inlines(&arr[1])?;
            
            let target_arr = arr[2].as_array().ok_or_else(|| JsonReadError::InvalidType("Link target must be array".to_string()))?;
            if target_arr.len() != 2 {
                return Err(JsonReadError::InvalidType("Link target array must have 2 elements".to_string()));
            }
            let url = target_arr[0].as_str().ok_or_else(|| JsonReadError::InvalidType("Link URL must be string".to_string()))?.to_string();
            let title = target_arr[1].as_str().ok_or_else(|| JsonReadError::InvalidType("Link title must be string".to_string()))?.to_string();
            let target = (url, title);
            
            Ok(Inline::Link(Link { attr, content, target }))
        }
        "RawInline" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("RawInline content must be array".to_string()))?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType("RawInline array must have 2 elements".to_string()));
            }
            let format = arr[0].as_str().ok_or_else(|| JsonReadError::InvalidType("RawInline format must be string".to_string()))?.to_string();
            let text = arr[1].as_str().ok_or_else(|| JsonReadError::InvalidType("RawInline text must be string".to_string()))?.to_string();
            Ok(Inline::RawInline(RawInline { format, text }))
        }
        "Image" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("Image content must be array".to_string()))?;
            if arr.len() != 3 {
                return Err(JsonReadError::InvalidType("Image array must have 3 elements".to_string()));
            }
            
            let attr = read_attr(&arr[0])?;
            let content = read_inlines(&arr[1])?;
            
            let target_arr = arr[2].as_array().ok_or_else(|| JsonReadError::InvalidType("Image target must be array".to_string()))?;
            if target_arr.len() != 2 {
                return Err(JsonReadError::InvalidType("Image target array must have 2 elements".to_string()));
            }
            let url = target_arr[0].as_str().ok_or_else(|| JsonReadError::InvalidType("Image URL must be string".to_string()))?.to_string();
            let title = target_arr[1].as_str().ok_or_else(|| JsonReadError::InvalidType("Image title must be string".to_string()))?.to_string();
            let target = (url, title);
            
            Ok(Inline::Image(Image { attr, content, target }))
        }
        "Span" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("Span content must be array".to_string()))?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType("Span array must have 2 elements".to_string()));
            }
            
            let attr = read_attr(&arr[0])?;
            let content = read_inlines(&arr[1])?;
            Ok(Inline::Span(Span { attr, content }))
        }
        "Note" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_blocks(c)?;
            Ok(Inline::Note(Note { content }))
        }
        "Cite" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let citations_arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("Cite content must be array".to_string()))?;
            
            let citations = citations_arr
                .iter()
                .map(|citation_val| {
                    let citation_obj = citation_val.as_object().ok_or_else(|| JsonReadError::InvalidType("Citation must be object".to_string()))?;
                    
                    let id = citation_obj.get("citationId")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| JsonReadError::MissingField("citationId".to_string()))?
                        .to_string();
                    
                    let prefix = read_inlines(citation_obj.get("citationPrefix").ok_or_else(|| JsonReadError::MissingField("citationPrefix".to_string()))?)?;
                    let suffix = read_inlines(citation_obj.get("citationSuffix").ok_or_else(|| JsonReadError::MissingField("citationSuffix".to_string()))?)?;
                    
                    let mode = read_citation_mode(citation_obj.get("citationMode").ok_or_else(|| JsonReadError::MissingField("citationMode".to_string()))?)?;
                    
                    let hash = citation_obj.get("citationHash")
                        .and_then(|v| v.as_i64())
                        .ok_or_else(|| JsonReadError::MissingField("citationHash".to_string()))? as usize;
                    
                    let note_num = citation_obj.get("citationNoteNum")
                        .and_then(|v| v.as_i64())
                        .ok_or_else(|| JsonReadError::MissingField("citationNoteNum".to_string()))? as usize;
                    
                    Ok(Citation { id, prefix, suffix, mode, hash, note_num })
                })
                .collect::<Result<Vec<_>>>()?;
            
            // The JSON writer only outputs citations, but the Cite inline needs content too
            // We'll use empty content for now since the writer doesn't provide it
            let content = vec![];
            
            Ok(Inline::Cite(Cite { citations, content }))
        }
        _ => Err(JsonReadError::UnsupportedVariant(format!("Inline: {}", t))),
    }
}

fn read_inlines(value: &Value) -> Result<Inlines> {
    let arr = value.as_array().ok_or_else(|| JsonReadError::InvalidType("Expected array for Inlines".to_string()))?;
    arr.iter().map(read_inline).collect()
}

pub fn read<R: std::io::Read>(reader: &mut R) -> Result<Pandoc> {
    let mut buffer = String::new();
    reader.read_to_string(&mut buffer).map_err(|e| JsonReadError::InvalidJson(serde_json::Error::io(e)))?;
    let json: Value = serde_json::from_str(&buffer).map_err(JsonReadError::InvalidJson)?;
    read_pandoc(&json)
}

fn read_pandoc(value: &Value) -> Result<Pandoc> {
    let obj = value.as_object().ok_or_else(|| JsonReadError::InvalidType("Expected object for Pandoc".to_string()))?;
    
    // We could validate the API version here if needed
    // let _api_version = obj.get("pandoc-api-version");
    
    let meta = read_meta(obj.get("meta").ok_or_else(|| JsonReadError::MissingField("meta".to_string()))?)?;
    let blocks = read_blocks(obj.get("blocks").ok_or_else(|| JsonReadError::MissingField("blocks".to_string()))?)?;
    
    Ok(Pandoc { meta, blocks })
}

fn read_blockss(value: &Value) -> Result<Vec<Vec<Block>>> {
    let arr = value.as_array().ok_or_else(|| JsonReadError::InvalidType("Expected array for blockss".to_string()))?;
    arr.iter()
        .map(|blocks_val| read_blocks(blocks_val))
        .collect()
}

fn read_list_attributes(value: &Value) -> Result<ListAttributes> {
    let arr = value.as_array().ok_or_else(|| JsonReadError::InvalidType("Expected array for ListAttributes".to_string()))?;
    
    if arr.len() != 3 {
        return Err(JsonReadError::InvalidType("ListAttributes array must have 3 elements".to_string()));
    }
    
    let start_num = arr[0].as_i64().ok_or_else(|| JsonReadError::InvalidType("ListAttributes start number must be integer".to_string()))? as usize;
    
    let number_style_obj = arr[1].as_object().ok_or_else(|| JsonReadError::InvalidType("ListAttributes number style must be object".to_string()))?;
    let number_style_t = number_style_obj.get("t").and_then(|v| v.as_str()).ok_or_else(|| JsonReadError::MissingField("t in number style".to_string()))?;
    let number_style = match number_style_t {
        "Decimal" => ListNumberStyle::Decimal,
        "LowerAlpha" => ListNumberStyle::LowerAlpha,
        "UpperAlpha" => ListNumberStyle::UpperAlpha,
        "LowerRoman" => ListNumberStyle::LowerRoman,
        "UpperRoman" => ListNumberStyle::UpperRoman,
        "DefaultStyle" => ListNumberStyle::Default,
        _ => return Err(JsonReadError::UnsupportedVariant(format!("ListNumberStyle: {}", number_style_t))),
    };
    
    let number_delimiter_obj = arr[2].as_object().ok_or_else(|| JsonReadError::InvalidType("ListAttributes number delimiter must be object".to_string()))?;
    let number_delimiter_t = number_delimiter_obj.get("t").and_then(|v| v.as_str()).ok_or_else(|| JsonReadError::MissingField("t in number delimiter".to_string()))?;
    let number_delimiter = match number_delimiter_t {
        "Period" => ListNumberDelim::Period,
        "OneParen" => ListNumberDelim::OneParen,
        "TwoParens" => ListNumberDelim::TwoParens,
        "DefaultDelim" => ListNumberDelim::Default,
        _ => return Err(JsonReadError::UnsupportedVariant(format!("ListNumberDelim: {}", number_delimiter_t))),
    };
    
    Ok((start_num, number_style, number_delimiter))
}

fn read_caption(value: &Value) -> Result<Caption> {
    let arr = value.as_array().ok_or_else(|| JsonReadError::InvalidType("Expected array for Caption".to_string()))?;
    
    if arr.len() != 2 {
        return Err(JsonReadError::InvalidType("Caption array must have 2 elements".to_string()));
    }
    
    let short = if arr[0].is_null() { 
        None 
    } else { 
        Some(read_inlines(&arr[0])?) 
    };
    
    let long = if arr[1].is_null() { 
        None 
    } else { 
        Some(read_blocks(&arr[1])?) 
    };
    
    Ok(Caption { short, long })
}

fn read_blocks(value: &Value) -> Result<Vec<Block>> {
    let arr = value.as_array().ok_or_else(|| JsonReadError::InvalidType("Expected array for blocks".to_string()))?;
    arr.iter().map(read_block).collect()
}

fn read_block(value: &Value) -> Result<Block> {
    let obj = value.as_object().ok_or_else(|| JsonReadError::InvalidType("Expected object for Block".to_string()))?;
    let t = obj.get("t").and_then(|v| v.as_str()).ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;
    
    // Extract location information if present
    let location = obj.get("l").and_then(read_location);
    let range = location.unwrap_or_else(|| Range {
        start: Location { offset: 0, row: 0, column: 0 },
        end: Location { offset: 0, row: 0, column: 0 },
    });
    let filename = obj.get("l")
        .and_then(|l| l.as_object())
        .and_then(|lo| lo.get("filename"))
        .and_then(|f| f.as_str())
        .map(|s| s.to_string());
    
    match t {
        "Para" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c)?;
            Ok(Block::Paragraph(Paragraph { content, filename, range }))
        }
        "Plain" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_inlines(c)?;
            Ok(Block::Plain(Plain { content, filename, range }))
        }
        "LineBlock" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("LineBlock content must be array".to_string()))?;
            let content = arr.iter().map(read_inlines).collect::<Result<Vec<_>>>()?;
            Ok(Block::LineBlock(LineBlock { content, filename, range }))
        }
        "CodeBlock" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("CodeBlock content must be array".to_string()))?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType("CodeBlock array must have 2 elements".to_string()));
            }
            let attr = read_attr(&arr[0])?;
            let text = arr[1].as_str().ok_or_else(|| JsonReadError::InvalidType("CodeBlock text must be string".to_string()))?.to_string();
            Ok(Block::CodeBlock(CodeBlock { attr, text, filename, range }))
        }
        "RawBlock" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("RawBlock content must be array".to_string()))?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType("RawBlock array must have 2 elements".to_string()));
            }
            let format = arr[0].as_str().ok_or_else(|| JsonReadError::InvalidType("RawBlock format must be string".to_string()))?.to_string();
            let text = arr[1].as_str().ok_or_else(|| JsonReadError::InvalidType("RawBlock text must be string".to_string()))?.to_string();
            Ok(Block::RawBlock(RawBlock { format, text, filename, range }))
        }
        "BlockQuote" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_blocks(c)?;
            Ok(Block::BlockQuote(BlockQuote { content, filename, range }))
        }
        "OrderedList" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("OrderedList content must be array".to_string()))?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType("OrderedList array must have 2 elements".to_string()));
            }
            let attr = read_list_attributes(&arr[0])?;
            let content = read_blockss(&arr[1])?;
            Ok(Block::OrderedList(OrderedList { attr, content, filename, range }))
        }
        "BulletList" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let content = read_blockss(c)?;
            Ok(Block::BulletList(BulletList { content, filename, range }))
        }
        "DefinitionList" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("DefinitionList content must be array".to_string()))?;
            let content = arr
                .iter()
                .map(|item| {
                    let item_arr = item.as_array().ok_or_else(|| JsonReadError::InvalidType("DefinitionList item must be array".to_string()))?;
                    if item_arr.len() != 2 {
                        return Err(JsonReadError::InvalidType("DefinitionList item must have 2 elements".to_string()));
                    }
                    let term = read_inlines(&item_arr[0])?;
                    let definition = read_blockss(&item_arr[1])?;
                    Ok((term, definition))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Block::DefinitionList(DefinitionList { content, filename, range }))
        }
        "Header" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("Header content must be array".to_string()))?;
            if arr.len() != 3 {
                return Err(JsonReadError::InvalidType("Header array must have 3 elements".to_string()));
            }
            let level = arr[0].as_u64().ok_or_else(|| JsonReadError::InvalidType("Header level must be number".to_string()))? as usize;
            let attr = read_attr(&arr[1])?;
            let content = read_inlines(&arr[2])?;
            Ok(Block::Header(Header { level, attr, content, filename, range }))
        }
        "HorizontalRule" => {
            Ok(Block::HorizontalRule(HorizontalRule { filename, range }))
        }
        "Figure" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("Figure content must be array".to_string()))?;
            if arr.len() != 3 {
                return Err(JsonReadError::InvalidType("Figure array must have 3 elements".to_string()));
            }
            let attr = read_attr(&arr[0])?;
            let caption = read_caption(&arr[1])?;
            let content = read_blocks(&arr[2])?;
            Ok(Block::Figure(Figure { attr, caption, content, filename, range }))
        }
        "Div" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("Div content must be array".to_string()))?;
            if arr.len() != 2 {
                return Err(JsonReadError::InvalidType("Div array must have 2 elements".to_string()));
            }
            let attr = read_attr(&arr[0])?;
            let content = read_blocks(&arr[1])?;
            Ok(Block::Div(Div { attr, content, filename, range }))
        }
        "BlockMetadata" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let meta = read_meta(c)?;
            Ok(Block::BlockMetadata(MetaBlock { meta, filename, range }))
        }
        _ => Err(JsonReadError::UnsupportedVariant(format!("Block: {}", t))),
    }
}

fn read_meta(value: &Value) -> Result<Meta> {
    let obj = value.as_object().ok_or_else(|| JsonReadError::InvalidType("Expected object for Meta".to_string()))?;
    
    let mut meta = HashMap::new();
    for (key, val) in obj {
        meta.insert(key.clone(), read_meta_value(val)?);
    }
    
    Ok(meta)
}

fn read_meta_value(value: &Value) -> Result<MetaValue> {
    let obj = value.as_object().ok_or_else(|| JsonReadError::InvalidType("Expected object for MetaValue".to_string()))?;
    let t = obj.get("t").and_then(|v| v.as_str()).ok_or_else(|| JsonReadError::MissingField("t".to_string()))?;
    
    match t {
        "MetaString" => {
            let c = obj.get("c").and_then(|v| v.as_str()).ok_or_else(|| JsonReadError::InvalidType("MetaString content must be string".to_string()))?;
            Ok(MetaValue::MetaString(c.to_string()))
        }
        "MetaInlines" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let inlines = read_inlines(c)?;
            Ok(MetaValue::MetaInlines(inlines))
        }
        "MetaBlocks" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let blocks = read_blocks(c)?;
            Ok(MetaValue::MetaBlocks(blocks))
        }
        "MetaBool" => {
            let c = obj.get("c").and_then(|v| v.as_bool()).ok_or_else(|| JsonReadError::InvalidType("MetaBool content must be boolean".to_string()))?;
            Ok(MetaValue::MetaBool(c))
        }
        "MetaList" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("MetaList content must be array".to_string()))?;
            let list = arr.iter().map(read_meta_value).collect::<Result<Vec<_>>>()?;
            Ok(MetaValue::MetaList(list))
        }
        "MetaMap" => {
            let c = obj.get("c").ok_or_else(|| JsonReadError::MissingField("c".to_string()))?;
            let arr = c.as_array().ok_or_else(|| JsonReadError::InvalidType("MetaMap content must be array".to_string()))?;
            let mut map = HashMap::new();
            for item in arr {
                let kv_arr = item.as_array().ok_or_else(|| JsonReadError::InvalidType("MetaMap item must be array".to_string()))?;
                if kv_arr.len() != 2 {
                    return Err(JsonReadError::InvalidType("MetaMap item must have 2 elements".to_string()));
                }
                let key = kv_arr[0].as_str().ok_or_else(|| JsonReadError::InvalidType("MetaMap key must be string".to_string()))?.to_string();
                let value = read_meta_value(&kv_arr[1])?;
                map.insert(key, value);
            }
            Ok(MetaValue::MetaMap(map))
        }
        _ => Err(JsonReadError::UnsupportedVariant(format!("MetaValue: {}", t))),
    }
}