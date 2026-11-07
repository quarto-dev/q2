---
title: 'KNR interlude: four spikes before KNR-2 to KNR-6'
date: 2026-11-01
description: 'Four spikes (presenter gap sizing, knitr replay and trace, structured records through preview with real sizes, and Positron reading and producing knitr records) that answer the open questions of the later knitr plans while the parent epic finishes.'
status: complete  # 2026-11-01; findings are in the I1 to I4 notes and the plans they informed
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (an interlude between KNR-1
and KNR-2; it is not a numbered plan and builds nothing for the product).
**Depends on:** [KNR-1](2026-10-30-knr-01-capture-spike-and-harness.md) (complete: the corpus,
the harness, the baseline), the parent plans that are complete on `feature/execution-records-epic`
(EXR-1 to EXR-7, EXR-10, EXR-13, EXR-14; read 2026-11-01), and for the Positron spike the
worktree `/Users/gordon/src/positron-spike-knr5`.
**Feeds:** KNR-2 (estimate and phase order), KNR-3 (fallback rate, record size, bd-sr73),
KNR-5 (scope), KNR-6 (decision 2 and the trace shape).
**Estimated sessions:** 4 to 6 (about one per spike; I1 may take two).

## Goal

KNR-3 decision 8 was settled in one afternoon by running the parent's real stage against a
hand-built record (the spikes in [KNR-3](2026-10-30-knr-03-record-adapter-hash-fallback.md),
decision 8). The later plans carry other questions of that kind, and most of them can be
answered now, because the code they test is either ours (the harness and the corpus) or
complete in the parent epic. This interlude answers the ones that change a later plan, writes
the answers into those plans, and leaves the plans that depend on unfinished parent work
(EXR-11, EXR-12, EXR-15, EXR-16) untouched.

## Ground rules

- Findings only. A spike writes a dated findings note under `claude-notes/research/` and
  updates the plan it informs. Scratch code lives on a spike branch or in the scratchpad and is
  not merged. A spike that wants a product change reports it and stops.
- Mark each claim as verified (file and line, or a log path) or "reported, not re-run".
- Never push. Do not edit the parent plans or the parent epic; do not change the live render
  path or the existing overrides in `patch.R`.
- A spike that overturns a settled epic decision stops and tells Gordon before anything else.
- The Positron spike also follows EXR-16's ground rules (below).

## Which decision each spike informs

| Spike | Question | Plan and decision it informs | Needs |
|---|---|---|---|
| I1 | How many of the 154 blockers does each presenter fix close, in what order? | KNR-2 estimate (5 to 10 sessions), phase order, which blockers become intentional divergences | the corpus and harness (done) |
| I2 | Does a knitr trace replay, and what must the knitr engine put in a trace record? | KNR-6 decision 2 and its phases; where the fix for bd-js24dold belongs (KNR-3 or KNR-6) | EXR-6 and the replay engine (complete) |
| I3 | Do structured knitr records survive the preview path, how big are they with real figures, and how often does a chunk fall back? | KNR-3 fallback rate (set by Gordon from this report), bd-sr73 (figures inline or as files), KNR-3 exit criteria | EXR-4, EXR-7, EXR-14 (complete) |
| I4 | What does Positron do with a q2 knitr record, and can an Ark cache be imported into a knitr unit? | KNR-5 scope and whether it is worth doing; the fallback unit's MIME | Gordon's Positron fork; Gordon to run the IDE |

## Order

I1 first: it is the largest unknown and needs nothing from anyone. I2 and I3 are independent
of each other and of I1 and can run in either order. I4 runs when Gordon has time to drive
Positron; its environment is built in the background now. Each spike ends with its findings
note and the edits below, so that a later spike starts from the corrected plans.

## I1: sizing the presenter gaps (for KNR-2)

**Question.** The corpus report names six presenter gaps (interleaving, options as attributes,
hook-equivalent text, `text/markdown` display data, error text, widget wrapper class) behind 154
blockers, 78 of which also have interleaved output that masks other causes. KNR-2 is estimated
at 5 to 10 sessions. How many blockers does each fix close, and what is left?

