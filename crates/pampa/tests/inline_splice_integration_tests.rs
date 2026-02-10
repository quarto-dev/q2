/*
 * inline_splice_integration_tests.rs
 *
 * End-to-end tests for Phase 5d/5e: inline splicing in the incremental writer.
 * These tests parse QMD, modify inlines in the AST, reconcile, and verify that
 * the incremental writer produces correct output via inline splicing.
 *
 * See: claude-notes/plans/2026-02-10-inline-splicing.md
 * Beads issue: bd-1hwd
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::pandoc::{Block, Inline, Pandoc};
use pampa::writers;
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

/// Modify a Str inline's text within a block's inline content.
/// Returns the modified AST.
fn modify_str_text(ast: &Pandoc, block_idx: usize, inline_idx: usize, new_text: &str) -> Pandoc {
    let mut new_ast = ast.clone();
    let inlines = block_inlines_mut(&mut new_ast.blocks[block_idx]);
    if let Some(Inline::Str(s)) = inlines.get_mut(inline_idx) {
        s.text = new_text.to_string();
        s.source_info = SourceInfo::default();
    } else {
        panic!(
            "Expected Str at block[{}].inlines[{}], got {:?}",
            block_idx,
            inline_idx,
            inlines.get(inline_idx).map(|i| std::mem::discriminant(i))
        );
    }
    new_ast
}

/// Get mutable inline content of a block.
fn block_inlines_mut(block: &mut Block) -> &mut Vec<Inline> {
    match block {
        Block::Paragraph(p) => &mut p.content,
        Block::Plain(p) => &mut p.content,
        Block::Header(h) => &mut h.content,
        other => panic!(
            "Expected inline-content block, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

/// Navigate into a BlockQuote → first inner block → inlines
fn blockquote_first_inlines_mut(block: &mut Block) -> &mut Vec<Inline> {
    let bq = match block {
        Block::BlockQuote(bq) => bq,
        other => panic!(
            "Expected BlockQuote, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    block_inlines_mut(&mut bq.content[0])
}

/// Navigate into a BulletList → first item → first block → inlines
fn bulletlist_first_item_inlines_mut(block: &mut Block) -> &mut Vec<Inline> {
    let bl = match block {
        Block::BulletList(bl) => bl,
        other => panic!(
            "Expected BulletList, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    block_inlines_mut(&mut bl.content[0][0])
}

/// Test that incremental write produces correct output and round-trips.
fn assert_incremental_write_correct(original_qmd: &str, new_ast: &Pandoc) {
    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, new_ast);

    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, new_ast, &plan)
            .expect("incremental_write failed");

    // Verify round-trip: parsing the result should produce an AST structurally
    // equivalent to new_ast
    let result_ast = parse_qmd(&result);
    let expected_qmd = write_qmd(new_ast);
    let expected_ast = parse_qmd(&expected_qmd);

    // Compare by re-writing both to QMD and checking equality
    let result_rewritten = write_qmd(&result_ast);
    let expected_rewritten = write_qmd(&expected_ast);
    assert_eq!(
        result_rewritten, expected_rewritten,
        "Incremental write round-trip failed.\nOriginal: {:?}\nResult:   {:?}\nExpected: {:?}",
        original_qmd, result, expected_qmd
    );
}

// =============================================================================
// Tests: Single Str change in top-level paragraph
// =============================================================================

#[test]
fn splice_str_change_in_paragraph() {
    let original_qmd = "Hello world.\n";
    let ast = parse_qmd(original_qmd);
    let new_ast = modify_str_text(&ast, 0, 0, "Goodbye");

    assert_incremental_write_correct(original_qmd, &new_ast);

    // Verify the actual output
    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .unwrap();
    assert_eq!(result, "Goodbye world.\n");
}

#[test]
fn splice_str_change_preserves_surrounding_text() {
    let original_qmd = "The quick brown fox.\n";
    let ast = parse_qmd(original_qmd);
    // Change "quick" (index 2, after "The" and Space) to "slow"
    let new_ast = modify_str_text(&ast, 0, 2, "slow");

    assert_incremental_write_correct(original_qmd, &new_ast);

    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .unwrap();
    assert_eq!(result, "The slow brown fox.\n");
}

// =============================================================================
// Tests: Str change in header (preserves ## prefix)
// =============================================================================

#[test]
fn splice_str_change_in_header() {
    let original_qmd = "## Hello World\n";
    let ast = parse_qmd(original_qmd);
    let mut new_ast = modify_str_text(&ast, 0, 0, "Goodbye");
    // Also update the header's auto-generated identifier to match the new text,
    // otherwise the round-trip comparison will fail due to ID mismatch.
    if let Block::Header(ref mut h) = new_ast.blocks[0] {
        h.attr.0 = "goodbye-world".to_string();
    }

    assert_incremental_write_correct(original_qmd, &new_ast);

    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .unwrap();
    // The header prefix "## " should be preserved
    assert_eq!(result, "## Goodbye World\n");
}

// =============================================================================
// Tests: Str change preserves multi-line paragraph (Scenario B)
// =============================================================================

#[test]
fn splice_str_change_in_multiline_paragraph() {
    // Multi-line paragraph: "Hello\nworld\n"
    // Change "Hello" to "Goodbye" — SoftBreak is KeepBefore → safe
    let original_qmd = "Hello\nworld\n";
    let ast = parse_qmd(original_qmd);
    let new_ast = modify_str_text(&ast, 0, 0, "Goodbye");

    assert_incremental_write_correct(original_qmd, &new_ast);

    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .unwrap();
    assert_eq!(result, "Goodbye\nworld\n");
}

// =============================================================================
// Tests: Str change inside blockquote
// =============================================================================

#[test]
fn splice_str_change_in_blockquote() {
    let original_qmd = "> Hello world.\n";
    let mut new_ast = parse_qmd(original_qmd);
    let inlines = blockquote_first_inlines_mut(&mut new_ast.blocks[0]);
    if let Inline::Str(ref mut s) = inlines[0] {
        s.text = "Goodbye".to_string();
        s.source_info = SourceInfo::default();
    }

    assert_incremental_write_correct(original_qmd, &new_ast);
}

#[test]
fn splice_str_change_in_multiline_blockquote() {
    // "> Hello\n> world\n" — change "Hello" to "Goodbye"
    // SoftBreak includes "\n> " and is KeepBefore → safe (Scenario B)
    let original_qmd = "> Hello\n> world\n";
    let mut new_ast = parse_qmd(original_qmd);
    let inlines = blockquote_first_inlines_mut(&mut new_ast.blocks[0]);
    if let Inline::Str(ref mut s) = inlines[0] {
        s.text = "Goodbye".to_string();
        s.source_info = SourceInfo::default();
    }

    assert_incremental_write_correct(original_qmd, &new_ast);

    // Verify the blockquote prefix is preserved
    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .unwrap();
    assert_eq!(result, "> Goodbye\n> world\n");
}

// =============================================================================
// Tests: Str change inside bullet list
// =============================================================================

#[test]
fn splice_str_change_in_bulletlist() {
    let original_qmd = "* Hello world.\n";
    let mut new_ast = parse_qmd(original_qmd);
    let inlines = bulletlist_first_item_inlines_mut(&mut new_ast.blocks[0]);
    if let Inline::Str(ref mut s) = inlines[0] {
        s.text = "Goodbye".to_string();
        s.source_info = SourceInfo::default();
    }

    assert_incremental_write_correct(original_qmd, &new_ast);
}

#[test]
fn splice_str_change_in_multiline_bulletlist() {
    // "* Hello\n  world\n" — change "Hello" to "Goodbye"
    let original_qmd = "* Hello\n  world\n";
    let mut new_ast = parse_qmd(original_qmd);
    let inlines = bulletlist_first_item_inlines_mut(&mut new_ast.blocks[0]);
    if let Inline::Str(ref mut s) = inlines[0] {
        s.text = "Goodbye".to_string();
        s.source_info = SourceInfo::default();
    }

    assert_incremental_write_correct(original_qmd, &new_ast);

    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .unwrap();
    // The list continuation indent should be preserved
    assert_eq!(result, "* Goodbye\n  world\n");
}

// =============================================================================
// Tests: Multi-block document with localized change
// =============================================================================

#[test]
fn splice_preserves_other_blocks() {
    let original_qmd = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n";
    let ast = parse_qmd(original_qmd);
    // Change "Second" to "Modified" in block index 1 (second paragraph)
    let new_ast = modify_str_text(&ast, 1, 0, "Modified");

    assert_incremental_write_correct(original_qmd, &new_ast);

    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .unwrap();
    assert_eq!(
        result,
        "First paragraph.\n\nModified paragraph.\n\nThird paragraph.\n"
    );
}

// =============================================================================
// Tests: Verify InlineSplice is actually used (not Rewrite)
// =============================================================================

#[test]
fn splice_uses_inline_splice_not_rewrite() {
    // Verify that the reconciliation plan has RecurseIntoContainer (not UseAfter)
    // for a paragraph with a single Str change
    let original_qmd = "Hello world.\n";
    let ast = parse_qmd(original_qmd);
    let new_ast = modify_str_text(&ast, 0, 0, "Goodbye");

    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);

    // The plan should have RecurseIntoContainer (not UseAfter) since the
    // paragraph structure is the same, only the inline content changed
    assert!(
        matches!(
            plan.block_alignments[0],
            BlockAlignment::RecurseIntoContainer { .. }
        ),
        "Expected RecurseIntoContainer, got {:?}",
        plan.block_alignments[0]
    );

    // And there should be an inline plan for this block
    assert!(
        plan.inline_plans.contains_key(&0),
        "Expected inline_plans to contain key 0"
    );
}

// =============================================================================
// Tests: Emphasis change (RecurseIntoContainer at inline level)
// =============================================================================

#[test]
fn splice_str_change_inside_emphasis() {
    let original_qmd = "*Hello* world.\n";
    let mut new_ast = parse_qmd(original_qmd);

    // Navigate into Emph → Str and change the text
    if let Block::Paragraph(ref mut p) = new_ast.blocks[0] {
        if let Inline::Emph(ref mut emph) = p.content[0] {
            if let Inline::Str(ref mut s) = emph.content[0] {
                s.text = "Goodbye".to_string();
                s.source_info = SourceInfo::default();
            }
        }
    }

    assert_incremental_write_correct(original_qmd, &new_ast);

    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .unwrap();
    // The emphasis delimiters should be preserved from original source
    assert_eq!(result, "*Goodbye* world.\n");
}

#[test]
fn splice_str_change_inside_strong() {
    let original_qmd = "**Hello** world.\n";
    let mut new_ast = parse_qmd(original_qmd);

    if let Block::Paragraph(ref mut p) = new_ast.blocks[0] {
        if let Inline::Strong(ref mut strong) = p.content[0] {
            if let Inline::Str(ref mut s) = strong.content[0] {
                s.text = "Goodbye".to_string();
                s.source_info = SourceInfo::default();
            }
        }
    }

    assert_incremental_write_correct(original_qmd, &new_ast);

    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .unwrap();
    assert_eq!(result, "**Goodbye** world.\n");
}

// =============================================================================
// Tests: Idempotence with inline splicing
// =============================================================================

#[test]
fn splice_idempotent_simple_paragraph() {
    let original_qmd = "Hello world.\n";
    let ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&ast, &ast);
    let result = writers::incremental::incremental_write(original_qmd, &ast, &ast, &plan).unwrap();
    assert_eq!(result, original_qmd);
}

#[test]
fn splice_idempotent_blockquote_multiline() {
    let original_qmd = "> Hello\n> world\n";
    let ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&ast, &ast);
    let result = writers::incremental::incremental_write(original_qmd, &ast, &ast, &plan).unwrap();
    assert_eq!(result, original_qmd);
}
