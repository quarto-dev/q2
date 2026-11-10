---
title: 'OPR-7i: page dependencies for Jupyter-style outputs on the recorded route (the Quarto 1 scan, declared by the engine)'
date: 2026-11-10
description: 'The plain route writes requirejs, jQuery and hoisted library scripts into the page head for any notebook with an HTML or JavaScript output; the recorded route writes none, so a Julia or Jupyter page that needs them breaks once OPR-7d makes the recorded route the only one. Restores the plain route behavior in the narrowest forward-compatible way: an engine declares in its record what kind of dependency data its outputs carry, core runs a port of the Quarto 1 scan for the one kind that exists (jupyter-outputs), carries the result as includes, and warns for widget views it cannot satisfy. The Julia engine (Gordon''s fork) declares the kind. No widget state, no AMD-only-when-needed, no per-output manifest: those are the widget epic''s.'
status: ready
depends-on: [opr-7h]
lane: D
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Evidence:** [OPR-7h research note](../research/2026-11-09-opr-7h-plots-differences.md) (cause A),
[OPR-7c parity report](../research/2026-11-09-opr-7c-parity-report.md).
**Implements early, narrowly:** phase 1 of [WGT-2](2026-11-02-wgt-02-dependency-contract.md) (the warning, then derive in
core), inside the [widget epic](2026-11-02-widget-epic.md)\'s option A. This plan edits no widget-epic file.
**Branch:** `opr/7i-jupyter-page-dependencies` · **Lane D** (`workspace-6`) · **Blocks:** OPR-7d.
**Needs:** Node 24; Julia 1.12 with `Plots` for the end-to-end rows (they skip when absent, say so if they did).
**Before you start:** run `git status` and `git branch --show-current` in the workspace; if another agent is using it
(a branch other than yours, uncommitted work), stop and tell Gordon. Before claiming, read the integration branch for
plans in flight that edit `ts_protocol.rs`, `record/types.rs`, the presenter or the Julia subtree (the epic\'s rule on
file collisions); if one is `in-progress`, wait for it.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-6 && pwd
git switch -c opr/7i-jupyter-page-dependencies feature/one-presenter   # already exists if the plan commit is on it
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7i-jupyter-page-dependencies
```

## Why this plan exists

OPR-7h found why three Julia documents differ between the routes (the Plots scripts, cause A). The plain route runs
`widgetDependencies(nb)` from `jupyterToMarkdown` (`ts-packages/quarto-api/src/jupyter/to-markdown.ts`, `isHtml` only):
a scan of the whole notebook that (1) turns on requirejs, jQuery and the `define('jquery')` shim when any output is
`text/html` or `application/javascript`, (2) turns on the ipywidgets loader and state when an output is a
`widget-view+json`, and (3) hoists library `script` outputs (today: Plotly) out of the cells into the head. The Julia
engine skips `toMarkdown` when core presents (`julia-engine.ts`, the `coreWillPresent` branch), and `present_from_record`
(`engine/ts_record.rs`) copies only the markdown, the written files and the notes onto the result. Nothing on the
recorded route replaces the scan (read). So a Julia or Jupyter page whose HTML needs requirejs loses it, and OPR-7d
would make that the only route.

The goal is parity with the plain route and no more: "as good as before", for the cases the plain route handled without
comm capture.

## Decisions (Gordon, 2026-11-10, in discussion)

1. **Narrow and Quarto 1 compatible.** Follow the Quarto 1 scan (`widgets.ts`) unless there is a good reason not to.
   The over-broad requirejs trigger is kept; narrowing it is WGT-1\'s detection work.
2. **The engine declares the data kind.** A record carries a label that says what dependency data its engine\'s outputs
   hold. Core runs the strategy the label names. An engine with no label gets no handling, so unknown and third-party
   engines are safe, and knitr and marimo (which have their own paths) never opt in.
3. **The label is stored in the record** (on the engine\'s entry), so replay and freeze present without the engine
   running. It is **not** part of the hash definition: changing it must not invalidate a cache.
4. **The Julia engine declares it**, as a second change to Gordon\'s fork (`gordonwoodhull/quarto-julia-engine`, branch
   `q2/node-port`), made in the subtree as ordinary commits the way OPR-3 did. No host-side table of engine names.
5. **A warning for what cannot work**: a widget-view output when no widget state exists (always, today).

## Forward compatibility with the widget epic (what this plan fixes and what it leaves open)

- The label is the seam. The only value this plan defines is `jupyter-outputs`: "the cell outputs are nbformat outputs,
  and page dependencies are derived from their MIME types". WGT-1\'s detection rules can replace what that strategy does
  without touching any caller. WGT-2\'s per-output manifest and knitr\'s `knit_meta` conversion become further values
  (the working names `manifest` and `knit-meta` are only illustrations; this plan reserves nothing).
- The channel is `includes` on the result (the epic\'s option A text: derive dependency text in core and carry it as
  `includes`). WGT-2\'s layer 2 prefers the structured `html_dependencies` channel and a presenter result that carries a
  dependency list; this plan does not build that, and a later plan can move the output without changing the label.
- Derived text is recomputed at present time and never stored in the record (the epic\'s decision). Only the label is
  stored.
- Not decided here: the name of the label beyond the working name below, the merge rule when several engines contribute,
  the AMD-only-when-needed rule, and the per-output form.

**Explicitly not in this plan:** ipywidgets state (comm capture is WGT-3), a loader that is "only when needed", a
per-output manifest, the knitr adapter, marimo, and any change to `@quarto/api` or `@quarto/types`.
`quarto.jupyter.widgetDependencies` does not exist in Quarto 1\'s facade, which has `widgetDependencyIncludes`,
`resultIncludes` and `resultEngineDependencies` only, so adding it would be a Q2-only member in the surface the
`bd-ixz93hjp` epic is trying to settle before the npm publish (read). Do the work in Rust, in core.

## Design

**The label.** Working names: wire `dependencyData` (camelCase, like `presentedBy`), Rust `dependency_data:
Option<String>`; the one value is `"jupyter-outputs"`. Rename freely before the wire ships, but keep one name across the
engine, `TsRecordInfo` and the record.

- Wire: an optional field on the engine\'s returned record info (`TsRecordInfo`, `engine/ts_protocol.rs`), absent from an
  older engine (serde default), additive and versioned like `presentedBy` was (OPR-3).
- Record: an optional field on `EngineEntry` (`record/types.rs`), filled through `EngineEntryInput`
  (`record/builder.rs`), `skip_serializing_if` when `None`, outside `HashDefinition`. Check whether the record format
  version needs a bump or a note (the execution-records epic\'s decision 5 and RIU stamping); say what you found.
- Native Jupyter sets it in code where its `EngineEntry` is built (`engine/jupyter/text_execute.rs`, which calls
  `present_document`; the other callers are `engine/ts_record.rs`, `engine/replay.rs` and `engine/ipynb_replay.rs`, per
  WGT-2, to be re-checked by symbol).
- knitr and marimo set nothing.

**The strategy `jupyter-outputs`,** one shared function that every caller of `present_document` reaches (not only
`present_from_record`), run once per engine record whose entry carries the label. It is a port of `widgets.ts`:

1. Whole-record scan, as Quarto 1: any `application/javascript` or `text/html` output sets the JavaScript-widgets flag;
   any `application/vnd.jupyter.widget-view+json` sets the Jupyter-widgets flag; hidden outputs still count (the epic\'s
   decision: visibility matches Quarto 1 for Jupyter and Julia).
2. Hoisted libraries: an output whose `text/html` is a widget include (`isWidgetIncludeHtml`, today
   `isPlotlyLibrary`) goes into the head, and the presenter does **not** emit it in the body. Quarto 1 removes it from
   the cell in place; the recorded route renders from the unmodified record, so the presenter skips it.
3. The includes text: requirejs 2.3.6, jQuery 3.5.1, the `define('jquery')` shim, and the html-manager and state
   fragments when the Jupyter-widgets flag is set. Port the literal text and the ordering from
   `widgetDependencyIncludes` (`widgets.ts`); do not paraphrase it. Put the result on `ExecuteResult.includes`.
4. Only for HTML-compatible formats and not ipynb output (Quarto 1: `toHtml && !toIpynb`). Take the rule from the
   format the way the plain route does, and test revealjs and a non-HTML format.
5. The widget state (`nb.metadata.widgets`) is read if present, as Quarto 1 does, so a record that already carries it
   (an imported or future one) works; nothing here writes it.

**The warning.** An output with a `widget-view+json` MIME entry in a record whose scan found no widget state: one
diagnostic per cell, naming it, saying the page will show the widget\'s text representation. Add the `Q-*` code to
`quarto-error-catalog/error_catalog.json`, its `docs/errors/<subsystem>/<code>.qmd` page and its `docs/_quarto.yml`
sidebar entry in the same commit (the lint rules `error-docs-page-missing` and `error-docs-sidebar-unlisted`). Draft of
the message:

```text
Warning [Q-?-?]: This cell shows an interactive widget (ipywidgets) that q2 can't render yet
  = The page shows the widget's text representation instead. The widget's state is not
    captured, so it can't be drawn. Other output in this document is unaffected.
