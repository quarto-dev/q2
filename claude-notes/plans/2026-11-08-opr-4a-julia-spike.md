---
title: 'OPR-4a: Julia spike, can the runner report expression starts, anchors and output kinds'
date: 2026-11-08
description: 'A pass or fail spike. Decides whether QuartoNotebookRunner can tie each output of a Julia cell to the expression that produced it, report where each expression starts, and tell a message from a warning. Produces a note with a verdict and a proposed set of optional wire fields. Gordon decides, which enables or cancels OPR-4b to OPR-4d.'
status: complete
depends-on: []
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/4a-julia-spike` · **Lane B** (`workspace-4`) · **Blocks:** OPR-4b (through Gordon\'s go).
**Needs:** Julia 1.12 (works offline), a checkout of the runner (`external-sources/QuartoNotebookRunner.jl` in the
main repo, `/Users/gordon/src/q2/external-sources/`), the `quarto` on PATH (Quarto 1 dev checkout) for one comparison.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c opr/4a-julia-spike feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/4a-julia-spike
```

## Goal (decision D4: Julia is in scope, subject to this spike)

Knitr\'s capture supplies three facts per cell that no other engine supplies: **expression starts** (where each
top-level expression begins), **anchors** (the number of source lines before each output, so output can interleave
with code) and **output kinds** (message, warning, value and so on). They give exact `prompt: true`, numeric
`echo: [1, 3]`, interleaved output and numeric `warning` and `message`
(`crates/quarto-core/src/presenter/cell.rs`, `render_cell_interleaved`; spike note S5 has the feature-by-input table).
Knitr derives them from R\'s ordered pieces (`engine/knitr/pieces_map.rs:31`, `:53`, `:88`).

The question: can Julia supply them? This is a **spike with a verdict**, not a design. It changes no q2 product code.

## What is known (relied from one review agent, 2026-11-08; re-check)

- The only place the runner sees expression boundaries is its `include_str` loop
  (`src/QuartoNotebookWorker/src/render_code.jl`, about lines 89-125): it walks `ast.args` with a `LineNumberNode`
  and calls `run_post_eval_hooks()` per expression, but returns only the last result.
- The runner tags each stream run with how many displays preceded it (`_record!`, `render_io.jl`, about line 44),
  which gives order among outputs, not which expression produced them.
- Message and warning are not distinguished: both are stderr.
- The runner\'s annotation carries one `line` per chunk (`evaluation.jl`, about lines 269-285), not per expression.
- Output order across streams and displays exists; whether stdout and stderr keep true order when mixed is unknown.

## Checklist (every box is `[unattended]`)

- [x] Read the runner source named above, and q2\'s side: `engine/ts_protocol.rs` (`TsRecordInfo`, `UnitResult`,
      `TsExecuteOptions`), `engine/ts_record.rs` (`read_engine_notebook`), and the engine extension under
      `resources/extension-subtrees/julia-engine/src/` (`julia-engine.ts`, `record-wire.ts`).
- [x] **Quarto 1 comparison.** Render a two-expression Julia cell with Quarto 1 (`quarto render`) and with q2; record
      what each shows (interleaved or source then outputs). This says whether interleaving is new or restored.
- [x] **Prototype in a scratch copy** of the runner (a `git worktree` of the runner repo on a scratch branch, outside
      q2\'s tree): record, per top-level expression, its starting line, and attribute each display and stream to the
      expression during which it arrived. Demonstrate on a cell with `println`, a value, a plot display and an
      `@warn`: each output tied to its expression, with line numbers. Report timing overhead.
- [x] **Message against warning.** Can the runner tell `@info`, `@warn` and `@error` apart (a logger hook), and does
      stdout keep order against stderr? Report separately; this is the hardest of the three.
- [x] **Propose, do not implement:** the minimal optional per-unit wire fields (names, shapes, versioning) that would
      carry the three facts through `ts_protocol.rs`, and which q2 cell-metadata keys they would become (the knitr
      names: `quarto_expression_starts`, `quarto_output_anchors`, `quarto_output_kinds`).
- [x] **Verdict per fact** (feasible, feasible with changes to the runner, not feasible) with effort in sessions, and
      a recommendation: which of the three to build, in what order. State the effect on OPR-4b to OPR-4d if the
      verdict is partial (for example expression starts and anchors only).
- [x] Render-check the note (`target/debug/q2 render --strict <file>`).

## Exit criteria

- The note `claude-notes/research/2026-11-xx-opr-julia-spike.md` has a verdict for each of the three facts, the
  demonstration output, the Quarto 1 comparison, the proposed wire fields, and a recommendation.
- No file outside `claude-notes/research/` changed; the scratch runner worktree is not part of the repository.
- **`[Gordon]` (after landing, not part of this plan\'s run):** he reads the note and sets OPR-4b to `ready` (with
  the scope he chooses written into its header) or to `cancelled`, which also cancels OPR-4c and OPR-4d.

## Notes from the run

Note: [2026-11-09-opr-julia-spike.md](../research/2026-11-09-opr-julia-spike.md). Verdict: all three facts feasible
with one runner change (no q2 product change; `ts_protocol.rs` untouched).

- Ran: prototype in a scratch worktree of the runner (`spike/opr-4a-expressions`, diff in the note's appendix) on four
  cells; timing (no measurable overhead); Quarto 1 and q2 renders of the same cell (neither interleaves, so interleaving
  is new for Julia). Servers were isolated with a scratch HOME and a copy of the runner project.
- Read only: q2's wire files and the knitr key names.
- Not done: the runner's own test suite, a Plots.jl figure, a q2 render through the prototype runner.
- Found (not fixed): the runner drops all stderr at capture when `warning: false`, which contradicts D2's reading of
  Julia's display-only declaration (OPR-3); fold the fix into OPR-4b. The runner checkout is at `5aa4bd7`, q2 pins `5e04284`.
- The plan named the note `2026-11-xx-opr-julia-spike.md` under `claude-notes/research/`; it is
  `2026-11-09-opr-julia-spike.md`.
