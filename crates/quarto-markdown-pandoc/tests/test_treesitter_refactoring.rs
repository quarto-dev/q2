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

/// Helper function to parse QMD input and convert to JSON
/// Use this for testing blocks that are not represented in Pandoc's native format
/// (e.g., NoteDefinitionPara, which Quarto keeps separate but Pandoc inlines)
fn parse_qmd_to_json(input: &str) -> String {
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let mut buf = Vec::new();
    let mut error_collector = DiagnosticCollector::new();
    let context = ASTContext::anonymous();

    let pandoc = treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        &input_bytes,
        &context,
        &mut error_collector,
    )
    .unwrap();

    writers::json::write(&pandoc, &context, &mut buf).unwrap();
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

// ============================================================================
// ATTRIBUTE TESTS (FOR CODE SPANS AND OTHER INLINE ELEMENTS)
// ============================================================================

/// Test code span with simple class attribute
#[test]
fn test_code_span_with_class() {
    let input = "`code`{.lang}";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ( "" , [ "lang" ] , [] ) "code" ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(
        result.contains("\"lang\""),
        "Should contain class \"lang\": {}",
        result
    );
    assert!(
        result.contains("\"code\""),
        "Should contain code text: {}",
        result
    );
}

/// Test code span with ID attribute
#[test]
fn test_code_span_with_id() {
    let input = "`code`{#myid}";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ( "myid" , [] , [] ) "code" ]
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(
        result.contains("\"myid\""),
        "Should contain id \"myid\": {}",
        result
    );
}

/// Test code span with multiple classes
#[test]
fn test_code_span_with_multiple_classes() {
    let input = "`code`{.class1 .class2 .class3}";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ( "" , [ "class1" , "class2" , "class3" ] , [] ) "code" ]
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(
        result.contains("\"class1\""),
        "Should contain class1: {}",
        result
    );
    assert!(
        result.contains("\"class2\""),
        "Should contain class2: {}",
        result
    );
    assert!(
        result.contains("\"class3\""),
        "Should contain class3: {}",
        result
    );
}

/// Test code span with key-value attribute
#[test]
fn test_code_span_with_key_value() {
    let input = "`code`{key=\"value\"}";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ( "" , [] , [ ( "key" , "value" ) ] ) "code" ]
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(result.contains("\"key\""), "Should contain key: {}", result);
    assert!(
        result.contains("\"value\""),
        "Should contain value: {}",
        result
    );
}

/// Test code span with combined attributes
#[test]
fn test_code_span_with_combined_attributes() {
    let input = "`code`{#myid .class1 .class2 key=\"value\"}";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ( "myid" , [ "class1" , "class2" ] , [ ( "key" , "value" ) ] ) "code" ]
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(result.contains("\"myid\""), "Should contain id: {}", result);
    assert!(
        result.contains("\"class1\""),
        "Should contain class1: {}",
        result
    );
    assert!(
        result.contains("\"class2\""),
        "Should contain class2: {}",
        result
    );
    assert!(result.contains("\"key\""), "Should contain key: {}", result);
    assert!(
        result.contains("\"value\""),
        "Should contain value: {}",
        result
    );
}

/// Test code span with no attributes (baseline)
#[test]
fn test_code_span_without_attributes() {
    let input = "`code`";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ( "" , [] , [] ) "code" ]
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(
        result.contains("\"code\""),
        "Should contain code text: {}",
        result
    );
}

/// Test code span with multiple key-value pairs
#[test]
fn test_code_span_with_multiple_key_values() {
    let input = "`code`{key1=\"value1\" key2=\"value2\"}";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Code ( "" , [] , [ ( "key1" , "value1" ) , ( "key2" , "value2" ) ] ) "code" ]
    assert!(result.contains("Code"), "Should contain Code: {}", result);
    assert!(
        result.contains("\"key1\""),
        "Should contain key1: {}",
        result
    );
    assert!(
        result.contains("\"value1\""),
        "Should contain value1: {}",
        result
    );
    assert!(
        result.contains("\"key2\""),
        "Should contain key2: {}",
        result
    );
    assert!(
        result.contains("\"value2\""),
        "Should contain value2: {}",
        result
    );
}

// ============================================================================
// Backslash Escape Tests
// ============================================================================

/// Test backslash escape for asterisk (should remove backslash)
#[test]
fn test_backslash_escape_asterisk() {
    let input = r"hello\*world";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [Str "hello*world"]
    // NOT: Para [Str "hello\\*world"]
    assert!(
        result.contains("Str \"hello*world\""),
        "Backslash should be removed, expected 'hello*world' but got: {}",
        result
    );
    assert!(
        !result.contains("hello\\\\*world"),
        "Should not contain escaped backslash: {}",
        result
    );
}

/// Test backslash escape for backquote
#[test]
fn test_backslash_escape_backquote() {
    let input = r"hello\`world";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"hello`world\""),
        "Expected 'hello`world' but got: {}",
        result
    );
}

/// Test backslash escape for underscore
#[test]
fn test_backslash_escape_underscore() {
    let input = r"hello\_world";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"hello_world\""),
        "Expected 'hello_world' but got: {}",
        result
    );
}

/// Test backslash escape for hash
#[test]
fn test_backslash_escape_hash() {
    let input = r"\# not a heading";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce a paragraph with # at the start, not a heading
    assert!(
        result.contains("Para"),
        "Should be Para not Header: {}",
        result
    );
    assert!(
        result.contains("Str \"#\""),
        "Expected Str \"#\" but got: {}",
        result
    );
}

/// Test backslash escape for brackets
#[test]
fn test_backslash_escape_brackets() {
    let input = r"\[not a link\]";
    let result = parse_qmd_to_pandoc_ast(input);

    // Pandoc splits this into: Str "[not", Space, Str "a", Space, Str "link]"
    assert!(
        result.contains("Str \"[not\""),
        "Expected Str \"[not\" but got: {}",
        result
    );
    assert!(
        result.contains("Str \"link]\""),
        "Expected Str \"link]\" but got: {}",
        result
    );
}

/// Test multiple backslash escapes in one string
#[test]
fn test_multiple_backslash_escapes() {
    let input = r"hello\*world\!test";
    let result = parse_qmd_to_pandoc_ast(input);

    // The str might be split or combined depending on grammar
    // Just verify the escaped characters appear without backslashes
    assert!(
        result.contains("*") && result.contains("!"),
        "Should contain * and ! without backslashes: {}",
        result
    );
}

/// Test backslash before non-special character (should preserve backslash)
#[test]
fn test_backslash_before_letter() {
    let input = r"hello\world";
    let result = parse_qmd_to_pandoc_ast(input);

    // Backslash before 'w' is not a valid escape (not ASCII punctuation)
    // So the backslash should be preserved
    // Note: Pandoc treats this as LaTeX raw inline, but we handle it differently
    assert!(
        result.contains("Str \"hello\\\\world\""),
        "Backslash before letter should be preserved: {}",
        result
    );
}

// ============================================================================
// Link Tests
// ============================================================================

/// Test basic link
#[test]
fn test_link_basic() {
    let input = "[link text](https://example.com)";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Link"), "Should contain Link: {}", result);
    assert!(
        result.contains("\"https://example.com\""),
        "Should contain URL: {}",
        result
    );
    assert!(
        result.contains("Str \"link\""),
        "Should contain link text: {}",
        result
    );
}

/// Test link with title
#[test]
fn test_link_with_title() {
    let input = r#"[link](url "title text")"#;
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Link"), "Should contain Link: {}", result);
    assert!(result.contains("\"url\""), "Should contain URL: {}", result);
    assert!(
        result.contains("\"title text\""),
        "Should contain title: {}",
        result
    );
}

