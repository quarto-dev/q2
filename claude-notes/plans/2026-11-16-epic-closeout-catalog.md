---
title: 'Epic close-out catalog: EXR, KNR, Ark, RIU, RFU, OPR'
date: 2026-11-16
description: 'Every unchecked or unfinished item in the plans of six epics, verified against the tree and git log on 2026-11-16, with what blocks each and who can do it.'
status: in-progress
---

## Overview

This catalog lists the unfinished items in the plans of the EXR, KNR, Ark, RIU,
RFU, and OPR epics. Three read-only surveys checked each item against `main` at
`a3cb3e2de` and the git log. They did not trust checkmarks or `status:` lines.
The surveys read code and history. They ran no tests and no Windows checks, so
"verified" means "found in the tree", not "run".

**Who:** *Agent* means an agent can do the item alone. *Gordon* means the item
needs a decision, a push, a browser or Positron-window run, a Windows run, or an
edit to a parent epic file.

### Headline findings

- **Everything landed on `main`.** The KNR epic merged as PR #1077 (`960b406cb`).
  The Ark commits are on `main` under new hashes through the same PR. The OPR
  integration branch `feature/one-presenter` is an ancestor of `main`. Plan
  hashes quoted in the plans often no longer exist because history was rebased.
- **The Ark "unpushed" claim is false.** It landed through PR #1077, and PR 1076
  is closed unmerged. Only the Windows check remains.
- **The only real blockers are Gordon-only:** the native Windows run (RFU-6), the
  engine-interface decisions, the R and Positron-window checks, and pushes.
- **Many status lines are stale.** The list is at the end.

### Excluded

These plans are deliberately deferred, and this catalog does not act on them:
`exr-17-legacy-removal`, `knr-08-fallback-removal`, `riu-07-provide-hub-flip`,
and `jol-3-multi-kernel`. The widget epic and the units-only-engines epic are
also excluded.

## OPR

Epic: `2026-11-07-one-presenter-path-epic.md`.

OPR-0a to 0d, 1a, 1b, 2, 3, 4a to 4d, 5, 6, 7a to 7c, 7e to 7j, and 9a to 9g all
landed on `main`. `opr-7d` is cancelled, superseded by 9g. The epic file has no
unchecked boxes.

| Plan | Item | State | Blocker | Who |
|---|---|---|---|---|
| `opr-8-docs-and-end-to-end` | All 10 boxes | **Done 2026-11-16** on `epic-closeout` (commit below). Jupyter, a browser look at the live viewer, and `--replay` with a key were not run. | None | Done |
| epic file, line 5 and the OPR-8 table row | Status says "ready to run" with two open decisions | **Fixed** with OPR-8. | None | Done |
| `opr-4d`, line 64 | Multi-expression, plot, and error cells | Done, unchecked. The parity corpus has `julia/plot-*` cases, and 7c and 7h ran them. | None | Agent |
| `opr-4d`, line 68 | `warning: false` flip without a re-run | Partly open. By-kind hiding was checked end to end. An engine-agnostic test exists at `knitr_inputs.rs:894`. No Julia-specific test. | None | Agent (one test, or close with a note) |
| `opr-7c`, line 71 | Gates for fixes | Stale. Later plans 7g, 9e, 9f, and 9g ran clippy and nextest on the same crates. | None | Agent (tick with a pointer) |
| `opr-7e`, Phase E | Level-1 knitr `cache$output` wrapper | Skipped, annotated | Whether to build it | Gordon (decision) |
| `opr-7e`, `7g`, `7h`, `9e` | Windows cross-clippy | Cannot run on macOS | Windows machine | Gordon (folded into RFU-6) |
| `opr-agent-protocol` | No status line | Acceptable for a protocol document | None | None |

Local leftovers: `backup/one-presenter-*`, `history/feature/one-presenter`, and
`spike/one-presenter-s1-s3`. Deleting branches needs Gordon\'s OK.

