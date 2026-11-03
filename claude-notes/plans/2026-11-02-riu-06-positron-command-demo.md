---
title: 'RIU-6: the Positron command demo'
date: 2026-11-02
description: 'Three commands in Gordon fork of Positron that exercise both directions between Positron and q2 records: run inline expressions, render with q2 and show the outputs inline, and preview from Positron cache with no execution. A spike in the fork, ending with a script, a capture and a findings note.'
status: draft  # written 2026-11-02; mechanisms are provisional until RIU-0 S5 and S6 report
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Depends on:** RIU-3, RIU-4, RIU-5, and [RIU-0](2026-11-02-riu-00-spikes.md) S5 (injection) and
S6 (running q2).
**Estimated sessions:** 2 to 3.
**Origin:** the request that began this epic, which asked for exactly these three commands
before records were integrated. This plan comes last on purpose.

> **For implementers:** this is spike or demo quality, in the fork only. It is not how the
> commands would ship. Nothing is sent to anyone; nothing is pushed without Gordon asking.
> Work in a separate `git worktree` of Gordon\'s fork cut from `spike/q2-s3-inline-exec`,
> never in `/Users/gordon/src/positron`. Follow the Positron conventions in RIU-0 (new files
> over edits, `// --- Start Positron ---` markers on edits to upstream files, vitest via
> `npm run test:positron`, no direct `tsc`).

## Overview

```text
Positron document ----(1) run spans------------------> inline values (S3)
q2 record  --export (RIU-5)--> Positron cache file --inject (S5)--> inline outputs   (2)
Positron cache --q2 preview --records positron (RIU-3, RIU-4)--> page, no execution (3)
```

## The three commands

1. **Run inline expressions.** The S3 command (`quartoInlineExprCommands.ts`), with the mode
   picker removed: always `Interactive`, the only mode that returns correct values without
   console or history side effects. Known side effect, to state in the demo: each span counts
   as a kernel execution.
2. **Render with q2, then show the outputs inline.** Runs `q2` (mechanism from S6, provisional:
   an extension-host `child_process.execFile` behind an internal `_` command, because a
   browser-layer command cannot spawn a process), exports with RIU-5\'s subcommand, then
   injects into the open document through S5\'s change: `reloadCachedOutputs()` on
   `QuartoOutputContribution` plus `clearCellOutputs` and `saveOutput` per cell, matching
   cells in TypeScript against the open model. Without that change Positron never notices an
   externally written cache file for an open document.
3. **Preview from Positron\'s cache.** Runs `q2 preview --records positron` (RIU-3, RIU-4) on
   the current document, with `preview.engine: off` semantics so nothing executes, and opens
   the result.

## Phase A: tests first

- [ ] vitest tests for the pure TypeScript pieces: cell matching between the exported record and
      the open model (by content hash with occurrence), and the command-line construction for
      `q2` with quoted paths.
- [ ] A scripted check, runnable without a window, that the exported file satisfies the
      `positron_loader` mirror (RIU-5 owns the test; this plan only calls it).
- [ ] The rest is manual with Gordon at the window; write the script first so the run is
      repeatable (Phase D).

## Phase B: the commands

- [ ] Port the S3 command into a new file in the worktree; remove the mode picker.
- [ ] **Provisional (S6, U19):** the `q2` runner. Resolve the binary from a setting, then
      `PATH`, with a clear error. A macOS GUI app may not inherit the login-shell `PATH`, so
      the setting is required in practice. Do not use the shell. Pass arguments as an array.
- [ ] **Provisional (S5, U17):** the injection change, wrapped in Positron markers.
- [ ] Command 2: run q2, read the JSON result (RIU-4 contract), write the exported file into a
      temporary location, inject; never write into the live cache directory by hand.
- [ ] Command 3: run `q2 preview`, capture the URL it prints, open it.
- [ ] Report q2 failures in a notification with the stderr tail; never swallow them.

## Phase C: languages

- [ ] Python first, Julia second, through Jupyter kernels (RIU-0 S0 decides what is installed).
- [ ] R through a Jupyter kernel or Ark if S0 finds one; R through knitr is out until the knitr
      epic\'s KNR-3 and KNR-5 land (epic, "Coordination with the knitr epic"). Say which R
      path was used.

## Phase D: demo and findings

- [ ] A short script (what to open, what to click, what to expect) and a screen capture or
      screenshots. A step that no one watched is marked not run.
- [ ] A findings note, `claude-notes/research/2026-MM-DD-riu-6-positron-demo-findings.md`:
      what worked, what did not, what each command would need to ship. Render it with
      `q2 render --strict` before any push.
- [ ] Push the spike branch to Gordon\'s fork only when he asks.

## Exit criteria

- The three commands work on a Python document and a Julia document, or the findings note
  says exactly what blocked each.
- A script and a capture exist.
- Plan checklists across the epic are reconciled with what landed.

## Risks

- **Needs Gordon at the window.** The dev build is slow; EXR-16 S0 spent most of its time on
  it. Reuse the S3 worktree build.
- **Command 2 depends on an upstream change** to Positron (S5) that is not a public API;
  state this in the demo. It is a spike in the fork, not a proposal.
- **Race between injection and the user typing** in the open document; matching by content
  hash makes a mismatch skip the cell, which the demo should show rather than hide.
