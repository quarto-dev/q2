---
title: 'Epic: units-only engine interface (engines execute units and never parse markdown)'
date: 2026-11-02
description: 'Draft. Extends the execution-records epic from the output side to the input side: core is the only qmd parser, and an engine that opts in receives an ordered unit list with options and returns a record. Starts with spikes that settle what is feasible per engine, knitr above all.'
status: draft  # spikes not started; every plan after Phase 0 is an outline to be written from the spike results
---

**Branch:** none yet. Proposed `feature/units-only-engines-epic`, based on
`feature/execution-records-epic` once that has landed (and on the knitr epic\'s
`feature/knitr-execution-records-epic` for the knitr plans).
**Extends:** [the execution-records epic](2026-10-28-execution-records-epic.md) (EXR) and
[the knitr execution-records epic](2026-10-30-knitr-execution-records-epic.md) (KNR).
**Origin:** a discussion with Gordon on 2026-11-02, after EXR-8 and EXR-9 merged. Everything
below the Goal is a proposal until Gordon settles it; the "Settled so far" list is the only
exception.
**Terminology:** as in EXR. A **unit** is a code cell or an inline expression span, in document
order. An engine is **units-only** when it receives units and never sees prose, fences or YAML
syntax. A **group** is the presenter\'s `OutputGroup` (code, outputs, anchors), introduced on
`knr-2-presenter` at `59390f4f5`.

## Goal

EXR made the record the interface for **output**: units go in beside the document, and
per-unit outputs come back. The **input** side is unchanged: Julia, marimo and knitr still
receive the whole qmd and re-parse it (QuartoNotebookRunner\'s CommonMark chunker, marimo\'s
`breakQuartoMd`, knitr\'s chunk regexes in `knit()`), then core reconciles their parse against
its own units with count checks, line-and-code alignment and wrapper Div ids. Most unit-identity
disagreements (EXR Risk 1) are that reconciliation failing.

The goal is to make core the only parser of qmd:

```text
qmd --> core scanner --> units (code, resolved options, ids) --> engine --> record --> presenter
                         (no markdown crosses the boundary for an opted-in engine)
```

Three benefits, weighted equally by Gordon: one source of truth for unit identity, a much
smaller contract for engine authors, and a natural base for single-cell execution and freeze.

## Settled so far (Gordon, 2026-11-02)

1. An engine receives an **ordered unit list with options already parsed**.
2. The execution model is **declared per engine**, batch or session.
3. Scope covers Jupyter, knitr, Julia and marimo; third-party TypeScript engines; inline
   expressions; and engine features that need document structure (`child=`, `ref.label`, setup
   chunks, QuartoNotebookRunner cell expansions).
4. Where possible, **no fallback to markdown**. Generated content should be structured, not an
   opaque markdown blob. The KNR fallback unit stays only for what cannot be captured.

## Proposed decisions (to confirm after the spikes)

1. **Additive and opt-in.** An optional engine capability (name to be chosen in S6); absent
   means today\'s behavior, including `target.markdown`. The Quarto API is not on npm yet
   (bd-ixz93hjp), which gives room, but the EXR pattern of optional versioned fields is kept.
2. **Two paths stay.** The markdown path remains for Q1-compatible extension engines. Removing it
   is out of scope, so the bug class is removed for opted-in engines only.
3. **Core owns structure.** Static `<<name>>`, `ref.label` and literal `child=` are resolved in
   core before units are sent (subject to S3). Option text is parsed by core; option **values**
   that need R are resolved by the engine and come back in the record (KNR decision 11).
4. **Batch first.** A session method (`executeUnit`) is a later, separate optional method.
5. **Runtime-generated units** (computed `child=`, `knit_child()`, `knit_expand()`) are nested
   sub-units: ids scoped under the parent unit, outputs kept as real nbformat outputs, grouped by
   a cell-metadata side table. Core\'s top-level ids and count check stay untouched. Subject to
   S2 and S5.

## Phase 0: spikes

Each spike answers one question, runs against real code, and ends in a short research note under
`claude-notes/research/` plus a recommendation that the epic text then absorbs. A spike that
cannot be answered mechanically stops and asks Gordon. Spikes are independent unless a
dependency is named; S6 goes first because it shapes the rest.

### S6: who calls the markdown today, and what is the additive surface?

- **Question:** which q2 and TypeScript-host sites read `target.markdown`, `markdownForFile`,
  `partitionedMarkdown` and the engine\'s `target()` result, and what capability shape lets an
  engine opt out without breaking a Q1 engine?
- **Method:** trace callers in `crates/quarto-core/src/engine/ts_engine.rs`,
  `ts_protocol.rs`, `ts-packages/quarto-types/src/execution-engine.ts` and `execution.ts`, and
  the engine host in `ts-packages/quarto-engine-host-node`. Sketch the optional fields.
- **Settles:** proposed decision 1; whether making `target.markdown` optional is ever needed or
  keeping it populated is enough; what third-party engines see (UNI-8).

### S1: can knitr run core-supplied chunks without `knit()` splitting a document?

- **Question:** is there a stable enough R surface to run one chunk, given its code and its
  resolved options, with knitr\'s hooks, engines and state intact? Candidates: `evaluate` plus
  `sew` (what KNR already patches), knitr\'s block-execution internals, or `knit()` on a document
  that R reconstructs from the units.
- **Method:** a throwaway R harness over the KNR corpus (`crates/quarto-core/tests/fixtures/knitr-corpus/`),
  on knitr 1.44, 1.50 and the snapshot in `external-sources/knitr`. For each surface, run the
  corpus and compare against the baseline the way KNR-1 does (differential, classified differences).
  List every internal function used and whether it is exported.
- **Settles:** whether a units-only knitr exists at all, or whether the realistic form is "core
  sends units for identity and order, and knitr still runs `knit()` on a reconstructed
  document". This gates UNI-6 and is the most important spike.
- **Needs from KNR:** KNR-1\'s harness and corpus; coordinate with the knitr epic\'s owner.

### S2: runtime-generated knitr units

- **Question:** what does the capture see for `child=` with a computed path, `knit_child()`
  called from inside a chunk, and `knit_expand()`? Do the pieces include the child chunk\'s
  source? Can each child chunk be linked to its parent unit?
- **Method:** fixtures for each construct; record the hook sequence and payloads. KNR-1 S5 covered
  a literal `child=`: its chunks fire the hooks like ordinary chunks, the parent fires none, and
  the child\'s output is spliced at the parent\'s position. Extend that, including nesting depth
  and a loop that calls `knit_child()` several times.
- **Settles:** whether nested sub-units (proposed decision 5) are feasible on the capture side,
  and what a parent link needs.

### S5: encoding sub-units in nbformat

- **Question:** can a flat cell carry grouped sub-unit outputs through a `quarto_subunits` side
  table, and does the result stay valid and presentable?
- **Method:** (a) confirm from the nbformat 4.5 JSON schema whether `stream` and `error`
  outputs have a `metadata` field (believed not; unverified), which decides whether per-output
  tagging is possible at all; (b) prototype the table `{id, label, options, hash, source, outputs:
  [start, end), anchors}` on a hand-built record; (c) drive the presenter\'s `interleave_group`
  over each sub-unit; (d) run `nbformat` validation and, if a Positron dev build is at hand, its
  cache reader, to check that the cell stays valid and the grouping is simply ignored.
- **Settles:** the sub-unit record shape. **Depends on:** KNR-2\'s group type (landed at
  `59390f4f5` on `knr-2-presenter`; confirm it is on the knitr epic branch).

