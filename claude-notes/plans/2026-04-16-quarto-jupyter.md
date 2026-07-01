# Plan 3: @quarto/api/jupyter

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plan 2A (the `@quarto/api` package skeleton). Phases 3A-3D and 3F otherwise independent, **except** `assets()` (Phase 3D) consumes the `PlatformHost.fs.walk` seam op that **Plan 1b** owns (interface member + `denoHost` impl, in lockstep) — **already landed** on the integration line (see *Platform dependencies*). Phase 3E (wiring into engine-host) targets `buildQuartoAPI` — the assembly that **Plan 2 Phase A** landed inside the `@quarto/engine-host-deno` package (the package itself was created by Plan 1b). Both have landed.
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

> **Follow-up review (2026-07-01).** A second pass ground three items against
> source that the 2026-06-29 reconciliation had left on assumption:
> (a) **`pandoc` field** — the *vendored* Q1 `jupyterToMarkdown`
> (`jupyter.ts:713-874`) never populates `pandoc`; it always returns
> `undefined`. So `toMarkdown` must return `pandoc: undefined` for **exact
> parity** — the earlier "emit `{}` / port the accumulation" guidance described
> *mainline* Quarto, not the vendored subtree, and is out of scope (Phase 3C).
> (b) **Figure naming is now pinned** to Q1's concrete scheme (Phase 3C/3D) —
> Q2's resolver and Plan 4's assertions already expect it. (c)
> **`notebookExtensions` is a real value** (`[".ipynb"]`, Q1
> `kJupyterNotebookExtensions`), not a `NotImplemented` stub; the full remaining
> roster is enumerated in Phase 3E. Also called out: the
> `mdRawOutput`/`mdFormatOutput` export seam (Phase 3C) and the canonical
> `kLangCommentChars` source (Phase 3A).

> **Blank-slate implementer audit (2026-07-01).** A fresh read against the
> *landed* code corrected the Phase 3E wiring and factory naming: (a) the
> package's factory convention is `make<Ns>(host)` — `makeConsole`/`makeSystem`/
> `makePathHost`/`makeMappedStringHost` — there is **no `create*`**, so the
> factory is `makeJupyter(host)` (the earlier `createJupyter` + its
> `createPath`/`createSystem` "precedent" were fabricated). (b) `buildQuartoAPI`
> is **Plan 2** work (already landed), not Plan 1b; Phase 3E replaces its
> throwing `jupyterStub` Proxy with `makeJupyter(host)` and drops the
> `notYetImplementedError` helper + cast — it does **not** import `denoHost`
> directly. (c) `resultEngineDependencies` **wraps** (`deps ? [deps] :
> undefined`), not a bare pass-through. (d) `displayDataLatexIsMath` is a
> `string[]→bool` **predicate**; the `{=tex}`-vs-math routing is a separate
> `displayDataWithMarkdownMath` (Test Row 3 rebound). (e) `./jupyter` must be
> added to `@quarto/api`'s `exports` map (2A landed without it); `@quarto/types`
> stays a `devDependency` (type-only imports). (f) `cell-options.ts` given a
> signature + the `JupyterCell→JupyterCellWithOptions` upgrade that must run
> **before** `tags.*` in the walk. No design questions — one convention call
> (`makeJupyter`, single-factory, by analogy to `makeSystem`) is flagged for the
> user in the session.

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
> (`ts-packages/quarto-api/src/platform/index.ts`, the `fs` block ~93-119;
> `walk` ~115-118) uses bare,
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
`makeJupyter(host)` that returns the full namespace. The host
is bound once and threaded into every FS-touching method (including `assets`,
`resultIncludes`, and `widgetDependencyIncludes`). This matches the **landed**
`@quarto/api` factory convention — `makeConsole(host)`,
`makeSystem(host, global)`, `makePathHost(host, global)`,
`makeMappedStringHost(host)` (there is **no** `create*` factory in the package;
each factory takes a **`Pick<PlatformHost, …>`** of the subset it uses, and
`global: HostGlobalConfig` **only if** it reads process-stable config): one
`make<Ns>(host[, global])` entry point per subpath, wired in
`@quarto/engine-host-deno`'s `buildQuartoAPI(global, host)` (**note: `global`
first**). Jupyter reads only `host.fs` (no `global`), so `makeJupyter(host)`
takes one arg — unlike `makeSystem(host, global)`. (Optionally narrow to
`makeJupyter(host: Pick<PlatformHost, "fs">)` to match the sibling `Pick`
convention; the internal impls would then take the same `Pick`.)

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
| `resultEngineDependencies(deps)` | Extract engine-specific deps | Low — **wraps** `deps ? [deps] : undefined` (pure, no host; **NOT** a bare pass-through — Q1 `executeResultEngineDependencies`, `jupyter.ts:2177-2185`) |
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

- [x] Confirm `@quarto/api` package skeleton from Plan 2A is in place (it **has**
  landed). If it hadn't, you would create the minimal scaffolding first
  (`package.json`, `tsconfig.json`, `exports` map).

- [x] **Add the `./jupyter` subpath to `@quarto/api`'s `exports` map**
  (unconditional — 2A landed *without* a `./jupyter` entry; `package.json`
  currently has none). Mirror the sibling entries, e.g.:
  ```json
  "./jupyter": {
    "types": "./src/jupyter/index.ts",
    "source": "./src/jupyter/index.ts",
    "import": "./dist/jupyter/index.js"
  }
  ```
  Without this, `import … from "@quarto/api/jupyter"` (used by engine-host in
  Phase 3E and by tests) won't resolve.

