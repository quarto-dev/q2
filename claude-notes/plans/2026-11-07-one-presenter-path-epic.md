---
title: 'Epic: one presenter path for every engine'
date: 2026-11-07
description: 'Every recorded cell goes through one presenter that reads what a cell supplies, not which engine made it. Plain render of Julia and knitr joins the recorded route, Julia gains the capture facts knitr has (subject to a spike), and the execution-records source list is one metadata key, execute: record-sources, read by one early reader in preview, provide-hub, convert and the new q2 render (no command-line option). Nineteen plans, plus the eight of the RIU follow-up epic, in four lanes on one integration branch, written to be run by agents with no orchestrator.'
status: 'plans written 2026-11-08; ready to run; integration branch feature/one-presenter in workspace-1; two decisions still open (the retained fallback and an opt-out; Julia spike scope: OPR-4a done 2026-11-09, all three facts feasible, Gordon chooses the scope for OPR-4b); R5 decided 2026-11-09: the list moves into metadata and the command-line option is removed'
---

**Follows from:** the RIU follow-up (its Item 7; its Item 9 folded into OPR-5), and
[the review note](../research/2026-11-07-riu-follow-up-review.md). **Runs beside:** the
[RIU follow-up epic](2026-11-08-rfu-epic-riu-follow-up.md) (plans `RFU-*`); this document holds the **one execution DAG**
for both.
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
2. **Julia gains the capture facts knitr has** (expression starts, anchors, output kinds). The OPR-4a spike (2026-11-09) found all three feasible with one runner change, so the verdict is not partial; Quarto 1 never interleaved Julia, so this is new behavior.
3. **The record source list is one metadata key**, `execute: record-sources`, read by one early reader for `preview`,
   `provide-hub`, `q2 convert` (RFU-1, which replaces `q2 record export-positron`) and `q2 render`; the `--records`
   option is removed and `-M` is the command-line way (decision R5, 2026-11-09).

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
- **D4. Julia is in scope; the OPR-4a spike (2026-11-09) found it feasible.** Starts, anchors and kinds come from one runner change (OPR-4b), ride in the cell `metadata.quarto` under `emitUnits`, and are mapped to the knitr keys in OPR-4d. Julia shows only the last value, so it never interleaves like knitr. Jupyter positions are a follow-up, not part of this epic.
- **D5. Plain render is in scope.** A cached knitr chunk keeps knitr\'s cache and uses the markdown fallback with no
  structured record, quietly (OPR-0c, 2026-11-09: the record must not simply lack the chunk, because the unit count check drops
  the whole record; OPR-7b excludes cached units from the expected count instead). The LaTeX family, `pptx`, `context` and unknown knitr targets stay on the engine route, quietly
  (strands `bd-4y5scdhp`, `bd-lo1cs7u5`, `bd-ka76vcrp` track widening; LaTeX notes live in the LaTeX plan on
  `explore/pandoc-hybrid-latex`). Decided 2026-11-09 (Gordon): the retained knitr fallback survives for plain render, and there is **no opt-out control** (recorded in OPR-7d).
- **D6. `render` reads `execute: record-sources`** (updated by R5, 2026-11-09; there is no render option). Read only
  (writing stays with `execute: record: true`); default `engine`; `cache` is ignored with an info line; the list is
  resolved **per document**; no source and no `engine` warns and leaves cells inert (`--strict` errors; prose-only
  silent); a stale record, or one recorded for another format, counts as not supplied; `want_record` unchanged;
  `--no-execute` vetoes `engine`; `--replay` wins over a key, with an info line (a runtime check, which covers
  `QUARTO_REPLAY`); `preview.engine` stays preview-only; `preview --static` is in scope.
