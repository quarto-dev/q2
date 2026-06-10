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
use pampa::node_lookup::{ContainerStep, NodePath, lookup_block};
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

/// A kanban-style div with two columns.  Used to test apply_node_edit
/// with a stripped subtree (no `s:` fields, no pool) — the scenario that
/// render-component authors produce via commitSubtreeEdit.
pub(crate) const KANBAN_DOC: &str = "\
::: {.kanban}

## backlog

* item one
* item two

## doing

* item three

:::
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
    assert_eq!(
        result,
        Some(NodePath::top_level(0)),
        "exact match must find block 0"
    );
}

/// (a) Exact value match for a heading.
#[test]
fn lookup_finds_heading_by_exact_source_info() {
    let ast = parse_qmd(SINGLE_HEADING);
    let target = ast.blocks[0].source_info().clone();
    let result = lookup_block(&ast, &target, FileId(0));
    assert_eq!(
        result,
        Some(NodePath::top_level(0)),
        "exact match must find block 0"
    );
}

/// (b) Synthetic Generated with no Invocation anchor → None.
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
    assert_eq!(
        lookup_block(&ast, &si0, FileId(0)),
        Some(NodePath::top_level(0))
    );
    assert_eq!(
        lookup_block(&ast, &si1, FileId(0)),
        Some(NodePath::top_level(1))
    );
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

