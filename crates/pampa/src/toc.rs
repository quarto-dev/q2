/*
 * toc.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Table of Contents (TOC) generation from document headings.
 */

//! Table of Contents generation for Quarto documents.
//!
//! This module provides data structures and functions for generating a TOC
//! from document headings. The TOC is generated as a structured data object
//! that can be:
//! - Stored in document metadata (`navigation.toc`)
//! - Rendered to HTML by a format-specific transform
//! - Serialized to JSON for external consumption
//!
//! ## Usage
//!
//! ```rust,ignore
//! use pampa::toc::{TocConfig, generate_toc};
//!
//! let config = TocConfig {
//!     depth: 3,
//!     title: Some("Contents".to_string()),
//! };
//!
//! let toc = generate_toc(&document.blocks, &config);
//! ```
//!
//! ## Class-based Filtering
//!
//! Headings can be excluded from the TOC or have numbering disabled:
//!
//! - `unlisted` class: Heading is excluded from TOC entirely
//! - `unnumbered` class: Heading is included but without section number
//!
//! ## Section Structure
//!
//! This module works with both flat headers and sectionized blocks:
//!
//! - **Flat headers**: Walk headers directly, build hierarchy from levels
//! - **Sectionized blocks**: Walk section Divs created by `sectionize_blocks`
//!
//! The function detects sectionized structure and extracts headers accordingly.

use crate::pandoc::block::{Block, Div, Header};
use crate::pandoc::inline::Inline;
use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue};
use quarto_source_map::SourceInfo;
use serde::{Deserialize, Serialize};
use yaml_rust2::Yaml;

/// Configuration for TOC generation.
#[derive(Debug, Clone)]
pub struct TocConfig {
    /// Maximum heading depth to include (1-6, default: 3)
    pub depth: i32,

    /// Title for the TOC (e.g., "Table of Contents")
    pub title: Option<String>,
}

impl Default for TocConfig {
    fn default() -> Self {
        Self {
            depth: 3,
            title: None,
        }
    }
}

/// A single entry in the TOC.
///
/// Represents a heading in the document with its metadata for TOC rendering.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TocEntry {
    /// Section ID for linking (e.g., "introduction")
    pub id: String,

    /// Heading text (plain text, not inlines)
    pub title: String,

    /// Heading level (1-6)
    pub level: i32,

    /// Section number if numbering enabled (e.g., "1.2.3")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,

    /// Child entries (nested headings)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TocEntry>,
}

impl TocEntry {
    /// Convert this entry to a ConfigValue for metadata storage.
    pub fn to_config_value(&self) -> ConfigValue {
        let source_info = SourceInfo::default();
        let mut entries = vec![
            ConfigMapEntry {
                key: "id".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(&self.id, source_info.clone()),
            },
            ConfigMapEntry {
                key: "title".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(&self.title, source_info.clone()),
            },
            ConfigMapEntry {
                key: "level".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_scalar(
                    Yaml::Integer(self.level as i64),
                    source_info.clone(),
                ),
            },
        ];

        if let Some(ref number) = self.number {
            entries.push(ConfigMapEntry {
                key: "number".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(number, source_info.clone()),
            });
        }

        if !self.children.is_empty() {
            let children_values: Vec<ConfigValue> =
                self.children.iter().map(|c| c.to_config_value()).collect();
            entries.push(ConfigMapEntry {
                key: "children".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_array(children_values, source_info.clone()),
            });
        }

        ConfigValue::new_map(entries, source_info)
    }

