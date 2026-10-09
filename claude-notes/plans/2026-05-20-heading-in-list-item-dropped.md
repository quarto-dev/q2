---
title: 'Heading inside list item is dropped'
date: 2026-05-20
---

Tracking issue: **bd-zpl4u**

## Overview

Pampa silently drops a heading that appears inside a bullet (or
ordered) list item. The minimal example

```qmd
* # Section 1
```

produces

```json
{ "t": "BulletList", "c": [ [] ] }
```

— a list with one empty item — instead of Pandoc's

```json
{ "t": "BulletList", "c": [ [ { "t": "Header", "c": [1, ["section-1", [], []], [...] ] } ] ] }
```

— a list with one item whose only block is a `Header`.

## Reproduction

```bash
echo '* # Section 1' | cargo run --bin pampa -- --to json | jq '.blocks'
# → [{"c":[[]],"s":0,"t":"BulletList"}]

echo '* # Section 1' | pandoc -f markdown -t json | jq '.blocks'
# → BulletList containing a Header
```

Verified 2026-05-20 on `main` at 6af6809a.

## Root cause

The tree-sitter parse tree is correct: the parser produces

```
list_item
  list_marker_star
  section
    atx_heading
      …
```

The bug is in the **tree-sitter → Pandoc AST converter**, not in the
grammar. In `crates/pampa/src/pandoc/treesitter.rs`, the
`process_list_item` function (lines 262–317) iterates its children
through a `filter_map` that has *no arm* for
`PandocNativeIntermediate::IntermediateSection`. The catch-all
`_ => None` (line 306) silently discards it, so the `Header` block
inside the section is never added to the list item's body.

By contrast, `process_section` in
`crates/pampa/src/pandoc/treesitter_utils/section.rs` (lines 27–29)
*does* handle `IntermediateSection` by extending its block list with
the section's contents. The fix to `process_list_item` should mirror
that pattern.

## Why this matters

Pandoc allows arbitrary block-level content inside list items —
including headings, block quotes, code blocks, and nested lists.
Silently dropping a heading is a correctness bug that produces
data-loss for documents using this construct (e.g. an outline-style
qmd file where each list item is a section heading).

## Test strategy (TDD)

Write the failing test **first**, then implement.

### Phase 1 — Failing tests

- [x] Add a parser-level test in `crates/pampa/tests/` that parses
      `* # Section 1\n` and asserts the result is a `BulletList`
      with one item whose sole block is a `Header { level: 1, … }`.
      → `tests/snapshots/native/heading-in-bullet-list-item.qmd` +
      `snapshots/native/heading-in-bullet-list-item.snap`.
- [x] Add a variant test with an ordered list:
      `1. # Section 1\n`.
      → `tests/snapshots/native/heading-in-ordered-list-item.qmd`.
- [x] Add a variant test with a heading item and a plain item
      side-by-side to confirm the regression doesn't cascade.
      → `tests/snapshots/native/heading-and-plain-list-items.qmd`.
- [x] Confirmed all three fail with the expected diff —
      `[BulletList [[]…]]` (empty item) vs. expected
      `[BulletList [[Header …]…]]`.

### Phase 2 — Implementation

- [x] Refactor `process_list_item` in
      `crates/pampa/src/pandoc/treesitter.rs` from `filter_map` to
      an explicit `Vec<Block>` builder loop, mirroring
      `process_section`. Added an `IntermediateSection(section)`
      arm that `blocks.extend(section)`.
- [x] Filter bullet markers (`list_marker_minus`,
      `list_marker_star`, `list_marker_plus`) and
      `block_continuation` by node name (they map to
      `IntermediateUnknown` but are structural noise).
- [x] Handle `IntermediateUnknown` explicitly as a no-op (tree-sitter
      `ERROR` nodes from error recovery, fence delimiters, etc.
      legitimately appear inside list items and were previously
      silently dropped).
- [x] Truly-unexpected variants now `panic!` with a descriptive
      message (matching `process_section`), so future regressions
      surface loudly rather than dropping data.

### Phase 3 — Regression sweep

- [x] `cargo nextest run -p pampa` — 3759 passed.
- [x] `cargo nextest run --workspace` — 9204 passed.
- [x] `cargo xtask verify --skip-hub-build` — all 12 verification
      steps passed.

### Phase 4 — End-to-end verification

- [x] Ran the binary against all three fixtures. Inspected JSON
      output and confirmed it matches Pandoc's structure.

  ```bash
  echo '* # Section 1' | cargo run --bin pampa -- --to json | jq '.blocks'
  ```

  Now returns a `BulletList` whose item contains a `Header` block
  (level 1, id "section-1", inlines `[Str "Section", Space, Str "1"]`) —
  structurally identical to Pandoc's output. Verified 2026-05-20.

## Out of scope

- The separate observation that, in the input
  ```
  * ## Section 2
    some text
  ```
  the indented "some text" gets *absorbed into* the
  `atx_heading` node by the grammar (its span runs to row 1, col
  2). That's a tree-sitter grammar issue distinct from this
  converter bug, and should be filed separately if it isn't
  already.
- bd-vet6 (multi-line block quote inside list item fails to
  parse) is a related-but-distinct tree-sitter scanner bug. Not
  this issue.

## Files

- `crates/pampa/src/pandoc/treesitter.rs` — site of the fix
  (`process_list_item`, lines 262–317).
- `crates/pampa/src/pandoc/treesitter_utils/section.rs` — the
  pattern to mirror (`process_section`, lines 15–43).
- `crates/pampa/src/pandoc/treesitter_utils/pandocnativeintermediate.rs`
  — the `PandocNativeIntermediate::IntermediateSection` variant
  definition.
