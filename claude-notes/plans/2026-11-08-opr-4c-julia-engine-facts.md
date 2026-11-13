---
title: 'OPR-4c: the runner pin moves and the Julia engine carries the capture facts through'
date: 2026-11-08
description: 'Conditional. Moves the pinned QuartoNotebookRunner revision to the one Gordon pushed and shows the three facts reach q2 in the cell metadata of the notebook the engine already returns, with output unchanged for every existing document. Per the OPR-4a spike no new request field and no ts_protocol.rs change are expected; this plan confirms that by running, and adds only what the confirmation shows is missing.'
status: complete
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

**Runner revision pushed by Gordon:** `ffe5dab72066ddd91bad480fc30f861749e305d4` (branch `q2/opr-expression-facts` on `gordonwoodhull/QuartoNotebookRunner.jl`, pushed 2026-11-09; three commits on `5e04284`).

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

- [x] **Confirm first, by running:** with the pin moved, render a two-expression Julia document with
      `execute: {record: true}` through `target/debug/q2` and read the notebook cell metadata that
      `read_engine_notebook` sees (a debug print or a test hook, removed afterwards). Record whether the three keys
      are present and in the spike\'s shapes. This decides whether the next boxes are needed.
- [x] **Engine tests first** (`resources/extension-subtrees/julia-engine/tests/unit/`), only if the confirmation shows
      the engine must change: with units absent, output is byte-identical to today; with units present, the keys are
      passed through per cell.
- [x] **q2 test first** (`engine/ts_record.rs`, beside the `JDOC` tests): a QNR-annotated notebook whose cells carry the
      three keys reads without loss and without error, and one that carries none reads exactly as today.
- [x] Implement what the tests require; in the subtree run `npm ci`, `npm run typecheck`, `npm test`, `npm run build`
      and commit the rebuilt bundle (CI verifies it matches the source).
- [x] Update the vendored registry entry only if `cargo xtask lint` requires it (the subtree-split trailer does not
      move with local commits; confirm the lint stays green).
- [x] **End-to-end** with Julia (the confirmation run, kept): record the invocation and what you saw; also a
      `warning: false` cell, to show `@info` and raw stderr now reach the record (OPR-4b\'s stderr fix). Before OPR-4d the
      keys are not turned into presenter inputs, so rendered HTML must be unchanged.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
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

Taken by lane D (workspace-6) on 2026-11-09 while lane D was blocked, at Gordon\'s instruction; the plan names lane B.

**What the confirmation run showed** (ran, `target/debug/q2`, a two-cell Julia document with `execute: {record: true}` and
`keep-ipynb: true`):

- **The pin bump alone had no effect.** `ensure_environment.jl` decides between `Pkg.instantiate()` and `Pkg.update()`
  with `Pkg.is_manifest_current`, which ignores the `[sources]` table: it returned `true` for a manifest resolved at the
  old revision against both the old and the new Project.toml (ran, Julia 1.12.7). So on any machine with a cached
  environment the old runner stayed. The first render after the bump therefore showed no facts and no stderr streams.
  Fixed here (smallest change): `environment_pins.jl` defines `sources_pins_match`, which compares each `[sources]` `rev`
  with the manifest\'s `repo-rev`, and `ensure_environment.jl` requires it as well. Tests first
  (`tests/unit/environment-pins.test.ts`, 4 tests, failing on the missing helper, then passing). Shown end to end in an
  isolated `HOME` seeded with the old pin and its manifest: the render re-resolved to `ffe5dab` (the manifest then read
  `repo-rev = ffe5dab...`).
- **With the new runner the facts arrive.** Cell one (`x = 1 + 1`, `x`, `y = x * 3`): `expression_starts [0,1,2]`,
  `output_expressions [3]`, `output_anchors [3]`, `output_kinds [null]`. Cell two (`warning: false`; `@info`, a raw
  `println(stderr)`, then `2 + 2`): starts `[0,1,2]`, anchors `[1,2,3]`, kinds `["message", null, null]`, and its outputs are
  `stream:stderr`, `stream:stderr`, `execute_result` (before the bump: only the `execute_result`, so the stderr fix of
  OPR-4b is in effect). **A fourth key, `output_expressions`, is written** and is not in this plan\'s table; q2 does not
  read it.
- **q2 dropped them.** `read_annotation` read only `language`, `role`, `line`, `expansion` and `inline`; the record cell
  metadata had no `quarto_expression_starts` and so on. Fix: a typed `CaptureFacts` on `QuartoAnnotation`
  (`ts_record.rs`) read by `read_capture_facts`, with a shape check that reports a wrong shape the way the other
  malformed keys are reported. Three tests first (read the three keys in the runner\'s shapes, read none as before, a
  malformed key is an error), failing to compile for the missing type, then passing. The field carries
  `#[allow(dead_code)]` with a comment: nothing consumes it until OPR-4d maps it onto the knitr keys.
- **The engine needed no change.** It already passes the runner notebook through (`doc.ipynb` from `keep-ipynb` carried the
  keys), so no engine TypeScript changed, `ts_protocol.rs` is untouched, and the bundle is unchanged (`npm run build`
  produced no diff). `RecordInfo.version` stays 1.

**Output before OPR-4d** (ran): the recorded route\'s HTML body is identical between the old and new runner. The plain
(engine) route differs in one place: the wrapper\'s `data-quarto` attribute embeds the whole `metadata.quarto`, so it now
lists the new keys. Nothing reads `data-quarto` (OPR-0c), and OPR-0b\'s proposed `julia-data-quarto` rule strips the whole
attribute, so the parity corpus is unaffected.

**Gates.** `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean. `cargo nextest run -p quarto-core
--no-fail-fast`: 8119 run, 8118 passed, 1 failed, 36 skipped; the failure is
`revealjs::assemble::tests::vendored_reveal_assets_match_npm_package` (this workspace\'s `node_modules/reveal.js` reset.css
differs from the vendored copy; unrelated files, and it failed the same way before this plan\'s changes). Subtree: `npm run
typecheck` clean, `npm test` 54 passed, `npm run build` no diff. `cargo xtask lint`: one violation, the known
`positron_cache.rs:360` `metadata-as-str` (reported, not fixed). Windows cross-clippy: **not done**; the aws-lc-sys and
wasmtime build scripts do not cross-compile from macOS. WASM build: not run (`ts_record.rs` is not among the files the
protocol lists). Snapshots: none.

**For Gordon (companion, not part of this plan):** push the subtree commits to the engine fork
(`git subtree push`): the pin bump, `environment_pins.jl`, the `ensure_environment.jl` change, the new unit test and the
changelog entries. The re-resolve fix matters beyond this epic: without it, a published engine that bumps its pin leaves
every user with a cached environment on the old runner.
