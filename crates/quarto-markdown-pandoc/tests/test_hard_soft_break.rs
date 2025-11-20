/*
 * test_hard_soft_break.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for hard line break followed by soft line break removal.
 * When tree-sitter emits both LineBreak (from backslash-newline) and SoftBreak (from the newline),
 * the postprocessor should remove the redundant SoftBreak to match Pandoc's behavior.
 *
 * Run with: cargo test --test test_hard_soft_break
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

    writers::native::write(&pandoc, &ASTContext::anonymous(), &mut buf).unwrap();
    String::from_utf8(buf).expect("Invalid UTF-8 in output")
}

/// Test basic hard line break (backslash-newline)
/// Should produce LineBreak ONLY, not LineBreak + SoftBreak
#[test]
fn test_hard_break_only() {
    let input = "hello\\\nworld";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "hello" , LineBreak , Str "world" ]
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );
    assert!(
        result.contains("LineBreak"),
        "Output should contain LineBreak: {}",
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

    // CRITICAL: Should NOT contain SoftBreak after LineBreak
    assert!(
        !result.contains("LineBreak , SoftBreak"),
        "Output should NOT contain both LineBreak and SoftBreak: {}",
        result
    );
}

/// Test that standalone soft break is preserved
/// (no backslash before newline)
#[test]
fn test_soft_break_preserved() {
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

    // Should NOT contain LineBreak
    assert!(
        !result.contains("LineBreak"),
        "Output should NOT contain LineBreak: {}",
        result
    );
}

/// Test multiple consecutive hard breaks
#[test]
fn test_multiple_hard_breaks() {
    let input = "hello\\\nthere\\\nworld";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "hello" , LineBreak , Str "there" , LineBreak , Str "world" ]
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );

    // Count LineBreak occurrences
    let linebreak_count = result.matches("LineBreak").count();
    assert_eq!(
        linebreak_count, 2,
        "Output should contain exactly 2 LineBreak instances: {}",
        result
    );

    // Should NOT contain any SoftBreak
    assert!(
        !result.contains("SoftBreak"),
        "Output should NOT contain SoftBreak: {}",
        result
    );
}

/// Test hard break inside bold formatting
#[test]
fn test_hard_break_in_bold() {
    let input = "**hello\\\nworld**";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Strong [ Str "hello" , LineBreak , Str "world" ] ]
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );
    assert!(
        result.contains("Strong"),
        "Output should contain Strong: {}",
        result
    );
    assert!(
        result.contains("LineBreak"),
        "Output should contain LineBreak: {}",
        result
    );

    // Should NOT contain SoftBreak
    assert!(
        !result.contains("SoftBreak"),
        "Output should NOT contain SoftBreak: {}",
        result
    );
}

/// Test hard break inside emphasis
#[test]
fn test_hard_break_in_emphasis() {
    let input = "*hello\\\nworld*";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Emph [ Str "hello" , LineBreak , Str "world" ] ]
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
    assert!(
        result.contains("LineBreak"),
        "Output should contain LineBreak: {}",
        result
    );

    // Should NOT contain SoftBreak
    assert!(
        !result.contains("SoftBreak"),
        "Output should NOT contain SoftBreak: {}",
        result
    );
}

/// Test hard break at end of paragraph
#[test]
fn test_hard_break_at_end() {
    let input = "hello\\\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "hello" , LineBreak ]
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );
    assert!(
        result.contains("LineBreak"),
        "Output should contain LineBreak: {}",
        result
    );

    // Should NOT contain SoftBreak
    assert!(
        !result.contains("SoftBreak"),
        "Output should NOT contain SoftBreak: {}",
        result
    );
}

/// Test mix of hard break and soft break in same paragraph
#[test]
fn test_mixed_hard_and_soft_breaks() {
    let input = "first\\\nsecond\nthird";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "first" , LineBreak , Str "second" , SoftBreak , Str "third" ]
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );
    assert!(
        result.contains("LineBreak"),
        "Output should contain LineBreak: {}",
        result
    );
    assert!(
        result.contains("SoftBreak"),
        "Output should contain SoftBreak: {}",
        result
    );

    // LineBreak should appear exactly once
    let linebreak_count = result.matches("LineBreak").count();
    assert_eq!(
        linebreak_count, 1,
        "Output should contain exactly 1 LineBreak: {}",
        result
    );

    // SoftBreak should appear exactly once
    let softbreak_count = result.matches("SoftBreak").count();
    assert_eq!(
        softbreak_count, 1,
        "Output should contain exactly 1 SoftBreak: {}",
        result
    );
}