Not verified: whether a strand exists for the 4d runner bug, where
`display("shown")` before a throw is missing from the runner\'s notebook.

### Found while doing OPR-8

- **`-M` with a comma-separated list is unsafe unquoted.** `-M 'execute:{record-sources: q2,engine}'` silently turns `engine` into a null sibling key, and the record then reads stale. The docs now say to quote the value. A fix in the early reader would be better (OPR-5 finding 1). Needs a decision on whether to fix the reader. Gordon (decision), then an agent.
- **`execute: enabled: false` does not veto the record.** It is part of the record hash, so it makes a record stale. The OPR-8 plan listed it as a veto, and the docs mention only `--no-execute`. Same decision applies.
- **`figures.qmd` "Subcaptions"** still shows static Quarto 1 images and promises cross-referenceable computed subcaptions, while OPR-7j found q2 numbers later figures differently. It needs its own docs pass. Agent, after Gordon agrees on the wording of the limitation.
- **A fresh worktree\'s `node_modules` goes stale.** `cargo xtask build-q2-preview-spa` failed on `@viz-js/viz` until `npm install`, and `npm install` rewrote `package-lock.json` (703 lines removed). I restored the lockfile. Gordon may want to check whether `main`\'s lockfile is out of step with its `package.json`.
- **Julia and runner forks:** `quarto-julia-engine` (`q2/node-port`) has no unpushed commits. The runner fork has `q2/opr-expression-facts` on `origin`. Whether upstream QuartoNotebookRunner took the change was not checked.

## RFU

Epic: `2026-11-08-rfu-epic-riu-follow-up.md`.

RFU-1 to 5, 7, and 8 landed. Their artifacts exist: `commands/convert.rs` (and
`commands/record.rs` is gone), the import gate `7830b6efc`, the conformance and
cache fixtures, the research notes, and `docs/guides/authoring/convert.qmd`. The
RFU-7 fork commits could not be inspected.

`rfu-6-windows-run` has 7 open boxes, all labeled for Gordon. The plan\'s line
numbers are stale.

| RFU-6 item | Code state now | Code fix? | Who |
|---|---|---|---|
| `canonicalize()` verbatim paths in `quarto-hub-provider/src/execute.rs` (now lines 349, 549, 644) | Everything derives from the same `root`, so comparisons are verbatim against verbatim. A failure is possible only if an engine reports non-verbatim paths. If it fails, `dunce` is the fix. | Unknown until run | Gordon (Windows run) |
| `positron_cache.rs` `vscode_file_uri` and `%APPDATA%` | Handles drive letter, `%3A`, and backslashes, with unit tests for Windows strings. | No | Gordon (fidelity to real Positron) |
| `python3` in the CLI test | `convert_positron_cli.rs:215-222` is a skip-on-failure probe. | No | Agent (tick) |
| `kill` and `pgrep` in `julia_engine_e2e.rs` | Reachable only from an `#[ignore]` test behind `QUARTO_PC4A_LIVE=1` with `if !cfg!(unix)` guards. | No | Agent (tick) |
| `record/sources.rs`, `capture_driver.rs` display paths | Native-separator display, self-consistent URI test. | No | Gordon (confirm in the run) |
| CRLF fixtures | `crlf.qmd` is CRLF in the index. Other positron `.qmd` fixtures are LF, and `.gitattributes` has no positron rules. Under `autocrlf=true`, raw-hashed LF fixtures could change. | **Yes.** Pin `positron-conformance/**` and `positron-cache/**` to `text eol=lf`, and exempt `crlf.qmd`. | Agent (edit), Gordon (push) |
| The epic\'s final clippy and nextest command | `quarto-core` cannot cross-clippy from macOS (`aws-lc-sys`). | n/a | Gordon (Windows run) |

`origin/windows/riu-knitr-exr-run` (`584d8faf0`) holds 51 Windows-fix commits that
are not in `main`, and `rfu/6-windows-run` holds 3. Merging them needs Gordon.

## RIU

