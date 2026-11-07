---
title: 'Epic: one presenter path for every engine'
date: 2026-11-07
description: 'The shared Rust presenter was meant to give every engine the same cell semantics, so knitr features reach Jupyter and Julia. The five spikes are done: the option readers are one reader plus a rename, the Jupyter goldens survive the knitr entry path except for warning enforcement, the document scope is not layered on the knitr path, and Julia and knitr reach the Rust presenter only when a record is wanted. The design questions are sharpened for the review with Gordon; the plan list and exit criteria are not written.'
status: spikes done; design questions open for the review session with Gordon; no plan written  # 2026-11-07
---

**Follows from:** [the RIU follow-up plan](2026-11-07-riu-follow-up-did-we-build-it.md), Item 7, and
[the review note](../research/2026-11-07-riu-follow-up-review.md), "Design findings after the review".
**Evidence:** [the spike results](../research/2026-11-07-one-presenter-spikes.md) (S1 to S5; every number below
comes from it, with its ran/read marks and its list of what was not verified).
**Intent sources:** [knitr epic](2026-10-30-knitr-execution-records-epic.md), decisions 4 ("One presenter") and 12
("The Quarto cell-option layer lives in the shared presenter"); [execution-records epic](2026-10-28-execution-records-epic.md),
decision 5 ("The Rust presenter is the long-term single presenter").

## Goal

Every engine's recorded cell goes through one presenter path. Knitr's features (interleaved output, `prompt`,
numeric `echo`, message and warning handling, unknown-option forwarding, figure labels and captions) are open to any
engine that supplies the inputs they need, and degrade cleanly for one that does not. What an engine can supply is
read from the data on its cells, not from its name.

**Scope the spikes force us to state.** "Recorded" matters. A plain `q2 render` of a Julia or knitr document asks
for no record, so neither reaches the Rust presenter: Julia is presented by the TS engine's `toMarkdown`, knitr by
R's finished markdown. Only `execute: record: true`, the preview, the hub and the trace make them go through it.
Unifying the record presenters does not touch that plain-render route. Whether the epic covers it is Gordon's
call (question 7).

## What the spikes found

- **One renderer, one reader, two front ends that are almost the same (S1, ran).** Every cell ends in
  `render_cell_whole` (`crates/quarto-core/src/presenter/cell.rs:131`) and `CellOptions::from_lookup` is the one
  place that gives each option its meaning. Over 65 option keys (the union of the Jupyter and knitr sweeps), the
  Jupyter front end (`from_config`) and the knitr front end (`from_knitr`) built **identical** options for 55 and
  differed for 10, all ten being knitr's dotted aliases (`fig.cap`, `out.width`, ...), which only the knitr side
  renames (`canonicalize_options`, `options.rs:151`). So "merging the front ends" is a rename step, not a reader.
- **Two loops, one reader behind them (S5, read).** `present_document` (`presenter/document.rs:104`, five
  non-test callers) and the splice stage's `present_cell` (`stage/stages/record_splice.rs:431`) are separate
  implementations of the per-cell step. Both try `present_knitr_cell` (`presenter/knitr_inputs.rs:59`), which
  returns `Some` only when the cell carries `quarto_resolved_options`, then fall through to `render_cell_with`.
  That one metadata key is the switch between the paths, not a description of capability.
- **With every capture input empty the knitr entry path reproduces Jupyter's bytes, except for `warning`
  (S2, ran).** 12 of the 13 shadow goldens are byte-identical; the 13th (`warning-false-stderr`) shows stderr that
  `#| warning: false` should hide, because `knitr_visibility` (`presenter/visibility.rs:113`) deliberately does
  not read `warning`. **Caveat:** none of the 13 fixtures sets a document-level `execute:` key, so the goldens do
  not test scope layering at all; the six `ipynb-*` goldens use another route and were not run.
- **The document scope is not layered on the knitr path (S3, ran).** On the splice path, a document
  `execute: {echo|output|include: false}` hides a knitr cell only if R already recorded that key; otherwise the
  scope is ignored. In a real record, `output` was recorded `null` under a document `output: false`, yet a real
  `q2 render` hid the output. **Unresolved:** what applies it on that route. (Candidates in the spike note.) So
  the claim "layering the scope under a fresh record's options is a no-op" is not established, and for `output`
  it is false by construction.
- **Julia carries none of the knitr inputs (S4, ran for the dump, read for the route).** A real Julia record has
  `quarto_cell_id`, `quarto_content_hash`, `quarto_engine`, `quarto_exec_hash`, `quarto_kernel`, `quarto_kind`,
  `quarto_lang`, `quarto_label` and the author's own `#|` keys, and none of the anchors, expression starts, output
  kinds, resolved options, hash rule or header keys. A Julia cell always takes the Jupyter fall-through.
  `core_presents` (`engine/ts_record.rs:500`) is false for a plain render, so the TS `toMarkdown` presents.
- **Who can enforce `warning` and `message` follows from the hash (S5, read).** For knitr, `warning`, `message`,
  `error`, `include` and `output` are in the hash (editing one re-runs the cell) because R changes what runs, so
  R has already dropped what the option forbids and the presenter must not drop it again. For Jupyter all four
  visibility keys are display-only, so the presenter enforces them. The record already carries each engine's hash
  declaration (`quarto_engines[].hash_definition`, seen in a dumped knitr record).