**Method.**

1. Tag every blocker with its primary cause automatically from the manifest (the swept option
   or the document), so the cause table is a program output, not a reading of diffs. Several
   causes per case are allowed; report the table both ways.
2. Branch `spike/knr-i1` off the epic branch. Apply fixes to the presenter in the order the
   report suggests, one at a time, re-running `corpus_report` after each (the runner already
   takes `KNR_ONLY` and `KNR_SAMPLE`, so a partial run is cheap):
   (a) interleaving, using the `quarto_output_anchors` cell key (named in the S2 section of the
   KNR-1 plan);
   (b) `text/markdown` display data;
   (c) options as cell and fence attributes;
   (d) the text hooks (numeric `echo`, `prompt`, `comment`, `fenced.echo`, `results`);
   (e) error text and the widget wrapper class.
   The prototypes are rough on purpose; the point is the count they close, and what each one
   touches in the presenter that Jupyter also uses.
3. For each fix, record: cases closed, cases opened (a Jupyter regression shows in the
   parent's presenter tests, which run unchanged), and the lines of presenter touched.
4. List the residual blockers and classify each as a gap to close, an intentional divergence
   (with a reason, as the blockquote case in the report), or Quarto 1 behavior the live path
   does not have either (compare with the Quarto 1 column, `KNR_Q1=1`).

**Outputs.** A table of fix, cases closed and residual after each step; a revised KNR-2
estimate in sessions with its range; the candidate allowlist of intentional divergences for the
runner's allowlist reader; a recommendation on phase order for KNR-2.

**Decides.** Whether the 5 to 10 session range narrows, whether KNR-2 should be split, and
whether any gap is better closed in the capture (R side) than in the presenter.

**Cannot answer.** Whether a prototype that closes a case is a clean change in the shared
presenter. That is KNR-2's work; this spike only shows that the output can be reached.

**Exit.** The findings note, the table in the epic's KNR-2 row, and the branch kept unmerged.

- [x] Cause table generated from the manifest (program output).
- [x] Fixes (a) to (e) prototyped in order with a `corpus_report` run after each.
- [x] Residual classified; divergence candidates listed (the Quarto 1 column was not re-run for the residual).
- [x] KNR-2 plan and the epic row revised; findings note written
  ([findings](../research/2026-11-01-knr-i1-presenter-gap-sizing.md)).

## I2: knitr traces, replay and the record in a trace (for KNR-6)

**Question.** bd-js24dold reports that `ReplayEngine` claims only a language equal to the engine
name, so `{r}` chunks of a knitr trace are not claimed and `--replay` renders unexecuted code
with no error (found in KNR-1 S6; read from `engine/replay.rs:107-113`, reproduced then). What
must change for a knitr trace to replay, what must the knitr engine put in a trace for a record
to replay structured, and does the Jupyter pattern from EXR-6 carry over?

**Method.**

1. Re-run the S6 repro on the epic branch (now rebased onto the parent): trace a small knitr
   document, replay it, show the unexecuted result. Record the exact commands.
2. Read how a Jupyter trace carries `record` (the replay doc says the capture carries it, and
   EXR-5 made the observer always ask) and how `ReplayEngine` presents from it (EXR-6 Phase 3b).
3. On a scratch branch, make the minimal change that lets a knitr trace replay (claim by the
   recorded owned languages instead of the engine name) and show the old trace replays through
   the pass-through presenter as KNR-6 decision 2 says. Then hand-build a trace whose capture
   carries a structured knitr record (the shape from the KNR-3 spike) and replay it.
4. Check the hard-fail policy: a trace with no claimed language must fail loudly, as the replay
   doc says.

**Outputs.** Whether the replay fix is a few lines (then it can land with KNR-3 as a standalone
fix, under the strand) or part of the flip (KNR-6); the trace fields the knitr engine must fill;
whether KNR-6 decision 2 holds as written; a revised phase list for KNR-6.

**Cannot answer.** How hub consumers (EXR-15, draft) read a knitr record. That waits for EXR-15.

**Exit.** The findings note; bd-js24dold updated with the minimal fix and where it belongs;
KNR-6 revised.

- [x] S6 repro re-run on the rebased branch.
- [x] Minimal replay change prototyped (branch `spike/knr-i2`); old trace replays through the real binary, and a structured knitr record replays in a scratch test (knitr-specific units wait for I3).
- [x] Hard-fail case checked.
- [x] KNR-6 and bd-js24dold updated; findings note written
  ([findings](../research/2026-11-01-knr-i2-replay-and-trace.md)).

## I3: structured records through preview, real sizes and the fallback rate (for KNR-3)

**Question.** Three things KNR-3 needs numbers for and has none: (a) does a structured knitr
record present correctly in the preview splice (EXR-14, complete); (b) how large is a record
built from real captures, with figures inline as base64 (EXR-4 measured a hand-built knitr
case and said to re-measure; bd-sr73 asks whether figures go inline or as files); (c) how many
chunks of the corpus would be fallback units under KNR-3's table.

**Method.**

1. Build records from the corpus captures with the adapter shape proven in the KNR-3 spike
   (public API only: `hash_unit` with a profile, flat keys including `quarto-output-format`,
   `hash_document`, `write_record`). Throwaway code in a test, not the product adapter.
2. Sizes: per fixture, record bytes, figure bytes inline, and the ratio to the oracle markdown
   and to the figure files on disk. Report the distribution, and the largest fixture.
3. Fallback: apply KNR-3's fallback table (the per-row triggers: hooks that change output,
   `child=`, non-R engines, `dev="pdf"`, and the rest) to every chunk of every case and report
   the rate per fixture and overall. This is the number the plan says Gordon sets the
   acceptable rate from.