/// Test link in context
#[test]
fn test_link_in_context() {
    let input = "text [link](url) more";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"text\""),
        "Should contain leading text: {}",
        result
    );
    assert!(result.contains("Link"), "Should contain Link: {}", result);
    assert!(
        result.contains("Str \"more\""),
        "Should contain trailing text: {}",
        result
    );
}

/// Test link with attributes
#[test]
fn test_link_with_attributes() {
    let input = "[link](url){#myid .class}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Link"), "Should contain Link: {}", result);
    assert!(result.contains("\"myid\""), "Should contain id: {}", result);
    assert!(
        result.contains("\"class\""),
        "Should contain class: {}",
        result
    );
}

/// Test link with nested formatting
#[test]
fn test_link_with_formatting() {
    let input = "[**bold** text](url)";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Link"), "Should contain Link: {}", result);
    assert!(
        result.contains("Strong"),
        "Should contain Strong: {}",
        result
    );
}

// ============================================================================
// Span Tests
// ============================================================================

/// Test basic span
#[test]
fn test_span_basic() {
    let input = "[text]{.class}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Span"), "Should contain Span: {}", result);
    assert!(
        result.contains("\"class\""),
        "Should contain class: {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain text: {}",
        result
    );
}

/// Test span with full attributes
#[test]
fn test_span_with_full_attributes() {
    let input = r#"[text]{#myid .c1 .c2 key="value"}"#;
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Span"), "Should contain Span: {}", result);
    assert!(result.contains("\"myid\""), "Should contain id: {}", result);
    assert!(result.contains("\"c1\""), "Should contain c1: {}", result);
    assert!(result.contains("\"c2\""), "Should contain c2: {}", result);
    assert!(result.contains("\"key\""), "Should contain key: {}", result);
    assert!(
        result.contains("\"value\""),
        "Should contain value: {}",
        result
    );
}

/// Test span with empty attributes - QMD difference!
#[test]
fn test_span_empty_attributes() {
    let input = "[text]{}";
    let result = parse_qmd_to_pandoc_ast(input);

    // QMD produces Span with empty attributes
    assert!(result.contains("Span"), "Should contain Span: {}", result);
    assert!(
        result.contains("Str \"text\""),
        "Should contain text: {}",
        result
    );
}

/// Test bare brackets - QMD difference!
#[test]
fn test_bare_brackets() {
    let input = "[text]";
    let result = parse_qmd_to_pandoc_ast(input);

    // QMD produces Span with empty attributes (differs from Pandoc)
    assert!(result.contains("Span"), "Should contain Span: {}", result);
    assert!(
        result.contains("Str \"text\""),
        "Should contain text: {}",
        result
    );
}

// ============================================================================
// Image Tests
// ============================================================================

/// Test basic image (inline)
#[test]
fn test_image_basic() {
    let input = "text ![alt](img.png) more";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Image"), "Should contain Image: {}", result);
    assert!(
        result.contains("\"img.png\""),
        "Should contain URL: {}",
        result
    );
    assert!(
        result.contains("Str \"alt\""),
        "Should contain alt text: {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain leading text: {}",
        result
    );
}

/// Test image with title
#[test]
fn test_image_with_title() {
    let input = r#"![alt](img.png "title text")"#;
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Image"), "Should contain Image: {}", result);
    assert!(
        result.contains("\"title text\""),
        "Should contain title: {}",
        result
    );
}

/// Test image with attributes
#[test]
fn test_image_with_attributes() {
    let input = "![alt](img.png){.class}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Image"), "Should contain Image: {}", result);
    assert!(
        result.contains("\"class\""),
        "Should contain class: {}",
        result
    );
}

// ============================================================================
// Quoted text tests
// ============================================================================

/// Test basic single quote
#[test]
fn test_single_quote_basic() {
    let input = "'text'";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Quoted"),
        "Should contain Quoted: {}",
        result
    );
    assert!(
        result.contains("SingleQuote"),
        "Should contain SingleQuote: {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain quoted text: {}",
        result
    );
}

/// Test basic double quote
#[test]
fn test_double_quote_basic() {
    let input = "\"text\"";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Quoted"),
        "Should contain Quoted: {}",
        result
    );
    assert!(
        result.contains("DoubleQuote"),
        "Should contain DoubleQuote: {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain quoted text: {}",
        result
    );
}

/// Test single quote in context
#[test]
fn test_single_quote_in_context() {
    let input = "before 'quoted' after";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"before\""),
        "Should contain 'before': {}",
        result
    );
    assert!(
        result.contains("Quoted"),
        "Should contain Quoted: {}",
        result
    );
    assert!(
        result.contains("SingleQuote"),
        "Should contain SingleQuote: {}",
        result
    );
    assert!(
        result.contains("Str \"quoted\""),
        "Should contain quoted text: {}",
        result
    );
    assert!(
        result.contains("Str \"after\""),
        "Should contain 'after': {}",
        result
    );
}

/// Test double quote in context
#[test]
fn test_double_quote_in_context() {
    let input = "before \"quoted\" after";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"before\""),
        "Should contain 'before': {}",
        result
    );
    assert!(
        result.contains("Quoted"),
        "Should contain Quoted: {}",
        result
    );
    assert!(
        result.contains("DoubleQuote"),
        "Should contain DoubleQuote: {}",
        result
    );
    assert!(
        result.contains("Str \"quoted\""),
        "Should contain quoted text: {}",
        result
    );
    assert!(
        result.contains("Str \"after\""),
        "Should contain 'after': {}",
        result
    );
}

/// Test nested quotes: single inside double
#[test]
fn test_nested_single_in_double() {
    let input = "\"outer 'inner' text\"";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should have outer DoubleQuote
    assert!(
        result.contains("DoubleQuote"),
        "Should contain DoubleQuote: {}",
        result
    );
    // Should have inner SingleQuote nested inside
    assert!(
        result.contains("SingleQuote"),
        "Should contain nested SingleQuote: {}",
        result
    );
    assert!(
        result.contains("Str \"outer\""),
        "Should contain 'outer': {}",
        result
    );
    assert!(
        result.contains("Str \"inner\""),
        "Should contain 'inner': {}",
        result
    );
}

/// Test nested quotes: double inside single
#[test]
fn test_nested_double_in_single() {
    let input = "'outer \"inner\" text'";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should have outer SingleQuote
    assert!(
        result.contains("SingleQuote"),
        "Should contain SingleQuote: {}",
        result
    );
    // Should have inner DoubleQuote nested inside
    assert!(
        result.contains("DoubleQuote"),
        "Should contain nested DoubleQuote: {}",
        result
    );
    assert!(
        result.contains("Str \"outer\""),
        "Should contain 'outer': {}",
        result
    );
    assert!(
        result.contains("Str \"inner\""),
        "Should contain 'inner': {}",
        result
    );
}

/// Test quotes with formatting
#[test]
fn test_quotes_with_formatting() {
    let input = "\"**bold** text\"";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Quoted"),
        "Should contain Quoted: {}",
        result
    );
    assert!(
        result.contains("DoubleQuote"),
        "Should contain DoubleQuote: {}",
        result
    );
    assert!(
        result.contains("Strong"),
        "Should contain Strong: {}",
        result
    );
    assert!(
        result.contains("Str \"bold\""),
        "Should contain bold text: {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain plain text: {}",
        result
    );
}

/// Test quotes with multiple words
#[test]
fn test_quotes_multiple_words() {
    let input = "'multiple word quote'";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Quoted"),
        "Should contain Quoted: {}",
        result
    );
    assert!(
        result.contains("SingleQuote"),
        "Should contain SingleQuote: {}",
        result
    );
    assert!(
        result.contains("Str \"multiple\""),
        "Should contain 'multiple': {}",
        result
    );
    assert!(
        result.contains("Str \"word\""),
        "Should contain 'word': {}",
        result
    );
    assert!(
        result.contains("Str \"quote\""),
        "Should contain 'quote': {}",
        result
    );
}

