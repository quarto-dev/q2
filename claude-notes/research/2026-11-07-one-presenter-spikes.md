---
title: 'One presenter path: spikes S1 to S5'
date: 2026-11-07
description: 'Results of the five approved spikes for the one-presenter-path epic: the option front ends differ only in dotted knitr aliases; 12 of 13 Jupyter goldens survive the knitr entry path with every capture input empty (the 13th is the warning-enforcement difference); the document scope is not layered on the knitr path; Julia and knitr reach the Rust presenter only when a record is wanted; and the feature-by-input table.'
---

**Epic:** [one presenter path](../plans/2026-11-07-one-presenter-path-epic.md). Tree: branch
`feature/riu-follow-up-review` at `8cfec0fc1`, plus the throwaway branch `spike/one-presenter-s1-s3`
(`62afa5177`; **never merge, never push**; three measurement tests that print results).
Marks: **ran** (I ran it and read the output), **read** (read the code, nothing run), **not done**.
Line numbers are for `8cfec0fc1`.

## Premises that did not hold

Read these first.

1. **"Julia already goes through the Rust presenter" is true only when a record is wanted.** `core_presents`
   (`engine/ts_record.rs:500`) is `engine == "julia" && ctx.want_record && !ctx.units.is_empty()`, and
   `want_record` is set only by `execute: record: true` or an observer that asks for a record (the preview, the
   hub, the trace): `stage/stages/engine_execution.rs:548-554`. **A plain `q2 render` of a Julia document has
   `want_record` false, so the TS engine's `toMarkdown` still presents it.** A unit test pins that
   (`ts_record.rs:1624`). The same holds for knitr: `plan_capture` (`engine/knitr/target_route.rs:78`) returns
   `capture: false` when no record is wanted, so R's finished markdown is used. (**read**; I did not render a
   plain Julia or knitr document to confirm the route by running it.) The epic's goal, "every engine's recorded
   cell goes through one presenter path", is about recorded cells, and that wording is accurate; but a plain
   render of Julia and knitr is a third path that no unification of the record presenters touches.
2. **"Layering the document scope under a fresh knitr record's options is a no-op" is not established** (S3).
   See below, including a contradiction I could not trace.
3. **The epic names two entry paths, `present_cell` and `presenter/document.rs`. There are two loops, not one
   function with two callers.** `present_document` (`presenter/document.rs:104`) has five non-test call sites
   (`engine/skeleton.rs:217` knitr, `engine/replay.rs:178`, `engine/ipynb_replay.rs:238`,
   `engine/jupyter/text_execute.rs:658`, `engine/ts_record.rs:542` Julia); the splice stage's `present_cell`
   (`stage/stages/record_splice.rs:431`) is a second, separate implementation of the per-cell step. Both try
   `present_knitr_cell` and fall through to `render_cell_with`. (**read**)

## S1. Option front-end diff (**ran**)

Method: for every key in the union of the Jupyter sweep and the knitr sweep (65 keys), build `CellOptions` through
`from_config` (a YAML `#|` line) and through `from_knitr` (the same value as JSON, derived from the parsed
config so quoting does not differ), with the one known difference (`keep_trailing_space`) masked, and compare.
Test: `spike_s1_front_end_diff` in `presenter/options.rs` on the spike branch.

- **55 of 65 identical.** That includes every key the Jupyter sweep covers that has a canonical spelling, plus
  `results`, `collapse`, `prompt`, `comment`, `fig.show`, `fenced.echo` and the `class.*` and `attr.*` families.
- **10 differ, and all ten are knitr's dotted aliases:** `fig.align`, `fig.env`, `fig.cap`, `fig.scap`,
  `fig.subcap`, `fig.pos`, `out.width`, `out.height`, `out.extra`, `fig.link`. `from_knitr` renames them to the
  dashed canonical name (`canonicalize_options`, `options.rs:151`, from `OPTION_ALIASES`, `options.rs:25`);
  `from_config` reads dashed names only, so a Jupyter cell that writes `fig.cap` gets nothing. This is
  intentional in the knitr direction (D1) and is a latent gap only if Jupyter authors are meant to use the dotted
  names, which Quarto 1 does not forward for Jupyter either (**not checked** against Q1).
- Consequence for question 2: the two front ends are one reader plus one rename step. "Where option resolution
  lives" is not about the reader; it is about *which values arrive*, which is S3.
- **Not done:** forwarding of unknown options (`forwarding` for Jupyter, `from_knitr_known` for knitr). My first
  attempt compared them and was invalid (an empty `known_options` list makes every key unknown), so I dropped it.
  Known by reading: Jupyter forwards from the cell's own `#|` options, not the document scope
  (`options.rs:318`, called at `document.rs:168`); knitr forwards each recorded key not in the engine's
  `known_options` (`options.rs:360`); with no `known_options`, nothing is forwarded.

