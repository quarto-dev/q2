---
title: 'KNR interlude I4: Positron and knitr records'
date: 2026-11-01
description: 'What Positron does with a q2 knitr record (loads it, shows it, consumes it, rewrites it without the q2 keys) and what happens when an Ark-written cache is imported into knitr-owned units and presented through the preview splice. KNR-5 is mostly a small gate change plus presenter limits; the wiring waits for EXR-16.'
---

Date: 2026-11-01. Plan: [the interlude](../plans/2026-11-01-knr-interlude-spikes.md) (I4), for
[KNR-5](../plans/2026-10-30-knr-05-positron-interop.md).
Two spike branches, both unmerged and unpushed:

- Positron fork worktree `/Users/gordon/src/positron-spike-knr5`, branch
  `spike/q2-knr5-positron-knitr`, commit `5d26bc3be5`: `quartoKnitrRecord.vitest.ts` (7 tests).
- q2 worktree `workspace-3`, branch `spike/knr-i4` (cut from `spike/knr-i3`), commit `560973712`:
  `knitr_i4_ark.rs` (an env-gated test; it does nothing unless `KNR_I4_ARK_CACHE` and
  `KNR_I4_QMD` are set).

Gordon drove the IDE. The Positron observations below are his, reported in the session; the
rest is read from code or produced by the tests. Positron build: the fork\'s `main` at
`f83aab9485` (the build or project Gordon ran was not recorded).

## Answer

1. **Positron reads a q2 knitr record.** The structured subset is valid there: every output
   loads and the plot, table, error and html present. A fallback unit
   (`text/x-quarto-markdown`) is harmless to load and shows as raw markdown text, which is noise
   but not breakage. The fallback MIME needs no change.
2. **Positron consumes and rewrites the file.** One matching cell hash loads the whole file, deletes it, and
   writes a new one under the document\'s URI hash with only Positron\'s fixed fields. Every
   notebook-level and cell-level q2 key is gone, so the rewritten file is no longer a q2 record.
   Output-level metadata is kept, so `quarto_knitr` markers and `quarto_format` survive.
3. **An Ark cache imports into knitr units, and the shared presenter shows it.** Plot, text,
   error and a plain result all present, with three presenter limits (below). The current gate
   refuses every knitr cell, so the widening is needed; it is a one-condition change.
4. **KNR-5 recommendation: wait for EXR-16, then do the widening there.** The importer is
   called from nowhere in product code (verified by grep: `import_positron_cache` appears only
   in `positron_import.rs` and its re-export). The widening and the presenter limits are small,
   but without the EXR-16 wiring there is nothing to widen. Details in the last section.

## K-S1: Positron reads a q2 knitr record

### Method

The record is hand-built in the I3 adapter shape (flat keys, `quarto_hash_rule`,
`quarto-output-format`, `quarto_output_anchors`, `quarto_output_kinds`, `quarto_knitr` markers),
with a real 240 by 180 R plot inline as base64. Three cells: a structured cell with stderr,
stdout, a PNG with `text/plain`, and an asis `text/html` plus `text/markdown`; a structured
cell with an error; a fallback unit with `text/x-quarto-markdown`. The vitest builds Positron\'s
real `QuartoOutputCacheService` on an in-memory file service (no `quartoOutputCacheService`
vitest existed, as EXR-16 S2 noted), so the checks run against Positron\'s code, not a copy.

### Code-level results (vitest, 7 passing, `spike/q2-knr5-positron-knitr`)

