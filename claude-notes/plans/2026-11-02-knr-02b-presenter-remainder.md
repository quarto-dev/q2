---
title: 'KNR-2b: the knitr presenter layer, remainder'
date: 2026-11-02
description: 'The part of KNR-2 that waits on the knitr record writer (KNR-3) and on inline capture (KNR-4): the options and fences that need known_options and the original chunk text, the figure sink, the hold wiring, and the corpus bar of zero blockers. Split out of KNR-2 on 2026-11-02 and sequenced after KNR-4.'
status: done  # 2026-11-05; the corpus gate is asserted and passes, so KNR-6 can start
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
gate, and measured the corpus on the direct path (r15): 198 of 221 cases accepted, 5
allowlisted, 11 never compared, 7 blockers outside the allowlist. Those 7 waited on KNR-3 (the
four `options/foo.unknown/*` cases, the `tbl-cap` attribute of `s2/01_kable`, and
`setup-state/knit_hooks_override`) and on KNR-4 (`s9/01_inline`). KNR-3 has since landed its
three and runs the gate through the written record (slice 4, 2026-11-03): 193 accepted, 5
allowlisted, 7 not compared, 16 blockers. Fifteen of those are KNR-3 slice 5\'s (a regression in
the `include_graphics` reader, header-option interpretation, list-nested indentation and the
`quarto_echoed_code` key); the sixteenth is `s9/01_inline` (KNR-4). The other open items here
need the same record fields, or have no corpus case yet. Splitting lets KNR-2 close and keeps
this work where its inputs exist. The presenter-only items below (Phase 2bB) need no other
plan; if someone is free earlier they may run first.

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

*Reconciled 2026-11-05 against the tree (tip `9105cc198` plus this session): the first two were
done by KNR-3, the third was open and is built.*

- [x] Unknown options as cell attributes. Done by KNR-3: the capture writes `known_options` as the
  full union, `present_knitr_cell` passes it to `CellOptions::from_knitr_known`, and the corpus
  runs through the written record (slice 4 on). The four `options/foo.unknown/*` cases and the
  `tbl-cap` attribute of `s2/01_kable` are accepted in the 2026-11-05 run (below). The JSON and
  quote-escaping forms have a unit test
  (`options_outside_the_known_list_become_cell_attributes`).
- [x] `fenced.echo` and the original chunk. Done by KNR-3 (`original_chunk` in
  `presenter/knitr_inputs.rs`); the four `options/fenced.echo/*` cases are accepted through the
  record.
- [x] Shared figure sink and `metadata.quarto_file_path` (epic decision 25). Built here, tests
  first (`presenter/figures.rs`): `FigureSink::write_image_at(mime, value, preferred)` writes at
  the producer\'s path when it is document-relative, forward-slash, free of `..` and `:`, and not
  claimed by another output; otherwise it uses the generated `cell-<n>-output-<m>` name and
  `path_fallbacks()` counts it. The slot is claimed either way, so generated numbers do not
  depend on which outputs named a path. `write_image` is a provided method that passes `None`,
  so Jupyter and imported Positron output keep today\'s names. `cell.rs` passes
  `metadata.quarto_file_path` from a display or execute-result output. Tests: four sink tests
  (inside, collision, six outside forms, the runtime sink through the `SystemRuntime`) and
  `a_plot_with_a_producer_path_is_written_there` in `cell.rs`. *The cell-level test was written
  after the wiring, so it was not seen failing; the sink tests were (compile error, then green).*

## Phase 2bB: presenter-only items

*Each item has a corpus fixture compared against live knitr, or a unit test that failed first,
or is recorded as unsupported with its reason.*

- [x] Wire the holds. Already wired when this plan was reconciled: `present_knitr_cell`
  (`presenter/knitr_inputs.rs`) applies `apply_holds` for `fig.show = "hold"` and
  `results = "hold"`, and `present_document` and the record splice both reach it. Product-path
  test: `results_hold_moves_printed_text_to_the_end`; corpus: the `options/fig.show` and
  `options/results` sweeps and `s1/05_fig_show_hold`, all accepted.
- [x] `fig.beforecode`. New sweep `options/fig.beforecode` (TRUE and FALSE, header and YAML, over
  the two-plot body). It failed first (two blockers: `00_true` in both forms), then
  `before_code_anchors` (`presenter/cell.rs`) fixed it: knitr\'s `fig_before_code` moves each
  plot after the first code to the start of the source run it followed, which is only a change
  of its anchor (the anchor of the output before it, or 0). It runs before the holds, as in
  knitr. Unit tests: `before_code_moves_each_plot_to_the_start_of_its_source_run`,
  `before_code_leaves_a_plot_before_any_code_and_other_outputs`.
