# q2 preview: deliver engine `include-in-header` to the pane (marimo hydration)

**Strand:** bd-5oyk1xce (discovered-from bd-5jxcio5d)
**Branch:** `braid/bd-5oyk1xce-q2-preview-drops-engine` (off the bd-5jxcio5d capture-splice branch)

## Overview

In `q2 preview`, marimo cells render their static server-side island markup
but never hydrate / execute dynamically. Root cause (code + browser confirmed —
see strand bd-5oyk1xce): the engine's `include-in-header` content (the
`@marimo-team/islands` CDN `<script type=module>`, the
`__MARIMO_EXPORT_CONTEXT__` trust-marker, `<marimo-code hidden>`, islands
`style.css`) never reaches the preview pane's `<head>`.

Two independent gaps, both must be closed:

1. **Delivery gap (Rust).** On the render path, `EngineExecutionStage` copies
   `result.includes.header_includes` into `ctx.includes`
   (`engine_execution.rs:416`), and `ApplyTemplateStage` late-folds `ctx.includes`
   into `meta.rendered.includes.header` (`apply_template.rs:149`). The
   `q2-preview` pipeline **excludes `ApplyTemplateStage`**
   (`Q2_PREVIEW_STAGE_EXCLUDED`, `pipeline.rs:361`) and bypasses
   `EngineExecutionStage`; `CaptureSpliceStage` extracts only
   `capture.result.markdown` and discards `capture.result.includes`
   (`capture_splice.rs:138–146`). So the includes are lost.
   The full `ExecuteResult` (incl. `includes.header_includes`) IS present in the
   recorded capture (serialized at `engine_execution.rs:385`).

2. **Execution gap (TS).** The preview delivery channel
   (`meta.rendered.includes.header` → `PreviewDocument` → `HeaderIncludesEffect`
   → iframe `document.head`) exists and is wired, BUT `HeaderIncludesEffect`
   (`chromeSlots.tsx:146–153`) builds nodes via `wrapper.innerHTML = …` then
   `appendChild`s them. **Scripts parsed from `innerHTML` are flagged
   non-executable; moving them into the DOM does not run them.** So even once
   delivered, the islands `<script>` and the inline `__MARIMO_EXPORT_CONTEXT__`
   would be inert. (This is a latent general bug for any authored
   `include-in-header` `<script>` in preview, not just marimo.)

## Design decision (Gordon, 2026-07-08)

**Narrow fix: `CaptureSpliceStage` writes engine header-includes directly into
`meta.rendered.includes.header`** (via the existing
`include_resolve::append_pandoc_includes` helper), rather than populating
`ctx.includes` + adding a drain stage. Rationale: captures are the ONLY
engine-include producer on the preview path (`EngineExecutionStage` is a no-op
after the splice), so a general `ctx.includes→meta` drain would carry nothing
extra — the narrow write is sufficient and adds no new pipeline surface.

## Test plan (TDD — write/verify-fail FIRST)

- [x] **T1 (Rust, native seam).** Unit-test a new pure helper
  `fold_capture_includes_into_meta(meta, capture)` in `capture_splice.rs`:
  given a capture whose `result.includes.header_includes = ["<script …islands…>"]`,
  after folding, `meta.rendered.includes.header` contains that string (and
  before/after-body slots for `include_before`/`include_after`). Also assert it
  APPENDS (does not clobber an authored header include already present) and is
  fail-soft on missing/garbage `includes` JSON. Bind: reverting the
  `append_pandoc_includes` call reddens T1.
- [x] **T2 (TS, vitest, jsdom integration).** Two tests in
  `PreviewDocument.integration.test.tsx`: (1) `rematerializeScript` returns a
  DISTINCT `<script>` copying attrs + inline body (binds the re-materialization
  logic — reverting the helper to `return source` reddens it, verified); (2) a
  `<script>` header include lands in `document.head` with the marker + correct
  `src`, and unmount cleanup removes it (binds delivery/wiring). NOTE: jsdom
  (no `runScripts`) cannot observe actual JS execution, so the execution proof
  is T3 (real browser) — this is consistent with the repo's end-to-end doctrine.
- [x] **T3 (e2e, real browser).** Extend the marimo preview e2e (SC23 sibling):
  after the widget capture splices, the iframe `<head>` contains the islands
  `<script src=…jsdelivr…islands…>`, AND the runtime executed —
  `__MARIMO_EXPORT_CONTEXT__` defined and `<marimo-code>` present in the iframe
  (the markers that were absent in the bd-5oyk1xce repro). Gated behind
  `QUARTO_SC21_LIVE=1` like its siblings.

## Implementation

- [x] I1 (Rust). Add `fold_capture_includes_into_meta` helper + call it per
  capture in `CaptureSpliceStage::run` (fold into `doc_ast.ast.meta`). Deserialize
  `capture.result["includes"]` → `PandocIncludes` (plain serde, no rename),
  fail-soft.
- [x] I2 (TS). Rewrite `HeaderIncludesEffect` node insertion to re-materialize
  `<script>` elements (fresh `document.createElement('script')`, copy attributes
  + inline text) so they execute; non-script nodes keep the current path;
  preserve order + `data-q2-header-include` marker + unmount cleanup.

## Verification

- [ ] V1. `cargo nextest run -p quarto-core` (T1 + no regressions), then
  `cargo nextest run --workspace`.
- [x] V2. `cd hub-client && npm run build:all` + preview-renderer vitest (T2).
- [x] V3. Rebuild the preview binary chain (WASM → q2-preview-spa/dist → q2)
  and run T3 in headed Chromium; record the observed markers.
- [ ] V4. `cargo xtask verify` (WASM leg affected — quarto-core changed).
- [ ] V5. Reconcile checklist, update strand, prepare commit (await push OK).

## Notes / guardrails

- Do NOT conflate with bd-5m1ni9if (RawBlock-misconsume edge).
- hub-client changelog: preview-renderer is a ts-package bundled into the SPA;
  if the commit touches `hub-client/` proper, follow the changelog two-commit
  rule. (preview-renderer under `ts-packages/` is not `hub-client/` itself —
  confirm at commit time.)
- Branch unpushed; commit path-scoped; never `git add -A`; never push without
  Gordon's OK.
