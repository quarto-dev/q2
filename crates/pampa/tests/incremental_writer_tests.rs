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
use proptest::prelude::*;
use quarto_ast_reconcile::compute_reconciliation;
use std::io::Cursor;

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

/// Write a Pandoc AST to JSON.
fn write_json(ast: &Pandoc) -> String {
    let mut buf = Vec::new();
    let context = pampa::pandoc::ASTContext::default();
    writers::json::write(ast, &context, &mut buf).expect("Failed to write JSON");
    String::from_utf8(buf).expect("Writer produced invalid UTF-8")
}

/// Read a Pandoc AST from JSON.
fn read_json(json: &str) -> Pandoc {
    let mut cursor = Cursor::new(json.as_bytes());
    pampa::readers::json::read(&mut cursor)
        .expect("Failed to read JSON")
        .0
}

/// Simulate the WASM incremental_write_qmd path:
/// 1. Parse original_qmd to get original_ast with accurate source spans
/// 2. JSON round-trip the new_ast (simulates client serialization/deserialization)
/// 3. Compute reconciliation plan and run incremental_write
fn incremental_write_via_json_roundtrip(original_qmd: &str, new_ast: &Pandoc) -> String {
    let original_ast = parse_qmd(original_qmd);
    let json = write_json(new_ast);
    let new_ast_from_json = read_json(&json);
    let plan = compute_reconciliation(&original_ast, &new_ast_from_json);
    writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast_from_json, &plan)
        .expect("incremental_write failed")
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

// =============================================================================
// Metadata gap preservation — bd-1kvf
// =============================================================================
//
// When the incremental writer rewrites a block (e.g., a Div with a toggled
// checkbox), the blank line between the YAML front matter and the first block
// must be preserved. This tests the WASM path where the new AST comes from
// JSON round-trip (which loses source_info accuracy in metadata).

