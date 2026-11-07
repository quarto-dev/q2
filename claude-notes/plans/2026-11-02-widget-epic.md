---
title: 'Epic: widgets and HTML dependencies across engines'
date: 2026-11-02
description: 'One widget and dependency system for every engine: an engine-neutral contract that enables an AMD loader only when needed, multi-engine coexistence on one page, and ipywidgets as the acid test that the general design is right. Split out of the execution-records epic, where the work began as EXR-19.'
status: draft  # WGT-1 environments built 2026-11-02; no plan has started; the contract design waits on the WGT-1 gate
---

**Branch:** none yet. Propose `feature/widget-epic`, cut from the tip of
`feature/execution-records-epic` when the first plan starts.
**Origin:** found by the Julia flip in the
[execution-records epic](2026-10-28-execution-records-epic.md) (EXR-8 B4: core presents, the engine
skips `toMarkdown`, and a widget output loses its scripts). It began as EXR-19 in that epic. On
2026-11-02 Gordon moved it here, because the old epic is otherwise complete and ipywidgets is too
large for it.
**Design review:** [EXR-19 design review](../research/2026-11-01-exr-19-design-review.md)
(findings, evidence and the decisions taken in discussion).

## Goal

A widget system that accommodates every strategy a widget or engine uses, in any engine, and that
works when several engines share a page:

- **An engine-neutral dependency contract (option B)**, designed only if the WGT-1 spikes teach
  enough to make it good. Until then the Jupyter and Julia path is option A: derive dependency text
  in core and carry it as `includes`.
- **An AMD loader (requirejs) only when something needs it**, not for every `text/html` output as
  Quarto 1 does.
- **Interoperation** with other systems: Positron and Ark, the Jupyter ecosystem, marimo.
- **Multi-engine pages.** Engines declare or derive dependencies differently, and nothing today
  stops two of them wanting incompatible things (Gordon has seen marimo and Julia clash over
  requirejs).
- **ipywidgets as the acid test.** It needs comm capture, notebook-level widget state, a record
  design for incremental execution, and view-time loading. If the general design cannot carry it,
  the design is not general. Code is ported from Quarto 1 (and nbclient\'s state algorithm) where
  that is right, and written new where it is not.

Not a goal: being foolproof, or fixing every inconsistency the spikes find. The spikes exist to
learn what the problems are, including the ones we cannot fix.

## Decisions so far (Gordon, 2026-11-01 and 2026-11-02)

- This is its own epic, with ipywidgets in scope (WGT-3), so there is room for work the spikes find.
- Quarto 1 is one strategy, not the reference answer. Anywidget, htmlwidgets, marimo and others are
  strategies the system must accommodate.
- Option B is wanted, **only if the spikes teach enough to make it good**; phase 1 ships as option A
  with a seam so B can replace it.
- Visibility for Jupyter and Julia matches Quarto 1 (document-wide; hidden outputs still count). The
  knitr adapter owns knitr\'s own quirk.
- Dependency bytes for file-backed producers (knitr\'s `knit_meta`) go in the record\'s
  `quarto_files`, because a frozen record must render without the original library. Derived text is
  recomputed and never stored. Quarto 1 freeze parity is not a requirement.
- The record stores what the producer made; the presenter handles a manifest and self-contained HTML;
  export to Positron converts a manifest to self-contained where files are readable.

## Plans and DAG

| ID | Plan | Notes |
|---|---|---|
| WGT-1 | [Widget and dependency spikes](2026-11-02-wgt-01-widget-spikes.md) | catalog across Python, Julia, R and marimo; the gate for option B; environments built |
| WGT-2 | [The dependency contract and the Julia and Jupyter widget regression](2026-11-02-wgt-02-dependency-contract.md) | was EXR-19; phase 1 (warn, then derive in core) fixes the Julia regression |
| WGT-3 | [ipywidgets and the widget state protocol](2026-11-02-wgt-03-ipywidgets.md) | comm capture, record state, presenter, page; draft |

Work the spikes are expected to produce, not yet planned: merge rules for dependencies from several
engines, the knitr adapter (with the knitr epic), and Positron export (with EXR-16). Each becomes a
plan here when WGT-1 names it.

| Plan | Hard dependencies | Soft |
|---|---|---|
| WGT-1 | none (environments already built; the q2 comparison needs a q2 built from the execution-records epic tip, where the Julia flip is merged) | EXR-8 (complete) |
| WGT-2 | Phase 1: EXR-1, EXR-6, EXR-8 (B4, the flip). Contract design and later phases: WGT-1 (the gate) | WGT-3, EXR-16 (export), the knitr epic (adapter) |
| WGT-3 | WGT-1 (the ipywidgets fixtures and state sizes), EXR-6 (the Jupyter engine). Design: WGT-2 (the contract must carry state) | EXR-4 |

The [execution-records epic](2026-10-28-execution-records-epic.md) is complete, and EXR-18 (docs) is
merged, so nothing in it waits on this epic. One question remains: whether the trigger-gated
[EXR-17](2026-10-28-exr-17-legacy-removal.md) (legacy removal) should wait on WGT-2 phase 1, since a
widget document must render correctly before the old path goes.

## Open questions for Gordon

- When an output carries both a manifest and self-contained HTML, is the manifest authoritative?
- Which lane runs these, and does the epic branch follow `feature/widget-epic`?
- Does EXR-17 (trigger-gated legacy removal) wait on WGT-2 phase 1 (above)?