- [x] `fig.show = "animate"`: **unsupported here, with a reason.** An animation is one file knitr
  composes from the plots through the `gifski` hook, so it is the capture\'s output, not a
  presenter rule: the presenter would see one image. The two corpus cases
  (`options/fig.show/*/02_animate`) stay "unavailable" because `gifski` is not installed on
  this machine, so nothing here can compare them. Whoever installs `gifski` should re-run the
  corpus and look at those two; if the capture hands the presenter one gif, they will pass
  without a presenter change.
- [x] The image and caption items that were left. All four compared against live knitr:
  - The `#fig-` label. The plan named only the case with both `fig.cap` and `fig.subcap`, but
    the basic case was missing too: **a `fig-` label never reached the image.** hooks.R puts it
    in the image attributes (`{#fig-one}`; `{#fig-two-1}` and `{#fig-two-2}` for several plots),
    and with both options set it moves to the cell div (`{#fig-pair .cell}`) with `fig.cap`
    closing the div as the cell caption. Built in `presenter/options.rs`
    (`with_figure_count`, `plot_image_attrs`, `cell_div_id`, `cell_caption`); the count is
    set by `present_knitr_cell`, so Jupyter, whose label belongs to the crossref layer, is
    unchanged. Fixtures `rich-output/fig_label_cap_one`, `_two`, `_subcap`; four unit tests in
    `knitr_inputs.rs`, which failed first.
  - `fig.sep` and `fig.ncol`: `synthesize_layout` (`presenter/knitr_inputs.rs`) builds `layout`
    from the plot count and `out.width`, else sets `layout-ncol`, as hooks.R does. Sweeps
    `options/fig.sep` (two values) and `options/fig.ncol`; six unit tests that failed first.
  - `fig.valign`: sets `layout-valign` unless `"default"`. **Divergence found in knitr:** knitr
    does not know `fig.valign`, so the live hook also forwards it as an unknown attribute
    (`fig.valign='top'`) beside `layout-valign`; the presenter matches, by not consuming it.
    Sweep `options/fig.valign`.
- [x] Language fences and the `knit_asis` fence length. `engines/shell_chunk` (a `bash` chunk) and
  the other engine fixtures are accepted, so nothing is missing for language fences. New fixture
  `rich-output/knit_asis_backticks` (three- and four-backtick runs inside asis HTML) is accepted
  as is: the fence-length code needed no change.
- [x] Unit tests of each hook feature. Present before this plan or added in it: caption and label
  dedupe (the `fig_label_*` tests, `subcaptions_win_over_the_figure_caption_and_recycle`), layout
  synthesis (the six tests above), class assembly (`the_cell_classes_follow_hooks_rs_order`),
  the `results: asis` bare rule (`results_asis_writes_text_bare_and_drops_the_wrapper` and
  the two after it), unknown-option forwarding (above), indent for list-nested chunks
  (`a_list_nested_structured_cell_keeps_its_indent_on_every_line`), and fence length for
  backtick runs (`test_format_outputs_grows_ticks_for_backtick_output`,
  `fenced_echo_lengthens_the_fences_around_backticks_in_the_code`).

## Phase 2bC: close-out

- [x] Per decision 13, R half: on a `Fallback` route R skips capture for the whole document and the
  diagnostic is surfaced once. Done by KNR-3 slice 3: `target_route::plan_capture` is called from
  `KnitrEngine::execute` (`engine/knitr/mod.rs:273`); see the KNR-3 plan\'s checklist.
  LaTeX figure attributes (`fig-pos`) and animations are the LaTeX epic\'s concern.
- [x] Dependencies of a hidden widget: handed to KNR-3 and KNR-6 as proposed below; nothing to do
  in the presenter. (The proposal, kept for the record: an `include=FALSE` chunk leaves no
  output in the record, so its `knit_meta` cannot hang on an output. The adapter carries it in
  the merged, record-level form, `EngineRecord.html_dependencies`, which WGT-2 keeps. At the flip,
  KNR-6 puts the record\'s merged `knit_meta` onto `ExecuteResult.html_dependencies`, which needs
  no WGT-2 contract; WGT-2 later replaces it with the neutral manifest. The widget wrapper (id
  and caption after the widget, three-tick `{=html}` fence) is done, slice 5.)