## S2. Inputs-absent equivalence (**ran**)

Method: the existing kernel-free oracle `stored_records_present_to_the_goldens`
(`tests/integration/jupyter_markdown_goldens.rs`) presents each stored golden record through `present_document`.
The spike copies it and adds one metadata key, `quarto_resolved_options: {}`, to every record cell. That key alone
makes `present_knitr_cell` return `Some` (`presenter/knitr_inputs.rs:59`), so every cell takes the knitr entry path
with anchors, expression starts and output kinds all absent and no `known_options`. Test:
`spike_s2_goldens_through_the_knitr_entry_path` on the spike branch. The unmodified oracle passes in the same run.

- **12 of 13 shadow goldens are byte-identical through the knitr entry path:** `ceded-ojs`, `echo-false`,
  `error-allowed`, `hidden-then-plot`, `html`, `include-false`, `markdown-only`, `output-false`, `plain-value`,
  `png-plus-text`, `stdout`, `twenty-cells`.
- **The 13th, `warning-false-stderr`, differs in exactly one way:** the knitr path shows the stderr output that
  `#| warning: false` should hide. The cause is `knitr_visibility` (`presenter/visibility.rs:113`), which
  deliberately does not read `warning` ("enforced by R at capture"), versus `CellVisibility::resolve`
  (`visibility.rs:87`), which filters stderr for Jupyter.
- **What this does and does not show.** It shows that, with every capture input empty, the knitr path degrades to
  Jupyter's bytes for everything except warning enforcement. It does **not** show equivalence under a document
  `execute:` scope: **none of the 13 shadow fixtures sets a document-level `execute:` key** (I grepped the
  fixtures), so the goldens never exercise scope layering. The six `ipynb-*` goldens use a different route
  (stored replay) and were **not run through this experiment**.

## S3. Document-scope layering (**ran**; one contradiction unresolved)

Method: `spike_s3_doc_scope_under_recorded_options` in `record_splice/tests/option_sweep.rs` on the spike branch.
A knitr-shaped record for `` ```{r} 1 ``` `` is spliced under a front matter `execute: {KEY: false}`, with the
recorded `quarto_resolved_options` either empty or already holding `KEY: false`. I report whether the code and the
output text reach the page. (My first column, "scope changes output", compares against a document without front
matter and is meaningless; ignore it.)

| KEY | Recorded options lack KEY | Recorded options hold KEY |
|---|---|---|
| `echo: false` | code **shown** (scope ignored) | code hidden |
| `output: false` | output **shown** (scope ignored) | output hidden |
| `include: false` | cell **shown** (scope ignored) | cell hidden |
| `warning: false` | no difference visible (the fixture has no stderr) | no difference visible |

