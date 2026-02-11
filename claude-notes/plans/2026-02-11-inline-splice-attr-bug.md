# InlineSplice silently drops header attribute changes

**Beads issue:** `bd-rcdo`
**Parent plan:** `claude-notes/plans/2026-02-07-incremental-writer.md` (Phase 5: Inline Splicing)

## Bug Report

When a Header block's attributes (classes, key-value pairs) change but its
inline content stays the same, the incremental writer uses the InlineSplice code
path. InlineSplice preserves the original block's byte range — including the
suffix that contains the attribute text (e.g., `{.feature created="2026-02-10"}`).
The new attributes (e.g., `status="todo"`) are silently lost.

**Discovered in:** kanban demo's `setCardStatus` mutation, which adds/modifies a
`status` key-value pair on a Header without changing the header's title text.

**Symptoms observed:**
- In the browser: WASM panic at `incremental.rs:232` ("byte index out of bounds")
  when the lost attribute change causes a cascading mismatch on a subsequent
  update cycle.
- In Rust tests: the attribute is simply missing from the round-tripped output
  (reproduced with hand-crafted tests).

## Root Cause

The `coarsen` function in `incremental.rs` (line ~139) decides whether to use
InlineSplice for `RecurseIntoContainer` blocks. The decision checks:

1. Does the block have an inline plan? (Yes — Header is an inline-content block)
2. Are the original inlines non-empty? (Yes)
3. Is the inline plan safe to splice? (`is_inline_splice_safe`)

It does NOT check whether the block's attributes changed. When only attributes
change, all three checks pass, and InlineSplice is used.

`assemble_inline_splice` (line ~533) then:
- Computes `block_span` from the **original** block's source info
- Extracts `prefix` = bytes before first inline (e.g., `## `)
- Extracts `suffix` = bytes after last inline (e.g., ` {.feature created="2026-02-10"}\n`)
- Splices the new inline content between them

The suffix contains the **original** attribute text, so the new attributes are lost.

### Why the todo app's toggleCheckbox doesn't hit this

The todo app modifies inline content (Span checkbox contents), not block-level
attributes. The inline change flows through InlineSplice correctly because
the prefix/suffix don't need to change.

## Fix Approach

Add an attribute equality check to the InlineSplice guard in `coarsen`. When the
block's source-visible attributes differ between original and new, fall back to
`Rewrite` (which regenerates the header from the AST, correctly including the
new attributes).

### What counts as "source-visible" attributes

For a Header like `## Title {.feature status="todo"}\n`:
- **Classes** (`.feature`) — always in the source when present
- **Key-value pairs** (`status="todo"`) — always in the source when present
- **Auto-generated ID** — NOT in the source (derived from header text by the
  parser). Changes to the auto-generated ID should NOT trigger Rewrite, because
  the suffix doesn't contain `{#id}`.
- **Explicit ID** (`{#custom-id}`) — IS in the source. Changes to an explicitly
  written ID should trigger Rewrite.

We can distinguish auto-generated from explicit IDs using `AttrSourceInfo.id`:
- `None` → auto-generated (no source position, not in text)
- `Some(...)` → explicitly written (has source position, in text)

### The check

In `coarsen`, when deciding InlineSplice for a `RecurseIntoContainer` block:

```rust
if !orig_inlines.is_empty()
    && is_inline_splice_safe(new_inlines, inline_plan)
    && block_attrs_eq(orig_block, new_block)  // NEW CHECK
{
    // Safe to splice
} else {
    // Fall back to Rewrite
}
```

Where `block_attrs_eq` compares:
- Classes (`attr.1`)
- Key-value pairs (`attr.2`)
- ID (`attr.0`) only when the original had an explicit ID (`attr_source.id.is_some()`)

For block types without attributes (Paragraph, Plain), `block_attrs_eq`
returns `true` (no attributes to compare).

### Affected block types

Only Header is affected in practice. `block_inlines()` returns `Some` for
Paragraph, Plain, and Header. Of these, only Header has attributes. But
for completeness and future safety, the check should also handle CodeBlock
and Div (even though they're not inline-content blocks today).

## Work Items

- [x] Write failing tests for header attribute changes (7 tests added to
      `incremental_writer_tests.rs`)
- [x] Verify tests fail (confirmed: attribute lost in round-trip)
- [x] Implement initial fix (compare classes + kvs, skip auto-generated ID)
- [x] Verify all 76 incremental writer tests pass
- [x] Verify all 6400 workspace tests pass
- [x] Refine fix: also check explicit ID using `attr_source.id.is_some()`
- [x] Add test for explicit ID change case (`## Title {#custom-id}` → `{#new-id}`)
- [x] Add test for auto-generated ID change (should still use InlineSplice)
- [x] Run full workspace test suite (6402 tests pass)
- [x] Update Phase 5 notes in incremental writer plan

## Current State

The fix is complete and passes all 6402 workspace tests. `block_attrs_eq`
compares classes, key-value pairs, and the ID when it's explicitly written
in the source (`attr_source.id.is_some()`). Auto-generated IDs are correctly
skipped.

Nine new tests were added to `incremental_writer_tests.rs`:
- `roundtrip_add_header_attribute` — add `status="todo"` to existing attrs
- `roundtrip_change_header_attribute` — change `status="todo"` to `status="done"`
- `roundtrip_add_header_attribute_with_frontmatter` — same with YAML front matter
- `roundtrip_add_header_attribute_via_json_roundtrip` — through JSON path (WASM simulation)
- `roundtrip_kanban_status_change` — full kanban scenario with multiple cards
- `roundtrip_kanban_status_change_via_json` — same through JSON path
- `roundtrip_change_header_attribute` — modify existing attribute value
- `roundtrip_change_explicit_header_id` — change explicit `{#custom-id}` to `{#new-id}`
- `roundtrip_auto_id_change_no_explicit_id_in_output` — auto-generated ID change uses InlineSplice