/// Test empty quotes (edge case)
#[test]
fn test_empty_quotes() {
    let input_single = "''";
    let result_single = parse_qmd_to_pandoc_ast(input_single);

    assert!(
        result_single.contains("Quoted"),
        "Should contain Quoted for single: {}",
        result_single
    );
    assert!(
        result_single.contains("SingleQuote"),
        "Should contain SingleQuote: {}",
        result_single
    );

    let input_double = "\"\"";
    let result_double = parse_qmd_to_pandoc_ast(input_double);

    assert!(
        result_double.contains("Quoted"),
        "Should contain Quoted for double: {}",
        result_double
    );
    assert!(
        result_double.contains("DoubleQuote"),
        "Should contain DoubleQuote: {}",
        result_double
    );
}

// ============================================================================
// Strikeout tests
// ============================================================================

/// Test basic strikeout
#[test]
fn test_strikeout_basic() {
    let input = "~~strikeout~~";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Strikeout"),
        "Should contain Strikeout: {}",
        result
    );
    assert!(
        result.contains("Str \"strikeout\""),
        "Should contain text: {}",
        result
    );
}

/// Test strikeout in context
#[test]
fn test_strikeout_in_context() {
    let input = "before ~~strikeout~~ after";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"before\""),
        "Should contain 'before': {}",
        result
    );
    assert!(
        result.contains("Strikeout"),
        "Should contain Strikeout: {}",
        result
    );
    assert!(
        result.contains("Str \"strikeout\""),
        "Should contain strikeout text: {}",
        result
    );
    assert!(
        result.contains("Str \"after\""),
        "Should contain 'after': {}",
        result
    );
}

/// Test strikeout with multiple words
#[test]
fn test_strikeout_multiple_words() {
    let input = "~~multiple words~~";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Strikeout"),
        "Should contain Strikeout: {}",
        result
    );
    assert!(
        result.contains("Str \"multiple\""),
        "Should contain 'multiple': {}",
        result
    );
    assert!(
        result.contains("Str \"words\""),
        "Should contain 'words': {}",
        result
    );
}

/// Test strikeout with formatting (NOTE: Currently fails - nested formatting in strikeout not fully supported)
#[test]
#[ignore]
fn test_strikeout_with_formatting() {
    let input = "~~**bold** text~~";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Strikeout"),
        "Should contain Strikeout: {}",
        result
    );
    assert!(
        result.contains("Strong"),
        "Should contain Strong: {}",
        result
    );
    assert!(
        result.contains("Str \"bold\""),
        "Should contain 'bold': {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain 'text': {}",
        result
    );
}

// ============================================================================
// Editorial Marks Tests
// ============================================================================

/// Test insert - basic
#[test]
fn test_insert_basic() {
    let input = "[++ inserted text]";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Span"), "Should contain Span: {}", result);
    assert!(
        result.contains("\"quarto-insert\""),
        "Should contain 'quarto-insert' class: {}",
        result
    );
    assert!(
        result.contains("Str \"inserted\""),
        "Should contain 'inserted': {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain 'text': {}",
        result
    );
}

/// Test insert in context
#[test]
fn test_insert_in_context() {
    let input = "before [++ inserted] after";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"before\""),
        "Should contain 'before': {}",
        result
    );
    assert!(
        result.contains("\"quarto-insert\""),
        "Should contain 'quarto-insert' class: {}",
        result
    );
    assert!(
        result.contains("Str \"inserted\""),
        "Should contain 'inserted': {}",
        result
    );
    assert!(
        result.contains("Str \"after\""),
        "Should contain 'after': {}",
        result
    );
}

/// Test insert with attributes
#[test]
fn test_insert_with_attributes() {
    let input = "[++ inserted text]{.insert-class}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-insert\""),
        "Should contain 'quarto-insert' class: {}",
        result
    );
    assert!(
        result.contains("Str \"inserted\""),
        "Should contain 'inserted': {}",
        result
    );
    assert!(
        result.contains("\"insert-class\""),
        "Should contain class: {}",
        result
    );
}

/// Test delete - basic
#[test]
fn test_delete_basic() {
    let input = "[-- deleted text]";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-delete\""),
        "Should contain 'quarto-delete' class: {}",
        result
    );
    assert!(
        result.contains("Str \"deleted\""),
        "Should contain 'deleted': {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain 'text': {}",
        result
    );
}

/// Test delete in context
#[test]
fn test_delete_in_context() {
    let input = "before [-- deleted] after";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"before\""),
        "Should contain 'before': {}",
        result
    );
    assert!(
        result.contains("\"quarto-delete\""),
        "Should contain 'quarto-delete' class: {}",
        result
    );
    assert!(
        result.contains("Str \"deleted\""),
        "Should contain 'deleted': {}",
        result
    );
    assert!(
        result.contains("Str \"after\""),
        "Should contain 'after': {}",
        result
    );
}

/// Test delete with attributes
#[test]
fn test_delete_with_attributes() {
    let input = "[-- deleted text]{key=\"value\"}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-delete\""),
        "Should contain 'quarto-delete' class: {}",
        result
    );
    assert!(
        result.contains("Str \"deleted\""),
        "Should contain 'deleted': {}",
        result
    );
    assert!(result.contains("\"key\""), "Should contain key: {}", result);
    assert!(
        result.contains("\"value\""),
        "Should contain value: {}",
        result
    );
}

/// Test highlight - basic
#[test]
fn test_highlight_basic() {
    let input = "[!! highlighted text]";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-highlight\""),
        "Should contain 'quarto-highlight' class: {}",
        result
    );
    assert!(
        result.contains("Str \"highlighted\""),
        "Should contain 'highlighted': {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain 'text': {}",
        result
    );
}

/// Test highlight in context
#[test]
fn test_highlight_in_context() {
    let input = "before [!! highlighted] after";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"before\""),
        "Should contain 'before': {}",
        result
    );
    assert!(
        result.contains("\"quarto-highlight\""),
        "Should contain 'quarto-highlight' class: {}",
        result
    );
    assert!(
        result.contains("Str \"highlighted\""),
        "Should contain 'highlighted': {}",
        result
    );
    assert!(
        result.contains("Str \"after\""),
        "Should contain 'after': {}",
        result
    );
}

/// Test highlight with attributes
#[test]
fn test_highlight_with_attributes() {
    let input = "[!! highlighted text]{#my-id .myclass}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-highlight\""),
        "Should contain 'quarto-highlight' class: {}",
        result
    );
    assert!(
        result.contains("Str \"highlighted\""),
        "Should contain 'highlighted': {}",
        result
    );
    assert!(
        result.contains("\"my-id\""),
        "Should contain id: {}",
        result
    );
    assert!(
        result.contains("\"myclass\""),
        "Should contain class: {}",
        result
    );
}

/// Test edit comment - basic
#[test]
fn test_edit_comment_basic() {
    let input = "[>> comment text]";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-edit-comment\""),
        "Should contain 'quarto-edit-comment' class: {}",
        result
    );
    assert!(
        result.contains("Str \"comment\""),
        "Should contain 'comment': {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain 'text': {}",
        result
    );
}

/// Test edit comment in context
#[test]
fn test_edit_comment_in_context() {
    let input = "before [>> comment] after";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"before\""),
        "Should contain 'before': {}",
        result
    );
    assert!(
        result.contains("\"quarto-edit-comment\""),
        "Should contain 'quarto-edit-comment' class: {}",
        result
    );
    assert!(
        result.contains("Str \"comment\""),
        "Should contain 'comment': {}",
        result
    );
    assert!(
        result.contains("Str \"after\""),
        "Should contain 'after': {}",
        result
    );
}

