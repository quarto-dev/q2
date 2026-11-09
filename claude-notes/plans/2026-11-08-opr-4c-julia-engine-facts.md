---
title: 'OPR-4c: the runner pin moves and the Julia engine carries the capture facts through'
date: 2026-11-08
description: 'Conditional. Moves the pinned QuartoNotebookRunner revision to the one Gordon pushed and shows the three facts reach q2 in the cell metadata of the notebook the engine already returns, with output unchanged for every existing document. Per the OPR-4a spike no new request field and no ts_protocol.rs change are expected; this plan confirms that by running, and adds only what the confirmation shows is missing.'
status: conditional
depends-on: [opr-4b, opr-3]
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Evidence:** [OPR-4a spike note](../research/2026-11-09-opr-julia-spike.md), sections "Proposed wire" and "Effect on OPR-4b to OPR-4d".
**Branch:** `opr/4c-julia-engine-facts` · **Lane B** (`workspace-4`) · **Blocks:** OPR-4d.
**Needs:** Node 24, Julia 1.12, and the runner revision Gordon pushed (below).

## Start condition

Start only when this plan\'s `status` is `ready` on the integration branch **and** the line below is filled in by
Gordon. If it is empty, stop and report.

**Runner revision pushed by Gordon:** (empty)

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c opr/4c-julia-engine-facts feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/4c-julia-engine-facts
```

## Goal

Bring the OPR-4b runner into the vendored engine, with no behavior change in q2 yet.

**What the spike established (read, not yet run through q2).** The facts are optional keys in each cell\'s
`metadata.quarto` (`expression_starts`, `output_anchors`, `output_kinds`), written only under `emitUnits`. The engine
already adds `emitUnits` whenever core sends units (`src/julia-engine.ts:~225`, `record-wire.ts:~109`), and
`read_engine_notebook` (`engine/ts_record.rs`) already reads `metadata.quarto` on each code cell. So the expected work
is a pin bump plus proof that the keys survive the engine and arrive in q2; **no new request field and no change to
`ts_protocol.rs`**, and `RecordInfo.version` may stay 1 because the keys are optional. The spike did not render
through q2 with the prototype runner, so the first checklist item below must confirm this before anything else is
built. If the engine or the reader drops or rewrites the keys, fix that here (smallest change) and say so in the notes.

1. Move the runner pin in `resources/extension-subtrees/julia-engine/_extensions/julia-engine/Project.toml` (and any
   lockfile the engine uses) to the revision above.
2. Confirm the three keys reach `read_engine_notebook` untouched; keep them on the cell\'s metadata for OPR-4d.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Confirm first, by running:** with the pin moved, render a two-expression Julia document with
      `execute: {record: true}` through `target/debug/q2` and read the notebook cell metadata that
      `read_engine_notebook` sees (a debug print or a test hook, removed afterwards). Record whether the three keys
      are present and in the spike\'s shapes. This decides whether the next boxes are needed.
- [ ] **Engine tests first** (`resources/extension-subtrees/julia-engine/tests/unit/`), only if the confirmation shows
      the engine must change: with units absent, output is byte-identical to today; with units present, the keys are
      passed through per cell.
- [ ] **q2 test first** (`engine/ts_record.rs`, beside the `JDOC` tests): a QNR-annotated notebook whose cells carry the
      three keys reads without loss and without error, and one that carries none reads exactly as today.
- [ ] Implement what the tests require; in the subtree run `npm ci`, `npm run typecheck`, `npm test`, `npm run build`
      and commit the rebuilt bundle (CI verifies it matches the source).
- [ ] Update the vendored registry entry only if `cargo xtask lint` requires it (the subtree-split trailer does not
      move with local commits; confirm the lint stays green).
- [ ] **End-to-end** with Julia (the confirmation run, kept): record the invocation and what you saw; also a
      `warning: false` cell, to show `@info` and raw stderr now reach the record (OPR-4b\'s stderr fix). Before OPR-4d the
      keys are not turned into presenter inputs, so rendered HTML must be unchanged.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core` (not runnable on the Lane B machine: the aws-lc-sys C build fails; say so
      if it still does); `cargo xtask lint` (it has a known pre-existing `metadata-as-str` violation at
      `positron_cache.rs:360`; report, do not fix).

## Exit criteria

- The pin is at the pushed revision; engine and q2 tests pass; the bundle matches the source.
- With units absent, Julia output is unchanged (OPR-0a tests and the Julia route tests pass).
- A real Julia run shows the three keys in the cell metadata q2 reads (shown in the end-to-end note).

## Companion (Gordon)

Push the subtree commits to the engine fork (`git subtree push`), not part of this plan.

## Notes from the run

(Fill in at landing.)
