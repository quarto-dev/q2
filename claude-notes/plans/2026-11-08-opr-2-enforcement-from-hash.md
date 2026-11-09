---
title: 'OPR-2: who enforces warning and message, derived from the hash definition'
date: 2026-11-08
description: 'One rule replaces knitr_visibility and CellVisibility::resolve: the presenter enforces warning and message exactly when the record declares them display-only for the cell engine; echo, output and include stay always presenter-enforced; an engine with no usable declaration, and an imported record, get presenter enforcement. Includes an audit of what each engine really enforces at capture.'
status: in-progress
depends-on: [opr-1b, opr-3]
lane: A
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/2-enforcement-from-hash` · **Lane A** (`workspace-3`) · **Blocks:** OPR-7c.
**Needs:** R 4.3 with knitr and Julia 1.12 for the audit runs (the audit boxes say what to do when absent).

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3 && pwd
git switch -c opr/2-enforcement-from-hash feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/2-enforcement-from-hash
```

## Goal (decision D2)

Today two functions decide who hides a cell\'s warnings: `CellVisibility::resolve` (`presenter/visibility.rs:~87`) filters
stderr for Jupyter, and `knitr_visibility` (`visibility.rs:~113`) deliberately does not read `warning` ("enforced by
R at capture"). That is why spike S2 saw one golden differ (`warning-false-stderr`) once a Jupyter record took the
knitr entry path. After OPR-1a and OPR-1b there is one path; this plan gives it one rule.

**The rule.**
1. For `warning` and `message`: the presenter enforces the option **exactly when** the record\'s hash definition for
   the cell\'s engine (`quarto_engines[].hash_definition`) lists it as display-only. When the option is in the hash,
   capture enforced it and the presenter leaves the output alone.
2. `echo`, `output` and `include` stay **always presenter-enforced** (hiding what capture already hid is harmless,
   and knitr records the outputs, so the presenter must hide them).
3. An engine with **no usable declaration** (a TS engine that sends none gets `HashDefinition::conservative`,
   `engine/ts_protocol.rs:~587`, which lists nothing as display-only) and an **imported Positron record** (no engine
   entries) get **presenter enforcement** of everything. The unsafe failure (showing what the author hid) must not
   be the default.

**Why not "in the hash means capture enforced it".** A hash is conservative: listing too much is safe (extra
re-runs), so "in the hash" means "might matter", not "capture handled it". Julia\'s declaration, for instance,
lists every option as hashed while its capture enforces almost nothing; OPR-3 corrects that declaration, which is why
this plan depends on it.

## Facts (read unless marked)

- Jupyter: `DISPLAY_ONLY_KEYS` (`engine/jupyter/mod.rs:~251`) lists `echo`, `output`, `warning`, `include`;
  `message` and `error` are not listed (`error` changes whether a failure aborts; Jupyter has no `message` option).
- Knitr: the structured profile\'s presentation list (`engine/knitr/hash_rule.rs`) contains `echo`, `results`,
  `collapse`, `prompt`, `comment`, ... and never `include`, `warning`, `message` or `output` (a test asserts it), so
  those four are hashed and `option_sweep.rs` marks them `Hashed`. (relied, one agent\'s run, 2026-11-08) R records
  no warning or message output under `warning: false` or `message: false`, but records outputs for `include: false`
  and `output: false`, which the presenter hides.
- Julia: `JULIA_HASH_DEFINITION` (`resources/extension-subtrees/julia-engine/src/record-wire.ts:15-27`) has empty
  `presentationCellOptions` and `presentationExecuteKeys`; (relied) capture enforces only a cell\'s own `#|` `warning`. OPR-4a confirmed this by running the runner: under `warning: false` it drops **all** stderr at capture (`@warn`, `@info` and raw stderr), so with OPR-3\'s display-only declaration a Julia record taken under `warning: false` holds nothing for the presenter to show. Not fixed here: the fix is in OPR-4b (runner keeps stderr under `emitUnits`). Until it lands the audit should record this Julia gap, and D2 holds for Julia only after OPR-4b.
- (relied) Marimo and the TS fixture engines declare `[]`. Marimo uses the legacy capture splice and is a documented
  exception to this epic (decision D12): record in the notes what its path does and do not change it.
- Knitr\'s `execute.R` sets `message = isTRUE(format$execute[["warning"]])`, the same as Quarto 1\'s line 320
  (checked, nothing to fix).

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Audit, by running.** For Jupyter (stored notebook or kernel if available), Julia and knitr: for each of
      `warning`, `message`, `error`, `include`, `output`, `echo`, with the option on a cell and at document level,
      record whether capture or the presenter hid the output (look in the record, then in the HTML). Report any
      option whose declaration and behavior disagree. Do not fix a capture gap in this plan: record it in the notes
      and in a section "Findings for Gordon"; the rule below already makes the presenter safe.
- [ ] **Tests first**, each failing for the stated reason: for every engine and every visibility option, the option is
      enforced exactly once overall (capture or presenter) on a fixture; `warning-false-stderr` passes through the
      one path; a cell with an undeclared engine and an imported record are presenter-enforced; a knitr cell with
      `warning: false` is not filtered twice; `message: false` on a Jupyter cell does what it does today (nothing).
- [ ] Implement the rule; delete `knitr_visibility` and `CellVisibility::resolve` (or reduce to the one rule).
- [ ] Update the sweeps: `presenter/jupyter_option_sweep.rs` (a display-only option that changes nothing fails the
      build) and the knitr `option_sweep.rs` `Hashed` rows still agree with the rule.
- [ ] Reference `bd-nn2fou8h` (execute-visibility options ignored; in progress) in the notes; do not change its status.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core`; `cd hub-client && npm run build:wasm` and the hub-client record tests.

## Exit criteria

- One enforcement rule; the two old functions are gone.
- The audit table is in "Notes from the run" with a line per engine and option, marked ran or read.
- `warning-false-stderr` passes on the one path; all other goldens are byte-identical; OPR-0a tests pass.
- Findings for capture gaps (if any) are written up for Gordon; none was papered over.

## Notes from the run

(Fill in at landing, including "Findings for Gordon".)
