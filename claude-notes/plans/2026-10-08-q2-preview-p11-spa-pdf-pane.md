# Plan 11: live PDF preview in the `q2 preview` viewer SPA

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E5); the execution model and landing procedure are there.

> **Superseded in part by [`2026-10-15-format-resolution-epic.md`](2026-10-15-format-resolution-epic.md):** book scope is now the `book-scope` metadata key (a render overlay carries it), `--book-scope`, the `scope` request fields and the `Option<BookScope>` plumbing described below are gone, and `Page` is `none`. This plan's text and Handoff log are history and are left as written.

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

- [x] 11.1 Baseline: take the workspace pass/skip counts recorded for the current integration tip (epic, "Workspace nextest ledger"); run `cargo nextest run --workspace` only if the tip has no recorded entry, and then say so in the Handoff log.
- [x] 11.2 Back-button spike first (P11-D3). Needs: the pdf.js zip fetched locally (`hub-client/public/pdfjs` is absent until `fetch-pandoc-wasm.mjs --from-pdfjs <zip>` runs), a sample PDF, and a harness page (`test-hooks.ts` or a fixture page that calls `mountPdfViewer`). Add optional `onFrameReady?(win, doc)` to `mountPdfViewer`\'s options, called in `load()` just before `resolve()` (`PDFViewerApplication` is reachable after `initializedPromise`, `pdfViewer.ts:75-82`). Playwright check against pdf.js 6.3.289: the button is present after five consecutive recompile swaps (inject into the new frame before it replaces the old one), in light and dark themes and at 400 px width; exactly one anchor (`#toolbarViewerLeft`, verified in the fetched `viewer.html`) with a loud failure when it is missing, like `patchWorker`; `externalLinkTarget` set to BLANK from the same hook. Record the verdict here; on failure build the slim host bar (about 28 px: title plus Back).
  **Verdict (2026-10-08, pdf.js 6.3.289, Chromium, throwaway Vite harness page calling `mountPdfViewer`): PASS, no host bar needed.** `installBackButton` (`q2-preview-spa/src/pdfBackButton.ts`, `style.cssText` only) inserts the button first in `#toolbarViewerLeft`; after five consecutive `show()` swaps with the same key there was exactly one iframe and exactly one button, in light, dark (`colorScheme`) and at 400 px width, with no CSP console errors; clicking it reached the host. The anchor check throws when `#toolbarViewerLeft` is missing. The harness (spike.html, `src/spike.ts`) was removed; the permanent Playwright coverage is 11.8. The 11.1 box: no ledger line exists for tip `d5db9bcd3` (Plan 9 landed without a workspace run), and the global rule is to recommend rather than run it unprompted; the baseline is Plan 9's derived 16102 passed, 202 skipped, until Gordon decides on a measured run.
