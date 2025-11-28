/*
 * meta.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::block::Blocks;
use crate::inline::Inlines;
use hashlink::LinkedHashMap;
use serde::{Deserialize, Serialize};

// Pandoc's MetaValue notably does not support numbers or nulls, so we don't either
// https://pandoc.org/lua-filters.html#type-metavalue
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
                    if let crate::inline::Inline::Str(str_node) = &content[0] {
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
