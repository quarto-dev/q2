---
title: 'OPR-7i: page dependencies for Jupyter-style outputs on the recorded route (the Quarto 1 scan, declared by the engine)'
date: 2026-11-10
description: 'The plain route writes requirejs, jQuery and hoisted library scripts into the page head for any notebook with an HTML or JavaScript output; the recorded route writes none, so a Julia page that needs them breaks once OPR-7d makes the recorded route the only one. Restores the plain route behavior narrowly: an engine declares what kind of dependency data its outputs carry, core runs a port of the Quarto 1 scan for the one kind that exists (jupyter-outputs), delivers the result as includes that are never stored, skips hoisted library outputs in the body, and warns for widget views it cannot satisfy. The Julia engine (Gordon''s fork) declares the kind. No widget state, no AMD-only-when-needed, no per-output manifest: those belong to the widget epic.'
status: complete
depends-on: [opr-7h]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Evidence:** [OPR-7h research note](../research/2026-11-09-opr-7h-plots-differences.md) (cause A),
[OPR-7c parity report](../research/2026-11-09-opr-7c-parity-report.md).
**Implements early, narrowly:** phase 1 of [WGT-2](2026-11-02-wgt-02-dependency-contract.md) (the warning, then derive in
core), inside the [widget epic](2026-11-02-widget-epic.md)\'s option A. This plan edits no widget-epic file.
**Branch:** `opr/7i-jupyter-page-dependencies` · **Lane D** (`workspace-6`, once 7h has landed and that workspace is
free; the branch may sit in another lane workspace until then) · **Blocks:** OPR-7d.
**Needs:** Node 24; Julia 1.12 with `Plots` for the end-to-end rows (they skip when absent; say so if they did).
**Before you start:** run `pwd`, `git status` and `git branch --show-current`. If another agent is using the workspace (a
branch other than yours, uncommitted work), stop and tell Gordon. Before claiming, read the integration branch for plans
in flight that edit `ts_protocol.rs`, `record/types.rs`, the presenter or the Julia subtree; if one is `in-progress`,
wait for it.

## Branch and land

```bash
cd <the lane workspace where this branch is checked out> && pwd
git switch -c opr/7i-jupyter-page-dependencies feature/one-presenter   # already exists if the plan commit is on it
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7i-jupyter-page-dependencies
```

## Why this plan exists

OPR-7h found why three Julia documents differ between the routes (the Plots scripts, cause A). The plain route runs
`widgetDependencies(nb)` from `jupyterToMarkdown` (`ts-packages/quarto-api/src/jupyter/to-markdown.ts`, `isHtml` only): a
scan of the whole notebook that (1) turns on requirejs, jQuery and the `define('jquery')` shim when any output is
`text/html` or `application/javascript`, (2) turns on the ipywidgets loader when an output is a `widget-view+json`, and
(3) hoists library `script` outputs (today: Plotly) out of the cells into the head. The Julia engine skips `toMarkdown`
when core presents (`julia-engine.ts`, the `coreWillPresent` branch), and `present_from_record` (`engine/ts_record.rs`)
copies only the markdown, the written files and the notes onto the result. Nothing on the recorded route replaces the
scan (read). So a Julia page whose HTML needs requirejs loses it, and OPR-7d would make that the only route.

The goal is parity with the plain route and no more: "as good as before", for the cases the plain route handled without
comm capture.

## Decisions (Gordon, 2026-11-10; not to be reopened by this plan)

1. **Narrow and Quarto 1 compatible.** Follow the Quarto 1 scan (`widgets.ts`) unless there is a good reason not to. The
   over-broad requirejs trigger is kept; narrowing it is WGT-1\'s detection work.
2. **The engine declares the data kind.** A record carries a label saying what dependency data its engine\'s outputs hold.
   Core runs the strategy the label names. An engine with no label gets no handling, so unknown and third-party engines
   are safe, and knitr and marimo (which have their own paths) never opt in.
3. **The label is stored in the record**, on the engine\'s entry, so replay and freeze present without the engine running.
   It is **not** part of the hash definition: changing it must not invalidate a cache.
4. **The Julia engine declares it**, as a second change to Gordon\'s fork (`gordonwoodhull/quarto-julia-engine`, branch
   `q2/node-port`), made in the subtree as ordinary commits the way OPR-3 did. No host-side table of engine names.
