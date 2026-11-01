---
title: 'KNR-5: Positron interop for knitr records'
date: 2026-10-30
description: 'Makes a q2 knitr record valid to present in Positron and lets q2 import a Positron cache for R cells into knitr-owned units, by widening the EXR-16 importer gate, presenting imported outputs through the knitr layer, and extending the Positron fork spikes.'
status: draft  # revised 2026-11-01 after interlude I4 (K-S1 and K-S2 observed); EXR-16 is now complete; the wiring waits for KNR-3
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (plan KNR-5).
**Depends on:** [KNR-3](2026-10-30-knr-03-record-adapter-hash-fallback.md) (structured knitr
units and hashes), [KNR-2](2026-10-30-knr-02-knitr-presenter-layer.md) (the presenter that
shows imported outputs), and [EXR-16](2026-10-28-exr-16-positron-interop.md) (hard: its
importer, fixtures and nbformat validator are the base this plan modifies; its Positron
fork spikes S0 to S6 are the base for K-S1 and K-S2). To show imported output in
`q2 render` or `q2 preview` without R, the consumer path of
[EXR-14](2026-10-28-exr-14-preview-splice.md) (the splice) must exist (a soft dependency:
the importer and presenter tests do not need it).
**Blocks:** nothing hard. KNR-6 and KNR-7 (docs) take it as soft (the epic\'s table).
**Estimated sessions:** about 1 after EXR-16 lands (interlude I4 ran the spikes; was 2 to 3), plus any spike time in Gordon\'s Positron fork. The spikes (Phase
5C) gate only the exit criteria below; the importer, presenter and validator tasks do not wait
for them, and Gordon schedules the fork time.

> **Status of this document.** Written 2026-10-30 and revised 2026-10-31 after the paper
> review. The Positron and Ark facts come from EXR-16\'s background (verified there on
> 2026-10-28 and 2026-10-30 by reading `external-sources/positron` and
> `external-sources/ark`; nothing was run against a real Positron). Whether a dev build of
> Positron has the Quarto feature active is not verified.

## Interlude I4 findings (2026-11-01)

Findings: [I4](../research/2026-11-01-knr-i4-positron-and-knitr-records.md). Recommendation:
**wait for EXR-16 and fold the gate widening into it.**

- **K-S1 (done).** A q2 knitr record loads in Positron and its outputs present; the structured
  subset needs no change. The fallback unit\'s `text/x-quarto-markdown` loads and shows as raw
  text, harmless, so the MIME stays. Positron consumes a hand-copied record on one matching hash
  and rewrites it without any q2 notebook-level or cell-level key (output-level metadata
  survives). A text/html or text/plain nbformat line array is mangled on load; q2\'s mapper emits
  strings, so a q2 record is safe.
- **K-S2 (done, partly).** An Ark cache imported into knitr units and presented through the
  shared presenter: plot, text, error and a plain result all appear. The current gate refuses
  every knitr cell; widening it is one condition, and it must also refuse fallback units (the
  spike filled one, so that is a required test). A fallback unit is one whose recorded output
  carries the `text/x-quarto-markdown` mime, which is the only fallback marker (epic decision
  8). `outputs_for` cannot see that, so the caller computes it and passes a `fallback: bool`
  that makes `outputs_for` refuse. The second figure mode already exists in the
  shared presenter. Three presenter limits: ANSI escapes and an empty `ename` in Ark errors, the
  message arriving on stdout (no stderr class), and image width and height metadata ignored.
  Not observed: a data frame, a warning, `q2 render` through the binary, a browser preview.
- **Nothing called the importer when interlude I4 ran** (grep), so Phase 5B had no call site then. EXR-16 is complete now; re-check for a call site before Phase 5B.

## Goal

The bar is **valid to present** (epic decision 6). It is checked, not delivered, in both
directions:

1. **q2 to Positron.** A q2 knitr record is valid nbformat that Positron\'s cache service
   can load and present for matching cells **when placed by hand in its cache directory**
   (spike K-S1). This plan builds no path that delivers a record there: the record lives
   under `.quarto/` and is never shared with Positron (parent decisions 3 and 14).
2. **Positron to q2.** A Positron-written cache can supply outputs for `{r}` cells that
   knitr owns, and q2 presents them through the knitr presenter layer as display-only
   output, with the limits listed below.

Equal-to-what-the-other-would-produce is not required.

## Background

**What EXR-16 already settles (parent epic; do not re-decide here).**
- Two files; q2 never writes into Positron's cache directory (spikes do, by hand).
- Matching metadata is Positron's: `quarto_content_hash` (SHA-1 of the raw lines between the
  fences, `#|` lines included, first 16 hex), `quarto_cell_id` (`{index}-{hash8}-{label|unlabeled}`),
  `quarto_label`; `quarto_cache_version: 1` stays only while the subset is compatible.
