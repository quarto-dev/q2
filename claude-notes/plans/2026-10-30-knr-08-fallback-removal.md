---
title: 'KNR-8: knitr fallback removal (trigger-gated)'
date: 2026-11-02
description: 'Defines the trigger for dropping the finished markdown R ships for structured knitr chunks after the KNR-6 flip, what is deleted and what is kept, and the tests that prove nothing regressed; nothing here starts until the trigger holds, and the trigger carries no date and no corpus claim.'
status: draft  # deferred by design (epic decision 24); do not start early
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (plan KNR-8; decision 24).
**Modeled on:** [EXR-17](2026-10-28-exr-17-legacy-removal.md), the trigger-gated removal plan of
the parent epic.
**Depends on:** [KNR-6](2026-10-30-knr-06-flip-and-consumers.md) (the flip, its retained
fallback and its fallback counter), [KNR-7](2026-10-30-knr-07-docs-e2e.md).
**Blocks:** nothing. It is **not** part of the epic\'s exit criteria.
**Estimated sessions:** 1 to 2, plus whatever time the owner lets pass before the trigger holds.

> **For implementers:** this plan is deliberately small and must not be started until the
> trigger holds. Its job is to make deletion safe, not fast. The owner may leave the fallback in
> place indefinitely; removal is not scheduled.

## Goal

After the KNR-6 flip, R ships the finished markdown of every chunk next to the record so that a
chunk Rust cannot present still renders (epic decision 24). That is a second mechanism kept
alive on purpose. Retire it for structured chunks once there is evidence it is no longer read,
so one mechanism remains and the two-mechanisms failure class (bd-mu9i0bct, bd-42202ipf)
cannot recur.

## Why a trigger and not a gate

The corpus will not run clean in this epic, and it does not capture every use of knitr (custom
hooks, odd graphics devices, packages that emit unusual outputs). A corpus result cannot show
that no real document needs the fallback, so the trigger is about observed use, and the owner
decides when it holds. It carries no release count and no date.

## What is deleted and what is kept

To be confirmed by Phase A against the tree as it stands when this plan starts.

- **Deleted:** the per-chunk finished markdown shipped for *structured* chunks (EXR-11\'s
  `chunks[].markdown` for those chunks) and the per-span fragments shipped for brace spans
  (KNR-6 decision 6), the Rust code that substitutes it
  (`pieces_to_cell` returning `None` then reading the shipped text), and the tests that exercise
  only that substitution.
- **Kept:** the chunk hook and shipped markdown for **fallback units** (`child=`, a cache hit, an
  overridden `source` or `output` hook), because the finished markdown of those exists nowhere
  else; the fallback counter, repurposed to report the new hard-failure case below; the
  skeleton, record and presenter; `ReplayEngine` and the trace.
- **New behavior after deletion:** a structured chunk Rust cannot present becomes a fallback
  unit at record time (KNR-3\'s table gains the row "unmappable output", decided in R by the
  same rules Rust uses, written once and shared as data), or a Q-coded diagnostic with the chunk
  shown as its source. It never renders empty.

## The trigger

All of the following, each checkable. The owner decides when it holds.

1. KNR-6 and KNR-7 have landed, and the fallback counter (KNR-6 Phase 6B) has shipped in a
   release that real users ran.
2. The counter shows no Rust-side fallback for structured chunks across the evidence the owner
   accepts (the project\'s own documents, the docs site, bug reports and any telemetry the
   project has), **or** every remaining reason in the counter has been moved to a recorded
   fallback unit (the "unmappable output" row), so Rust never has to fall back by itself.
3. No open strand reports a document where the retained markdown rescued a chunk.
4. No committed test or fixture needs the shipped markdown for a structured chunk, other than
   fallback units: the inventory in Phase A is empty or each remaining item has an owner.

## Design decisions

1. **No in-place rewrite of old traces or records** (epic decision 4). A stored knitr record
   from before the removal still presents; one that needed the shipped markdown now shows the
   chunk as source with a diagnostic.
2. **Delete per mechanism, in separate commits**, each with its own passing test run.
3. **Never run both mechanisms on one chunk** is an invariant after deletion (trivially true).
4. **Old knitr traces replay unchanged** (KNR-6 decision 2); this plan does not touch them.

## Out of scope

- Removing the chunk hook, `ReplayEngine`, the trace, or the fallback-unit path.
- Anything about EXR-17 (it has its own trigger and is not edited).
- The units-only engines draft; if it lands first, re-read this plan against it.

## Phase A: inventory and tests first

- [ ] Inventory every reader of `chunks[].markdown` and of the Rust-side substitution
  (`crates/quarto-core/src/engine/knitr/`, the legacy capture path, preview and hub writers,
  tests, trace fixtures) and write it into this plan.
- [ ] Re-read the fallback counter\'s output as shipped and record, in this plan, the evidence
  the owner accepted for trigger item 2.
- [ ] Tests first: an unmappable output (gif, out-of-directory figure, `Other` piece) renders
  through the new recorded-fallback row or the source-plus-diagnostic form, never empty; a
  presented chunk never reads shipped markdown; the corpus control still matches.
- [ ] Confirm the KNR-6 record-path tests are green on `main` and record the baseline
  (run count and skips) to compare against afterward.

## Phase B: deletions (one commit each)

- [ ] Add the "unmappable output" row to KNR-3\'s table in the code (R decides, same rules).
- [ ] Stop shipping finished markdown for structured chunks; delete the Rust substitution.
- [ ] Delete the tests that exercised only the substitution, listing them by name.
- [ ] Update `claude-notes/instructions/replay-engine.md` if it describes the shipped markdown.

## Phase C: proof of no regression

- [ ] Workspace run (Gordon decides whether to run it): report pass and skip counts against
  the Phase A baseline and account for every difference by test name.
- [ ] End-to-end: `q2 render` on a document with a gif, an out-of-directory image and an
  overridden hook; inspect the output and record the invocation and the observed snippet.
- [ ] `cargo xtask verify` clean.

## Exit criteria

- No structured chunk reads shipped markdown; `rg` for the deleted substitution returns nothing
  outside history.
- The test-count delta is exactly the deleted tests.
- An unmappable output never renders empty.

## Risks

- **The evidence is weak.** The counter only sees documents that users render with a build
  that has it. The owner may decide the trigger never holds; that is acceptable.
- **R and Rust rules drift** while the row is being written; share the rules as data and test
  both sides against the same table.

## References

- [KNR-6](2026-10-30-knr-06-flip-and-consumers.md), [EXR-17](2026-10-28-exr-17-legacy-removal.md)
- [Implementability review 3](../research/2026-11-02-knitr-epic-implementability-review-3.md),
  findings A1 and A2
