/*
 * test_inline_locations.rs
 * Copyright (c) 2025 Posit, PBC
 */

use quarto_markdown_pandoc::pandoc::{ASTContext, treesitter_to_pandoc};
use quarto_markdown_pandoc::utils::diagnostic_collector::DiagnosticCollector;
use quarto_markdown_pandoc::writers;
use tree_sitter_qmd::MarkdownParser;

/// Helper to resolve a source info reference from the pool (compact format)
/// Returns (start_offset, start_row, start_col, end_offset, end_row, end_col, type_code)
///
/// Note: The new JSON format only stores offsets, not row/column. This function computes
/// row/column from the offsets using the FileInformation.
fn resolve_source_ref(
    source_ref: &serde_json::Value,
    pool: &[serde_json::Value],
    file_info: &quarto_source_map::FileInformation,
) -> (usize, usize, usize, usize, usize, usize, usize) {
    let ref_id = source_ref
        .as_u64()
        .expect("Expected source ref to be a number");
    let source_info = &pool[ref_id as usize];

    let r = source_info["r"]
        .as_array()
        .expect("Expected r to be an array");
    let t = source_info["t"]
        .as_u64()
        .expect("Expected t to be a number") as usize;

    // New format: r is [start_offset, end_offset]
    let start_offset = r[0].as_u64().unwrap() as usize;
    let end_offset = r[1].as_u64().unwrap() as usize;

    // For Substring type (t=1), we need to recursively resolve the offsets
    // through the parent chain to get the file offsets
    let (absolute_start, absolute_end) = match t {
        0 => {
            // Original: offsets are already absolute in the file
            (start_offset, end_offset)
        }
        1 => {
            // Substring: need to resolve through parent
            let parent_id = source_info["d"].as_u64().unwrap() as usize;
            let parent = &pool[parent_id];
            let parent_r = parent["r"].as_array().unwrap();
            let parent_start = parent_r[0].as_u64().unwrap() as usize;
            // Substring offsets are relative to parent
            (parent_start + start_offset, parent_start + end_offset)
        }
        2 => {
            // Concat: use the range directly (should span all pieces)
            (start_offset, end_offset)
        }
        _ => panic!("Unknown source info type: {}", t),
    };

    // Compute row/column from absolute offsets using FileInformation
    let start_loc = file_info
        .offset_to_location(absolute_start)
        .expect("Failed to convert start offset to location");
    let end_loc = file_info
        .offset_to_location(absolute_end)
        .expect("Failed to convert end offset to location");

    (
        start_loc.offset,
        start_loc.row,
        start_loc.column,
        end_loc.offset,
        end_loc.row,
        end_loc.column,
        t,
    )
}

