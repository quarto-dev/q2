---
title: 'KNR-2b: the knitr presenter layer, remainder'
date: 2026-11-02
description: 'The part of KNR-2 that waits on the knitr record writer (KNR-3) and on inline capture (KNR-4): the options and fences that need known_options and the original chunk text, the figure sink, the hold wiring, and the corpus bar of zero blockers. Split out of KNR-2 on 2026-11-02 and sequenced after KNR-4.'
status: draft  # split from KNR-2 at its pause point; runs after KNR-3 and KNR-4
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (plan KNR-2b).
**Split from:** [KNR-2](2026-10-30-knr-02-knitr-presenter-layer.md) on 2026-11-02, at the
point where everything left in it waited on another plan. Read KNR-2 for the design
decisions, the Background and the finished phases; this plan carries only what is open.
**Depends on:** [KNR-2](2026-10-30-knr-02-knitr-presenter-layer.md) (done),
[KNR-3](2026-10-30-knr-03-record-adapter-hash-fallback.md) (Phase 3B writes the record
fields below; Phase 3C runs the corpus gate in report mode), and
[KNR-4](2026-10-30-knr-04-inline-typed-values.md) (clears `s9/01_inline`, the last blocker
outside KNR-3).
**Blocks:** KNR-6 (the flip), whose corpus bar is zero blockers.
**Estimated sessions:** 1 to 2 (a guess, made at the split: the items are small, and the
corpus has no case for the holds).

## Why this is separate

KNR-2 built the presenter, the fallback-unit presenter, the target router and the corpus
gate, and measured the corpus: 198 of 221 cases accepted, 5 allowlisted, 11 never compared,
7 blockers outside the allowlist. All 7 wait on other plans: the four `options/foo.unknown/*`
cases and the `tbl-cap` attribute of `s2/01_kable` need KNR-3 to write `known_options`,
`setup-state/knit_hooks_override` needs KNR-3\'s fallback-unit rule, and `s9/01_inline` needs
KNR-4. The other open items need the same record fields, or have no corpus case yet. Splitting
lets KNR-2 close and keeps this work where its inputs exist. The presenter-only items below
(Phase 2bB) need no other plan; if someone is free earlier they may run first.

## Record contract this plan consumes

What KNR-3 Phase 3B must write, from the KNR-2 hand-off (the epic lists them too):

- `quarto_output_kinds`, parallel to the outputs.
- The original info string and `#|` lines, for `fenced.echo` and language fences.
- `known_options` on the `quarto_engines` entry, as the full union the live R path excludes
  (`hooks.R` 470-505), not only `names(knitr::opts_chunk$get())`.
- `knitr_visibility` inputs, knitr errors with empty `ename` and `evalue` and the printed text
  as the traceback, `include_graphics` marks and widths.
- `metadata.quarto_file_path` in place of `quarto_knitr.file` (epic decision 25).
- The `include=FALSE` dependencies in the merged record-level form (epic decision 27).
- A call to `target_route::route_for_target`, with R told to skip capture on a fallback route
  and `fallback_diagnostic` surfaced once, and `format` passed to `present_document`.

## Phase 2bA: items that need KNR-3\'s record fields

The presenter side of each is built and tested over hand-built records; what is open is
proving it over a record KNR-3 wrote.

- [ ] Unknown options as cell attributes. `CellOptions::from_knitr_known(options,
  known_options)` is built (KNR-2, Phase 2D slice 5), and an absent `known_options` means no
  filtering. Once KNR-3 writes `known_options` as the full union, the four
  `options/foo.unknown/*` cases and the `tbl-cap` attribute of `s2/01_kable` should become
  syntax-level. Tests first: a corpus run that presents from a written record, not the
  runner\'s direct path.
- [ ] `fenced.echo` and the original chunk. The presenter side is built
  (`CellOptions::with_original_chunk`, Phase 2D slice 2), and the four `options/fenced.echo/*`
  cases are syntax-level through the runner. KNR-3\'s mapper must write the info string and
  the `#|` lines into the record; then drive the same four cases from the record.
- [ ] Shared figure sink and `metadata.quarto_file_path` (epic decision 25, merged
  2026-11-02): write an output\'s bytes at that document-relative path when it is inside the
  document directory and collides with no other output\'s file; otherwise use the generated
  `cell-<n>-output-<m>.<ext>` name and count the fallback. Outputs without the key (Jupyter,
  imported Positron output) keep today\'s names. Tests first, over hand-built outputs. The
  writer rename itself is KNR-3 Phase 3B; this item lands with or after it.

## Phase 2bB: presenter-only items

No other plan blocks these, and no corpus case needs them yet, so each starts from a new
fixture and a test that fails first.

- [ ] Wire the holds. `apply_holds` (`presenter/cell.rs`) is tested over hand-built records
  but is called only by the corpus runner, not by `present_document`. Wire `fig.show = "hold"`
  and `results = "hold"` into the product path (epic decision 21: holds run at present time
  over the capture-order anchors), then add a fixture for each.
- [ ] The rest of decision 10\'s list, each with a test against knitr\'s behavior in
  `external-sources/knitr/R/block.R` (re-read in the released versions KNR-1 tests):
  `fig.show = "animate"` (two corpus cases are unavailable here for want of `gifski`) and
  `fig.beforecode`. Done in KNR-2: logical and numeric `echo`, `results` (`markup`, `asis`,
  `hide`), numeric `warning` and `message`, and the message and warning classes.
