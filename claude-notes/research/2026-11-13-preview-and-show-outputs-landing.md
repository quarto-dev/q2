---
title: 'Landing note: Preview with q2 and Show Outputs'
date: 2026-11-13
description: 'What landed for the fourth q2-records-demo command, by commit, with what was verified, what was not, and what to run before pushing.'
---

**Plan:** [2026-11-12-preview-with-q2-and-show-outputs.md](../plans/2026-11-12-preview-with-q2-and-show-outputs.md).
Nothing is pushed. q2 branch `plan/preview-with-q2-and-show-outputs` (`workspace-6`); fork branch
`q2/preview-and-show-outputs` in `/Users/gordon/src/positron-riu6-a`.

## q2 commits

| Commit | What |
|---|---|
| `a9192a99c` | `hash_document` ignores `execute.record`, as its contract says. No hash-definition `version` bump (see the message). |
| `23842d767` | `overlay_free_run` moves to `quarto_core::record`; `quarto-hub-provider` re-exports it. |
| `f26a96f8f` | `-M` reaches every preview engine run; runs are made overlay-free; cache key includes `-M`; re-execute honors the record sources; `auto` re-runs whenever stale and re-checks after a run. Closes bd-twox482l. |
| `790d4c880` | `preview.engine` from `-M` (also for `--static`), the `--share` warning, docs. |
| `cf0a7a9a7`, and this note's commit | Plan updates. |

Snapshot files: none added or changed.

## Fork commits (local, not pushed)

| Commit | What |
|---|---|
| `b4fc05f1c3` | Command 4: contract, `liveSync.ts`, `liveSession.ts`, shared `convertAndPush.ts`, `package.json`. |
| `4dd0cdde52` | Playwright cases (not run), demo README, demo `.gitignore`. |

The fork's uncommitted demo files were left alone. The README commit contains only my hunks; the working copy still has
the other uncommitted README edits.

## Verified

- q2: clippy `--all-targets -D warnings` clean for quarto-core, quarto-hub-provider, quarto-preview and quarto;
  `nextest -p quarto-preview` 383 passed, 1 skipped (the bd-9brz `#[ignore]`); `nextest -p quarto -E 'test(preview)'`
  121 passed. The seven new integration tests were red on the pre-fix source for the reasons the plan gives, then green.
- `nextest -p quarto-core`: 8146 passed, 249 failed. Every failure group runs pandoc, and the local pandoc is 3.12.1
  against q2's 3.13 floor; two more are revealjs vendoring checks. No record or hash test fails. I did not run a
  clean-`main` baseline, so this is an attribution from the error text.
- End to end through the real `q2` binary: see the plan's "End-to-end run".
- Fork: 97 vitests, `tsc --noEmit` on the extension, and the fork's hygiene hook.

## Not verified

- **Most of the Playwright cases have not run.** One has: the relayout case (an edit to the table cell of the
  demo's Python document leaves the other outputs in place). The others need Positron's Python, R and Julia
  fixtures. The harness does not start in this tree without a temporary `./lanes` import fix in `scenarios.ts`.
  The extension was used by hand in Positron (the status-bar item, the Viewer, the push), and two problems
  turned up that tests could not have shown, listed under "Found after landing".
- The Viewer's own WASM render under `-M execute:{record: true}` ("Execution record not written" question).
- Docs were not render-checked (pandoc floor).
- The `hub-client` and WASM legs: not touched, `cargo xtask verify` not run.
- Windows: no cross-check run; `liveSync`/`q2Contract` path code is tested with a win32 fake only.

## Found after landing

- A `q2` on `PATH` older than this branch fails silently: its re-execute does not see `-M`, so the `.quarto` copy
  is never rewritten and nothing reaches the editor. The extension does not check the version (an idea, not done).
- Every push redrew every cell. Positron's `pushCells` cleared and redrew each matched cell, and parsing the pushed
  file gives each output a fresh `outputId`, so even an equality check on outputs failed until the id was left out.
  Fixed in the fork (`8def82ab77`, `b89f334658`), with a Playwright case that tags the output nodes.

## Corrections to the plan found on the way

- `-M` does not merge repeated keys, so the command passes one `execute` map (bd-k8ui7n5m).
- The engine-run sites call `runtime_with_overlay()` instead of threading a runtime through signatures.
- A stub engine needs `owned_languages`, and test hub contexts need their own data dir (the plan's Phase 0 notes).

## Strands filed (outside the plan)

- bd-k8ui7n5m: `-M` repeated-key footgun.
- bd-stbigif1: the `.quarto` copy is written in place, not atomically.

## Before pushing

`overlay_free_run` moved into quarto-core, so run `cargo nextest run -p quarto-hub-provider -p quarto` (or the
workspace) first; I did not run them. The preview changes touch `quarto-preview`\'s dependents only through `quarto`, whose
preview tests passed.