5. **A warning for what cannot work**: a widget-view output when no widget state exists (always, today).
6. **Native Jupyter does not set the label.** Native Jupyter never derived page dependencies, so setting it would be new
   behavior (CDN requirejs, a global jQuery and the shim on every page with a `text/html` output), not a restore. Native
   Jupyter and the ipynb replay path get the widget warning only, and OPR-7d carries that as a known, documented
   difference from the plain route. Julia is the only engine that sets the label.
7. **Derived text is derived at present time on every route, never stored, and deduplicated across engines.** The
   splice route (preview, hub, supplied records, `q2 convert`) and the render route get identical head text and an
   identical body. Derived includes never reach `entry.includes`. When several engine records contribute, an identical
   block is emitted once.
8. **The label is set only together with the presentation acknowledgement** (Gordon, after the implementability review).
   `EngineEntry` records no presenter, so core cannot tell a record whose engine presented itself (its `toMarkdown`
   includes are already stored in the entry) from one core presents. Tying the label to `presentedBy: "core"` makes
   "label present" mean "core presents, so derive".

## What the code looks like today (read in the reviews; re-check by symbol)

- `present_document` (`presenter/document.rs`) takes all record cells, the engine entries and the format, calls
  `match_cells`, then `present_units`, and returns a `PresentResult` (markdown, files written, notes, ranged units). It has
  one `PresentResult` construction site. Its callers: `engine/ts_record.rs` (`present_from_record`), `engine/replay.rs`,
  `engine/ipynb_replay.rs`, `engine/skeleton.rs` (knitr), `engine/knitr/adapter.rs`, `engine/jupyter/text_execute.rs`
  (native Jupyter; passes `&[]` for the engines), `presenter/jupyter_option_sweep.rs`, and tests.
- `present_units` (same file) takes a `PresentInput` and the matched `(unit, cell)` pairs and returns `Vec<UnitReport>`
  (outcome, image failures, attribute rejections). It sees only matched cells. `present_document` turns reports into
  `PresentNote` values (an enum: `InlineNoUsableValue`, `EngineAttrsRejected`, `ImageUnusable`, `MarkdownFormatMismatch`);
  `engine/diagnostics.rs::note_diagnostic` turns a note into a diagnostic.
- The splice (`stage/stages/record_splice.rs`) calls `present_units` directly, never `present_document`. It matches
  cells itself, drops metadata cells, uses only `UnitOutcome::Replace` edits, and ignores notes. Its stored-entry folds
  are `fold_includes` and `fold_warnings` in `splice_record_with`, outside `splice_units` (whose reconcile drops anything
  folded into the meta earlier). Stored warnings reach the splice only through `entry.warnings`.
- `present_from_record` puts the presenter notes on `result.warnings` and the stage (`engine_execution.rs`) then
  serializes `result` into the trace capture, calls `fold_side_data` (which copies `result.includes` and
  `result.warnings` into the entry) and only after that drains `result.warnings` and `result.includes` into the context.
  `ExecuteResult.record` is `#[serde(skip)]` so it stays out of the capture; a new field needs the same.
- `ReplayEngine::present_record` (`engine/replay.rs`) starts from the stored `ExecuteResult`, replaces the markdown and
  extends `result.warnings` with the notes of a fresh `present_document`. Whether the stored warnings already hold
  the same note (a duplicate on replay) is **not yet established**; a test settles it (see the warning).
- `format_outputs_with` (`presenter/cell.rs`) reads `options.output_kinds()` by position; filtering outputs without the
  kinds and anchors shifts them. `select_by_kind` already remaps all three together.
- `EngineEntryInput` derives `Default` and every literal uses a `..` spread; `EngineEntry {` has two literals
  (`record/builder.rs` production, `record/types.rs` test); `TsRecordInfo` has no `Default` and seven literals
  (`ts_record.rs` and `ts_protocol.rs` tests). `wasm-quarto-hub-client` constructs none of them.
- The wire is guarded: `ts-packages/quarto-engine-host-node/src/wire-parity.test.ts` holds an exhaustive
  `TS_RECORD_INFO_KEYS` (a `keyof TsRecordInfo` type check) and compares key sets with
  `crates/quarto-core/tests/fixtures/ts_wire_parity.json`, which `ts_protocol::tests::test_ts_wire_parity_fixture`
  regenerates.

## Design

### The label

Working names: wire `dependencyData`, Rust `dependency_data: Option<String>`; the one value is `"jupyter-outputs"`
("the cell outputs are nbformat outputs, and page dependencies are derived from their MIME types"). Rename freely before
the wire ships, but keep one name across the engine, `TsRecordInfo` and the record.

