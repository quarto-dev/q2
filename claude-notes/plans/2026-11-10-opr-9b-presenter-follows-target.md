---
title: 'OPR-9b: the presenter follows the target (display priority and figure directory)'
date: 2026-11-10
description: 'Makes the presenter pick a rich output by the output format as Quarto 1 does, and name figure directories after the format, for every engine. Fixes the recorded route inlining a base64 image on gfm and losing the figure on pptx, which the plain-render flip would otherwise ship. Changes native Jupyter on non-HTML targets, as a separate bugfix.'
status: ready
depends-on: [opr-7j, opr-9a]
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Origin:** split from [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (now a tombstone) on 2026-11-10 by Gordon's decision; evidence in the
[7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) and its addendum, and the [7d plan review](../research/2026-11-10-opr-7d-plan-review.md).
**Branch:** `opr/9b-presenter-follows-target` · **Lane C** (`workspace-5`) · **Blocks:** 9e, 9f, 9g.
**Needs:** `hub-client` tooling for the WASM build (the presenter compiles for wasm32); a Jupyter notebook fixture; Julia and Plots for the end-to-end check.

> **Reading text copied from OPR-7d.** Passages copied from the old 7d plan say "this plan" or "7d". Read them as this
> piece of the series 9a to 9g (see the [tombstone](2026-11-08-opr-7d-switch-plain-render.md)). Marks: (read), (ran), (relied).

## Start condition

Start when 7j and 9a are `complete` on the integration branch. 7j edits `presenter/options.rs`, `cell.rs` and the figure arm, which
this plan edits too: branch from a tip that has it. Read 9a\'s note, especially S1 (which targets get which bundle) and S2 (the
prototype and the list of tests that change). If the note and this plan disagree, the note\'s measurements win and you say so.

## The decision (Gordon, 2026-11-10)

**6. The presenter follows the target: display priority and figure directory (Gordon, 2026-11-10).** The 7d implementability
review ([report](../research/2026-11-10-opr-7d-implementability-review.md)) found two presenter defects that the flip would
expose to Julia on non-HTML targets, and Gordon decided to fix both in the presenter, not to work around them in the
harness or gate the targets. Evidence (ran, `julia/records-plot` fixture, plain against recorded):

- *Display priority.* `format_outputs_with` in `presenter/cell.rs` picks a rich output in one fixed order: `text/html`
  first, then markdown, then images, then plain text. That is Quarto 1\'s `displayDataMimeType` for an HTML target only.
  Quarto 1\'s function (`src/core/jupyter/display-data.ts`) depends on the target: for an HTML-compatible target
  `text/html` first; for a markdown target (gfm) markdown, then svg, png, jpeg, and `text/html` last; for any other
  target (docx, pptx, typst, odt) markdown, svg, png, jpeg, `text/html` only when it holds a table, then plain text. The
  presenter already has a faithful port, `display_data_mime_type(bundle, target)` with an `InlineTarget` of three flags
  (`presenter/inline.rs`, with a shared-fixture test), but only inline spans use it. The effect: a Plots output carries
  `application/pdf`, `image/png`, `image/svg+xml` and `text/html` on gfm, epub and pptx. On **gfm** the recorded route
  writes the `text/html` bundle as a raw block, so the page holds `<img src="data:image/png;base64,...">` where plain
  writes `figure-gfm/cell-4-output-1.svg` and a markdown image. On **pptx** the recorded route writes the same raw block,
  pandoc drops it, and the figure is **missing** (plain has `ppt/media/image1.svg`, recorded has no media): a regression
  the flip would ship. docx and typst take the image path on both routes because the Julia runner sends them an
  image-only bundle; epub, html and revealjs are HTML-compatible and unchanged.
