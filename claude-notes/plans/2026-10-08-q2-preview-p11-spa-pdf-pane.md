# Plan 11: live PDF preview in the `q2 preview` viewer SPA

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E5); the execution model and landing procedure are there.

## Execution

- **Lane:** C (`workspace-3`), binding: one agent per plan, in this order: Plan 1, Plan 4, Plan 7, Plan 8, Plan 11. This plan is number 5 of 5 in the lane.
- **Waits on:** Plans 4 (pdf.js packaged, `ts-packages/pdf-viewer`, `/pdfjs/` served) and 9 (`POST /api/preview/pdf` and its contract) merged into `feature/q2-preview-hub-parity`. Nothing inside this plan waits on another plan.
- **Start gate:** every plan under "Waits on" is merged. Check from your worktree: `git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p04-pdfjs-packaging' && git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p09-preview-routes' && echo ready`. If it does not print `ready`, do not start: report which plans are missing and stop (plan branches are never deleted after landing, so `--merged` is the test). Before coding, read the Handoff log at the end of each plan you wait on.
- **Branch:** `q2-preview/p11-spa-pdf-pane`, cut from the tip of `feature/q2-preview-hub-parity` when you start.
- **Merges to:** `feature/q2-preview-hub-parity`, by the landing procedure in the epic.
- **Unlocks:** Plan 13 (documents it and closes the epic).
- **Next in lane:** none; this is the last plan in the lane.

## Goal