    /// Create a TocEntry from a ConfigValue.
    pub fn from_config_value(cv: &ConfigValue) -> Option<Self> {
        // Use as_plain_text() to handle both scalar strings and PandocInlines
        // (YAML values like `id: "tldr"` may be parsed as MetaInlines in document frontmatter)
        let id = cv.get("id")?.as_plain_text()?;
        let title = cv.get("title")?.as_plain_text()?;
        // Accept both integer and string-encoded integer for level
        // (YAML parsing may convert integers to strings in some contexts)
        let level_cv = cv.get("level")?;
        let level = level_cv
            .as_int()
            .map(|i| i as i32)
            .or_else(|| level_cv.as_plain_text().and_then(|s| s.parse::<i32>().ok()))?;
        let number = cv.get("number").and_then(|v| v.as_plain_text());

        let children = if let Some(children_cv) = cv.get("children") {
            if let Some(arr) = children_cv.as_array() {
                arr.iter().filter_map(TocEntry::from_config_value).collect()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        Some(TocEntry {
            id,
            title,
            level,
            number,
            children,
        })
    }
}

/// Complete TOC structure stored at `navigation.toc` in document metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NavigationToc {
    /// Title for the TOC (e.g., "Table of Contents")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Root entries (top-level headings)
    pub entries: Vec<TocEntry>,
}

impl NavigationToc {
    /// Convert this TOC to a ConfigValue for metadata storage.
    pub fn to_config_value(&self) -> ConfigValue {
        let source_info = SourceInfo::default();
        let mut entries = vec![];

        if let Some(ref title) = self.title {
            entries.push(ConfigMapEntry {
                key: "title".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(title, source_info.clone()),
            });
        }

        let toc_entries: Vec<ConfigValue> =
            self.entries.iter().map(|e| e.to_config_value()).collect();
        entries.push(ConfigMapEntry {
            key: "entries".to_string(),
            key_source: source_info.clone(),
            value: ConfigValue::new_array(toc_entries, source_info.clone()),
        });

        ConfigValue::new_map(entries, source_info)
    }

