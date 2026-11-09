---
title: 'RFU-7: landing note for the Positron fork work'
date: 2026-11-09
description: 'The fork branch and its commits, what ran under Playwright, what a person saw, what was not run (three e2e cases and the raw-record experiment), the non-default-profile failure and its cause, and the fork environment incidents.'
---

**Plan:** [RFU-7](../plans/2026-11-08-rfu-7-positron-fork.md) \
**Fork:** `/Users/gordon/src/positron-riu6-a`, branch `q2/rfu-7-demo-followups`, cut from `spike/riu-6-phase-a` at `99aa72fcb4`.
Eight commits, local only. Nothing was pushed and no other fork branch was touched.

## Fork commits

| Commit | Subject |
|---|---|
| `4692725893` | q2-records-demo: use q2 convert and the execute: record-sources key |
| `38dbb21a30` | q2-records-demo: add the demo run guide |
| `5a34132928` | e2e: the Julia warm-up call uses q2 convert and execute: record-sources |
| `4cbd86bcb4` | q2-records-demo: ask Positron for the output cache directory |
| `2821b5302b` | e2e: R render-and-push test, and a non-default-profile test (fixme) |
| `dd1132bbdb` | e2e: the non-default-profile test opens the palette with the standard shortcut |
| `7251378ab2` | e2e: untitled, q2-too-old and dirty-document cases, and command 1 on R spans |
| `18e1bdfacb` | e2e: raw-record experiment for knitr fallback cells and marimo |

**For Gordon to review:** `4cbd86bcb4` changes Positron core, not just the extension. It adds the internal command
`_positronQuarto.getOutputCacheDir` (in `quartoPushOutputs.ts` and `quartoOutputCacheService.ts`), so that command 3
asks Positron where the cache is instead of guessing from the extension's own storage path.

## What ran

Marks: **Playwright** (automated), **CLI**, **not run**. The R test (commands 2 and 3) and the two Julia tests (commands 2 and 3) are additionally marked **seen**: Gordon watched them on
2026-11-09. Gordon reviewed `4cbd86bcb4` and approved it.

| Check | Mark | Result |
|---|---|---|
| Python, three tests in `quarto-render-and-push.test.ts` | Playwright | pass |
| Julia, two tests | Playwright | pass |
| R: command 2 outputs arrive; command 3 shows them from the cache with no new R process | Playwright | pass (the R-process detector was checked to catch `exec/R`) |
| Commands 2 and 3 under `--profile q2-e2e-profile` | Playwright | pass (52 s, after the fix below) |
| Extension vitests (`extensions/q2-records-demo/src`) | CLI | 66 of 66 |
| Core type check (`npm run build-check`) | CLI | 0 errors |
| "q2 too old" (a fake q2 that exits 2): the toast says "probably too old" | Playwright | pass |
| Untitled document: the toast says "Save the document first" | Playwright | pass |
| Dirty document: the saved-file note appears, and the saved file's outputs still arrive | Playwright | pass |
| Command 1 on R spans (`inline_exprs_r.qmd`) | Playwright | pass (2 values, 42 and 3) |
| Command 1 on Julia spans | not run | Positron has no Julia kernel in this build, so there is nothing for the command to run on |
| Raw-record experiment (fallback cell, marimo) | Playwright | ran; both shapes show raw markdown or an encoded payload, see below |

All runs used `target/debug/q2` built from this branch.

Finding from the R span case: a document with no code chunk has no primary language, so command 1 finds no spans.
The first R fixture had only spans and showed no values; adding one `{r}` chunk fixed it. This is existing Positron
behavior, not a change made here, and it is not documented in the demo guide.

## The non-default-profile test

The test had been marked `test.fixme`: the window opened and Python started, but the palette never found
`q2.renderAndPush` within 15 s, although the extension loaded.

**Cause (checked with a screenshot):** the extension was fine. The palette lists both q2 commands in the new
profile. The harness's `quickaccess.runCommand` opens the palette with a custom `Cmd+J E` keybinding that only the
default profile has. A new profile starts with no `keybindings.json`, so the palette never opened. An 8 s delay did not
help, which ruled out slow extension loading.

**Fix:** the test opens the palette with the standard `Cmd+Shift+P` through a local `runFromPalette` helper. Commands 2
and 3 then pass under the new profile, which meets the plan's condition for the cache-location box. I did not run the
test against the old cache-directory code, so the run shows the feature works in a non-default profile but not that
this test alone would catch its absence.

## Seen by a person

Gordon watched the R and Julia e2e runs (rerun on 2026-11-09 after the landing). Earlier in the session, nothing was watched. Gordon saw stray "E" characters land in the document during some earlier runs. They did not
appear in the last run. Gordon said to ignore them while they cause no trouble, and I left them alone and did not
investigate.

## Raw-record experiment

Ran under Playwright with a rebuilt `q2spike.pushRaw` helper. The push reports every cell matched, but the fallback
cells and the marimo cells show raw markdown source or a base64 payload instead of outputs. **Recommendation: keep the
two export skips.** Details, fixtures, the helper source and the run recipe are in the
[raw-record experiment note](2026-11-09-rfu-7-raw-record-experiment.md). The helper is not committed to the fork
because its commit hook rejects new `.js` files; the test is committed (`18e1bdfacb`).

## Fork environment incidents

- Replacing the fork's `node_modules` symlinks with a real install emptied `positron-spike-s3`\'s extension
  `node_modules`. Gordon approved a repair and `npm ci` was re-run there. Node 24.22.1 is required. The pre-commit hook
  fails under Node 24.5.0, so put 24.22.1 on `PATH` before committing.
- Nine orphaned `esbuild --watch` processes from `spike-s3` may remain.
- The `workspace-3` `target/` directory vanished mid-session for an unknown reason, and q2 was rebuilt.
- One e2e run failed for an unknown reason and the next passed with no code change.
