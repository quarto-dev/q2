/*
 * appendix.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that consolidates appendix content into a single appendix container.
 */

//! Appendix structure transform for HTML rendering.
//!
//! This transform collects various appendix-related content and consolidates it
//! into a single appendix container at the end of the document. It runs in the
//! **finalization phase** of the pipeline, after FootnotesTransform and CiteprocTransform.
//!
//! ## Input
//!
//! - Div blocks with class `appendix` (user-defined appendix sections)
//! - Footnotes section (from FootnotesTransform, id="footnotes")
//! - Bibliography (from CiteprocTransform when implemented, id="refs")
//! - License/copyright/citation metadata
//!
//! ## Output
//!
//! A consolidated appendix container at end of document:
//!
//! ```html
//! <div id="quarto-appendix" class="default">
//!   <!-- User appendix sections -->
//!   <!-- Bibliography (if present and not margin) -->
//!   <!-- Footnotes (if present and not margin) -->
//!   <!-- License section (if metadata present) -->
//!   <!-- Copyright section (if metadata present) -->
//!   <!-- Citation section (if metadata present) -->
//! </div>
//! ```
//!
//! ## Configuration
//!
//! - `appendix-style`: Controls appendix processing
//!   - `default` (default): Standard appendix processing
//!   - `plain`: Minimal appendix styling
//!   - `none`: Disable appendix processing
//!
//! - `reference-location`: If `margin`, footnotes are NOT moved into appendix
//! - `citation-location`: If `margin`, bibliography is NOT moved into appendix

