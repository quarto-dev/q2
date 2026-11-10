---
title: 'Subfloat parity with Quarto 1 (DRAFT): measure first, decide where panels are made, then build'
date: 2026-11-09
description: 'Draft plan, not an approved one and not necessarily correct. Subfloats (sub-figures and sub-tables, Figure 1 (a)) were deferred by design in the crossref design plan and are partly, unevenly implemented across engines. This draft proposes a Quarto 1 oracle corpus and a baseline report first, then one design decision (where panel identity is created), then phases to build the model, the engine side and the writers. Tracked by existing strands; this plan does not replace them.'
status: draft
---

**Status: DRAFT (Claude, 2026-11-09, after the OPR-7a session; updated 2026-11-10 after the OPR-7d review, see [OPR-7j](2026-11-10-opr-7j-group-caption-and-panel-ids.md)).** Written from what that session turned up, not from a
full read of Quarto 1. Anything marked (guess) or (relied) needs checking before it is believed. Gordon has not
approved it; it can be rewritten freely. It lives outside the one-presenter epic and its `depends-on` DAG; it was
committed on the epic\'s integration branch only so it is not lost.

**Tracking surface: the strands, not this file.**

| Strand | What it is |
|---|---|
| `bd-plcqhfcn` | The umbrella: subfloat support (parent and child ids, nested numbering, `Figure 1a`). Deferred by design in [the crossref design plan](2026-04-15-crossref-design.md). Parent: `bd-jsbg`. |
| `bd-gbaykhth` | The Jupyter family (in-process Jupyter engine and the TS route that Julia uses) has no panel synthesis for `fig-subcap` and `tbl-subcap`. A child of `bd-plcqhfcn`. Its comment of 2026-11-09 has the Julia probe. |
| `bd-au5gts6e` | knitr numbers the panels of a multi-plot `fig-subcap` cell as separate figures. Linked `related`; whether it is the same root cause is **unconfirmed**. |
| `bd-lnb5x22l` | Lightbox galleries for subfigures. Blocked by `bd-plcqhfcn`. |

If this draft is adopted, put the strand ids in its header and keep them the place where status lives. Do not open a
strand for work that is in this plan\'s checklist (the repository\'s braid-versus-plans rule).

## Overview

Quarto 1 treats a cell with several figures or tables, a label and a `*-subcap` list as one float with panels:
`Figure 1`, then `Figure 1 (a)` and `Figure 1 (b)`, with references such as `@fig-x-1` resolving to the panels. q2 does
this unevenly. The deferral was deliberate (the design plan called it "delicate" and said it "needs its own plan"; the
plan was never written). This draft is that plan\'s first sketch.

The goal is parity with Quarto 1 for sub-figures and sub-tables across the engines q2 runs, in the formats q2 writes,
with the parity measured against Quarto 1 itself and not argued.

## What is known (evidence marks: read, ran, relied)