| Case | Observed |
| --- | --- |
| `loadCache` on the exact path | All 3 cells load; the file is kept. PNG data passes through byte-identical. Stream, error, `text/html`, `text/markdown` map as expected. Output metadata (`quarto_knitr`) is kept as `outputMetadata`. |
| Fallback unit | Loads as one item with mime `text/x-quarto-markdown`. |
| Hash scan with one matching cell, record under an arbitrary name | The whole file loads (all 3 cells, matched or not) and the file is **deleted**. |
| Rewrite after the scan | New file under the document\'s URI hash. Notebook metadata is only `quarto_source`, `quarto_cache_version`, `last_updated`. Cell metadata is only `quarto_cell_id`, `quarto_content_hash`, `quarto_label`. `display_data` becomes `execute_result`. Output-level metadata and the fallback unit\'s mime survive. |
| A cell with no `outputs` array, `loadCache` path | `_ipynbToCachedDocument` throws, the outer handler deletes the file. |
| `quarto_cache_version: 2`, `loadCache` path | File deleted. |
| A cell with no `outputs`, hash-scan path | The file is skipped, not deleted (the inner `catch` continues). |
| No matching hash | File untouched. |
| nbformat line arrays | A stream\'s `text` array is joined correctly. A `text/html` or `text/plain` array becomes `JSON.stringify` of the array (`["<Figure>"]`), not the HTML. |

The line-array row matters only if a record carries arrays. The knitr mapper emits strings
(`pieces_map.rs` on `spike/knr-i3` builds `Value::String`), so a q2-written record is safe; a
record that passes through a tool that splits lines is not. Positron\'s writer emits arrays for
streams and source only, so its own files round-trip.

### IDE observations (Gordon, 2026-11-01)

- Copied `ks1-record.ipynb` into Positron\'s `quarto-inline-outputs/` by hand and opened
  `ks1.qmd`.
- Chunk 1: a table (the stdout text) and the plot. Chunk 2: `Error: boom`. Chunk 3 (the fallback
  unit): the raw markdown, which he read as "-?- raw markdown?".
- On disk afterward (verified): `q2-ks1-record.ipynb` was gone and `78a67421b3b6d9f3.ipynb`
  existed with `quarto_source` pointing at `ks1.qmd`, three cells, and the outputs shapes the
  vitest predicted.
- Not confirmed: whether the stderr line and the asis html showed in chunk 1 (the report named
  the table and plot only).

K-S1 as the plan defines it passes: the hand-copied record loaded, its outputs presented, and
the structured subset needed no change. "Survives a Positron rewrite" holds for the outputs
and not for the record: the rewritten file has lost everything q2 needs, so a Positron rewrite
of a copied record is not a q2 record. That is the design (two files, EXR-16 decision 1), not a
defect.

## K-S2: an Ark output into a knitr unit

### How the file was obtained

Gordon created `ks3.qmd` with the same three chunks as `ks1.qmd`. Because Positron matches by
content hash, it seeded the new notebook from the rewritten K-S1 file (and consumed it); he
then ran the cells, so the cache at `2d70fef49b77f134.ipynb` holds Ark outputs (copied to the
scratchpad `ks2/ark-cache.ipynb`; the `quarto_source` is `ks3.qmd`). Gordon saw the new notebook
show recognizably the same results with UI changes (the plot at full width, different colors),
and the third chunk as a plain `[1] 2`, which is what he expected.

### What Ark wrote (verified, `ks2/ark-cache.ipynb`)

| Cell | Outputs |
| --- | --- |
| 1 (`message`, `print(head(cars, 2))`, `plot(cars)`) | `stream` stdout `hi` (the message arrives on **stdout**, not stderr), `stream` stdout with the table, `execute_result` with `image/png` only (no `text/plain`), image metadata `{"height": 726, "width": 968}` (the plot size follows the editor, not knitr) |
| 2 (`stop("boom")`) | `error`, `ename` empty, `evalue` with ANSI escapes, a two-element traceback |
| 3 (`knit_hooks$set(...)`, `1 + 1`) | `execute_result` with `text/plain` `[1] 2` |

Cell ids are `0-<hash8>-unlabeled` even for the labeled chunk: Positron cannot see the
`#| label:` line (as EXR-16 predicted).

### What q2 did with it (verified, `knitr_i4_ark.rs`, log `ks2/run.log`)