- [x] `@quarto/types` is **already a `devDependency`** of `@quarto/api` (spec
  `"*"`). Since every `jupyter/` type reference is `import type` (erased at
  runtime), a devDep is sufficient — do **not** add it as a runtime
  `dependency`. All `jupyter/` modules import their public types from there —
  `import type { JupyterNotebook, JupyterToMarkdownOptions,
  JupyterToMarkdownResult, JupyterCellOutput, JupyterNotebookAssetPaths,
  JupyterWidgetDependencies } from "@quarto/types"`. **Do not create a
  `src/jupyter/types.ts` that re-declares these.** (Note: `index.ts` *re-exports*
  these types; that's a type-only re-export through a devDep. It resolves in this
  monorepo because downstream consumers — engine-host — carry `@quarto/types`
  themselves. If a future external consumer relied on the re-export without its
  own `@quarto/types`, promote it to a `dependency` then.)

- [x] *(Optional, implementer's discretion.)* The vendored `JupyterOutput` is
  loose (`output_type: string; [key: string]: unknown`). For ergonomics inside
  the output-formatting switch in `to-markdown.ts`, an **internal-only**
  discriminated union (`stream | display_data | execute_result | error`) may be
  declared and the loose `outputs` array narrowed to it once at the top of the
  cell walk. This type never crosses the namespace boundary — parameters and
  return values stay vendored. (Skip it and narrow inline if preferred; it
  changes no contract.)

- [x] Create `src/jupyter/constants.ts` — MIME type constants, cell option
  keys, etc. Reference: Quarto 1's `external-sources/quarto-cli/src/config/constants.ts` (just the
  subset we need). Include `kQuartoMimeType = "quarto_mimetype"`
  (`jupyter.ts:187`; injected into widget `<script>` tags, see P3-10) and the
  language-comment-char table `kLangCommentChars` (needed by percent-script
  detection, see P3-14). **Port `kLangCommentChars` from the canonical exported
  copy at `core/lib/partition-cell-options.ts:310`** — NOT the stale
  non-exported duplicate at `jupyter.ts:1208` (the two tables diverge in several
  entries — e.g. `ojs`, `prql`, `scss`, `tikz`, `mermaid` — so pick the canonical
  one deliberately rather than whichever is nearer). **Value type is `string |
  [string, string]`** (a `[open, close]` tuple for block-comment langs like `c`);
  py/jl/r are all plain `"#"`, so the percent regex's `${cms}` interpolation is
  fine for the languages we care about, but the type must be handled (don't assume
  every value is a bare string).

### Phase 3B: Supporting modules

Small, focused modules that `toMarkdown` depends on. Each is self-contained.