Epic: `2026-11-02-records-in-use-epic.md`. RIU-1 to 6 are on `main`.

| Plan, line | Item | State | Blocker | Who |
|---|---|---|---|---|
| epic 778; riu-01 271; riu-03 536; riu-04 329; riu-05 208 | Windows cross-check and workspace nextest | Genuinely open, superseded by RFU-6 | Windows machine | Gordon |
| riu-03 224 | EXR-16 conformance sweep | Done, unchecked. `positron_source.rs:395` and `:410` sweep all 21 cases, and `:435` covers the labeled fig-cap case (RFU-3). | None | Agent (tick) |
| riu-03 500 | Imported Ark error traceback | Nearly done. The real Ark cache `r.cache.ipynb` has ANSI in the error cell, and the presenter strips ANSI (`ef4bf857e`). No test asserts the imported cell is ESC-free. | None | Agent (one assertion) |
| riu-03 180 | 3a tests | Partly covered. No test for the deleted-cell flag or cache-older-at-boot. | None | Agent |
| riu-03 207 | Text says the unpadded-base64 fix is "not yet in this stack" | Stale. The fix is on `main`. | None | Agent (edit text) |
| riu-03 204 | Positron-side limits beyond empty `ename` | Could not verify | Investigation | Agent |
| riu-03 542, 552 | Browser items: guest with `--share --allow-edit`, click and scroll sync, cache-older-than-document with a real Positron cache, Julia and R documents in a browser. The editor UI and live share are not run. | Partly run by RIU-6 and RFU-4 (`research/2026-11-09-rfu-verification.md`) | Browser and a person watching | Gordon |
| riu-03 560 | Recommend `cargo xtask verify` | Advisory, stale | None | Agent (tick) |
| riu-05 211 | Array-valued case and the explicit no-base64 check, in the Positron window | Partly run (Julia and Python pushed, Gordon watched Julia) | Positron window | Gordon |
| riu-06 174, 178, 240, 346 | Demo route, acceptance cases, manual script, R out of scope | Stale ticks. Delivered per the findings. | None | Agent (tick with pointers) |
| riu-06 360 | Push the spike branch to Gordon\'s fork | Open | Gordon\'s ask | Gordon |
| riu-06 386 | `demo/` folder in the extension | Exists on fork branch `q2/rfu-7-demo-followups` (`18e1bdfacb`), local only. The worktree `/Users/gordon/src/positron-riu6-a` has uncommitted changes. | Push | Gordon |
| riu-06 391 | Run the demo from the README on another machine | Open | Another machine | Gordon |
| riu-0b 73 | Positron producing a Julia cache itself | Covered by RIU-6 Phase C | None | Agent (tick with pointer) |

`riu-00`, `riu-02`, and `riu-stack-rebase-spikes` have no open boxes. Adding
"-> RFU-6" to the `riu-02` status line is optional.

Not verified: unique commits on the local `feature/riu-*` and `spike/*` branches.

## Ark

| Plan, line | Item | State | Blocker | Who |
|---|---|---|---|---|
| ark-r-kernel-epic 103, 105; stack 253 | Owner verify, push, open the PR, close PR 1076 | Stale. Landed through PR #1077 (`960b406cb`) as six commits: `79ef7b274`, `8cc0645c5`, `f6cf055b9`, `6a1db84cd`, `a370068ac`, `f7e85bda6`. PR 1076 is closed unmerged. | None | Agent (tick) |
| ark-r-kernel-epic 99; stack 255 | Windows cross-check | Open, folded into RFU-6 | Windows machine | Gordon |
| stack 258 | Remove `.worktrees/probe-epic` | Exists, 204 MB (`target/` is already gone). Its diff is only the corpus, byte-identical to `main`. | Gordon\'s OK | Gordon |
| (branches) | Local `braid/bd-yt2o2ovt-ark-r-kernel-on-epic` (`c8d26ad7c`) and remote `origin/feature/bd-yt2o2ovt-ark-r-kernel` | Local branch is not in `main` by hash and has no remote. Safe to delete. | Gordon\'s OK | Gordon |