- Positron-written records are display-only: never reuse, never freeze.
- The importer **normalizes at read**: `execute_result` accepted where q2 has `display_data`,
  string-valued JSON mimes parsed to objects, `quarto_webview_metadata` ignored.
- **The language gate** (EXR-16 decision 7): import only into units whose owner is the
  Jupyter engine and whose token is a plain kernel language, because Positron records no
  language per cell.
- The Positron items are spikes in Gordon's fork `gordonwoodhull/positron`, not
  proposals; nothing is sent to anyone.

**What this epic changes for them.**
- *The gate must widen.* A Positron R cell is a `{r}` fence. After this epic, knitr owns
  `{r}` by default, so EXR-16's gate (Jupyter-owned only) would never fire for R in a
  default document. Widen it to: owner Jupyter with a plain kernel language, **or** owner
  knitr with token `r` (the exact token, not `r.something`).
- *Outputs from Ark are not knitr outputs.* They are kernel messages: `stream`,
  `execute_result` and `display_data` with `text/plain`, `text/html`, `image/png`, and
  errors. Ark takes `fig-width` and `fig-height` from the execute request (EXR-4, citing
  `ark/crates/amalthea/src/wire/execute_request.rs:49-54`), not knitr's `dev`, `dpi` or `fig.path`.
  They carry no file paths, no `fig.cap`, no knitr label placeholders.