- **D7. Consolidation, decided by R5 (Gordon, 2026-11-09): move, no flag.** The source list is the metadata key
  `execute: record-sources` (a comma-separated scalar; entries `engine`, `q2`, `positron`, `cache` and an in-project
  directory), read before the pipeline by one early reader built on `collect_metadata_layers`, with per-command allowed
  values and defaults; `--records` is deleted from `preview` and `provide-hub` with no alias, and `q2 convert` gains `-M`
  (OPR-5, RFU-1). `provide-hub` and a `--share` preview treat the host list as a ceiling a guest document may only reorder
  or narrow (OPR-5 Part B, OPR-6). The evidence and every sub-decision are in the
  [OPR-0d note](../research/2026-11-08-opr-metadata-key-research.md).
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
verified. Line numbers are for `94906556f`; each plan says (read), (ran) or (relied).

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
  **Updated 2026-11-09 by the OPR-0c follow-up ([note](../research/2026-11-09-opr-measurements.md), ran):** for a cell that
  emits a bare PNG (Plots emits an inline `text/html` bundle instead, so its figures are inline on both routes), plain
  and recorded Julia name the files differently (plain `cell-2-output-1.png`, recorded `cell-1-output-1.png` for the first
  code cell after a prose block; Quarto 1 names the unlabeled one `cell-2-output-1.png` like plain and a labeled one by its
  label, `fig-b-output-1.png`, which neither q2 route does), and the recorded route drops the generic `cell-output` class
  from the figure's output div. Nothing in `crates/`, `ts-packages/`, `hub-client/src` or `resources/` reads `id="cell-N"`,
  `data-quarto` or `data-execution_count` (grep, read). `fig-format: svg` or `pdf` changes nothing on either q2 route or
  Quarto 1 for these cells (ran).
- **Correction, 2026-11-09 (ran):** a stored `.ipynb` with a labeled figure, a `fig-subcap` group and a labeled table
  resolves the single figure and the table on q2; only the group (`@fig-two`, `-1`, `-2`) is unresolved
  (`bd-gbaykhth`). Julia on the recorded route fails the same way; Julia on the plain route resolves the group with wrong
  numbers. So the flip moves Julia group references from wrong to unresolved.

## Plans, lanes and dependencies