/// Test edit comment with attributes
#[test]
fn test_edit_comment_with_attributes() {
    let input = "[>> comment text]{#comment-id .comment-class key=\"value\"}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-edit-comment\""),
        "Should contain 'quarto-edit-comment' class: {}",
        result
    );
    assert!(
        result.contains("Str \"comment\""),
        "Should contain 'comment': {}",
        result
    );
    assert!(
        result.contains("\"comment-id\""),
        "Should contain id: {}",
        result
    );
    assert!(
        result.contains("\"comment-class\""),
        "Should contain class: {}",
        result
    );
    assert!(result.contains("\"key\""), "Should contain key: {}", result);
    assert!(
        result.contains("\"value\""),
        "Should contain value: {}",
        result
    );
}

/// Test all editorial marks together
#[test]
fn test_editorial_marks_combined() {
    let input = "Text with [++ insert], [-- delete], [!! highlight], and [>> comment].";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-insert\""),
        "Should contain 'quarto-insert' class: {}",
        result
    );
    assert!(
        result.contains("\"quarto-delete\""),
        "Should contain 'quarto-delete' class: {}",
        result
    );
    assert!(
        result.contains("\"quarto-highlight\""),
        "Should contain 'quarto-highlight' class: {}",
        result
    );
    assert!(
        result.contains("\"quarto-edit-comment\""),
        "Should contain 'quarto-edit-comment' class: {}",
        result
    );
}

// ============================================================================
// Shortcode Tests
// ============================================================================

/// Test basic shortcode - name only
#[test]
fn test_shortcode_basic() {
    let input = "{{< myshortcode >}}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-shortcode__\""),
        "Should contain shortcode class: {}",
        result
    );
    assert!(
        result.contains("\"myshortcode\""),
        "Should contain shortcode name: {}",
        result
    );
}

/// Test shortcode in context
#[test]
fn test_shortcode_in_context() {
    let input = "before {{< name >}} after";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"before\""),
        "Should contain 'before': {}",
        result
    );
    assert!(
        result.contains("\"quarto-shortcode__\""),
        "Should contain shortcode class: {}",
        result
    );
    assert!(
        result.contains("\"name\""),
        "Should contain shortcode name: {}",
        result
    );
    assert!(
        result.contains("Str \"after\""),
        "Should contain 'after': {}",
        result
    );
}

/// Test shortcode with single positional argument
#[test]
fn test_shortcode_with_positional_arg() {
    let input = "{{< name arg1 >}}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-shortcode__\""),
        "Should contain shortcode class: {}",
        result
    );
    assert!(
        result.contains("\"name\""),
        "Should contain shortcode name: {}",
        result
    );
    assert!(
        result.contains("\"arg1\""),
        "Should contain positional arg: {}",
        result
    );
}

/// Test shortcode with multiple positional arguments
#[test]
fn test_shortcode_with_multiple_args() {
    let input = "{{< name arg1 arg2 arg3 >}}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-shortcode__\""),
        "Should contain shortcode class: {}",
        result
    );
    assert!(
        result.contains("\"arg1\""),
        "Should contain arg1: {}",
        result
    );
    assert!(
        result.contains("\"arg2\""),
        "Should contain arg2: {}",
        result
    );
    assert!(
        result.contains("\"arg3\""),
        "Should contain arg3: {}",
        result
    );
}

/// Test shortcode with keyword argument
#[test]
fn test_shortcode_with_keyword_arg() {
    let input = "{{< name key=\"value\" >}}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-shortcode__\""),
        "Should contain shortcode class: {}",
        result
    );
    assert!(
        result.contains("\"name\""),
        "Should contain shortcode name: {}",
        result
    );
    assert!(
        result.contains("\"key\""),
        "Should contain keyword name: {}",
        result
    );
    assert!(
        result.contains("\"value\""),
        "Should contain keyword value: {}",
        result
    );
}

/// Test shortcode with mixed arguments
/// Note: positional args must come before keyword args (grammar restriction)
#[test]
fn test_shortcode_with_mixed_args() {
    let input = "{{< name pos1 pos2 key1=\"val1\" key2=\"val2\" >}}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-shortcode__\""),
        "Should contain shortcode class: {}",
        result
    );
    assert!(
        result.contains("\"pos1\""),
        "Should contain pos1: {}",
        result
    );
    assert!(
        result.contains("\"pos2\""),
        "Should contain pos2: {}",
        result
    );
    assert!(
        result.contains("\"key1\""),
        "Should contain key1: {}",
        result
    );
    assert!(
        result.contains("\"val1\""),
        "Should contain val1: {}",
        result
    );
    assert!(
        result.contains("\"key2\""),
        "Should contain key2: {}",
        result
    );
    assert!(
        result.contains("\"val2\""),
        "Should contain val2: {}",
        result
    );
}

/// Test shortcode with boolean argument
#[test]
fn test_shortcode_with_boolean() {
    let input = "{{< name flag=true >}}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-shortcode__\""),
        "Should contain shortcode class: {}",
        result
    );
    assert!(
        result.contains("\"flag\""),
        "Should contain flag name: {}",
        result
    );
    assert!(
        result.contains("true"),
        "Should contain boolean value: {}",
        result
    );
}

/// Test shortcode with number argument
#[test]
fn test_shortcode_with_number() {
    let input = "{{< name count=42 >}}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-shortcode__\""),
        "Should contain shortcode class: {}",
        result
    );
    assert!(
        result.contains("\"count\""),
        "Should contain count name: {}",
        result
    );
    assert!(
        result.contains("42"),
        "Should contain number value: {}",
        result
    );
}

/// Test escaped shortcode
#[test]
fn test_shortcode_escaped() {
    let input = "{{{< name >}}}";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("\"quarto-shortcode__\""),
        "Should contain shortcode class: {}",
        result
    );
    assert!(
        result.contains("\"name\""),
        "Should contain shortcode name: {}",
        result
    );
    // The escaped shortcode should have is_escaped = true
    // but the native output format may not show this explicitly
}

// ============================================================================
// Citation Tests
// ============================================================================

/// Test basic author-in-text citation
#[test]
fn test_citation_author_in_text() {
    let input = "See @smith2020 for details";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Cite"), "Should contain Cite: {}", result);
    assert!(
        result.contains("\"smith2020\""),
        "Should contain citation id: {}",
        result
    );
    assert!(
        result.contains("AuthorInText"),
        "Should contain AuthorInText mode: {}",
        result
    );
}

/// Test normal bracketed citation
#[test]
fn test_citation_normal() {
    let input = "See [@smith2020] for details";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Cite"), "Should contain Cite: {}", result);
    assert!(
        result.contains("\"smith2020\""),
        "Should contain citation id: {}",
        result
    );
    assert!(
        result.contains("NormalCitation"),
        "Should contain NormalCitation mode: {}",
        result
    );
}

/// Test suppress author citation
#[test]
fn test_citation_suppress_author() {
    let input = "See [-@smith2020] for details";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Cite"), "Should contain Cite: {}", result);
    assert!(
        result.contains("\"smith2020\""),
        "Should contain citation id: {}",
        result
    );
    assert!(
        result.contains("SuppressAuthor"),
        "Should contain SuppressAuthor mode: {}",
        result
    );
}

/// Test citation in context
#[test]
fn test_citation_in_context() {
    let input = "before @citekey after";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("Str \"before\""),
        "Should contain 'before': {}",
        result
    );
    assert!(result.contains("Cite"), "Should contain Cite: {}", result);
    assert!(
        result.contains("\"citekey\""),
        "Should contain citation id: {}",
        result
    );
    assert!(
        result.contains("Str \"after\""),
        "Should contain 'after': {}",
        result
    );
}

