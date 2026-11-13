---
title: 'OPR-7j: group caption and panel ids for fig-subcap cells, for every engine'
date: 2026-11-10
description: 'The presenter applies the group rules of knitr''s hooks.R to every engine: a cell with a figure label, fig-cap, fig-subcap and two or more plots keeps its group caption and gives each panel its own label-N id. Written with both reviews folded in; the checklist shows which steps have landed and which remain.'
status: complete
depends-on: [opr-7f]
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Blocks:** [OPR-9b](2026-11-10-opr-9b-presenter-follows-target.md) and [OPR-9f](2026-11-10-opr-9f-run-the-gate.md), which list `opr-7j` in their `depends-on`. OPR-7d, which this plan was written for, is now a cancelled tombstone split into OPR-9a to 9g.
**Reviews, both folded in below:** [first-round plan review](../research/2026-11-10-opr-7j-plan-review.md) (design, edge cases,
Quarto 1 evidence) and [implementability review](../research/2026-11-10-opr-7j-implementability-review.md) (could an agent
run the plan cold). The second complements the first and disagrees with it in three places, noted where they matter.
**Related, not replaced:** [subfloat parity draft](2026-11-09-subfloat-parity-draft.md) (`bd-plcqhfcn`, `bd-gbaykhth`,
`bd-au5gts6e`); [OPR-7d plan review](../research/2026-11-10-opr-7d-plan-review.md), items 15 and 16, the original evidence.

**Where it stands.** Complete (second run, 2026-11-10). The core change landed on `feature/one-presenter` (commits `3d50f2443`, `959fed073`, `c765b539c`). This
plan was then reopened for its unchecked boxes; the second run finished them (see "Notes from the run").

## Overview

On a plain render, Julia `fig-subcap` groups go through Quarto 1's TypeScript conversion. After OPR-7d's flip they go
through the presenter, which would drop two things the old route has: the group caption text and the per-panel ids
(`fig-two-1`, `fig-two-2`), so `@fig-two-1` stays unresolved. Native Jupyter never had them on either route (no `fig-two` id,
no caption, three unresolved references, three warnings). knitr is correct and is the model.

The cause is one gate. The presenter wrote the group caption, the label on the cell div and the `label-N` panel ids only when
`figure_count` was known, and only the knitr path sets it (`label_on_cell` and `plot_image_id` in `presenter/options.rs`,
copied from `hooks.R`).

**Goal.** The group rules of `hooks.R` apply to every engine that goes through the presenter. A cell with a figure label,
`fig-cap`, `fig-subcap` and two or more plots gets: the label as the cell div's id, the `fig-cap` as the group caption that
closes the div, and the id `label-N` on panel N. A Julia, Jupyter and knitr group then render the same way.

**Not in scope.** Real subfloats (panels numbered "Figure 2 (a)" under one parent, `bd-plcqhfcn`); tables (a `tbl-cap` plus
`tbl-subcap` group renders the same on every engine and route today, with the caption text lost and the panel references
unresolved, so the flip changes nothing for it; the gate here is figure-only); layout options; other formats' existing
differences; `bd-au5gts6e`.

## Evidence

- **The markup is Quarto 1's own; the numbers are a q2 accident** (first-round review, finding 1; ran on a Quarto 1 build,
  relied for the exact lines). On `jupyter/fig-subcap` Quarto 1 gives the group caption "Figure 2: A group.", panels
  `div#fig-two-1` and `div#fig-two-2` captioned "(a) First." and "(b) Second.", and references "Figure 2", "Figure 2 (a)",
  "Figure 2 (b)". Source (read): the panel id is `label + "-" + nextOutputSuffix++` (`to-markdown.ts`), the closing caption
  is `cellCaption`, and the cell label is gated by `shouldLabelCellContainer` (`labels.ts`, which does not label the cell for
  one output). The Julia engine calls the same `toMarkdown`, so Jupyter stands in for Julia.
