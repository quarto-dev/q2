/*
 * inline_splice_property_tests.rs
 *
 * Phase 5f: Comprehensive property tests for inline splicing.
 * Tests Properties 6-10 from the plan using both hand-crafted and generated inputs.
 *
 * See: claude-notes/plans/2026-02-10-inline-splicing.md
 * Beads issue: bd-1hwd
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::pandoc::{Block, Inline, Pandoc};
use pampa::writers;
use proptest::prelude::*;
use quarto_ast_reconcile::compute_reconciliation;
use quarto_ast_reconcile::types::BlockAlignment;
use quarto_source_map::SourceInfo;

// =============================================================================
// Helpers
// =============================================================================

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

fn write_qmd(ast: &Pandoc) -> String {
    let mut buf = Vec::new();
    writers::qmd::write(ast, &mut buf).expect("Failed to write QMD");
    String::from_utf8(buf).expect("Writer produced invalid UTF-8")
}

/// A location of a Str inline within the AST, suitable for modification.
#[derive(Debug, Clone)]
struct StrLocation {
    block_idx: usize,
    /// Path to navigate from the block's inline content to the Str.
    /// Empty means top-level inline; [0] means first child of first container, etc.
    inline_idx: usize,
    /// For containers, the path through nested containers to reach the Str.
    container_path: Vec<usize>,
    /// The current text of the Str (for debugging).
    #[allow(dead_code)]
    current_text: String,
}

/// Find all Str inlines at the top level of each block's inline content.
fn find_top_level_str_locations(ast: &Pandoc) -> Vec<StrLocation> {
    let mut locations = Vec::new();
    for (block_idx, block) in ast.blocks.iter().enumerate() {
        let inlines = match block {
            Block::Paragraph(p) => &p.content[..],
            Block::Plain(p) => &p.content[..],
            Block::Header(h) => &h.content[..],
            _ => continue,
        };
        for (inline_idx, inline) in inlines.iter().enumerate() {
            if let Inline::Str(s) = inline {
                locations.push(StrLocation {
                    block_idx,
                    inline_idx,
                    container_path: vec![],
                    current_text: s.text.clone(),
                });
            }
        }
    }
    locations
}

/// Find Str inlines inside container inlines (Emph, Strong) at the top level.
fn find_contained_str_locations(ast: &Pandoc) -> Vec<StrLocation> {
    let mut locations = Vec::new();
    for (block_idx, block) in ast.blocks.iter().enumerate() {
        let inlines = match block {
            Block::Paragraph(p) => &p.content[..],
            Block::Plain(p) => &p.content[..],
            Block::Header(h) => &h.content[..],
            _ => continue,
        };
        for (inline_idx, inline) in inlines.iter().enumerate() {
            let children = match inline {
                Inline::Emph(e) => &e.content[..],
                Inline::Strong(s) => &s.content[..],
                _ => continue,
            };
            for (child_idx, child) in children.iter().enumerate() {
                if let Inline::Str(s) = child {
                    locations.push(StrLocation {
                        block_idx,
                        inline_idx,
                        container_path: vec![child_idx],
                        current_text: s.text.clone(),
                    });
                }
            }
        }
    }
    locations
}

/// Modify a Str inline at a given location, returning the new AST.
fn modify_str_at(ast: &Pandoc, loc: &StrLocation, new_text: &str) -> Pandoc {
    let mut new_ast = ast.clone();
    let inlines = match &mut new_ast.blocks[loc.block_idx] {
        Block::Paragraph(p) => &mut p.content,
        Block::Plain(p) => &mut p.content,
        Block::Header(h) => &mut h.content,
        other => panic!(
            "Expected inline-content block, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    if loc.container_path.is_empty() {
        // Top-level Str
        if let Inline::Str(ref mut s) = inlines[loc.inline_idx] {
            s.text = new_text.to_string();
            s.source_info = SourceInfo::default();
        }
    } else {
        // Str inside a container
        let children = match &mut inlines[loc.inline_idx] {
            Inline::Emph(e) => &mut e.content,
            Inline::Strong(s) => &mut s.content,
            other => panic!(
                "Expected container inline, got {:?}",
                std::mem::discriminant(other)
            ),
        };
        let child_idx = loc.container_path[0];
        if let Inline::Str(ref mut s) = children[child_idx] {
            s.text = new_text.to_string();
            s.source_info = SourceInfo::default();
        }
    }

    // If this is a header, update the auto-generated ID to match new text
    if let Block::Header(ref mut h) = new_ast.blocks[loc.block_idx] {
        let new_id = h
            .content
            .iter()
            .filter_map(|i| match i {
                Inline::Str(s) => Some(s.text.to_lowercase()),
                Inline::Space(_) => Some("-".to_string()),
                _ => None,
            })
            .collect::<String>();
        h.attr.0 = new_id;
    }

    new_ast
}

/// Assert inline round-trip correctness (Property 6):
/// read(incremental_write(qmd, orig, new, plan)) ≡ new  (via QMD re-serialization comparison)
fn assert_inline_roundtrip(original_qmd: &str, new_ast: &Pandoc) {
    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, new_ast);

    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, new_ast, &plan)
            .expect("incremental_write failed");

    // Round-trip: parse result, write both to QMD, compare
    let result_ast = parse_qmd(&result);
    let expected_qmd = write_qmd(new_ast);
    let expected_ast = parse_qmd(&expected_qmd);

    let result_rewritten = write_qmd(&result_ast);
    let expected_rewritten = write_qmd(&expected_ast);
    assert_eq!(
        result_rewritten, expected_rewritten,
        "Inline round-trip failed.\nOriginal: {:?}\nResult:   {:?}\nExpected: {:?}",
        original_qmd, result, expected_qmd
    );
}

/// Assert inline splice produces same semantic result as full writer (Property 10).
fn assert_splice_equivalent_to_full_writer(original_qmd: &str, new_ast: &Pandoc) {
    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, new_ast);

    let incremental_result =
        writers::incremental::incremental_write(original_qmd, &original_ast, new_ast, &plan)
            .expect("incremental_write failed");

    let full_result = write_qmd(new_ast);

    // Parse both and compare structurally
    let incremental_ast = parse_qmd(&incremental_result);
    let full_ast = parse_qmd(&full_result);

    assert_eq!(
        incremental_ast.blocks.len(),
        full_ast.blocks.len(),
        "Block count mismatch between incremental and full writer.\n  incremental: {:?}\n  full: {:?}",
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
            "Block {} differs between incremental and full writer.\n  incremental text: {:?}\n  full text: {:?}",
            i,
            incremental_result,
            full_result
        );
    }
}

/// Assert inline locality (Property 8):
/// When a single inline changes in block i, all OTHER blocks should be
/// preserved byte-for-byte in the incremental writer output.
/// (The current implementation produces whole-document edits, so we check
/// the result text rather than individual edit ranges.)
fn assert_inline_locality(original_qmd: &str, new_ast: &Pandoc, changed_block_idx: usize) {
    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, new_ast);

    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, new_ast, &plan)
            .expect("incremental_write failed");

    // For each unchanged block, verify its text appears in the result.
    for (i, block) in original_ast.blocks.iter().enumerate() {
        if i == changed_block_idx {
            continue; // Skip the changed block
        }
        let si = match block {
            Block::Paragraph(p) => &p.source_info,
            Block::Plain(p) => &p.source_info,
            Block::Header(h) => &h.source_info,
            _ => continue, // Skip non-inline blocks
        };
        let start = si.start_offset();
        let end = si.end_offset();
        if start < end && end <= original_qmd.len() {
            let original_text = &original_qmd[start..end];
            assert!(
                result.contains(original_text),
                "Block {} should be preserved verbatim.\n  Expected to find: {:?}\n  In result: {:?}",
                i,
                original_text,
                result
            );
        }
    }
}

// =============================================================================
// Generators
// =============================================================================

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

/// Generate a simple paragraph block.
fn gen_paragraph_block() -> BoxedStrategy<String> {
    gen_paragraph_text()
        .prop_map(|text| format!("{}\n", text))
        .boxed()
}

/// Generate a paragraph with emphasis (e.g., "*word* rest of text").
fn gen_paragraph_with_emph() -> BoxedStrategy<String> {
    (gen_word(), gen_paragraph_text())
        .prop_map(|(emph_word, rest)| format!("*{}* {}\n", emph_word, rest))
        .boxed()
}

/// Generate a paragraph with strong (e.g., "**word** rest of text").
fn gen_paragraph_with_strong() -> BoxedStrategy<String> {
    (gen_word(), gen_paragraph_text())
        .prop_map(|(strong_word, rest)| format!("**{}** {}\n", strong_word, rest))
        .boxed()
}

/// Generate a header block.
fn gen_header_block() -> BoxedStrategy<String> {
    (1..4usize, gen_paragraph_text())
        .prop_map(|(level, text)| format!("{} {}\n", "#".repeat(level), text))
        .boxed()
}

/// Generate a blockquote with a paragraph.
fn gen_blockquote_block() -> BoxedStrategy<String> {
    gen_paragraph_text()
        .prop_map(|text| format!("> {}\n", text))
        .boxed()
}

/// Generate a bullet list with 2-3 items.
fn gen_bullet_list_block() -> BoxedStrategy<String> {
    prop::collection::vec(gen_paragraph_text(), 2..4)
        .prop_map(|items| {
            items
                .iter()
                .map(|item| format!("* {}\n", item))
                .collect::<String>()
        })
        .boxed()
}

/// Generate a multi-block document with inline-rich content.
fn gen_inline_rich_doc() -> BoxedStrategy<String> {
    prop::collection::vec(
        prop_oneof![
            4 => gen_paragraph_block(),
            2 => gen_paragraph_with_emph(),
            2 => gen_paragraph_with_strong(),
            2 => gen_header_block(),
            2 => gen_blockquote_block(),
            2 => gen_bullet_list_block(),
        ],
        2..6,
    )
    .prop_map(|blocks| blocks.join("\n"))
    .boxed()
}

// =============================================================================
// Property 6: Inline Round-Trip Correctness
// =============================================================================
//
// ∀ original_qmd, inline_change:
//   read(incremental_write(qmd, orig, new, plan)) ≡ new_ast

#[test]
fn prop6_roundtrip_str_change_in_paragraph() {
    let qmd = "Hello world.\n";
    let ast = parse_qmd(qmd);
    let locs = find_top_level_str_locations(&ast);
    assert!(!locs.is_empty());
    let new_ast = modify_str_at(&ast, &locs[0], "Goodbye");
    assert_inline_roundtrip(qmd, &new_ast);
}

#[test]
fn prop6_roundtrip_str_change_in_blockquote() {
    let qmd = "> Hello world.\n";
    let ast = parse_qmd(qmd);
    // BlockQuote contains Paragraph which contains the Str inlines
    // We need to navigate into the blockquote
    let mut new_ast = ast.clone();
    if let Block::BlockQuote(ref mut bq) = new_ast.blocks[0] {
        if let Block::Paragraph(ref mut p) = bq.content[0] {
            if let Inline::Str(ref mut s) = p.content[0] {
                s.text = "Goodbye".to_string();
                s.source_info = SourceInfo::default();
            }
        }
    }
    assert_inline_roundtrip(qmd, &new_ast);
}

#[test]
fn prop6_roundtrip_str_change_in_bullet_list() {
    let qmd = "* First item\n* Second item\n";
    let ast = parse_qmd(qmd);
    let mut new_ast = ast.clone();
    if let Block::BulletList(ref mut bl) = new_ast.blocks[0] {
        if let Block::Plain(ref mut p) = bl.content[0][0] {
            if let Inline::Str(ref mut s) = p.content[0] {
                s.text = "Modified".to_string();
                s.source_info = SourceInfo::default();
            }
        }
    }
    assert_inline_roundtrip(qmd, &new_ast);
}

#[test]
fn prop6_roundtrip_str_change_in_emphasis() {
    let qmd = "*Hello* world.\n";
    let ast = parse_qmd(qmd);
    let locs = find_contained_str_locations(&ast);
    assert!(!locs.is_empty());
    let new_ast = modify_str_at(&ast, &locs[0], "Goodbye");
    assert_inline_roundtrip(qmd, &new_ast);
}

#[test]
fn prop6_roundtrip_str_change_in_strong() {
    let qmd = "**Hello** world.\n";
    let ast = parse_qmd(qmd);
    let locs = find_contained_str_locations(&ast);
    assert!(!locs.is_empty());
    let new_ast = modify_str_at(&ast, &locs[0], "Goodbye");
    assert_inline_roundtrip(qmd, &new_ast);
}

#[test]
fn prop6_roundtrip_multiple_str_changes_in_one_block() {
    let qmd = "The quick brown fox.\n";
    let ast = parse_qmd(qmd);
    let _locs = find_top_level_str_locations(&ast);
    // Change "quick" and "fox"
    let mut new_ast = ast.clone();
    // "The" is locs[0], "quick" is locs[1], "brown" is locs[2], "fox." is locs[3]
    if let Block::Paragraph(ref mut p) = new_ast.blocks[0] {
        for inline in p.content.iter_mut() {
            if let Inline::Str(s) = inline {
                if s.text == "quick" {
                    s.text = "slow".to_string();
                    s.source_info = SourceInfo::default();
                } else if s.text.starts_with("fox") {
                    s.text = "cat.".to_string();
                    s.source_info = SourceInfo::default();
                }
            }
        }
    }
    assert_inline_roundtrip(qmd, &new_ast);
}

#[test]
fn prop6_roundtrip_in_multiline_blockquote() {
    let qmd = "> Hello\n> world.\n";
    let ast = parse_qmd(qmd);
    let mut new_ast = ast.clone();
    if let Block::BlockQuote(ref mut bq) = new_ast.blocks[0] {
        if let Block::Paragraph(ref mut p) = bq.content[0] {
            if let Inline::Str(ref mut s) = p.content[0] {
                s.text = "Goodbye".to_string();
                s.source_info = SourceInfo::default();
            }
        }
    }
    assert_inline_roundtrip(qmd, &new_ast);
}

#[test]
fn prop6_roundtrip_in_multiline_bullet_list() {
    let qmd = "* Hello\n  world.\n";
    let ast = parse_qmd(qmd);
    let mut new_ast = ast.clone();
    if let Block::BulletList(ref mut bl) = new_ast.blocks[0] {
        if let Block::Plain(ref mut p) = bl.content[0][0] {
            if let Inline::Str(ref mut s) = p.content[0] {
                s.text = "Goodbye".to_string();
                s.source_info = SourceInfo::default();
            }
        }
    }
    assert_inline_roundtrip(qmd, &new_ast);
}

proptest! {
    #[test]
    fn proptest_inline_roundtrip_paragraph(
        qmd in gen_paragraph_block(),
        new_word in gen_word(),
        selector in 0usize..1000,
    ) {
        let ast = parse_qmd(&qmd);
        let locs = find_top_level_str_locations(&ast);
        if locs.is_empty() {
            return Ok(());
        }
        let loc = &locs[selector % locs.len()];
        let new_ast = modify_str_at(&ast, loc, &new_word);
        assert_inline_roundtrip(&qmd, &new_ast);
    }

    #[test]
    fn proptest_inline_roundtrip_rich_doc(
        qmd in gen_inline_rich_doc(),
        new_word in gen_word(),
        selector in 0usize..1000,
    ) {
        let ast = parse_qmd(&qmd);
        let locs = find_top_level_str_locations(&ast);
        if locs.is_empty() {
            return Ok(());
        }
        let loc = &locs[selector % locs.len()];
        let new_ast = modify_str_at(&ast, loc, &new_word);
        assert_inline_roundtrip(&qmd, &new_ast);
    }

    #[test]
    fn proptest_inline_roundtrip_emphasis(
        qmd in gen_paragraph_with_emph(),
        new_word in gen_word(),
    ) {
        let ast = parse_qmd(&qmd);
        let locs = find_contained_str_locations(&ast);
        if locs.is_empty() {
            return Ok(());
        }
        let new_ast = modify_str_at(&ast, &locs[0], &new_word);
        assert_inline_roundtrip(&qmd, &new_ast);
    }

    #[test]
    fn proptest_inline_roundtrip_strong(
        qmd in gen_paragraph_with_strong(),
        new_word in gen_word(),
    ) {
        let ast = parse_qmd(&qmd);
        let locs = find_contained_str_locations(&ast);
        if locs.is_empty() {
            return Ok(());
        }
        let new_ast = modify_str_at(&ast, &locs[0], &new_word);
        assert_inline_roundtrip(&qmd, &new_ast);
    }
}

// =============================================================================
// Property 7: Inline Idempotence
// =============================================================================
//
// ∀ original_qmd:
//   incremental_write(qmd, ast, ast, identity_plan) = qmd  (byte-for-byte)
//
// These supplement the existing idempotence tests with inline-rich content.

#[test]
fn prop7_idempotent_paragraph_with_emphasis() {
    let qmd = "*Hello* world.\n";
    let ast = parse_qmd(qmd);
    let plan = compute_reconciliation(&ast, &ast);
    let result = writers::incremental::incremental_write(qmd, &ast, &ast, &plan).unwrap();
    assert_eq!(result, qmd);
}

#[test]
fn prop7_idempotent_paragraph_with_strong() {
    let qmd = "**Hello** world.\n";
    let ast = parse_qmd(qmd);
    let plan = compute_reconciliation(&ast, &ast);
    let result = writers::incremental::incremental_write(qmd, &ast, &ast, &plan).unwrap();
    assert_eq!(result, qmd);
}

#[test]
fn prop7_idempotent_paragraph_with_code() {
    let qmd = "Use `code` here.\n";
    let ast = parse_qmd(qmd);
    let plan = compute_reconciliation(&ast, &ast);
    let result = writers::incremental::incremental_write(qmd, &ast, &ast, &plan).unwrap();
    assert_eq!(result, qmd);
}

#[test]
fn prop7_idempotent_mixed_inline_formatting() {
    let qmd = "Normal *emph* **strong** `code` end.\n";
    let ast = parse_qmd(qmd);
    let plan = compute_reconciliation(&ast, &ast);
    let result = writers::incremental::incremental_write(qmd, &ast, &ast, &plan).unwrap();
    assert_eq!(result, qmd);
}

#[test]
fn prop7_idempotent_multiline_blockquote_with_emphasis() {
    let qmd = "> *Hello*\n> world.\n";
    let ast = parse_qmd(qmd);
    let plan = compute_reconciliation(&ast, &ast);
    let result = writers::incremental::incremental_write(qmd, &ast, &ast, &plan).unwrap();
    assert_eq!(result, qmd);
}

proptest! {
    #[test]
    fn proptest_inline_idempotent_rich_doc(qmd in gen_inline_rich_doc()) {
        let ast = parse_qmd(&qmd);
        let plan = compute_reconciliation(&ast, &ast);
        let result =
            writers::incremental::incremental_write(&qmd, &ast, &ast, &plan).unwrap();
        prop_assert_eq!(result, qmd);
    }
}

// =============================================================================
// Property 8: Inline Locality
// =============================================================================
//
// When a single inline changes in block i, edits should be contained
// within block i's source span.

#[test]
fn prop8_locality_single_paragraph() {
    let qmd = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n";
    let ast = parse_qmd(qmd);
    let locs = find_top_level_str_locations(&ast);
    // Modify "Second" (block 1, first Str)
    let block1_locs: Vec<_> = locs.iter().filter(|l| l.block_idx == 1).collect();
    assert!(!block1_locs.is_empty());
    let new_ast = modify_str_at(&ast, block1_locs[0], "Modified");
    assert_inline_locality(qmd, &new_ast, 1);
}

#[test]
fn prop8_locality_header_change() {
    let qmd = "## Title\n\nFirst paragraph.\n\nSecond paragraph.\n";
    let ast = parse_qmd(qmd);
    let locs = find_top_level_str_locations(&ast);
    // Modify "Title" (block 0)
    let block0_locs: Vec<_> = locs.iter().filter(|l| l.block_idx == 0).collect();
    assert!(!block0_locs.is_empty());
    let new_ast = modify_str_at(&ast, block0_locs[0], "NewTitle");
    assert_inline_locality(qmd, &new_ast, 0);
}

#[test]
fn prop8_locality_last_block_change() {
    let qmd = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n";
    let ast = parse_qmd(qmd);
    let locs = find_top_level_str_locations(&ast);
    let block2_locs: Vec<_> = locs.iter().filter(|l| l.block_idx == 2).collect();
    assert!(!block2_locs.is_empty());
    let new_ast = modify_str_at(&ast, block2_locs[0], "Modified");
    assert_inline_locality(qmd, &new_ast, 2);
}

proptest! {
    #[test]
    fn proptest_inline_locality(
        blocks in prop::collection::vec(gen_paragraph_block(), 3..6),
        new_word in gen_word(),
        str_selector in 0usize..1000,
    ) {
        let qmd = blocks.join("\n");
        let ast = parse_qmd(&qmd);
        let locs = find_top_level_str_locations(&ast);
        if locs.is_empty() {
            return Ok(());
        }

        // Pick a Str to modify
        let loc = &locs[str_selector % locs.len()];
        let new_ast = modify_str_at(&ast, loc, &new_word);
        assert_inline_locality(&qmd, &new_ast, loc.block_idx);
    }
}

// =============================================================================
// Property 9: No-Patch-Newlines Invariant
// =============================================================================
//
// When is_inline_splice_safe returns true and inline splicing is used,
// the resulting edits should not introduce incorrectly-indented newlines.
// We verify this indirectly: the incremental result must parse back to
// the same AST as the full writer result.

#[test]
fn prop9_no_newlines_in_splice_simple() {
    let qmd = "Hello world.\n";
    let ast = parse_qmd(qmd);
    let new_ast = modify_str_at(&ast, &find_top_level_str_locations(&ast)[0], "Goodbye");

    let original_ast = parse_qmd(qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);

    // Verify the plan uses RecurseIntoContainer (inline splicing)
    assert!(
        matches!(
            plan.block_alignments[0],
            BlockAlignment::RecurseIntoContainer { .. }
        ),
        "Expected RecurseIntoContainer for Str change"
    );

    // Verify the incremental write result
    let result =
        writers::incremental::incremental_write(qmd, &original_ast, &new_ast, &plan).unwrap();

    // The result should be correct
    assert_eq!(result, "Goodbye world.\n");
}

#[test]
fn prop9_no_newlines_in_blockquote_splice() {
    let qmd = "> Hello world.\n";
    let mut new_ast = parse_qmd(qmd);
    if let Block::BlockQuote(ref mut bq) = new_ast.blocks[0] {
        if let Block::Paragraph(ref mut p) = bq.content[0] {
            if let Inline::Str(ref mut s) = p.content[0] {
                s.text = "Goodbye".to_string();
                s.source_info = SourceInfo::default();
            }
        }
    }

    let original_ast = parse_qmd(qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);

    let result =
        writers::incremental::incremental_write(qmd, &original_ast, &new_ast, &plan).unwrap();

    // Verify the result parses correctly (critical for indentation contexts)
    assert_inline_roundtrip(qmd, &new_ast);
    assert_eq!(result, "> Goodbye world.\n");
}

#[test]
fn prop9_no_newlines_in_multiline_blockquote_splice() {
    // This is Scenario B: SoftBreak exists but is KeepBefore
    let qmd = "> Hello\n> world.\n";
    let mut new_ast = parse_qmd(qmd);
    if let Block::BlockQuote(ref mut bq) = new_ast.blocks[0] {
        if let Block::Paragraph(ref mut p) = bq.content[0] {
            if let Inline::Str(ref mut s) = p.content[0] {
                s.text = "Goodbye".to_string();
                s.source_info = SourceInfo::default();
            }
        }
    }

    let original_ast = parse_qmd(qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);

    let result =
        writers::incremental::incremental_write(qmd, &original_ast, &new_ast, &plan).unwrap();

    // The > prefix after the SoftBreak must be preserved
    assert_eq!(result, "> Goodbye\n> world.\n");
}

// =============================================================================
// Property 10: Splice Produces Same Result as Full Writer
// =============================================================================
//
// When inline splicing is used, the semantic result should match
// what the full writer would produce for the same AST.

#[test]
fn prop10_splice_equiv_simple_paragraph() {
    let qmd = "Hello world.\n";
    let ast = parse_qmd(qmd);
    let new_ast = modify_str_at(&ast, &find_top_level_str_locations(&ast)[0], "Goodbye");
    assert_splice_equivalent_to_full_writer(qmd, &new_ast);
}

#[test]
fn prop10_splice_equiv_emphasis() {
    let qmd = "*Hello* world.\n";
    let ast = parse_qmd(qmd);
    let locs = find_contained_str_locations(&ast);
    assert!(!locs.is_empty());
    let new_ast = modify_str_at(&ast, &locs[0], "Goodbye");
    assert_splice_equivalent_to_full_writer(qmd, &new_ast);
}

#[test]
fn prop10_splice_equiv_strong() {
    let qmd = "**Hello** world.\n";
    let ast = parse_qmd(qmd);
    let locs = find_contained_str_locations(&ast);
    assert!(!locs.is_empty());
    let new_ast = modify_str_at(&ast, &locs[0], "Goodbye");
    assert_splice_equivalent_to_full_writer(qmd, &new_ast);
}

#[test]
fn prop10_splice_equiv_blockquote() {
    let qmd = "> Hello world.\n";
    let mut new_ast = parse_qmd(qmd);
    if let Block::BlockQuote(ref mut bq) = new_ast.blocks[0] {
        if let Block::Paragraph(ref mut p) = bq.content[0] {
            if let Inline::Str(ref mut s) = p.content[0] {
                s.text = "Goodbye".to_string();
                s.source_info = SourceInfo::default();
            }
        }
    }
    assert_splice_equivalent_to_full_writer(qmd, &new_ast);
}

#[test]
fn prop10_splice_equiv_multiblock() {
    let qmd = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n";
    let ast = parse_qmd(qmd);
    let locs = find_top_level_str_locations(&ast);
    let block1_locs: Vec<_> = locs.iter().filter(|l| l.block_idx == 1).collect();
    assert!(!block1_locs.is_empty());
    let new_ast = modify_str_at(&ast, block1_locs[0], "Modified");
    assert_splice_equivalent_to_full_writer(qmd, &new_ast);
}

proptest! {
    #[test]
    fn proptest_splice_equiv_paragraph(
        qmd in gen_paragraph_block(),
        new_word in gen_word(),
        selector in 0usize..1000,
    ) {
        let ast = parse_qmd(&qmd);
        let locs = find_top_level_str_locations(&ast);
        if locs.is_empty() {
            return Ok(());
        }
        let loc = &locs[selector % locs.len()];
        let new_ast = modify_str_at(&ast, loc, &new_word);
        assert_splice_equivalent_to_full_writer(&qmd, &new_ast);
    }

    #[test]
    fn proptest_splice_equiv_rich_doc(
        qmd in gen_inline_rich_doc(),
        new_word in gen_word(),
        selector in 0usize..1000,
    ) {
        let ast = parse_qmd(&qmd);
        let locs = find_top_level_str_locations(&ast);
        if locs.is_empty() {
            return Ok(());
        }
        let loc = &locs[selector % locs.len()];
        let new_ast = modify_str_at(&ast, loc, &new_word);
        assert_splice_equivalent_to_full_writer(&qmd, &new_ast);
    }

    #[test]
    fn proptest_splice_equiv_emphasis(
        qmd in gen_paragraph_with_emph(),
        new_word in gen_word(),
    ) {
        let ast = parse_qmd(&qmd);
        let locs = find_contained_str_locations(&ast);
        if locs.is_empty() {
            return Ok(());
        }
        let new_ast = modify_str_at(&ast, &locs[0], &new_word);
        assert_splice_equivalent_to_full_writer(&qmd, &new_ast);
    }
}

// =============================================================================
// Stress Tests
// =============================================================================

#[test]
fn stress_deeply_nested_blockquote_list() {
    // BlockQuote > BulletList > Plain with Str change
    let qmd = "> * Hello world.\n";
    let mut new_ast = parse_qmd(qmd);
    if let Block::BlockQuote(ref mut bq) = new_ast.blocks[0] {
        if let Block::BulletList(ref mut bl) = bq.content[0] {
            if let Block::Plain(ref mut p) = bl.content[0][0] {
                if let Inline::Str(ref mut s) = p.content[0] {
                    s.text = "Goodbye".to_string();
                    s.source_info = SourceInfo::default();
                }
            }
        }
    }
    assert_inline_roundtrip(qmd, &new_ast);
    assert_splice_equivalent_to_full_writer(qmd, &new_ast);
}

#[test]
fn stress_many_blocks_single_change() {
    // Document with 10 paragraphs, change one word in the 5th
    let paragraphs: Vec<String> = (0..10)
        .map(|i| format!("Paragraph number {}.\n", i))
        .collect();
    let qmd = paragraphs.join("\n");
    let ast = parse_qmd(&qmd);

    let locs = find_top_level_str_locations(&ast);
    // Find a Str in block 4
    let block4_locs: Vec<_> = locs.iter().filter(|l| l.block_idx == 4).collect();
    if !block4_locs.is_empty() {
        let new_ast = modify_str_at(&ast, block4_locs[0], "Modified");

        assert_inline_roundtrip(&qmd, &new_ast);
        assert_splice_equivalent_to_full_writer(&qmd, &new_ast);

        // Verify the edits are small (Property 8 / locality)
        let original_ast = parse_qmd(&qmd);
        let plan = compute_reconciliation(&original_ast, &new_ast);
        let edits =
            writers::incremental::compute_incremental_edits(&qmd, &original_ast, &new_ast, &plan)
                .unwrap();

        // Should have exactly 1 edit (replacing the Str)
        assert_eq!(
            edits.len(),
            1,
            "Expected 1 edit for single Str change, got {}",
            edits.len()
        );
    }
}

#[test]
fn stress_many_blocks_first_and_last_change() {
    let paragraphs: Vec<String> = (0..8)
        .map(|i| format!("Paragraph number {}.\n", i))
        .collect();
    let qmd = paragraphs.join("\n");
    let ast = parse_qmd(&qmd);

    let locs = find_top_level_str_locations(&ast);
    let block0_locs: Vec<_> = locs.iter().filter(|l| l.block_idx == 0).collect();
    let last_block = ast.blocks.len() - 1;
    let block_last_locs: Vec<_> = locs.iter().filter(|l| l.block_idx == last_block).collect();

    if !block0_locs.is_empty() && !block_last_locs.is_empty() {
        // Modify both first and last blocks
        let mut new_ast = modify_str_at(&ast, block0_locs[0], "First");
        // Apply second modification
        let inlines = match &mut new_ast.blocks[last_block] {
            Block::Paragraph(p) => &mut p.content,
            Block::Plain(p) => &mut p.content,
            Block::Header(h) => &mut h.content,
            _ => panic!("Expected inline block"),
        };
        if let Inline::Str(ref mut s) = inlines[block_last_locs[0].inline_idx] {
            s.text = "Last".to_string();
            s.source_info = SourceInfo::default();
        }

        assert_inline_roundtrip(&qmd, &new_ast);
        assert_splice_equivalent_to_full_writer(&qmd, &new_ast);
    }
}

#[test]
fn stress_document_with_all_block_types() {
    let qmd = "\
## Title

First paragraph with *emphasis* and **strong**.

> Blockquoted paragraph.

* List item one
* List item two

```python
code_block()
```

Last paragraph.
";
    let ast = parse_qmd(qmd);
    let locs = find_top_level_str_locations(&ast);

    // Modify the first Str in each paragraph block
    for block_idx in [1, 4] {
        // block 1 = paragraph, block 4 = last paragraph
        let block_locs: Vec<_> = locs.iter().filter(|l| l.block_idx == block_idx).collect();
        if !block_locs.is_empty() {
            let new_ast = modify_str_at(&ast, block_locs[0], "Changed");
            assert_inline_roundtrip(qmd, &new_ast);
            assert_splice_equivalent_to_full_writer(qmd, &new_ast);
        }
    }
}

// =============================================================================
// Edit Monotonicity with Inline Splicing (extends Property 5)
// =============================================================================

proptest! {
    #[test]
    fn proptest_inline_edits_monotonic(
        qmd in gen_inline_rich_doc(),
        new_word in gen_word(),
        selector in 0usize..1000,
    ) {
        let ast = parse_qmd(&qmd);
        let locs = find_top_level_str_locations(&ast);
        if locs.is_empty() {
            return Ok(());
        }
        let loc = &locs[selector % locs.len()];
        let new_ast = modify_str_at(&ast, loc, &new_word);

        let original_ast = parse_qmd(&qmd);
        let plan = compute_reconciliation(&original_ast, &new_ast);
        let edits = writers::incremental::compute_incremental_edits(
            &qmd,
            &original_ast,
            &new_ast,
            &plan,
        )
        .expect("compute_incremental_edits failed");

        // Verify sorted and non-overlapping
        for window in edits.windows(2) {
            prop_assert!(
                window[0].range.end <= window[1].range.start,
                "Edits overlap or out of order: {:?} and {:?}",
                window[0],
                window[1]
            );
        }

        // Verify within bounds
        for edit in &edits {
            prop_assert!(
                edit.range.end <= qmd.len(),
                "Edit range {:?} exceeds document length {}",
                edit.range,
                qmd.len()
            );
        }
    }
}