/// Test citation with underscore and numbers
#[test]
fn test_citation_complex_id() {
    let input = "@smith_jones_2020 is cited";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Cite"), "Should contain Cite: {}", result);
    assert!(
        result.contains("\"smith_jones_2020\""),
        "Should contain complex citation id: {}",
        result
    );
}

// ============================================================================
// Citation Whitespace Tests
// ============================================================================

/// Test citation WITH leading space (should inject Space node)
#[test]
fn test_citation_with_leading_space() {
    let input = "Hi @cite";
    let result = parse_qmd_to_json(input);

    // Should have a Space node between "Hi" and the Cite
    assert!(
        result.contains("\"t\":\"Space\""),
        "Should contain Space node: {}",
        result
    );
    assert!(
        result.contains("\"t\":\"Cite\""),
        "Should contain Cite: {}",
        result
    );

    // The Cite content should NOT have leading space
    assert!(
        result.contains("\"@cite\""),
        "Should contain '@cite' without leading space: {}",
        result
    );
    assert!(
        !result.contains("\" @cite\""),
        "Should NOT contain ' @cite' with leading space: {}",
        result
    );

    // Verify the Space comes before the Cite in the JSON
    let space_pos = result.find("\"t\":\"Space\"").unwrap();
    let cite_pos = result.find("\"t\":\"Cite\"").unwrap();
    assert!(
        space_pos < cite_pos,
        "Space should appear before Cite in output: {}",
        result
    );
}

/// Test citation WITHOUT leading space (should NOT inject Space node)
#[test]
fn test_citation_without_leading_space() {
    let input = "Hi@cite";
    let result = parse_qmd_to_json(input);

    // Should NOT have a Space node
    assert!(
        !result.contains("\"t\":\"Space\""),
        "Should NOT contain Space node: {}",
        result
    );
    assert!(
        result.contains("\"t\":\"Cite\""),
        "Should contain Cite: {}",
        result
    );
    assert!(
        result.contains("\"Hi\""),
        "Should contain 'Hi' text: {}",
        result
    );
    assert!(
        result.contains("\"@cite\""),
        "Should contain '@cite': {}",
        result
    );
}

/// Test citation with leading AND trailing space
#[test]
fn test_citation_leading_and_trailing_space() {
    let input = "Hi @cite bye";
    let result = parse_qmd_to_json(input);

    // Should have TWO Space nodes (one injected for leading, one from tree-sitter for trailing)
    let space_count = result.matches("\"t\":\"Space\"").count();
    assert_eq!(
        space_count, 2,
        "Should have exactly 2 Space nodes: {}",
        result
    );
    assert!(
        result.contains("\"t\":\"Cite\""),
        "Should contain Cite: {}",
        result
    );
}

/// Test citation at start of paragraph (no leading space possible)
#[test]
fn test_citation_paragraph_start() {
    let input = "@cite word";
    let result = parse_qmd_to_json(input);

    // Should have exactly one Space node (trailing space from tree-sitter)
    let space_count = result.matches("\"t\":\"Space\"").count();
    assert_eq!(
        space_count, 1,
        "Should have exactly 1 Space node (trailing): {}",
        result
    );
    assert!(
        result.contains("\"t\":\"Cite\""),
        "Should contain Cite: {}",
        result
    );

    // Verify the Cite comes before the Space in the JSON
    let cite_pos = result.find("\"t\":\"Cite\"").unwrap();
    let space_pos = result.find("\"t\":\"Space\"").unwrap();
    assert!(
        cite_pos < space_pos,
        "Cite should appear before Space in output: {}",
        result
    );
}

/// Test multiple citations with different spacing patterns
#[test]
fn test_citation_multiple_spacing_patterns() {
    let input = "A@cite1 B @cite2C@cite3 D";
    let result = parse_qmd_to_json(input);

    // Expected pattern: A, @cite1, Space, B, Space, @cite2, C, @cite3, Space, D
    // Space count: 1 (after cite1) + 1 (injected before cite2) + 1 (after cite3) = 3
    let space_count = result.matches("\"t\":\"Space\"").count();
    assert_eq!(
        space_count, 3,
        "Should have exactly 3 Space nodes: {}",
        result
    );

    // Verify all three citations are present
    let cite_count = result.matches("\"t\":\"Cite\"").count();
    assert_eq!(cite_count, 3, "Should have exactly 3 citations: {}", result);
}

/// Test suppress author citation with leading space
#[test]
fn test_citation_suppress_author_with_leading_space() {
    let input = "Hi [-@cite]";
    let result = parse_qmd_to_json(input);

    // Should have a Space node
    assert!(
        result.contains("\"t\":\"Space\""),
        "Should contain Space node: {}",
        result
    );
    assert!(
        result.contains("\"t\":\"Cite\""),
        "Should contain Cite: {}",
        result
    );
    assert!(
        result.contains("\"SuppressAuthor\""),
        "Should be SuppressAuthor citation: {}",
        result
    );

    // The Cite content should NOT have leading space
    assert!(
        !result.contains("\" [-@cite\""),
        "Should NOT contain leading space in citation content: {}",
        result
    );
}

// ============================================================================
// Block Quote Tests
// ============================================================================

/// Test basic single-line block quote
#[test]
fn test_block_quote_basic() {
    let input = "> quote";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"BlockQuote\""),
        "Should contain BlockQuote: {}",
        result
    );
    assert!(
        result.contains("\"quote\""),
        "Should contain quoted text: {}",
        result
    );
}

/// Test multi-line block quote
#[test]
fn test_block_quote_multiline() {
    let input = "> first line\n> second line";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"BlockQuote\""),
        "Should contain BlockQuote: {}",
        result
    );
    assert!(
        result.contains("\"first\""),
        "Should contain first line: {}",
        result
    );
    assert!(
        result.contains("\"second\""),
        "Should contain second line: {}",
        result
    );
}

/// Test block quote with paragraph continuation (no > on next line)
#[test]
fn test_block_quote_lazy_continuation() {
    let input = "> first line\ncontinued";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"BlockQuote\""),
        "Should contain BlockQuote: {}",
        result
    );
    assert!(
        result.contains("\"first\""),
        "Should contain first line: {}",
        result
    );
    assert!(
        result.contains("\"continued\""),
        "Should contain continued line: {}",
        result
    );
}

/// Test nested block quotes
#[test]
fn test_block_quote_nested() {
    let input = "> outer\n>> inner";
    let result = parse_qmd_to_json(input);

    // Should have two BlockQuote nodes
    let quote_count = result.matches("\"t\":\"BlockQuote\"").count();
    assert!(
        quote_count >= 2,
        "Should have at least 2 BlockQuote nodes (nested): {}",
        result
    );
    assert!(
        result.contains("\"outer\""),
        "Should contain outer quote: {}",
        result
    );
    assert!(
        result.contains("\"inner\""),
        "Should contain inner quote: {}",
        result
    );
}

/// Test block quote in context (between paragraphs)
#[test]
fn test_block_quote_in_context() {
    let input = "before\n\n> quote\n\nafter";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"BlockQuote\""),
        "Should contain BlockQuote: {}",
        result
    );
    assert!(
        result.contains("\"before\""),
        "Should contain before text: {}",
        result
    );
    assert!(
        result.contains("\"quote\""),
        "Should contain quoted text: {}",
        result
    );
    assert!(
        result.contains("\"after\""),
        "Should contain after text: {}",
        result
    );
}

// ============================================================================
// Horizontal Rule Tests
// ============================================================================

/// Test basic horizontal rule
#[test]
fn test_horizontal_rule_basic() {
    let input = "---\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"HorizontalRule\""),
        "Should contain HorizontalRule: {}",
        result
    );
}

