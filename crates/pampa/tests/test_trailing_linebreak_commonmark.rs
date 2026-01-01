/*
 * test_trailing_linebreak_commonmark.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for CommonMark-correct behavior of trailing backslash.
 *
 * Per CommonMark spec (lines 9362-9391), a hard line break does NOT work
 * at the end of a block element. A backslash at the end of a paragraph
 * should produce a literal "\", not a LineBreak.
 *
 * Spec examples:
 * - `foo\` at end of paragraph → `<p>foo\</p>` (literal backslash)
 * - `### foo\` at end of header → `<h3>foo\</h3>` (literal backslash)
 *
 * Run with: cargo test --test test_trailing_linebreak_commonmark
 */

use pampa::pandoc::{ASTContext, treesitter_to_pandoc};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use pampa::writers;
use tree_sitter_qmd::MarkdownParser;

/// Helper function to parse QMD input and convert to Pandoc AST
fn parse_qmd_to_pandoc_ast(input: &str) -> String {
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let mut buf = Vec::new();
    let mut error_collector = DiagnosticCollector::new();

    let pandoc = treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        input_bytes,
        &ASTContext::anonymous(),
        &mut error_collector,
    )
    .unwrap();

    writers::native::write(&pandoc, &ASTContext::anonymous(), &mut buf).unwrap();
    String::from_utf8(buf).expect("Invalid UTF-8 in output")
}

/// Test: Backslash at end of paragraph should produce literal backslash, not LineBreak.
/// CommonMark spec example 670: `foo\` → `<p>foo\</p>`
#[test]
fn test_backslash_at_end_of_paragraph_is_literal() {
    let input = "foo\\\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "foo" , Str "\\" ] or Para [ Str "foo\\" ]
    // (depending on whether strings are merged)
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );

    // CRITICAL: Should NOT contain LineBreak at end of paragraph
    // Per CommonMark spec, backslash at end of block is literal, not hard break
    assert!(
        !result.contains("LineBreak"),
        "Output should NOT contain LineBreak for backslash at end of paragraph.\n\
         CommonMark spec says backslash at end of block is literal '\\', not LineBreak.\n\
         Got: {}",
        result
    );

    // Should contain the literal backslash as part of Str content
    assert!(
        result.contains("Str \"foo\\\\\"") || result.contains("Str \"\\\\\""),
        "Output should contain Str with literal backslash.\n\
         Expected 'Str \"foo\\\\\"' or separate 'Str \"\\\\\"' for the backslash.\n\
         Got: {}",
        result
    );
}

/// Test: Backslash in middle of paragraph should still be LineBreak.
/// This ensures we only convert trailing LineBreaks, not all LineBreaks.
#[test]
fn test_backslash_in_middle_of_paragraph_is_linebreak() {
    let input = "foo\\\nbar\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "foo" , LineBreak , Str "bar" ]
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );
    assert!(
        result.contains("LineBreak"),
        "Output should contain LineBreak for backslash in middle of paragraph: {}",
        result
    );
    assert!(
        result.contains("Str \"foo\""),
        "Output should contain Str \"foo\": {}",
        result
    );
    assert!(
        result.contains("Str \"bar\""),
        "Output should contain Str \"bar\": {}",
        result
    );
}

/// Test: Backslash at end of header should produce literal backslash.
/// CommonMark spec example 672: `### foo\` → `<h3>foo\</h3>`
#[test]
fn test_backslash_at_end_of_header_is_literal() {
    let input = "# foo\\\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Header 1 ("foo-1",[],[]) [ Str "foo" , Str "\\" ]
    assert!(
        result.contains("Header"),
        "Output should contain Header: {}",
        result
    );

    // CRITICAL: Should NOT contain LineBreak at end of header
    assert!(
        !result.contains("LineBreak"),
        "Output should NOT contain LineBreak for backslash at end of header.\n\
         CommonMark spec says backslash at end of block is literal '\\', not LineBreak.\n\
         Got: {}",
        result
    );

    // Should contain the literal backslash
    assert!(
        result.contains("Str \"foo\\\\\"") || result.contains("Str \"\\\\\""),
        "Output should contain Str with literal backslash.\n\
         Got: {}",
        result
    );
}

/// Test: Backslash at end of Plain block (tight list item) should produce literal backslash.
#[test]
fn test_backslash_at_end_of_plain_is_literal() {
    let input = "- foo\\\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: BulletList [[ Plain [ Str "foo" , Str "\\" ] ]]
    assert!(
        result.contains("BulletList"),
        "Output should contain BulletList: {}",
        result
    );
    assert!(
        result.contains("Plain"),
        "Output should contain Plain: {}",
        result
    );

    // CRITICAL: Should NOT contain LineBreak at end of Plain block
    assert!(
        !result.contains("LineBreak"),
        "Output should NOT contain LineBreak for backslash at end of list item.\n\
         CommonMark spec says backslash at end of block is literal '\\', not LineBreak.\n\
         Got: {}",
        result
    );
}

/// Test: Multiple paragraphs, each with trailing backslash.
/// Verifies we handle the end-of-block detection correctly across multiple blocks.
#[test]
fn test_multiple_paragraphs_with_trailing_backslash() {
    let input = "first\\\n\nsecond\\\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce two Para blocks, neither with LineBreak
    let para_count = result.matches("Para").count();
    assert_eq!(
        para_count, 2,
        "Output should contain 2 Para blocks: {}",
        result
    );

    // Should NOT contain any LineBreak
    assert!(
        !result.contains("LineBreak"),
        "Output should NOT contain LineBreak for trailing backslashes: {}",
        result
    );
}

/// Test: Backslash at end of emphasis at end of paragraph.
/// The backslash should still become literal even when inside formatting.
#[test]
fn test_backslash_at_end_of_emphasis_at_end_of_paragraph() {
    // Note: *foo\* would close emphasis before the backslash, so we test *foo*\
    // which is emphasis followed by trailing backslash
    let input = "*foo*\\\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Emph [ Str "foo" ] , Str "\\" ]
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );
    assert!(
        result.contains("Emph"),
        "Output should contain Emph: {}",
        result
    );

    // Should NOT contain LineBreak at end
    assert!(
        !result.contains("LineBreak"),
        "Output should NOT contain LineBreak for backslash at end of para (after emphasis).\n\
         Got: {}",
        result
    );
}
