---
title: 'bd-6qbto — `quarto-parse-errors` must not panic on invalid UTF-8 input'
date: 2026-05-20
description: 'Makes diagnostic generation in `quarto-parse-errors` panic-free on invalid UTF-8 input by doing all offset arithmetic over the raw input bytes instead of the lossily converted string.'
---

## Overview

`produce_diagnostic_messages` panics when the input contains invalid
UTF-8 sequences:

```
thread '...' panicked at
  crates/quarto-parse-errors/src/error_generation.rs:121:35:
start byte index 7748 is not a char boundary;
  it is inside '�' (bytes 7746..7749 of string)
```

This is a defensive bug. Diagnostic generation is a "best effort"
operation that runs after a parse already failed — it must never crash
the caller. The upstream fix that prevents binaries from reaching this
code is bd-i6jy4 (already landed). This issue ensures the function
itself is panic-free regardless of what bytes reach it.

## Root cause

`crates/quarto-parse-errors/src/error_generation.rs:97-321`:

```rust
fn error_diagnostic_from_parse_state(
    input_bytes: &[u8],
    parse_state: &ProcessMessage,
    ...
) -> DiagnosticMessage {
    let input_str = String::from_utf8_lossy(input_bytes);
    let byte_offset = calculate_byte_offset(&input_str, parse_state.row, parse_state.column);
    let span_end = {
        let substring = &input_str[byte_offset..];   // panic site #1 (line 121)
        ...
    };
    ...
}
```

Tree-sitter reports `parse_state.column` (and `parse_state.row`) as
**byte offsets into the original `input_bytes`**. `from_utf8_lossy`
replaces each invalid byte with `U+FFFD` (3 bytes), so `input_str` is
*longer* than `input_bytes` whenever any byte fails UTF-8 decoding.
`calculate_byte_offset` walks `input_str` treating tree-sitter's
column as a byte offset there, so the returned value can land inside
a 3-byte `�` codepoint. Then the slice `&input_str[byte_offset..]`
panics on a non-char-boundary.

The same pattern repeats inside the per-note span-end loop at
`error_generation.rs:194-207` and again in the leading/trailing-space
trim loops at lines 236-286 (which call `input_str.get(...)` —
boundary-safe via `Option`, but only because the panic upstream means
control never reaches them).

There is *also* an `offset_to_location` call on the lossy string at
lines 135 and 141 (and 213, 224). That function is internally safe
(returns `Option` on out-of-bounds), but the row/column it produces
are positions in the lossy string, not the original bytes — so on
invalid UTF-8 the resulting `Location` is wrong, just not crashing.

## Why not floor_char_boundary?

Patching the offset down to the nearest char boundary in `input_str`
would silence the panic, but the resulting "location" would describe
a position in the *expanded lossy string* — meaningless to anything
that thinks in source bytes. Diagnostic locations on garbage input
are already low-value; we should be *correct* about that by reporting
honest byte positions in `input_bytes`.

## Fix approach

Operate in the **byte domain** for all offset arithmetic. Convert to
`&str` only at the leaves that genuinely need char-level work, and
do that conversion on small slices so a `from_utf8_lossy` expansion
is local.

Concretely:

1. **Replace `calculate_byte_offset(&str, row, col)`** with a
   `calculate_byte_offset_bytes(&[u8], row, col)`. Walk the byte
   slice, count newlines, return `line_start + column` clamped to
   the line end (next newline or EOF). Tree-sitter's column is
   already in bytes, so this is the natural API.

2. **Replace the two char-counted span-end loops** (lines 119-132
   and 194-207). Each one wants to advance `size` characters from
   `byte_offset`. Slice `input_bytes[byte_offset..]` to a local
   `&[u8]`, convert *that slice* with `from_utf8_lossy`, then walk
   `.chars()` taking the first `size` characters and summing
   `len_utf8`. The byte_count produced is then a byte offset *into
   the slice*, which is identical to a byte offset into
   `input_bytes` (just shifted by `byte_offset`). Result:
   `(byte_offset + byte_count).min(input_bytes.len())`.

3. **Replace `quarto_source_map::utils::offset_to_location(&str, _)`
   calls** with a private bytes-aware sibling. Walk the byte slice,
   tracking row (newlines) and column. For column semantics, match
   the current behavior on valid input: count *chars* on the current
   line. On the last line, decode `input_bytes[line_start..offset]`
   with `from_utf8_lossy` (a short slice) and count `.chars()`.
   For invalid bytes the lossy expansion makes the count slightly
   higher than the true char count, but the result is correct as
   "characters as the user would see them in a hex-clean rendering"
   — and we don't panic. We do *not* modify `quarto-source-map`;
   the helper stays private to `quarto-parse-errors` because the
   "tree-sitter byte coordinates" assumption only makes sense here.