#[test]
fn test_inline_source_locations() {
    let input = "hello _world_.";
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let context = ASTContext::anonymous();
    let mut error_collector = DiagnosticCollector::new();
    let pandoc = treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        &input_bytes,
        &context,
        &mut error_collector,
    )
    .expect("Failed to convert to Pandoc AST");

    let mut buf = Vec::new();
    writers::json::write(&pandoc, &context, &mut buf).unwrap();
    let json_output = String::from_utf8(buf).expect("Invalid UTF-8 in output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_output).expect("Failed to parse JSON output");

    // Get the source info pool
    let pool = json_value["astContext"]["sourceInfoPool"]
        .as_array()
        .expect("Expected sourceInfoPool to be an array");

    // Create FileInformation for computing row/column from offsets
    let file_info = quarto_source_map::FileInformation::new(input);

    // Check that the source locations are correct for the inline nodes
    let blocks = json_value["blocks"].as_array().unwrap();
    let para = &blocks[0];
    let inlines = para["c"].as_array().unwrap();

    // First inline should be "hello" with correct location
    let hello_str = &inlines[0];
    assert_eq!(hello_str["t"], "Str");
    assert_eq!(hello_str["c"], "hello");
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _type) =
        resolve_source_ref(&hello_str["s"], pool, &file_info);
    assert_eq!(start_col, 0);
    assert_eq!(start_off, 0);
    assert_eq!(end_col, 5);
    assert_eq!(end_off, 5);

    // Second inline should be a Space
    let space = &inlines[1];
    assert_eq!(space["t"], "Space");
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&space["s"], pool, &file_info);
    assert_eq!(start_col, 5);
    assert_eq!(start_off, 5);
    assert_eq!(end_col, 6);
    assert_eq!(end_off, 6);

    // Third inline should be Emph containing "world"
    let emph = &inlines[2];
    assert_eq!(emph["t"], "Emph");
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&emph["s"], pool, &file_info);
    assert_eq!(start_col, 6);
    assert_eq!(start_off, 6);
    assert_eq!(end_col, 13);
    assert_eq!(end_off, 13);

    // Check the "world" string inside Emph
    let emph_content = emph["c"].as_array().unwrap();
    let world_str = &emph_content[0];
    assert_eq!(world_str["t"], "Str");
    assert_eq!(world_str["c"], "world");
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&world_str["s"], pool, &file_info);
    assert_eq!(start_col, 7);
    assert_eq!(start_off, 7);
    assert_eq!(end_col, 12);
    assert_eq!(end_off, 12);

    // Fourth inline should be "."
    let period = &inlines[3];
    assert_eq!(period["t"], "Str");
    assert_eq!(period["c"], ".");
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&period["s"], pool, &file_info);
    assert_eq!(start_col, 13);
    assert_eq!(start_off, 13);
    assert_eq!(end_col, 14);
    assert_eq!(end_off, 14);
}

#[test]
fn test_merged_strings_preserve_location() {
    // Test that when truly adjacent strings are merged, the location spans both
    // Using input that will produce adjacent Str nodes that get merged
    let input = "hello world";
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let context = ASTContext::anonymous();
    let mut error_collector = DiagnosticCollector::new();
    let pandoc = treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        &input_bytes,
        &context,
        &mut error_collector,
    )
    .expect("Failed to convert to Pandoc AST");

    let mut buf = Vec::new();
    writers::json::write(&pandoc, &context, &mut buf).unwrap();
    let json_output = String::from_utf8(buf).expect("Invalid UTF-8 in output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_output).expect("Failed to parse JSON output");

    // Get the source info pool
    let pool = json_value["astContext"]["sourceInfoPool"]
        .as_array()
        .expect("Expected sourceInfoPool to be an array");

    // Create FileInformation for computing row/column from offsets
    let file_info = quarto_source_map::FileInformation::new(input);

    let blocks = json_value["blocks"].as_array().unwrap();
    let para = &blocks[0];
    let inlines = para["c"].as_array().unwrap();

    // The parser should produce "hello", Space, "world"
    // Check that each has correct location
    assert_eq!(inlines.len(), 3);

    // First inline should be "hello"
    let hello = &inlines[0];
    assert_eq!(hello["t"], "Str");
    assert_eq!(hello["c"], "hello");
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&hello["s"], pool, &file_info);
    assert_eq!(start_col, 0);
    assert_eq!(start_off, 0);
    assert_eq!(end_col, 5);
    assert_eq!(end_off, 5);

    // Second should be Space
    let space = &inlines[1];
    assert_eq!(space["t"], "Space");

    // Third should be "world"
    let world = &inlines[2];
    assert_eq!(world["t"], "Str");
    assert_eq!(world["c"], "world");
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&world["s"], pool, &file_info);
    assert_eq!(start_col, 6);
    assert_eq!(start_off, 6);
    assert_eq!(end_col, 11);
    assert_eq!(end_off, 11);
}

