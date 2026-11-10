---
title: 'OPR-7j: group caption and panel ids for fig-subcap cells, for every engine (OUTLINE)'
date: 2026-11-10
description: 'Outline, not yet reviewed by Gordon. Makes the presenter apply knitr''s hooks.R group rules to every engine: a cell with a figure label, fig-cap and fig-subcap keeps its group caption and gives each panel its own label-N id. This removes the Julia regression OPR-7d would otherwise ship (lost group caption, unresolved panel references) and fixes the same two gaps for native Jupyter, without building real subfloats.'
status: ready
depends-on: [opr-7f]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Blocks:** [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (it lists `opr-7j` in its `depends-on`).
**Evidence:** [OPR-7d plan review](../research/2026-11-10-opr-7d-plan-review.md), items 15 and 16 (superseded by this plan).
**Related, not replaced:** [subfloat parity draft](2026-11-09-subfloat-parity-draft.md) (`bd-plcqhfcn`, `bd-gbaykhth`, `bd-au5gts6e`).
**Status:** `ready` (Gordon, 2026-11-10). The Jupyter question and the two review questions are decided (below).

## Why this plan exists

On a plain render, Julia `fig-subcap` groups go through Quarto 1\'s TypeScript conversion. For OPR-7d\'s flip they go
through the presenter instead, and the presenter drops two things the old route had (all rendered, 2026-11-10; the
same page as the pre-exr commit `7016d595a`, so the plain route is unchanged by the epics):

| Difference | Today (plain Julia) | Recorded route |
|---|---|---|
| Group caption text | "Figure 4: A group." (number wrong) | lost; a bare "Figure 2", class `quarto-uncaptioned` |
| Panel ids | each panel is `div#fig-two-1`, `div#fig-two-2` | none |
| `@fig-two-1`, `@fig-two-2` | resolve (wrong numbers: Figure 2, 3) | `?fig-two-1?`, unresolved, 2 warnings |

Native Jupyter already behaves like the recorded route on both routes, so Jupyter users have lost the group caption and
every group reference all along: no `fig-two` id, no caption, three unresolved references and three warnings. knitr is
correct in this respect and is the model.

The cause is one gate. The presenter writes the group caption, the label on the cell div and the per-panel
`label-N` ids only when `figure_count` is known, and only the knitr path sets it (`label_on_cell` and `plot_image_id` in
`presenter/options.rs`, copied from `hooks.R`). Julia and Jupyter never pass it.

## Goal

The group rules of `hooks.R` apply to every engine that goes through the presenter. A cell with a figure label,
`fig-cap` and `fig-subcap` gets: the label as the cell div\'s id, the `fig-cap` as the group caption that closes the
div, and the id `label-N` on panel N. After this plan a Julia, Jupyter and knitr group render the same way.

## What this plan does not do

Real subfloats: the model where panels are numbered "Figure 2 (a)" under one parent (`bd-plcqhfcn`). Numbers stay as
they are today for knitr and plain Julia: each panel counts as its own figure, and the group is numbered after its
panels (Figure 4). That wrongness predates this plan for knitr and Julia, and the subfloat plan fixes it, by making the
Rust crossref index carry `parent`. **For Jupyter it is not predated:** today the group is not a figure at all, so every
later figure is numbered one too low against Quarto 1; after 7j the group counts as three figures, so every later figure is
one too high (see "Decided: later-figure numbering" below). Quarto 1 itself numbers the group "Figure 2" with panels "(a)" and "(b)", and
the next figure "Figure 3" (review report, item 1). The plan also does not change tables (below), layout options, other
formats\' differences that exist today, or `bd-au5gts6e`.

## Fit with the subfloat draft

Verdict from the 7d review: a tiny step toward the draft, no contradiction. The panel ids `fig-x-1`, `fig-x-2` and the
group caption are the half of the work that creates panel identity; the draft says the other half, the `parent` link in
the index, is shared by every design and is what yields "(a)" numbering. This plan writes the ids and leaves `parent`
alone, so the later work reads the same markup. Two cautions, both for the plan author:

- The draft leaves open where panel identity is created (its options A, B, C) as Gordon\'s Phase 3 decision. This plan
  creates the ids in the presenter, which is option B\'s direction, for the current Quarto 1 behavior only. Say so in
  the notes, and say that Phase 3 may move it. The knitr path (option A) is unchanged, so the draft\'s "who wins" rule is
  not triggered.
- The draft says nothing in it blocks 7d; this plan does block 7d, as a prerequisite. Update the draft\'s "Interaction"
  paragraph when this lands. The 7j review added the Jupyter later-figure numbers to that paragraph and to the draft\'s
  Phase 2 baseline (done in the review session).
- The review found that the markup 7j writes (the cell id, the closing group caption, `label-N` on each panel) is Quarto 1\'s
  own (`jupyter.ts`: the per-output `label + "-" + nextOutputSuffix++` and `cellCaption` closing the div; ran on a stored
  notebook, relied from the sub-agent), so every option in the draft reads it. Only the numbers are a q2 accident.
- 7i (in progress in lane D) also edits `presenter/cell.rs`. Whichever of 7i and 7j lands second rebases onto the other;
  7i touches the page-dependency scan, not the group rules, so no semantic conflict is expected (relied: I did not diff
  7i\'s branch).

## Prototype evidence (review branch, reverted; patch kept)

Two small edits in `presenter/options.rs` (plus a one-line `id()` getter on `EngineAttrs`): `label_on_cell` no longer
requires `figure_count`, and `plot_image_id` returns `label-N` in the group case when `figure_count` is `None`. Rendered
with the review branch `q2` (all four marked ran):

| Fixture | Result |
|---|---|
| Julia `fig-subcap`, recorded | "Figure 2: First.", "Figure 3: Second.", "Figure 4: A group."; `@fig-two`, `@fig-two-1`, `@fig-two-2` resolve; no warnings. The same as plain Julia. |
| Jupyter `fig-subcap` (notebook) | the same page as Julia and knitr; the `fig-two`, `fig-two-1`, `fig-two-2` ids exist; three warnings gone |
| knitr `fig-subcap`, recorded | unchanged |
| Parity harness, `julia/fig-subcap` | the only remaining difference is the interleaved code blocks (OPR-4d) |

With both edits, 810 presenter, knitr, subfigure and Julia end-to-end tests passed (ran; `-E 'test(presenter) | test(knitr) |
test(fig_subcap) | test(subfig) | test(julia_engine_e2e)'`). The full `quarto-core` suite, clippy and the other parity
cases were not run, and the 7j review could not re-run the 810 either (its throwaway build did not finish on a loaded
machine). The patch is not committed (prototype code does not go in the tree) and lives only in a session scratchpad, so
the two edits are spelled out here: (1) `label_on_cell` drops its `figure_count.is_some()` term; (2) `plot_image_id`,
when `figure_count` is `None`, returns `#label-<index+1>` if `label_on_cell()`. A third piece, an engine-id branch in
`cell_caption` with a one-line `EngineAttrs::id()` getter, was in the first patch; the review found it unreachable once
`label_on_cell` is relaxed (see open question 1), so do not add it. The prototype is not the implementation: the cases
below are covered by the review only as listed in its report.

## Settled by the review, and what remains for the implementer

The 7j review (see "Review findings") turned the outline\'s five open questions into the rules below. The implementer
confirms each in a test before the code.

1. **The engine-id branch goes (default).** `cell_caption`\'s engine-id branch only fires when the presenter has no
   figure label but the engine wrote a `fig-` id, which is a different gate from the one 7j relaxes; `build_head` already
   lets the presenter id win over the engine id (read). Write the Julia group test without the branch first. If it passes,
   do not add the branch or the `EngineAttrs::id()` getter. If it fails, the branch needs a stated case.
2. **A group needs two plots (decision, replaces "count from the outputs").** The prototype writes `label-1` for a
   one-plot group, so a Julia cell with one plot and one `fig-subcap` entry renders two figures ("Figure 1: Only." and
   "Figure 2: Grp A.") where knitr renders one ("Figure 1: Grp A.") and plain Julia renders one (ran, re-run in the
   review). Do **not** fix this by filling `figure_count` for every cell: OPR-1a records that the builder fills it only
   for cells with resolved options because filling it for Jupyter would change figure output, and `plot_image_id` with
   `Some(1)` writes a bare `#label` on the image, which would change single-figure labeled Jupyter cells and break the
   byte-identical rule (D14). Instead add one generic input, the cell\'s plot count (`is_plot_output` and not
   `is_included_graphic`, the same filter `from_cell` uses), filled for every cell, and consulted only when
   `figure_count` is `None`: `label_on_cell` is true on that path only for a plot count of two or more, and the panel
   id is `label-<index+1>` as in the prototype. A one-plot group then renders as it does today (the label stays on the
   image, `fig-cap` is the caption; Quarto 1 does the same, `labels.ts` `shouldLabelCellContainer`). Knitr keeps its own
   `Some(n)` path untouched.
3. **Edge cases, each with a test (the review rendered them with the prototype; "same" means the same page as knitr
   unless stated).** Same as knitr and plain Julia: more subcaps than plots; `layout-ncol` and `layout-nrow` (panel
   references resolve); `fig-cap` without `fig-subcap` (unchanged); `fig-subcap` without a label (no ids);
   `echo: false`, `results: asis`, `include: false` (nothing renders; references stay unresolved as today).
   Different from plain Julia, accepted (it is knitr\'s rule): fewer subcaps than plots recycles the subcaps
   (C1, C2, C1; plain left the third panel uncaptioned); `fig-cap` as a list uses the first entry as the group caption
   (knitr itself fails on a list `fig-cap` in these renders, a separate engine bug). `output: false` gives the group
   caption with no panels, as plain does. **Do not promise** resolved panel references for `fig-subcap: true`: they stay
   unresolved on every engine, head and knitr included. A non-`fig-` label (`foo`) keeps the recorded route\'s behavior
   (only `foo` on the cell, no panel ids; plain Julia wrote `foo-1`, `foo-2`); 7j does not change it, so the flip
   changes it. List it as a known difference in 7d\'s notes; it is outside figure groups.
4. **A user-written panel id (decision: accept, pin with a test).** A group cell whose own output writes
   `{#fig-l-1}` plus two plots gets `id="fig-l-1"` twice and a broken caption (ran; head had no duplicate). The presenter
   cannot see an id the engine put in an output\'s markdown. Treat it as author error, add a test that pins the output
   (so a later change is a visible decision), and say so in the notes. If Gordon wants the presenter to skip ids that
   clash, that is a separate plan.
5. **Mixed groups and the counter.** `*figures` in `interleave_group` is declared once per cell group and is not reset per
   run, so a group of plots from two display calls numbers `label-1`, `label-2` (ran, real Julia cell with `println` and two
   `display` calls, echoed and not). It also counts included graphics (which carry no id), so a group that mixes an
   `include_graphics` image with plots numbers the plot by position. Test a plot plus an included graphic and a plot plus
   a widget; the review did not render them.
6. **A knitr fallback chunk** (no resolved options, so no `figure_count`) with a label, `fig-cap` and `fig-subcap` now
   takes this generic path. Confirm in a test whether such a chunk reaches the cell step at all; if it does, name the
   change in the notes (it is the same intended change as Julia and Jupyter).
7. **Tables:** decided above (left alone). Add one test that the table group page is unchanged by this plan.
8. **Preview and splice.** `present_document` and the splice stage both call `present_units`, which reaches the cell
   step (read, relied from the sub-agent: `document.rs`, `record_splice.rs`), so a test at the `present_units` level covers
   both. The splice writes the AST back as qmd with its own attribute quoting (OPR-1a, Deviations), so the claim is
   "the same AST and page", not "the same bytes".
9. **`cell_div_id` has no caller** (read); keep it consistent (it reads `label_on_cell`) but do not give it new work.

## Decision (Gordon, 2026-11-10): apply to every engine, Jupyter included

The epic said Jupyter output stays byte-identical unless a cell sets a new option. A Jupyter `fig-subcap` cell today
silently loses its group caption and its panel references, and this plan fixes that. Gordon accepted the change and
overrode the byte-identical rule for this one case; it is recorded as D14 in the epic. The rest of Jupyter output must
stay byte-identical, and a checklist box tests that.

**Decided: later-figure numbering (Gordon, 2026-11-10: accepted as no worse than before; recorded in D14).** The review
found that a Jupyter `fig-subcap` group changes the number of every **later** figure in the document. Gordon accepted
that, and asked for a more precise definition and more examples to judge the impact. Both follow.

*Definition.* A group cell with `n` panels, in q2 with 7j, counts as `n + 1` figures: each panel is its own figure (it
carries the id `label-N`), and the group, which carries the cell label, is numbered after its panels. Quarto 1 counts the
group as one figure with `n` sub-panels `(a)`, `(b)`, and so on. So after a group, every later figure is `n` too high
against Quarto 1 (the numbers of the panels and the group differ as well). Today, q2 Jupyter counts the group as zero
figures (no id, no caption), so later figures are one too low. The offset accumulates across groups. The rule is the
same on Jupyter, knitr and plain Julia once 7j lands; knitr and plain Julia already behave this way today.

*Examples* (derived from the definition and the one stored-notebook run in the review; **not rendered case by case**: the
implementer renders each row on Jupyter with the changed binary and the Quarto 1 column from the sub-agent\'s method, and
corrects this table where a row is wrong; rows 1 to 3 have a figure before the group and one after):

| Document (Jupyter) | Quarto 1: group / later figure | q2 today | q2 with 7j: group / later figure |
|---|---|---|---|
| one figure, one group of 2 panels, one figure | Figure 2 / Figure 3 | no group figure / Figure 2 | Figure 4 / Figure 5 |
| same, group of 3 panels | Figure 2 / Figure 3 | no group figure / Figure 2 | Figure 5 / Figure 6 |
| one figure, two groups of 2 panels, one figure | Figures 2 and 3 / Figure 4 | no group figures / Figure 2 | Figures 4 and 7 / Figure 8 |
| a group of 2 panels alone (no figure before or after) | Figure 1 | no figure | Figure 3 |

(The first row is the case the review ran; the others follow from the definition.) What a reader sees: references to later
figures resolve and read two or more too high, instead of one too low. References to the group (`@fig-l`) and to its
panels (`@fig-l-1`) resolve, which they did not before. The real fix, a `parent` link so panels are "Figure 2 (a)", is
the subfloat plan\'s (`bd-plcqhfcn`); 7j neither depends on it nor blocks it.

**Decided: a user-written panel id (Gordon, 2026-11-10).** Stays "author error" with a pinning test, as in the numbered
list above (item 4). Gordon: no worse than before, and no worse than Quarto 1. No plan for skipping clashing ids.

## Tables (tested 2026-11-10; out of scope here, nothing for the flip to fix)

A `tbl-cap` plus `tbl-subcap` group (two markdown tables in one labeled cell) was rendered on Julia, knitr and Jupyter,
plain and recorded, with and without the prototype (ran). Every one of the twelve pages is the same: `tbl-two` exists and
`@tbl-two` resolves ("Table 2"), but the group caption text is lost (class `quarto-uncaptioned`), `@tbl-two-1` and
`@tbl-two-2` are `?tbl-two-N?` with two warnings, and the panels have no ids. The pre-epic plain render (commit
`7016d595a`) of Julia and knitr gives the same page, so the flip and the epics change nothing for tables on those two
engines. (The pre-epic Jupyter plain page had no `tbl-two` id at all and three warnings; today it matches the others. I did not
find which change fixed that.) Because the gate here is figure-only (`label_on_cell` reads `figure_label`), the prototype changes none
of the table pages. So tables are a gap that exists on every engine and route, not a regression, and fixing it needs its
own work under the subfloat draft (knitr writes no table panel ids either in these renders; I did not read `hooks.R` for why). 7j leaves tables alone.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Tests first.** Unit tests in `presenter/options.rs` and `presenter/cell.rs` (the knitr pins named in the review
      are `the_label_moves_to_the_cell_div_with_both_fig_cap_and_fig_subcap`, `several_plots_number_the_figure_label`,
      `each_figure_takes_its_own_subcaption`, `captions_count_figures_across_interleaved_runs`; none has a figure count
      of `None`). First test, and its negative control, are in the review report. Watch them fail on today\'s code, for
      that reason.
- [ ] **Jupyter byte-identity guard (D14), with a mechanism.** The parity harness cannot guard Jupyter: both of its routes
      present through the same function, so `jupyter/fig-subcap` compares a render with itself. Instead render every
      Jupyter fixture (the parity corpus notebooks, the 13 shadow goldens and the six `ipynb-*` goldens through
      `present_document`) with a binary built at the merge base and one built with the change, and diff the HTML and the
      markdown; list the fixtures compared. The existing goldens are the standing guard (OPR-1a exit criteria); expect
      **zero** changed `.snap` or golden files (read: no Jupyter golden contains a real `fig-subcap` cell, the strings in
      `jupyter-markdown-goldens/*.record.json` are option-name lists). The Jupyter `fig-subcap` fixture is the one
      expected change. Add one test that a single-figure labeled Jupyter cell is byte-identical (the case a generic
      figure count would have broken).
- [ ] A test per item 3 to 6 of "Settled by the review", including the one-plot group, the user-id collision (pinned
      as author error), a plot plus an included graphic, a plot plus a widget, and the knitr output unchanged.
- [ ] **A test through the binary** (model `crates/quarto/tests/integration/no_execute_cli.rs`): a stored `.ipynb` with a
      `fig-subcap` group renders the group caption and resolves `@fig-x` and the panel references, with no
      unresolved-crossref warning. This one **runs in CI**: `q2 render doc.ipynb` uses the stored outputs and needs no
      kernel (ran in the review). Julia and knitr versions skip cleanly when the engine is absent and are local evidence
      only (CI installs a Jupyter venv, neither R nor Julia).
- [ ] **A splice test at the `present_units` level** (covers render and preview): same AST and page, not the same bytes.
- [ ] Implement: relax the gate as described in "Settled by the review", reusing `label_on_cell`, `cell_caption` and
      `plot_image_id`; do not add a second code path, and do not add the engine-id branch unless its test needs it.
- [ ] **Targets.** The group markup is format-agnostic presenter output (read), and the review rendered a Jupyter group on
      html, revealjs, gfm, typst, docx, epub and pptx with the prototype: html and revealjs match knitr; typst writes the
      same `quarto_super` shape as knitr recorded; docx matches knitr; epub shows "(a)", "(b)" captions as knitr does;
      pptx shows the group on no slide, as before. Add `gfm`, `typst` and `revealjs` to the `formats` of the
      `julia/fig-subcap` manifest entry (all three entries list only `html`), so the harness compares them; 7d adds the zip
      formats. The gfm `figure-html` directory is presenter-wide (`FsFigureSink`, `figures.rs`), not Julia-specific, and
      is 7d\'s item.
- [ ] Parity gate: run the harness; `julia/fig-subcap` differs from plain only by the interleaved code blocks, and
      `knitr/fig-subcap` stays identical on both routes. Report the caption and reference lines of the three fixtures
      before and after. (`jupyter/fig-subcap` cannot differ between routes; its evidence is the guard above.)
- [ ] Stale comments: `crossref/codeblock_shorthand.rs` (about lines 164 to 175 and 1165 to 1181) says a `fig-subcap`
      cell is left unwrapped "so the engine can synthesize" the panel ids; after 7j the presenter does that for Jupyter and
      Julia too. Reword them; behavior does not change.
- [ ] Snapshots: count and list any changed `.snap` or golden files, per the protocol (expected zero; a new committed
      Jupyter group golden would be three new files: markdown, preview markdown, record JSON).
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy; `cargo xtask lint`; `cd hub-client && npm run build:wasm` (the presenter compiles for wasm32)
      and the two WASM vitest files that pin splice HTML (`recordPayload.wasm.test.ts`, `captureSpliceHtml.wasm.test.ts`,
      as OPR-1a ran them).
- [x] OPR-7d and the subfloat draft are updated to fit this plan (done in the 7d review session, 2026-11-10): 7d
      depends on `opr-7j`, its decision 3 is replaced, the `julia-subfigure-group` normalizer is dropped from its
      design, and the draft\'s "Interaction" paragraph and known-facts table say what 7j does. The 7j review session
      (2026-11-10) added the Jupyter later-figure numbers to the draft and corrected two items of 7d\'s Julia non-HTML
      checklist. At landing, re-read both and correct anything this plan\'s implementation changed.
- [ ] Docs note for OPR-8: `docs/guides/authoring/figures.qmd` (about line 476) promises "(a)" subcaptions for computed
      output; HTML shows "Figure 2: First." on every engine, and q2 numbers later figures differently from Quarto 1. The
      migration notes say so (the `migrating-markdown` page has no subfigure entry today).
- [ ] Remove the allowlist header sentence that calls group references "not an allowlist candidate" (the header is in
      `opr-parity-allowlist.toml`; 7d\'s checklist updates it for the interleaving entries, so whichever plan lands
      second makes the final edit), and reword the `julia/fig-subcap` manifest note that says "blocker".

- [ ] **THE FLIP** (Gordon, 2026-11-10: this box moved here from OPR-7g, because 7j is the last prerequisite). Set
      OPR-7d\'s `status:` from `blocked-on-gordon` to `ready` and its epic-table row to match, **only if all hold**: every plan in
      7d\'s `depends-on` is `complete`; the parity gate shows no unallowlisted difference except the four interleaving
      cases that 7d itself allowlists (its decision 5: a step of 7d, not a prerequisite); every existing allowlist entry is
      `signed`, or you list the `proposed` ones for Gordon and do not flip (signing stays Gordon\'s); no plan note says
      "needs Gordon" about anything still open. Change nothing else in 7d except that line, and say in the landing report
      that you flipped it, or exactly what blocked it.

## Exit criteria

- A labeled `fig-subcap` group with two or more plots renders the same group caption, panel ids and resolved references
  for Julia, Jupyter and knitr; the Julia recorded page matches plain Julia on those; the parity case differs only by
  interleaving.
- No new warnings; knitr output unchanged; the splice route matches the render route; every Jupyter output other than a
  `fig-subcap` group is byte-identical (the guard above).
- Panel numbering is not changed for knitr and Julia (Figure 2, 3, 4 as today); Jupyter\'s later-figure numbers change as
  in "Decided: later-figure numbering" (recorded in D14); the notes say real subfloats remain `bd-plcqhfcn`.

## Review findings

The review is [the OPR-7j plan review](../research/2026-11-10-opr-7j-plan-review.md). Its changes to this plan: the
engine-id branch is dropped; a one-plot group is not a group (a generic plot count, not `figure_count`); edge cases and
the user-id collision are decided; the Jupyter byte-identity guard has a mechanism (the harness cannot be one); the
binary test runs in CI; targets and manifest formats are added; the Jupyter later-figure numbering is a question for
Gordon; stale comments and a docs note are added.

## Notes from the run

(Fill in at landing.)
