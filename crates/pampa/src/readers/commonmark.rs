/*
 * commonmark.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * CommonMark reader using comrak for parsing.
 *
 * This provides an alternative to the QMD reader for pure CommonMark content,
 * leveraging the comrak parser and the comrak-to-pandoc conversion library.
 */

use crate::pandoc::ast_context::ASTContext;
use comrak::{Arena, Options, parse_document};
use comrak_to_pandoc::{SourceLocationContext, convert_document_with_source};
use quarto_pandoc_types::Pandoc;
use quarto_source_map::FileId;

/// Read CommonMark input and convert to Pandoc AST with source tracking.
///
/// # Arguments
/// * `input` - The CommonMark source text
/// * `filename` - The filename for error reporting and source tracking
///
/// # Returns
/// A tuple of (Pandoc document, ASTContext with source info)
pub fn read(input: &str, filename: &str) -> (Pandoc, ASTContext) {
    let arena = Arena::new();

    // Use pure CommonMark options (no GFM extensions)
    let options = Options::default();

    // Parse with comrak
    let root = parse_document(&arena, input, &options);

    // Set up source tracking
    let mut context = ASTContext::with_filename(filename.to_string());
    context
        .source_context
        .add_file(filename.to_string(), Some(input.to_string()));
    let file_id = FileId(0); // First file added gets ID 0

    // Create source location context for conversion
    let source_ctx = SourceLocationContext::new(input, file_id);

    // Convert to Pandoc AST with source tracking
    let pandoc = convert_document_with_source(root, Some(&source_ctx));

    (pandoc, context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{Block, Inline};

    #[test]
    fn test_simple_paragraph() {
        let (pandoc, _ctx) = read("Hello world.\n", "test.md");
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            Block::Paragraph(p) => {
                // Should be: Str("Hello"), Space, Str("world.")
                assert_eq!(p.content.len(), 3);
                match &p.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "Hello"),
                    _ => panic!("Expected Str"),
                }
            }
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_heading() {
        let (pandoc, _ctx) = read("# Hello\n", "test.md");
        assert_eq!(pandoc.blocks.len(), 1);
        match &pandoc.blocks[0] {
            Block::Header(h) => {
                assert_eq!(h.level, 1);
            }
            _ => panic!("Expected Header"),
        }
    }

    #[test]
    fn test_source_tracking() {
        let input = "Hello world.\n";
        let (pandoc, _ctx) = read(input, "test.md");

        // Check that source info is present
        match &pandoc.blocks[0] {
            Block::Paragraph(p) => {
                // Paragraph should have source info covering the whole line
                assert!(p.source_info.start_offset() < p.source_info.end_offset());

                // First Str should have precise source info
                match &p.content[0] {
                    Inline::Str(s) => {
                        assert_eq!(s.source_info.start_offset(), 0);
                        assert_eq!(s.source_info.end_offset(), 5); // "Hello"
                    }
                    _ => panic!("Expected Str"),
                }
            }
            _ => panic!("Expected Paragraph"),
        }
    }

    #[test]
    fn test_emphasis_source() {
        let input = "*hello*\n";
        let (pandoc, _ctx) = read(input, "test.md");

        match &pandoc.blocks[0] {
            Block::Paragraph(p) => {
                match &p.content[0] {
                    Inline::Emph(e) => {
                        // Emph should cover "*hello*"
                        assert_eq!(e.source_info.start_offset(), 0);
                        assert_eq!(e.source_info.end_offset(), 7);
                    }
                    _ => panic!("Expected Emph"),
                }
            }
            _ => panic!("Expected Paragraph"),
        }
    }
}