- **What q2 does for Julia today** (rendered, 2026-11-10; the same page as the pre-epic commit `7016d595a`):

  | Difference | Plain Julia | Recorded route before 7j |
  |---|---|---|
  | Group caption | "Figure 4: A group." (number wrong) | lost; a bare "Figure 2", class `quarto-uncaptioned` |
  | Panel ids | `div#fig-two-1`, `div#fig-two-2` | none |
  | `@fig-two-1`, `@fig-two-2` | resolve (Figures 2, 3) | `?fig-two-1?`, two warnings |

- **A prototype** (two edits in `options.rs`, reverted) rendered Julia, Jupyter and knitr groups alike with no warnings, and
  810 presenter, knitr, subfigure and Julia tests passed with it. It had two defects the design below removes: a one-plot,
  one-subcap group became two figures, and a generic `figure_count` (the outline's fix) would have broken single-figure
  labeled Jupyter cells.
- **Other targets** (first-round review, finding 10; ran on a Jupyter group, relied): html and revealjs match knitr
  byte-for-byte on the caption, id and reference lines; gfm gives a nested `div#fig-two` with two panels; typst gives
  `#quarto_super(... subcapnumbering: "(a)", label: <fig-two>)` with `<fig-two-1>` and `<fig-two-2>`, the shape knitr
  already writes (head wrote an unlabeled grid); docx matches knitr; epub shows "(a)", "(b)" captions as knitr does; pptx
  shows the group on no slide before or after. The new markup is safe on these classes; 7j extends an existing
  inconsistency to Julia and Jupyter and creates none. Julia on non-HTML targets was not reproduced.
- **The figure directory is presenter-wide** (finding 11, and the implementability review, finding 8): `FsFigureSink`
  writes `{files_dir}/figure-html/` for every format, where knitr names `figure-<format>` and Quarto 1 writes
  `figure-commonmark` for gfm. Native Jupyter already writes `figure-html` on gfm. It is not a 7j concern and is 7d's item,
  which is why this plan's parity gate is HTML only.
- **Nothing outside the presenter reads these ids or captions** (finding 14: no hub-client or ts-packages code does; the
  preview renderer treats a nested Figure generically; not seen in a browser).

## Design (settled; the code follows these)

1. **Where the count lives.** `CellOptions` carries `plot_count: Option<usize>` with `with_plot_count`. It is set on a clone
   at the top of `render_cell_whole` and in `render_cell_interleaved` after `select_by_kind` (both exits: it can return
   through `render_cell_whole` when anchors are unusable), from `result.outputs` with `is_plot_output &&
   !is_included_graphic`. Not in `CellInputs::from_cell`: an expanded unit never reads `CellInputs` (`present_cell_step`
   returns into `render_expanded_unit`, which builds `CellOptions` per expansion), so the count would be missing there and
   every test written against `render_cell_with` would still pass. Not from the `figures` counter, which is the panel index.
   Not in `wrap_cell`, which sees only options and the finished body.
2. **The rule.** `label_on_cell` and `plot_image_id` read `figure_count` first (knitr, untouched). With `None`, a group needs
   `plot_count >= 2` and the panel id is `label-<index+1>`. A one-plot group is not a group: the label stays on the image and
   `fig-cap` is the caption, as in knitr, plain Julia and Quarto 1. Do not fill `figure_count` for every cell: OPR-1a records
   that it is filled only for cells with resolved options because filling it for Jupyter would change figure output, and
   `plot_image_id` with `Some(1)` writes a bare `#label` on the image, which would change every single-figure labeled
   Jupyter cell and break the byte-identical rule (D14).
3. **No engine-id branch** in `cell_caption`, and no `EngineAttrs::id()` getter. The branch fires only when the presenter has
   no figure label but the engine wrote a `fig-` id, a different gate from the one relaxed here, and `build_head` already
   lets the presenter id win over the engine id. If the Julia-shaped test ever needs the branch, state its case first.
4. **Counters and mixed groups.** `figures` in `interleave_group` is declared once per cell group and is not reset per run, so
   plots from several display calls number `label-1`, `label-2` (a real Julia cell with `println` and two `display` calls
   gave no duplicates, echoed or not). It also counts included graphics, which carry no id, so a plot beside an included
   graphic is numbered by position; the plot count excludes included graphics, so a plot plus one included graphic is not a
   group. A plot whose image write fails still counts; write failures are reported elsewhere (`image_failures`), and the
   group markup follows the count, not the writes.
5. **Preview and splice.** `present_document` and the splice stage both call `present_units`, which reaches the cell step, so
   a test at that level covers both. The splice writes the AST back as qmd with its own attribute quoting (OPR-1a,
   Deviations), so the claim is "same AST and page", not "same bytes". A unit with stored markdown, or hoisted outputs
   removed by `without_outputs`, takes other branches first; none of those is a plot.
6. **`cell_div_id` has no caller** (read). Keep it consistent (it reads `label_on_cell`); give it no new work.
7. **Edge cases** (the first-round review rendered them with the prototype; "same" means the same page as knitr unless
   stated). Same as knitr and plain Julia: more subcaps than plots; `layout-ncol` and `layout-nrow` (panel references
   resolve); `fig-cap` without `fig-subcap` (unchanged); `fig-subcap` without a label (no ids); `echo: false`,
   `results: asis`, `include: false` (nothing renders; references stay unresolved as today). Different from plain Julia,
   accepted because it is knitr's rule: fewer subcaps than plots recycle (C1, C2, C1; plain left the third panel
   uncaptioned); `fig-cap` as a list uses its first entry as the group caption (knitr itself fails on a list `fig-cap` in
   these renders, a separate engine bug). `output: false` gives the group caption with no panels, as plain does. Do not
   promise resolved panel references for `fig-subcap: true`: they stay unresolved on every engine, head and knitr included.
   A non-`fig-` label (`foo`) keeps the recorded route's behavior (only `foo` on the cell, no panel ids; plain Julia wrote
   `foo-1`, `foo-2`); 7j does not change it, the flip does; list it as a known difference in 7d's notes.
