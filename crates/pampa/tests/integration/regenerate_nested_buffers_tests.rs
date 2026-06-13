/*
 * regenerate_nested_buffers_tests.rs
 *
 * Integration tests for pampa::regenerate_nested_buffers.
 *
 * TDD suite for P3.1 of the block-editing feature (depth-cursor clean buffers).
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::pandoc::ast_context::ASTContext;
use pampa::regenerate_nested_buffers::{regenerate_nested_buffers, regenerate_nested_buffers_ast};
use pampa::wasm_entry_points::qmd_to_pandoc;
use pampa::writers;
use quarto_source_map::SourceInfo;

// =============================================================================
// Helpers
// =============================================================================

fn parse_qmd(content: &str) -> pampa::pandoc::Pandoc {
    qmd_to_pandoc(content.as_bytes())
        .map(|(pandoc, _ctx)| pandoc)
        .unwrap_or_else(|errs| panic!("Failed to parse QMD: {errs:?}"))
}

fn parse_qmd_with_context(content: &str) -> (pampa::pandoc::Pandoc, ASTContext) {
    qmd_to_pandoc(content.as_bytes()).unwrap_or_else(|errs| panic!("Failed to parse QMD: {errs:?}"))
}

/// Serialize a Pandoc AST to JSON using the same writer as the pipeline.
fn ast_to_json(ast: &pampa::pandoc::Pandoc, ctx: &ASTContext) -> String {
    let mut buf = Vec::new();
    writers::json::write(ast, ctx, &mut buf).expect("json write failed");
    String::from_utf8(buf).expect("non-utf8 json")
}

/// Return (start_offset, end_offset) for an Original SourceInfo, panicking otherwise.
fn original_offsets(si: &SourceInfo) -> (usize, usize) {
    match si {
        SourceInfo::Original {
            start_offset,
            end_offset,
            ..
        } => (*start_offset, *end_offset),
        other => panic!("Expected Original SourceInfo, got: {other:?}"),
    }
}

// =============================================================================
// Test 1: siKey contract
// =============================================================================

#[test]
fn test_sikey_format_matches_blockquote_child_offsets() {
    let content = "> Hello\n> world\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);

    // Find the blockquote and its child block
    let bq = match &ast.blocks[0] {
        pampa::pandoc::Block::BlockQuote(bq) => bq,
        other => panic!("Expected BlockQuote, got: {other:?}"),
    };
    let child = &bq.content[0];
    let (start, end) = original_offsets(child.source_info());

    let expected_key = format!("0:{start}-{end}:0");
    assert!(
        map.contains_key(&expected_key),
        "Map should contain key '{expected_key}', but keys are: {map:?}",
    );
}

// =============================================================================
// Test 2: Inclusion — multi-line prefixed child, no leading '> '
// =============================================================================

#[test]
fn test_multiline_blockquote_child_included_and_clean() {
    // Two-line blockquote paragraph (continuation line has '> ' prefix)
    let content = "> Hello\n> world\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);

    assert!(
        !map.is_empty(),
        "Map should not be empty for multiline blockquote"
    );
    for (key, val) in &map {
        assert!(
            !val.contains("> "),
            "Regenerated buffer for '{key}' contains '> ' prefix: {val:?}"
        );
    }
}

#[test]
fn test_multiline_bulletlist_child_included_and_clean() {
    let content = "- Hello\n  world\n\n- Second item\n  continued\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);

    // Should have at least one multi-line item
    assert!(
        !map.is_empty(),
        "Map should not be empty for multiline bullet list"
    );
    // Regenerated buffers must not contain list-indent patterns
    for val in map.values() {
        assert!(
            !val.starts_with("- "),
            "Regenerated buffer should not start with list marker: {val:?}"
        );
    }
}

// =============================================================================
// Test 3: Exclusion — single-line items
// =============================================================================

#[test]
fn test_singleline_blockquote_child_excluded() {
    let content = "> Just one line\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);
    assert!(
        map.is_empty(),
        "Single-line blockquote child must not appear in map, but got: {map:?}"
    );
}

#[test]
fn test_singleline_bulletlist_item_excluded() {
    let content = "- Single item\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);
    assert!(
        map.is_empty(),
        "Single-line bullet-list item must not appear in map, but got: {map:?}"
    );
}

// =============================================================================
// Test 4: Exclusion — fenced-div child (no prefixing ancestor)
// =============================================================================

#[test]
fn test_fenced_div_multiline_child_excluded() {
    let content = "::: {.my-div}\nLine one\nLine two\n:::\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);
    assert!(
        map.is_empty(),
        "Fenced-div child must not appear in map (no prefixing ancestor): {map:?}"
    );
}

// =============================================================================
// Test 5: Nested containers — list-in-blockquote and blockquote-in-list-item
// =============================================================================

#[test]
fn test_list_inside_blockquote_inner_items_included() {
    // blockquote containing a bullet list with a multi-line item
    let content = "> - Item one\n>   continued\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);

    assert!(
        !map.is_empty(),
        "Inner multi-line items inside list-in-blockquote should be included"
    );
    for val in map.values() {
        assert!(
            !val.contains("> "),
            "Value should not contain blockquote prefix: {val:?}"
        );
    }
}

#[test]
fn test_blockquote_inside_list_inner_items_included() {
    // bullet list item containing a blockquote with a multi-line paragraph.
    // Structure: BulletList > [BlockQuote > [Para]].
    //
    // - The BlockQuote (s=1) has BulletList as a prefixing ancestor and is
    //   multi-line, so it IS included.  Its regenerated value uses the
    //   blockquote's OWN syntax (">" prefix) — that is NOT a spurious
    //   container-added prefix; it is the block's own markup.
    // - The Para (s=2) has *both* BulletList and BlockQuote as prefixing
    //   ancestors and is multi-line, so it is ALSO included.  Its
    //   regenerated value should have NO ">" prefix.
    let content = "- > Hello\n  > world\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);

    assert!(
        !map.is_empty(),
        "Blockquote-in-list inner content should be included"
    );

    // Find the Para value: it should not start with '>' and should not
    // contain the list-indent marker.
    let para_values: Vec<&String> = map.values().filter(|v| !v.starts_with('>')).collect();
    assert!(
        !para_values.is_empty(),
        "Expected at least one clean (no '>' prefix) value for the nested Para, got: {map:?}"
    );
    for val in &para_values {
        assert!(
            !val.starts_with("- "),
            "Para value should not have list marker: {val:?}"
        );
    }

    // The BlockQuote entry (has BulletList as prefixing ancestor) will have
    // blockquote syntax in its regenerated form — confirm it is present and
    // does NOT have a spurious list-indent prefix.
    let bq_values: Vec<&String> = map.values().filter(|v| v.starts_with('>')).collect();
    for val in &bq_values {
        assert!(
            !val.contains("  > ") && !val.contains("- > "),
            "BlockQuote value should not contain list-indent prefix: {val:?}"
        );
    }
}

// =============================================================================
// Test 6: Source fidelity — shortcode + inline math + raw span
// =============================================================================

#[test]
fn test_source_fidelity_shortcode_and_math() {
    // A blockquote containing a paragraph with a shortcode invocation and
    // inline math — spanning two lines so it qualifies.
    // We use two separate lines to force multi-line.
    let content = "> Hello {{< kbd Enter >}}\n> and $x^2$ here\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);

    assert!(
        !map.is_empty(),
        "Multi-line paragraph with shortcode+math should be included"
    );
    for val in map.values() {
        // The shortcode must appear in source form (not expanded)
        assert!(
            val.contains("{{<") || val.contains("$x"),
            "Regenerated buffer should contain shortcode or math in source form: {val:?}"
        );
        // No blockquote prefix
        assert!(
            !val.contains("> "),
            "No blockquote prefix expected: {val:?}"
        );
    }
}

// =============================================================================
// Test 7: Code-block fidelity — fenced code block inside blockquote
// =============================================================================

#[test]
fn test_code_block_fidelity_inside_blockquote() {
    let content = "> ```\n> let x = 1;\n> let y = 2;\n> ```\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);

    // The code block is multi-line and inside a blockquote
    assert!(
        !map.is_empty(),
        "Multi-line fenced code block in blockquote should be in map"
    );
    for val in map.values() {
        // No blockquote prefixes in regenerated content
        assert!(
            !val.contains("> "),
            "Code block buffer should not contain blockquote prefix: {val:?}"
        );
        // Should contain code content
        assert!(
            val.contains("let x") || val.contains("```"),
            "Code block buffer should contain code content: {val:?}"
        );
    }
}

// =============================================================================
// Test 8: Offset-domain assertion
// =============================================================================

#[test]
fn test_offset_domain_slice_matches_source() {
    let content = "> First line\n> second line\n";
    let ast = parse_qmd(content);

    // Get the blockquote child's offsets and verify the slice
    let bq = match &ast.blocks[0] {
        pampa::pandoc::Block::BlockQuote(bq) => bq,
        other => panic!("Expected BlockQuote, got: {other:?}"),
    };
    let child = &bq.content[0];
    let (start, end) = original_offsets(child.source_info());

    // Verify multi-line (the predicate we're using)
    let slice = &content[start..end];
    assert!(
        slice.contains('\n'),
        "Byte slice content[{start}..{end}] should contain a newline: {slice:?}"
    );

    // Verify the map contains this child
    let map = regenerate_nested_buffers_ast(content, &ast);
    let key = format!("0:{start}-{end}:0");
    assert!(
        map.contains_key(&key),
        "Map must contain key for offset range [{start}..{end}]"
    );
}

// =============================================================================
// Test 9: JSON-string boundary (end-to-end via regenerate_nested_buffers)
// =============================================================================

#[test]
fn test_json_boundary_produces_parseable_object() {
    let content = "> Hello\n> world\n";
    let (ast, ctx) = parse_qmd_with_context(content);
    // Serialize ast to JSON using the same pipeline writer
    let ast_json = ast_to_json(&ast, &ctx);

    let result = regenerate_nested_buffers(content, &ast_json);
    assert!(
        result.is_ok(),
        "regenerate_nested_buffers should succeed: {result:?}"
    );

    let json_str = result.unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("Result should be valid JSON");
    assert!(
        parsed.is_object(),
        "Result should be a JSON object: {parsed:?}"
    );
    // The blockquote has a multi-line child, so the object should be non-empty
    let obj = parsed.as_object().unwrap();
    assert!(
        !obj.is_empty(),
        "JSON object should be non-empty for multiline blockquote content"
    );
}

// =============================================================================
// Test 10: Ordered list children included when multi-line
// =============================================================================

#[test]
fn test_ordered_list_multiline_item_included() {
    let content = "1. Hello\n   world\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);
    // Ordered lists are prefixing containers; multi-line items should appear
    assert!(
        !map.is_empty(),
        "Multi-line ordered-list item should appear in map"
    );
    for val in map.values() {
        assert!(
            !val.starts_with("1. "),
            "Regenerated buffer should not start with ordered-list marker: {val:?}"
        );
    }
}

// =============================================================================
// Test 11: DefinitionList — multi-line definition body is included
// =============================================================================

#[test]
fn test_definition_list_multiline_body_included_and_clean() {
    // A definition list (QMD fenced-div syntax) with a two-line definition body.
    // The definition body plain block spans two lines, so it qualifies.
    // QMD uses `::: {.definition-list}` fenced-div syntax, not `Term\n:   def`.
    let content = "::: {.definition-list}\n* Term\n  - First line\n    of the definition\n\n:::\n";
    let ast = parse_qmd(content);
    let map = regenerate_nested_buffers_ast(content, &ast);

    assert!(
        !map.is_empty(),
        "Multi-line definition body should appear in map; got: {map:?}"
    );
    // The regenerated value must not contain the list-indent prefix from the definition body.
    // The definition body is a Plain block — its regenerated form should be plain text lines.
    for val in map.values() {
        assert!(
            !val.starts_with("  - "),
            "Regenerated buffer should not start with definition-list item marker: {val:?}"
        );
    }
}
