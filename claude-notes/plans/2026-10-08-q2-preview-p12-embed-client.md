# Plan 12: the `q2 preview --ui editor` embed: native downloads, PDF preview and the embed test harness

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E1, E4, E7); the execution model and landing procedure are there.

## Execution

- **Lane:** B (`workspace-2`), binding: one agent per plan, in this order: Plan 2, Plan 6, Plan 9, Plan 12. This plan is number 4 of 4 in the lane.
- **Waits on:** Plans 4 (viewer bundle serves `/pdfjs/`; `ts-packages/pdf-viewer`), 5 (scope types and the explicit-scope menu in hub-client) and 9 (routes and contract) merged into `feature/q2-preview-hub-parity`. Nothing inside this plan waits on another plan. The embed's book menu entries are Plan 13.
- **Start gate:** every plan under "Waits on" is merged. Check from your worktree: `git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p04-pdfjs-packaging' && git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p05-book-scope-clients' && git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p09-preview-routes' && echo ready`. If it does not print `ready`, do not start: report which plans are missing and stop (plan branches are never deleted after landing, so `--merged` is the test). Before coding, read the Handoff log at the end of each plan you wait on.
- **Branch:** `q2-preview/p12-embed-client`, cut from the tip of `feature/q2-preview-hub-parity` when you start.
- **Merges to:** `feature/q2-preview-hub-parity`, by the landing procedure in the epic.
- **Unlocks:** Plan 13.
- **Next in lane:** none; this is the last plan in the lane.

## Goal

In `q2 preview --ui editor` (the hub-client embed, `VITE_PREVIEW_EMBED=1`, no pandoc.wasm), reach the browser path's behaviour using the server's native routes: (1) PDF preview of a `format: typst` document in the editor's preview pane; (2) Download as PDF, and odt; (3) typst source (`.typ`) download; (4) correct docx/pptx/epub/odt downloads: code-cell outputs, Word comment and track-change authors, the unexecuted-cells notice. This plan also builds the embed Playwright harness that Plans 12 and 13 use.

## Background facts

(2026-10-08 survey; re-check line numbers.)

- Embed menu: `NATIVE_FORMATS = docx, pptx, epub` (`hub-client/src/pandoc/downloadService.ts:42`). `odt` is in `PANDOC_FORMATS` but was missing from the route's `FORMATS` (Plan 9 adds it).
- `NativeRenderRequest` is `{path, format, content}` (`nativeRender.ts:6-13`). `runNative` (`downloadController.ts:346-361`) ignores scope, sends `content ?? ''` (the server took `Some("")` as the source, so omitting `content` gave a blank document), sets `notices: []` and `unexecutedCells: 0`, never calls `onPdf`, always calls `save`; overlap mode does not abort older same-path runs (`:301-308`); the overlap watermark is `mayShow` (`:541`). `renderNatively` rethrows when aborted (`nativeRender.ts:57`) and maps every non-ok response to `native-error` (`:63-64`), so every 409 becomes `{kind:'error', message:'{"busy":true}'}`.
- `pdfPreviewAvailable() = pandocWasmEnabled() && !isPreviewEmbed()` (`downloadService.ts:165`). `PdfPreviewPane.tsx:62-70` calls `createPdfPreviewController` unconditionally and calls `controller.start({path, format, projectKey, captureDocId})` with no `content` (`:~88`). `PreviewRouter.tsx:145-147` gates on `pdfPreviewAvailable` and `menuFormats()`. `classifyPreviewMode`'s `pdf` branch needs `canDownload('typst-pdf')` and `canPreviewPdf()`, both false in the embed (`getQ2Format.ts:99-101`).
- `attributionJsonFor` is a plain module import in `downloadService.ts`, so the `native` closure can call it (`request.format` is available); it returns undefined for formats other than docx/pptx. `done.book` already exists in the status type; only `runNative` consumes `NativeRenderOutcome`, so adding variants breaks no other producer or mock.
- pdf.js is pruned from the embed (`scripts/prune-embed-dist.mjs:19`) and stays pruned: `lookup_embedded(Editor)` falls back to the viewer bundle (`lib.rs:804-810`), so `/pdfjs/...` is served from there once Plan 4 lands. `quarto-preview/build.rs:57-60,173` dedupes embed files against the viewer bundle; `build.rs:57` embeds a placeholder when `hub-client/dist-preview-embed/index.html` is missing (`build.rs:318-321`). vite `base: './'` in both apps, so `./pdfjs/web/viewer.html` resolves to `/pdfjs/...` from `/` (read; not built and run).
- No Playwright spec drives a `--ui editor` embed over a native render; `hub-client/e2e/q2-preview-click-to-editor-scroll.spec.ts` targets the `hub` binary, and `hub-client-e2e.yml` installs neither pandoc nor typst. `q2-preview-spa/e2e/helpers/previewServer.ts` spawns `q2 preview` for the viewer (args around `:258-265`: `--no-browser`, `--data-dir`, `--allow-edit`; no `--ui`); the editor boot URL is a hash-route share URL that its banner regex already matches (`preview.rs:312-331`). `playwright.config.ts`: `firefox` already has a `testIgnore` (ws-queue); `cargo xtask verify --e2e` (`verify.rs:749-756`) runs all projects.

