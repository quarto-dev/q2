---
title: 'Subfloat parity with Quarto 1 (DRAFT): measure first, decide where panels are made, then build'
date: 2026-11-09
description: 'Draft plan, not an approved one and not necessarily correct. Subfloats (sub-figures and sub-tables, Figure 1 (a)) were deferred by design in the crossref design plan and are partly, unevenly implemented across engines. This draft proposes a Quarto 1 oracle corpus and a baseline report first, then one design decision (where panel identity is created), then phases to build the model, the engine side and the writers. Tracked by existing strands; this plan does not replace them.'
status: draft
---

**Status: DRAFT (Claude, 2026-11-09, after the OPR-7a session).** Written from what that session turned up, not from a
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
| Julia, figures | The OPR-7c parity report says plain builds the group and numbers it wrongly and recorded leaves the references unresolved. | relied (7c report; `Plots` is not installed here) |
| Lightbox | Subfigure galleries wait on this feature. | `bd-lnb5x22l` |

**Not known (the draft depends on finding out):** what Quarto 1 renders for each of these cases; whether `bd-au5gts6e` is
a numbering bug downstream of a missing `parent` or an independent bug; whether the dropped parent caption on Julia is a
symptom of the missing synthesis or of the attribute-forwarding step (`to-markdown.ts`, "forward other attributes we
don\'t know about"); how much of Quarto 1\'s subfloat behavior is in `jupyter.ts` and `hooks.R` (engine side) and how much
in the Lua filters (pandoc side); what typst, LaTeX and docx need.

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

- That epic keeps this out of scope (OPR-7d decision 3 and the 7c report cite the strands). Its flip changes Julia
  sub-figure references from wrong numbers to unresolved; tables are unchanged (the probe above). Nothing here blocks it.
- If option B is chosen, the work lands on `presenter/cell.rs` and `engine/ts_record.rs`, which OPR-7f changes. Start
  Phase 4 only after OPR-7f and OPR-7d have landed. Phases 0 to 2 do not touch them.
- OPR-7f, Part C, stops `metadata.quarto` from being forwarded as `data-quarto`. It does **not** stop `tbl-cap` and
  `fig-subcap` from being forwarded as `data-*` attributes; that belongs to this plan\'s synthesis work.

## Risks and things this draft may have wrong

- It assumes the right unit of work is "subfloats" as one feature. The corpus may show that tables, figures and
  `layout-*` options each need separate handling.
- Option B is a lean, not a finding. It needs a look at how much of the Jupyter-dialect `subcap` handling the presenter
  already has (its option sweep lists `fig-subcap` and `tbl-subcap` as "Presents"; I did not read what that proves).
- Quarto 1 itself may be inconsistent across engines; parity then needs a stated choice per case, not one rule.
- The estimate is a few focused sessions for Phases 0 to 2, plus Gordon\'s decision, then an epic-sized build; the build
  size cannot be estimated until Phase 1 says how the work divides.

## Notes from the run

(Not started.)
