---
title: 'OPR-9b: the presenter follows the target (display priority, figure directory and cell id)'
date: 2026-11-10
description: 'Makes the presenter pick a rich output by the output format as Quarto 1 does, and name figure directories after the format, for every engine. Fixes the recorded route inlining a base64 image on gfm and losing the figure on pptx, which the plain-render flip would otherwise ship. Changes native Jupyter on non-HTML targets, as a separate bugfix.'
status: in-progress
depends-on: [opr-7j, opr-9a]
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Origin:** split from [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (now a tombstone) on 2026-11-10 by Gordon's decision; evidence in the
[7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) and its addendum, and the [7d plan review](../research/2026-11-10-opr-7d-plan-review.md).
**Branch:** `opr/9b-presenter-follows-target` · **Lane B** (`workspace-4`) · **Blocks:** 9e, 9f, 9g.
**Needs:** `hub-client` tooling for the WASM build (the presenter compiles for wasm32); a Jupyter notebook fixture; Julia and Plots for the end-to-end check.

> **Reading text copied from OPR-7d.** Passages copied from the old 7d plan say "this plan" or "7d". Read them as this
> piece of the series 9a to 9g (see the [tombstone](2026-11-08-opr-7d-switch-plain-render.md)). Marks: (read), (ran), (relied).

## Start condition

Start when 7j and 9a are `complete` on the integration branch. 7j edits `presenter/options.rs`, `cell.rs` and the figure arm, which
this plan edits too: branch from a tip that has it. Read [9a\'s note](../research/2026-11-10-opr-9a-spike-results.md): S1 (done) gives the target classes below, and S2 (done, 2026-11-10) the rest: the prototype made recorded match plain on figures and directory for all 22 formats, **and `cargo nextest run -p quarto-core` with it applied gave 8256 passed, 0 failed**, so no existing test pins the old behavior and every test below is new. S2 also found a second hardcoded `figure-html` (item 2 below). If the note and this plan disagree, the note\'s measurements win and you say so.

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
  (`presenter/inline.rs`, with a shared-fixture test), but only inline spans use it. The effect, measured by 9a\'s S1 on 22
  formats (ran): the Julia runner gives only docx and typst an image-only bundle (`render_mimetypes.jl`), so every other target
  records `application/pdf`, `image/png`, `image/svg+xml` and `text/html` (Plots\' html is an `<img>` with a base64 PNG), and the
  recorded route takes the html. **Nine formats inline the base64 image** (gfm, commonmark, markdown, djot, rst, org, native,
  textile, mediawiki), where plain writes `figure-<format>/cell-4-output-1.svg` and links it. **Seven formats lose the figure**
  (pptx, odt, rtf, icml, fb2, docbook, plain): plain has the image (`ppt/media/image1.svg`, `Pictures/0.svg`), recorded has none.
  docx and typst take the image path on both routes and differ only in the directory name; epub, html, revealjs and dashboard are
  HTML-compatible and unchanged. The target classes for this fix: (1) HTML-compatible, unchanged; (2) docx and typst, already
  image-only, need only the directory name; (3) every other target, needs the gated `text/html` branch and the directory name.
- *Figure directory.* `FigureNaming::next_file` (`presenter/figures.rs`) hardcodes `figure-html`. The decided name is
  `figure-` plus the format identifier (`figure-gfm`, `figure-typst`, `figure-revealjs`), as plain Julia and q2 knitr write
  it today (not Quarto 1\'s `figure-commonmark`). S1 confirmed that the plain route already writes it for every format
  (`figure-odt`, `figure-rst`, ...): the plain route is the reference and this plan makes the recorded route agree with it.
- *Cell wrapper id (Gordon, 2026-11-10: make it faithful to Quarto 1).* Found by 9a\'s S2: the recorded Julia docx carries 4 hashed
  `w:bookmarkStart` elements that plain does not. Cause (ran, read): q2\'s `presentation_for` (`engine/ts_presentation.rs:52`) gives every
  cell the id `<label>` or `cell-<n>` as `quarto_attrs.id`, and the presenter writes it on the `.cell` wrapper for every target, where pandoc\'s docx
  writer turns an identified div into a bookmark. Quarto 1 writes the auto id on the wrapper only for **ipynb, JATS and html-family output**
  (`src/core/jupyter/jupyter.ts:1366-1376`: `isIpynbOutput || isJatsOutput || isHtmlOutput(format.pandoc)`, non-strict;
  `isHtmlOutput` is `src/config/format.ts:55-74`: html, html4, html5, dashboard, the slide formats s5, dzslides, slidy, slideous and
  revealjs, and epub, epub2 and epub3). A label that labels the container (`shouldLabelCellContainer`) goes on it for every target; that
  branch comes first. That is why plain, whose markdown comes from Quarto 1\'s `toMarkdown`, has the ids on html and none on docx or markdown.

The first two are presenter-wide, so native Jupyter changes too: for a bundle that carries `text/html` and an image, on a non-HTML
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
   (`figure-gfm`, `figure-typst`, `figure-revealjs`). **A second site hardcodes it too (S2, ran):** `presentation_for` in
   `engine/ts_presentation.rs` (line 83) builds the `quarto_file_path` of every Julia image output as
   `{stem}_files/figure-html/...`, and the sink prefers a producer path over its own name, so fixing `FigureNaming` alone leaves Julia on
   `figure-html`. Give `presentation_for` the format (its caller in `engine/ts_record.rs` has `ctx.format`; about 15 test calls). The
   S2 prototype used a `with_format` builder on the sinks to avoid touching the test callers; choose the constructor change or the builder. About five production callers (`engine/ts_record.rs`, `ipynb_replay.rs`,
   `replay.rs`, `jupyter/text_execute.rs` twice) all have a context; about 15 test callers. Check `transforms/hephaestus.rs`
   (`ARTIFACT_DIR = "figure-html"`) for an effect on non-HTML formats and say what you found.
3. **Jupyter.** Native Jupyter goes through the same code on every render, so it changes on non-HTML targets: a bundle with
   both `text/html` and an image now picks the image on gfm, pptx, docx and typst, and Jupyter figure directories are named
   after the format. Gordon: a separate bugfix, not an exception to the epic. Say so in the landing report and OPR-8\'s notes.
   Every HTML result stays byte-identical.
4. **Cell wrapper id.** In `present_cell_step`, where `cell_engine_attrs(cell)` is merged into the options (`presenter/cell_step.rs`), drop the
   engine\'s `id`, and nothing else of `quarto_attrs` (`execution_count` and classes stay, as in Quarto 1), unless `target.to_html`. Add an
   `EngineAttrs::without_id`. `InlineTarget.to_html` is the same identifier list as Quarto 1\'s non-strict `isHtmlOutput` (compared
   against `InlineTarget::from_format`), so the gate is that flag. q2 has no ipynb or JATS format identifier (`format.rs`), so those two
   Quarto 1 cases map to nothing today; if either format is added it joins the gate. An id the presenter itself writes (a labeled container,
   7j) is untouched: the engine id only fills the slot when the presenter wrote none. The target is the one 9b already threads into
   `present_cell_step`; the figure-directory item needs no change here. Prototype (ran, 9a follow-up): recorded docx 4 bookmarks to 0 (plain 0);
   `--to markdown` has no `{#cell-N}` on either route; html, revealjs and epub keep 4 of 4 ids on both routes; `cargo nextest run -p
   quarto-core` 8256 tests, the only two failures being the prototype\'s own test helper passing a non-HTML target (all 12 `cell_step` tests
   pass with an html target), so no existing test pins the id on a non-HTML target. Not measured: knitr and native Jupyter on docx
   (the gate is in the shared presenter, so it applies to them; whether they had the ids is unchecked).

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Tests first, display priority.** A unit test per target class (html, gfm, docx, pptx, typst, a LaTeX-flag target) on one
      bundle with `text/html`, `image/png`, `image/svg+xml` and `text/plain`, asserting the branch taken (html: the raw block;
      gfm, docx, pptx, typst: the svg file); a table bundle on docx keeps `text/html`; an html-only bundle on docx falls to
      `text/plain`; and one test that walks the shared fixture `display-data-precedence.json` through the cell path so the cell
      and inline paths cannot diverge. First red test: the gfm case (it picks the raw block today). Run them, see them fail for
      that reason, then implement.
- [ ] **Tests first, figure directory.** A unit test that a figure written for each of gfm, typst and revealjs lands under
      `figure-<identifier>`; the existing html tests stay green unchanged. Then implement.
- [ ] **Tests first, the second site.** A unit test that `presentation_for` names the path `figure-<format>` for a non-HTML format (first red test: gfm), and that html stays `figure-html`.
- [ ] **Tests first, the cell id.** Unit tests on one record cell whose `quarto_attrs` carries `{"id": "cell-1", "attributes": {"execution_count": "1"}}`:
      html, revealjs, dashboard and epub keep `#cell-1` on the wrapper; gfm, markdown, docx, pptx and typst write the wrapper with no id and
      still with `execution_count`; a labeled container (7j) keeps its label id on every target. First red test: docx (it writes `#cell-1`
      today). Then a binary check: the recorded docx of `julia/records-plot` has 0 `w:bookmarkStart`, like plain.
- [ ] **Guards.** Every existing presenter golden and snapshot is unchanged (expect zero changed files; S2 saw none; list any that change and why).
      Render the 13 shadow goldens and the parity-corpus notebooks on html before and after and diff (the 7j implementability
      review\'s recipe, in `2026-11-10-opr-7j-implementability-review.md`). The Jupyter change on non-HTML targets gets its own tests:
      a notebook with a Plots-like bundle on gfm, pptx and docx (S2\'s stored-notebook fixture, `jup/nb.ipynb` in the 9a scratchpad: html unchanged; gfm gets a figure file in `figure-gfm` instead of base64; pptx and docx gain the figure).
- [ ] **Verify end to end** with the binary: render `julia/records-plot` to every format in 9a\'s S1 matrix on both routes (plain and
      `execute: record: true`; 22 formats, a loop over a scratch dir each) and compare: no inlined base64, no lost figure (pptx and odt
      media present), figure directory `figure-<format>` on both. The two routes must match apart from interleaving (and the docx must carry no cell bookmarks). Rebuild `target/debug/q2` from a
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
- a format outside the S1 matrix needs a runner change, or S2 contradicts S1 (S1 found no runner change is needed): report it, do not widen this plan;
- the WASM build or vitest files fail for a reason outside this change;
- a LaTeX-flag target turns out to be renderable in this tree and behaves differently from the unit test (Quarto 1\'s
  rule says images, then `text/latex`, then `application/pdf`): report it.

## Handoff

End the session with the protocol's handoff ("Ending your session"). Lane B has no further plan after this one. Say that 9e (lane D, `workspace-6`) becomes startable when 9d is also complete, with 9f and 9g behind it, and say what changed for Jupyter on non-HTML targets.

## Exit criteria

On every format of the S1 matrix the recorded route matches the plain route for the `records-plot` figure (no inlined base64, no lost figure); figure directories are named after the
format on every engine; a cell wrapper carries the engine\'s auto id only on html-family targets, as in Quarto 1 (the recorded docx has no cell bookmarks); every HTML result is byte-identical; the Jupyter non-HTML change is listed in the landing report.

## Notes from the run

(Fill in at landing.)
