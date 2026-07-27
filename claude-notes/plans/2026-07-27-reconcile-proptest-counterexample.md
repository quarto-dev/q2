# quarto-ast-reconcile: proptest counterexample — reconciliation does not preserve structure (bd-9fwn1504)

**Date:** 2026-07-27
**Braid:** bd-9fwn1504
**Checkout:** worktree for bd-en2hvrwn (branch `main` @ `78d55deb`) — investigation only; the fix should land on its own branch.
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The counterexample is deterministic, the root cause is identified and confirmed with a minimal hand-built repro, and the affected sites are enumerated. What remains is choosing the fix shape (three options below) — a small, well-contained change either way.

## Issue context

Filed 2026-07-24 by Carlos (priority 1, bug). CI on PR #415 hit a failing random case in `property_tests::reconciliation_preserves_structure_full_ast` (`crates/quarto-ast-reconcile/src/lib.rs`, "Result should be structurally equal to after"). Reproduced on both the PR branch and `main` — pre-existing latent bug, unrelated to that PR. Deterministic reproducer: seed file `crates/quarto-ast-reconcile/proptest-regressions/lib.txt` with line `cc 2c379a4ae900cb3d235f771b204e0cb0e307ee6808562d588dc6ecd81088e38e` (present locally, deliberately untracked; the fix commits it as the TDD regression pin).

## Dependency graph

- **discovered-from:** bd-eiku4ymo (capture-docs audit/GC metadata + hub admin tools, `in_progress`, PR #415 open). Purely incidental context — that PR's CI run happened to draw the failing seed. Nothing in the parent constrains this fix; nothing here blocks the parent (the seed file was deliberately kept off that PR).
- No blockers, no dependents otherwise. No incoming pressure beyond "the property test is latently red for anyone who draws this seed."

## What the code looks like today

Verified at `main` @ 78d55deb: `cargo xtask verify --skip-hub-build` is green **except** exactly this test once the seed file is present (7744 passed, 1 failed, fail-fast stopped the rest).

**Root cause (confirmed):** the `needs_plan` optimization in `compute.rs` drops a nested plan when every executed-side alignment is `KeepBefore` and no nested plans exist, inferring "contents identical." But alignments are per-*executed* item: the check misses **extra original items** (deletions) and **reordering** (`KeepBefore` matches by hash at any position). With the plan dropped, apply-side fallbacks use the *original* content wholesale, resurrecting deleted content.

Four sites share the fallacy:

| # | Site | compute (plan dropped) | apply (bad fallback) |
|---|------|------------------------|----------------------|
| 1 | Table caption.long | `compute.rs:507-526` | `apply.rs:713-719` |
| 2 | Table cells | `compute.rs:539-556` | `apply.rs:600-612` |
| 3 | CustomNode `Slot::Blocks` | `compute.rs:435-451` | `apply.rs:456-464` |
| 4 | CustomNode `Slot::Inlines` | `compute.rs:465-479` | `apply.rs:456-464` |

The CI counterexample is site 1: original caption.long had 4 blocks, executed had 1 (which hash-matched original's first) → plan dropped → all 4 original blocks kept. Minimal repro (validated, fails identically): caption.long `[para "a", para "b"]` → `[para "a"]` yields result of length 2.

**Aggravating finding:** `structural_eq_block` for tables is a simplified comparison (`hash.rs:860-865`, only attr/colspec/caption.long), while the table *hash* covers everything. Sites 2-4 are therefore invisible to the property test today, yet are real content-corruption bugs on the preview/write-back path. Filed as bd-fp069xyh (discovered-from this strand).

Full evidence, localization recipe (the proptest "debug twin" trick), preserved repro code, and cleaned dumps: `claude-notes/plans/reconcile-proptest-counterexample-investigation/`.

## Proposed phases (draft)

- Phase 0 — Test plan (TDD):
  - Commit `proptest-regressions/lib.txt` as the regression pin (fails first).
  - Unit tests for each of the 4 sites: original-superset (deletion) case; plus a reorder case (identical items, swapped) for at least caption and one slot type.
  - Verify each fails before the fix.
- Phase 1 — Fix the `needs_plan` predicate (per chosen design option) at all 4 sites.
- Phase 2 — (If agreed) tighten table `structural_eq_block` so the property test covers cells — or defer to bd-fp069xyh.
- Phase 3 — `cargo nextest run --workspace` + `cargo xtask verify` (reconcile is in the WASM closure → full verify).

## Open design questions for the user

1. **Fix shape.** Three options for making plan omission sound:
   - **(a) Strict compute-side predicate:** keep the optimization but only drop the plan when it is provably the identity — `orig.len() == exec.len()` **and** alignments are exactly `KeepBefore(0), KeepBefore(1), …, KeepBefore(n-1)` in order (plus the existing no-nested-plans checks). Minimal diff, preserves the source_info-preservation fast path. My recommendation.
   - **(b) Always store the plan:** delete the `needs_plan` optimization at these 4 sites. Simplest and hardest to get wrong, but grows plan size for the common all-unchanged case (mostly tables in executed documents).
   - **(c) Fix apply-side fallbacks** to be defensive (e.g. fall back to *executed* content when lengths differ). Wrong layer, and loses source-info preservation in cases (a) handles — listed only for completeness.
2. **Scope of Phase 2.** Tightening table `structural_eq_block` makes the property test actually verify cell content, but eq would then be stricter than the hash in places (e.g. hash only covers `ColWidth` discriminants, not values) — hash-equal-but-eq-unequal pairs become possible, and every compute-side `hash match → eq confirm` site needs a look. Do it in this fix, or keep this fix minimal and handle eq-tightening under bd-fp069xyh separately? (My lean: separately — the fix here is provable without it, and the eq change has wider blast radius.)
3. **Reorder regression tests.** The reorder manifestation (original `[a,b]`, executed `[b,a]` → old order restored) is guaranteed by inspection but wasn't separately run during investigation. Include reorder unit tests for all 4 sites, or just caption + one slot type as representative?

## Risks / tradeoffs (draft)

- The crate is in the WASM closure (`wasm-quarto-hub-client` → hub-client preview), so full `cargo xtask verify` is required, and behavior changes affect the live editor write-back path. The fix *reduces* how often original content is kept verbatim, so the risk is losing source-info preservation in edge cases, not corrupting content.
- Option (a)'s in-order requirement is deliberately conservative: a permuted all-KeepBefore alignment will now carry a plan, and apply will follow the alignments (correct order) instead of using original wholesale. That's the desired behavior change.
- proptest shrink hit `max_shrink_iters=1024` on the CI seed; the stored case is large. Not a problem for the fix (unit tests are the debugging vehicle; the seed is just the pin).

## Work items

- [x] Reproduce at HEAD with the CI seed (deterministic; only failure in the workspace)
- [x] Localize divergence (table caption.long; debug-twin + source-info-stripping diff)
- [x] Confirm root cause with minimal hand-built repro
- [x] Enumerate affected sites (4) and the eq/hash asymmetry
- [x] File discovered strand for the table structural_eq gap (bd-fp069xyh)
- [ ] Design alignment with user (questions above)
- [ ] Implement per agreed design (separate branch/session)