| Case | Behavior in q2 | Mark |
|---|---|---|
| Rust crossref index | `CrossrefEntry.parent` exists and is always `None` (`transforms/crossref_index.rs`, "subfloats deferred"); no reader. No Rust code renders `quarto-subfloat-*` or sub-reference numbers. | read, in `bd-plcqhfcn` and re-grepped 2026-11-09 |
| HTML surface | Designed: [float layout class taxonomy](../designs/float-layout-class-taxonomy.md) (`quarto-subfloat-<ref>`, `quarto-layout-cell-subref`, `data-qf-subfloat`). | read |
| Vendored Q1 Lua | `crossref_mark_subfloats` (`resources/pandoc-filters/filters/crossref/preprocess.lua`) sets `parent_id` purely by custom-node nesting; `prependSubrefNumber` and `floatreftarget.lua` render it. This is the pandoc-hybrid route, not the direct HTML path. | read |
| knitr | `hooks.R` labels each panel image from `fig.subcap` and the label, so panels get ids; the presenter\'s knitr option path handles `fig.subcap` per figure (unit tests). Numbering then comes out as separate figures. | read (code and tests); the numbering is relied on `bd-au5gts6e`, measured there on 2026-11-08 |
| In-process Jupyter engine | No per-panel id or caption synthesis; pre-engine desugaring deliberately leaves the cell unwrapped so the engine can synthesize (`codeblock_shorthand.rs`). | relied on `bd-gbaykhth` (Python, typst) |
| Julia, tables | Probe (one cell, `tbl-cap` and `tbl-subcap`, two markdown displays): both the plain and the recorded route resolve `@tbl-x` and leave `@tbl-x-1` and `@tbl-x-2` unresolved; the parent caption is lost (`quarto-uncaptioned`); the options come out as `data-tbl-cap` and `data-tbl-subcap` attributes. | ran, 2026-11-09, release `q2`, HTML read |
| Tables, all engines | 2026-11-10 (ran): a `tbl-cap` plus `tbl-subcap` group of two markdown tables on Julia, knitr and Jupyter, plain and recorded, and the pre-epic commit for Julia and knitr: all the same page. `tbl-two` exists and `@tbl-two` resolves ("Table 2"), but the group caption text is lost (`quarto-uncaptioned`), the panels have no ids and `@tbl-two-1`, `@tbl-two-2` are unresolved with two warnings. A gap on every engine, not a regression; [OPR-7j](2026-11-10-opr-7j-group-caption-and-panel-ids.md) does not touch it. | ran |
| Julia, figures | Rendered 2026-11-10 (ran, the review branch `q2`, HTML read; the pre-epic commit `7016d595a` gives the same page as today\'s plain route). **Plain:** the TS conversion (`to-markdown.ts`) writes each panel output as `fig-x-N` (the output label is `label + "-" + suffix`), so crossref sees three figures: panels "Figure 2: First." and "Figure 3: Second." and the group "Figure 4: A group."; `@fig-x`, `@fig-x-1`, `@fig-x-2` all resolve, with those wrong numbers, and there are no warnings. **Recorded route:** the presenter writes no panel ids and no group caption for a non-knitr cell (the gate is `figure_count`, set only by knitr), so the caption text is lost, the panels are unnumbered and `@fig-x-1`, `@fig-x-2` are `?fig-x-N?` with two warnings. | ran |
| Julia and knitr, same markup | knitr\'s `hooks.R` labels each panel the same way (`label-N`) and its page matches plain Julia\'s (Figure 2, 3, 4). Both are wrong for the same visible reason: panel ids without a `parent` link, so each panel counts as its own figure. This is evidence that `bd-au5gts6e` is downstream of the missing `parent`, not an independent bug. It is not a finding until Phase 2 measures it. | ran (pages); the causal claim is a guess |
| Native Jupyter | `fig-subcap` group: no `fig-x` id, no group caption, three unresolved references and three warnings, on both routes (rendered 2026-11-10). Not a regression of any epic; the same gate as the Julia recorded route. | ran |
| Lightbox | Subfigure galleries wait on this feature. | `bd-lnb5x22l` |

**Not known (the draft depends on finding out):** what Quarto 1 renders for each of these cases; whether `bd-au5gts6e` is
a numbering bug downstream of a missing `parent` or an independent bug; whether the dropped parent caption on Julia is a
symptom of the missing synthesis or of the attribute-forwarding step (`to-markdown.ts`, "forward other attributes we
don\'t know about"); how much of Quarto 1\'s subfloat behavior is in `jupyter.ts` and `hooks.R` (engine side) and how much
in the Lua filters (pandoc side); what typst, LaTeX and docx need.

## Subtables are in scope (added 2026-11-10)

Sub-tables (`tbl-cap` with `tbl-subcap`, shown as "Table 1 (a)") are half of this plan\'s goal, not an afterthought, and
they are in worse shape than sub-figures. [OPR-7j](2026-11-10-opr-7j-group-caption-and-panel-ids.md) fixes the
figure group\'s caption and panel ids for every engine and **deliberately leaves tables alone**, because no engine handles
them and the one-presenter flip does not make them worse. This section says what the 2026-11-10 renders showed, so Phase 0
starts from a measured baseline.

**What happens today (ran, 2026-11-10).** One cell with a label, `tbl-cap` and `tbl-subcap` holding two markdown tables,
on Julia, knitr and Jupyter, plain and recorded routes, plus the pre-epic commit `7016d595a` for Julia and knitr: every
page is the same.

| Element | Result on every engine and route |
|---|---|
| Group `tbl-two` | exists; `@tbl-two` resolves ("Table 2") |
| Group caption text | lost; "Table 2" alone, class `quarto-uncaptioned` |
| Panel ids and captions | none; the subcaptions are not rendered as panel captions |
| `@tbl-two-1`, `@tbl-two-2` | `?tbl-two-N?`, unresolved, two warnings |
| A single labeled table next to it | correct ("Table 1: One table.") |

The seed cell (Julia; the knitr and Jupyter versions use `kable` and two `text/markdown` outputs):

```julia
#| label: tbl-two
#| tbl-cap: A table group.
#| tbl-subcap:
#|   - First table.
#|   - Second table.
display(MIME("text/markdown"), md"| a | b |\n|---|---|\n| 1 | 2 |")
display(MIME("text/markdown"), md"| c | d |\n|---|---|\n| 3 | 4 |")
```

**How tables differ from figures.** For figures, plain Julia and knitr write one id per panel (`fig-x-1`, `fig-x-2`), so
the panels exist as separate figures with wrong numbers, and the missing piece is the `parent` link. For tables nothing
writes panel ids on any engine, so the missing pieces are both the panel identity *and* the `parent` link, and even the
group caption is lost. So a table fix is the bigger of the two: it cannot start from "reuse the figure ids and add
`parent`". I did not read `hooks.R` or `to-markdown.ts` for why tables get no panel ids; Phase 1 must.

**What this plan commits to for tables.**

- Phase 0 puts at least these table cases in the corpus, on all three engines and in Quarto 1: the seed cell; the same
  without a parent caption; a table group beside an ordinary table; the references `@tbl-x`, `@tbl-x-1`, `@tbl-x-2`;
  and a group with a `layout-ncol`. (The figure cases are already listed there.)
- Phase 1\'s note covers the table path in Quarto 1 explicitly (`crossref/tables.lua`, the `tbl-subcap` handling in
  `jupyter.ts` and `hooks.R`) and says whether tables and figures share one mechanism there.
- Phase 2\'s baseline report has a table section, and says how many cases are a regression against nothing (there is no
  earlier correct behavior to regress from) and how many differ from Quarto 1 in a nameable way.