#[test]
fn test_separate_strings_keep_separate_locations() {
    // Test that strings separated by other inline elements aren't merged
    // and each keeps its own location
    let input = "a**b**c";
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let context = ASTContext::anonymous();
    let mut error_collector = DiagnosticCollector::new();
    let pandoc = treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        &input_bytes,
        &context,
        &mut error_collector,
    )
    .expect("Failed to convert to Pandoc AST");

    let mut buf = Vec::new();
    writers::json::write(&pandoc, &context, &mut buf).unwrap();
    let json_output = String::from_utf8(buf).expect("Invalid UTF-8 in output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_output).expect("Failed to parse JSON output");

    // Get the source info pool
    let pool = json_value["astContext"]["sourceInfoPool"]
        .as_array()
        .expect("Expected sourceInfoPool to be an array");

    // Create FileInformation for computing row/column from offsets
    let file_info = quarto_source_map::FileInformation::new(input);

    let blocks = json_value["blocks"].as_array().unwrap();
    let para = &blocks[0];
    let inlines = para["c"].as_array().unwrap();

    // Should have three elements: "a", Strong("b"), "c"
    assert_eq!(inlines.len(), 3);

    // First inline should be "a"
    let a_str = &inlines[0];
    assert_eq!(a_str["t"], "Str");
    assert_eq!(a_str["c"], "a");
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&a_str["s"], pool, &file_info);
    assert_eq!(start_col, 0);
    assert_eq!(start_off, 0);
    assert_eq!(end_col, 1);
    assert_eq!(end_off, 1);

    // Second inline should be Strong containing "b"
    let strong = &inlines[1];
    assert_eq!(strong["t"], "Strong");
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&strong["s"], pool, &file_info);
    assert_eq!(start_col, 1);
    assert_eq!(start_off, 1);
    assert_eq!(end_col, 6);
    assert_eq!(end_off, 6);

    // Third inline should be "c"
    let c_str = &inlines[2];
    assert_eq!(c_str["t"], "Str");
    assert_eq!(c_str["c"], "c");
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&c_str["s"], pool, &file_info);
    assert_eq!(start_col, 6);
    assert_eq!(start_off, 6);
    assert_eq!(end_col, 7);
    assert_eq!(end_off, 7);
}

#[test]
fn test_note_source_location() {
    // Test that inline notes have proper source location tracking
    // including the synthetic Paragraph wrapper inside the Note
    let input = "text^[note content]more";
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let context = ASTContext::anonymous();
    let mut error_collector = DiagnosticCollector::new();
    let pandoc = treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        &input_bytes,
        &context,
        &mut error_collector,
    )
    .expect("Failed to convert to Pandoc AST");

    let mut buf = Vec::new();
    writers::json::write(&pandoc, &context, &mut buf).unwrap();
    let json_output = String::from_utf8(buf).expect("Invalid UTF-8 in output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_output).expect("Failed to parse JSON output");

    // Get the source info pool
    let pool = json_value["astContext"]["sourceInfoPool"]
        .as_array()
        .expect("Expected sourceInfoPool to be an array");

    // Create FileInformation for computing row/column from offsets
    let file_info = quarto_source_map::FileInformation::new(input);

    let blocks = json_value["blocks"].as_array().unwrap();
    let para = &blocks[0];
    let inlines = para["c"].as_array().unwrap();

    // Should have three elements: "text", Note, "more"
    assert_eq!(inlines.len(), 3);

    // First inline should be "text"
    let text_str = &inlines[0];
    assert_eq!(text_str["t"], "Str");
    assert_eq!(text_str["c"], "text");

    // Second inline should be Note with proper source location
    let note = &inlines[1];
    assert_eq!(note["t"], "Note");

    // Check Note's source location spans the entire ^[note content]
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&note["s"], pool, &file_info);
    assert_eq!(start_col, 4);
    assert_eq!(start_off, 4);
    assert_eq!(end_col, 19);
    assert_eq!(end_off, 19);

    // Check Note content - should be a single Block::Paragraph
    let note_blocks = note["c"].as_array().unwrap();
    assert_eq!(note_blocks.len(), 1);

    let note_para = &note_blocks[0];
    assert_eq!(note_para["t"], "Para");

    // CRITICAL: The Paragraph wrapper should have proper source location
    // not SourceInfo::default() which would be FileId(0) with offset 0
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&note_para["s"], pool, &file_info);

    // The paragraph wrapper should have the same source location as the Note itself
    // since it's a synthetic wrapper for the note's content
    assert_eq!(start_col, 4);
    assert_eq!(start_off, 4);
    assert_eq!(end_col, 19);
    assert_eq!(end_off, 19);

    // Check the content inside the paragraph
    // The parser splits "note content" into three inlines: "note", Space, "content"
    let note_para_inlines = note_para["c"].as_array().unwrap();
    assert_eq!(note_para_inlines.len(), 3);

    let note_str = &note_para_inlines[0];
    assert_eq!(note_str["t"], "Str");
    assert_eq!(note_str["c"], "note");

    let space = &note_para_inlines[1];
    assert_eq!(space["t"], "Space");

    let content_str = &note_para_inlines[2];
    assert_eq!(content_str["t"], "Str");
    assert_eq!(content_str["c"], "content");

    // Third inline should be "more"
    let more_str = &inlines[2];
    assert_eq!(more_str["t"], "Str");
    assert_eq!(more_str["c"], "more");
}

