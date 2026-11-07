---
title: 'EXR-19 design review: widget and HTML-dependency outputs'
date: 2026-11-01
description: 'Review of the draft EXR-19 plan against the as-built code, Quarto 1, Ark, Positron and the knitr epic. Ranks the findings, separates what was verified from what was inferred, proposes plan edits and lists the questions only Gordon can answer.'
---

**Reviewed:** EXR-19 at `d2ff28d2a` on `exr/EXR-8-b`. Since 2026-11-02 that plan is
[WGT-2](../plans/2026-11-02-wgt-02-dependency-contract.md) in the
[widget epic](../plans/2026-11-02-widget-epic.md); this note keeps its original name and the
"EXR-19" wording as a record of the review.
**Method:** two read-only sonnet reviewers (A: code and integration; B: risk, interop,
adversarial), then the reviewer-in-chief re-read the code behind each finding that would
change the plan. Nothing was built or run. Sources are cited as `external-sources/...`.

Status words: **verified** means I read the cited code myself; **reported** means a
sub-agent read it and I did not re-check; **inference** means reasoning from code, not a
reading of it.

## Summary

The plan's diagnosis is right (Julia lost something, Jupyter never had it), but three
premises in its design do not survive contact with the code:

1. Quarto 1 never produced structured dependencies for Jupyter or Julia. It produced
   `includes` text, and q2 receives exactly that. The "neutral manifest" is not what the
   regression needs; it is what knitr needs.
2. Quarto 1 derives Jupyter and Julia dependencies document-wide, before cell visibility.
   "A hidden output brings no dependencies" is therefore a deliberate divergence from Quarto 1
   for Jupyter and Julia too, not only a knitr non-copy.
3. Dependencies reached by path cannot satisfy freeze, replay, hub or WASM. Bytes versus
   paths is the load-bearing decision and the plan defers it.

Also found: an ordering trap that would persist presenter-derived dependencies as if the
engine made them, and a second replay path the plan does not mention.

## Findings, ranked

### 1. The regression can be fixed without a record schema (scope-shaping)

**Verified.** q2 sends `dependencies: true` (`crates/quarto-core/src/engine/ts_engine.rs:566`),
so `julia-engine.ts` takes the `quarto.jupyter.resultIncludes` branch
(`resources/extension-subtrees/julia-engine/src/julia-engine.ts:317-322`), not
`resultEngineDependencies`. The wire field `engine_dependencies` is declared
(`engine/ts_protocol.rs:506`) and never mapped on the Rust side. The result is header and
after-body text in `ExecuteResult.includes`, which `EngineExecutionStage` appends to
`ctx.includes` (`stage/stages/engine_execution.rs:770-777`).

Quarto 1 derives that text from the notebook alone
(`external-sources/quarto-cli/src/core/jupyter/widgets.ts:23-66`). The Julia engine declares
nothing. So for Jupyter and Julia, "derive in core from the MIME bundle" is not an option to
weigh against "the engine declares it": it is how Quarto 1 worked, and it needs no per-output
manifest and no record change, because the dependencies are a pure function of the recorded
outputs.