#[test]
fn metadata_gap_preserved_when_block_rewritten_via_json() {
    // Document with front matter + blank line + div containing a checkbox list
    let original_qmd = "\
---
title: Hello
---

::: {#todo}

* [x] First item
* [x] Second item

:::
";

    // Parse to get the AST, then modify it (toggle first checkbox)
    let mut new_ast = parse_qmd(original_qmd);

    // Navigate: blocks[0] = Div -> content[0] = BulletList -> content[0][0] = Plain -> content[0] = Span
    // Toggle the first checkbox from [x] to [ ] by clearing the span content
    if let pampa::pandoc::Block::Div(ref mut div) = new_ast.blocks[0] {
        if let pampa::pandoc::Block::BulletList(ref mut bl) = div.content[0] {
            if let pampa::pandoc::Block::Plain(ref mut plain) = bl.content[0][0] {
                if let pampa::pandoc::Inline::Span(ref mut span) = plain.content[0] {
                    span.content.clear(); // Toggle [x] -> [ ]
                }
            }
        }
    }

    // Run through JSON round-trip path (simulates WASM/client)
    let result = incremental_write_via_json_roundtrip(original_qmd, &new_ast);

    // The blank line between --- and ::: {#todo} must be preserved
    assert!(
        result.contains("---\n\n::: {#todo}") || result.contains("---\n\n:::"),
        "Blank line between front matter and first block was lost!\nResult:\n{}",
        result
    );
}

#[test]
fn metadata_gap_preserved_when_paragraph_rewritten_via_json() {
    // Simpler case: front matter + blank line + paragraph
    let original_qmd = "\
---
title: Hello
---

A paragraph.
";

    let mut new_ast = parse_qmd(original_qmd);
    // Change the paragraph text
    if let pampa::pandoc::Block::Paragraph(ref mut p) = new_ast.blocks[0] {
        p.content = vec![pampa::pandoc::Inline::Str(pampa::pandoc::Str {
            text: "Modified paragraph.".to_string(),
            source_info: quarto_source_map::SourceInfo::default(),
        })];
    }

    let result = incremental_write_via_json_roundtrip(original_qmd, &new_ast);

    assert!(
        result.contains("---\n\n"),
        "Blank line between front matter and first block was lost!\nResult:\n{}",
        result
    );
}

#[test]
fn metadata_gap_preserved_identical_ast_via_json() {
    // Even with NO changes, JSON round-trip should preserve the gap
    let original_qmd = "\
---
title: Hello
---

A paragraph.
";

    let ast = parse_qmd(original_qmd);
    let result = incremental_write_via_json_roundtrip(original_qmd, &ast);

    assert_eq!(
        result, original_qmd,
        "Idempotence violated when AST goes through JSON round-trip!\nExpected:\n{:?}\nGot:\n{:?}",
        original_qmd, result
    );
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

// =============================================================================
// QMD String Generators (for property-based tests)
// =============================================================================
//
// These generators produce QMD strings at increasing levels of complexity.
// Each generated string is a valid QMD document that can be parsed by the reader.
//
// The approach is to generate QMD text directly rather than ASTs, because:
// 1. The reader gives us accurate source spans (needed by the incremental writer)
// 2. We don't need the test-only generators from quarto-ast-reconcile
// 3. QMD strings are the natural input domain for the incremental writer
//
// Text uses only lowercase letters and spaces to avoid accidentally producing
// markdown syntax (e.g., `*` for lists, `#` for headers, `>` for block quotes).

/// Generate a single word: 2-8 lowercase letters.
fn gen_word() -> BoxedStrategy<String> {
    "[a-z]{2,8}".boxed()
}

/// Generate paragraph text: 2-6 words separated by spaces.
fn gen_paragraph_text() -> BoxedStrategy<String> {
    prop::collection::vec(gen_word(), 2..7)
        .prop_map(|words| words.join(" "))
        .boxed()
}

/// Generate a paragraph block (text ending with `\n`).
fn gen_paragraph_block() -> BoxedStrategy<String> {
    gen_paragraph_text()
        .prop_map(|text| format!("{}\n", text))
        .boxed()
}

/// Generate a header block (`## Title\n`).
fn gen_header_block() -> BoxedStrategy<String> {
    (1..4usize, gen_paragraph_text())
        .prop_map(|(level, text)| format!("{} {}\n", "#".repeat(level), text))
        .boxed()
}

/// Generate a fenced code block.
fn gen_code_block() -> BoxedStrategy<String> {
    ("[a-z]{3,6}", "[a-z0-9 ]{3,20}")
        .prop_map(|(lang, code)| format!("```{}\n{}\n```\n", lang, code))
        .boxed()
}

/// Generate a horizontal rule.
fn gen_hr_block() -> BoxedStrategy<String> {
    Just("***\n".to_string()).boxed()
}

/// Generate a single-line block quote.
fn gen_blockquote_block() -> BoxedStrategy<String> {
    gen_paragraph_text()
        .prop_map(|text| format!("> {}\n", text))
        .boxed()
}

/// Generate a bullet list with 2-4 items.
fn gen_bullet_list_block() -> BoxedStrategy<String> {
    prop::collection::vec(gen_paragraph_text(), 2..5)
        .prop_map(|items| {
            items
                .iter()
                .map(|item| format!("* {}\n", item))
                .collect::<String>()
        })
        .boxed()
}

/// Generate an ordered list with 2-4 items.
fn gen_ordered_list_block() -> BoxedStrategy<String> {
    prop::collection::vec(gen_paragraph_text(), 2..5)
        .prop_map(|items| {
            items
                .iter()
                .enumerate()
                .map(|(i, item)| format!("{}. {}\n", i + 1, item))
                .collect::<String>()
        })
        .boxed()
}

/// Level 0: Paragraphs only. The simplest documents.
fn gen_qmd_level0() -> BoxedStrategy<String> {
    prop::collection::vec(gen_paragraph_block(), 1..6)
        .prop_map(|blocks| blocks.join("\n"))
        .boxed()
}

/// Level 1: Leaf blocks — paragraphs, headers, code blocks, horizontal rules.
fn gen_qmd_level1() -> BoxedStrategy<String> {
    prop::collection::vec(
        prop_oneof![
            4 => gen_paragraph_block(),
            2 => gen_header_block(),
            2 => gen_code_block(),
            1 => gen_hr_block(),
        ],
        1..6,
    )
    .prop_map(|blocks| blocks.join("\n"))
    .boxed()
}

/// Level 2: Container blocks — adds block quotes, bullet lists, ordered lists.
fn gen_qmd_level2() -> BoxedStrategy<String> {
    prop::collection::vec(
        prop_oneof![
            4 => gen_paragraph_block(),
            2 => gen_header_block(),
            2 => gen_code_block(),
            1 => gen_hr_block(),
            2 => gen_blockquote_block(),
            2 => gen_bullet_list_block(),
            1 => gen_ordered_list_block(),
        ],
        1..6,
    )
    .prop_map(|blocks| blocks.join("\n"))
    .boxed()
}

/// Level 3: Adds YAML front matter.
fn gen_qmd_level3() -> BoxedStrategy<String> {
    (
        prop::bool::ANY,
        prop::collection::vec(
            prop_oneof![
                4 => gen_paragraph_block(),
                2 => gen_header_block(),
                2 => gen_code_block(),
                1 => gen_hr_block(),
                2 => gen_blockquote_block(),
                2 => gen_bullet_list_block(),
                1 => gen_ordered_list_block(),
            ],
            1..6,
        ),
    )
        .prop_map(|(has_front_matter, blocks)| {
            let body = blocks.join("\n");
            if has_front_matter {
                format!("---\ntitle: Test\n---\n\n{}", body)
            } else {
                body
            }
        })
        .boxed()
}

// =============================================================================
// QMD Mutation Generators (for Property 1 round-trip tests)
// =============================================================================
//
// These generate (original_qmd, new_qmd) pairs where the documents share some
// blocks but differ in others, testing the incremental writer's ability to
// preserve unchanged blocks while correctly rewriting changed ones.

/// Generate a (original, new) pair by mutating a single block in a document.
///
/// Strategy: generate a document of 2-5 blocks, pick one block to replace with
/// a freshly generated block. The other blocks remain identical.
fn gen_qmd_pair_single_mutation() -> BoxedStrategy<(String, String)> {
    // Generate the block list, then pick an index to mutate
    prop::collection::vec(gen_paragraph_block(), 2..6)
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..n, gen_paragraph_block())
        })
        .prop_map(|(blocks, idx, new_block)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks[idx] = new_block;
            let new = new_blocks.join("\n");
            (original, new)
        })
        .boxed()
}

