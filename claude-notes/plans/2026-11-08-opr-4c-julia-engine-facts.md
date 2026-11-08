---
title: 'OPR-4c: the Julia engine and wire carry the capture facts, and the runner pin moves'
date: 2026-11-08
description: 'Conditional. Moves the pinned QuartoNotebookRunner revision to the one Gordon pushed, makes the vendored Julia engine request and pass through expression starts, anchors and output kinds as optional per-unit wire fields, and adds the additive q2 wire parsing. q2 behavior is unchanged until OPR-4d consumes the fields.'
status: conditional
depends-on: [opr-4b, opr-3]
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
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

Bring the OPR-4a and OPR-4b results into the vendored engine and the wire, with no behavior change in q2 yet.

1. Move the runner pin in `resources/extension-subtrees/julia-engine/_extensions/julia-engine/Project.toml` (and any
   lockfile the engine uses) to the revision above.
2. In the engine (`src/julia-engine.ts`, `src/record-wire.ts`), when core asks (an optional request field), ask the
   runner for the facts and return them as **optional** per-unit fields in the shape the OPR-4a note proposed,
   versioned per the execution-records epic decision 5 (new wire fields are optional and versioned).
3. In q2, add the optional fields to `engine/ts_protocol.rs` (`UnitResult` and the request options) and parse them.
   Core only **requests** the facts in OPR-4d; here, parsing an absent field must still work.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Engine tests first** (`resources/extension-subtrees/julia-engine/tests/unit/`): with the request field absent,
      output is byte-identical to today; with it present, the fields appear per unit in the agreed shape.
- [ ] **q2 tests first** (`engine/ts_protocol.rs`): the new optional fields round-trip; an engine that sends none
      parses unchanged; older records without them still read.
- [ ] Implement; in the subtree run `npm ci`, `npm run typecheck`, `npm test`, `npm run build` and commit the rebuilt
      bundle (CI verifies it matches the source).
- [ ] Update the vendored registry entry only if `cargo xtask lint` requires it (the subtree-split trailer does not
      move with local commits; confirm the lint stays green).
- [ ] **End-to-end** with Julia: render a two-expression Julia document with `execute: {record: true}` through
      `target/debug/q2`; read the unit data in the returned record (before OPR-4d they are not turned into cell
      metadata). Record the invocation and what you saw.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core`; `cargo xtask lint`.

## Exit criteria

- The pin is at the pushed revision; engine and q2 tests pass; the bundle matches the source.
- With the request field absent, Julia output is unchanged (OPR-0a tests and the Julia route tests pass).
- With it present, a real Julia run returns the facts (shown in the end-to-end note).

## Companion (Gordon)

Push the subtree commits to the engine fork (`git subtree push`), not part of this plan.

## Notes from the run

(Fill in at landing.)