/// Test horizontal rule between paragraphs
#[test]
fn test_horizontal_rule_in_context() {
    let input = "before\n\n---\n\nafter";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"HorizontalRule\""),
        "Should contain HorizontalRule: {}",
        result
    );
    assert!(
        result.contains("\"before\""),
        "Should contain before text: {}",
        result
    );
    assert!(
        result.contains("\"after\""),
        "Should contain after text: {}",
        result
    );
}

/// Test multiple horizontal rules
#[test]
fn test_horizontal_rule_multiple() {
    let input = "---\n\ntext\n\n---\n";
    let result = parse_qmd_to_json(input);

    let rule_count = result.matches("\"t\":\"HorizontalRule\"").count();
    assert_eq!(
        rule_count, 2,
        "Should have exactly 2 HorizontalRule nodes: {}",
        result
    );
}

// ============================================================================
// Code Block Tests
// ============================================================================

/// Test basic code block without language
#[test]
fn test_code_block_basic() {
    let input = "```\ncode here\n```\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"CodeBlock\""),
        "Should contain CodeBlock: {}",
        result
    );
    assert!(
        result.contains("code here"),
        "Should contain code text: {}",
        result
    );
}

/// Test code block with language
#[test]
fn test_code_block_with_language() {
    let input = "```python\nprint('hello')\n```\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"CodeBlock\""),
        "Should contain CodeBlock: {}",
        result
    );
    assert!(
        result.contains("python"),
        "Should contain language: {}",
        result
    );
    assert!(
        result.contains("print('hello')"),
        "Should contain code text: {}",
        result
    );
}

/// Test empty code block
#[test]
fn test_code_block_empty() {
    let input = "```\n```\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"CodeBlock\""),
        "Should contain CodeBlock: {}",
        result
    );
}

/// Test code block with attributes
/// NOTE: Skipped - complex attribute syntax `{python #id .class}` not yet supported by grammar
#[test]
#[ignore]
fn test_code_block_with_attributes() {
    let input = "```{python #my-code .class}\ncode\n```\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"CodeBlock\""),
        "Should contain CodeBlock: {}",
        result
    );
    assert!(result.contains("my-code"), "Should contain id: {}", result);
    assert!(result.contains("class"), "Should contain class: {}", result);
}

/// Test multi-line code block
#[test]
fn test_code_block_multiline() {
    let input = "```\nline 1\nline 2\nline 3\n```\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"CodeBlock\""),
        "Should contain CodeBlock: {}",
        result
    );
    assert!(
        result.contains("line 1"),
        "Should contain first line: {}",
        result
    );
    assert!(
        result.contains("line 3"),
        "Should contain last line: {}",
        result
    );
}

// ============================================================================
// Div Tests
// ============================================================================

/// Test basic div with info string (naked value)
#[test]
fn test_div_basic() {
    let input = "::: note\nContent inside div\n:::\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"Div\""),
        "Should contain Div: {}",
        result
    );
    assert!(
        result.contains("Content") && result.contains("inside") && result.contains("div"),
        "Should contain content: {}",
        result
    );
    assert!(
        result.contains("note"),
        "Should contain info string: {}",
        result
    );
}

/// Test div with classes
#[test]
fn test_div_with_class() {
    let input = "::: {.callout-note}\nThis is a note\n:::\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"Div\""),
        "Should contain Div: {}",
        result
    );
    assert!(
        result.contains("callout-note"),
        "Should contain class: {}",
        result
    );
}

/// Test div with id
#[test]
fn test_div_with_id() {
    let input = "::: {#my-div}\nContent\n:::\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"Div\""),
        "Should contain Div: {}",
        result
    );
    assert!(result.contains("my-div"), "Should contain id: {}", result);
}

/// Test empty div (with info string)
#[test]
fn test_div_empty() {
    let input = "::: empty\n:::\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"Div\""),
        "Should contain Div: {}",
        result
    );
}

/// Test nested divs
#[test]
fn test_div_nested() {
    let input = "::: {.outer}\n::: {.inner}\nNested content\n:::\n:::\n";
    let result = parse_qmd_to_json(input);

    let div_count = result.matches("\"t\":\"Div\"").count();
    assert_eq!(div_count, 2, "Should have exactly 2 Div nodes: {}", result);
    assert!(
        result.contains("outer"),
        "Should contain outer class: {}",
        result
    );
    assert!(
        result.contains("inner"),
        "Should contain inner class: {}",
        result
    );
}

// ============================================================================
// Fenced Note Definition Tests
// ============================================================================

/// Test basic single-block fenced note definition
#[test]
fn test_note_def_fenced_basic() {
    let input = "::: ^mynote\nThis is a note.\n:::\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("^mynote") || result.contains("mynote"),
        "Should contain note id: {}",
        result
    );
    assert!(
        result.contains("This") && result.contains("note"),
        "Should contain note content: {}",
        result
    );
}

/// Test multi-block fenced note definition
#[test]
fn test_note_def_fenced_multiblock() {
    let input = "::: ^note2\nFirst paragraph.\n\nSecond paragraph.\n:::\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("note2"),
        "Should contain note id: {}",
        result
    );
    assert!(
        result.contains("First") && result.contains("paragraph"),
        "Should contain first paragraph: {}",
        result
    );
    assert!(
        result.contains("Second"),
        "Should contain second paragraph: {}",
        result
    );
}

/// Test fenced note with complex content
#[test]
fn test_note_def_fenced_complex() {
    let input = "::: ^complex\nA paragraph.\n\n> A quote\n:::\n";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("complex"),
        "Should contain note id: {}",
        result
    );
    assert!(
        result.contains("paragraph"),
        "Should contain paragraph: {}",
        result
    );
    assert!(
        result.contains("BlockQuote") || result.contains("quote"),
        "Should contain quote: {}",
        result
    );
}

// ============================================================================
// List Tests
// ============================================================================

/// Test simple bullet list with - marker
#[test]
fn test_bullet_list_simple() {
    let input = "- Item 1\n- Item 2\n- Item 3\n";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("BulletList"),
        "Should contain BulletList: {}",
        result
    );
    assert!(
        result.contains("Item") && result.contains("1"),
        "Should contain first item: {}",
        result
    );
    assert!(
        result.contains("2") && result.contains("3"),
        "Should contain other items: {}",
        result
    );
}

/// Test simple ordered list
#[test]
fn test_ordered_list_simple() {
    let input = "1. First item\n2. Second item\n3. Third item\n";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("OrderedList"),
        "Should contain OrderedList: {}",
        result
    );
    assert!(
        result.contains("First"),
        "Should contain first item: {}",
        result
    );
    assert!(
        result.contains("Second") && result.contains("Third"),
        "Should contain other items: {}",
        result
    );
}

/// Test nested bullet lists
#[test]
fn test_bullet_list_nested() {
    let input = "- Outer 1\n  - Inner 1\n  - Inner 2\n- Outer 2\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should have 2 BulletList nodes (outer and inner)
    let list_count = result.matches("BulletList").count();
    assert!(
        list_count >= 2,
        "Should have at least 2 BulletList nodes for nesting: {}",
        result
    );
    assert!(
        result.contains("Outer"),
        "Should contain outer items: {}",
        result
    );
    assert!(
        result.contains("Inner"),
        "Should contain inner items: {}",
        result
    );
}

/// Test nested ordered lists
#[test]
fn test_ordered_list_nested() {
    let input = "1. First\n   1. Nested first\n   2. Nested second\n2. Second\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should have 2 OrderedList nodes (outer and inner)
    let list_count = result.matches("OrderedList").count();
    assert!(
        list_count >= 2,
        "Should have at least 2 OrderedList nodes for nesting: {}",
        result
    );
    assert!(
        result.contains("First") && result.contains("Second"),
        "Should contain outer items: {}",
        result
    );
    assert!(
        result.contains("Nested"),
        "Should contain nested items: {}",
        result
    );
}

