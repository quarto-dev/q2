# Plan: the import service in hub-client (document import P4)

**Date:** 2026-10-03
**Epic:** [`2026-10-03-document-import-epic.md`](2026-10-03-document-import-epic.md) (I8, I9, I10, I15, I16, I19; uses interfaces 1, 2 and 3)
**Depends on:** P1 (host inputs and collection) and P3 (the wasm exports), both landed. T0 can start earlier. **Unblocks:** P5.
**Branch:** `import/p4-import-service` from `feature/hub-import`.

## Why

P5's UI needs one call: give it a `File` and a target path, and get back the qmd, the image files to store and the report. This plan builds that call with no UI:
- runs pandoc on its own runner (I10);
- keeps image bytes in TS (I9);
- converts EMF/WMF with rtf.js (I8, I15);
- applies the 10 MB image rule (I16);
- has Rust plan and report everything else (interface 2).

## Key files

- `hub-client/src/pandoc/`:
  - `pandocService.ts` (`getPandoc()`: the app-wide loader and runner);
  - `pandocRunner.ts` (single `current` slot at :172/:199; `uiStateFor(o: RunOutcome): UiState` at :93, `UiState` at :63, `RunFailureKind` at :30; 120 s timeout at :17);
  - `pandocLoader.ts`, `pandoc.worker.ts`;
  - `downloadService.ts` and `downloadController.ts`, the closest pattern for an orchestrating service;
  - `buildFlag.ts` / `featureFlag.ts`: `pandocWasmEnabled()` and `isPreviewEmbed()`. Note `downloadAvailable()` is `pandocWasmEnabled() || isPreviewEmbed()` (`downloadService.ts:38-39`, true in the embed, which renders natively); the gate import needs is `pdfPreviewAvailable()`'s, `pandocWasmEnabled() && !isPreviewEmbed()` (`:102-103`).
- Size limit: `FILE_SIZE_LIMITS.MAX_FILE_SIZE` in `hub-client/src/services/resourceService.ts:167`. MIME inference: `inferMimeType` in `ts-packages/quarto-automerge-schema/src/index.ts`.
- The wasm wrapper layer: `downloadService.ts` imports `renderPandocRequest` / `classifyPandocCompletion` / `getPandocFormats` from `@quarto/preview-runtime` (`wasmRenderer.ts:653,711,724`). P3 T8 adds the four import wrappers there: `getImportFormatTable`, `prepareImport`, `finishImport`, `classifyImportFailure`. They are synchronous and need `initWasm()` awaited first.
- Reuse rather than rebuild:
  - `computeSHA256` from `@quarto/quarto-sync-client` (`ts-packages/quarto-sync-client/src/hash.ts:9`, lowercase hex);
  - the deps pattern of `DownloadController` / `DownloadDeps` (`downloadController.ts:99,141`, constructed in `downloadService.ts`);
  - `RunOptions.onLoadProgress` and `LoadProgress` (`pandocLoader.ts`) for the first-use download;
  - the test utilities `nodePandocWorker` (`test-utils/nodeWorker.ts`), `fakeCache.ts` and `pandocRecordings.ts` (P1 T7 adds `loadImportRecording`);
  - `pdfChain.wasm.test.ts` as the real-wasm skeleton, and `downloadService.embed.integration.test.ts` for stubbing the feature flags.

## The service contract (P5 builds against this)

```ts
// hub-client/src/pandoc/importService.ts
export interface ImportFormat { id: string; label: string; extensions: string[]; mimeTypes: string[] }
export interface ImportFormats { formats: ImportFormat[]; maxSourceBytes: number }   // mapped from get_import_formats' snake_case
export interface ImportedMedia { projectPath: string; bytes: Uint8Array; mimeType: string }
export type ImportProgress = 'reading' | 'loading-pandoc' | 'converting' | 'images' | 'finishing';
export type ImportHostCode = 'import-read-failed' | 'import-write-failed' | 'import-cleanup-failed';   // TS-side import diagnostics (epic interface 3)
export type ImportHostDiagnostic = Omit<HostDiagnostic, 'code'> & { code: ImportHostCode };
export type ImportDiagnostic = Diagnostic | ImportHostDiagnostic;
export type ImportOutcome =
  | { ok: true; qmd: string; media: ImportedMedia[]; diagnostics: ImportDiagnostic[] }
  | { ok: false; cancelled?: true; diagnostics: ImportDiagnostic[]; uiState?: UiState };   // uiState from uiStateFor for load-failed / worker-blocked
export interface ImportOptions {
  signal?: AbortSignal;
  onProgress?: (p: ImportProgress) => void;
  /** First-use pandoc download/verify progress, passed through from RunOptions.onLoadProgress. */
  onLoadProgress?: (p: LoadProgress) => void;
}
export interface ImportService {
  getImportFormats(): Promise<ImportFormats>;
  /** Validation-only prepare_import on name and size: [] when importable, else Rust's Q-24-1 / Q-24-2. Reads no bytes, loads no pandoc. */
  validateImportSource(file: File): Promise<ImportDiagnostic[]>;
  importDocument(file: File, targetQmdPath: string, opts?: ImportOptions): Promise<ImportOutcome>;
}
/** Runner, wasm wrappers, image converter and clock, injected like DownloadDeps. */
export interface ImportDeps { /* fields fixed in T0 */ }
export function createImportService(deps: ImportDeps): ImportService;
export function getImportService(): ImportService;          // the default instance
export function setImportServiceForTests(s: ImportService | undefined): void;
export function importAvailable(): boolean;                 // pandocWasmEnabled() && !isPreviewEmbed()
```