A `format: typst` page in `q2 preview` (default `--ui viewer`, `q2-preview-spa`) shows a live PDF: the server compiles with native `typst` (Plan 9's route), and the SPA displays the bytes in the bundled pdf.js. Today the SPA renders typst through the HTML pipeline and shows a "no live preview" overlay (`noLivePreviewMessage`, `PreviewApp.tsx:282`).

## Background facts

(Line numbers from the 2026-10-08 survey; re-check.)

- **Classification.** `resolvePandocFormats(path)` (`@quarto/preview-runtime`, exported synchronously at `wasmRenderer.ts:808`) is pure and already folds in `output-ext`; for a doc with `output-ext: typ` it returns key `typst`, not `typst-pdf` (`pandoc_request/formats.rs:243-252`). hub-client's `classifyPreviewMode` (`getQ2Format.ts:79-101`) cannot be imported by the SPA. `initWasm` is awaited at boot before the active file is chosen (`PreviewApp.tsx:777`).
- **SPA structure.** The render effect is `PreviewApp.tsx:1144-1291`, keyed on `[activeFile, contentTick, captures]`; the render-tree gates are at `:1349`, `:1366`, `:1378`; the title effect at `:1301`; `handleNavigate` at `:681` with the single `pushState` at `:706` (it passes `null` as state) and a popstate handler; `pickInitialPage.ts` chooses the first page.
- **Hub-client code to reuse.** `ts-packages/pdf-viewer` (Plan 4) exports `mountPdfViewer`; `PdfPreviewPane.tsx` is coupled to the download controller and is not reusable.
- **Back button.** pdf.js's `viewer.html` carries a CSP meta (`script-src 'self'; style-src 'self'`): from the parent, `setAttribute('style')` and inline `<style>` are blocked; `style.cssText` and CSSOM work (ran, 6.3.289). `#toolbarViewerLeft` exists in the viewer. `PdfPane` does not exist until task 11.5, so the spike runs in a harness page.
- `--static` never loads the SPA, so there is no static-mode handling in the SPA (Plan 8 owns it).
- Missing typst or pandoc shows the existing `Q-21-1` (typst) and `Q-20-1`/`Q-20-2` (pandoc) text in the banner (P9-D6).

## Decisions

- **P11-D1** Classification is SPA-local: `isTypstPdf(path)` = `isWasmReady()` and `resolvePandocFormats(path).formats[0].key === 'typst-pdf'` and class `download`; a doc whose `output-ext` is not `pdf` resolves to key `typst`, so it keeps the "no live preview" overlay (the download route's E8 behaviour does not apply to the SPA).
- **P11-D2** The PDF pane always requests `scope: 'chapter'` (E11); the viewer SPA never previews a whole book. Plan 4's `mountPdfViewer` takes `base`; the SPA passes `'./'`.
- **P11-D3** A Back button is injected into the pdf.js toolbar (spiked first); fall back to a slim host bar if the spike fails. The injected UI uses CSSOM or a same-origin stylesheet only.
- **P11-D4** The Back index lives in `history.state` (the existing `pushState` passes `null`), so browser Back and Forward keep it correct; a hand-kept counter ignores them and would let Back leave the SPA.

## Tasks

Per task: `tsc` plus `vitest` in `q2-preview-spa` and `ts-packages/pdf-viewer`. Workspace nextest at the end of the plan only if Rust code changed (this plan is TypeScript and CI; run it anyway at the end for the delta). Tests needing `typst` or `pandoc` skip with a clear reason when absent.

- [ ] 11.1 Baseline `cargo nextest run --workspace` on the integration tip.
- [ ] 11.2 Back-button spike first (P11-D3). Needs: the pdf.js zip fetched locally (`hub-client/public/pdfjs` is absent until `fetch-pandoc-wasm.mjs --from-pdfjs <zip>` runs), a sample PDF, and a harness page (`test-hooks.ts` or a fixture page that calls `mountPdfViewer`). Add optional `onFrameReady?(win, doc)` to `mountPdfViewer`'s options, called in `load()` just before `resolve()` (`PDFViewerApplication` is reachable after `initializedPromise`, `pdfViewer.ts:75-82`). Playwright check against pdf.js 6.3.289: the button is present after five consecutive recompile swaps (inject into the new frame before it replaces the old one), in light and dark themes and at 400 px width; exactly one anchor (`#toolbarViewerLeft`, verified in the fetched `viewer.html`) with a loud failure when it is missing, like `patchWorker`; `externalLinkTarget` set to BLANK from the same hook. Record the verdict here; on failure build the slim host bar (about 28 px: title plus Back).
- [ ] 11.3 `onBack`: a navigation index stored in `history.state` (P11-D4): seed index 0 on boot with `replaceState` (the only existing `pushState`, `:706`, passes `null`, so a null state is read as index 0), then update it in `handleNavigate` and the popstate handler; go back when the index is above zero; otherwise the project's first page (`pickInitialPage.ts`); hide Back when the pane is the only page.
- [ ] 11.4 Classification (P11-D1): `isTypstPdf` and `pageKind: 'html' | 'pdf'` state recomputed on every `contentTick`.
- [ ] 11.5 `PreviewApp.tsx` (render effect `:1144-1291`): the PDF branch sits above the render-tree gates (`:1349`, `:1366`, `:1378`) and skips `renderPageForPreview`; on HTML-to-PDF navigation clear `astJson`, `renderedContent`, `themeFingerprint`, warnings and the title effect (`:1301`); the connection banner stays.
- [ ] 11.6 `q2-preview-spa/src/components/PdfPane.tsx` (+ `PdfPane.css`, `PdfPane.test.tsx`): `PdfPane({path, contentTick, onBack})`; 500 ms debounce, `fetch('/api/preview/pdf')` with `AbortController` and a sequence guard, body `{path, content, scope: 'chapter'}`, `mountPdfViewer`, a first-PDF spinner, an "updating" chip, an error banner keeping the last good PDF (monospace diagnostics), the missing-typst message, a collapsed warnings list from `X-Q2-Diagnostics`, the unexecuted-cells notice from `X-Q2-Stats`; reads text via `vfsReadFile`. Vitest: `debounces to one POST`, `aborts the previous request`, `ignores a stale sequence`, `keeps the last PDF under the banner on 422`, `shows the Q-21-1 text when typst is missing`, `409 superseded is silent`, `sends content and scope chapter`.
- [ ] 11.7 `PreviewApp.integration.test.tsx`: `typst page renders PdfPane not iframe`; `html -> pdf -> html leaves no stale AST`; typst-pdf as the first page (no boot-spinner hang); a non-active-file edit (image, `_quarto.yml`) re-renders; Back after browser Back/Forward stays correct.
- [ ] 11.8 Playwright `q2-preview-spa/e2e/typst-pdf-preview.spec.ts`. Fixtures: valid typst doc; broken `{=typst}` block; `output-ext: typ` doc (expects the "no live preview" overlay, not a PDF iframe, consistent with P11-D1); project doc inheriting `format: typst`; vendored-package doc (`.column-margin`); a book chapter (chapter-alone). Cases: the PDF iframe appears with the heading in its text layer; an edit recompiles; a broken edit shows the banner and keeps the page. Skip without `typst` or `pandoc` or when the embedded SPA is the placeholder. No non-vendored package in CI.
- [ ] 11.9 CI (this and Plan 12's task 12.11 both edit `q2-preview-spa-e2e.yml` and its `paths:` filter, and neither waits on the other: whichever lands second resolves a mechanical conflict by keeping both sets of steps and paths): `q2-preview-spa-e2e.yml` (PR-only, `continue-on-error`) installs typst and pandoc (as `test-suite.yml:88-108`, copying its `PANDOC_VERSION` and `TYPST_VERSION` pins) and has network for the SPA prebuild; its `paths:` filter (`:10-14`) adds `q2-preview-spa/src/**`, `q2-preview-spa/e2e/**`, `crates/quarto/**`, `crates/quarto-preview/**`, `crates/quarto-core/**`, `crates/quarto-hub/**`, `crates/spa-manifest/**`, `scripts/fetch-pandoc-wasm.mjs`, `resources/pdfjs-viewer.json`, `ts-packages/pdf-viewer/**`, `ts-packages/preview-runtime/**`, `ts-packages/preview-renderer/**`, `Cargo.lock`.
- [ ] 11.10 Plan end: workspace nextest against 11.1; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- pdf.js bumps need `patchWorker` updates; it throws loudly when its anchor is missing.
- Several tabs on one path coalesce on the server: one tab's request can supersede another's.

## Handoff log

Written by the implementing agent in the plan-end task, committed with the checklist: what landed (commit range), any task that deviated from the plan and why, names and signatures of the interfaces later plans consume exactly as they ended up, the workspace nextest baseline and final counts, and anything the next agent in the lane or a plan that waits on this one must know. Empty until the plan lands.