- [ ] The image and caption options Phase 2C slice 3 left unbuilt: the `#fig-` label and
  cell-level caption when `fig.cap` and `fig.subcap` are both set, `fig.scap`, `fig.link`,
  image `width` and `height`, `fig.align`, the `fig.sep` layout synthesis, `fig-column`,
  `tbl-column` and the `cap-location` family. For each, add a fixture and build it, or record
  it as unsupported with a reason.
- [ ] Language fences (`hooks_markdown()`) and the `knit_asis` fence length when the HTML
  contains backtick runs. No corpus case fails (`s1/11_knit_asis` is syntax-level), so write a
  fixture for each and decide whether anything is missing.
- [ ] Unit tests of each hook feature against hand-built outputs: caption and label
  dedupe, layout synthesis, class assembly, the `results: asis` bare rule, unknown-option
  forwarding (JSON, escaping), indent for list-nested chunks, fence length for outputs
  containing backtick runs.

## Phase 2bC: close-out

- [ ] Per decision 13, R half (KNR-3\'s capture call; KNR-3\'s table lists the rule): on a
  `Fallback` route tell R to skip capture for the whole document, so the chunk hook writes
  the fallback units, and surface `fallback_diagnostic` once. LaTeX figure attributes
  (`fig-pos`) and animations are the LaTeX epic\'s concern.
- [ ] Dependencies of a hidden widget (settled 2026-11-01; the work moved from EXR-19 to
  WGT-2 in the widget epic, `2026-11-02-widget-epic.md`, read 2026-11-02): the neutral
  presenter rule is "a hidden output brings no dependencies", with no `include=FALSE` special
  case. Quarto 1 keeps the dependencies of a widget in an `include=FALSE` chunk, a knitr-only
  artifact, and the knitr adapter owns it. Revised proposal, replacing the earlier idea of a
  per-output mark the presenter reads (that would be a presenter special case): an
  `include=FALSE` chunk leaves no output in the record, so its `knit_meta` cannot hang on an
  output. The adapter (KNR-3 capture, KNR-6) carries it in the merged, record-level form,
  `EngineRecord.html_dependencies`, which WGT-2 keeps. The presenter then needs no knitr
  rule. A shown widget\'s `knit_meta` goes in its own output\'s metadata and goes away with the
  output under `results = "hide"`, which this plan already does. This plan does not wait on
  the widget epic: WGT-2 lists the knitr epic only as a soft dependency, so the knitr epic can
  finish first. The question this leaves is the knitr flip (KNR-6): without the recorded
  dependencies shown widgets lose theirs, as the Jupyter and Julia flips do today. Proposed:
  KNR-6 puts the record\'s merged `knit_meta` onto the existing
  `ExecuteResult.html_dependencies` field at flip time, which needs no WGT-2 contract; WGT-2
  later replaces it with the neutral manifest. Hand this to KNR-3 and KNR-6, not the presenter.
  The wrapper (id and caption
  after the widget, three-tick `{=html}` fence) is done here (slice 5) and is the knitr half of
  the shared raw-HTML path that WGT-2 wants.
- [ ] Re-run the corpus. Every difference is classified; zero blockers; the intentional
  divergences are written down with one-line reasons. Measured 2026-11-02 (report r15, 221
  cases, html plus typst, docx, gfm, revealjs, epub on the rich-output documents): 198
  accepted (identical or syntax-level), 5 allowlisted, 11 not compared (5 fallback units, 4
  consistent aborts, 2 unavailable `gifski`), and 7 blockers outside the allowlist. All 7 wait
  on other plans, so this item stays open until they land: the four `options/foo.unknown/*`
  cases and the `tbl-cap` attribute of `s2/01_kable` (KNR-3 writing `known_options`),
  `setup-state/knit_hooks_override` (a user hook, so the fallback-unit rule), and
  `s9/01_inline` (KNR-4).
- [ ] Assert the gate. `corpus_report` currently writes the gate verdict
  (`knitr_corpus_gate`, epic decision 26) into its summary without failing on it, because
  the residual cases waited on other plans. Once they have landed, make a failed gate fail a
  local run (skipping without R, as the other real-R tests do), so KNR-6 Phase 6A promotes
  one harness and not two. KNR-3 Phase 3C runs the gate in report mode; the assertion
  waits for KNR-4, because `s9/01_inline` is a blocker until inline capture lands.
- [ ] Run the corpus on the minimum and the latest supported knitr (epic decision 17: a local
  run) and report the pass and skip counts and the R and knitr versions.
- [ ] Check the presenter changes of KNR-2 and this plan build for `wasm32` and add no
  dependency on the engine (EXR-1\'s availability check). Not yet verified for KNR-2\'s
  commits.

## Exit criteria

- Zero blockers on the corpus (the allowlist entries are signed off, per epic decision 26),
  on the minimum and the latest supported knitr, with the counts and versions reported.
- `latex`, `beamer`, `pdf` and uncovered targets route to fallback units with one diagnostic
  (the Rust half is done in KNR-2; the R half is KNR-3\'s capture call).
- `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core` are green, counts reported against the live baseline.
- Jupyter output is unchanged on its goldens.
- The presenter builds for `wasm32`.