- [x] Create `src/jupyter/display-data.ts` — MIME bundle dispatch:
  - `displayDataMimeType(output, options)` — select best MIME type from bundle
  - **Predicate signatures take the selected `mimeType: string`, NOT the output**
    (verified against Q1): `displayDataIsImage(mimeType)` (`display-data.ts:156`),
    `displayDataIsJson(mimeType)` (`:176`), `displayDataIsMarkdown`/`IsLatex`/
    `IsHtml(mimeType)`. There is **no** `displayDataIsTextPlain` export in Q1 — do
    not invent one. (The earlier `displayDataIsImage(output)` shape was wrong.)
  - **MIME priority is computed DYNAMICALLY from the target format — NOT a fixed
    list** (corrected, P3-9, `display-data.ts:45-106`):
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
    - The splices listed here are **illustrative, not exhaustive** — the function
      also handles `application/pdf` (added on `toLatex`; also matched by
      `displayDataIsImage`) and pushes `text/html` for `toMarkdown`-only, among
      others. **Port the whole `displayDataMimeType` algorithm from source; do
      not re-derive the set from this list.**
  - `displayDataIsJson(output)` matches **only the widget MIME types**
    (`display-data.ts:176-179`) and emits a `<script type=…>` tag with
    `kQuartoMimeType` injected first (falling back to a json code block only for
    `toIpynb`). There is **no generic `application/json → code block` path**
    (corrected, P3-10).
  - **Two functions + a downstream emitter — do not conflate (P3-10):**
    `displayDataLatexIsMath(latex: string[])` (`display-data.ts:108-120`) is a
    **pure predicate** returning `boolean` (does the latex start with `$` /
    `\begin{`?); it does not route or emit. `displayDataWithMarkdownMath(output)`
    (`display-data.ts:122-137`) is a **pre-transform**: when the predicate holds
    (and there is no existing `text/markdown` slot) it **hoists the latex into
    `data["text/markdown"]`** so it later renders as math; for non-math latex it
    returns the output **unchanged**. It does **not** emit `{=tex}`. The `{=tex}`
    raw block for non-math latex is emitted **downstream** by the output emitter
    (`mdLatexOutput` → `mdFormatOutput("tex")`, Q1 `jupyter.ts:2084-2086` /
    `1062-1065`) once `displayDataMimeType` selects `text/latex`. Net: **math
    latex → markdown slot → math; non-math latex → stays latex → `{=tex}`
    block.** `text/latex` is not unconditionally math.
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/display-data.ts`
  - ~150 lines

- [x] Create `src/jupyter/tags.ts` — cell visibility logic:
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

- [x] Create `src/jupyter/labels.ts` — cell label and caption handling
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

- [x] Create `src/jupyter/preserve.ts` — HTML preservation (corrected
  signature, P3-13, `preserve.ts:12-42`):
  - `removeAndPreserveHtml(nb: JupyterNotebook) => Record<string, string> |
    undefined` — takes the **whole notebook** and returns the preserve map (or
    `undefined`). Its per-output swap (`data[text/markdown]=[key]; delete
    data[text/html]`, `preserve.ts:24-31`) **mutates cell output bundles in place
    — but only when `isPreservedHtml(htmlText)` holds**. Since `isPreservedHtml`
    is constant-`false` today (`:58-60`), **the swap never fires**: no bundle is
    mutated, `htmlPreserve` comes back empty, and `text/html` passes through
    literally (this is why Test Row 9 asserts the literal `<table>`). It is
    **not** a per-output pure `(output) => { output, preserved }` transform as the
    earlier draft had it. Port the gated swap anyway for parity — it stays inert.
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

- [x] Create `src/jupyter/widgets.ts` — Jupyter widget dependency extraction:
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
    - **Input arity (verified — wrap in an array):** Q1's
      `includesForJupyterWidgetDependencies(dependencies: JupyterWidgetDependencies[],
      tempDir)` takes an **array** (`widgets.ts:73-75`), but the namespace method
      passes a **single** dep (`widgetDependencyIncludes(deps, tempDir)`,
      `quarto-api.ts:345-348`). The port body must call it as
      `includesForJupyterWidgetDependencies([deps], tempDir)`.
    - **Port shape adaptation (verified — do NOT copy Q1's keys verbatim):** Q1's
      `includesForJupyterWidgetDependencies` returns a *different* local shape —
      `{ inHeader: string, afterBody: string }` (**camelCase, scalar** strings).
      The port must translate that onto the vendored `PandocIncludes`: rename
      `inHeader`→`"include-in-header"`, `afterBody`→`"include-after-body"`, and
      wrap each scalar temp-file path in an **array** (`string[]`).
    - **Wire contract (verified):** once in vendored `PandocIncludes` form, the
      method's return type is *identical* to `DependenciesResult.includes:
      PandocIncludes` (`execution.ts:132`) — so **the method itself needs no
      further adaptation**. There *is* still a kebab→camelCase rename at the wire,
      but the **harness** performs it, not this method: `PandocIncludes`
      (kebab, `include-in-header`…) → `TsPandocIncludes` (camelCase
      `inHeader`/`beforeBody`/`afterBody`) via the harness's `renameIncludes()`
      (defined in `@quarto/engine-host-deno`'s `host.ts`; the `TsPandocIncludes`
      shape + rename are described at `pandoc.ts:18-25`). Return kebab
      `PandocIncludes`; let the harness convert.
    - **MUST be exported from the `makeJupyter` factory** (P3-7) with the host
      bound, so the namespace's `widgetDependencyIncludes` and RTQ's FC-2
      deferred-deps fold reach it via `quarto.jupyter.*`. The earlier draft built
      it inside `widgets.ts` but never exposed it. (Its array-vs-singular *type*
      is D.2 drift owned by Plan 2 Phase B; the **exposure + real body** are this
      plan's.)
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/widgets.ts`
  - ~100 lines

- [x] Create `src/jupyter/pandoc-id.ts` — identifier generation:
  - `pandocAutoIdentifier(text, asciify)` — generate Pandoc-style IDs from
    heading text. **Note the 2nd boolean arg** — Q1 names it `asciify`
    (`pandoc-id.ts:9`) and calls it with two args (`jupyter.ts:1548`); the
    earlier draft's 1-arg signature dropped it (P3-11). (Distinct from
    `asHtmlId` in `core/html.ts:16`, which is single-arg — used for cell-label
    id normalization, see labels.ts.)
  - Pure string manipulation, no dependencies
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/pandoc/pandoc-id.ts`
  - Note: lives under `jupyter/` for now because jupyter is the only
    consumer. If other consumers emerge, promote to a top-level `pandoc/`
    subpath — cheap move, cheap rename.
  - ~50 lines

- [x] Create `src/jupyter/cell-options.ts` — simplified cell options parsing:
  - **Signature/return shape (specify — the earlier draft left this blank):**
    `parseCellOptions(source: string[], language: string): { options:
    Record<string, unknown>; optionsSource: string[] }`. Strip the
    language-comment prefix + `| ` from each leading option line (the comment
    char comes from `kLangCommentChars[language]`, see Phase 3A / `constants.ts`),
    collect the contiguous `#| …` block, `yaml`-parse it to `options`, and return
    the raw lines as `optionsSource`. Reference Q1's `partitionCellOptions` /
    `partition-cell-options.ts` for the prefix-strip logic.
  - Use `yaml` package directly (`import { parse } from "yaml"`) — no schema
    validation, no tree-sitter (**Simplified from Quarto 1**).
  - **Type-upgrade + ordering (load-bearing):** the vendored `JupyterCell`
    (`jupyter.ts:97-109`) has **no `.options` field** — `.options` /
    `.optionsSource` live on `JupyterCellWithOptions` (`jupyter.ts:88-92`). The
    `tags.*` visibility checks read `cell.options[…]`, so the cell walk must
    **run `parseCellOptions` first and upgrade `JupyterCell → JupyterCellWithOptions`
    per code cell BEFORE calling `tags.*`**. State this ordering in the walk
    (Phase 3C) so tags never read an absent `.options`.
  - ~100 lines

