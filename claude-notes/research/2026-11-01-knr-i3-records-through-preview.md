---
title: 'KNR interlude I3: structured knitr records through preview, sizes and the fallback rate'
date: 2026-11-01
description: 'What a record built from the knitr corpus captures weighs, how many chunks fall back under the KNR-3 table, and what the preview splice does with a structured and a mixed record. The additive parent edit of KNR-3 decision 8 does not suffice; a variant that does is prototyped.'
---

Date: 2026-11-01. Plan: [the interlude](../plans/2026-11-01-knr-interlude-spikes.md) (I3).
Branch `spike/knr-i3` (cut from `spike/knr-i1`; unmerged, unpushed), worktree `workspace-3`, R 4.3.2
with the system knitr 1.50, macOS arm64. Findings only: the adapter is throwaway test code on the
public record API, not the product adapter.

Every number comes from `corpus_records` runs whose JSON and summary are in the session
scratchpad (`.../scratchpad/i3/records.json` and `.summary.txt`), unless marked "reported, not
re-run". The run used `KNR_R_LIBS` pointing at another session\'s scratch library that holds
RSQLite, so that `s5/01_odd_chunks` runs (the library only adds a package; knitr is the system
one).

## Answer

1. **Preview.** Structured and mixed records splice correctly through the real preview pipeline
   (`render_qmd_to_preview_ast`, empty engine registry): outputs appear, figures are written to
   `doc_files/`, a fallback unit\'s stored markdown is placed, and an `echo: false` edit is
   honored. But **the additive edit of KNR-3 decision 8, as written, does not suffice.** It fails
   for two reasons, both found by running it (below). A variant of about 60 lines in
   `record/matching.rs` plus a call in `splice_units` does suffice, with every existing record,
   splice, replay and presenter test still passing.
2. **Sizes.** The median record is 2.4 KB; figures dominate everything above that. A record with
   figures inline is a median 46 KB and at most 163 KB (7 figures at the default size). Inline
   costs 1.31 times the figure files. Recommendation for bd-sr73: **inline**, below.
3. **Fallback rate.** 8 of 278 cells (2.9%) in the 206 documents that built a record, and one
   more document (9 cells) yields no record at all. That rate is a property of the corpus, which
   is a one-construct-per-document sweep, so it says *which constructs fall back*, not how often
   real documents do.

## Preview: the additive edit does not suffice

Method: tests first in `stage/stages/record_splice.rs` (module `tests`, names `i3_*`), against a
record built the way the KNR-3 spike adapter builds it (profile `structured`, format flat key,
document hash recomputed). Expected red state per KNR-3 Phase 3A: format mismatch reports Fresh,
and a presentation-only edit reports Stale.

Before any edit, three of nine tests failed
(`.../scratchpad/i3/red.log`):

- a structured record built for `docx` and rendered as `html` reports **Fresh** (should be Stale);
- an `echo` edit on a structured unit reports **Stale** (should be Fresh);
- the same `echo` edit with a new cell inserted above reports Stale (my extra test).

I then wrote decision 8 as specified: `UnitKey::from_author_text_for_cell`, which takes the record
cell with the unit\'s id, uses its `quarto_hash_rule` as the profile and adds the current format
under `quarto-output-format`; `splice_units` finds the cell by id. The same three tests **still
failed** (`green.log`). Two causes, each read in the code and confirmed by the failing tests:

1. **The id cannot find the cell after the text changes.** `fill_ids` builds the id as
   `{index}-{hash8}-{label}` where `hash8` is the content hash (`record/ids.rs`, the
   `fill_ids` doc). Any edit to the author text, an `echo` edit included, changes the id, so the
   lookup misses exactly when it is needed; a new cell above shifts the index the same way. A
   label-less or labeled cell alike.
2. **The content-hash pass still matches the cell.** `match_units` runs the exec hash and then a
   second pass on the content hash. When only the format differs, the author text is unchanged, so
   pass 2 matches and the unit is Fresh whatever pass 1 said. A key change alone cannot make the
   format count.

The variant that works (`matching.rs` +65 lines, `record_splice.rs` +7):

- `UnitKey` gains `alt_exec_hashes`: one extra exec hash per profile in the engine\'s stored
  declaration, each computed with the current format under `quarto-output-format`
  (`UnitKey::from_author_text_with_profiles`). No id lookup.
- `match_units` pass 1 tries the default hash and then the alternatives.
- `match_units` pass 2 skips cells that name a profile: their hash is the whole freshness rule.
- The coupling to state: a profile in a knitr declaration implies the format key on its units.

