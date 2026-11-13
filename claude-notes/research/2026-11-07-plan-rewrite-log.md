---
title: 'Plan rewrite log: RIU follow-up and one-presenter-path'
date: 2026-11-07
description: 'What changed in the 2026-11-07 rewrite of the RIU follow-up plan and the one-presenter-path epic, box by box, and why, so the plans themselves can read as the current truth. Also the two corrections made in other plans.'
---

The two plans were rewritten, not appended to. This note is where the deleted structure is accounted for. Nothing
here was silent: every old box is listed as kept, ticked, moved, merged or replaced. "Old" means the plan at
`8cfec0fc1`. Evidence for the ticks: [Phase A note](2026-11-07-r-through-knitr-phase-a.md),
[spike note](2026-11-07-one-presenter-spikes.md), and checks listed per row.

## Corrections outside the two plans

| File | What was wrong | What I did |
|---|---|---|
| `plans/2026-11-02-riu-06-positron-command-demo.md` | Two boxes said the demo's run guide is `extensions/q2-records-demo/demo/README.md` (script "now also lives" there; "a README that is the run guide"). That file is not in fork commit `99aa72fcb4`\'s file list, not tracked (`git ls-files`), and not on disk (checked 2026-11-07). The commit message says it added one. | Struck the sentence, un-ticked the `demo/` folder box, added a dated correction pointing at follow-up Item 3, which now has a box to write it. |
| `plans/2026-11-02-riu-03-positron-cache-as-record-source.md` | Status line (line 5) still said 3b "never started"; the plan's own "3b done 2026-11-07" section and the epic's exit criteria say it landed. | Rewrote the status line (3b Phase E landed; editor UI, Phase F browser items and live share not run). |

## RIU follow-up plan

**Structure.** Old order: title, overview, "Order and dependencies", "Before the next review session", Items 1 to 9,
"Eventual goal". New: overview replaced by "What is established", new "Gordon's decisions" and "Open decisions"
sections, "Order and dependencies" rewritten, "Before the next review session" removed, Items 1 to 9 kept as
numbered items, "Eventual goal" kept. Status line and description rewritten. The old note "this is still a demo"
moved into Gordon's decisions.

**Removed section.** "Before the next review session" had two boxes: Item 1 Phase A (now ticked item by item in
Item 1, with the gate left open) and the presenter spikes (ticked in Item 7).