- **Features that need no capture-side input already reach Jupyter:** `collapse`, `comment`, `prompt` (inexactly,
  see below), `fenced.echo`, `results: hide` and `asis`, `class.*` and `attr.*`, layout, `panel`, `code-fold`. The
  Jupyter option sweep (`presenter/jupyter_option_sweep.rs`) fails the build if a display-only option changes
  nothing.
- **Jupyter cannot supply anchors or expression starts today:** an `execute_request` reports no per-line positions
  (stated by the earlier review; not re-checked). Supplying them would be new capture work per language.
- **`fig-cap` is not a conflict.** Dashed wins when both spellings are present; the presenter emits a caption only
  when `figure_count` is `Some`, which only the knitr path sets (S5 table has the line).
- **Constraint:** Jupyter output must stay byte-identical unless a cell sets a new option (knitr epic decision 12;
  goldens in `tests/fixtures/jupyter-markdown-goldens`). Because the goldens never exercise a document `execute:`
  scope, they are a weaker guard than they look for the layering question.

The complete feature-by-input table is in the spike note (S5). In short: interleaving needs anchors; `prompt` and
numeric `echo` need expression starts (without them `prompt` treats the code as one expression, which is wrong
for a multi-expression cell, and numeric `echo` is ignored); numeric `warning` and `message` need output kinds;
figure captions need the plot count; echoed code needs `quarto_echoed_code`.

## Open design questions (for the review session with Gordon)

Questions 1, 3, 4 and 7 are design choices; the spike results are evidence for them, not answers.

1. **What should one `present_cell` take as optional inputs, and is "the data is present" the right capability
   test?** The spikes say the inputs are the six in the S5 table, each independently optional, and that
   degradation already works per input. What does not work is the *selection* of the path by the marker key
   `quarto_resolved_options`: it bundles resolution, `known_options` forwarding, the figure count and the dotted
   rename together. A concrete shape to discuss: one struct of optional inputs built from the cell's metadata,
   with `quarto_resolved_options` becoming one field of it instead of the switch.
2. **Where does option resolution live, and which values arrive?** The reader is shared (S1), so the question is
   about values. Options: (a) layer the document scope under the recorded options at present time; not a no-op
   for `output`, and the goldens cannot catch a mistake; (b) make capture record every presentation-relevant
   `execute:` key under its own name; (c) leave it. Before choosing, **trace what applies a document `output: false`
   on the live knitr route** (the unresolved item above); the answer may show (a) or (b) is already half done.
3. **Who enforces `warning` and `message`?** Proposal for discussion, not verified by running: derive it from the
   data that already exists, the engine's hash declaration. The presenter enforces an option exactly when the
   record's `hash_definition` lists it as display-only; when it is in the hash, capture enforced it and the
   presenter leaves stderr alone. That replaces `knitr_visibility` versus `CellVisibility::resolve` with one rule
   and needs no new per-engine flag. What it must be tested against: the one differing golden
   (`warning-false-stderr`) and the knitr sweep's `Hashed` rows.
4. **Should q2 add anchors and expression starts to other engines' captures, and which first?** Only `prompt`,
   numeric `echo` and interleaved output need them, and for Jupyter-family engines they need per-line positions
   the protocol does not give. This is a feature, not part of a merge; it probably belongs in a different epic.
   Recommended: out of scope here; record it as a follow-up and let the merge degrade cleanly without them.
5. **What does Julia carry, and is the `want_record`-false route reachable?** Answered: Julia carries none of the
   knitr inputs, and the TS `toMarkdown` route is the **default** one (a plain render), not an edge case.
6. **How many plans does this need, and in what order?** Left to the review session. Evidence for the ordering:
   the goldens need a document-scope fixture before any layering change (a tests-first plan on its own); the trace
   in question 2 gates the options plan; the entry-path merge and the enforcement rule are independent of each
   other once those exist.
7. **Does the epic cover the plain-render route of Julia and knitr?** New. If yes, the plain render would need a
   record built and presented every time (cost, and a behavior change for every Julia and knitr user); if no, the
   epic's goal sentence stays "every recorded cell". Gordon's decision.

## Still to investigate (for the review session's sub-agents, not for a spike)

- What applies a document-level `output: false` to a knitr cell on the live render route, and on the preview and
  hub routes that present from a stored record.
- The six `ipynb-*` stored-replay goldens through the knitr entry path.
- A real plain render of a Julia and a knitr document, to confirm the engine-owned route by running it (read from
  the code and one unit test so far).
- Hash coupling for any change that moves where an option is resolved: the hash definitions
  (`engine/knitr/hash_rule.rs`, `jupyter_hash_definition()`) decide which options re-run a cell.
- What a merge touches in the WASM, preview and hub paths (`record_splice.rs` is shared by preview and the hub).

## Plans

None yet. The review session decides how many there are and their order. Likely areas, not commitments: a
document-scope golden fixture (tests first); the entry-path merge; the enforcement rule; the options-resolution
change; any new capture inputs for other engines.

## Exit criteria

To be written in the review session.