```

The text-representation claim is read from the presenter\'s MIME priority (it ends at `text/plain`); confirm it by
rendering a widget output, or say you did not.

## The change to the Julia engine fork (Gordon\'s repository)

Made in the q2 tree, in the subtree `resources/extension-subtrees/julia-engine`, as ordinary commits (OPR-3 precedent):
not a subtree merge, and **no push**.

- `src/record-wire.ts`: `RecordInfo` gains the optional field; `buildRecordResult` sets `"jupyter-outputs"` whenever it
  builds a `recordInfo` (the label describes the data, not who presents it, so it is not tied to `presentedBy`).
- `tests/unit/record-wire.test.ts` first: the label is present in a record info and absent when core sent no units.
- Then `npm run typecheck`, `npm test` and `npm run build` in the subtree; commit the rebuilt bundle
  `_extensions/julia-engine/julia-engine.js` (the fork\'s CI verifies it matches the source).
- A `CHANGELOG.md` entry under `## Unreleased` in the subtree (the fork\'s rule for meaningful changes).
- Read the subtree\'s `CLAUDE.md`, `AGENTS.md` and `README.md` and `resources/extension-subtrees/README.md` first.
- Do not touch `to-markdown.ts` or the `engine-host-node` bundle: nothing in this plan changes them. If a gate says the
  bundle is stale, say so instead of rebuilding blindly.
