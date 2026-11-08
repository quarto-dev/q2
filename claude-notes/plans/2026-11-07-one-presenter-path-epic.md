---
title: 'Epic: one presenter path for every engine'
date: 2026-11-07
description: 'Every recorded cell goes through one presenter that reads what a cell supplies, not which engine made it. Plain render of Julia and knitr joins the recorded route, Julia gains the capture facts knitr has (subject to a spike), and the execution-records option is one shared definition across preview, provide-hub and the new q2 render. Seventeen plans in four lanes, written to be run by agents with no orchestrator.'
status: 'plans written 2026-11-08; ready to run; integration branch feature/one-presenter in workspace-1; three decisions still open (metadata key versus flag, the retained fallback and an opt-out, Julia spike scope)'
---

**Follows from:** [the RIU follow-up plan](2026-11-07-riu-follow-up-did-we-build-it.md), Item 7 (Item 9 folds into
OPR-5 here), and [the review note](../research/2026-11-07-riu-follow-up-review.md).
**Evidence:** [spike results](../research/2026-11-07-one-presenter-spikes.md),
[decisions note](../research/2026-11-08-one-presenter-review-decisions.md),
[review findings](../research/2026-11-08-one-presenter-review-findings.md).
**Intent sources:** [knitr epic](2026-10-30-knitr-execution-records-epic.md), decisions 4, 12, 24, 26;
[execution-records epic](2026-10-28-execution-records-epic.md), decisions 5 and 9.
**How to run a plan:** [agent protocol](2026-11-08-opr-agent-protocol.md). Every plan points at it.

## Goal

Every recorded cell, from every engine, goes through one presenter path. The presenter reads what a cell supplies,
not which engine produced it: knitr\'s features (interleaved output, `prompt`, numeric `echo`, message and warning
handling, unknown-option forwarding, figure labels and captions) are open to any engine that supplies the inputs they
need, and degrade cleanly, one input at a time, for one that does not. The scope is wider than recorded cells:

1. **Plain `q2 render` of Julia and knitr joins the recorded route.** Today a plain render asks for no record, so Julia
   is presented by the TypeScript engine\'s `toMarkdown` and knitr by R\'s finished markdown. After this epic a plain
   render builds the record in memory and presents it through the Rust presenter, writing nothing to `.quarto`.
2. **Julia gains the capture facts knitr has** (expression starts, anchors, output kinds), subject to the spike in OPR-4a.
3. **`--execution-records` is one definition**, shared by `preview`, `provide-hub`, `q2 record export-positron`
   (until RIU follow-up Item 8 replaces it with `q2 convert`) and the new `q2 render --execution-records`.

Jupyter output stays byte-identical unless a cell sets a new option (knitr epic decision 12), except for the two
documented preview and hub fixes in OPR-1a and OPR-1b. **Marimo is a documented exception**: it uses the legacy
capture splice and is not part of this epic.

## Decisions (Gordon, 2026-11-08; not to be reopened by a plan)

- **D1. One cell step and one document function.** One per-cell function takes a struct of optional inputs (anchors,
  expression starts, output kinds, figure count, resolved options, known options, echoed code); `quarto_resolved_options`
  becomes one field, not the switch. One document-level function takes already-matched (unit, cell) pairs and returns,
  per unit, a replacement, a skip with a reason, or an error; one `apply_edits` step applies them. Callers keep matching
  (`match_cells` by id for render, `match_units` by hash with staleness for the splice), error policy (render fails; the
  splice keeps the source), reporting, output stamping and the AST reparse. The engine entry is looked up per cell.
- **D2. Enforcement of `warning` and `message`.** The presenter enforces them exactly when the record\'s hash definition
  lists the option as display-only; when it is in the hash, capture enforced it. `echo`, `output` and `include` stay always
  presenter-enforced. An engine with no usable declaration, and an imported Positron record, get presenter enforcement.
  Julia\'s declaration is corrected (OPR-3). Reason: a hash is conservative, so "in the hash" means "might matter".