8. **Contradictions checked** (first-round review, finding 9): none with the `.cell` wrapper contract (the group is still one
   `Div.cell` and the id moves onto it; a hidden group emits no stray caption), the transform-phase rule, the no-DOM-
   postprocessor rule (presentation-time cell markdown, format-agnostic), OPR-1b, 4d, 7f (the wrapper id already wins) or
   7i (complete; its label concerns page dependencies; nothing to rebase).

## Decisions (Gordon, 2026-11-10)

- **D14: apply to every engine, Jupyter included.** The epic said Jupyter output stays byte-identical unless a cell sets a new
  option. A Jupyter `fig-subcap` cell silently lost its group caption and panel references; Gordon accepted the change and
  overrode the byte-identical rule for this one case. Every other Jupyter output must stay byte-identical.
- **Later-figure numbering: accepted as no worse than before** (recorded in D14). A group of `n` panels counts as `n + 1`
  figures: each panel is a figure with its own id, and the group is numbered after its panels. Quarto 1 counts the group as
  one figure with sub-panels (a), (b). After a group, every later figure is `n` too high against Quarto 1; today, for
  Jupyter, it is one too low (the group is not a figure). Knitr and plain Julia already behave this way. The fix is the
  `parent` link of the subfloat plan, which 7j neither needs nor blocks. The last column below is **derived, not
  rendered** (the other columns were rendered on stored `.ipynb` files with a one-pixel image in every output); a box below
  renders it.

  | Document (Jupyter) | Quarto 1 | q2 today | q2 with 7j (derived) |
  |---|---|---|---|
  | one figure, one group of 2 panels, one figure | group 2, later 3 | no group figure, later 2 | group 4, later 5 |
  | same, group of 3 panels | group 2, later 3 | no group figure, later 2 | group 5, later 6 |
  | one figure, two groups of 2 panels, one figure | groups 2 and 3, later 4 | no group figures, later 2 | groups 4 and 7, later 8 |
  | a group of 2 panels alone | group 1 | no figure | group 3 |