## Decisions

- **P12-D1** Download as PDF uses `POST /api/preview/render` with key `typst-pdf` (attachment); the pane uses `POST /api/preview/pdf` (inline, supersede-aware); both always send `content`.
- **P12-D2** `NativeRenderOutcome` gains a `superseded` variant and a distinct busy variant; the preview controller drops `superseded` silently and a `{busy: true}` 409 (Plan 13) is a visible notice. The client discriminates by body field.
- **P12-D3** The menu follows the route's table, defined once here: `NATIVE_FORMATS` becomes `getPandocFormats()` filtered by one `nativeSupported(f)` rule, true for every key in the route's table (docx, odt, pptx, epub, typst, typst-pdf; P9-D8). Until Plan 13 turns book info on in the embed, the plain typst, typst-pdf and epub entries send `scope: 'chapter'` (Plan 5's P5-D1), so a click never downloads a whole book.
- **P12-D4** No Back button in the editor; the pdf-viewer hook is optional and the editor passes none.
- **P12-D5** Output identity (E4): document that hub PDFs and preview PDFs are not byte-identical; do not chase pixel parity.

## Tasks

Per task: `tsc` plus `vitest` in `hub-client` and `ts-packages/pdf-viewer`. Workspace nextest at the end of the plan. Tests that need typst or pandoc, or the real embed dist, skip with a clear reason when absent or when the embed is the placeholder.

### Part A: client download path

- [ ] 12.1 Baseline `cargo nextest run --workspace` on the integration tip. Prove the embed receives the capture sidecar for a code document in a real `q2 preview --ui editor` session (the server resolves captures itself; this checks the sidecar is populated and synced). Record what the embed's full-DOM preview shows for a `format: typst` document today (the "before").
- [ ] 12.2 `nativeRender.ts`: request fields `attributionJson`, `scope`, `content`; `stats` and `superseded`/busy in the outcome; the pdf route client. Tests: header parsing, 409 body discrimination, abort.
- [ ] 12.3 `runNative`: forward `attributionJson` (via `attributionJsonFor`, called directly in the `native` closure), `scope` and `content`; feed `unexecutedCells` from `X-Q2-Stats` and notices from `X-Q2-Diagnostics`. No new `DownloadDeps` member.
- [ ] 12.4 `NATIVE_FORMATS`/`selectMenuFormats` per P12-D3 (odt and typst join); vitest in `downloadService.embed.integration.test.ts`: menu contents, request body fields, the unexecuted-cells status text. Existing tests that change: `nativeRender.test.ts`, `downloadService.test.ts`, `downloadService.embed.integration.test.ts`, `useDownloadAs.integration.test.tsx`.

### Part B: PDF preview in the embed

- [ ] 12.5 Build (gates: also `cargo clippy -p quarto-preview --all-targets -- -D warnings` plus `cargo nextest run -p quarto-preview`, since this task's tests are Rust): `'pdfjs'` stays in `EMBED_EXCLUDED_DIRS`; no fetch and no `pruneEmbedDist.test.ts` change. Test: a built `q2` serving the editor UI answers `GET /pdfjs/web/viewer.html` and `/pdfjs/build/pdf.mjs` from the viewer bundle; check that the embed's `mountPdfViewer` base URL (`import.meta.env.BASE_URL` under `VITE_PREVIEW_EMBED=1`) resolves to `/pdfjs/`; `editor_embed_holds_no_byte_identical_duplicates_of_viewer_files` stays green.
- [ ] 12.6 `DownloadController.runNative`: add the PDF branch (blob to `Uint8Array`, `onPdf(bytes, {path, fileName, seq})`, no `save`), the overlap watermark and client-side ordering; verify an aborted run still rejects through `renderNatively`. Test `native pdf controller hands bytes to onPdf and does not save`.
- [ ] 12.7 `createNativePdfPreviewController(onPdf)`; `PdfPreviewPane.tsx` picks it when `isPreviewEmbed()`, guards the `acquire`/`release`/pool calls, and passes `content` (the editor's current text) in `controller.start(...)` (omitting it produces a blank PDF); superseded responses are ignored; the `PdfPreviewPane.test.tsx` mock and the router tests are updated.
- [ ] 12.8 `pdfPreviewAvailable()` and `NATIVE_FORMATS` change together; `classifyPreviewMode`'s `pdf` branch is reachable in the embed; `PreviewRouter.tsx` gates verified; vitest for the router in embed mode.
- [ ] 12.9 Missing typst or pandoc: the banner shows `Q-21-1`/`Q-21-2` (P9-D6).

### Part C: harness, end to end, docs

- [ ] 12.10 Embed harness: `helpers/previewServer.ts` gains `ui?: 'viewer' | 'editor'` (appends `--ui editor`); a Playwright project `embed` in `q2-preview-spa/playwright.config.ts` (`testMatch: /embed-.*\.spec\.ts/`) and `testIgnore` for `embed-*.spec.ts` on the existing `chromium` and `firefox` projects (`firefox` already has one, so merge into an array); a check that the embed is not the build.rs placeholder gated on the `embed` project (or `test.skip` inside the embed specs), not added to the existing repo-wide `globalSetup` (which does a binary sanity check), because `cargo xtask verify --e2e` runs every project and would fail on a placeholder build.
- [ ] 12.11 CI (Plan 11's task 11.9 edits the same workflow and `paths:` filter without an ordering; whichever lands second keeps both sets of steps and paths): extend `q2-preview-spa-e2e.yml` and its `paths:` filter; its matrix is `[chromium, firefox]` with `--project=${{ matrix.browser }}`, so add the `embed` project as a matrix entry or its own step, otherwise it never runs: the SPA build first, then `cargo xtask build-hub-client-embed` (about 4 GB heap; it re-runs `build:wasm`, and the dedupe needs the real viewer dist), then `cargo build --bin q2` (otherwise `build.rs` embeds a placeholder); install pandoc and typst with the version pins from `test-suite.yml`.
- [ ] 12.12 `embed-typst-pdf.spec.ts`: the PDF pane appears; an edit re-renders; a broken edit keeps the last PDF under a banner; a docx download with a code-cell capture contains the output; odt, PDF and `.typ` downloads succeed.
- [ ] 12.13 User docs: embed capabilities and requirements; the output-identity note (P12-D5). hub-client changelog entry.
- [ ] 12.14 Plan end: workspace nextest against 12.1; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- The embed's browser tier has never been automated; the harness may be real work.
- Authorship payloads on long histories (Plan 9 sets the body limit).
- Native versus wasm output differs (E4); side-by-side comparisons will show it.

## Handoff log

Written by the implementing agent in the plan-end task, committed with the checklist: what landed (commit range), any task that deviated from the plan and why, names and signatures of the interfaces later plans consume exactly as they ended up, the workspace nextest baseline and final counts, and anything the next agent in the lane or a plan that waits on this one must know. Empty until the plan lands.
