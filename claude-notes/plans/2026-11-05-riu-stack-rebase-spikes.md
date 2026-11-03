---
title: 'RIU: spikes after rebasing onto the execution-records and knitr stack'
date: 2026-11-05
description: 'Preliminary plan. Six spikes that settle, by running the rebased branch, the questions the stack review raised about the RIU plans.'
status: done  # S0 to S4 run 2026-11-05 on the rebased branch; results in the findings note; two parts need a person (see the findings)
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Why now:** `feature/records-in-use` was rebased onto `feature/knitr-structured-records`, which sits on
`feature/execution-records-epic`. Two read-only reviews (code drift against the plans' cites, and the stack's
plans against the RIU decisions) found changes the RIU plans were not written against. Several of them are
claims about behavior, and the RIU-0 spikes that supported them ran before the knitr structured records
(KNR-6 6A and 6B, landed 2026-11-05) existed. This plan lists the spikes that settle those claims by running
the rebased branch. It decides nothing: a result that contradicts a settled decision is reported to Gordon
and not resolved here.

## What the reviews found, in short

Need a spike (behavior, not text):

- **Mixed documents with the gate on.** EXR-15 says the gate stays off until every shipped engine builds
  records, because a mixed document loses the second engine's output. RIU-0 S1 B10 said this did not
  reproduce, but it ran before knitr built structured records.
- **Format-dependent knitr records.** The knitr epic makes the output format an input to the structured
  profile's hash. A record written by one render may read as stale to a preview in another format. RIU-4's
  staleness check at the choice of a `q2` source assumes otherwise.
- **The Positron gate now admits knitr `r`** (KNR-5, merged), with a `fallback` flag the caller must supply.
  RIU-3's `record_from_positron` has no such flag.
- **Compile and test state of the rebase.** Two RIU commits touch `capture_driver.rs` and the stack changed
  the same file's tests. Nothing has been built since the rebase.

Text edits only (no spike): line cites shifted in `engine_execution.rs` (about +28), `record_splice.rs`,
`matching.rs` and `capture_driver.rs`; the `UnitKey` struct gained `alt_exec_hashes` and `header_keys`; RIU-3's
"until KNR-5 lands" wording is stale; the test `gate_on_with_an_engineless_record_falls_back_to_legacy` is
not named in RIU-2 although RIU-2 inverts its rule; RIU-1 must say knitr records now exist under the gate.

Four contradictions with the stack's plans and the runbook were found (when the preview flips, mixed documents,
engineless records, reuse of a record file). Gordon answered all four on 2026-11-06; the answers are in the
epic's "Decisions recorded 2026-11-06". Spike S5, added after the answers, found that the mixed-document
worry is real for an engine that builds no record; Gordon decided option A for it (RIU-1).

## Rules for these spikes

- No product code changes. Scratch files and fixtures live in the session scratchpad or under
  `crates/quarto-preview/tests/fixtures/records-in-use/`. A spike that needs product code stops and reports.
- Use the binary built from this worktree, `target/debug/q2`, never the `q2` on PATH.
- Never touch `/Users/gordon/src/positron`. No pushes, no hosted hub.
- Findings go in a research note, one block per spike, with the exact command and the observed output.

## Spike S0: the rebased branch builds and its tests pass

- **Question.** Does the rebase compile, and do the tests that cover what the RIU commits touched still pass?
- **Method.** `cargo build --bin q2`, then `cargo nextest run -p quarto-preview`, `-p quarto-hub-provider`, and
  in `quarto-core` the filters `record`, `capture_files` and `positron`.
- **Reads as.** Green: the rebase is sound. Red: attribute each failure to a stack commit or to a RIU commit
  (`f607ad7d2` as rebased).
- [x] Run and record. Results are in the findings note.

## Spike S1: a mixed document with the gate on and off

- **Question.** With knitr building structured records, does a document with a Jupyter cell and a knitr chunk
  still show every output with `QUARTO_ENGINE_RECORD_WRITE` on, and identically with it off?
- **Method.** The committed fixtures `mixed.qmd`, `knitr.qmd` and `julia.qmd`, plus one added knitr fixture
  with a printed data frame, a plot and an `asis` chunk. Run `q2 preview` with the gate off and on, read the
  rendered page text with Playwright as RIU-0 S1 did, and inspect the stored record's cells and engines.
  Edit one cell in each engine and check that only that cell's output drops.
