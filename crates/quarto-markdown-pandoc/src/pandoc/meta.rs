/*
 * meta.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::block::Blocks;
use crate::pandoc::inline::{Inline, Inlines, Span, Str};
use crate::pandoc::location::empty_source_info;
use crate::readers;
use crate::{pandoc::RawBlock, utils::output::VerboseOutput};
use hashlink::LinkedHashMap;
use std::{io, mem};
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};

// Pandoc's MetaValue notably does not support numbers or nulls, so we don't either
// https://pandoc.org/lua-filters.html#type-metavalue
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MetaValue {
    MetaString(String),
    MetaBool(bool),
    MetaInlines(Inlines),
    MetaBlocks(Blocks),
    MetaList(Vec<MetaValue>),
    MetaMap(LinkedHashMap<String, MetaValue>),
}

impl Default for MetaValue {
    fn default() -> Self {
        MetaValue::MetaMap(LinkedHashMap::new())
    }
}

pub type Meta = LinkedHashMap<String, MetaValue>;

// Phase 4: MetaValueWithSourceInfo - Meta with full source tracking
// This replaces Meta for use in PandocAST, preserving source info through
// the YAML->Meta transformation where strings are parsed as Markdown.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MetaValueWithSourceInfo {
    MetaString {
        value: String,
        source_info: quarto_source_map::SourceInfo,
    },
    MetaBool {
        value: bool,
        source_info: quarto_source_map::SourceInfo,
    },
    MetaInlines {
        content: Inlines,
        source_info: quarto_source_map::SourceInfo,
    },
    MetaBlocks {
        content: Blocks,
        source_info: quarto_source_map::SourceInfo,
    },
    MetaList {
        items: Vec<MetaValueWithSourceInfo>,
        source_info: quarto_source_map::SourceInfo,
    },
    MetaMap {
        entries: Vec<MetaMapEntry>,
        source_info: quarto_source_map::SourceInfo,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MetaMapEntry {
    pub key: String,
    pub key_source: quarto_source_map::SourceInfo,
    pub value: MetaValueWithSourceInfo,
}

impl Default for MetaValueWithSourceInfo {
    fn default() -> Self {
        MetaValueWithSourceInfo::MetaMap {
            entries: Vec::new(),
            source_info: quarto_source_map::SourceInfo::default(),
        }
    }
}

impl MetaValueWithSourceInfo {
    /// Get a value by key if this is a MetaMap
    pub fn get(&self, key: &str) -> Option<&MetaValueWithSourceInfo> {
        match self {
            MetaValueWithSourceInfo::MetaMap { entries, .. } => {
                entries.iter().find(|e| e.key == key).map(|e| &e.value)
            }
            _ => None,
        }
    }

    /// Check if a key exists if this is a MetaMap
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Check if this MetaMap is empty
    pub fn is_empty(&self) -> bool {
        match self {
            MetaValueWithSourceInfo::MetaMap { entries, .. } => entries.is_empty(),
            _ => false,
        }
    }

    /// Check if this MetaValue represents a string with a specific value
    ///
    /// This handles both:
    /// - MetaString { value, .. } where value == expected
    /// - MetaInlines { content, .. } where content is a single Str with text == expected
    ///
    /// This is needed because after k-90/k-95, YAML strings are parsed as markdown
    /// and become MetaInlines containing a single Str node.
    pub fn is_string_value(&self, expected: &str) -> bool {
        match self {
            MetaValueWithSourceInfo::MetaString { value, .. } => value == expected,
            MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                // Check if it's a single Str inline with the expected text
                if content.len() == 1 {
                    if let crate::pandoc::Inline::Str(str_node) = &content[0] {
                        return str_node.text == expected;
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Convert to old Meta format (loses source info)
    pub fn to_meta_value(&self) -> MetaValue {
        match self {
            MetaValueWithSourceInfo::MetaString { value, .. } => {
                MetaValue::MetaString(value.clone())
            }
            MetaValueWithSourceInfo::MetaBool { value, .. } => MetaValue::MetaBool(*value),
            MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                MetaValue::MetaInlines(content.clone())
            }
            MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
                MetaValue::MetaBlocks(content.clone())
            }
            MetaValueWithSourceInfo::MetaList { items, .. } => {
                MetaValue::MetaList(items.iter().map(|item| item.to_meta_value()).collect())
            }
            MetaValueWithSourceInfo::MetaMap { entries, .. } => {
                let mut map = LinkedHashMap::new();
                for entry in entries {
                    map.insert(entry.key.clone(), entry.value.to_meta_value());
                }
                MetaValue::MetaMap(map)
            }
        }
    }

    /// Convert to old Meta format when self is a MetaMap (loses source info)
    /// Panics if self is not a MetaMap
    pub fn to_meta(&self) -> Meta {
        match self {
            MetaValueWithSourceInfo::MetaMap { entries, .. } => {
                let mut map = LinkedHashMap::new();
                for entry in entries {
                    map.insert(entry.key.clone(), entry.value.to_meta_value());
                }
                map
            }
            _ => panic!("to_meta() called on non-MetaMap variant"),
        }
    }
}

/// Convert old Meta to new format (with dummy source info)
pub fn meta_from_legacy(meta: Meta) -> MetaValueWithSourceInfo {
    let entries = meta
        .into_iter()
        .map(|(k, v)| MetaMapEntry {
            key: k,
            key_source: quarto_source_map::SourceInfo::default(),
            value: meta_value_from_legacy(v),
        })
        .collect();

    MetaValueWithSourceInfo::MetaMap {
        entries,
        source_info: quarto_source_map::SourceInfo::default(),
    }
}

/// Convert old MetaValue to new format (with dummy source info)
pub fn meta_value_from_legacy(value: MetaValue) -> MetaValueWithSourceInfo {
    match value {
        MetaValue::MetaString(s) => MetaValueWithSourceInfo::MetaString {
            value: s,
            source_info: quarto_source_map::SourceInfo::default(),
        },
        MetaValue::MetaBool(b) => MetaValueWithSourceInfo::MetaBool {
            value: b,
            source_info: quarto_source_map::SourceInfo::default(),
        },
        MetaValue::MetaInlines(inlines) => MetaValueWithSourceInfo::MetaInlines {
            content: inlines,
            source_info: quarto_source_map::SourceInfo::default(),
        },
        MetaValue::MetaBlocks(blocks) => MetaValueWithSourceInfo::MetaBlocks {
            content: blocks,
            source_info: quarto_source_map::SourceInfo::default(),
        },
        MetaValue::MetaList(list) => MetaValueWithSourceInfo::MetaList {
            items: list.into_iter().map(meta_value_from_legacy).collect(),
            source_info: quarto_source_map::SourceInfo::default(),
        },
        MetaValue::MetaMap(map) => {
            let entries = map
                .into_iter()
                .map(|(k, v)| MetaMapEntry {
                    key: k,
                    key_source: quarto_source_map::SourceInfo::default(),
                    value: meta_value_from_legacy(v),
                })
                .collect();
            MetaValueWithSourceInfo::MetaMap {
                entries,
                source_info: quarto_source_map::SourceInfo::default(),
            }
        }
    }
}

/// Parse a YAML string value as markdown
///
/// - If tag_source_info is Some: This is a !md tagged value, ERROR on parse failure
/// - If tag_source_info is None: This is an untagged value, WARN on parse failure
///
/// On success: Returns MetaInlines or MetaBlocks
/// On failure with !md: Returns error (will need to panic or collect diagnostic)
/// On failure untagged: Returns MetaInlines with yaml-markdown-syntax-error Span + warning
fn parse_yaml_string_as_markdown(
    value: &str,
    source_info: &quarto_source_map::SourceInfo,
    _context: &crate::pandoc::ast_context::ASTContext,
    tag_source_info: Option<quarto_source_map::SourceInfo>,
    diagnostics: &mut crate::utils::diagnostic_collector::DiagnosticCollector,
) -> MetaValueWithSourceInfo {
    use quarto_error_reporting::DiagnosticMessageBuilder;

    let mut output_stream = VerboseOutput::Sink(io::sink());
    let result = readers::qmd::read(value.as_bytes(), false, "<metadata>", &mut output_stream);

    match result {
        Ok((mut pandoc, _, warnings)) => {
            // Propagate warnings from recursive parse
            for warning in warnings {
                diagnostics.add(warning);
            }
            // Parse succeeded - return as MetaInlines or MetaBlocks
            if pandoc.blocks.len() == 1 {
                if let crate::pandoc::Block::Paragraph(p) = &mut pandoc.blocks[0] {
                    return MetaValueWithSourceInfo::MetaInlines {
                        content: mem::take(&mut p.content),
                        source_info: source_info.clone(),
                    };
                }
            }
            MetaValueWithSourceInfo::MetaBlocks {
                content: pandoc.blocks,
                source_info: source_info.clone(),
            }
        }
        Err(_parse_errors) => {
            if let Some(_tag_loc) = tag_source_info {
                // !md tag: ERROR on parse failure
                let diagnostic =
                    DiagnosticMessageBuilder::error("Failed to parse !md tagged value")
                        .with_code("Q-1-100")
                        .with_location(source_info.clone())
                        .problem("The `!md` tag requires valid markdown syntax")
                        .add_detail(format!("Could not parse: {}", value))
                        .add_hint("Remove the `!md` tag or fix the markdown syntax")
                        .build();

                // Collect diagnostic instead of printing
                diagnostics.add(diagnostic);

                // For now, also return the error span so we can continue
                // In the future, we might want to actually fail the parse
                let span = Span {
                    attr: (
                        String::new(),
                        vec!["yaml-markdown-syntax-error".to_string()],
                        LinkedHashMap::new(),
                    ),
                    content: vec![Inline::Str(Str {
                        text: value.to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                    attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
                };
                MetaValueWithSourceInfo::MetaInlines {
                    content: vec![Inline::Span(span)],
                    source_info: source_info.clone(),
                }
            } else {
                // Untagged: WARN on parse failure
                let diagnostic = DiagnosticMessageBuilder::warning("Failed to parse metadata value as markdown")
                    .with_code("Q-1-101")
                    .with_location(source_info.clone())
                    .problem(format!("Could not parse '{}' as markdown", value))
                    .add_hint("Add the `!str` tag to treat this as a plain string, or fix the markdown syntax")
                    .build();

                // Collect diagnostic instead of printing
                diagnostics.add(diagnostic);

                // Return span with yaml-markdown-syntax-error class
                let span = Span {
                    attr: (
                        String::new(),
                        vec!["yaml-markdown-syntax-error".to_string()],
                        LinkedHashMap::new(),
                    ),
                    content: vec![Inline::Str(Str {
                        text: value.to_string(),
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    source_info: quarto_source_map::SourceInfo::default(),
                    attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
                };
                MetaValueWithSourceInfo::MetaInlines {
                    content: vec![Inline::Span(span)],
                    source_info: source_info.clone(),
                }
            }
        }
    }
}

/// Transform YamlWithSourceInfo to MetaValueWithSourceInfo
///
/// This is the core transformation that:
/// 1. Parses YAML strings as Markdown (creating Substring SourceInfos)
/// 2. Preserves source tracking through nested structures
/// 3. Handles special YAML tags (bypassing markdown parsing)
/// 4. Converts YAML types to Pandoc Meta types
///
/// Takes ownership of the YamlWithSourceInfo to avoid unnecessary clones.
pub fn yaml_to_meta_with_source_info(
    yaml: quarto_yaml::YamlWithSourceInfo,
    _context: &crate::pandoc::ast_context::ASTContext,
    diagnostics: &mut crate::utils::diagnostic_collector::DiagnosticCollector,
) -> MetaValueWithSourceInfo {
    use yaml_rust2::Yaml;

    // Check if this is an array or hash first, since we need to consume
    // the value before matching on yaml.yaml
    if yaml.is_array() {
        let (items, source_info) = yaml.into_array().unwrap();
        let meta_items = items
            .into_iter()
            .map(|item| yaml_to_meta_with_source_info(item, _context, diagnostics))
            .collect();

        return MetaValueWithSourceInfo::MetaList {
            items: meta_items,
            source_info,
        };
    }

    if yaml.is_hash() {
        let (entries, source_info) = yaml.into_hash().unwrap();
        let meta_entries = entries
            .into_iter()
            .filter_map(|entry| {
                // Keys must be strings in Pandoc metadata
                entry.key.yaml.as_str().map(|key_str| MetaMapEntry {
                    key: key_str.to_string(),
                    key_source: entry.key_span,
                    value: yaml_to_meta_with_source_info(entry.value, _context, diagnostics),
                })
            })
            .collect();

        return MetaValueWithSourceInfo::MetaMap {
            entries: meta_entries,
            source_info,
        };
    }

    // For scalars, destructure to get owned values
    let quarto_yaml::YamlWithSourceInfo {
        yaml: yaml_value,
        source_info,
        tag,
        ..
    } = yaml;

    match yaml_value {
        Yaml::String(s) => {
            // Check for YAML tags (e.g., !path, !glob, !str, !md)
            if let Some((tag_suffix, tag_source_info)) = tag {
                match tag_suffix.as_str() {
                    "str" | "path" => {
                        // !str and !path: Emit plain Str without markdown parsing
                        // No wrapper span, just a plain Str node
                        MetaValueWithSourceInfo::MetaInlines {
                            content: vec![Inline::Str(Str {
                                text: s.clone(),
                                source_info: source_info.clone(),
                            })],
                            source_info,
                        }
                    }
                    "md" => {
                        // !md: Parse as markdown immediately, ERROR if fails
                        parse_yaml_string_as_markdown(
                            &s,
                            &source_info,
                            _context,
                            Some(tag_source_info),
                            diagnostics,
                        )
                    }
                    _ => {
                        // Other tags (!glob, !expr, etc.): Keep current behavior
                        // Wrap in Span with class "yaml-tagged-string" and tag attribute
                        let mut attributes = LinkedHashMap::new();
                        attributes.insert("tag".to_string(), tag_suffix.clone());

                        let span = Span {
                            attr: (
                                String::new(),
                                vec!["yaml-tagged-string".to_string()],
                                attributes,
                            ),
                            content: vec![Inline::Str(Str {
                                text: s.clone(),
                                source_info: source_info.clone(),
                            })],
                            source_info: quarto_source_map::SourceInfo::default(),
                            attr_source: crate::pandoc::attr::AttrSourceInfo {
                                id: None,            // No id
                                classes: vec![None], // "yaml-tagged-string" class has no source tracking
                                attributes: vec![
                                    (None, Some(tag_source_info)), // "tag" key has no source, value points to the tag
                                ],
                            },
                        };
                        MetaValueWithSourceInfo::MetaInlines {
                            content: vec![Inline::Span(span)],
                            source_info, // Overall node source
                        }
                    }
                }
            } else {
                // Untagged string: Parse as markdown immediately, WARN if fails
                parse_yaml_string_as_markdown(&s, &source_info, _context, None, diagnostics)
            }
        }

        Yaml::Boolean(b) => MetaValueWithSourceInfo::MetaBool {
            value: b,
            source_info,
        },

        // Pandoc doesn't support null, numbers, etc. in metadata
        // For now, convert them to strings
        Yaml::Null => MetaValueWithSourceInfo::MetaString {
            value: String::new(),
            source_info,
        },

        Yaml::Integer(i) => MetaValueWithSourceInfo::MetaString {
            value: i.to_string(),
            source_info,
        },

        Yaml::Real(r) => MetaValueWithSourceInfo::MetaString {
            value: r,
            source_info,
        },

        Yaml::BadValue => MetaValueWithSourceInfo::MetaString {
            value: String::new(),
            source_info,
        },

        Yaml::Alias(_) => {
            // YAML aliases are resolved by yaml-rust2, so this shouldn't happen
            // But if it does, treat as empty string
            MetaValueWithSourceInfo::MetaString {
                value: String::new(),
                source_info,
            }
        }

        // Array and Hash should have been handled above
        Yaml::Array(_) | Yaml::Hash(_) => {
            unreachable!("Array/Hash should be handled by into_array/into_hash")
        }
    }
}

fn extract_between_delimiters(input: &str) -> Option<&str> {
    let parts: Vec<&str> = input.split("---").collect();
    if parts.len() >= 3 {
        Some(parts[1].trim())
    } else {
        None
    }
}

enum ContextFrame {
    Map(LinkedHashMap<String, MetaValue>, Option<String>),
    List(Vec<MetaValue>),
    Root,
}

struct YamlEventHandler {
    stack: Vec<ContextFrame>,
    result: Option<Meta>,
}

impl YamlEventHandler {
    fn new() -> Self {
        YamlEventHandler {
            stack: vec![ContextFrame::Root],
            result: None,
        }
    }

    fn push_value(&mut self, value: MetaValue) {
        match self.stack.last_mut() {
            Some(ContextFrame::Map(map, Some(key))) => {
                map.insert(key.clone(), value);
                if let Some(ContextFrame::Map(_, key_slot)) = self.stack.last_mut() {
                    *key_slot = None;
                }
            }
            Some(ContextFrame::Map(_, None)) => {
                panic!("Map expecting key, got value");
            }
            Some(ContextFrame::List(list)) => {
                list.push(value);
            }
            Some(ContextFrame::Root) => {
                if let MetaValue::MetaMap(map) = value {
                    self.result = Some(map);
                }
            }
            None => panic!("Empty stack"),
        }
    }

    fn parse_scalar(&self, s: &str, tag: Option<yaml_rust2::parser::Tag>) -> MetaValue {
        // Check if this scalar has a YAML tag (like !path, !glob, !str)
        if let Some(t) = tag {
            // Tagged strings bypass markdown parsing - wrap in Span immediately
            let mut attributes = LinkedHashMap::new();
            attributes.insert("tag".to_string(), t.suffix.clone());

            let span = Span {
                attr: (
                    String::new(),
                    vec!["yaml-tagged-string".to_string()],
                    attributes,
                ),
                content: vec![Inline::Str(Str {
                    text: s.to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
                attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
            };
            return MetaValue::MetaInlines(vec![Inline::Span(span)]);
        }

        // Untagged scalars: parse as booleans or strings (will be parsed as markdown later)
        if s == "true" {
            MetaValue::MetaBool(true)
        } else if s == "false" {
            MetaValue::MetaBool(false)
        } else if s.is_empty() {
            MetaValue::MetaString(String::new())
        } else {
            MetaValue::MetaString(s.to_string())
        }
    }
}

impl MarkedEventReceiver for YamlEventHandler {
    fn on_event(&mut self, ev: Event, _mark: yaml_rust2::scanner::Marker) {
        match ev {
            Event::StreamStart | Event::DocumentStart => {}
            Event::MappingStart(..) => {
                self.stack
                    .push(ContextFrame::Map(LinkedHashMap::new(), None));
            }
            Event::MappingEnd => {
                if let Some(ContextFrame::Map(map, _)) = self.stack.pop() {
                    self.push_value(MetaValue::MetaMap(map));
                }
            }
            Event::SequenceStart(..) => {
                self.stack.push(ContextFrame::List(Vec::new()));
            }
            Event::SequenceEnd => {
                if let Some(ContextFrame::List(list)) = self.stack.pop() {
                    self.push_value(MetaValue::MetaList(list));
                }
            }
            Event::Scalar(s, _style, _anchor, tag) => match self.stack.last_mut() {
                Some(ContextFrame::Map(_, key_slot @ None)) => {
                    *key_slot = Some(s.to_string());
                }
                Some(ContextFrame::Map(_, Some(_))) | Some(ContextFrame::List(_)) => {
                    let value = self.parse_scalar(&s, tag);
                    self.push_value(value);
                }
                _ => {}
            },
            Event::DocumentEnd | Event::StreamEnd => {}
            _ => {}
        }
    }
}

/// Convert RawBlock to MetaValueWithSourceInfo using quarto-yaml (Phase 4)
///
/// This is the new implementation that preserves source location information
/// throughout the YAML -> Meta transformation.
pub fn rawblock_to_meta_with_source_info(
    block: &RawBlock,
    context: &crate::pandoc::ast_context::ASTContext,
    diagnostics: &mut crate::utils::diagnostic_collector::DiagnosticCollector,
) -> MetaValueWithSourceInfo {
    if block.format != "quarto_minus_metadata" {
        panic!(
            "Expected RawBlock with format 'quarto_minus_metadata', got {}",
            block.format
        );
    }

    // Extract YAML content between --- delimiters
    let content = extract_between_delimiters(&block.text).unwrap();

    // Calculate offsets within RawBlock.text
    // Find the actual position of the trimmed content in the original text
    // extract_between_delimiters trims the content, so we need to find where it actually starts
    let yaml_start = block.text.find(content).unwrap();

    // block.source_info is already quarto_source_map::SourceInfo
    let parent = block.source_info.clone();

    // Create Substring SourceInfo for the YAML content within the RawBlock
    let yaml_parent =
        quarto_source_map::SourceInfo::substring(parent, yaml_start, yaml_start + content.len());

    // Parse YAML with source tracking
    let yaml = match quarto_yaml::parse_with_parent(content, yaml_parent.clone()) {
        Ok(y) => y,
        Err(e) => panic!(
            "(unimplemented syntax error - this is a bug!) Failed to parse metadata block as YAML: {}",
            e
        ),
    };

    // Transform YamlWithSourceInfo to MetaValueWithSourceInfo
    // Pass by value since yaml is no longer needed
    let mut result = yaml_to_meta_with_source_info(yaml, context, diagnostics);

    // For the top-level metadata, replace the source_info with yaml_parent
    // to ensure it spans the entire YAML content, not just where the mapping starts
    if let MetaValueWithSourceInfo::MetaMap {
        ref mut source_info,
        ..
    } = result
    {
        *source_info = yaml_parent;
    }

    result
}

/// Legacy version: Convert RawBlock to Meta (old implementation)
///
/// This version uses yaml-rust2 directly and doesn't preserve source information.
/// Kept for backward compatibility during Phase 4.
pub fn rawblock_to_meta(block: RawBlock) -> Meta {
    if block.format != "quarto_minus_metadata" {
        panic!(
            "Expected RawBlock with format 'quarto_minus_metadata', got {}",
            block.format
        );
    }
    let content = extract_between_delimiters(&block.text).unwrap();
    let mut parser = Parser::new_from_str(content);
    let mut handler = YamlEventHandler::new();
    let parse_result = parser.load(&mut handler, false);
    if parse_result.is_err() {
        panic!(
            "(unimplemented syntax error - this is a bug!) Failed to parse metadata block as YAML: {:?}",
            parse_result.err()
        );
    }
    handler.result.unwrap()
}

/// Parse metadata strings as markdown, preserving source information
pub fn parse_metadata_strings_with_source_info(
    meta: MetaValueWithSourceInfo,
    outer_metadata: &mut Vec<MetaMapEntry>,
    diagnostics: &mut crate::utils::diagnostic_collector::DiagnosticCollector,
) -> MetaValueWithSourceInfo {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, source_info } => {
            let mut output_stream = VerboseOutput::Sink(io::sink());
            let result =
                readers::qmd::read(value.as_bytes(), false, "<metadata>", &mut output_stream);
            match result {
                Ok((mut pandoc, _context, warnings)) => {
                    // Propagate warnings from recursive parse
                    for warning in warnings {
                        diagnostics.add(warning);
                    }
                    // Merge parsed metadata, preserving full MetaMapEntry with key_source
                    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = pandoc.meta {
                        for entry in entries {
                            outer_metadata.push(entry);
                        }
                    }
                    // Check if it's a single paragraph - if so, return MetaInlines with original source_info
                    if pandoc.blocks.len() == 1 {
                        if let crate::pandoc::Block::Paragraph(p) = &mut pandoc.blocks[0] {
                            return MetaValueWithSourceInfo::MetaInlines {
                                content: mem::take(&mut p.content),
                                source_info, // Preserve the original source_info from YAML
                            };
                        }
                    }
                    MetaValueWithSourceInfo::MetaBlocks {
                        content: pandoc.blocks,
                        source_info,
                    }
                }
                Err(_) => {
                    // Markdown parse failed - wrap in Span with class "yaml-markdown-syntax-error"
                    let span = Span {
                        attr: (
                            String::new(),
                            vec!["yaml-markdown-syntax-error".to_string()],
                            LinkedHashMap::new(),
                        ),
                        content: vec![Inline::Str(Str {
                            text: value.clone(),
                            source_info: quarto_source_map::SourceInfo::default(),
                        })],
                        source_info: quarto_source_map::SourceInfo::default(),
                        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
                    };
                    MetaValueWithSourceInfo::MetaInlines {
                        content: vec![Inline::Span(span)],
                        source_info,
                    }
                }
            }
        }
        MetaValueWithSourceInfo::MetaList { items, source_info } => {
            let parsed_items = items
                .into_iter()
                .map(|item| {
                    parse_metadata_strings_with_source_info(item, outer_metadata, diagnostics)
                })
                .collect();
            MetaValueWithSourceInfo::MetaList {
                items: parsed_items,
                source_info,
            }
        }
        MetaValueWithSourceInfo::MetaMap {
            entries,
            source_info,
        } => {
            let parsed_entries = entries
                .into_iter()
                .map(|entry| MetaMapEntry {
                    key: entry.key,
                    key_source: entry.key_source,
                    value: parse_metadata_strings_with_source_info(
                        entry.value,
                        outer_metadata,
                        diagnostics,
                    ),
                })
                .collect();
            MetaValueWithSourceInfo::MetaMap {
                entries: parsed_entries,
                source_info,
            }
        }
        other => other,
    }
}

pub fn parse_metadata_strings(meta: MetaValue, outer_metadata: &mut Meta) -> MetaValue {
    match meta {
        MetaValue::MetaString(s) => {
            let mut output_stream = VerboseOutput::Sink(io::sink());
            let result = readers::qmd::read(s.as_bytes(), false, "<metadata>", &mut output_stream);
            match result {
                Ok((mut pandoc, _context, _warnings)) => {
                    // TODO: Handle warnings from recursive parse
                    // pandoc.meta is now MetaValueWithSourceInfo, convert it to Meta
                    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = pandoc.meta {
                        for entry in entries {
                            outer_metadata.insert(entry.key, entry.value.to_meta_value());
                        }
                    }
                    // we need to examine pandoc.blocks to see if it's a single paragraph or multiple blocks
                    // if it's a single paragraph, we can return MetaInlines
                    if pandoc.blocks.len() == 1 {
                        let first = &mut pandoc.blocks[0];
                        match first {
                            crate::pandoc::Block::Paragraph(p) => {
                                return MetaValue::MetaInlines(mem::take(&mut p.content));
                            }
                            _ => {}
                        }
                    }
                    MetaValue::MetaBlocks(pandoc.blocks)
                }
                Err(_) => {
                    // Markdown parse failed - wrap in Span with class "yaml-markdown-syntax-error"
                    let span = Span {
                        attr: (
                            String::new(),
                            vec!["yaml-markdown-syntax-error".to_string()],
                            LinkedHashMap::new(),
                        ),
                        content: vec![Inline::Str(Str {
                            text: s.clone(),
                            source_info: empty_source_info(),
                        })],
                        source_info: empty_source_info(),
                        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
                    };
                    MetaValue::MetaInlines(vec![Inline::Span(span)])
                }
            }
        }
        MetaValue::MetaList(list) => {
            let parsed_list = list
                .into_iter()
                .map(|value| parse_metadata_strings(value, outer_metadata))
                .collect();
            MetaValue::MetaList(parsed_list)
        }
        MetaValue::MetaMap(map) => {
            let parsed_map = map
                .into_iter()
                .map(|(k, v)| (k, parse_metadata_strings(v, outer_metadata)))
                .collect();
            MetaValue::MetaMap(parsed_map)
        }
        other => other,
    }
}