- Phase 3\'s decision (the existing "do tables and figures ship together" question) is made with this baseline. If
  tables ship later, the plan\'s first slice is figures only, on top of 7j, and tables are a named second slice, not an
  open end.
- Phase 4 builds the panel identity and the `parent` link for tables as well, with the group caption restored first
  (it is the cheapest visible fix and independent of the numbering), then panel ids, then the "(a)" numbering.

## The central design question (decision for Gordon, after Phase 2)

**Where is panel identity created** (the ids `fig-x-1`, `fig-x-2`, the panel captions, the parent and child link)?

- **A. In each engine, as Quarto 1 does.** knitr in `hooks.R`, Jupyter in `to-markdown.ts` (`resolveCaptions`, per-output
  ids), and so on. Closest to Quarto 1\'s structure and to ported code. Cost: every engine must implement it, and the
  unevenness we have today is exactly what that produces.
- **B. In the presenter, uniformly (draft lean, guess).** The presenter sees the cell\'s outputs and its options
  (`label`, `*-cap`, `*-subcap`) for every engine that goes through a record, and OPR-7d makes that every plain Julia and
  knitr render. It could emit the panel structure once. Cost: it overrides what an engine already does for a cell, so
  knitr\'s `hooks.R` path needs a rule for who wins; it only covers engines that use the presenter; and the splice and
  preview paths must produce the same bytes (they share the one cell step, OPR-1a).
