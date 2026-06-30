# Plan 3: @quarto/api/jupyter

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plan 2A (the `@quarto/api` package skeleton). Phases 3A-3D and 3F otherwise independent, **except** `assets()` (Phase 3D) consumes the `PlatformHost.fs.walk` seam op that **Plan 1b** owns (interface member + `denoHost` impl, in lockstep) — **already landed** on the integration line (see *Platform dependencies*). Phase 3E (wiring into engine-host) requires Plan 1b to have created the `@quarto/engine-host-deno` package.
**Blocks:** Plan 4 (Julia Validation)
**Estimated sessions:** 2-3

> **Reconciled against the usage model (2026-06-29).** This plan was reviewed
> field-by-field against the Julia consumer (`julia-engine.ts`) and q2's
> already-vendored contract (`ts-packages/quarto-types/src/jupyter.ts`). See
> `claude-notes/research/2026-06-29-plan3-vs-usage-model-reconciled.md`. The
> headline change: **implement against the vendored `@quarto/types` Jupyter
> contract, not a hand-drafted redraft.** The earlier draft redrafted its own
> narrower option/result types; that redraft broke the Julia validation target
> at runtime in four places (`cellOutputs`, `assets` casing,
> `executeOptions`/`figPos` drop) and is now redundant because the real types
> are vendored. Sections below have been updated; the Phase 3B/3C mechanism
> descriptions were also corrected against Q1 source.

## Overview

Populate the `jupyter/` subpath of `@quarto/api` — a clean implementation of
Jupyter notebook → markdown conversion and related utilities. This is the
`quarto.jupyter` namespace of the QuartoAPI.

