---
title: 'OPR-4b: the Julia runner reports expression starts, anchors and output kinds'
date: 2026-11-08
description: 'Conditional on Gordon''s go. The OPR-4a spike showed all three facts are feasible with one runner change, gated on the existing emitUnits option, and found that the runner drops all stderr under warning: false. This plan implements both in a local branch of the QuartoNotebookRunner fork. The work happens in another repository; q2 lands only a note. Gordon pushes the runner branch and records the pinned revision, which unblocks OPR-4c.'
status: complete
depends-on: [opr-4a]
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/4b-julia-runner-facts` · **Lane B** (`workspace-4`) · **Blocks:** OPR-4c.
**Evidence:** [OPR-4a spike note](../research/2026-11-09-opr-julia-spike.md), including a 381-line prototype diff in its
appendix that this plan starts from.
**Needs:** Julia 1.12, a git worktree of the runner repo (`gordonwoodhull/QuartoNotebookRunner.jl`; a checkout is at
`/Users/gordon/src/q2/external-sources/QuartoNotebookRunner.jl`), and Gordon\'s go.

## Start condition

Do **not** start unless this plan\'s `status` is `ready` on the integration branch. Gordon sets it after reading the
OPR-4a note and confirms or changes the scope under "Scope chosen by Gordon". If that section is empty, stop and
report; do not assume the recommendation below.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c opr/4b-julia-runner-facts feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# the runner work happens in a separate worktree of the runner repo (below)
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/4b-julia-runner-facts
```

## Scope recommended by OPR-4a

All three facts, in one runner change, plus the stderr fix below. The verdict was not partial: starts and anchors share
the same plumbing, and kinds add about 25 lines.

## Scope chosen by Gordon

**Decided by Gordon, 2026-11-09:**

1. **All three facts** (expression starts, anchors, output kinds), in one runner change, as recommended.
2. **Stderr fix, option A:** under `emitUnits` the runner keeps stderr at capture and the presenter hides warnings
   using the kinds; with `emitUnits` off nothing changes. `warning` stays in Julia\'s display-only list.

Gordon set `status: ready` on 2026-11-09.

## Goal

Make the runner report, per code cell, the three facts, **only when the caller set `emitUnits`** (the option q2\'s
engine already sends whenever core passes units), so every existing caller gets byte-identical results.

| Cell `metadata.quarto` key | Shape |
|---|---|
| `expression_starts` | `[int]`, 0-based line within the code after the `#|` options |
| `output_anchors` | `[int]`, parallel to `outputs`: the index of the expression that produced each output |
| `output_kinds` | `[null or "message" or "warning" or "error"]`, parallel to `outputs` |
| `output_expressions` | `[int]`, 1-based, parallel to `outputs`; debugging only |

Mechanism (from the prototype): `InlineDisplay` records the running expression for each queued display; stream runs
are keyed by `(stream, displays before, expression, kind)`; a `KindLogger` wrapping `ConsoleLogger` sets the kind from
the log level; `include_str` records `loc.line - first_line` per top-level expression; `evaluation.jl` derives the
three arrays. The worker IPC `PROTOCOL_VERSION` goes 8 to 9 (internal to the runner).

