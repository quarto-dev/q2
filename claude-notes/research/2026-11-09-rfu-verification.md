---
title: 'RFU-4 verification results (partial)'
date: 2026-11-09
description: 'Results of the RFU-4 checks run so far: the decision 12 fail-soft re-check, and the unhighlighted edited cell in q2 preview. Hub verification, Julia and the RIU-3 browser items are not run yet.'
---

Binary: `target/debug/q2` built from `rfu/4-verification-and-small-fixes` (claim commit `62bf6f2fb`). Python and Jupyter are not
available on this machine ("Engine 'jupyter' is registered but its runtime is not available"), so the checks use `{r}` cells (R is installed).

| Item | Result | Ran, read or not run |
|---|---|---|
| Hub verification | Not done. A search of the RIU plans found no record of a hub run; RIU-6 states that `provide-hub` to the hosted hub is not part of the demo. | not run |
| Decision 12 (failed `record: true` copy fails soft) | Confirmed. See below. | ran |
| Edited cell loses highlighting in `q2 preview` | Bug reproduced. See below. | ran (Playwright) |
| Stale Julia server, missing Plots project | Not done. | not run |
| RIU-3 browser items | Not done. | not run |

## Decision 12

Invocation, in a scratch project whose `.quarto/execution-records` is a regular file, with `execute: {record: true}` and one `{r}` cell:

```bash
q2 render doc.qmd
```

Observed (output inspected): exit code 0; `Warning: Execution record not written` with the text "the execution record copy requested by
`record: true` could not be written: ... creating .../.quarto/execution-records: I/O error: File exists (os error 17)" and the line
"The render is unaffected. Check that `.quarto/execution-records` is a directory you can write to."; `doc.html` was produced and contains the
document text. Decision 12 holds.

## Findings for Gordon

### Edited cell is not highlighted in the records-only preview (RIU-3a)

Reproduction: render a project once with `execute: {record: true}` and an `{r}` cell (this fills `.quarto/execution-records/doc.ipynb`), edit
the cell source, then:

```bash
q2 preview --no-browser --port 18741 -M 'execute:{record-sources: .quarto/execution-records}' doc.qmd
```

A Playwright page load of `?page=doc.qmd` shows, inside the `q2-preview.html` frame, the edited cell as
`<pre class="{r} code-with-copy"><code>x &lt;- 1+2 ...` with no highlight spans. `q2 render -M execute:eval:false` of the same cell gives
`<pre class="sourceCode r">` with highlighting. So the unmatched cell reaches the writer still carrying the raw `{r}` attribute class instead of
the `sourceCode r` class. Not fixed here, as the plan says.