### S3: static indirection and the meaning of a unit\'s code

- **Question:** what do `<<name>>`, `ref.label` and `knit_expand` mean for a unit\'s source,
  hash and id, and who should resolve them?
- **Method:** check how q2 handles them today (a first grep of the knitr Rust code found no
  handling, so knitr itself resolves them); run a corpus of reuse patterns; decide whether the
  content hash and the Positron-compatible id are taken over the authored text (as EXR decision 18
  says) and the exec hash over the resolved code.
- **Settles:** proposed decision 3 and what the unit payload carries (`code` as authored,
  `resolvedCode`, or both).

### S4: which option values need R?

- **Question:** how many real chunks have option values that cannot be resolved without R
  (expressions, `opts_chunk$set` defaults, `opts_template`, `opts.label`, `opts_hooks`)?
- **Method:** scan the KNR corpus and a sample of public qmd and Rmd documents; classify each
  option as literal, static reference or R-dependent. Feeds the unit payload: parsed values,
  raw text, or both.
- **Settles:** the wire shape of a unit\'s options and what core can pre-resolve.

### S7: can QuartoNotebookRunner run a cell list?

- **Question:** is there, or can we add in the fork (`gordonwoodhull/QuartoNotebookRunner.jl`),
  an entry point that runs a given list of cells and spans instead of parsing a file? What
  becomes of cell expansions and the line-and-code alignment (EXR-8 B5b)?
- **Method:** read the fork\'s `parsing.jl` and server entry points; prototype the smallest
  change; rerun the Julia fixtures.
- **Settles:** UNI-4 scope.

### S8: can marimo take a cell list?

- **Question:** can the marimo Python compiler (`extract.py`, `command.py`) take cells directly,
  and does the reactive dependency analysis still work when core owns ordering? What about the
  bare-`sql` interop rewrite?
- **Method:** read the fork\'s engine and Python entry points; try a cell list on the EXR-9
  fixtures.
- **Settles:** UNI-5 scope, and whether marimo must stay batch-only.

### Inline spans in knitr (not a spike: follows KNR-4)

