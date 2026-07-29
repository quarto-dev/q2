# Mermaid render component for q2-preview

**Braid:** bd-c3dtpe36
**Status:** draft / awaiting go-ahead to execute

> **Security note:** the experiment files live in a quarto-hub demo
> playground project. That project's automerge index-document id is a
> *bearer write token* and must **never** be committed. This plan refers
> to the playground only by file paths (e.g. `cscheid/mermaid/mermaid.tsx`),
> never by id. Same rule for any braid comment/snapshot.

## Overview

We want `q2-preview` to render [mermaid.js](https://mermaid.js.org/)
diagrams. `q2-preview` lets a document extend rendering by listing React
"render components" in front-matter (`render-components:`); each exported
component *shadows* a built-in renderer for a given AST node type. This
plan prototypes mermaid support as a **user TSX render component** in the
demo playground, then folds the working approach into the **built-in**
preview renderer so every `q2-preview`/`q2 preview` document gets mermaid
for free.

The experiment answers three questions the eventual built-in must also
answer:

1. Can a render component run arbitrary imperative JS against a real DOM
   node inside the preview iframe? (Yes — via a `ref` + `useEffect`.)
2. Can that JS dynamically `import()` mermaid from a CDN inside the
   sandboxed iframe, or is it blocked by CSP / the iframe `sandbox`
   attribute? **(Primary risk — validate first.)**
3. Can we drive the diagram from the code block's source text and
   re-render cleanly on edits?

## How render components actually work (findings from the codebase)

Studied: `kan_ban.tsx`, `gordon/render-components2/{drag,comment}.tsx`
(playground), and `ts-packages/preview-renderer/src/q2-preview/`
(`entry.tsx`, `registry.ts`, `blocks/CodeBlock.tsx`, `PreviewRoot.tsx`).

- **Declaration.** Front-matter `render-components:` lists TSX paths
  (relative to the doc, or project-root-absolute with a leading `/`).
  `ReactRenderer.tsx` transpiles each file (`transpileTSX`) and posts the
  JS to the iframe via `LOAD_CUSTOM_COMPONENTS`.
- **Loading.** `entry.tsx#loadCustomComponents` wraps each transpiled
  module in a `Blob`, `URL.createObjectURL`s it, and `await import()`s the
  blob URL. So **dynamic `import()` already works inside the iframe** — of
  blob URLs at least. Whether an `https://` CDN import also works is the
  open question (see risks).
- **Override by export name.** A module's exports are merged over the
  built-in registry: `{ ...previewRegistry, ...customRegistry }`
  (`PreviewRoot.tsx:1424`). Export name = AST tag name. So
  `export const CodeBlock = …` **replaces the built-in `CodeBlock` for
  every code block in the document**, mermaid or not.
- **Node shape.** A component receives `NodeArgs = { node, setLocalAst,
  onNavigateToDocument, … }`. For a `CodeBlock`, `node.c === [[id,
  classes, kvs], codeText]`. A ```` ```mermaid ```` fence parses to a
  `CodeBlock` whose `classes` include `"mermaid"`. **No pipeline stage
  rewrites mermaid blocks in the preview path** (the Rust `mermaid`
  references are all `format: html` engine-execution detection, which
  q2-preview does not run) — confirmed by grep. So the code block arrives
  intact and the `CodeBlock` override is the correct, sufficient
  interception point. (The user framed this as "take over
  `<pre class=\"mermaid\">`"; in this architecture we intercept the *AST
  node* one step upstream of that `<pre>`, which is cleaner and matches
  how kanban/drag/comment work.)
- **Renderer surface.** `window.__Q2_PREVIEW_RENDERER__` exposes
  `renderChildren`, `renderNode`, `previewRegistry`, `usePreviewEdit`,
  meta/plain-text helpers, attribution hooks, etc. `window.React` and
  `window.katex` are materialized when custom components load. Crucially
  `previewRegistry.CodeBlock` is the **un-shadowed built-in**, so our
  override can delegate non-mermaid blocks to it.
- **Iframe sandbox.** `Q2PreviewIframe.tsx:439` →
  `sandbox="allow-scripts allow-same-origin"`. No CSP `<meta>` in
  `q2-preview.html`. So a CDN import is *probably* allowed, but must be
  confirmed empirically.

## Prior art in the playground

- `cscheid/mermaid/mermaid.tsx` — currently just a placeholder comment
  ("we'll be using this file…"). This is where the experiment component
  goes.
