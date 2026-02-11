# Incremental Writer Trailing Newline Bug

## Overview

The incremental writer panics with "byte index out of bounds" when the original QMD text
doesn't end with `\n`. This happens because the QMD reader internally pads the input with
a trailing `\n` before parsing, producing source spans relative to the padded input. But
`incremental_write_qmd` passes the unpadded original string to `incremental_write`, causing
the spans to be 1 byte out of bounds.

**Trigger**: Changing a kanban card status (todo→done) via the hub-client UI when the synced
document text doesn't end with a newline.

**Root cause**: `crates/pampa/src/readers/qmd.rs:75-88` adds `\n` if missing, shifting source
spans. `crates/wasm-quarto-hub-client/src/lib.rs:2172-2214` passes unpadded `original_qmd` to
`incremental_write` alongside the AST parsed from padded input.

## Work Items

- [x] Write a failing test in `incremental_writer_tests.rs` that uses a document WITHOUT a trailing `\n`
  - Test idempotence: `assert_idempotent("Hello world.")` (no trailing `\n`)
  - Test roundtrip with modification: original without `\n`, new without `\n`
  - Test the JSON-roundtrip path (mimics WASM): `incremental_write_via_json_roundtrip` with no trailing `\n`
  - Test with a kanban-like document without trailing `\n`
  - All 6 tests confirmed to fail with "byte index out of bounds" before fix
- [x] Fix: Normalize `original_qmd` in `incremental_write` and `compute_incremental_edits`
  - Added `ensure_trailing_newline` helper that conditionally pads without allocation in the common case
  - Both public API functions pad input, run normally, then strip the padding from results
  - This is in the library itself (not just the WASM entry point) so ALL callers are protected
- [x] Verify all tests pass: `cargo nextest run --workspace`
  - 84/84 incremental writer tests pass (including 6 new + 2 pre-existing ignored)
  - 6408/6408 workspace tests pass
- [ ] Rebuild WASM and verify in browser (manual verification)

## Details

### Why the existing tests don't catch this

All test documents in `incremental_writer_tests.rs` end with `\n`:
```rust
fn idempotent_single_paragraph() { assert_idempotent("Hello world.\n"); }
```

### Why the kanban demo triggers it

The Automerge sync layer stores document text. When the document text is retrieved
(`changedDoc.text`), it may not end with `\n`. The sync client caches this text as
`cached.source`. When `updateFileAst` calls `incrementalWriteQmd(cached.source, newAst)`,
the WASM function receives a string that may not end with `\n`.

### Fix approach (implemented)

The fix was placed in the library's public API (`incremental_write` and
`compute_incremental_edits` in `incremental.rs`) rather than the WASM entry point.
This protects ALL callers, not just the WASM path.

An `ensure_trailing_newline` helper avoids allocation in the common case (input
already ends with `\n`) and returns `(normalized_str, did_pad)`. When padding was
needed, the trailing `\n` is stripped from the result/edit text to preserve the
original document's trailing-newline convention.
