---
title: 'OPR-1a: one per-cell step over a struct of optional inputs'
date: 2026-11-08
description: 'Replaces the two per-cell presenter steps (present_document and the splice stage) with one function that takes a struct of optional inputs. quarto_resolved_options stops being the switch between the knitr and Jupyter entry paths; capability is read from which fields are present. Carries the scope rule, per-cell engine lookup and the forwarding fix for preview and hub.'
status: complete
depends-on: [opr-0a]
lane: A
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/1a-one-cell-step` · **Lane A** (`workspace-3`) · **Blocks:** OPR-1b.
**Needs:** R 4.3 with knitr (some tests skip without it); `hub-client` build tooling for the WASM gate.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3 && pwd
git switch -c opr/1a-one-cell-step feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/1a-one-cell-step
```

## Goal (decisions D1, D3, D9)

One per-cell function, used by both loops, with **no engine name and no marker key selecting a path**. It takes a
struct built from the cell\'s metadata. Its fields, each independently optional:

anchors, expression starts, output kinds, figure count, resolved options, known options, echoed code.

`quarto_resolved_options` becomes one field of it. Today it is the switch: `present_knitr_cell`
(`crates/quarto-core/src/presenter/knitr_inputs.rs:59`) returns `Some` only when the cell carries it, and both loops
then fall through to `render_cell_with`; that one key bundles resolution, `known_options` forwarding, the figure
count and the dotted-alias rename. Each missing input already degrades on its own (spike S5: whole-cell rendering
without anchors; `prompt` as one expression without expression starts; numeric `echo` ignored; numeric `warning`
and `message` ignored without kinds; no figure id or caption wrapper without a figure count).

The loops themselves stay as two callers in this plan (OPR-1b merges them). In this plan both call the one
function for the per-cell step.

## Decisions that shape this plan

- **D3, scope rule (data, not engine):** if a cell carries resolved options (the knitr case), R already baked the
  document `execute:` scope in (`execute.R:348-353` translates `output: false` to `results: hide` and
  `fig.show: hide`; knitr epic decision 11), so the presenter does not layer the scope. If it carries none
  (Jupyter, Julia, an imported Positron record, a fallback unit), the presenter layers the scope at present time
  as it does today (`resolve_cell_options`, `presenter/visibility.rs:~21`).
- **Forwarding:** Jupyter cells\' unknown `#|` options are forwarded as cell attributes on **both** surfaces.
  Today only `present_document` does it (`document.rs:~168`, `CellOptions::forwarding` at `options.rs:~313`); the
  splice does not. This is a deliberate change to preview and hub output: a bug fix (preview now matches render).
- **Engine entry per cell:** the lookup the splice does (`engine_entry_for`, by the cell\'s `quarto_engine`, else the
  only entry) is used by both. `present_document` today receives one entry for the whole document.
- **Errors and reporting stay with the callers.** The function returns a `Result`; render propagates, the splice
  keeps the source. (OPR-1b turns this into outcomes.)
- **Unchanged here:** matching, spans, stamping, the document-level loops.

## Checklist (tests first; every box is `[unattended]`)

- [x] Start from OPR-0a\'s "Handoff to OPR-1a and OPR-1b" table in `claude-notes/plans/2026-11-08-opr-0a-pinning-tests.md`.
      Every test marked `// OPR-1a:` is on your list; none may change except as the table says.
- [x] **New tests first, each failing for the stated reason:** (1) the struct with every field absent reproduces
      the Jupyter output for all 13 shadow goldens and the six `ipynb-*` goldens, **including
      `warning-false-stderr`** (this plan keeps today\'s knitr enforcement; OPR-2 changes it, so mark this one
      `// OPR-2:`); (2) a cell with one field supplied gets that one feature and no other (anchors alone interleave;
      expression starts alone make `prompt` exact; a figure count alone adds figure ids); (3) the D3 rule on both
      sides (a knitr cell with resolved options is not layered; a Jupyter cell is); (4) forwarding through the splice;
      (5) per-cell engine lookup in a mixed-engine document.
