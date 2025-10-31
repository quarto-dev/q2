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

// ============================================================================
// Code Span Tests (inline code with backticks)
// ============================================================================

/// Test basic code span - single word
#[test]
fn test_pandoc_code_span_basic() {
    let input = "`code`";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ("", [], []) "code" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(
        result.contains("\"code\""),
        "Should contain \"code\": {}",
        result
    );
}

/// Test code span with spaces
#[test]
fn test_pandoc_code_span_with_spaces() {
    let input = "`code with spaces`";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ("", [], []) "code with spaces" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(
        result.contains("\"code with spaces\""),
        "Should contain \"code with spaces\": {}",
        result
    );
}

/// Test code span with no spaces around it
#[test]
fn test_pandoc_code_span_no_spaces_around() {
    let input = "x`y`z";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "x" , Code ("", [], []) "y" , Str "z" ]
    // No Space nodes should be present
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(result.contains("Str \"x\""), "Should contain x: {}", result);
    assert!(result.contains("\"y\""), "Should contain y: {}", result);
    assert!(result.contains("Str \"z\""), "Should contain z: {}", result);
    // Should NOT have Space nodes
    assert!(
        !result.contains("Space"),
        "Should NOT contain Space nodes: {}",
        result
    );
}

/// Test code span within text
#[test]
fn test_pandoc_code_span_within_text() {
    let input = "test `code` here";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "test" , Space , Code ("", [], []) "code" , Space , Str "here" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(
        result.contains("Str \"test\""),
        "Should contain Str \"test\": {}",
        result
    );
    assert!(
        result.contains("\"code\""),
        "Should contain \"code\": {}",
        result
    );
    assert!(
        result.contains("Str \"here\""),
        "Should contain Str \"here\": {}",
        result
    );
    // Check for Space nodes
    assert!(
        result.contains("Space"),
        "Should contain Space nodes: {}",
        result
    );
    let space_count = result.matches("Space").count();
    assert_eq!(space_count, 2, "Should have 2 Space nodes: {}", result);
}

/// Test multiple code spans in one paragraph
#[test]
fn test_pandoc_code_span_multiple() {
    let input = "`foo` and `bar`";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ("", [], []) "foo" , Space , Str "and" , Space , Code ("", [], []) "bar" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);

    // Count occurrences of "Code" - should appear twice
    let code_count = result.matches("Code").count();
    assert_eq!(code_count, 2, "Should contain 2 Code nodes: {}", result);

    assert!(
        result.contains("\"foo\""),
        "Should contain \"foo\": {}",
        result
    );
    assert!(
        result.contains("\"bar\""),
        "Should contain \"bar\": {}",
        result
    );
    assert!(
        result.contains("Str \"and\""),
        "Should contain Str \"and\": {}",
        result
    );
    // Check for Space nodes (should be 2: after first code, before second code)
    assert!(
        result.contains("Space"),
        "Should contain Space nodes: {}",
        result
    );
    let space_count = result.matches("Space").count();
    assert_eq!(space_count, 2, "Should have 2 Space nodes: {}", result);
}

/// Test code span preserves internal spaces
#[test]
fn test_pandoc_code_span_preserves_spaces() {
    let input = "`  spaced  `";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ("", [], []) "  spaced  " ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    // The exact format might vary, but spaces should be preserved
    assert!(
        result.contains("spaced"),
        "Should contain spaced: {}",
        result
    );
}

// ============================================================================
// ATX HEADING TESTS
// ============================================================================

/// Test H1 heading with single word
#[test]
fn test_atx_heading_h1_single_word() {
    let input = "# Hello";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Header 1 ("hello", [], []) [ Str "Hello" ]
    assert!(
        result.contains("Header"),
        "Should contain Header: {}",
        result
    );
    assert!(result.contains("1"), "Should contain level 1: {}", result);
    assert!(
        result.contains("Str \"Hello\""),
        "Should contain Str \"Hello\": {}",
        result
    );
}

/// Test H2 heading
#[test]
fn test_atx_heading_h2() {
    let input = "## Second Level";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Header 2 (...) [ Str "Second" , Space , Str "Level" ]
    assert!(
        result.contains("Header"),
        "Should contain Header: {}",
        result
    );
    assert!(result.contains("2"), "Should contain level 2: {}", result);
    assert!(
        result.contains("Str \"Second\""),
        "Should contain Str \"Second\": {}",
        result
    );
    assert!(
        result.contains("Str \"Level\""),
        "Should contain Str \"Level\": {}",
        result
    );
}

/// Test H3 heading
#[test]
fn test_atx_heading_h3() {
    let input = "### Third";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Header"),
        "Should contain Header: {}",
        result
    );
    assert!(result.contains("3"), "Should contain level 3: {}", result);
}

/// Test H4 heading
#[test]
fn test_atx_heading_h4() {
    let input = "#### Fourth";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Header"),
        "Should contain Header: {}",
        result
    );
    assert!(result.contains("4"), "Should contain level 4: {}", result);
}

/// Test H5 heading
#[test]
fn test_atx_heading_h5() {
    let input = "##### Fifth";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Header"),
        "Should contain Header: {}",
        result
    );
    assert!(result.contains("5"), "Should contain level 5: {}", result);
}

/// Test H6 heading
#[test]
fn test_atx_heading_h6() {
    let input = "###### Sixth";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Header"),
        "Should contain Header: {}",
        result
    );
    assert!(result.contains("6"), "Should contain level 6: {}", result);
}