### Phase 3C: Core toMarkdown function

The main conversion function. Takes a `JupyterNotebook` and options, returns a
`JupyterToMarkdownResult` (`cellOutputs` array + `dependencies`/`htmlPreserve`/
`notebookOutputs`/`pandoc`) — **not** a bare markdown string; the caller joins
`cellOutputs.map(o => o.markdown)` (as Julia does, `julia:272`).

- [x] Create `src/jupyter/to-markdown.ts`. **Use the vendored
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
  //   NB: `assets: JupyterNotebookAssetPaths` is an INPUT field of
  //     JupyterToMarkdownOptions (jupyter.ts:250) — snake_case base_dir/... ,
  //     the dir the writer writes figures into — NOT a JupyterToMarkdownResult
  //     field (P3-2).

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
  - `pandoc?` — Julia forwards `result.pandoc` to the host verbatim
    (`julia:289`), but the **vendored** Q1 `jupyterToMarkdown`
    (`jupyter.ts:713-874`) **never populates it**: its `return` omits the field,
    there is no front-matter promotion, no cell-`pandoc` hoisting, and no
    cross-cell accumulator, so at runtime `result.pandoc` is always `undefined`.
    **Return `pandoc: undefined` for exact parity with the vendored source.**
    The cross-cell metadata accumulation the earlier draft described is
    *mainline* Quarto behavior that is NOT in the vendored subtree q2 ports
    against — it is **out of scope, not a deferred gap**. (Emitting `{}` would be
    harmless downstream — the render merge is guarded by
    `if (executeResult.pandoc)` and `mergeConfigs(x, {})` is a no-op — but
    `undefined` is the faithful port.)

- [x] **Pre-walk pass (runs BEFORE the cell walk — ordering is load-bearing).**
  Q1 computes `dependencies` and `htmlPreserve` at `jupyter.ts:738-741`, *then*
  walks the cells: `:744` declares the `cellOutputs` array; the `for` loop over
  `nb.cells` begins at `:754`. Both pre-walk calls **mutate `nb` in place** — most
  importantly `widgetDependencies(nb)` strips hoisted HTML libraries (P3-17). If
  the walk runs first, those libs are already emitted and get double-emitted.
  Run this pass first; see the result-assembly item below for the exact
  assignments. (Test row 10 guards this ordering — the plotly `<script>` must
  appear exactly once.)

- [x] Implement cell walking logic. **Each iteration produces one
  `JupyterCellOutput` — not a bare string.** The per-cell markdown computed by
  the steps below is the `markdown` field; assemble the rest of the
  `JupyterCellOutput` (`{ id, markdown, metadata, options }`) from the cell:
  `id` from `cellLabel`/`asHtmlId` (labels.ts; falls back to a counter),
  `options` from cell-options.ts, `metadata` from the cell's `metadata`. The
  result's `cellOutputs` is the array of these (P3-1).
  1. Iterate notebook cells
  2. For each markdown cell: `markdown` = source as-is
  3. For each code cell:
     a. **Extract options + label FIRST** (→ `options`, → `id`): run
        `parseCellOptions` (cell-options.ts) and upgrade `JupyterCell →
        JupyterCellWithOptions`; derive `id` via `cellLabel`/`asHtmlId`
        (labels.ts, falls back to a counter). This MUST precede visibility —
        `tags.*` reads `cell.options`.
     b. Check visibility (echo, include, output options via tags.ts). Honor
        `options.keepHidden`: when set, hidden cells are still emitted (marked
        hidden) rather than dropped — Q1 branches on it in the walk.
     c. Emit code fence with language and options
     d. Format each output (see below)
     e. Handle figure outputs (write to disk, emit `![]()` reference)
  4. For each raw cell: `markdown` = source with format marker

- [x] **Assemble the rest of the result object.** `cellOutputs` (above) is not
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
    (prefix/suffix) per the interface notes above (P3-5). **`pandoc` is
    `undefined`** — the vendored Q1 producer never sets it (see the `pandoc?`
    note above); do not synthesize a value.

- [x] Implement output formatting:
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
  - **Export `mdRawOutput` and `mdFormatOutput` from this module** — they are
    internal helpers in Q1's `jupyter.ts`, but `percentScriptToMarkdown`
    (Phase 3D) imports them (`percent.ts:12`), so they must be public here or the
    percent-script module seam won't compile.

- [x] Implement figure handling:
  - Write image data to `assets.figures_dir` (snake_case, from the vendored
    `JupyterNotebookAssetPaths` — P3-2) via `host.fs.writeFileSync`
    (base64 decode for PNG/JPEG, write as bytes; SVG written as text). The
    target dir is the one `assets()` creates with `host.fs.ensureDir` — these two
    must agree, which is why `assets` is host-dependent (P3-2).
    The host is captured via the `makeJupyter(host)` factory closure —
    `to-markdown.ts` itself takes `host` as a parameter on the internal
    implementation function.
  - **Filename scheme — port Q1 exactly (verified, do not invent).** Q1 builds
    the base as `[<outputPrefix>-]<labelName>-output`, where `labelName` is the
    cell label with a leading `#` stripped and `:`→`-`, else `cell-<cellIndex+1>`
    (`jupyter.ts:1541-1548`); each display-data image appends `-<outputIndex+1>`
    (`:1693`) and the extension from `extensionForMimeImageType(mime)`, and the
    final path is `figures_dir + "/" + name + "." + ext` (`jupyter.ts:1998`). Net
    template: **`<stem>_files/figure-<to>/[<prefix>-]<label|cell-N>-output-<m>.<ext>`**
    (e.g. `notebook_files/figure-html/cell-2-output-1.png`). This is load-bearing:
    Q2's resolver already expects `<stem>_files/figure-html/…`
    (`resource_resolver.rs:561,578`) and Plan 4 4C/4H assert
    `plot_files/figure-html/...`. (The native Rust jupyter engine does NOT write
    figures — `output.rs:172-197` emits an `[Image output: <ext>]` placeholder
    (literal `"[Image output: {}]"`) + TODO — so Q1 is the only reference
    implementation to match.)
  - Emit markdown image reference with optional caption, width/height
  - Handle `fig-format` option (request specific format from kernel)