**Order.** The old order had 8 numbered lines. Changed: line 1 (Item 4's second box first) was already done, so it
is dropped; Items 9 and 8 are now the first step (Item 9 before Item 8, both before Item 1 Phase B), as the old
lines 6 and 7 already said but in the reverse numbering; the embedded-docs refresh for Items 4, 8 and 9 is
consolidated into one step because three items each carried their own. Item 7's line 8 is kept in substance.

### Item 1

| Old box | Now | Why |
|---|---|---|
| Phase A: dump a knitr record (ticked) | ticked, kept | unchanged |
| Phase A: hash comparison (struck, ticked) | ticked, kept, extended | Positron's own native run is now a second proof: 8 of 8 hashes equal; Julia 2 of 2 matched |
| Phase A: raw-record experiment | ticked | done, Playwright; see the note |
| Phase A: read Ark's shapes and compare | ticked | done; per-chunk table in the note |
| Phase A: Julia `#|` spike | ticked | done: 2 of 2 matched |
| Phase A: validity list | ticked | written from the loader; the q2 mirror was not compared (stated) |
| Phase A: design, import side (agreed) | turned into a design paragraph under Item 1 | it was an agreed design, not a check; its implementation is Phase C's second box |
| Phase A: findings note with verdict | ticked | the note exists |
| Phase A: `[Gordon]` gate | kept as a box and as the first open decision | still open; unchanged |
| (new) | box: extend the raw-record experiment to a fallback cell and a marimo document | the old Phase B test depends on "if the experiment showed they do no harm", and the experiment did not include those two shapes |
| Phase B: 4 test boxes | kept; the CLI test now targets `q2 convert` | Item 8 replaces the command |
| Phase B: image rule kept (ticked) | moved to "Gordon's decisions" | it is a decision, not work |
| Phase B: track removal of the image rule | kept | unchanged |
| Phase C: 3 boxes | kept, reworded for the verdict and the extended experiment | |
| Phase D: R session exists | ticked | the Phase A run used the `r` fixture with an R 4.3.2 session |
| Phase D: e2e test, watch once | kept | the spike file (untracked) is named as a starting point |

### Item 2

All five boxes kept as they were (three ticked, two open). The ticked fork commit was re-checked: `53bf9ca105` is
in the fork's log, `warmJuliaServer` is at line 17 of the e2e file, the Julia tests carry `setTimeout(480000)`.

### Item 3

| Old box | Now | Why |
|---|---|---|
| Positron cache location prototype | kept | unchanged |
| Viewer big X: document (ticked); write the limitation into the guide | kept both | the guide did not exist (next row) |
| (new) | box: write the run guide | the file the RIU-6 plan cites does not exist |
| (new) | box: write the decided limits into the run guide | gathers the decided items that said "say so in the run guide": replaces-outputs-no-prompt, kernel state unchanged, command 1 values not persisted |
| Command 2 progress and cancel | moved to "Open decisions", facts added | Gordon owns fix versus document; the "confirm what it shows" half I did by reading `exportCommand.ts`: indeterminate notification, no cancel button, only a 300 s timeout |
| Dirty document wording | moved to "Open decisions", facts added | read `checkDocument` and the push summary: the two statements agree in substance, so the "make them agree" box is answered; fix or document stays Gordon's |
| Stale Julia servers, missing Plots | kept; one related observation noted | Phase A showed a noisy stderr stream from a Julia environment; not investigated |
| Banner text (ticked), push-over-existing (ticked), command 1 values (ticked) | ticked, kept | |
| Not run in any window | kept | |
| Edited cell loses highlighting | kept; wording shortened | the earlier `q2 render` comparison is retained |
| Command 1 on Julia and R spans | kept | |
| "Command 2 does not change Positron's kernel state: say it in the user text" | merged into the run-guide limits box | not deleted |

### Item 4

Old: 9 boxes. Four ticked boxes (Phase E; re-read the intent; fix the epic's closing reconciliation; fix the stale
Julia lines) and the stamped-cells box became three ticked lines; the epic's reconciliation and the Julia lines were
re-checked (no "never started" in the epic; `b4d827186` is cited in the execution-records epic and in
`exr-08-julia.md`). Open boxes kept: hub verification, embedded-docs refresh (now notes that Items 8 and 9 stale it
again), RIU-3 browser items (flag renamed), decision 12 re-verification, the stale comment at `options.rs:237`
(re-checked: still stale; a note says the one-presenter work touches the file).

### Item 5

The ticked "decide whether 3b is wanted" moved to Gordon's decisions; the ticked re-execute test became a prose line
(checked present at `capture_driver.rs:3194`). The two open boxes (EXR-16 sweep, labeled `fig-cap` splice case) are
unchanged.

### Items 6 to 9

- Item 6: all boxes kept; a sentence says Item 8 deletes `commands/record.rs` and its test file, so those entries
  move to the convert command.
- Item 7: now a pointer to the epic; the second box (pre-review spikes) is ticked.
- Item 8: boxes kept; the fork-extension box gained one fact: the fork's e2e `warmJuliaServer` also calls
  `record export-positron` (seen at the top of `quarto-render-and-push.test.ts`). The docs box no longer carries
  its own embedded-docs refresh (it is in the Order section). Nothing weakened.
- Item 9: boxes kept; the docs box no longer carries its own refresh; the count of matching lines in `main.rs` was
  re-checked (9).

## One-presenter-path epic

| Old section | Now | Why |
|---|---|---|
| Goal | kept, plus a "scope the spikes force us to state" paragraph | the spikes showed plain render bypasses the presenter for Julia and knitr |
| What is known | merged into "What the spikes found", with S1 to S5 numbers | one list instead of two |
| "Julia already goes through the Rust presenter" (a bullet) | corrected to conditional on a record being wanted | S4: `core_presents` needs `want_record`; a plain render has none |
| Open questions 1 to 6 | kept, sharpened with evidence; question 5 marked answered; question 7 added (does the epic cover plain render?) | |
| "Spikes (approved)" | replaced by "What the spikes found"; remaining unknowns listed under "Still to investigate" | |
| "Needs discussion first, not a spike" | folded into the intro of the questions | |
| Plans | kept as "none yet", with likely areas and evidence for ordering | left for the review session, per the instructions |
| Exit criteria | unchanged ("to be written in the review session") | left for the review session |

## What I did not change

Gordon's recorded decisions are carried over as written (forward-slash default, kernel equivalence on the import
side only, the temporary image rule, `q2 convert` replacing `q2 record`, `--execution-records`, command 1 live-only,
documenting the Viewer's big X, the Phase A gate). The Phase A verdict and the command-2 progress, cancel and
dirty-wording decisions are left as open boxes. I implemented nothing, opened no braid strand, and pushed nothing.
The fork has one new untracked file, `test/e2e/tests/quarto/q2-raw-record-spike.test.ts`; I made no commit there.
The throwaway branch `spike/one-presenter-s1-s3` (`62afa5177`) exists locally only.
