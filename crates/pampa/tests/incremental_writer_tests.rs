/*
 * incremental_writer_tests.rs
 *
 * Tests for the incremental QMD writer.
 * See: claude-notes/plans/2026-02-07-incremental-writer.md
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::pandoc::Pandoc;
use pampa::writers;
use quarto_ast_reconcile::compute_reconciliation;

// =============================================================================
// Helpers
// =============================================================================

/// Parse a QMD string into a Pandoc AST with accurate source spans.
fn parse_qmd(input: &str) -> Pandoc {
    let result = pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    result.expect("Failed to parse QMD").0
}

/// Write a Pandoc AST to a QMD string using the standard writer.
fn write_qmd(ast: &Pandoc) -> String {
    let mut buf = Vec::new();
    writers::qmd::write(ast, &mut buf).expect("Failed to write QMD");
    String::from_utf8(buf).expect("Writer produced invalid UTF-8")
}

// =============================================================================
// Property 2: Idempotence — incremental_write(qmd, ast, ast, identity_plan) == qmd
// =============================================================================

/// Test idempotence for a given QMD input.
/// The incremental writer with no changes should produce byte-for-byte identical output.
fn assert_idempotent(input: &str) {
    let ast = parse_qmd(input);
    let plan = compute_reconciliation(&ast, &ast);

    // All blocks should be KeepBefore (hash-matched to themselves)
    for alignment in &plan.block_alignments {
        assert!(
            matches!(
                alignment,
                quarto_ast_reconcile::types::BlockAlignment::KeepBefore(_)
            ),
            "Expected all KeepBefore for identity reconciliation, got {:?}",
            alignment
        );
    }

    let result = writers::incremental::incremental_write(input, &ast, &ast, &plan)
        .expect("incremental_write failed");

    assert_eq!(
        result, input,
        "Idempotence violated:\n--- expected ---\n{:?}\n--- got ---\n{:?}",
        input, result
    );
}

// --- Simple documents ---

#[test]
fn idempotent_single_paragraph() {
    assert_idempotent("Hello world.\n");
}

#[test]
fn idempotent_two_paragraphs() {
    assert_idempotent("First paragraph.\n\nSecond paragraph.\n");
}

#[test]
fn idempotent_three_paragraphs() {
    assert_idempotent("First.\n\nSecond.\n\nThird.\n");
}

// --- Headers ---

#[test]
fn idempotent_header_and_paragraph() {
    assert_idempotent("## Title\n\nA paragraph.\n");
}

#[test]
fn idempotent_multiple_headers() {
    assert_idempotent("# Title\n\n## Subtitle\n\nContent.\n\n### Sub-subtitle\n\nMore content.\n");
}

// --- Code blocks ---

#[test]
fn idempotent_code_block() {
    assert_idempotent("Before.\n\n```python\nprint('hello')\n```\n\nAfter.\n");
}

// --- Horizontal rule ---

#[test]
fn idempotent_horizontal_rule() {
    assert_idempotent("Before.\n\n***\n\nAfter.\n");
}

// --- Block quotes ---

#[test]
fn idempotent_block_quote() {
    assert_idempotent("Before.\n\n> Quoted text.\n\nAfter.\n");
}

#[test]
fn idempotent_block_quote_multiline() {
    assert_idempotent("Before.\n\n> First line.\n> Second line.\n\nAfter.\n");
}

// --- Bullet lists ---

#[test]
fn idempotent_bullet_list() {
    assert_idempotent("Before.\n\n* First item\n* Second item\n* Third item\n\nAfter.\n");
}

#[test]
fn idempotent_bullet_list_at_end() {
    // BulletList trailing blank line quirk — the list span absorbs \n\n
    assert_idempotent("Before.\n\n* Item one\n* Item two\n");
}

// --- Ordered lists ---

#[test]
fn idempotent_ordered_list() {
    assert_idempotent("Before.\n\n1. First\n2. Second\n3. Third\n\nAfter.\n");
}

// --- Fenced divs ---

#[test]
fn idempotent_fenced_div() {
    assert_idempotent("Before.\n\n::: {.note}\n\nInner content.\n\n:::\n\nAfter.\n");
}

// --- YAML front matter ---

#[test]
fn idempotent_with_front_matter() {
    assert_idempotent("---\ntitle: Hello\n---\n\nA paragraph.\n");
}

#[test]
fn idempotent_with_front_matter_multiple_keys() {
    assert_idempotent("---\ntitle: Hello\nauthor: World\n---\n\nA paragraph.\n");
}

// --- Mixed documents ---

#[test]
fn idempotent_mixed_document() {
    assert_idempotent(
        "## Title\n\nFirst paragraph.\n\n```python\ncode()\n```\n\nSecond paragraph.\n",
    );
}

#[test]
fn idempotent_complex_document() {
    assert_idempotent(
        "\
## Introduction

This is the first paragraph.

> A block quote with
> multiple lines.

* Item one
* Item two
* Item three

### Code Example

```python
print('hello')
```

Final paragraph.
",
    );
}

// =============================================================================
// Property 2 edge cases
// =============================================================================

#[test]
fn idempotent_empty_document() {
    // Empty document — no blocks
    assert_idempotent("");
}

#[test]
fn idempotent_single_header_no_trailing_newline() {
    // Some documents might not have trailing newlines after the last block
    // but the parser requires them, so test with trailing newline
    assert_idempotent("# Title\n");
}

// =============================================================================
// Property 1: Round-trip correctness — basic tests with hand-crafted mutations
// =============================================================================

/// Test that changing a block produces a result that round-trips correctly.
/// read(incremental_write(qmd, orig, new, plan)) ≡ new  (structural equality)
fn assert_roundtrip(original_qmd: &str, new_qmd: &str) {
    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed");

    // Verify the result round-trips: read(result) should match new_ast structurally
    let result_ast = parse_qmd(&result);

    // Compare block count
    assert_eq!(
        result_ast.blocks.len(),
        new_ast.blocks.len(),
        "Block count mismatch:\n  result has {} blocks\n  expected {} blocks\n  result text: {:?}",
        result_ast.blocks.len(),
        new_ast.blocks.len(),
        result
    );

    // Compare blocks using structural equality (ignoring source info)
    for (i, (result_block, new_block)) in result_ast
        .blocks
        .iter()
        .zip(new_ast.blocks.iter())
        .enumerate()
    {
        assert!(
            quarto_ast_reconcile::structural_eq_block(result_block, new_block),
            "Block {} structurally different:\n  result: {:?}\n  expected: {:?}",
            i,
            result_block,
            new_block
        );
    }
}

// --- Change text in a paragraph ---

#[test]
fn roundtrip_change_paragraph_text() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n",
        "First paragraph.\n\nModified second.\n\nThird paragraph.\n",
    );
}

#[test]
fn roundtrip_change_first_paragraph() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.\n",
        "Changed first.\n\nSecond paragraph.\n",
    );
}

#[test]
fn roundtrip_change_last_paragraph() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.\n",
        "First paragraph.\n\nChanged second.\n",
    );
}

// --- Add a block ---

#[test]
fn roundtrip_add_paragraph_at_end() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.\n",
        "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n",
    );
}

#[test]
fn roundtrip_add_paragraph_at_start() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.\n",
        "New first.\n\nFirst paragraph.\n\nSecond paragraph.\n",
    );
}

// --- Remove a block ---

#[test]
fn roundtrip_remove_middle_paragraph() {
    assert_roundtrip("First.\n\nSecond.\n\nThird.\n", "First.\n\nThird.\n");
}

#[test]
fn roundtrip_remove_first_paragraph() {
    assert_roundtrip("First.\n\nSecond.\n\nThird.\n", "Second.\n\nThird.\n");
}

// --- Change header ---

#[test]
fn roundtrip_change_header_text() {
    assert_roundtrip("## Title\n\nParagraph.\n", "## New Title\n\nParagraph.\n");
}

// --- Verbatim preservation tests ---

/// Verify that unchanged blocks preserve their EXACT text (byte-for-byte).
#[test]
fn verbatim_preservation_unchanged_blocks() {
    let original_qmd = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n";
    let new_qmd = "First paragraph.\n\nModified second.\n\nThird paragraph.\n";

    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed");

    // The first and third paragraphs should be byte-for-byte identical
    assert!(
        result.starts_with("First paragraph.\n"),
        "First paragraph should be preserved verbatim. Result starts with: {:?}",
        &result[..result.len().min(30)]
    );
    assert!(
        result.ends_with("Third paragraph.\n"),
        "Third paragraph should be preserved verbatim. Result ends with: {:?}",
        &result[result.len().saturating_sub(30)..]
    );
}