- [x] Implement HTML preservation (faithful port — **inert today**, P3-15):
  - Port `removeAndPreserveHtml(nb)` (gated `isHtml`, see result-assembly above)
    and `isPreservedHtml`. Because `isPreservedHtml` returns constant-`false`,
    nothing is actually protected — `htmlPreserve` ends up empty and html outputs
    pass through unchanged. **Do not** build a live UUID-placeholder/restore
    mechanism; that's deferred (RTQ F2/B2, No-DOM AST-transform restorer).
  - The (empty) map flows out as `result.htmlPreserve`; Julia gates `postProcess`
    on it being non-empty (`julia:292-294`), so today `postProcess` is always
    false — matching Q1 end-to-end.

- [x] ANSI code handling (deliberate simplification — documented divergence,
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

- [x] Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/jupyter.ts` function `jupyterToMarkdown` (~lines 380-700)

### Phase 3D: Utility functions

The simpler methods that the Julia engine also calls.

- [x] Create `src/jupyter/percent-script.ts` — host-dependent (reads files):
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
    mis-claims (or fails to claim) Julia percent files. (Q1's *default* `exts`
    when omitted is `[".py", ".jl", ".r", ".q"]` — includes `.q` — `percent.ts:16-21`;
    the plan/namespace doc-comment's `['.py','.jl','.r']` is imprecise, but Julia
    always passes `[".jl"]` explicitly, so the default rarely matters.)
  - `percentScriptToMarkdown` — read file + convert percent-format to markdown:
    - language-comment `%%+ [markdown]` → markdown cells; `[raw]` → raw cells
    - other `%%`-delimited content → code cells
    - **Not a self-contained ~80-line regex module:** Q1's
      `markdownFromJupyterPercentScript` imports `mdRawOutput`/`mdFormatOutput`
      from `jupyter.ts` (`percent.ts:12`), so percent-script **couples to the
      to-markdown module**. Plan accordingly (shared output-formatting helpers,
      not a duplicate). **Import them via `../to-markdown.js`** — they must be
      exported there (the Phase 3C output-formatting item adds that export);
      without it the coupling cannot compile.
  - The public `makeJupyter(host)` factory binds `host` so callers see the
    natural 1-arg / 2-arg signatures.
  - Reference: Quarto 1's `external-sources/quarto-cli/src/core/jupyter/percent.ts`
  - ~80-120 lines

- [x] Create `src/jupyter/assets.ts` — **host-dependent** (P3-2). It is not a
  pure path computation: Q1's `jupyterAssets` (`jupyter.ts:665-696`) does FS I/O
  (`ensureDirSync(figures_dir)` + `walkSync` to promote the supporting dir) and
  returns the **snake_case 4-field** `JupyterNotebookAssetPaths` from
  `@quarto/types` (`{ base_dir, files_dir, figures_dir, supporting_dir }` —
  `base_dir` absolute, the rest relative + forward-slashed). The earlier draft's
  pure, camelCase 3-field shape broke Julia, which reads
  `join(assets.base_dir, assets.supporting_dir)` (`julia:287`).
  - **Concrete field values (port Q1, `jupyter.ts:665-696`):** `base_dir` =
    `dirname(input)`; `files_dir` = `<stem>_files` (Q1 `inputFilesDir`,
    `render.ts:13-16`); `figures_dir` = `<files_dir>/figure-<to>`, where the
    `figure-<to>` segment comes from Q1 `figuresDir(to)` (`render.ts:20-26`):
    normalize `html4`→`html`, strip any `+…`/`-…` suffix from the pandoc `to`,
    prefix `figure-`, and **default to `figure-html` when `to` is undefined**.
    `supporting_dir` = `files_dir`, unless `files_dir` already holds other
    subdirs (the `walkSync` check, `jupyter.ts:680-687`), in which case it is
    `figures_dir`. The `figures_dir` computed here MUST equal the dir the
    figure-write step in `to-markdown.ts` writes into (Phase 3C figure handling).
  - **Q1-faithful cwd coupling (eyes-open, verify at the `.ipynb` smoke test):**
    the returned `figures_dir` is **relative** (forward-slashed,
    `pathWithForwardSlashes(relative(base_dir, figures_dir))`, `jupyter.ts:690-694`)
    while `ensureDir` runs on the **absolute** path. `to-markdown` then writes to
    the returned *relative* `assets.figures_dir`, so the write lands in the dir
    `ensureDir` created **only when cwd == `base_dir`**. This mirrors Q1 exactly;
    don't "fix" it, but confirm the smoke test runs with cwd at the input's dir.
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

- [x] Create `src/jupyter/result-helpers.ts`:
  - `resultIncludes(host, tempDir, deps?)` — **host-dependent** (P3-3). It reuses
    the same widget includes-builder as `widgetDependencyIncludes` — **import it
    from `../widgets.js`** (do not duplicate): call
    `includesForJupyterWidgetDependencies([deps], tempDir)` (array-wrap, as in the
    widgets.ts item), which routes through `widgetTempFile` (`widgets.ts:148-154`)
    to materialize widget includes to disk via `makeTempFile` + `writeFileSync`.
    Needs `host.fs`. **When `deps` is `undefined`, return an empty
    `PandocIncludes` (`{}`)** — do not call the builder. Julia calls it on the
    widget hot path (`julia:256`).
  - `resultEngineDependencies(deps?)` — **wrap** a single deps object in an array:
    `return deps ? [deps] : undefined` (Q1 `executeResultEngineDependencies`,
    `jupyter.ts:2177-2185`). Return type is `Array<JupyterWidgetDependencies> |
    undefined` (namespace `quarto-api.ts:368-370`) — a bare `return deps` is both
    a type error and wrong behavior. The one genuinely pure method; no host.
  - ~50 lines

- [x] Create `src/jupyter/index.ts` — exports the `makeJupyter(host)`
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

  export function makeJupyter(host: PlatformHost) {
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
          // ...plus `notebookExtensions: [".ipynb"]` (a real Q1 value, NOT a
          // stub) and NotImplemented throwers for the other 15 remaining
          // quarto.jupyter members (see Phase 3E) so the object satisfies the
          // namespace type.
      };
  }
  export type { /* public types re-exported from @quarto/types */ };
  ```

### Phase 3E: Integration with engine-host

Wire `@quarto/api/jupyter` into the `quarto.jupyter` namespace in
`@quarto/engine-host-deno`.

- [x] Wire `makeJupyter` into `@quarto/engine-host-deno/src/quarto-api.ts`.
  **`buildQuartoAPI` is Plan 2 work (Phase A / B2), not Plan 1b** — and it has
  **already landed**. Match the pattern the file actually uses today: every
  namespace factory takes the **`host` parameter** of
  `buildQuartoAPI(global, host)` (**global first, host second**) — e.g.
  `makeConsole(host)`, `makeSystem(host, global)`, `makePathHost(host, global)` —
  **not** a directly-imported `denoHost`. So the
  change is:
  - `import { makeJupyter } from "@quarto/api/jupyter";`
  - **Replace the `jupyterStub` `Proxy`** (a throwing placeholder that raises
    `notYetImplementedError("jupyter.<prop>", "Plan 3")`) with
    `const jupyterNs = makeJupyter(host);`, and set `jupyter: jupyterNs` in the
    returned object.
  - **Delete** the now-dead `notYetImplementedError` helper and the
    `jupyterStub`'s `as unknown as QuartoAPI["jupyter"]` cast (it was the last
    remaining cast in the file).
  (Cite by symbol, not line number — `quarto-api.ts` is actively churning as
  Plan 2 lands.) Any per-call wrappers (e.g. supplying the per-execute `tempDir`
  to `resultIncludes`) can be composed around the factory output. (RTQ Item A
  removed the combined `EngineHostContext`: process-stable config is the ambient
  `Init` `global`, and per-render project context arrives on `launchEngine`.)
