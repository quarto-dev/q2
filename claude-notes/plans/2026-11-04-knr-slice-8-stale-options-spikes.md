---
title: 'KNR-3 slice 8 spikes: stale presentation options'
date: 2026-11-04
description: 'Two spikes that decide the shape of KNR-3 slice 8 (a presentation-only option deleted from a knitr chunk header, or set at document level, keeps its recorded value): whether the staleness is real and how far it reaches, and whether capture can record what the fix needs. Findings only; they feed the slice 8 plan.'
status: done  # run 2026-11-04 (knitr 1.50); findings in claude-notes/research/2026-11-04-stale-presentation-options.md
---

**Part of:** [KNR-3](2026-10-30-knr-03-record-adapter-hash-fallback.md), slice 8 (a progress item
there). Epic: [knitr execution records](2026-10-30-knitr-execution-records-epic.md).
**Depends on:** KNR-3 slices 1 to 6 (landed on `knr-2-presenter`). It does not need slice 7
([plan](2026-11-03-presenter-generic-cell-options.md)) to land; only the slice 8 plan itself does
(see "After the spikes").
**Feeds:** the slice 8 plan; slice 7\'s Phase 7.0 header-removal test (S1 produces it) and finding 8.
**Estimated sessions:** 1 to 2.

## Goal

Slice 7 pins a known gap: the structured knitr record stores the options knitr resolved, the hash
excludes presentation-only options, and `layer_options` only overlays the chunk\'s current header on
the record. A presentation option deleted from the header therefore keeps its recorded value, and
nothing re-runs the cell. Today this is shadow output; after KNR-6 flips the live path a user sees
it. Slice 8 fixes it. Two facts decide how, and neither is known:

1. Is it real, and how far does it reach (header only, or also document-level options, fence
   parameters and `opts_chunk`)? So far it is *read*, never run.
2. Can `capture.R` record what a fix needs: which keys the header supplied, and the value knitr
   would use without the override?

## Ground rules

- Findings only (the rule of the [interlude spikes](2026-11-01-knr-interlude-spikes.md)): write a
  dated note under `claude-notes/research/` and change no product code, the hash declaration, the
  sweep or the corpus. Scratch work goes under the session scratchpad.
- Mark every claim **run** (executed; give the command) or **read** (code only).
- Use the worktree\'s own build (`cargo run --bin q2 --`), not the `q2` on `PATH`: an earlier review
  ran the nightly binary on `PATH`, so its worktree claims were read-only.
- R with knitr is needed (`Rscript`); say which knitr version ran.
- No braid strand: this is plan work.

## Spike S1: is the staleness real, and how wide?

Question: for each way a presentation-only option can be supplied, what does the presenter show
after the option is removed or changed, and does the unit read as stale (re-run) or fresh?

Cases (each: render once so the record exists, change the document, render again, read the output
and the unit state), using an option that shows in the markup (`fig-align`, `fig-cap`, `out-width`,
`echo: false`):

- [x] A `#|` header option deleted. A header value changed (control: expected to show at once).
- [x] A header option changed to `default` for `fig-align` (D2\'s sentinel case).
- [x] A document-level option (`execute:` and `format: html:` keys, and `knitr: opts_chunk:`) added,
  changed and deleted.
- [x] An option set by a setup chunk\'s `opts_chunk$set`, then the setup chunk edited.
- [x] An option from a fence parameter (`{r, fig.align="center"}`): real `.qmd` rejects some of
  these with a warning (Q-2-36), so record what is accepted.
- [x] A `ref.label` chunk whose referenced chunk\'s option changes (a pointer to the already
  out-of-scope S8 item; record it, do not size it).
- [x] The Phase 7.0 test of slice 7, written as a scratch test in the scratchpad against the current
  code (build a record with a header option, present it, present it again with the option deleted),
  to confirm the in-process behavior matches the rendered one. Its final form goes into slice 7.

Output: a table of case, supplied by, change, shown value, unit stale or fresh, run or read.
The answer to "how far does it reach" decides whether slice 8 is header-only or the whole category.

## Spike S2: what can capture record?

Question: can the capture, for a presentation-only option that the header (or a fence parameter, or
a document-level key) set, also record the value knitr would use without that override, and the set
of keys the source supplied? Read `capture_known_options` and the chunk hook in
`crates/quarto-core/src/engine/knitr/resources/rmd/capture.R` and `hooks.R` first.

- [x] Dump records with `KNR_DUMP_RECORDS=<dir>` (the corpus report keeps them; see
  `tests/integration/knitr_corpus_record.rs`) for a chunk with a header override, one with a
  setup-chunk default, one with a fence parameter and one with a document-level value. Say which
  keys each record holds and which it cannot distinguish (R cannot tell a parser-set `indent` from
  a header-set one; slice 7 finding 7).
- [x] Test in R, in a scratch script, whether the value `knitr::opts_chunk$get(key)` at chunk start
  equals the no-override value in each case, including when `format_opts_chunk` (ft.shadow,
  `knitr: opts_chunk`) is in play. Report where it does not.
- [x] Record the cost of the two designs the spike can price: (a) additive record fields (the
  header-supplied keys and the fallback values), including whether `quarto_record_version` can stay
  1 and what the adapter, reader and `EngineEntry` must change; (b) hashing the set of
  presentation keys that are present, not their values (an add or a delete then re-runs the cell:
  count how often that fires on the corpus\'s 270 cases and on the smoke tests that run knitr). Do
  not recommend; say which facts favor each.
- [x] Say whether the fix can live in `layer_options` (D1\'s step order from slice 7: rename,
  overlay, drop sentinels, derive `layout-align`) as a fifth step, or needs a different place.

Output: for each design, what it needs, what it breaks (the hash versions, the frozen corpus
records), and what it leaves stale.

## After the spikes

- [x] Findings note: [2026-11-04-stale-presentation-options.md](../research/2026-11-04-stale-presentation-options.md).
- [x] Slice 7\'s finding 8 and Phase 7.0 item rewritten with the measured answer; the scratch
  test is now its Phase 7.0 known-gap test.
- [x] The slice 8 plan, [2026-11-04-knr-slice-8-header-removal.md](2026-11-04-knr-slice-8-header-removal.md),
  written from the findings. Its two-view implementability review is the next step (not yet run).
- [x] KNR-3\'s slice 8 progress item links the new plan.

## Result

S1: real for a deleted `#|` header option only (every other route re-runs, is rejected, or never
reaches R). S2: capture can see the header\'s keys (`chunk_opts`) and the no-override values
(`opts_chunk$get` at chunk start). The two designs are priced in the note; the slice 8 plan is
written for additive fields.

## Exit criteria

S1 and S2 each have a findings table with every claim marked, the slice 8 design choice is
laid out with its measured costs, and slice 7\'s finding 8 states a run result instead of a reading.

## Out of scope

Fixing anything; the `ref.label` staleness (only noted); Jupyter (its options come from the
document on every run, so it does not have this gap).
