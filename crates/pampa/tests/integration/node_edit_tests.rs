/*
 * node_edit_tests.rs
 *
 * Fixtures and shared helpers for the apply_node_edit test suite
 * (Phases 0–5 of claude-notes/plans/2026-06-04-target-incremental-writes.md).
 *
 * Phase 0: QMD corpus + helper functions.
 * Phase 2: Destination-node lookup (added below).
 * Phase 3: apply_node_edit end-to-end (added below).
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::apply_node_edit::apply_node_edit;
use pampa::node_lookup::lookup_block;
use pampa::pandoc::{ASTContext, Block, Pandoc};
use pampa::wasm_entry_points::qmd_to_pandoc;
use pampa::writers;
use quarto_source_map::{FileId, SourceInfo};

// =============================================================================
// Fixtures
// =============================================================================

/// A document with a single paragraph.
pub(crate) const SINGLE_PARA: &str = "Hello world.\n";

/// A document with a single heading.
pub(crate) const SINGLE_HEADING: &str = "# My Heading\n";

/// A paragraph adjacent to a fenced div.
pub(crate) const PARA_AND_DIV: &str = "\
Hello world.

::: {.my-div}
Content in div.
:::
";

/// A document containing an inline shortcode invocation.
/// In the untransformed AST (qmd_to_pandoc output) the shortcode
/// appears as Inline::Shortcode.  After ShortcodeResolveStage it
/// becomes a Generated node with preimage pointing at the {{< >}} token.
pub(crate) const SHORTCODE_DOC: &str = "\
Paragraph with {{< kbd Enter >}} shortcode.
";

/// Two identical paragraphs — guards the structural-minimality claim:
/// editing one must leave the twin untouched (Phase 3).
pub(crate) const DUPLICATE_BLOCKS: &str = "\
Hello world.

Hello world.
";

// =============================================================================
// Helpers
// =============================================================================

/// Parse a QMD string into a Pandoc AST.
/// Uses the same reader as qmd_to_pandoc (the WASM / pipeline entry point).
pub(crate) fn parse_qmd(content: &str) -> Pandoc {
    qmd_to_pandoc(content.as_bytes())
        .map(|(pandoc, _ctx)| pandoc)
        .unwrap_or_else(|errs| panic!("Failed to parse QMD: {:?}", errs))
}

/// Parse a QMD string and return its top-level blocks — a "pure subtree"
/// suitable for use as a replacement subtree in apply_node_edit.
pub(crate) fn pure_blocks_from_qmd(content: &str) -> Vec<Block> {
    parse_qmd(content).blocks
}

/// Pick the block at `idx` from an AST (panics if out of bounds).
pub(crate) fn block_at(ast: &Pandoc, idx: usize) -> &Block {
    &ast.blocks[idx]
}

// =============================================================================
// Phase 0 smoke tests — every fixture parses cleanly
// =============================================================================

#[test]
fn single_para_parses() {
    let ast = parse_qmd(SINGLE_PARA);
    assert_eq!(ast.blocks.len(), 1);
    assert!(matches!(ast.blocks[0], Block::Paragraph(_)));
}

#[test]
fn single_heading_parses() {
    let ast = parse_qmd(SINGLE_HEADING);
    assert_eq!(ast.blocks.len(), 1);
    assert!(matches!(ast.blocks[0], Block::Header(_)));
}

#[test]
fn para_and_div_parses() {
    let ast = parse_qmd(PARA_AND_DIV);
    assert_eq!(ast.blocks.len(), 2);
    assert!(matches!(ast.blocks[0], Block::Paragraph(_)));
    assert!(matches!(ast.blocks[1], Block::Div(_)));
}

#[test]
fn shortcode_doc_parses() {
    let ast = parse_qmd(SHORTCODE_DOC);
    assert!(
        !ast.blocks.is_empty(),
        "shortcode doc should parse to at least one block"
    );
    // The shortcode is inline — the containing paragraph must be present.
    assert!(matches!(ast.blocks[0], Block::Paragraph(_)));
}

#[test]
fn duplicate_blocks_parses() {
    let ast = parse_qmd(DUPLICATE_BLOCKS);
    assert_eq!(ast.blocks.len(), 2);
    assert!(matches!(ast.blocks[0], Block::Paragraph(_)));
    assert!(matches!(ast.blocks[1], Block::Paragraph(_)));
}

/// The two identical paragraphs in DUPLICATE_BLOCKS have *different*
/// source_info values (different byte ranges) even though their text is the same.
/// This is a prerequisite for the minimality claim in Phase 3.
#[test]
fn duplicate_blocks_have_distinct_source_info() {
    let ast = parse_qmd(DUPLICATE_BLOCKS);
    assert_eq!(ast.blocks.len(), 2);
    let si0 = ast.blocks[0].source_info();
    let si1 = ast.blocks[1].source_info();
    assert_ne!(
        si0, si1,
        "identical-text paragraphs must have different source_info (different byte ranges)"
    );
}

#[test]
fn pure_blocks_from_qmd_returns_single_para() {
    let blocks = pure_blocks_from_qmd("New text.\n");
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0], Block::Paragraph(_)));
}

#[test]
fn pure_blocks_from_qmd_returns_single_heading() {
    let blocks = pure_blocks_from_qmd("# Replaced Heading\n");
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0], Block::Header(_)));
}

// =============================================================================
// Phase 2 tests — destination-node lookup
// =============================================================================

/// (a) Exact value match: Original source_info of a paragraph finds that block.
#[test]
fn lookup_finds_para_by_exact_source_info() {
    let ast = parse_qmd(SINGLE_PARA);
    let target = ast.blocks[0].source_info().clone();
    let result = lookup_block(&ast, &target, FileId(0));
    assert_eq!(result, Some(0), "exact match must find block 0");
}

/// (a) Exact value match for a heading.
#[test]
fn lookup_finds_heading_by_exact_source_info() {
    let ast = parse_qmd(SINGLE_HEADING);
    let target = ast.blocks[0].source_info().clone();
    let result = lookup_block(&ast, &target, FileId(0));
    assert_eq!(result, Some(0), "exact match must find block 0");
}

/// (b) Generated source_info with Invocation anchor: preimage_in fallback finds
/// the block whose byte range covers the invocation site.
#[test]
fn lookup_finds_block_via_generated_preimage_fallback() {
    use quarto_source_map::{AnchorRole, By};
    use std::sync::Arc;

    let ast = parse_qmd(SINGLE_PARA);
    // Get the actual byte range of block 0 from its source_info.
    let block_si = ast.blocks[0].source_info().clone();
    // Wrap it in a Generated with an Invocation anchor.
    let mut generated_si = SourceInfo::generated(By {
        kind: "shortcode".to_string(),
        data: serde_json::Value::Null,
    });
    generated_si.append_anchor(AnchorRole::Invocation, Arc::new(block_si));

    let result = lookup_block(&ast, &generated_si, FileId(0));
    assert_eq!(
        result,
        Some(0),
        "preimage_in fallback must find the block covering the invocation byte range"
    );
}

/// (c) Synthetic Generated with no Invocation anchor → None.
#[test]
fn lookup_returns_none_for_synthetic_no_preimage() {
    let ast = parse_qmd(SINGLE_PARA);
    let synthetic = SourceInfo::for_test(); // Generated, no anchors
    let result = lookup_block(&ast, &synthetic, FileId(0));
    assert_eq!(
        result, None,
        "synthetic source_info with no preimage must return None"
    );
}

/// (d) Duplicate blocks: each paragraph's source_info resolves to its own block.
#[test]
fn lookup_disambiguates_duplicate_text_blocks() {
    let ast = parse_qmd(DUPLICATE_BLOCKS);
    assert_eq!(ast.blocks.len(), 2);
    let si0 = ast.blocks[0].source_info().clone();
    let si1 = ast.blocks[1].source_info().clone();
    assert_eq!(lookup_block(&ast, &si0, FileId(0)), Some(0));
    assert_eq!(lookup_block(&ast, &si1, FileId(0)), Some(1));
}

// =============================================================================
// Phase 3 helpers
// =============================================================================

/// Serialize a Pandoc AST to JSON (same format as parse_qmd_content / render).
pub(crate) fn ast_to_json(ast: &Pandoc) -> String {
    let mut buf = Vec::new();
    let context = ASTContext::default();
    writers::json::write(ast, &context, &mut buf).expect("json write failed");
    String::from_utf8(buf).expect("non-utf8 json")
}

/// Serialize a SourceInfo to JSON (the resolved value, not a pool id).
pub(crate) fn source_info_to_json(si: &SourceInfo) -> String {
    serde_json::to_string(si).expect("SourceInfo serialize failed")
}

/// High-level helper: parse `content`, edit block at `block_idx` to
/// `replacement_text`, and return the resulting QMD via apply_node_edit.
pub(crate) fn edit_block(content: &str, block_idx: usize, replacement_text: &str) -> String {
    let a_u = parse_qmd(content);
    let a_u_json = ast_to_json(&a_u);
    let target_si = a_u.blocks[block_idx].source_info().clone();
    let si_json = source_info_to_json(&target_si);
    let subtree = parse_qmd(replacement_text);
    let subtree_json = ast_to_json(&subtree);
    apply_node_edit(content, &a_u_json, &si_json, &subtree_json).expect("apply_node_edit failed")
}

// =============================================================================
// Phase 3 tests — apply_node_edit end-to-end
// =============================================================================

/// Editing the single paragraph changes only that block's text.
#[test]
fn apply_node_edit_replaces_single_paragraph() {
    let result = edit_block(SINGLE_PARA, 0, "Replaced text.\n");
    assert!(
        result.contains("Replaced text."),
        "new text must appear in result; got: {result:?}"
    );
    assert!(
        !result.contains("Hello world."),
        "old text must not appear in result; got: {result:?}"
    );
}

/// Editing the heading changes only that block.
#[test]
fn apply_node_edit_replaces_heading() {
    let result = edit_block(SINGLE_HEADING, 0, "# New Heading\n");
    assert!(result.contains("New Heading"), "got: {result:?}");
    assert!(!result.contains("My Heading"), "got: {result:?}");
}

/// Structural minimality: editing block 0 of DUPLICATE_BLOCKS leaves block 1
/// byte-identical to the original.
#[test]
fn apply_node_edit_duplicate_blocks_minimality() {
    let result = edit_block(DUPLICATE_BLOCKS, 0, "Edited first.\n");
    // The edited block changed.
    assert!(result.contains("Edited first."), "got: {result:?}");
    // The twin is preserved verbatim.
    assert!(
        result.contains("Hello world."),
        "twin block must remain; got: {result:?}"
    );
    // Exactly one "Hello world." remains (the twin), not two.
    assert_eq!(
        result.matches("Hello world.").count(),
        1,
        "exactly one twin must remain; got: {result:?}"
    );
}

/// Editing block 1 of DUPLICATE_BLOCKS leaves block 0 unchanged.
#[test]
fn apply_node_edit_duplicate_blocks_edit_second() {
    let result = edit_block(DUPLICATE_BLOCKS, 1, "Edited second.\n");
    assert!(result.contains("Edited second."), "got: {result:?}");
    assert!(
        result.contains("Hello world."),
        "first twin must remain; got: {result:?}"
    );
    assert_eq!(result.matches("Hello world.").count(), 1, "got: {result:?}");
}

/// apply_node_edit returns DestinationNotFound for a synthetic source_info.
#[test]
fn apply_node_edit_returns_error_for_synthetic_target() {
    use pampa::apply_node_edit::ApplyNodeEditError;
    let a_u = parse_qmd(SINGLE_PARA);
    let a_u_json = ast_to_json(&a_u);
    let synthetic = SourceInfo::for_test();
    let si_json = source_info_to_json(&synthetic);
    let subtree_json = ast_to_json(&parse_qmd("New text.\n"));
    let result = apply_node_edit(SINGLE_PARA, &a_u_json, &si_json, &subtree_json);
    assert!(
        matches!(result, Err(ApplyNodeEditError::DestinationNotFound)),
        "expected DestinationNotFound, got: {result:?}"
    );
}

// =============================================================================
// Phase 5 tests — edit surface: inline markdown round-trips correctly
// =============================================================================

/// Inline markdown typed by the user (e.g. `*emph*`) round-trips through
/// `parse_qmd_content` and back to QMD correctly — no JS tokenisation
/// or double-escaping.
#[test]
fn apply_node_edit_inline_markdown_roundtrips() {
    // Content: a heading followed by a plain paragraph.
    let content = "# Title\n\nHello world.\n";
    let a_u = parse_qmd(content);
    let a_u_json = ast_to_json(&a_u);
    // Edit the paragraph (block 1) to include inline emphasis.
    let target_si = a_u.blocks[1].source_info().clone();
    let si_json = source_info_to_json(&target_si);
    // The replacement text has inline markdown — it must survive parse/write.
    let replacement = "Hello *world*.\n";
    let subtree = parse_qmd(replacement);
    let subtree_json = ast_to_json(&subtree);

    let result = apply_node_edit(content, &a_u_json, &si_json, &subtree_json)
        .expect("apply_node_edit failed");

    // The heading should be verbatim.
    assert!(
        result.starts_with("# Title"),
        "heading must be verbatim; got: {result:?}"
    );
    // The paragraph should contain the emphasis markup.
    assert!(
        result.contains("*world*"),
        "inline emph must round-trip; got: {result:?}"
    );
    assert!(
        !result.contains("Hello world."),
        "old text must be gone; got: {result:?}"
    );
}

/// Editing a heading updates just the heading, leaving the paragraph unchanged.
#[test]
fn apply_node_edit_heading_leaves_paragraph_unchanged() {
    let content = "# Old Title\n\nSome text.\n";
    let a_u = parse_qmd(content);
    let a_u_json = ast_to_json(&a_u);
    let target_si = a_u.blocks[0].source_info().clone(); // heading = block 0
    let si_json = source_info_to_json(&target_si);
    let subtree = parse_qmd("# New Title\n");
    let subtree_json = ast_to_json(&subtree);

    let result =
        apply_node_edit(content, &a_u_json, &si_json, &subtree_json).expect("apply failed");
    assert!(
        result.contains("New Title"),
        "new heading must appear; got: {result:?}"
    );
    assert!(
        !result.contains("Old Title"),
        "old heading must be gone; got: {result:?}"
    );
    assert!(
        result.contains("Some text."),
        "paragraph must be unchanged; got: {result:?}"
    );
}

// =============================================================================
// Include round-trip
//
// Includes need *no* include-specific write-back machinery under the
// node-edit architecture. `apply_node_edit` reconciles against the
// *untransformed* AST (`qmd_to_pandoc(content)`), which contains the raw,
// unexpanded `{{< include child.qmd >}}` token — `IncludeExpansionStage`
// runs later in the pipeline and never touches this tree. So:
//
//   - editing a node *outside* the include leaves the include token as a
//     KeepBefore block, copied verbatim; and
//   - editing a node *inside* the include is impossible from the parent: that
//     node's source_info is rooted in the included file, so it does not
//     resolve in the parent's AST and `apply_node_edit` returns
//     DestinationNotFound (i.e. included content is read-only from the parent).
//
// These two tests pin that behavior. (They replace the former
// "Plan 8 — IncludeExpansion CustomNode" design, whose wrapper / soft-drop
// machinery was for the reverted Plan-7 write-back model and is unnecessary
// here.)
// =============================================================================

/// A parent document with an include between two paragraphs. In the
/// untransformed AST the include is `Para[Shortcode("include", "child.qmd")]`
/// (block 1); the paragraphs are blocks 0 and 2.
const INCLUDE_DOC: &str = "\
Intro paragraph.

{{< include child.qmd >}}

Outro paragraph.
";

/// Editing a paragraph outside the include leaves the `{{< include >}}` token
/// byte-for-byte intact (it reconciles as KeepBefore and is copied verbatim).
#[test]
fn apply_node_edit_preserves_include_token_on_outside_edit() {
    let result = edit_block(INCLUDE_DOC, 0, "Edited intro.\n");
    assert!(
        result.contains("{{< include child.qmd >}}"),
        "include token must be preserved verbatim; got: {result:?}"
    );
    assert!(result.contains("Edited intro."), "got: {result:?}");
    assert!(!result.contains("Intro paragraph."), "got: {result:?}");
    assert!(
        result.contains("Outro paragraph."),
        "the other untouched paragraph must remain; got: {result:?}"
    );
}

/// Content that originated *inside* an include carries source_info rooted in
/// the included file (a different FileId). It does not resolve in the parent's
/// untransformed AST, so the edit is rejected — included content is read-only
/// from the parent document, with no include-specific code path involved.
#[test]
fn apply_node_edit_rejects_edit_inside_include() {
    use pampa::apply_node_edit::ApplyNodeEditError;
    let a_u = parse_qmd(INCLUDE_DOC);
    let a_u_json = ast_to_json(&a_u);
    // A node from the included file (FileId(1)), not the parent (FileId(0)).
    let foreign = SourceInfo::original(FileId(1), 0, 10);
    let si_json = source_info_to_json(&foreign);
    let subtree_json = ast_to_json(&parse_qmd("Edited included content.\n"));
    let result = apply_node_edit(INCLUDE_DOC, &a_u_json, &si_json, &subtree_json);
    assert!(
        matches!(result, Err(ApplyNodeEditError::DestinationNotFound)),
        "editing included content must be rejected; got: {result:?}"
    );
}