- [x] **State the namespace seam (P3-6).** The `quarto.jupyter` namespace
  (`quarto-types/src/quarto-api.ts`) is a **23-member** interface; this plan
  implements **7** with real bodies (`toMarkdown`, `isPercentScript`,
  `percentScriptToMarkdown`, `assets`, `resultIncludes`,
  `resultEngineDependencies`, `widgetDependencyIncludes`). A partial object
  **won't typecheck** against the full namespace type (guarded by test row 19).
  Handle the **16 remaining members by deriving each from the Q1 interface** —
  most are stubs, but not all:
  - **`notebookExtensions` is a REAL value, not a stub.** Q1 exposes it as
    `kJupyterNotebookExtensions = [".ipynb"]` (`jupyter.ts:191`), assigned
    directly. Implement it as `notebookExtensions: [".ipynb"]`. A
    `NotImplemented` *thrower* is a function, which is not assignable to the
    `string[]` property type → row 19 RED. This is the one non-function member,
    so the blanket "stub the rest" rule does not apply to it.
  - **The other 15 are `NotImplemented` throwers** (defer-with-seam, not drop;
    all are function-typed, so throwers satisfy the type). All are
    jupyter-built-in-only — q2's jupyter engine is native-Rust and marimo uses
    `system.pandoc`, so **no current q2 TS runtime consumer needs them**:
    - *Detection/introspection:* `isJupyterNotebook`, `kernelspecFromMarkdown`,
      `kernelspecForLanguage`, `fromJSON`
    - *Conversion:* `markdownFromNotebookFile`, `markdownFromNotebookJSON`,
      `quartoMdToJupyter`
    - *Processing:* `notebookFiltered`
    - *Runtime & Environment (the cluster the earlier draft elided with "…"):*
      `pythonExec`, `capabilities`, `capabilitiesMessage`, `capabilitiesJson`,
      `installationMessage`, `unactivatedEnvMessage`, `pythonInstallationMessage`
  Do **not** silently narrow the `JupyterNamespace` type instead — that's
  undiscussed and loses the contract.
