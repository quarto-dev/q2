/*
 * format_writers.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Format-specific writers for template context building.
//!
//! This module provides a trait for format-specific AST-to-string conversion,
//! and implementations for HTML output.

use anyhow::Result;
use pampa::pandoc::block::Block;
use pampa::pandoc::inline::Inlines;

/// Format-specific writers for converting Pandoc AST to strings.
///
/// Implementations of this trait provide the format-specific rendering
/// needed when converting document metadata to template values.
pub trait FormatWriters {
    /// Write blocks to a string.
    fn write_blocks(&self, blocks: &[Block]) -> Result<String>;

    /// Write inlines to a string.
    fn write_inlines(&self, inlines: &Inlines) -> Result<String>;
}

/// HTML format writers.
///
/// Uses the HTML writer from pampa to convert
/// Pandoc AST nodes to HTML strings.
pub struct HtmlWriters;

impl FormatWriters for HtmlWriters {
    fn write_blocks(&self, blocks: &[Block]) -> Result<String> {
        let mut buf = Vec::new();
        pampa::writers::html::write_blocks(blocks, &mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn write_inlines(&self, inlines: &Inlines) -> Result<String> {
        let mut buf = Vec::new();
        pampa::writers::html::write_inlines(inlines, &mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pampa::pandoc::Inline;
    use pampa::pandoc::block::Paragraph;
    use pampa::pandoc::inline::{Emph, Space, Str};

    fn dummy_source_info() -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::from_range(
            quarto_source_map::FileId(0),
            quarto_source_map::Range {
                start: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: quarto_source_map::Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    #[test]
    fn test_html_writers_inlines() {
        let writers = HtmlWriters;
        let inlines = vec![
            Inline::Str(Str {
                text: "Hello".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::Space(Space {
                source_info: dummy_source_info(),
            }),
            Inline::Emph(Emph {
                content: vec![Inline::Str(Str {
                    text: "world".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            }),
        ];

        let result = writers.write_inlines(&inlines).unwrap();
        assert_eq!(result, "Hello <em>world</em>");
    }

    #[test]
    fn test_html_writers_blocks() {
        let writers = HtmlWriters;
        let blocks = vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "A paragraph.".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];

        let result = writers.write_blocks(&blocks).unwrap();
        assert_eq!(result, "<p>A paragraph.</p>\n");
    }
}
