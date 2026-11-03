---
title: 'WebKit pdf-viewer harness flake: the test proceeds before the first PDF has painted (bd-pdf-pane-flash-flake-c76rrou5, bd-pdf-preview-status-transient-4cu7tgg2, bd-e09w5yhd)'
date: 2026-11-02
description: 'Root-causes the recurring "the preview pane keeps the reader" failure in the WebKit pandoc harness: its readiness proxy fires before the first page is drawn, so the flash guard counts pre-paint frames and the edit can start before the pane has a PDF. Deterministic local repro, a plan to fix the test, and one product bug it exposed (a superseded viewer load surfaces its watchdog as an error).'
status: in-progress  # diagnosis 2026-11-02; fixes on branch braid/bd-c76rrou5-pdf-viewer-harness-flake 2026-11-03, PR pending CI
braid:
  strand: bd-pdf-pane-flash-flake-c76rrou5
  related: [bd-pdf-preview-status-transient-4cu7tgg2, bd-e09w5yhd, bd-xmb7rdc7, bd-gxntobaj]
---

**Strands:** bd-pdf-pane-flash-flake-c76rrou5 (the `__empty` guard),
bd-pdf-preview-status-transient-4cu7tgg2 (the status never seen),
bd-e09w5yhd (in progress, same `__empty` symptom); all under the flaky-test
epic bd-gxntobaj. bd-ynqwyyi7 (closed) widened a timeout for the same test.
**Latest sighting:** PR #1006, run 41437007253, job 125249201716 (2026-11-02).
**Test:** `hub-client/e2e/pandoc-pdf-viewer.harness.spec.ts:77`, run by the
"Run pandoc harness tests in WebKit" step of the chromium E2E job.
**Branch:** `braid/bd-c76rrou5-pdf-viewer-harness-flake` (topic branch in the
main checkout, off `main` @ `4170dd39f`). Commits: `9e9f81418` (viewer fix,
item C, strand bd-xmb7rdc7), `200c6d148` (test + harness config, items A, B, D).
**Probe used for the repro:** kept outside the tree (it must not run in CI);
its contents are summarised in § 4; its per-sample snapshot now lives in the
real test's guard (§ 5, item D).

## 1. TL;DR

- The test's "the viewer is ready" check (`viewState`, which returns as soon as
  pdf.js has `pagesCount` and `pdfDocument`) is true **before any page has
  been drawn and before the frame is promoted**. pdf.js 6.4 attaches a page's
  canvas only when that page's render finishes (or after 500 ms of rendering),
  so there is a window, 15–36 ms on an M-series Mac and much longer on a
  loaded 2-core Linux WebKit runner, in which the proxy holds and the frame
  is empty.