use hashlink::LinkedHashMap;
use quarto_pandoc_types::Blocks;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div, Header, Paragraph};
use quarto_pandoc_types::inline::{Inline, Link, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::{AppendixStyle, ReferenceLocation};

/// Transform that consolidates appendix content into a single container.
///
/// This transform is part of the **finalization phase** and runs late in the
/// pipeline, after FootnotesTransform and CiteprocTransform have created their
/// respective sections.
pub struct AppendixStructureTransform;

impl AppendixStructureTransform {
    /// Create a new appendix structure transform.
    pub fn new() -> Self {
        Self
    }

    /// Get the appendix-style configuration.
    fn get_appendix_style(&self, ctx: &RenderContext) -> AppendixStyle {
        ctx.format_metadata("appendix-style")
            .map(|v| {
                if let Some(b) = v.as_bool() {
                    AppendixStyle::from_bool(b)
                } else if let Some(s) = v.as_str() {
                    AppendixStyle::from_str(s)
                } else {
                    AppendixStyle::default()
                }
            })
            .unwrap_or_default()
    }

    /// Get the reference-location configuration.
    fn get_reference_location(&self, ctx: &RenderContext) -> ReferenceLocation {
        ctx.format_metadata("reference-location")
            .and_then(|v| v.as_str())
            .map(ReferenceLocation::from_str)
            .unwrap_or_default()
    }

    /// Check if this is a book format (appendix processing is skipped for books).
    fn is_book_format(&self, ctx: &RenderContext) -> bool {
        ctx.format_metadata("book")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

impl Default for AppendixStructureTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl AstTransform for AppendixStructureTransform {
    fn name(&self) -> &str {
        "appendix-structure"
    }

    fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let appendix_style = self.get_appendix_style(ctx);
        let reference_location = self.get_reference_location(ctx);

        // Skip appendix processing if disabled or book format
        if !appendix_style.is_enabled() || self.is_book_format(ctx) {
            return Ok(());
        }

        // Collect appendix sections
        let mut appendix_sections: Blocks = Vec::new();

        // 1. Collect user-defined appendix sections (Divs with class "appendix")
        let user_appendices = extract_appendix_divs(&mut ast.blocks);
        appendix_sections.extend(user_appendices);

        // 2. Collect bibliography (if not margin mode)
        // For now, look for Div with id="refs" - CiteprocTransform will create this later
        if reference_location != ReferenceLocation::Margin {
            if let Some(bibliography) = extract_bibliography(&mut ast.blocks) {
                appendix_sections.push(wrap_bibliography(bibliography));
            }
        }

        // 3. Collect footnotes section (if not margin mode)
        if reference_location != ReferenceLocation::Margin {
            if let Some(footnotes) = extract_footnotes(&mut ast.blocks) {
                appendix_sections.push(footnotes);
            }
        }

        // 4. Create metadata-driven sections
        // License section
        if let Some(license_section) = create_license_section(ctx) {
            appendix_sections.push(license_section);
        }

        // Copyright section
        if let Some(copyright_section) = create_copyright_section(ctx) {
            appendix_sections.push(copyright_section);
        }

        // Citation section
        if let Some(citation_section) = create_citation_section(ctx) {
            appendix_sections.push(citation_section);
        }

        // Only create appendix container if we have content
        if !appendix_sections.is_empty() {
            let appendix_class = appendix_style.as_str().to_string();
            let appendix_container = create_appendix_container(appendix_sections, &appendix_class);
            ast.blocks.push(appendix_container);
        }

        Ok(())
    }
}

/// Extract Div blocks with class "appendix" from the document.
fn extract_appendix_divs(blocks: &mut Vec<Block>) -> Blocks {
    let mut appendix_divs = Vec::new();

    blocks.retain(|block| {
        if let Block::Div(div) = block {
            if div.attr.1.contains(&"appendix".to_string()) {
                appendix_divs.push(block.clone());
                return false; // Remove from original position
            }
        }
        true
    });

    appendix_divs
}

/// Extract the bibliography block (Div with id="refs" or class="references").
fn extract_bibliography(blocks: &mut Vec<Block>) -> Option<Block> {
    let mut bibliography = None;

    blocks.retain(|block| {
        if let Block::Div(div) = block {
            // Check for id="refs" or class="references"
            if div.attr.0 == "refs" || div.attr.1.contains(&"references".to_string()) {
                bibliography = Some(block.clone());
                return false; // Remove from original position
            }
        }
        true
    });

    bibliography
}

/// Extract the footnotes section (Div with id="footnotes").
fn extract_footnotes(blocks: &mut Vec<Block>) -> Option<Block> {
    let mut footnotes = None;

    blocks.retain(|block| {
        if let Block::Div(div) = block {
            if div.attr.0 == "footnotes" {
                footnotes = Some(block.clone());
                return false; // Remove from original position
            }
        }
        true
    });

    footnotes
}

/// Wrap bibliography in a section with appropriate attributes.
fn wrap_bibliography(bibliography: Block) -> Block {
    let source_info = SourceInfo::default();

    // Create header for the bibliography section
    let header = Block::Header(Header {
        level: 2,
        attr: (String::new(), Vec::new(), LinkedHashMap::new()),
        content: vec![Inline::Str(Str {
            text: "References".to_string(),
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });

    Block::Div(Div {
        attr: (
            "quarto-bibliography".to_string(),
            vec!["section".to_string()],
            LinkedHashMap::from_iter([("role".to_string(), "doc-bibliography".to_string())]),
        ),
        content: vec![header, bibliography],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Create the appendix container div.
fn create_appendix_container(sections: Blocks, style_class: &str) -> Block {
    Block::Div(Div {
        attr: (
            "quarto-appendix".to_string(),
            vec![style_class.to_string()],
            LinkedHashMap::new(),
        ),
        content: sections,
        source_info: SourceInfo::default(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Create license section from metadata.
fn create_license_section(ctx: &RenderContext) -> Option<Block> {
    let license = ctx.format_metadata("license")?;

    // License can be a string (e.g., "CC BY") or an object with more details
    let license_text = if let Some(s) = license.as_str() {
        s.to_string()
    } else if let Some(obj) = license.as_object() {
        // Try to get "text" or "type" field
        obj.get("text")
            .or_else(|| obj.get("type"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?
    } else {
        return None;
    };

    let source_info = SourceInfo::default();

    let header = Block::Header(Header {
        level: 2,
        attr: (String::new(), Vec::new(), LinkedHashMap::new()),
        content: vec![Inline::Str(Str {
            text: "Reuse".to_string(),
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });

    let content = Block::Paragraph(Paragraph {
        content: vec![Inline::Str(Str {
            text: license_text,
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
    });

    Some(Block::Div(Div {
        attr: (
            "quarto-reuse".to_string(),
            vec!["section".to_string()],
            LinkedHashMap::new(),
        ),
        content: vec![header, content],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    }))
}

/// Create copyright section from metadata.
fn create_copyright_section(ctx: &RenderContext) -> Option<Block> {
    let copyright = ctx.format_metadata("copyright")?;

    // Copyright can be a string or an object
    let copyright_text = if let Some(s) = copyright.as_str() {
        s.to_string()
    } else if let Some(obj) = copyright.as_object() {
        // Try to get "holder" or "statement" field
        obj.get("statement")
            .or_else(|| obj.get("holder"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?
    } else {
        return None;
    };

    let source_info = SourceInfo::default();

    let header = Block::Header(Header {
        level: 2,
        attr: (String::new(), Vec::new(), LinkedHashMap::new()),
        content: vec![Inline::Str(Str {
            text: "Copyright".to_string(),
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });

    let content = Block::Paragraph(Paragraph {
        content: vec![Inline::Str(Str {
            text: copyright_text,
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
    });

    Some(Block::Div(Div {
        attr: (
            "quarto-copyright".to_string(),
            vec!["section".to_string()],
            LinkedHashMap::new(),
        ),
        content: vec![header, content],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    }))
}

/// Create citation section from metadata.
fn create_citation_section(ctx: &RenderContext) -> Option<Block> {
    let citation = ctx.format_metadata("citation")?;

    // Citation metadata typically includes how to cite this document
    // It can have various formats - for now, look for a "url" or create a simple reference
    let citation_url = citation
        .as_object()
        .and_then(|obj| obj.get("url"))
        .and_then(|v| v.as_str());

    let source_info = SourceInfo::default();

    let header = Block::Header(Header {
        level: 2,
        attr: (String::new(), Vec::new(), LinkedHashMap::new()),
        content: vec![Inline::Str(Str {
            text: "Citation".to_string(),
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });

    // Create citation content based on what's available
    let content_inlines = if let Some(url) = citation_url {
        vec![
            Inline::Str(Str {
                text: "For attribution, please cite this work as: ".to_string(),
                source_info: source_info.clone(),
            }),
            Inline::Link(Link {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                content: vec![Inline::Str(Str {
                    text: url.to_string(),
                    source_info: source_info.clone(),
                })],
                target: (url.to_string(), String::new()),
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
                target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
            }),
        ]
    } else {
        // If no URL, just note that citation info is available
        vec![Inline::Str(Str {
            text: "Please cite this work appropriately.".to_string(),
            source_info: source_info.clone(),
        })]
    };

    let content = Block::Paragraph(Paragraph {
        content: content_inlines,
        source_info: source_info.clone(),
    });

    Some(Block::Div(Div {
        attr: (
            "quarto-citation".to_string(),
            vec!["section".to_string()],
            LinkedHashMap::new(),
        ),
        content: vec![header, content],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::Plain;
    use quarto_source_map::{FileId, Location, Range};

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::BinaryDependencies;

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
            dir: std::path::PathBuf::from("/project"),
            config: None,
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
        }
    }

    fn make_str(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })
    }

    fn make_appendix_div(id: &str, content: &str) -> Block {
        Block::Div(Div {
            attr: (
                id.to_string(),
                vec!["appendix".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str(content)],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn make_footnotes_section() -> Block {
        Block::Div(Div {
            attr: (
                "footnotes".to_string(),
                vec!["footnotes".to_string(), "section".to_string()],
                LinkedHashMap::from_iter([("role".to_string(), "doc-endnotes".to_string())]),
            ),
            content: vec![Block::Plain(Plain {
                content: vec![make_str("Footnote content")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn make_bibliography() -> Block {
        Block::Div(Div {
            attr: (
                "refs".to_string(),
                vec!["references".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Block::Plain(Plain {
                content: vec![make_str("Bibliography entries")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    #[test]
    fn test_transform_name() {
        let transform = AppendixStructureTransform::new();
        assert_eq!(transform.name(), "appendix-structure");
    }

    #[test]
    fn test_no_appendix_content() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("Regular content")],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // No appendix should be created if no appendix content
        assert_eq!(ast.blocks.len(), 1);
        assert!(matches!(ast.blocks[0], Block::Paragraph(_)));
    }

    #[test]
    fn test_user_appendix_sections() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_appendix_div("appendix-a", "Appendix A content"),
                make_appendix_div("appendix-b", "Appendix B content"),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have main content + appendix container
        assert_eq!(ast.blocks.len(), 2);

        // Check appendix container
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "quarto-appendix");
            // Should contain both appendix sections
            assert_eq!(div.content.len(), 2);
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[test]
    fn test_footnotes_moved_to_appendix() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_footnotes_section(),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have main content + appendix container (footnotes moved into it)
        assert_eq!(ast.blocks.len(), 2);

        // Check appendix container contains footnotes
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "quarto-appendix");
            assert_eq!(div.content.len(), 1);

            // First item should be the footnotes section
            if let Block::Div(footnotes) = &div.content[0] {
                assert_eq!(footnotes.attr.0, "footnotes");
            } else {
                panic!("Expected footnotes Div in appendix");
            }
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[test]
    fn test_bibliography_moved_to_appendix() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_bibliography(),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have main content + appendix container
        assert_eq!(ast.blocks.len(), 2);

        // Check appendix container contains wrapped bibliography
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "quarto-appendix");
            assert_eq!(div.content.len(), 1);

            // First item should be the wrapped bibliography section
            if let Block::Div(bib_section) = &div.content[0] {
                assert_eq!(bib_section.attr.0, "quarto-bibliography");
            } else {
                panic!("Expected bibliography section Div in appendix");
            }
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[test]
    fn test_appendix_section_ordering() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                // Add in wrong order to test ordering
                make_footnotes_section(),
                make_bibliography(),
                make_appendix_div("appendix-a", "User appendix"),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Check ordering: user appendix → bibliography → footnotes
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.content.len(), 3);

            // 1. User appendix
            if let Block::Div(user) = &div.content[0] {
                assert!(user.attr.1.contains(&"appendix".to_string()));
            } else {
                panic!("First item should be user appendix");
            }

            // 2. Bibliography
            if let Block::Div(bib) = &div.content[1] {
                assert_eq!(bib.attr.0, "quarto-bibliography");
            } else {
                panic!("Second item should be bibliography");
            }

            // 3. Footnotes
            if let Block::Div(footnotes) = &div.content[2] {
                assert_eq!(footnotes.attr.0, "footnotes");
            } else {
                panic!("Third item should be footnotes");
            }
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[test]
    fn test_appendix_style_none_skips_processing() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_appendix_div("appendix-a", "User appendix"),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html().with_metadata(serde_json::json!({
            "appendix-style": "none"
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Blocks should be unchanged - appendix div stays in place
        assert_eq!(ast.blocks.len(), 2);
        // The appendix div should still be there, not moved
        if let Block::Div(div) = &ast.blocks[1] {
            assert!(div.attr.1.contains(&"appendix".to_string()));
            assert_ne!(div.attr.0, "quarto-appendix"); // NOT the container
        }
    }

    #[test]
    fn test_margin_mode_footnotes_not_moved() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_footnotes_section(),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html().with_metadata(serde_json::json!({
            "reference-location": "margin"
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Footnotes should stay in place, no appendix created
        assert_eq!(ast.blocks.len(), 2);
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "footnotes");
        } else {
            panic!("Footnotes should remain in place");
        }
    }

    #[test]
    fn test_license_metadata_creates_section() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("Main content")],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html().with_metadata(serde_json::json!({
            "license": "CC BY 4.0"
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have main content + appendix with license section
        assert_eq!(ast.blocks.len(), 2);

        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "quarto-appendix");
            assert_eq!(div.content.len(), 1);

            if let Block::Div(license) = &div.content[0] {
                assert_eq!(license.attr.0, "quarto-reuse");
            } else {
                panic!("Expected license section");
            }
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[test]
    fn test_appendix_style_plain() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_appendix_div("appendix-a", "User appendix"),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html().with_metadata(serde_json::json!({
            "appendix-style": "plain"
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Check that appendix container has "plain" class
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "quarto-appendix");
            assert!(div.attr.1.contains(&"plain".to_string()));
        } else {
            panic!("Expected appendix Div");
        }
    }
}
