# Plan: Viewer, incremental spike and PDF parity (pandoc-host H9)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (PDF section, T2, T6, T7, D10)
**Depends on:** H8; chapter navigation consumes anchors from the later book plan (request R7 stages 2-3). **Unblocks:** nothing (closes the epic).
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H8 is fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** none; lane H is done.

## Overview

Show the PDF in a pdf.js viewer that keeps zoom and scroll across recompiles (and jumps to the active chapter for books), spike incremental recompile, and check typst/PDF parity against native.

## Checklist

### Tasks
- [x] **Viewer** (scroll/zoom retention done; chapter jump is the separate open item below). Start from T7's pattern (the stock pdf.js viewer in an iframe or blob URL with a stable per-file fingerprint, so zoom and scroll survive recompiles); fall back to `pdfjs-dist`'s `PDFViewer` component (scroll restored from saved state) if the stock viewer's footprint is a problem. Viewer assets go under `public/pdfjs/` and are excluded from the workbox precache (`globIgnores: ['pdfjs/**']` beside the existing ones, since `globPatterns` would otherwise precache the viewer's html, css and svg, `vite.config.ts:143-147`; a CI check confirms `dist/sw.js` lists none; the viewer is ~14 MB; the precache has a per-file ceiling and an atomic install, `OFFLINE.md`, GH #447). The stock `viewer.html` is not in the `pdfjs-dist` npm package (6.3.289 ships `web/pdf_viewer.mjs`, css and images only), so the assets come from a checksum-pinned pdf.js release download like pandoc's, or from a `gulp generic` build, and are gitignored. The fingerprint is the trailer `/ID` when valid, else an MD5 of the first 1024 bytes (`src/core/document.js:1616`), and typst writes `/ID` from the content and the creation date, so it changes with every edit (and, unless the date is pinned in R4, every second): patching `pdf.worker.js` for a constant fingerprint is required, not optional. Vendoring and patching `pdf.worker.js` for a stable fingerprint is time-boxed to 2 days. Done when a recompile keeps the scroll position, and, once the book plan's chapter anchors exist, when the viewer jumps to the active chapter's anchor.
- [ ] **Viewer: jump to the active chapter's anchor.** Open: request R7 stages 2-3 (whole-book request, per-chapter anchors) have no plan and are unticked in R7, so no anchors exist. Blocked on that plan, not on H9.
- [ ] **Viewer: user-facing placement.** Open for Gordon: where the viewer appears (a `PreviewRouter` mode or a dialog). Today `pdfViewer.ts` + `onPdfCompiled` exist and only the `__quartoTest.pandoc.mountPdfViewer` hook mounts it, so there is no changelog entry yet.
- [x] **`incr_compile` spike (T6), time-boxed.** `incr_compile` emits a vector-format delta for typst.ts's own renderer, not PDF (`packages/compiler/src/lib.rs`, v0.7.0), so a preview mode on it uses that renderer instead of pdf.js; the spike compares its latency with a warm plain PDF `compile`. A separate PDF-preview mode, off by default, on the `PreviewRouter` mode enum (D8.7). It only speeds the typst leg while each edit still pays the pandoc leg (~630 ms with a fresh instance plus mount in reactor mode, ~0.3-0.5 s in Node command mode; D2a), so it needs D2(b) (the warm-instance spike in H6) or an explicit latency budget. H6 did not run the D2(b) spike and set the budget instead: the pandoc leg is **165 ms in Chromium and 299 ms in WebKit** per render with the module resident (fresh instance each time, callouts.qmd; evidence section 12), the first run after a load ~285 ms. Run D2(b) only if this spike's per-edit number needs the pandoc leg lower. Output: a yes/no and a per-edit latency number, not a feature.
- [x] **Parity.** Compare typst source and PDF text/page count against native on the typst fixtures; document known font/layout differences. CI's native typst is 0.15.1 (`ts-test-suite.yml`) while the browser targets 0.14.2, so the parity job installs a pinned 0.14.2 binary on both OSes (CI's brew install is unpinned), and the criterion is equal page count and text content, not layout. Also check the pandoc 3.11 typst writer's output against typst 0.14.2 (writer-versus-compiler skew; T2).

## Verification

Playwright for the viewer's scroll retention; the parity job in CI.

## Exit

PDF is viewable and checked. Incremental recompile has a documented yes/no.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/h9-viewer-parity` in workspace-7, landed into `feature/pandoc-wasm`, retired to `parking/workspace-7`.
- Last commit; tasks ticked: viewer (retention), `incr_compile` spike (NO; `claude-notes/research/2026-10-02-incr-compile-spike.md`), parity (6/6, CI job with pinned 0.14.2). Open: chapter jump (blocked on R7 stages 2-3), user-facing viewer placement.
- State and gotchas: viewer assets come from `scripts/fetch-pandoc-wasm.mjs` (pinned pdf.js 6.3.289, worker patched); see `dev-docs/pandoc-wasm-host.md` ("PDF viewer", "Browser/native typst parity"). Gates: test:ci unit 1397 / integration 157 / wasm 292; workspace nextest 15626 passed, 202 skipped (H9 touches no Rust; the handoff baseline said 15616, so the +10 is not from H9 and was not chased). Viewer spec passes in Chromium and WebKit. Still open from H5/H7: real-Safari check and the CSP `wasm-unsafe-eval` check (the harness cannot exercise a CSP); the viewer iframe and its blob URL are also untested under a CSP.
- Next step: none in lane H.
