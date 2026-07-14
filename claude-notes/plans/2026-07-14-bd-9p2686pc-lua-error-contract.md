# Lua marshaling error contract + divergence registry (bd-9p2686pc)

**Strand**: bd-9p2686pc (Lua parity H). **Epic**: bd-grkrb9nj
(`claude-notes/plans/2026-07-13-lua-api-pandoc-parity.md`, Phase 3.3 / 4.1).
**Status**: investigation complete 2026-07-14; implementation not started.
Catalog row 12 of
`claude-notes/research/2026-07-13-lua-api-mismatch-catalog.md`.

## Overview

Coercion rejections in the Lua marshaling layer are bare mlua runtime
strings today (94 `Error::runtime` sites across `types.rs`,
`constructors.rs`, `list.rs`, `filter.rs` in `crates/pampa/src/lua/`).
This strand defines the error contract — entry point (constructor /
property / filter function), expected-vs-got, filter file:line, Q-code —
and formalizes the divergence registry + `# DIVERGENCE` ratchet marker
that bd-d4wd6r3i seeded.

## What already exists (build on, don't duplicate)

- **Registry file**: `crates/pampa/tests/lua-conformance/divergences.md`
  (seeded by bd-d4wd6r3i with the SimpleTable/Q-11-2 entry).
- **`# DIVERGENCE` xfail marker**: today a comment convention in
  `xfail.txt`; `load_xfail_file` (lua_conformance.rs:153) treats `#` as
  a comment and is shared by both ratchets.
- **Q-code plumbing**: `DiagnosticMessageBuilder::…​.with_code("Q-11-1")`
  pattern in `crates/pampa/src/lua/diagnostics.rs` (quarto.warn/error).
  Q-11 is the lua subsystem; Q-11-2 taken (SimpleTable); next free
  Q-11-3.
- **A half-contract already shipped**: `filter_return_error`
  (filter.rs) names the filter function + got-type and defers Q-coding
  to this strand in a comment.
- **The oracle's contract shape** (probed vs pandoc 3.9.0.2): hslua
  errors read `"<expected> expected, got <type>"` followed by context
  frames `while retrieving function argument <arg>` / `while retrieving
  arguments for function <fn>`.

## Design decisions (Carlos, 2026-07-14)

1. **Inlines/Blocks constructors error on nil/no-arg**, matching
   pandoc. Rationale: the permissive empty-list reading is ambiguous —
   `nil` could mean "no change to the filtered element" while `{}`
   means "remove it"; silently picking one hides bugs. Consequence:
   the 2 upstream error-contract tests should FLIP to passing (our
   message must contain pandoc's phrase), not become divergences.
   q2 tests pinning the old `pandoc.Inlines()` → empty-list
   convenience get updated.
2. **Granular Q-codes**, one per error family — errors must state what
   went wrong and what would fix it; a catch-all "Lua marshaling
   error" says nothing actionable. Allocation:
   - **Q-11-3** Invalid Lua constructor/function argument
     (expected-vs-got + which argument of which function).
   - **Q-11-4** Invalid Lua filter return value (names the filter
     function + got-type; upgrades `filter_return_error`).
   - **Q-11-5** Invalid element property assignment (setter type
     errors; read-only field writes).

## Checklist

### Phase 1 — ratchet formalization — DONE 2026-07-14

- [x] `parse_xfail`/`load_xfail_file` → `XfailList` with divergence
      flags (trailing comment starting `DIVERGENCE`). Shared by
      Track 1 + Track 2 (lua_differential imports it).
- [x] Unexpected PASS of a DIVERGENCE-marked entry gets its own error
      text ("q2 now matches pandoc here — remove the xfail AND the
      divergences.md entry"), in both ratchets.
- [x] Consistency test `divergence_xfails_are_registered`: every
      DIVERGENCE-marked xfail id (both files) appears literally in
      `divergences.md`; the checker's failure path is unit-tested
      with synthetic input (`unregistered_divergences`).

### Phase 2 — error contract — DONE 2026-07-14 (core adoption)

- [x] TDD: 7 pinning tests written first and observed red
      (2 unit constructor-nil tests — replacing the two that pinned
      the old empty-list convenience, per Decision 1 — plus
      integration tests for Q-11-4/Q-11-5 and the no-double-got
      message shape).
- [x] Q-11-3/4/5 allocated in quarto-error-catalog.
- [x] `type_mismatch_error(expected, got)` + `lua_facing_type_name`
      in types.rs; hslua's `"<expected> expected, got <type>"` shape.
- [x] Adopted in the four fuzzy peekers (terminal branches),
      `filter_return_error` (Q-11-4, deduplicates the inner Q-11-3
      detail), and both element `__newindex` fallbacks (Q-11-5:
      read-only tag, unknown field).
- [x] `pandoc.Inlines`/`pandoc.Blocks` error on nil/no-arg like
      pandoc (Decision 1) → the 2 upstream error-contract tests
      FLIPPED (Track-1: 184 pass / 19 xfail); lua-types stubs updated.
- [x] Filter file:line: mlua traceback at the filter boundary carries
      it (verified in e2e output).

### Phase 3 — remaining rollout (follow-up strand)

- [ ] Sweep the remaining ~85 `Error::runtime` sites (table-part
      setters "cannot set field on X", eager property validators,
      Citation/ListAttributes/Meta peekers) onto Q-11-3/5.
      Tracked on a discovered-from strand; see epic Phase 3.3.
