---
title: 'EXR-16 S7: self-contained widget outputs in Positron'
date: 2026-11-02
description: 'Positron inline execution caches an R htmlwidget as one self-contained text/html output, and the widget renders again when loaded from the cache. Only htmlwidget objects get text/html from Ark. An extra per-output metadata key survives the q2 record code. Seeding a q2 record by hand was not done.'
---

**Question.** Does Positron\'s inline Quarto execution produce a self-contained `text/html`
output for an R widget, does that output render when loaded from the cache, and does the
Positron-compatible subset of a q2 record tolerate an extra per-output metadata key? This is
the question from lane C\'s widget work ([WGT-2](../plans/2026-11-02-wgt-02-dependency-contract.md) in the widget epic, first called EXR-19).

**Answer.** Yes to all three, with one narrowing: only `htmlwidget` objects get a `text/html`
output from Ark. A full hand-built q2 record placed in the cache (part c as written in the
plan) was not done; the cache-reload check below stands in for it.

**Branch.** The Positron side ran in the `spike/q2-s3-inline-exec` dev build
(`/Users/gordon/src/positron-spike-s3`). The q2 side is commit `f6394d0df` on `exr/EXR-16`.
The example document is `~/docs/inline-output/s7-widget.qmd` (plotly, leaflet, and a plain
`HTML()` control).

## Part a: the extra key survives (no GUI)

Two characterization tests in `quarto-core` pass on the first run:

- `an_extra_per_output_metadata_key_round_trips_and_moves_no_key` in
  `record/freeze_readiness.rs`: a `quarto_widget` object and an `unknown_future_key` on a
  `display_data` output round-trip, the bytes are stable, and no matching, staleness or reuse
  key moves.
- `an_extra_per_output_metadata_key_survives_the_import` in `record/positron_import.rs`: only
  `quarto_webview_metadata` is dropped; the extra keys survive the import.

## Part b: what Positron caches (GUI, then read from disk)

Gordon ran the three cells with inline execution in the dev build. I then read Positron\'s cache
file under `<user-data-dir>/User/globalStorage/quarto-inline-outputs/`.

| Cell | Rendered | Cached output |
|---|---|---|
| `plot_ly(...)` | interactive plot | one `execute_result`, `text/html` only, about 3.87 MB |
| `leaflet() \|> addTiles() ...` | map of Boston | one `execute_result`, `text/html` only, about 460 KB |
| `htmltools::HTML("<p><b>plain html</b></p>")` | raw text `<p><b>plain html</b></p>` | one `execute_result`, `text/plain` only |

For the plotly output:

- no `<script src>`, no `<link href>`, no `<img>`; every `<script>` is inline;
- the `data:` URIs and `http(s)://` strings (38 of them) are string constants inside the
  plotly.js source (attribution links, tile-server addresses, license comments, a `cdn.plot.ly`
  string), not load-time dependencies;
- no temp paths (the `/private/...` hits are plotly.js internal symbol names).

The two `stream` outputs in the first cell (package-loading messages) are cached as streams, as
expected.

**Why `HTML()` is plain text.** Ark overrides only `print.htmlwidget`
(`extensions/positron-r/ark/crates/ark/src/modules/positron/html_widgets.R:275`).
`htmltools::HTML()` returns a character string of class `html`, which R prints as text. So the
control cell was the wrong control for "plain HTML": it shows that non-widget HTML (including
`htmltools` tags and `HTML()` strings) arrives as `text/plain` unless a different hook claims
it. This was not traced further.

## Part c: the widget renders from the cache (GUI)

Gordon quit Positron, relaunched, and reopened `s7-widget.qmd` without running any cell. The
plotly and leaflet outputs appeared immediately, and the third cell showed its raw text. So a
`text/html` output loaded from the cache renders as a widget. A q2 record whose output carries
the same bundle is read by the same loader (S2a showed a clean load).

## What this means for the widget epic (WGT-2, first called EXR-19)

- A widget exported from q2 should be one `text/html` bundle with every JS and CSS dependency
  inline. Ark\'s own output is exactly that, so the Positron reader needs nothing beyond the
  MIME bundle.
- The extra per-output key is safe in a q2 record. Positron\'s cache keeps all MIME types.
  S2a showed that rewriting the record drops q2 keys, so any widget metadata that must
  survive a Positron round trip has to live in the MIME bundle itself.
- Do not expect `text/html` from non-widget R values in Positron.

## Not tested

A q2 record placed in the cache by hand (S2b, skipped by Gordon on 2026-11-02); Python widgets;
other widget packages (only plotly and leaflet); offline rendering (leaflet tiles need the
network); widgets larger than a few MB beyond plotly\'s 3.87 MB; why the control cell stays
plain text beyond the single `print.htmlwidget` override.