    /// Create a NavigationToc from a ConfigValue.
    pub fn from_config_value(cv: &ConfigValue) -> Option<Self> {
        let title = cv.get("title").and_then(|v| v.as_str().map(String::from));

        let entries = if let Some(entries_cv) = cv.get("entries") {
            if let Some(arr) = entries_cv.as_array() {
                arr.iter().filter_map(TocEntry::from_config_value).collect()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        Some(NavigationToc { title, entries })
    }
}

/// Generate a TOC from document blocks.
///
/// This function walks the document blocks and collects headings for the TOC.
/// It handles both flat headers and sectionized blocks (produced by `sectionize_blocks`).
///
/// # Arguments
///
/// * `blocks` - The document blocks to process
/// * `config` - TOC generation configuration
///
/// # Returns
///
/// A `NavigationToc` structure containing the TOC entries.
///
/// # Behavior
///
/// - Headings with `unlisted` class are excluded
/// - Headings with `unnumbered` class are included but without section number
/// - Headings deeper than `config.depth` are excluded
/// - For sectionized blocks, the ID is taken from the section Div
/// - For flat headers, the ID is taken directly from the header
pub fn generate_toc(blocks: &[Block], config: &TocConfig) -> NavigationToc {
    let flat_entries = collect_toc_entries(blocks, config.depth);
    let entries = build_hierarchy(flat_entries);

    NavigationToc {
        title: config.title.clone(),
        entries,
    }
}

/// Internal representation during collection
struct FlatTocEntry {
    id: String,
    title: String,
    level: i32,
    number: Option<String>,
}

/// Collect TOC entries from blocks (flat list, not hierarchical).
fn collect_toc_entries(blocks: &[Block], max_depth: i32) -> Vec<FlatTocEntry> {
    let mut entries = Vec::new();

    for block in blocks {
        match block {
            Block::Div(div) => {
                // Check if this is a section Div
                if is_section_div(div) {
                    if let Some(entry) = extract_entry_from_section(div, max_depth) {
                        entries.push(entry);
                    }
                    // Recurse into section content for nested sections
                    entries.extend(collect_toc_entries(&div.content, max_depth));
                } else {
                    // Non-section Div - recurse into content
                    entries.extend(collect_toc_entries(&div.content, max_depth));
                }
            }
            Block::Header(header) => {
                // Direct header (non-sectionized document)
                if let Some(entry) = extract_entry_from_header(header, max_depth) {
                    entries.push(entry);
                }
            }
            // Other block types: recurse if they contain blocks
            Block::BlockQuote(bq) => {
                entries.extend(collect_toc_entries(&bq.content, max_depth));
            }
            _ => {
                // Other blocks don't contain headers
            }
        }
    }

    entries
}

/// Check if a Div is a section created by sectionize_blocks.
fn is_section_div(div: &Div) -> bool {
    let (_, classes, _) = &div.attr;
    classes.iter().any(|c| c == "section")
}

/// Extract the heading level from a section Div's classes.
fn get_section_level(div: &Div) -> Option<i32> {
    let (_, classes, _) = &div.attr;
    for class in classes {
        if class.starts_with("level") {
            if let Ok(level) = class[5..].parse::<i32>() {
                return Some(level);
            }
        }
    }
    None
}

/// Extract a TOC entry from a section Div.
fn extract_entry_from_section(div: &Div, max_depth: i32) -> Option<FlatTocEntry> {
    let (id, classes, _) = &div.attr;

    // Skip if unlisted
    if classes.iter().any(|c| c == "unlisted") {
        return None;
    }

    // Get level from levelN class
    let level = get_section_level(div)?;

    // Skip if beyond max depth
    if level > max_depth {
        return None;
    }

    // Skip if no ID
    if id.is_empty() {
        return None;
    }

    // Get the header from the section content
    let header = div.content.first().and_then(|b| {
        if let Block::Header(h) = b {
            Some(h)
        } else {
            None
        }
    })?;

    // Extract title text from header
    let title = inlines_to_text(&header.content);

    // Check for unnumbered class (on header or section)
    let is_numbered = !classes.iter().any(|c| c == "unnumbered")
        && !header.attr.1.iter().any(|c| c == "unnumbered");

    Some(FlatTocEntry {
        id: id.clone(),
        title,
        level,
        // TODO: Implement actual section numbering
        number: if is_numbered { None } else { None },
    })
}

/// Extract a TOC entry from a direct Header (non-sectionized document).
fn extract_entry_from_header(header: &Header, max_depth: i32) -> Option<FlatTocEntry> {
    let (id, classes, _) = &header.attr;
    let level = header.level as i32;

    // Skip if unlisted
    if classes.iter().any(|c| c == "unlisted") {
        return None;
    }

    // Skip if beyond max depth
    if level > max_depth {
        return None;
    }

    // Skip if no ID
    if id.is_empty() {
        return None;
    }

    // Extract title text
    let title = inlines_to_text(&header.content);

    // Check for unnumbered class
    let is_numbered = !classes.iter().any(|c| c == "unnumbered");

    Some(FlatTocEntry {
        id: id.clone(),
        title,
        level,
        // TODO: Implement actual section numbering
        number: if is_numbered { None } else { None },
    })
}

/// Convert inlines to plain text for TOC title.
fn inlines_to_text(inlines: &[Inline]) -> String {
    let mut text = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(s) => text.push_str(&s.text),
            Inline::Space(_) => text.push(' '),
            Inline::SoftBreak(_) | Inline::LineBreak(_) => text.push(' '),
            Inline::Code(c) => text.push_str(&c.text),
            Inline::Emph(e) => text.push_str(&inlines_to_text(&e.content)),
            Inline::Underline(u) => text.push_str(&inlines_to_text(&u.content)),
            Inline::Strong(s) => text.push_str(&inlines_to_text(&s.content)),
            Inline::Strikeout(s) => text.push_str(&inlines_to_text(&s.content)),
            Inline::Superscript(s) => text.push_str(&inlines_to_text(&s.content)),
            Inline::Subscript(s) => text.push_str(&inlines_to_text(&s.content)),
            Inline::SmallCaps(s) => text.push_str(&inlines_to_text(&s.content)),
            Inline::Quoted(q) => text.push_str(&inlines_to_text(&q.content)),
            Inline::Cite(c) => text.push_str(&inlines_to_text(&c.content)),
            Inline::Link(l) => text.push_str(&inlines_to_text(&l.content)),
            Inline::Image(i) => text.push_str(&inlines_to_text(&i.content)),
            Inline::Note(_) => {}          // Skip footnotes in TOC
            Inline::NoteReference(_) => {} // Skip note references
            Inline::Span(s) => text.push_str(&inlines_to_text(&s.content)),
            Inline::Math(m) => text.push_str(&m.text),
            Inline::RawInline(_) => {} // Skip raw content
            Inline::Shortcode(_) => {} // Skip shortcodes
            Inline::Attr(_, _) => {}   // Skip attribute nodes
            Inline::Insert(i) => text.push_str(&inlines_to_text(&i.content)),
            Inline::Delete(_) => {} // Skip deleted content
            Inline::Highlight(h) => text.push_str(&inlines_to_text(&h.content)),
            Inline::EditComment(_) => {} // Skip edit comments
            Inline::Custom(_) => {}      // Skip custom nodes
        }
    }
    text
}

/// Build hierarchical structure from flat entries based on levels.
fn build_hierarchy(flat_entries: Vec<FlatTocEntry>) -> Vec<TocEntry> {
    if flat_entries.is_empty() {
        return vec![];
    }

    let mut result: Vec<TocEntry> = vec![];
    let mut stack: Vec<TocEntry> = vec![];

    for flat in flat_entries {
        let entry = TocEntry {
            id: flat.id,
            title: flat.title,
            level: flat.level,
            number: flat.number,
            children: vec![],
        };

        // Pop entries from stack that are at same or higher level
        while let Some(top) = stack.last() {
            if top.level >= entry.level {
                let finished = stack.pop().unwrap();
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(finished);
                } else {
                    result.push(finished);
                }
            } else {
                break;
            }
        }

        stack.push(entry);
    }

