# Plan: deterministic diagnostic output (GH issue #222)

## Overview

Diagnosis is captured in `claude-notes/issue-reports/222/triage.md`.

Short version: GLR parses produce multiple `TreeSitterProcessLog` entries
keyed by version number; they're held in
`HashMap<usize, TreeSitterProcessLog>` (`crates/quarto-parse-errors/src/tree_sitter_log.rs:48`)
and iterated with `.values()` in
`produce_diagnostic_messages` (`crates/quarto-parse-errors/src/error_generation.rs:44`).
A `(row, column)` dedupe means whichever version is iterated first
wins. Default `RandomState` makes that order vary per process.

The user clarification on the GH issue: *either diagnostic is
acceptable, as long as the tie is always broken consistently to one of
them.*

## Work Items

Phase 1 — test first (TDD).

- [ ] Write a regression test that parses the issue-222 input N times
      and asserts byte-identical diagnostic output across runs.
      Decide N: 20 is more than enough — at the observed 63/37 split,
      P(all-same) < 1e-4.
- [ ] Place the test where it actually exercises the diagnostic path
      end-to-end. Two candidates:
      - `crates/pampa/tests/` driving `readers::qmd::read`
        (matches the "test the real binary path" guidance in
        `CLAUDE.md` § End-to-end verification);
      - `crates/quarto-parse-errors/` driving
        `produce_diagnostic_messages` directly (smaller, faster).
      Pick pampa-side — that's what the binary uses. A small unit-level
      test in quarto-parse-errors is fine *in addition*, not as a
      replacement.
- [ ] Run the test and confirm it fails on `99e7f89c`.

Phase 2 — fix.

- [ ] Swap `HashMap<usize, TreeSitterProcessLog>` for
      `BTreeMap<usize, TreeSitterProcessLog>` in
      `tree_sitter_log.rs`. Touches the field declaration, the import,
      and the `HashMap::new()` initializer at line 181. Iteration sites
      keep working without modification (BTreeMap implements the same
      `.values()` / `.iter()` API and indexing).
- [ ] Run the regression test, confirm it passes.
- [ ] Run the full pampa test suite (`cargo nextest run -p pampa
      -p quarto-parse-errors`).
- [ ] Run `cargo xtask verify --skip-hub-build` (Rust + tree-sitter
      legs at minimum). If the hub-build leg is needed for CI,
      the maintainer can run the full verify before merge.

Phase 3 — guardrail.

- [ ] Decide whether to add a `CLAUDE.md` note (or a `.claude/rules/`
      rule) about: "containers iterated to produce user-visible output
      must have deterministic iteration order — prefer `BTreeMap` /
      `Vec` over `HashMap` when iteration is observable."
      Discuss with the user before writing.

## Open questions for the user (before implementing)

1. **Tie-break direction.** BTreeMap picks the lowest version key
   (here, GLR version 0 ⇒ Variant A, the "underscore-emphasis" diag).
   Acceptable per the GH clarification, but is there a reason to
   prefer the highest version, or a non-version-based tie-break
   (e.g. lexically earliest, fewest skips)? Default
   recommendation: lowest version. It's the path tree-sitter
   considered first (and Pandoc broadly agrees on this case — it
   parses `_blank` inside the quote as text, not as emphasis open,
   *because* the quote isn't closed, but only Quarto reports both
   problems).

2. **Scope of the regression test.** Is it acceptable to add a test
   that loops N times in-process? Two concerns:
   - Slow tests: at the observed pampa parse cost this is negligible
     (~ms per run), so 20 runs ≈ 20-30 ms. Should be safe.
   - Flakiness: if we fix the root cause, the test is deterministic;
     if the fix regresses, it'll fail every time we run CI. Net win.

3. **Wider HashMap audit.** Should we file a follow-up beads issue to
   audit every `HashMap<*>` iteration in user-facing output paths
   (writers, diagnostic emission, source maps), or treat that as
   out-of-scope for this fix?

Once the user signs off on these, proceed with Phase 1.

## Notes

- `processes` is also iterated by `TreeSitterParseLog::is_good`
  (`tree_sitter_log.rs:72`). That's a boolean reduction (`all` over
  process is_good) — order-independent. The BTreeMap swap is a no-op
  there.
- BTreeMap's performance characteristics differ from HashMap (O(log n)
  vs amortized O(1) lookup), but `processes` is at most a handful of
  entries (one per concurrent GLR version, typically 1-3). Performance
  is irrelevant at this size.
- We retain the `(row, column)` dedupe. Reconsidering it (e.g.
  emitting all distinct GLR interpretations) is a separate question;
  the user has explicitly said "either diagnostic is acceptable" so
  the dedupe stays.