/// Generate a (original, new) pair by adding a block.
fn gen_qmd_pair_add_block() -> BoxedStrategy<(String, String)> {
    prop::collection::vec(gen_paragraph_block(), 2..5)
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..=n, gen_paragraph_block())
        })
        .prop_map(|(blocks, insert_pos, new_block)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks.insert(insert_pos, new_block);
            let new = new_blocks.join("\n");
            (original, new)
        })
        .boxed()
}

/// Generate a (original, new) pair by removing a block.
fn gen_qmd_pair_remove_block() -> BoxedStrategy<(String, String)> {
    prop::collection::vec(gen_paragraph_block(), 3..6)
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..n)
        })
        .prop_map(|(blocks, remove_idx)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks.remove(remove_idx);
            let new = new_blocks.join("\n");
            (original, new)
        })
        .boxed()
}

/// Generate a (original, new) pair using mixed block types and any mutation.
fn gen_qmd_pair_mixed() -> BoxedStrategy<(String, String)> {
    prop_oneof![
        // Mutate a single block in a level 1 document
        prop::collection::vec(
            prop_oneof![
                4 => gen_paragraph_block(),
                2 => gen_header_block(),
                2 => gen_code_block(),
                1 => gen_hr_block(),
            ],
            2..6,
        )
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..n, gen_paragraph_block())
        })
        .prop_map(|(blocks, idx, new_block)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks[idx] = new_block;
            let new_doc = new_blocks.join("\n");
            (original, new_doc)
        }),
        // Add a block to a level 2 document
        prop::collection::vec(
            prop_oneof![
                3 => gen_paragraph_block(),
                1 => gen_header_block(),
                1 => gen_blockquote_block(),
                1 => gen_bullet_list_block(),
            ],
            2..5,
        )
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..=n, gen_paragraph_block())
        })
        .prop_map(|(blocks, insert_pos, new_block)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks.insert(insert_pos, new_block);
            let new_doc = new_blocks.join("\n");
            (original, new_doc)
        }),
        // Remove a block from a level 2 document
        prop::collection::vec(
            prop_oneof![
                3 => gen_paragraph_block(),
                1 => gen_header_block(),
                1 => gen_blockquote_block(),
                1 => gen_bullet_list_block(),
            ],
            3..6,
        )
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..n)
        })
        .prop_map(|(blocks, remove_idx)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks.remove(remove_idx);
            let new_doc = new_blocks.join("\n");
            (original, new_doc)
        }),
    ]
    .boxed()
}