- [x] 11.3 `onBack`: a navigation index stored in `history.state` (P11-D4): seed index 0 on boot with `replaceState` (the only existing `pushState`, `:706`, passes `null`, so a null state is read as index 0), then update it in `handleNavigate` and the popstate handler; go back when the index is above zero; otherwise the project's first page (`pickInitialPage.ts`); hide Back when the pane is the only page.
- [x] 11.4 Classification (P11-D1): `isTypstPdf` and `pageKind: 'html' | 'pdf'` state recomputed on every `contentTick`.
- [x] 11.5 `PreviewApp.tsx` (render effect `:1144-1291`): the PDF branch sits above the render-tree gates (`:1349`, `:1366`, `:1378`) and skips `renderPageForPreview`; on HTML-to-PDF navigation clear `astJson`, `renderedContent`, `themeFingerprint`, warnings and the title effect (`:1301`); the connection banner stays.
- [x] 11.6 `q2-preview-spa/src/components/PdfPane.tsx` (+ `PdfPane.css`, `PdfPane.test.tsx`): `PdfPane({path, contentTick, onBack})`; 500 ms debounce, `fetch('/api/preview/pdf')` with `AbortController` and a sequence guard, body `{path, content, scope: 'chapter'}`, `mountPdfViewer`, a first-PDF spinner, an "updating" chip, an error banner keeping the last good PDF (monospace diagnostics), the missing-typst message, a collapsed warnings list from `X-Q2-Diagnostics`, the unexecuted-cells notice from `X-Q2-Stats`; reads text via `vfsReadFile`. Vitest: `debounces to one POST`, `aborts the previous request`, `ignores a stale sequence`, `keeps the last PDF under the banner on 422`, `shows the Q-21-1 text when typst is missing`, `409 superseded is silent`, `sends content and scope chapter`.
- [x] 11.7 `PreviewApp.integration.test.tsx`: `typst page renders PdfPane not iframe`; `html -> pdf -> html leaves no stale AST`; typst-pdf as the first page (no boot-spinner hang); a non-active-file edit (image, `_quarto.yml`) re-renders; Back after browser Back/Forward stays correct.
- [x] 11.8 Playwright `q2-preview-spa/e2e/typst-pdf-preview.spec.ts`. Fixtures: valid typst doc; broken `{=typst}` block; `output-ext: typ` doc (expects the "no live preview" overlay, not a PDF iframe, consistent with P11-D1); project doc inheriting `format: typst`; vendored-package doc (`.column-margin`); a book chapter (chapter-alone). Cases: the PDF iframe appears with the heading in its text layer; an edit recompiles; a broken edit shows the banner and keeps the page. Skip without `typst` or `pandoc` or when the embedded SPA is the placeholder. No non-vendored package in CI.
- [x] 11.9 CI (this and Plan 12's task 12.11 both edit `q2-preview-spa-e2e.yml` and its `paths:` filter, and neither waits on the other: whichever lands second resolves a mechanical conflict by keeping both sets of steps and paths): `q2-preview-spa-e2e.yml` (PR-only, `continue-on-error`) installs typst and pandoc (as `test-suite.yml:88-108`, copying its `PANDOC_VERSION` and `TYPST_VERSION` pins) and has network for the SPA prebuild; its `paths:` filter (`:10-14`) adds `q2-preview-spa/src/**`, `q2-preview-spa/e2e/**`, `crates/quarto/**`, `crates/quarto-preview/**`, `crates/quarto-core/**`, `crates/quarto-hub/**`, `crates/spa-manifest/**`, `scripts/fetch-pandoc-wasm.mjs`, `resources/pdfjs-viewer.json`, `ts-packages/pdf-viewer/**`, `ts-packages/preview-runtime/**`, `ts-packages/preview-renderer/**`, `Cargo.lock`.
- [x] 11.10 Plan end: workspace nextest against 11.1 (not run: no Rust changed, see Handoff log); reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- pdf.js bumps need `patchWorker` updates; it throws loudly when its anchor is missing.
- Several tabs on one path coalesce on the server: one tab's request can supersede another's.

## Handoff log

**Headline.** The viewer SPA shows a live PDF for `format: typst` pages. Branch `q2-preview/p11-spa-pdf-pane` from `d5db9bcd3` (Plan 9's tip, unmoved at landing, so no rebase). TypeScript and CI only; no Rust changed.

**What landed** (`git log d5db9bcd3..q2-preview/p11-spa-pdf-pane`): `onFrameReady` hook and Back button; `PdfPane`, classification, history index and the `PreviewApp` branch; unit and integration tests; the Playwright spec and workflow changes; fixes to the spec; this log.

**Interfaces (as built).**
- `@quarto/pdf-viewer`: `mountPdfViewer(container, base, options?: PdfViewerOptions)`; `PdfViewerOptions.onFrameReady?(win, doc)` runs for each new frame once its first page has drawn, just before it replaces the live one; a throw rejects that `show()`. hub-client's wrapper is unchanged (the new argument is optional).
- `q2-preview-spa/src/pdfBackButton.ts`: `installBackButton(win, doc, onBack | null)` inserts the button first in `#toolbarViewerLeft` (CSSOM only, because of the viewer's CSP), sets `externalLinkTarget` to BLANK, and throws when the anchor is missing.
- `q2-preview-spa/src/pdfPage.ts`: `isTypstPdf(path)` (P11-D1). `src/navIndex.ts`: `readNavIndex`, `seedNavIndex`, `nextNavState` over `history.state.q2NavIndex`.
- `components/PdfPane.tsx`: `PdfPane({ path, contentTick, onBack })`, exports `PDF_ROUTE`, `DEBOUNCE_MS` (500). The first compile starts at once; later ones wait for the quiet period. 409 (superseded or busy) is silent; 422 and other failures show a banner and keep the last PDF.
- `PreviewApp`: `pageKind` is a memo over `[boot, activeFile, contentTick, files]`; the PDF branch returns above the AST gates, and the render effect is skipped for PDF pages. Entering a PDF page clears the AST, theme and render status. Both `@quarto/pdf-viewer` aliases (vitest unit and integration configs) were added; the four integration test files\' runtime mocks gained `resolvePandocFormats`.

**Deviations.**
- 11.1: no ledger line exists for the tip (Plan 9 landed without a workspace run), and the global rule is to recommend rather than run the workspace unprompted. Baseline taken as Plan 9's derived 16102 passed, 202 skipped. This plan changes no Rust, so the Rust counts are unchanged by construction; a measured run is Gordon's call.
- The deps filter only drops unrelated `.qmd`/`.md` edits; images and `_quarto.yml` always pass (so 11.7's "non-active-file edit" test asserts that, not a dep-set lookup).
- The Back button's presence is decided when a frame is built (the pane is keyed by path, so it remounts on navigation).

**Tests.** `q2-preview-spa`: 65 unit (9 `PdfPane`, 4 `pdfBackButton`), 90 integration (8 new in `PreviewApp.integration.test.tsx`); `ts-packages/pdf-viewer` 2; hub-client `src/pandoc` 270; `tsc` clean in all. Mutation-checked: the stale-sequence test fails without the guards. Playwright `typst-pdf-preview.spec.ts` (Chromium): 7 pass against a freshly built binary (wasm, SPA, `q2`); five existing specs (25 tests) still pass. Firefox was not run. The Back button's real-browser coverage is the spike (5 swaps, light/dark, 400 px); there is no e2e for the click.

**For Plan 13 and later.**
- The book-chapter e2e uses the book template, which fetches `@preview/orange-book`: it needs network, and the book template needs `book.author` (without it typst fails with "expected content, found array"). CI has network; offline runs of that one case fail.
- pdf.js builds text layers only for visible pages, so assert on later pages through `PDFViewerApplication.pdfDocument` (`allPdfText` in the spec).
- `startPreviewServer`\'s `url` already carries `?page=`; build page URLs with `URL.searchParams`.
- `q2-preview-spa-e2e.yml`: `paths:` filter, `PANDOC_VERSION`/`TYPST_VERSION` env and the pandoc and typst install steps are mine; Plan 12's task 12.11 edits the same file, so whichever lands second keeps both.
- `docs`: user-facing behaviour for Plan 13.7: typst-pdf pages show a live PDF with a Back button; `output-ext` other than `pdf` keeps the "no live preview" overlay; the preview always uses chapter scope.
