---
title: 'OPR-7j: group caption and panel ids for fig-subcap cells, for every engine (OUTLINE)'
date: 2026-11-10
description: 'Outline, not yet reviewed by Gordon. Makes the presenter apply knitr''s hooks.R group rules to every engine: a cell with a figure label, fig-cap and fig-subcap keeps its group caption and gives each panel its own label-N id. This removes the Julia regression OPR-7d would otherwise ship (lost group caption, unresolved panel references) and fixes the same two gaps for native Jupyter, without building real subfloats.'
status: blocked-on-gordon
depends-on: [opr-7f]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Blocks:** [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (it lists `opr-7j` in its `depends-on`).
**Evidence:** [OPR-7d plan review](../research/2026-11-10-opr-7d-plan-review.md), items 15 and 16 (superseded by this plan).
**Related, not replaced:** [subfloat parity draft](2026-11-09-subfloat-parity-draft.md) (`bd-plcqhfcn`, `bd-gbaykhth`, `bd-au5gts6e`).
**Status:** an outline written during the 7d review. It becomes `ready` when Gordon flips it. The Jupyter question is
decided (below).

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
panels (Figure 4). That wrongness predates this plan and the subfloat plan fixes it, by making the Rust crossref index
carry `parent`. It also does not change tables (below), layout options, other formats, or `bd-au5gts6e`.

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
  paragraph when this lands.

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
cases were not run. The combined patch is `group-caption-and-panel-ids-prototype.patch` in the review session\'s
scratchpad (`/private/tmp/claude-502/-Users-gordon-src-q2/a2aca94d-da29-41a9-bb45-3d647af619f1/scratchpad/`); the first
edit alone is `group-caption-prototype.patch`. The prototype is not the implementation: the cases below
are untested.

## Open questions for the implementer (settle in tests first)

1. **Is the engine-id branch needed?** The first edit also made `cell_caption` accept an engine-written `fig-` id.
   After `label_on_cell` was relaxed, check whether that branch can go.
2. **Edge cases.** `fig-subcap` with one entry; `fig-cap` without `fig-subcap` (unchanged: the cell div keeps its
   current handling); `fig-subcap` without a label; a panel count different from the subcap count; `fig-subcap: true`;
   a cell where the engine already wrote `label-N` ids; a group cell whose engine id and the presenter\'s label differ.
   A single-figure labeled cell must not get a doubled id.
3. **Count from the outputs, not from the subcaps.** The prototype numbers panels by position. Hooks.R numbers by the
   plot count. Compute the figure count generically (from the cell\'s plot outputs) so the knitr rule applies as written.
4. **Tables:** decided above (left alone; not a regression, not fixed by the prototype). Add one test that the table
   group page is unchanged by this plan.
5. **Preview and splice.** The splice route (`present_units`) and the render route share the cell step; add one splice
   test so preview and render produce the same bytes.

## Decision (Gordon, 2026-11-10): apply to every engine, Jupyter included

The epic said Jupyter output stays byte-identical unless a cell sets a new option. A Jupyter `fig-subcap` cell today
silently loses its group caption and its panel references, and this plan fixes that. Gordon accepted the change and
overrode the byte-identical rule for this one case; it is recorded as D14 in the epic. The rest of Jupyter output must
stay byte-identical, and a checklist box tests that.

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

- [ ] **Tests first.** Unit tests in `presenter/options.rs` and `presenter/cell.rs` for a labeled cell with `fig-cap`
      and `fig-subcap` and no figure count: the cell div carries the label, the closing caption is the `fig-cap`, panel N
      has the id `label-N`. Watch them fail on today\'s code, for that reason.
- [ ] **Jupyter byte-identity guard (D14).** Every existing Jupyter golden and parity fixture except a `fig-subcap`
      group renders byte-identically before and after; list the fixtures compared. The Jupyter `fig-subcap` fixture is
      the one expected change, with the group caption and panel ids as in the prototype table.
- [ ] A test per open question 2 case, including the single-figure cell with no doubled id and the knitr output
      unchanged (an existing knitr group test pins it).
- [ ] A test through the binary (model `crates/quarto/tests/integration/no_execute_cli.rs`): a Jupyter notebook
      with a `fig-subcap` group renders the group caption and resolves `@fig-x` and the panel references, with no
      unresolved-crossref warning. Julia and knitr versions skip cleanly when the engine is absent.
- [ ] Implement: remove the knitr-only gate as described, reusing the knitr presenter functions
      (`label_on_cell`, `cell_caption`, `plot_image_id`); do not add a second code path.
- [ ] Parity gate: run the harness; `julia/fig-subcap` differs from plain only by the interleaved code blocks, and
      `jupyter/fig-subcap` and `knitr/fig-subcap` remain identical on both routes (their text changes for Jupyter).
      Report the pages\' caption and reference lines before and after.
- [ ] Snapshots: count and list any changed `.snap` or golden files, per the protocol.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy; `cargo xtask lint`; `cd hub-client && npm run build:wasm` (the presenter compiles for wasm32).
- [x] OPR-7d and the subfloat draft are updated to fit this plan (done in the 7d review session, 2026-11-10): 7d
      depends on `opr-7j`, its decision 3 is replaced, the `julia-subfigure-group` normalizer is dropped from its
      design, and the draft\'s "Interaction" paragraph and known-facts table say what 7j does. At landing, re-read both
      and correct anything this plan\'s implementation changed (for example if tables end up in scope).
- [ ] Remove the allowlist header sentence that calls group references "not an allowlist candidate" (the header is in
      `opr-parity-allowlist.toml`; 7d\'s checklist updates it for the interleaving entries, so whichever plan lands
      second makes the final edit).

## Exit criteria

- A labeled `fig-subcap` group renders the same group caption, panel ids and resolved references for Julia, Jupyter and
  knitr; the Julia recorded page matches plain Julia on those; the parity case differs only by interleaving.
- No new warnings; knitr output unchanged; the splice route matches the render route.
- Panel numbering is not changed (Figure 2, 3, 4 as today) and the notes say real subfloats remain `bd-plcqhfcn`.

## Notes from the run

(Fill in at landing.)