- *The knitr layer must present outputs with no knitr provenance.* An imported unit has no
  recorded resolved options, so the layering of epic decision 11 has nothing under the
  chunk\'s current header options: options that drive presentation (`echo`, `output`,
  `fig.cap`, `layout`, `label`, classes) come from the *current document source*, including the document scope (`execute:`), as
  they do for Jupyter cells; KNR-3 decision 3 (no scope layering) is about records that
  carry resolved options, which imported units do not. Known limit: options a setup chunk sets with `opts_chunk$set` are not
  applied to imported output; K-S2 shows an example. Images arrive as base64 and need the
  figure sink (naming `cell-<n>-output-<m>`), so the presenter needs both figure modes: outputs
  that carry `quarto_file_path` (knitr\'s) keep that path from the files map, and writes from
  inline data get the sink\'s generated name (epic decision 25).
- *Hash agreement.* Positron matches on the content hash of the author's source. knitr's
  label re-injection rewrites chunk headers at the engine boundary (EXR-10); the record's
  `quarto_content_hash` must be over the author's text (parent decision 18), or a Positron
  cache and a q2 record would never match on labeled chunks.

## Design decisions (from the epic; do not re-litigate)

1. Valid to present, two ways; display-only for imported outputs (epic decision 6).
2. knitr stays whole-document; imported outputs never feed reuse (decision 7).
3. Fallback units are opaque to Positron and use the all-options hash (decision 8): a
   Positron record never fills a fallback unit's slot, and q2 never imports into one.

## Provisional: decisions this plan must make

1. **Widening the gate**, exactly as above, and the test that documents its limit (a
   Positron cell and a q2 cell with identical text but different language still collide;
   the occurrence rule and owner check are the only protection, as in EXR-16).
2. **Which imported outputs to accept for R.** Proposal: everything the importer already
   normalizes, plus `text/html` and `image/png`; reject any output whose mime the presenter
   cannot show, with a logged notice rather than an error. Imported `text/html` for R
   carries no htmlwidget dependencies (the record has no `knit_meta`), so an imported widget
   renders without them; the plan documents this limit and K-S2 checks what Ark sends.
3. **How the presenter treats imported plots** (the second figure mode above), whether an
   imported record\'s files go into the same `quarto_files` map or only into the render\'s
   output directory, and **how q2 records figures for Positron**: parent decision 3 says
   figures stay inline as base64, while KNR-1 S2 carries knitr file paths through a files
   map. A record with only a path cannot show an image in Positron; K-S1 records whether
   Positron needs inline image data, and the answer is decided with bd-sr73 (the epic\'s open
   question on figures).
4. **Whether to record, for a record q2 writes, an `execution_count`** (kept `null` in
   EXR-4) so Positron orders cells sensibly; decided from KNR-1\'s S2 table and the K-S1 observation
   in Gordon\'s fork.
5. **The `{r}`-through-Jupyter-kernel alternative.** The owned-languages epic lets a
   document route `{r}` to a Jupyter kernel (ark or IRkernel), where interop already works
   by EXR-16's existing gate. Document which route a user should pick for which goal; do not
   build more here.

## Out of scope

- Production Positron code; contributing anything to Positron or Ark.
- Reuse or freeze of imported outputs.
- Positron executing knitr, or execution of inline expressions in Positron (EXR-16 S3).

## Phases (provisional)

### Phase 5A: q2 side, tests first

- [ ] Importer tests over a real Positron-shaped file: an R cell with `stream`, `execute_result`
  (`text/plain`, `text/html`), `display_data` (`image/png`), and an error fills a knitr-owned
  `{r}` unit; the same file does nothing for a unit owned by another engine or for a fallback
  unit.
- [ ] Presenter tests: imported outputs present through the knitr layer using current document
  options; image outputs write through the figure sink; unknown mimes are skipped with a notice.
- [ ] A q2 knitr record carries `quarto_output_anchors`, `quarto_output_kinds`,
  `quarto_output_kinds` on its cells, `known_options` on its `quarto_engines` entry, and
  `quarto_markdown_passthrough` on `knit_asis` output (KNR-3).
  Positron must read the record without them mattering; interlude I4 (K-S1) checks this, and
  this item is the test of its result.
- [ ] Round-trip: a q2 knitr record passes the nbformat validator (EXR-4 / EXR-16 Phase A) and
  the Positron-shaped checks (`nbformat`, `cells`, `metadata.quarto_cache_version: 1`, each code
  cell has `outputs`).
- [ ] Hash agreement tests: a labeled chunk (record `quarto_content_hash` equals the SHA-1
  vector for the author\'s text, not the re-injected text); a chunk with a `#| label:` line,
  which Positron cannot see (parent decision 39: matching is by hash, and the label segment
  of the id may differ); a chunk in a blockquote, a list and a four-backtick fence, where
  Positron\'s parser has no nesting rule and its indices differ; and CRLF input.

### Phase 5B: implementation

- [ ] Widen the importer gate; add the second figure mode; wire the knitr layer to imported
  outputs.
- [ ] Shared fixtures with EXR-16 S1 (conformance) extended with an R cell and a labeled chunk.

### Phase 5C: Positron spikes (in Gordon's fork; follow EXR-16's ground rules)

- [x] **K-S1, Positron reads a q2 knitr record** (done in interlude I4, 2026-11-01). Extend EXR-16 S2: a record with structured
  knitr outputs (stream, `display_data` with `image/png` and `text/html`, an error) and, in the
  same file, a fallback unit with `text/x-quarto-markdown`. Copy the file by hand, never move
  it (Positron\'s cache lookup deletes a file on one matching cell hash, EXR-16 background).
  Observe what loads, what is ignored, what is deleted and what survives a Positron rewrite.
- [x] **K-S2, Ark output into a q2 knitr unit** (done in interlude I4, except a data frame, a warning and the real binary; see the findings). Run an R cell in Positron, take its cache
  file, import into a knitr-owned unit, present through the knitr layer in `q2 render` and
  `q2 preview`. Observe plots, data frames, warnings and errors.
- [x] Findings note and the vitest cases on the spike branches (`spike/q2-knr5-positron-knitr`, `spike/knr-i4`). Nothing was sent to anyone.

## Exit criteria

- The importer tests and presenter tests pass; the record passes the validator.
- **K-S1 passes** when a hand-copied q2 knitr record loads in the Positron fork and its
  outputs present for matching cells, and the file survives a Positron rewrite. If Positron
  rejects or deletes it, the plan is done only with the failing check and the minimal record
  change that avoids it written down (a record that Positron discards is a failure, not a
  finding).
- **K-S2 passes** when an Ark-written R cache imports into a knitr-owned unit and presents
  through `q2 render` and `q2 preview` with plots, data frames, warnings and errors
  observed; each case that does not present is recorded with its cause.
- clippy and nextest green; counts against the live baseline.

## Risks

1. **Positron deletes a cache file on any structural surprise, and on one matching cell
   hash** (EXR-16 background; parent decision 39). A knitr record with an unusual output
   shape could be discarded, and any record copied for K-S1 is consumed by the first
   matching cell; K-S1 is the test.
2. **R output differences** (print width, ANSI, data frame rendering, plot size) mean an
   imported cell looks different from a knitr run of the same code. Accepted by the "valid to
   present" bar; document it.
3. **Positron has no language per cell**, so R and Python cells with identical text collide
   (EXR-16 risk, unchanged).

## References

- [EXR-16](2026-10-28-exr-16-positron-interop.md) (background, gate, spikes S0-S6),
  [EXR-4](2026-10-28-exr-04-record-format.md), [EXR-10](2026-10-28-exr-10-knitr-k0-options-map.md)
- `external-sources/positron/src/vs/workbench/contrib/positronQuarto/browser/quartoOutputCacheService.ts`,
  `external-sources/ark`