- **A user-written panel id is author error.** A group cell whose own output writes `{#fig-l-1}` plus two plots gets
  `id="fig-l-1"` twice and a broken caption (head had no duplicate). The presenter cannot see an id the engine put in an
  output's markdown. Pin the output with a test so a later change is a visible decision. No plan for skipping clashing ids.

## Fit with the subfloat draft

A small step toward the draft, no contradiction. The panel ids and the group caption are the half of the work that creates
panel identity; the `parent` link in the index is shared by every design and is the other half, which this plan leaves
alone. The ids are created in the presenter, which is the draft's option B direction, for the current Quarto 1 behavior only;
Phase 3 may move it, and the knitr path (option A) is unchanged, so the draft's "who wins" rule is not triggered. The draft
says nothing in it blocks 7d; this plan does, as a prerequisite. The first-round review already added the Jupyter later-figure
numbers to the draft's Interaction paragraph and Phase 2 baseline.

## Checklist (tests first; every box is `[unattended]`)

Order: claim, tests, implementation, guards, hand evidence, gates, documents, land. `[x]` means done and landed; every `[ ]`
is still to do on `opr/7j-group-caption-and-panel-ids` (stay on it; do not create another branch).

**Claim**

- [x] Claim the plan (`status: in-progress`, commit, ff-merge in `workspace-1`): `3d50f2443`.

**Tests first** (in `crates/quarto-core/src/presenter/cell.rs`, module `option_tests`, helpers `opts`, `render_figs`, `plot`;
watch each fail for the stated reason before implementing)

- [x] **The first test**: a cell with label `fig-two`, `fig-cap` "A group", `fig-subcap` `["First", "Second"]`, no
      `figure_count`, two PNG outputs. Assert `::: {#fig-two .cell}`, `){#fig-two-1}`, `){#fig-two-2}`, and that the cell ends
      `\nA group\n:::`. On the unchanged tree it failed with `::: {.cell}`, no id, no panel ids.