**Recommendation.** Make derive-in-core the Jupyter and Julia adapter, and say so in the plan.
The per-output manifest is then needed only for producers that cannot be derived from the
bundle (knitr's `knit_meta`) and for export. This lets the regression fix ship before any
schema decision (see the phasing below).

### 2. Visibility: the "knitr-only quirk" premise is wrong for Quarto 1 Jupyter

**Verified.** `extractJupyterWidgetDependencies(nb)` runs at `jupyter.ts:738-741`, after
only the notebook fixups and before any cell is rendered. It sets `jsWidgets` if any output
anywhere has `text/html` or `application/javascript`, and `jupyterWidgets` if any has
`widget-view+json` (`widgets.ts:30-39`). Hidden cells are still in `nb`. So Quarto 1 Jupyter
and Julia keep a hidden output's dependencies too. Gordon's 2026-11-01 decision (hidden
output brings none) diverges from Quarto 1 for these engines.

This is a concrete problem with a decision, so it is flagged, not overridden. It matters
for two reasons beyond parity:

- Plotly through QuartoNotebookRunner is a cross-cell dependency (finding 3). If the
  preamble cell is hidden and the rule is per-output, the plot loses its loader.
- Quarto 1 sets `jsWidgets` for any `text/html` output, including a DataFrame table, so
  requirejs and jquery arrive on most Jupyter pages. A per-output rule gives a different,
  smaller page. That is arguably better, but it is a behavior change the docs should state.

### 3. Plotly in Julia needs hoist-and-delete, and it is cross-cell

**Reported (A), partly checked by me.** `QuartoNotebookWorkerPlotlyBaseExt.jl` in
`external-sources/QuartoNotebookRunner.jl/src/QuartoNotebookWorker/ext/` splits the first
plot into a loader cell (`text/html` from `_requirejs_config()`) and the plot cell with
`include_plotlyjs = "require-loaded"`. Its comment relies on Quarto hoisting the loader into
the head. Quarto 1 does that in `widgets.ts:42-55`: an output matching `isPlotlyLibrary` is
removed from the cell and its text goes to `htmlLibraries`.

A Rust port must (a) recognize the three patterns, (b) delete the matching output from the
presented cell, which is a change to `presenter/cell.rs`, not only a dependency list, and
(c) dedupe by text. Makie (CairoMakie only appears in the runner), WGLMakie, Bonito and the
Plots plotly backend are **unverified**: not found in the runner source.

### 4. The neutral manifest type cannot carry what exists

**Verified.** `HtmlDependency` is `name`, `version`, `stylesheets: Vec<PathBuf>`,
`scripts: Vec<PathBuf>` (`crates/pampa/src/lua/quarto_doc.rs:27-38`), and
`store_html_dependencies` reads each file at store time (`crates/quarto-core/src/dependency.rs`).
It has no inline text, no CDN `href` with `integrity`, no head snippet, no script
attributes. **Reported (B):** the store flattens to `libs/name/basename`, so two scripts with
one basename collide and sibling assets (fonts, WASM) are never copied.

Quarto 1\'s Jupyter output needs exactly the things this type cannot hold: CDN tags with
integrity hashes (requirejs 2.3.6, jquery 3.5.1), a `define('jquery', ...)` shim, the
html-manager `embed-amd.js` tag and the widget-state script (**reported (A)**, `widgets.ts`).
ipywidgets state is notebook-level (`nb.metadata.widgets`, `widgets.ts:~100-118`), so a
per-output key cannot hold it. Whether the record keeps `nb.metadata.widgets` is
**unverified**; `RecordMetadata.extra` (`record/types.rs:60`) could, and no code was found
that does.

**Recommendation.** Two channels, named in the plan: `includes` text for derive-in-core
engines (as Quarto 1), and a file or bytes manifest for producers like knitr. Do not call
`html_dependencies` "the" neutral manifest until it is extended, and add the extension (head,
href, attributes, subdirectory layout) and a losslessness test (`knit_meta` to manifest,
leaflet or plotly fixture) to the checklist.

### 5. Bytes versus paths is the epic-level decision

**Verified for q2, reported for Quarto 1.** The manifest holds `PathBuf`s into an R or Julia
library; a record replayed on another machine fails at `file_read`. **Reported (B):** the
store only warns on a failed read, so the loss is silent. Quarto 1 freeze persists bytes:
`freezeExecuteResult` reads each include file into the freeze JSON and the `_files` directory
(with the libs) is copied into `_freeze`
(`external-sources/quarto-cli/src/command/render/freeze.ts:20-87`, `:190+`). The `version`
field on `HtmlDependency` exists for freeze already.

WASM and hub have no filesystem. **Reported (B):** EXR-13 and EXR-14 mention no dependencies.
Whatever the answer, dependency files must be resolved to bytes at record-write time, while
the library is readable, and the bytes must be keyed by content hash. If bytes, the manifest
becomes a blob table, which is a larger change than a metadata key.

### 6. Ordering trap: derived dependencies would be recorded as engine-made

**Verified.** In `EngineExecutionStage`, `fold_side_data` runs at `engine_execution.rs:751`,
before the drains of `includes` (`:770`) and `html_dependencies` (`:794`). It copies the
result\'s `includes` and `html_dependencies` into `EngineEntry`
(`fold_side_data`, `:1004`). `present_from_record` runs inside the engine\'s `execute`, so any
dependency it sets is already on `result` when the fold runs. The record would then say the
engine produced them, contradicting "the record stores what the producer made".

On replay, `replay.rs:137-155` starts from `result = stored` (which carries those recorded
includes) and then presents again, so a re-derivation would double them (**inference** for the
doubling; the ordering is verified).

**Recommendation.** Derive after the fold, or tag derived entries so the fold skips them.
Test it through the stage, not only through `present_from_record`.

### 7. Both forms: keep the decision, define the corner

The two-forms decision is mostly consistent with a smaller design (**inference**, B): treat
self-contained `text/html` as ordinary HTML that the presenter emits and attributes no
dependencies to. That is already what "emitted as is" says. The corner the plan leaves open
is an output that carries both. Proposal: the manifest is authoritative, the HTML is emitted
as is, and the presenter does not try to recognize an inlined copy. Document that a producer
that lies is undetectable and the result is a double load, not corruption.

**Verified (Ark).** `embed_tags` builds a whole `<!DOCTYPE html><html>` document
(`external-sources/ark/crates/ark/src/modules/positron/html_widgets.R:53-84`), with a
per-snippet AMD guard (`:101-102`). Emitting that into a Quarto page as an `{=html}` block
nests documents. The plan does not distinguish "Ark-shaped document" from "page fragment".
Export to Positron may want the document shape; the presenter needs a fragment. Whether the
guard breaks a page-level AMD loader is **unverified** (B reported it as a possible blocker; I
did not confirm).

Accepted costs (**reported**, B): a 3.5 MB plotly bundle per widget (dedupe needs manifest
knowledge, which favors keeping the manifest); the record changes when the library version
changes; SRI and richer script attributes are dropped by Ark.

### 8. Positron

**Reported (B), cache behavior.** Positron\'s cache filters data-explorer MIME types, stores
`quarto_webview_metadata` in output metadata and peels only that key on read, and evicts by
total size (`external-sources/positron/.../quartoOutputCacheService.ts`). So an extra metadata
key survives, a manifest-only widget shows `text/plain` there, and a large inlined bundle
raises the eviction risk. `record/positron_import.rs` keeps other keys (**verified** by A
at `:189-198`).

**Update, 2026-11-02:** S7 has since run ([the findings](2026-11-02-exr-16-s7-widget-outputs-findings.md)).
Ark produced one self-contained `text/html` for plotly (3.87 MB) and leaflet (460 KB) and they
rendered again from the cache, so the self-contained form works in Positron for `htmlwidget`
objects (a plain `HTML()` string arrives as `text/plain`). An extra per-output key survives the q2
record code and import, but a Positron rewrite of a record drops q2 keys, so only the MIME bundle
survives a Positron round trip. Still untested: a hand-seeded q2 record, Python widgets, other
packages, offline rendering, and whether the document shape versus a page fragment renders.

### 9. Per-output placement: sound for Julia, incomplete for knitr and inline

**Inference (B), consistent with the code A read.** Julia `expand` cells are fine, since each
expansion is its own output. Gaps: an inline span has no `display_data` to hang a key on;
knitr level-3 cache hits have no pieces, so there is no output to carry the set; knitr\'s own
hidden-but-included handling implies a slot outside the visible output. Per-output stays the
primary placement, with an allowed cell-level slot for outputs that cannot carry one, and an
explicit statement for inline results.

### 10. Record impact of a metadata key is nil

**Verified by A, spot-checked.** Execution hash covers source, options and metadata only
(`record/exec_hash.rs:339-356`); the content hash covers source (`ids.rs:26`); outputs are
not hashed. The writer sorts keys and keeps unknown ones (`writer.rs:28`). `freeze_readiness.rs`
constrains only `quarto_format` (`:216-238`), so an unknown key passes; add a test.
The EXR-14 splice reads `quarto_format` (`place.rs:116`) and does not see the key; its real
exposure is dependency paths, as in finding 5.

## Corrections to the plan text

Made in EXR-19 itself (commit `405c3646d`):

- "MIME types requested from the kernel (`execute.rs:252-258`)" was wrong: that range maps
  received media types to MIME names.
- "Both flips call `present_document`" was incomplete: there are four non-test callers,
  including the two replay paths (`engine/replay.rs`, `engine/ipynb_replay.rs`).
- `html_dependencies` was described as `{name, stylesheets, scripts}`; it also has `version`
  and is path-only. The `toMarkdown` description now records the `resultIncludes` branch,
  the unmapped `engine_dependencies` and the always-empty `preserve`.

Verified and left alone: `EngineRecord` fields, `RecordCell.outputs` as raw values,
`PresentResult` without a dependency field, `present_from_record` copying only markdown,
files and notes, the `cell.rs:172` follow-up comment, `coreWillPresent`, and what the
Positron import keeps. Not rechecked: the knitr and Positron claims in the plan beyond what
B read.

## Proposals for the plan (not applied; for Gordon)

**Phasing.** One plan, three phases, each shippable on its own:

1. **Warn and derive.** The warning (below), then a pure `derive_dependencies` over the recorded
   outputs that ports the Quarto 1 rules (what to reproduce is below), emitted as `includes`
   after the fold. Fixes the Julia regression and adds Jupyter capability. No schema change.
2. **Record and bytes.** The bytes-versus-paths decision, the manifest schema, the
   `HtmlDependency` extension, the knitr adapter, freeze and replay behavior, hub and WASM.
3. **Export.** Manifest to self-contained fragment or document, with EXR-16 and the S7 result.

**What to port from Quarto 1 (reported by A, from `widgets.ts`, `jupyter.ts:738-741`):**
the requirejs and jquery tags with their integrity hashes and the `define('jquery')` shim; the
`embed-amd.js` tag when a widget-view output exists; the widget-state script with the
`</script>` escape variant (quarto-cli issue 8433); `isPlotlyLibrary` hoisting with deletion;
the HTML-targets-only gate. Can be left out: `removeAndPreserveHtml`, because
`isPreservedHtml` is constant `false` (`preserve.ts:58-60`, **verified**). That also makes the
wire `preserve` and `postProcess` loss in the flip a non-loss for Jupyter and Julia.

**First slice, the warning.** A `PresentNote` variant in `presenter/document.rs` (A:
`WidgetDependenciesNotLoaded { unit }`), raised for a `widget-view+json` output or for
`text/html` or `application/javascript` matching a Plotly or require pattern (not every
`text/html`; most are tables), and turned into a diagnostic where the other notes are
(`jupyter::note_diagnostic`). It needs an error-catalog code and a `docs/errors` page in the
same commit (AGENTS.md lint rules). Tests first as unit tests in `presenter/document.rs` and
`presenter/cell.rs`, then a new file under `crates/quarto-core/tests/integration/`.

**Test order after that:** golden strings for `derive_dependencies` (all three Plotly
patterns, deletion, dedupe, widget state); the four `present_document` callers and the stage
ordering from finding 6; the visibility rule once decided; real QuartoNotebookRunner fixtures
with a parity row against Quarto 1 `toMarkdown`; an end-to-end row in `julia_engine_e2e.rs`;
export last.

**Checklist additions:** resolve dependency files to bytes at record-write time;
`HtmlDependency` extension and losslessness test; WASM and hub behavior; freeze and replay
parity with Quarto 1; a cell-level slot for cache hits; inline spans; page fragment versus
document shape; cross-engine dedupe (the store\'s first-wins guard only warns on conflict);
the replay paths; `q2 render` end-to-end for a Jupyter widget document, not only Julia.

**Lanes.** EXR-17 and EXR-18 blocking is right. EXR-6 (lane A) and EXR-8 (lane C) both
depend on phase 1, so phase 1 should not wait for the schema work.

## Where I overrode or narrowed a sub-agent

- **B, "cut the both-forms case":** narrowed to a proposal. It contradicts Gordon\'s decision
  that the presenter handles both, so I kept the decision and proposed only a rule for the
  corner case (manifest authoritative).
- **B, AMD guard as a blocker:** downgraded to unverified. The nested-document problem is
  real and verified; whether the guard clashes with a page loader was not shown.
- **A, "channel should be `includes`":** accepted for derive-in-core engines only, with
  `html_dependencies` kept for file-backed producers. B\'s point that the type is too weak
  applies to both channels; I kept both findings.
- **A, `execute.rs` finding and B, Quarto 1 freeze bytes:** accepted. I confirmed the first;
  the freeze claim is B\'s reading and was not re-read by me.

## Questions for Gordon

1. **Visibility.** Match Quarto 1 for Jupyter and Julia (document-wide, hidden outputs still
   count), or keep "a hidden output brings no dependencies" as a deliberate divergence? The
   knitr-only premise does not hold for Quarto 1 Jupyter.
2. **Bytes or paths.** Do dependency bytes travel in the record, or in a content-addressed side
   store? Paths alone cannot satisfy freeze, hub or WASM. Is Quarto 1 freeze parity required
   for widgets?
3. **Phasing.** Is it acceptable to ship phase 1 (warn, then derive in core as `includes`) before
   the manifest and bytes decision, so the Julia regression closes first?
4. **Channels.** Two channels (`includes` text for derived engines, a manifest for file-backed
   producers), or extend `HtmlDependency` until it can express Quarto 1\'s output?
5. **Both forms.** If an output carries both a manifest and self-contained HTML, is the manifest
   authoritative?
6. **ipywidgets.** Widget state is notebook-level, so it does not fit a per-output key. Is
   ipywidgets in scope for the first release, or only Plotly-style HTML widgets?

## Decisions and proposals after discussion (2026-11-02)

Gordon answered some of the questions above in conversation. Recorded here so the plan can cite
them; the questions that remain open are listed last.

**Decided**

- **Freeze (question 2):** no Quarto 1 freeze loading is planned; a future q2 freeze would
  probably be based on execution records, and a frozen record must render a widget document
  without the original R or Julia library.
- **Question 1 (visibility):** match Quarto 1 for Jupyter and Julia. Document-wide, hidden
  outputs still count. This reverses the 2026-11-01 "hidden output brings none" rule for those
  engines; the knitr adapter keeps its own handling.
- **Question 3 (phasing):** agreed. Phase 1 (warn, then derive in core as `includes`) ships
  before the record and bytes work.
- **Question 4 (channels):** option B (one engine-neutral contract) is the goal, **only if the
  spikes teach enough to make it good**. Until then phase 1 uses option A, with a seam so B can
  replace it. The spikes and the gate are in
  [WGT-1](../plans/2026-11-02-wgt-01-widget-spikes.md).

- **Question 6 (ipywidgets):** in scope, but not in this plan. Gordon wants ipywidgets designed and
  ported so the general system is proven correct; it is too much for the Julia regression fix, so
  it moved to [WGT-3](../plans/2026-11-02-wgt-03-ipywidgets.md) in the new widget epic, together
  with the rest of this work (the old EXR-19 is now WGT-2).

**Liked, as a proposal (not yet in EXR-19)**

- **Bytes in `quarto_files`.** The record already has a notebook-level `quarto_files` map
  (`record/types.rs:49`, golden fixture `crates/quarto-core/tests/fixtures/records/golden.ipynb`):
  doc-relative path to base64 `data` and `size`. Dependency files, for producers that have them
  (knitr\'s `knit_meta`), would be read while the library is readable at record-write time and
  stored under content-hash keys such as `deps/<hash>/<basename>`; the per-output manifest would
  name them instead of holding machine paths. No schema bump. Costs: record size (a plotly bundle
  is about 3.5 MB per the risk reviewer, plus roughly a third for base64), a need to diagnose an
  unreadable library at write time, and a bytes variant of `store_html_dependencies`.
  `quarto_files_ref` stays the later sidecar escape hatch (which would be a version bump).
  Unchecked: who writes `quarto_files` for engines other than knitr, and whether `coalesce.rs`
  and `overlay.rs` would drop or duplicate `deps/` entries.
- **Derive-in-core stores nothing.** The Jupyter and Julia dependencies are CDN tags and inline
  text, recomputed from the recorded outputs each time.

**Facts established in discussion (verified by reading unless marked)**

- Positron\'s cache stores widget payloads and, optionally, `resourceRoots` (machine-local
  directory paths, `positronQuarto/common/quartoExecutionTypes.ts:93-104`). It stores no
  dependency files, so it gives no precedent for bytes. Not traced: how the webview uses the roots.
- marimo is a different strategy again: no ipywidgets, one compiled app per document,
  `marimo-quarto-island` custom elements, adapter injected once via `include-in-header`
  (`external-sources/quarto-marimo/src/engine/projection.ts`). It meets core-presented engines at
  the stage\'s append-only merge, which is why a cross-engine collision is plausible (Gordon has
  seen one between marimo and Julia).
- ipywidgets 7.0.0 is from 2017-08-18 and 7.8.5 (the last 7.x) from 2024-10-22; 8.0.0 is from
  2022-08-18 (PyPI).

**Still open for Gordon**

- Question 5: when an output carries both a manifest and self-contained HTML, is the manifest
  authoritative?
- Which lane runs the widget epic, and do EXR-17 and EXR-18 wait on WGT-2 phase 1 (see the widget
  epic).
