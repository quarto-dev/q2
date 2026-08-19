# Flaky proptest: reconciliation_preserves_structure_full_ast (bd-205v6)

**Date:** 2026-08-19
**Braid:** bd-205v6
**Branch:** `main` @ `4b4a63ce` (investigated in the main checkout; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The "flaky" proptest failure is a real, deterministic `apply_reconciliation` bug — a recurrence of the bd-3zp3z4jx container-identity bug class for `Cite` — with a confirmed 2-inline minimal repro and a precedent-backed fix shape.

## Issue context

Filed 2026-05-26 by gordon (bug, P2, open): the proptest
`property_tests::reconciliation_preserves_structure_full_ast` in
`quarto-ast-reconcile` failed under `cargo xtask verify` with a persisted seed, but
did not reproduce with fresh seeds — hence "flaky". The strand asked to (1)
regenerate/inspect the seed, (2) decide real-bug vs `structural_eq_blocks`
false-negative, (3) commit the regression seed once resolved.

A 2026-08-19 comment (Carlos) captured a second, deterministic seed
(`cc c8f78493…`, deliberately not committed) noting the shrunk case involves a
`Cite` with nested prefix/suffix inlines.

## Dependency graph

**Empty** — no edges in either direction. No incoming pressure; the context lives
entirely in the description + comment. (The relevant prior art, bd-3zp3z4jx, is
referenced in a code comment rather than a braid edge.)

## What the code looks like today

Reproducible at HEAD (`4b4a63ce`): adding the comment's `cc` line to
`crates/quarto-ast-reconcile/proptest-regressions/lib.txt` fails the proptest
deterministically. Full diagnosis + minimal repro in
`claude-notes/plans/cite-reconciliation-proptest-investigation/` (notes.md,
minimal-repro-test.rs).

**Root cause (confirmed, not a structural_eq false-negative):**

- `compute_inline_alignments` Step 2 (type-based container matching,
  `crates/quarto-ast-reconcile/src/compute.rs` ~654–689) pairs two `Cite`s by
  discriminant alone — the bd-3zp3z4jx identity guard covers
  `Custom`/`Link`/`Image`/`Span` but falls through to `_ => true` for `Cite`.
- `apply_inline_container_reconciliation`'s `Cite` arm
  (`crates/quarto-ast-reconcile/src/apply.rs:346`) reconciles only `content` and
  keeps the original's `citations` wholesale.

Result: when before/after Cites differ in `citations` (id, mode, prefix, suffix)
and sit in a block that recurses (needs one hash-identical sibling inline), the
reconciled AST keeps the stale before-citations. The hash and `structural_eq`
already cover citation fields correctly; only Step 2's guard is missing. The
"flakiness" was just generator rarity — every saved seed fails deterministically.

## Proposed phases (draft)

- **Phase 0 — Test plan (TDD).**
  - Re-add the minimal unit test from the investigation dir
    (`minimal-repro-test.rs`) to `lib.rs`'s test module; verify it fails at HEAD.
  - Add both saved seeds (`cc 8f798bbf…` from the description, `cc c8f78493…`
    from the comment) to `proptest-regressions/lib.txt`; verify both fail.
- **Phase 1 — Fix.** Add a `Cite` identity guard to compute Step 2, mirroring
  bd-3zp3z4jx: pair two Cites only when their `citations` are structurally equal
  (same length; per-citation `id`/`mode` equal, `prefix`/`suffix` via
  `structural_eq_inlines`); otherwise fall through to `UseAfter`. Consider
  factoring the citation comparison shared with `structural_eq_inline`'s Cite arm.
- **Phase 2 — Verify + seed commit.** Unit test + both seeds green; full
  `cargo nextest run --workspace`; commit the regression seeds (strand item 3).
- **Phase 3 — Close out.** Decide whether to file the sibling source-splice
  question (see below) as its own strand; close bd-205v6.

## Open design questions for the user

1. **Fix shape.** I propose the compute-side guard (Option A, consistent with
   bd-3zp3z4jx's "fall through to UseAfter" rule), since the incremental writer
   splices a recursed container's non-child region verbatim from original source —
   copying `e.citations` in apply (Option B) would fix the AST but disagree with
   the spliced source text. Belt-and-braces (do both, as Link does with
   attr/target) is also possible. Guard-only, or guard + apply-side copy?
2. **Sibling audit follow-up.** For `Quoted`/`Insert`/`Delete`/`Highlight`/
   `EditComment`, Step 2 has no identity guard but apply copies the exec side's
   `quote_type`/`attr`, so the AST comes out right — yet the verbatim source
   splice may then emit the *old* delimiters/attr in incremental output. Should I
   file that as a separate strand (source-level, not covered by this proptest),
   and should this fix also add guards for those containers for consistency?
3. **Where to land the fix.** This investigation was done on `main` in the main
   checkout. Fix directly on a topic branch off `main`, or do you want a worktree?

## Risks / tradeoffs (draft)

- Tightening Step 2 turns "recurse + keep stale citations" into `UseAfter`, which
  loses original source-info mapping for a Cite whose citations changed — same
  accepted tradeoff as bd-3zp3z4jx made for Link/Image/Span.
- Committing the regression seeds before the fix would hard-red the workspace;
  they must land in the same commit as (or after) the fix.
- `quarto-ast-reconcile` feeds pampa's incremental writer; any behavior change
  here should get a full-workspace test run (monorepo rule).