- [x] **Negative controls** (they pass before and after; their job is to fail after a wrong change, so a passing control is
      not a defect): (a) `fig-cap` with no `fig-subcap`, two plots: no group; (b) one plot, one subcap: no `fig-two-1` (catches
      the prototype's bug); (c) a single labeled figure: no id (the D14 guard a generic `figure_count` would break).
- [x] More cases: subcaps recycle over three plots; a plot plus an included graphic is not a group; an expanded unit with two
      plots renders the group (`expansion.rs`); a `present_units` group test (`document.rs`, next to the helper
      `reports_for`); an engine `quarto_attrs` id on a plot loses to the generated id.
- [x] **Edge cases, one test each** (Design rule 7): `layout-ncol` and `layout-nrow`; `fig-subcap` without a label; `echo:
      false`, `results: asis`, `include: false`; `output: false`; `fig-subcap: true`; a non-`fig-` label (`foo`); `fig-cap` as a
      list. (Recycled subcaps and `fig-cap` without `fig-subcap` are done above.)
- [x] **Plot plus a widget**: a labeled group cell with a plot and a widget output (`application/vnd.jupyter.widget-view+json`,
      which `has_widget_view` in `document.rs` reads). Pin what renders.
- [x] **A knitr fallback chunk** (no resolved options, so no `figure_count`): build a `RecordCell` without resolved options
      (through `present_cell_step` it takes the `CellInputs` default path) with a label, `fig-cap` and `fig-subcap`. Say what
      renders; if it takes the generic path, name that in the notes (the same intended change as Julia and Jupyter).
- [x] **The user-written panel id**: the duplicate is not reproducible at unit level, because the cell step does not render a
      `text/x-quarto-markdown` output; a test pins instead an engine `quarto_attrs` id on a plot
      (`an_engine_written_panel_id_loses_to_the_generated_one`, done). Reproduce the real duplicate through `present_units`
      or a real Julia notebook and pin it, or record in the notes that it stays untested and why.
- [x] **A test through the binary**, `crates/quarto/tests/integration/jupyter_subcap_group_cli.rs` (registered in `main.rs`),
      model `no_execute_cli.rs`: the stored parity notebook `jupyter/fig-subcap/doc.ipynb` (`include_str!`, a repo path, not
      `external-sources/`) renders the group caption, the `fig-two`, `fig-two-1`, `fig-two-2` ids and no unresolved
      reference. It runs in CI (a stored notebook needs no kernel). Julia and knitr versions are local evidence only.

**Implement**

- [x] Implement Design rules 1 to 3 in `options.rs` (`plot_count`, `with_plot_count`, `label_on_cell`, `plot_image_id`) and
      `cell.rs` (`count_plots`, set in `render_cell_whole` and in `render_cell_interleaved` after `select_by_kind`). Reuse
      `label_on_cell`, `cell_caption` and `plot_image_id`; no second code path.

**Guards**

- [x] **Jupyter byte-identity (D14).** The parity harness cannot guard Jupyter: both of its routes present through one function,
      so `jupyter/fig-subcap` compares a render with itself. Build `q2-base` before the first production edit (`cargo build
      --bin q2`, copy `target/debug/q2` to the scratchpad) and `q2-new` after; no second worktree (disk is tight). Render
      eight stored notebooks (the six `jupyter-markdown-goldens/ipynb-*.ipynb`, parity `jupyter/{ansi-output,fig-subcap}/
      doc.ipynb`) into two directories with `render X --to html` and run `diff -r -x render-manifest.json -x '*.err'`; only
      absolute paths differ otherwise. Result: only `parity-fig-subcap.html` differs, and its three unresolved-crossref warnings
      went to none. The 13 shadow goldens are guarded by `-E 'test(jupyter_markdown_goldens) | test(presenter_pinning) |
      test(record_preview_splice)'` (they pass in the `quarto-core` run). Expected changed `.snap` or golden files: zero.
- [x] **Parity gate, HTML only**: `OPR_PARITY_REPORT=<scratchpad>/report.md cargo nextest run -p quarto-core -E
      'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture` (needs R, Julia and Plots; a skipped case
      is "not compared", not passed). Result with all three present: 68 identical, 7 allowlisted, 4 blockers, 0 skipped. The
      four blockers are exactly `julia/capture-facts`, `julia/presentation-attrs`, `julia/records-plot` and `julia/fig-subcap`
      (7d's four interleaving cases, which 7d allowlists in its own step); `julia/fig-subcap` differs only by interleaved
      code blocks; `knitr/fig-subcap` and `jupyter/fig-subcap` are identical. **Do not add `gfm`, `typst` or `revealjs` to the
      three `fig-subcap` manifest entries**: each adds a figure-directory blocker (`figure-html` against `figure-<format>`)
      that is 7d's item (its "Julia on non-HTML targets" step 5 adds them), and the gate could not then pass.
- [x] **Snapshots**: 0 `.snap` or golden files added, changed or removed (`git status` after the gates).

**Hand evidence**

- [x] **Per-format hand renders** (the harness does not check these formats for `fig-subcap` before 7d): render
      `crates/quarto-core/tests/fixtures/opr-parity/jupyter/fig-subcap/doc.ipynb` with the binary (`q2 render doc.ipynb --to
      <format>`) on html, revealjs, gfm, typst, docx, epub and pptx; read the caption, id and reference lines and write them
      in the notes. Expected from the first-round review: html and revealjs match knitr; typst writes the `quarto_super`
      shape; docx matches knitr; epub shows "(a)", "(b)" captions; pptx shows the group on no slide. Say in the landing report
      that gfm, typst and revealjs are checked only by these hand renders.
- [x] **Render the later-figure table**: the four rows (stored `.ipynb` files with a one-pixel image in every output) with the
      changed binary; correct the derived column where a row is wrong.

**Gates** (re-run all of them after the open boxes above; the first run's results are in the notes)

- [x] `cargo clippy -p quarto-core -p quarto --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the binary
      test in `quarto` (first run: clean, 8245 passed, binary test passed).
- [x] `cargo xtask lint` (first run: one `metadata-as-str` violation in `crates/quarto-core/src/positron_cache.rs:360`, a file
      this plan does not touch; do not fix it, say so).
- [x] Windows cross-clippy `cargo clippy -p quarto-core --all-targets --target x86_64-pc-windows-msvc -- -D warnings`: a
      failure inside a build script (`aws-lc-sys`; `wasmtime` needs `setjmp.h`) is the known macOS issue; record it and go
      on. The change adds no `cfg` code.
- [x] WASM, because `presenter/` compiles for `wasm32`: `npm install` from the repository root if `hub-client/node_modules`
      is missing (revert any `package-lock.json` change), then from `hub-client`: `npm run build:wasm` and `npm run test:wasm
      -- recordPayload captureSpliceHtml` (the two vitest files that pin splice HTML). First run: passed, 2 files, 8 tests. No
      file under `hub-client/` changes, so no changelog entry.

**Documents**

- [x] **Stale comments** in `crates/quarto-core/src/crossref/codeblock_shorthand.rs` (about lines 164 to 175 and 1165 to 1181)
      said a `fig-subcap` cell is left unwrapped so the engine can synthesize the panel ids; reworded to say the presenter
      does it for every engine. Behavior unchanged.
- [x] **Allowlist header and manifest note**: the sentence in `opr-parity-allowlist.toml` that calls group references "not an
      allowlist candidate" is replaced, and the `julia/fig-subcap` manifest note no longer says "blocker". 7d's checklist
      updates the same header for the interleaving entries; whichever plan lands second makes the final edit.
- [x] **Re-read 7d and the subfloat draft** and correct anything the implementation changed (the `CellOptions` count and the
      generic rule), or say that nothing needed it.
- [x] **Read the first-round review against this plan again** after the open boxes, to confirm nothing in it is missing
      (this rewrite folded its findings 1 to 17 and its "first test" section; it did not re-verify its relied claims).
- [x] **Docs note for OPR-8**: `docs/guides/authoring/figures.qmd` (about line 474) promises "(a)" subcaptions for computed
      output; HTML shows "Figure 2: First." on every engine, and q2 numbers later figures differently from Quarto 1. The
      `migrating-markdown` page has no subfigure entry. Do not edit `docs/` here; the note is in "Notes from the run".
- [x] **THE FLIP**: moot. Gordon set 7d to `ready` himself (commit `260349df9`); verified, nothing changed in 7d. If a
      re-run ever needs it: after this plan is `complete`, every plan in 7d's `depends-on` is `complete` (7j counted), the
      parity report lists as blockers exactly the four interleaving cases, every allowlist entry is `signed`, and a grep for
      "needs Gordon" over 7d and the epic prints nothing open.

**Land**

- [x] Reconcile this checklist against what landed (verify each box against the tree, not against its tick), fill in "Notes
      from the run", set `status: complete`, `git rebase feature/one-presenter` (stop and sweep for conflict markers after any
      conflict), re-run the gates, ff-merge from `workspace-1`, verify `git log`. Render this note with `q2 render --strict`
      (only the known Q-12-13 errors are acceptable).

## Stop conditions

Stop, commit progress, add "Notes from an earlier attempt", set `ready`, land only the notes commit and report, if: (1) the
first test passes on the unchanged tree; (2) any Jupyter golden or snapshot changes, or the binary diff shows a changed
notebook other than the `fig-subcap` one; (3) the one-plot group renders two figures; (4) the count has to be filled in
`CellInputs` to pass a test (an expansion path was missed: say which); (5) the parity report shows a blocker other than the
four interleaving cases (html); (6) the machine cannot build a second `q2`, or the base binary cannot be made before a
production edit; (7) a gate fails for a reason you cannot attribute to your change.

## Exit criteria

- A labeled `fig-subcap` group with two or more plots renders the same group caption, panel ids and resolved references for
  Julia, Jupyter and knitr; the Julia recorded page matches plain Julia on those; the parity case differs only by interleaving.
- No new warnings; knitr output unchanged; the splice route matches the render route; every Jupyter output other than a
  `fig-subcap` group is byte-identical (the guard above).
- Panel numbering is unchanged for knitr and Julia (Figure 2, 3 for the panels, 4 for the group); Jupyter's later-figure
  numbers change as in D14; the notes say real subfloats remain `bd-plcqhfcn`.

## Notes from the run

First run, 2026-11-10, lane C (`workspace-5`), one agent, no sub-agents.

**What landed.** `CellOptions.plot_count`, set in `render_cell_whole` and `render_cell_interleaved`; `label_on_cell` and
`plot_image_id` use it when there is no figure count. `CellInputs`, `cell_step.rs` and the production code of `expansion.rs`
and `document.rs` are untouched. Snapshot and golden files changed: 0.

**Gate results.** Clippy and the `quarto-core` nextest (8245 passed, 36 skipped), the binary test, the HTML parity run and the
WASM build and vitest files passed. `cargo xtask lint` reported the one violation named above, which this plan does not
touch. The Windows cross-clippy failed in build scripts, the known macOS issue.

**What the first run over-claimed, and why the boxes above are open.** It ticked the Targets, Gates and "re-read 7d and the
draft" boxes more generously than its work supported: it did no per-format hand renders, `cargo xtask lint` and the Windows
cross-clippy did not pass, and it did not re-read 7d or the draft. It never opened the first-round review, so its findings
were not checked against the plan. It skipped the edge-case tests, the widget test, the fallback-chunk test and the render
of the later-figure table, and it could not reproduce the user-written id duplicate at unit level. It also applied the
implementability review's placement rule, recipe and Windows/WASM handling from its prompt, not from the plan file; they are
now in this plan.

**Docs note for OPR-8.** `docs/guides/authoring/figures.qmd` (about line 474) promises "(a)" subcaptions for computed output;
HTML shows "Figure 2: First." on every engine, and q2 numbers later figures differently from Quarto 1 (a Jupyter group counts
as `n + 1` figures). The `migrating-markdown` page has no subfigure entry; add one.

**Flip.** OPR-7d was already `ready` (Gordon, commit `260349df9`); its epic-table row says `ready`; nothing changed in 7d.

## Second run, 2026-11-10, lane C (`workspace-5`), one agent, no sub-agents

Ran, on branch `opr/7j-group-caption-and-panel-ids`. Snapshot and golden files changed: 0.

**Tests added** (all pass on the first run, as expected: they pin behavior the core change already produced; the
failing-first test is the first one above). In `presenter/cell.rs`, `option_tests`: `layout_options_keep_the_group_and_panel_ids`
(`layout-ncol` and `layout-nrow` both keep the group and ids), `fig_subcap_without_a_label_has_no_group_and_no_ids`,
`echo_false_still_renders_the_group`, `results_asis_does_not_change_a_plot_group`,
`a_hidden_group_emits_nothing_and_no_stray_caption` (`include: false`), `output_false_gives_no_panels` (caption stays, no
panels), `fig_subcap_true_keeps_ids_with_empty_subcaptions` (the ids exist at the cell level; the references stay unresolved
on every engine, as the plan says), `a_non_fig_label_keeps_the_recorded_routes_behavior`,
`a_list_fig_cap_uses_its_first_entry_as_the_group_caption`, and `a_plot_and_a_widget_is_one_plot_so_not_a_group` (the
widget is not a plot, so the count is 1: one figure plus the widget's text fallback, no group). In `presenter/cell_step.rs`:
`a_chunk_without_resolved_options_renders_the_group_on_the_generic_path` (no `quarto_resolved_options`, so no
`figure_count`: the cell takes the generic path Julia and Jupyter take and renders the group; the same intended change).
Note: `echo`, `include` and `output` are `CellVisibility`, not `CellOptions`, so those three tests drive the visibility
argument.

**The user-written panel id stays untested.** The cell step does not render a `text/x-quarto-markdown` output, and a
hand-written `{#fig-l-1}` lives in such an output's markdown, so `present_units` would not reach it either; I did not
build a real Julia notebook for it. The pinned neighbor is `an_engine_written_panel_id_loses_to_the_generated_one`. It
stays author error with no skipping of clashing ids, as decided.

**Per-format hand renders** (binary `target/debug/q2`, `render doc.ipynb --to <format>` on
`opr-parity/jupyter/fig-subcap/doc.ipynb`; the fixture sets `layout-ncol: 2`):

| Format | Caption, id and reference lines read |
|---|---|
| html | `div#fig-two` (class `cell`), panels `fig-two-1` "Figure 2: First." and `fig-two-2` "Figure 3: Second.", group caption "Figure 4: A group."; refs "Figure 4", "Figure 2", "Figure 3"; 0 warnings |
| revealjs | the same ids, captions and refs as html; 0 warnings |
| gfm | nested `div#fig-two`, `div#fig-two-1`, `div#fig-two-2`, captions as html, refs 4, 2, 3. One warning `Q-11-1` "No renderer for PanelLayout": it comes from the fixture's `layout-ncol` (it disappears when that option is removed), not from the group |
| typst | `#quarto_super(... subcapnumbering: "(a)" ...)` with labels `<fig-two>`, `<fig-two-1>`, `<fig-two-2>` (read in the kept `.typ`); the references are `#ref(<fig-two>)` and so on |
| docx | paragraphs "Figure 1: One figure.", "Figure 2: First.", "Figure 3: Second.", "Figure 4: A group."; refs "Figure 1, Figure 4, Figure 2, Figure 3, Table 1" |
| epub | captions "(a) First.", "(b) Second.", "Figure 2: A group." (panels in a `quarto-layout-panel`); refs 4, 2, 3 |
| pptx | the group is on no slide (slide 2 holds only "Figure 1"); two `Q-11-1` warnings "PowerPoint output for FloatRefTargets require a single image as content" remain with `layout-ncol` removed. Not compared with the base binary (none was built this run); the first-round review saw the group on no slide before or after |

gfm, typst and revealjs are checked only by these hand renders; the parity manifest still lists html only for the three
`fig-subcap` entries (7d adds the rest). The figure directory is `figure-html` on every format (7d's item).

**The later-figure table, rendered** (generated notebooks with one-pixel images, HTML, changed binary; 0 warnings each):
a group of 2 between two single figures gives group Figure 4 and the later figure Figure 5; a group of 3 gives 5 and 6; two
groups of 2 give 4 and 7, later 8; a group of 2 alone gives Figure 3. **The derived column was right in all four rows; no
correction.**

**Gates, second run.** `cargo clippy -p quarto-core -p quarto --all-targets -- -D warnings`: clean. `cargo nextest run -p
quarto-core -p quarto`: 9117 passed, 38 skipped (includes `jupyter_subcap_group_cli`). `cargo xtask lint`: one violation,
`metadata-as-str` at `crates/quarto-core/src/positron_cache.rs:360`, which this plan does not touch and predates it (not
fixed). Windows cross-clippy: failed in the `aws-lc-sys` build script (cc cannot target windows-msvc from macOS), the known
issue; the change adds no `cfg` code, so it was not checked past that. WASM: `npm run build:wasm` passed; `npm run test:wasm
-- recordPayload captureSpliceHtml` passed (2 files, 8 tests). No `hub-client/` file changed, so no changelog entry.

**Documents.** 7d: its reference to a 7j heading "Decided: later-figure numbering" pointed at nothing; it now says "decision
D14, Later-figure numbering". Nothing else in 7d needed a change (it already treats 7j as the Julia group fix and leaves
the three `fig-subcap` manifest entries to html). Subfloat draft: its Interaction paragraph said a Jupyter group "counts as three
figures ... two too high"; it now says n + 1 figures and n too high, from the rendered rows. The first-round review was read
against the plan again (findings 1 to 17): all are folded in; finding 6's two unrendered cases (plot plus graphic, plot plus
widget) are now tests.

**After the second rebase.** OPR-7d was cancelled and split into OPR-9a to 9g while this run was landing. The only conflict was
my one-line fix in the 7d file, dropped in favor of the tombstone; the same stale pointer in
[OPR-9g](2026-11-10-opr-9g-flip-plain-render.md) is fixed there. The code and gates are unchanged by that rebase
(the integration branch added plan files only after the gate run at `d446c7bb6`).
