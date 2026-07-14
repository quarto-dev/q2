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

## Open design questions (Carlos)

1. **Nil-permissiveness of Inlines/Blocks constructors.** Pandoc errors
   on `Inlines(nil)`, `Blocks(nil)`, and no-arg `Inlines()`; q2 returns
   an empty list (and has tests pinning that convenience,
   constructors.rs:4896). Options:
   - (a) Keep permissive → behavioral divergence: registry entry +
     the 2 upstream error-contract xfails become permanent
     `# DIVERGENCE` entries (catalog row-12 disposition).
   - (b) Match pandoc: error on nil/no-arg. If the message *contains*
     pandoc's phrase ("Inline, list of Inlines, or string"), both
     upstream tests simply pass — no divergence entry at all.
2. **Q-code granularity.** One `Q-11-3` "Lua marshaling type error"
   with structured detail, vs a small family (constructor-arg /
   property-set / filter-return / read-only-field).

## Checklist (proposed phases)

### Phase 1 — ratchet formalization (no policy dependency)

- [ ] `load_xfail_file` → return entries + divergence flags
      (`# DIVERGENCE` on the entry line or a `DIVERGENCE:` prefix in
      the trailing comment). Shared by Track 1 + Track 2.
- [ ] Unexpected PASS of a DIVERGENCE-marked entry gets its own error
      text ("q2 now matches pandoc here — remove the xfail AND the
      divergences.md entry").
- [ ] Consistency test: every DIVERGENCE-marked xfail id appears in
      `divergences.md` (cheap textual containment check), and vice
      versa entries list their xfail ids.

### Phase 2 — error contract (blocked on Q1/Q2 decisions)

- [ ] TDD: pin the contract shape with tests (unit + upstream xfail
      flips where messages start matching).
- [ ] `marshal_error` helper (expected, got, entry-point context)
      emitting the agreed Q-code(s); message body mirrors hslua's
      `"<expected> expected, got <type>"` so upstream `error_matches`
      patterns pass wherever behavior already matches.
- [ ] Adopt in the four fuzzy peekers + `filter_return_error` first
      (highest-traffic sites), then sweep the remaining
      `Error::runtime` sites incrementally.
- [ ] Filter file:line: verify mlua traceback coverage suffices at the
      filter.rs boundary; if not, attach via DiagnosticMessage at the
      quarto-core error path.

### Phase 3 — bookkeeping

- [ ] Resolve the 2 error-message-contract xfails per Q1 (flip or mark
      `# DIVERGENCE` + registry entry).
- [ ] Update epic plan Phase 3.3/4.1; close strand.