// =============================================================================
// Property 2: Idempotence — proptest
// =============================================================================

proptest! {
    #[test]
    fn proptest_idempotent_level0(qmd in gen_qmd_level0()) {
        assert_idempotent(&qmd);
    }

    #[test]
    fn proptest_idempotent_level1(qmd in gen_qmd_level1()) {
        assert_idempotent(&qmd);
    }

    #[test]
    fn proptest_idempotent_level2(qmd in gen_qmd_level2()) {
        assert_idempotent(&qmd);
    }

    #[test]
    fn proptest_idempotent_level3(qmd in gen_qmd_level3()) {
        assert_idempotent(&qmd);
    }
}

// =============================================================================
// Property 1: Round-trip correctness — proptest
// =============================================================================

proptest! {
    #[test]
    fn proptest_roundtrip_single_mutation(
        (original, new) in gen_qmd_pair_single_mutation()
    ) {
        assert_roundtrip(&original, &new);
    }

    #[test]
    fn proptest_roundtrip_add_block(
        (original, new) in gen_qmd_pair_add_block()
    ) {
        assert_roundtrip(&original, &new);
    }

    #[test]
    fn proptest_roundtrip_remove_block(
        (original, new) in gen_qmd_pair_remove_block()
    ) {
        assert_roundtrip(&original, &new);
    }

    #[test]
    fn proptest_roundtrip_mixed(
        (original, new) in gen_qmd_pair_mixed()
    ) {
        assert_roundtrip(&original, &new);
    }
}

// =============================================================================
// Property 3: Equivalence with full writer — proptest
// =============================================================================
//
// read(incremental_write(qmd, orig, new, plan)) ≡ read(write(new_ast))
//
// The incremental writer and full writer should produce semantically equivalent
// documents, even though the byte-level representations may differ.

/// Assert Property 3: incremental result is semantically equivalent to full writer result.
fn assert_equivalent_to_full_writer(original_qmd: &str, new_qmd: &str) {
    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let incremental_result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed");

    let full_result = write_qmd(&new_ast);

    // Parse both results and compare structurally
    let incremental_ast = parse_qmd(&incremental_result);
    let full_ast = parse_qmd(&full_result);

    assert_eq!(
        incremental_ast.blocks.len(),
        full_ast.blocks.len(),
        "Block count mismatch between incremental and full writer:\n  incremental: {} blocks\n  full: {} blocks\n  incremental text: {:?}\n  full text: {:?}",
        incremental_ast.blocks.len(),
        full_ast.blocks.len(),
        incremental_result,
        full_result
    );

    for (i, (inc_block, full_block)) in incremental_ast
        .blocks
        .iter()
        .zip(full_ast.blocks.iter())
        .enumerate()
    {
        assert!(
            quarto_ast_reconcile::structural_eq_block(inc_block, full_block),
            "Block {} structurally different between incremental and full writer:\n  incremental: {:?}\n  full: {:?}",
            i,
            inc_block,
            full_block
        );
    }
}

proptest! {
    #[test]
    fn proptest_equivalent_to_full_writer(
        (original, new) in gen_qmd_pair_mixed()
    ) {
        assert_equivalent_to_full_writer(&original, &new);
    }
}

// =============================================================================
// Property 4: Verbatim preservation of unchanged blocks — proptest
// =============================================================================
//
// When a single block changes, all other blocks should be preserved
// byte-for-byte in the incremental writer's output.
//
// Note: The stronger form of Property 4 (locality of edit ranges from
// compute_incremental_edits) requires fine-grained edit computation,
// which is a future optimization. This tests the weaker but important
// invariant that unchanged blocks are verbatim-preserved.

