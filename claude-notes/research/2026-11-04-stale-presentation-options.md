---
title: "KNR-3 slice 8 spikes: stale presentation options"
description: "Whether a presentation-only option removed from a knitr chunk header keeps its recorded value, how far the staleness reaches, and whether capture.R can record what a fix needs. Result: real for header removal only; capture can see the header's keys and the no-override values."
---

# Question

The structured knitr record stores the options knitr resolved, the `structured` hash profile
leaves presentation-only options out, and `layer_options` overlays only the chunk\'s current `#|`
header on the record. Two questions came out of the slice 7 plan
([plan](../plans/2026-11-03-presenter-generic-cell-options.md), finding 8) and the
[spikes plan](../plans/2026-11-04-knr-slice-8-stale-options-spikes.md):

1. **S1.** Is the staleness real, and how wide is it?
2. **S2.** Can `capture.R` record what a fix needs?

Every claim is marked **run** (executed; the command is given) or **read** (code only).

# How it was run

- Worktree build, not the `q2` on `PATH`: `cargo run --bin q2 -- render <doc>` for the end-to-end
  probes, and a scratch harness for the in-process cases. knitr 1.50 (**run**: `Rscript -e
  'cat(as.character(packageVersion("knitr")))'`), R 4.3.
- The record is shadow output until KNR-6, so `q2 render` never shows it. The harness
  (`scratchpad/s1_scratch.rs`, built as a throwaway `cargo` example and deleted afterward; no tracked
  file changed) does what the splice will do. It captures a record from document A with the real
  `KnitrEngine`, writes and reads it back, then takes document B (A edited), computes B\'s unit
  keys under the record\'s own stored declaration and structured profile, matches them with
  `match_units` (the splice\'s matcher, **read** in `stage/stages/record_splice.rs`), and presents B
  over A\'s record with `present_document`. It also captures B afresh, as the ground truth. A
  document-level verdict compares `hash_document` for A\'s and B\'s front matter over the same
  unit hashes.
- Not run: the splice stage itself with a stale-and-fresh mix, and anything on knitr older than
  1.44.

# S1: is the staleness real, and how wide?

Presentation shows the options the presenter reads today: `fig-cap` and `echo` (`fig-align` and
`out-width` are recorded but not presented until slice 7, so those rows say what the record holds).

| # | Case | Change A to B | Unit | Document hash | Shown from A\'s record | Fresh run shows | Result |
|---|------|---------------|------|---------------|-----------------------|-----------------|--------|
| 1 | header `fig-cap: "Cap one"` | deleted | fresh | same | caption `Cap one` | no caption | **stale, wrong.** run |
| 2 | header `echo: false` | deleted | fresh | same | code hidden | code shown | **stale, wrong.** run |
| 3 | header `fig-cap` | `One` to `Two` | fresh | same | `Two` | `Two` | correct (control). run |
| 4 | header `fig-align: right` | to `default` | fresh | same | read: `default` replaces the recorded `right` in the overlay, and D2 drops it afterward | none | correct if D1\'s order holds (overlay, then drop). read; the record holds `fig.align: right`, run, and `fig.align: default` with no header, run |
| 5 | `execute: echo: false` | added | fresh | **changed** | n/a | n/a | re-runs; no gap. run (proxy) |
| 6 | `execute: echo` | `false` to `true` | fresh | **changed** | n/a | n/a | re-runs; no gap. run (proxy) |
| 7 | `execute: echo: false` | deleted | fresh | **changed** | n/a | n/a | re-runs; no gap. run (proxy) |
| 8 | `knitr: opts_chunk: echo: false` | deleted | fresh | **changed** | n/a | n/a | re-runs (the declaration hashes the `knitr` key; read `hash_rule.rs`, run proxy); see "Side finding" |
| 9 | `format: html: echo: false` | deleted or added | fresh | same | n/a | n/a | no effect on R, live or recorded: `q2 render` of such a document shows the code. run. Not a staleness route |
| 10 | setup chunk `opts_chunk$set(echo = FALSE)` | to `TRUE` | setup chunk **stale**, next chunk **fresh** | n/a | next chunk shows no code | code shown | the stale unit is the setup chunk; the fresh neighbor is wrong if reused alone. run. Whether one stale unit re-runs the whole knitr document is KNR-6\'s decision (it says knitr is whole-document, **read**); if it does, no gap |
| 11 | fence `{r echo=FALSE}` and `{r, fig.cap=...}` | any | n/a | n/a | n/a | n/a | q2 rejects both with Q-2-36 and no record is made. run (`q2 render`, harness "no record"). Not a route; fence text is also in the hash through `attr_tail` (run: removing `echo=FALSE` leaves the unit unmatched) |
| 12 | `ref.label` chunk | the referenced chunk\'s `echo` edited | fresh | same | the referencing chunk follows its own header | same | no presentation gap, because knitr copies the code and not the options. The code staleness (S8) is out of scope. run |

How to read the table:

- **The gap is real and narrow (run, rows 1 and 2).** A `#|` option deleted from a chunk keeps the
  recorded value, because the unit still matches (the hash leaves the option out) and the overlay
  has nothing to say about a key that is no longer in the header. This holds for any presented
  option, and the record of row 1 shows why: it holds the full resolved set, with the old header
  value in it (`fig.align: right`, `fig.cap: Cap one`, `out.width: 50%`). A *changed* value is fine
  (row 3).
- **Document-level options do not reach the overlay, and do not need to (read and run).**
  `present_knitr_cell` receives only the `#|` header (`presenter/document.rs`; the `scope` argument
  is not passed to it). Every `execute:` key and the `knitr` key are in the knitr document hash, so
  any change to them marks the record stale and re-runs. So there is no document-level staleness
  today, and slice 8 is header-only for the options q2 honors. If a later change moves an
  `execute:` key out of the knitr hash (the way D10 does for Jupyter), this row reopens; the plan
  should say so.
- **The setup-chunk case depends on KNR-6.** KNR-6 already states the intended behavior: "a
  setup-chunk edit does not show until the next run" (Phase 6A tests). Because the setup chunk
  itself goes stale, a whole-document re-run closes the gap. It is a gap only if a stale unit does
  not force a re-run.

## Side finding (not slice 8)

`knitr: opts_chunk:` has **no effect in a real render** (run): a document with
`knitr: opts_chunk: {echo: false, out.width: "40%"}` shows the code, and
`knitr::opts_chunk$get("out.width")` inside a chunk is `NULL`. `fig.pos: "h"` did not apply either.
`format: html: echo: false` is also ignored (row 9). Both are outside every active plan; the user
should decide whether either is a bug (Quarto 1 documents `knitr: opts_chunk`). No strand was
opened.

# S2: what can capture record?

## What a record holds today

- **Run (a dumped record, `KNR_DUMP_RECORDS`):** `quarto_resolved_options` is knitr\'s final resolved
  options for the chunk: 45 or so keys, dotted names, defaults included (`fig.align: default`,
  `fig.env: figure`, `fig.pos: ""`), NULL-valued options absent (so a header-less `fig.cap` is not
  a key). The only values the record loses are the ones `capture_options` drops
  (`capture_option_skip`: `code`, `params.src`, `original.params.src`, `yaml.code`,
  `original.code`, `hash`, `fig.path`, `cache.path`, `fig.cur`; `capture.R` lines 31 to 35).
- **Cannot tell (read):** which keys the header, the setup chunk or `opts_chunk` supplied. It also
  cannot tell a parser-set `indent` from a header one (slice 7, finding 7).

## What capture.R can see at chunk time (run)

Probe: a `.qmd` whose chunks call, from inside the chunk,
`names(attr(knitr::knit_code$get(label), "chunk_opts"))` and, per option, both
`knitr::opts_current$get(k)` and `knitr::opts_chunk$get(k)`. Run through
`cargo run --bin q2 -- render` (so q2\'s own `hooks.R` is in play), knitr 1.50.

| chunk | `chunk_opts` keys | `opts_current` vs `opts_chunk$get` |
|-------|-------------------|-------------------------------------|
| header `fig-cap`, `echo`, `out-width` | `label, fig-cap, echo, out-width` | `fig.cap` Header cap vs NULL; `out.width` 30% vs **70%** (the setup chunk\'s value); `echo` TRUE vs TRUE |
| no header | `label` | every option equal |
| header `fig-align: left` | `label, fig-align` | `fig.align` left vs **center** (the setup chunk\'s value) |

Three facts follow:

1. **The header\'s key set is visible.** `chunk_opts` holds exactly the keys the chunk source
   supplied, in the author\'s spelling (`fig-cap`, `out-width`). The spelling needs the same alias
   rename as D1\'s step 1. With knitr 1.44 or later (`knitr_has_yaml_chunk_options()`) this is
   how `#|` reaches knitr. **Read, not run:** for knitr older than 1.44 q2\'s own hook merges the
   header and sets `options$yaml.code` instead (`hooks.R` 988 to 1006), so the keys would come from
   parsing that text; the repo has no pre-1.44 run to test it on.
2. **The no-override value is `opts_chunk$get(k)` at chunk time.** It carries the setup chunk\'s
   `opts_chunk$set`, the format\'s `opts_chunk` (the `execute: fig-width: 5` case: `fig.width`
   current 5, default 5), and the `opts_chunk$set` made before `knit` in the plain-knitr probe
   (`out.width` 80% for both). It is dotted-named, so the lookup needs the dotted spelling of the
   header key.
3. **Where it does not equal the no-override value (read, from the probe\'s design):** an
   `opts_chunk$set` made by the chunk\'s own code; an option an `opts_hook` rewrites from another
   (none of the presentation options does); and options that are NULL by default (`fig.cap`,
   `fig.scap`, `fig.alt`, `out.height`, `out.extra`, `fig.link`, `fig.subcap`), whose no-override
   value is "absent". The capture should read at the start of evaluation (it already has
   `capture_evaluate_hook`, which runs before `orig(code, ...)`), and an absent default must be
   representable.

## The designs and the outcome

Three designs were priced. The reviews that followed the spike (run, same day) settled it: Gordon
chose (c) on 2026-11-04.

**(a) Record header keys and no-override values; restore them on deletion. Dropped.** Everything
capture needs is visible (see the table above). But `opts_current` and `opts_chunk$get` differ for
options q2\'s own `opts_hooks` derive from other keys: in a `fig-` labeled chunk with `echo: fenced`,
`results` is `hold` versus `markup`, `fig.show` is `hold` versus `asis`, and `fenced.echo` is `TRUE`
versus `FALSE`; `collapse: true` sets `comment` to `##` where the default is `NA`. Deleting a header key
restores only that key, so the derived ones stay stale, and a kept key can change the right
restored value of a deleted one. A per-key round trip passes every single-key test and is wrong.
A blank chunk also has no `chunk_opts` (plain knitr gives `NULL` for a chunk holding only a `#|`
header), so capture cannot record an empty key set for it.

**(b) Hash the set of presentation keys present. Not chosen.** Correct whatever the defaults are, but
every *add* of a presentation key re-runs the document. Cost to existing data (run, `corpus_report`
with `KNR_DUMP_RECORDS`): 263 records, 334 cells, 329 structured; 80 cells carry a presentation key
in their `#|` header, so those hashes would move, and the declaration version goes 2 to 3. No
records are committed under the corpus fixtures (run: `git ls-files`), so nothing needs regenerating.

**(c) Record only the header keys; a unit that lost one is stale. Chosen.** Adds and value changes
still show at once; only a deletion re-runs, and a fresh run gets every derived option right.
It needs no hash change (the new `quarto_` key is dropped by `CellMetadata::options()`) and no
record version bump. Two placement decisions followed (read, `record/builder.rs`, `adapter.rs`,
`record/staleness.rs`, `capture_driver.rs`): the key list comes from the author text, which the knitr
adapter already holds, not from R\'s `chunk_opts` (that has no entry for a blank chunk and none
before knitr 1.44); and the check is one predicate used by the splice (per unit) and by
`record_is_stale` (whole document, which the preview driver calls), because either alone leaves a
level wrong.

Neither design touches document-level options, because they already re-run. That claim rests on the
knitr declaration\'s empty `presentation_execute_keys`, asserted by
`the_declaration_keeps_the_id_and_stores_the_structured_profile` (read).

# What this changes in the plans

- Slice 7 finding 8 and Phase 7.0: stated as a run result (rows 1 and 2); the scratch test becomes
  the Phase 7.0 known-gap test, which slice 8 flips. The document-level, fence and `opts_chunk` rows
  are no longer open.
- Slice 8 is header-only for the options q2 honors; the plan is
  [2026-11-04-knr-slice-8-header-removal.md](../plans/2026-11-04-knr-slice-8-header-removal.md).
- KNR-6: a setup-chunk edit closes through a whole-document re-run only under `EnginePolicy::Auto`
  (read, `capture_driver.rs`); `record_splice.rs` runs no engine.