P3's wrappers return interface 2's raw snake_case JSON, typed by P3 T8's `.d.ts` (`ImportFormatTable`, `PrepareImportResponse`, `FinishImportResponse`, `ClassifyImportFailureResponse`). This service is the only place that maps them: `getImportFormats()` maps `mime_types` → `mimeTypes` and `max_source_bytes` → `maxSourceBytes`, and `importDocument` reads `request`, `share_tree` and `source_path` from `PrepareImportResponse`. P5 sees only the camelCase types above.

`Diagnostic` is the pandoc-host union (`ts-packages/pandoc-host/src/types.ts`): a `RustDiagnostic` has a title and problem text, a `HostDiagnostic` only a `message`. `HostDiagnostic.code` is the closed `HostDiagnosticCode` union, hence `ImportHostDiagnostic` (epic interface 3). P5 calls only `getImportService()` and `importAvailable()`; its tests install the stub with `setImportServiceForTests`.

`ImportProgress` mapping: `reading` while the file is read and hashed; `loading-pandoc` for the runner's `loading` and `starting` stages (with `onLoadProgress` carrying the download/verify detail); `converting` for `mounting` and `running`; `images` during T3; `finishing` for `finish_import`. `media` is in `media_plan` order, one entry per distinct project path. Imports are **serialized**: a second `importDocument` call waits for the first, because the import runner, like every `PandocRunner`, supersedes its current run.

## Checklist

### Tasks