**One integration plan, one execution DAG, for both epics.** Each plan is a unit of merge to the integration branch
`feature/one-presenter` (checked out in `workspace-1`, a landing pad nobody edits), branched from it, rebased onto it
and fast-forwarded back. A plan depends **only on complete (or cancelled) plans**; "complete" is the plan file\'s
`status:` on the integration branch. The [protocol](2026-11-08-opr-agent-protocol.md) says how. OPR plans are
[this epic\'s](2026-11-07-one-presenter-path-epic.md); RFU plans are the
[RIU follow-up epic\'s](2026-11-08-rfu-epic-riu-follow-up.md). The two run concurrently.

| Plan | Epic | What | Depends on | Lane | Status at start |
|---|---|---|---|---|---|
| [OPR-0a](2026-11-08-opr-0a-pinning-tests.md) | OPR | Pinning tests for the loops, scope, spans and entry paths | nothing | A | ready |
| [OPR-0b](2026-11-08-opr-0b-parity-corpus-and-harness.md) | OPR | Parity corpus and harness | nothing | D | ready |
| [OPR-0c](2026-11-08-opr-0c-measurements-and-spikes.md) | OPR | Cost, cache, figure and route measurements (a note) | nothing | D | ready |
| [OPR-0d](2026-11-08-opr-0d-metadata-key-research.md) | OPR | Metadata-key research (a note) | nothing | C | ready |
| [OPR-1a](2026-11-08-opr-1a-one-cell-step.md) | OPR | One per-cell step over a struct of optional inputs | 0a | A | ready |
| [OPR-1b](2026-11-08-opr-1b-one-document-function.md) | OPR | One document function returning outcomes | 1a | A | ready |
| [OPR-2](2026-11-08-opr-2-enforcement-from-hash.md) | OPR | Enforcement derived from the hash, plus audit | 1b, OPR-3 | A | ready |
| [OPR-3](2026-11-08-opr-3-julia-engine-wire.md) | OPR | Julia declaration fix and acknowledgement | nothing | B | ready |
| [OPR-4a](2026-11-08-opr-4a-julia-spike.md) | OPR | Julia spike (a note) | nothing | B | complete |
| [OPR-4b](2026-11-08-opr-4b-julia-runner-facts.md) | OPR | Runner reports the facts (another repository) | 4a, Gordon\'s go | B | ready |
| [OPR-4c](2026-11-08-opr-4c-julia-engine-facts.md) | OPR | Engine, wire and pin carry the facts | 4b, 3, Gordon\'s push | B | conditional |
| [OPR-4d](2026-11-08-opr-4d-julia-facts-consumed.md) | OPR | q2 writes the facts as cell metadata | 4c, 1b | A | ready |
| [OPR-5](2026-11-08-opr-5-execution-records-definition.md) | OPR | The `execute: record-sources` key, one early reader, `--records` removed, a ceiling for provide-hub and `--share` | 0d, RFU-1, 3 | C | ready |
| [OPR-6](2026-11-08-opr-6-render-execution-records.md) | OPR | `q2 render` reads `execute: record-sources` | 5, 1b | C | ready |
| [OPR-7a](2026-11-08-opr-7a-presentation-handshake-and-record.md) | OPR | Handshake and record without the extra cost, behind a control | 1b, 3, 6 | C | ready |
| [OPR-7b](2026-11-08-opr-7b-knitr-cache-fallback.md) | OPR | Cached knitr chunks fall back, no record | 0c | B | ready |
| [OPR-7c](2026-11-08-opr-7c-run-the-parity-gate.md) | OPR | Run the parity gate, fix or propose the allowlist | 0b, 1b, 2, 7b, 4d resolved | D | ready |
| [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) | OPR | Switch plain render to the recorded route | 7a, 7b, 7c, Gordon\'s decisions | D | blocked-on-gordon |
| [OPR-8](2026-11-08-opr-8-docs-and-end-to-end.md) | OPR | Docs, embedded-docs refresh, end-to-end | 1b, 2, 5, 6, 7d, 4d resolved, RFU-8 | D | ready |
| [RFU-1](2026-11-08-rfu-1-convert-command.md) | RFU | `q2 convert` replaces `q2 record export-positron` | nothing | C | ready |
| [RFU-2](2026-11-08-rfu-2-knitr-export-and-import-gate.md) | RFU | knitr cells export; import gate by declared kernel equivalence | RFU-1, OPR-3 | B | ready |
| [RFU-3](2026-11-08-rfu-3-positron-splice-tests.md) | RFU | EXR-16 sweep and the labeled-cell `fig-cap` splice test | OPR-1b | A | ready |
| [RFU-4](2026-11-08-rfu-4-verification-and-small-fixes.md) | RFU | Hub verification, decision 12 re-check, highlight check, Julia server messages | OPR-5 | B (was C) | ready |
| [RFU-5](2026-11-08-rfu-5-julia-e2e-flake.md) | RFU | The Julia e2e flake: find the EPIPE closer (a note) | nothing | B | ready |
| [RFU-6](2026-11-08-rfu-6-windows-run.md) | RFU | The Windows run (Gordon\'s machine) | OPR-6, RFU-1, RFU-2 | none | blocked-on-gordon |
| [RFU-7](2026-11-08-rfu-7-positron-fork.md) | RFU | Positron fork: extension update, guide, e2e, cache location, command 2 | RFU-1, RFU-2, OPR-5 | A (was C) | ready |
| [RFU-8](2026-11-08-rfu-8-docs.md) | RFU | Docs for `q2 convert` and the Positron export | RFU-1, RFU-2 resolved | B (was D) | ready |

**Lanes** (an agent takes the first startable plan in its own lane list; if none, it stops and reports):

| Lane | Workspace | Plans, in order |
|---|---|---|
| A, the presenter chain | `workspace-3` | OPR-0a, 1a, 1b, 2, 4d, RFU-3, **RFU-7** (moved from C) |
| B, Julia and knitr capture | `workspace-4` | OPR-3, 4a, 7b, RFU-5, RFU-2, OPR-4b, 4c, **RFU-8** (moved from D), **RFU-4** (moved from C) |
| C, the flag and the fork | `workspace-5` | OPR-0d, RFU-1, OPR-5, OPR-6, OPR-7a |
| D, measurement, gate, docs | `workspace-6` | OPR-0c, 0b, OPR-7c, OPR-7d, OPR-8 |

> **Lane change, 2026-11-09.** Lanes A and B went idle (all their plans complete) while C and D still held startable plans, so three plans were **reassigned**: RFU-7 from lane C to **lane A** (`workspace-3`), RFU-4 from lane C to **lane B** (`workspace-4`), and RFU-8 from lane D to **lane B** (`workspace-4`). Dependencies are unchanged. The Lane column and lane lists below show the new lanes; the plan files and the lanes diagram carry the same change.

The integration checkout is `/Users/gordon/src/q2/.worktrees/workspace-1`. Lane B puts OPR-7b and RFU-5 before OPR-4b
and 4c because those two only wait on Gordon. Lane C starts with OPR-0d (a short note Gordon must read) and then RFU-1,
so RFU-1 runs while Gordon reads the note; R5 is now decided, and OPR-5 (flipped to `ready` 2026-11-09) follows when RFU-1 and OPR-3 are done.

**How the epics run concurrently.** RFU-1 lands first and depends on nothing, so OPR-5 wires the key over the
final commands and never touches the deleted `q2 record` code. RFU-2 waits for OPR-3 only to keep edits to
`ts_protocol.rs` and the vendored Julia subtree from colliding. RFU-3 waits for OPR-1b because it pins splice output.
RFU-4 and RFU-7 wait for OPR-5 because they use the final flag spelling. The docs of both epics land before the one
embedded-docs refresh: RFU-8, then OPR-8. RFU-6 (Windows) runs once at the end, on Gordon\'s machine.

**Critical path:** OPR-0a, 1a, 1b, then OPR-6 (also needs OPR-5, which needs OPR-0d, RFU-1 and Gordon), then OPR-7a, then
OPR-7d (also needs OPR-7b, 7c and Gordon\'s decisions), then RFU-8 and OPR-8. Parallel from the start: OPR-0a, 0b, 0c, 0d,
3, 4a and RFU-1, RFU-5.

## What Gordon does between plans

Completing a plan never needs a manual flip. A plan is `ready` from the start, and its `depends-on` list alone holds it
back until the plans it waits on are resolved. `conditional` is reserved for a plan that waits on a stated condition
Gordon must check (for example, odd results from a measurement); the plan names that condition. `blocked-on-gordon` is
for work only Gordon can do.

- OPR-5, RFU-2 and RFU-7 were flipped to `ready` on 2026-11-09 (their decisions are recorded; each still waits on its `depends-on`).
- OPR-4a landed (2026-11-09): read the [note](../research/2026-11-09-opr-julia-spike.md); write the scope into OPR-4b and set it to `ready`, or set 4b, 4c and 4d to `cancelled`. **Decided 2026-11-09:** all three facts, and the stderr fix is the runner keeping stderr under `emitUnits` (written into OPR-4b); only the flip to `ready` remains.
- After OPR-4b lands: push the runner branch to the fork and write the revision into OPR-4c; set it to `ready`.
- After OPR-7c lands: sign the allowlist and set OPR-7d to `ready` (its two decisions were recorded 2026-11-09).
- When OPR-6, RFU-1 and RFU-2 have landed: run RFU-6 on a Windows machine.
- At the end: pushes of the engine and runner forks, a push or merge of the Positron fork branch, the rebase of
  `feature/one-presenter` onto `main` and the pull request. The integration branch was cut from the local review branch,
  175 commits that are on no remote and 158 commits behind the `main` tip on 2026-11-08.

## Decision points (which results could change the plan list)

| Result | If it comes back this way | What changes |
|---|---|---|
| OPR-0d, R5 | **decided 2026-11-09: metadata only** | recorded in OPR-5 (key, early reader, hash exclusions in the Jupyter, knitr and Julia declarations, ceiling), OPR-6, RFU-1 and the docs plans |
| OPR-0c, cost | expensive | OPR-7a grows into a new seam (its own plan) |
| OPR-0c, cache spike | a mixed document is wrong | OPR-7b needs Gordon (blocked) and may grow. **Came back 2026-11-09:** the document is right but only through whole-document fallback; the plan\'s design was sharpened (cached units excluded from the expected count; hook behavior on a hit measured), no Gordon decision needed |
| OPR-7c, parity | blockers beyond the known Julia three | fix plans appear before OPR-7d |
| OPR-2, audit | a capture gap in an engine | a capture-side plan, coupled to lane B |
| OPR-0a, loop divergence | more than the table | OPR-1a or OPR-1b splits again |
| OPR-4a, spike | **came back 2026-11-09: all three facts feasible, not partial** | scope chosen 2026-11-09: all three plus the stderr-at-capture fix; 4b to 4d are rewritten to the spike wire (keys in `metadata.quarto`, no `ts_protocol.rs` change expected); Jupyter stays a follow-up |

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
- Everything in the [RIU follow-up epic](2026-11-08-rfu-epic-riu-follow-up.md) is that epic\'s; it shares this
  document\'s DAG and lanes but not its decisions.