- Everything the test does next assumes the PDF is on screen. When the poll
  lands inside that window:
  - the rAF flash guard is armed against an unpainted frame and counts frames
    until page 7 at 150 % finishes drawing: `__empty` = 2–6 (symptom A, every
    sighting since October);
  - the edit's recompile can start while the pane still has `hasPdf === false`,
    so it renders the loading spinner instead of the "Updating the PDF…"
    status, and `toBeVisible('pdf-preview-status')` waits 60 s for an element
    that never existed (symptom B);
  - if the recompile's PDF arrives before the first frame has painted, the
    viewer discards the still-pending first frame but its 30 s watchdog keeps
    running and later rejects the first `show()`, which the pane reports as
    "The PDF viewer drew nothing within 30 s" over a correctly displayed PDF
    (symptom B′, the retry in today's run and in bd-4cu7tgg2). **That one is a
    product bug** (§ 5, item C), reachable by typing during a slow first
    compile or by two warm-path results landing within one paint interval.
- Symptom A is reproduced deterministically on macOS, in both WebKit and
  Chromium, by arming the guard at the proxy instant: 2–3 empty samples, all
  within the first 31 ms, all before the recompile's second iframe exists.
  It is a test-sequencing bug, not a viewer flash: the frame it measures is
  the unpromoted first frame, which the product keeps under the spinner.
- Fix: wait on the state the test asserts (first PDF shown and promoted, page
  7 painted at 150 %), latch the status transient instead of polling for it,
  cancel a superseded load's watchdog in the viewer, and make the guard
  self-describing on failure. Details in § 5.

## 2. Evidence from CI

Hub-Client E2E Tests runs since 2026-10-29, as of 2026-11-02 (`gh run list`):
63 runs, 49 green, 8 failed (6 cancelled or skipped); this test accounts for
5 of the 8 failures, on `main` and on PRs that do not touch hub-client. Every sighting is the WebKit project of the chromium job.

| run | branch | attempt 1 | retry |
|---|---|---|---|
| 40992279658 (10-30) | main | `__empty` 2 | `__empty` 2 |
| 41113432791 (10-30) | main | `__empty` 3 | `__empty` 2 |
| 41127766899 (10-30) | PR #973 | status not visible (60 s) | `__empty` 2 |
| 41241020004 (10-31) | main | `__empty` 3 | `__empty` 4 |
| 41437007253 (11-02) | PR #1006 | `__empty` 6 | status not visible; page shows "drew nothing within 30 s" banner |

Earlier: bd-e09w5yhd lists `__empty` 2 on 39588600246 (10-21, also
`__empty` 4 on its firefox shard), 40520119937, 40648674125, 40974857374,
41098932659; bd-4cu7tgg2 lists 39824362802 (status not visible on attempt 1,
the watchdog banner on the retry). The harness Playwright config records no
trace, screenshot or video, so each sighting leaves only the error-context
snapshot. Today's retry snapshot shows the viewer on page 7 of 12 under the
banner "The PDF could not be updated. Showing the last PDF that compiled. The
PDF viewer drew nothing within 30 s".

## 3. The code path

- `PdfPreviewPane.tsx`: the pane mounts `mountPdfViewer`; `onPdf` calls
  `viewer.show()` and sets `hasPdf` only when that promise resolves. The
  "Updating the PDF…" status renders when `phase === 'working' && hasPdf`; with
  `hasPdf` false the same phase renders the `pdf-preview-loading` spinner.
- `ts-packages/pdf-viewer/src/pdfViewer.ts` `load()`: a new iframe at z-index
  0 (1 for the very first frame), promoted to z-index 2 and titled only once
  `pagesloaded`, `isInitialViewSet` and the current page's
  `renderingState === FINISHED` all hold, with a 30 s watchdog. `open()`
  discards a still-pending frame when a newer PDF arrives (`if (pending)
  discard(pending)`), but `discard` only removes the iframe: the old load's
  timer and listeners stay armed, so 30 s later it rejects the old `show()`.
- pdf.js 6.4.299 (`hub-client/public/pdfjs`): `PDFPageView.update()` keeps the
  existing canvas across a zoom change (`keepCanvasWrapper: true`), so once a
  page has drawn, `.page canvas` stays present. A page that has never drawn
  gets its canvas on the first render-continue callback only after
  `minDurationToUpdateCanvas` (500 ms), otherwise when the render completes.
  The inputs of `viewState` (`pagesCount`, `pdfDocument`) are set in
  `setDocument`, before any render.
- The test (lines 86–119): `viewState` poll → set zoom 1.5 and page 7 → poll
  `viewState` again (synchronous properties, passes at once) → arm the
  sampler → edit → expect status visible → hidden → swap → `__empty === 0`.

## 4. Local reproduction (2026-11-02, macOS, Playwright 1.60, WebKit 2311)

A copy of the test with the sampler instrumented to record a snapshot of every
host iframe (z-index, title, canvas count, pdf.js `renderingState`,
`isInitialViewSet`) at arm time and at each empty sample, plus a
MutationObserver timeline of the host (iframes added/removed, promotion
attributes, pane overlays). Built with `VITE_E2E=1 npm run build`, run with
`--project=webkit --workers=1 --retries=0`.

1. **As written** (default poll intervals), 3 runs idle and 6 runs with all 18
   cores saturated by `yes`: `__empty` 0 every time. Timeline of a recompile
   (ms after `pane('two')`): status added ≈115, second iframe added ≈250,
   status removed ≈251, promotion + old frame removed ≈360–418. The status
   lives ≈135 ms. At arm time the live frame had 2 canvases (pages 1–2) and
   page 7 was `RUNNING`; canvases survive the zoom and page change, so the
   guard cannot be tripped by them.
2. **Polling `viewState` every 5 ms** to catch the proxy instant: at that
   instant the only frame has `title ''` (unpromoted), z-index 1, **0
   canvases**, page 1 `RUNNING`. First canvas 31–36 ms later in WebKit, 14–22
   ms in Chromium. Five of five runs in each browser.
3. **Arming the sampler at that instant** (what a slow runner's 100–1000 ms
   poll does by accident): `__empty` = 3, 3, 3, 3, 3 in WebKit and 3, 2, 3, 2, 3
   in Chromium. Every empty sample is at t ≤ 31 ms, with frames
   `[(z 1, title '', 0 canvases, page 7, state INITIAL→RUNNING)]`; the
   recompile's second iframe is added at t ≈ 210–246 ms. No sample is ever
   empty at or after the swap.
4. **Edit at the proxy instant with `debounceMs 0`** and render-heavy pages:
   the pane showed `pdf-preview-loading` ("Starting the converter…") from
   t = 19 ms and replaced it with `pdf-preview-status` only at t ≈ 90 ms, when
   the first `show()` resolved. That is symptom B's mechanism; on this machine
   paint (≈90 ms) still beats the compile (≈630 ms), so the status did appear
   and no watchdog banner followed. Symptom B′ rests on code reading plus the
   retry snapshot's exact banner text, not on a local repro.

## 5. Fix plan

**A. Test: wait on the shown state, not on `pagesCount`** (fixes A and B).

- [x] After mounting the pane, wait for the first PDF to be *shown*: exactly
      one `#pdf-viewer-host iframe[title="PDF preview"]`. The title is set in
      the same synchronous block that promotes the frame, just before
      `show()` resolves and `hasPdf` flips, so it is the painted state. Do
      not use "`pdf-preview-loading` hidden" as the signal: the spinner is
      `working && !hasPdf`, so it also disappears when the compile ends,
      which is before the paint. Only then read `viewState`.
- [x] After setting zoom 1.5 and page 7, wait until page 7 is drawn at that
      zoom: poll `getPageView(6).renderingState === 3` (and a `.page canvas`
      in the live frame) before arming the sampler. Give `viewState` a
      `painted` field for this so both tests share it.
- [x] Make the test's readiness polls *fast* (`intervals: [5]`) rather than
      slow: with the right predicate an early poll is harmless, and it keeps
      the test from passing only because the default intervals happened to
      land late. Done as: the long waits are locator assertions (Playwright
      re-checks those on DOM changes, without a round trip per poll), the
      short `viewState` polls use 25 ms intervals. 2026-11-03: spec 5× in
      WebKit (10/10 tests) and 5× in Chromium, `__empty` 0 and the status
      latched every time.
- [x] Apply the same `shown` wait to the sibling test at line 52 (it has no
      sampler, but it edits on the same proxy).

**B. Test: latch the status transient instead of polling for it.**

- [x] Before `pane('two')`, install a MutationObserver on the pane that sets
      `window.__sawStatus` when `pdf-preview-status` is added, then assert
      the latch after the recompile (status hidden, viewer updated, one
      iframe). The window is ≈135 ms on a fast machine and the controller's
      `working` phase is the only thing that creates it; with A in place
      `hasPdf` is true before the edit, so it always exists, and the latch
      makes a brief one impossible to miss.

**C. Product: a superseded pending load must not report its watchdog.**

- [x] `pdfViewer.ts` `open()`: when `pending` is replaced by a newer load,
      cancel the old load (clear its watchdog, unsubscribe its event-bus
      handlers) and settle its promise as superseded, and make `show()`
      resolve (or reject with a typed `superseded`) in that case rather than
      with the watchdog error. The `onPdf` rejection handler in `PdfPreviewPane`
      must not turn a superseded load into `viewerError`.
- [x] Unit test in `ts-packages/pdf-viewer/src/pdfViewer.test.ts` (today 13
      lines): two `show()` calls before the first frame is ready, fake timers
      advanced past `VIEWER_WATCHDOG_MS`, expect no watchdog rejection and the
      second frame promoted. Filed as bd-xmb7rdc7 (discovered-from
      bd-4cu7tgg2) since it is user-reachable: typing during the first compile
      on a slow machine, or two warm-path results within one paint interval,
      yields a bogus error banner 30 s later, cleared only by the next
      compile.

**D. Diagnostics, so the next unexplained hit is self-describing.**

- [x] `playwright.harness.config.ts`: `trace: 'on-first-retry'` and
      `screenshot: 'only-on-failure'`; the job already uploads
      `playwright-report/` on failure.
- [x] Keep the probe's per-sample snapshot in the real guard: on
      `__empty > 0`, attach the empty samples (t, frames with z/title/canvas
      count/state) and the host timeline via `testInfo.attach` before
      asserting. Cost is one object per empty sample; nothing when it passes.

**E. Bookkeeping.**

- [x] Comments on bd-c76rrou5, bd-4cu7tgg2, bd-e09w5yhd pointing here (2026-11-02).
- [ ] Close bd-e09w5yhd and bd-c76rrou5 as the same root cause once A lands;
      close bd-4cu7tgg2 and bd-xmb7rdc7 once B and C land.
- [ ] Verification bar: 20 consecutive green runs of the WebKit step on
      `main` and PRs (baseline: this test failed 5 of the last 8 failed runs
      in four days).

## 6. Open observations (not verified, not blocking)

- Stacking: the first frame is appended at z-index 1 inside a host that is
  `position: relative` without a z-index, and `.pdf-preview-loading` is
  absolute with z-index auto. Within the nearest stacking context that puts
  the empty viewer chrome *above* the spinner during the first load's
  pre-paint window. If a quick visual check on a slow machine confirms the
  spinner disappears behind the empty viewer, give the overlay a z-index above
  the frames (or make `.pdf-preview-host` a stacking context). Not a flash
  (nothing was shown before) and not what the guard is for, so separate.
- Why the Linux WebKit runner is so much slower to first paint than macOS
  (software raster, 2 cores, pdf.js paced by rAF in an iframe) was not
  measured; A removes the dependence on it rather than explaining it.