4. Preview: run a structured record, and a mixed one with a fallback unit, through the record
   splice and the preview path (`engine/preview_record.rs`, `stage/stages/record_splice.rs`)
   with the format key in place, and show what appears. This needs the additive parent edit
   from KNR-3 decision 8 prototyped on a scratch branch; the spike reports whether it is
   enough.

**Outputs.** Sizes and the fallback rate in a table; a recommendation on bd-sr73 for Gordon
(inline or files, with the numbers); confirmation or a correction of the additive edit; a note
on anything in the structured record that the preview path drops.

**Cannot answer.** How the hub transport behaves at these sizes (EXR-13 is complete, but the
hub integration, EXR-15, is a draft).

**Exit.** The findings note; KNR-3's exit criteria and decision 8 updated if the edit needs a
change; bd-sr73 annotated.

- [x] Records built from the corpus; sizes tabulated.
- [x] Fallback rate per fixture computed.
- [x] Structured and mixed records shown through the preview splice (the decision-8 edit prototyped on `spike/knr-i3`; it does not suffice, a variant does).
- [x] KNR-3 and bd-sr73 updated; findings note written
  ([findings](../research/2026-11-01-knr-i3-records-through-preview.md)).

## I4: Positron and knitr records (for KNR-5)

**Question.** KNR-5 widens EXR-16's importer gate to knitr-owned units and presents imported
output through the knitr layer. Its Positron-side facts are mostly unverified. Does Positron
read a q2 knitr record, and what does it do to the file; and can the cache Positron writes
for an R cell be imported into a knitr unit?

**Where.** Gordon's fork, `gordonwoodhull/positron`, in the worktree
`/Users/gordon/src/positron-spike-knr5` on the branch `spike/q2-knr5-positron-knitr`, cut from
the fork's local `main` at `f83aab9485` on 2026-11-01. The EXR-16 spike worktree
(`/Users/gordon/src/positron-spike-s1`, branch `spike/q2-s1-conformance`) is not touched.

**Ground rules (from EXR-16 and KNR-5).** Copy a record file into Positron's cache by hand,
never move it: Positron's cache lookup deletes a file on one matching cell hash. Nothing is
sent to anyone. Findings are kept as vitest cases on the spike branch where the code can be
reached without the IDE. Gordon drives the IDE; the agent prepares files and reads results.