- `cscheid/mermaid/hand-written-test.qmd` — already wired:
  `format: q2-preview`, `render-components: [./mermaid.tsx]`, and a
  ```` ```mermaid ```` block (`flowchart LR / a -> b`). This is the
  end-to-end fixture.
- `cscheid/mermaid/tests/*` (the `.lua` files) — **ignore** (per user,
  not currently working).

## Design decisions

- **Interception:** `export const CodeBlock`. If `classes.includes(
  "mermaid")` → render a mermaid diagram; else delegate to
  `window.__Q2_PREVIEW_RENDERER__.previewRegistry.CodeBlock(args)` so
  ordinary/highlighted code blocks are unaffected.
- **Rendering API:** use `mermaid.render(id, code)` (returns an SVG
  string) + `dangerouslySetInnerHTML`, **not** `mermaid.run()` (which
  scans the DOM for `.mermaid` nodes and mutates them — fights React and
  risks double-processing). One unique id per diagram instance.
- **Load once:** cache the `import()` + `mermaid.initialize(
  { startOnLoad: false })` in a module-level promise so N diagrams share
  a single library load.
- **Re-render on edit:** the diagram effect is keyed on the code text;
  q2-preview re-renders the AST on every edit, so the effect re-runs and
  re-renders the SVG. Clean up / guard against races (stale async render
  writing into an unmounted node).
- **Errors:** invalid mermaid syntax throws; catch and show the error
  message plus the offending source instead of blanking the preview.
- **v1 is display-only:** no edit-back-to-source (`usePreviewEdit` not
  needed), no theming-to-match-document, no pan/zoom, no click events.

## Work items

### Phase 0 — arbitrary-JS escape hatch (proves imperative DOM access)

- [x] Write the `CodeBlock` override: for a `mermaid`-classed block,
      render a `<div ref>` + `useEffect` running arbitrary JS (write
      `textContent`, `console.log`); for any other block, delegate to
      `previewRegistry.CodeBlock`. Hooks live in a `MermaidDiagram`
      sub-component so the top-level `CodeBlock` stays hook-free.
- [x] **E2E verify (2026-07-17, live quarto-hub.com):** verified against
      `mermaid-lab.qmd` + `mermaid-lab.tsx` (root-level fixture — see
      finding below). Evidence:
      - Preview DOM contains `[data-mermaid-phase="0"]` whose text is
        `Phase 0 OK — arbitrary JS ran inside the preview iframe. …
        flowchart LR\n  a --> b` → the effect ran **and read the code
        block's source text**.
      - The sibling plain `python` block rendered via the built-in (one
        `<pre>` = `print("hello")`); the mermaid block was taken over
        (not in the `<pre>` list) → **delegation works**.
      - Console: `[Q2PreviewIframe] Loaded custom component:
        mermaid-lab.tsx` and `[mermaid-lab.tsx] Phase 0 effect ran.
        code = "flowchart LR\n  a --> b"`; no "not found" warning.

> **BLOCKER FOUND — render-component loader path-keying mismatch.**
> The first attempt (`/cscheid/mermaid/hand-written-test.qmd` +
> `./mermaid.tsx`) failed with `[ReactRenderer] Component file not
> found: ./mermaid.tsx`, even online with the `.tsx` content synced.
> Root cause (confirmed via the live React fiber): in this project
> `fileContents` is keyed **with a leading slash** for the `/cscheid/…`
> subtree (`/cscheid/mermaid/mermaid.tsx`) and `currentFilePath` is
> `/cscheid/mermaid/hand-written-test.qmd`, but
> `resolveComponentPath()`'s `normalize()` **always strips the leading
> slash**, producing the lookup key `cscheid/mermaid/mermaid.tsx` (no
> slash) → `fileContents.get(...)` misses. Root-level files
> (e.g. `kan_ban.tsx`) are keyed *without* a leading slash, which is why
> the existing kanban demo — and the `mermaid-lab.*` root fixture —
> resolve. Filed as a discovered-from bug (see strand). Until it's
> fixed, render components must live where their `fileContents` keys are
> slash-less (project root, or the `gordon/…` no-slash subtree).

### Phase 1 — load mermaid from CDN (primary-risk gate)

Collapsed Phase 1+2 into one component (Phase 0 already proved we can
read the code text, so there was no value in a hardcoded intermediate).

- [x] Effect does `await import(
      "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs")`,
      `initialize({ startOnLoad: false })`, then `mermaid.render(id, code)`
      and injects the returned SVG.
- [x] **PRIMARY RISK RESOLVED — CDN `import()` works in the sandboxed
      iframe.** No CSP/sandbox block. Console:
      `[mermaid-lab.tsx] mermaid loaded from CDN, …`; no import/CSP
      errors. (The iframe is `sandbox="allow-scripts allow-same-origin"`
      with no CSP meta; jsdelivr serves permissive CORS.)
- [x] **E2E verify (2026-07-17, live):** real SVG rendered — host
      `[data-mermaid-phase="1"]` contains an `<svg viewBox="0 0
      203.32 70">` with mermaid's own injected styles (`#mmd-1{font-
      family:"trebuchet ms"…}`). Plain `python` block still via built-in
      (`<pre>print("hello")`). Screenshot:
      `claude-notes/plans/assets/2026-07-17-mermaid-phase1.png`.

### Phase 2 — content-driven + robustness

- [x] `mermaid.render` is driven by `node.c[1]` (the fenced source);
      effect keyed on `code`; unique `mmd-<seq>` id per render; stale
      results guarded by a `cancelled` flag; module-level load-once cache.
- [x] try/catch renders an error box (message + source) instead of
      blanking.
- [x] **E2E verified (2026-07-17, live):** both robustness paths, on the
      real fixture `cscheid/mermaid/hand-written-test.qmd`:
      - Error path: the fixture's `flowchart LR / a -> b` (invalid
        single-dash arrow) rendered the error box — *"Mermaid error:
        Parse error on line 2 … got 'MINUS'"* (console:
        `[mermaid.tsx] render failed`).
      - Live-edit / happy path: editing the source to `a --> b` **via
        MCP (no page reload)** flowed through `astJson` → the component
        re-rendered → the error box disappeared and a real
        `<svg viewBox="0 0 203.32 70">` with nodes `a`/`b` appeared.

### Path-keying workaround applied (2026-07-17)

Per the user, the two experiment files were **moved** off the
leading-slash keys so `resolveComponentPath` resolves them:
`/cscheid/mermaid/hand-written-test.qmd` → `cscheid/mermaid/…` and
`/cscheid/mermaid/mermaid.tsx` → `cscheid/mermaid/mermaid.tsx`. Console
then showed `Loaded custom component: ./mermaid.tsx` (no "not found").
The root-level `mermaid-lab.*` diagnostic fixture was deleted. Note: a
duplicate `cscheid` folder appears in the file tree while other
`/cscheid/…` (leading-slash) files remain — expected, harmless, and it
resolves when the loader bug is fixed. The real fix stays tracked on the
discovered-from bug strand.

### Phase 3 — decide & scope productionization (built-in support)

- [ ] Write up findings (CDN vs bundled mermaid; `render` vs `run`;
      re-render behavior; theming needs) at the end of this plan.
- [ ] Decide the built-in shape: a mermaid-aware branch in the built-in
      `blocks/CodeBlock.tsx`, or a dedicated registry entry. **Bundle**
      mermaid (no CDN) for the built-in, per the External Sources Policy
      spirit and offline/repro builds.
- [ ] File follow-up braid strand(s) for the built-in work with
      `discovered-from:bd-c3dtpe36`, including **vitest** coverage in
      `ts-packages/preview-renderer` (unit-test the mermaid-vs-plain
      branch + delegation; the browser experiment is not an automated
      test).

## Testing / verification note (TDD honesty)

The Phase 0–2 experiment lives as user TSX in the playground, not in the
repo, so it is verified by **browser end-to-end inspection**, not
automated tests — each phase's checklist ends with an explicit "E2E
verify + record observation" step (per CLAUDE.md's end-to-end rule).
Automated (vitest) tests come with Phase 3 productionization, where the
code lands in `ts-packages/preview-renderer` and TDD applies normally
(write the mermaid-branch test first, watch it fail, implement).

## Risks / open questions

1. **CDN import blocked by sandbox/CSP (primary).** Mitigations, in
   order: try a different CDN; inject a `<script type="module">` into the
   iframe head instead of `import()`; ship mermaid bundled with the
   built-in renderer (the eventual production answer anyway).
2. **`mermaid.render` id collisions / re-entrancy** across multiple
   diagrams and rapid edits — needs unique ids and stale-write guards.
3. **Theming:** mermaid's default theme won't match the document's
   Bootstrap theme. Out of scope for the experiment; note for built-in.
4. **Highlight interaction:** the built-in `CodeBlock` special-cases
   `data-hl-spans`. A `mermaid` block shouldn't get highlight spans, but
   confirm our class check runs before any highlight path.
