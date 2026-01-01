/*
 * title_block.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that adds a title header from metadata if not present.
 */

//! Title block transform.
//!
//! This transform ensures the document has a visible title by:
//! 1. Checking if there's an existing level-1 header in the document
//! 2. If not, prepending a level-1 header from the `title` metadata
//!
//! This is a simplified version of Quarto's title block handling for
//! prototyping purposes.

use quarto_pandoc_types::attr::{AttrSourceInfo, empty_attr};
use quarto_pandoc_types::block::{Block, Header};
use quarto_pandoc_types::inline::{Inline, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that adds a title header from metadata if not present.
///
/// If the document has no level-1 header but has a `title` in metadata,
/// this transform prepends a level-1 header with the title text.
pub struct TitleBlockTransform;

impl TitleBlockTransform {
    /// Create a new title block transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for TitleBlockTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl AstTransform for TitleBlockTransform {
    fn name(&self) -> &str {
        "title-block"
    }

    fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        // Check if there's already a level-1 header
        if has_level1_header(&ast.blocks) {
            return Ok(());
        }

        // Try to get title from metadata
        if let Some(title_text) = extract_title(&ast.meta) {
            // Create a level-1 header with the title
            let header = create_title_header(&title_text);
            ast.blocks.insert(0, header);
        }

        Ok(())
    }
}

/// Check if the document has any level-1 header.
fn has_level1_header(blocks: &[Block]) -> bool {
    blocks
        .iter()
        .any(|block| matches!(block, Block::Header(h) if h.level == 1))
}

/// Extract title text from metadata.
fn extract_title(meta: &ConfigValue) -> Option<String> {
    let ConfigValueKind::Map(entries) = &meta.value else {
        return None;
    };

    let title_entry = entries.iter().find(|e| e.key == "title")?;
    extract_plain_text(&title_entry.value)
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
            Inline::Strong(s) => result.push_str(&inlines_to_plain_text(&s.content)),
            Inline::Code(c) => result.push_str(&c.text),
            Inline::Link(l) => result.push_str(&inlines_to_plain_text(&l.content)),
            Inline::Span(s) => result.push_str(&inlines_to_plain_text(&s.content)),
            _ => {}
        }
    }
    result
}

/// Convert blocks to plain text (simplified).
fn blocks_to_plain_text(blocks: &[Block]) -> String {
    let mut result = String::new();
    for block in blocks {
        match block {
            Block::Plain(p) => result.push_str(&inlines_to_plain_text(&p.content)),
            Block::Paragraph(p) => result.push_str(&inlines_to_plain_text(&p.content)),
            _ => {}
        }
    }
    result
}

/// Create a level-1 header block with the given title.
fn create_title_header(title: &str) -> Block {
    Block::Header(Header {
        level: 1,
        attr: empty_attr(),
        content: vec![Inline::Str(Str {
            text: title.to_string(),
            source_info: SourceInfo::default(),
        })],
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::block::Paragraph;
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
    fn test_adds_title_header_when_missing() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![ConfigMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("My Document", dummy_source_info()),
                }],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TitleBlockTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should now have 2 blocks: header + paragraph
        assert_eq!(ast.blocks.len(), 2);

        // First block should be the title header
        match &ast.blocks[0] {
            Block::Header(h) => {
                assert_eq!(h.level, 1);
                match &h.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "My Document"),
                    _ => panic!("Expected Str inline"),
                }
            }
            _ => panic!("Expected Header block"),
        }
    }

    #[test]
    fn test_does_not_add_when_h1_exists() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![ConfigMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("My Document", dummy_source_info()),
                }],
                dummy_source_info(),
            ),
            blocks: vec![
                Block::Header(Header {
                    level: 1,
                    attr: empty_attr(),
                    content: vec![Inline::Str(Str {
                        text: "Existing Title".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                }),
                Block::Paragraph(Paragraph {
                    content: vec![Inline::Str(Str {
                        text: "Content".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                }),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TitleBlockTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should still have 2 blocks (no new header added)
        assert_eq!(ast.blocks.len(), 2);

        // First block should be the existing header
        match &ast.blocks[0] {
            Block::Header(h) => match &h.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "Existing Title"),
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Header block"),
        }
    }

    #[test]
    fn test_does_nothing_without_title_metadata() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(vec![], dummy_source_info()),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TitleBlockTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should still have 1 block (no header added)
        assert_eq!(ast.blocks.len(), 1);
    }

    #[test]
    fn test_transform_name() {
        let transform = TitleBlockTransform::new();
        assert_eq!(transform.name(), "title-block");
    }
}