With it, all eight behavior tests pass: format mismatch Stale, same format Fresh, `echo` edit Fresh
and honored (also with a cell inserted above), an execution-relevant edit Stale, a fallback unit\'s
presentation edit Stale (default profile, as decision 9 says), a Jupyter-shaped record unchanged.
Regression run: 349 tests (`test(/record::/) | record_splice | capture_splice | preview_record |
replay | presenter:: | knitr_i3`) all pass, including
`record_preview_splice::record_splice_matches_legacy_splice_on_the_jupyter_fixtures`.

For KNR-3 this changes decision 8: the edit is not "one function called by `splice_units`" but a
field on `UnitKey` and two small changes in `match_units`. Still no wire or schema change, and
cells that name no profile behave as before. It should come with the tests above, plus one for the
content-hash pass skip.

## Preview: real captures

`knitr_i3_preview.rs` runs a three-chunk document (interleaved source and output with a message,
a plot, an `asis_output`) through R, builds the record, and renders it with
`render_qmd_to_preview_ast` and `CapturePayload::Record` (`preview.log`):

| Case | Record | Result |
| --- | --- | --- |
| structured, html | 26,074 bytes, 3 structured | all outputs, figure written to `doc_files/`, asis text present |
| mixed, chunk 2 forced to a fallback unit | 4,050 bytes | structured chunks spliced, the fallback unit\'s stored markdown placed, no figure and no `doc_files/` (the stand-in markdown holds none) |
| structured, chunk 1 edited to `echo: false` | the same record | source of chunk 1 gone, its output kept |
| the same record rendered as docx | the same record | knitr itself ran (its `processing file` lines are in the log) and the output is interleaved: the record was not spliced. Why an empty registry still ran knitr was not traced |

The preview format string is `q2-preview`, but its identifier is `html` (`format.rs:460`), which is
what `ctx.format.identifier` gives the splice. The format flat key must therefore hold the
**base format identifier**, not the format string, or a preview and a render of the same format
would disagree.

**What the preview path drops** (it presents with `render_cell`, not the knitr layer):

- **Interleaving.** The test document has source `y <- x * 2` after the first output. In the
  preview AST the code block holds all source lines (offset 824) and the first output follows it
  (offset 1287). `quarto_output_anchors` is ignored, as KNR-2 is to fix.
- **`quarto_output_kinds`, `quarto_unknown_options`, `class.*`/`attr.*` attributes, `comment`,
  numeric `echo`, `collapse`, `prompt`:** not read by the splice (the I1 prototype reads them in
  `render_cell_interleaved`/`render_cell_attrs`, but `present_cell` calls `render_cell`).
- **Not dropped, on this branch:** `text/markdown` display data marked `quarto_knitr.asis` is
  presented (the I1 marker works through the splice; the asis text is in the AST). On the base
  branch without the I1 presenter, it would be dropped.
- Header-form options (`{r c1, fig.width=3}`): the qmd parser rejects them
  (`i3_qmd_rejects_the_knitr_header_option_form`), so the unit hash, which reads only `#|` lines,
  never misses one. The corpus\'s "header" fixtures are harness-only.

## Sizes

Built cases: 206 of 213 (4 aborted by design, 2 need `gifski`, 1 builds no record; see below).
`size` = `write_record` output; gzip = flate2 default; "figures removed" empties the image
payloads (`image/png`, `image/jpeg`, `image/svg+xml`).

| Per case, bytes | min | p50 | p90 | p99 | max |
| --- | ---: | ---: | ---: | ---: | ---: |
| record | 2,052 | 2,381 | 46,371 | 87,666 | 163,314 |
| record, gzip | 798 | 918 | 23,669 | 46,170 | 82,060 |
| record, figures removed | 1,470 | 1,696 | 2,207 | 6,260 | 40,536 |

- 49 cases have figures (97 figures). Inline figure bytes total 2,081,548; figure files on disk
  (decoded) total 1,582,609: inline is **1.31 times** the files. Average figure: 21,459 bytes
  inline, 16,315 as a file.
- Figure cases: record p50 46,340 bytes; gzip is 0.51 of the raw record (PNG base64 does not
  compress much). No-figure cases (157): p50 2,243 bytes.
- Against the oracle markdown: aggregate 36.1 times with figures, **5.95 times without**; per
  case p50 9.4 times, p90 131 times, max 216 times (the large ratios are tiny documents with a
  figure).
- Largest: `s1/10_fig_keep` (html): 163,314 bytes, 7 figures (157,016 inline, 117,756 as files),
  oracle 988 bytes. Then `s1/16_fig_show` 142,706 (6 figures), `s2/08_figures` 87,666 (4).
  The two largest without a figure are widgets (`s2/09_widget_filters` 41,903 bytes, of which
  40,536 is the `text/html` payload).
