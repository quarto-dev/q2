---
title: 'EXR-19: widget and HTML-dependency outputs in the Rust presenter'
date: 2026-11-01
description: 'The Rust presenter has no handling for Jupyter widget and dependency-bearing HTML outputs, and the engine path that produced their dependencies is skipped when core presents. Proposes one engine-neutral dependency contract (per-output manifest, presenter policy, per-engine adapters) shared with the knitr epic, and records what the knitr epic and Positron already do. Draft, not yet reviewed.'
status: draft  # found during EXR-8 B4/B5; in scope for the epic, missed in review; to be reviewed outside the EXR-8 session
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md) (EXR-19).
**Found by:** [EXR-8](2026-10-28-exr-08-julia.md) B4, the Julia flip.
**Corrects (all complete or in flight elsewhere; this plan amends them and edits none of them):**
[EXR-1](2026-10-28-exr-01-presenter-extraction.md) and
[EXR-4](2026-10-28-exr-04-record-format.md) (both complete on the integration branch, per Gordon,
2026-11-01), [EXR-6](2026-10-28-exr-06-jupyter-flip.md) (lane A), EXR-8 (lane C),
[EXR-16](2026-10-28-exr-16-positron-interop.md) (in progress in lane A, `workspace-4`).
**Proposed to block:** [EXR-17](2026-10-28-exr-17-legacy-removal.md) and
[EXR-18](2026-10-28-exr-18-docs-e2e.md) (a widget document must render correctly before the
legacy path goes and before the docs promise parity).

This plan is a first write-up, not a researched design. Everything marked **unverified** is
from reading code and plans in the EXR-8 session, not from running a widget document.

## Why this exists

An engine that returns a notebook can have its output presented two ways: by the engine's
own `toMarkdown`, or by core's Rust presenter from the execution record. The two are not
equal for outputs that need page-level dependencies. Julia is the first engine flipped to
the core presenter (EXR-8 B4, `presentedBy: "core"`), and in doing so it lost something the
old path supplied. That gap is not Julia-specific, so it belongs to the epic.

## What is known

1. **What `toMarkdown` produced.** In the Julia engine (`julia-engine.ts`, the `toMarkdown`
   call and what follows it), `toMarkdown` returns more than markdown. It returns
   `dependencies`, which the engine turns into either `includes`
   (`quarto.jupyter.resultIncludes`) or `engineDependencies`
   (`quarto.jupyter.resultEngineDependencies`), plus `preserve` and `pandoc` entries, for
   widget and HTML outputs. These are the scripts and styles a widget needs to run on the
   page.
2. **What the core path drops.** When core presents, the engine skips `toMarkdown`
   (`coreWillPresent`, `record-wire.ts`) and returns `markdown: ""`, so none of the above is
   produced. A widget output renders without its scripts. The flip commit records this as a
   "known loss"; at the time it was left as an open decision.
3. **The Rust presenter has no widget handling.** `presenter/cell.rs:172`: "Jupyter-widget
   payloads remain unhandled (follow-up)". Its MIME priority is `text/html`, then
   markdown or math, then SVG, PNG and JPEG, then `text/plain`. `presenter/inline.rs` names
   the two widget MIME types (`application/vnd.jupyter.widget-state+json` and
   `...widget-view+json`) but only to rank them for inline targets.
4. **The Jupyter engine has the same gap, but it is not a regression.** Lane A
   (`ipynb-execution-lane-a-12`, 2026-11-01, a static read of branch `exr/EXR-6`, nothing run)
   found no dependency derivation anywhere in `engine/jupyter` or `presenter` (no includes,
   requirejs or html-preserve handling). The only widget mentions are the MIME types requested
   from the kernel (`execute.rs:252-258`) and `cell.rs:172`, and the flip's caller forwards
   only `files_written` (`text_execute.rs:679`). So for Jupyter this is new capability, not a
   loss; Julia is the only engine that lost something, because its `toMarkdown` used to supply
   the dependencies. Not rendered: whether a plotly or ipywidgets Jupyter document happens to
   be self-contained HTML today.
5. **The channels exist in core.** `ExecuteResult` carries `includes` (pre-rendered
   fragments), `html_dependencies` (structured `{name, stylesheets, scripts}` manifests that
   `EngineExecutionStage` stores through `store_html_dependencies`), and on the wire
   `preserve` and `engine_dependencies`. The wire `preserve` map is documented as
   "carried-and-ignored until a consumer lands". So the plumbing to receive dependencies is
   partly there; what is missing is something that produces them on the core path.

