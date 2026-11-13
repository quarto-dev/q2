---
title: 'R through knitr, Phase A: what Positron does with a raw q2 record'
date: 2026-11-07
description: 'Phase A of Item 1 in the RIU follow-up plan. Unfiltered q2 execution records (knitr R, Julia, Python, an inline span) were pushed into open Positron documents under Playwright, and Positron\''s own native run of the same documents was captured for comparison. Verdict, Positron''s validity rules, and what was not checked.'
---

**Plan:** [RIU follow-up epic](../plans/2026-11-08-rfu-epic-riu-follow-up.md) (this was Item 1, Phase A of the single follow-up plan it replaced; the work continues in RFU-2 and RFU-7).
**Status of this note:** findings only. Phase B is not started. The gate stays: Gordon decides after reading this.

How each result was obtained is marked **Playwright** (an automated run against a real Positron window; nobody
watched), **read** (I read the code), or **CLI** (shell). **Nothing here was watched by a person.**

## Verdict

1. **Positron accepts a raw q2 knitr record and shows every output.** The record, exactly as `q2 render` with
   `execute: record: true` writes it (all the `quarto_*` metadata still on it), was pushed with the fork's own
   `_positronQuarto.pushOutputsFromFile`: **8 of 8 cells matched by content hash, 0 skipped**. For R this is a
   Playwright result on a document Positron had open with an R session. So the only reason the export refuses
   knitr cells is the owner rule (`EXPORT_OWNERS`); nothing on the Positron side blocks R.
2. **The content hashes agree for R and for Julia**, including cells with `#|` lines. Positron's own native run of
   the same R document wrote cache cells whose 8 `quarto_content_hash` values are identical to q2's (**CLI** diff
   of the two lists: empty). Positron's own hash for the two Julia cells (one with `#| echo: true`, one with
   `#| label: fig-j`) also matched q2's, because the push matched 2 of 2. This settles the Julia `#|` check.
3. **What Positron shows is what the record holds, not what q2's presenter would show.** Positron has no notion
   of q2's options. That produces four visible differences, all Positron-side behavior, none a q2 bug:
   `results: hide` still shows its output; `results: asis` shows the markdown as text; a raw-HTML chunk shows its
   HTML as text; a Julia plot bundle shows several representations (below). Under the plan's principle ("q2 does
   nothing special for Positron") none is a q2 workaround. Whether any should be one is **Gordon's decision**.
4. **Likeliest outcome for the export:** write the record as a valid cache file and nothing else, with the
   image-representation rule an explicit, temporary decision (already decided: keep it until the fork's view
   zone picks one representation). Open for Gordon: whether the export should apply `results: hide` (see 3).

## What was run

**Method.** One throwaway spike extension in the scratchpad registers `q2spike.pushRaw`, which calls
`_positronQuarto.pushOutputsFromFile` on a record file named by a setting. One throwaway Playwright file in the
fork, `test/e2e/tests/quarto/q2-raw-record-spike.test.ts` (**untracked; I committed nothing to the fork**), opens
each fixture, runs the command, scrolls through the document, and records every inline output (text, image count,
CSS classes) plus a screenshot, and copies the cache files Positron wrote. The same file also ran the R and Python
fixtures natively in Positron (run all cells) for comparison. Six tests, **6 passed (6.0 min)**; the harness ended
with its usual "worker teardown timeout" (already recorded in the commands findings).
Environment: Node 24.22.1 (`/Users/gordon/src/positron-node-24.22.1`), R 4.3.2 with knitr 1.50, Python 3.14.2,
`target/debug/q2` built in this checkout, `POSITRON_R_VER_SEL=4.3.2`. Fixtures were rendered once with
`target/debug/q2 render` (the knitr, Python and span records) and the Julia record from a Julia project with
Plots. Artifacts live in the session scratchpad and are not committed: the four fixtures and records under
`.../scratchpad/fx/`, per-case JSON, screenshots and copied caches under `.../scratchpad/results/`.

| Case (all Playwright) | Push result | What Positron showed |
|---|---|---|
| knitr, 8 chunks | `pushed 8 cell(s): 8 matched, 0 skipped` | see the next table |
| Julia, `1 + 1` and a plot | `2 matched, 0 skipped` | `2`; for the plot cell the DOM held 3 `img` elements (a PNG, an SVG data URI and one with an empty `src`) plus the stderr stream. |
| Python, array / JSON / multi-line print | `3 matched, 0 skipped` | `array([0, 1, 2, 3, 4])`; `{"k":[1,2,3]}<IPython.core.display.JSON object>`; `line1line2line3`. Identical to Positron's own native run of the same cells. |
| Python with an inline span | `3 cell(s): 2 matched, 1 skipped` | The two code cells matched; Positron skipped the inline unit itself. `AFTER-SPAN` shown. |

Per knitr chunk, raw push versus Positron's native run (Ark):

