---
title: 'OPR-3: Julia engine wire, corrected hash declaration and core-presentation acknowledgement'
date: 2026-11-08
description: 'Two small engine-side wire changes in the vendored Julia engine, plus the additive q2 parsing: Julia declares its presentation-only options in its hash definition, and acknowledges that core presented its output, so core can stop deciding by engine name. Local edits in the subtree; pushing them to the fork is Gordon''s companion step.'
status: ready
depends-on: []
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/3-julia-engine-wire` · **Lane B** (`workspace-4`) · **Blocks:** OPR-2, OPR-4c, OPR-7a.
**Needs:** Node 24 (the subtree\'s `package.json` requires `>=24`; `fnm` selects it from `.nvmrc`), `npm ci` in the
subtree, and Julia 1.12 for the end-to-end check.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c opr/3-julia-engine-wire feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/3-julia-engine-wire
```

## Goal (decisions D2, D8)

1. **Corrected declaration.** Julia\'s `JULIA_HASH_DEFINITION`
   (`resources/extension-subtrees/julia-engine/src/record-wire.ts:15-27`) lists every option as hashed
   (`presentationCellOptions: []`, `presentationExecuteKeys: []`). It over-claims: OPR-2\'s rule reads "in the hash"
   as "capture enforced it". Declare the options that only change presentation as display-only: at least `echo`,
   `output`, `include` and `warning` (the Jupyter list is `DISPLAY_ONLY_KEYS`, `engine/jupyter/mod.rs:~251`; use it
   as the reference, and keep `error` hashed). Document in the notes the cost recorded in the execution-records
   epic: QuartoNotebookRunner hashes the whole front matter and any chunk with a span for its own cache, so an
   edit to a display-only key can still make the runner re-run; the record stays fresh in core and the cost is
   time, never stale output.
2. **Acknowledgement.** Core already offers `presentedBy: "core"` in the options it sends when it wants to present
   (`TsExecuteOptions.presented_by`, `engine/ts_protocol.rs:~425`; the engine then skips its own markdown and returns
   the notebook only). Add an optional `presentedBy: "core"` to the engine\'s **returned** record info
   (`TsRecordInfo`, `ts_protocol.rs:~554`, additive, optional, versioned per the execution-records epic decision 5) and
   have the Julia engine set it when it honored the offer. q2 parses it and keeps today\'s name-based decision
   (`core_presents`, `engine/ts_record.rs:500`) in this plan: **no behavior changes in q2**. OPR-7a later switches the
   decision to "present from the record exactly when the engine acknowledged". Order matters: if q2 switched first,
   Julia would regress.

## Checklist (tests first; every box is `[unattended]`)

- [ ] Read the subtree\'s `AGENTS.md` and `README.md`, and `resources/extension-subtrees/README.md` (the subtree flow:
      `cargo xtask pull-extension-subtree julia-engine`). You edit the subtree **in the q2 tree** as ordinary commits
      (not a subtree merge). Do not push; pushing to `gordonwoodhull/quarto-julia-engine` branch `q2/node-port` is
      Gordon\'s companion step (see the epic).
- [ ] **Engine tests first** (`resources/extension-subtrees/julia-engine/tests/unit/`, run with `npm test` there;
      extend `record-wire.test.ts`, which pins the declaration): the declaration lists the display-only options;
      `recordInfo.presentedBy` is `"core"` exactly when the options carried `presentedBy: "core"`, and absent
      otherwise.
- [ ] **q2 tests first** in `crates/quarto-core/src/engine/ts_protocol.rs`: `TsRecordInfo` round-trips the optional
      `presentedBy` and an old engine\'s info (no field) still parses.
- [ ] Implement in the engine (`src/record-wire.ts`, `src/julia-engine.ts`), then `npm run typecheck`, `npm test` and
      `npm run build` in the subtree (the bundle `_extensions/julia-engine/julia-engine.js` must match the source,
      CI verifies it); commit the rebuilt bundle.
- [ ] Implement the additive q2 field and parsing; do not change `core_presents`.
- [ ] Check the declaration reaches q2: render a Julia document with `execute: {record: true}` through
      `target/debug/q2` and read `quarto_engines[].hash_definition` and the new field from the written record
      (`jq`). Record the invocation and output.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core`; `cargo xtask lint` (the vendored-registry rule: the subtree\'s
      registry `version` is the subtree-split trailer, which local commits do not move; confirm the lint stays
      green).

## Exit criteria

- The declaration lists the display-only options and the unit and conformance tests that pin it pass.
- A Julia record written by `q2 render` carries the acknowledgement; a record from an engine that does not
  acknowledge parses unchanged.
- The rebuilt bundle is committed and matches the source; `npm test` and `npm run typecheck` pass in the subtree.
- q2 behavior is unchanged: OPR-0a tests and the Julia route test pass.

## Companion (Gordon, after this epic or whenever he chooses)

Push the subtree commits to the fork and re-pin if needed (`git subtree push`); this plan does neither.

## Notes from the run

(Fill in at landing.)
