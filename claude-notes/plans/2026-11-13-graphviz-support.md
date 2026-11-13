---
title: 'Closing the Graphviz gap in Quarto 2 (bd-ijlapt70)'
date: 2026-11-13
description: 'Starting notes and a high-level plan for Graphviz (dot) diagram support, built as a rendering transform in parallel to Mermaid. Not yet ratified.'
status: in-progress  # preview component and render transform built for the HTML family; the decisions in "Decisions to ratify" still need team ratification
braid:
  strand: bd-ijlapt70
  priority: P2
  labels: [diagrams, graphviz]
---

## How to use this file

This file is the starting context for a new working session. It records what was
found while building the Quarto 1 to Quarto 2 feature checklist, what Quarto 1
does for Graphviz, how the Mermaid work in q2 is built (Graphviz sits in parallel
to it), and a proposed plan.

Three kinds of statement appear here. They are labelled where it matters:

- **Verified**: I ran it or read the file on 2026-11-10 to 2026-11-13.
- **Read, not run**: taken from source or a plan document without executing it.
- **Proposed**: a recommendation that the q2 team has not ratified.

Nothing in the plan section has been agreed with the q2 team. The first real task
of the new session is to get the two design decisions in
[Decisions to ratify](#decisions-to-ratify) settled.

## Why this exists

The one-page management checklist (`~/Desktop/Projects/q2-checklist/checklist.qmd`,
rendered to `checklist.pdf`) has a row "Graphviz (dot) diagrams". It is the only
row marked "not yet" in every format, including HTML, where q2 is otherwise at
91%. Mermaid, its sibling row, is done in HTML, RevealJS and Dashboard.

Closing Graphviz for the HTML family would turn three cells green. Closing it for
Typst would turn a fourth, and that needs a different technique (see
[Where Graphviz differs from Mermaid](#where-graphviz-differs-from-mermaid)).

When this work lands, update the Graphviz row in `checklist.qmd` (the nine codes
are in the order HTML, Dashboard, RevealJS, Typst, PDF, Beamer, Docx, Pptx, GFM).

## Current state in q2 (verified)

Tested with `q2` 0.45.0, built from `origin/main` at `936c29e13` (2026-11-13),
in this strand's worktree. The behaviour is the same as on 0.43.0 (`929891fa5`,
2026-11-10), where it was first recorded.

| Input | What q2 does today |
|---|---|
| ```` ```{dot} ```` (Quarto 1 cell form) | Warning `Q-2-58`, then a plain code block whose class is the literal `{dot}`. No diagram. |
| ```` ```dot ```` (plain fence) | A syntax-highlighted code block (`<pre class="sourceCode dot">`). No diagram, no warning. |

The same result (source printed as text) was seen in HTML, RevealJS, Dashboard
and Typst on the older installed 0.40.0 binary.

The `Q-2-58` message says, word for word: "Quarto 2 does not run `{dot}` cells
and has no Graphviz support yet, so this block renders as a code block, not a
diagram."

Relevant places in the q2 tree (line numbers as of `936c29e13`):

- `crates/quarto-core/src/transforms/brace_diagram_cell_diagnostics.rs`: the
  read-only transform that emits `Q-2-58` for `{mermaid}` and `{dot}` cells
  (GH issue 1067). Wired in at `crates/quarto-core/src/pipeline.rs:2392`.
- `crates/quarto-error-catalog/error_catalog.json`: the `Q-2-58` entry.
- `docs/guides/authoring/diagrams.qmd` (around line 263): the user docs list
  "Graphviz (`{dot}`) diagrams" under "not yet available", next to sizing
  options, pre-rendered PNG/SVG for print formats, and theming.
- `crates/quarto-core/src/cell_options/mod.rs:109`: `dot` is already in the
  comment-marker table with `//`, so `//|` cell options should parse for a
  `dot` block without new work. **Read, not run.**
- `crates/quarto-core/src/engine/mod.rs:201`:
  `HANDLED_LANGUAGES = ["ojs", "mermaid", "dot"]` still exists and a test pins
  it. The tombstone note in the plan below says the constant is gone from
  `main`; that note is out of date on this point.

### Prior q2 thinking on Graphviz

`claude-notes/plans/2026-07-02-plan8-mermaid-absorption-graphviz-ts-extension.md`
proposed Graphviz as a TypeScript engine extension. It was **tombstoned on
2026-07-24**. The banner states the decision on `main`: diagrams are not engines,
and "`dot`, like mermaid, belongs on the transform path, not an engine claim".

So the direction is already set by the team: Graphviz should be a rendering
transform, built the way Mermaid was. This plan follows that.

## What Quarto 1 does

### Implementation (read, not run)

`~/Desktop/Projects/quarto-cli/src/core/handlers/dot.ts` (file header says
`graphviz.ts`). It is a language cell handler, stage `post-engine`, for the
language name `dot`.

- **Layout happens at render time, not in the browser.** The handler imports
  `src/resources/js/graphviz-wasm.js` and calls
  `graphviz().layout(source, "svg", options["graph-layout"])`. The wasm binary is
  a separate file, `share/wasm/graphvizlib.wasm`. The JS file is a hand-edited
  copy of the `hpcc-systems/hpcc-js-wasm` build, adapted to run on Deno.
- **Defaults**: `echo: false`, `eval: true`, `include: true`,
  `graph-layout: "dot"`. Comment marker `//`, so options are written `//|`.
- **HTML-family output** (anything "JavaScript compatible", except ipynb): the
  SVG is inlined as a raw HTML block. Sizing goes through `core/svg.ts`:
  responsive by default, or fixed when `fig-width` / `fig-height` are set.
  `fig-alt` is written into the SVG (`setSvgAlt`). In RevealJS, `fig-align` is
  applied to the SVG.
- **Every other format** (PDF, Typst, Docx and so on): the SVG is turned into a
  PNG with headless Chrome (`createPngsFromHtml`, 4x device scale) and emitted
  as a markdown image with `width`, `height`, optional `fig-pos`, `fig-env`,
  label, caption and alt text. Typst gets a small height offset (0.1in).
- **Errors**: a Graphviz "syntax error in line N" is rewritten to point at the
  line in the source `.qmd` file.

### Documented contract

`~/Desktop/Projects/quarto-web/docs/authoring/diagrams.qmd` (my local copy of
quarto-web is from 2025-11-13, so it may be a little behind):

- Graphviz diagrams are `{dot}` executable cells.
- Cell options use `//|` and must sit directly under the fence line.
- `fig-cap` and `label` make the diagram a cross-referenceable figure.
- `file: path/to/diagram.dot` reads the source from a separate file.
- Sizing: natural size by default, responsive in HTML;
  `fig-responsive: false`, `fig-width`, `fig-height` override.
- Code is hidden by default; `echo: true` shows it.

### Quarto 1 tests worth porting as fixtures

All under `~/Desktop/Projects/quarto-cli/tests/docs/smoke-all/`:

- `2023/02/13/dot-bug.qmd`
- `2023/07/10/6153.qmd`
- `2023/09/30/_pdf-marginfloats.qmd`
- `2023/12/05/knitr-handled-language-cell-options.qmd`
- `2026/02/12/dot-pdf.qmd`
- `2026/10/15/diagram-fig-alt.qmd`
- `2026/10/15/diagram-fig-alt-escaping.qmd`

The Q1 feature matrix has no Graphviz entry at all, and its Mermaid entry is
unrated, so these smoke tests are the best statement of the contract.

## How Mermaid is built in q2 (the pattern to follow)

Mermaid landed in stages. Each stage has a plan document in
`claude-notes/plans/` that is worth reading before writing the Graphviz one:

1. `2026-07-20-mermaid-regular-rendering.md`: the core design. Read this first.
2. `2026-08-10-mermaid-cell-options.md`: `%%|` options, captions, alt text.
3. `2026-08-11-mermaid-runtime-not-bundled.md`: moving from CDN to a vendored
   runtime.

The pattern, piece by piece:

**Syntax.** q2 standardised on the plain GFM fence ```` ```mermaid ````, not Q1's
```` ```{mermaid} ````. The brace form is deliberately not recognised, and
`Q-2-58` tells the author to drop the braces. The reasons given: the plain fence
needs no engine, parses the same in native and wasm, matches GitHub and GitLab,
and degrades to a code block on any surface that does not know about it.

**Not an engine.** No `engine:` declaration, no capture and replay. It is an AST
transform.

**The render transform** is `crates/quarto-core/src/transforms/mermaid.rs`
(`MermaidRenderTransform`):

- Phase `TransformPhase::Finalization`, placed immediately before
  `CodeBlockRenderTransform` so diagram blocks never get copy-button or
  filename chrome, and never reach syntax highlighting.
- Self-gated to HTML-family formats (`ctx.format.identifier.is_html_based()`).
  For anything else it does nothing.
- Replaces each `CodeBlock` with class `mermaid` by a raw HTML block,
  `<pre class="mermaid">` with the escaped source. It walks into block quotes,
  divs, figures and lists.
- When at least one diagram is found, it registers the runtime and appends one
  loader script to `rendered.includes.after-body`, guarded by a sentinel comment
  so a re-run does not duplicate it (`append_with_sentinel`).

**The runtime is vendored, not fetched from a CDN.** The bytes live in
`resources/mermaid/mermaid.min.js` with a `README.md` and `LICENSE`, are embedded
with `include_str!`, and are registered as an `ArtifactScope::Project` artifact
at `mermaid/mermaid.min.js`. A website gets one copy under `site_libs/`. Three
details are load-bearing and guarded by tests:

- The artifact key is `mermaid:runtime`, deliberately not a `js:` key. A `js:`
  key would add a second `<script>` in the Bootstrap `<head>` and would be
  ignored by the RevealJS scaffold, which only collects `js:revealjs:*`.
- The file must be the self-contained build, not the chunk-loading ESM build.
- The version constant (`MERMAID_VERSION`) must match the vendored file, and the
  React preview component pins the same version.

**Preview and hub.** The transform is excluded from the q2-preview pipeline
(`Q2_PREVIEW_TRANSFORM_EXCLUDED` in `pipeline.rs`), so the raw `CodeBlock`
reaches the React layer. There, a built-in component renders it:
`ts-packages/preview-renderer/src/q2-preview/blocks/MermaidCodeBlock.tsx`, with
tests beside it. This is what makes diagrams work in `q2 preview` and in
QuartoHub without running any engine.

**Cell options.** Handled by `PreEngineSugaringStage`, which is a stage, not a
transform, so it runs for the preview too. It was made language-aware so that
mermaid's `%%|` marker works. `fig-cap` and `label` wrap the diagram in a Figure,
and `fig-alt` is folded into the diagram's accessible description. A `pipeline.rs`
test (`render_qmd_to_preview_ast_processes_mermaid_cell_options`) pins that
render and preview agree. `fig-align` on a bare diagram becomes a
`quarto-figure-<align>` class.

**Tests.** Unit tests inside `mermaid.rs`; smoke-all fixtures in
`crates/quarto/tests/smoke-all/mermaid/` (`basic.qmd`, `revealjs.qmd`,
`fig-align.qmd`, `fig-align-revealjs.qmd`, `no-mermaid.qmd`); RevealJS scaffold
include tests in `revealjs/assemble.rs`; pipeline wiring tests in `pipeline.rs`;
vitest tests for the React component.

**Process.** Plan ratified first, then tests first, then the native render
transform, then the React component, then end-to-end checks in a browser with
evidence written into the plan, then the docs page and a hub-client changelog
entry. The team's porting guide is
`claude-notes/instructions/q1-feature-porting.md`. It asks for three studies
before any plan (Q1 docs, Q1 implementation, q2 current state) and allows
deliberate differences from Q1 only for stricter diagnostics or explicit
declaration.

## Where Graphviz differs from Mermaid

This is the one place the Mermaid pattern does not carry over cleanly, and it
drives the main design decision.

Mermaid needs a browser DOM to draw. That is why q2 renders it client-side and
why it only works in HTML-family output. Graphviz does not need a DOM. Its layout
engine takes text and returns an SVG string, and Quarto 1 does this at render
time and ships a static SVG.

That gives Graphviz options Mermaid never had:

| Approach | What it is | For | Against |
|---|---|---|---|
| A. Client-side, like Mermaid | Vendor a self-contained Graphviz wasm build for the browser. Emit a placeholder element plus a loader script. React component for preview. | Same shape as Mermaid, so most of the code and tests have a direct model. The browser asset is needed for preview and hub in any case. | Not what Q1 emits (Q1 ships a finished SVG). Needs JavaScript in the reader's browser. Does nothing for Typst or other non-HTML formats. |
| B. Render-time SVG inside q2 | Run a Graphviz wasm module from Rust during render and inline the SVG. | Matches Q1 output. One implementation serves HTML and Typst (Typst can embed SVG). No JavaScript needed to view. | More work. The hub build of q2 is itself wasm and cannot host a second wasm runtime the same way, so preview still needs approach A's component. Whether the hpcc-js wasm build loads outside a JS host is **not verified**. |
| C. Call a system `dot` binary | Shell out when Graphviz is installed. | Small. | Adds an install requirement Q1 does not have. No good for hub. |
| D. Pure-Rust layout library | Use a Rust crate that implements dot layout. | No wasm, no external binary. | Fidelity against real Graphviz is unknown. **Not investigated.** |

Two facts that bear on B. **Read, not run**: `wasmtime` is already a native
dependency of q2 through `quarto-highlight` (user grammar loading), and `usvg` is
already in `Cargo.lock`. Neither has been tried for this purpose.

## Proposed plan

**Proposed, not ratified.** The shape mirrors the Mermaid plan's phases.

### Scope of the first cut

- Formats: HTML, RevealJS, Dashboard for `q2 render`; `q2 preview` and QuartoHub.
- Approach A (client-side), because it is the direct parallel to Mermaid and its
  browser asset is needed for preview whichever way render goes. Chosen by
  Andrew on 2026-11-13; see [Decisions to ratify](#decisions-to-ratify).
- Cell options that Mermaid already supports: `fig-cap`, `label`, `fig-alt`,
  `fig-align`. Plus `graph-layout`, which is specific to Graphviz and simple to
  pass through.

Out of the first cut, tracked as follow-ups, not dropped silently:

- Typst and other non-HTML formats (needs approach B or C). Tracked as
  `bd-atz1c25y`.
- `file:` to read source from a `.dot` file.
- `echo: true`.
- `fig-width`, `fig-height`, `fig-responsive`. Mermaid does not have these in q2
  yet either, so they are best done once for both.

### Phase 0: study and ratify

- [ ] Read the three Mermaid plans and `q1-feature-porting.md`.
- [ ] Finish the Q1 study: read the seven Q1 fixtures listed above and
      `core/svg.ts` (sizing and alt text), and refresh quarto-web for the
      current docs.
- [x] Check the q2 issue tracker and braid for an existing Graphviz strand.
      Done 2026-11-13: none existed. The only GitHub issue is the closed 1067
      (`Q-2-58`), and the related braid strands are `bd-p7ionkmv` (closed,
      `Q-2-58`) and `bd-7u2xu4gp` (open, brace-form warning). This work is now
      tracked as `bd-ijlapt70`.
- [x] Spike: confirm that a current Graphviz wasm build ships as one
      self-contained browser file, and record its size and licence. Done
      2026-11-13: yes, `@viz-js/viz` `dist/viz-global.js`. See
      [Spike: self-contained browser build](#spike-self-contained-browser-build).
- [ ] Write the q2 plan document in `claude-notes/plans/` in the house format.
- [ ] Get the decisions below ratified by the q2 team (Carlos is `cscheid` on
      GitHub).

### Phase 1: tests first, native render

- [x] Unit tests for the new transform, modelled one-for-one on the tests in
      `mermaid.rs`: block becomes the placeholder, script appended once, re-run
      does not duplicate, nothing happens without a diagram, nothing happens for
      a non-HTML format, nested blocks convert, source is escaped, runtime is
      registered only when needed and is not keyed as `js:`.
- [x] Vendored-asset guard tests: version matches the pin, bundle is
      self-contained, bundle matches the published package.
- [x] smoke-all fixtures in a new `crates/quarto/tests/smoke-all/graphviz/`
      directory: `basic`, `revealjs`, `cell-options`, `no-graphviz`. The Q1
      fixtures are **not ported yet**; that is still open.
- [x] Pipeline wiring tests (`graphviz_render_present_before_code_block_render`
      for html, revealjs and dashboard; `q2_preview_pipeline_excludes_graphviz_render`).
      No separate RevealJS scaffold test was added: the scaffold's after-body
      slot is already covered by the existing tests in `revealjs/assemble.rs`,
      and the `revealjs` smoke fixture checks the script reaches a deck.

### Phase 2: implement native render

- [x] `crates/quarto-core/src/transforms/graphviz.rs`, a
      `GraphvizRenderTransform` in the Finalization phase, before
      `CodeBlockRenderTransform`, gated to HTML-family formats.
- [x] Vendor the Graphviz browser runtime under `resources/graphviz/` with a
      `README.md` and `LICENSE`, embedded and registered as a Project-scoped
      artifact under a non-`js:` key.
- [x] Loader script through `rendered.includes.after-body` with a sentinel.
- [x] Pass `graph-layout` from the cell options to the layout call.
- [x] Surface Graphviz syntax errors usefully. In approach A the error happens
      in the browser. The page shows Graphviz's message above the source and
      marks the block `graphviz-error`. The render itself cannot warn.

### Phase 3: preview component

- [x] `GraphvizCodeBlock.tsx` beside `MermaidCodeBlock.tsx`, with vitest tests
      first, pinned to the same runtime version as the Rust constant. **Done as
      a prototype on 2026-11-13**, ahead of Phases 1 and 2 and before team
      ratification, so that there is something to try in QuartoHub. See
      [Prototype: preview component](#prototype-preview-component). The Rust
      constant does not exist yet; the test pins `3.31.0` on the TypeScript
      side only.
- [x] Add the transform to `Q2_PREVIEW_TRANSFORM_EXCLUDED`.
- [x] A `pipeline.rs` test that render and preview agree on cell-option
      handling, like the Mermaid one
      (`render_qmd_to_preview_ast_processes_graphviz_cell_options`).

### Phase 4: end-to-end evidence

- [x] `q2 render` to HTML, RevealJS and Dashboard, opened in a browser, with
      the diagram actually drawn. See
      [Render transform](#render-transform-phases-1-and-2).
- [ ] `q2 preview` for a document and for slides. Not done. Only the hub was
      opened.
- [x] Offline check: the page draws with the network disabled.
- [ ] Firefox and Safari.

### Phase 5: diagnostics, docs, close-out

- [x] Update `Q-2-58`: its text says q2 "has no Graphviz support yet". Change
      the message and the `brace_diagram_cell_diagnostics.rs` wording so `{dot}`
      gets the same "drop the braces" advice as `{mermaid}`.
- [x] Update `docs/guides/authoring/diagrams.qmd`: add a Graphviz section and
      remove it from the "not yet available" list. Also updated the error
      pages `Q-2-47`, `Q-2-48` and `Q-2-58`. The docs site itself was not
      built (it needs the generated `docs/examples`); the four pages were
      rendered on their own to check they parse. There is no embedded live
      example for Graphviz yet, as there is for Mermaid.
- [ ] hub-client changelog entry.
- [ ] `cargo xtask verify`.
- [ ] Update the Graphviz row in `~/Desktop/Projects/q2-checklist/checklist.qmd`.

### Later phase: Typst and other formats

Tracked as `bd-atz1c25y`, which waits for `bd-ijlapt70`.

A spike to choose between approach B and approach C, then a render-time path
that writes an SVG file and emits an image. This is also the route to Mermaid in
Typst, so it may be worth designing for both at once.

The first cut must not make this harder. Three constraints follow:

- The fence spelling and the cell options (`fig-cap`, `label`, `fig-alt`,
  `graph-layout`) mean the same thing in every format, so this path reads the
  same source.
- This path pins the same Graphviz version as the browser runtime, so a diagram
  lays out the same in HTML and in Typst.
- Once this path exists, `q2 render` to HTML could inline the SVG as well (no
  JavaScript for the reader, and it matches Q1). That choice stays open. Preview
  and QuartoHub keep the browser component whichever way it goes.

## Spike: self-contained browser build

**Verified** on 2026-11-13. Question: does a current Graphviz wasm build ship as
one self-contained file that fits the Mermaid vendoring rule (a classic script
that sets a global, embedded with `include_str!`, loaded with a plain
`<script src>`)?

**Answer: yes. Recommended file: `dist/viz-global.js` from `@viz-js/viz`
3.31.0.**

Two packages were compared. Both embed the wasm binary inside the JavaScript
file and both build Graphviz 16.1.0.

| | `@viz-js/viz` 3.31.0 | `@hpcc-js/wasm-graphviz` 1.30.0 |
|---|---|---|
| File | `dist/viz-global.js` | `dist/index.js` |
| Kind | Classic script, sets `globalThis.Viz` | ES module only (`export {...}`) |
| Size | 1,329,882 bytes; 481,656 gzipped | 820,107 bytes; 636,665 gzipped |
| Wrapper licence | MIT | Apache-2.0 |
| Graphviz | 16.1.0 | 16.1.0 |
| Layout engines | dot, neato, fdp, sfdp, circo, twopi, osage, patchwork, nop | same set |
| Published | 2026-09-28, about monthly releases, npm provenance attestation | 2026-10-21 |
| Used by Q1 | no | yes (a 2022 copy, adapted for Deno) |

Why `viz-global.js` and not the hpcc file:

- The hpcc package ships only an ES module. A module script does not load from
  a `file://` page, so a rendered `.html` opened from disk would show no
  diagram. Using it would mean building our own classic bundle, which breaks
  the "copied verbatim from the npm tarball" rule.
- `viz-global.js` is a classic script, like `mermaid.min.js`, so the transform,
  the loader and the guard tests carry over directly.
- It is smaller on the wire (the hpcc file is already compressed inside, so
  gzip gains little).

Evidence:

- Static scan of `viz-global.js`: no `import(`, no `import.meta`, no `.wasm`
  reference, no `eval`. It contains one `fetch(` and one `require(`; both are
  Emscripten fallback paths and neither ran in the tests below.
- Browser: a page with `<script src="viz-global.js">` and an inline init,
  opened from `file://` in headless Chrome 157 with all host name lookups
  disabled (`--host-resolver-rules="MAP * ~NOTFOUND"`). Result: two diagrams
  drawn as inline `<svg>` (one `dot`, one `neato`), about 20 ms after the
  script loaded, no network resource requested.
- Node, with `fetch` and `XMLHttpRequest` replaced by stubs that throw: the hpcc
  module and the ESM build of Viz (`dist/viz.js`) both lay out a graph with no
  network attempt.
- Same output: for one test graph (boxes, a cluster, an edge label) the two
  packages return the same SVG, byte for byte, except for the build stamp in
  the "Generated by graphviz" comment.
- Syntax errors come back as data, not a thrown exception:
  `viz.render(src)` returns `status: "failure"` with
  `errors: [{level: "error", message: "syntax error in line 1 near '}'"}]`.
  The line number is relative to the diagram source, as in Q1.

What this settles for the plan:

- Vendor `dist/viz-global.js` under `resources/graphviz/`, pinned to 3.31.0.
  SHA-256:
  `c9e0b310f9883910e01c66054b48b7ff4be3d8695141116635e72b0af152692e`.
- The version guard can match the `Viz.js 3.31.0` line in the file's header
  comment, and `graphvizVersion="16.1.0"` for the Graphviz version.
- The self-containment guard cannot be a blanket "no `fetch(`", because of the
  Emscripten fallback. Check for no `import(` and no `.wasm` reference.
- `graph-layout` maps to the `engine` option of `viz.render`.
- The preview component can report a syntax error with its line, because the
  error is structured.
- The later SVG path (`bd-atz1c25y`) can use either package and still match the
  browser output, as long as the Graphviz version is the same.

Not settled:

- **Licence text.** The npm tarball of `@viz-js/viz` has no `LICENSE` file; the
  MIT text has to come from the `mdaines/viz-js` repository. The file also
  contains Graphviz and Expat in object-code form. Graphviz is under the
  Eclipse Public License, and Q1 already ships a Graphviz wasm build, so there
  is precedent, but the team should confirm what notice q2 needs to carry.
- **Other browsers.** Only Chrome was tested. Firefox and Safari are part of
  Phase 4.
- **Preview.** `MermaidCodeBlock.tsx` imports its runtime from jsDelivr, so
  Mermaid in preview does not work offline today. Whether the Graphviz
  component should copy that or bundle `dist/viz.js` (no dependencies, 1.2 MB)
  is a design choice for Phase 3.

## Prototype: preview component

**Verified** on 2026-11-13. A working preview component, built early so the
feature can be tried in QuartoHub. It is a prototype: the design choices below
are open to change when the plan is ratified.

What exists:

- `ts-packages/preview-renderer/src/q2-preview/blocks/GraphvizCodeBlock.tsx`.
  Draws a `CodeBlock` with class `dot`. The brace form `{dot}` is not matched.
- `blocks/DiagramCodeBlock.tsx`. A dispatcher, now the registry's `CodeBlock`
  entry: `dot` goes to Graphviz, everything else to `MermaidCodeBlock`. Two
  assertions in `MermaidCodeBlock.test.tsx` changed to match.
- `@viz-js/viz` `3.31.0` as an exact-pinned dependency of
  `@quarto/preview-renderer`, loaded with a dynamic `import()`. The bundler
  splits it into its own lazy chunk (`viz-*.js`), so it is served with the app
  and needs no CDN. This differs from `MermaidCodeBlock`, which imports from
  jsDelivr.
- `graph-layout` and `fig-align` are read from the block's attributes. An
  unknown engine name falls back to `dot`.
- A Graphviz syntax error shows an error box with the message and the source,
  like the mermaid one.

Design choices to review:

- **SVG sanitising.** Graphviz copies `URL`/`href` attributes and HTML-like
  labels from the source into its SVG, and hub projects are shared between
  authors. `sanitizeGraphvizSvg` parses the SVG (it is never assigned to
  `innerHTML`), removes `<script>` and `<foreignObject>`, removes `on*`
  attributes, and keeps links only when they are `http(s):`, `mailto:` or a
  `#fragment`. This is a minimal hand-written filter. The team should decide
  whether it is enough or whether to use the sanitiser mermaid relies on.
- **Bundled, not CDN.** See above. It adds a 1.2 MB lazy chunk to the hub
  client and the sandboxed preview.
- **Dispatcher.** The alternative was to chain the components. A dispatcher
  keeps each renderer unaware of the other.
- **Sizing.** The SVG gets `max-width: 100%` and `height: auto`, so it keeps
  its natural size and shrinks to the column. There is no `fig-width` or
  `fig-height` handling.

Not done in the prototype:

- Cell options. `//| fig-cap`, `label` and `fig-alt` are not handled by the
  component, and whether `PreEngineSugaringStage` parses `//|` lines for a
  `dot` block in preview was **not checked**.
- `q2 render`. There is no Rust transform, so rendered HTML still shows the
  source as a code block.
- `q2 preview` and slides were not opened. Only the hub was.
- Firefox and Safari.
- hub-client changelog entry. No file under `hub-client/` changed.

End-to-end evidence (hub, Aside browser, Chromium-based):

- Stack: `cargo build --bin hub`, `cd hub-client && npm run build:local-prod`,
  `npm run local-prod`, then `http://127.0.0.1:8080`.
- A new project from the Default template, with `index.qmd` holding four
  blocks: a plain `dot` digraph, a `{.dot graph-layout="neato"}` graph, a
  `dot` block with a syntax error, and a `mermaid` flowchart.
- Observed in the preview iframe's DOM (inspected, not only inferred from the
  absence of errors):
  - two `[data-graphviz-diagram] svg` elements: 335 by 113 px with 3 nodes
    and 3 edges, and 205 by 176 px with 4 nodes and 5 edges;
  - one `[data-graphviz-error]` box with the message
    `Graphviz error: syntax error in line 1 near '}'` and the source under it;
  - one `[data-mermaid-diagram] svg`.
- The check was on the DOM. A screenshot was taken but not kept, so the visual
  result was judged by Andrew in the browser ("working quite well").

Build and test results:

| Step | Result |
|---|---|
| `npm test -w ts-packages/preview-renderer` | 679 passed, 40 skipped |
| `npm run build -w ts-packages/preview-renderer` | passed |
| `hub-client`: `npx tsc -b` | passed |
| `hub-client`: `vite build` | passed, `viz-*.js` chunk emitted |
| `check-sw-precache` | ok |
| `hub-client`: `npm run test` | 1801 passed |
| `hub-client`: `npm run test:integration` | 241 passed |
| `hub-client`: `npm run test:wasm` | 452 passed, 28 skipped |

`npm run build:all` was not run as one command. Its steps were run
separately, and the `vite build` step wrote to a temporary directory, so the
running local hub kept its local-prod build. The wasm and sandboxed-preview
steps had already run as part of `build:local-prod`. `cargo xtask verify` was
not run; no Rust source changed.

## Render transform (Phases 1 and 2)

**Verified** on 2026-11-13. `q2 render` now draws ```` ```dot ```` blocks in
HTML, RevealJS and Dashboard output. Like the preview prototype, this was built
before team ratification.

What exists:

- `crates/quarto-core/src/transforms/graphviz.rs`: `GraphvizRenderTransform`
  (`graphviz-render`), Finalization phase, placed after `mermaid-render` and
  before `code-block-render`, self-gated to HTML-family formats. A `dot`
  `CodeBlock` becomes `<pre class="graphviz">` with the escaped source.
- `resources/graphviz/`: `viz-global.js` (verbatim from the npm tarball),
  `LICENSE`, `README.md`. Registered in `resources/vendored.toml`. Artifact
  key `graphviz:runtime`, path `graphviz/viz-global.js`, Project scope.
- A loader plus an inline init in `rendered.includes.after-body`. The init
  lays out every `pre.graphviz` and swaps the text for the inline SVG. The
  `<pre>` stays as the container, as mermaid.js keeps `pre.mermaid`.
- Cell options for `dot` blocks, written `//|`, in
  `crossref/codeblock_shorthand.rs`: `label`, `fig-cap`, `fig-scap`,
  `fig-alt`, `fig-align`, and `graph-layout`. `dot` is now a "diagram
  language" there, beside mermaid.
- `Q-2-58` tells `{dot}` authors to drop the braces. `Q-2-47` covers an
  invalid `graph-layout`. `Q-2-48` covers `#|` on a `dot` block.
- Alignment rules for `pre.graphviz > svg` in both stylesheets, sharing the
  mermaid selectors.
- The preview component now reads `fig-alt`, so render and preview agree.

Choices made while building it, for review:

- **Placeholder element.** `<pre class="graphviz">`, not `<pre class="dot">`,
  so no highlighter or existing `pre.dot` style touches it.
- **`fig-alt`.** DOT has no accessibility directive, so the option travels
  as a `fig-alt` attribute on the code block, then `data-fig-alt` on the
  `<pre>`, and the init sets `role="img"` and `aria-label` on the SVG. Q1
  writes it into the SVG at render time; the effect for the reader is the
  same.
- **`graph-layout` validation.** A value that is not an engine in the
  vendored build is dropped with a `Q-2-47` warning at render time, and the
  transform allow-lists the attribute form again before writing
  `data-graph-layout`.
- **No sanitising in render.** The init parses the SVG with `DOMParser` and
  imports it, but does not filter it, because a rendered document is the
  author's own content and may already contain raw HTML. The preview
  component does filter, because hub projects are shared.
- **`charset="utf-8"` on the script tag.** The bundle stores its WebAssembly
  binary as raw non-ASCII characters. Read in another encoding it is
  corrupt. For the same reason `.gitattributes` marks the file `-text`, and
  the npm drift test compares bytes without normalising line endings.
- **`fetch(` is allowed in the bundle.** See the spike. The
  self-containment guard checks for `import(` and `.wasm` only.
- **Behaviour change.** A plain `dot` fence that an author used to *show*
  DOT source now draws a diagram. This is decision 2 below, taken here
  without ratification. The docs say to use another language name.
- **`{dot}` cells with an engine.** `language_of` strips braces, so a
  `{dot}` cell is also treated as a diagram cell by the option desugaring,
  as `{mermaid}` already is. Not examined further.

A bug the browser check caught:

The first build drew nothing. The init script is JavaScript inside a Rust raw
string, and a `'\n'` had been written as a real newline, which is a syntax
error in a JavaScript string literal. All 25 Rust unit tests passed, because
none of them can parse JavaScript. Opening the rendered page showed it at
once. The fix is in, with a structural test
(`init_script_is_structurally_sound`) for unterminated strings and unbalanced
brackets. That test is a guard, not a parser; the real protection is a browser
run. The Playwright smoke-all sweep should cover these fixtures but was **not
run**.

End-to-end evidence:

- Invocation: `target/debug/q2 render <fixture>.qmd` for copies of the four
  smoke fixtures, a dashboard document and a syntax-error document, in a
  scratch directory.
- The rendered pages were opened from `file://` in headless Chrome 157 with
  all host lookups disabled (`--host-resolver-rules="MAP * ~NOTFOUND"`), and
  the resulting DOM was inspected.

| Document | Observed |
|---|---|
| `basic` (html) | 1 `pre.graphviz` holding an SVG with 2 nodes and 1 edge; no source text left; no console message |
| `cell-options` (html) | 4 diagrams drawn. `div#fig-flow.quarto-figure-right` wraps the first, with "Figure 1: A numbered float, aligned right." and a working `@fig-flow` reference. The third has `quarto-figure-left`, `data-graph-layout="neato"` and `aria-label="Two nodes joined by an edge."` on its SVG. The fourth (`{.dot graph-layout="circo"}`) has 3 nodes and 3 edges |
| `revealjs` | 1 diagram inside a slide `<section>`, 3 nodes and 2 edges, drawn although the slide is not the visible one |
| dashboard | 2 diagrams drawn inside cards, one with `circo` |
| syntax error | `pre.graphviz.graphviz-error` reading `Graphviz error: syntax error in line 1 near '}'` above the source; the render itself reported nothing |

- A screenshot of `cell-options` was inspected: the float is right-aligned,
  the unnumbered figure is centred, the bare diagrams sit at the left.
- The written `viz-global.js` is byte-identical to the vendored file.
- Chrome only. The dashboard page logged one unrelated console message
  about a dashboard web-component module blocked on `file://`.

Build and test results:

| Step | Result |
|---|---|
| `cargo xtask lint` | passed |
| `cargo clippy -p quarto-core --all-targets -- -D warnings` | clean |
| `cargo nextest run --workspace --no-fail-fast` | 19,979 of 19,982 passed; 3 failed, none from this work (see below) |
| claude-notes render (part of `cargo xtask verify`) | 1828 of 1828 |
| `hub-client`: `npm run build:wasm` | passed |
| `hub-client`: `npx tsc -b`, `vite build`, `check-sw-precache` | passed |
| `hub-client`: `npm run test` | 1801 passed |
| `hub-client`: `npm run test:integration` | 241 passed |
| `hub-client`: `npm run test:wasm` (includes the smoke-all sweep and DOM parity) | 480 passed |

The three failing Rust tests:

- `smoke_all`: 9 fixtures under `typst/margin-layout` need the Python package
  `great_tables` and the R packages `gt` and `flextable`, which this machine
  does not have. The other 362 fixtures passed, including the four Graphviz
  ones.
- `dashboard_goldens`: 2 of 76 fixtures need the R package `bsicons`, also
  missing here.
- `quarto-preview`
  `capture_driver::tests::a_prose_edit_while_stopped_reuses_the_record_through_the_ref`
  fails when the crate's tests run in parallel and passes alone. It fails
  the same way on clean `origin/main` (`936c29e13`), twice. No braid strand
  has been filed for it yet.

One fixture was re-captured: `phase5-single-doc-baseline/expected_hashes.txt`.
The `styles.css` hash changed because the alignment rules now select
`pre.graphviz > svg`. The `doc.html` hash did not change.

`cargo xtask verify` was not run as one command to completion. Its first run
stopped at the claude-notes render, because this plan file had a parse error
(an unbalanced quote, now fixed). Its second run stopped at the first test
failure. The steps above are the same steps run separately, with
`--no-fail-fast`. The hub-client `vite build` wrote to a temporary directory
so the running local hub kept its build.

Not done:

- `q2 preview` and slides preview were not opened after this change.
- Firefox and Safari.
- The Q1 smoke fixtures are not ported.
- `file:`, `echo: true`, `fig-width`, `fig-height`, `fig-responsive`: still
  out of scope, as listed above.
- No embedded live example in the docs.
- The hub-client changelog entry (no file under `hub-client/` changed).

## Decisions to ratify

1. **Client-side first (A), or render-time SVG from the start (B)?** A is the
   faithful parallel to Mermaid and the shortest path to three green cells. B
   matches Q1 output and reaches Typst, at more cost and with an unverified
   technical risk. My recommendation is A first, then a spike on B.

   **Decided by Andrew, 2026-11-13: A for the HTML family now, a render-time
   SVG path for the other formats later (`bd-atz1c25y`).** Team ratification is
   still open. The reasoning:

   - The browser renderer is needed in any case. Preview and QuartoHub skip the
     render transform and draw diagrams in a React component, and the hub build
     of q2 is wasm, so it cannot run a second Graphviz wasm module at render
     time. The browser work is never thrown away.
   - It turns the HTML, Dashboard and RevealJS cells of the feature checklist
     green on its own, and changes show up in QuartoHub immediately.

   Accepted costs:

   - Two renderers to keep in step once the SVG path exists (see the version
     constraint under [Later phase](#later-phase-typst-and-other-formats)).
   - A syntax error shows only in the page, not as a source-mapped render
     diagnostic. Mermaid in q2 has the same limit.
   - Until the SVG path lands, non-HTML formats keep printing the source as a
     code block, as Mermaid does in Typst today.
2. **Which fence spelling?** Mermaid set the precedent: plain fence only, brace
   form warns. Following it means ```` ```dot ```` draws a diagram. That changes
   behaviour for anyone who uses a plain `dot` fence today to *show* Graphviz
   source, since it currently renders as highlighted code. Mermaid made the same
   trade. Worth a deliberate yes from the team.

## Open questions

- ~~Which Graphviz browser build to vendor, and its size and licence.~~
  Answered by the [spike](#spike-self-contained-browser-build):
  `@viz-js/viz` `dist/viz-global.js`. The licence notice is the part still
  open.
- In approach A, where does a Graphviz syntax error show up? Q1 fails the render
  with a source line. A client-side renderer can only show an error in the page.
  This is a real regression against Q1's behaviour and against q2's own "source
  mapped diagnostics" principle, and it is an argument for B.
- Should ```` ```graphviz ```` be accepted as an alias for ```` ```dot ````?
- `fig-alt`: Q1 writes it into the SVG. Mermaid in q2 folds it into mermaid's
  own `accDescr`. Graphviz has no equivalent in its source language, so the
  component would need to set it on the rendered SVG.
- What should happen to `"dot"` in `HANDLED_LANGUAGES` (`engine/mod.rs:201`)?
  Mermaid is still listed there too, so probably nothing, but confirm.
- Does knitr claim `dot` cells the way it claims `mermaid`? The Mermaid plan
  says knitr's `handledLanguages` includes `mermaid`. **Not checked** for `dot`.

## Environment notes for the new session

- **Repos.** The Graphviz work lives in this worktree,
  `~/Desktop/Projects/q2/.worktrees/bd-ijlapt70-graphviz-support`, on branch
  `braid/bd-ijlapt70-graphviz-support`, created from `origin/main` at
  `936c29e13` with `cargo xtask create-worktree`. `~/Desktop/Projects/q2-latest`
  is a separate worktree on a detached HEAD at the same commit, kept as a clean
  copy of main. To refresh it:
  `git fetch origin && git checkout --detach origin/main`.
  `~/Desktop/Projects/quarto-cli` is a shallow clone of Quarto 1 (v1.12.1).
  `~/Desktop/Projects/quarto-web` is the Q1 docs site (2025-11-13).
- **Binaries.** A debug build is at `target/debug/q2` in this worktree
  (0.45.0), and another at `~/Desktop/Projects/q2-latest/target/debug/q2`. The
  `q2` on the PATH
  (`~/.local/bin/q2`) is the older 0.40.0. Build with
  `cargo build -p quarto --bin q2`.
- **braid** 0.6.0 is installed and resolves the q2 skein from any q2 checkout.
  The strand is `bd-ijlapt70`.
- **Pandoc** is 3.13 from Homebrew. q2 needs 3.12 or newer for Docx and Pptx.
- **Project rules.** Read `AGENTS.md` and `.claude/rules/` in the q2 repo before
  changing anything. The repo expects plan documents, tests first, and recorded
  end-to-end evidence.
- **Minimal reproduction** of today's behaviour:

  ````markdown
  ---
  title: T
  format: html
  ---

  ```{dot}
  digraph G { a -> b }
  ```

  ```dot
  digraph H { c -> d }
  ```
  ````

  Rendering this with 0.45.0 gives the `Q-2-58` warning for the first block, two
  code blocks, and no `<svg>` in the output.