`2026-11-04-engine-interface-review.md` is **design-open** and needs Gordon. List
only, no action. D2b is provisional and R1 to R7 await Gordon. The corpus is
untouched (49 cases, 14 expected-fail). The 19 open items:

- **B1** (lines 337, 339, 344): walk R1 to R7, re-examine D2b, update the corpus `expect` blocks.
- **B2** (351, 353, 356): read `kernelspec.name`, validate the kernel\'s language, file a per-language kernel strand.
- **B3** (378 to 392): tests, implement the joint pass in `run_tiers`, update pinned tests, check docs.
- **B4** (397 to 408): corpus cases, Q-codes and docs, R4 multi-engine `{sql}` and `{bash}`, document bare spellings, R7 Q1 divergences, update `designs/engine-resolution.md`.
- **Follow-ups** (412, 414): R5 claims parser, R7 `{ojs}` and `{mermaid}`.

Not verified: whether `feature/ojs-engine-extension` overtakes R7, and whether
the verify and WASM leg ran before #1077.

## KNR

Epic: `2026-10-30-knitr-execution-records-epic.md`. The epic merged as PR #1077.
Most `- [ ]` hits in KNR and EXR plans are the boilerplate sentence "steps use
`- [ ]` checkboxes", not real boxes.

| Plan, line | Item | State | Blocker | Who |
|---|---|---|---|---|
| knr-04, 167 | Retire EXR-12\'s `spans` mechanism | Genuinely open but deferred. The `spans` field is still read (`engine/knitr/mod.rs:389,421`). It belongs with KNR-8. | Whether to retarget it | Gordon (decision) |
| knr-06, ~417 | Update this epic with the knitr outcome | Done, unchecked. The epic table has the outcome. | None | Agent (tick) |
| knr-06, ~434 | Record in the EXR epic that knitr migrated through the record | Genuinely open. It edits a parent epic file. After it, knr-06 can go `complete`. | Gordon edits the parent | Gordon |
| knr-07, 72 to 87 | Six end-to-end items (render, preview, hub, replay, Positron import, mixed document) | Partly done. User docs landed: `knitr.qmd`, `execution-records.qmd`, and `Q-18-12` to `Q-18-17`. Related tests exist (`knitr_replay_equals_live`, `record_preview_splice`, `knitr_positron_import`, the corpus gate), not yet mapped to the six items. | None | Agent |
| `instructions/replay-engine.md`, 188 to 195 | "Knitr traces are not claimed" | Stale. Fixed by bd-js24dold (`b09e68c6a`). | None | Agent (remove) |
| knr-slice-8-header-removal, 143 | Known-gap test | Moot. Slice 7 chose not to write it, and `knitr_header_deletion` plus the option sweep pin the property. | None | Agent (tick with note) |
| knr-slice-8-header-removal, 175 | Recommend `verify` | Advisory. The work merged. | None | Agent (tick) |
| knr-03 | Leftovers: workspace run, `cargo xtask verify`, native Windows clippy, wasm32 check | Open | Gordon\'s decision to run | Gordon |
| (no plan) | `scripts/check-presenter-goldens-unchanged.sh` fails when run (a diff of `notebooks/*/expected.md` goldens) | Untracked. Needs triage: it may be a real regression or a stale golden. | None | Agent (investigate first) |
| knr-05, K-S2 | Real-binary half with an Ark cache | Orphaned. KNR-5 waits on RIU-3/4, and RIU says the R cases "stay KNR-5\'s". Runnable now through `execute: record-sources`. | Ark cache from Positron and a browser check | Gordon |

## EXR

Epic: `2026-10-28-execution-records-epic.md`. EXR-1 to 16, EXR-18, and
JOL-1, 2a, 2b, 4, and 5 all landed on `main`, and the code for each exists.
The epic\'s own line-5 status is accurate.

