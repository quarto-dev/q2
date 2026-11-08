---
title: 'OPR-4d: q2 turns the Julia facts into cell metadata so the presenter uses them'
date: 2026-11-08
description: 'Conditional. Core requests the three facts from the Julia engine and writes them as the same cell-metadata keys knitr uses, so the one per-cell step from OPR-1a reads them: interleaved output, exact prompt, numeric echo and numeric warning and message for Julia. With the fields absent, output is unchanged.'
status: conditional
depends-on: [opr-4c, opr-1b]
lane: A
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/4d-julia-facts-consumed` · **Lane A** (`workspace-3`) · **Blocks:** nothing directly; OPR-7c waits for it to be resolved.
**Needs:** Julia 1.12, R 4.3 with knitr (regression runs), `hub-client` tooling for the WASM gate.

## Start condition

`status: ready` on the integration branch (Gordon flips it after OPR-4c lands). If Gordon cancelled OPR-4b, this
plan is `cancelled`; do not start.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3 && pwd
git switch -c opr/4d-julia-facts-consumed feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/4d-julia-facts-consumed
```

## Goal (decision D4)

Core asks the Julia engine for the facts (the request field OPR-4c added) and, when it gets them, records them on
the unit\'s record cell as `quarto_expression_starts`, `quarto_output_anchors` and `quarto_output_kinds`: the same
keys the knitr adapter writes (`engine/knitr/adapter.rs:~124-146`, derived in `engine/knitr/pieces_map.rs`). The
struct from OPR-1a reads those keys, so Julia cells gain exactly the features whose inputs they now supply. No
engine name is tested anywhere.

Scope follows what Gordon chose after OPR-4a (the plan header of OPR-4b says which facts were built). Build only
the facts that exist on the wire.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Tests first**, failing for the stated reason: a Julia-shaped record with anchors renders interleaved; with
      expression starts, `prompt: true` is exact and numeric `echo: [1, 3]` selects the right expressions; with
      output kinds, numeric `warning` and `message` select correctly; with none of the fields the output is
      byte-identical to today (all Julia parity fixtures, `tests/fixtures/julia-presenter-parity/`).
- [ ] In `engine/ts_record.rs` (`read_engine_notebook`, record building), request the facts when a record is wanted
      and write the metadata keys; do not change how `core_presents` decides (OPR-7a does that).
- [ ] Check a multi-expression cell, a plot cell and a cell with an error (an error output in the middle must not
      shift later anchors).
- [ ] **End-to-end** with Julia and `target/debug/q2`: a document with a two-expression cell, `prompt: true`, and
      `echo: [2]`; read the HTML; compare with the Quarto 1 behavior recorded in the OPR-4a note. Record the
      invocations and what you saw.
- [ ] Add the Julia cases to the figure-and-feature matrix of the OPR-0b corpus if it has landed; do not wait for it.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core`; `cd hub-client && npm run build:wasm` and the hub-client record tests.

## Exit criteria

- A real Julia document shows interleaved output, exact `prompt` and numeric `echo` for the facts that exist, and
  Julia output is byte-identical to before when the engine supplies none.
- No test or code path in `presenter/` selects behavior by engine name.

## Notes from the run

(Fill in at landing.)