## How it was missed in review

EXR-8 B3 (the parity suite) listed widgets among "known candidates to check, not asserted".
Its status note then recorded widgets as an open gap, not a measured divergence. B4's gate
was "zero unaccepted divergences", and a gap that was never asserted counted as zero. The
flip therefore passed its gate with this unmeasured. The fixtures were also eight hand-built
notebooks with no widget output, so nothing could have shown it.

## What exists (as built, read from `workspace-6`, 2026-11-01)

This plan corrects completed work, so it starts from the code:
- **EXR-4, record.** `EngineRecord` (`record/types.rs`) already folds the engine result per
  engine: `includes`, `filters`, `preserve`, `needs_postprocess`, `html_dependencies`,
  `pandoc`, `metadata`, `resource_files`, `warnings`, plus a flattened `extra`. So the
  merged per-engine form exists. `RecordCell.outputs` is `Vec<Value>` (raw nbformat JSON), so
  a key on an output's metadata needs no struct change.
- **EXR-4, Positron import** (`record/positron_import.rs`). It rewrites every rich output
  to `display_data`, drops `quarto_webview_metadata`, keeps every other metadata key and the
  whole `data` MIME map. So an imported self-contained widget keeps its HTML today.
  Nothing exports to Positron yet.
- **EXR-1, presenter.** `present_document` returns `PresentResult { markdown, files_written,
  notes }` (`presenter/document.rs`). There is no dependency field, and
  `present_from_record` (`engine/ts_record.rs`) copies only `markdown`, the written files
  and notes onto the `ExecuteResult`. That is where the dependencies stop.
- **Widget payloads** are unhandled in `format_outputs` (`presenter/cell.rs`, "follow-up").

## What the knitr epic and Positron already do

Read in the EXR-8 session (2026-11-01); nothing was run.

**The knitr epic** (`2026-10-30-knitr-execution-records-epic.md` and KNR-1 to KNR-7, all
`draft`, on `workspace-3`):
- Decision 19 requires the record to carry `knit_meta` (the HTML dependencies of an
  htmlwidget) beside the output text. KNR-1 spike S2 carries it as `jsonlite::serializeJSON`
  output in `metadata.quarto_knitr.knit_meta`. In a four-widget fixture (normal,
  `results='hide'`, `include=FALSE`, `echo=FALSE`) the set rebuilt from the pieces equalled the
  live 24-entry set on knitr 1.44, 1.50 and 1.52. `results='hide'` drops a widget's
  dependencies and `include=FALSE` keeps them.
- The carriage is R-specific (R object serialization, equality tested in R), so it cannot be
  renamed into a neutral form. No Rust code reads `knit_meta` today and no plan says where the
  set is rebuilt at present time; KNR-6 does not say how recorded `knit_meta` returns to the
  result at flip time. That is a gap in their plans, not a conflicting design.
- Under their decision 9 a hidden widget stays in the record with its `knit_meta` and the
  presenter applies `results=hide` at present time, so the rule runs per output.
- KNR-2 plans to reproduce q2's `wrap_asis_output` (caption after the widget, `{=html}`
  fencing, widget wrapper class) in the Rust presenter.
- Library paths: the dependencies are paths into the R library, not bytes. No plan covers
  freeze, replay or hub.
- The knitr session (reply of 2026-11-01) agreed a neutral contract is wanted, saw no reason
  knitr should own it, and knows of no existing neutral shape. Its answers are its reading of
  the plans, not a decision. Its caution: unlike `quarto_markdown_passthrough` (a flag the
  presenter reads), a dependency manifest is data with producers and consumers beyond the
  presenter (the fold, preview, freeze), so the passthrough pattern may not carry over.

**Positron** (`external-sources/positron`, `positronQuarto/`, and `external-sources/ark`;
versions as checked out, not a release):
- Its output cache stores an interactive output as a `display_data` or `execute_result` with
  every MIME type in `data`, and puts `quarto_webview_metadata` (`webviewType`: `widget`,
  `display` or `preload`; the raw MIME payload; optional `resourceRoots`) in that output's own
  metadata. On read it peels that key and keeps every other metadata key. So per-output
  metadata is Positron's own granularity, and extra keys on an output are not dropped.
- Positron has no dependency manifest. A widget renders in a notebook webview from the MIME
  payload, with notebook renderers and RequireJS preloaded by the webview.
