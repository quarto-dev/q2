/*
 * test_editorial_mark_spacing.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for ensuring editorial marks have proper Space nodes around them,
 * consistent with how regular spans are handled.
 *
 * Bug: Editorial marks (delete, insert, highlight, edit_comment) don't have
 * Space nodes before them, while regular spans do.
 *
 * Run with: cargo nextest run -p pampa test_editorial_mark_spacing
 */

use pampa::pandoc::{ASTContext, treesitter_to_pandoc};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use pampa::writers;
use tree_sitter_qmd::MarkdownParser;

/// Helper function to parse QMD input and convert to Pandoc AST native format
fn parse_qmd_to_native(input: &str) -> String {
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

/// Test that editorial marks (delete) have Space nodes around them,
/// just like regular spans do.
///
/// Input: "a [span]{.span} and an [--editorial mark] treat space differently"
///
/// Expected: Both the span and the editorial mark should have Space nodes before
/// and after them. Currently, the editorial mark is missing the Space before it.
#[test]
fn test_delete_mark_has_space_before_it() {
    let input = "a [span]{.span} and an [--editorial mark] end";
    let result = parse_qmd_to_native(input);

    // The result should contain Space nodes around the Delete span (quarto-delete)
    // Expected sequence: Str "an", Space, Span (quarto-delete) [...], Space, Str "end"

    // Check that we have the basic structure
    assert!(
        result.contains("Para"),
        "Output should contain Para: {}",
        result
    );
    assert!(
        result.contains("quarto-delete"),
        "Output should contain quarto-delete: {}",
        result
    );

    // CRITICAL TEST: There should be a Space before the quarto-delete span
    // The pattern should be: Str "an", Space, Span ( "" , ["quarto-delete"]
    // NOT: Str "an", Span ( "" , ["quarto-delete"]
    assert!(
        result.contains(r#"Str "an", Space, Span ( "" , ["quarto-delete"]"#),
        "Output should have Space before the editorial mark (quarto-delete).\n\
         Expected: Str \"an\", Space, Span ( \"\" , [\"quarto-delete\"]\n\
         Got: {}",
        result
    );
}

/// Test that insert marks have Space nodes around them
#[test]
fn test_insert_mark_has_space_before_it() {
    let input = "word [++inserted text] end";
    let result = parse_qmd_to_native(input);

    assert!(
        result.contains("quarto-insert"),
        "Output should contain quarto-insert: {}",
        result
    );

    // CRITICAL: Space before the insert span
    assert!(
        result.contains(r#"Str "word", Space, Span ( "" , ["quarto-insert"]"#),
        "Output should have Space before the editorial mark (quarto-insert).\n\
         Expected: Str \"word\", Space, Span ( \"\" , [\"quarto-insert\"]\n\
         Got: {}",
        result
    );
}

/// Test that highlight marks have Space nodes around them
#[test]
fn test_highlight_mark_has_space_before_it() {
    let input = "word [!!highlighted text] end";
    let result = parse_qmd_to_native(input);

    assert!(
        result.contains("quarto-highlight"),
        "Output should contain quarto-highlight: {}",
        result
    );

    // CRITICAL: Space before the highlight span
    assert!(
        result.contains(r#"Str "word", Space, Span ( "" , ["quarto-highlight"]"#),
        "Output should have Space before the editorial mark (quarto-highlight).\n\
         Expected: Str \"word\", Space, Span ( \"\" , [\"quarto-highlight\"]\n\
         Got: {}",
        result
    );
}

/// Test that edit comment marks have Space nodes around them
#[test]
fn test_edit_comment_mark_has_space_before_it() {
    let input = "word [>>comment text] end";
    let result = parse_qmd_to_native(input);

    assert!(
        result.contains("quarto-edit-comment"),
        "Output should contain quarto-edit-comment: {}",
        result
    );

    // CRITICAL: Space before the edit comment span
    assert!(
        result.contains(r#"Str "word", Space, Span ( "" , ["quarto-edit-comment"]"#),
        "Output should have Space before the editorial mark (quarto-edit-comment).\n\
         Expected: Str \"word\", Space, Span ( \"\" , [\"quarto-edit-comment\"]\n\
         Got: {}",
        result
    );
}

/// Test that regular spans have Space nodes around them (reference behavior)
/// This test documents the EXPECTED behavior that editorial marks should match.
#[test]
fn test_regular_span_has_space_around_it() {
    let input = "word [span]{.class} end";
    let result = parse_qmd_to_native(input);

    assert!(
        result.contains("Span"),
        "Output should contain Span: {}",
        result
    );

    // Regular spans should have Space before them
    assert!(
        result.contains(r#"Str "word", Space, Span"#),
        "Regular span should have Space before it.\n\
         Expected: Str \"word\", Space, Span\n\
         Got: {}",
        result
    );

    // And Space after
    assert!(
        result.contains(r#"], Space, Str "end""#),
        "Regular span should have Space after it.\n\
         Expected: ], Space, Str \"end\"\n\
         Got: {}",
        result
    );
}

/// Test comparing span and editorial mark spacing directly in the same paragraph
#[test]
fn test_span_and_editorial_mark_spacing_comparison() {
    // This is the exact test case from the bug report
    let input = "a [span]{.span} and an [--editorial mark] treat space differently";
    let result = parse_qmd_to_native(input);

    // Count Space occurrences - there should be one before each inline element
    // "a" Space [span] Space "and" Space "an" Space [editorial mark] Space "treat" Space "space" Space "differently"
    // That's 7 Space nodes

    // The actual content of the paragraph should look like:
    // Str "a", Space, Span (span), Space, Str "and", Space, Str "an", Space, Span (quarto-delete), Space, Str "treat", Space, Str "space", Space, Str "differently"

    // Verify Space before the regular span (after "a")
    assert!(
        result.contains(r#"Str "a", Space, Span"#),
        "Should have Space after 'a' and before span: {}",
        result
    );

    // Verify Space before the editorial mark (after "an")
    // This is the bug - currently missing
    assert!(
        result.contains(r#"Str "an", Space, Span ( "" , ["quarto-delete"]"#),
        "Should have Space after 'an' and before editorial mark.\n\
         This is the BUG: editorial marks are missing the Space before them.\n\
         Got: {}",
        result
    );
}