- **Reads as.** Same pages both ways: B10 stands after KNR-6. A difference: EXR-15's concern is live, and
  the epic's "no special rule for mixed documents" needs Gordon's decision.
- [x] Run and record. Results are in the findings note.

## Spike S2: a knitr record written in one format, read in another

- **Question.** What does `q2 preview` (HTML) do with a knitr record that a docx render wrote with
  `execute: record: true`? Is it read, reported stale, ignored, or does it change the page?
- **Method.** A knitr fixture. `q2 render --to docx` with `record: true`, then `q2 render --to html` and
  `q2 preview`, with the record file in place. Read `recorded_format` in the record and observe the page and
  stderr. Repeat with a Jupyter-only document as the control.
- **Reads as.** If the knitr record reads as stale or is rejected, RIU-4's staleness at choice needs a format
  note and RIU-4's `q2` source needs a test for it. If it is used as is, the plan text stands. The control
  must behave the same under any format.
- [x] Run and record. Results are in the findings note.

## Spike S3: a Positron cache for an R document against the widened gate

- **Question.** How does `outputs_for` treat a knitr-owned `r` unit, and what is the `fallback` flag in
  practice? Which knitr chunk kinds produce a fallback unit (a recorded output with the
  `text/x-quarto-markdown` mime) in a real recorded run?
- **Method.** Run the stack's `knitr_positron_import` tests. Record a knitr fixture with an `asis` chunk, a
  shell chunk, a labeled chunk and a plain chunk, and list each unit's output mimes from the record. Look for
  a real Positron cache for an R document on this machine (read only); use it if one exists, and say so if not.
- **Reads as.** This fixes what RIU-3 must say about the `fallback` argument at conversion time, and which R
  chunks an imported record can never fill.
- **S3b (added 2026-11-06).** Gordon ran the Ark document in Positron; the env-gated `KS2_ARK` test was run on
  that real cache. See the findings note.
- [x] Run and record. Results are in the findings note.

## Spike S4: the engineless record with the gate on, today

- **Question.** What exactly does the current code do with a record that has no engines when the gate is on,
  so that RIU-2's replacement of the "has engines" test by `describes_something` names every test it flips?
- **Method.** Run `gate_on_with_an_engineless_record_falls_back_to_legacy` and list every test in
  `quarto-core` and `quarto-preview` that mentions an engineless record or `quarto_engines.is_empty()`.
- **Reads as.** The list goes into RIU-2 Phase A as the tests to invert, so the change is deliberate.
- [x] Run and record. Results are in the findings note.

## Spike S5: an engine that builds no record, beside one that does

- **Question.** EXR-15 says a mixed document loses the output of an engine that builds no record when the gate is
  on. S1 and RIU-0 B10 never tested that, because every engine they used builds a record. Is it real?
- **Method.** The `echo` fixture engine bundled into a scratch project, a Jupyter cell beside an `echo` cell,
  `q2 preview` with the gate on and off, and an echo-only control.
- **Reads as.** Output lost with the gate on: the epic's "no mixed-document rule" is wrong for such engines and
  the flip needs a decision. Output kept: the epic stands as written.
- [x] Run and record. Output was lost; see the findings note.

## After the spikes

- [x] Write the [findings note](../research/2026-11-05-riu-stack-rebase-spikes-findings.md).
- [x] Apply the text-only edits listed above to the RIU plans, and the spike results where they change a plan
      (RIU-1, RIU-2, RIU-3, RIU-4 and the epic; RIU-5 and RIU-6 needed none).
- [x] Report the contradictions to Gordon; he answered them on 2026-11-06 (epic, decisions of 2026-11-06).
- [x] Render the changed notes with `target/debug/q2 render --strict` and commit. No push.

## Still needs a person

- Nothing for a real Ark cache: it was run on 2026-11-06 (S3b in the findings). It depends on an unpushed base64 fix.
- Whether docx-recorded knitr output presents correctly in an HTML preview: needs RIU-4's `q2` source.
- What reuse, preview without running code, and export do for a document with an engine that builds no record (the diagnostic's hint says "may not work" until this runs).
