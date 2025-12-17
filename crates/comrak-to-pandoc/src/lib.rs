/*
 * lib.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Convert comrak's CommonMark AST to quarto-pandoc-types AST.
 *
 * This crate provides direct conversion from comrak's arena-based AST
 * to our owned Pandoc AST structures. Only the CommonMark subset is
 * supported; GFM extensions will panic.
 */

mod block;
mod compare;
mod inline;
pub mod source_location;
mod text;

pub mod normalize;

pub use block::{convert_document, convert_document_with_source};
pub use compare::ast_eq_ignore_source;
pub use normalize::normalize;
pub use source_location::SourceLocationContext;

use hashlink::LinkedHashMap;
use quarto_pandoc_types::Attr;
use quarto_source_map::{FileId, SourceInfo};

/// Create an empty source info (we ignore source locations in this converter).
pub(crate) fn empty_source_info() -> SourceInfo {
    SourceInfo::original(FileId(0), 0, 0)
}

/// Create an empty attribute tuple.
pub(crate) fn empty_attr() -> Attr {
    (String::new(), vec![], LinkedHashMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{Arena, Options, parse_document};

    fn parse_comrak(markdown: &str) -> quarto_pandoc_types::Pandoc {
        let arena = Arena::new();
        // Pure CommonMark, no GFM extensions (default is CommonMark-only)
        let options = Options::default();
        let root = parse_document(&arena, markdown, &options);
        convert_document(root)
    }

    #[test]
    fn test_simple_paragraph() {
        let md = "Hello world.\n";
        let pandoc = parse_comrak(md);
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            quarto_pandoc_types::Block::Paragraph(p) => {
                // Should be: Str("Hello"), Space, Str("world.")
                assert_eq!(p.content.len(), 3);
            }
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_heading() {
        let md = "# Hello\n";
        let pandoc = parse_comrak(md);
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            quarto_pandoc_types::Block::Header(h) => {
                assert_eq!(h.level, 1);
            }
            _ => panic!("Expected Header"),
        }
    }

    #[test]
    fn test_emphasis() {
        let md = "*hello*\n";
        let pandoc = parse_comrak(md);
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            quarto_pandoc_types::Block::Paragraph(p) => {
                assert_eq!(p.content.len(), 1);
                match &p.content[0] {
                    quarto_pandoc_types::Inline::Emph(e) => {
                        assert_eq!(e.content.len(), 1);
                    }
                    _ => panic!("Expected Emph"),
                }
            }
            _ => panic!("Expected Paragraph"),
        }
    }
}