- [ ] **T0 Contract and stub.** Commit `importService.ts` with the types above (fixing `ImportDeps`'s fields), `createImportService`, and a stub `ImportService` that `setImportServiceForTests` installs. Until T7, `getImportService()` returns the stub, so P5's UI also runs against it in the dev server. The stub's `importDocument` returns a hand-written canned outcome (a short qmd, one media entry, one diagnostic of each kind) or a canned failure, and `validateImportSource` returns `[]` or a canned Q-24-1 / Q-24-2 by extension and size. Hand-written because T0 may start before P3 lands. P5 can develop against it. Commit this first and note the SHA in the Handoff log.
- [ ] **T1 The import runner.** `getImportRunner()` in `pandocService.ts`: a second runner, `new PandocRunner({ loader: getPandoc().loader, createWorker: createBrowserWorker })`, sharing the app-wide `PandocLoader` (I10). H10b plans a separate warm, preview-only runner beside it (`getPreviewPandocRunner()`, H10b Task 1b) but has not landed; don't wait for it, and don't route imports through it (I10; epic: H10a/H10b overlap). `RunOptions.inputs` and `collected` already exist (P1 T6). The service takes the runner through `ImportDeps`, so tests pass a runner built on `nodePandocWorker`. It keeps the runner's default 120 s wall timeout (`pandocRunner.ts:17`) unless P1 T8's wall-time STOP led Gordon to raise it; check P1's Handoff log. Test in `pandocRunner.test.ts`: two runners on one fake loader, an import and a download started together, both complete, neither is superseded.
- [ ] **T2 rtf.js spike (I15).** One day; stop when P1's `emf-docx` fixture converts to a PNG that looks right, or at the end of the day with the findings recorded. Known from the implementability review (npm tarball): `rtf.js` 3.0.9, MIT, last published 2022-07-16.
  - Add `rtf.js` at `3.0.9` to `hub-client/package.json` and the root lockfile (`npm install rtf.js@3.0.9 -w hub-client`); no package depends on it today.
  - Import `rtf.js/dist/EMFJS.bundle.min.js` and `WMFJS.bundle.min.js` directly (about 56 KB each), never the package root, which pulls the 2.1 MB RTF bundle. The package has no `exports` map; add ambient `declare module` types. Both bundles are UMD wrappers (`exports.EMFJS = …` / `this.EMFJS`), so check first that a dynamic `import()` of them works under Vite's CJS interop, in the dev server and the production build. Load them as a lazy chunk (dynamic `import()`), so they stay out of the main bundle. The PWA precache's `**/*.js` glob (`vite.config.ts:155`) will still precache the chunk; at about 112 KB that is acceptable, so don't add it to `globIgnores` (which holds only the multi-MB sass, Monaco-worker and pdf.js assets). Record the chunk's size.
  - `Renderer.render()` builds an `SVGElement` with `document.createElementNS`, so conversion runs **on the main thread**: not in a worker, and not in the node wasm suite. Rasterize with `<img>` and `<canvas>` (`OffscreenCanvas` can't decode SVG in a worker).
  - `Renderer` takes `{ width, height, wExt, hExt, xExt, yExt, mapMode }` and exposes no frame size (`src/emfjs/Renderer.ts:34-42`), so parse the size yourself: the EMF header's `rclFrame`, or the WMF placeable header.
  - `render()` is synchronous. The 10 s / 60 s timeouts (T3) bound rasterization only; a render that hangs on a malformed file blocks the tab. Record that in the code comment.

  **The output must be PNG** (I8): pandoc.wasm has no `rsvg-convert`, so an SVG image in a docx download becomes alt text with no picture (`claude-notes/designs/pandoc-wasm-architecture.md:122`, `claude-notes/research/2026-10-01-pandoc-wasm-evidence.md:174`). Size: the frame at 2x, longest side capped at 4096 px. Convert P1's `emf-docx` fixture's EMF and look at the result. Record everything in the Handoff log, including the PNG's dimensions and where you saved it (scratchpad path), and whether the `EMR_RECTANGLE` is visible. **STOP** if rtf.js can't render the fixture. If P1 has no WMF (it asks Gordon for one), the WMF path is untested: record that as blocked, not failed.
- [ ] **T3 Media pipeline.** For each `collected` file, in order:
  1. If the extension is emf or wmf, convert it to PNG, with a 10 s timeout per image and 60 s in total (images after the total budget aren't converted). On error or timeout, keep the original and set `conversion_failed`. If the PNG exceeds `MAX_FILE_SIZE` but the original doesn't, keep the original and set `conversion_failed`.
  2. If the final bytes exceed `MAX_FILE_SIZE`, the entry is `skipped` with reason `too-large`, and its bytes are dropped.
  3. Otherwise compute sha256 (`computeSHA256`) and record `stored`, with `ext`, and `converted_from` if converted.

  Check `signal.aborted` before each image; if set, stop and return `{ ok: false, cancelled: true }`. The converter is injected (`ImportDeps`), so these tests run without a DOM. `inferMimeType` returns `application/octet-stream` for `emf`/`wmf` until P5 T1 adds them; tests go through `inferMimeType` rather than asserting the literal.

  Then, for each host `collect-limit` warning, append a `skipped` / `too-large` entry with the warning's `path` and `size` (I16; the host dropped the file, so there are no bytes). Build `media_manifest_json` exactly as interface 2 pins it. Unit tests with fake bytes, including a converter that throws, a converter that hangs (timeout), a PNG larger than its EMF, a `collect-limit` warning, and a file of exactly 10 MB versus 10 MB + 1 byte.
- [ ] **T4 Orchestration.** `importDocument`:
  1. Validate first, without reading the file: `validateImportSource(file)` (validation-only `prepare_import`, interface 2). If it returns diagnostics (Q-24-1 unknown type, Q-24-2 over the cap), return them. Pandoc is not loaded.
  2. Read the bytes; sha256 them. If `file.arrayBuffer()` throws (file removed, permission revoked), return `{ ok: false }` with a host-style `import-read-failed` diagnostic.
  3. Call `prepare_import(name, bytes.length, sha256)` for the request (the real length, since the host checks it).
  4. Run on the import runner with `inputs = { [source_path]: bytes }`.
  5. On failure, by `RunFailureKind` (the epic's interface-2 mapping):

     | kind | outcome |
     |---|---|
     | `pandoc-exit`, `no-output` | `classify_import_failure` → Q-24-3 |
     | `oom`, `crash`, `timeout` | `classify_import_failure` → Q-24-13 (not the host's "a filter may be stuck" timeout text; no filters run) |
     | `invalid-request` (any host code, e.g. `input-mismatch`, `limit-exceeded`), `superseded` | `classify_import_failure` → Q-24-12, host diagnostics appended |
     | `aborted` | `{ ok: false, cancelled: true }`, no diagnostics |
     | `load-failed`, `worker-blocked` | `uiState` from `uiStateFor`, plus its diagnostics |
  6. On success: decode `out.json` as UTF-8, run T3 on `collected` and the host warnings, then call `finish_import(json, stderr, targetQmdPath, manifest)`. If it returns `success: false` (Q-24-12), return `{ ok: false }` with its diagnostics.
  7. Join `media_plan` with the T3 bytes and MIME types (`inferMimeType` by the final extension).
  8. Return.

  Emit `onProgress` at each stage (mapping in the contract above) and pass `onLoadProgress` and `signal` to the runner. Host warnings (`collect-limit`) are appended to the outcome's diagnostics. `await initWasm()` before the first Rust call. Unit tests for each row of the table with a fake runner, plus an abort during image conversion.
- [ ] **T5 Availability.** `importAvailable()` is `pandocWasmEnabled() && !isPreviewEmbed()`, like `pdfPreviewAvailable()` (not `downloadAvailable()`, which is true in the embed). False in the `q2 preview` embed (out of scope for v1). Unit test both.
- [ ] **T6 Real-wasm tests.** `hub-client/src/pandoc/importDocument.wasm.test.ts`, on the `pdfChain.wasm.test.ts` skeleton, in the `vitest.wasm.config.ts` suite: real pandoc.wasm plus the real Rust wasm (rebuilt with `npm run build:wasm` after P3), for each P1 fixture, with tests reported as run, not skipped. `importDocument` with a `File` built from `source.<ext>` and `targetQmdPath = "<directory name>.qmd"` (P3's convention) gives the fixture's `expected.qmd` (`import-recordings/<directory name>/expected.qmd`, P3), and media whose bytes match the fixture's `media/`. The `emf-docx` fixture runs here with the converter stubbed to fail (the original is stored, `conversion_failed`), matching the manifest P3's expected output was written from. Real conversion needs the DOM (T2), so it gets a Playwright test: add an `importDocument` hook to `hub-client/src/test-hooks.ts` and `e2e/pandoc-import-emf.harness.spec.ts` (the `pandoc-*` prefix puts it in the WebKit project too, `playwright.harness.config.ts:66`, where SVG rasterization differs most; the config's `firefox` project, `:56`, runs every harness spec, so it covers all three engines). It imports the `emf-docx` fixture and checks the PNG's signature and dimensions, then decodes it in the page (`<canvas>` `getImageData`) and asserts at least one pixel differs from the background, so a blank render fails. Also: the corrupt fixture gives Q-24-3; an over-cap file gives Q-24-2 without loading pandoc (assert the loader was not called), both through `importDocument` and through `validateImportSource`.
- [ ] **T7 Replace the stub.** `getImportService()` returns `createImportService` with the real deps; the stub stays only behind `setImportServiceForTests`. P5's tests that use it keep working.

### Verification

- [ ] hub-client typecheck, unit, integration and wasm suites green; `ts-packages/pandoc-host` suites green.
- [ ] A manual run in the dev server: import P1's `basic-docx` through a temporary call in the dev console or the dev harness, and inspect the outcome. Record the result.
- [ ] Workspace nextest at the plan boundary, if any Rust changed; report "no Rust changes" otherwise.

### Close-out

- [ ] Checklist reconciled, committed; rebased and fast-forwarded into `feature/hub-import`; epic Progress ticked.

## Handoff log

Append-only.

- 2026-10-03: plan written. Not started.
- 2026-10-03: revised after the implementability review (epic status line): createImportService/ImportDeps seam; ImportDiagnostic types; progress mapping and onLoadProgress; rtf.js facts (main thread, deep imports); emf conversion moves to Playwright.
- 2026-10-03: revised after the angle review: `ImportFormat` defined and the snake→camel mapping owned here; timeout follows P1 T8; rtf.js dependency added and UMD interop checked first; `invalid-request` wording; T6 reads `expected.qmd` by directory name; EMF spec renamed `pandoc-import-emf` (WebKit) and checks for a non-blank image.
- 2026-10-03: rebased onto `feature/pandoc-wasm` `3dfa5b296`; citations re-checked (the pandoc-host and hub-client code P4 cites is unchanged). T1 points at H10b's warm preview runner instead of the removed H10 plan; the import keeps its own fresh runner. T6: WebKit project now `playwright.harness.config.ts:66`, and the new `firefox` project also runs the EMF spec.
