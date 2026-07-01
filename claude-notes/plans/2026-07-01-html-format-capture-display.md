# Display executed code output in the default `format: html` preview

**Strand:** bd-uy4uygha (bug, P1; discovered-from bd-sfet3264).
**Date:** 2026-07-01.
**Status:** Planned — awaiting review. No implementation started.

## Overview

hub-client shows server-recorded engine output (executed code results) **only**
for documents whose resolved format is `q2-preview` (or `q2-slides` / `q2-debug`
/ `revealjs`). For the **default `format: html`** — plain documents and every
website page — the preview renders code cells as *source* even when a capture
exists: the "Executor online" and "Showing executed output" bars appear, the
provider runs the engine and writes the capture, but the output never lands in
the preview.

Discovered while verifying the remote-execution feature (bd-sfet3264)
end-to-end. Switching a document to `format: q2-preview` immediately shows the
output — that is the current **workaround**.

**Why it never surfaced before:** `q2 preview` only ever renders in the
`q2-preview` (AST-splice) format, so its capture path is always exercised.
hub-client additionally supports the plain-`html` render (with full website
chrome), which `q2 preview` does not — an untested mode. The Phase 1 capture
tests only covered `ReactPreview` (the `q2-*`/revealjs component), not the
default `Preview`.

## Root cause (three layers, verified 2026-07-01)

The capture bytes already flow **into Rust** — `render_page_in_project_with_attribution`
parses them (`crates/wasm-quarto-hub-client/src/lib.rs:1148`) — but every HTML
branch drops them, and the TS default-HTML path never sends them.

1. **WASM — the HTML branch ignores captures.** `render_single_doc_to_response`
   (`lib.rs:1351`) dispatches on `format.pipeline_kind`: the `Some("preview")`
   branch folds captures through the splice; the `_ =>` HTML branch
   (`lib.rs:1438`) calls `render_qmd_to_html(...)` and never references
   `captures` (explicit comment at `lib.rs:1357-1362`). The website path
   `render_project_active_page_to_response` (`lib.rs:1519`) is the same: its
   HTML `_ =>` arm (`lib.rs:1616`) builds a `RenderToHtmlRenderer` with no
   captures. Both HTML branches funnel through `render_qmd_to_html`
   (`crates/quarto-core/src/pipeline.rs` ~833; the Pass-2 site is
   `crates/quarto-core/src/project/pass2_renderer.rs:825`), which has **no
   captures channel**.

2. **preview-runtime — `renderToHtml` doesn't thread `captureGzJson`.**
   `RenderToHtmlOptions` (`ts-packages/preview-runtime/src/wasmRenderer.ts:959`)
   has no `captureGzJson`; `renderToHtmlInner` (`:1189`) calls
   `renderPageInProject(documentPath, grammarsHandle)` and `renderPageInProject`
   (`:459`) hard-codes `renderPageInProjectWithAttribution(path, ug, null)` with
   no captures. (The lowest wrapper `renderPageInProjectWithAttribution` **does**
   already accept `captureGzJson` — `:499` — so only the two convenience
   wrappers above it drop it.)

3. **hub-client — `<Preview>` never receives captures.** `PreviewRouter`
   routes non-`q2-*` formats to `<Preview>` (`PreviewRouter.tsx:157,163`) and
   destructures `captures` **out** of the props it forwards there
   (`PreviewRouter.tsx:148`, comment `:146-147`). `Preview.tsx` calls
   `renderToHtml({ documentPath, userGrammars })` (`Preview.tsx:106`) — it never
   sees `captures`.

## Key finding that makes this tractable

`CaptureSpliceStage` (`crates/quarto-core/src/stage/stages/capture_splice.rs`)
is a **plain `DocumentAst → DocumentAst` pipeline stage** with no
q2-preview-specific coupling — empty captures = pass-through. The q2-preview
builder inserts it **immediately before `EngineExecutionStage`** and rebuilds
that stage with `.with_spliced_engines(names)` to suppress the spurious
"(no execution)" warning (`pipeline.rs:406-433`). We insert it into the HTML
stage list the exact same way.

Two properties make this low-risk:

- **Cell alignment is guaranteed.** The server records captures via
  `build_capture_pipeline_stages` = the **HTML** stage list truncated at
  `engine-execution` (`crates/quarto-core/src/engine/preview_record.rs:109-117`).
  So the HTML render's pre-engine stages produce byte-identical cell input to
  what the capture recorded — the `(content-hash, occurrence-index)` match the
  splice relies on holds by construction.