/// Test tight vs loose lists
/// A tight list has no blank lines between items
#[test]
fn test_list_tight() {
    let input = "- Item 1\n- Item 2\n- Item 3\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // In a tight list, items should be Plain, not Para
    assert!(
        result.contains("Plain"),
        "Tight list should contain Plain nodes: {}",
        result
    );
}

/// Test loose list (item with multiple paragraphs makes list loose)
#[test]
fn test_list_loose() {
    let input = "- First paragraph in item 1\n\n  Second paragraph in item 1\n\n- Item 2\n";
    let result = parse_qmd_to_pandoc_ast(input);

    // A list with an item containing multiple paragraphs is loose
    assert!(
        result.contains("Para"),
        "Loose list should contain Para nodes: {}",
        result
    );
    // Should have multiple Para nodes
    let para_count = result.matches("Para").count();
    assert!(
        para_count >= 2,
        "Loose list should have multiple Para nodes: {}",
        result
    );
}

/// Test list with complex content (multiple paragraphs)
#[test]
fn test_list_complex_content() {
    let input = "- First paragraph\n\n  Second paragraph\n\n- Another item\n";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("BulletList"),
        "Should contain BulletList: {}",
        result
    );
    // Should have multiple Para nodes for the paragraphs
    let para_count = result.matches("Para").count();
    assert!(
        para_count >= 2,
        "Should have multiple Para nodes for complex content: {}",
        result
    );
}

/// Test ordered list with custom start number
#[test]
fn test_ordered_list_start_number() {
    let input = "5. Fifth item\n6. Sixth item\n7. Seventh item\n";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("OrderedList"),
        "Should contain OrderedList: {}",
        result
    );
    // The start number should be 5
    assert!(
        result.contains("(5,") || result.contains("( 5,") || result.contains("( 5 ,"),
        "Should have start number 5: {}",
        result
    );
}

/// Test bullet list with + marker
#[test]
fn test_bullet_list_plus() {
    let input = "+ Item 1\n+ Item 2\n";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("BulletList"),
        "Should contain BulletList with + marker: {}",
        result
    );
}

/// Test bullet list with * marker
#[test]
fn test_bullet_list_star() {
    let input = "* Item 1\n* Item 2\n";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("BulletList"),
        "Should contain BulletList with * marker: {}",
        result
    );
}

/// Test ordered list with ) delimiter
#[test]
fn test_ordered_list_paren() {
    let input = "1) First\n2) Second\n";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(
        result.contains("OrderedList"),
        "Should contain OrderedList with ) delimiter: {}",
        result
    );
}

// ============================================================================
// Inline Note Tests
// ============================================================================

/// Test basic inline note
#[test]
fn test_inline_note_basic() {
    let input = "^[note text]";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should produce: Para [ Note [ Para [ Str "note" , Space , Str "text" ] ] ]
    assert!(result.contains("Para"), "Should contain Para: {}", result);
    assert!(result.contains("Note"), "Should contain Note: {}", result);
    assert!(
        result.contains("Str \"note\""),
        "Should contain note text: {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain note text: {}",
        result
    );
}

/// Test inline note in context
#[test]
fn test_inline_note_in_context() {
    let input = "Some text^[with a note] here.";
    let result = parse_qmd_to_pandoc_ast(input);

    // Should contain the surrounding text and the note
    assert!(
        result.contains("Str \"Some\""),
        "Should contain 'Some': {}",
        result
    );
    assert!(result.contains("Note"), "Should contain Note: {}", result);
    assert!(
        result.contains("Str \"with\""),
        "Should contain note content: {}",
        result
    );
    assert!(
        result.contains("Str \"here.\""),
        "Should contain trailing text: {}",
        result
    );
}

/// Test inline note with single word
#[test]
fn test_inline_note_single_word() {
    let input = "text^[note]more";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Note"), "Should contain Note: {}", result);
    assert!(
        result.contains("Str \"note\""),
        "Should contain note text: {}",
        result
    );
    assert!(
        result.contains("Str \"text\""),
        "Should contain leading text: {}",
        result
    );
    assert!(
        result.contains("Str \"more\""),
        "Should contain trailing text: {}",
        result
    );
}

/// Test inline note with formatting
#[test]
fn test_inline_note_with_formatting() {
    let input = "^[**bold** text]";
    let result = parse_qmd_to_pandoc_ast(input);

    assert!(result.contains("Note"), "Should contain Note: {}", result);
    assert!(
        result.contains("Strong"),
        "Should contain Strong: {}",
        result
    );
    assert!(
        result.contains("Str \"bold\""),
        "Should contain bold text: {}",
        result
    );
}

/// Test multiple inline notes
#[test]
fn test_multiple_inline_notes() {
    let input = "First^[note one] and second^[note two].";
    let result = parse_qmd_to_pandoc_ast(input);

    // Count Note occurrences - should appear twice
    let note_count = result.matches("Note").count();
    assert_eq!(note_count, 2, "Should have 2 Note nodes: {}", result);

    assert!(
        result.contains("Str \"one\""),
        "Should contain first note: {}",
        result
    );
    assert!(
        result.contains("Str \"two\""),
        "Should contain second note: {}",
        result
    );
}

// ============================================================================
// Note Definition Tests (footnote definitions like [^id]: content)
// ============================================================================

/// Test basic note definition
#[test]
fn test_note_definition_basic() {
    let input = "[^note1]: This is the note content.";
    let result = parse_qmd_to_json(input);

    // Should produce: NoteDefinitionPara with id "note1" and content
    // JSON format: {"t":"NoteDefinitionPara","c":["note1",[...inlines...]]}
    assert!(
        result.contains("\"t\":\"NoteDefinitionPara\""),
        "Should contain NoteDefinitionPara: {}",
        result
    );
    assert!(
        result.contains("\"note1\""),
        "Should contain note ID: {}",
        result
    );
    assert!(
        result.contains("\"This\""),
        "Should contain note content: {}",
        result
    );
    assert!(
        result.contains("\"content.\""),
        "Should contain note content: {}",
        result
    );
}

/// Test note definition with simple ID
#[test]
fn test_note_definition_numeric() {
    let input = "[^1]: First note.";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"NoteDefinitionPara\""),
        "Should contain NoteDefinitionPara: {}",
        result
    );
    assert!(
        result.contains("\"1\""),
        "Should contain numeric note ID: {}",
        result
    );
    assert!(
        result.contains("\"First\""),
        "Should contain note content: {}",
        result
    );
}

/// Test note definition with multiword content
#[test]
fn test_note_definition_multiword() {
    let input = "[^ref]: This is a longer note with multiple words.";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"NoteDefinitionPara\""),
        "Should contain NoteDefinitionPara: {}",
        result
    );
    assert!(
        result.contains("\"ref\""),
        "Should contain note ID: {}",
        result
    );
    assert!(
        result.contains("\"longer\""),
        "Should contain note content: {}",
        result
    );
    assert!(
        result.contains("\"multiple\""),
        "Should contain note content: {}",
        result
    );
}

/// Test note definition with formatting
#[test]
fn test_note_definition_with_formatting() {
    let input = "[^fmt]: This has **bold** text.";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"t\":\"NoteDefinitionPara\""),
        "Should contain NoteDefinitionPara: {}",
        result
    );
    assert!(
        result.contains("\"fmt\""),
        "Should contain note ID: {}",
        result
    );
    assert!(
        result.contains("\"t\":\"Strong\""),
        "Should contain Strong formatting: {}",
        result
    );
    assert!(
        result.contains("\"bold\""),
        "Should contain formatted text: {}",
        result
    );
}

// ============================================================================
// Inline Note Reference Tests (footnote references like [^id])
// ============================================================================