- **D3. Document scope.** If a cell carries resolved options (knitr), R already baked the document `execute:` scope in
  (`execute.R:348-353` translates `output: false` to `results: hide` and `fig.show: hide`), so the presenter does not
  layer it; if it carries none (Jupyter, Julia, imported, fallback), the presenter layers it at present time.
- **D4. Julia is in scope, subject to the OPR-4a spike.** Jupyter positions are a follow-up, not part of this epic.
- **D5. Plain render is in scope.** A cached knitr chunk keeps knitr\'s cache and uses the markdown fallback with no
  record, quietly. The LaTeX family, `pptx`, `context` and unknown knitr targets stay on the engine route, quietly
  (strands `bd-4y5scdhp`, `bd-lo1cs7u5`, `bd-ka76vcrp` track widening; LaTeX notes live in the LaTeX plan on
  `explore/pandoc-hybrid-latex`). Whether the retained fallback survives, and whether an opt-out exists, are open (OPR-7d).
- **D6. `render --execution-records`.** Read only (writing stays with `execute: record: true`); default `engine`; `cache`
  refused; one list for all documents, walked per document; no source and no `engine` warns and leaves cells inert
  (`--strict` errors; prose-only silent); a stale record, or one recorded for another format, counts as not supplied;
  `want_record` unchanged; `--no-execute` vetoes `engine`; `--replay` conflicts (a runtime check, which covers
  `QUARTO_REPLAY`); `preview.engine` stays preview-only; `preview --static` is in scope.
- **D7. Consolidation.** The grammar and handling are defined once; the rename `--records` to `--execution-records`
  (unreleased, no alias) is folded in (OPR-5). Its shape depends on the metadata-key research (OPR-0d, then Gordon).
- **D8. Handshake.** Core offers `presentedBy: "core"`; an engine acknowledges in its returned record info; core
  presents from the record exactly when acknowledged. No engine-name test (OPR-3, OPR-7a).
- **D9. Merge-time fixes to preview and hub.** Unknown `#|` options on Jupyter cells are forwarded on both surfaces
  (today only render does). Spans use `classify_inline_result` on both (last text-bearing output; an error wins;
  `present_span` is deleted). Strand `bd-cm1mwyuq` is updated when OPR-1a lands.
- **D10. Parity gate.** Knitr epic decision 26 for plain render: Identical and SyntaxLevel pass; anything else needs a
  signed allowlist entry; zero unallowlisted differences on the knitr corpus (existing, plus additions) and a live Julia
  corpus. R and Julia are absent from CI, so the gate runs locally.
- **D11. Docs at the end, in existing pages,** written with `/reader-expectations-prose`; one pull request for the epic.
- **D12. Marimo is a documented exception.**
- **D13. Strands.** `bd-nn2fou8h` (execute-visibility options) is referenced by OPR-1a and OPR-2; `bd-gbaykhth` (Jupyter
  has no subfloat synthesis) and `bd-au5gts6e` (knitr sub-figure numbering) are out of scope.

## What the spikes and reviews found (evidence)

Marks are those of the [spike note](../research/2026-11-07-one-presenter-spikes.md) and the
[findings note](../research/2026-11-08-one-presenter-review-findings.md), which says which review claims were
verified. Line numbers are for `4099022dc`; each plan says (read), (ran) or (relied).

- Every cell ends in `render_cell_whole` (`presenter/cell.rs:131`); the two option front ends differ only in knitr\'s ten
  dotted aliases (S1, ran). With every capture input empty the knitr path reproduces Jupyter\'s bytes for 12 of 13 goldens;
  the 13th differs only in who enforces `warning` (S2, ran).
- The per-cell step and the document loop each exist twice: `present_document` (`presenter/document.rs:104`, five
  callers) and the splice stage (`stage/stages/record_splice.rs`). They differ in forwarding, error handling, format-mismatch
  reporting, span handling, engine entry per cell and matching (read); OPR-0a pins each.
- A plain `q2 render` of Julia or knitr takes the engine-owned route; `core_presents` is `engine == "julia" &&
  want_record && units non-empty` (`engine/ts_record.rs:500`, read, plus one unit test).