**Also in scope (OPR-4a Finding 1).** Under `warning: false` the runner\'s `_tidy_streams` drops all stderr, including
`@info` and raw `stderr` writes. OPR-3 declared `warning` display-only for Julia, and D2 makes the presenter enforce
a display-only option, which needs the stderr to be in the record. So when `emitUnits` is set, do not drop stderr at
capture (the kinds let the presenter hide just warnings); with `emitUnits` off, behavior is unchanged. If Gordon
prefers the other fix (remove `warning` from Julia\'s display-only list), he says so under "Scope chosen by Gordon".

## Known limits to put in the tests and the note

- Raw `println(stderr, ...)` has no log level, so its kind is `null`, not `"warning"`.
- The anchor of an expression that throws is the end of the code, not the start of the next (never-run) expression.
- Julia shows only the last expression\'s value, so Julia cells never interleave exactly as knitr cells do.
- Output from C code through libc `puts` is lost, before and after this change (pre-existing, not in scope).
- Quarto 1 does not interleave Julia either, so this is new behavior, not a restoration.

## Checklist (every box is `[unattended]` unless marked)

- [x] Create a worktree of the runner repo on a new branch `q2/opr-expression-facts` from the revision pinned today
      in the engine (`resources/extension-subtrees/julia-engine/_extensions/julia-engine/Project.toml`; the spike read it
      as `5e04284`, an ancestor of the checkout\'s `5aa4bd7`). Never modify the runner checkout\'s own branch. Do not push.
- [x] **Tests first** in the runner\'s `test/testsets/` (see its README), failing for the stated reason: with
      `emitUnits` off, results are byte-identical to today on the existing testsets; with it on, the 12-expression
      demonstration cell from the spike note gives `expression_starts=[0..11]`,
      `output_expressions=[1,4,8,9,10,11,12]` and `output_kinds=[null,"warning",null,"message",null,null,null]`; a cell
      with `#|` options, a comment and multi-line expressions gives starts relative to the code after the options
      (the spike\'s case: `[1,8,10,11,12,13,14,16]` against 17 lines); a subprocess write and a late `@async` print
      are attributed to the running expression; `warning: false` with `emitUnits` keeps `@info` and raw stderr.
- [x] Implement, starting from the prototype diff (it touches `InlineDisplay.jl`, `render_io.jl`, `render_code.jl`,
      `render.jl`, `protocol.jl`, `precompile.jl`, `evaluation.jl`). **Gate stream-run splitting on `emitUnits`**: the
      prototype splits unconditionally.
- [x] **Update the runner tests the prototype breaks.** The prototype changes the `streams` tuple shape, so
      `QuartoNotebookWorker/test/streams_test.jl` and `protocol_test.jl` must change; the spike did not run the runner\'s
      suite. Run the **full** suite and record the result. Do not weaken an assertion: change a shape only where
      the wire changed on purpose, and say which in the note.
- [x] A figure: add one Plots.jl (or other real figure) cell to the tests; the spike used only a custom `display`.
- [x] Re-time the 1200-expression cell (spike: median 4.22 s baseline, 4.10 s prototype) with `emitUnits` on and off.
- [x] Commit in the runner worktree with clear messages. Write the list of commits (hashes and subjects) into
      "Notes from the run", and put a `git format-patch` of them under the note\'s directory in this repository if it
      is small (under 200 lines); otherwise name the runner branch only.
- [x] Land in q2 **only** a note `claude-notes/research/2026-11-xx-opr-julia-runner-facts.md` (what changed, how to
      reproduce, the test results) plus the plan\'s own status change. No q2 product code changes in this plan.
- [x] Render-check the note (`target/debug/q2 render --strict <file>`).

## Exit criteria

- The runner branch exists locally with a passing full runner suite; the note records its commits and how to run the
  new behavior (the three keys appear only under `emitUnits`).
- **`[Gordon]` (after landing):** push the runner branch to the fork, then write the pushed revision into OPR-4c\'s
  header and set OPR-4c to `ready`. OPR-4c cannot start without that revision.

## Notes from the run

Full record: [runner facts note](../research/2026-11-09-opr-julia-runner-facts.md).

- **Runner branch:** `q2/opr-expression-facts` in the runner repository, from `5e04284`; commits `1551a78` (tests, red),
  `b3724f6` (feature), `ffe5dab` (formatting). Not pushed. The patch series is about 1100 lines, so it is not copied here.
- **Gate:** a `trace` flag on the worker `RenderRequest`, set when the notebook runs with `emitUnits`; stream-run splitting, the
  `KindLogger` and the stderr keep under `warning: false` all depend on it. Without `emitUnits` the output is unchanged (a test
  pins it, including unsplit stream runs).
- **Tests:** new `expression_facts_test.jl` (27 assertions), two items in the worker `streams_test.jl`, a Plots.jl figure item in
  `plots_test.jl`. Existing `streams`-tuple expectations changed only for the two added fields; `protocol_test.jl` assertions
  grew stronger.
- **Results:** main suite 1152 pass, 1 fail, 4 error of 1157; all five are `julia +channel` items that a plain `julia` symlink cannot run
  (inferred from the messages, not re-run on the base revision). Worker suite 248/248 in a reduced environment (CairoMakie, PlotlyBase,
  RemoteREPL, SymPy, PythonCall, RCall items skipped; the worker\'s own `Pkg.test` cannot resolve offline).
- **Timing:** 1200-output cell, medians 9.28 s with `emitUnits` and 9.47 s without; within noise. Absolute times are not
  comparable with the spike\'s (different cell, shared machine).
- **Surprises:** (1) the plan\'s expected starts for the options cell counted a blank line the worker does not count; the worker
  counts from the first line of code after `#|` options and blank lines, which matches the spike\'s case. (2) A throwing cell
  records starts only for expressions that ran. Both are now pinned.
- **Not done:** a q2 render through the changed runner (OPR-4c); the five juliaup-channel items on the base revision; the
  worker items listed above; a push.
- **Gordon, still owed:** push the runner branch, write the pushed revision into OPR-4c, and set OPR-4c to `ready`.