/// Test basic inline note reference
#[test]
fn test_inline_note_reference_basic() {
    let input = "this [^ref]";
    let result = parse_qmd_to_json(input);

    // Should produce: Para [ Str "this", Span with class "quarto-note-reference" and reference-id="ref" ]
    assert!(
        result.contains("\"t\":\"Span\""),
        "Should contain Span: {}",
        result
    );
    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"reference-id\""),
        "Should contain reference-id key: {}",
        result
    );
    assert!(
        result.contains("\"ref\""),
        "Should contain note ID 'ref': {}",
        result
    );
    assert!(
        result.contains("\"this\""),
        "Should contain 'this' text: {}",
        result
    );
}

/// Test inline note reference with numeric ID
#[test]
fn test_inline_note_reference_numeric() {
    let input = "test [^123]";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"reference-id\""),
        "Should contain reference-id key: {}",
        result
    );
    assert!(
        result.contains("\"123\""),
        "Should contain numeric note ID '123': {}",
        result
    );
}

/// Test inline note reference with alphanumeric ID
#[test]
fn test_inline_note_reference_alphanumeric() {
    let input = "test [^note1]";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"note1\""),
        "Should contain alphanumeric note ID 'note1': {}",
        result
    );
}

/// Test inline note reference with hyphenated ID
#[test]
fn test_inline_note_reference_hyphenated() {
    let input = "test [^my-note]";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"my-note\""),
        "Should contain hyphenated note ID 'my-note': {}",
        result
    );
}

/// Test inline note reference with underscore ID
#[test]
fn test_inline_note_reference_underscore() {
    let input = "test [^my_note]";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"my_note\""),
        "Should contain underscore note ID 'my_note': {}",
        result
    );
}

/// Test inline note reference in context
#[test]
fn test_inline_note_reference_in_context() {
    let input = "before [^note] after";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"before\""),
        "Should contain 'before' text: {}",
        result
    );
    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"note\""),
        "Should contain note ID 'note': {}",
        result
    );
    assert!(
        result.contains("\"after\""),
        "Should contain 'after' text: {}",
        result
    );
}

/// Test multiple inline note references
#[test]
fn test_multiple_inline_note_references() {
    let input = "test [^foo] and [^bar]";
    let result = parse_qmd_to_json(input);

    // Count Span occurrences - should appear twice (once for each note reference)
    let span_count = result.matches("\"t\":\"Span\"").count();
    assert!(
        span_count >= 2,
        "Should have at least 2 Span nodes (one per note reference): {}",
        result
    );

    assert!(
        result.contains("\"foo\""),
        "Should contain first note ID 'foo': {}",
        result
    );
    assert!(
        result.contains("\"bar\""),
        "Should contain second note ID 'bar': {}",
        result
    );
    assert!(
        result.contains("\"and\""),
        "Should contain 'and' text: {}",
        result
    );
}

/// Test inline note reference with no space before
#[test]
fn test_inline_note_reference_no_space_before() {
    let input = "test[^note]";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"note\""),
        "Should contain note ID 'note': {}",
        result
    );
    assert!(
        result.contains("\"test\""),
        "Should contain 'test' text: {}",
        result
    );
}

/// Test inline note reference with no space after
#[test]
fn test_inline_note_reference_no_space_after() {
    let input = "[^note]test";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"note\""),
        "Should contain note ID 'note': {}",
        result
    );
    assert!(
        result.contains("\"test\""),
        "Should contain 'test' text: {}",
        result
    );
}

/// Test inline note reference at start of paragraph
#[test]
fn test_inline_note_reference_at_start() {
    let input = "[^note] at start";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"note\""),
        "Should contain note ID 'note': {}",
        result
    );
    assert!(
        result.contains("\"start\""),
        "Should contain 'start' text: {}",
        result
    );
}

/// Test inline note reference at end of paragraph
#[test]
fn test_inline_note_reference_at_end() {
    let input = "at end [^note]";
    let result = parse_qmd_to_json(input);

    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"note\""),
        "Should contain note ID 'note': {}",
        result
    );
    assert!(
        result.contains("\"end\""),
        "Should contain 'end' text: {}",
        result
    );
}

// ============================================================================
// Inline Note Reference Whitespace Tests
// ============================================================================

/// Test inline note reference WITH leading space (should inject Space node)
#[test]
fn test_inline_note_reference_with_leading_space() {
    let input = "Hi [^ref]";
    let result = parse_qmd_to_json(input);

    // Should have a Space node between "Hi" and the Span
    assert!(
        result.contains("\"t\":\"Space\""),
        "Should contain Space node: {}",
        result
    );
    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"ref\""),
        "Should contain note ID 'ref': {}",
        result
    );

    // Verify the Space comes before the Span in the JSON
    let space_pos = result.find("\"t\":\"Space\"").unwrap();
    let span_pos = result.find("\"t\":\"Span\"").unwrap();
    assert!(
        space_pos < span_pos,
        "Space should appear before Span in output: {}",
        result
    );
}

/// Test inline note reference WITHOUT leading space (should NOT inject Space node)
#[test]
fn test_inline_note_reference_without_leading_space() {
    let input = "Hi[^ref]";
    let result = parse_qmd_to_json(input);

    // Should NOT have a Space node
    assert!(
        !result.contains("\"t\":\"Space\""),
        "Should NOT contain Space node: {}",
        result
    );
    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
    assert!(
        result.contains("\"Hi\""),
        "Should contain 'Hi' text: {}",
        result
    );
    assert!(
        result.contains("\"ref\""),
        "Should contain note ID 'ref': {}",
        result
    );
}

/// Test inline note reference with leading AND trailing space
#[test]
fn test_inline_note_reference_leading_and_trailing_space() {
    let input = "Hi [^ref] bye";
    let result = parse_qmd_to_json(input);

    // Should have TWO Space nodes (one injected for leading, one from tree-sitter for trailing)
    let space_count = result.matches("\"t\":\"Space\"").count();
    assert_eq!(
        space_count, 2,
        "Should have exactly 2 Space nodes: {}",
        result
    );
    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );
}

/// Test inline note reference at start of paragraph (no leading space possible)
#[test]
fn test_inline_note_reference_paragraph_start() {
    let input = "[^ref] word";
    let result = parse_qmd_to_json(input);

    // Should have exactly one Space node (trailing space from tree-sitter, after the ref)
    let space_count = result.matches("\"t\":\"Space\"").count();
    assert_eq!(
        space_count, 1,
        "Should have exactly 1 Space node (trailing): {}",
        result
    );
    assert!(
        result.contains("\"quarto-note-reference\""),
        "Should contain quarto-note-reference class: {}",
        result
    );

    // Verify the Span comes before the Space in the JSON (ref, then space, then "word")
    let span_pos = result.find("\"t\":\"Span\"").unwrap();
    let space_pos = result.find("\"t\":\"Space\"").unwrap();
    assert!(
        span_pos < space_pos,
        "Span should appear before Space in output: {}",
        result
    );
}

/// Test multiple consecutive note references with different spacing
#[test]
fn test_inline_note_reference_multiple_spacing_patterns() {
    let input = "A[^1] B [^2]C[^3] D";
    let result = parse_qmd_to_json(input);

    // Expected pattern: A, [^1], Space, B, Space, [^2], C, [^3], Space, D
    // Space count: 1 (after [^1]) + 1 (injected before [^2]) + 1 (after [^3]) = 3
    let space_count = result.matches("\"t\":\"Space\"").count();
    assert_eq!(
        space_count, 3,
        "Should have exactly 3 Space nodes: {}",
        result
    );

    // Verify all three note references are present
    let span_count = result.matches("\"quarto-note-reference\"").count();
    assert_eq!(
        span_count, 3,
        "Should have exactly 3 note references: {}",
        result
    );
}
