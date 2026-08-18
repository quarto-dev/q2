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
//!     // Inlines, not a String — `toc-title` carries markup.
//!     title: Some(vec![Inline::Str(Str::from("Contents"))]),
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
//!
//! ## The walk stops at a non-section Div
//!
//! The table of contents *is* the section tree. A `Div` that is not a
//! section ends the walk, with no recursion past it — this is pandoc's
//! rule, whose `sectionToListItem` matches only `Div(_, _, Header:rest)`
//! and yields nothing for anything else.
//!
//! This is not a loss of reach, because `sectionize_blocks` *absorbs* a
//! transparent wrapper — an empty-id `Div` around a single header-led
//! run — into the section itself, so a heading inside `::: {.column-margin}`
//! or a plain `:::` block is a section by the time we get here. What stays
//! wrapped in a plain `Div` is filter-built chrome: a callout body, a
//! tabset pane. Quarto 1 does not list those either, and listing them was
//! actively harmful — the entries were indistinguishable from one another
//! and every one past the first pointed at `display: none` content
//! (bd-tabset-headings-in-toc-t04ie7f7).
//!
//! **Precondition:** callers that want headings nested in Divs to appear
//! must pass blocks that have been through `sectionize_blocks`. See
//! `SectionizeTransform`, which runs before `TocGenerateTransform`;
//! `DocumentProfile::extract_outline` sectionizes a copy for the same
//! reason. Revealjs skips `SectionizeTransform` and would therefore get
//! an empty TOC — it emits none today, and bd-tebu6o4a tracks the trap.

use crate::pandoc::block::{Block, Div, Header};
use crate::pandoc::inline::{Inline, Inlines, Str};
use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue, ConfigValueKind};
use quarto_source_map::{By, SourceInfo};
use serde::{Deserialize, Serialize};
use yaml_rust2::Yaml;

/// Configuration for TOC generation.
#[derive(Debug, Clone)]
pub struct TocConfig {
    /// Maximum heading depth to include (1-6, default: 3)
    pub depth: i32,

    /// Title for the TOC (e.g. "Table of Contents"), as inlines.
    ///
    /// Carries markup — see [`NavigationToc::title`].
    pub title: Option<Inlines>,
}

impl Default for TocConfig {
    fn default() -> Self {
        Self {
            depth: 3,
            title: None,
        }
    }
}

/// Read a metadata value as inlines, accepting either shape the
/// interpretation layer can hand us.
///
/// `InterpretationContext` resolves an untagged YAML string at *load*
/// time, and the two sources have opposite defaults
/// (`quarto_pandoc_types::config_value::InterpretationContext`):
///
/// - document front matter parses markdown, giving `PandocInlines` —
///   used as-is;
/// - `_quarto.yml` keeps strings literal, giving `Scalar(String)` —
///   wrapped in a single `Str`, deliberately *not* re-parsed. Project
///   config is literal by design; the sanctioned way to opt a key into
///   markdown there is `config_markdown.rs`'s `MARKDOWN_CONFIG_PATHS`
///   registry, not an ad-hoc parse here.
///
/// Programmatically-constructed values (Lua filters, tests) also arrive
/// as `Scalar(String)` and get the same literal treatment.
pub fn config_value_to_inlines(cv: &ConfigValue) -> Option<Inlines> {
    if let ConfigValueKind::PandocInlines(inlines) = &cv.value {
        return Some(inlines.clone());
    }
    // Any other scalar shape: take its plain text and wrap it literally.
    let text = cv.as_plain_text()?;
    Some(vec![Inline::Str(Str {
        text,
        source_info: cv.source_info.clone(),
    })])
}

/// Wrap literal text as a single `Str` inline.
///
/// For values that are genuinely plain text and must stay literal — a
/// localized term from the language catalog, a built-in English default
/// — rather than markdown that happens not to contain markup.
pub fn plain_inlines(text: impl Into<String>) -> Inlines {
    vec![Inline::Str(Str {
        text: text.into(),
        source_info: SourceInfo::generated(By::programmatic_config()),
    })]
}