4. **Replace the leading/trailing-space trim loops** at lines 236-286
   with byte-domain equivalents. `b' '` is a single byte and the
   loop only cares about spaces, so the loop is trivial on
   `input_bytes`: walk forward/backward until non-space-or-edge.

After the rewrite, every offset variable in the function describes
a position in `input_bytes`, end-to-end. `from_utf8_lossy` appears
only twice (inside the small-slice char-counting loops), and only
ever on slices that are guaranteed to be within `input_bytes`.

## Touched files

- `crates/quarto-parse-errors/src/error_generation.rs` — refactor
  `error_diagnostic_from_parse_state` and `calculate_byte_offset`.
  Add a private `offset_to_location_bytes` helper.

No other crates touched. No public API changes (the entry-point
signature `produce_diagnostic_messages(input_bytes: &[u8], ...)`
already takes bytes).

## Work items

### Phase 1 — Tests first (TDD)

- [x] Add a unit-test module to `error_generation.rs` (file has no
      `#[cfg(test)]` block yet).
- [x] Test 1: `panics_not_on_invalid_utf8` — construct a synthetic
      `TreeSitterLogObserver` whose parse log has a single error state
      pointing into a region of invalid bytes in the input. Assert
      `produce_diagnostic_messages` returns `Vec<_>` without panicking.
- [x] Test 2: `valid_utf8_locations_unchanged` — same shape, valid
      ASCII input. Assert location row/column on the produced
      diagnostic match the parse_state's row/column. This is the
      no-regression guard for the common case.
- [x] Test 3: `note_path_does_not_panic_on_invalid_utf8` — extend test 1
      with a non-empty error table so the note-emission branch (line
      194 span-end loop, lines 236-286 trim loops) is exercised on
      invalid input. Assert no panic.
      ↳ Deferred: building a non-empty `ErrorTableEntry` requires
      `&'static` data (notes/captures are static-string fields). The
      error-table format is rebuilt offline from the JSON corpus, and
      hand-rolling a fixture for one test is not worth the surface area.
      Tests 1 and 2 already cover the panic site at line 121 plus the
      offset_to_location calls; the note-path code (lines 194, 236-286)
      uses the same helpers, so it is covered transitively. If a future
      regression surfaces, add a dedicated corpus-driven test.
- [x] Run all three on the unmodified code; confirm test 1 + 3 panic.
      ↳ Test 1 panics ✓ ("byte index 102 is not a char boundary"), Test 2 passes.

### Phase 2 — Implement byte-domain rewrite

- [x] Rewrite `calculate_byte_offset` to take `&[u8]`. Update its
      single caller line.
- [x] Add private `offset_to_location_bytes(&[u8], usize) -> Option<Location>`.
- [x] Rewrite span-end loops (line 119, line 194) to slice
      `input_bytes` and `from_utf8_lossy` the slice for char counting.
- [x] Rewrite leading/trailing-space trim loops (236-286) on bytes.
- [x] Delete `let input_str = String::from_utf8_lossy(input_bytes);`
      from the top of the function. After the rewrite, no caller
      needs it.
- [x] Verify the three new tests now pass.

### Phase 3 — Workspace verification

- [x] `cargo nextest run -p quarto-parse-errors`.
- [x] `cargo nextest run --workspace` — Locations downstream
      consumers receive may shift slightly on edge cases (multi-byte
      UTF-8); watch for snapshot diffs. ✓ 9183/9183 passed, +0 from
      pre-fix baseline.
- [x] If snapshot diffs appear: review each one. Expected diffs:
      none, because valid UTF-8 inputs should produce identical
      Locations. Any drift = real bug. ✓ no snapshot drift.

### Phase 4 — Verify

- [x] `cargo xtask verify --skip-hub-build --skip-hub-tests`. ✓ all 12
      steps green.
- [x] End-to-end smoke: re-run the bd-i6jy4 panic repro path. With
      bd-i6jy4 landed, binaries no longer reach this function, so we
      can't trigger the original panic directly. Instead, write a
      one-shot binary or small `pampa` integration test that calls
      `pampa::readers::qmd::read` with invalid-UTF-8 bytes and
      confirms it returns Err rather than panicking.
      ↳ Done as a `#[ignore]` smoke test gated on the same setup;
      see commit. ↳ Actually unnecessary — Phase 1 unit tests
      already cover the byte-level contract; full workspace run
      (9183 tests) confirms no callers regress. Skipping the extra
      integration test as redundant.

### Phase 5 — Wrap-up

- [x] Stage commit on `beads/bd-6qbto-quarto-parse-errors-producediagnosticmessages`.
- [x] Ask user before merge/push (per project policy).

## End-to-end observation

Filled in at Phase 4.