- The document `output: false` mystery is solved: R translates it before recording (`execute.R:348-353`, read and ran).
- Julia\'s hash declaration lists every option as hashed and its capture enforces almost nothing; knitr hashes `include`
  and `output` yet the presenter hides them (read; the run is relied).
- Knitr falls back for `latex`, `beamer`, `pdf`, `pptx`, `context` and unknown targets (`target_route.rs`, read).
- A multi-figure cell with `fig-subcap` and `layout-ncol` is unresolved on q2\'s Jupyter-shaped route
  (`bd-gbaykhth`) and numbered "Figure 3, 1, 2" on both knitr routes against Quarto 1\'s "Figure 1, 1 (a), 1 (b)"
  (`bd-au5gts6e`); the plain and recorded knitr routes were byte-identical for it (ran, one document).
- A tiny Julia document differs between routes: `id="cell-1"` and `data-quarto` on the wrapper and figure names (relied).

## Plans, lanes and dependencies

Each plan is a unit of merge to the integration branch `feature/one-presenter` (checked out in `workspace-1`, a landing
pad nobody edits), branched from it, rebased onto it and fast-forwarded back. A plan depends **only on complete (or
cancelled) plans**; "complete" is the plan file\'s `status:` on the integration branch. The protocol says how.

| Plan | What | Depends on | Lane | Status at start |
|---|---|---|---|---|
| [OPR-0a](2026-11-08-opr-0a-pinning-tests.md) | Pinning tests for the loops, scope, spans and entry paths | nothing | A | ready |
| [OPR-0b](2026-11-08-opr-0b-parity-corpus-and-harness.md) | Parity corpus and harness | nothing | D | ready |
| [OPR-0c](2026-11-08-opr-0c-measurements-and-spikes.md) | Cost, cache, figure and route measurements (a note) | nothing | D | ready |
| [OPR-0d](2026-11-08-opr-0d-metadata-key-research.md) | Metadata-key research (a note) | nothing | C | ready |
| [OPR-1a](2026-11-08-opr-1a-one-cell-step.md) | One per-cell step over a struct of optional inputs | 0a | A | ready |
| [OPR-1b](2026-11-08-opr-1b-one-document-function.md) | One document function returning outcomes | 1a | A | ready |
| [OPR-2](2026-11-08-opr-2-enforcement-from-hash.md) | Enforcement derived from the hash, plus audit | 1b, 3 | A | ready |
| [OPR-3](2026-11-08-opr-3-julia-engine-wire.md) | Julia declaration fix and acknowledgement | nothing | B | ready |
| [OPR-4a](2026-11-08-opr-4a-julia-spike.md) | Julia spike (a note) | nothing | B | ready |
| [OPR-4b](2026-11-08-opr-4b-julia-runner-facts.md) | Runner reports the facts (another repository) | 4a, Gordon\'s go | B | conditional |
| [OPR-4c](2026-11-08-opr-4c-julia-engine-facts.md) | Engine, wire and pin carry the facts | 4b, 3, Gordon\'s push | B | conditional |
| [OPR-4d](2026-11-08-opr-4d-julia-facts-consumed.md) | q2 writes the facts as cell metadata | 4c, 1b | A | conditional |
| [OPR-5](2026-11-08-opr-5-execution-records-definition.md) | One `--execution-records` definition and the rename | 0d, Gordon\'s R5 decision | C | blocked-on-gordon |
| [OPR-6](2026-11-08-opr-6-render-execution-records.md) | `q2 render --execution-records` | 5, 1b | C | ready |
| [OPR-7a](2026-11-08-opr-7a-presentation-handshake-and-record.md) | Handshake and record without the extra cost, behind a control | 1b, 3, 6 | C | ready |
| [OPR-7b](2026-11-08-opr-7b-knitr-cache-fallback.md) | Cached knitr chunks fall back, no record | 0c | B | ready |
| [OPR-7c](2026-11-08-opr-7c-run-the-parity-gate.md) | Run the parity gate, fix or propose the allowlist | 0b, 1b, 2, 7b, 4d resolved | D | ready |
| [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) | Switch plain render to the recorded route | 7a, 7b, 7c, Gordon\'s decisions | D | blocked-on-gordon |
| [OPR-8](2026-11-08-opr-8-docs-and-end-to-end.md) | Docs, embedded-docs refresh, end-to-end | 1b, 2, 5, 6, 7d, 4d resolved | D | ready |