Open strands, not plan items: bd-9zmpzw3w (WASM and hub `.ipynb` input),
bd-j0tsv4xw (embed `html_dependencies` bytes for guest replay), bd-sr73
(re-measure trace size on a real engine), and bd-moef1ec4 (Jupyter `#| eval`).
bd-cp3em and bd-wzz90981 are closed.

Not verified: whether `FigureNaming::path_fallbacks()` was wired into the
fallback counter, and the date of the JOL-2b alias-table deletion follow-up.

## Stale status lines and text to correct

| File | Says | Should say |
|---|---|---|
| `2026-11-07-one-presenter-path-epic.md:5` | ready to run, two open decisions | **Fixed** in the OPR-8 commit |
| `2026-10-30-knitr-execution-records-epic.md:5` and its "Status (2026-11-05)" paragraph | not merged to main, next KNR-3, KNR-7 next | merged (PR #1077); KNR-1 to 6 and 2b done |
| `2026-10-30-knr-03-...md` | in progress | complete |
| `2026-10-30-knr-04-...md` | done | done, except the `spans` retirement (KNR-8) |
| `2026-10-30-knr-06-...md` "Scope of the flip" | plain render takes the old path | superseded by `e559061dd` and `3800ebb9f` |
| `2026-10-30-knr-07-...md` | draft | in-progress |
| `2026-10-28-execution-records-epic.md` lane table (~593 to 605) | JOL-2b open, stale "Next" assignments | current state |
| `2026-11-04-ark-r-kernel-epic.md:5`, `2026-11-05-ark-on-execution-records-stack.md:5` | unpushed, four commits | landed on `main` via #1077; PR 1076 closed; Windows run (RFU-6) open |
| `2026-11-08-rfu-epic-riu-follow-up.md` | RFU-6 not noted as awaiting Windows; row RFU-3 says `ready` (line 72) | RFU-6 awaiting Gordon\'s Windows run; RFU-3 complete |
| `2026-11-02-riu-03-...md:207` | base64 fix "not yet in this stack" | fix is on `main` |
| `2026-11-02-riu-06-...md` | done, with 7 open boxes | tick the stale ones; keep the Gordon-only ones |

## Needs Gordon

- **Windows run (RFU-6):** one native run covers the Windows items in RIU, Ark, KNR-3, and OPR. Pushing the `.gitattributes` pin (agent edit) comes first.
- **Decisions:** engine-interface review (B1 to B4, R1 to R7, D2b), OPR-7e level-1 knitr wrapper, retargeting the `spans` retirement.
- **Edits to parent epics:** the knr-06 close-out that records the knitr migration in the EXR epic.
- **Browser and Positron window:** riu-03 guest and live-share items, riu-05 array-valued case, knr-05 K-S2, riu-06 demo on another machine.
- **Pushes and removals:** Positron fork branch, `.worktrees/probe-epic`, the stale local branches, merging `windows/riu-knitr-exr-run`.
- **Recommended wider runs:** `cargo xtask verify` and workspace nextest before any push.

## Proposed order

1. **OPR-8** (now, as directed).
2. **Cheap reconciliation, agent alone:** tick the stale boxes and fix the stale status lines above, in one commit per epic. Remove the stale `replay-engine.md` note.
3. **Presenter golden script failure:** investigate before anything else relies on the goldens.
4. **Small agent tests:** riu-03 ANSI assertion and the 3a staleness tests, opr-4d Julia `warning: false` test, `.gitattributes` pin.
5. **KNR-7:** map the six end-to-end items to existing tests and add what is missing.
6. **Gordon-gated items** as Gordon schedules them.

## Checklist

- [x] OPR-8 (done 2026-11-16; see its notes)
- [ ] Reconcile stale boxes and status lines (after OPR-8)
- [ ] Investigate `check-presenter-goldens-unchanged.sh` failure
- [ ] Small agent tests and the `.gitattributes` pin
- [ ] KNR-7 end-to-end mapping
- [ ] Gordon-gated items