/// apply_node_edit no-ops and returns original content for a synthetic source_info
/// (lookup miss → stale-AST graceful degrade, Plan 2b).
#[test]
fn apply_node_edit_noops_for_synthetic_target() {
    let a_u = parse_qmd(SINGLE_PARA);
    let a_u_json = ast_to_json(&a_u);
    let synthetic = SourceInfo::for_test();
    let si_json = source_info_to_json(&synthetic);
    let subtree_json = ast_to_json(&parse_qmd("New text.\n"));
    let result = apply_node_edit(SINGLE_PARA, &a_u_json, &si_json, &subtree_json);
    assert!(
        matches!(&result, Ok(s) if s == SINGLE_PARA),
        "expected original content unchanged, got: {result:?}"
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
//     resolve in the parent's AST and `apply_node_edit` returns the original
//     content unchanged (no-op; included content is read-only from the parent).
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
/// untransformed AST, so the edit is a no-op — included content is read-only
/// from the parent document. The original content is returned unchanged (Plan 2b).
#[test]
fn apply_node_edit_noops_for_edit_inside_include() {
    let a_u = parse_qmd(INCLUDE_DOC);
    let a_u_json = ast_to_json(&a_u);
    // A node from the included file (FileId(1)), not the parent (FileId(0)).
    let foreign = SourceInfo::original(FileId(1), 0, 10);
    let si_json = source_info_to_json(&foreign);
    let subtree_json = ast_to_json(&parse_qmd("Edited included content.\n"));
    let result = apply_node_edit(INCLUDE_DOC, &a_u_json, &si_json, &subtree_json);
    assert!(
        matches!(&result, Ok(s) if s == INCLUDE_DOC),
        "editing included content must no-op and return original; got: {result:?}"
    );
}

// =============================================================================
// Plan 2b — stale-AST miss guard: lookup miss → no-op, return original content
// =============================================================================

/// apply_node_edit must accept a replacement subtree that has no `s:` fields
/// and no pool — the wire format that render-components produce via
/// `commitSubtreeEdit` in TypeScript (which strips all `s:` fields before
/// sending).  The lenient `read_completing_source_info` path must fill in
/// the missing SourceInfo with Generated(by="direct-write") instead of
/// throwing InvalidSourceInfoRef.
///
/// Scenario: a kanban div whose children are reordered.  The subtree JSON
/// is built manually to match what TypeScript sends:
///   { "pandoc-api-version": [1,23,0], "meta": {}, "blocks": [stripped_div] }
/// where `stripped_div` has NO `s:` field and its children have no `s:` fields.
///
/// TDD: written RED before the fix; goes GREEN once read_completing_source_info
/// handles a pool-less subtree whose `s:` fields are entirely absent.
#[test]
fn apply_node_edit_accepts_stripped_subtree_no_s_fields() {
    let a_u = parse_qmd(KANBAN_DOC);
    let a_u_json = ast_to_json(&a_u);

    // Find the kanban Div (index 0 of top-level blocks).
    let kanban_block = block_at(&a_u, 0);
    let target_si = kanban_block.source_info().clone();
    let si_json = source_info_to_json(&target_si);

    // Build a replacement subtree manually — no `s:` fields, no pool.
    // Mirrors what kanban.tsx produces after commitSubtreeEdit strips s:.
    // The replacement moves "item one" from backlog to doing.
    let stripped_subtree_json = r#"{
        "pandoc-api-version": [1, 23, 0],
        "meta": {},
        "blocks": [{
            "t": "Div",
            "c": [["", ["kanban"], []], [
                {"t": "Header", "c": [2, ["", [], []], [{"t": "Str", "c": "backlog"}]]},
                {"t": "BulletList", "c": [[{"t": "Plain", "c": [{"t": "Str", "c": "item two"}]}]]},
                {"t": "Header", "c": [2, ["", [], []], [{"t": "Str", "c": "doing"}]]},
                {"t": "BulletList", "c": [
                    [{"t": "Plain", "c": [{"t": "Str", "c": "item three"}]}],
                    [{"t": "Plain", "c": [{"t": "Str", "c": "item one"}]}]
                ]}
            ]]
        }]
    }"#;

    let result = apply_node_edit(KANBAN_DOC, &a_u_json, &si_json, stripped_subtree_json);
    let new_qmd = result.expect("apply_node_edit must succeed with a stripped subtree");
    assert!(
        new_qmd.contains("item one"),
        "moved item must appear in result; got: {new_qmd:?}"
    );
    assert!(
        new_qmd.contains("item two"),
        "remaining backlog item must appear; got: {new_qmd:?}"
    );
}

/// When `lookup_block` returns `None` (stale-AST race: the target block was
/// removed between the last render and this edit), `apply_node_edit` must
/// silently no-op and return the original `content` string unchanged rather
/// than surfacing an error to the caller.
///
/// TDD: this test was written to be RED before the fix (old code returned
/// `Err(DestinationNotFound)`); it goes GREEN once the miss path returns
/// `Ok(content.to_string())`.
#[test]
fn stale_ast_miss_noops_and_returns_original_content() {
    let a_u = parse_qmd(SINGLE_PARA);
    let a_u_json = ast_to_json(&a_u);
    // A synthetic SourceInfo that will never match any block → lookup miss.
    let synthetic = SourceInfo::for_test();
    let si_json = source_info_to_json(&synthetic);
    let subtree_json = ast_to_json(&parse_qmd("New text.\n"));

    let result = apply_node_edit(SINGLE_PARA, &a_u_json, &si_json, &subtree_json);
    assert!(
        matches!(&result, Ok(s) if s == SINGLE_PARA),
        "stale-AST miss must return original content unchanged; got: {result:?}"
    );
}

// =============================================================================
// Plan 3 — Fixtures: nested-block containers
// =============================================================================

/// A paragraph and then a fenced Div (index 1) containing one Para (index 0
/// in the Div's content), then a trailing Para.
pub(crate) const PARA_IN_DIV: &str = "\
Before div.

::: {.my-div}
Hello inside div.
:::

After div.
";

/// A paragraph and then a BlockQuote (index 1) containing one Para, then a
/// trailing Para.
pub(crate) const PARA_IN_BLOCKQUOTE: &str = "\
Before quote.

> Hello inside quote.

After quote.
";

/// A loose BulletList (index 1, two items each containing a Para) bookended
/// by paragraphs.
pub(crate) const PARA_IN_BULLET_LIST: &str = "\
Before list.

- Hello in item one.

- Hello in item two.

After list.
";

/// A loose OrderedList (index 1, two items each containing a Para) bookended
/// by paragraphs.
pub(crate) const PARA_IN_ORDERED_LIST: &str = "\
Before list.

1. First ordered item.

2. Second ordered item.

After list.
";

/// A Div nested inside another Div.  Root contains one block: the outer Div
/// (index 0).  The outer Div's content contains one block: the inner Div
/// (index 0).  The inner Div's content contains one Para (index 0).
pub(crate) const DIV_IN_DIV: &str = "\
::: {.outer}
::: {.inner}
Deepest paragraph.
:::
:::
";

/// A DefinitionList desugared from a `.definition-list` div.
/// Two terms, each with one body item, then a trailing Para.
/// Exercises the four-coordinate path: DefBody(list_idx, def_idx, body_idx) + leaf_idx.
pub(crate) const PARA_IN_DEFLIST: &str = "\
::: {.definition-list}

* Term One

  * Body item one.

* Term Two

  * Body item two.

:::

After deflist.
";

/// A standalone image that produces a Figure block (index 0) whose content is
/// a Plain block (index 0) wrapping the Image inline.
pub(crate) const FIGURE_WITH_CONTENT: &str = "![alt text](image.png)\n";

/// A callout-note div (plain Div with class `callout-note`) containing one Para.
/// In the untransformed AST this is just a regular Div — no CustomNode yet.
pub(crate) const PARA_IN_CALLOUT: &str = "\
::: {.callout-note}
A paragraph inside callout.
:::
";

// =============================================================================
// Plan 3 — Helpers: nested-target traversal
// =============================================================================

/// Return the block at `inner_idx` inside the Div at `div_idx` in the root.
pub(crate) fn div_inner_block(ast: &Pandoc, div_idx: usize, inner_idx: usize) -> &Block {
    let Block::Div(d) = &ast.blocks[div_idx] else {
        panic!(
            "Expected Div at blocks[{div_idx}], got {:?}",
            ast.blocks[div_idx]
        );
    };
    &d.content[inner_idx]
}

/// Return the block at `inner_idx` inside the BlockQuote at `bq_idx`.
pub(crate) fn blockquote_inner_block(ast: &Pandoc, bq_idx: usize, inner_idx: usize) -> &Block {
    let Block::BlockQuote(bq) = &ast.blocks[bq_idx] else {
        panic!(
            "Expected BlockQuote at blocks[{bq_idx}], got {:?}",
            ast.blocks[bq_idx]
        );
    };
    &bq.content[inner_idx]
}

/// Return the block at `inner_idx` inside item `item_idx` of the list
/// (BulletList or OrderedList) at `list_idx`.
pub(crate) fn list_item_block(
    ast: &Pandoc,
    list_idx: usize,
    item_idx: usize,
    inner_idx: usize,
) -> &Block {
    match &ast.blocks[list_idx] {
        Block::BulletList(bl) => &bl.content[item_idx][inner_idx],
        Block::OrderedList(ol) => &ol.content[item_idx][inner_idx],
        b => panic!("Expected BulletList or OrderedList at blocks[{list_idx}], got {b:?}"),
    }
}

/// Return the block at `block_idx` inside body `body_idx` of definition
/// `def_idx` in the DefinitionList at `list_idx`.
pub(crate) fn deflist_body_block(
    ast: &Pandoc,
    list_idx: usize,
    def_idx: usize,
    body_idx: usize,
    block_idx: usize,
) -> &Block {
    let Block::DefinitionList(dl) = &ast.blocks[list_idx] else {
        panic!(
            "Expected DefinitionList at blocks[{list_idx}], got {:?}",
            ast.blocks[list_idx]
        );
    };
    &dl.content[def_idx].1[body_idx][block_idx]
}

/// Return the block at `block_idx` inside Figure.content at `fig_idx`.
pub(crate) fn figure_content_block(ast: &Pandoc, fig_idx: usize, block_idx: usize) -> &Block {
    let Block::Figure(f) = &ast.blocks[fig_idx] else {
        panic!(
            "Expected Figure at blocks[{fig_idx}], got {:?}",
            ast.blocks[fig_idx]
        );
    };
    &f.content[block_idx]
}

/// High-level helper: parse `content`, edit the block whose source_info is
/// `target_si` (which may be nested) to `replacement_text`, return the
/// resulting QMD.
pub(crate) fn edit_nested_block(
    content: &str,
    target_si: SourceInfo,
    replacement_text: &str,
) -> String {
    let a_u = parse_qmd(content);
    let a_u_json = ast_to_json(&a_u);
    let si_json = source_info_to_json(&target_si);
    let subtree = parse_qmd(replacement_text);
    let subtree_json = ast_to_json(&subtree);
    apply_node_edit(content, &a_u_json, &si_json, &subtree_json).expect("apply_node_edit failed")
}

/// High-level helper: delete a nested block (N→0 splice) by passing an empty
/// replacement blocks list.
pub(crate) fn delete_nested_block(content: &str, target_si: SourceInfo) -> String {
    let a_u = parse_qmd(content);
    let a_u_json = ast_to_json(&a_u);
    let si_json = source_info_to_json(&target_si);
    let empty_subtree_json = r#"{"pandoc-api-version":[1,23,0],"meta":{},"blocks":[]}"#;
    apply_node_edit(content, &a_u_json, &si_json, empty_subtree_json)
        .expect("apply_node_edit failed")
}

// =============================================================================
// Plan 3 — Lookup tests: recursive path discovery
// =============================================================================

/// lookup_block finds a Para nested inside a fenced Div.
#[test]
fn lookup_finds_block_inside_div() {
    let ast = parse_qmd(PARA_IN_DIV);
    // Div is at root index 1; its content[0] is the inner Para.
    let target = div_inner_block(&ast, 1, 0).source_info().clone();
    let result = lookup_block(&ast, &target, FileId(0));
    let expected = NodePath {
        steps: vec![ContainerStep::Blocks(1)],
        leaf_idx: 0,
    };
    assert_eq!(
        result,
        Some(expected),
        "nested Para inside Div must be found"
    );
}

/// lookup_block finds a Para nested inside a BlockQuote.
#[test]
fn lookup_finds_block_inside_blockquote() {
    let ast = parse_qmd(PARA_IN_BLOCKQUOTE);
    // BlockQuote is at root index 1.
    let target = blockquote_inner_block(&ast, 1, 0).source_info().clone();
    let result = lookup_block(&ast, &target, FileId(0));
    let expected = NodePath {
        steps: vec![ContainerStep::Blocks(1)],
        leaf_idx: 0,
    };
    assert_eq!(
        result,
        Some(expected),
        "nested Para inside BlockQuote must be found"
    );
}

/// lookup_block finds a block inside a BulletList item.
#[test]
fn lookup_finds_block_inside_bullet_list_item() {
    let ast = parse_qmd(PARA_IN_BULLET_LIST);
    // BulletList is at root index 1; item 0, inner block 0.
    let target = list_item_block(&ast, 1, 0, 0).source_info().clone();
    let result = lookup_block(&ast, &target, FileId(0));
    let expected = NodePath {
        steps: vec![ContainerStep::ListItem(1, 0)],
        leaf_idx: 0,
    };
    assert_eq!(
        result,
        Some(expected),
        "nested block inside BulletList item must be found"
    );
}

/// lookup_block finds a block inside an OrderedList item.
#[test]
fn lookup_finds_block_inside_ordered_list_item() {
    let ast = parse_qmd(PARA_IN_ORDERED_LIST);
    // OrderedList is at root index 1; item 1, inner block 0 (second item).
    let target = list_item_block(&ast, 1, 1, 0).source_info().clone();
    let result = lookup_block(&ast, &target, FileId(0));
    let expected = NodePath {
        steps: vec![ContainerStep::ListItem(1, 1)],
        leaf_idx: 0,
    };
    assert_eq!(
        result,
        Some(expected),
        "nested block inside OrderedList item 1 must be found"
    );
}

/// lookup_block navigates two levels deep: Para inside inner Div inside outer Div.
#[test]
fn lookup_finds_block_in_nested_div() {
    let ast = parse_qmd(DIV_IN_DIV);
    // Root[0] = outer Div; outer.content[0] = inner Div; inner.content[0] = Para.
    let inner_para = div_inner_block(&div_inner_block_as_ast(&ast, 0, 0), 0, 0)
        .source_info()
        .clone();
    let result = lookup_block(&ast, &inner_para, FileId(0));
    let expected = NodePath {
        steps: vec![ContainerStep::Blocks(0), ContainerStep::Blocks(0)],
        leaf_idx: 0,
    };
    assert_eq!(
        result,
        Some(expected),
        "two-level nested Para must be found with two-step path"
    );
}

/// lookup_block finds a block in a DefinitionList body (four-coordinate path).
#[test]
fn lookup_finds_block_in_deflist_body() {
    let ast = parse_qmd(PARA_IN_DEFLIST);
    // The DefinitionList is at root index 0 (before "After deflist." at index 1).
    // def 0, body 0, block 0.
    let target = deflist_body_block(&ast, 0, 0, 0, 0).source_info().clone();
    let result = lookup_block(&ast, &target, FileId(0));
    let expected = NodePath {
        steps: vec![ContainerStep::DefBody(0, 0, 0)],
        leaf_idx: 0,
    };
    assert_eq!(
        result,
        Some(expected),
        "block inside DefinitionList body must be found with four-coordinate path"
    );
}

/// lookup_block finds a block in Figure.content (not Figure.caption).
#[test]
fn lookup_finds_block_in_figure_content() {
    let ast = parse_qmd(FIGURE_WITH_CONTENT);
    // Figure is at root index 0; its content[0] is a Plain block.
    let target = figure_content_block(&ast, 0, 0).source_info().clone();
    let result = lookup_block(&ast, &target, FileId(0));
    let expected = NodePath {
        steps: vec![ContainerStep::Blocks(0)],
        leaf_idx: 0,
    };
    assert_eq!(
        result,
        Some(expected),
        "block inside Figure.content must be found"
    );
}

/// lookup_block finds a Para inside a callout-note Div (plain Div in the
/// untransformed AST, before CustomNode transforms run).
#[test]
fn lookup_finds_block_in_callout_div() {
    let ast = parse_qmd(PARA_IN_CALLOUT);
    // The callout-note Div is at root index 0.
    let target = div_inner_block(&ast, 0, 0).source_info().clone();
    let result = lookup_block(&ast, &target, FileId(0));
    let expected = NodePath {
        steps: vec![ContainerStep::Blocks(0)],
        leaf_idx: 0,
    };
    assert_eq!(
        result,
        Some(expected),
        "nested Para inside callout-note Div must be found"
    );
}

/// A target not present in the tree returns None.
#[test]
fn lookup_nested_none_for_absent_target() {
    let ast = parse_qmd(PARA_IN_DIV);
    let synthetic = SourceInfo::for_test();
    assert_eq!(
        lookup_block(&ast, &synthetic, FileId(0)),
        None,
        "absent target must return None from recursive lookup"
    );
}

/// Uniqueness: a container's own source_info does not also match its nested child.
#[test]
fn lookup_nested_container_and_child_are_distinct() {
    let ast = parse_qmd(PARA_IN_DIV);
    // The Div at root[1] has its own source_info (the whole div range).
    let div_si = ast.blocks[1].source_info().clone();
    // The Para inside the Div has a different source_info (the inner para range).
    let inner_si = div_inner_block(&ast, 1, 0).source_info().clone();
    assert_ne!(
        div_si, inner_si,
        "container and nested child must have different source_info"
    );
    // The container resolves to its own top-level path, not the child's path.
    assert_eq!(
        lookup_block(&ast, &div_si, FileId(0)),
        Some(NodePath::top_level(1)),
        "container source_info must resolve to the container's top-level path"
    );
    // The child resolves to the nested path, not the container's path.
    assert_eq!(
        lookup_block(&ast, &inner_si, FileId(0)),
        Some(NodePath {
            steps: vec![ContainerStep::Blocks(1)],
            leaf_idx: 0
        }),
        "child source_info must resolve to the nested path"
    );
}

// Helper for two-level DIV_IN_DIV test: build a synthetic Pandoc whose
// blocks slice holds the block at `outer[div_idx].content[inner_idx]`.
// This lets `div_inner_block` be called recursively on the result.
fn div_inner_block_as_ast(ast: &Pandoc, div_idx: usize, inner_idx: usize) -> Pandoc {
    let Block::Div(outer) = &ast.blocks[div_idx] else {
        panic!("Expected outer Div at blocks[{div_idx}]");
    };
    let inner_block = &outer.content[inner_idx];
    // Verify it is a Div before cloning (panics early with a clear message).
    assert!(
        matches!(inner_block, Block::Div(_)),
        "Expected inner Div at outer.content[{inner_idx}]"
    );
    let mut p = parse_qmd("");
    p.blocks = vec![inner_block.clone()];
    p
}

// =============================================================================
// Plan 3 — Apply tests: nested block editing
// =============================================================================

/// Edit a Para inside a fenced Div: result has new text, outer content intact.
#[test]
fn apply_node_edit_nested_para_in_div() {
    let ast = parse_qmd(PARA_IN_DIV);
    let target_si = div_inner_block(&ast, 1, 0).source_info().clone();
    let result = edit_nested_block(PARA_IN_DIV, target_si, "Edited inside div.\n");
    assert!(result.contains("Edited inside div."), "got: {result:?}");
    assert!(!result.contains("Hello inside div."), "got: {result:?}");
    // Text outside the container must be verbatim.
    assert!(result.contains("Before div."), "got: {result:?}");
    assert!(result.contains("After div."), "got: {result:?}");
}

/// Edit a Para inside a BlockQuote.
#[test]
fn apply_node_edit_nested_para_in_blockquote() {
    let ast = parse_qmd(PARA_IN_BLOCKQUOTE);
    let target_si = blockquote_inner_block(&ast, 1, 0).source_info().clone();
    let result = edit_nested_block(PARA_IN_BLOCKQUOTE, target_si, "Edited inside quote.\n");
    assert!(result.contains("Edited inside quote."), "got: {result:?}");
    assert!(!result.contains("Hello inside quote."), "got: {result:?}");
    assert!(result.contains("Before quote."), "got: {result:?}");
    assert!(result.contains("After quote."), "got: {result:?}");
}

/// Edit a block inside a BulletList item; sibling item is preserved.
#[test]
fn apply_node_edit_nested_para_in_bullet_list() {
    let ast = parse_qmd(PARA_IN_BULLET_LIST);
    // Edit item 0 (first item).
    let target_si = list_item_block(&ast, 1, 0, 0).source_info().clone();
    let result = edit_nested_block(PARA_IN_BULLET_LIST, target_si, "Edited item one.\n");
    assert!(result.contains("Edited item one."), "got: {result:?}");
    assert!(!result.contains("Hello in item one."), "got: {result:?}");
    // Text outside the list must be verbatim.
    assert!(result.contains("Before list."), "got: {result:?}");
    assert!(result.contains("After list."), "got: {result:?}");
    // The second item must still appear (wholesale rewrite preserves its content).
    assert!(
        result.contains("Hello in item two."),
        "sibling must survive; got: {result:?}"
    );
}

/// Edit a block inside an OrderedList item.
#[test]
fn apply_node_edit_nested_para_in_ordered_list() {
    let ast = parse_qmd(PARA_IN_ORDERED_LIST);
    let target_si = list_item_block(&ast, 1, 0, 0).source_info().clone();
    let result = edit_nested_block(PARA_IN_ORDERED_LIST, target_si, "Edited first item.\n");
    assert!(result.contains("Edited first item."), "got: {result:?}");
    assert!(!result.contains("First ordered item."), "got: {result:?}");
    assert!(result.contains("Before list."), "got: {result:?}");
    assert!(result.contains("After list."), "got: {result:?}");
    assert!(
        result.contains("Second ordered item."),
        "sibling must survive; got: {result:?}"
    );
}

/// Edit a Para two levels deep (inner Div inside outer Div).
#[test]
fn apply_node_edit_nested_div_in_div() {
    let ast = parse_qmd(DIV_IN_DIV);
    // Navigate to the innermost Para's source_info.
    let outer_div_ast = div_inner_block_as_ast(&ast, 0, 0);
    let inner_para_si = div_inner_block(&outer_div_ast, 0, 0).source_info().clone();
    let result = edit_nested_block(DIV_IN_DIV, inner_para_si, "Edited deepest.\n");
    assert!(result.contains("Edited deepest."), "got: {result:?}");
    assert!(!result.contains("Deepest paragraph."), "got: {result:?}");
}

/// Edit a block inside a DefinitionList body (four-coordinate path).
#[test]
fn apply_node_edit_nested_para_in_deflist() {
    let ast = parse_qmd(PARA_IN_DEFLIST);
    let target_si = deflist_body_block(&ast, 0, 0, 0, 0).source_info().clone();
    let result = edit_nested_block(PARA_IN_DEFLIST, target_si, "Edited body one.\n");
    assert!(result.contains("Edited body one."), "got: {result:?}");
    assert!(!result.contains("Body item one."), "got: {result:?}");
    // Text outside the list must be verbatim.
    assert!(result.contains("After deflist."), "got: {result:?}");
}

/// Edit a Para inside a callout-note Div.
#[test]
fn apply_node_edit_nested_callout_div() {
    let ast = parse_qmd(PARA_IN_CALLOUT);
    let target_si = div_inner_block(&ast, 0, 0).source_info().clone();
    let result = edit_nested_block(PARA_IN_CALLOUT, target_si, "Edited callout content.\n");
    assert!(
        result.contains("Edited callout content."),
        "got: {result:?}"
    );
    assert!(
        !result.contains("A paragraph inside callout."),
        "got: {result:?}"
    );
}

/// 1→N nested edit: replace one Para inside a Div with two Paras.
#[test]
fn apply_node_edit_nested_one_to_n_splice() {
    let ast = parse_qmd(PARA_IN_DIV);
    let target_si = div_inner_block(&ast, 1, 0).source_info().clone();
    let result = edit_nested_block(
        PARA_IN_DIV,
        target_si,
        "First replacement.\n\nSecond replacement.\n",
    );
    assert!(result.contains("First replacement."), "got: {result:?}");
    assert!(result.contains("Second replacement."), "got: {result:?}");
    assert!(!result.contains("Hello inside div."), "got: {result:?}");
    assert!(
        result.contains("Before div."),
        "outer text must survive; got: {result:?}"
    );
    assert!(
        result.contains("After div."),
        "outer text must survive; got: {result:?}"
    );
}

/// N→0 nested deletion: remove a Para inside a Div; container remains with zero blocks.
#[test]
fn apply_node_edit_nested_deletion_n_to_0() {
    let ast = parse_qmd(PARA_IN_DIV);
    let target_si = div_inner_block(&ast, 1, 0).source_info().clone();
    let result = delete_nested_block(PARA_IN_DIV, target_si);
    assert!(
        !result.contains("Hello inside div."),
        "deleted block must be gone; got: {result:?}"
    );
    // The enclosing div must still be present (container stays, just empty).
    assert!(
        result.contains(":::"),
        "enclosing div fences must remain; got: {result:?}"
    );
    assert!(result.contains("Before div."), "got: {result:?}");
    assert!(result.contains("After div."), "got: {result:?}");
}

/// Attr-strip at depth: a nested Div replacement with no `s`/`a` fields is
/// accepted via the lenient reader and written cleanly.
#[test]
fn apply_node_edit_nested_attr_strip_round_trips() {
    // A Div with an inner Div (attr-bearing).
    let content = "::: {.outer}\n\n::: {.inner}\nContent.\n:::\n\n:::\n";
    let a_u = parse_qmd(content);
    let a_u_json = ast_to_json(&a_u);
    // Target: the inner Div at outer.content[0] (root index 0, inner index 0).
    let inner_div_si = div_inner_block(&a_u, 0, 0).source_info().clone();
    let si_json = source_info_to_json(&inner_div_si);
    // Replacement: stripped subtree (no `s:` or `a:` fields) — simulates
    // commitSubtreeEdit in TypeScript.
    let stripped_replacement = r#"{
        "pandoc-api-version":[1,23,0],
        "meta":{},
        "blocks":[{"t":"Div","c":[["",["inner-modified"],[]],[{"t":"Para","c":[{"t":"Str","c":"Modified."}]}]]}]
    }"#;
    let result = apply_node_edit(content, &a_u_json, &si_json, stripped_replacement);
    let new_qmd = result.expect("attr-strip at depth must succeed");
    assert!(new_qmd.contains("Modified."), "got: {new_qmd:?}");
    assert!(!new_qmd.contains("Content."), "got: {new_qmd:?}");
}

/// Sibling fidelity: bytes OUTSIDE the enclosing container and its boundary
/// separators are verbatim; the container body may be reformatted (wholesale
/// rewrite).  We snapshot the expected reformatted form.
#[test]
fn apply_node_edit_nested_sibling_fidelity() {
    // Edit the first BulletList item; "Before list." and "After list." must be
    // byte-identical to their occurrences in the original source.
    let ast = parse_qmd(PARA_IN_BULLET_LIST);
    let target_si = list_item_block(&ast, 1, 0, 0).source_info().clone();
    let result = edit_nested_block(PARA_IN_BULLET_LIST, target_si, "Edited.\n");

    // Check outer bytes are verbatim (contains the exact substring).
    assert!(
        result.contains("Before list."),
        "pre-list bytes must be verbatim; got: {result:?}"
    );
    assert!(
        result.contains("After list."),
        "post-list bytes must be verbatim; got: {result:?}"
    );
    // Sibling item content survives (may be re-bulleted by wholesale rewrite).
    assert!(
        result.contains("Hello in item two."),
        "sibling item content must survive; got: {result:?}"
    );
}
