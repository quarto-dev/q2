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

- [x] Write a regression test that parses the issue-222 input N times
      and asserts byte-identical diagnostic output across runs. Used
      N=50 (P(all-same by chance) < 1e-10 at the observed 63/37 split).
- [x] Place the test in `crates/pampa/tests/`. File:
      `test_diagnostic_determinism.rs`. Drives `readers::qmd::read`,
      so it exercises the same path the `pampa` binary uses.
- [x] Run the test on `99e7f89c` — confirmed fails (the test's
      assertion message shows the Variant A vs Variant B
      divergence directly).

Phase 2 — fix.

- [x] Swap `HashMap<usize, TreeSitterProcessLog>` for
      `hashlink::LinkedHashMap<usize, TreeSitterProcessLog>` in
      `tree_sitter_log.rs`. Done via `use hashlink::LinkedHashMap as
      HashMap;` so the in-file name `HashMap` keeps working at every
      use site. Also had to swap the import in `error_generation.rs`'s
      test module (`use hashlink::LinkedHashMap as HashMap;`) — the
      `processes` field is now typed `LinkedHashMap`, so the test
      builder's `HashMap::new()` literal had to match.
      Added `hashlink = "0.11"` to `crates/quarto-parse-errors/Cargo.toml`.

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
- [x] Run the regression test, confirm it passes. (50/50 runs
      produce identical diagnostic text.)
- [x] Run the full pampa test suite (`cargo nextest run -p pampa
      -p quarto-parse-errors`) — 3792 passed, 2 skipped.
- [x] Run `cargo xtask verify` Rust + tree-sitter legs — green.
      Hub-build + JS legs skipped (pre-existing local
      `wasm-quarto-hub-client` package-not-found state, unrelated to
      issue #222); the maintainer should run the full verify before
      merge to confirm the WASM build.

End-to-end check: `printf -- 'The "_blank" word.' | cargo run --bin
pampa -- --no-prune-errors` was run 30 times against the patched
binary. Result: 30/30 Variant A (Q-2-11 at col 13 + Q-2-5 at col 19).
Diagnostic visually inspected.

Phase 3 — guardrail.

- [x] Decide whether to add a `CLAUDE.md` note (or a `.claude/rules/`
      rule) about: "containers iterated to produce user-visible output
      must have deterministic iteration order — prefer
      `hashlink::LinkedHashMap` / `BTreeMap` / `Vec` over `HashMap`
      when iteration is observable."
      Resolution: rolled into the follow-up audit issue (bd-x5tx2)
      so the rule lands together with the broader audit, rather than
      as a standalone codification step here.

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
   out-of-scope for this fix? **Resolved: filed bd-x5tx2
   (discovered-from bd-hwdlq).**

User signed off on all three (2026-05-21): tie-break direction is
irrelevant as long as it's predictable, test goes in
`crates/pampa/tests`, audit follow-up filed.

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
