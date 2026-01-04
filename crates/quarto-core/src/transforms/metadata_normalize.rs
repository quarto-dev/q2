/*
 * metadata_normalize.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that normalizes document metadata.
 */

//! Metadata normalization transform.
//!
//! This transform prepares document metadata for rendering by adding
//! derived fields:
//!
//! - `pagetitle`: Plain-text version of `title` (for HTML `<title>` element)
//!
//! More derived fields can be added in the future (author-meta, date-meta, etc.)

use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, Slot};
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that normalizes document metadata.
///
/// This adds derived metadata fields needed for template rendering:
/// - `pagetitle` from `title` (plain text version)
///
/// The transform is idempotent - running it multiple times has no effect
/// if the derived fields already exist.
pub struct MetadataNormalizeTransform;

impl MetadataNormalizeTransform {
    /// Create a new metadata normalization transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for MetadataNormalizeTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl AstTransform for MetadataNormalizeTransform {
    fn name(&self) -> &str {
        "metadata-normalize"
    }

    fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        normalize_metadata(&mut ast.meta);
        Ok(())
    }
}

/// Normalize document metadata by adding derived fields.
fn normalize_metadata(meta: &mut ConfigValue) {
    // Only process if meta is a Map
    let ConfigValueKind::Map(entries) = &mut meta.value else {
        return;
    };
    let source_info = meta.source_info.clone();

    // Add pagetitle if not present
    add_pagetitle_if_missing(entries, source_info);
}

/// Add `pagetitle` field derived from `title` if not already present.
fn add_pagetitle_if_missing(entries: &mut Vec<ConfigMapEntry>, source_info: SourceInfo) {
    // Check if pagetitle already exists
    let has_pagetitle = entries.iter().any(|e| e.key == "pagetitle");
    if has_pagetitle {
        return;
    }

    // Look for title field
    let title_entry = entries.iter().find(|e| e.key == "title");
    if let Some(entry) = title_entry {
        let plain_text = extract_plain_text(&entry.value);
        if let Some(text) = plain_text {
            entries.push(ConfigMapEntry {
                key: "pagetitle".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(text, source_info),
            });
        }
    }
}

/// Extract plain text from a metadata value.
fn extract_plain_text(meta: &ConfigValue) -> Option<String> {
    // Check for string scalar first
    if let Some(s) = meta.as_str() {
        return Some(s.to_string());
    }
    // Check for Pandoc content
    match &meta.value {
        ConfigValueKind::PandocInlines(content) => Some(inlines_to_plain_text(content)),
        ConfigValueKind::PandocBlocks(content) => Some(blocks_to_plain_text(content)),
        _ => None,
    }
}

/// Convert inlines to plain text.
fn inlines_to_plain_text(inlines: &[Inline]) -> String {
    let mut result = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(s) => result.push_str(&s.text),
            Inline::Space(_) => result.push(' '),
            Inline::SoftBreak(_) => result.push(' '),
            Inline::LineBreak(_) => result.push('\n'),
            Inline::Emph(e) => result.push_str(&inlines_to_plain_text(&e.content)),
            Inline::Underline(u) => result.push_str(&inlines_to_plain_text(&u.content)),
            Inline::Strong(s) => result.push_str(&inlines_to_plain_text(&s.content)),
            Inline::Strikeout(s) => result.push_str(&inlines_to_plain_text(&s.content)),
            Inline::Superscript(s) => result.push_str(&inlines_to_plain_text(&s.content)),
            Inline::Subscript(s) => result.push_str(&inlines_to_plain_text(&s.content)),
            Inline::SmallCaps(s) => result.push_str(&inlines_to_plain_text(&s.content)),
            Inline::Quoted(q) => {
                let quote_char = match q.quote_type {
                    quarto_pandoc_types::inline::QuoteType::SingleQuote => '\'',
                    quarto_pandoc_types::inline::QuoteType::DoubleQuote => '"',
                };
                result.push(quote_char);
                result.push_str(&inlines_to_plain_text(&q.content));
                result.push(quote_char);
            }
            Inline::Cite(c) => result.push_str(&inlines_to_plain_text(&c.content)),
            Inline::Code(c) => result.push_str(&c.text),
            Inline::Math(m) => result.push_str(&m.text),
            Inline::RawInline(r) => result.push_str(&r.text),
            Inline::Link(l) => result.push_str(&inlines_to_plain_text(&l.content)),
            Inline::Image(i) => result.push_str(&inlines_to_plain_text(&i.content)),
            Inline::Note(n) => result.push_str(&blocks_to_plain_text(&n.content)),
            Inline::Span(s) => result.push_str(&inlines_to_plain_text(&s.content)),
            Inline::Insert(i) => result.push_str(&inlines_to_plain_text(&i.content)),
            Inline::Delete(d) => result.push_str(&inlines_to_plain_text(&d.content)),
            Inline::Highlight(h) => result.push_str(&inlines_to_plain_text(&h.content)),
            Inline::EditComment(e) => result.push_str(&inlines_to_plain_text(&e.content)),
            Inline::Custom(c) => {
                // For custom nodes, try to extract text from slots
                for (_name, slot) in &c.slots {
                    match slot {
                        Slot::Inline(inline) => {
                            result.push_str(&inlines_to_plain_text(&[(**inline).clone()]));
                        }
                        Slot::Inlines(inlines) => {
                            result.push_str(&inlines_to_plain_text(inlines));
                        }
                        Slot::Block(block) => {
                            result.push_str(&blocks_to_plain_text(&[(**block).clone()]));
                        }
                        Slot::Blocks(blocks) => {
                            result.push_str(&blocks_to_plain_text(blocks));
                        }
                    }
                }
            }
            // Skip these - they don't contribute meaningful text
            Inline::Shortcode(_) | Inline::NoteReference(_) | Inline::Attr(_, _) => {}
        }
    }
    result
}

