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

use quarto_pandoc_types::Slot;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::meta::{MetaMapEntry, MetaValueWithSourceInfo};
use quarto_pandoc_types::pandoc::Pandoc;

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
fn normalize_metadata(meta: &mut MetaValueWithSourceInfo) {
    // Only process if meta is a MetaMap
    let MetaValueWithSourceInfo::MetaMap {
        entries,
        source_info,
    } = meta
    else {
        return;
    };

    // Add pagetitle if not present
    add_pagetitle_if_missing(entries, source_info.clone());
}

/// Add `pagetitle` field derived from `title` if not already present.
fn add_pagetitle_if_missing(
    entries: &mut Vec<MetaMapEntry>,
    source_info: quarto_source_map::SourceInfo,
) {
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
            entries.push(MetaMapEntry {
                key: "pagetitle".to_string(),
                key_source: source_info.clone(),
                value: MetaValueWithSourceInfo::MetaString {
                    value: text,
                    source_info,
                },
            });
        }
    }
}

/// Extract plain text from a metadata value.
fn extract_plain_text(meta: &MetaValueWithSourceInfo) -> Option<String> {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => Some(value.clone()),
        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
            Some(inlines_to_plain_text(content))
        }
        MetaValueWithSourceInfo::MetaBlocks { content, .. } => Some(blocks_to_plain_text(content)),
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
    use quarto_source_map::{FileId, Location, Range, SourceInfo};
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
            meta: MetaValueWithSourceInfo::MetaMap {
                entries: vec![MetaMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaString {
                        value: "My Document".to_string(),
                        source_info: dummy_source_info(),
                    },
                }],
                source_info: dummy_source_info(),
            },
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
        if let MetaValueWithSourceInfo::MetaMap { entries, .. } = &ast.meta {
            let pagetitle = entries.iter().find(|e| e.key == "pagetitle");
            assert!(pagetitle.is_some());
            if let Some(entry) = pagetitle {
                if let MetaValueWithSourceInfo::MetaString { value, .. } = &entry.value {
                    assert_eq!(value, "My Document");
                } else {
                    panic!("Expected MetaString for pagetitle");
                }
            }
        } else {
            panic!("Expected MetaMap");
        }
    }

    #[test]
    fn test_adds_pagetitle_from_inlines_title() {
        let mut ast = Pandoc {
            meta: MetaValueWithSourceInfo::MetaMap {
                entries: vec![MetaMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: MetaValueWithSourceInfo::MetaInlines {
                        content: vec![Inline::Str(Str {
                            text: "Inline Title".to_string(),
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    },
                }],
                source_info: dummy_source_info(),
            },
            blocks: vec![],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = MetadataNormalizeTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        if let MetaValueWithSourceInfo::MetaMap { entries, .. } = &ast.meta {
            let pagetitle = entries.iter().find(|e| e.key == "pagetitle");
            assert!(pagetitle.is_some());
            if let Some(entry) = pagetitle {
                if let MetaValueWithSourceInfo::MetaString { value, .. } = &entry.value {
                    assert_eq!(value, "Inline Title");
                } else {
                    panic!("Expected MetaString for pagetitle");
                }
            }
        }
    }

    #[test]
    fn test_preserves_existing_pagetitle() {
        let mut ast = Pandoc {
            meta: MetaValueWithSourceInfo::MetaMap {
                entries: vec![
                    MetaMapEntry {
                        key: "title".to_string(),
                        key_source: dummy_source_info(),
                        value: MetaValueWithSourceInfo::MetaString {
                            value: "My Document".to_string(),
                            source_info: dummy_source_info(),
                        },
                    },
                    MetaMapEntry {
                        key: "pagetitle".to_string(),
                        key_source: dummy_source_info(),
                        value: MetaValueWithSourceInfo::MetaString {
                            value: "Custom Page Title".to_string(),
                            source_info: dummy_source_info(),
                        },
                    },
                ],
                source_info: dummy_source_info(),
            },
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
        if let MetaValueWithSourceInfo::MetaMap { entries, .. } = &ast.meta {
            let pagetitle_entries: Vec<_> =
                entries.iter().filter(|e| e.key == "pagetitle").collect();
            assert_eq!(pagetitle_entries.len(), 1);
            if let MetaValueWithSourceInfo::MetaString { value, .. } = &pagetitle_entries[0].value {
                assert_eq!(value, "Custom Page Title");
            }
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
}
