/*
 * test_math_attr.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Tests for Math+Attr desugaring feature documented in
 * docs/syntax/desugaring/math-attributes.qmd
 */

use pampa::pandoc::{ASTContext, treesitter_to_pandoc};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use pampa::writers;
use serde_json::Value;
use tree_sitter_qmd::MarkdownParser;

/// Helper function to parse QMD input and convert to JSON AST
fn qmd_to_json_ast(input: &str) -> Value {
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let mut buf = Vec::new();
    let mut error_collector = DiagnosticCollector::new();
    let context = ASTContext::anonymous();

    writers::json::write(
        &treesitter_to_pandoc(
            &mut std::io::sink(),
            &tree,
            &input_bytes,
            &context,
            &mut error_collector,
        )
        .unwrap(),
        &context,
        &mut buf,
    )
    .unwrap();

    let json_str = String::from_utf8(buf).expect("Invalid UTF-8 in output");
    serde_json::from_str(&json_str).expect("Failed to parse JSON")
}

/// Helper to extract first block's content from AST
fn get_first_block_content(ast: &Value) -> &Value {
    &ast["blocks"][0]["c"]
}

/// Helper to find a Span element in the inlines
fn find_span_in_inlines(inlines: &Value) -> Option<&Value> {
    if let Some(arr) = inlines.as_array() {
        for inline in arr {
            if inline["t"].as_str() == Some("Span") {
                return Some(inline);
            }
        }
    }
    None
}

/// Helper to find a Math element in the inlines
fn find_math_in_inlines(inlines: &Value) -> Option<&Value> {
    if let Some(arr) = inlines.as_array() {
        for inline in arr {
            if inline["t"].as_str() == Some("Math") {
                return Some(inline);
            }
        }
    }
    None
}

#[test]
fn test_inline_math_with_id() {
    // Input: $E = mc^2$ {#eq-einstein}
    // Expected: Span with id="eq-einstein" wrapping the Math
    let input = r#"The famous equation $E = mc^2$ {#eq-einstein} shows the relationship."#;
    let ast = qmd_to_json_ast(input);
    let content = get_first_block_content(&ast);

    // Should find a Span in the para content
    let span = find_span_in_inlines(content).expect("Should find a Span element in paragraph");

    // Check that the Span has the correct id
    let span_attrs = &span["c"][0];
    let span_id = span_attrs[0].as_str().unwrap();
    assert_eq!(span_id, "eq-einstein", "Span should have id 'eq-einstein'");

    // Check that the Span wraps a Math element
    let span_content = &span["c"][1];
    let math = find_math_in_inlines(span_content).expect("Span should contain a Math element");

    // Verify the math content
    let math_content = &math["c"];
    assert_eq!(math_content[0]["t"].as_str().unwrap(), "InlineMath");
    assert_eq!(math_content[1].as_str().unwrap(), "E = mc^2");
}

#[test]
fn test_inline_math_with_class() {
    // Input: $x$ {.equation}
    // Expected: Span with class="equation" wrapping the Math
    let input = r#"Here is $x$ {.equation} in text."#;
    let ast = qmd_to_json_ast(input);
    let content = get_first_block_content(&ast);

    let span = find_span_in_inlines(content).expect("Should find a Span element in paragraph");

    // Check that the Span has both the marker class and user class
    let span_attrs = &span["c"][0];
    let span_classes = span_attrs[1].as_array().unwrap();
    assert_eq!(
        span_classes.len(),
        2,
        "Should have marker class + user class"
    );
    assert_eq!(
        span_classes[0].as_str().unwrap(),
        "quarto-math-with-attribute",
        "First class should be marker"
    );
    assert_eq!(
        span_classes[1].as_str().unwrap(),
        "equation",
        "Second class should be user's class"
    );

    // Check that the Span wraps a Math element
    let span_content = &span["c"][1];
    let math = find_math_in_inlines(span_content).expect("Span should contain a Math element");

    let math_content = &math["c"];
    assert_eq!(math_content[1].as_str().unwrap(), "x");
}

#[test]
fn test_inline_math_with_multiple_attrs() {
    // Input: $x$ {#eq1 .equation key="value"}
    // Expected: Span with all attributes wrapping the Math
    let input = r#"Formula $x$ {#eq1 .equation key="value"} here."#;
    let ast = qmd_to_json_ast(input);
    let content = get_first_block_content(&ast);

    let span = find_span_in_inlines(content).expect("Should find a Span element in paragraph");

    let span_attrs = &span["c"][0];

    // Check id
    assert_eq!(span_attrs[0].as_str().unwrap(), "eq1");

    // Check class (should have marker + user class)
    let classes = span_attrs[1].as_array().unwrap();
    assert_eq!(classes.len(), 2, "Should have marker class + user class");
    assert_eq!(classes[0].as_str().unwrap(), "quarto-math-with-attribute");
    assert_eq!(classes[1].as_str().unwrap(), "equation");

    // Check key-value attributes
    let kvs = span_attrs[2].as_array().unwrap();
    assert_eq!(kvs.len(), 1);
    assert_eq!(kvs[0][0].as_str().unwrap(), "key");
    assert_eq!(kvs[0][1].as_str().unwrap(), "value");
}