- [x] Define the struct (suggested home: `presenter/knitr_inputs.rs`, renamed if it no longer describes knitr) and its
      builder from `RecordCell` metadata. Keep the dotted-alias rename (`canonicalize_options`, `options.rs:~151`) as a
      shared front-end step applied where resolved options are present, so no Jupyter result changes.
- [x] Replace the per-cell bodies of `present_document` and the splice\'s `present_cell` with calls to the one
      function. Delete the now-dead marker-key path. Do not touch the outer loops.
- [x] Apply D3 (scope layering rule), forwarding on both surfaces, and per-cell engine lookup.
- [x] Regenerate the preview and hub fixtures/snapshots that change **only** through forwarding; review each diff;
      document the count and the reason in the commit message and the landing report (protocol: snapshot rule).
- [x] Update strand `bd-cm1mwyuq` (Jupyter forward-unknown-cell-options): add a comment that the render path was
      fixed on 2026-11-04 (`2649c02ea`) and the preview and hub path is fixed by this plan; close it if nothing else
      remains in its text. (Gordon, 2026-11-08: "updated".) Reference `bd-nn2fou8h` (execute-visibility options) in
      your notes; do not change its status.
- [x] Fix the stale doc comment at `crates/quarto-core/src/presenter/options.rs:~237` ("read under knitr\'s dotted names
      only"; the code reads the canonical dashed `fig-cap` for both front ends); it was carried over from the RIU follow-up
      plan (Item 4), whose boxes moved here and to RFU-4.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`;
      the dependents that call the presenter (confirm with `cargo tree -i quarto-core` that no other crate calls
      `present_document` or `present_cell`; (relied) none does); the Windows cross-clippy for `quarto-core`;
      `cd hub-client && npm run build:wasm` and the hub-client tests that touch records
      (`hub-client/src/services/recordPayload.wasm.test.ts`, `captureSpliceHtml.wasm.test.ts`).

(Every box reconciled against the tree at landing; see "Notes from the run" for the two places the work differs from the plan\'s wording.)

## Exit criteria

- `git grep -n 'quarto_resolved_options'` shows it read in exactly one place (the struct builder) and nowhere used as
  a path selector; no `engine == "..."` or marker test chooses a presenter path in `presenter/` or `record_splice.rs`.
- OPR-0a\'s `// OPR-1a:` tests pass with their new expectations, and every other OPR-0a test passes unchanged.
- The 13 shadow goldens and six `ipynb-*` goldens are byte-identical through `present_document`; preview and hub
  output differ from before **only** by forwarded unknown `#|` options on Jupyter cells.
- WASM build and the Windows cross-clippy are green; the snapshot counts are in the landing report.

## Regression baseline from OPR-0c (2026-11-09)

The refactor must keep bytes unchanged, so the OPR-0c measurements are a ready baseline: the Julia and knitr figure
reference table ([note](../research/2026-11-09-opr-measurements.md) section 5, including the unresolved Julia-recorded group)
and the bare-PNG file names (`cell-1-output-1.png`, `cell-2-output-1.png` recorded). A change in any of them in this plan
is a bug in the refactor, not an improvement; the fixes belong to OPR-7c.

## Risks

(relied) The goldens never set a document `execute:` scope; OPR-0a\'s fixtures close that gap, so trust them over
the goldens. If OPR-0a\'s table shows divergence beyond the rows listed there, stop and report; do not widen this
plan.

## Notes from the run

**Landed:** one per-cell step, `present_cell_step` in the new `presenter/cell_step.rs`, called by `present_document` and by
the splice\'s `present_cell`. It partitions the `#|` options, layers the document scope, handles expanded cells, forwards
unknown options (both surfaces now) and renders. `CellInputs` (in `knitr_inputs.rs`, not renamed) is the struct of optional
inputs: echoed code, anchors, expression starts, output kinds, figure count, resolved options, known options, plus the
original chunk for `fenced.echo`. `CellInputs::from_cell` is the only reader of `quarto_resolved_options`
(`git grep` over `src/`: the writer in `engine/knitr/adapter.rs` aside). `present_knitr_cell` is gone from product code;
a test-only wrapper with that name keeps the 30-odd old unit tests unchanged. No `engine == "..."` test chooses a path.

**Deviations from the plan\'s wording.**

- `present_document` now takes `engines: &[EngineEntry]` instead of one `Option<&EngineEntry>`, so the entry is looked up per
  cell (`engine_entry_for`, by `quarto_engine`) in both loops. Callers: skeleton and `ts_record` pass
  `slice::from_ref(&record.engine)`, replay passes the record\'s entries, the Jupyter engines pass `&[]`. About 20 test
  call sites changed mechanically. One test fixture (`a_structured_knitr_cell_reaches_the_shared_reader`) had to name its
  engine on the cell, because it used to hand the entry in directly.
- `figure_count` and `original_chunk` are fields of the struct, but the builder fills them only when resolved options are
  present, because those were the only cells that used them; filling them for Jupyter would change `echo: fenced` and
  figure output. The scope rule (D3) reads `resolved_options.is_some()` inside the step, as the decision says.
- Holds and `fig.beforecode` read the resolved options, so they apply only to cells that carry them.
- Test (1) of the checklist (all fields absent reproduces the goldens, including `warning-false-stderr`) is the existing
  `jupyter_markdown_goldens`, `shadow_goldens_through_the_knitr_entry_path` and `ipynb_goldens_through_the_knitr_entry_path`,
  all unchanged and green; I added `with_every_input_absent_the_cell_renders_whole` for the struct alone.

**Tests.** Two OPR-0a tests changed, as the handoff table says, and were run first: `unknown_cell_option_is_forwarded_by_render_and_by_the_splice`
(renamed; failed because the splice emitted `::: {.cell}`) and `the_engine_entry_a_knitr_cell_sees_is_looked_up_per_cell_in_both_loops`
(renamed; failed because render did not forward when handed Jupyter\'s entry). The splice writes the AST back as qmd, so its
attribute is `data-testid="hero-plot"` with double quotes where render writes single quotes. `run_both` lost its `engine`
argument. New tests (all passed on first run, since they cover a new function over existing behavior; the two above are the
red-first evidence): `with_every_input_absent_the_cell_renders_whole`, `anchors_alone_interleave_source_and_output`,
`expression_starts_alone_make_prompt_exact`, `a_figure_count_alone_adds_the_figure_id`, and the two D3 tests
`a_cell_with_resolved_options_is_not_layered_with_the_document_scope` and
`a_cell_without_resolved_options_is_layered_with_the_document_scope`.

**Gates.** `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean. `cargo nextest run -p quarto-core`: 8061 run, 8060 passed (OPR-0a baseline 8054 run; +7 new tests, +1 from splitting a test); the only failure is the environmental `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`. Windows cross-clippy: not runnable here (`wasmtime` and `aws-lc-sys` build scripts fail for `x86_64-pc-windows-msvc` on macOS); the change uses no platform API and no path literal. `cd hub-client && npm run build:wasm`: green. `vitest run --config vitest.wasm.config.ts` on `recordPayload.wasm.test.ts` and `captureSpliceHtml.wasm.test.ts`: 8 of 8 pass. Not run: dependents of `quarto-core` (the plan says to confirm none calls `present_document`; `git grep` shows callers only inside `quarto-core`), and no end-to-end `q2 render` or `q2 preview` of a document.

**Snapshots and goldens.** None added, changed or removed; no golden regenerated. Preview and hub output changed only by forwarded unknown `#|` options on Jupyter cells, and no existing fixture exercised that.

**Strands.** `bd-cm1mwyuq` got a comment (the preview and hub path is fixed here); left open because its lightbox-allowlist suggestion is not done. `bd-nn2fou8h` was not touched. The stale `fig_cap` doc comment in `presenter/options.rs` is fixed.