**K-S1: Positron reads a q2 knitr record.** Extend EXR-16 S2 with a record that holds
structured knitr outputs (stream, `display_data` with `image/png` and `text/html`, an error)
and, in the same file, a fallback unit with `text/x-quarto-markdown`. Observe what loads, what
is ignored, what is deleted, and what survives a Positron rewrite. Decides whether the fallback
unit's MIME is harmless to Positron and whether the structured subset is valid there.

**K-S2: an Ark output into a knitr unit.** Gordon runs an R cell in Positron and hands over the
cache file. Import it into a knitr-owned unit, present it through the pass-through presenter
today and the knitr layer later, in `q2 render` and `q2 preview`. Observe plots, data frames,
warnings and errors. Decides whether KNR-5's importer widening is small or needs the KNR-2
presenter first.

**Environment (built in the background; status recorded here).**

- The fork needs Node 24.22.1 (`.nvmrc`; the installed Node is 24.5.0 and the repo's
  preinstall refuses it). The build runs through `npx -y -p node@24.22.1 npm install` so
  that nothing changes on the system and the repo's version check stays on.
- **Result (2026-11-01, run in this worktree; logs in the scratchpad):**
  - `npx -y -p node@24.22.1 npm install` exited 0 (`positron-npm-install.log`). It rewrote nine
    `package-lock.json` files (327 deletions); I restored them with `git checkout`, so the tree
    is clean at `f83aab9485`.
  - `npx vitest run` on `notebookRenderCache.vitest.ts` and `quartoInsertion.vitest.ts`
    passed: 2 files, 12 tests, 2.3 s (`positron-vitest.log`).
  - `npm run build-start` then `npm run build-check` exited 0. The client typecheck
    (`src/tsconfig.json`), the transpile, `watch:typecheck` and every extension reported 0
    errors. The `watch-e2e` watcher reported 4 errors, all missing e2e-only packages (`ncp`,
    `canvas`, `resemblejs`, types for `mkdirp`); it is not needed for the spike.
    `npm run build-stop` stopped the daemons afterward (`positron-build-check.log`).
  - `npm run test:positron:check-ts` does **not** pass: 368 errors here, and 573 in the
    spike-s1 worktree (`positron-check-ts.log`, `positron-check-ts-s1.log`). The largest group
    is a missing `DebugProtocol` namespace (127), so it is an environment baseline, not a
    result of anything done here. Use it only to compare error counts before and after a
    change to a vitest file, not as a gate.
  - Not run: the Electron app, the Core Mocha suite, the full vitest suite (515 files).

**Outputs.** A findings note on what Positron does with each file; the vitest cases kept on the
spike branch; a recommendation on KNR-5's scope (worth doing now, wait for EXR-16, or drop).

**Cannot answer.** Anything that needs a Positron release newer than the fork's `main`.

**Exit.** The findings note; KNR-5 revised; the branch kept unmerged and unpushed.

- [x] Install and build checks pass (results above, 2026-11-01).
- [x] K-S1 files prepared; Gordon's observations recorded.
- [x] K-S2 cache file obtained; import and presentation shown (preview pipeline only; no data frame or warning case).
- [x] KNR-5 revised; findings note written
  ([findings](../research/2026-11-01-knr-i4-positron-and-knitr-records.md)).

## Housekeeping

- [x] KNR-2 still called the format-in-record question open (decision 8 was settled on
  2026-11-01); KNR-4\'s header still says no inline spike exists, though S9 reported.
  Correct both when the first spike touches them.
- [x] After each spike, one line in the epic's plan table says what changed (done for I1 to I4).
- [x] `claude-notes/plans/CURRENT.md` points at this plan while the interlude is active (2026-11-01); repoint it at KNR-2 when that starts.

## Decisions signed off by Gordon (2026-11-01, after I1 and I2)

Findings: [I1](../research/2026-11-01-knr-i1-presenter-gap-sizing.md),
[I2](../research/2026-11-01-knr-i2-replay-and-trace.md). All five recommendations were
accepted as written.