/// Convert blocks to plain text.
fn blocks_to_plain_text(blocks: &[Block]) -> String {
    let mut result = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        match block {
            Block::Plain(p) => result.push_str(&inlines_to_plain_text(&p.content)),
            Block::Paragraph(p) => result.push_str(&inlines_to_plain_text(&p.content)),
            Block::LineBlock(lb) => {
                for line in &lb.content {
                    result.push_str(&inlines_to_plain_text(line));
                    result.push('\n');
                }
            }
            Block::CodeBlock(cb) => result.push_str(&cb.text),
            Block::RawBlock(rb) => result.push_str(&rb.text),
            Block::BlockQuote(bq) => result.push_str(&blocks_to_plain_text(&bq.content)),
            Block::OrderedList(ol) => {
                for item in &ol.content {
                    result.push_str(&blocks_to_plain_text(item));
                    result.push('\n');
                }
            }
            Block::BulletList(bl) => {
                for item in &bl.content {
                    result.push_str(&blocks_to_plain_text(item));
                    result.push('\n');
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in &dl.content {
                    result.push_str(&inlines_to_plain_text(term));
                    result.push('\n');
                    for def in defs {
                        result.push_str(&blocks_to_plain_text(def));
                        result.push('\n');
                    }
                }
            }
            Block::Header(h) => result.push_str(&inlines_to_plain_text(&h.content)),
            Block::Div(d) => result.push_str(&blocks_to_plain_text(&d.content)),
            Block::Table(t) => {
                // Extract text from caption if present
                if let Some(caption) = &t.caption.short {
                    result.push_str(&inlines_to_plain_text(caption));
                }
            }
            Block::Figure(f) => {
                // Extract text from caption
                if let Some(caption) = &f.caption.short {
                    result.push_str(&inlines_to_plain_text(caption));
                }
            }
            Block::Custom(c) => {
                // For custom nodes, try to extract text from slots
                for (_name, slot) in &c.slots {
                    match slot {
                        Slot::Block(block) => {
                            result.push_str(&blocks_to_plain_text(&[(**block).clone()]));
                        }
                        Slot::Blocks(blocks) => {
                            result.push_str(&blocks_to_plain_text(blocks));
                        }
                        Slot::Inline(inline) => {
                            result.push_str(&inlines_to_plain_text(&[(**inline).clone()]));
                        }
                        Slot::Inlines(inlines) => {
                            result.push_str(&inlines_to_plain_text(inlines));
                        }
                    }
                }
            }
            // These don't contribute meaningful text
            Block::HorizontalRule(_)
            | Block::BlockMetadata(_)
            | Block::NoteDefinitionPara(_)
            | Block::NoteDefinitionFencedBlock(_)
            | Block::CaptionBlock(_) => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::inline::Str;
    use quarto_source_map::{FileId, Location, Range};
    use std::path::PathBuf;

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::{BinaryDependencies, RenderContext};

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
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
            },
        )
    }

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: None,
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    #[test]
    fn test_adds_pagetitle_from_string_title() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![ConfigMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("My Document", dummy_source_info()),
                }],
                dummy_source_info(),
            ),
            blocks: vec![],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = MetadataNormalizeTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Check that pagetitle was added
        if let ConfigValueKind::Map(entries) = &ast.meta.value {
            let pagetitle = entries.iter().find(|e| e.key == "pagetitle");
            assert!(pagetitle.is_some());
            if let Some(entry) = pagetitle {
                let value = entry.value.as_str().expect("Expected string for pagetitle");
                assert_eq!(value, "My Document");
            }
        } else {
            panic!("Expected Map");
        }
    }

    #[test]
    fn test_adds_pagetitle_from_inlines_title() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![ConfigMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_inlines(
                        vec![Inline::Str(Str {
                            text: "Inline Title".to_string(),
                            source_info: dummy_source_info(),
                        })],
                        dummy_source_info(),
                    ),
                }],
                dummy_source_info(),
            ),
            blocks: vec![],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = MetadataNormalizeTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        if let ConfigValueKind::Map(entries) = &ast.meta.value {
            let pagetitle = entries.iter().find(|e| e.key == "pagetitle");
            assert!(pagetitle.is_some());
            if let Some(entry) = pagetitle {
                let value = entry.value.as_str().expect("Expected string for pagetitle");
                assert_eq!(value, "Inline Title");
            }
        }
    }

    #[test]
    fn test_preserves_existing_pagetitle() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![
                    ConfigMapEntry {
                        key: "title".to_string(),
                        key_source: dummy_source_info(),
                        value: ConfigValue::new_string("My Document", dummy_source_info()),
                    },
                    ConfigMapEntry {
                        key: "pagetitle".to_string(),
                        key_source: dummy_source_info(),
                        value: ConfigValue::new_string("Custom Page Title", dummy_source_info()),
                    },
                ],
                dummy_source_info(),
            ),
            blocks: vec![],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = MetadataNormalizeTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Check that pagetitle was NOT overwritten
        if let ConfigValueKind::Map(entries) = &ast.meta.value {
            let pagetitle_entries: Vec<_> =
                entries.iter().filter(|e| e.key == "pagetitle").collect();
            assert_eq!(pagetitle_entries.len(), 1);
            let value = pagetitle_entries[0]
                .value
                .as_str()
                .expect("Expected string for pagetitle");
            assert_eq!(value, "Custom Page Title");
        }
    }

    #[test]
    fn test_inlines_to_plain_text() {
        let inlines = vec![
            Inline::Str(Str {
                text: "Hello".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::Space(quarto_pandoc_types::inline::Space {
                source_info: dummy_source_info(),
            }),
            Inline::Str(Str {
                text: "World".to_string(),
                source_info: dummy_source_info(),
            }),
        ];

        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn test_transform_name() {
        let transform = MetadataNormalizeTransform::new();
        assert_eq!(transform.name(), "metadata-normalize");
    }

    // ============================================================================
    // Tests for inlines_to_plain_text - covering various inline types
    // ============================================================================

    #[test]
    fn test_inlines_soft_break() {
        let inlines = vec![
            Inline::Str(Str {
                text: "Hello".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::SoftBreak(quarto_pandoc_types::inline::SoftBreak {
                source_info: dummy_source_info(),
            }),
            Inline::Str(Str {
                text: "World".to_string(),
                source_info: dummy_source_info(),
            }),
        ];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn test_inlines_line_break() {
        let inlines = vec![
            Inline::Str(Str {
                text: "Line1".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::LineBreak(quarto_pandoc_types::inline::LineBreak {
                source_info: dummy_source_info(),
            }),
            Inline::Str(Str {
                text: "Line2".to_string(),
                source_info: dummy_source_info(),
            }),
        ];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "Line1\nLine2");
    }

    #[test]
    fn test_inlines_emph() {
        let inlines = vec![Inline::Emph(quarto_pandoc_types::inline::Emph {
            content: vec![Inline::Str(Str {
                text: "emphasized".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "emphasized");
    }

    #[test]
    fn test_inlines_underline() {
        let inlines = vec![Inline::Underline(quarto_pandoc_types::inline::Underline {
            content: vec![Inline::Str(Str {
                text: "underlined".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "underlined");
    }

    #[test]
    fn test_inlines_strong() {
        let inlines = vec![Inline::Strong(quarto_pandoc_types::inline::Strong {
            content: vec![Inline::Str(Str {
                text: "bold".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "bold");
    }

    #[test]
    fn test_inlines_strikeout() {
        let inlines = vec![Inline::Strikeout(quarto_pandoc_types::inline::Strikeout {
            content: vec![Inline::Str(Str {
                text: "strikeout".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "strikeout");
    }

    #[test]
    fn test_inlines_superscript() {
        let inlines = vec![Inline::Superscript(
            quarto_pandoc_types::inline::Superscript {
                content: vec![Inline::Str(Str {
                    text: "sup".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            },
        )];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "sup");
    }

    #[test]
    fn test_inlines_subscript() {
        let inlines = vec![Inline::Subscript(quarto_pandoc_types::inline::Subscript {
            content: vec![Inline::Str(Str {
                text: "sub".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "sub");
    }

    #[test]
    fn test_inlines_smallcaps() {
        let inlines = vec![Inline::SmallCaps(quarto_pandoc_types::inline::SmallCaps {
            content: vec![Inline::Str(Str {
                text: "smallcaps".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "smallcaps");
    }

    #[test]
    fn test_inlines_quoted_single() {
        let inlines = vec![Inline::Quoted(quarto_pandoc_types::inline::Quoted {
            quote_type: quarto_pandoc_types::inline::QuoteType::SingleQuote,
            content: vec![Inline::Str(Str {
                text: "quoted".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "'quoted'");
    }

    #[test]
    fn test_inlines_quoted_double() {
        let inlines = vec![Inline::Quoted(quarto_pandoc_types::inline::Quoted {
            quote_type: quarto_pandoc_types::inline::QuoteType::DoubleQuote,
            content: vec![Inline::Str(Str {
                text: "quoted".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "\"quoted\"");
    }

    #[test]
    fn test_inlines_cite() {
        let inlines = vec![Inline::Cite(quarto_pandoc_types::inline::Cite {
            citations: vec![],
            content: vec![Inline::Str(Str {
                text: "citation".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "citation");
    }

    #[test]
    fn test_inlines_code() {
        let inlines = vec![Inline::Code(quarto_pandoc_types::inline::Code {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            text: "code()".to_string(),
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "code()");
    }

    #[test]
    fn test_inlines_math() {
        let inlines = vec![Inline::Math(quarto_pandoc_types::inline::Math {
            math_type: quarto_pandoc_types::inline::MathType::InlineMath,
            text: "E=mc^2".to_string(),
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "E=mc^2");
    }

    #[test]
    fn test_inlines_raw_inline() {
        let inlines = vec![Inline::RawInline(quarto_pandoc_types::inline::RawInline {
            format: "html".to_string(),
            text: "<b>bold</b>".to_string(),
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "<b>bold</b>");
    }

    #[test]
    fn test_inlines_link() {
        let inlines = vec![Inline::Link(quarto_pandoc_types::inline::Link {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: "link text".to_string(),
                source_info: dummy_source_info(),
            })],
            target: ("http://example.com".to_string(), String::new()),
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
            target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "link text");
    }

    #[test]
    fn test_inlines_image() {
        let inlines = vec![Inline::Image(quarto_pandoc_types::inline::Image {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: "alt text".to_string(),
                source_info: dummy_source_info(),
            })],
            target: ("image.png".to_string(), String::new()),
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
            target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "alt text");
    }

    #[test]
    fn test_inlines_span() {
        let inlines = vec![Inline::Span(quarto_pandoc_types::inline::Span {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: "span content".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "span content");
    }

    #[test]
    fn test_inlines_insert() {
        let inlines = vec![Inline::Insert(quarto_pandoc_types::inline::Insert {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: "inserted".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "inserted");
    }

    #[test]
    fn test_inlines_delete() {
        let inlines = vec![Inline::Delete(quarto_pandoc_types::inline::Delete {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: "deleted".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "deleted");
    }

    #[test]
    fn test_inlines_highlight() {
        let inlines = vec![Inline::Highlight(quarto_pandoc_types::inline::Highlight {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: "highlighted".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "highlighted");
    }

    #[test]
    fn test_inlines_edit_comment() {
        let inlines = vec![Inline::EditComment(
            quarto_pandoc_types::inline::EditComment {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                content: vec![Inline::Str(Str {
                    text: "comment".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
                attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
            },
        )];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "comment");
    }

    #[test]
    fn test_inlines_note() {
        use quarto_pandoc_types::block::Plain;
        let inlines = vec![Inline::Note(quarto_pandoc_types::inline::Note {
            content: vec![Block::Plain(Plain {
                content: vec![Inline::Str(Str {
                    text: "note content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "note content");
    }

    #[test]
    fn test_inlines_shortcode_skipped() {
        let inlines = vec![
            Inline::Str(Str {
                text: "before".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::Shortcode(quarto_pandoc_types::shortcode::Shortcode {
                name: "test".to_string(),
                is_escaped: false,
                positional_args: vec![],
                keyword_args: std::collections::HashMap::new(),
            }),
            Inline::Str(Str {
                text: "after".to_string(),
                source_info: dummy_source_info(),
            }),
        ];
        let text = inlines_to_plain_text(&inlines);
        assert_eq!(text, "beforeafter");
    }

    // ============================================================================
    // Tests for blocks_to_plain_text - covering various block types
    // ============================================================================

    fn make_str_inline(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })
    }

    #[test]
    fn test_blocks_plain() {
        use quarto_pandoc_types::block::Plain;
        let blocks = vec![Block::Plain(Plain {
            content: vec![make_str_inline("plain text")],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "plain text");
    }

    #[test]
    fn test_blocks_paragraph() {
        use quarto_pandoc_types::block::Paragraph;
        let blocks = vec![Block::Paragraph(Paragraph {
            content: vec![make_str_inline("paragraph text")],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "paragraph text");
    }

    #[test]
    fn test_blocks_line_block() {
        use quarto_pandoc_types::block::LineBlock;
        let blocks = vec![Block::LineBlock(LineBlock {
            content: vec![
                vec![make_str_inline("line1")],
                vec![make_str_inline("line2")],
            ],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "line1\nline2\n");
    }

    #[test]
    fn test_blocks_code_block() {
        use quarto_pandoc_types::block::CodeBlock;
        let blocks = vec![Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            text: "fn main() {}".to_string(),
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "fn main() {}");
    }

    #[test]
    fn test_blocks_raw_block() {
        use quarto_pandoc_types::block::RawBlock;
        let blocks = vec![Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text: "<div>raw content</div>".to_string(),
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "<div>raw content</div>");
    }

    #[test]
    fn test_blocks_block_quote() {
        use quarto_pandoc_types::block::{BlockQuote, Plain};
        let blocks = vec![Block::BlockQuote(BlockQuote {
            content: vec![Block::Plain(Plain {
                content: vec![make_str_inline("quoted")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "quoted");
    }

    #[test]
    fn test_blocks_ordered_list() {
        use quarto_pandoc_types::block::{OrderedList, Plain};
        use quarto_pandoc_types::list::{ListNumberDelim, ListNumberStyle};
        let blocks = vec![Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![
                vec![Block::Plain(Plain {
                    content: vec![make_str_inline("item1")],
                    source_info: dummy_source_info(),
                })],
                vec![Block::Plain(Plain {
                    content: vec![make_str_inline("item2")],
                    source_info: dummy_source_info(),
                })],
            ],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "item1\nitem2\n");
    }

    #[test]
    fn test_blocks_bullet_list() {
        use quarto_pandoc_types::block::{BulletList, Plain};
        let blocks = vec![Block::BulletList(BulletList {
            content: vec![
                vec![Block::Plain(Plain {
                    content: vec![make_str_inline("bullet1")],
                    source_info: dummy_source_info(),
                })],
                vec![Block::Plain(Plain {
                    content: vec![make_str_inline("bullet2")],
                    source_info: dummy_source_info(),
                })],
            ],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "bullet1\nbullet2\n");
    }

    #[test]
    fn test_blocks_definition_list() {
        use quarto_pandoc_types::block::{DefinitionList, Plain};
        let blocks = vec![Block::DefinitionList(DefinitionList {
            content: vec![(
                vec![make_str_inline("term")],
                vec![vec![Block::Plain(Plain {
                    content: vec![make_str_inline("definition")],
                    source_info: dummy_source_info(),
                })]],
            )],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "term\ndefinition\n");
    }

    #[test]
    fn test_blocks_header() {
        use quarto_pandoc_types::block::Header;
        let blocks = vec![Block::Header(Header {
            level: 1,
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![make_str_inline("heading")],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "heading");
    }

    #[test]
    fn test_blocks_div() {
        use quarto_pandoc_types::block::{Div, Plain};
        let blocks = vec![Block::Div(Div {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![Block::Plain(Plain {
                content: vec![make_str_inline("div content")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "div content");
    }

    #[test]
    fn test_blocks_table_with_caption() {
        use quarto_pandoc_types::caption::Caption;
        use quarto_pandoc_types::table::{
            Alignment, ColWidth, Table, TableBody, TableFoot, TableHead,
        };
        let blocks = vec![Block::Table(Table {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            caption: Caption {
                short: Some(vec![make_str_inline("table caption")]),
                long: None,
                source_info: dummy_source_info(),
            },
            colspec: vec![(Alignment::Default, ColWidth::Default)],
            head: TableHead {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                rows: vec![],
                source_info: dummy_source_info(),
                attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
            },
            bodies: vec![TableBody {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                rowhead_columns: 0,
                head: vec![],
                body: vec![],
                source_info: dummy_source_info(),
                attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
            }],
            foot: TableFoot {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                rows: vec![],
                source_info: dummy_source_info(),
                attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
            },
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "table caption");
    }

    #[test]
    fn test_blocks_figure_with_caption() {
        use quarto_pandoc_types::block::Figure;
        use quarto_pandoc_types::caption::Caption;
        let blocks = vec![Block::Figure(Figure {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            caption: Caption {
                short: Some(vec![make_str_inline("figure caption")]),
                long: None,
                source_info: dummy_source_info(),
            },
            content: vec![],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "figure caption");
    }

    #[test]
    fn test_blocks_horizontal_rule_skipped() {
        use quarto_pandoc_types::block::HorizontalRule;
        let blocks = vec![Block::HorizontalRule(HorizontalRule {
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "");
    }

    #[test]
    fn test_blocks_multiple_with_newlines() {
        use quarto_pandoc_types::block::{Paragraph, Plain};
        let blocks = vec![
            Block::Plain(Plain {
                content: vec![make_str_inline("first")],
                source_info: dummy_source_info(),
            }),
            Block::Paragraph(Paragraph {
                content: vec![make_str_inline("second")],
                source_info: dummy_source_info(),
            }),
        ];
        let text = blocks_to_plain_text(&blocks);
        assert_eq!(text, "first\nsecond");
    }

    // ============================================================================
    // Tests for edge cases and extract_plain_text
    // ============================================================================

    #[test]
    fn test_extract_plain_text_from_blocks() {
        use quarto_pandoc_types::block::Plain;
        let meta = ConfigValue::new_blocks(
            vec![Block::Plain(Plain {
                content: vec![make_str_inline("block text")],
                source_info: dummy_source_info(),
            })],
            dummy_source_info(),
        );
        let result = extract_plain_text(&meta);
        assert_eq!(result, Some("block text".to_string()));
    }

    #[test]
    fn test_extract_plain_text_returns_none_for_map() {
        let meta = ConfigValue::new_map(vec![], dummy_source_info());
        let result = extract_plain_text(&meta);
        assert!(result.is_none());
    }

    #[test]
    fn test_normalize_metadata_non_map() {
        // Test that normalize_metadata handles non-Map values gracefully
        let mut meta = ConfigValue::new_string("just a string", dummy_source_info());
        normalize_metadata(&mut meta);
        // Should not panic or change the value
        assert_eq!(meta.as_str(), Some("just a string"));
    }

    #[test]
    fn test_normalize_metadata_no_title() {
        // Test that normalize_metadata handles metadata without a title
        let mut meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "author".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("John Doe", dummy_source_info()),
            }],
            dummy_source_info(),
        );
        normalize_metadata(&mut meta);

        // Should not add pagetitle when there's no title
        if let ConfigValueKind::Map(entries) = &meta.value {
            let has_pagetitle = entries.iter().any(|e| e.key == "pagetitle");
            assert!(!has_pagetitle);
        }
    }

    #[test]
    fn test_default_trait() {
        let _transform: MetadataNormalizeTransform = Default::default();
    }
}