| Chunk | Raw q2 record, pushed | Positron native (Ark) |
|---|---|---|
| `1 + 1` | `stream` stdout `[1] 2` | `execute_result` `text/plain` |
| `plot(1:10)` | `display_data` `image/png`, 1 image | `execute_result` `image/png`, 1 image |
| `message("KNITR-MESSAGE")` | `stream` **stderr** `KNITR-MESSAGE` | `stream` **stdout** |
| data frame | `stream` stdout, plain text table | `execute_result` `text/plain`; the pane shows a data-explorer summary ("3 rows x 2 columns, Open in Data Explorer") |
| `stop()` with `error: true` | `error`, `ename: ""`, shown as `Error: KNITR-ERROR` | `error`, `ename: ""`, with Fix and Explain controls |
| `results: asis` | `stream` stdout, `**KNITR-ASIS**` as text | same text |
| `results: hide` | `stream` stdout `[1] "KNITR-HIDDEN"`, **shown** | same text, shown |
| raw HTML, `results: asis` | `stream` stdout, the HTML as text | same text |

So the shapes differ from Ark's in five places (value versus stream for a bare expression, plot as `display_data`
versus `execute_result`, message on stderr versus stdout, data frame as text versus a data-explorer value,
`results: hide` and `asis` unknown to Ark), and Positron shows both. The follow-up plan says RIU-3 lists the
data-frame, message-stream and empty-`ename` rows as KNR-5 presenter limits (I did not re-read RIU-3 for that);
here they are seen directly from Positron's side.

## Positron's validity rules (read)

From `src/vs/workbench/contrib/positronQuarto/browser/quartoOutputCacheService.ts` in the fork
(`_validateCacheStructure`, line 217; the version check near line 262; `_ipynbToCachedDocument`, line 826;
`_ipynbToOutput`). A cache file is deleted on load unless:

- the JSON is an object with a numeric `nbformat`, an array `cells`, and an object `metadata`;
- `metadata.quarto_cache_version` is a number **and equals the loader's `CACHE_VERSION`** (a different number
  also deletes the file).

Beyond that gate, what the loader reads:

- Only `cell_type: "code"` cells are used. Each needs an `outputs` array (`cell.outputs.map` throws otherwise).
- `metadata.quarto_cell_id`, `quarto_content_hash` and `quarto_label` are read with defaults; `source` is
  optional; `metadata.last_updated` defaults to now.
- Outputs: `stream` (text string or array), `error` (`ename`, `evalue`, `traceback`), `execute_result` and
  `display_data` (each `data` entry; a non-string value is `JSON.stringify`\'d, so an object or array value is
  shown as compact JSON, matching the Python JSON row above). Output `metadata.quarto_webview_metadata` and the
  rest of the output metadata are kept.
- Everything else on the record is ignored. Positron rewrites the file on save to cell metadata with only
  `quarto_cell_id` and `quarto_content_hash` (plus `quarto_label` when present) and a notebook metadata of
  `quarto_source`, `quarto_cache_version` and `last_updated` (seen in the copied caches).

So the **minimum a valid file needs** is the four notebook-level conditions, `outputs` on each code cell, and a
content hash that matches. The record itself satisfies all of that unmodified (it was loaded unmodified, above).
Everything the export currently does beyond it is a decision, not a requirement: the RIU-5 rules for inline
units, owner and language, fallback cells, duplicate groups, array and JSON stringifying (the loader stringifies
itself), the metadata reduction (Positron does it on save), and the image-representation rule.
**Not read:** the q2 conformance mirror (`positron_loader` in `positron_conformance.rs`); I did not compare it to
this loader line by line.

## Two things the run showed that were not asked

- **The Julia plot's raw bundle is shown several times** (the RIU-5 problem, J8, reproduced on the raw record).
  This confirms why the representation rule exists; it does not re-test the rule (the filtered export was not run
  in this session). The Julia run in this session also carried a long stderr stream of
  `SYSTEM: caught exception of type :MethodError while trying to print a failed Task notice` on the plot cell.
  That came from the Julia environment used here (a Manifest resolved by Julia 1.12.1, run under 1.12.7); I did
  not investigate it and it is not a q2 finding. The RIU-6 Julia e2e test, with a different project, saw exactly
  one plot.
- **A record with an inline unit pushes fine.** Positron skips the span cell itself, so the export's
  inline-unit skip is not needed for Positron (S7 said the same for Python).

## Import side (not a spike, a design answer from the data)

The kernel-equivalence declaration (Item 1, "Design, import side") needs a fact the table above supports for R:
knitr's `r` chunks and Ark's native run produce the same *hash* and a different *shape*. That is the import
question's whole content: whether Positron's outputs, once attributed to a q2 knitr unit, are acceptable.
RIU-3 already decided yes for R, as display only (epic decision 6). This note adds nothing against it.

## What was not verified

- **Anything by a person's eye.** All window results are Playwright DOM and screenshot captures. I read two
  screenshots (the Julia plot cell); I did not look at every capture.
- **The filtered export in a window for R.** The export still refuses knitr cells, so there was nothing to push;
  this note is about the raw record.
- **A Python plot, data frame, error** raw push (S7 covered those on 2026-11-03; not repeated).
- **Image count by eye for the Julia plot**: the DOM had 3 `img` elements and the third had an empty `src`; I did
  not establish what the third is.
- **Large records, a non-default profile, Windows, dirty documents.**
- **The `ename: ""` case beyond the DOM text**, and whether Positron's display of an empty `ename` differs from
  Ark's in any way a user would see.
- **That the q2 conformance mirror agrees with Positron's loader** (see above).
- **Whether `last_updated` or an absent `source` matter**: the record carries a `source`; I did not test a file
  without one.
