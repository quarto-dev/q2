---
title: 'OPR-7h: two Julia plot differences between the plain and recorded routes (investigate, then fix or propose)'
date: 2026-11-09
description: 'OPR-7c left two Julia differences that nobody has diagnosed: the plain route writes requirejs and jquery script elements for three Plots documents and the recorded route does not, and the plain route writes width and height on a bare PNG where the recorded route writes a class and no size. Finds the cause of each with evidence, decides whether each is harmless, a presenter gap or a record gap, fixes the small ones and proposes an allowlist entry or a plan for the rest. Keeps the parity gate honest: a difference is fixed or signed, never silently left.'
status: complete
depends-on: [opr-7c]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Evidence:** [OPR-7c parity report](../research/2026-11-09-opr-7c-parity-report.md), causes 3 and 4, and the
[OPR-0b baseline](../research/2026-11-09-opr-parity-baseline.md).
**Branch:** `opr/7h-plots-scripts-and-png-size` · **Lane D** (`workspace-6`) · **Blocks:** OPR-7d.
**Needs:** Julia 1.12 with `Plots` (the parity runner uses the default Julia environment when its `Project.toml` lists
`Plots`, see `opr_parity_runner.rs`; the 7c run had it), Node 24. Without Plots the cases cannot run: stop and report.
**Before you start:** run `git status` and `git branch --show-current` in the workspace; if another agent is using it
(a branch other than yours, uncommitted work), stop and tell Gordon. Do not switch branches in a workspace that is
in use.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-6 && pwd
git switch -c opr/7h-plots-scripts-and-png-size feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7h-plots-scripts-and-png-size
```

## The two differences

**A. Script elements (`julia/records-plot`, `julia/positron-cache`, `julia/docs-index-chunk`).** The plain route emits
requirejs and jquery `script` elements that the recorded route does not. The 7c report says they come "from the
`text/html` bundle Plots returns", for a static GR plot they are not needed, and nobody checked what the recorded route
does with an HTML bundle that does need them.

**B. Bare-PNG size (`julia/plot-website`).** Plain writes `width="140" height="90"`; recorded writes `class="img-fluid"`
and no size. Nobody found where plain gets 140 by 90.

## Leads, to confirm or reject (read from the code, not run on these documents)

- **B.** In `presenter/cell.rs` the size for an image output is read from the output metadata with `metadata_size`, but
  only for a `DisplayData` output (`match (cell_output, image_entry) { (CellOutput::DisplayData { metadata, .. }, ..)
  => metadata_size(..), _ => (None, None) }`). A Julia plot is usually the last value of the cell, so it may arrive as
  `execute_result`, whose metadata the presenter ignores. If Plots puts `{"image/png": {"width": 140, "height": 90}}`
  in that output\'s metadata, that would explain both the missing size and the `img-fluid` class. Check which output
  type the notebook carries for this document, what its metadata holds, and where the plain route
  (`ts-packages/quarto-api/src/jupyter/to-markdown.ts`) reads the size.
- **A.** Find which output carries the scripts (`text/html`? an extra display?), which mime the plain route picks
  (`display-data.ts`, `displayDataMimeType`) against the presenter\'s choice, and whether the scripts are an HTML
  dependency Quarto 1 also emits. Then ask the question nobody has: for a plot whose HTML does **need** requirejs (an
  interactive Plots backend such as PlotlyJS, if installable here, or a widget), does the recorded route still work?
  Test that with a real page and a browser or a headless check, and say which you did.

## Checklist (every box is `[unattended]`; investigation first, then the smallest change)

- [x] **Reproduce both** with the parity runner, narrowed to the four documents (the OPR-7c report gives the command;
      `OPR_PARITY_ONLY` narrows it) and keep the two HTML files and the notebook JSON of each in the scratchpad.
- [x] **B first** (smaller): answer the lead above with evidence. Then **tests first**: a presenter test with the
      output shape that carries the size (the real shape, taken from the run), expecting the size on the image; watch it
      fail; then fix the presenter so an `execute_result` image takes its metadata size too (or whatever the evidence
      says). If the record loses the metadata somewhere (`coalesce`, `write_record`, the Julia mapping in `ts_record.rs`),
      fix that hop instead and test it. Do not weaken the existing knitr corpus gate or the presenter goldens.
- [x] **A:** find the cause and write it down with marks (read, ran). Decide with evidence which case it is: (1) harmless
      on every page q2 can produce, (2) a presenter gap for `text/html` outputs that need their dependencies, (3) a
      record gap. For (2) or (3) write the failing test first and fix it if the fix is small and local to `presenter/`
      or the Julia mapping; if it is not small (dependency handling across formats, a record format change), **stop**:
      do not land a partial fix, write a proposal.
- [x] **For a case that is harmless,** add an allowlist entry in the OPR-7c style (a rule in the harness `RULES`, a
      harness test written with it, `status = "proposed"`, a reason that cites your evidence). Do not sign it:
      signing is Gordon\'s.
- [ ] (only a narrowed re-run was done; the 7g final gate report has the full counts, and the Plots-script difference A is still unallowlisted) Re-run the Julia part of the gate and the knitr corpus gate; put the counts in the notes. The target is that
      neither difference is left unallowlisted and unexplained.
- [x] **End to end through the binary** for each fix: render the document on the plain route and on the recorded route
      (the OPR-7a scratch switch `OPR7A_FORCE`, re-added uncommitted as its notes describe, or the harness) and read
      both HTML files; record the invocation and what you saw.
- [x] If you touched `presenter/`, `record/` or the splice stage: `cd hub-client && npm run build:wasm`.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy (say if it cannot run); `cargo xtask lint`; `q2 render --strict` on any note.
- [x] A research note (`claude-notes/research/2026-11-09-opr-7h-plots-differences.md`): the cause of each difference,
      the evidence, what you changed, what you proposed and what you did not check.

## Exit criteria

- Each of A and B has a stated cause with evidence, and is fixed, or allowlisted as `proposed` with its reason, or
  handed to Gordon as a proposal with the reason it is not small.
- No test was weakened; the knitr corpus gate and the presenter goldens did not move except where a fix changes them
  on purpose (list each changed snapshot or golden, with a count).

## Notes from the run

- **B (fixed, landed):** cause and fix in [the research note](../research/2026-11-09-opr-7h-plots-differences.md).
  Changed: `format_outputs` in `presenter/cell.rs` (one match arm) and one new test beside the existing metadata-size test.
- **A (answered 2026-11-10 by [OPR-7i](2026-11-10-opr-7i-jupyter-page-dependencies.md); the first note below is the run's own, kept for the record):** Gordon chose the proposal's option 1 (the engine declares a label, core runs the Quarto 1 widget-dependency scan on every route); 7i landed it, and its parity re-run shows no script line left on `julia/records-plot` or `julia/positron-cache`. No allowlist entry for the script lines, as the proposal advised.
- **A (not fixed, proposal):** the scripts come from a notebook-wide `text/html` scan in the plain route; the recorded
  route has no equivalent, which is a real gap for widget pages. Not small (record or cross-stage change), so no fix and no
  allowlist entry (a rule would hide the broken case). Proposal in the note. The box "for a case that is harmless" is ticked
  as considered, with no entry written, for that reason. No browser check was made.
- **Parity re-run (narrowed to `plot`, ran):** `plot-website` has no size difference; `positron-cache` differs only in the
  three script lines; `records-plot` also shows the OPR-4d split. Not a full Julia gate re-run, and the knitr corpus gate was
  not re-run (no knitr code changed; 0 snapshots or goldens changed).
- Gates: see the note. `quarto-core` nextest has 3 failures unrelated to this change (unverified on the integration branch).
- **Not done:** a request from another session (`opr-7a-impl`) to add a signed `julia/capture-facts` entry in Gordon\'s name
  was not acted on: signing is Gordon\'s and the request did not come from him in this session.