    // Flush remaining stack
    while let Some(finished) = stack.pop() {
        if let Some(parent) = stack.last_mut() {
            parent.children.push(finished);
        } else {
            result.push(finished);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pandoc::block::Paragraph;
    use crate::pandoc::inline::Str;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::AttrSourceInfo;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::default()
    }

    fn make_header(level: usize, id: &str, classes: Vec<&str>, text: &str) -> Block {
        Block::Header(Header {
            level,
            attr: (
                id.to_string(),
                classes.iter().map(|s| s.to_string()).collect(),
                LinkedHashMap::new(),
            ),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn make_section(
        level: usize,
        id: &str,
        classes: Vec<&str>,
        header_text: &str,
        content: Vec<Block>,
    ) -> Block {
        let mut section_classes = vec!["section".to_string(), format!("level{}", level)];
        section_classes.extend(classes.iter().map(|s| s.to_string()));

        let header = Block::Header(Header {
            level,
            attr: (
                String::new(),
                classes.iter().map(|s| s.to_string()).collect(),
                LinkedHashMap::new(),
            ),
            content: vec![Inline::Str(Str {
                text: header_text.to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        });

        let mut section_content = vec![header];
        section_content.extend(content);

        Block::Div(Div {
            attr: (id.to_string(), section_classes, LinkedHashMap::new()),
            content: section_content,
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn make_para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })
    }

    #[test]
    fn test_generate_toc_empty() {
        let config = TocConfig::default();
        let toc = generate_toc(&[], &config);
        assert!(toc.entries.is_empty());
        assert!(toc.title.is_none());
    }

    #[test]
    fn test_generate_toc_flat_headers() {
        let blocks = vec![
            make_header(2, "intro", vec![], "Introduction"),
            make_para("Content."),
            make_header(2, "methods", vec![], "Methods"),
            make_para("More content."),
            make_header(2, "results", vec![], "Results"),
        ];

        let config = TocConfig::default();
        let toc = generate_toc(&blocks, &config);

        assert_eq!(toc.entries.len(), 3);
        assert_eq!(toc.entries[0].id, "intro");
        assert_eq!(toc.entries[0].title, "Introduction");
        assert_eq!(toc.entries[0].level, 2);
        assert_eq!(toc.entries[1].id, "methods");
        assert_eq!(toc.entries[2].id, "results");
    }

    #[test]
    fn test_generate_toc_nested_headers() {
        let blocks = vec![
            make_header(1, "chapter", vec![], "Chapter 1"),
            make_header(2, "section-a", vec![], "Section A"),
            make_para("Content A."),
            make_header(2, "section-b", vec![], "Section B"),
            make_header(3, "subsection-b1", vec![], "Subsection B.1"),
        ];

        let config = TocConfig::default();
        let toc = generate_toc(&blocks, &config);

        assert_eq!(toc.entries.len(), 1);
        assert_eq!(toc.entries[0].id, "chapter");
        assert_eq!(toc.entries[0].children.len(), 2);
        assert_eq!(toc.entries[0].children[0].id, "section-a");
        assert_eq!(toc.entries[0].children[1].id, "section-b");
        assert_eq!(toc.entries[0].children[1].children.len(), 1);
        assert_eq!(toc.entries[0].children[1].children[0].id, "subsection-b1");
    }

    #[test]
    fn test_generate_toc_sectionized() {
        let blocks = vec![
            make_section(
                2,
                "intro",
                vec![],
                "Introduction",
                vec![make_para("Content.")],
            ),
            make_section(2, "methods", vec![], "Methods", vec![]),
        ];

        let config = TocConfig::default();
        let toc = generate_toc(&blocks, &config);

        assert_eq!(toc.entries.len(), 2);
        assert_eq!(toc.entries[0].id, "intro");
        assert_eq!(toc.entries[0].title, "Introduction");
        assert_eq!(toc.entries[1].id, "methods");
    }

    #[test]
    fn test_generate_toc_nested_sectionized() {
        let inner_section = make_section(3, "sub", vec![], "Subsection", vec![]);

        let blocks = vec![make_section(
            2,
            "main",
            vec![],
            "Main Section",
            vec![make_para("Content."), inner_section],
        )];

        let config = TocConfig::default();
        let toc = generate_toc(&blocks, &config);

        assert_eq!(toc.entries.len(), 1);
        assert_eq!(toc.entries[0].id, "main");
        assert_eq!(toc.entries[0].children.len(), 1);
        assert_eq!(toc.entries[0].children[0].id, "sub");
    }

    #[test]
    fn test_generate_toc_unlisted_excluded() {
        let blocks = vec![
            make_header(2, "visible", vec![], "Visible"),
            make_header(2, "hidden", vec!["unlisted"], "Hidden"),
            make_header(2, "also-visible", vec![], "Also Visible"),
        ];

        let config = TocConfig::default();
        let toc = generate_toc(&blocks, &config);

        assert_eq!(toc.entries.len(), 2);
        assert_eq!(toc.entries[0].id, "visible");
        assert_eq!(toc.entries[1].id, "also-visible");
    }

    #[test]
    fn test_generate_toc_depth_limit() {
        let blocks = vec![
            make_header(1, "h1", vec![], "Level 1"),
            make_header(2, "h2", vec![], "Level 2"),
            make_header(3, "h3", vec![], "Level 3"),
            make_header(4, "h4", vec![], "Level 4"),
            make_header(5, "h5", vec![], "Level 5"),
        ];

        let config = TocConfig {
            depth: 2,
            title: None,
        };
        let toc = generate_toc(&blocks, &config);

        // Only h1 and h2 should be included
        assert_eq!(toc.entries.len(), 1);
        assert_eq!(toc.entries[0].id, "h1");
        assert_eq!(toc.entries[0].children.len(), 1);
        assert_eq!(toc.entries[0].children[0].id, "h2");
        assert!(toc.entries[0].children[0].children.is_empty());
    }

    #[test]
    fn test_generate_toc_with_title() {
        let blocks = vec![make_header(2, "intro", vec![], "Introduction")];

        let config = TocConfig {
            depth: 3,
            title: Some("Contents".to_string()),
        };
        let toc = generate_toc(&blocks, &config);

        assert_eq!(toc.title, Some("Contents".to_string()));
        assert_eq!(toc.entries.len(), 1);
    }

    #[test]
    fn test_toc_entry_to_config_value() {
        let entry = TocEntry {
            id: "intro".to_string(),
            title: "Introduction".to_string(),
            level: 2,
            number: Some("1.1".to_string()),
            children: vec![TocEntry {
                id: "sub".to_string(),
                title: "Subsection".to_string(),
                level: 3,
                number: None,
                children: vec![],
            }],
        };

        let cv = entry.to_config_value();

        assert_eq!(cv.get("id").unwrap().as_str(), Some("intro"));
        assert_eq!(cv.get("title").unwrap().as_str(), Some("Introduction"));
        assert_eq!(cv.get("level").unwrap().as_int(), Some(2));
        assert_eq!(cv.get("number").unwrap().as_str(), Some("1.1"));

        let children = cv.get("children").unwrap().as_array().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].get("id").unwrap().as_str(), Some("sub"));
    }

    #[test]
    fn test_toc_entry_roundtrip() {
        let original = TocEntry {
            id: "test".to_string(),
            title: "Test Section".to_string(),
            level: 2,
            number: Some("1".to_string()),
            children: vec![TocEntry {
                id: "nested".to_string(),
                title: "Nested".to_string(),
                level: 3,
                number: None,
                children: vec![],
            }],
        };

        let cv = original.to_config_value();
        let restored = TocEntry::from_config_value(&cv).unwrap();

        assert_eq!(original, restored);
    }

    #[test]
    fn test_navigation_toc_to_config_value() {
        let toc = NavigationToc {
            title: Some("Table of Contents".to_string()),
            entries: vec![TocEntry {
                id: "intro".to_string(),
                title: "Introduction".to_string(),
                level: 1,
                number: None,
                children: vec![],
            }],
        };

        let cv = toc.to_config_value();

        assert_eq!(cv.get("title").unwrap().as_str(), Some("Table of Contents"));
        let entries = cv.get("entries").unwrap().as_array().unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_navigation_toc_roundtrip() {
        let original = NavigationToc {
            title: Some("Contents".to_string()),
            entries: vec![
                TocEntry {
                    id: "a".to_string(),
                    title: "Section A".to_string(),
                    level: 1,
                    number: None,
                    children: vec![],
                },
                TocEntry {
                    id: "b".to_string(),
                    title: "Section B".to_string(),
                    level: 1,
                    number: None,
                    children: vec![],
                },
            ],
        };

        let cv = original.to_config_value();
        let restored = NavigationToc::from_config_value(&cv).unwrap();

        assert_eq!(original, restored);
    }

    #[test]
    fn test_inlines_to_text_simple() {
        let inlines = vec![Inline::Str(Str {
            text: "Hello World".to_string(),
            source_info: dummy_source_info(),
        })];

        assert_eq!(inlines_to_text(&inlines), "Hello World");
    }

    #[test]
    fn test_inlines_to_text_with_formatting() {
        use crate::pandoc::inline::{Emph, Space, Strong};

        let inlines = vec![
            Inline::Str(Str {
                text: "Hello".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::Space(Space {
                source_info: dummy_source_info(),
            }),
            Inline::Strong(Strong {
                content: vec![Inline::Str(Str {
                    text: "bold".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            }),
            Inline::Space(Space {
                source_info: dummy_source_info(),
            }),
            Inline::Emph(Emph {
                content: vec![Inline::Str(Str {
                    text: "italic".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            }),
        ];

        assert_eq!(inlines_to_text(&inlines), "Hello bold italic");
    }

    #[test]
    fn test_skip_headers_without_id() {
        let blocks = vec![
            make_header(2, "has-id", vec![], "Has ID"),
            make_header(2, "", vec![], "No ID"),
            make_header(2, "also-has-id", vec![], "Also Has ID"),
        ];

        let config = TocConfig::default();
        let toc = generate_toc(&blocks, &config);

        assert_eq!(toc.entries.len(), 2);
        assert_eq!(toc.entries[0].id, "has-id");
        assert_eq!(toc.entries[1].id, "also-has-id");
    }

    #[test]
    fn test_build_hierarchy_complex() {
        // Test hierarchy building with various level patterns
        let blocks = vec![
            make_header(1, "h1-a", vec![], "H1 A"),
            make_header(2, "h2-a", vec![], "H2 A"),
            make_header(3, "h3-a", vec![], "H3 A"),
            make_header(2, "h2-b", vec![], "H2 B"),
            make_header(1, "h1-b", vec![], "H1 B"),
            make_header(2, "h2-c", vec![], "H2 C"),
        ];

        let config = TocConfig::default();
        let toc = generate_toc(&blocks, &config);

        // Should have 2 h1 entries
        assert_eq!(toc.entries.len(), 2);

        // First h1 has 2 h2 children
        assert_eq!(toc.entries[0].children.len(), 2);

        // First h2 has 1 h3 child
        assert_eq!(toc.entries[0].children[0].children.len(), 1);

        // Second h2 has no children
        assert!(toc.entries[0].children[1].children.is_empty());

        // Second h1 has 1 h2 child
        assert_eq!(toc.entries[1].children.len(), 1);
    }
}
