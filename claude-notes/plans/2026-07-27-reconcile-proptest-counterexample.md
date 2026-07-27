# quarto-ast-reconcile: proptest counterexample — reconciliation does not preserve structure (bd-9fwn1504)

**Date:** 2026-07-27
**Braid:** bd-9fwn1504
**Checkout:** worktree for bd-en2hvrwn (branch `main` @ `78d55deb`) — investigation only; the fix should land on its own branch.
**Status:** Design settled (2026-07-27, see "Design decisions") — ready to implement on its own branch.

## Triage verdict

**Ready to implement.** The counterexample is deterministic, the root cause is identified and confirmed with a minimal hand-built repro, the affected sites are enumerated, and the fix shape is decided: option (b), always store the plan, with the apply-side fallback cleanup as part of the same change.

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

## Design decisions (settled with Carlos, 2026-07-27)

### Fix shape: option (b) — always store the plan; delete `needs_plan` at the 4 sites

Options considered: **(a)** keep the optimization behind a strict identity
predicate (`orig.len() == exec.len()` and alignments exactly
`KeepBefore(0..n)` in order); **(b)** always store the computed plan; **(c)**
defensive apply-side fallbacks (rejected outright — wrong layer, loses
source-info preservation).

**Decision: (b).** The initial (a) recommendation implicitly weighed a
compute saving that does not exist. Rationale, from the consumer audit:

- **No compute is saved by dropping plans.** At all 4 sites the nested plan
  is fully computed *before* `needs_plan` decides to discard it
  (compute.rs:509→511, :539→545, :437→440, :466→468). The entire delta of
  (b) is storing/applying the plan, not computing it.
- **Plans are transient.** Computed, applied, dropped within one
  `reconcile()` call; production callers (`engine_execution.rs:477`,
  `apply_node_edit.rs`, wasm client) keep them only for `stats`. Nothing in
  production serializes a plan (the `Serialize` derives are exercised only by
  the dev-only `reconcile-viewer`).
- **The incremental qmd writer is unaffected.** It coarsens only
  `block_alignments` + top-level `inline_plans`
  (`pampa/src/writers/incremental.rs:159-176`) and never reads
  `table_plans`/`custom_node_plans`/`caption_plan`; changed tables are
  already full-block Rewrites. (b) changes only nested-plan storage, so
  writer behavior is identical.
- **No consumer treats "nested plan present" as a change signal.** That
  invariant existed only inside the crate — and it is exactly the invariant
  that turned out to be false.
- **Output equivalence for the identity case.** Applying an all-KeepBefore
  identity plan *moves* the original nodes, exactly like the fallback —
  bit-identical results. (b) changes behavior only in the buggy
  deletion/reorder cases, which is the fix.
- **Cost:** transient plan storage ~200-400 bytes per cell/caption/slot
  (a pathological 10k-cell table → a few MB, per preview keystroke in WASM);
  apply does O(n) pointer moves instead of one wholesale Vec move. Assessed
  as negligible; no benchmark deemed necessary. If measurement ever says
  otherwise, reintroduce the skip as a single shared, tested
  `plan_is_identity(&plan, orig_len)` helper — an optimization layered on
  correct (b) semantics, not four hand-rolled semantic branches.
- **Structural argument:** this bug — and the previous reconcile bug
  (bd-3zp3z4jx) — is a fast-path/general-path divergence. (b) deletes the
  dual path at these sites, so the bug class cannot recur there, and the
  property test exercises the same path production runs.
- **Accepted cost (risk transfer):** the general apply path becomes
  load-bearing for the ubiquitous unchanged case; a latent bug in
  apply-with-identity-plan would now fire on every preview keystroke instead
  of only on edits. Mitigated by the output-equivalence argument and the
  property suite. Plan dumps also get noisier (reconcile-viewer / debugging
  by eye), and any tests asserting plan shape (e.g. `cell_plans.is_empty()`
  for identical tables) will churn.

**Required cleanup (part of the same change, not optional):** with (b), the
four apply-side no-plan fallbacks become unreachable for the
both-sides-present cases — caption `apply.rs:713-719`, cells
`apply.rs:600-612`, custom slots `apply.rs:456-464`. They must be deleted
(or reduced to exec-preferring defensive arms where a genuinely-missing
entry remains possible, e.g. exec-only cells from row/column growth). Their
"no plan means content matched exactly" comments encode the false invariant;
leaving them as live-looking code invites the bug class back through a
future compute site. (b)'s "simpler" claim is only true with this cleanup
done.

### Phase 2 scope: deferred