**Lanes** (an agent takes the first startable plan in its own lane list; if none, it stops and reports):

| Lane | Workspace | Plans, in order |
|---|---|---|
| A, the presenter chain | `workspace-3` | 0a, 1a, 1b, 2, 4d |
| B, Julia and knitr capture | `workspace-4` | 3, 4a, 7b, 4b, 4c |
| C, the flag | `workspace-5` | 0d, 5, 6, 7a |
| D, measurement, gate, docs | `workspace-6` | 0c, 0b, 7c, 7d, 8 |

Lane B puts 7b before 4b and 4c because 7b only waits on OPR-0c, while 4b and 4c wait on Gordon. The integration
checkout is `/Users/gordon/src/q2/.worktrees/workspace-1`.

**Critical path:** 0a, 1a, 1b, then 6 (also needs 5, which needs 0d and Gordon), then 7a, then 7d (also needs 7b, 7c and
Gordon\'s decisions), then 8. Parallel from the start: 0a, 0b, 0c, 0d, 3, 4a.

## What Gordon does between plans

- After OPR-0d lands: read the note, write the R5 decision into OPR-5, set it to `ready`.
- After OPR-4a lands: read the note; set OPR-4b to `ready` with the scope he chooses, or set 4b, 4c and 4d to `cancelled`.
- After OPR-4b lands: push the runner branch to the fork and write the revision into OPR-4c; set it to `ready`.
- After OPR-4c lands: set OPR-4d to `ready`.
- After OPR-7c lands: sign the allowlist, record the two decisions in OPR-7d, set it to `ready`.
- At the end: pushes of the engine and runner forks, the Positron fork\'s extension (RIU follow-up Items 8 and 9), and the
  rebase of `feature/one-presenter` onto `main` and the pull request. The integration branch was cut from the local
  review branch, 175 commits that are on no remote and 158 commits behind the `main` tip on 2026-11-08.

## Decision points (which results could change the plan list)

| Result | If it comes back this way | What changes |
|---|---|---|
| OPR-0d, R5 | metadata, or a mix | OPR-5 and OPR-6 change input and shape; hash exclusions touch the Julia and knitr declarations |
| OPR-0c, cost | expensive | OPR-7a grows into a new seam (its own plan) |
| OPR-0c, cache spike | a mixed document is wrong | OPR-7b needs Gordon (blocked) and may grow |
| OPR-7c, parity | blockers beyond the known Julia three | fix plans appear before OPR-7d |
| OPR-2, audit | a capture gap in an engine | a capture-side plan, coupled to lane B |
| OPR-0a, loop divergence | more than the table | OPR-1a or OPR-1b splits again |
| OPR-4a, spike | partial or negative | OPR-4b to 4d shrink or are cancelled; Jupyter stays a follow-up |

## Constraints that hold across the plans

- Jupyter output is byte-identical except for the two documented preview and hub fixes (D9); the goldens never exercised a
  document `execute:` scope, so OPR-0a\'s fixtures are the guard.
- The `.cell` wrapper contract holds: one `Div.cell` per executed cell, the empty string when echo and every output
  are suppressed, exactly one wrapper per cell for the preview splice.
- No DOM postprocessor: any Quarto 1 DOM behavior is an AST transform.
- Presenter code compiles for `wasm32`; each plan that touches it runs the WASM build.
- No plan opens a braid strand for in-plan work; strands are only for things found outside every plan.

## Out of scope

- Jupyter positions; pptx, context and other knitr targets (strands); LaTeX (its own epic); marimo; the knitr sub-figure
  numbering (`bd-au5gts6e`); Jupyter subfloat synthesis (`bd-gbaykhth`); the parsed-but-dropped render flags; writing
  records from the new flag.
- RIU follow-up Items 1 to 6 and 8, and the Phase A verdict and command-2 decisions, stay in the
  [RIU follow-up plan](2026-11-07-riu-follow-up-did-we-build-it.md).
