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
      `hashlink::LinkedHashMap<usize, TreeSitterProcessLog>` in
      `tree_sitter_log.rs`. Touches the field declaration, the import,
      the `HashMap::new()` initializer at line 181, and a one-line
      addition to `crates/quarto-parse-errors/Cargo.toml`
      (`hashlink = "0.11"`). Iteration sites keep working without
      modification (LinkedHashMap implements `.values()` / `.iter()`
      / indexing the same way).

      Rationale for LinkedHashMap over BTreeMap: it's the data
      structure the rest of the workspace already reaches for when
      iteration order has to be deterministic — 8 crates depend on
      `hashlink` (quarto-pandoc-types, quarto-ast-reconcile,
      quarto-core, comrak-to-pandoc, quarto-citeproc, pampa,
      quarto-highlight, plus the reconcile-viewer experiment).
      `pampa/src/readers/json.rs` uses `LinkedHashMap<String, _>` for
      the same kind of "iteration order matters" reason. Sticking with
      the established pattern is preferred.

      For *this particular bug* either choice gives the same observable
      output: tree-sitter inserts GLR versions in numeric order (0, 1,
      2 on the issue-222 trace), and the lowest version wins under
      both BTreeMap (sorted) and LinkedHashMap (insertion). Both
      experimentally produce 30/30 Variant A. The two would only
      diverge if tree-sitter ever introduced a brand-new high-numbered
      version *before* a lower-numbered one after a condense; nothing
      in the captured trace suggests that happens, but the audit
      should keep an eye out.
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

1. **Tie-break direction.** LinkedHashMap picks the version inserted
   first (here, GLR version 0 ⇒ Variant A, the
   "underscore-emphasis" diag). Acceptable per the GH clarification,
   but is there a reason to prefer the highest version, or a
   non-version-based tie-break (e.g. lexically earliest, fewest
   skips)? Default recommendation: first-inserted (version 0). It's
   the path tree-sitter considered first — and on this input Pandoc
   broadly agrees on the spirit of it (parses `_blank` inside the
   quote as text, not as emphasis open, *because* the quote isn't
   closed); only Quarto reports both problems.

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
  process is_good) — order-independent. The LinkedHashMap swap is a
  no-op there.
- LinkedHashMap keeps amortized O(1) insert/lookup (it's a HashMap
  backed by a doubly-linked list for iteration order); performance
  is a wash. `processes` is at most a handful of entries anyway.
- We retain the `(row, column)` dedupe. Reconsidering it (e.g.
  emitting all distinct GLR interpretations) is a separate question;
  the user has explicitly said "either diagnostic is acceptable" so
  the dedupe stays.