- *Figure directory.* `FigureNaming::next_file` (`presenter/figures.rs`) hardcodes `figure-html`. The decided name is
  `figure-` plus the format identifier (`figure-gfm`, `figure-typst`, `figure-revealjs`), as plain Julia and q2 knitr write
  it today (not Quarto 1\'s `figure-commonmark`).

Both are presenter-wide, so native Jupyter changes too: for a bundle that carries `text/html` and an image, on a non-HTML
target, and for the directory of every non-HTML Jupyter figure. Gordon: that is a separate bugfix, not an exception to
the rest of the epic. State it in the landing report and in OPR-8\'s migration notes, and keep every HTML result
byte-identical (below).

## What the fix is

1. **Display priority.** `CellOptions` gains a `target: InlineTarget` (the three flags `to_html`, `to_latex`, `to_markdown`
   that `presenter/inline.rs` already defines, with `display_data_mime_type(bundle, target)` and a shared-fixture test). The
   default is `InlineTarget::from_format("html")`, so every caller that does not set it, and every existing golden and
   snapshot, behaves as today. `present_cell_step` sets it from `PresentInput.target`, which `present_document` and
   `present_units` already carry (no new classification). In `format_outputs_with`, the `text/html` branch is taken only when
   the target is HTML-compatible or `display_data_mime_type(data, target)` returns `text/html`; otherwise the markdown, image and
   plain-text branches that exist run. Do not touch the HTML path.
2. **Figure directory.** `FsFigureSink::new` and `FigureNaming` take the format identifier and write `figure-` plus it
   (`figure-gfm`, `figure-typst`, `figure-revealjs`). About five production callers (`engine/ts_record.rs`, `ipynb_replay.rs`,
   `replay.rs`, `jupyter/text_execute.rs` twice) all have a context; about 15 test callers. Check `transforms/hephaestus.rs`
   (`ARTIFACT_DIR = "figure-html"`) for an effect on non-HTML formats and say what you found.
3. **Jupyter.** Native Jupyter goes through the same code on every render, so it changes on non-HTML targets: a bundle with
   both `text/html` and an image now picks the image on gfm, pptx, docx and typst, and Jupyter figure directories are named
   after the format. Gordon: a separate bugfix, not an exception to the epic. Say so in the landing report and OPR-8\'s notes.
   Every HTML result stays byte-identical.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Tests first, display priority.** A unit test per target class (html, gfm, docx, pptx, typst, a LaTeX-flag target) on one
      bundle with `text/html`, `image/png`, `image/svg+xml` and `text/plain`, asserting the branch taken (html: the raw block;
      gfm, docx, pptx, typst: the svg file); a table bundle on docx keeps `text/html`; an html-only bundle on docx falls to
      `text/plain`; and one test that walks the shared fixture `display-data-precedence.json` through the cell path so the cell
      and inline paths cannot diverge. First red test: the gfm case (it picks the raw block today). Run them, see them fail for
      that reason, then implement.
- [ ] **Tests first, figure directory.** A unit test that a figure written for each of gfm, typst and revealjs lands under
      `figure-<identifier>`; the existing html tests stay green unchanged. Then implement.
- [ ] **Guards.** Every existing presenter golden and snapshot is unchanged (expect zero changed files; list any that change and why).
      Render the 13 shadow goldens and the parity-corpus notebooks on html before and after and diff (the 7j implementability
      review\'s recipe, in `2026-11-10-opr-7j-implementability-review.md`). The Jupyter change on non-HTML targets gets its own tests:
      a notebook with a Plots-like bundle on gfm and pptx.
- [ ] **Verify end to end** with the binary: render `julia/records-plot` to gfm and pptx on both routes (plain and `execute: record:
      true`); the two pages must match (gfm: the svg file in `figure-gfm`; pptx: media present). Rebuild `target/debug/q2` from a
      clean tree first. Record the invocations and say you read the output.
- [ ] **Gates:** `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the Windows
      cross-clippy (if `aws-lc-sys` cannot cross-check from this machine, say so in the report and go on); `cargo xtask lint`;
      `cd hub-client && npm run build:wasm` and the two WASM vitest files that pin splice HTML (`recordPayload.wasm.test.ts`,
      `captureSpliceHtml.wasm.test.ts`). Hub-client changes (none expected) need the changelog rule.
- [ ] **Snapshots:** count and list any changed `.snap` or golden file in the commit message and the report (expected zero).

## Stop conditions

Stop, commit your progress, write "Notes from an earlier attempt" and tell Gordon (the protocol's "When you cannot
finish") if any of these happens:

- an existing HTML golden or snapshot changes, or a Jupyter html result changes in the guard diff: the default target is wrong;
  do not update goldens to match;
- 9a\'s note shows the runner, not the presenter, must change for a target class (S1): report it, do not widen this plan;
- the WASM build or vitest files fail for a reason outside this change;
- a LaTeX-flag target turns out to be renderable in this tree and behaves differently from the unit test (Quarto 1\'s
  rule says images, then `text/latex`, then `application/pdf`): report it.

## Handoff

End the session with the protocol's handoff ("Ending your session"). Name 9e (and 9f, 9g behind it) as the lane's follow-on, and say what changed for Jupyter on non-HTML targets.

## Exit criteria

On gfm and pptx the recorded route matches the plain route for the `records-plot` figure; figure directories are named after the
format on every engine; every HTML result is byte-identical; the Jupyter non-HTML change is listed in the landing report.

## Notes from the run

(Fill in at landing.)
