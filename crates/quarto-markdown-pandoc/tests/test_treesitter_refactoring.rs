/*
 * test_treesitter_refactoring.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for tree-sitter grammar refactoring work.
 * Run with: cargo test --test test_treesitter_refactoring
 *
 * These tests are isolated from the main test suite to allow incremental
 * refactoring without breaking existing tests.
 */

use quarto_markdown_pandoc::pandoc::{ASTContext, treesitter_to_pandoc};
use quarto_markdown_pandoc::utils::diagnostic_collector::DiagnosticCollector;
use quarto_markdown_pandoc::writers;
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
        &input_bytes,
        &ASTContext::anonymous(),
        &mut error_collector,
    )
    .unwrap();

    writers::native::write(&pandoc, &mut buf).unwrap();
    String::from_utf8(buf).expect("Invalid UTF-8 in output")
}

/// Test basic pandoc_str node - single word
#[test]
fn test_pandoc_str_single_word() {
    let input = "hello";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [Str "hello"]
    // The exact format from native writer is: [ Para [ Str "hello" ] ]
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );
    assert!(
        result.contains("Str \"hello\""),
        "Output should contain Str \"hello\": {}",
        result
    );
}

/// Test pandoc_str with multiple words (should be multiple Str nodes with Space between)
#[test]
fn test_pandoc_str_multiple_words() {
    let input = "hello world";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [Str "hello", Space, Str "world"]
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );
    assert!(
        result.contains("Str \"hello\""),
        "Output should contain Str \"hello\": {}",
        result
    );
    assert!(
        result.contains("Str \"world\""),
        "Output should contain Str \"world\": {}",
        result
    );
}

/// Test pandoc_str with numbers
#[test]
fn test_pandoc_str_with_numbers() {
    let input = "test123";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"test123\""),
        "Output should contain Str \"test123\": {}",
        result
    );
}

/// Test pandoc_str with allowed punctuation
#[test]
fn test_pandoc_str_with_punctuation() {
    let input = "hello-world";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"hello-world\""),
        "Output should contain Str \"hello-world\": {}",
        result
    );
}

/// Test soft break - single newline within a paragraph
#[test]
fn test_soft_break() {
    let input = "hello\nworld";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "hello" , SoftBreak , Str "world" ]
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );
    assert!(
        result.contains("SoftBreak"),
        "Output should contain SoftBreak: {}",
        result
    );
    assert!(
        result.contains("Str \"hello\""),
        "Output should contain Str \"hello\": {}",
        result
    );
    assert!(
        result.contains("Str \"world\""),
        "Output should contain Str \"world\": {}",
        result
    );

    // Should NOT concatenate the words
    assert!(
        !result.contains("helloworld"),
        "Output should NOT concatenate words: {}",
        result
    );
}

/// Test basic single-word emphasis with asterisk
#[test]
fn test_pandoc_emph_basic_asterisk() {
    let input = "*hello*";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Emph [ Str "hello" ] ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(
        result.contains("Str \"hello\""),
        "Should contain Str \"hello\": {}",
        result
    );
}

/// Test basic single-word emphasis with underscore
#[test]
fn test_pandoc_emph_basic_underscore() {
    let input = "_hello_";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Emph [ Str "hello" ] ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(
        result.contains("Str \"hello\""),
        "Should contain Str \"hello\": {}",
        result
    );
}

/// Test multi-word emphasis
#[test]
fn test_pandoc_emph_multiple_words() {
    let input = "*hello world*";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Emph [ Str "hello" , Space , Str "world" ] ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(
        result.contains("Str \"hello\""),
        "Should contain Str \"hello\": {}",
        result
    );
    assert!(
        result.contains("Str \"world\""),
        "Should contain Str \"world\": {}",
        result
    );
}

/// Test emphasis within text
#[test]
fn test_pandoc_emph_within_text() {
    let input = "before *hello* after";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "before" , Space , Emph [ Str "hello" ] , Space , Str "after" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(
        result.contains("Str \"before\""),
        "Should contain Str \"before\": {}",
        result
    );
    assert!(
        result.contains("Str \"hello\""),
        "Should contain Str \"hello\": {}",
        result
    );
    assert!(
        result.contains("Str \"after\""),
        "Should contain Str \"after\": {}",
        result
    );
    // Check for Space nodes around emphasis
    assert!(
        result.contains("Space"),
        "Should contain Space nodes: {}",
        result
    );
    let space_count = result.matches("Space").count();
    assert_eq!(space_count, 2, "Should have 2 Space nodes: {}", result);
}