#[test]
fn test_display_math_with_id() {
    // Input: $$\sum_{i=1}^{n} i = \frac{n(n+1)}{2}$$ {#eq-sum}
    // Expected: Span with id wrapping DisplayMath
    let input = r#"The sum formula:

$$\sum_{i=1}^{n} i = \frac{n(n+1)}{2}$$ {#eq-sum}

is well known."#;
    let ast = qmd_to_json_ast(input);

    // Second block should be a Para with the display math
    let content = &ast["blocks"][1]["c"];

    let span = find_span_in_inlines(content).expect("Should find a Span element");

    // Check id
    let span_attrs = &span["c"][0];
    assert_eq!(span_attrs[0].as_str().unwrap(), "eq-sum");

    // Check that it wraps DisplayMath
    let span_content = &span["c"][1];
    let math = find_math_in_inlines(span_content).expect("Should contain Math");

    assert_eq!(math["c"][0]["t"].as_str().unwrap(), "DisplayMath");
    assert!(math["c"][1].as_str().unwrap().contains("sum"));
}

#[test]
fn test_math_with_space_before_attr() {
    // Input: $x$  {.eq} (extra space before attribute)
    // Expected: Should still create Span
    let input = r#"Formula $x$  {.eq} here."#;
    let ast = qmd_to_json_ast(input);
    let content = get_first_block_content(&ast);

    let span = find_span_in_inlines(content).expect("Should find a Span even with extra space");

    let span_attrs = &span["c"][0];
    let classes = span_attrs[1].as_array().unwrap();
    assert_eq!(classes.len(), 2, "Should have marker class + user class");
    assert_eq!(classes[0].as_str().unwrap(), "quarto-math-with-attribute");
    assert_eq!(classes[1].as_str().unwrap(), "eq");
}

#[test]
fn test_math_without_attr_unchanged() {
    // Input: $x$ (no attribute)
    // Expected: Just Math, no Span wrapper
    let input = r#"Formula $x$ here."#;
    let ast = qmd_to_json_ast(input);
    let content = get_first_block_content(&ast);

    // Should NOT find a Span
    assert!(
        find_span_in_inlines(content).is_none(),
        "Should not create Span when no attributes present"
    );

    // Should find raw Math instead
    let math = find_math_in_inlines(content).expect("Should find Math element");

    assert_eq!(math["c"][0]["t"].as_str().unwrap(), "InlineMath");
    assert_eq!(math["c"][1].as_str().unwrap(), "x");
}

#[test]
fn test_multiple_math_with_attrs_in_one_para() {
    // Input: $x$ {#eq1} and $y$ {#eq2}
    // Expected: Two separate Spans, each wrapping their Math
    let input = r#"We have $x$ {#eq1} and $y$ {#eq2} in one paragraph."#;
    let ast = qmd_to_json_ast(input);
    let content = get_first_block_content(&ast);

    // Count Spans
    let inlines = content.as_array().unwrap();
    let span_count = inlines
        .iter()
        .filter(|inline| inline["t"].as_str() == Some("Span"))
        .count();

    assert_eq!(span_count, 2, "Should have two Spans in the paragraph");

    // Verify both spans have correct ids
    let mut found_eq1 = false;
    let mut found_eq2 = false;

    for inline in inlines {
        if inline["t"].as_str() == Some("Span") {
            let id = inline["c"][0][0].as_str().unwrap();
            if id == "eq1" {
                found_eq1 = true;
            } else if id == "eq2" {
                found_eq2 = true;
            }
        }
    }

    assert!(found_eq1, "Should find Span with id eq1");
    assert!(found_eq2, "Should find Span with id eq2");
}

// Note: Isolated attributes (not following math) are currently not handled
// and will cause a crash in the JSON writer. This is a separate issue
// from the Math+Attr feature and should be addressed separately.

#[test]
fn test_math_attr_in_list_item() {
    // Input: Math+Attr inside a list item
    let input = r#"- First equation: $E = mc^2$ {#eq-einstein}
- Second equation: $F = ma$ {#eq-newton}"#;

    let ast = qmd_to_json_ast(input);

    // First block should be BulletList
    assert_eq!(ast["blocks"][0]["t"].as_str().unwrap(), "BulletList");

    // Get first list item's first paragraph
    let first_item_para = &ast["blocks"][0]["c"][0][0]["c"];

    let span = find_span_in_inlines(first_item_para).expect("Should find Span in first list item");

    let span_attrs = &span["c"][0];
    assert_eq!(span_attrs[0].as_str().unwrap(), "eq-einstein");
}

#[test]
fn test_math_attr_in_emphasis() {
    // Input: Math+Attr inside emphasis
    let input = r#"This is *emphasized $x$ {.eq} text*."#;

    let ast = qmd_to_json_ast(input);
    let content = get_first_block_content(&ast);

    // Find the Emph element
    let inlines = content.as_array().unwrap();
    let emph = inlines
        .iter()
        .find(|inline| inline["t"].as_str() == Some("Emph"))
        .expect("Should find Emph element");

    // Inside the Emph, should find a Span
    let emph_content = &emph["c"];
    let span = find_span_in_inlines(emph_content).expect("Should find Span inside Emph");

    let span_attrs = &span["c"][0];
    let classes = span_attrs[1].as_array().unwrap();
    assert_eq!(classes.len(), 2, "Should have marker class + user class");
    assert_eq!(classes[0].as_str().unwrap(), "quarto-math-with-attribute");
    assert_eq!(classes[1].as_str().unwrap(), "eq");
}
