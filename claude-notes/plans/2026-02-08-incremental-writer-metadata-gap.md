# Fix: Incremental Writer Loses Blank Line Between Front Matter and First Block

## Overview

When the incremental writer rewrites a block (e.g., toggling a checkbox in the todo
demo), the blank line between the YAML front matter `---` and the first block (`::: {#todo}`)
disappears. This is visible in the hub-client Monaco editor after clicking a checkbox in
the React todo demo.

## Root Cause

Two related bugs in `crates/pampa/src/writers/incremental.rs`:

### Bug 1: False metadata inequality (primary)

`metadata_structurally_equal()` (line 374) uses `a == b` on `ConfigValue`, which derives
`PartialEq` including the `source_info` field. The WASM entry point
`incremental_write_qmd()` works as follows:

1. Re-parses `original_qmd` → `original_ast` with **real** source positions in metadata
2. Deserializes `new_ast` from JSON → metadata has **default** source positions (0, 0, 0)

Since `source_info` differs, `PartialEq` returns `false` even though the metadata content
(`title: This document is the storage for a todo app.`) is identical. This incorrectly
triggers the metadata rewrite path.

### Bug 2: Missing separator after metadata rewrite (latent)

When metadata IS rewritten (the `else` branch in `emit_metadata_prefix`, line 214),
`write_metadata_to_string()` produces `---\n<content>\n---\n` — just the front matter
with a single trailing newline. The assembly step doesn't add a separator before the
first block (because `prev_entry` is `None`), so the blank line gap is lost.

Even if Bug 1 is fixed, Bug 2 would cause the same symptom whenever metadata genuinely
changes.

## Data Flow

```
User clicks checkbox → toggleCheckbox() modifies AST clone
  → WASM incremental_write_qmd(original_qmd, new_ast_json)
    → original_ast = parse(original_qmd)     // has real source_info
    → new_ast = json_read(new_ast_json)       // has default source_info
    → metadata_structurally_equal(orig.meta, new.meta)  // FALSE! (source_info differs)
    → rewrite metadata: "---\ntitle: ...\n---\n"        // no blank line gap
    → append rewritten Div directly                      // no separator added
  → result: "---\ntitle: ...\n---\n::: {#todo}\n..."    // MISSING BLANK LINE
```

## Fix Plan

### Phase 1: Write failing tests

- [x] Add test: idempotence with front matter + blank line + div (should be byte-identical)
- [x] Add test: metadata unchanged but blocks changed — blank line preserved
- [x] Add test: metadata actually changed — blank line separator still present

### Phase 2: Fix Bug 1 — metadata comparison

- [x] Replace `metadata_structurally_equal()` with a comparison that ignores `source_info`
      and `merge_op`. Used option (a): custom `config_value_content_eq()` recursive comparator.
  - Added `config_value_content_eq()`, `config_value_kind_content_eq()`, and
    `config_map_entry_content_eq()` in `incremental.rs`
  - Uses `structural_eq_inlines`/`structural_eq_blocks` from `quarto_ast_reconcile` for
    PandocInlines/PandocBlocks cases
  - Added `structural_eq_inlines` to `quarto_ast_reconcile` re-exports

### Phase 3: Fix Bug 2 — separator after metadata rewrite

- [x] In `emit_metadata_prefix`, when metadata is rewritten, also compute and append the
      appropriate separator between the closing `---` and the first block.
  - Added `find_metadata_trailing_gap()` helper that extracts the gap between the
    closing `---\n` and the first block start from the original document
  - Applied in the rewrite branch of `emit_metadata_prefix`

### Phase 4: Verify

- [x] Run all tests: `cargo nextest run --workspace` — 6250 tests pass, 0 failures
- [ ] Reproduce in browser: toggle checkbox, verify blank line preserved in hub-client editor
- [ ] Verify bullet style inside div is still `*` (expected: rewrite uses canonical writer)

## Key Files

| File | Relevance |
|------|-----------|
| `crates/pampa/src/writers/incremental.rs` | Bug location (lines 211, 214, 370-376) |
| `crates/quarto-pandoc-types/src/config_value.rs` | `ConfigValue` with `derive(PartialEq)` including `source_info` |
| `crates/quarto-source-map/src/source_info.rs` | `SourceInfo` with `derive(PartialEq)` |
| `crates/wasm-quarto-hub-client/src/lib.rs` | WASM entry point (lines 2157-2239) |
| `crates/pampa/src/writers/qmd.rs` | `write_metadata()` function |

## Reproduction

1. Open https://cscheid.net/static/quarto-hub/#/project/fb93f22b-b272-4f4b-add6-6192eae33b08/file/todo.qmd
2. Open http://localhost:5173/
3. Ensure there's a blank line between `---` and `::: {#todo}` in the Monaco editor
4. Click any checkbox in the React todo demo
5. Observe: the blank line disappears in the Monaco editor