- **C. In the crossref transform, from the options on an unwrapped cell.** One place, before the engine runs or after
  it, independent of the engine. Cost: it runs where the outputs do not exist yet (before the engine), so it can only
  count panels it can predict.

Whichever is chosen, the **Rust index must carry `parent`** (populated at construction, not inferred later by nesting);
that is the half every option shares and the half `bd-plcqhfcn` names. The echo-splicing workaround for the phantom
subfloat (`d7562d86a`, `clear_matching_id`) depends on this gap and should disappear with it (see the comment on
`bd-plcqhfcn`).

## Phases

Every phase starts with its tests. Phases 0 to 2 only add tests, fixtures and notes; they change no product code, so
they can run beside the one-presenter epic. Phase 3 is Gordon\'s. Phases 4 and later are an **outline** to be re-planned
after the decision, not a commitment.

### Phase 0: the corpus and the oracle (tests first)

Goal: parity becomes measurable. Reuse the shape of the one-presenter parity harness
(`crates/quarto-core/tests/integration/opr_parity_harness.rs`, its fixtures and allowlist form) and its lessons; the
oracle here is Quarto 1 (the dev checkout in `external-sources/quarto-cli`, run as the existing Q1 comparisons do; find
how, do not assume).

- [ ] Find how existing tests run Quarto 1 as an oracle (and the recipe in the maintainer\'s notes on running Q1 against
      q2\'s vendored filters), and say which one this corpus uses. If none is suitable, say what is needed.
- [ ] Write the corpus, each case a small `.qmd`: `fig-subcap` and `tbl-subcap` in a single cell with a label and a
      parent caption; the same without a parent caption; unlabeled; fewer or more subcaps than outputs; `fig-subcap: true`
      (empty subcaps); `layout-ncol` and `layout-nrow`; explicit nested divs (`::: {#fig-x}` containing images with ids);
      markdown images in a div; a table pair; a mix with an ordinary figure; references `@fig-x`, `@fig-x-1`, `@fig-x-a`
      (whatever Quarto 1 accepts); chapter numbering in a book; the same under each engine (knitr, Python through
      Jupyter, Julia) and in at least HTML and Typst.
- [ ] A runner that renders each case with Quarto 1 and with q2 and compares structure and numbers (the resolved reference
      texts, the caption prefixes, the panel ids), not bytes. Skips cleanly when R, Python or Julia is absent.

### Phase 1: read Quarto 1 end to end (a note)

- [ ] A research note under `claude-notes/research/`: the algorithm as Quarto 1 runs it, with file and line marks:
      `parsefiguredivs.lua` (the subfloat paragraph the design plan points at), `crossref/preprocess.lua`,
      `crossref/format.lua` (`prependSubrefNumber`), `crossref/tables.lua`, `customnodes/floatreftarget.lua` (render),
      `jupyter.ts` (`resolveCaptions`, `outputCaptions`, per-output ids), `hooks.R` (`output_label`, `figure_cap`), and what
      each writer does with a subfloat. Say which pieces are engine-side and which pandoc-side. This answers the open
      question of how the work divides.

### Phase 2: the q2 baseline report (a note)

- [ ] Run the Phase 0 corpus over q2 for every engine, route (plain and recorded) and format; classify each case as
      identical to Quarto 1, differing in a nameable way, or failing. Write the report (a table like the OPR-7c report)
      and link it from the strands. Confirm or refute whether `bd-au5gts6e` and the dropped Julia caption are the same
      gap as the missing `parent`, and say so on the strands.

### Phase 3: the decision \[Gordon\]

- [ ] Choose A, B or C (or a mix) from the Phase 1 and 2 evidence, and decide the contract for engines that already
      synthesize (knitr): who wins. Record it here and on `bd-plcqhfcn`. Decide whether tables and figures ship together
      and which formats are in the first slice.

### Phase 4 onward (outline; re-plan after Phase 3)

- The model: populate `CrossrefEntry.parent` at construction; resolve panel references in the Rust index; nested
  numbering with the language\'s panel format; remove the echo-splicing special case if the model makes it unneeded.
- Panel creation per the decision, with a test per engine from the corpus.
- Writers: HTML first (the taxonomy is designed), then Typst, then the others in the order the corpus shows demand;
  each with its own parity cases.
- Layout interplay (`layout-ncol` and friends) only as far as the corpus requires.
- The lightbox galleries (`bd-lnb5x22l`) once subfloats exist.
- A parity gate with an allowlist in the one-presenter style (a difference is fixed or signed, never silently left), the
  docs page, and the user-facing migration note if a deliberate difference remains.

## Interaction with the one-presenter epic

- **OPR-7j (2026-11-10) comes first and is a small step on this plan\'s road.** The 7d review found that the flip makes
  Julia figure groups worse than the 2026-11-09 text here and in 7d decision 3 assumed (the group caption text and the
  panel ids are lost, not just the numbers). [OPR-7j](2026-11-10-opr-7j-group-caption-and-panel-ids.md) removes that
  regression by applying knitr\'s `hooks.R` group rules (the label on the cell, the `fig-cap` as the group caption,
  `label-N` panel ids) to every engine in the presenter. It leaves the numbers exactly as they are today for knitr and
  plain Julia and does not build subfloats. It **does not decide the central design question** below: it creates panel
  ids in the presenter (the direction of option B) for the current Quarto 1 behavior only, the knitr path stays option A,
  and Phase 3 may move or replace it. The markup it writes (`fig-x-1`, `fig-x-2`, the group caption) is the same markup
  the model in Phase 4 needs, so the later work reads it, adds the `parent` link and the "(a)" numbering, and removes
  7j\'s ids only if the chosen option creates them elsewhere. One consequence for the Phase 2 baseline: after 7j a Jupyter group