- **No phase-ordering contract to satisfy.** `CaptureSpliceStage` is a top-level
  *pipeline stage*, not a member of `build_transform_pipeline`, so the
  `TransformPhase` contract (which only governs transforms inside
  `AstTransformsStage`) does not apply. The only invariant — splice **before**
  engine execution — is the same one the preview builder already relies on.

## Design decision

**Insert `CaptureSpliceStage` into the HTML pipeline at the stage level,
immediately before `EngineExecutionStage`** — a captures-aware sibling of
`build_html_pipeline_stages_with_options`, mirroring
`build_q2_preview_pipeline_stages`. Rejected alternative: adding a transform
inside `build_transform_pipeline` (would re-implement splice logic, run
post-engine, and drag in the phase-ordering contract).

**Plumbing surface for `render_qmd_to_html`:** carry the captures on
**`HtmlRenderConfig`** (a `captures: Vec<EngineCapture>` field, default empty)
rather than a new positional argument. Rationale: `render_qmd_to_html` is called
from two sites (the single-doc WASM branch and `pass2_renderer.rs:825`) and
across the wasm-bindgen boundary conventions; a config field is the smallest
churn and keeps the empty-captures path byte-identical. **(Confirm during 1A.)**

## Phased plan (TDD)

> Per CLAUDE.md: write/adjust the test first, watch it fail, then implement.
> This change touches `quarto-core` + the WASM crate, so the WASM leg is
> affected — full `cargo xtask verify` (with WASM rebuild) is required before
> the push request, and hub-client needs `npm run build:wasm` for the browser
> e2e.

### Phase 1 — Rust: captures-aware HTML pipeline (`quarto-core`)

- [x] **1A — captures-aware HTML stage builder.** ✅ done. Added a
      `captures: Vec<EngineCapture>` field + `with_captures` to `HtmlRenderConfig`
      (default empty); extracted a shared `insert_capture_splice_stage` helper
      (the splice-insert + engine-stage-rebuild-with-spliced-names, formerly
      inline in `build_q2_preview_pipeline_stages`, now used by both);
      `build_html_pipeline_stages_with_captures` variant; `render_qmd_to_html`
      uses it when `config.captures` is non-empty (empty → unchanged builder,
      byte-identical). RED→GREEN native test
      `render_qmd_to_html_splices_captures` (hand-built `.cell`-wrapped capture
      with a fictitious non-spawning engine, mirroring `captureSplice.wasm.test.ts`)
      → the marker appears in the HTML as a `.cell` Div; empty captures render
      source-only. Full quarto-core suite (2406) green, clippy clean.
      **Note:** the raw-cell `input_qmd` hash-matches the doc's post-sugar cell
      (as the WASM test already relied on); a real engine name spawns a
      subprocess, so tests must use a fictitious engine + a `.cell`-wrapped
      `result.markdown` (a bare passthrough echo isn't a `Div.cell` and won't
      splice).
- [x] **1B — `RenderToHtmlRenderer::with_captures`.** ✅ done. Added the
      `captures` field + `with_captures` builder (mirrors
      `RenderToPreviewAstRenderer`); `render` now builds the config with
      `.with_captures(self.captures.clone())`. New integration test
      `render_to_html_captures.rs`: a multi-file project (`_quarto.yml` +
      index.qmd cell + about.qmd prose) rendered in `ActivePage` mode with a
      capture → the active page's HTML has the spliced `.cell-output`; no
      captures → source-only. Green (reuses the 1A `render_qmd_to_html` path).

### Phase 2 — WASM: use captures in both HTML branches (`wasm-quarto-hub-client`)

- [ ] **2A — thread the already-parsed `captures` into the HTML branches.**
      `render_single_doc_to_response` `_ =>` arm (`lib.rs:1438`): build the
      `HtmlRenderConfig` with the in-scope `captures`.
      `render_project_active_page_to_response` `_ =>` arm (`lib.rs:1616`): call
      `.with_captures(captures)` on the `RenderToHtmlRenderer`. No new WASM
      export or wasm-bindgen signature is needed — the bytes already arrive via
      `render_page_in_project_with_attribution` (`lib.rs:1148`).
      - **RED→GREEN (WASM vitest):** a new test mirroring
        `captureSplice.wasm.test.ts` but driving the **`render_page_in_project` /
        html** path (a `format: html` doc + a real gzipped capture) → asserts the
        spliced `.cell-output` marker is in the rendered HTML; no-capture ⇒
        source-only.