This plan implements **7 methods** of the namespace: the 6 the Julia engine
calls on its execute path, plus `widgetDependencyIncludes` — the producer the
deferred-dependencies protocol (RTQ FC-2's `Dependencies` verb) requires. The
core function
`toMarkdown()` is the single most complex piece of the entire engine
extension project (~1300 lines of logic), but it's conceptually
straightforward: walk notebook cells, format outputs as markdown, handle
figures and HTML preservation.

**Reference:** The Quarto 1 implementations to study are in
`external-sources/quarto-cli/src/core/jupyter/` (the in-repo symlink to
quarto-dev/quarto-cli — reference/parity only, never an import). Key files:
`jupyter.ts` (main toMarkdown), `display-data.ts`, `tags.ts`, `labels.ts`,
`preserve.ts`, `widgets.ts`, `types.ts`.

## Package location

All files under `ts-packages/quarto-api/src/jupyter/`. No separate
`package.json` — `jupyter/` is a subpath of the single `@quarto/api`
package created in Plan 2A. Consumers import via
`@quarto/api/jupyter`.

## Platform dependencies

**Six of the seven methods touch the filesystem** and therefore take a
`PlatformHost` (the `@quarto/api/platform` interface from Plan 2A §2aa). Only
`resultEngineDependencies` is pure. The earlier draft mislabeled `assets` and
`resultIncludes` as pure — both do real FS I/O in Q1 (verified, P3-2/P3-3) — and
`widgetDependencyIncludes` is host-touching for the same reason (verified
below):

| Function | Needs host for |
|---|---|
| `toMarkdown(nb, opts)` | Writing figure image files (base64 decode → `host.fs.writeFileSync`) |
| `isPercentScript(file, exts)` | Reading the file to check for percent markers |
| `percentScriptToMarkdown(file)` | Reading the source file |
| `assets(input, to)` | `host.fs.ensureDir(figures_dir)` + `host.fs.walk(...)` to promote the supporting dir (Q1 `jupyter.ts:665-696` does `ensureDirSync` + `walkSync`) — creates the dir `toMarkdown`'s figures are written into |
| `resultIncludes(tempDir, deps)` | Materializes widget includes to disk via `host.fs.makeTempFile` + `host.fs.writeFileSync` (Q1 `widgets.ts:148-154` uses `Deno.makeTempFileSync`/`writeTextFileSync`) — Julia's **inline execute-path** widget materializer (`julia:256`) |
| `widgetDependencyIncludes(deps, tempDir)` | Same temp-file machinery — Q1 `includesForJupyterWidgetDependencies` (`widgets.ts:73`) routes through `widgetTempFile` (`widgets.ts:148-152`: `makeTempFileSync` + `writeTextFileSync`). The **deferred-deps-path** sibling of `resultIncludes` (see the 7th-method note below) |

> **Seam-name mapping (don't grep for `*Sync`).** The Q1 references above use
> Deno's `*Sync` names; the landed `PlatformHost.fs`
> (`ts-packages/quarto-api/src/platform/index.ts:56-71`) uses bare,
> all-synchronous names: `readTextFileSync`, `writeFileSync(string |
> Uint8Array)`, `exists`, `ensureDir`, `makeTempDir`, `makeTempFile`, `remove`.
> Map Q1 → seam: `readTextFileSync`→`readTextFileSync`,
> `writeTextFileSync`/`writeFileSync`→`writeFileSync`,
> `ensureDirSync`→`ensureDir`, `makeTempFileSync`→`makeTempFile`.
>
> **The `fs.walk` seam op (`assets()` needs it).** `assets()` needs recursive
> directory enumeration to promote the supporting dir. This is **upstream work,
> not Plan 3's**: **Plan 1b owns the whole addition** — it adds the
> `PlatformHost.fs.walk` member to `@quarto/api` *and* implements `denoHost.walk`
> in lockstep (so `denoHost` typechecks as `PlatformHost` the moment 1b lands;
> nothing for Plan 2 to add). **This has already landed** on the integration
> line. Plan 3's `assets` is a pure *consumer* of `host.fs.walk(...)`. Shape:
> `walk(root, opts?) => Array<{ path; isFile; isDirectory }>`. Unit-test `assets`
> against a mock host that stubs `walk`.

The one remaining pure method (`resultEngineDependencies`) plus all the
internal supporting modules (`display-data`, `tags`, `labels`, `preserve`,
`widgets`, `pandoc-id`, `cell-options`) are pure.

Public shape: `src/jupyter/index.ts` exports a single factory
`createJupyter(host: PlatformHost)` that returns the full namespace. The host
is bound once and threaded into every FS-touching method (including `assets`,
`resultIncludes`, and `widgetDependencyIncludes`). This matches the
`createPath` / `createSystem` / `createMappedStringFromFile` pattern in
Plan 2: one entry point per subpath, consistent wiring in
`@quarto/engine-host-deno`.

## What the namespace methods do

The first 6 are Julia's **execute-path** calls; the 7th
(`widgetDependencyIncludes`) is the **deferred-deps-path** producer (RTQ FC-2).

| Method | What it does | Complexity |
|--------|-------------|------------|
| `toMarkdown(nb, opts)` | Convert Jupyter notebook → markdown string (**async** — returns `Promise`) | **High** — the core function |
| `isPercentScript(file, exts)` | Check if file is a percent-format script | Low (host) — ext check + read file + match a **language-comment `%%` + `[markdown]`/`[raw]` marker** (not a bare `# %%`; see P3-14) |
| `percentScriptToMarkdown(file)` | Convert percent script → markdown | Low–Med (host) — reads file; **couples to the to-markdown module** (imports `mdRawOutput`/`mdFormatOutput`), not a self-contained regex pass |
| `assets(input, to)` | Compute + **create** asset directories for figures | Low (host) — path computation **plus** `ensureDirSync` + `walkSync` |
| `resultIncludes(tempDir, deps)` | Extract pandoc includes from widget deps — **inline execute path** (Julia calls it when `options.dependencies` is true, `julia:256`) | Low (host) — **writes widget temp files to disk** |
| `resultEngineDependencies(deps)` | Extract engine-specific deps | Low — pass-through (pure) |
| `widgetDependencyIncludes(deps, tempDir)` | Produce `PandocIncludes` from widget deps for the **deferred-deps path** (RTQ FC-2 `Dependencies` verb). **The only producer of that wire** — a stub here makes the whole deferred-deps protocol inert (see note below) | Low (host) — same temp-file writes as `resultIncludes` |

> **The 7th method — `widgetDependencyIncludes` (real body, not a stub).** RTQ
> FC-2 built the entire deferred-dependencies wire — the `Dependencies` verb,
> `TsDependenciesOptions`, `engineDependencies`, and
> `DependenciesResult.includes` — and gated its real-engine usefulness on Plan 3
> shipping `quarto.jupyter.widgetDependencyIncludes`. **If Plan 3 stubs it, that
> protocol path is permanently dead even after the full epic** — a designed-in
> verb with no producer. So it is implemented for real here, host-bound like the
> others.
>
> **Why the 2026-06-29 reconciliation under-counted it (6 → 7).** That pass
> audited Julia's *execute-path* call-sites and found 6. `widgetDependencyIncludes`
> sits on Julia's `dependencies()` path, not the execute path, so the
> field-by-field execute-call audit missed it. (Julia's own `dependencies()` hook
> is currently a TODO pass-through stub at `julia:146`; the live consumers today
> are the jupyter built-in's dependency path — `jupyter.ts:2161` calls
> `includesForJupyterWidgetDependencies` — and the harness's `Dependencies`-verb
> handler, which the book/project renderer drives.)
>
> **No end-to-end caller yet — by design.** The deferred round-trip
> (`dependencies: false` → `Dependencies` verb at merged output) is not exercised
> until the book/project renderer (Plan 1c defers that consumer), and no v1
> engine sends `dependencies: false`. That is *why* RTQ pre-built the wire;
> a stub is what would make it dead. Implement the producer now; unit-test in
> isolation with a mock host; the book feature lights it up later.

## Work Items

### Phase 3A: Types and foundation

**Type-strategy decision (2026-06-29): adopt the vendored `@quarto/types`
Jupyter contract at every boundary. Do NOT redraft notebook/option/result
types.** q2 has already vendored the real Q1 contract at
`ts-packages/quarto-types/src/jupyter.ts` (`JupyterNotebook`, `JupyterCell`,
`JupyterOutput`, `JupyterToMarkdownOptions`, `JupyterToMarkdownResult`,
`JupyterCellOutput`, `JupyterNotebookAssetPaths`, `JupyterWidgetDependencies`,
…). Those types are *already* flattened to loose index-signature shapes
(`FormatExecute = { [key]: unknown }`, etc.), so they deliver the plan's
original "no 30-dep type explosion" benefit **and** are the actual contract the
`quarto.jupyter` namespace (`quarto-api.ts`) and the Julia consumer rely on.
`@quarto/types` is itself vendored-from-Q1/shared (not q2-specific), so
importing it satisfies the portability constraint ("no q2-specific imports").

- [ ] Confirm `@quarto/api` package skeleton from Plan 2A is in place. If
  Plan 2A hasn't landed, create the minimal package scaffolding first
  (`package.json`, `tsconfig.json`, `exports` map including `./jupyter`).

- [ ] Add `@quarto/types` as a dependency of `@quarto/api` (if not already
  pulled in by Plan 2A). All `jupyter/` modules import their public types from
  there — `import type { JupyterNotebook, JupyterToMarkdownOptions,
  JupyterToMarkdownResult, JupyterCellOutput, JupyterNotebookAssetPaths,
  JupyterWidgetDependencies } from "@quarto/types"`. **Do not create a
  `src/jupyter/types.ts` that re-declares these.**

- [ ] *(Optional, implementer's discretion.)* The vendored `JupyterOutput` is
  loose (`output_type: string; [key: string]: unknown`). For ergonomics inside
  the output-formatting switch in `to-markdown.ts`, an **internal-only**
  discriminated union (`stream | display_data | execute_result | error`) may be
  declared and the loose `outputs` array narrowed to it once at the top of the
  cell walk. This type never crosses the namespace boundary — parameters and
  return values stay vendored. (Skip it and narrow inline if preferred; it
  changes no contract.)

- [ ] Create `src/jupyter/constants.ts` — MIME type constants, cell option
  keys, etc. Reference: Quarto 1's `external-sources/quarto-cli/src/config/constants.ts` (just the
  subset we need). Include `kQuartoMimeType` (injected into widget `<script>`
  tags, see P3-10) and the language-comment-char table `kLangCommentChars`
  (needed by percent-script detection, see P3-14).

### Phase 3B: Supporting modules

Small, focused modules that `toMarkdown` depends on. Each is self-contained.

- [ ] Create `src/jupyter/display-data.ts` — MIME bundle dispatch:
  - `displayDataMimeType(output, options)` — select best MIME type from bundle
  - `displayDataIsImage(output)`, `displayDataIsTextPlain(output)`, etc.
  - **MIME priority is computed DYNAMICALLY from the target format — NOT a fixed
    list** (corrected, P3-9, `display-data.ts:45-97`):
    - Base order is `[text/markdown, image/svg+xml, image/png, image/jpeg]` —
      `text/markdown` is the **highest base** (the earlier draft inverted this,
      ranking `text/html` first).
    - The html/widget cluster — `application/vnd.jupyter.widget-state+json`,
      `application/vnd.jupyter.widget-view+json`, `application/javascript`,
      `text/html` — is spliced in **conditionally** on `toHtml`/`toMarkdown`.
      (The earlier draft omitted all three widget/javascript MIME types →
      widgets never render.)
    - `text/latex` is added **only** for `toLatex`.
    - An html-table special case force-adds `text/html`.
  - `displayDataIsJson(output)` matches **only the widget MIME types**
    (`display-data.ts:176-179`) and emits a `<script type=…>` tag with
    `kQuartoMimeType` injected first (falling back to a json code block only for
    `toIpynb`). There is **no generic `application/json → code block` path**
    (corrected, P3-10).
  - `displayDataLatexIsMath(output)` (`display-data.ts:108-137`) decides whether
    `text/latex` routes into the markdown slot as math, **else** it emits a
    `{=tex}` raw block. `text/latex` is not unconditionally math (corrected,
    P3-10).
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/display-data.ts`
  - ~150 lines

- [ ] Create `src/jupyter/tags.ts` — cell visibility logic:
  - `hideCell(options)`, `hideCode(options)`, `hideOutput(options)`, `hideWarnings(options)`
  - `includeCell(cell, options)`, `includeCode(cell, options)`, `includeOutput(cell, options)`
  - **Also `echoFenced` (drives `echo: fenced`, `tags.ts:68-75`) and
    `includeWarnings`** — the earlier draft's hide*/include* list omitted both
    (P3-12).
  - Implement the "global `false` + local `true`" warning override logic
    (`tags.ts:39-44,93-101`), not just a flat per-cell read.
  - Based on cell-level `echo`, `include`, `output`, `warning` options
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/tags.ts`
  - ~100 lines

- [ ] Create `src/jupyter/labels.ts` — cell label and caption handling
  (corrected roster, P3-11 — the earlier draft invented `cellLabelClass`, which
  does not exist in Q1, and omitted the three real consumer-needed exports):
  - `cellLabel(cell)` — extract label from cell metadata or options
  - `cellLabelValidator()` — duplicate-label guard (`labels.ts:47-61`)
  - `shouldLabelCellContainer(...)` / `shouldLabelOutputContainer(...)` —
    crossref div wrapping (`labels.ts:63-134`)
  - `resolveCaptions(cell)` — extract **`fig-cap` / `fig-subcap` only**.
    `tbl-cap` is handled by a downstream lua filter, **not here** (the earlier
    draft's "extract fig-cap, tbl-cap, etc." was wrong).
  - Id normalization uses **`asHtmlId` (`core/html.ts`)**, not
    `pandocAutoIdentifier`.
  - Remove the invented `cellLabelClass`.
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/labels.ts`
  - ~100 lines

- [ ] Create `src/jupyter/preserve.ts` — HTML preservation (corrected
  signature, P3-13, `preserve.ts:12-42`):
  - `removeAndPreserveHtml(nb: JupyterNotebook) => Record<string, string> |
    undefined` — takes the **whole notebook**, **mutates cell output bundles in
    place** (swaps `text/html` for a markdown placeholder), and returns the
    preserve map (or `undefined`). It is **not** a per-output pure
    `(output) => { output, preserved }` transform as the earlier draft had it.
  - `isPreservedHtml(_html) => false` — **port as the constant-`false` no-op it
    is in Q1 today** (`preserve.ts:58-60`). Q1's producer preserves nothing
    currently, so `htmlPreserve` is always empty and the `preserve`/`postProcess`
    path is inert end-to-end (P3-15). **Do not** describe a live "protect/restore"
    mechanism, and do not make `isPreservedHtml` return `true` without a restorer.
  - **The restore half is out of scope here.** `postProcessRestorePreservedHtml`
    lives in `quarto.text` and is deferred (RTQ F2/B2); under the No-DOM rule it
    is re-expressed as an AST transform reading this `preserve` map, not a DOM
    postprocessor. If a future change makes `isPreservedHtml` live, the restorer
    must land with it or output ships literal `preserve<uuid>` tokens.
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/preserve.ts`
  - ~80 lines

- [ ] Create `src/jupyter/widgets.ts` — Jupyter widget dependency extraction:
  - `widgetDependencies(nb)` — find widget state in output MIME bundles. In Q1
    (`extractJupyterWidgetDependencies`, `widgets.ts:47-62`) this **mutates
    `cell.outputs` in place** to strip hoisted HTML libraries before the
    cell-walk. The earlier draft's `widgetDependencies(outputs)` dropped the
    strip → plotly/HTML-library `<script>` could double-emit (P3-17). Keep the
    in-place strip.
  - `widgetDependencyIncludes(host, deps, tempDir)` — **a real 7th method, not a
    stub** (see "The 7th method" note under *What the namespace methods do*).
    Ports Q1's `includesForJupyterWidgetDependencies` (`widgets.ts:73`), which
    writes the widget head/after-body fragments to temp files via `widgetTempFile`
    (`widgets.ts:148-152`: `makeTempFileSync` + `writeTextFileSync`) — so it is
    **host-dependent** and takes `host` (verified). Returns the vendored
    `PandocIncludes`, whose keys are **kebab-case with an `include-` prefix**
    (`pandoc.ts:9-12`): `{ "include-in-header"?: string[]; "include-before-body"?:
    string[]; "include-after-body"?: string[] }`.
    - **Port shape adaptation (verified — do NOT copy Q1's keys verbatim):** Q1's
      `includesForJupyterWidgetDependencies` returns a *different* local shape —
      `{ inHeader: string, afterBody: string }` (**camelCase, scalar** strings).
      The port must translate that onto the vendored `PandocIncludes`: rename
      `inHeader`→`"include-in-header"`, `afterBody`→`"include-after-body"`, and
      wrap each scalar temp-file path in an **array** (`string[]`).
    - **Wire contract (verified):** once in vendored `PandocIncludes` form, the
      return type is *identical* to `DependenciesResult.includes: PandocIncludes`
      (`execution.ts:132`) — the field the harness's `Dependencies`-verb handler
      forwards. No further adaptation at the wire boundary; the q2 method's return
      type **is** the wire payload type.
    - **MUST be exported from the `createJupyter` factory** (P3-7) with the host
      bound, so the namespace's `widgetDependencyIncludes` and RTQ's FC-2
      deferred-deps fold reach it via `quarto.jupyter.*`. The earlier draft built
      it inside `widgets.ts` but never exposed it. (Its array-vs-singular *type*
      is D.2 drift owned by Plan 2 Phase B; the **exposure + real body** are this
      plan's.)
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/widgets.ts`
  - ~100 lines

- [ ] Create `src/jupyter/pandoc-id.ts` — identifier generation:
  - `pandocAutoIdentifier(text, asciiOnly)` — generate Pandoc-style IDs from
    heading text. **Note the 2nd boolean arg** (`asciiOnly`) — Q1 calls it with
    two args (`jupyter.ts:1548`); the earlier draft's 1-arg signature dropped it
    (P3-11).
  - Pure string manipulation, no dependencies
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/pandoc/pandoc-id.ts`
  - Note: lives under `jupyter/` for now because jupyter is the only
    consumer. If other consumers emerge, promote to a top-level `pandoc/`
    subpath — cheap move, cheap rename.
  - ~50 lines

- [ ] Create `src/jupyter/cell-options.ts` — simplified cell options parsing:
  - Parse YAML from code cell comments (`#| key: value` lines)
  - Use `yaml` package directly (no schema validation)
  - Extract cell-level execution options
  - **Simplified from Quarto 1**: no schema validation, no tree-sitter
  - ~100 lines

### Phase 3C: Core toMarkdown function

The main conversion function. Takes a `JupyterNotebook` and options, returns a
`JupyterToMarkdownResult` (`cellOutputs` array + `dependencies`/`htmlPreserve`/
`notebookOutputs`/`pandoc`) — **not** a bare markdown string; the caller joins
`cellOutputs.map(o => o.markdown)` (as Julia does, `julia:272`).

- [ ] Create `src/jupyter/to-markdown.ts`. **Use the vendored
  `JupyterToMarkdownOptions` and `JupyterToMarkdownResult` from `@quarto/types`
  — do not redraft them.** The earlier draft's redraft is broken against the
  Julia consumer in four places (P3-1/2/3/4); the vendored types are correct and
  are the namespace contract. Key fields the redraft got wrong or dropped:
  ```typescript
  import type {
      JupyterNotebook, JupyterToMarkdownOptions, JupyterToMarkdownResult,
  } from "@quarto/types";
  // For reference, the vendored shapes (quarto-types/src/jupyter.ts) include:
  //   JupyterToMarkdownOptions: executeOptions, figPos, preserveCellMetadata,
  //     preserveCodeCellYaml, keepHidden?, figFormat?, figDpi?, ...   (P3-4/P3-8)
  //   JupyterToMarkdownResult:
  //     cellOutputs: JupyterCellOutput[]   // NOT string[]            (P3-1)
  //     notebookOutputs?: { prefix?; suffix? }                       (P3-5)
  //     dependencies?, htmlPreserve?, pandoc?
  //   JupyterCellOutput = { id; markdown; metadata; options }
  //   assets: JupyterNotebookAssetPaths   // snake_case base_dir/... (P3-2)

  // toMarkdown is ASYNC — the quarto.jupyter namespace types it
  // Promise<JupyterToMarkdownResult> (quarto-api.ts). The internal impl takes
  // the bound host as its first parameter:
  export function jupyterToMarkdown(
      host: PlatformHost,
      nb: JupyterNotebook,
      options: JupyterToMarkdownOptions,
  ): Promise<JupyterToMarkdownResult>;
  ```
  - `cellOutputs` carries `JupyterCellOutput[]` (`{ id, markdown, metadata,
    options }`), because Julia reads `result.cellOutputs.map(o => o.markdown)`
    (`julia:272`) and uses the per-cell `id`/`options` downstream. A `string[]`
    makes `o.markdown` `undefined` → garbage join (P3-1).
  - `executeOptions` is load-bearing: it drives the book/single-file/minimal
    **fixup-profile** selection (`options.executeOptions.project`,
    `jupyter.ts:719-733`). Julia passes it (`julia:231`); `figPos` too
    (`julia:245`). Both must be present (P3-4).
  - `notebookOutputs?` is read behind `if (result.notebookOutputs)` (`julia:273`)
    — absence degrades gracefully (lost ipynb prefix/suffix), but include it for
    fidelity (P3-5).
  - `pandoc?` **is consumed** — Julia forwards `result.pandoc` to the host
    verbatim (`julia:289`), so it is not optional in practice for the validation
    target. Populate it with the pandoc metadata Q1's `jupyterToMarkdown`
    accumulates (front-matter / cell-promoted `pandoc` options merged across
    cells; see `jupyter.ts`). If a full port is deferred, emit an **empty
    object** rather than omitting the field, and note the gap — do not silently
    leave document-level metadata unpopulated, which would diverge from Q1 on
    any notebook that sets it.

- [ ] **Pre-walk pass (runs BEFORE the cell walk — ordering is load-bearing).**
  Q1 computes `dependencies` and `htmlPreserve` at `jupyter.ts:738-741`, *then*
  starts the `cellOutputs` walk at `:744`. Both **mutate `nb` in place** — most
  importantly `widgetDependencies(nb)` strips hoisted HTML libraries (P3-17). If
  the walk runs first, those libs are already emitted and get double-emitted.
  Run this pass first; see the result-assembly item below for the exact
  assignments. (Test row 10 guards this ordering — the plotly `<script>` must
  appear exactly once.)

- [ ] Implement cell walking logic. **Each iteration produces one
  `JupyterCellOutput` — not a bare string.** The per-cell markdown computed by
  the steps below is the `markdown` field; assemble the rest of the
  `JupyterCellOutput` (`{ id, markdown, metadata, options }`) from the cell:
  `id` from `cellLabel`/`asHtmlId` (labels.ts; falls back to a counter),
  `options` from cell-options.ts, `metadata` from the cell's `metadata`. The
  result's `cellOutputs` is the array of these (P3-1).
  1. Iterate notebook cells
  2. For each markdown cell: `markdown` = source as-is
  3. For each code cell:
     a. Check visibility (echo, include, output options via tags.ts). Honor
        `options.keepHidden`: when set, hidden cells are still emitted (marked
        hidden) rather than dropped — Q1 branches on it in the walk.
     b. Extract cell label (→ `id`) and options (→ `options`)
     c. Emit code fence with language and options
     d. Format each output (see below)
     e. Handle figure outputs (write to disk, emit `![]()` reference)
  4. For each raw cell: `markdown` = source with format marker

- [ ] **Assemble the rest of the result object.** `cellOutputs` (above) is not
  the only field — `JupyterToMarkdownResult` also carries `dependencies?`,
  `notebookOutputs?`, `htmlPreserve?`, and `pandoc?`, and Q1 computes them inside
  `jupyterToMarkdown`. Don't omit them (the earlier checklist only covered the
  cell walk):
  - **Pre-walk (see the pre-walk pass item above):**
    - `dependencies` = **`isHtml ? widgetDependencies(nb) : undefined`** — the
      `isHtml = options.toHtml && !options.toIpynb` gate is load-bearing
      (`jupyter.ts:737-739`). **Consumed by Julia** via `resultIncludes(tempDir,
      result.dependencies)` (`julia:256-258`); omit it and the entire widget path
      is dead. (`widgetDependencies` also strips hoisted HTML libs in place, which
      is *why* it must run before the walk — P3-17.)
    - `htmlPreserve` = **`isHtml ? removeAndPreserveHtml(nb) : undefined`**
      (`jupyter.ts:741`) — same `isHtml` gate, same pre-walk requirement (it
      mutates cell bundles). Inert today because `isPreservedHtml` is
      constant-`false` (P3-15), so this is `{}`/`undefined` in practice; port it
      anyway for parity.
  - **Post-walk (independent of cell ordering):** `notebookOutputs`
    (prefix/suffix) and `pandoc` (accumulated document metadata) — populate per
    the interface notes above (P3-5 / `julia:289`).

- [ ] Implement output formatting:
  - **stream output** (stdout/stderr): emit as text, strip ANSI codes
  - **display_data / execute_result**: dispatch by MIME type (display-data.ts)
    - `text/html` → emit as raw HTML block (the preserve/restore path is inert
      today — `isPreservedHtml` is `false`, P3-15 — so html passes through as-is)
    - `image/png`, `image/jpeg` → decode base64, write to file, emit `![](path)`
    - `image/svg+xml` → write to file, emit `![](path)`
    - `text/plain` → emit as text output
    - `text/latex` → math **only when `displayDataLatexIsMath` holds**; otherwise
      emit a `{=tex}` raw block (corrected, P3-10 — not unconditionally math)
    - `text/markdown` → emit directly
    - widget MIME types (`displayDataIsJson`) → emit a `<script type=…>` tag with
      `kQuartoMimeType` injected first (json code block only for `toIpynb`).
      **There is no generic `application/json → code block` path** (corrected,
      P3-10)
  - **error output**: format traceback, strip ANSI codes

- [ ] Implement figure handling:
  - Write image data to `assets.figures_dir` (snake_case, from the vendored
    `JupyterNotebookAssetPaths` — P3-2) via `host.fs.writeFileSync`
    (base64 decode for PNG/JPEG, write as bytes; SVG written as text). The
    target dir is the one `assets()` creates with `host.fs.ensureDir` — these two
    must agree, which is why `assets` is host-dependent (P3-2).
    The host is captured via the `createJupyter(host)` factory closure —
    `to-markdown.ts` itself takes `host` as a parameter on the internal
    implementation function.
  - Generate filename from cell label or counter
  - Emit markdown image reference with optional caption, width/height
  - Handle `fig-format` option (request specific format from kernel)

- [ ] Implement HTML preservation (faithful port — **inert today**, P3-15):
  - Port `removeAndPreserveHtml(nb)` (gated `isHtml`, see result-assembly above)
    and `isPreservedHtml`. Because `isPreservedHtml` returns constant-`false`,
    nothing is actually protected — `htmlPreserve` ends up empty and html outputs
    pass through unchanged. **Do not** build a live UUID-placeholder/restore
    mechanism; that's deferred (RTQ F2/B2, No-DOM AST-transform restorer).
  - The (empty) map flows out as `result.htmlPreserve`; Julia gates `postProcess`
    on it being non-empty (`julia:292-294`), so today `postProcess` is always
    false — matching Q1 end-to-end.

- [ ] ANSI code handling (deliberate simplification — documented divergence,
  P3-16):
  - Strip ANSI escape codes from text outputs via simple regex replacement.
  - **This matches Q1 exactly for `toLatex`/`toMarkdown`/`toIpynb` targets.** It
    differs **only on HTML output**, where Q1 (`core/ansi-colors.ts`) uses
    `ansi_up` (color → `<span>`) plus a small deno-dom step for an `ansi-bold`
    class. Stripping therefore loses HTML color + the `.ansi-escaped-output` CSS
    hook on HTML output only.
  - Retained as a known gap. A faithful HTML port = `ansi_up` + a regex (or DOM)
    bold-class swap; add later if HTML-output color fidelity is wanted. (Earlier
    notes that "Q1 uses ansi_up, not deno-dom" were inaccurate — Q1 uses both.)

- [ ] Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/jupyter.ts` function `jupyterToMarkdown` (~lines 380-700)

### Phase 3D: Utility functions

The simpler methods that the Julia engine also calls.

- [ ] Create `src/jupyter/percent-script.ts` — host-dependent (reads files):
  - Internal functions take `host: PlatformHost` as first parameter:
    ```typescript
    export function isPercentScript(host: PlatformHost, file: string, exts?: string[]): boolean;
    export function percentScriptToMarkdown(host: PlatformHost, file: string): string;
    ```
  - `isPercentScript` — check extension + read file + match the percent marker.
    **Detection is NOT a bare `# %%`** (corrected, P3-14, `percent.ts:32-45`):
    `isJupyterPercentScript` requires `^\s*${cms}\s*%%+\s+\[(markdown|raw)\]` —
    a **language-specific comment char** (`kLangCommentChars`) followed by a
    `%%` run and a `[markdown]` or `[raw]` marker. A `.jl` with only `# %%`
    *code* markers is **not** detected. This feeds Julia's `claimsFile` →
    `isPercentScript(file, [".jl"])` (`julia:95,164,167`), so getting it wrong
    mis-claims (or fails to claim) Julia percent files.
  - `percentScriptToMarkdown` — read file + convert percent-format to markdown:
    - language-comment `%%+ [markdown]` → markdown cells; `[raw]` → raw cells
    - other `%%`-delimited content → code cells
    - **Not a self-contained ~80-line regex module:** Q1's
      `markdownFromJupyterPercentScript` imports `mdRawOutput`/`mdFormatOutput`
      from `jupyter.ts` (`percent.ts:12`), so percent-script **couples to the
      to-markdown module**. Plan accordingly (shared output-formatting helpers,
      not a duplicate).
  - The public `createJupyter(host)` factory binds `host` so callers see the
    natural 1-arg / 2-arg signatures.
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/percent.ts`
  - ~80-120 lines

- [ ] Create `src/jupyter/assets.ts` — **host-dependent** (P3-2). It is not a
  pure path computation: Q1's `jupyterAssets` (`jupyter.ts:665-696`) does FS I/O
  (`ensureDirSync(figures_dir)` + `walkSync` to promote the supporting dir) and
  returns the **snake_case 4-field** `JupyterNotebookAssetPaths` from
  `@quarto/types` (`{ base_dir, files_dir, figures_dir, supporting_dir }` —
  `base_dir` absolute, the rest relative + forward-slashed). The earlier draft's
  pure, camelCase 3-field shape broke Julia, which reads
  `join(assets.base_dir, assets.supporting_dir)` (`julia:287`).
  ```typescript
  import type { JupyterNotebookAssetPaths } from "@quarto/types";
  function assets(
      host: PlatformHost, input: string, to?: string,
  ): JupyterNotebookAssetPaths;   // { base_dir, files_dir, figures_dir, supporting_dir }
  ```
  - Implement against the seam: `host.fs.ensureDir(figures_dir)` for the dir
    creation, and **`host.fs.walk(...)`** for the supporting-dir promotion (the
    `walkSync` analogue).
  - **Upstream dependency (not Plan 3's to build), already satisfied:**
    `host.fs.walk` is owned by **Plan 1b** — the `PlatformHost.fs.walk` interface
    member *and* the `denoHost` impl, in lockstep — and has **already landed** on
    the integration line. `assets` here is a pure consumer. See the "Seam-name
    mapping" note under *Platform dependencies*. Unit-test `assets` with a mock
    host that stubs `walk`.
  - ~40 lines (incl. the dir creation + supporting-dir walk)

- [ ] Create `src/jupyter/result-helpers.ts`:
  - `resultIncludes(host, tempDir, deps?)` — **host-dependent** (P3-3). It routes
    through `includesForJupyterWidgetDependencies` → `widgetTempFile`
    (`widgets.ts:148-154`), which materializes widget includes to disk via
    `Deno.makeTempFileSync` + `writeTextFileSync`. Needs `host.fs`. Julia calls
    it on the widget hot path (`julia:256`).
  - `resultEngineDependencies(deps?)` — pass-through or wrap engine deps (the one
    genuinely pure method; no host).
  - ~50 lines

- [ ] Create `src/jupyter/index.ts` — exports the `createJupyter(host)`
  factory and **re-exports the public types from `@quarto/types`**
  (`JupyterNotebook`, `JupyterCell`, `JupyterToMarkdownOptions`,
  `JupyterToMarkdownResult`, `JupyterCellOutput`, `JupyterNotebookAssetPaths`,
  …) — it does not declare its own. Internal functions remain accessible via
  relative paths inside `@quarto/api` for tests and callers that want to pass
  their own host. The factory exposes **seven implemented methods** — the six
  Julia execute-path calls plus the host-bound `widgetDependencyIncludes` (the
  RTQ FC-2 deferred-deps producer, P3-7) — with `NotImplemented` stubs for the
  rest of the namespace (Phase 3E).

  ```typescript
  // src/jupyter/index.ts
  // NB: this package is ESM/NodeNext — relative imports use .js specifiers
  // (resolving the .ts source), and the platform barrel is ../platform/index.js.
  import type { PlatformHost } from "../platform/index.js";
  import { jupyterToMarkdown as _toMarkdown } from "./to-markdown.js";
  import { isPercentScript as _isPercent, percentScriptToMarkdown as _percentMd }
      from "./percent-script.js";
  import { assets as _assets } from "./assets.js";
  import { widgetDependencyIncludes as _widgetIncludes } from "./widgets.js";
  import {
      resultIncludes as _resultIncludes, resultEngineDependencies,
  } from "./result-helpers.js";

  export function createJupyter(host: PlatformHost) {
      return {
          toMarkdown: (nb, opts) => _toMarkdown(host, nb, opts),   // async → Promise
          isPercentScript: (file, exts) => _isPercent(host, file, exts),
          percentScriptToMarkdown: (file) => _percentMd(host, file),
          assets: (input, to) => _assets(host, input, to),        // host (P3-2)
          resultIncludes: (tempDir, deps) =>                       // host (P3-3)
              _resultIncludes(host, tempDir, deps),
          widgetDependencyIncludes: (deps, tempDir) =>             // host (P3-7, RTQ FC-2 producer)
              _widgetIncludes(host, deps, tempDir),
          resultEngineDependencies,                               // pure, no host
          // ...plus NotImplemented stubs for the remaining quarto.jupyter
          // members (see Phase 3E) so the object satisfies the namespace type.
      };
  }
  export type { /* public types re-exported from @quarto/types */ };
  ```

### Phase 3E: Integration with engine-host

Wire `@quarto/api/jupyter` into the `quarto.jupyter` namespace in
`@quarto/engine-host-deno`.

- [ ] Update `@quarto/engine-host-deno/src/quarto-api.ts` to call the
  `createJupyter` factory with the same `denoHost` used for the other
  namespaces in Plan 1b's `buildQuartoAPI(global, host)` assembly:
  ```typescript
  import { createJupyter } from "@quarto/api/jupyter";
  import { denoHost } from "./deno-host.js";

  function buildJupyterNamespace() {
      return createJupyter(denoHost);
  }
  ```
  The factory returns the implemented methods directly; the
  engine-host layer just forwards it. Any per-call wrappers
  (e.g. supplying the per-execute `tempDir` to `resultIncludes`) can be
  composed around the factory output. (RTQ Item A removed the combined
  `EngineHostContext`: process-stable config is the ambient `Init`
  `global`, and per-render project context arrives on `launchEngine`.)
- [ ] **State the namespace seam (P3-6).** The `quarto.jupyter` namespace
  (`quarto-types/src/quarto-api.ts`) is a wide interface (~20+ members);
  this plan implements 7 of them (`toMarkdown`, `isPercentScript`,
  `percentScriptToMarkdown`, `assets`, `resultIncludes`,
  `resultEngineDependencies`, and `widgetDependencyIncludes` — all real bodies).
  A partial object **won't typecheck** against the full namespace type. The
  remaining members
  (`isJupyterNotebook`, `notebookExtensions`, `kernelspecFromMarkdown`,
  `kernelspecForLanguage`, `fromJSON`, `markdownFromNotebookFile`,
  `markdownFromNotebookJSON`, `quartoMdToJupyter`, `notebookFiltered`, …) are
  **jupyter-built-in-only** — q2's jupyter engine is native-Rust and marimo uses
  `system.pandoc`, so **no current q2 TS runtime consumer needs them**. Provide
  them in `createJupyter` as `NotImplemented` throwers (defer-with-seam, not
  drop) so the object satisfies the namespace type and a future jupyter port has
  named slots. Do **not** silently narrow the `JupyterNamespace` type instead —
  that's undiscussed and loses the contract.
- [ ] `@quarto/api` is already a dependency of `@quarto/engine-host-deno`
  (added in Plan 1b) — no new dependency needed.

### Phase 3F: Testing — frozen Test Seam Spec

Check existing ts-packages for the test runner convention (likely Vitest).
Run `npm install` from the repo root if the package structure changed.

**Tier (one choice for the whole phase).** All tests run in **Node/Vitest**
against the **real `jupyter/` modules** — the unit under test is never mocked.
The only mocked boundary is `PlatformHost`: a **recording in-memory `host.fs`**
(captures `writeFileSync`/`ensureDir`/`makeTempFile`/`walk` calls + serves
reads), except the real-`.ipynb` smoke test, which may use a real temp dir.
**No browser/Playwright tier** — this library is pure text transform + FS
through the host; there is no layout/geometry/scroll engine, so a browser tier
would be slower *and* vacuous (jsdom would add nothing to assert here). **Frozen
rule:** once a row is green, its harness and assertions are not edited to go
green — if a later refactor changes an expected value, re-run the vacuity check
(does the new value still differ across the states this test distinguishes?)
before migrating it.

**Bound tests** — every row names the production hunk whose revert reddens the
named assertion (the impl doesn't exist yet, so hunks are named by behavior +
the Q1 line they port). The "discriminator" column is the input state that must
differ across the two sides of the behavior, or the row goes vacuous.

| # | Real unit · assertion surface | Mock boundary | Named revert → assertion RED · discriminator |
|---|---|---|---|
| 1 | `displayDataMimeType` — bundle `{text/markdown, text/html}`, opts `{toMarkdown:true}` ⇒ returns `'text/markdown'` | none (pure) | Revert dynamic base-order → fixed html-first list ⇒ returns `'text/html'`. **Disc:** bundle must hold *both* md+html (P3-9) |
| 2 | `displayDataIsJson` / `displayDataMimeType` — bundle w/ `…widget-view+json`, `{toHtml:true}` ⇒ widget MIME selected + `<script>` path | none | Revert the conditional widget-cluster splice ⇒ widget MIME never chosen (P3-9) |
| 3 | `displayDataLatexIsMath` — a **non-math** `text/latex` ⇒ emitted as `{=tex}` raw block | none | Revert the is-math test (unconditional latex→math) ⇒ emitted as math. **Disc:** latex must be non-math (P3-10) |
| 4 | `includeWarnings` — cell `{global warning:false, local warning:true}` ⇒ included | none | Revert the global-false+local-true override branch ⇒ excluded. **Disc:** global≠local (P3-12) |
| 5 | `tags` `echoFenced` — cell `echo: fenced` ⇒ fenced-echo path | none | Revert the `echoFenced` branch ⇒ plain echo (P3-12) |
| 6 | `cellLabelValidator` — two cells, same label ⇒ duplicate flagged | none | Revert the dedup check ⇒ no flag (P3-11) |
| 7 | `resolveCaptions` — cell w/ `fig-cap` ⇒ returned; cell w/ `tbl-cap` ⇒ **no** caption here | none | Revert fig-cap extraction ⇒ fig-cap case RED. **Disc:** tbl-cap negative asserts the boundary (P3-11) |
| 8 | `cell-options` — `#\| label: fig-1` ⇒ `options.label === 'fig-1'` | none | Revert the comment-YAML parser ⇒ undefined |
| 9 | **preserve INERT** (`isPreservedHtml` + `to-markdown`) — notebook w/ `text/html` output, `toMarkdown({toHtml:true})` ⇒ cell markdown contains the **literal** `<table>…`, `result.htmlPreserve` empty/undefined, `isPreservedHtml(anyHtml)===false` | recording `host.fs` | Revert `isPreservedHtml`→`true` (no restorer) ⇒ html replaced by `preserve<uuid>` ⇒ literal-html assertion RED. **Replaces the old "→ preservation markers" line, which asserted markers that don't exist today** (P3-15) |
| 10 | `widgetDependencies(nb)` in-place strip — notebook w/ a hoisted plotly lib in a cell output ⇒ after walk the lib `<script>` appears **exactly once** | none | Revert the in-place strip ⇒ appears twice. **Path-exercised:** also assert the strip mutated `cell.outputs` (P3-17) |
| 11 | `widgetDependencyIncludes(host, deps, tempDir)` — vendored `JupyterWidgetDependencies` ⇒ (a) `host.fs.makeTempFile`+`writeFileSync` were **called**, (b) return has key `"include-in-header"` with an **array** value, (c) assignable to `DependenciesResult.includes` | recording `host.fs` | Revert the key/array translation (emit camelCase scalar `inHeader`) ⇒ `"include-in-header"` absent RED; revert to `NotImplemented` stub ⇒ throws RED (P3-7, D) |
| 12 | **`result.dependencies` isHtml gate** (`to-markdown`) — widget notebook: `toMarkdown({toHtml:true,toIpynb:false})` ⇒ `result.dependencies` **defined**; `toMarkdown({toIpynb:true})` ⇒ **undefined** | recording `host.fs` | Revert `dependencies = isHtml ? widgetDependencies(nb) : undefined` (jupyter.ts:737-739 analogue) → always undefined ⇒ first case RED. **Disc:** the two opts states straddle the gate. **(Missing-test add — Finding A)** |
| 13 | image figure write (`to-markdown`) — notebook w/ `image/png` base64 + `assets` ⇒ recorded `host.fs.writeFileSync` to `assets.figures_dir/*.png` with **decoded bytes**, and markdown has `![](…png)` | recording `host.fs` | Revert the base64-decode→`writeFileSync` ⇒ no write recorded RED. **Path-exercised:** assert ≥1 write to `figures_dir` (a stub could emit the `![]()` ref without writing) |
| 14 | error traceback (`to-markdown`) — error output w/ ANSI ⇒ traceback text present, ANSI escape bytes absent | recording `host.fs` | Revert the ANSI strip ⇒ escape bytes present RED |
| 15 | **Julia-consumer-shape** (`to-markdown`) — `result.cellOutputs[0].markdown` is a defined string; `assets.base_dir`/`supporting_dir` present + snake_case; `notebookOutputs` (when present) has `prefix`/`suffix` | recording `host.fs` | Revert `cellOutputs` type `JupyterCellOutput[]`→`string[]` ⇒ `o.markdown` undefined RED (P3-1/2/5) |
| 16 | **percent-script detection** (`percent-script`) — `# %%` code-only `.jl` ⇒ `false`; `# %% [markdown]` ⇒ `true` | recording `host.fs` (reads file) | Revert the `[markdown\|raw]`-marker requirement (loosen to bare `%%`) ⇒ code-only detected `true` RED. **Disc:** the `false` case is the discriminator — assert both polarities (P3-14) |
| 17 | `percentScriptToMarkdown` (`percent-script`) — `# %% [markdown]\n# Hello` ⇒ a **markdown** cell (`# Hello`), not a code fence | recording `host.fs` | Revert the `[markdown]` branch ⇒ emitted as code RED |
| 18 | `assets(host, input, to)` — returns snake_case 4-field; recorded `host.fs.ensureDir(figures_dir)` + `host.fs.walk(...)` | recording `host.fs` | Revert the `ensureDir` call ⇒ mock has no `ensureDir` RED. **(Missing-test add)** (P3-2) |
| 19 | **Contract-conformance (tsc)** — `createJupyter(host)` is assignable to the full `quarto.jupyter` namespace type | n/a (compile-time) | Revert (delete) one `NotImplemented` stub ⇒ `tsc` build error RED (P3-6) |

- [ ] Smoke/shape (not discriminators — the rows above carry the binding):
  convert a simple notebook JSON (2 code, 1 markdown) → markdown; and run a real
  `.ipynb` fixture through `toMarkdown` end-to-end (real temp dir) to catch gross
  integration breakage.

**Accepted-untested (logged, with rationale — not silently omitted):**
- **`pandoc` field population** — only guarded for *presence* (vendored type +
  row 19). The Q1 metadata-accumulation port is deferred; the plan allows
  emitting `{}` with a noted gap (Phase 3C), so there is no behavior to bind
  yet. Re-spec a bound row when the accumulation is actually ported.
- **`executeOptions`/`figPos` fixup-profile selection** (P3-4) — the
  book/single-file/minimal fixup behavior is exercised only under project
  renders (no v1 caller); field *presence* is guarded by the vendored type + row
  19. Accepted-untested until the project renderer exists.
- **ANSI → HTML color on HTML output** (P3-16) — deliberately not ported
  (strip-only); the divergence is documented, not a regression to guard.

## Design Notes

### Simplified vs. Quarto 1

Key simplifications in our rewrite:

1. **No YAML schema validation** for cell options — just parse with js-yaml
2. **No deno-dom** for ANSI→HTML — just strip ANSI codes (matches Q1 except on
   HTML output; can add conversion later — see Phase 3C ANSI note / P3-16)
3. **No tree-sitter** — cell options parsing uses regex/yaml
4. **No MappedString provenance** — `toMarkdown` returns plain markdown strings;
   Julia builds its own source ranges separately (`mappedString.splitLines` /
   `indexToLineCol`, `julia:638-647`), so dropping provenance here breaks no
   consumer read.
5. ~~**Flattened options types** — redraft `JupyterToMarkdownOptions` …~~
   **SUPERSEDED (2026-06-29).** Do **not** redraft. Use the vendored
   `@quarto/types` Jupyter contract directly (`JupyterToMarkdownOptions/Result`,
   `JupyterCellOutput`, `JupyterNotebookAssetPaths`). The vendored types are
   *already* flattened to loose index-signature shapes, so they deliver the
   intended "no transitive-dep explosion" benefit **without** the redraft — and
   unlike the redraft they are the actual namespace contract and the shape the
   Julia consumer reads. The redraft narrowed/mistyped the contract and broke
   Julia at runtime (P3-1/2/4). See the reconciliation banner at the top.

The first four simplifications keep this to ~1300 lines of clean code vs.
~5000+ lines of tangled Quarto 1 code. The dependency-explosion concern that
motivated #5 is solved by the vendored types, not by redrafting.

### Dependency on `@quarto/api/text` or `@quarto/api/markdown`

If `jupyter/` needs text helpers (e.g., `lines`, `pandocAutoIdentifier`) or
markdown parsing, it imports them directly from the sibling subpath (e.g.,
`import { lines } from "../text/text.js"`). Since both live in the same
`@quarto/api` package, there's no cross-package version coordination — they
are released together. If internal relative imports become noisy, we can
use the package's own exports map inside the package (`@quarto/api/text`).

### Accuracy target

The output should match Quarto 1's for the common cases:
- Code cells with text, image, and HTML outputs
- Cell visibility options (echo, include, output)
- Figure file generation and referencing
- HTML preservation

Edge cases where we may differ:
- Rare MIME types (vdom, plotly — add support as needed)
- Complex widget dependency chains
- ANSI color preservation **on HTML output only** (we strip; Q1 converts to
  HTML spans + an `ansi-bold` class). Latex/markdown/ipynb targets match Q1
  exactly. See Phase 3C ANSI note / P3-16.

### Portability constraints

Same rules as Plan 2 (see "Portability constraints" in that plan):

1. No q2-specific imports from `jupyter/`.
2. **No `Deno.*` or `node:*` references inside `jupyter/`.** All I/O goes
   through the `PlatformHost` passed to `createJupyter(host)`. The **six**
   FS-touching methods (`toMarkdown`'s figure writes, `isPercentScript`,
   `percentScriptToMarkdown`, `assets`'s `ensureDir`/`walk`,
   `resultIncludes`'s widget temp writes, and `widgetDependencyIncludes`'s temp
   writes) call `host.fs.*` explicitly; only `resultEngineDependencies` and the
   rest of `jupyter/` are pure.
3. No dependency on `@quarto/engine-host-deno` (dependency runs the other direction).
4. Same package can later run under `@quarto/engine-host-wasm` with a
   VFS-backed host — no changes to `jupyter/` required, only a different
   `PlatformHost` implementation plugged in at the engine-host layer.

### Future: Quarto 1 adoption

`@quarto/api/jupyter` is designed to be importable by Quarto 1, replacing:
- `external-sources/quarto-cli/src/core/jupyter/jupyter.ts` (the `jupyterToMarkdown` function)
- `external-sources/quarto-cli/src/core/jupyter/display-data.ts`, `tags.ts`, `labels.ts`, `preserve.ts`, `widgets.ts`
- Parts of `external-sources/quarto-cli/src/core/jupyter/jupyter-shared.ts`

Because this plan now implements against the **vendored `@quarto/types`
contract** (which is itself vendored from Q1), the public signatures match Q1's
by construction — no "adapt Q1's options to our flattened types" step is needed.
(The earlier draft's claim that its *redrafted* signatures were "compatible" was
false — disproven by `julia:272,287,231,245`; it has been removed.)

## Success Criteria

- [ ] `@quarto/api/jupyter` populated with all **7 methods** (the 6 Julia
  execute-path calls + `widgetDependencyIncludes`), exposed via
  `createJupyter(host)` factory
- [ ] `widgetDependencyIncludes` is a **real host-bound body**, not a
  `NotImplemented` stub — returns `PandocIncludes` assignable to
  `DependenciesResult.includes`, so the RTQ FC-2 deferred-deps wire has its
  producer (unit-tested in isolation; book renderer lights it up later)
- [ ] **Public types come from `@quarto/types`** — no redrafted
  `JupyterToMarkdownOptions`/`Result`/notebook types inside `jupyter/`
- [ ] `createJupyter(host)` typechecks against the full `quarto.jupyter`
  namespace type (remaining members are `NotImplemented` stubs — P3-6)
- [ ] No `Deno.*` or `node:*` references inside `@quarto/api/jupyter`; `assets`,
  `resultIncludes`, and `widgetDependencyIncludes` take the host (not pure —
  P3-2/P3-3/P3-7)
- [ ] `toMarkdown` is async, returns `cellOutputs: JupyterCellOutput[]` (not
  `string[]`), and round-trips through the Julia consumer (P3-1)
- [ ] `toMarkdown` correctly converts notebooks with code, markdown, and raw cells
- [ ] Image outputs write files to disk (via `host.fs.writeFileSync`) into
  `assets.figures_dir` and emit correct markdown references
- [ ] HTML preservation ports `isPreservedHtml` as the constant-`false` no-op it
  is in Q1 today; no live restore mechanism is claimed (P3-15)
- [ ] Error outputs format tracebacks readably
- [ ] All tests pass (unit tests can pass a mock host with in-memory FS)
- [ ] Integrated into `@quarto/engine-host-deno`'s QuartoAPI via
  `createJupyter(denoHost)` in `buildJupyterNamespace`
