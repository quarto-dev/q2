/*
 * depth_cursor_roundtrip_tests.rs
 *
 * P3.5 tier-(ii) WASM round-trip composition snapshot for the block-editing
 * depth-cursor feature.
 *
 * Honesty note: this test adds NO new production code.  It is a composition
 * and regression snapshot over already-shipped `regenerate_nested_buffers` and
 * `apply_node_edit`.  Its fail-on-revert lever is the shared qmd-writer
 * BlockQuoteContext `> ` prefixer (qmd.rs:128), which also backs many other
 * tests.  The incremental value here is pinning the *end-to-end round-trip*
 * (regenerate's clean output → user edit → apply_node_edit re-wrap) that no
 * single existing test exercises.
 *
 * Round-trip:
 *   1. Parse a document with a multi-line blockquote.
 *   2. Call `regenerate_nested_buffers_ast` → the iframe receives a CLEAN
 *      buffer (no `> ` prefixes) for the blockquote child Para.
 *   3. Simulate the user editing that clean buffer.
 *   4. Re-parse the edited buffer as a subtree, then call `apply_node_edit`
 *      to commit it back.
 *   5. Assert that the committed document has `> ` markers restored on BOTH
 *      blockquote lines, and that blocks OUTSIDE the quote survive verbatim.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::apply_node_edit::apply_node_edit;
use pampa::regenerate_nested_buffers::regenerate_nested_buffers_ast;
use quarto_source_map::SourceInfo;

use crate::node_edit_tests::{ast_to_json, blockquote_inner_block, parse_qmd, source_info_to_json};

/// Helper: extract (start_offset, end_offset) from an Original SourceInfo,
/// panicking if the variant is anything else.
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

/// Full round-trip test:
///   regenerate_nested_buffers_ast (clean buffer) → user edit → apply_node_edit
///   (re-wrapped with `> ` markers).
///
/// This is the load-bearing assertion for P3.5: the qmd writer's
/// BlockQuoteContext must re-add `> ` prefixes when the edited subtree is
/// committed back.  If you neutralize the prefixer in qmd.rs:128, the
/// `result.contains("> EDITED line two.")` assertion will go RED.
#[test]
fn depth_cursor_blockquote_round_trip() {
    // ── Fixture ──────────────────────────────────────────────────────────────
    // Parses as: Para("Before quote.") [0], BlockQuote [1] with one multi-line
    // Para child, Para("After quote.") [2].
    let content = "Before quote.\n\n> Quote line one.\n> Quote line two.\n\nAfter quote.\n";

    // ── Step 1: parse ─────────────────────────────────────────────────────────
    let ast = parse_qmd(content);

    // ── Step 2: regenerate clean buffers ─────────────────────────────────────
    let map = regenerate_nested_buffers_ast(content, &ast);

    // ── Step 3: locate the blockquote child and build its siKey ──────────────
    let child = blockquote_inner_block(&ast, 1, 0);
    let (r0, r1) = original_offsets(child.source_info());
    let si_key = format!("0:{r0}-{r1}:0");

    // The map must contain an entry for this child.
    assert!(
        map.contains_key(&si_key),
        "regenerate_nested_buffers_ast must produce an entry for siKey '{si_key}'; \
         got keys: {map:?}"
    );

    // The clean buffer must NOT contain `> ` (the blockquote prefix that the
    // iframe user should NOT see).
    let clean_buf = &map[&si_key];
    assert!(
        !clean_buf.contains("> "),
        "Regenerated buffer must not contain '> ' prefix; got: {clean_buf:?}"
    );
    // But it must carry both original lines.
    assert!(
        clean_buf.contains("Quote line one."),
        "Clean buffer must contain 'Quote line one.'; got: {clean_buf:?}"
    );
    assert!(
        clean_buf.contains("Quote line two."),
        "Clean buffer must contain 'Quote line two.'; got: {clean_buf:?}"
    );

    // ── Step 4 & 5: simulate the user editing the clean buffer ───────────────
    let edited_clean = clean_buf.replace("Quote line two.", "EDITED line two.");

    // ── Step 6: re-parse the edited clean buffer as the subtree ──────────────
    let subtree = parse_qmd(&edited_clean);
    let subtree_json = ast_to_json(&subtree);

    // ── Step 7: serialize the original child's SourceInfo ────────────────────
    let si_json = source_info_to_json(child.source_info());

    // ── Step 8: serialize the original full AST ───────────────────────────────
    let a_u_json = ast_to_json(&ast);

    // ── Step 9: commit the edit ───────────────────────────────────────────────
    let result = apply_node_edit(content, &a_u_json, &si_json, &subtree_json)
        .expect("apply_node_edit failed");

    // ── Assertions ────────────────────────────────────────────────────────────

    // The edit landed.
    assert!(
        result.contains("EDITED line two."),
        "Result must contain 'EDITED line two.'; got:\n{result}"
    );

    // The old child text is gone.
    assert!(
        !result.contains("Quote line two."),
        "Result must not contain old 'Quote line two.'; got:\n{result}"
    );

    // Blocks OUTSIDE the quote survive verbatim.
    assert!(
        result.contains("Before quote."),
        "Result must contain 'Before quote.'; got:\n{result}"
    );
    assert!(
        result.contains("After quote."),
        "Result must contain 'After quote.'; got:\n{result}"
    );

    // LOAD-BEARING: the qmd writer's BlockQuoteContext must re-wrap the
    // committed clean buffer with `> ` markers on EVERY line.
    // If qmd.rs:128 (`self.inner.write_all(b"> ")`) is neutralized, these
    // two assertions go RED.
    assert!(
        result.contains("> Quote line one."),
        "Result must contain '> Quote line one.' (blockquote re-wrap); got:\n{result}"
    );
    assert!(
        result.contains("> EDITED line two."),
        "Result must contain '> EDITED line two.' (blockquote re-wrap of edited line); \
         got:\n{result}"
    );
}
