---
title: 'Epic: one presenter path for every engine'
date: 2026-11-07
description: 'The shared Rust presenter was meant to give every engine the same cell semantics, so knitr features reach Jupyter and Julia. Today a cell goes through one renderer but two entry paths, chosen by whether the cell carries knitr-only inputs. This epic holds the intent, what is known, the open design questions and the proposed spikes; the plans are decided in the next review session.'
status: structure only; no plan written; waiting for the review session with Gordon  # 2026-11-07
---

**Follows from:** [the RIU follow-up plan](2026-11-07-riu-follow-up-did-we-build-it.md), Item 7, and
[the review note](../research/2026-11-07-riu-follow-up-review.md), "Design findings after the review".
**Intent sources:** [knitr epic](2026-10-30-knitr-execution-records-epic.md), decisions 4 ("One presenter") and 12
("The Quarto cell-option layer lives in the shared presenter"); [execution-records epic](2026-10-28-execution-records-epic.md),
decision 5 ("The Rust presenter is the long-term single presenter").

## Goal

Every engine's recorded cell goes through one presenter path. Knitr's features (interleaved output, `prompt`,
numeric `echo`, message and warning handling, unknown-option forwarding, figure labels and captions) are open to
any engine that supplies the inputs they need, and degrade cleanly for one that does not. What an engine can
supply is read from the data on its cells, not from its name.

## What is known (read 2026-11-07; verified by reading, nothing run unless stated)

- **One renderer.** Every cell ends in `render_cell_whole` (`crates/quarto-core/src/presenter/cell.rs`), and
  `CellOptions::from_lookup` (`presenter/options.rs`) is the one place that gives each option its meaning. Two
  front ends feed it: `from_config` (Jupyter `#|` options resolved at present time) and `from_knitr` (a record's
  JSON options).
- **Two entry paths.** `present_cell` (`stage/stages/record_splice.rs:431`) and `presenter/document.rs` try
  `present_knitr_cell` (`presenter/knitr_inputs.rs:59`), which returns `Some` only when a cell carries
  `quarto_resolved_options`, then fall through to `render_cell_with`.
- **What differs between them:** where option resolution happens (R at capture for knitr; the presenter for
  Jupyter); who enforces `warning` and `message` (`knitr_visibility` versus `CellVisibility::resolve`,
  `presenter/visibility.rs`); the code shown (the echoed code in the record versus the authored code); and the
  capture-side inputs only knitr supplies: output anchors, expression starts, output kinds, `known_options` and
  the plot count.
- **Features degrade when an input is absent** (`render_cell_interleaved`): no usable anchors means whole-cell
  rendering, empty expression starts disables `prompt` and numeric `echo`, empty output kinds disables message and
  warning handling.
- **Features that need no capture-side input already reach Jupyter:** `collapse`, `comment`, `prompt` (as an
  option), `fenced.echo`, `results: hide` and `asis`, `class.*` and `attr.*`, layout, `panel`, `code-fold`. The
  Jupyter option sweep (`presenter/jupyter_option_sweep.rs`) fails the build if a display-only option changes
  nothing.
- **Jupyter cannot supply anchors or expression starts today:** an `execute_request` reports no per-line positions.
  Supplying them would be new capture work per language.
- **`fig-cap` is not a conflict.** Dashed wins when both spellings are present (`canonicalize_options`,
  `options.rs:151`; Q1 `hooks.R:847`). The presenter emits a caption only when `figure_count` is `Some`, which
  only the knitr path sets.
- **Julia already goes through the Rust presenter** (`b4d827186`, 2026-11-01; `core_presents` in
  `engine/ts_record.rs:500`: Julia, a record wanted, a non-empty unit list). The TS `toMarkdown` path still runs
  when either condition fails.
- **Constraint:** Jupyter output must stay byte-identical unless a cell sets a new option (knitr epic decision 12;
  the goldens in `tests/fixtures/jupyter-markdown-goldens`).

## Open design questions (for the review session)

1. What should one `present_cell` take as optional inputs, and is "the data is present" the right capability test?
2. Where does option resolution live: the presenter (as for Jupyter), at capture (as for knitr), or layered? In
   particular, is layering the document scope under a fresh knitr record's options a no-op? For Jupyter the
   presenter must layer it because the hash excludes presentation `execute:` keys; for knitr every `execute:` key
   counts in the hash.
3. Who enforces `warning` and `message`: one declared flag per engine, or derive it from the presence of output
   kinds?
4. Should q2 add anchors and expression starts to other engines' captures, and which first? (A feature, not a
   merge; it may belong in a different epic.)
5. What does Julia's annotated notebook carry of the knitr inputs, and is the `want_record`-false route to the TS
   `toMarkdown` still reachable in a normal render?
6. How many plans does this need, and in what order? Settle this in the review session.

## Spikes (approved by Gordon, 2026-11-07)

All five are approved. Read-only or throwaway-branch; none changes product code. Results go in a research note,
and the epic is then rewritten from them.

- **S1. Option-front-end diff.** For every option key that appears in both the Jupyter sweep and the knitr option
  sweep, build `CellOptions` through `from_config` and through `from_knitr` and list where they differ. Answers
  how much of question 2 is real.
- **S2. Inputs-absent equivalence.** On a throwaway branch, present each existing Jupyter golden through the knitr
  path with every capture-side input empty, and compare bytes. Answers whether one path can reproduce Jupyter's
  goldens.
- **S3. Document-scope layering.** For the knitr option-sweep fixtures, layer the document scope under the
  recorded options and compare output bytes. Answers question 2's no-op claim.
- **S4. What Julia carries.** Read `engine/ts_record.rs` and dump a real Julia record's cell metadata keys
  (`quarto_*`); list which of the knitr inputs are present. Also trace when `core_presents` is false.
- **S5. Feature-by-input table.** One table: each presenter feature, the input it needs, which engines supply it
  today, and what it does without it. Mostly done above; make it complete and cite lines.

## Needs discussion first, not a spike

Questions 1, 3 and 4 are design choices, not facts. The review session should discuss them with Gordon before
anything is written; spikes S1 to S5 only supply evidence for that conversation.

## Plans

None yet. The review session decides how many there are and their order. Likely areas, not commitments: the entry
path merge, the options front-end merge, the enforcement flag, and any new capture inputs for other engines.

## Exit criteria

To be written in the review session.
