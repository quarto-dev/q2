---
title: 'WGT-3: ipywidgets and the widget state protocol'
date: 2026-11-02
description: 'ipywidgets needs comm capture in the Jupyter engine, notebook-level widget state in the record, view output in the presenter and a loader on the page. It is the acid test of the general widget design. Draft: what is known, what is not, and the open design questions.'
status: draft  # written from a read of q2 and Quarto 1; nothing run
---

**Part of:** [the widget epic](2026-11-02-widget-epic.md) (WGT-3).
**Depends on:** [WGT-1](2026-11-02-wgt-01-widget-spikes.md) (ipywidgets, bqplot and anywidget
fixtures and state sizes), [EXR-6](2026-10-28-exr-06-jupyter-flip.md) (the Jupyter engine).
**Design depends on:** [WGT-2](2026-11-02-wgt-02-dependency-contract.md): the contract has to carry
notebook-level state.

## Why this plan exists

Gordon wants ipywidgets designed and ported in order to be sure the general widget system is
correct. It is too large for the Julia regression fix, so it lives here. Where Quarto 1 or nbclient
has a working algorithm we port it; where q2\'s architecture differs (records, a Rust engine,
incremental execution) we write new code.

## What an ipywidget is

A cell output is a small pointer: a `widget-view+json` MIME entry holding a `model_id`. The widget\'s
data (its value, layout, children, and the JS module and version that draws it) is **widget state**,
which exists in Jupyter only as comm messages between the kernel and the front end while it runs.
A static page needs both the view pointers and the state.

## What is known (read, not run)

1. **Capture is missing in q2.** The Jupyter execute loop matches `Status`, `StreamContent`,
   `DisplayData`, `ExecuteResult` and `ErrorOutput` only
   (`crates/quarto-core/src/engine/jupyter/execute.rs:139-171`); there is no comm handling in
   `engine/jupyter`. `execute.rs:252-258` maps received media types to MIME names and nothing more.
2. **Quarto 1 gets state from nbclient.** `handle_comm_msg` accumulates
   `widget_state[comm_id].update(data["state"])`, base64-encodes binary buffers, and
   `set_widgets_metadata()` writes `nb.metadata.widgets["application/vnd.jupyter.widget-state+json"]`
   with `version_major` 2 and `version_minor` 0, keeping models that have a `_model_name`
   (`nbclient/client.py:718-737, 1210-1240`; Quarto 1 calls it at
   `external-sources/quarto-cli/src/resources/jupyter/notebook.py:361`). Quarto 1 also carries the
   `widgets` notebook metadata through its cache (`notebook.py:515`).
3. **The record has room, nothing writes it.** `RecordMetadata` has a flattened `extra` map
   (`record/types.rs:60`), so a `widgets` key survives a round trip. No code sets it. The state is
   notebook-level and accumulates across cells, so per-cell incremental execution needs a design.
4. **The page needs** (from `external-sources/quarto-cli/src/core/jupyter/widgets.ts:100-118`):
   requirejs 2.3.6, jQuery 3.5.1 and the `define('jquery')` shim; the
   `@jupyter-widgets/html-manager` `embed-amd.js` from jsdelivr with the version unpinned (`@*`); and
   the state script after the body, with the `</script>` escape variant (quarto-cli issue 8433).
5. **View-time loading.** The page fetches html-manager and each third-party widget module (bqplot,
   ipyleaflet, and others) from a CDN by module name and version. These pages need the network, and
   their behavior can change without any change to the document.

## What is not known

- Whether anywidget (and drawdata, jupyter-scatter, lonboard) uses the same state mechanism and works
  under the static html-manager embed. Unverified.
- Whether QuartoNotebookRunner supports comms at all. Unverified.
- How large the state gets with binary buffers (bqplot, jupyter-scatter, lonboard). WGT-1 measures it.
- How Quarto 1 emits the `widget-view+json` output itself (`display-data.ts` was not read in detail).
- Whether html-manager `@*` still renders ipywidgets 7 models. Unverified; the legacy environment was
  not built.

## Design questions (open)

- **State attribution.** Widgets reference children by `IPY_MODEL_<id>`. Per-view attribution means
  following that graph; the alternative is one notebook-level blob that is rebuilt on each run. Which
  one survives incremental execution and unit-level cache hits?
- **Where the state lives** in the record, and how a freeze or hub copy carries it (it is state, not
  files, so `quarto_files` may not be the home).
- **Pin or vendor html-manager.** An unpinned `@*` is neither reproducible nor offline. Does the
  contract support a pinned reference, or vendored bytes for an offline page?
- **Port or write new.** The comm-tracking algorithm is small (port nbclient\'s). The page includes are
  literal text (port from Quarto 1). The record and incremental parts are new.
- **Non-HTML formats.** The view output has a `text/plain` fallback; what does each format do?

## Checklist (to be refined once WGT-1 reports)

- [ ] Read how Quarto 1 emits the `widget-view+json` output (`display-data.ts`) and what it puts in
  the page body.
- [ ] Spike: save the state JSON for ipywidgets, bqplot and an anywidget fixture under Quarto 1
  (a WGT-1 fixture), and measure size and buffers.
- [ ] Spike: check whether anywidget works under the static embed, and whether the runner supports
  comms.
- [ ] Design: state attribution, record location, pinned or vendored loader. Tests first.
- [ ] Capture: comm tracking in the Jupyter engine, with the fake kernel extended to send comm
  messages.
- [ ] Record: write and read `metadata.widgets`; round trip and freeze-readiness tests.
- [ ] Presenter and page: emit the view output; derive the loader includes and the state script after
  the body (through the WGT-2 contract).
- [ ] End-to-end: an ipywidgets document through `q2 render`, inspected in a browser.