/// Assert that unchanged blocks in the result are byte-for-byte identical
/// to their original text.
fn assert_verbatim_preservation(blocks: &[String], mutate_idx: usize, new_block: &str) {
    let original = blocks.join("\n");
    let mut new_blocks: Vec<String> = blocks.to_vec();
    new_blocks[mutate_idx] = new_block.to_string();
    let new = new_blocks.join("\n");

    let original_ast = parse_qmd(&original);
    let new_ast = parse_qmd(&new);

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result = writers::incremental::incremental_write(&original, &original_ast, &new_ast, &plan)
        .expect("incremental_write failed");

    // For each unchanged block, verify its text appears verbatim in the result.
    // We check by finding the original block text in the result string.
    for (i, block_text) in blocks.iter().enumerate() {
        if i == mutate_idx {
            continue; // This block was mutated — skip
        }
        // The block text (without trailing separator) should appear in the result
        let block_content = block_text.trim_end_matches('\n');
        assert!(
            result.contains(block_content),
            "Block {} should be preserved verbatim.\n  Expected to find: {:?}\n  In result: {:?}",
            i,
            block_content,
            result
        );
    }
}

proptest! {
    #[test]
    fn proptest_verbatim_preservation(
        blocks in prop::collection::vec(gen_paragraph_block(), 3..6),
        new_block in gen_paragraph_block(),
    ) {
        let n = blocks.len();
        // Pick a random block to mutate (use first byte of new_block as seed)
        let mutate_idx = new_block.as_bytes()[0] as usize % n;
        assert_verbatim_preservation(&blocks, mutate_idx, &new_block);
    }
}

// =============================================================================
// Property 5: Monotonicity of edit spans — proptest
// =============================================================================
//
// compute_incremental_edits produces edits that are:
// - Sorted by range.start
// - Non-overlapping (each edit's range.end <= next edit's range.start)

/// Assert Property 5: edits are sorted and non-overlapping.
fn assert_edits_monotonic(original_qmd: &str, new_qmd: &str) {
    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let edits = writers::incremental::compute_incremental_edits(
        original_qmd,
        &original_ast,
        &new_ast,
        &plan,
    )
    .expect("compute_incremental_edits failed");

    // Verify sorted by range.start
    for window in edits.windows(2) {
        assert!(
            window[0].range.start <= window[1].range.start,
            "Edits not sorted by start: {:?} before {:?}",
            window[0],
            window[1]
        );
    }

    // Verify non-overlapping
    for window in edits.windows(2) {
        assert!(
            window[0].range.end <= window[1].range.start,
            "Edits overlap: {:?} and {:?}",
            window[0],
            window[1]
        );
    }

    // Verify all edit ranges are within bounds
    for edit in &edits {
        assert!(
            edit.range.end <= original_qmd.len(),
            "Edit range {:?} exceeds document length {}",
            edit.range,
            original_qmd.len()
        );
    }
}

proptest! {
    #[test]
    fn proptest_edits_monotonic(
        (original, new) in gen_qmd_pair_mixed()
    ) {
        assert_edits_monotonic(&original, &new);
    }

    #[test]
    fn proptest_edits_monotonic_identity(qmd in gen_qmd_level2()) {
        // Identity case: should produce zero edits
        let ast = parse_qmd(&qmd);
        let plan = compute_reconciliation(&ast, &ast);
        let edits =
            writers::incremental::compute_incremental_edits(&qmd, &ast, &ast, &plan)
                .expect("compute_incremental_edits failed");
        prop_assert!(
            edits.is_empty(),
            "Identity reconciliation should produce zero edits, got {} edits",
            edits.len()
        );
    }
}

// =============================================================================
// Sugar/Desugar Roundtrip Tests
// =============================================================================
//
// These verify that desugar(sugar(node)) ≡ node for Table and DefinitionList.
// The incremental writer relies on this property: when a Table or DefinitionList
// block is marked as `KeepBefore`, its verbatim source text must parse back to
// an equivalent AST node. When it's marked as `Rewrite`, the writer's sugaring
// must produce output that deserializes identically.