#[test]
fn test_note_reference_source_location() {
    // Test that NoteReference nodes have proper source location tracking
    // This is verified through the Span it gets converted to in postprocess
    let input = r#"Some text [^note1].

[^note1]: Note content here."#;
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let context = ASTContext::anonymous();
    let mut error_collector = DiagnosticCollector::new();
    let pandoc = treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        &input_bytes,
        &context,
        &mut error_collector,
    )
    .expect("Failed to convert to Pandoc AST");

    let mut buf = Vec::new();
    writers::json::write(&pandoc, &context, &mut buf).unwrap();
    let json_output = String::from_utf8(buf).expect("Invalid UTF-8 in output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_output).expect("Failed to parse JSON output");

    // Get the source info pool
    let pool = json_value["astContext"]["sourceInfoPool"]
        .as_array()
        .expect("Expected sourceInfoPool to be an array");

    // Create FileInformation for computing row/column from offsets
    let file_info = quarto_source_map::FileInformation::new(input);

    let blocks = json_value["blocks"].as_array().unwrap();
    let para = &blocks[0];
    let inlines = para["c"].as_array().unwrap();

    // Should have six elements: "Some", Space, "text", Space, Span (converted from NoteReference), "."
    assert_eq!(inlines.len(), 6);

    // The Span (converted from NoteReference) should be the 5th element (index 4)
    let span = &inlines[4];
    assert_eq!(span["t"], "Span");

    // Check that it has the quarto-note-reference class
    let attr = &span["c"][0];
    let classes = attr[1].as_array().unwrap();
    assert!(classes.iter().any(|c| c == "quarto-note-reference"));

    // Check that the reference-id is correct
    let kvs = attr[2].as_array().unwrap();
    assert_eq!(kvs.len(), 1);
    assert_eq!(kvs[0][0], "reference-id");
    assert_eq!(kvs[0][1], "note1");

    // CRITICAL: The Span should have proper source location from the NoteReference
    // not SourceInfo::default() which would be FileId(0) with offset 0
    let (start_off, _start_row, start_col, end_off, _end_row, end_col, _t) =
        resolve_source_ref(&span["s"], pool, &file_info);

    // The [^note1] spans from column 10 to 18 (0-indexed)
    assert_eq!(start_col, 10);
    assert_eq!(start_off, 10);
    assert_eq!(end_col, 18);
    assert_eq!(end_off, 18);

    // Last inline should be "."
    let period = &inlines[5];
    assert_eq!(period["t"], "Str");
    assert_eq!(period["c"], ".");
}