The knitr epic owns this question: KNR-4 decision 1 treats a span as a unit, and its provisional
decision 3 settles whether the spelling is recorded and whether the two spellings keep their
different `NULL` rule (EXR-2 decision 2 had classic `r expr` spans outside the unit list).
This epic follows whatever KNR-4 settles and adds no spike of its own. UNI-1 reads the result
when it defines unit payloads for inline units.

## Plans after the spikes (outlines; to be written from the spike results)

| ID | Plan | Depends on |
|---|---|---|
| UNI-1 | Core: resolved units (options, `code`, reference resolution) in the shared scanner output; golden fixtures | S3, S4, S6 |
| UNI-2 | Wire and TypeScript types: optional fields and capability flag; Rust and TypeScript mirrors; wire-parity test | UNI-1, S6 |
| UNI-3 | Jupyter as the reference units-only engine; per-unit options shape | UNI-1 |
| UNI-4 | Julia: cell-list entry point in QuartoNotebookRunner; retire expansion and alignment special cases for opt-in | UNI-2, S7 |
| UNI-5 | marimo: batch consumer of the unit list; drop the `breakQuartoMd` re-split | UNI-2, S8 |
| UNI-6 | knitr: units-only execution (form decided by S1), sub-units for generated chunks | UNI-2, S1, S2, S5, KNR-3 |
| UNI-7 | Session model (`executeUnit`); optional and possibly deferred | UNI-3 |
| UNI-8 | Third-party engines: the opt-in path and a migration guide | UNI-2, S6 |
| UNI-9 | Docs and end-to-end: extension-author documentation, one render per engine, a conformance corpus | all |

## Overrides of the knitr epic

Nothing here conflicts with the knitr epic today. Two of its settled decisions would be
overridden if the spikes come out a certain way, so the work is sequenced after KNR-6 (the flip)
and carries this list, as KNR does against EXR. Checked against the epic text, KNR-3 and KNR-4 on
2026-11-02; KNR-5 and KNR-6 were not read.

- **Decisions 3 and 7 (`knit()` stays the driver; reuse is whole-document).** If S1 finds a
  stable per-chunk surface, UNI-6 replaces the `knit()`-driven capture with core-supplied units.
  If S1 does not, knitr stays as KNR defines it and UNI-6 shrinks to the reconstructed-document
  form.
- **Decision 8 and KNR-3 decision 6 (`child=` becomes a fallback unit or a counted mismatch).**
  Nested sub-units (proposed decision 5) would replace that for generated chunks. Fallback
  units stay for knitr cache hits, overridden hooks and LaTeX targets (KNR decisions 8 and 13).

Consistent with the knitr epic: resolved options in the record with the current header winning
(decision 11), single-cell execution out of scope (the session model here is optional and
deferred), core\'s unit list handed to R (KNR-4 decision 1), and the anchors keys (decision 21),
which sub-units reuse per group.

## Risks

1. **knitr may not support it** (S1). If only internals reach a per-chunk run, the knitr form
   is core-sent units around a reconstructed document, and the "engines never see markdown" goal
   holds for three of four engines.
2. **Two parsers persist** for Q1-compatible engines, so the bug class is removed only for
   opted-in engines.
3. **Format drift with the knitr epic.** KNR-3\'s record shape and KNR-6\'s flip are in flight;
   sub-units must reuse its anchors and kinds keys, not fork them.
4. **marimo is a reactive graph**, so per-cell execution is out; batch is the only fit (S8).
5. **Hash semantics for resolved code** (S3) touch Positron interop (EXR-16 and KNR-5).

## Out of scope (proposed)

- Removing the markdown path or the Q1-compatible engine contract.
- Freeze itself (this epic is a base for it, as EXR is).
- Upstreaming engine fork changes (EXR decision 32 applies).

## Open questions for Gordon

1. Should `ref.label`, `<<name>>` and a literal `child=` be resolved in core, or declared
   unsupported in units-only mode? (S3 informs it.)
2. Does knitr\'s `` `r expr` `` become a unit? (Owned by KNR-4 decision 3; this epic follows it.)
3. Is the session model (UNI-7) in this epic or deferred?
4. Separate epic on its own branch, or folded into the knitr epic? (This draft assumes separate.)

## Checklist

### Phase 0: spikes

- [ ] S6: callers of the markdown and the additive capability shape (research note)
- [ ] S1: knitr per-chunk execution without `knit()` (research note; gates UNI-6)
- [ ] S2: runtime-generated knitr units (research note)
- [ ] S5: sub-unit encoding in nbformat (research note and a hand-built record)
- [ ] S3: static indirection and the meaning of a unit\'s code (research note)
- [ ] S4: which option values need R (research note with corpus counts)
- [ ] S7: QuartoNotebookRunner cell-list entry point (research note)
- [ ] S8: marimo cell-list entry point (research note)
- [ ] Fold the spike results into this epic: settle the proposed decisions, rewrite the plans
  table, and ask Gordon the open questions

### Later phases

Written once Phase 0 reports. Each UNI plan starts with its tests, per the repo\'s TDD rule.
