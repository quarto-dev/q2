---
title: 'OPR-3: Julia engine wire, corrected hash declaration and core-presentation acknowledgement'
date: 2026-11-08
description: 'Two small engine-side wire changes in the vendored Julia engine, plus the additive q2 parsing: Julia declares its presentation-only options in its hash definition, and acknowledges that core presented its output, so core can stop deciding by engine name. Local edits in the subtree; pushing them to the fork is Gordon''s companion step.'
status: complete
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

- [x] Read the subtree\'s `AGENTS.md` and `README.md`, and `resources/extension-subtrees/README.md` (the subtree flow:
      `cargo xtask pull-extension-subtree julia-engine`). You edit the subtree **in the q2 tree** as ordinary commits
      (not a subtree merge). Do not push; pushing to `gordonwoodhull/quarto-julia-engine` branch `q2/node-port` is
      Gordon\'s companion step (see the epic).
- [x] **Engine tests first** (`resources/extension-subtrees/julia-engine/tests/unit/`, run with `npm test` there;
      extend `record-wire.test.ts`, which pins the declaration): the declaration lists the display-only options;
      `recordInfo.presentedBy` is `"core"` exactly when the options carried `presentedBy: "core"`, and absent
      otherwise.
- [x] **q2 tests first** in `crates/quarto-core/src/engine/ts_protocol.rs`: `TsRecordInfo` round-trips the optional
      `presentedBy` and an old engine\'s info (no field) still parses.
- [x] Implement in the engine (`src/record-wire.ts`, `src/julia-engine.ts`), then `npm run typecheck`, `npm test` and
      `npm run build` in the subtree (the bundle `_extensions/julia-engine/julia-engine.js` must match the source,
      CI verifies it); commit the rebuilt bundle.
- [x] Implement the additive q2 field and parsing; do not change `core_presents`.
- [x] Check the declaration reaches q2: render a Julia document with `execute: {record: true}` through
      `target/debug/q2` and read `quarto_engines[].hash_definition` and the new field from the written record
      (`jq`). Record the invocation and output.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
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

**What landed** (subtree edited in the q2 tree as ordinary commits; nothing pushed):

- `record-wire.ts`: `JULIA_HASH_DEFINITION.presentationCellOptions` and `presentationExecuteKeys` now list the
  Jupyter engine\'s `DISPLAY_ONLY_KEYS` (40 keys, copied literally; `error` stays hashed, as do `eval`, `cache` and the
  figure-size keys). `buildRecordResult` takes the run options as an optional third argument and adds
  `recordInfo.presentedBy: "core"` exactly when `coreWillPresent(options)` is true; `julia-engine.ts` passes them.
  The rebuilt `julia-engine.js` bundle is committed (its diff is only this change).
- q2: `TsRecordInfo.presented_by: Option<TsPresentedBy>` (`ts_protocol.rs`), additive, skipped when `None`, parsed
  and otherwise unused. `core_presents` is unchanged. `ts_wire_parity.json` gained two `presentedBy` lines (the
  fixture sets it so a rename shows); the TypeScript mirror (`ts-packages/quarto-engine-host-node` `types.ts` and
  the key list in `wire-parity.test.ts`) follows. Snapshot and golden files: 1 changed (`ts_wire_parity.json`, +2
  lines), none added or removed.
- QuartoNotebookRunner caveat (as planned): it hashes the whole front matter and any chunk with a span for its own
  cache, so editing a display-only key can still make the runner re-run. The record stays fresh in core; the cost is
  time, never stale output.

**What I ran:** `npm test` (50 pass) and `npm run typecheck` in the subtree; vitest `wire-parity.test.ts` (9 pass) and
`tsc --noEmit` in `quarto-engine-host-node`; `cargo clippy -p quarto-core --all-targets -- -D warnings` (clean);
`cargo nextest run -p quarto-core` (8042 passed, 0 failed, 36 skipped; the Julia rows `j1` to `j8` ran and passed, including `j8_record_row_core_presents_the_output`).

**End-to-end check (ran).** `HOME=<scratch> JULIA_DEPOT_PATH=~/.julia QUARTO_JULIA_PROJECT=<copy of the Julia project>
target/debug/q2 render doc.qmd` on a one-cell Julia document with `execute: {record: true}` inside a project.
`.quarto/execution-records/doc.ipynb`, `metadata.quarto_engines[0].hash_definition` (camelCase keys) carries
`presentationCellOptions` and `presentationExecuteKeys` with the 40 keys (`echo`, `output`, `warning`, `include` present,
`error` absent) and `documentKeys: ["julia","format","fig-width","fig-height","fig-format","fig-dpi"]`. I inspected
the output. The **acknowledgement is not in the record** (q2 parses it and drops it; the plan keeps behavior
unchanged), so it is verified by the unit tests on both sides only, plus the rendered HTML coming through core\'s
presenter (`class="cell"`, `cell-output-display`), which shows core made the offer.

**Findings.**

- The Julia e2e tests (`julia_engine_e2e`, for example `j1_minimal_julia_render`) fail here with `ArgumentError:
  startpath must be non-empty` from QuartoNotebookRunner `evaluation.jl:213` (`relpath(f.path, pwd())` falls back to `""`).
  Cause: several long-lived detached runner servers (some days old) sit behind the shared transport file; the one
  clients reach has a working directory that no longer exists. The failure is in the runner before any record code runs
  (j1 sends no units). I did not kill those servers (they may belong to other lanes or to Gordon). An isolated `HOME`
  gives a fresh server and the render works, so I used that for the check above. The failure was transient: the
  full `quarto-core` run an hour later passed `j1` (the stale server had gone). This is also relevant to RFU-5 (the
  Julia e2e flake): a stale shared server is a failure mode distinct from the EPIPE one.
- `cargo xtask lint` reports one violation I did not cause: `[metadata-as-str]` at
  `crates/quarto-core/src/positron_cache.rs:360`. The vendored-registry rule is green.
- The Windows cross-clippy (`--target x86_64-pc-windows-msvc`) cannot run on this machine: `aws-lc-sys` compiles C for
  the target and fails. The change has no platform-specific code, but a Windows check is not done.

