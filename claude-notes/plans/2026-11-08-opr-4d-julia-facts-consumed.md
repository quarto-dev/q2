---
title: 'OPR-4d: q2 turns the Julia facts into cell metadata so the presenter uses them'
date: 2026-11-08
description: 'Core maps the three facts the Julia runner puts in each cell''s metadata.quarto onto the same cell-metadata keys knitr uses, so the one per-cell step from OPR-1a reads them: interleaved output, exact prompt, numeric echo and numeric warning and message for Julia. With the facts absent, output is unchanged. Julia cells interleave per expression with Julia''s own limits, which is new behavior, not a restoration of Quarto 1.'
status: in-progress
depends-on: [opr-4c, opr-1b]
lane: A
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Evidence:** [OPR-4a spike note](../research/2026-11-09-opr-julia-spike.md).
**Branch:** `opr/4d-julia-facts-consumed` · **Lane A** (`workspace-3`) · **Blocks:** nothing directly; OPR-7c waits for it to be resolved.
**Needs:** Julia 1.12, R 4.3 with knitr (regression runs), `hub-client` tooling for the WASM gate.

## Start condition

`status: ready` on the integration branch, with OPR-4c and OPR-1b resolved. Nothing else gates it: landing OPR-4c is
the trigger, not a manual flip. If Gordon cancelled OPR-4b, this plan is `cancelled`; do not start.

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

Where `read_engine_notebook` / record building (`engine/ts_record.rs`) reads a Julia code cell\'s `metadata.quarto`, copy
the three runner keys onto the record cell\'s metadata under the knitr names, which the struct from OPR-1a already reads
(`presenter/knitr_inputs.rs`):

| Runner key (`metadata.quarto`) | Cell-metadata key |
|---|---|
| `expression_starts` | `quarto_expression_starts` (`EXPRESSION_STARTS_KEY`) |
| `output_anchors` | `quarto_output_anchors` |
| `output_kinds` | `quarto_output_kinds` |

The shapes already agree (the spike note\'s wire table; knitr\'s derivation is in `engine/knitr/pieces_map.rs`). No engine
name is tested anywhere, and `ts_protocol.rs` is not expected to change (OPR-4c confirms the keys arrive). Core sends
no new request: the engine already asks the runner for annotations whenever core sends units.

Scope follows what Gordon chose after OPR-4a (OPR-4b says which facts were built). Map only the keys present.

**What users will see, and what not.** Julia prints only the last expression\'s value, so a Julia cell interleaves
source, output and the final value per expression, but never shows intermediate values as knitr does. Quarto 1 does
not interleave Julia at all (it prints the whole source block, then the outputs), so there is **no Quarto 1 behavior to
match**; the check is against the facts themselves. Raw `println(stderr, ...)` has kind `null`, so numeric `warning`
and `message` selection leaves it alone.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Tests first**, failing for the stated reason: a Julia-shaped record with anchors renders interleaved; with
      expression starts, `prompt: true` is exact and numeric `echo: [1, 3]` selects the right expressions; with output
      kinds, numeric `warning` and `message` select correctly (a raw stderr write is untouched); with none of the keys
      the output is byte-identical to today (all Julia parity fixtures, `tests/fixtures/julia-presenter-parity/`).
- [ ] Map the keys in `engine/ts_record.rs` and write them as cell metadata; do not change how `core_presents`
      decides (OPR-7a does that).
- [ ] Check a multi-expression cell, a plot cell and a cell with an error (the anchor of the throwing expression is
      the end of the code, so later anchors must not shift; there are none after it), and a cell with `#|` options
      (starts are relative to the code after them).
- [ ] **`warning: false` end to end.** With OPR-4b\'s stderr fix the record holds stderr; check the presenter hides
      warnings but not `@info`, and that flipping the option does not need a re-run (this is OPR-2\'s D2 rule applied to
      Julia; if OPR-2 left a Julia gap in its notes, close it here).
- [ ] **End-to-end** with Julia and `target/debug/q2`: a document with a two-expression cell, `prompt: true`, and
      `echo: [2]`; read the HTML and check the facts (interleaving, prompt lines, the selected expression). Record the
      invocations and what you saw.
- [ ] Add the Julia cases to the figure-and-feature matrix of the OPR-0b corpus if it has landed; do not wait for it.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core` (see OPR-4c for the known local limit); `cd hub-client && npm run build:wasm`
      and the hub-client record tests.

## Exit criteria

- A real Julia document shows interleaved output, exact `prompt` and numeric `echo` for the facts that exist, and
  Julia output is byte-identical to before when the runner supplies none.
- No test or code path in `presenter/` selects behavior by engine name.
- The docs plan (OPR-8) can state the Julia limits above as written here.

## Notes from the run

(Fill in at landing.)
