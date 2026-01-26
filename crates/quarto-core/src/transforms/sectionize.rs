/*
 * sectionize.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that wraps headers in section Divs.
 */

//! Sectionize transform for HTML rendering.
//!
//! This transform wraps headers and their following content in section Divs,
//! analogous to Pandoc's `--section-divs` option. It delegates to the
//! implementation in `pampa::transforms::sectionize_blocks`.
//!
//! For HTML output, this is always enabled (matching TS Quarto's Bootstrap
//! HTML format behavior).

use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that wraps headers in section Divs.
///
/// This transform is enabled by default for HTML output, matching TS Quarto's
/// Bootstrap HTML format behavior where `section-divs: true` is always set.
///
/// The implementation delegates to `pampa::transforms::sectionize_blocks`,
/// which handles the actual AST transformation.
pub struct SectionizeTransform;

impl SectionizeTransform {
    /// Create a new sectionize transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for SectionizeTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl AstTransform for SectionizeTransform {
    fn name(&self) -> &str {
        "sectionize"
    }

    fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        ast.blocks = pampa::transforms::sectionize_blocks(std::mem::take(&mut ast.blocks));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::block::{Block, Header, Paragraph};
    use quarto_pandoc_types::inline::{Inline, Str};
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::default()
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
    fn test_transform_name() {
        let transform = SectionizeTransform::new();
        assert_eq!(transform.name(), "sectionize");
    }

    #[test]
    fn test_transform_wraps_headers_in_sections() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Header(Header {
                    level: 2,
                    attr: ("sec-a".to_string(), vec![], hashlink::LinkedHashMap::new()),
                    content: vec![Inline::Str(Str {
                        text: "Section A".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                    attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
                }),
                Block::Paragraph(Paragraph {
                    content: vec![Inline::Str(Str {
                        text: "Content.".to_string(),
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

        let transform = SectionizeTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have one section Div
        assert_eq!(ast.blocks.len(), 1);
        let Block::Div(div) = &ast.blocks[0] else {
            panic!("Expected Div, got {:?}", ast.blocks[0]);
        };

        // Section should have the ID and section class
        assert_eq!(div.attr.0, "sec-a");
        assert!(div.attr.1.contains(&"section".to_string()));
        assert!(div.attr.1.contains(&"level2".to_string()));

        // Section should contain header and paragraph
        assert_eq!(div.content.len(), 2);
    }

    #[test]
    fn test_default_trait() {
        let _transform: SectionizeTransform = Default::default();
    }
}