/// A single entry in the TOC.
///
/// Represents a heading in the document with its metadata for TOC rendering.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TocEntry {
    /// Section ID for linking (e.g., "introduction")
    pub id: String,

    /// Heading content, as inlines.
    ///
    /// Carries the heading's inline markup verbatim — emphasis, code,
    /// math, quoted spans and their delimiters. This is deliberately
    /// *not* flattened to a `String`: Quarto 1 renders `<code>`, `<em>`
    /// and math spans inside TOC entries, and flattening also silently
    /// dropped `Quoted` delimiters, so a TOC label disagreed with the
    /// heading it pointed at (bd-toc-smart-quotes-6nro57ed).
    ///
    /// Consumers that cannot render markup are responsible for
    /// projecting it themselves. Renderers with structural constraints
    /// are likewise responsible for their own filtering — the HTML TOC
    /// strips links and notes at render time (`toc_render`), because
    /// "an anchor may not nest" is a fact about that output, not about
    /// the document's heading structure. This field is also
    /// `DocumentProfile::outline`, which is meant to be a faithful
    /// semantic outline.
    pub title: Inlines,

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
        let source_info = SourceInfo::generated(By::programmatic_config());
        let mut entries = vec![
            ConfigMapEntry {
                key: "id".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(&self.id, source_info.clone()),
            },
            ConfigMapEntry {
                key: "title".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_inlines(self.title.clone(), source_info.clone()),
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
    ///
    /// The `title` accepts either shape the metadata layer can produce —
    /// see [`config_value_to_inlines`].
    pub fn from_config_value(cv: &ConfigValue) -> Option<Self> {
        // Use as_plain_text() to handle both scalar strings and PandocInlines
        // (YAML values like `id: "tldr"` may be parsed as MetaInlines in document frontmatter)
        let id = cv.get("id")?.as_plain_text()?;
        let title = config_value_to_inlines(cv.get("title")?)?;
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
    /// Title for the TOC (e.g. "Table of Contents"), as inlines.
    ///
    /// Inlines rather than a `String` for the same reason as
    /// [`TocEntry::title`]: Quarto 1 renders `toc-title` through Pandoc,
    /// so `toc-title: "On **this** page"` produces markup, and flattening
    /// it here would silently discard what the metadata layer already
    /// parsed (bd-toc-smart-quotes-6nro57ed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<Inlines>,

    /// Root entries (top-level headings)
    pub entries: Vec<TocEntry>,
}

impl NavigationToc {
    /// Convert this TOC to a ConfigValue for metadata storage.
    pub fn to_config_value(&self) -> ConfigValue {
        let source_info = SourceInfo::generated(By::programmatic_config());
        let mut entries = vec![];

        if let Some(ref title) = self.title {
            entries.push(ConfigMapEntry {
                key: "title".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_inlines(title.clone(), source_info.clone()),
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
        // `as_str()` would return `None` for the `PandocInlines` a
        // front-matter `toc-title` produces — the `metadata-as-str` trap.
        let title = cv.get("title").and_then(config_value_to_inlines);

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
    title: Inlines,
    level: i32,
    number: Option<String>,
}

/// Collect TOC entries from blocks (flat list, not hierarchical).
fn collect_toc_entries(blocks: &[Block], max_depth: i32) -> Vec<FlatTocEntry> {
    let mut entries = Vec::new();

    for block in blocks {
        match block {
            // Only section Divs are walked. A Div that is not a section
            // ends the walk, with no recursion past it — pandoc's
            // `sectionToListItem` matches `Div(_, _, Header:rest)` and
            // returns nothing for anything else.
            Block::Div(div) if is_section_div(div) => {
                if let Some(entry) = extract_entry_from_section(div, max_depth) {
                    entries.push(entry);
                }
                // Recurse into section content for nested sections
                entries.extend(collect_toc_entries(&div.content, max_depth));
            }
            Block::Header(header) => {
                // Direct header (non-sectionized document)
                if let Some(entry) = extract_entry_from_header(header, max_depth) {
                    entries.push(entry);
                }
            }
            _ => {
                // Nothing else carries a section. In particular
                // `BlockQuote`: `sectionize_blocks` does not descend
                // into one (matching pandoc's `makeSections`), so a
                // quoted heading is never a section and is never
                // listed. bd-8yjvs3bj.
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
        if class.starts_with("level")
            && let Ok(level) = class[5..].parse::<i32>()
        {
            return Some(level);
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

    // Carry the heading's inlines verbatim; markup is preserved and
    // format-specific filtering happens at render time.
    let title = header.content.clone();

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

    // Carry the heading's inlines verbatim (see above).
    let title = header.content.clone();

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
        SourceInfo::for_test()
    }

    /// A title made of one literal `Str`, for the many tests that only
    /// care about the surrounding structure rather than the markup.
    fn plain(text: &str) -> Inlines {
        vec![Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })]
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
        assert_eq!(toc.entries[0].title, plain("Introduction"));
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
        assert_eq!(toc.entries[0].title, plain("Introduction"));
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

    // ── the walk stops at a non-section Div ────────────────────────
    //
    // pandoc's `toTableOfContents` matches only `Div(_, _, Header:rest)`
    // — a section — so a Div that is not one ends the walk with no
    // recursion past it. Everything a reader expects to see in the TOC
    // has already been *absorbed* into the section tree by
    // `sectionize_blocks`; what remains wrapped in a plain Div is filter
    // chrome (a callout, a tabset pane), and Quarto 1 does not list it.

    /// Every id in the tree, depth-first. Checking only `toc.entries`
    /// would miss a leaked entry, which `build_hierarchy` nests *under*
    /// the preceding top-level section rather than beside it.
    fn all_ids(entries: &[TocEntry]) -> Vec<String> {
        let mut out = Vec::new();
        for e in entries {
            out.push(e.id.clone());
            out.extend(all_ids(&e.children));
        }
        out
    }

    /// The default depth is 3, which would exclude the level-4 headings
    /// these tests use for reasons that have nothing to do with the walk.
    fn deep_config() -> TocConfig {
        TocConfig {
            depth: 4,
            title: None,
        }
    }

    fn make_plain_div(id: &str, classes: Vec<&str>, content: Vec<Block>) -> Block {
        Block::Div(Div {
            attr: (
                id.to_string(),
                classes.iter().map(|s| s.to_string()).collect(),
                LinkedHashMap::new(),
            ),
            content,
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    #[test]
    fn test_non_section_div_terminates_the_walk() {
        // The shape a resolved tabset leaves behind: a section, real and
        // correct, buried under two plain Divs.
        let blocks = vec![
            make_section(2, "configuration", vec![], "Configuration", vec![]),
            make_plain_div(
                "",
                vec!["panel-tabset"],
                vec![make_plain_div(
                    "tabset-1-1",
                    vec!["tab-pane"],
                    vec![make_section(4, "in-a-tab", vec![], "In a tab", vec![])],
                )],
            ),
            make_section(2, "next-steps", vec![], "Next steps", vec![]),
        ];
        let toc = generate_toc(&blocks, &deep_config());
        assert_eq!(
            all_ids(&toc.entries),
            vec!["configuration", "next-steps"],
            "the section inside the tab pane is real, but unreachable"
        );
    }

    #[test]
    fn test_section_nested_in_a_section_is_still_collected() {
        // The walk must still recurse through *sections*, or the TOC
        // would flatten to top-level headings only.
        let blocks = vec![make_section(
            2,
            "outer",
            vec![],
            "Outer",
            vec![make_section(3, "inner", vec![], "Inner", vec![])],
        )];
        let toc = generate_toc(&blocks, &TocConfig::default());
        assert_eq!(toc.entries.len(), 1);
        assert_eq!(toc.entries[0].id, "outer");
        assert_eq!(toc.entries[0].children.len(), 1);
        assert_eq!(toc.entries[0].children[0].id, "inner");
    }

    /// bd-8yjvs3bj. `makeSections` never descends into a `BlockQuote`, so
    /// a heading quoted there is not a section and is not listed.
    #[test]
    fn test_blockquote_heading_is_not_collected() {
        let blocks = vec![
            make_section(2, "outer", vec![], "Outer", vec![]),
            Block::BlockQuote(quarto_pandoc_types::block::BlockQuote {
                content: vec![make_header(4, "quoted", vec![], "Quoted")],
                source_info: dummy_source_info(),
            }),
        ];
        let toc = generate_toc(&blocks, &deep_config());
        assert_eq!(all_ids(&toc.entries), vec!["outer"]);
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
            title: Some(plain("Contents")),
        };
        let toc = generate_toc(&blocks, &config);

        assert_eq!(toc.title, Some(plain("Contents")));
        assert_eq!(toc.entries.len(), 1);
    }

    #[test]
    fn test_toc_entry_to_config_value() {
        let entry = TocEntry {
            id: "intro".to_string(),
            title: plain("Introduction"),
            level: 2,
            number: Some("1.1".to_string()),
            children: vec![TocEntry {
                id: "sub".to_string(),
                title: plain("Subsection"),
                level: 3,
                number: None,
                children: vec![],
            }],
        };

        let cv = entry.to_config_value();

        assert_eq!(cv.get("id").unwrap().as_str(), Some("intro"));
        // The title is `PandocInlines`, not a scalar — `as_str()` returns
        // `None` for it (the `metadata-as-str` lint's whole point), so
        // assert on the inline content itself.
        assert_eq!(
            cv.get("title").map(|v| &v.value),
            Some(&ConfigValueKind::PandocInlines(plain("Introduction")))
        );
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
            title: plain("Test Section"),
            level: 2,
            number: Some("1".to_string()),
            children: vec![TocEntry {
                id: "nested".to_string(),
                title: plain("Nested"),
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
            title: Some(plain("Table of Contents")),
            entries: vec![TocEntry {
                id: "intro".to_string(),
                title: plain("Introduction"),
                level: 1,
                number: None,
                children: vec![],
            }],
        };

        let cv = toc.to_config_value();

        // `PandocInlines`, not a scalar — `as_str()` returns `None`.
        assert_eq!(
            cv.get("title").map(|v| &v.value),
            Some(&ConfigValueKind::PandocInlines(plain("Table of Contents")))
        );
        let entries = cv.get("entries").unwrap().as_array().unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_navigation_toc_roundtrip() {
        let original = NavigationToc {
            title: Some(plain("Contents")),
            entries: vec![
                TocEntry {
                    id: "a".to_string(),
                    title: plain("Section A"),
                    level: 1,
                    number: None,
                    children: vec![],
                },
                TocEntry {
                    id: "b".to_string(),
                    title: plain("Section B"),
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

    // -----------------------------------------------------------------
    // title-as-inlines (bd-toc-smart-quotes-6nro57ed)
    // -----------------------------------------------------------------

    /// `to_config_value` must emit the title as `PandocInlines`, and
    /// `from_config_value` must read it back unchanged. This is the
    /// round-trip `navigation.toc` metadata depends on: the entries are
    /// serialized into metadata by `TocGenerateTransform` and parsed
    /// back out by `TocRenderTransform`.
    #[test]
    fn test_toc_entry_config_value_round_trip_preserves_inlines() {
        use crate::pandoc::inline::{Code, QuoteType, Quoted};

        let title = vec![
            Inline::Str(Str {
                text: "Using a ".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::Quoted(Quoted {
                quote_type: QuoteType::DoubleQuote,
                content: vec![Inline::Str(Str {
                    text: "raw".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            }),
            Inline::Code(Code {
                text: "volume".to_string(),
                attr: (String::new(), vec![], LinkedHashMap::new()),
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            }),
        ];

        let original = TocEntry {
            id: "using-a-raw-volume".to_string(),
            title: title.clone(),
            level: 2,
            number: None,
            children: vec![],
        };

        let restored = TocEntry::from_config_value(&original.to_config_value()).unwrap();

        assert_eq!(
            restored.title, title,
            "the quoted span and the code span must survive the metadata round-trip"
        );
        assert_eq!(restored.id, original.id);
        assert_eq!(restored.level, original.level);
    }

    /// A hand-written `navigation.toc` from `_quarto.yml` arrives as
    /// `Scalar(String)` because `InterpretationContext::ProjectConfig`
    /// keeps strings literal. It must still parse, wrapped in a single
    /// `Str` — and deliberately *not* re-parsed as markdown.
    #[test]
    fn test_toc_entry_accepts_scalar_string_title_literally() {
        let cv = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "id".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("sec", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("A **literal** title", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "level".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_scalar(Yaml::Integer(2), dummy_source_info()),
                },
            ],
            dummy_source_info(),
        );

        let entry = TocEntry::from_config_value(&cv).expect("legacy string title must parse");

        assert_eq!(
            entry.title,
            vec![Inline::Str(Str {
                text: "A **literal** title".to_string(),
                source_info: dummy_source_info(),
            })],
            "a project-config string is literal: one Str, asterisks intact, no Strong"
        );
    }

    /// A front-matter title arrives already parsed as `PandocInlines`;
    /// it must be used as-is rather than flattened.
    #[test]
    fn test_toc_entry_accepts_pandoc_inlines_title() {
        use crate::pandoc::inline::Strong;

        let inlines = vec![Inline::Strong(Strong {
            content: vec![Inline::Str(Str {
                text: "bold".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];

        let cv = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "id".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("sec", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_inlines(inlines.clone(), dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "level".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_scalar(Yaml::Integer(2), dummy_source_info()),
                },
            ],
            dummy_source_info(),
        );

        let entry = TocEntry::from_config_value(&cv).unwrap();
        assert_eq!(entry.title, inlines);
    }

    /// `generate_toc` must hand the heading's inlines through untouched
    /// — including the `Quoted` node whose delimiters the old flattener
    /// silently dropped.
    #[test]
    fn test_generate_toc_carries_heading_inlines() {
        use crate::pandoc::inline::{QuoteType, Quoted};

        let header_content = vec![
            Inline::Str(Str {
                text: "Using a ".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::Quoted(Quoted {
                quote_type: QuoteType::DoubleQuote,
                content: vec![Inline::Str(Str {
                    text: "raw".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            }),
        ];

        let blocks = vec![Block::Header(Header {
            level: 2,
            attr: (
                "using-a-raw-volume".to_string(),
                vec![],
                LinkedHashMap::new(),
            ),
            content: header_content.clone(),
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })];

        let toc = generate_toc(&blocks, &TocConfig::default());

        assert_eq!(toc.entries.len(), 1);
        assert_eq!(
            toc.entries[0].title, header_content,
            "the TOC entry must carry the heading's inlines verbatim"
        );
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