- Ark (`html_widgets.R`), in notebook or background mode, renders an R htmlwidget to one
  self-contained `text/html` `display_data`: every JS and CSS dependency is inlined into
  `<script>` and `<style>` blocks behind an AMD guard, with no temp-directory paths and no
  `data:` URIs (asserted in `tests/html_widgets.rs`). A `ark.html_widget.deduplicate` option
  exists and is off by default.
- Consequence for knitr's KNR-5: it says an imported Positron widget renders without
  dependencies because the record has no `knit_meta`. If Positron's R widgets arrive as
  Ark's self-contained HTML, that limit does not apply. **Unverified:** that Positron's
  inline Quarto execution runs Ark in notebook mode.

## Decisions so far (Gordon, 2026-11-01)

- **EXR-19 owns the engine-neutral dependency contract.** The knitr epic's decision 19
  becomes one adapter into it.
- **Per-output placement (leaning, Gordon: "I think that's right").** A dependency manifest
  hangs on the output that needs it, so the visibility rule runs per output (the knitr rule,
  and Julia `expand` cells whose expansions carry their own options). EXR-4's per-engine
  folded result stays as the merged form.
- **The `include: false` quirk is a knitr-only artifact (Gordon, 2026-11-01).** The neutral
  presenter rule is: a hidden output brings no dependencies. knitr's behavior (a
  hidden-but-included widget keeps its dependencies) is not copied to other engines. Because
  the knitr adapter must still match Quarto 1, it owns the quirk: it can keep a hidden
  output's dependencies by how it marks them in the record, not by a presenter special case.
  How the adapter expresses that is the knitr epic's to design (KNR-3).

## Proposed solution (not researched)

Four layers, cheapest first:

1. **A warning now.** When the core path presents a record that contains a widget or
   dependency-bearing HTML output, emit a diagnostic naming the cell: its dependencies are
   not loaded. A silent loss is the worst outcome, and this is small. It can land before the
   rest.
2. **Presenter policy, engine-neutral.** `present_document` applies one rule: dependencies
   follow the outputs it shows, and a hidden output brings none (the `include: false` case,
   which knitr keeps, needs a decision for the other engines). It also owns the raw-HTML
   wrapper (caption after the widget, `{=html}` fence, wrapper class) as one code path shared
   with KNR-2's `wrap_asis_output` reproduction and the Jupyter presenter's `text/html`
   handling. The presenter returns the merged, deduplicated dependency set, through the
   structured `html_dependencies` channel (not `includes`). Its result type needs a
   dependency list and `present_from_record` puts it on the `ExecuteResult`.
