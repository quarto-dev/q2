---
title: 'Records in use: follow-up, did we build what we intended?'
date: 2026-11-07
description: 'The epic is done. This plan checks the result against the intent, one question at a time, starting with R through knitr (the export skips every knitr cell, so the demo cannot show R), then the loose ends the RIU-6 demo exposed. Each item says what was intended, how it will be checked, and what counts as done.'
status: ready  # written 2026-11-07 after the RIU-6 demo; nothing started
---

**Follows:** [the records-in-use epic](2026-11-02-records-in-use-epic.md) (done).
**Evidence so far:** (RIU-6 commands findings)(../research/2026-11-07-riu-6-commands-findings.md),
(RIU-6 plan)(2026-11-02-riu-06-positron-command-demo.md).
**Related:** [knitr execution-records epic](2026-10-30-knitr-execution-records-epic.md).

> **For implementers:** tests first. Check with `target/debug/q2` built in the checkout, never the
> `q2` on `PATH`. Say which window steps were seen by a person, which ran under Playwright, and which
> were not run. Gates per crate: `cargo clippy -p <crate> --all-targets -- -D warnings` and
> `cargo nextest run -p <crate>`. Nothing is pushed without Gordon's say-so.

## Overview

The epic intended this: a q2 execution record is the one place outputs live, and Positron can read
it, write it, and preview from it, for the languages q2 runs. RIU-6 showed that for Python and Julia.
It also showed that for R through knitr the loop is open on one side: the import direction handles
knitr-owned units (KNR-5), but `q2 record export-positron` skips every knitr cell with
"engine is not jupyter or julia" (`crates/quarto-core/src/record/positron_export.rs`, `SkipReason::EngineNotExportable`).
The epic's decision 11 left R out of the demo; with the knitr stack landed, that is now a gap to
close or to state plainly.

## Item 1: R through knitr (top of the list)

**Intended:** a `.qmd` with `engine: knitr` renders once in q2, and its outputs can be pushed into an
open Positron editor and previewed from Positron's cache, as Python and Julia can.

**Seen so far (CLI, 2026-11-07):** a three-chunk knitr document (`x <- 6 * 7; print(x)`, `plot(1:10)`,
`message()` plus a data frame). `q2 record export-positron` ran the document through knitr, produced a
record, then exited 11 with `{"cells":0,"skipped":[3 x "engine is not jupyter or julia"]}`. R 4.x and
knitr 1.50 are installed. Not run: anything in a Positron window.

### Phase A: characterize (spike, no product code)

- [ ] Read what a knitr record cell holds: where the text output, the figure, the message and the data
      frame go, and how they differ from a Jupyter cell's outputs (KNR-1 to KNR-5; start at
      `record/positron_import.rs` and the knitr-owned-unit import commit `5db29f6bc`).
- [ ] Read how Positron's R side stores and shows outputs for a `.qmd` (Ark kernel, the same
      `quarto-inline-outputs` cache?), and whether the cache loader accepts an R cell with the shapes
      q2 would write. Fork worktree: `/Users/gordon/src/positron-riu6-a`.
- [ ] Check whether the content hash Positron computes for an R cell equals the hash in a knitr record
      (header options such as `#| label` and `#| fig-cap` are the likely mismatch; the Julia `#|` case
      is the same question and is still open).
- [ ] Write the answers into a findings note, with a verdict: the export can cover knitr as is, needs
      a mapping, or needs a change on the Positron side.

### Phase B: tests first

- [ ] Export unit tests for a knitr record: text output, a figure (one `image/png`), a message, a
      data frame, an error, a chunk with options. Each fails on the current skip.
- [ ] A CLI test through the binary: `q2 record export-positron` on an `engine: knitr` fixture exits 0
      and prints cells greater than 0. Keep the Python and Julia tests green.
- [ ] Decide, with Gordon, which knitr cells stay skipped (for example `asis`, raw HTML chunks, inline
      `` `r expr` `` spans) and give each its own skip reason in the JSON.

### Phase C: export

- [ ] Replace `EngineNotExportable` for knitr with a real mapping, per the Phase A verdict. Update the
      subcommand help text and the RIU-5 plan's rule list.
- [ ] Gates on `quarto-core` and the `quarto` binary crate. Run the real binary on the fixture and read
      the JSON and the ipynb.

### Phase D: in a Positron window

- [ ] An R session exists in the dev build (the e2e runs set `POSITRON_R_VER_SEL=4.3.2`; check the
      session starts, as the kernel server and supervisor needed care for Python).
- [ ] An e2e test, R version of `quarto-render-and-push.test.ts`: command 2 on an R document, outputs
      appear; command 3 shows them from the cache with no R process started by the preview.
- [ ] Someone watches it once. Mark each step seen, Playwright, or not run.

**Done when:** an R document shows its knitr outputs in the open editor after command 2, the preview
shows them from the cache, and the findings note says which cell kinds are skipped and why.

## Item 2: the Julia e2e tests are slow and flaky

**Intended:** the demo's tests pass on a clean run. **Seen:** on 2026-11-07 the Julia push tests passed
once (the five-output case and the preview), then timed out at 120 seconds on a second full run
(`.quarto-inline-output` count stayed 0 while waiting for 5). A first run after a Julia server restart
pays startup and precompile time.

- [ ] Find the real cost: time `q2 record export-positron` on the Julia fixture cold and warm; set the
      test timeout from that, not from a guess.
- [ ] Decide whether the test should start from no Julia server, and whether it can stop its own
      (the shared server directory is `~/Library/Caches/quarto/julia`).
- [ ] Re-run the whole file three times and report pass counts.

## Item 3: loose ends from RIU-6

Each is a question for Gordon, not an assumption. Recommendation in parentheses.

- [ ] **Positron's cache location from an API** instead of the sibling-of-`globalStorageUri` guess
      (wrong for non-default profiles). (Needed before anything ships.)
- [ ] **Viewer close button leaves q2 running.** A Positron Viewer change. (Needed before shipping
      command 3.)
- [ ] **Edited cell loses syntax highlighting in the q2 preview** (RIU-3a question). (Check against the
      intent: is the preview meant to be editable?)
- [ ] **Command 1 on Julia and R spans**, and whether values should persist. (Python only so far.)
- [ ] **Not run in any window:** wrong `q2.path`, untitled document, "q2 too old" (exit 2), non-default
      profile, a dirty document. (Run the first three by hand; they are quick.)
- [ ] **Command 2 does not change Positron's kernel state:** a later cell that uses a variable from a
      pushed cell fails. (Say so in user-facing docs if this ships.)

## Item 4: does the epic match the tree

- [ ] RIU-6 Phase E, still open: re-read the epic and RIU-0b to RIU-5, check each `- [ ]` and `- [x]`
      against the tree, and update the epic's table and "Decisions for review". Decision 11 (R) gets
      its outcome from Item 1.
- [ ] Re-read the epic's stated intent once more against what the three commands do, and list anything
      the epic promised that no command or test covers.

## Order

Item 1 first, Phase A alone before committing to B to D. Item 2 next, since it decides whether the
e2e tests can be trusted for Item 1. Items 3 and 4 after, in whatever order Gordon prefers.