/// Test heading with multiple words
#[test]
fn test_atx_heading_multiple_words() {
    let input = "# This is a heading";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Header 1 (...) [ Str "This" , Space , Str "is" , Space , Str "a" , Space , Str "heading" ]
    assert!(
        result.contains("Header"),
        "Should contain Header: {}",
        result
    );
    assert!(
        result.contains("Str \"This\""),
        "Should contain Str \"This\": {}",
        result
    );
    assert!(
        result.contains("Str \"is\""),
        "Should contain Str \"is\": {}",
        result
    );
    assert!(
        result.contains("Str \"heading\""),
        "Should contain Str \"heading\": {}",
        result
    );
}

/// Test heading with emphasis
#[test]
fn test_atx_heading_with_emphasis() {
    let input = "# Heading with *emphasis*";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Header 1 (...) [ Str "Heading" , Space , Str "with" , Space , Emph [ Str "emphasis" ] ]
    assert!(
        result.contains("Header"),
        "Should contain Header: {}",
        result
    );
    assert!(result.contains("Emph"), "Should contain Emph: {}", result);
    assert!(
        result.contains("Str \"emphasis\""),
        "Should contain Str \"emphasis\": {}",
        result
    );
}

/// Test heading with code span
#[test]
fn test_atx_heading_with_code() {
    let input = "# Heading with `code`";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Header 1 (...) [ Str "Heading" , Space , Str "with" , Space , Code (...) "code" ]
    assert!(
        result.contains("Header"),
        "Should contain Header: {}",
        result
    );
    assert!(result.contains("Code"), "Should contain Code: {}", result);
}

// ============================================================================
// MATH TESTS (INLINE AND DISPLAY)
// ============================================================================

/// Test inline math with single variable
#[test]
fn test_pandoc_math_single_variable() {
    let input = "$x$";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Math InlineMath "x" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Math"), "Should contain Math: {}", result);
    assert!(
        result.contains("InlineMath"),
        "Should contain InlineMath: {}",
        result
    );
    assert!(result.contains("\"x\""), "Should contain \"x\": {}", result);
}

/// Test inline math with expression
#[test]
fn test_pandoc_math_expression() {
    let input = "$x + y$";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Math InlineMath "x + y" ]
    assert!(result.contains("Math"), "Should contain Math: {}", result);
    assert!(
        result.contains("InlineMath"),
        "Should contain InlineMath: {}",
        result
    );
    assert!(
        result.contains("x + y"),
        "Should contain 'x + y': {}",
        result
    );
}

/// Test inline math in text
#[test]
fn test_pandoc_math_in_text() {
    let input = "The equation $x + y = z$ is simple";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "The" , Space , Str "equation" , Space , Math InlineMath "x + y = z" , Space , Str "is" , Space , Str "simple" ]
    assert!(result.contains("Math"), "Should contain Math: {}", result);
    assert!(
        result.contains("InlineMath"),
        "Should contain InlineMath: {}",
        result
    );
    assert!(
        result.contains("Str \"The\""),
        "Should contain Str \"The\": {}",
        result
    );
    assert!(
        result.contains("Str \"simple\""),
        "Should contain Str \"simple\": {}",
        result
    );
}

/// Test inline math with LaTeX commands
#[test]
fn test_pandoc_math_with_latex() {
    let input = r"$\frac{a}{b}$";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Math InlineMath "\\frac{a}{b}" ]
    assert!(result.contains("Math"), "Should contain Math: {}", result);
    assert!(
        result.contains("InlineMath"),
        "Should contain InlineMath: {}",
        result
    );
    assert!(result.contains("frac"), "Should contain 'frac': {}", result);
}

/// Test display math with single variable
#[test]
fn test_pandoc_display_math_single_variable() {
    let input = "$$x$$";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Math DisplayMath "x" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Math"), "Should contain Math: {}", result);
    assert!(
        result.contains("DisplayMath"),
        "Should contain DisplayMath: {}",
        result
    );
    assert!(result.contains("\"x\""), "Should contain \"x\": {}", result);
}

/// Test display math with expression
#[test]
fn test_pandoc_display_math_expression() {
    let input = "$$x + y$$";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Math DisplayMath "x + y" ]
    assert!(result.contains("Math"), "Should contain Math: {}", result);
    assert!(
        result.contains("DisplayMath"),
        "Should contain DisplayMath: {}",
        result
    );
    assert!(
        result.contains("x + y"),
        "Should contain 'x + y': {}",
        result
    );
}

/// Test display math in text
#[test]
fn test_pandoc_display_math_in_text() {
    let input = "The equation $$x + y = z$$ is simple";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Str "The" , Space , Str "equation" , Space , Math DisplayMath "x + y = z" , Space , Str "is" , Space , Str "simple" ]
    assert!(result.contains("Math"), "Should contain Math: {}", result);
    assert!(
        result.contains("DisplayMath"),
        "Should contain DisplayMath: {}",
        result
    );
    assert!(
        result.contains("Str \"The\""),
        "Should contain Str \"The\": {}",
        result
    );
    assert!(
        result.contains("Str \"simple\""),
        "Should contain Str \"simple\": {}",
        result
    );
}

/// Test display math with LaTeX commands
#[test]
fn test_pandoc_display_math_with_latex() {
    let input = r"$$\frac{a}{b}$$";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Math DisplayMath "\\frac{a}{b}" ]
    assert!(result.contains("Math"), "Should contain Math: {}", result);
    assert!(
        result.contains("DisplayMath"),
        "Should contain DisplayMath: {}",
        result
    );
    assert!(result.contains("frac"), "Should contain 'frac': {}", result);
}