Tightening table `structural_eq_block` (so the property test covers cells)
is deferred to **bd-fp069xyh**. The eq-tightening has wider blast radius —
eq would become stricter than the hash in places (hash covers only
`ColWidth` discriminants, not values), making hash-equal-but-eq-unequal
pairs possible, and every compute-side `hash match → eq confirm` site needs
review. This fix is provable without it.

### Unit-test matrix: full 4×2

Deletion (original-superset) **and** reorder (identical items, swapped)
variants at each of the 4 sites — 8 small unit tests. Once the
table/custom-node builders exist for the deletion tests, each reorder
variant is a few lines, and under (b) each site's fallback is a distinct
piece of code being deleted, so per-site pins are what catch a future
regression in any one of them. The reorder manifestation (original `[a,b]`,
executed `[b,a]` → old order restored) is guaranteed by inspection but was
not separately run during investigation — the Phase 0 failing-test run
confirms it.

## Phases

- Phase 0 — Test plan (TDD):
  - Commit `proptest-regressions/lib.txt` as the regression pin (fails first).
  - Unit tests, 4 sites × {deletion, reorder} (matrix above).
  - Verify each fails before the fix.
- Phase 1 — Remove `needs_plan` at the 4 compute sites **and** delete/reduce
  the corresponding apply-side fallbacks (required cleanup above). Update
  any plan-shape assertions in existing tests.
- Phase 2 — `cargo nextest run --workspace` + full `cargo xtask verify`
  (reconcile is in the WASM closure).

## Risks / tradeoffs

- The crate is in the WASM closure (`wasm-quarto-hub-client` → hub-client preview), so full `cargo xtask verify` is required, and behavior changes affect the live editor write-back path. The fix *reduces* how often original content is kept verbatim, so the risk is losing source-info preservation in edge cases, not corrupting content.
- Risk transfer under (b): see decision rationale above.
- proptest shrink hit `max_shrink_iters=1024` on the CI seed; the stored case is large. Not a problem for the fix (unit tests are the debugging vehicle; the seed is just the pin).

## Work items

Investigation:

- [x] Reproduce at HEAD with the CI seed (deterministic; only failure in the workspace)
- [x] Localize divergence (table caption.long; debug-twin + source-info-stripping diff)
- [x] Confirm root cause with minimal hand-built repro
- [x] Enumerate affected sites (4) and the eq/hash asymmetry
- [x] File discovered strand for the table structural_eq gap (bd-fp069xyh)
- [x] Design alignment with user (decisions recorded above, 2026-07-27)

Implementation (branch `braid/bd-9fwn1504-quarto-ast-reconcile-proptest`):

- [x] Phase 0: 4×2 unit-test matrix written (`lib.rs` `mod tests`, "Plan-omission
  soundness tests"); all 8 verified failing with the predicted modes
  (deletion resurrects the deleted item with original source info; reorder
  restores original order — including the reorder cases that were previously
  only confirmed by inspection)
- [x] Phase 1a: compute side — `needs_plan` removed at all 4 sites (caption,
  cells, `Slot::Blocks`, `Slot::Inlines`); plans stored unconditionally
- [x] Phase 1b: apply side — false-invariant fallbacks removed: cell no-plan
  branch now exec-wins, caption no-plan branch now exec-wins, custom-slot
  fallback keeps original only for eq-verified single-node slots and
  exec-wins for sequence kinds
- [x] All 8 new tests pass; seeded `reconciliation_preserves_structure_full_ast`
  passes; full crate suite 226/226; property tests pass at
  `PROPTEST_CASES=5000` (no new counterexamples exposed by the
  always-store path)
- [x] Phase 0 (pin): `proptest-regressions/lib.txt` staged for the fix commit
- [x] Phase 2: `cargo nextest run --workspace` — 10508/10508 passed, 0 regressions
- [x] Phase 2: full `cargo xtask verify` (WASM closure) — all steps passed
- [x] Pre-commit review checklist (`claude-notes/instructions/review.md`):
      HashMap greps clean, clippy clean, fmt via hook, TDD fail-first
      verified for all 8 tests, no TODOs added
- [ ] Commit (awaiting approval per review checklist), then merge/PR per
      user's direction

Note on end-to-end verification: this fix is library-internal (the
reconcile step of engine execution and editor write-back). It is verified
by the property suite (incl. the CI seed and a 5000-case run), the 4×2
unit matrix, pampa's node-edit/splice integration tests, quarto-preview's
integration tests, and the full WASM build — no separate `q2 render`
invocation exercises engine-driven caption deletion, because constructing
one requires an engine runtime producing a table-caption edit; the
workspace's existing integration tests are the end-to-end coverage here.