/// Assert that a QMD document round-trips through write→parse with structural equality.
fn assert_sugar_roundtrip(input: &str) {
    let ast1 = parse_qmd(input);
    let written = write_qmd(&ast1);
    let ast2 = parse_qmd(&written);

    assert_eq!(
        ast1.blocks.len(),
        ast2.blocks.len(),
        "Block count changed after write→parse roundtrip:\n  original: {} blocks\n  after roundtrip: {} blocks\n  written text: {:?}",
        ast1.blocks.len(),
        ast2.blocks.len(),
        written
    );

    for (i, (b1, b2)) in ast1.blocks.iter().zip(ast2.blocks.iter()).enumerate() {
        assert!(
            quarto_ast_reconcile::structural_eq_block(b1, b2),
            "Block {} structurally different after write→parse roundtrip:\n  before: {:?}\n  after: {:?}\n  written text: {:?}",
            i,
            b1,
            b2,
            written
        );
    }
}

// --- List-table sugar/desugar roundtrips ---

#[test]
fn sugar_roundtrip_list_table_basic() {
    assert_sugar_roundtrip(
        "::: {.list-table}\n\n* - Cell 1,1\n  - Cell 1,2\n* - Cell 2,1\n  - Cell 2,2\n\n:::\n",
    );
}

#[test]
fn sugar_roundtrip_list_table_with_header() {
    assert_sugar_roundtrip(
        "::: {.list-table header-rows=\"1\"}\n\n* - Header 1\n  - Header 2\n* - Cell 1,1\n  - Cell 1,2\n\n:::\n",
    );
}

// --- Definition-list sugar/desugar roundtrips ---

// NOTE: Definition-list sugar/desugar roundtrip is LOSSY. The writer produces
// Pandoc-native definition list syntax ("term\n:   definition\n") but the reader
// only recognizes the `::: {.definition-list}` div syntax. This means:
//   - KeepBefore (verbatim copy): works correctly (preserves original div syntax)
//   - Rewrite: broken — writer output doesn't parse back to DefinitionList
// This is a pre-existing writer bug, not specific to the incremental writer.
// These tests are ignored until the writer is fixed to produce div syntax for
// definition lists (or the reader is extended to parse Pandoc-native syntax).

#[test]
#[ignore = "definition-list sugar roundtrip is lossy: writer produces Pandoc-native syntax, reader expects div syntax"]
fn sugar_roundtrip_definition_list_basic() {
    assert_sugar_roundtrip(
        "::: {.definition-list}\n* term one\n  - definition one\n* term two\n  - definition two\n\n:::\n",
    );
}

#[test]
#[ignore = "definition-list sugar roundtrip is lossy: writer produces Pandoc-native syntax, reader expects div syntax"]
fn sugar_roundtrip_definition_list_multiple_defs() {
    assert_sugar_roundtrip(
        "::: {.definition-list}\n* term\n  - definition a\n  - definition b\n\n:::\n",
    );
}

// --- Idempotence of incremental writer with sugared constructs ---

#[test]
fn idempotent_list_table() {
    assert_idempotent(
        "::: {.list-table}\n\n* - Cell 1,1\n  - Cell 1,2\n* - Cell 2,1\n  - Cell 2,2\n\n:::\n",
    );
}

#[test]
fn idempotent_definition_list() {
    assert_idempotent(
        "::: {.definition-list}\n* term one\n  - definition one\n* term two\n  - definition two\n\n:::\n",
    );
}

#[test]
fn idempotent_mixed_with_table() {
    assert_idempotent(
        "## Title\n\nBefore the table.\n\n::: {.list-table}\n\n* - A\n  - B\n* - C\n  - D\n\n:::\n\nAfter the table.\n",
    );
}

// --- Roundtrip of incremental writer with sugared constructs ---

#[test]
fn roundtrip_change_paragraph_near_table() {
    assert_roundtrip(
        "Before.\n\n::: {.list-table}\n\n* - A\n  - B\n* - C\n  - D\n\n:::\n\nAfter.\n",
        "Changed before.\n\n::: {.list-table}\n\n* - A\n  - B\n* - C\n  - D\n\n:::\n\nAfter.\n",
    );
}

#[test]
fn roundtrip_change_paragraph_near_deflist() {
    assert_roundtrip(
        "Before.\n\n::: {.definition-list}\n* term\n  - def\n\n:::\n\nAfter.\n",
        "Before.\n\n::: {.definition-list}\n* term\n  - def\n\n:::\n\nChanged after.\n",
    );
}