### Phase 3 — preview-runtime: forward `captureGzJson` through `renderToHtml`

- [ ] **3A — thread captures through the convenience wrappers.** Add
      `captureGzJson?: Uint8Array` to `RenderToHtmlOptions`
      (`wasmRenderer.ts:959`); forward it from `renderToHtmlInner` (`:1189`) into
      `renderPageInProject`; give `renderPageInProject` (`:459`) a captures param
      that forwards to the already-capable `renderPageInProjectWithAttribution`
      (`:503`). RED→GREEN unit test asserting the bytes reach the (mocked) WASM
      binding.

### Phase 4 — hub-client: feed captures to the default `<Preview>`

- [ ] **4A — route captures to `<Preview>`.** `PreviewRouter.tsx:163` — pass
      `captures` into `<Preview>` (stop dropping it at `:148`).
- [ ] **4B — `<Preview>` consumes captures.** Replicate ReactPreview's
      capture-doc resolution + fetch (`ReactPreview.tsx:582-605`): derive
      `activeCaptureDocId` from `captures[path]?.captureDocId`, fetch bytes via
      `getBinaryDocById`, and pass `captureGzJson` into the `renderToHtml({...})`
      call (`Preview.tsx:106`). Add the render-trigger dep so a freshly-arrived
      capture re-renders. Consider factoring the shared fetch hook out of
      ReactPreview to avoid duplication.
      - **RED→GREEN (integration):** a `Preview` test (mirroring
        `ReactPreview.capture.integration.test.tsx`) — a capture in props is
        fetched by id and its bytes reach the `renderToHtml` call; no-capture ⇒
        no fetch.

### Phase 5 — verify + end-to-end

- [ ] **5A — `cargo xtask verify`** (full, incl. WASM rebuild + hub-client
      tests). `npm run build:wasm` so the browser picks up the new WASM.
- [ ] **5B — browser e2e (manual, recorded).** A `format: html` document with a
      `{r}`/`{python}` cell + a connected `q2 provide-hub --allow-all`: click Run
      → the preview shows the executed output (no `format: q2-preview` needed).
      Reuse the `interop-repro/` harness (create a project, run the provider) and
      the Chrome DevTools flow used to diagnose this. Record the observed output.

## Risks / open questions

- **`HtmlRenderConfig` field vs. argument for `render_qmd_to_html`** — confirm in
  1A that a config field is clean across both call sites (the wasm single-doc
  branch and `pass2_renderer.rs`). Fall back to an explicit argument if the
  config is shared in a way that makes an empty-default awkward.
- **Shared builder helper** — 1A should factor the "insert splice + rebuild
  engine stage with spliced names" logic so the q2-preview and HTML builders
  can't drift (they must stay cell-aligned with `build_capture_pipeline_stages`).
- **Website multi-page** — captures are per-file and only the **active page**
  (Pass-2) needs them; Pass-1 builds the index/profile with no captures. Verify
  a capture on the active page doesn't leak into sibling-page renders (1B test).
- **`Preview`/`ReactPreview` duplication** — the capture-fetch effect is now
  needed in both; factor a hook rather than copy-paste (bd-uy4uygha follow-up if
  it grows).

## Key source references

- WASM HTML branches: `crates/wasm-quarto-hub-client/src/lib.rs:1438` (single-doc),
  `:1616` (project active page); captures parsed at `:1148`.
- Preview splice model: `build_q2_preview_pipeline_stages`
  (`crates/quarto-core/src/pipeline.rs:387-436`), splice insert `:431-433`.
- `CaptureSpliceStage`: `crates/quarto-core/src/stage/stages/capture_splice.rs`.
- HTML pipeline: `build_html_pipeline_stages_with_options`
  (`crates/quarto-core/src/pipeline.rs:249-339`); `render_qmd_to_html` (~`:833`).
- Pass-2 renderer: `crates/quarto-core/src/project/pass2_renderer.rs:727,825,991`.
- Capture recording (cell alignment): `build_capture_pipeline_stages`
  (`crates/quarto-core/src/engine/preview_record.rs:109-117`).
- TS wrappers: `ts-packages/preview-runtime/src/wasmRenderer.ts:459,491,959,1168`.
- hub-client: `hub-client/src/components/render/{PreviewRouter.tsx:148,157,163,
  Preview.tsx:106, ReactPreview.tsx:582-605}`.