1. **`text/markdown` display data is opt-in through a marker.** *(Amended 2026-11-01 and
   2026-11-02: the marker is the flat, engine-neutral `quarto_markdown_passthrough: true`, and
   `quarto_knitr.raw` is dropped; see epic decision 22. The next sentences are the original
   decision, kept for history.)* The mapper sets
   `metadata.quarto_knitr.asis` on knitr `asis` output; the shared presenter handles
   `text/markdown` only when it is present (and `quarto_knitr.raw` for results that carry no
   wrapper div). The Jupyter presenter keeps dropping markdown, as its goldens require. Not a
   presenter parameter.
2. **KNR-6 decision 2 is refined, not overturned.** An old knitr trace (markdown result, no
   record) replays by returning the stored markdown verbatim, with no presenter. The
   pass-through presenter matters only for fallback units inside a record. The `{r}` claim fix
   (about 20 lines, bd-js24dold) lands on its own before KNR-3.
3. **Two internal cell-metadata keys carry what the presenter cannot see today.** Both start
   with `quarto_`, so `CellMetadata::options()` drops them from user options, as with
   `quarto_output_anchors`. Both belong to KNR-3, tests first, and KNR-2 step 6 depends on them.
   - `quarto_output_kinds`: an array parallel to `outputs`, entries `"message"`, `"warning"` or
     null (meaningful on stderr streams). Set by the mapper from the piece classes. Without
     the key the presenter treats stderr as a warning, as Jupyter does today.
   - `quarto_unknown_options`: the option names that are neither knitr defaults nor Quarto cell
     options, computed by the capture from the knitr that ran (`names(knitr::opts_chunk$get())`,
     so it follows the version) and shipped per chunk as `unknown_options`; the mapper stores
     it, and the presenter writes those options as cell attributes. Not a hard-coded list.
   - Cost: about half a session of KNR-3's 3 to 4 (an estimate). The `capture.R` change is
     checked on knitr 1.44 and the latest, as in KNR-1.
4. **The candidate allowlist of intentional divergences is accepted** (KNR-2 adds the reader
   with its first entry): `structure/chunk_in_blockquote` and `s7/01_structure` (the baseline
   form keeps blockquote continuation lines quoted), and `markdown-stress/empty_output` and
   `s1/03_eval_false` (an empty `.cell` wrapper is still a box in the page). The
   `knit_hooks_override` case becomes a fallback rule (a document that overrides the `source`
   or `output` hook becomes a fallback unit), not an allowlist entry.
5. **No new strand for the replay footgun.** Replaying a document with `trace: true`
   overwrites the trace being replayed; it is recorded on bd-js24dold, with the proposed
   unused-capture check (KNR-6 Phase 6B).

Both open refinements were settled by Gordon after I3 (see the next section).

## Decisions signed off by Gordon (2026-11-01, after I3)

Findings: [I3](../research/2026-11-01-knr-i3-records-through-preview.md). All six
recommendations were accepted as written.

1. **Known option names live once per record**, not per cell. The presenter subtracts them from
   the chunk\'s current options, so a header edit that adds an unknown option shows without a
   re-run. This replaces the per-cell `quarto_unknown_options` key of decision 3 above.
2. **Logical `warning` and `message` do not drop stderr in a knitr cell** (R enforced them at
   capture). `quarto_output_kinds` is needed only for `class.message`, `class.warning` and
   numeric values. KNR-2 verifies it with a test first.
3. **The format flat key is `quarto-output-format`**, holding the base format identifier.
4. **The parent edit for KNR-3 decision 8 takes the per-profile alternative-hash shape**
   (`UnitKey.alt_exec_hashes`; pass 2 skips cells that name a profile), not the id lookup.
5. **bd-sr73: figures inline as base64** in the cell output.
6. **Fallback rate: no numeric threshold.** The four seen constructs are accepted fallbacks;
   the rate is reported on any new corpus.

## What the spikes do not settle

- Anything that depends on EXR-11, EXR-12, EXR-15 or EXR-16 once they land. Their drafts may
  change, and a spike on a draft proves little. The interlude tests only complete parent code,
  our own harness, and Positron itself.
- Whether Gordon relaxes epic decision 1 (the parent lands first); the interlude works either
  way because it writes findings, not product code.