- Wire: optional on `TsRecordInfo` (`engine/ts_protocol.rs`), absent from an older engine (serde default), additive like
  `presentedBy` (OPR-3). Move the wire-parity guard with it: the key in
  `ts-packages/quarto-engine-host-node/src/types.ts` and `TS_RECORD_INFO_KEYS`, and regenerate the JSON fixture. The seven
  `TsRecordInfo` literals gain the field. `types.ts` is type-only and `dist/` is untracked, so no engine-host bundle
  changes; verify at gate time.
- Record: optional on `EngineEntry`, set from the record info in `present_from_record` (after the builder\'s `finish()`),
  `skip_serializing_if` when `None`, outside `HashDefinition`. No format-version bump: `known_options` is the precedent (an
  optional `EngineEntry` field that stayed at version 1). Bump only if a round-trip test of an old record fails; say so
  in the notes.
- The label is a hint that selects a core-side strategy, so core may later detect a strategy from the cells instead.

### One pure function, two callers

A new module `presenter/page_dependencies.rs` holds one pure function:

`page_dependencies(cells: &[RecordCell], engines: &[EngineEntry], format: &str) -> PageDependencies`

`PageDependencies` carries `header_includes: Vec<String>` (the derived head text, already deduplicated within the call)
and the set of hoisted outputs (cell index, output index). It is computed **once per record from all of its cells** (stale
and metadata cells count, as in Quarto 1), never from the matched subset. Callers:

- Render: `present_document` calls it and puts the result in `PresentInput` (a new field; grep `PresentInput {` for its
  literals) and `header_includes` on `PresentResult` (a new field; one construction site).
- Splice: `splice_record_with` calls it with `record.cells`, the record\'s engines and the format, puts it in
  `PresentInput`, and folds `header_includes` into `meta.rendered.includes` with `append_pandoc_includes`, right after
  `fold_includes`, after the stored entries.

Which cells belong to which engine: a cell\'s `quarto_engine`; a cell with none belongs to the only engine entry if the
record has exactly one, and to none if it has several (so it is never scanned or hoisted; its library output stays in
the body; say so in the notes). Only cells of an entry carrying the label are scanned. An entry that carries no label
contributes nothing, so knitr, marimo, an older Julia engine and native Jupyter (no entry) are unchanged.

### Delivery, and why nothing is stored

- Render: `present_from_record` sets a new `#[serde(skip)]` field on `ExecuteResult` (working name
  `derived_header_includes`) from `PresentResult.header_includes`. It does **not** touch `result.includes`, which the trace
  capture serializes and `fold_side_data` copies into `entry.includes`. The stage drains the new field into
  `ctx.includes.header_includes` after `fold_side_data` (the drain block at the end of the engine loop), through one
  `push_unique` helper that skips a string already present.
- Replay: `ReplayEngine::present_record` keeps the stored includes untouched and recomputes the new field from
  `present_document`, so a replayed record gives one copy.
- The splice does the same through `splice_record_with` (above). `ipynb_replay.rs`, `skeleton.rs`, `knitr/adapter.rs` and
  `text_execute.rs` carry no label and ignore the field.
- Dedupe is `push_unique` on the **derived strings only**, from the stage drain and the splice fold. Stored and
  engine-authored includes are never deduplicated, so marimo and knitr output is unchanged by construction. Only Julia is
  labeled today, so the cross-engine case is insurance; one test with two labeled records covers it.
- A record whose engine presented itself carries no label (decision 8), so nothing derived is added to the includes its
  `toMarkdown` stored. State this rule in a comment where the label is read.

### The strategy `jupyter-outputs` (inside `page_dependencies`)

A port of `widgets.ts`:

1. Whole-record scan, as Quarto 1: any `application/javascript` or `text/html` output sets the JavaScript-widgets flag;
   any `application/vnd.jupyter.widget-view+json` sets the Jupyter-widgets flag. Hidden outputs still count.
2. Hoisted libraries: an output whose `text/html` is a widget include (`isWidgetIncludeHtml`, today `isPlotlyLibrary`)
   goes into the head, and the presenter does **not** emit it in the body (Quarto 1 removes it from the cell in place; the
   recorded route renders from the unmodified record). Port each of these as its own oracle fixture: the
   common-leading-whitespace removal (`line.replace(" ".repeat(n), "")` removes the first match anywhere in the line;
   whitespace-only lines are blanked; lines split on `/\r\n?|\n/`); dedupe of identical library texts; stripping only on
   `display_data` and `execute_result`; `text/html` as a string or an array joined with `""`; and that the scan sets the
   JavaScript-widgets flag **before** the strip, so a Plotly library output also turns on requirejs and jQuery. The
   `isPlotlyLibrary` regexes move to the Rust `regex` crate. JS `.` excludes `\r` and the two Unicode line separators
   while Rust `.` excludes only `\n`: add an oracle fixture whose text puts `\r` where `hasModuleScript`\'s pattern has a
   `.`, and make the Rust regex give the TypeScript result.
3. The includes text: requirejs 2.3.6, jQuery 3.5.1, the `define('jquery')` shim, then the libraries, then the
   html-manager fragment when the Jupyter-widgets flag is set. Port the literal text and the ordering from
   `widgetDependencyIncludes`; do not paraphrase it. The head is one string, `head.join("\n") + "\n"`
   (`widgetTempFile`). The html-manager script keeps Quarto 1\'s unpinned `@*`.
4. Only for HTML-compatible formats and not ipynb output (Quarto 1: `toHtml && !toIpynb`; the Julia engine computes
   `toHtml` as `quarto.format.isHtmlCompatible(format)`; the definition is in
   `external-sources/quarto-cli/src/config/format.ts`). `InlineTarget::from_format` cannot see `prefer-html`: do not port
   it; record in the notes that a `prefer-html` markdown document differs. Test revealjs, a non-HTML format and ipynb.
5. **No widget state.** Records carry no notebook `metadata.widgets`, so the state-script and `</script>` escape variant
   of `widgetDependencyIncludes` is **not ported** (WGT-3, with comm capture). The Jupyter-widgets flag still selects the
   html-manager fragment, as in Quarto 1.

### The body skip

`present_units` hands the cell step a cell with the hoisted outputs removed. The filter drops the output **and its entry
in `output_kinds` and its anchor together**, as `select_by_kind` does, never mid-loop, because `format_outputs_with`
indexes by position. Cover the expanded path as well (`split_expansions` and `render_expanded_unit` in `expansion.rs`
build their own `CellResult`). A cell left with no outputs follows the existing empty-output rules; Quarto 1 does the
same, because `widgetDependencies` empties `cell.outputs` before `jupyterToMarkdown` runs, and it hoists regardless of
`output: false`. An entry with no label never skips.

### The warning

A widget-view output (`widget-view+json` MIME) is reported once per cell, **independent of the label**, so native Jupyter
and the ipynb replay path get it too (decision 6).

- `present_units` sets a flag on the cell\'s `UnitReport` (a new field, like `image_failures`) when an output of a matched
  cell has that MIME. `present_document` turns it into a new `PresentNote` variant (working name
  `WidgetViewUnsupported { unit }`), and `note_diagnostic` turns the note into a diagnostic. Add the `Q-*` code to
  `quarto-error-catalog/error_catalog.json`, its `docs/errors/<subsystem>/<code>.qmd` page and its `docs/_quarto.yml`
  sidebar entry in the same commit (lint rules `error-docs-page-missing` and `error-docs-sidebar-unlisted`). Use the next
  free number in the section where `ImageUnusable` (`Q-18-11`) lives; no `Q-?-?` may remain.
- **Splice route: no warning is computed there.** This is the existing design for every presenter note (the splice
  ignores notes and keeps the source). A record that was live-run reaches the splice with the warning already stored in
  `entry.warnings`, which `fold_warnings` re-raises; a supplied record without it shows none. Say so in the notes.
- **Replay rule.** `ReplayEngine::present_record` extends the stored warnings with a fresh presentation\'s notes, so the
  same warning may be raised twice. First write the test (render, then replay, and expect one warning). If it fails for
  that reason, raise the note on replay only when no equal diagnostic is in the stored warnings (compare by rendered
  text). If existing notes (for example `ImageUnusable`) show the same duplication, say so in the notes and fix only the
  new warning here. If the test passes without a fix, the premise was wrong: say so and add nothing.
- Message draft:

```text
Warning [Q-?-?]: This cell shows an interactive widget (ipywidgets) that q2 can't render yet
  = The page shows the widget's text representation instead. The widget's state is not
    captured, so it can't be drawn. Other output in this document is unaffected.
```

The text-representation claim comes from the presenter\'s MIME priority (it ends at `text/plain`); confirm it by rendering
a widget output, or say you did not. Knitr outputs carry this MIME only if a record says so; test Jupyter and the
record fixture only.

## The change to the Julia engine fork (Gordon\'s repository)

Made in the q2 tree, in the subtree `resources/extension-subtrees/julia-engine`, as ordinary commits (OPR-3 precedent):
not a subtree merge, and **no push**. Read the subtree\'s `CLAUDE.md`, `AGENTS.md` and `README.md` and
`resources/extension-subtrees/README.md` first.

- `src/record-wire.ts`: `RecordInfo` gains the optional field; `buildRecordResult` sets `"jupyter-outputs"` **only when it
  also sets `presentedBy: "core"`** (decision 8), with a comment stating the rule.
- `tests/unit/record-wire.test.ts` first: the label is present when `presentedBy: "core"` is acknowledged; absent when
  core sent no units; absent when the engine did not acknowledge. The existing test that `buildRecordResult(..., undefined)`
  is `{}` must keep passing.
- Then `npm run typecheck`, `npm test` and `npm run build` in the subtree; commit the rebuilt bundle
  `_extensions/julia-engine/julia-engine.js` (the fork\'s CI diffs it against the source).
- A `CHANGELOG.md` entry under `## Unreleased` in the subtree. `resources/vendored.toml` carries the subtree version; the
  OPR-3 precedent updated it for local commits, so follow it and run `cargo xtask lint`.
- Do not touch `to-markdown.ts` or the `engine-host-node` bundle. If a gate says a bundle is stale, say so instead of
  rebuilding blindly.
- **Gordon\'s companion step** (the plan ends before it): push these commits to `gordonwoodhull/quarto-julia-engine` branch
  `q2/node-port`, and refresh with `cargo xtask pull-extension-subtree julia-engine`. Until then q2 works from the local
  subtree edit; an older engine without the label gets no handling.

## Explicitly not in this plan

ipywidgets state (comm capture is WGT-3), a loader that is "only when needed", a per-output manifest, the knitr adapter,
marimo, and any change to `@quarto/api` or `@quarto/types`. `quarto.jupyter.widgetDependencies` does not exist in
Quarto 1\'s facade (`widgetDependencyIncludes`, `resultIncludes` and `resultEngineDependencies` only), so adding it would be
a Q2-only member in the surface the `bd-ixz93hjp` epic is settling before the npm publish. Do the work in Rust, in core.
The structured `html_dependencies` channel (WGT-2 layer 2) is not built; a later plan can move the output without
changing the label. WGT-2\'s manifest and knitr\'s `knit_meta` conversion may become further label values.

## Stop conditions

Stop, commit your progress, write "Notes from an earlier attempt" and tell Gordon (the protocol\'s "When you cannot
finish") if any of these happens:

- the design in "One pure function, two callers" cannot work, or needs a new field on more than `ExecuteResult`,
  `PresentResult`, `PresentInput` and `UnitReport`, a signature change to `present_units` beyond `PresentInput`, or a
  change to `@quarto/api`;
- a test that passed on the integration tip fails after your change, or a test fails that is not one of the three seen in
  7h (reveal assets vs npm, merged bibliography language, orange-book Typst): report it as a finding and do not weaken it;
- a parity row regresses for a reason this plan did not predict (re-run it once, narrowed, before deciding; the
  Julia server has a known transient timeout);
- a gate says the `engine-host-node` bundle is stale.

If you stop early, the handoff says the subtree commits are local and must stay unpushed.

## Checklist (tests first; every box is `[unattended]` unless marked)

- [x] **Read** `widgets.ts` (`widgetDependencies`, `widgetDependencyIncludes`, `isWidgetIncludeHtml`, `isPlotlyLibrary`,
      the includes text), `to-markdown.ts` (`isHtml`, the pre-walk), the Julia engine\'s `coreWillPresent` branch,
      `present_from_record`, `ReplayEngine::present_record`, `fold_side_data` and the stage drain, `present_document`,
      `present_units`, `present_cell_step`, `format_outputs_with`, `splice_record_with`, `fold_includes` and
      `fold_warnings`. Check that "What the code looks like today" is true; write any difference in the notes. Stop per
      "Stop conditions" if the design cannot work.
- [x] **Oracle fixtures.** Run the TypeScript `widgetDependencies` and `widgetDependencyIncludes` over small notebooks (no
      HTML output; a `text/html` data-URI image like Plots; a hand-written `text/html`; a `widget-view+json` with no
      state; a Plotly library output; and the whitespace-strip, duplicate-library, array-valued `text/html` and `\r`
      cases from step 2). Store each input and the includes text it produced as committed fixtures (generate once with
      Node; the Rust tests read the committed files, so CI needs no Node). The Rust port reproduces them exactly.
- [x] **Add stubs, then write the Rust tests and see them fail for the stated reason.** Stubs: `page_dependencies` (empty
      result), the new `PresentInput`, `PresentResult`, `UnitReport` and `ExecuteResult` fields, the new `PresentNote`
      variant. Tests: the scan flags and the hoist for each fixture; the includes text equals the oracle; no label gives
      nothing; a knitr record through the skeleton path gives no includes; a cell with no `quarto_engine` in a
      several-engine record is not scanned; a non-HTML format and ipynb output give nothing; hidden outputs count; a
      hoisted output is not in the presented body and a hoisted output followed by a warning-kind output keeps the right
      kinds; the same head text and body through `present_units` (the splice route, via `splice_record_with`) as through
      `present_document`; two labeled records give one requirejs block; render then replay gives one requirejs block and
      one warning; the warning fires for a widget view in native Jupyter (`&[]` engines) and not for plain `text/html`;
      the derived text is absent from `entry.includes` and from the trace capture after a live run.
- [x] **Label tests** (add the optional fields first, then the failing assertions): `TsRecordInfo` round-trips
      `dependencyData` and an older engine\'s info still parses; `EngineEntry` round-trips it, omits it when `None`, and it
      does not change the document hash or the hash definition; the wire-parity test and fixture agree.
- [x] **Julia engine tests first** in the subtree, then the engine change, typecheck, test, build.
- [x] Implement the label (wire and entry; native Jupyter does not set it), `page_dependencies`, the delivery field and the
      body skip, then the warning and its catalog entry, docs page and sidebar entry.
- [x] **Parity.** Re-run the parity harness for the Julia documents with the command at the top of the 7c report
      (`OPR_PARITY_REPORT=<file> cargo nextest run -p quarto-core -E 'binary(integration) &
      test(opr_parity_runner::opr_parity_report)'`, about 7 minutes), narrowed with `OPR_PARITY_ONLY=<document>`. The
      script lines of `julia/records-plot` must now match; `plot-website` and `positron-cache` must not regress. Take the
      "before" counts from the 7h notes (`claude-notes/research/2026-11-09-opr-7h-plots-differences.md`) and the 7c report.
      Do **not** add or change an allowlist entry; report counts before and after. Also re-run the Julia part of the gate
      and the knitr corpus gate that 7h left unticked, and say so if you could not.
- [x] **End to end** through the binary: rebuild `target/debug/q2` in this workspace (one exists but may be stale). Render
      `julia/records-plot` with the record route on and show the requirejs and jQuery lines in the head and nothing
      duplicated in the body; render a document with no HTML output and show the head has none; render a Julia document
      whose output is a hoisted Plotly library if PlotlyJS is installable, and say if it is not. Record each invocation,
      the lines you saw, and that you inspected the output. Say that you did not do a browser check.
- [x] **Gates:** `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core` (run any
      failure on the integration tip too and say whether it is the same); the Windows cross-clippy for `quarto-core` (it
      may fail to build tree-sitter and wasmtime under `cc` on macOS, as in 7h; say so if it does); `cargo xtask lint`;
      the subtree\'s `npm test`, `npm run typecheck`, `npm run build`; the engine-host package\'s tests (the
      `wire-parity.test.ts` guard) and `ts_protocol::tests::test_ts_wire_parity_fixture`; `cd hub-client && npm run
      build:wasm` (the presenter, `record/` and the splice are touched); `target/debug/q2 render --strict` on every note
      you add or edit.
- [x] Reconcile this checklist, fill in "Notes from the run", set `status: complete`, rebase, re-run the gates, fast-forward
      from workspace-1 (protocol, "Landing a plan"). Report snapshot or golden changes by count and file (expected:
      `ts_wire_parity.json`, the new oracle fixtures, and the parity goldens only; list them).
- [ ] **\[Gordon\]** Push the subtree commits to the fork and refresh the subtree (above). Not an agent step.

## Exit criteria

- A Julia document whose output includes `text/html` renders on the recorded route with the same requirejs, jQuery and
  shim lines in the head as the plain route, produced by the one shared function; a document with no HTML output has none.
- A hoisted library output is in the head and not in the body, and the output kinds of the other outputs are unchanged.
- An engine with no label (knitr, marimo, an older Julia engine) is unchanged; so is a record whose engine presented
  itself.
- The Julia engine declares the label only with the acknowledgement; its unit tests and the rebuilt bundle are committed
  in the subtree, unpushed.
- A widget-view output without state produces the warning once per cell, including on replay and for native Jupyter. The
  splice shows it only through stored warnings (documented).
- The splice route shows the same requirejs lines and the same body as render for a labeled record.
- Derived text is not stored in the record or the trace capture, and a render followed by a replay gives one copy of each
  block.
- Native Jupyter output is unchanged except for the warning, and the notes say OPR-7d inherits that difference.
- `julia/records-plot` has no script-line difference left, and no allowlist entry was added.
- The notes say what was read, what was run, what was not (browser, PlotlyJS, Windows), the cells with no engine in a
  several-engine record and the `prefer-html` difference, and which names Gordon may want to change before the wire ships.

## Notes from the run

Branch `opr/7i-jupyter-page-dependencies` in lane B (`workspace-4`), by Gordon\'s assignment (the plan names lane D).
Marks: (ran) unless noted.

### What changed

- `presenter/page_dependencies.rs` (new): `page_dependencies(cells, engines, format)` ports the Quarto 1 scan; `without_outputs`
  drops hoisted outputs with their `quarto_output_kinds` and `quarto_output_anchors` entries.
- `EngineEntry.dependency_data` and `TsRecordInfo.dependencyData` (wire name `dependencyData`, value `"jupyter-outputs"`),
  `types.ts`, `TS_RECORD_INFO_KEYS` and `ts_wire_parity.json` (+2 lines). No format-version bump; an entry written before the
  field reads back with `None`.
- `PresentInput.page_deps`, `PresentResult.header_includes`, `UnitReport.widget_view`, `PresentNote::WidgetViewUnsupported`,
  `ExecuteResult.derived_header_includes` (`#[serde(skip)]`). No other new field, and `present_units` kept its signature.
- Render: the stage drains the derived text after `fold_side_data`, skipping a block already on the page. Replay recomputes
  it. The splice computes it in `splice_record_with` over every cell of the record and folds it after the stored includes.
- Warning `Q-18-16` (catalog entry, `docs/errors/engine/Q-18-16.qmd`, sidebar entry).
- Julia engine subtree: `src/record-wire.ts` sends the label only with `presentedBy: "core"`; test, CHANGELOG entry and the
  rebuilt `_extensions/julia-engine/julia-engine.js` are **local commits, not pushed** (Gordon\'s step below).

### Differences from the plan\'s wording

- The label is copied onto the entry in `build_record` (`ts_record.rs`), not in `present_from_record`: `build_record` has the
  record info, and it runs only on the acknowledged path. The condition (acknowledgement present) is the one decision 8 asks for.
- The Quarto 1 scan looks only at `display_data` and `execute_result` outputs (`isDisplayData`), for the flags and the strip.
  The plan said "any output". A `stream` or `error` output never sets a flag (fixture `stream-is-not-display`).
- Hoisted outputs are keyed by the cell\'s nbformat `id` and output index.
- "HTML output format" is `InlineTarget::to_html` (html, revealjs, slides, epub, dashboard); `ipynb` is not in it. Quarto 1\'s
  `prefer-html` markdown documents are **not** recognized, so such a document differs from the plain route.
- The Rust regexes spell out JavaScript\'s `.` (no `\n`, `\r`, U+2028, U+2029) and ASCII `\d`. The JavaScript whitespace set
  (including U+FEFF) is used for the strip. `\s` in `hasRequireScript` is Rust\'s Unicode set (differs from JavaScript only for
  U+FEFF and U+0085); not covered by a fixture.
- Cells with no `quarto_engine` in a record with several engine entries are never scanned or hoisted; their library output stays
  in the body. With exactly one entry they belong to it.
- Order of work: the oracle tests and the label tests were seen failing against a stub or a missing assignment first. For the
  presenter, splice, stage and replay wiring I wrote the implementation before the tests, so those tests were not seen failing
  first, with one exception: the replay duplicate-warning test was written first and failed (two warnings) as the plan predicted.

### Replay duplicate warning

The premise held (ran): a live run stores the note\'s warning on the result, and replay added a second copy.
`ReplayEngine::present_record` now skips the `WidgetViewUnsupported` note when an equal diagnostic (by rendered text) is already
stored. Other notes (an unusable image, for one) are still raised again on replay, as before; I fixed only the new one.
The splice computes no note: a live-run record reaches it with the warning in `entry.warnings`, which `fold_warnings` re-raises,
and a supplied record without it shows none (the existing design for every presenter note).

### Evidence

- Oracle fixtures: 19 files under `crates/quarto-core/tests/fixtures/page_dependencies/` (inputs, TypeScript output) and the
  generator `generate.mjs` (Node 24; the Rust tests read the committed files). The Rust port matches all 19.
- `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean. `cargo nextest run -p quarto-core`: 8236 passed,
  36 skipped (the 7h baseline was 8157 passed, 3 failed, 36 skipped on a different tree; the three failures there did not occur
  here, and I did not investigate why).
- Knitr corpus and goldens (`knitr_corpus_record`, `julia_presenter_parity`, `presenter_pinning`, `jupyter_markdown_goldens`):
  14 passed. These are the integration tests I selected as the knitr corpus gate; I did not run the whole-corpus R parity run.
- Engine-host package: 177 passed, including `wire-parity.test.ts`. `ts_protocol::tests::test_ts_wire_parity_fixture`: passes
  after `QUARTO_REGEN_WIRE_FIXTURES=1` (it failed before regenerating, as the guard should). Subtree: `npm run typecheck` clean,
  `npm test` 55 passed, `npm run build` done.
- `cd hub-client && npm run build:wasm`: succeeded.
- `cargo xtask lint`: one violation, `metadata-as-str` at `crates/quarto-core/src/positron_cache.rs:360`, a file this plan does
  not touch. Not fixed here.
- **Windows cross-clippy did not run:** `aws-lc-sys` fails to build under `cc` for `x86_64-pc-windows-msvc` on macOS. A Windows
  user needs `cargo clippy -p quarto-core --all-targets -- -D warnings`.

### Parity (Julia, ran, `OPR_PARITY_ONLY`)

| Document | Before (7c/7h) | After |
|---|---|---|
| `julia/records-plot` | three script lines, plus the 4d split | no script line; the 4d split and code-copy scaffold remain |
| `julia/positron-cache` | three script lines | identical |
| `julia/plot-website` | size difference (fixed in 7h) | identical |
| `julia/docs-index-chunk` | three script lines | allowlisted (`julia-code-leading-newline`) |

Whole `julia/` set: 11 identical, 1 allowlisted, 4 unallowlisted (`capture-facts`, `presentation-attrs`, `records-plot`,
`fig-subcap`), 1 expected both-failed. None of the four involves a script line: they are the OPR-4d per-expression split and the
7j sub-figure group. No allowlist entry was added or changed.

### End to end (ran, binary, output inspected; no browser)

`target/debug/q2 render doc.qmd` in a scratch directory with the default Julia environment\'s `Project.toml` and `Manifest.toml`
copied beside it (so `Plots` resolves). The recorded document is the `julia/records-plot` fixture with `execute: {record: true}`.

- Recorded and plain pages each have exactly one `requirejs@2.3.6`, one `jquery@3.5.1` and one `define('jquery'...)` line, at
  lines 14 to 16 of the page, inside `<head>`; none in the body. The `<script>` lines of the two pages are identical; the
  remaining 7 diff lines are the 4d split and code-copy scaffold.
- A Julia document with `1 + 1` and `println` (no HTML output) has none of the three lines.
- **Not done:** a Plotly library output (PlotlyJS was not checked or installed), so the hoist and body skip are covered by the
  oracle fixtures and the Rust tests only; a browser check; the Windows build.

### For Gordon

- Names to change before the wire ships: `dependencyData` / `dependency_data` / `"jupyter-outputs"` / `derived_header_includes`.
- Push the subtree commits to the fork (`gordonwoodhull/quarto-julia-engine`, `q2/node-port`) and refresh with
  `cargo xtask pull-extension-subtree julia-engine`. Until then an older engine has no label and gets no handling.
- OPR-7d inherits one documented difference: native Jupyter and the ipynb replay path never set the label, so they derive no
  page dependencies and get only the widget warning (decision 6).
- `claude-notes/plans/2026-11-10-opr-7i-...md` frontmatter says `lane: D`; this run used lane B by assignment.