3. **A neutral per-output manifest, with per-engine adapters.** The carriage is the structured
   `{name, stylesheets, scripts}` manifest that `quarto.htmlDependency()` and
   `ExecuteResult.html_dependencies` already use, in the metadata of the output that needs it
   (`display_data` and `execute_result` have metadata; `stream` and `error` do not and never
   carry dependencies). Duplicates (the same 8 plotly entries per widget) are deduplicated in
   the fold. Adapters: knitr converts `knit_meta`; Jupyter and Julia either derive the set
   from the MIME payload in core (then nothing needs storing) or the engine declares it.
   Engine-private data needed for exact round trip (knitr's serialized R objects) stays an
   opaque field beside the neutral one.
4. **A self-contained form, supported alongside the manifest (decided, Gordon, 2026-11-01).**
   The widget's `text/html` carries its own scripts and styles (Ark's form). Positron reads
   only the MIME bundle, so this is the form that renders there, and it also removes the
   paths-versus-bytes problem for freeze, replay and hub. Costs: size, and the loss of
   page-level dedupe. The rule, with the least machinery:
   - The record stores what the producer made: a manifest, self-contained HTML, or both.
     Nothing is converted on the way in.
   - The presenter handles both. An output with a manifest adds its dependencies to the
     page's deduplicated set; a self-contained output is emitted as is and adds none. Both
     reach the document as an `{=html}` block.
   - Export to Positron converts a manifest to self-contained HTML. It must run where the
     dependency files are readable (the manifest holds paths), at record write or export,
     not at replay. Import already keeps self-contained HTML (see above).
   - Open: how to avoid conflicts between inlined scripts and an AMD loader already on the
     page (Ark's AMD guard exists for this), and the doubled test matrix per widget family.

Why not the shortcut of always running the engine's `toMarkdown` and discarding its
markdown: it writes the engine's figure files next to core's differently named ones, leaving
orphans in the output, and it brings back the cost the flip removed. Whether `quarto.jupyter`
exposes a dependencies-only function is **unverified**.

## Open questions for review

- Does a Jupyter plotly or ipywidgets document render today because its HTML happens to be
  self-contained? (Answered at the code level: Jupyter never derived dependencies. A render
  would tell us how much it matters in practice.)
- Where does knitr rebuild the dependency set at present time (R or Rust)? Nothing in KNR-1
  to KNR-7 decides it.
- How do dependency files travel (paths versus bytes, `quarto_files`)? It affects freeze,
  replay, hub and Positron export, so it may be an epic-level question and not only this
  plan's.
- Inline form: is it part of this plan or EXR-16, and does it replace the manifest for
  Positron-bound records or sit beside it?
- Does Positron's inline Quarto execution use Ark's notebook mode (self-contained HTML)? If
  so, KNR-5's "imported widgets lose their dependencies" limit is overstated.
- What do QNR-produced Julia widget outputs contain: do their `text/html` payloads inline or
  CDN-load their scripts, and does the engine need to declare anything?
- The `include: false` quirk: decided, knitr-only (see Decisions). Open: how the knitr
  adapter expresses it in the record.
- Which widget families matter: ipywidgets, Plotly, Makie WebGL, Bokeh, htmlwidgets? Each
  derives dependencies differently.
- Is the Positron-compatible record subset (EXR-4, EXR-16) allowed to carry the manifest
  metadata at all?

## Corrections to completed or in-flight plans

Each is additive; none reopens a decision.
- **EXR-1 (presenter; complete).** `PresentResult` gains a dependency list, and
  `present_document` applies the visibility rule and the manifest-or-self-contained rule per
  output. Widget payloads in `format_outputs` stop being a follow-up. The raw-HTML wrapper
  becomes one shared path.
- **EXR-4 (record; complete).** Document one per-output metadata key for the manifest, and
  that a self-contained widget is a plain MIME bundle with no extra metadata. No struct
  change. `EngineRecord.html_dependencies` stays the merged form. Open: whether the
  Positron-compatible subset (`freeze_readiness.rs`, EXR-16) tolerates the key.
- **EXR-6 (Jupyter flip, lane A) and EXR-8 (Julia, lane C).** Both flips call
  `present_document`; both must forward the new dependency list onto the `ExecuteResult`
  (`present_from_record` today does not). This is the fix for the "known loss" EXR-8 B4
  records.
- **EXR-16 (Positron interop; lane A, in progress).** Export converts manifest to
  self-contained HTML. Lane A is adding the spike as S7 (needs the GUI, so it waits for
  Gordon) and checking `freeze_readiness.rs` for the extra metadata key. The spike: does Positron's inline Quarto execution run Ark in
  notebook mode, and does a q2-exported widget render there?
- **Knitr epic (workspace-3).** The knitr adapter converts `knit_meta` into the neutral
  manifest and keeps the serialized R objects as an opaque engine-private field. KNR-5's
  imported-widget limit needs rechecking against Ark's self-contained form. Their call.

## Tests, when it is scoped

- Fixtures with real saved widget outputs (the EXR-8 B3 fixtures have none), including a
  QNR-produced Plots or Makie notebook.
- A parity row against the engine's `toMarkdown` result for the same notebook, comparing the
  dependencies reported, not only the markdown.
- An end-to-end row in `julia_engine_e2e.rs` with a widget cell, asserting the scripts are
  on the rendered page.

## Checklist (to be refined in review)

- [x] Check whether native Jupyter widget documents lose dependencies after EXR-6: no, it never derived any (lane A, static read); the work for Jupyter is new capability.
- [ ] Agree the contract with the knitr epic's owner (per-output manifest, adapters, where
  knitr rebuilds the set) and with Gordon (paths versus bytes, inline form).
- [ ] Warning diagnostic on the core path for a widget or dependency-bearing output.
- [ ] Neutral per-output manifest in the record (schema in EXR-4), tests first.
- [ ] Presenter policy and dependency channel out of `present_document`, tests first.
- [ ] Adapters: Julia and Jupyter (derive or declare), knitr (`knit_meta`).
- [ ] Self-contained form: presenter rule, manifest-to-self-contained conversion for export (with EXR-16, lane A), AMD guard.
- [ ] Send the EXR-16 requests to lane A, and keep EXR-1 and EXR-4 corrections in this plan.
- [ ] Widget fixtures, a parity row and an end-to-end row.
- [ ] Update EXR-8 (known loss), KNR-5 (imported widget limit) and the epic tables when
  scoped.