The test runs knitr on `ks3.qmd` through the I3 adapter to get a record with correct hashes,
replaces each cell\'s outputs with what `import_positron_cache` yields for the hash of the
original text, removes the anchors and kinds keys (an imported unit carries none), and renders
the result with `render_qmd_to_preview_ast` and `CapturePayload::Record`.

- **The current gate refuses all 3 knitr cells** (`outputs_for(..., "knitr", "r")` is `None`
  for each), as KNR-5 predicted. Widened to "owner knitr and token exactly `r`" (emulated by
  asking as jupyter), all 3 are filled. No notices from the importer.
- **Preview output.** The `hi` line, the table, the plot, `boom` and `[1] 2` are all in the AST.
  The figure is written to `doc_files/figure-html/cell-1-output-1.png` and referenced with
  class `img-fluid`: the shared presenter already has the second figure mode (write inline
  base64), so KNR-5 does not need to build one.
- **Presenter limits, all in the shared presenter and not specific to knitr:**
  1. The error text keeps the ANSI escapes (`\u001b[1m\u001b[33mError...`), and with an empty
     `ename` it starts with `: `, and the traceback repeats the message in plain text. Ark\'s
     error shape needs ANSI stripping and an empty-`ename` rule. Jupyter kernels send
     escapes in tracebacks too, so this is a presenter fix that serves both.
  2. The stderr class is absent (`cell-output-stderr` is not emitted) because Ark sends
     messages on stdout. Nothing to fix; it is a difference from knitr output to document.
  3. The image metadata width and height (968 by 726) is not used. Not checked whether the
     presenter reads it for Jupyter; the plot shows at its natural size.
- **The `knit_hooks$set` chunk.** The I3 table classifies it as a fallback unit (hook override).
  The widened gate must refuse fallback units (KNR-5 decision 3: q2 never imports into one);
  the spike filled it anyway, so that rule is a required test, not a free consequence.
  `structured 2` in the log is the two non-hook cells.

### Not done or not observed

- **A data frame and a warning.** The test document has `print(head(cars, 2))` (text) and no
  `warning()`, so Ark\'s `text/html` data-frame output and its warning shape were not seen.
  Gordon can add them to a cell if KNR-5 needs them; the importer already normalizes
  `execute_result`.
- **`q2 render` end to end.** The splice stage is shared, but a knitr document with a record
  does not reach it through the binary until KNR-6, so only the preview pipeline was driven (in
  process, with an empty engine registry, as in I3). That is an in-process test, not a run of
  the binary; the record path in `q2 render` for knitr is unverified.
- **`q2 preview` in a browser.** Not run.
- The presenter used is the pass-through one (`render_cell`), not the knitr layer, which does
  not exist yet.

## KNR-5 recommendation

**Wait for EXR-16, then fold the widening into it; keep K-S1 and K-S2 as the evidence.**

- The widening is one condition in `PositronImport::outputs_for`: owner `knitr` with token
  `r`, and never a fallback unit. Both halves need tests first (a knitr cell fills; a fallback
  cell, a `python.marimo` cell and a different owner do not).
- Nothing calls the importer today, so KNR-5\'s Phase 5B ("wire the knitr layer to imported
  outputs") has no call site until EXR-16 lands. Doing it before would be building on a draft.
- Presenting Ark output needs no KNR-2 layer first: the shared presenter handled every output
  kind in the sample. The errors need the ANSI and empty-`ename` fix, which is worth doing in
  the shared presenter for both engines (KNR-2 step 6 is the natural place).
- Phase 5C is done by this interlude except for a data frame and a warning, which are optional.
- The Positron side needs nothing from q2: the record loads, the fallback MIME is harmless, and
  the record is rewritten without q2 keys as designed. The only caution for KNR-5 is that a
  hand-copied record is consumed on its first match.

Cost estimate for what remains of KNR-5: about one session for the gate, tests and the error
fix once EXR-16 has landed (an estimate, down from 2 to 3 because Phase 5C and the second figure
mode are no longer work).