So on the splice path a knitr cell ignores the document `execute:` scope entirely, and the scope matters only
through whatever R recorded. That matches the doc comment at `presenter/knitr_inputs.rs:18` ("the document scope
is not layered, because it is part of the hash") and the code: `present_cell` and `present_document` compute
`resolved` and `visibility` from the scope but use them only in the Jupyter fall-through
(`record_splice.rs:447-474`, `document.rs:163-209`).

**The contradiction.** I rendered a real knitr document with `execute: {record: true, output: false}` through
`target/debug/q2 render` (**ran**). The record's cells held their outputs and `quarto_resolved_options.output` was
`null` (not `false`); `warning` and `message` were recorded as `false`. By the table, the splice path would show
the output. **The rendered HTML had no `cell-output` divs** (0 of them; the same document without `output: false`
had 2). So something on the real render path applies a document-level `output: false` to a knitr cell. I did not
trace what. Candidates I looked at and could not rule in or out: the knitr format config receives the document
scope (`engine/knitr/mod.rs:536`, `overlay_document_scope`); `present_skeleton` receives `ctx.execute_scope`
(`engine/skeleton.rs:180`, `:221`) and hands it to `present_document`, which ignores it for knitr cells.
**Next step for a2:** trace where `output: false` is applied to a knitr cell on the live render route, and then
whether the preview and hub routes (which present from a stored record) do the same.

What this means for question 2: "layering is a no-op" holds only for keys R records under their own name. It
does not hold for `output`, which R left `null` in the real record. Whether to layer, or to make capture record
every presentation-relevant `execute:` key, is the design choice, and it needs the trace above first.

## S4. What Julia carries (**ran** for the record dump; **read** for the route)

A real Julia record (a two-cell document, the second a Plots plot with `#| label: fig-j`, rendered with
`execute: record: true` through `target/debug/q2`) has this per-cell metadata:
`quarto_cell_id`, `quarto_content_hash`, `quarto_engine`, `quarto_exec_hash`, `quarto_kernel`, `quarto_kind`,
`quarto_lang`, `quarto_label` (labeled cell) and the author's own `#|` keys as plain keys (`echo`, `label`).
Compared with a knitr cell's metadata (dumped the same day), **Julia carries none of the knitr-only inputs**:
no `quarto_output_anchors`, `quarto_expression_starts`, `quarto_output_kinds`, `quarto_resolved_options`,
`quarto_hash_rule` or `quarto_header_keys`, and the engine entry has no `known_options` for the presenter
(**read**: `present_knitr_cell` returns `None` without `quarto_resolved_options`, `knitr_inputs.rs:67`). The plot
cell's outputs are a stderr `stream` and an `execute_result` bundle with `application/pdf`, `image/png`,
`image/svg+xml` and `text/html`. So a Julia cell takes the Jupyter fall-through, always.

Route (**read**, plus the unit test at `ts_record.rs:1623-1625`): `core_presents` is false when any of these holds:
the engine is not `julia`; no record is wanted (**a plain render**); the unit list is empty. The first and third
are marimo and a document with no cells; the second is the normal `q2 render`. In those cases the extension's own
`toMarkdown` presents. This answers epic question 5: yes, the `want_record`-false route is reachable, and it is
the default one.

## S5. Feature by input (**read**)

"Supplies it" is read from the code that fills the input. "Without it" is what the presenter does.

| Feature | Input it needs | Who supplies it today | Without it |
|---|---|---|---|
| Source and outputs, whole cell | outputs, code | every engine | (the baseline) `render_cell_whole`, `presenter/cell.rs:131` |
| Interleaved source and output | `quarto_output_anchors` | knitr (R capture) | whole-cell rendering when anchors are absent, wrong length, decreasing or past the end (`render_cell_interleaved`, `cell.rs:169`, usable check at `:200`) |
| `prompt` | `quarto_expression_starts` | knitr | still applies, but the code is treated as one expression: first line `> `, every other line `+ ` (`options.rs:709`), inexact for a multi-expression cell. Jupyter reads `prompt` too (S1), so a Jupyter `prompt: true` already degrades this way |
| Numeric `echo` (`echo: [1, 3]`) | expression starts | knitr | ignored: whole code shown (`cell.rs:180` filters on non-empty starts) |
| Numeric `warning` and `message` | `quarto_output_kinds` | knitr | ignored when the kinds list is missing or the wrong length (`select_by_kind`, `cell.rs:281`) |
| Boolean `warning: false` | none, but who enforces differs | Jupyter: the presenter filters stderr (`CellVisibility::resolve`, `visibility.rs:87`). knitr: R enforces at capture; the presenter must not (`knitr_visibility`, `visibility.rs:113`) | S2: this is the one golden that differs |
| `message: false` | knitr only | R at capture (`message` is in the knitr hash, `option_sweep.rs`: Hashed) | Jupyter has no `message` option |
| Figure caption, label, ids | `figure_count` (`with_figure_count`, `knitr_inputs.rs:139`) | knitr path only | `plot_image_id` returns `None` (`options.rs:590`): no figure id; the caption wrapper is not emitted |
| Layout synthesis (`fig.sep`, `fig.ncol`) | plot count and recorded options | knitr (`synthesize_layout`, `knitr_inputs.rs:233`) | not applied; explicit `layout-*` keys still work (S1) |
| Echoed code (what knitr printed) | `quarto_echoed_code` | knitr | the authored code is shown |
| Dotted option names | recorded options | knitr (`layer_options`, `knitr_inputs.rs:189`) | Jupyter reads dashed names only (S1) |
| Unknown-option forwarding | Jupyter: the cell's own `#|` options; knitr: recorded keys not in `known_options` | both, from different sources | none forwarded |
| Document `execute:` scope | the document scope | Jupyter: layered at present time (`resolve_cell_options`, `visibility.rs:21`); knitr: not layered on the splice path (S3) | see S3 |

Hash coupling that decides who can own an option (**read**, `option_sweep.rs` tables): for knitr,
`warning`, `message`, `error`, `include` and `output` are in the hash (`Hashed`: editing one re-runs the cell),
because R changes what runs; for Jupyter all four visibility keys are display-only. That is why
`knitr_visibility` ignores `warning`: R already dropped what the option forbids. It is the real answer to epic
question 3: "who enforces `warning` and `message`" follows from whether the engine's hash contains the option.

## What was not verified

- Any real preview, hub or WASM path for these questions. All of S1 to S3 are in-process tests of the presenter
  and the splice stage; S3's contradiction is in the real `q2 render` only.
- The six `ipynb-*` stored-replay goldens through the knitr entry path (S2).
- That a plain `q2 render` of Julia or knitr takes the engine-owned path: read from the code and one unit test,
  not run.
- Whether Jupyter's own capture could supply anchors or expression starts (the epic says no per-line positions
  exist in an `execute_request`; not re-checked).
- Windows, hub-client, and the revealjs and Typst targets.
- Everything on the spike branch is a printed measurement; none of it is a regression test.