/// Test multiple emphasis in one paragraph
#[test]
fn test_pandoc_emph_multiple() {
    let input = "*hello* and *world*";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Emph [ Str "hello" ] , Space , Str "and" , Space , Emph [ Str "world" ] ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);

    // Count occurrences of "Emph" - should appear twice
    let emph_count = result.matches("Emph").count();
    assert_eq!(emph_count, 2, "Should contain 2 Emph nodes: {}", result);

    assert!(
        result.contains("Str \"hello\""),
        "Should contain Str \"hello\": {}",
        result
    );
    assert!(
        result.contains("Str \"world\""),
        "Should contain Str \"world\": {}",
        result
    );
    assert!(
        result.contains("Str \"and\""),
        "Should contain Str \"and\": {}",
        result
    );
    // Check for Space nodes (should be 2: after first emph, around "and", before second emph)
    assert!(
        result.contains("Space"),
        "Should contain Space nodes: {}",
        result
    );
    let space_count = result.matches("Space").count();
    assert_eq!(space_count, 2, "Should have 2 Space nodes: {}", result);
}

/// Test emphasis with newline (soft break)
#[test]
fn test_pandoc_emph_with_softbreak() {
    let input = "*hello\nworld*";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Emph [ Str "hello" , SoftBreak , Str "world" ] ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(
        result.contains("SoftBreak"),
        "Should contain SoftBreak: {}",
        result
    );
    assert!(
        result.contains("Str \"hello\""),
        "Should contain Str \"hello\": {}",
        result
    );
    assert!(
        result.contains("Str \"world\""),
        "Should contain Str \"world\": {}",
        result
    );
}

/// Test emphasis with no spaces around it
#[test]
fn test_pandoc_emph_no_spaces() {
    let input = "x*y*z";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "x" , Emph [ Str "y" ] , Str "z" ]
    // No Space nodes should be present around the emphasis
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(result.contains("Str \"x\""), "Should contain x: {}", result);
    assert!(result.contains("Str \"y\""), "Should contain y: {}", result);
    assert!(result.contains("Str \"z\""), "Should contain z: {}", result);
    // Should NOT have Space nodes injected
    assert!(
        !result.contains("Space"),
        "Should NOT contain Space nodes: {}",
        result
    );
}

/// Test emphasis with space only before
#[test]
fn test_pandoc_emph_space_before_only() {
    let input = "x *y*z";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "x" , Space , Emph [ Str "y" ] , Str "z" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(result.contains("Space"), "Should contain Space: {}", result);
    // Should have exactly 1 Space node
    let space_count = result.matches("Space").count();
    assert_eq!(
        space_count, 1,
        "Should have exactly 1 Space node: {}",
        result
    );
}

/// Test emphasis with space only after
#[test]
fn test_pandoc_emph_space_after_only() {
    let input = "x*y* z";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "x" , Emph [ Str "y" ] , Space , Str "z" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(result.contains("Space"), "Should contain Space: {}", result);
    // Should have exactly 1 Space node
    let space_count = result.matches("Space").count();
    assert_eq!(
        space_count, 1,
        "Should have exactly 1 Space node: {}",
        result
    );
}

/// Test basic strong emphasis with asterisks
#[test]
fn test_pandoc_strong_basic() {
    let input = "**hello**";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Strong [ Str "hello" ] ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(
        result.contains("Strong"),
        "Should contain Strong: {}",
        result
    );
    assert!(
        result.contains("Str \"hello\""),
        "Should contain Str \"hello\": {}",
        result
    );
}

/// Test strong emphasis with spaces
#[test]
fn test_pandoc_strong_with_spaces() {
    let input = "x **y** z";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "x" , Space , Strong [ Str "y" ] , Space , Str "z" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(
        result.contains("Strong"),
        "Should contain Strong: {}",
        result
    );
    assert!(result.contains("Space"), "Should contain Space: {}", result);
    let space_count = result.matches("Space").count();
    assert_eq!(space_count, 2, "Should have 2 Space nodes: {}", result);
}

