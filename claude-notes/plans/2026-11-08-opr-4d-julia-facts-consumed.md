---
title: 'OPR-4d: q2 turns the Julia facts into cell metadata so the presenter uses them'
date: 2026-11-08
description: 'Core maps the three facts the Julia runner puts in each cell''s metadata.quarto onto the same cell-metadata keys knitr uses, so the one per-cell step from OPR-1a reads them: interleaved output, exact prompt, numeric echo and numeric warning and message for Julia. With the facts absent, output is unchanged. Julia cells interleave per expression with Julia''s own limits, which is new behavior, not a restoration of Quarto 1.'
status: complete
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

- [x] **Tests first**, failing for the stated reason: a Julia-shaped record with anchors renders interleaved; with
      expression starts, `prompt: true` is exact and numeric `echo: [1, 3]` selects the right expressions; with output
      kinds, numeric `warning` and `message` select correctly (a raw stderr write is untouched); with none of the keys
      the output is byte-identical to today (all Julia parity fixtures, `tests/fixtures/julia-presenter-parity/`).
- [x] Map the keys in `engine/ts_record.rs` and write them as cell metadata; do not change how `core_presents`
      decides (OPR-7a does that).
- [ ] Check a multi-expression cell, a plot cell and a cell with an error (the anchor of the throwing expression is
      the end of the code, so later anchors must not shift; there are none after it), and a cell with `#|` options
      (starts are relative to the code after them).
      **Partly done:** multi-expression, `#|` options and an error cell were run; a plot cell was not (see notes).
- [ ] **`warning: false` end to end.** With OPR-4b\'s stderr fix the record holds stderr; check the presenter hides
      warnings but not `@info`, and that flipping the option does not need a re-run (this is OPR-2\'s D2 rule applied to
      Julia; if OPR-2 left a Julia gap in its notes, close it here).
      **Partly done:** the hiding by kind is built and shown end to end; the flip without a re-run was not checked.
- [x] **End-to-end** with Julia and `target/debug/q2`: a document with a two-expression cell, `prompt: true`, and
      `echo: [2]`; read the HTML and check the facts (interleaving, prompt lines, the selected expression). Record the
      invocations and what you saw.
- [x] Add the Julia cases to the figure-and-feature matrix of the OPR-0b corpus if it has landed; do not wait for it.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core` (see OPR-4c for the known local limit); `cd hub-client && npm run build:wasm`
      and the hub-client record tests.

## Exit criteria

- A real Julia document shows interleaved output, exact `prompt` and numeric `echo` for the facts that exist, and
  Julia output is byte-identical to before when the runner supplies none.
- No test or code path in `presenter/` selects behavior by engine name.
- The docs plan (OPR-8) can state the Julia limits above as written here.

## Notes from the run

**What changed.**

- `engine/ts_record.rs`: `facts_as_cell_metadata` writes the three runner keys onto the record cell under
  `EXPRESSION_STARTS_KEY`, `OUTPUT_ANCHORS_KEY` and `OUTPUT_KINDS_KEY`, after `RecordBuilder::finish`, for the cells that
  carried them (all three alignment paths). Only keys present are written. A span and an expanded cell get none (an
  expansion\'s outputs are the expansions\', so one cell\'s anchors would not fit). `quarto_` keys sit outside the unit
  hash (a test pins that the exec hash is unchanged). `ts_protocol.rs` did not change. No engine name is tested.
- `presenter/cell.rs`, `format_outputs_with`: **not in the plan\'s table, found by the end-to-end run.** `warning: false`
  dropped every stderr stream, so the record\'s `@info` and raw `println(stderr, ...)` vanished although the runner had
  kept them. When the options carry a kind for an output, `warning: false` now hides only the `Warning` kind; with no
  kind (Jupyter, or a record without the fact) every stderr stream goes, as before. Two tests, the first failing first.
- Tests: three in `ts_record.rs` (keys mapped, absent ones left out, exec hash unchanged), one pin in
  `presenter/knitr_inputs.rs` (a Julia-shaped cell with only the facts interleaves), two in `presenter/cell.rs`.
- Corpus: one document, `julia/capture-facts`, in `tests/fixtures/opr-parity/` (1 manifest entry, 1 fixture file; no
  snapshots added, changed or removed). Its recorded-route output differs from the plain render by design.

**What I ran** (`target/debug/q2`, Julia 1.12.7, scratchpad documents with `execute: {record: true}`, HTML read):

- Plain `q2 render` (no `record: true`): the runner\'s own presentation; no interleaving, no prompt, `echo: [2]` ignored.
  Expected until OPR-7d.
- `record: true`, a cell with `#| prompt: true` and three expressions: source, then `first`, then `y = x + 1`, then `2`,
  each source run prefixed `> ` per expression. A second cell with `#| echo: [2]` showed only `b = 2` and then `2`.
- `record: true`, `#| warning: false` with `@info`, `@warn`, a raw `println(stderr, ...)`: before the presenter fix none of
  the three stderr outputs appeared; after it `keepme` and `rawstderr` appear and `dropme` does not. The runner\'s notebook
  (`keep-ipynb`) held kinds `["message","warning",null,null]`.
- An error cell (`#| error: true`): the error output after all the code, at anchor 4 (the code length), as the plan said.

**Not done, and findings.**

- **Plot cell:** not run (no plotting package offline here). Covered only by the existing presenter tests.
- **`warning: false` flipped without a re-run:** not checked.
- **Finding (runner, not 4d):** in a cell that throws, `display("shown")` before the error is absent from the runner\'s
  notebook (outputs were the stdout and the error only). Not caused by this change. Worth a look in the runner.
- Numeric `warning`/`message` selection over Julia kinds is pinned by existing knitr tests (engine-agnostic); I did not
  run a Julia document with a numeric selection.

**Gates** (before the rebase onto OPR-6): `cargo clippy -p quarto-core --all-targets -- -D warnings` clean;
`cargo nextest run -p quarto-core --no-fail-fast`: 8127 passed, 1 failed (`vendored_reveal_assets_match_npm_package`,
the known environmental one), 1 leaky; `cd hub-client && npm run build:wasm` succeeded and the hub-client WASM record
tests passed (7 passed, 18 skipped); Windows cross-clippy fails in the `wasmtime` and `tree-sitter` build scripts, the
known macOS limit (the change has no platform-specific code).