- Every record round-trips: all 206 recompute every stored hash from the cell and the stored
  declaration (`recompute_cell_hash`).

### bd-sr73: figures inline or as files

Recommendation: **inline `image/png` as base64 in the cell output**, the nbformat form.

- It is what Jupyter and Positron write and read, so a knitr record needs no special case there.
- The record stays self-contained: the preview splice writes the figure files itself (verified:
  `doc_files/` appears from the record alone).
- The price is 1.31 times the files, and gzip takes a figure record to about half. At the default
  figure size a document with 10 figures is about 0.2 MB inline. bd-sr73's budgets (100 KB
  compressed for CI fixtures, 1 MB for user-attached traces) hold for every corpus case:
  the largest gzips to 82 KB. A document of roughly 50 figures reaches 1 MB (extrapolated from
  the 21 KB average, not measured).
- Two cautions that are not measured: (a) a higher `dpi` or `fig.retina` grows each figure by
  about the square of the factor, so a retina document is several times these numbers; (b) a
  trace may carry each figure twice, once in `engine_capture.files` (the existing
  capture-files mechanism) and once inline in the record. Dedup would need the record to point
  into `files`, which costs the self-containment. Neither needs deciding before KNR-3; KNR-6
  should measure a real trace.

## Fallback rate

Method: for every cell of every case, in order, apply the KNR-3 table to what the capture shows.
`child=` is read from the header (knitr fires no hook for the parent, S5), every other cell is
aligned to a captured chunk by order, and a document whose expected and captured counts differ
produces no record (KNR-3 decision 4). A cell is a fallback unit for: a `source` or `output`
`knit_hooks$set` in its code or any earlier cell (decision 7), a non-R `engine` option, a
`child=` header, an `other` piece, or an image the mapper cannot present (a `pdf` figure).
The target rows never fire: the corpus formats are html, typst, docx and gfm; **gfm is treated
as covered**, as the KNR-3 table allows once the corpus covers it.

| | Cells | Of built cases |
| --- | ---: | ---: |
| structured | 270 | 97.1% |
| fallback | 8 | 2.9% |
| `hook-override` | 2 | `setup-state/knit_hooks_override`: the overriding setup chunk and a later one |
| `non-R-engine` | 4 | `engines/shell_chunk` (bash), and `ojs`, `mermaid`, `dot` in `engines/handled_languages` |
| `unsupported-image` | 2 | `options/dev/*/02_pdf`, one construct in two spellings |
| no record (count mismatch) | 9 | `s5/01_odd_chunks`: 8 cells expected, 9 captured, the extra being the `child=` document\'s inner chunk |

- Including the no-record document as lost, 17 of 287 cells (5.9%).
- Five documents (of 206) hold a fallback unit, and in all five **no cell is structured**: each is
  a single fallback chunk, or the hook-override case where the first chunk already overrides.
- Per group: `engines` 4 of 5, `setup-state` 2 of 10, `options` 2 of 124 (1.6%, both `dev="pdf"`); every other group is 0.
- In the product the `ojs`, `mermaid` and `dot` cells are not knitr\'s (`handled_languages` is
  non-empty there), so the real count of non-R fallbacks in this corpus is the bash chunk only.
- **Anchors key omitted: 0 of 244** structured chunks that have output. The source pieces\' lines
  always add up to the chunk\'s code in the corpus, so the omission rule is a safety net the corpus
  never exercises. (`quarto_output_kinds` had the same length as `outputs` in every chunk.)
- Not exercised by the corpus: the `latex`, `beamer` and `pdf` and other-target rows, level-3
  cache hits (the capture forces the cache off, KNR-1 S4), a hook set after the first chunk of a
  longer document.

The corpus caveat bears on the number Gordon sets. 270 of 278 cells sit in documents with one or
two chunks, and every construct appears about once. A real document is several chunks of
ordinary R, which the table sends to structured, so its fallback rate is likely lower than 2.9%
unless it uses a non-R engine, `child=`, `dev="pdf"` or a `source`/`output` hook, in which case
the table\'s consequence is per chunk (and for a hook override, per rest of document). That is a
judgment, not a measurement.

## Things this spike did not do

- The adapter writes `quarto_unknown_options` as an empty list (the capture does not compute it
  yet) and uses a spike list of presentation-only options, not the S3 list.
- Fallback units hold stand-in markdown (source plus printed text), so their sizes are a proxy.
- Windows cross-check and the wasm32 build were not run (no platform-specific code was added).
- ReplayEngine (I2's branch) was not on this branch; the preview here is the record splice only.
- Record dispatch in the hub path (EXR-15) is out of scope.