counts as n + 1 figures for n panels (rendered 2026-11-10: a group of 2 shifts later figures by 2, of 3 by 3), so every later figure in a Jupyter document is numbered n too high against Quarto 1 (today one
too low; knitr and Julia already number this way). Include a later-figure case per engine in the corpus.
- That epic keeps real subfloats out of scope (OPR-7d decision 3, replaced on 2026-11-10 by 7j, and the 7c report cite
  the strands). After 7j the flip changes nothing about figure groups; tables are unchanged by the flip (see "Subtables are in scope"
  above, which is where they get fixed). Nothing here blocks the epic any more than it already is by 7j, which is a prerequisite of 7d.
- If option B is chosen, the work lands on `presenter/cell.rs`, `presenter/options.rs` and `engine/ts_record.rs`, which
  OPR-7f changed and 7j changes. Start Phase 4 only after OPR-7f, 7j and 7d have landed. Phases 0 to 2 do not touch them.
- OPR-7f, Part C, stops `metadata.quarto` from being forwarded as `data-quarto`. It does **not** stop `tbl-cap` and
  `fig-subcap` from being forwarded as `data-*` attributes; that belongs to this plan\'s synthesis work.

## Risks and things this draft may have wrong

- After 7j the Julia and Jupyter figure paths share the presenter\'s knitr-derived group rules. The corpus may show that
  Quarto 1 does something different for Jupyter (where it wins over `hooks.R`); parity then needs a stated choice per case.
- Tables may need a separate design from figures (see "Subtables are in scope": no engine writes table panel ids today).
- It assumes the right unit of work is "subfloats" as one feature. The corpus may show that tables, figures and
  `layout-*` options each need separate handling.
- Option B is a lean, not a finding. It needs a look at how much of the Jupyter-dialect `subcap` handling the presenter
  already has (its option sweep lists `fig-subcap` and `tbl-subcap` as "Presents"; I did not read what that proves).
- Quarto 1 itself may be inconsistent across engines; parity then needs a stated choice per case, not one rule.
- The estimate is a few focused sessions for Phases 0 to 2, plus Gordon\'s decision, then an epic-sized build; the build
  size cannot be estimated until Phase 1 says how the work divides.

## Notes from the run

(Not started.)
