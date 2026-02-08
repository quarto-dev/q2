# Span Canonical Form: drop `{}` for empty attributes

**Beads issue:** `bd-1s21`
**Branch:** `feature/incremental-writer`
**Status:** COMPLETE

## Overview

Change the QMD writer so that spans with empty attributes omit the trailing `{}`. This makes the output cleaner and, critically, allows `[ ]` and `[x]` to roundtrip cleanly for the checkbox use case in hub-react-todo.

**Before:**
- `Span([], [])` → `[]{}`
- `Span([], [Str("x")])` → `[x]{}`
- `Span([], [Str("hello")])` → `[hello]{}`

**After:**
- `Span([], [])` → `[ ]` (space for readability/disambiguation)
- `Span([], [Str("x")])` → `[x]`
- `Span([], [Str("hello")])` → `[hello]`

## Context

- The QMD parser already treats `[content]` (without `{}`) as a Span. Verified:
  - `[hello]` → `Span(("", [], []), [Str("hello")])` ✓
  - `[x]` → `Span(("", [], []), [Str("x")])` ✓
  - `[two words]` → `Span(("", [], []), [Str("two"), Space, Str("words")])` ✓
  - `[**bold**]` → `Span(("", [], []), [Strong([Str("bold")])])` ✓
  - `[ ]` → `Span(("", [], []), [])` ✓
  - `[]` → `Span(("", [], []), [])` ✓
- So dropping `{}` for empty attrs is always safe — the parser will still produce the same AST.
- The incremental writer delegates to the standard writer for rewrites, so this change propagates automatically.

## Key Design Decisions

**1. Drop `{}` when `is_empty_attr(&span.attr)` is true.** This is the simple, general rule. No special-casing for `[x]` vs `[hello]` is needed because the parser handles all of them the same way.

**2. Empty span gets a space: `[ ]` not `[]`.** When `span.content` is empty AND `is_empty_attr`, write `[ ]` with a space. This is for readability and to distinguish from the empty-cell `[]` syntax in list tables.

**3. Spans with non-empty attributes are unchanged.** `[hello]{.foo}` and `[]{.foo}` continue to work as before.

## Work Items

### Phase 1: Assess Blast Radius

- [x] Make the change in `write_span` in `qmd.rs`
- [x] Run the full workspace test suite (`cargo nextest run --workspace`)
- [x] Report test failures to the user before fixing anything
  - **Result: ZERO test failures.** All 6247 tests passed. The roundtrip tests compare ASTs (not strings), so the change was transparent.

### Phase 2: Fix Tests

- [x] Update roundtrip test `span-empty-attributes.qmd` (`[hello]{}` → `[hello]`)
- [x] No other tests were affected (zero failures)
- [x] Add new roundtrip test `span-empty-content.qmd` with `[ ]` as input
- [x] Add new roundtrip test `span-checkbox.qmd` with `[x]` as input
- [x] Run full workspace tests to confirm everything passes — 6247 passed

## Technical Details

### Change Location

**File:** `crates/pampa/src/writers/qmd.rs`, function `write_span` (line ~1494)

### Parser Compatibility

All forms (`[x]`, `[hello]`, `[ ]`, `[]`) parse as Span nodes with empty attributes. No parser changes needed.

### Affected Components

- **`crates/pampa/src/writers/qmd.rs`** — `write_span` function (changed)
- **`span-empty-attributes.qmd`** — updated to canonical form `[hello]`
- **`span-empty-content.qmd`** — new test for `[ ]`
- **`span-checkbox.qmd`** — new test for `[x]`
- **Incremental writer** — no changes needed (delegates to `write_span`)
- **WASM module** — no changes needed (uses the same writer)
- **List-table empty cell syntax** — NOT affected