- [x] `@quarto/api` is already a dependency of `@quarto/engine-host-deno`
  (it is consumed by `buildQuartoAPI`, landed in Plan 2 Phase A) — no new
  dependency needed. **But add the `./jupyter` subpath export** (see Phase 3A):
  without it, `import … from "@quarto/api/jupyter"` here won't resolve.

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

> **Prevalidation provenance.** This spec was prevalidated with
> `prevalidating-test-seams` (commit `8ee687e83`). Rows edited **after** that
> freeze were re-prevalidated 2026-07-01: **Row 3** (rebound to the to-markdown
> `{=tex}` emission path — the interim "the router emits `{=tex}`" wording was a
> mis-attribution: `displayDataWithMarkdownMath` only hoists *math* latex; the
> `{=tex}` for non-math latex is emitted downstream by `mdLatexOutput`) and
> **Row 19** (`createJupyter`→`makeJupyter` rename only — seam logic unchanged,
> still binds).

| # | Real unit · assertion surface | Mock boundary | Named revert → assertion RED · discriminator |
|---|---|---|---|
| 1 | `displayDataMimeType` — bundle `{text/markdown, text/html}`, opts `{toMarkdown:true}` ⇒ returns `'text/markdown'` | none (pure) | Revert dynamic base-order → fixed html-first list ⇒ returns `'text/html'`. **Disc:** bundle must hold *both* md+html (P3-9) |
| 2 | `displayDataIsJson` / `displayDataMimeType` — bundle w/ `…widget-view+json`, `{toHtml:true}` ⇒ widget MIME selected + `<script>` path | none | Revert the conditional widget-cluster splice ⇒ widget MIME never chosen (P3-9) |
| 3 | **to-markdown output path** for a display_data output whose sole data is a **non-math** `text/latex` ⇒ emitted cell markdown contains a `` ```{=tex} `` raw block (from the `mdLatexOutput`→`mdFormatOutput("tex")`-equivalent), **not** a math/markdown rendering. (`displayDataLatexIsMath` is the pivot predicate returning `false`; the `{=tex}` emission is **downstream**, **not** in `displayDataWithMarkdownMath` — that pre-transform only hoists *math* latex into the markdown slot and leaves non-math unchanged; P3-10.) | recording `host.fs` (unused on the latex path) | Revert `displayDataLatexIsMath`'s is-math test → `return true` ⇒ `displayDataWithMarkdownMath` hoists the non-math latex into the markdown slot ⇒ `displayDataMimeType` picks `text/markdown` ⇒ emitted as math, **no** `{=tex}` ⇒ RED. **Disc:** input latex must be **non-math** — a math latex is hoisted either way, so it can't discriminate (P3-10) |
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
| 17 | `percentScriptToMarkdown` (`percent-script`) — `# %% [markdown]\n# Hello` ⇒ a **markdown** cell (`# Hello`), not a code fence | recording `host.fs` | **Merge the `[markdown]` case into the code branch** ⇒ emitted as a `` ``` ``-fenced code cell ⇒ RED. (Impl-verified wording fix: a *literal* "delete the `[markdown]` branch" falls through to the **raw** path, which is byte-identical to markdown for a metadata-less cell and does **not** redden — the binding axis is markdown-vs-**code**, not markdown-vs-raw.) |
| 18 | `assets(host, input, to)` — returns snake_case 4-field; recorded `host.fs.ensureDir(figures_dir)` + `host.fs.walk(...)` | recording `host.fs` | Revert the `ensureDir` call ⇒ mock has no `ensureDir` RED. **(Missing-test add)** (P3-2) |
| 19 | **Contract-conformance (tsc)** — `makeJupyter(host)` is assignable to the full `quarto.jupyter` namespace type | n/a (compile-time) | Revert (delete) one `NotImplemented` stub ⇒ `tsc` build error RED (P3-6) |

- [x] Smoke/shape (not discriminators — the rows above carry the binding):
  convert a simple notebook JSON (2 code, 1 markdown) → markdown; and run a real
  `.ipynb` fixture through `toMarkdown` end-to-end (real temp dir) to catch gross
  integration breakage.

**Accepted-untested (logged, with rationale — not silently omitted):**
- **`pandoc` field population** — **not applicable.** The vendored Q1
  `jupyterToMarkdown` never populates `pandoc` (always `undefined`,
  `jupyter.ts:713-874`), so returning `undefined` is exact parity and there is no
  behavior to bind. The cross-cell accumulation is mainline-Quarto-only and out
  of scope (Phase 3C `pandoc?` note), not a deferred gap.
- **`executeOptions`/`figPos` fixup-profile selection** (P3-4) — the
  book/single-file/minimal fixup behavior is exercised only under project
  renders (no v1 caller); field *presence* is guarded by the vendored type + row
  19. Accepted-untested until the project renderer exists.
- **ANSI → HTML color on HTML output** (P3-16) — deliberately not ported
  (strip-only); the divergence is documented, not a regression to guard.

## Design Notes

### Simplified vs. Quarto 1

Key simplifications in our rewrite:

1. **No YAML schema validation** for cell options — just parse with the `yaml`
   package's `parse()` (already a runtime dep; **not** `js-yaml`/`load()` — see
   the cell-options item in Phase 3B)
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
   through the `PlatformHost` passed to `makeJupyter(host)`. The **six**
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

- [x] `@quarto/api/jupyter` populated with all **7 methods** (the 6 Julia
  execute-path calls + `widgetDependencyIncludes`), exposed via
  `makeJupyter(host)` factory