- [x] Re-run the corpus (2026-11-05, `R 4.3.2`, `knitr 1.50`, through the written record): **292
  cases** (274 before this plan\'s 18 new ones), **278 accepted** (identical or syntax-level), **7
  allowlisted**, **7 not compared** (4 consistent aborts, 3 unavailable: `gifski` twice,
  `RSQLite` once), **0 unexpected, 0 stale**. Frozen oracle: 285 match, 0 differ, 0 missing.
  The 7 allowlisted cases carry their one-line reasons in `tests/fixtures/knitr-corpus/allowlist.toml`
  (the signed-off divergences):
  - `structure/chunk_in_blockquote` and `s7/01_structure`: blank lines inside a quoted chunk
    differ (epic decision 23).
  - `markdown-stress/empty_output`, `s1/03_eval_false`, `s8/01_setup_state`: knitr writes an empty
    `.cell` wrapper for a chunk that renders nothing; the presenter writes none (decision 23).
  - `options/indent/header/00_empty` and `options/indent/yaml/00_empty`: the presenter ignores
    `indent`, because the splice re-applies container indent (KNR-3 slice 7, D8; KNR-6 decides).
- [x] Assert the gate. `corpus_report` now calls `assert_gate` after it writes the report and
  summary, so a failed gate fails a local run and still leaves the files to read. It skips
  without R, as before. Tests: `a_failed_gate_fails_the_report` (`should_panic`) and
  `a_passing_gate_is_not_an_error`. KNR-6 Phase 6A promotes this one harness, not two.
- [x] Run the corpus on the minimum and a later knitr (epic decision 17, local). Scratch libraries
  from `tests/fixtures/knitr-corpus/scratch-libs.R`, selected with `KNR_R_LIBS`: **knitr 1.44**
  (the minimum, with `evaluate 0.21` and `xfun 0.40`) and **knitr 1.52** (`evaluate 1.0.5`,
  `xfun 0.62`) both give 292 cases, 278 accepted, 7 allowlisted, 7 not compared, 0 unexpected,
  and the frozen oracle 285 match, same as 1.50 above. Same machine, R 4.3.2.

- [x] Check the presenter changes of KNR-2 and this plan build for `wasm32` and add no
  dependency on the engine (EXR-1\'s availability check). *Done (2026-11-03): `cargo check
  --target wasm32-unknown-unknown` of `crates/wasm-quarto-hub-client`, run from that directory with
  `CC_wasm32_unknown_unknown` set to Homebrew LLVM clang and the crate\'s `wasm-sysroot` headers (as
  `hub-client/scripts/build-wasm.js` does), now passes. It first failed with E0433 in
  `engine/replay.rs`: `ReplayEngine` called `knitr::knitr_claims` in the native-only `knitr`
  module (commit `5c6885686`, bd-js24dold, not on `main`). The pure function moved to
  `engine/mod.rs`. No presenter, record, engine or stage file draws a wasm warning. The
  presenter\'s imports from `engine` are `ExecutionContext` (`cell.rs`) and `units` (`document.rs`),
  both built on wasm32. Only the type-check ran, not the release build, `wasm-bindgen` or
  `wasm-opt`.* *Re-checked 2026-11-05 after this plan\'s presenter changes, same command: passes, and none of the
  seven `quarto-core` warnings is in a presenter file.*

## End-to-end check (2026-11-05, inspected)

Invocation: `cargo run --bin q2 -- render <scratch>/e2e/doc.qmd` with `execute: {record: true}`.
The document had a `{r}` chunk with `label: fig-pair`, `fig.cap`, `fig.subcap: ["First", "Second"]`,
`fig.ncol: 2` and two plots, and a second chunk with `fig.beforecode: true`. I read the output:

- The record, `.quarto/execution-records/doc.ipynb`, has two structured cells (2 and 1 outputs).
- In `doc.html`: `<div id="fig-pair" class="quarto-float ...">` with `fig-pair-1` and `fig-pair-2`
  (and their captions), `alt="First"` and `alt="Second"`, so the label reached the cell and the
  images. The figure files sit at knitr\'s own paths, `doc_files/figure-html/fig-pair-1.png`,
  `fig-pair-2.png` and `unnamed-chunk-1-1.png`; no generated `cell-<n>-output-<m>` name was used.
- In the second chunk the `<img>` (line 72) comes before the source (line 75), as
  `fig.beforecode` requires.
- The same document without `record: true` (the live path) gives **byte-identical HTML**
  (`diff` of the two files: 0 lines).

Not checked: a Windows build (`cargo clippy --target x86_64-pc-windows-msvc` cannot cross-compile
`aws-lc-sys` here, so the figure-path code, which uses `Path::join` on forward-slash paths, is
unverified on Windows); the `wasm32` release build, `wasm-bindgen` and `wasm-opt` (only the type-check ran).

## Exit criteria

- Zero blockers on the corpus (the allowlist entries are signed off, per epic decision 26),
  on the minimum and the latest supported knitr, with the counts and versions reported.
- `latex`, `beamer`, `pdf` and uncovered targets route to fallback units with one diagnostic
  (the Rust half is done in KNR-2; the R half is KNR-3\'s capture call).
- `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core` are green, counts reported against the live baseline.
- Jupyter output is unchanged on its goldens.
- The presenter builds for `wasm32`.