- **Gordon\'s companion step** (the plan ends before it): push these commits to `gordonwoodhull/quarto-julia-engine`
  branch `q2/node-port`, and refresh with `cargo xtask pull-extension-subtree julia-engine`. Until then q2 works from
  the local subtree edit; an older engine without the label simply gets no handling.

## Checklist (tests first; every box is `[unattended]` unless marked)

- [ ] **Read** `widgets.ts` (`widgetDependencies`, `widgetDependencyIncludes`, `isWidgetIncludeHtml`,
      `isPlotlyLibrary`, the includes text), `to-markdown.ts` (`isHtml`, the pre-walk), the Julia engine\'s
      `coreWillPresent` branch, `present_from_record`, the callers of `present_document`, `ExecuteResult.includes`
      (its type, and how the plain route\'s includes reach the head), `EngineEntry`, `EngineEntryInput` and
      `TsRecordInfo`. Write down in the notes: the includes shape (text or file paths), where each caller gets the
      engine entry, and whether knitr\'s path ever reaches `present_document`. If includes set on the result do **not**
      reach the head on the recorded route, say so and size the extra plumbing before writing code; if it makes this
      more than a small plan, stop and tell Gordon (the protocol\'s "When you cannot finish").
- [ ] **Oracle fixture.** Run the TypeScript `widgetDependencies` and `widgetDependencyIncludes` over a small set of
      notebooks (no HTML output; a `text/html` data-URI image like Plots; a hand-written `text/html`; a
      `widget-view+json` with and without state; a Plotly library output) and store each input notebook and the
      includes text it produced as test fixtures. The Rust port must reproduce them exactly.
- [ ] **Rust unit tests first**: the scan flags and the hoist for each fixture; the includes text equals the oracle;
      the label absent gives nothing; a knitr record gives nothing; a non-HTML format and ipynb output give nothing;
      hidden outputs still count; a hoisted-library output is not in the presented body; the warning fires for a
      widget-view without state and not for plain `text/html`.
- [ ] **Label tests first**: `TsRecordInfo` round-trips `dependencyData` and an old engine\'s info still parses;
      `EngineEntry` round-trips it, omits it when `None`, and it does not change the document hash or the hash
      definition.
- [ ] **Julia engine tests first** in the subtree (see above), then implement the engine change, typecheck, test, build.
- [ ] Implement the label (wire, entry, native Jupyter), then the strategy, the presenter skip and the warning.
- [ ] **Parity.** Re-run the parity harness for the Julia documents, narrowed with `OPR_PARITY_ONLY` (the 7c report has
      the command): the script lines of `julia/records-plot` must now match, and `plot-website` and `positron-cache`
      must not regress (7h fixed their image size). Do **not** add or change an allowlist entry; report the counts
      before and after. Re-run the Julia part of the gate and the knitr corpus gate that 7h left unticked, and say
      so if you could not.
- [ ] **End to end** through the binary: render `julia/records-plot` with `target/debug/q2 render` (build it in this
      workspace first; it is not there today) with the record route on, and show the requirejs and jQuery lines in the
      head and nothing duplicated in the body; render a document with no HTML output and show the head has none; render
      a Julia document whose output is a hoisted Plotly library, if PlotlyJS is installable, and say if it is not.
      Record each invocation, the lines you saw, and that you inspected the output. A browser check is not required;
      say that you did not do one.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core` (three
      failures were seen in 7h on this workspace: reveal assets vs npm, merged bibliography language, orange-book
      Typst; run them on the integration tip too and say whether they are the same); the Windows cross-clippy for
      `quarto-core` (it fails to build tree-sitter and wasmtime under `cc` on macOS, as in 7h; say so if it does);
      `cargo xtask lint`; the subtree\'s `npm test`, `npm run typecheck`, `npm run build`; `cd hub-client && npm run
      build:wasm` (the presenter and `record/` are touched); `target/debug/q2 render --strict` on every note you add
      or edit.
- [ ] Reconcile the plan checklist, fill in "Notes from the run", set `status: complete`, rebase, re-run the gates,
      fast-forward from workspace-1 (protocol, "Landing a plan"). Report snapshot or golden changes by count and file
      (expected: none, or the parity goldens only; list them).
- [ ] **\[Gordon\]** Push the subtree commits to the fork and refresh the subtree (above). Not an agent step.

## Exit criteria

- A Julia document whose output includes `text/html` renders on the recorded route with the same requirejs, jQuery and
  shim lines in the head as the plain route, produced by the one shared function; a document with no HTML output has
  none.
- A hoisted library output is in the head and not in the body.
- An engine with no label (knitr, marimo, an older Julia engine) is unchanged.
- The Julia engine declares the label; its unit tests and the rebuilt bundle are committed in the subtree, unpushed.
- A widget-view output without state produces the warning.
- `julia/records-plot` has no script-line difference left, and no allowlist entry was added.
- The notes say what was read, what was run, what was not (browser, PlotlyJS, Windows), and which names Gordon may
  want to change before the wire ships.

## Notes from the run

(To be filled in by the agent that runs this plan.)