- [x] `widgetDependencyIncludes` is a **real host-bound body**, not a
  `NotImplemented` stub — returns `PandocIncludes` assignable to
  `DependenciesResult.includes`, so the RTQ FC-2 deferred-deps wire has its
  producer (unit-tested in isolation; book renderer lights it up later)
- [x] **Public types come from `@quarto/types`** — no redrafted
  `JupyterToMarkdownOptions`/`Result`/notebook types inside `jupyter/`
- [x] `makeJupyter(host)` typechecks against the full `quarto.jupyter`
  namespace type (remaining members are `NotImplemented` stubs — P3-6)
- [x] No `Deno.*` or `node:*` references inside `@quarto/api/jupyter`; `assets`,
  `resultIncludes`, and `widgetDependencyIncludes` take the host (not pure —
  P3-2/P3-3/P3-7)
- [x] `toMarkdown` is async, returns `cellOutputs: JupyterCellOutput[]` (not
  `string[]`), and round-trips through the Julia consumer (P3-1)
- [x] `toMarkdown` correctly converts notebooks with code, markdown, and raw cells
- [x] Image outputs write files to disk (via `host.fs.writeFileSync`) into
  `assets.figures_dir` and emit correct markdown references
- [x] HTML preservation ports `isPreservedHtml` as the constant-`false` no-op it
  is in Q1 today; no live restore mechanism is claimed (P3-15)
- [x] Error outputs format tracebacks readably
- [x] All tests pass (unit tests can pass a mock host with in-memory FS)
- [x] Integrated into `@quarto/engine-host-deno`'s QuartoAPI: `makeJupyter(host)`
  wired **inside `buildQuartoAPI(global, host)`** — replacing the throwing
  `jupyterStub` Proxy, with the `notYetImplementedError` helper + `as unknown as`
  cast deleted (Phase 3E). (There is no separate `buildJupyterNamespace`
  function, and it does **not** import `denoHost` directly.)

## Completion status (2026-07-01)

**COMPLETE** — executed via superpowers subagent-driven-development (one implementer
+ a spec+quality review per task; an Opus whole-branch review at the end).
All 13 tasks landed review-clean on `feature/ts-engine-extensions`; every frozen
Phase 3F row was RED-verified via its named revert then GREEN.

Commit range (Plan 3): `99c1fed2b..3fcade285`
- T1 `adbe1229a` ./jupyter export + constants (MIME, kLangCommentChars canonical)
- T2 `57483bd99` display-data (Rows 1,2 + latex predicates)
- T3 `921eed33b` tags (Rows 4,5)
- T4 `9e8e3daf0` labels + pandoc-id (Rows 6,7)
- T5 `dde5fa65f` cell-options (Row 8)
- T6 `899b9ff11` preserve (inert, faithful Q1)
- T7 `8db70b881` widgets — in-place strip + kebab/array `widgetDependencyIncludes` (Row 11)
- T8 `5c5d94927` **to-markdown core** (Rows 3,9,10,12,13,14,15) + `mdRawOutput`/`mdFormatOutput`
- T9 `0cc59eed1` percent-script (Rows 16,17)
- T10 `f9ec2bf5b` assets (Row 18)
- T11 `0099d45f4` result-helpers
- T12 `6da1d9597` `makeJupyter` factory (7 real + `notebookExtensions` value + 15 NotImplemented) + Row 19 conformance + smoke
- T13 `3fcade285` wire `makeJupyter` into `buildQuartoAPI`; **dropped the last cast** → cast-free `: QuartoAPI`

**Verification:** per-package `tsc --noEmit` clean (quarto-api, quarto-types [zero
edits], engine-host-deno cast-free); vitest green (quarto-api incl. 118 jupyter
tests; engine-host-deno); deno leg green (deno-host, wire-parity `--sloppy-imports`).
Full `cargo xtask verify` run at wrap-up (see session/branch).

**End-to-end status (honest):** there is **no CLI end-to-end caller for the jupyter
namespace yet — by design.** No v1 engine calls `quarto.jupyter.*` (the native
jupyter engine is Rust; Julia — Plan 4 — is the first real consumer, and RTQ FC-2's
deferred-deps path lights up only under the project/book renderer). Plan 3 is
verified by unit/integration tests against the real `jupyter/` modules (mock
`PlatformHost` only), the Row-19 tsc conformance gate, the engine-host wiring
compiling cast-free, and the `.ipynb` smoke through `makeJupyter` (in-memory
recording host, per the Phase 3F tier's "may use a real temp dir"). Full
`cargo xtask verify` exercises the whole workspace + hub build + the embedded
engine-host bundle, confirming the wiring breaks nothing downstream.

**Deferred follow-ups (surfaced to the user; neither blocks merge):**
1. **Helper consolidation.** `isDisplayData` is copied verbatim (zero drift) in
   `to-markdown.ts`/`labels.ts`/`widgets.ts`/`preserve.ts`; `isCaptionableData` in
   `to-markdown.ts`/`labels.ts`; `pandocAttrKeyvalueFromText` in `percent-script.ts`.
   Q1 exports these from `display-data.ts`/`markdownRegex`; each local copy carries a
   self-documenting GAP NOTE. Consolidate by exporting once + importing.
2. **Q1 upstream quirk** (do **not** "fix" here — faithful REWRITE parity):
   `to-markdown.ts` guards on `cap-location` but reads `fig-cap-location` for the
   `caption-<loc>` class — a byte-faithful port of Q1 `jupyter.ts:1408-1409`. Note
   upstream separately.
